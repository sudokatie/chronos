//! Random scheduling strategy for exploring interleavings.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use crate::TaskId;

use super::strategy::ScheduleStrategy;

/// Random scheduling strategy.
///
/// Randomly selects from ready tasks, using a seeded RNG for reproducibility.
/// Same seed always produces the same sequence of choices.
#[derive(Debug)]
pub struct RandomStrategy {
    rng: StdRng,
    seed: u64,
}

impl RandomStrategy {
    /// Creates a new random strategy with the given seed.
    pub fn new(seed: u64) -> Self {
        Self {
            rng: StdRng::seed_from_u64(seed),
            seed,
        }
    }

    /// Returns the seed used for this strategy.
    pub fn seed(&self) -> u64 {
        self.seed
    }
}

impl ScheduleStrategy for RandomStrategy {
    fn select(&mut self, ready: &[TaskId]) -> TaskId {
        if ready.len() == 1 {
            return ready[0];
        }
        let idx = self.rng.gen_range(0..ready.len());
        ready[idx]
    }

    fn on_yield(&mut self, _task: TaskId) {}

    fn on_decision(&mut self, _chosen: TaskId, _ready: &[TaskId]) {}

    fn reset(&mut self) {
        self.rng = StdRng::seed_from_u64(self.seed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_same_seed_same_sequence() {
        let ready: Vec<TaskId> = vec![1, 2, 3, 4, 5];
        
        let mut s1 = RandomStrategy::new(42);
        let mut s2 = RandomStrategy::new(42);
        
        for _ in 0..10 {
            assert_eq!(s1.select(&ready), s2.select(&ready));
        }
    }

    #[test]
    fn test_different_seeds_different_sequences() {
        let ready: Vec<TaskId> = vec![1, 2, 3, 4, 5];
        
        let mut s1 = RandomStrategy::new(42);
        let mut s2 = RandomStrategy::new(123);
        
        let seq1: Vec<_> = (0..10).map(|_| s1.select(&ready)).collect();
        let seq2: Vec<_> = (0..10).map(|_| s2.select(&ready)).collect();
        
        // Very unlikely to be equal with different seeds
        assert_ne!(seq1, seq2);
    }

    #[test]
    fn test_single_element() {
        let mut s = RandomStrategy::new(42);
        let ready = vec![99];
        
        for _ in 0..10 {
            assert_eq!(s.select(&ready), 99);
        }
    }

    #[test]
    fn test_distribution_roughly_uniform() {
        let mut s = RandomStrategy::new(12345);
        let ready: Vec<TaskId> = vec![0, 1, 2, 3];
        let mut counts: HashMap<TaskId, usize> = HashMap::new();
        
        let iterations = 10000;
        for _ in 0..iterations {
            let chosen = s.select(&ready);
            *counts.entry(chosen).or_insert(0) += 1;
        }
        
        // Each should be roughly 25% (2500), allow 20% deviation
        let expected = iterations / 4;
        let tolerance = expected / 5; // 20%
        
        for &task in &ready {
            let count = counts.get(&task).copied().unwrap_or(0);
            assert!(
                count > expected - tolerance && count < expected + tolerance,
                "Task {} had count {}, expected ~{}", task, count, expected
            );
        }
    }

    #[test]
    fn test_reset_restarts_sequence() {
        let ready: Vec<TaskId> = vec![1, 2, 3, 4, 5];
        let mut s = RandomStrategy::new(42);
        
        let first_run: Vec<_> = (0..5).map(|_| s.select(&ready)).collect();
        
        s.reset();
        
        let second_run: Vec<_> = (0..5).map(|_| s.select(&ready)).collect();
        
        assert_eq!(first_run, second_run);
    }

    #[test]
    fn test_seed_accessor() {
        let s = RandomStrategy::new(999);
        assert_eq!(s.seed(), 999);
    }
}
