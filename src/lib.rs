//! Chronos - Deterministic simulation testing for distributed systems.
//!
//! Chronos intercepts time, randomness, and I/O to enable reproducible test
//! execution, fault injection, and schedule exploration. Find bugs in your
//! distributed systems before they find you.
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use chronos::prelude::*;
//!
//! #[chronos::test]
//! async fn test_distributed_counter() {
//!     let cluster = Cluster::new(3);
//!     
//!     // Advance simulated time
//!     cluster.advance_time(Duration::from_secs(1)).await;
//!     
//!     // Inject faults
//!     cluster.partition(&[&[0, 1], &[2]]);
//!     
//!     // Assert invariants
//!     chronos::assert!(cluster.is_stable(), "cluster should stabilize");
//! }
//! ```

pub mod cli;
pub mod cluster;
pub mod config;
pub mod detection;
pub mod error;
pub mod network;
pub mod prelude;
pub mod recording;
pub mod runtime;
pub mod scheduler;
pub mod sim;
pub mod time;

// Re-export the test macro from chronos-macros (when available)
#[cfg(feature = "macros")]
pub use chronos_macros::test;

pub use error::Error;

// ============================================================================
// Top-level API Re-exports (convenience aliases matching spec)
// ============================================================================

/// Network APIs - re-exported from sim::net for convenience.
/// Use `chronos::net::*` for simulated network operations.
pub mod net {
    pub use crate::sim::net::{
        Endpoint, Connection, partition, heal, can_communicate
    };
}

/// Filesystem APIs - re-exported from sim::fs for convenience.
/// Use `chronos::fs::*` for simulated filesystem operations.
pub mod fs {
    pub use crate::sim::fs::{
        read, write, exists, remove, list, reset,
        set_read_failure_rate, set_write_failure_rate
    };
}

/// Unique identifier for a simulated task.
pub type TaskId = u32;

/// Unique identifier for a simulated node.
pub type NodeId = u32;

/// Unique identifier for a message.
pub type MessageId = u64;

/// Unique identifier for an event (for happens-before tracking).
pub type EventId = u64;

/// Result type using Chronos error.
pub type Result<T> = std::result::Result<T, Error>;

// ============================================================================
// Top-level APIs
// ============================================================================

/// Spawn a task in the current simulation context.
///
/// The task will be scheduled by the simulation runtime and executed
/// deterministically based on the scheduling strategy.
///
/// # Panics
/// Panics if called outside a simulation context.
pub fn spawn<F>(future: F) -> runtime::TaskHandle
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    sim::spawn_task(future)
}

/// Get the current simulated time.
///
/// Shorthand for `sim::time::now()`.
pub fn now() -> time::Instant {
    sim::time::now()
}

/// Sleep for the given duration of simulated time.
///
/// Shorthand for `sim::time::sleep()`.
pub fn sleep(duration: std::time::Duration) -> sim::time::Sleep {
    sim::time::sleep(duration)
}

/// Yield control to the scheduler.
///
/// Shorthand for `sim::time::yield_now()`.
pub async fn yield_now() {
    sim::time::yield_now().await
}

/// Generate a random value.
///
/// Shorthand for `sim::random::gen()`.
pub fn random<T>() -> T
where
    rand::distributions::Standard: rand::distributions::Distribution<T>,
{
    sim::random::gen()
}

/// Check if running inside a simulation.
pub fn is_simulation() -> bool {
    sim::is_simulation()
}

// ============================================================================
// Assertion Macros and Functions
// ============================================================================

/// Assertion functions for simulation tests.
pub mod assertions {
    use std::panic::Location;
    use std::time::Duration;

    /// Assert a condition in a simulation test.
    ///
    /// Unlike `std::assert!`, this records the assertion in the execution trace
    /// and provides better error messages for simulation debugging.
    #[track_caller]
    pub fn assert(condition: bool, message: &str) {
        if !condition {
            let location = Location::caller();
            panic!(
                "chronos assertion failed at {}:{}: {}",
                location.file(),
                location.line(),
                message
            );
        }
    }

