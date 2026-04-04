//! Bug detection utilities.
//!
//! Provides detection for:
//! - Deadlocks (circular wait dependencies)
//! - Livelocks (no progress being made)
//! - Data races (concurrent conflicting memory accesses)

mod deadlock;
mod livelock;
mod race;

pub use deadlock::{DeadlockDetector, WaitGraph};
pub use livelock::{LivelockDetector, ProgressTracker};
pub use race::{RaceDetector, DataRace, MemoryAccess, AccessType};
