//! Error types for Chronos simulation framework.

use crate::{TaskId, NodeId};
use thiserror::Error;

/// Errors that can occur during simulation.
#[derive(Debug, Error)]
pub enum Error {
    /// A deadlock was detected in the simulation.
    #[error("deadlock detected: cycle involves tasks {}", format_task_ids(.cycle))]
    Deadlock {
        /// Tasks involved in the deadlock cycle.
        cycle: Vec<TaskId>,
    },

    /// A livelock was detected (tasks running but making no progress).
    #[error("livelock detected: {} tasks stuck without progress", stuck_tasks.len())]
    Livelock {
        /// Tasks that are stuck in a livelock.
        stuck_tasks: Vec<TaskId>,
    },

    /// An assertion failed during simulation.
    #[error("assertion failed at {location}: {message}")]
    AssertionFailed {
        /// Description of the failed assertion.
        message: String,
        /// Source location of the assertion.
        location: String,
    },

    /// Simulation exceeded the time limit.
    #[error("timeout after {simulated_nanos}ns of simulated time")]
    Timeout {
        /// Simulated time in nanoseconds when timeout occurred.
        simulated_nanos: u64,
    },

    /// Replay diverged from the recorded execution.
    #[error("replay mismatch: expected {expected}, got {got}")]
    ReplayMismatch {
        /// What the recording expected.
        expected: String,
        /// What actually happened.
        got: String,
    },

    /// Recording file is invalid or corrupted.
    #[error("invalid recording: {reason}")]
    InvalidRecording {
        /// Why the recording is invalid.
        reason: String,
    },

    /// Configuration error.
    #[error("config error: {message}")]
    ConfigError {
        /// Description of the configuration error.
        message: String,
    },

    /// IO error.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// Node not found.
    #[error("node {0} not found")]
    NodeNotFound(NodeId),

    /// Task not found.
    #[error("task {0} not found")]
    TaskNotFound(TaskId),
}

fn format_task_ids(ids: &[TaskId]) -> String {
    ids.iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(" -> ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deadlock_display() {
        let err = Error::Deadlock {
            cycle: vec![1, 2, 3, 1],
        };
        assert!(err.to_string().contains("1 -> 2 -> 3 -> 1"));
    }

    #[test]
    fn test_livelock_display() {
        let err = Error::Livelock {
            stuck_tasks: vec![1, 2],
        };
        assert!(err.to_string().contains("2 tasks stuck"));
    }

    #[test]
    fn test_assertion_failed_display() {
        let err = Error::AssertionFailed {
            message: "x should be 5".to_string(),
            location: "test.rs:42".to_string(),
        };
        assert!(err.to_string().contains("test.rs:42"));
        assert!(err.to_string().contains("x should be 5"));
    }

    #[test]
    fn test_timeout_display() {
        let err = Error::Timeout {
            simulated_nanos: 1_000_000_000,
        };
        assert!(err.to_string().contains("1000000000ns"));
    }

    #[test]
    fn test_replay_mismatch_display() {
        let err = Error::ReplayMismatch {
            expected: "task 1 runs".to_string(),
            got: "task 2 runs".to_string(),
        };
        assert!(err.to_string().contains("task 1 runs"));
        assert!(err.to_string().contains("task 2 runs"));
    }

    #[test]
    fn test_config_error_display() {
        let err = Error::ConfigError {
            message: "invalid timeout".to_string(),
        };
        assert!(err.to_string().contains("invalid timeout"));
    }
}
