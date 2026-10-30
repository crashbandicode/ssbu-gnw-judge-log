//! Host-testable half: the hook site, each fighter's recent actions, Judge
//! detection, the record the hook hands to the writer, and the TSV row.
//!
//! Game & Watch's Judge panel is the instance work int `SPECIAL_S_KIND` (0..=8
//! for 1..9, -1 for none); `FighterSpecializer_GameWatch::init_panel`
//! (`.text+0xa84470`) copies it to `SPECIAL_S_PREV_KIND` and resets it to -1.
//! The draw runs in G&W's fighter scripts (`lua2cpp_gamewatch`) and, as Eden
//! shows, the number is drawn and moved within one frame: between frames
//! `SPECIAL_S_KIND` is always -1 and `SPECIAL_S_PREV_KIND` holds the number
//! of the Judge in progress (the G&W hit handler at `.text+0xa82660` reads it
//! as such). So once per frame the plugin reads `SPECIAL_S_PREV_KIND`: a new
//! value is a new Judge, and the value it replaced is the number the draw
//! could not repeat. The plugin never touches the draw.

use crate::names;
use core::fmt;

pub const SUPPORTED_VERSION: &str = "13.0.5";

/// An inline hook site and the 64 bytes that must be there (13.0.5 image).
pub struct Site {
    pub name: &'static str,
    pub offset: usize,
    pub guard: [u8; 64],
}

/// Entry of the Fighter vtable's per-frame function (vtable slot
/// `.text+0x4f80568`), run once per frame for every fighter before it
/// updates: `sub sp, sp, #0xc0` then register saves. x0 = the Fighter.
pub const FIGHTER_FRAME_OFFSET: usize = 0x614630;
pub const FIGHTER_FRAME: Site = Site {
    name: "fighter_frame",
    offset: FIGHTER_FRAME_OFFSET,
    guard: [
        0xff, 0x03, 0x03, 0xd1, 0xea, 0x23, 0x00, 0xfd, 0xe9, 0x23, 0x05, 0x6d, 0xfc, 0x6f, 0x06,
        0xa9, 0xfa, 0x67, 0x07, 0xa9, 0xf8, 0x5f, 0x08, 0xa9, 0xf6, 0x57, 0x09, 0xa9, 0xf4, 0x4f,
        0x0a, 0xa9, 0xfd, 0x7b, 0x0b, 0xa9, 0xfd, 0xc3, 0x02, 0x91, 0x28, 0x65, 0x02, 0xb0, 0x19,
        0x7d, 0x42, 0xf9, 0x28, 0x65, 0x02, 0x90, 0x08, 0xa9, 0x42, 0xf9, 0x09, 0x21, 0x40, 0x39,
        0xf3, 0x03, 0x00, 0xaa,
    ],
};

/// `app::BattleObject` fields: battle object id, fighter kind, entry, module accessor.
pub const OBJECT_ID: usize = 0x8;
pub const OBJECT_KIND: usize = 0xc;
pub const OBJECT_ENTRY: usize = 0x10;
pub const OBJECT_MODULE_ACCESSOR: usize = 0x20;

pub const ENTRIES: usize = 8;
pub const FIGHTER_KIND_GAMEWATCH: u32 = names::GAMEWATCH;
pub const STATUS_SPECIAL_S: i32 = 0x1dd;
pub const WORK_SPECIAL_S_KIND: i32 = 0x1000_00c2;
pub const WORK_SPECIAL_S_PREV_KIND: i32 = 0x1000_00c1;
/// Frames left in which a Judge cannot be a 7 (after a 7 hits).
pub const WORK_SPECIAL_S_PANEL_7_INVALID_FRAME: i32 = 0x1000_00c5;
/// `.text` slot holding a pointer to the RNG manager pointer; the manager
/// starts with a `[begin, end)` vector of 20-byte stream states. Stream 1 is
/// `fighter`, the one `sv_math::rand(hash40("fighter"), n)` draws from.
pub const RNG_MANAGER: usize = 0x52c35b0;
pub const RNG_STATE_STRIDE: u64 = 0x14;
pub const FIGHTER_STREAM: u64 = 1;
pub const SITUATION_GROUND: i32 = 0;
pub const SITUATION_CLIFF: i32 = 1;
pub const SITUATION_AIR: i32 = 2;

/// Where a fighter is.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Place {
    #[default]
    Unknown,
    Ground,
    Platform,
    Air,
    Ledge,
}

impl Place {
    /// From `StatusModule::situation_kind` and `GroundModule::is_passable_ground`
    /// (true while standing on a soft platform).
    pub fn new(situation: i32, passable: bool) -> Self {
        match situation {
            SITUATION_GROUND if passable => Place::Platform,
            SITUATION_GROUND => Place::Ground,
            SITUATION_AIR => Place::Air,
            SITUATION_CLIFF => Place::Ledge,
            _ => Place::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Place::Unknown => "-",
            Place::Ground => "ground",
            Place::Platform => "platform",
            Place::Air => "air",
            Place::Ledge => "ledge",
        }
    }

    fn code(self) -> u64 {
        self as u64
    }

    fn from_code(code: u64) -> Self {
        match code {
            1 => Place::Ground,
            2 => Place::Platform,
            3 => Place::Air,
            4 => Place::Ledge,
            _ => Place::Unknown,
        }
    }
}

/// Collision masks for `AttackModule::is_infliction_status`.
pub const COLLISION_KIND_MASK_ATTACK: u32 = 0x1;
pub const COLLISION_KIND_MASK_HIT: u32 = 0x2;
pub const COLLISION_KIND_MASK_SHIELD: u32 = 0x4;

