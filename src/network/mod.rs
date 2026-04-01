//! Network simulation for distributed systems testing.

mod latency;
mod message;

pub use latency::LatencyModel;
pub use message::{Message, MessageId};