    /// Assert that a condition will eventually become true.
    ///
    /// This registers the assertion with the simulation runtime to be checked
    /// continuously. If the condition doesn't become true before the deadline,
    /// the test fails.
    #[track_caller]
    pub fn assert_eventually<F>(condition: F, message: &str)
    where
        F: Fn() -> bool + Send + Sync + 'static,
    {
        // First check - maybe it's already true
        if condition() {
            return;
        }

        // Register with runtime for continuous checking
        if crate::sim::is_simulation() {
            crate::sim::assertions::register_eventually(
                condition,
                message,
                Some(Duration::from_secs(60)), // Default 60s timeout
            );
        } else {
            // Not in simulation - just check once
            let location = Location::caller();
            panic!(
                "chronos assert_eventually failed at {}:{}: {}",
                location.file(),
                location.line(),
                message
            );
        }
    }

    /// Assert that a condition will eventually become true within a timeout.
    #[track_caller]
    pub fn assert_eventually_within<F>(condition: F, message: &str, timeout: Duration)
    where
        F: Fn() -> bool + Send + Sync + 'static,
    {
        if condition() {
            return;
        }

        if crate::sim::is_simulation() {
            crate::sim::assertions::register_eventually(condition, message, Some(timeout));
        } else {
            let location = Location::caller();
            panic!(
                "chronos assert_eventually failed at {}:{}: {}",
                location.file(),
                location.line(),
                message
            );
        }
    }

    /// Assert that a condition remains true throughout the simulation.
    ///
    /// This registers the assertion with the simulation runtime to be checked
    /// at every step. If the condition ever becomes false, the test fails.
    #[track_caller]
    pub fn assert_always<F>(condition: F, message: &str)
    where
        F: Fn() -> bool + Send + Sync + 'static,
    {
        // First check
        if !condition() {
            let location = Location::caller();
            panic!(
                "chronos assert_always failed at {}:{}: {}",
                location.file(),
                location.line(),
                message
            );
        }

        // Register for continuous checking
        if crate::sim::is_simulation() {
            crate::sim::assertions::register_always(condition, message);
        }
    }

    /// Verify all eventually-assertions have been satisfied.
    ///
    /// Call this at the end of a test to ensure all `assert_eventually`
    /// conditions were met.
    pub fn verify_all() -> std::result::Result<(), String> {
        if crate::sim::is_simulation() {
            crate::sim::assertions::verify_eventually()
        } else {
            Ok(())
        }
    }
}

// Re-export assertion functions at crate root for convenience
pub use assertions::{assert, assert_always, assert_eventually};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_assert_pass() {
        assertions::assert(true, "should pass");
    }

    #[test]
    #[should_panic(expected = "chronos assertion failed")]
    fn test_assert_fail() {
        assertions::assert(false, "should fail");
    }

    #[test]
    fn test_assert_eventually_immediate() {
        // Should pass immediately if condition is true
        assertions::assert_eventually(|| true, "should be true");
    }

    #[test]
    fn test_assert_always_pass() {
        // Outside simulation, just checks once
        assertions::assert_always(|| true, "should always be true");
    }

    #[test]
    fn test_is_simulation() {
        // Should be false outside sim context
        assert!(!is_simulation());
        
        // Install context
        let ctx = sim::SimContext::new(42);
        ctx.install();
        assert!(is_simulation());
        sim::SimContext::uninstall();
        
        assert!(!is_simulation());
    }

    #[test]
    fn test_top_level_now() {
        let ctx = sim::SimContext::new(42);
        ctx.install();
        
        let t = now();
        assert_eq!(t, time::Instant::from_nanos(0));
        
        sim::SimContext::uninstall();
    }

    #[test]
    fn test_spawn_in_context() {
        let ctx = sim::SimContext::new(42);
        ctx.install();
        
        let handle = spawn(async {});
        assert!(!handle.is_complete()); // Task spawned but not yet run
        
        sim::SimContext::uninstall();
    }
}