/// What the fighter's own hitboxes connected with during a status (from
/// `AttackModule::is_infliction_status`; projectiles are separate objects and
/// do not count). Ordered: a status that hit and was also shielded is `Hit`.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub enum Contact {
    #[default]
    None,
    /// Traded with another attack.
    Clank,
    Shield,
    Hit,
}

impl Contact {
    pub fn new(hit: bool, shield: bool, clank: bool) -> Self {
        if hit {
            Contact::Hit
        } else if shield {
            Contact::Shield
        } else if clank {
            Contact::Clank
        } else {
            Contact::None
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Contact::None => "none",
            Contact::Clank => "clank",
            Contact::Shield => "shield",
            Contact::Hit => "hit",
        }
    }

    fn code(self) -> u64 {
        self as u64
    }

    fn from_code(code: u64) -> Self {
        match code {
            1 => Contact::Clank,
            2 => Contact::Shield,
            3 => Contact::Hit,
            _ => Contact::None,
        }
    }
}

/// A status a fighter was in: its kind, the last motion seen in it, the
/// fighter frame it started on and what its hitboxes connected with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Action {
    pub status: i32,
    pub motion: u64,
    pub start: u32,
    pub contact: Contact,
}

impl Action {
    pub const NONE: Action = Action {
        status: -1,
        motion: 0,
        start: 0,
        contact: Contact::None,
    };

    /// A hash40 uses 40 bits; the contact goes above it.
    fn words(self) -> [u64; 2] {
        [
            u64::from(self.status as u32) | u64::from(self.start) << 32,
            self.motion & 0xff_ffff_ffff | self.contact.code() << 56,
        ]
    }

    fn from_words(words: [u64; 2]) -> Self {
        Action {
            status: words[0] as u32 as i32,
            motion: words[1] & 0xff_ffff_ffff,
            start: (words[0] >> 32) as u32,
            contact: Contact::from_code(words[1] >> 56),
        }
    }
}

/// Whether a status is a move (attack, special, grab, throw, final smash...).
/// Fighter-specific statuses past the common range are nearly all parts of
/// special moves, so they count as moves; for G&W the names decide.
pub fn is_move(fighter: u32, status: i32) -> bool {
    if status < 0 {
        return false;
    }
    if status <= names::COMMON_STATUS_LAST {
        return names::COMMON_STATUS[status as usize].1;
    }
    if fighter == FIGHTER_KIND_GAMEWATCH {
        let index = (status - names::COMMON_STATUS_LAST - 1) as usize;
        return names::GAMEWATCH_STATUS
            .get(index)
            .is_none_or(|entry| entry.1);
    }
    true
}

/// One fighter's frame counter, current, previous and last move status, and
/// (for G&W) the Judge panel last seen. Owned by that fighter's own tick.
#[derive(Clone, Copy, Debug)]
pub struct History {
    pub session: u32,
    pub frame: u32,
    pub cur: Action,
    pub prev: Action,
    /// The most recent move status before the current status.
    pub last_move: Action,
    started: bool,
    panel: Option<i32>,
    last_number: u8,
    pub special_s: u32,
    pub judges: u32,
}

/// A Judge number as it appeared.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Drawn {
    pub number: u8,
    /// The number the game held as the previous panel when drawing, the one
    /// it could not repeat; 0 when none (start of a match, after a KO).
    pub excluded: u8,
    /// The previous Judge's number in this session, 0 for the first.
    pub last_number: u8,
}

impl History {
    pub const fn new() -> Self {
        Self {
            session: 0,
            frame: 0,
            cur: Action::NONE,
            prev: Action::NONE,
            last_move: Action::NONE,
            started: false,
            panel: None,
            last_number: 0,
            special_s: 0,
            judges: 0,
        }
    }

    /// Forgets everything; the next frame is frame 0 of `session`.
    pub fn begin(&mut self, session: u32) {
        *self = Self::new();
        self.session = session;
    }

    /// One frame of this fighter: its status, motion and what its hitboxes
    /// have connected with in this status so far. Returns true when it
    /// entered a new status.
    pub fn observe(&mut self, fighter: u32, status: i32, motion: u64, contact: Contact) -> bool {
        let frame = self.frame;
        self.frame = self.frame.wrapping_add(1);
        let action = Action {
            status,
            motion,
            start: frame,
            contact,
        };
        if !self.started {
            self.started = true;
            self.cur = action;
            return false;
        }
        if status == self.cur.status {
            self.cur.motion = motion;
            self.cur.contact = self.cur.contact.max(contact);
            return false;
        }
        if is_move(fighter, self.cur.status) {
            self.last_move = self.cur;
        }
        self.prev = self.cur;
        self.cur = action;
        if fighter == FIGHTER_KIND_GAMEWATCH && status == STATUS_SPECIAL_S {
            self.special_s += 1;
        }
        true
    }

    /// G&W's `SPECIAL_S_PREV_KIND` this frame; a `Drawn` when it changed to a
    /// panel since the last frame. The first value of a session only sets the
    /// baseline.
    pub fn panel(&mut self, prev_kind: i32) -> Option<Drawn> {
        let previous = self.panel.replace(prev_kind)?;
        if (0..=8).contains(&prev_kind) && prev_kind != previous {
            self.judges += 1;
            let number = prev_kind as u8 + 1;
            let last_number = core::mem::replace(&mut self.last_number, number);
            return Some(Drawn {
                number,
                excluded: panel_number(previous),
                last_number,
            });
        }
        None
    }

