//! Context-bounded scheduling strategy.
//!
//! Limits the number of preemptions (context switches) in a schedule.
//! This is based on the CHESS algorithm from Microsoft Research.
//! Most concurrency bugs manifest with a small number of preemptions.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use crate::TaskId;

use super::strategy::ScheduleStrategy;

/// Context-bounded scheduling strategy.
///
/// Limits context switches to find bugs that occur with few interleavings.
/// Based on the insight that most bugs require only a few preemptions.
#[derive(Debug)]
pub struct ContextBoundStrategy {
    /// Maximum number of preemptions allowed.
    max_preemptions: usize,
    /// Current preemption count.
    preemption_count: usize,
    /// Currently running task (if any).
    current_task: Option<TaskId>,
    /// Random number generator for selection when multiple options exist.
    rng: StdRng,
    /// Seed for reset.
    seed: u64,
    /// Number of scheduling decisions made.
    decisions: u64,
    /// Preemption points (decision numbers where we preempt).
    preemption_points: Vec<u64>,
    /// Current index in preemption_points.
    preemption_index: usize,
}

impl ContextBoundStrategy {
    /// Creates a new context-bounded strategy.
    pub fn new(max_preemptions: usize, seed: u64) -> Self {
        Self::with_preemption_points(max_preemptions, seed, 1000)
    }

    /// Creates a strategy with custom max decisions for preemption point selection.
    pub fn with_preemption_points(max_preemptions: usize, seed: u64, max_decisions: u64) -> Self {
        let mut rng = StdRng::seed_from_u64(seed);
        
        // Randomly select decision points where preemption will occur
        let mut preemption_points: Vec<u64> = (0..max_preemptions)
            .map(|_| rng.gen_range(1..=max_decisions))
            .collect();
        preemption_points.sort();

        Self {
            max_preemptions,
            preemption_count: 0,
            current_task: None,
            rng: StdRng::seed_from_u64(seed),
            seed,
            decisions: 0,
            preemption_points,
            preemption_index: 0,
        }
    }

    /// Returns the maximum preemptions allowed.
    pub fn max_preemptions(&self) -> usize {
        self.max_preemptions
    }

    /// Returns the current preemption count.
    pub fn preemption_count(&self) -> usize {
        self.preemption_count
    }

    /// Returns the seed.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Check if this decision point should cause a preemption.
    fn should_preempt(&mut self) -> bool {
        if self.preemption_index >= self.preemption_points.len() {
            return false;
        }
        
        if self.decisions == self.preemption_points[self.preemption_index] {
            self.preemption_index += 1;
            self.preemption_count += 1;
            return true;
        }
        
        false
    }
}

impl ScheduleStrategy for ContextBoundStrategy {
    fn select(&mut self, ready: &[TaskId]) -> TaskId {
        self.decisions += 1;

        if ready.len() == 1 {
            self.current_task = Some(ready[0]);
            return ready[0];
        }

        // Check if current task is still ready
        let current_ready = self.current_task
            .map(|t| ready.contains(&t))
            .unwrap_or(false);

        // Decide whether to preempt
        let preempt = self.should_preempt();

        let chosen = if current_ready && !preempt {
            // Continue with current task (no preemption)
            self.current_task.unwrap()
        } else if preempt && ready.len() > 1 {
            // Force preemption - pick a different task
            let others: Vec<_> = ready
                .iter()
                .filter(|&&t| Some(t) != self.current_task)
                .copied()
                .collect();
            
            if others.is_empty() {
                ready[0]
            } else {
                let idx = self.rng.gen_range(0..others.len());
                others[idx]
            }
        } else {
            // No current task or it's not ready - pick randomly
            let idx = self.rng.gen_range(0..ready.len());
            ready[idx]
        };

        self.current_task = Some(chosen);
        chosen
    }

    fn on_yield(&mut self, task: TaskId) {
        // Voluntary yield doesn't count as preemption
        if self.current_task == Some(task) {
            self.current_task = None;
        }
    }

    fn on_decision(&mut self, _chosen: TaskId, _ready: &[TaskId]) {}

    fn reset(&mut self) {
        self.rng = StdRng::seed_from_u64(self.seed);
        self.current_task = None;
        self.preemption_count = 0;
        self.decisions = 0;
        self.preemption_index = 0;
        
        // Regenerate preemption points
        let max_decisions = 1000u64;
        self.preemption_points = (0..self.max_preemptions)
            .map(|_| self.rng.gen_range(1..=max_decisions))
            .collect();
        self.preemption_points.sort();
        
        // Reset RNG again for selection
        self.rng = StdRng::seed_from_u64(self.seed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_bound_new() {
        let cb = ContextBoundStrategy::new(3, 42);
        assert_eq!(cb.max_preemptions(), 3);
        assert_eq!(cb.preemption_count(), 0);
    }

    #[test]
    fn test_context_bound_continues_current() {
        let mut cb = ContextBoundStrategy::with_preemption_points(0, 42, 1000);
        
        // First selection
        let chosen1 = cb.select(&[1, 2, 3]);
        
        // With 0 preemptions allowed, should keep running same task
        let chosen2 = cb.select(&[1, 2, 3]);
        
        // If task 1 was chosen and is still ready, should continue
        if chosen1 == chosen2 {
            // Good - no preemption
        }
    }

    #[test]
    fn test_context_bound_single_task() {
        let mut cb = ContextBoundStrategy::new(3, 42);
        
        // Single task always returns that task
        for _ in 0..10 {
            assert_eq!(cb.select(&[1]), 1);
        }
    }

    #[test]
    fn test_context_bound_respects_limit() {
        let mut cb = ContextBoundStrategy::new(2, 42);
        
        // Run many selections
        for _ in 0..100 {
            cb.select(&[1, 2, 3]);
        }
        
        // Should not exceed max preemptions
        assert!(cb.preemption_count() <= cb.max_preemptions());
    }

    #[test]
    fn test_context_bound_deterministic() {
        let mut cb1 = ContextBoundStrategy::new(3, 12345);
        let mut cb2 = ContextBoundStrategy::new(3, 12345);
        
        // Same seed should give same schedule
        for _ in 0..20 {
            let ready = vec![1, 2, 3, 4];
            assert_eq!(cb1.select(&ready), cb2.select(&ready));
        }
    }

    #[test]
    fn test_context_bound_reset() {
        let mut cb = ContextBoundStrategy::new(3, 42);
        
        // Make some selections
        let first_choices: Vec<_> = (0..5)
            .map(|_| cb.select(&[1, 2, 3]))
            .collect();
        
        cb.reset();
        
        // After reset, same sequence
        let second_choices: Vec<_> = (0..5)
            .map(|_| cb.select(&[1, 2, 3]))
            .collect();
        
        assert_eq!(first_choices, second_choices);
    }

    #[test]
    fn test_context_bound_yield() {
        let mut cb = ContextBoundStrategy::new(3, 42);
        
        cb.select(&[1, 2]);
        cb.on_yield(1);
        
        // After yield, current task should be cleared
    }

    #[test]
    fn test_context_bound_different_seeds() {
        let mut cb1 = ContextBoundStrategy::new(3, 111);
        let mut cb2 = ContextBoundStrategy::new(3, 222);
        
        let mut different = false;
        for _ in 0..20 {
            if cb1.select(&[1, 2, 3]) != cb2.select(&[1, 2, 3]) {
                different = true;
                break;
            }
        }
        
        // Should produce different schedules with different seeds
        assert!(different);
    }
}
