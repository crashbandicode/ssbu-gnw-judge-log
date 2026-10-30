//! Eden-only diagnostics (cargo feature `diag`; never in the shipped NRO).
//! Logs every `sv_math::rand(hash40("fighter"), n)` call while a Game & Watch
//! plays: the fighter-stream draw count before the call, `n` and the caller.
//! The first call asking for 7..=9 from outside main also dumps the code
//! around its caller (G&W's script), once, for disassembly.
use super::{fighter_rng, push_note, read_u32, GW_SESSION, NOTE_DIAG_DUMP, SESSION, TEXT_BASE};
use crate::model::Site;
use core::cell::UnsafeCell;
use skyline::hooks::InlineCtx;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// `app::sv_math::rand(phx::Hash40, int)` entry: x0 = hash, w1 = n, x30 = caller.
pub const SV_MATH_RAND_OFFSET: usize = 0x2275320;
pub const SV_MATH_RAND: Site = Site {
    name: "sv_math_rand",
    offset: SV_MATH_RAND_OFFSET,
    guard: [
        0x09, 0x55, 0x8b, 0xd2, 0xe9, 0x73, 0xbf, 0xf2, 0x08, 0x9c, 0x40, 0x92, 0xa9, 0x00, 0xc0,
        0xf2, 0x1f, 0x01, 0x09, 0xeb, 0x6c, 0x04, 0x00, 0x54, 0x09, 0x6d, 0x92, 0xd2, 0x89, 0x4f,
        0xb8, 0xf2, 0xa9, 0x00, 0xc0, 0xf2, 0x1f, 0x01, 0x09, 0xeb, 0x0c, 0x08, 0x00, 0x54, 0xa9,
        0x3c, 0x92, 0xd2, 0x29, 0x2a, 0xa1, 0xf2, 0x49, 0x00, 0xc0, 0xf2, 0x1f, 0x01, 0x09, 0xeb,
        0xa0, 0x0e, 0x00, 0x54,
    ],
};
const HASH40_FIGHTER: u64 = 0x7_7a08_c3fc;
pub const DUMP_BEFORE: u64 = 0xc00;
pub const DUMP_BYTES: usize = 0x1000;
const RING: usize = 512;

struct Draw {
    seq: AtomicU64,
    /// session | count << 32
    a: AtomicU64,
    /// n | caller << 16 (caller's low 48 bits)
    b: AtomicU64,
}
static DRAWS: [Draw; RING] = [const {
    Draw {
        seq: AtomicU64::new(0),
        a: AtomicU64::new(0),
        b: AtomicU64::new(0),
    }
}; RING];
static HEAD: AtomicU64 = AtomicU64::new(0);

struct Dump(UnsafeCell<[u8; DUMP_BYTES]>);
// SAFETY: written once by the hook before DUMP_READY, read by the writer after.
unsafe impl Sync for Dump {}
static DUMP: Dump = Dump(UnsafeCell::new([0; DUMP_BYTES]));
static DUMP_TAKEN: AtomicBool = AtomicBool::new(false);
pub static DUMP_READY: AtomicBool = AtomicBool::new(false);
pub static DUMP_START: AtomicU64 = AtomicU64::new(0);

fn in_main(address: u64) -> bool {
    let text = TEXT_BASE.load(Ordering::Relaxed);
    address >= text && address - text < 0x0600_0000
}

#[skyline::hook(offset = SV_MATH_RAND_OFFSET, inline)]
unsafe fn gnw_judge_diag_rand(ctx: &InlineCtx) {
    if ctx.registers[0].x() & 0xff_ffff_ffff != HASH40_FIGHTER {
        return;
    }
    let session = SESSION.load(Ordering::Acquire);
    if session == 0 || GW_SESSION.load(Ordering::Acquire) != session {
        return;
    }
    let n = ctx.registers[1].w();
    let caller = ctx.registers[30].x();
    let count = fighter_rng().map_or(u32::MAX, |rng| rng.count);
    let index = HEAD.fetch_add(1, Ordering::AcqRel);
    let slot = &DRAWS[(index % RING as u64) as usize];
    slot.a.store(
        u64::from(session) | u64::from(count) << 32,
        Ordering::Relaxed,
    );
    slot.b.store(
        u64::from(n & 0xffff) | (caller & 0xffff_ffff_ffff) << 16,
        Ordering::Relaxed,
    );
    slot.seq.store(index + 1, Ordering::Release);
    if (7..=9).contains(&n) && !in_main(caller) && !DUMP_TAKEN.swap(true, Ordering::AcqRel) {
        let start = (caller - DUMP_BEFORE) & !3;
        let dump = &mut *DUMP.0.get();
        for (i, word) in dump.chunks_exact_mut(4).enumerate() {
            word.copy_from_slice(&read_u32(start + 4 * i as u64).to_le_bytes());
        }
        DUMP_START.store(start, Ordering::Relaxed);
        DUMP_READY.store(true, Ordering::Release);
        push_note(NOTE_DIAG_DUMP, start, caller);
    }
}

pub fn install() -> bool {
    if !unsafe { super::guard_matches(&SV_MATH_RAND) } {
        return false;
    }
    skyline::install_hook!(gnw_judge_diag_rand);
    true
}

/// Rows for `draws.tsv` since `tail`: `session count n caller in_main`.
pub fn drain(tail: &mut u64, mut emit: impl FnMut(&[u8])) {
    use core::fmt::Write as _;
    let head = HEAD.load(Ordering::Acquire);
    if head.saturating_sub(*tail) > RING as u64 {
        *tail = head - RING as u64;
    }
    let mut line = crate::model::Line::<96>::new();
    while *tail < head {
        let slot = &DRAWS[(*tail % RING as u64) as usize];
        if slot.seq.load(Ordering::Acquire) != *tail + 1 {
            break;
        }
        let (a, b) = (
            slot.a.load(Ordering::Relaxed),
            slot.b.load(Ordering::Relaxed),
        );
        *tail += 1;
        let caller = b >> 16;
        let text = TEXT_BASE.load(Ordering::Relaxed);
        line.clear();
        let _ = if in_main(caller) {
            writeln!(
                line,
                "{}\t{}\t{}\tmain+{:#x}",
                a as u32,
                a >> 32,
                b & 0xffff,
                caller - text
            )
        } else {
            writeln!(
                line,
                "{}\t{}\t{}\t{caller:#x}",
                a as u32,
                a >> 32,
                b & 0xffff
            )
        };
        emit(line.as_bytes());
    }
}

/// The dumped code, once it is ready.
pub fn dump() -> Option<&'static [u8]> {
    DUMP_READY
        .load(Ordering::Acquire)
        .then(|| unsafe { &*DUMP.0.get() }.as_slice())
}
