//! Switch half: version check, one guarded inline hook, per-fighter snapshots,
//! a Judge ring and one writer thread. The hook runs at the start of every
//! fighter's frame and only calls the game's own getters (status, motion,
//! situation, ground, damage, position, work int) on that fighter; it writes
//! nothing the game reads. The writer thread runs on a static stack and owns
//! every file, opened through `nn::fs` with paths built on the stack. Nothing
//! allocates: the counting allocator below reports the plugin's heap
//! allocations (expected 0) in `plugin.log`.
use crate::model::{
    self, outcome, write_row, Contact, History, Judge, Line, Outcome, Place, Site, Snapshot,
    COLLISION_KIND_MASK_ATTACK, COLLISION_KIND_MASK_HIT, COLLISION_KIND_MASK_SHIELD, ENTRIES,
    FIGHTER_FRAME, FIGHTER_KIND_GAMEWATCH, JUDGE_WORDS, OBJECT_ENTRY, OBJECT_ID, OBJECT_KIND,
    OBJECT_MODULE_ACCESSOR, SNAPSHOT_WORDS, WORK_SPECIAL_S_KIND, WORK_SPECIAL_S_PREV_KIND,
};
use core::cell::UnsafeCell;
use core::fmt::Write as _;
use skyline::hooks::{getRegionAddress, InlineCtx, Region};
use skyline::nn;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{fence, AtomicU32, AtomicU64, Ordering};

const DATA_DIR: &str = "sd:/ultimate/ssbu-gnw-judge-log";
const JUDGE_RING: usize = 64;
const NOTE_RING: usize = 32;
const TICKS_PER_SECOND: u64 = 19_200_000;
/// A new session starts after this long without fighter frames (a new match,
/// or a pause longer than this).
const SESSION_GAP_TICKS: u64 = 5 * TICKS_PER_SECOND;
const WRITER_STACK: usize = 32 * 1024;
const OPEN_WRITE_APPEND: i32 = 2 | 4;