    /// Frames since `action` started, as of the current frame.
    pub fn ago(&self, action: Action) -> u32 {
        self.frame.wrapping_sub(1).wrapping_sub(action.start)
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

/// A fighter as one frame saw it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snapshot {
    pub session: u32,
    pub frame: u32,
    pub entry: u8,
    pub fighter: u32,
    pub place: Place,
    pub damage: f32,
    pub x: f32,
    pub y: f32,
    pub cur: Action,
    pub prev: Action,
    pub last_move: Action,
}

pub const SNAPSHOT_WORDS: usize = 10;

impl Snapshot {
    pub fn to_words(&self) -> [u64; SNAPSHOT_WORDS] {
        let [cur0, cur1] = self.cur.words();
        let [prev0, prev1] = self.prev.words();
        let [last0, last1] = self.last_move.words();
        [
            u64::from(self.session) | u64::from(self.frame) << 32,
            u64::from(self.fighter) | u64::from(self.entry) << 32 | self.place.code() << 40,
            u64::from(self.damage.to_bits()) | u64::from(self.x.to_bits()) << 32,
            u64::from(self.y.to_bits()),
            cur0,
            cur1,
            prev0,
            prev1,
            last0,
            last1,
        ]
    }

    pub fn from_words(w: &[u64; SNAPSHOT_WORDS]) -> Self {
        Snapshot {
            session: w[0] as u32,
            frame: (w[0] >> 32) as u32,
            fighter: w[1] as u32,
            entry: (w[1] >> 32) as u8,
            place: Place::from_code(w[1] >> 40 & 0xff),
            damage: f32::from_bits(w[2] as u32),
            x: f32::from_bits((w[2] >> 32) as u32),
            y: f32::from_bits(w[3] as u32),
            cur: Action::from_words([w[4], w[5]]),
            prev: Action::from_words([w[6], w[7]]),
            last_move: Action::from_words([w[8], w[9]]),
        }
    }

    /// Frames since `action` started.
    pub fn ago(&self, action: Action) -> u32 {
        self.frame.wrapping_sub(action.start)
    }
}

/// The game's `fighter` RNG stream (index 1 of the RNG manager): xorshift128
/// state and draw counter, laid out as in memory (`+0x0..+0x10`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Rng {
    pub x: u32,
    pub y: u32,
    pub z: u32,
    pub w: u32,
    pub count: u32,
}

impl Rng {
    /// The next raw output, exactly as `sv_math::rand` (`.text+0x2275660`)
    /// computes it, advancing this copy (never the game's).
    pub fn step(&mut self) -> u32 {
        let t = self.x ^ self.x << 11;
        let t = t ^ t >> 8;
        let out = t ^ self.w ^ self.w >> 19;
        (self.x, self.y, self.z, self.w) = (self.y, self.z, self.w, out);
        self.count = self.count.wrapping_add(1);
        out
    }

    /// `sv_math::rand(hash, n)`: the next output modulo `n`.
    pub fn rand(&mut self, n: u32) -> u32 {
        let out = self.step();
        if n >= 1 {
            out % n
        } else {
            out
        }
    }
}

/// The fighter stream around a Judge: at the start of G&W's frame in which
/// the number was drawn (`at_draw`), one frame earlier (`before`), the draw
/// count at the start of the next frame, and G&W's "no 7" timer
/// (`SPECIAL_S_PANEL_7_INVALID_FRAME`) at the draw frame.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RngTrace {
    pub at_draw: Rng,
    pub before: Rng,
    pub count_after: u32,
    pub seven_block: i32,
}

impl RngTrace {
    fn to_words(self) -> [u64; 6] {
        let pair = |a: u32, b: u32| u64::from(a) | u64::from(b) << 32;
        let (d, b) = (self.at_draw, self.before);
        [
            pair(d.x, d.y),
            pair(d.z, d.w),
            pair(d.count, self.count_after),
            pair(b.x, b.y),
            pair(b.z, b.w),
            pair(b.count, self.seven_block as u32),
        ]
    }

    fn from_words(w: &[u64]) -> Self {
        let (lo, hi) = (|v: u64| v as u32, |v: u64| (v >> 32) as u32);
        RngTrace {
            at_draw: Rng {
                x: lo(w[0]),
                y: hi(w[0]),
                z: lo(w[1]),
                w: hi(w[1]),
                count: lo(w[2]),
            },
            count_after: hi(w[2]),
            before: Rng {
                x: lo(w[3]),
                y: hi(w[3]),
                z: lo(w[4]),
                w: hi(w[4]),
                count: lo(w[5]),
            },
            seven_block: hi(w[5]) as i32,
        }
    }
}

/// One Judge: its number (with the excluded and the previous number), what
/// the Judge itself connected with, G&W and the opponent (the lowest other
/// entry seen in the same session). Context is as of the frame the number
/// appeared; `hit` is known once G&W leaves that side special.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Judge {
    pub drawn: Drawn,
    /// `app::stage::get_stage_id` when the number appeared.
    pub stage: u16,
    /// `None` while unknown (the session ended during the Judge).
    pub hit: Option<Contact>,
    pub gw: Snapshot,
    pub opp: Option<Snapshot>,
    pub rng: RngTrace,
}

pub const JUDGE_WORDS: usize = 1 + 2 * SNAPSHOT_WORDS + 6;
const RNG_AT: usize = 1 + 2 * SNAPSHOT_WORDS;
const HIT_UNKNOWN: u64 = 0xff;

impl Judge {
    pub fn to_words(&self) -> [u64; JUDGE_WORDS] {
        let mut words = [0; JUDGE_WORDS];
        words[0] = u64::from(self.drawn.number)
            | u64::from(self.drawn.excluded) << 8
            | u64::from(self.opp.is_some()) << 24
            | self.hit.map_or(HIT_UNKNOWN, Contact::code) << 32
            | u64::from(self.drawn.last_number) << 40
            | u64::from(self.stage) << 48;
        words[1..=SNAPSHOT_WORDS].copy_from_slice(&self.gw.to_words());
        if let Some(opp) = self.opp {
            words[1 + SNAPSHOT_WORDS..RNG_AT].copy_from_slice(&opp.to_words());
        }
        words[RNG_AT..].copy_from_slice(&self.rng.to_words());
        words
    }

