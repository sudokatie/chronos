//! Task scheduling with pluggable strategies.

mod core;
mod random;
mod strategy;

pub use core::Scheduler;
pub use random::RandomStrategy;
pub use strategy::{FifoStrategy, ScheduleStrategy, Strategy};
