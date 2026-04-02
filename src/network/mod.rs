//! Network simulation for distributed systems testing.

mod latency;
mod link;
mod message;

pub use latency::LatencyModel;
pub use link::Link;
pub use message::{Message, MessageId};
