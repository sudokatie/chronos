//! Task scheduling with pluggable strategies.

mod core;
mod strategy;

pub use core::Scheduler;
pub use strategy::{FifoStrategy, ScheduleStrategy, Strategy};