    pub fn from_words(words: &[u64; JUDGE_WORDS]) -> Self {
        let snapshot = |at: usize| {
            let mut w = [0; SNAPSHOT_WORDS];
            w.copy_from_slice(&words[at..at + SNAPSHOT_WORDS]);
            Snapshot::from_words(&w)
        };
        Judge {
            drawn: Drawn {
                number: words[0] as u8,
                excluded: (words[0] >> 8) as u8,
                last_number: (words[0] >> 40) as u8,
            },
            stage: (words[0] >> 48) as u16,
            hit: match words[0] >> 32 & 0xff {
                HIT_UNKNOWN => None,
                code => Some(Contact::from_code(code)),
            },
            gw: snapshot(1),
            opp: (words[0] >> 24 & 1 != 0).then(|| snapshot(1 + SNAPSHOT_WORDS)),
            rng: RngTrace::from_words(&words[RNG_AT..]),
        }
    }
}

/// The Judge number for one fighter-stream output: the numbers the draw may
/// give (1..=9 without `excluded`, and without 7 while `seven_blocked`) in
/// order, indexed by the output modulo how many there are. Fitted on Eden
/// Judges and checked forward by the `predicted` column, not taken from the
/// game's script.
pub fn judge_number(output: u32, excluded: u8, seven_blocked: bool) -> u8 {
    let mut allowed = [0u8; 9];
    let mut count = 0;
    for number in 1..=9u8 {
        if number != excluded && !(seven_blocked && number == 7) {
            allowed[count] = number;
            count += 1;
        }
    }
    allowed[(output % count as u32) as usize]
}

/// What a side special started now would show: the next output of `rng`.
pub fn predict(rng: Rng, excluded: u8, seven_blocked: bool) -> u8 {
    let mut copy = rng;
    judge_number(copy.step(), excluded, seven_blocked)
}

/// One stream state as G&W saw it on consecutive frames: the number a side
/// special would have shown on any of them, and the context on the first.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub session: u32,
    pub start: u32,
    pub frames: u32,
    pub predicted: u8,
    pub excluded: u8,
    pub seven_blocked: bool,
    /// Fighter-stream draws that ended it (0: the excluded number changed).
    pub end_draws: u32,
    pub stage: u16,
    pub gw: Snapshot,
    pub opp: Option<Snapshot>,
}

pub const SEGMENT_WORDS: usize = 3 + 2 * SNAPSHOT_WORDS;

impl Segment {
    pub fn to_words(&self) -> [u64; SEGMENT_WORDS] {
        let mut words = [0; SEGMENT_WORDS];
        words[0] = u64::from(self.session) | u64::from(self.start) << 32;
        words[1] = u64::from(self.frames) | u64::from(self.end_draws) << 32;
        words[2] = u64::from(self.predicted)
            | u64::from(self.excluded) << 8
            | u64::from(self.seven_blocked) << 16
            | u64::from(self.opp.is_some()) << 17
            | u64::from(self.stage) << 32;
        words[3..3 + SNAPSHOT_WORDS].copy_from_slice(&self.gw.to_words());
        if let Some(opp) = self.opp {
            words[3 + SNAPSHOT_WORDS..].copy_from_slice(&opp.to_words());
        }
        words
    }

    pub fn from_words(words: &[u64; SEGMENT_WORDS]) -> Self {
        let snapshot = |at: usize| {
            let mut w = [0; SNAPSHOT_WORDS];
            w.copy_from_slice(&words[at..at + SNAPSHOT_WORDS]);
            Snapshot::from_words(&w)
        };
        Segment {
            session: words[0] as u32,
            start: (words[0] >> 32) as u32,
            frames: words[1] as u32,
            end_draws: (words[1] >> 32) as u32,
            predicted: words[2] as u8,
            excluded: (words[2] >> 8) as u8,
            seven_blocked: words[2] >> 16 & 1 != 0,
            stage: (words[2] >> 32) as u16,
            gw: snapshot(3),
            opp: (words[2] >> 17 & 1 != 0).then(|| snapshot(3 + SNAPSHOT_WORDS)),
        }
    }
}

/// Columns of `windows.tsv`: one row per fighter-stream state G&W lived
/// through in a 1v1 (`predicted` = what a side special would have shown).
pub const SEGMENT_HEADER: &str = "boot\tsession\tstart_frame\tframes\tpredicted\tnine\t\
excluded\tseven_block\tend_draws\tstage\tgw_place\tgw_damage\tgw_status\tgw_motion\t\
gw_status_ago\tgw_prev\tgw_prev_motion\tgw_prev_ago\tgw_prev_hit\tgw_last_move\t\
gw_last_move_motion\tgw_last_move_ago\tgw_last_move_hit\topp_fighter\topp_place\t\
opp_damage\topp_status\topp_motion\topp_status_ago\topp_status_hit\topp_prev\t\
opp_prev_motion\topp_prev_ago\topp_prev_hit\topp_last_move\topp_last_move_motion\t\
opp_last_move_ago\topp_last_move_hit\tdistance";

