//! Network simulation for distributed systems testing.

mod fault;
mod latency;
mod link;
mod message;

pub use fault::{Fault, FaultSchedule, FaultState};
pub use latency::LatencyModel;
pub use link::Link;
pub use message::{Message, MessageId};
