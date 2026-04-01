//! Chronos - Deterministic simulation testing for distributed systems.
//!
//! Chronos intercepts time, randomness, and I/O to enable reproducible test
//! execution, fault injection, and schedule exploration. Find bugs in your
//! distributed systems before they find you.

pub mod error;
pub mod network;
pub mod prelude;
pub mod runtime;
pub mod scheduler;
pub mod time;

pub use error::Error;

/// Unique identifier for a simulated task.
pub type TaskId = u32;

/// Unique identifier for a simulated node.
pub type NodeId = u32;

/// Unique identifier for a message.
pub type MessageId = u64;

/// Result type using Chronos error.
pub type Result<T> = std::result::Result<T, Error>;
