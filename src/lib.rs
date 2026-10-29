//! ssbu-gnw-judge-log: logs every Game & Watch Judge (side special) number in
//! Smash Ultimate 13.0.5 with both players' context, for statistics. Read-only:
//! one hook reads each fighter's state through the game's own getters; nothing
//! is drawn from any RNG and no game state changes.
pub mod model;
pub mod names;
#[cfg(target_os = "switch")]
mod runtime;