/// Counts calls into the heap. Nothing in the plugin is meant to make one.
struct CountingAllocator;
static HEAP_ALLOCATIONS: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        HEAP_ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        System.alloc(layout)
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        HEAP_ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        System.alloc_zeroed(layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        HEAP_ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        System.realloc(ptr, layout, size)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

extern "C" {
    #[link_name = "\u{1}_ZN3app8lua_bind24WorkModule__get_int_implEPNS_26BattleObjectModuleAccessorEi"]
    fn work_int(accessor: u64, id: i32) -> i32;
    #[link_name = "\u{1}_ZN3app8lua_bind30StatusModule__status_kind_implEPNS_26BattleObjectModuleAccessorE"]
    fn status_kind(accessor: u64) -> i32;
    #[link_name = "\u{1}_ZN3app8lua_bind33StatusModule__situation_kind_implEPNS_26BattleObjectModuleAccessorE"]
    fn situation_kind(accessor: u64) -> i32;
    #[link_name = "\u{1}_ZN3app8lua_bind37GroundModule__is_passable_ground_implEPNS_26BattleObjectModuleAccessorE"]
    fn is_passable_ground(accessor: u64) -> bool;
    #[link_name = "\u{1}_ZN3app8lua_bind30MotionModule__motion_kind_implEPNS_26BattleObjectModuleAccessorE"]
    fn motion_kind(accessor: u64) -> u64;
    #[link_name = "\u{1}_ZN3app8lua_bind25DamageModule__damage_implEPNS_26BattleObjectModuleAccessorEi"]
    fn damage(accessor: u64, kind: i32) -> f32;
    #[link_name = "\u{1}_ZN3app8lua_bind25PostureModule__pos_x_implEPNS_26BattleObjectModuleAccessorE"]
    fn pos_x(accessor: u64) -> f32;
    #[link_name = "\u{1}_ZN3app8lua_bind25PostureModule__pos_y_implEPNS_26BattleObjectModuleAccessorE"]
    fn pos_y(accessor: u64) -> f32;
    #[link_name = "\u{1}_ZN3app8lua_bind39AttackModule__is_infliction_status_implEPNS_26BattleObjectModuleAccessorEj"]
    fn is_infliction_status(accessor: u64, mask: u32) -> bool;
    #[link_name = "\u{1}_ZN3app7fighter20get_id_from_entry_idEi"]
    fn id_from_entry(entry: i32) -> u32;
}

/// State touched by one thread only (each entry's history by that fighter's
/// own frame; the writer's stack and thread object once at startup).
struct Owned<T>(UnsafeCell<T>);
// SAFETY: see the type's comment; no two threads use the same cell.
unsafe impl<T> Sync for Owned<T> {}

static HISTORY: [Owned<History>; ENTRIES] =
    [const { Owned(UnsafeCell::new(History::new())) }; ENTRIES];

/// Each entry's latest snapshot behind a sequence counter (odd while written).
struct Published {
    seq: AtomicU64,
    words: [AtomicU64; SNAPSHOT_WORDS],
}
static SNAPSHOTS: [Published; ENTRIES] = [const {
    Published {
        seq: AtomicU64::new(0),
        words: [const { AtomicU64::new(0) }; SNAPSHOT_WORDS],
    }
}; ENTRIES];

/// Per G&W entry: session, SPECIAL_S entries and Judges seen, for the
/// end-of-session line.
struct Counts {
    session: AtomicU32,
    special_s: AtomicU32,
    judges: AtomicU32,
}
static COUNTS: [Counts; ENTRIES] = [const {
    Counts {
        session: AtomicU32::new(0),
        special_s: AtomicU32::new(0),
        judges: AtomicU32::new(0),
    }
}; ENTRIES];

struct JudgeSlot {
    seq: AtomicU64,
    session: AtomicU32,
    words: [AtomicU64; JUDGE_WORDS],
}
static JUDGES: [JudgeSlot; JUDGE_RING] = [const {
    JudgeSlot {
        seq: AtomicU64::new(0),
        session: AtomicU32::new(0),
        words: [const { AtomicU64::new(0) }; JUDGE_WORDS],
    }
}; JUDGE_RING];
static JUDGE_HEAD: AtomicU64 = AtomicU64::new(0);

/// Each G&W entry's drawn Judge until it leaves that side special (then the
/// hook queues it with its outcome). The writer takes one left over when the
/// session ends (frames stopped mid-Judge).
const PENDING_EMPTY: u32 = 0;
const PENDING_SET: u32 = 1;
const PENDING_TAKEN: u32 = 2;
struct PendingSlot {
    state: AtomicU32,
    session: AtomicU32,
    words: [AtomicU64; JUDGE_WORDS],
}
static PENDING: [PendingSlot; ENTRIES] = [const {
    PendingSlot {
        state: AtomicU32::new(PENDING_EMPTY),
        session: AtomicU32::new(0),
        words: [const { AtomicU64::new(0) }; JUDGE_WORDS],
    }
}; ENTRIES];

const NOTE_GW: u8 = 1;
const NOTE_FIRST: u8 = 2;
struct NoteSlot {
    seq: AtomicU64,
    tick: AtomicU64,
    code: AtomicU64,
    a: AtomicU64,
    b: AtomicU64,
}
static NOTES: [NoteSlot; NOTE_RING] = [const {
    NoteSlot {
        seq: AtomicU64::new(0),
        tick: AtomicU64::new(0),
        code: AtomicU64::new(0),
        a: AtomicU64::new(0),
        b: AtomicU64::new(0),
    }
}; NOTE_RING];
static NOTE_HEAD: AtomicU64 = AtomicU64::new(0);

static LAST_FRAME_TICK: AtomicU64 = AtomicU64::new(0);
static SESSION: AtomicU32 = AtomicU32::new(0);
static FIRST_SEEN: AtomicU32 = AtomicU32::new(0);

/// The writer thread's object and stack, static so no heap is involved.
#[repr(C, align(4096))]
struct ThreadStack(UnsafeCell<[u8; WRITER_STACK]>);
#[repr(C, align(16))]
struct ThreadObject(UnsafeCell<[u8; 0x1c0]>);
// SAFETY: handed to nn::os once, at startup, and never touched again.
unsafe impl Sync for ThreadStack {}
unsafe impl Sync for ThreadObject {}
static WRITER_STACK_MEMORY: ThreadStack = ThreadStack(UnsafeCell::new([0; WRITER_STACK]));
static WRITER_THREAD: ThreadObject = ThreadObject(UnsafeCell::new([0; 0x1c0]));

fn tick() -> u64 {
    unsafe { nn::os::GetSystemTick() as u64 }
}

unsafe fn read_u32(address: u64) -> u32 {
    (address as *const u32).read_unaligned()
}

unsafe fn read_u64(address: u64) -> u64 {
    (address as *const u64).read_unaligned()
}

fn push_note(code: u8, a: u64, b: u64) {
    let index = NOTE_HEAD.fetch_add(1, Ordering::AcqRel);
    let slot = &NOTES[(index % NOTE_RING as u64) as usize];
    slot.tick.store(tick(), Ordering::Relaxed);
    slot.code.store(u64::from(code), Ordering::Relaxed);
    slot.a.store(a, Ordering::Relaxed);
    slot.b.store(b, Ordering::Relaxed);
    slot.seq.store(index + 1, Ordering::Release);
}

fn push_judge(session: u32, judge: &Judge) {
    let index = JUDGE_HEAD.fetch_add(1, Ordering::AcqRel);
    let slot = &JUDGES[(index % JUDGE_RING as u64) as usize];
    slot.session.store(session, Ordering::Relaxed);
    for (cell, word) in slot.words.iter().zip(judge.to_words()) {
        cell.store(word, Ordering::Relaxed);
    }
    slot.seq.store(index + 1, Ordering::Release);
}

/// Single writer per entry: that fighter's own frame.
fn publish(entry: usize, snapshot: &Snapshot) {
    let slot = &SNAPSHOTS[entry];
    let seq = slot.seq.load(Ordering::Relaxed);
    slot.seq.store(seq + 1, Ordering::Relaxed);
    fence(Ordering::Release);
    for (cell, word) in slot.words.iter().zip(snapshot.to_words()) {
        cell.store(word, Ordering::Relaxed);
    }
    slot.seq.store(seq + 2, Ordering::Release);
}

fn snapshot_of(entry: usize) -> Option<Snapshot> {
    let slot = &SNAPSHOTS[entry];
    for _ in 0..4 {
        let before = slot.seq.load(Ordering::Acquire);
        if before == 0 {
            return None;
        }
        if before & 1 != 0 {
            continue;
        }
        let mut words = [0; SNAPSHOT_WORDS];
        for (word, cell) in words.iter_mut().zip(&slot.words) {
            *word = cell.load(Ordering::Relaxed);
        }
        fence(Ordering::Acquire);
        if slot.seq.load(Ordering::Relaxed) == before {
            return Some(Snapshot::from_words(&words));
        }
    }
    None
}

fn read_judge(words: &[AtomicU64; JUDGE_WORDS]) -> Judge {
    let mut raw = [0; JUDGE_WORDS];
    for (word, cell) in raw.iter_mut().zip(words) {
        *word = cell.load(Ordering::Relaxed);
    }
    Judge::from_words(&raw)
}

/// Queues the entry's pending Judge with `hit`, unless the writer took it.
fn finish_pending(slot: &PendingSlot, mut judge: Judge, hit: Option<Contact>) {
    judge.hit = hit;
    if slot
        .state
        .compare_exchange(
            PENDING_SET,
            PENDING_EMPTY,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_ok()
    {
        push_judge(slot.session.load(Ordering::Relaxed), &judge);
    }
}

/// Follows G&W's pending Judge one frame: records the contact so far, or
/// queues it once G&W left the side special (or the session changed).
fn follow_pending(entry: usize, session: u32, history: &History) {
    let slot = &PENDING[entry];
    if slot.state.load(Ordering::Acquire) != PENDING_SET {
        return;
    }
    let judge = read_judge(&slot.words);
    if slot.session.load(Ordering::Relaxed) != session {
        finish_pending(slot, judge, judge.hit);
        return;
    }
    let so_far = judge.hit.unwrap_or_default();
    match outcome(judge.gw.cur, so_far, history) {
        Outcome::Running(contact) => {
            let hit = (contact != Contact::None).then_some(contact);
            let word = Judge { hit, ..judge }.to_words()[0];
            slot.words[0].store(word, Ordering::Relaxed);
        }
        Outcome::Done(contact) => finish_pending(slot, judge, Some(contact)),
    }
}

/// Holds a new Judge until its outcome is known.
fn hold(entry: usize, session: u32, judge: &Judge) {
    let slot = &PENDING[entry];
    if slot.state.load(Ordering::Acquire) == PENDING_SET {
        // Drawn again within one side special: the earlier one is done.
        let earlier = read_judge(&slot.words);
        finish_pending(slot, earlier, earlier.hit);
    }
    if slot.state.load(Ordering::Acquire) != PENDING_EMPTY {
        push_judge(session, judge);
        return;
    }
    for (cell, word) in slot.words.iter().zip(judge.to_words()) {
        cell.store(word, Ordering::Relaxed);
    }
    slot.session.store(session, Ordering::Relaxed);
    slot.state.store(PENDING_SET, Ordering::Release);
}

/// The session of a frame starting now; the first frame after a gap opens one.
fn session_at(now: u64) -> u32 {
    let last = LAST_FRAME_TICK.swap(now, Ordering::AcqRel);
    if last == 0 || now.wrapping_sub(last) > SESSION_GAP_TICKS {
        SESSION.fetch_add(1, Ordering::AcqRel) + 1
    } else {
        SESSION.load(Ordering::Acquire)
    }
}

#[skyline::hook(offset = model::FIGHTER_FRAME_OFFSET, inline)]
unsafe fn gnw_judge_fighter_frame(ctx: &InlineCtx) {
    let fighter = ctx.registers[0].x();
    if fighter < 0x10000 {
        return;
    }
    // The game reads the entry as a 32-bit value (`ldp w9, w8, [x0, #0xc]`).
    let entry = read_u32(fighter + OBJECT_ENTRY as u64) as usize;
    let accessor = read_u64(fighter + OBJECT_MODULE_ACCESSOR as u64);
    if entry >= ENTRIES || accessor < 0x10000 {
        return;
    }
    // Ice Climbers' partner and Pokémon Trainer's resting Pokémon share their
    // entry: follow only the entry's active fighter.
    if read_u32(fighter + OBJECT_ID as u64) != id_from_entry(entry as i32) {
        return;
    }
    let kind = read_u32(fighter + OBJECT_KIND as u64);
    if FIRST_SEEN.swap(1, Ordering::AcqRel) == 0 {
        push_note(
            NOTE_FIRST,
            u64::from(kind) | (entry as u64) << 32,
            accessor.wrapping_sub(fighter),
        );
    }
    let session = session_at(tick());
    // SAFETY: only this entry's own frame touches its history.
    let history = &mut *HISTORY[entry].0.get();
    if history.session != session {
        history.begin(session);
    }
    let contact = Contact::new(
        is_infliction_status(accessor, COLLISION_KIND_MASK_HIT),
        is_infliction_status(accessor, COLLISION_KIND_MASK_SHIELD),
        is_infliction_status(accessor, COLLISION_KIND_MASK_ATTACK),
    );
    history.observe(kind, status_kind(accessor), motion_kind(accessor), contact);
    let snapshot = Snapshot {
        session,
        frame: history.frame.wrapping_sub(1),
        entry: entry as u8,
        fighter: kind,
        place: Place::new(situation_kind(accessor), is_passable_ground(accessor)),
        damage: damage(accessor, 0),
        x: pos_x(accessor),
        y: pos_y(accessor),
        cur: history.cur,
        prev: history.prev,
        last_move: history.last_move,
    };
    publish(entry, &snapshot);
    if kind != FIGHTER_KIND_GAMEWATCH {
        return;
    }
    follow_pending(entry, session, history);
    let panel = work_int(accessor, WORK_SPECIAL_S_PREV_KIND);
    if history.frame == 1 {
        let kind = work_int(accessor, WORK_SPECIAL_S_KIND);
        push_note(
            NOTE_GW,
            u64::from(session) | (entry as u64) << 32,
            u64::from(kind as u32) | u64::from(panel as u32) << 32,
        );
    }
    let drawn = history.panel(panel);
    let counts = &COUNTS[entry];
    counts.special_s.store(history.special_s, Ordering::Relaxed);
    counts.judges.store(history.judges, Ordering::Relaxed);
    counts.session.store(session, Ordering::Release);
    let Some(drawn) = drawn else {
        return;
    };
    let mut players = 1;
    let mut opp = None;
    for other in (0..ENTRIES).filter(|other| *other != entry) {
        if let Some(seen) = snapshot_of(other).filter(|seen| seen.session == session) {
            players += 1;
            opp = opp.or(Some(seen));
        }
    }
    hold(
        entry,
        session,
        &Judge {
            drawn,
            players,
            hit: None,
            gw: snapshot,
            opp,
        },
    );
}

type Path = Line<96>;

fn path(args: core::fmt::Arguments) -> Path {
    let mut path = Path::new();
    let _ = path.write_fmt(args);
    path
}

/// An `nn::fs` file opened to append; closed on drop. A closed file is on
/// the card at once, while Eden keeps an open file's writes until the host
/// file is closed (its `FlushFile` does not reach the host).
struct NxFile {
    handle: nn::fs::FileHandle,
    offset: i64,
}

impl NxFile {
    fn append(path: &mut Path) -> Option<Self> {
        let path = path.with_nul()?.as_ptr();
        unsafe {
            // Fails harmlessly when the file exists.
            nn::fs::CreateFile(path, 0);
            let mut handle = nn::fs::FileHandle { handle: 0 };
            if nn::fs::OpenFile(&mut handle, path, OPEN_WRITE_APPEND) != 0 {
                return None;
            }
            let mut offset = 0;
            if nn::fs::GetFileSize(&mut offset, handle) != 0 {
                nn::fs::CloseFile(handle);
                return None;
            }
            Some(Self { handle, offset })
        }
    }

    fn write(&mut self, bytes: &[u8]) -> bool {
        let option = nn::fs::WriteOption { flags: 0 };
        let written = unsafe {
            nn::fs::WriteFile(
                self.handle,
                self.offset,
                bytes.as_ptr(),
                bytes.len() as u64,
                &option,
            )
        } == 0;
        if written {
            self.offset += bytes.len() as i64;
        }
        written
    }
}

impl Drop for NxFile {
    fn drop(&mut self) {
        unsafe {
            nn::fs::FlushFile(self.handle);
            nn::fs::CloseFile(self.handle);
        }
    }
}

fn append_log(bytes: &[u8]) {
    if let Some(mut log) = NxFile::append(&mut path(format_args!("{DATA_DIR}/plugin.log"))) {
        log.write(bytes);
    }
}

/// Appends rows to `judges.tsv`, writing the header first into a new file.
fn append_rows(bytes: &[u8]) {
    let Some(mut file) = NxFile::append(&mut path(format_args!("{DATA_DIR}/judges.tsv"))) else {
        return;
    };
    if file.offset == 0 && !(file.write(model::HEADER.as_bytes()) && file.write(b"\n")) {
        return;
    }
    file.write(bytes);
}

/// This boot's id for the `boot` column: the local time of its first row,
/// `YYYYMMDD-hhmmss` (the clock is not set yet when plugins load), or the
/// system tick if it still is not.
fn boot_id() -> Line<32> {
    use nn::time::{
        CalendarAdditionalInfo, CalendarTime, PosixTime, StandardUserSystemClock, TimeZone,
    };
    let mut id = Line::new();
    unsafe {
        let mut posix = PosixTime { time: 0 };
        let mut calendar = CalendarTime {
            year: 0,
            month: 0,
            day: 0,
            hour: 0,
            minute: 0,
            second: 0,
        };
        let mut info = CalendarAdditionalInfo {
            dayOfTheWeek: 0,
            dayofYear: 0,
            timeZone: TimeZone::default(),
        };
        if nn::time::IsInitialized()
            && StandardUserSystemClock::GetCurrentTime(&mut posix) == 0
            && nn::time::ToCalendarTime(&mut calendar, &mut info, &posix) == 0
        {
            let _ = write!(
                id,
                "{:04}{:02}{:02}-{:02}{:02}{:02}",
                calendar.year,
                calendar.month,
                calendar.day,
                calendar.hour,
                calendar.minute,
                calendar.second
            );
        } else {
            let _ = write!(id, "tick{}", tick());
        }
    }
    id
}

struct Writer {
    boot: Line<32>,
    judge_tail: u64,
    note_tail: u64,
    reported: u32,
}

impl Writer {
    fn boot(&mut self) -> Line<32> {
        if self.boot.as_bytes().is_empty() {
            self.boot = boot_id();
        }
        let mut copy = Line::new();
        let _ = copy.write_str(core::str::from_utf8(self.boot.as_bytes()).unwrap_or("-"));
        copy
    }

    fn drain_judges(&mut self) {
        let head = JUDGE_HEAD.load(Ordering::Acquire);
        if head.saturating_sub(self.judge_tail) > JUDGE_RING as u64 {
            self.judge_tail = head - JUDGE_RING as u64;
        }
        if self.judge_tail == head {
            return;
        }
        let boot_line = self.boot();
        let boot = core::str::from_utf8(boot_line.as_bytes()).unwrap_or("-");
        let mut line = Line::<1024>::new();
        let mut batch = [0u8; 8192];
        let mut used = 0;
        while self.judge_tail < head {
            let slot = &JUDGES[(self.judge_tail % JUDGE_RING as u64) as usize];
            if slot.seq.load(Ordering::Acquire) != self.judge_tail + 1 {
                break;
            }
            let mut words = [0; JUDGE_WORDS];
            for (word, cell) in words.iter_mut().zip(&slot.words) {
                *word = cell.load(Ordering::Relaxed);
            }
            let session = slot.session.load(Ordering::Relaxed);
            self.judge_tail += 1;
            line.clear();
            let _ = write_row(&mut line, boot, session, &Judge::from_words(&words));
            let _ = line.write_char('\n');
            let text = line.as_bytes();
            if used + text.len() > batch.len() {
                append_rows(&batch[..used]);
                used = 0;
            }
            batch[used..used + text.len()].copy_from_slice(text);
            used += text.len();
        }
        if used > 0 {
            append_rows(&batch[..used]);
        }
    }

    fn drain_notes(&mut self) {
        let head = NOTE_HEAD.load(Ordering::Acquire);
        if head.saturating_sub(self.note_tail) > NOTE_RING as u64 {
            self.note_tail = head - NOTE_RING as u64;
        }
        let mut line = Line::<192>::new();
        while self.note_tail < head {
            let slot = &NOTES[(self.note_tail % NOTE_RING as u64) as usize];
            if slot.seq.load(Ordering::Acquire) != self.note_tail + 1 {
                break;
            }
            let (when, code) = (
                slot.tick.load(Ordering::Relaxed),
                slot.code.load(Ordering::Relaxed) as u8,
            );
            let (a, b) = (
                slot.a.load(Ordering::Relaxed),
                slot.b.load(Ordering::Relaxed),
            );
            self.note_tail += 1;
            line.clear();
            let _ = match code {
                NOTE_FIRST => write!(
                    line,
                    "{when} first_fighter kind={} entry={} accessor_offset={:#x}",
                    model::fighter_name(a as u32),
                    a >> 32,
                    b
                ),
                NOTE_GW => write!(
                    line,
                    "{when} gamewatch session={} entry={} panel={} prev_panel={}",
                    a as u32,
                    a >> 32,
                    b as u32 as i32,
                    (b >> 32) as u32 as i32
                ),
                _ => write!(line, "{when} note code={code} a={a:#x} b={b:#x}"),
            };
            let _ = line.write_char('\n');
            append_log(line.as_bytes());
        }
    }

    /// Writes a Judge whose side special was still running when the frames
    /// stopped (outcome unknown unless it had already connected).
    fn flush_pending(&mut self) {
        if PENDING
            .iter()
            .all(|slot| slot.state.load(Ordering::Acquire) != PENDING_SET)
        {
            return;
        }
        let boot_line = self.boot();
        let boot = core::str::from_utf8(boot_line.as_bytes()).unwrap_or("-");
        let mut line = Line::<1024>::new();
        for slot in &PENDING {
            if slot
                .state
                .compare_exchange(
                    PENDING_SET,
                    PENDING_TAKEN,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_err()
            {
                continue;
            }
            let judge = read_judge(&slot.words);
            line.clear();
            let _ = write_row(
                &mut line,
                boot,
                slot.session.load(Ordering::Relaxed),
                &judge,
            );
            let _ = line.write_char('\n');
            append_rows(line.as_bytes());
            slot.state.store(PENDING_EMPTY, Ordering::Release);
        }
    }

    /// After the session gap: leftover Judges, then one line per G&W in the
    /// session that ended (side specials started vs Judges logged).
    fn report_session(&mut self) {
        let session = SESSION.load(Ordering::Acquire);
        let idle = tick().saturating_sub(LAST_FRAME_TICK.load(Ordering::Acquire));
        if session == self.reported || idle <= SESSION_GAP_TICKS {
            return;
        }
        self.reported = session;
        self.flush_pending();
        let mut line = Line::<192>::new();
        for (entry, counts) in COUNTS.iter().enumerate() {
            if counts.session.load(Ordering::Acquire) != session {
                continue;
            }
            line.clear();
            let _ = writeln!(
                line,
                "{} session_end session={session} entry={entry} special_s={} judges={} heap_allocations={}",
                tick(),
                counts.special_s.load(Ordering::Relaxed),
                counts.judges.load(Ordering::Relaxed),
                HEAP_ALLOCATIONS.load(Ordering::Relaxed)
            );
            append_log(line.as_bytes());
        }
    }

    fn run(mut self) -> ! {
        loop {
            unsafe {
                nn::os::SleepThread(nn::TimeSpan {
                    nanoseconds: 100_000_000,
                })
            };
            self.drain_notes();
            self.drain_judges();
            self.report_session();
        }
    }
}

extern "C" fn writer_main(_: *mut skyline::libc::c_void) {
    Writer {
        boot: Line::new(),
        judge_tail: 0,
        note_tail: 0,
        reported: 0,
    }
    .run();
}

fn start_writer() -> bool {
    unsafe {
        let thread = WRITER_THREAD.0.get().cast::<nn::os::ThreadType>();
        let priority = nn::os::GetThreadPriority(nn::os::GetCurrentThread());
        let created = nn::os::CreateThread(
            thread,
            writer_main,
            core::ptr::null_mut(),
            WRITER_STACK_MEMORY.0.get().cast::<u8>(),
            WRITER_STACK,
            priority,
        );
        if created != 0 {
            return false;
        }
        nn::os::SetThreadNamePointer(thread, c"gnw-judge-log".as_ptr().cast());
        nn::os::StartThread(thread);
        true
    }
}

unsafe fn guard_matches(site: &Site) -> bool {
    let base = getRegionAddress(Region::Text) as usize;
    let end = getRegionAddress(Region::Rodata) as usize;
    let address = base + site.offset;
    address.is_multiple_of(4)
        && address + site.guard.len() <= end
        && core::slice::from_raw_parts(address as *const u8, site.guard.len()) == site.guard
}

fn version_is_supported() -> bool {
    let mut version = nn::oe::DisplayVersion { name: [0; 16] };
    unsafe { nn::oe::GetDisplayVersion(&mut version) };
    let end = version.name.iter().position(|v| *v == 0).unwrap_or(16);
    version.name[..end] == *model::SUPPORTED_VERSION.as_bytes()
}

/// Appends the install line to `plugin.log` and the Skyline log.
fn report_install(report: &mut Line<256>) {
    let _ = write!(
        report,
        " heap_allocations={}",
        HEAP_ALLOCATIONS.load(Ordering::Relaxed)
    );
    skyline::logging::log(core::str::from_utf8(report.as_bytes()).unwrap_or(""));
    let _ = report.write_char('\n');
    for directory in ["sd:/ultimate", DATA_DIR] {
        if let Some(directory) = path(format_args!("{directory}")).with_nul() {
            unsafe { nn::fs::CreateDirectory(directory.as_ptr()) };
        }
    }
    append_log(report.as_bytes());
}

#[skyline::main(name = "ssbu-gnw-judge-log")]
pub fn main() {
    let mut report = Line::<256>::new();
    let _ = write!(
        report,
        "{} plugin=ssbu-gnw-judge-log/{}",
        tick(),
        env!("CARGO_PKG_VERSION")
    );
    if !version_is_supported() {
        let _ = write!(report, " result=unsupported_version");
        report_install(&mut report);
        return;
    }
    if !unsafe { guard_matches(&FIGHTER_FRAME) } {
        let _ = write!(report, " result=guard_mismatch site={}", FIGHTER_FRAME.name);
        report_install(&mut report);
        return;
    }
    // The writer appends to the same log, so it starts after this line (and
    // the folder) exist; the hook only fills rings until then.
    skyline::install_hook!(gnw_judge_fighter_frame);
    let _ = write!(report, " hooks={}", FIGHTER_FRAME.name);
    report_install(&mut report);
    if !start_writer() {
        report.clear();
        let _ = write!(report, "{} writer=not_started", tick());
        report_install(&mut report);
    }
}
