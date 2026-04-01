//! Task scheduling with pluggable strategies.

mod core;
mod pct;
mod random;
mod strategy;

pub use core::Scheduler;
pub use pct::PCTStrategy;
pub use random::RandomStrategy;
pub use strategy::{FifoStrategy, ScheduleStrategy, Strategy};