/// One `windows.tsv` row (no newline).
pub fn write_segment(out: &mut impl fmt::Write, boot: &str, segment: &Segment) -> fmt::Result {
    let gw = &segment.gw;
    write!(
        out,
        "{boot}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.1}",
        segment.session,
        segment.start,
        segment.frames,
        segment.predicted,
        u8::from(segment.predicted == 9),
        segment.excluded,
        u8::from(segment.seven_blocked),
        segment.end_draws,
        StageName(segment.stage),
        gw.place.as_str(),
        gw.damage,
    )?;
    write_action(out, gw, gw.cur, false)?;
    write_action(out, gw, gw.prev, true)?;
    write_action(out, gw, gw.last_move, true)?;
    match &segment.opp {
        Some(opp) => {
            write!(
                out,
                "\t{}\t{}\t{:.1}",
                fighter_name(opp.fighter),
                opp.place.as_str(),
                opp.damage
            )?;
            write_action(out, opp, opp.cur, true)?;
            write_action(out, opp, opp.prev, true)?;
            write_action(out, opp, opp.last_move, true)?;
            let distance = ((opp.x - gw.x).powi(2) + (opp.y - gw.y).powi(2)).sqrt();
            write!(out, "\t{distance:.1}")
        }
        None => {
            // fighter, place, damage, 3 actions x 4, distance
            for _ in 0..16 {
                out.write_str("\t-")?;
            }
            Ok(())
        }
    }
}

/// A drawn Judge one frame later.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// Still in the side special it was drawn in: its contact so far.
    Running(Contact),
    /// G&W left it: what the Judge connected with.
    Done(Contact),
}

/// `drawn` = G&W's status when the number appeared, `so_far` = its contact as
/// last seen.
pub fn outcome(drawn: Action, so_far: Contact, history: &History) -> Outcome {
    let same = |action: Action| action.start == drawn.start && action.status == drawn.status;
    if same(history.cur) {
        Outcome::Running(history.cur.contact)
    } else if same(history.prev) {
        Outcome::Done(history.prev.contact)
    } else {
        Outcome::Done(so_far)
    }
}

/// A panel as a Judge number, 0 for none.
pub fn panel_number(prev_kind: i32) -> u8 {
    if (0..=8).contains(&prev_kind) {
        prev_kind as u8 + 1
    } else {
        0
    }
}

/// Columns of `judges.tsv`. `*_ago` = frames since that status started, at
/// the frame the number appeared; `*_hit` = what that status's hitboxes
/// connected with (`opp_status_hit`: so far). Statuses and motions are the
/// game's names (hex when the name is not known).
pub const HEADER: &str = "boot\tsession\tframe\tnumber\tnine\texcluded\tlast_number\t\
judge_hit\tstage\t\
gw_entry\tgw_place\tgw_damage\tgw_status\tgw_motion\tgw_status_ago\t\
gw_prev\tgw_prev_motion\tgw_prev_ago\tgw_prev_hit\t\
gw_last_move\tgw_last_move_motion\tgw_last_move_ago\tgw_last_move_hit\tgw_x\tgw_y\t\
opp_entry\topp_fighter\topp_place\topp_damage\t\
opp_status\topp_motion\topp_status_ago\topp_status_hit\t\
opp_prev\topp_prev_motion\topp_prev_ago\topp_prev_hit\t\
opp_last_move\topp_last_move_motion\topp_last_move_ago\topp_last_move_hit\t\
opp_x\topp_y\tdistance\tpredicted\t\
rng_draw\trng_draw_count\trng_before\trng_before_count\trng_count_after\tseven_block";

struct StatusName(u32, i32);

impl fmt::Display for StatusName {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let (fighter, status) = (self.0, self.1);
        if status < 0 {
            return f.write_str("-");
        }
        let name = if status <= names::COMMON_STATUS_LAST {
            names::COMMON_STATUS[status as usize].0
        } else if fighter == FIGHTER_KIND_GAMEWATCH {
            names::GAMEWATCH_STATUS
                .get((status - names::COMMON_STATUS_LAST - 1) as usize)
                .map_or("", |entry| entry.0)
        } else {
            ""
        };
        if name.is_empty() {
            write!(f, "{status:#x}")
        } else {
            f.write_str(name)
        }
    }
}

struct MotionName(u64);

impl fmt::Display for MotionName {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.0 == 0 {
            return f.write_str("-");
        }
        match names::MOTION.binary_search_by_key(&self.0, |entry| entry.0) {
            Ok(index) => f.write_str(names::MOTION[index].1),
            Err(_) => write!(f, "{:#x}", self.0),
        }
    }
}

struct StageName(u16);

impl fmt::Display for StageName {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match names::STAGE
            .get(usize::from(self.0))
            .filter(|name| !name.is_empty())
        {
            Some(name) => f.write_str(name),
            None => write!(f, "{:#x}", self.0),
        }
    }
}

pub fn fighter_name(kind: u32) -> &'static str {
    names::FIGHTER
        .get(kind as usize)
        .copied()
        .filter(|name| !name.is_empty())
        .unwrap_or("?")
}

/// Status, motion and frames ago, then the contact when `with_hit`; dashes
/// for a status the fighter has not had yet.
fn write_action(
    out: &mut impl fmt::Write,
    snapshot: &Snapshot,
    action: Action,
    with_hit: bool,
) -> fmt::Result {
    write!(
        out,
        "\t{}\t{}",
        StatusName(snapshot.fighter, action.status),
        MotionName(action.motion)
    )?;
    if action.status < 0 {
        out.write_str("\t-")?;
        if with_hit {
            out.write_str("\t-")?;
        }
        return Ok(());
    }
    write!(out, "\t{}", snapshot.ago(action))?;
    if with_hit {
        write!(out, "\t{}", action.contact.as_str())?;
    }
    Ok(())
}

