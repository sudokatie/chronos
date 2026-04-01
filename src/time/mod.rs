//! Virtual time for deterministic simulation.

mod clock;
mod instant;
mod timer;

pub use clock::Clock;
pub use instant::Instant;
pub use timer::{TimerId, TimerWheel};
