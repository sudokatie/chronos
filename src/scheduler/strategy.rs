//! Scheduling strategy trait and configuration.

use crate::TaskId;

/// Configuration for scheduling strategy.
#[derive(Clone, Debug)]
#[derive(Default)]
pub enum Strategy {
    /// Random selection with given seed for reproducibility.
    Random { seed: u64 },
    /// First-in-first-out (deterministic baseline).
    #[default]
    Fifo,
    /// Depth-first exploration with bounded stack.
    DepthFirst { max_depth: usize },
    /// Probabilistic Concurrency Testing.
    PCT { seed: u64, bug_depth: usize },
    /// Bound the number of context switches.
    ContextBound { max_preemptions: usize, seed: u64 },
}


/// Trait for scheduling strategy implementations.
pub trait ScheduleStrategy: Send {
    /// Select which task to run from the ready set.
    fn select(&mut self, ready: &[TaskId]) -> TaskId;

    /// Called when a task yields voluntarily.
    fn on_yield(&mut self, task: TaskId);

    /// Called after a scheduling decision is made.
    fn on_decision(&mut self, chosen: TaskId, ready: &[TaskId]);

    /// Reset state for a new simulation run.
    fn reset(&mut self);
}

/// FIFO strategy - always picks the first ready task.
#[derive(Debug, Default)]
pub struct FifoStrategy;

impl FifoStrategy {
    pub fn new() -> Self {
        Self
    }
}

impl ScheduleStrategy for FifoStrategy {
    fn select(&mut self, ready: &[TaskId]) -> TaskId {
        ready[0]
    }

    fn on_yield(&mut self, _task: TaskId) {}

    fn on_decision(&mut self, _chosen: TaskId, _ready: &[TaskId]) {}

    fn reset(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strategy_default() {
        let s = Strategy::default();
        assert!(matches!(s, Strategy::Fifo));
    }

    #[test]
    fn test_fifo_strategy_select() {
        let mut s = FifoStrategy::new();
        assert_eq!(s.select(&[1, 2, 3]), 1);
        assert_eq!(s.select(&[5, 4, 3]), 5);
    }

    #[test]
    fn test_fifo_strategy_callbacks() {
        let mut s = FifoStrategy::new();
        s.on_yield(1);
        s.on_decision(1, &[1, 2]);
        s.reset();
        // Just verify no panics
    }
}