/// One `judges.tsv` row (no newline).
pub fn write_row(
    out: &mut impl fmt::Write,
    boot: &str,
    session: u32,
    judge: &Judge,
) -> fmt::Result {
    let gw = &judge.gw;
    write!(
        out,
        "{boot}\t{session}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.1}",
        gw.frame,
        judge.drawn.number,
        u8::from(judge.drawn.number == 9),
        judge.drawn.excluded,
        judge.drawn.last_number,
        judge.hit.map_or("-", Contact::as_str),
        StageName(judge.stage),
        gw.entry,
        gw.place.as_str(),
        gw.damage,
    )?;
    write_action(out, gw, gw.cur, false)?;
    write_action(out, gw, gw.prev, true)?;
    write_action(out, gw, gw.last_move, true)?;
    write!(out, "\t{:.1}\t{:.1}", gw.x, gw.y)?;
    match &judge.opp {
        Some(opp) => {
            write!(
                out,
                "\t{}\t{}\t{}\t{:.1}",
                opp.entry,
                fighter_name(opp.fighter),
                opp.place.as_str(),
                opp.damage
            )?;
            write_action(out, opp, opp.cur, true)?;
            write_action(out, opp, opp.prev, true)?;
            write_action(out, opp, opp.last_move, true)?;
            let distance = ((opp.x - gw.x).powi(2) + (opp.y - gw.y).powi(2)).sqrt();
            write!(out, "\t{:.1}\t{:.1}\t{distance:.1}", opp.x, opp.y)?;
        }
        None => {
            // entry, fighter, place, damage, 3 actions x 4, x, y, distance
            for _ in 0..19 {
                out.write_str("\t-")?;
            }
        }
    }
    let state = |r: Rng| RngState(r.x, r.y, r.z, r.w);
    let trace = judge.rng;
    let predicted = predict(trace.at_draw, judge.drawn.excluded, trace.seven_block > 0);
    write!(
        out,
        "\t{predicted}\t{}\t{}\t{}\t{}\t{}\t{}",
        state(trace.at_draw),
        trace.at_draw.count,
        state(trace.before),
        trace.before.count,
        trace.count_after,
        trace.seven_block
    )
}

/// A stream state as `xxxxxxxx:yyyyyyyy:zzzzzzzz:wwwwwwww`.
struct RngState(u32, u32, u32, u32);

impl fmt::Display for RngState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{:08x}:{:08x}:{:08x}:{:08x}",
            self.0, self.1, self.2, self.3
        )
    }
}

/// A text line built in a fixed buffer; anything past `N` bytes is dropped.
pub struct Line<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> Line<N> {
    pub const fn new() -> Self {
        Self {
            buf: [0; N],
            len: 0,
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// The text plus a NUL, for `nn::fs` paths; `None` when there is no room.
    pub fn with_nul(&mut self) -> Option<&[u8]> {
        if self.len >= N {
            return None;
        }
        self.buf[self.len] = 0;
        Some(&self.buf[..=self.len])
    }
}

impl<const N: usize> Default for Line<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> fmt::Write for Line<N> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let take = s.len().min(N - self.len);
        self.buf[self.len..self.len + take].copy_from_slice(&s.as_bytes()[..take]);
        self.len += take;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::fmt::Write as _;

    const WAIT: i32 = 0x0;
    const DASH: i32 = 0x3;
    const ATTACK_AIR: i32 = 0x36;
    const LANDING_ATTACK_AIR: i32 = 0x18;
    const GW: u32 = FIGHTER_KIND_GAMEWATCH;
    const MARIO: u32 = 0;

    fn hash40(name: &str) -> u64 {
        // CRC-32 (IEEE) | length << 32, as the game hashes motion names.
        let mut crc = 0xffff_ffffu32;
        for byte in name.bytes() {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    crc >> 1 ^ 0xedb8_8320
                } else {
                    crc >> 1
                };
            }
        }
        u64::from(!crc) | (name.len() as u64) << 32
    }

    #[test]
    fn the_guard_starts_with_the_hooked_instruction() {
        // sub sp, sp, #0xc0
        assert_eq!(FIGHTER_FRAME.guard[..4], [0xff, 0x03, 0x03, 0xd1]);
    }

    #[test]
    fn names_resolve() {
        assert_eq!(fighter_name(GW), "gamewatch");
        assert_eq!(fighter_name(MARIO), "mario");
        assert_eq!(StageName(0x101).to_string(), "animal_city");
        assert_eq!(StageName(0x15b).to_string(), "battlefield_s");
        assert_eq!(StageName(0xffff).to_string(), "0xffff");
        assert_eq!(StatusName(GW, STATUS_SPECIAL_S).to_string(), "special_s");
        assert_eq!(StatusName(MARIO, ATTACK_AIR).to_string(), "attack_air");
        assert_eq!(StatusName(MARIO, 0x1f0).to_string(), "0x1f0");
        assert_eq!(StatusName(GW, -1).to_string(), "-");
        assert_eq!(
            MotionName(hash40("attack_air_f")).to_string(),
            "attack_air_f"
        );
        assert_eq!(
            MotionName(hash40("special_air_s")).to_string(),
            "special_air_s"
        );
        assert_eq!(MotionName(0x12_3456_789a).to_string(), "0x123456789a");
        assert!(names::MOTION.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn moves_are_classified() {
        assert!(is_move(MARIO, ATTACK_AIR));
        assert!(is_move(MARIO, STATUS_SPECIAL_S));
        // 0x1dc is also COMMON_NUM in lua_const; it must read as special_n.
        assert!(is_move(MARIO, 0x1dc));
        assert_eq!(StatusName(MARIO, 0x1dc).to_string(), "special_n");
        assert!(!is_move(MARIO, WAIT));
        assert!(!is_move(MARIO, LANDING_ATTACK_AIR));
        assert!(is_move(MARIO, 0x1f0));
        assert!(!is_move(MARIO, -1));
    }

    #[test]
    fn places() {
        assert_eq!(Place::new(SITUATION_GROUND, false), Place::Ground);
        assert_eq!(Place::new(SITUATION_GROUND, true), Place::Platform);
        assert_eq!(Place::new(SITUATION_AIR, true), Place::Air);
        assert_eq!(Place::new(SITUATION_CLIFF, false), Place::Ledge);
        assert_eq!(Place::new(-1, false), Place::Unknown);
    }

    const NONE: Contact = Contact::None;

    #[test]
    fn history_keeps_previous_status_and_last_move() {
        let mut h = History::new();
        h.begin(3);
        let nair = hash40("attack_air_n");
        h.observe(GW, WAIT, hash40("wait1"), NONE); // frame 0
        h.observe(GW, ATTACK_AIR, nair, NONE); // 1
        for frame in 2..=20 {
            let contact = if frame < 8 { NONE } else { Contact::Hit };
            h.observe(GW, ATTACK_AIR, nair, contact);
        }
        h.observe(GW, LANDING_ATTACK_AIR, hash40("landing_air_n"), NONE); // 21
        h.observe(GW, WAIT, hash40("wait1"), NONE); // 22
        assert!(h.observe(GW, STATUS_SPECIAL_S, hash40("special_s"), NONE)); // 23
        assert_eq!(h.special_s, 1);
        assert_eq!(h.cur.status, STATUS_SPECIAL_S);
        assert_eq!(h.prev.status, WAIT);
        assert_eq!(h.prev.contact, NONE);
        assert_eq!(h.last_move.status, ATTACK_AIR);
        assert_eq!(h.last_move.motion, nair);
        assert_eq!(h.last_move.contact, Contact::Hit);
        assert_eq!(h.ago(h.last_move), 22);
        assert_eq!(h.ago(h.prev), 1);
        assert_eq!(h.ago(h.cur), 0);
    }

    #[test]
    fn motion_and_contact_follow_the_status() {
        let mut h = History::new();
        h.observe(MARIO, 0x27, hash40("attack_11"), NONE);
        h.observe(MARIO, 0x27, hash40("attack_12"), Contact::Shield);
        h.observe(MARIO, 0x27, hash40("attack_12"), Contact::Hit);
        h.observe(MARIO, 0x27, hash40("attack_12"), Contact::Clank);
        h.observe(MARIO, WAIT, hash40("wait1"), NONE);
        assert_eq!(h.prev.motion, hash40("attack_12"));
        assert_eq!(h.prev.contact, Contact::Hit);
        assert_eq!(h.last_move.status, 0x27);
        assert_eq!(Contact::new(false, true, true), Contact::Shield);
        assert_eq!(Contact::new(true, true, false), Contact::Hit);
        assert_eq!(Contact::new(false, false, false), NONE);
    }

    #[test]
    fn a_panel_is_reported_once_per_draw() {
        let drawn = |number, excluded, last_number| {
            Some(Drawn {
                number,
                excluded,
                last_number,
            })
        };
        let mut h = History::new();
        assert_eq!(h.panel(-1), None); // baseline: no Judge yet this match
        assert_eq!(h.panel(-1), None);
        assert_eq!(h.panel(8), drawn(9, 0, 0));
        assert_eq!(h.panel(8), None); // the same Judge, later frames
        assert_eq!(h.panel(2), drawn(3, 9, 9));
        assert_eq!(h.panel(5), drawn(6, 3, 3));
        assert_eq!(h.panel(-1), None); // cleared on a KO
        assert_eq!(h.panel(5), drawn(6, 0, 6)); // a repeat is now possible
        assert_eq!(h.judges, 4);
        let mut late = History::new();
        assert_eq!(late.panel(4), None); // already showing when first seen
        assert_eq!(panel_number(8), 9);
        assert_eq!(panel_number(-1), 0);
    }

    #[test]
    fn the_judge_outcome_is_known_when_the_side_special_ends() {
        let mut h = History::new();
        h.observe(GW, WAIT, hash40("wait1"), NONE);
        h.observe(GW, STATUS_SPECIAL_S, hash40("special_s"), NONE);
        let drawn = h.cur;
        h.observe(GW, STATUS_SPECIAL_S, hash40("special_s"), NONE);
        assert_eq!(outcome(drawn, NONE, &h), Outcome::Running(NONE));
        h.observe(GW, STATUS_SPECIAL_S, hash40("special_s"), Contact::Hit);
        assert_eq!(outcome(drawn, NONE, &h), Outcome::Running(Contact::Hit));
        h.observe(GW, WAIT, hash40("wait1"), NONE);
        assert_eq!(
            outcome(drawn, Contact::Hit, &h),
            Outcome::Done(Contact::Hit)
        );
        h.observe(GW, 0x0b, hash40("jump_f"), NONE);
        assert_eq!(
            outcome(drawn, Contact::Shield, &h),
            Outcome::Done(Contact::Shield)
        );
    }

    fn sample() -> Judge {
        let action = |status: i32, motion: &str, start: u32, contact: Contact| Action {
            status,
            motion: hash40(motion),
            start,
            contact,
        };
        Judge {
            drawn: Drawn {
                number: 9,
                excluded: 4,
                last_number: 4,
            },
            stage: 0x15b,
            hit: Some(Contact::Hit),
            gw: Snapshot {
                session: 2,
                frame: 600,
                entry: 0,
                fighter: GW,
                place: Place::Platform,
                damage: 37.5,
                x: -10.0,
                y: 35.25,
                cur: action(STATUS_SPECIAL_S, "special_s", 598, NONE),
                prev: action(DASH, "dash", 590, NONE),
                last_move: action(ATTACK_AIR, "attack_air_f", 540, Contact::Shield),
            },
            opp: Some(Snapshot {
                session: 2,
                frame: 600,
                entry: 1,
                fighter: MARIO,
                place: Place::Air,
                damage: 82.0,
                x: 20.0,
                y: 75.25,
                cur: action(ATTACK_AIR, "attack_air_b", 595, NONE),
                prev: action(0x0c, "jump_aerial_f", 585, NONE),
                last_move: action(0x27, "attack_13", 500, Contact::Hit),
            }),
            rng: RngTrace {
                at_draw: Rng {
                    x: 1,
                    y: 2,
                    z: 3,
                    w: 4,
                    count: 7,
                },
                before: Rng {
                    x: 5,
                    y: 6,
                    z: 7,
                    w: 8,
                    count: 6,
                },
                count_after: 9,
                seven_block: 0,
            },
        }
    }

    #[test]
    fn rng_steps_like_sv_math_rand() {
        let mut rng = Rng {
            x: 1,
            y: 2,
            z: 3,
            w: 4,
            count: 0,
        };
        assert_eq!(rng.step(), 0x80d);
        assert_eq!((rng.x, rng.y, rng.z, rng.w, rng.count), (2, 3, 4, 0x80d, 1));
        assert_eq!(rng.rand(9), 0x181f % 9);
        assert_eq!(rng.count, 2);
    }

    #[test]
    fn judge_numbers_skip_the_excluded_and_blocked_7() {
        assert_eq!(judge_number(8, 0, false), 9);
        assert_eq!(judge_number(8, 9, false), 1);
        assert_eq!(judge_number(6, 3, true), 9);
        assert_eq!(judge_number(5, 7, false), 6);
    }

    /// Eden, 2026-10-29 (run g20): the fighter stream at the start of the
    /// draw frame, the excluded number, and the Judge the game showed.
    #[test]
    fn eden_judges_are_predicted() {
        let observed = [
            ([0x7bc45005, 0xcfc5782f, 0x04de0281, 0x44b7f967], 0, 2),
            ([0xd46a25da, 0x9fbd67d0, 0xd065480c, 0x91b5c6dd], 0, 2),
            ([0x3df1294f, 0xb2881d98, 0x0f0fdca8, 0x686453a7], 2, 7),
            ([0x84be6088, 0xc1048bbe, 0x07a5d20d, 0x99d280f6], 0, 7),
            ([0xd281f80b, 0x78060ccd, 0xe84e9cca, 0xa066aef2], 7, 6),
        ];
        for ([x, y, z, w], excluded, number) in observed {
            let rng = Rng {
                x,
                y,
                z,
                w,
                count: 0,
            };
            assert_eq!(predict(rng, excluded, false), number);
        }
    }

    #[test]
    fn segments_round_trip_and_fill_their_columns() {
        let judge = sample();
        let segment = Segment {
            session: 2,
            start: 590,
            frames: 41,
            predicted: 9,
            excluded: 4,
            seven_blocked: true,
            end_draws: 3,
            stage: 0x15b,
            gw: judge.gw,
            opp: judge.opp,
        };
        assert_eq!(Segment::from_words(&segment.to_words()), segment);
        let columns = SEGMENT_HEADER.split('\t').count();
        for segment in [
            segment,
            Segment {
                opp: None,
                ..segment
            },
        ] {
            let mut line = Line::<1024>::new();
            write_segment(&mut line, "b", &segment).unwrap();
            let row = core::str::from_utf8(line.as_bytes()).unwrap();
            assert_eq!(row.split('\t').count(), columns, "{row}");
            assert!(
                row.starts_with("b\t2\t590\t41\t9\t1\t4\t1\t3\tbattlefield_s\t"),
                "{row}"
            );
        }
    }

    #[test]
    fn records_round_trip() {
        let judge = sample();
        assert_eq!(Judge::from_words(&judge.to_words()), judge);
        let alone = Judge {
            opp: None,
            hit: None,
            ..judge
        };
        assert_eq!(Judge::from_words(&alone.to_words()), alone);
        let none = Action::NONE;
        assert_eq!(Action::from_words(none.words()), none);
    }

    #[test]
    fn a_row_has_one_value_per_column() {
        let judge = sample();
        let mut line = Line::<1024>::new();
        write_row(&mut line, "20261029-201500", 2, &judge).unwrap();
        let row = core::str::from_utf8(line.as_bytes()).unwrap();
        let columns = HEADER.split('\t').count();
        assert_eq!(row.split('\t').count(), columns, "{row}");
        assert_eq!(
            row,
            "20261029-201500\t2\t600\t9\t1\t4\t4\thit\tbattlefield_s\t0\tplatform\t37.5\
             \tspecial_s\tspecial_s\t2\tdash\tdash\t10\tnone\
             \tattack_air\tattack_air_f\t60\tshield\t-10.0\t35.2\
             \t1\tmario\tair\t82.0\tattack_air\tattack_air_b\t5\tnone\
             \tjump_aerial\tjump_aerial_f\t15\tnone\tattack\tattack_13\t100\thit\
             \t20.0\t75.2\t50.0\t7\t00000001:00000002:00000003:00000004\t7\t00000005:00000006:00000007:00000008\t6\t9\t0"
        );
        line.clear();
        let alone = Judge {
            opp: None,
            hit: None,
            ..judge
        };
        write_row(&mut line, "b", 1, &alone).unwrap();
        let row = core::str::from_utf8(line.as_bytes()).unwrap();
        assert_eq!(row.split('\t').count(), columns, "{row}");
        assert!(
            row.starts_with("b\t1\t600\t9\t1\t4\t4\t-\tbattlefield_s\t"),
            "{row}"
        );
    }

    #[test]
    fn lines_are_fixed() {
        let mut short = Line::<8>::new();
        write!(short, "0123456789").unwrap();
        assert_eq!(short.as_bytes(), b"01234567");
        assert_eq!(short.with_nul(), None);
    }
}
