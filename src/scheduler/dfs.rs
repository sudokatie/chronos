//! Depth-first search scheduling strategy.
//!
//! DFS systematically explores the schedule space by trying all possible
//! interleavings up to a maximum depth. This provides exhaustive coverage
//! for small state spaces.


use crate::TaskId;

use super::strategy::ScheduleStrategy;

/// DFS scheduling strategy.
///
/// Explores schedules depth-first, backtracking when reaching max depth
/// or when all possibilities at a level have been tried.
#[derive(Debug)]
pub struct DFSStrategy {
    /// Maximum exploration depth.
    max_depth: usize,
    /// Current depth in the exploration tree.
    current_depth: usize,
    /// Stack of decision points: (ready_set, index_to_try_next).
    decision_stack: Vec<(Vec<TaskId>, usize)>,
    /// Number of schedules explored.
    schedules_explored: u64,
    /// Whether we've completed exploration.
    exhausted: bool,
}

impl DFSStrategy {
    /// Creates a new DFS strategy with the given max depth.
    pub fn new(max_depth: usize) -> Self {
        Self {
            max_depth,
            current_depth: 0,
            decision_stack: Vec::new(),
            schedules_explored: 0,
            exhausted: false,
        }
    }

    /// Returns the number of schedules explored so far.
    pub fn schedules_explored(&self) -> u64 {
        self.schedules_explored
    }

    /// Returns true if exploration is exhausted.
    pub fn is_exhausted(&self) -> bool {
        self.exhausted
    }

    /// Returns the maximum depth.
    pub fn max_depth(&self) -> usize {
        self.max_depth
    }

    /// Backtrack to try the next alternative at a decision point.
    fn backtrack(&mut self) -> bool {
        while let Some((ready, idx)) = self.decision_stack.pop() {
            if idx + 1 < ready.len() {
                // There's another alternative to try at this level
                self.decision_stack.push((ready, idx + 1));
                return true;
            }
            // No more alternatives at this level, continue backtracking
        }
        // Exhausted all possibilities
        self.exhausted = true;
        false
    }

    /// Signal that the current schedule is complete (test finished).
    pub fn schedule_complete(&mut self) {
        self.schedules_explored += 1;
        self.backtrack();
    }
}

impl ScheduleStrategy for DFSStrategy {
    fn select(&mut self, ready: &[TaskId]) -> TaskId {
        if ready.len() == 1 {
            return ready[0];
        }

        self.current_depth += 1;

        // Check if we've hit max depth
        if self.current_depth > self.max_depth {
            // Just pick first and don't record
            return ready[0];
        }

        // Check if we have a pending decision for this level
        if self.current_depth <= self.decision_stack.len() {
            // Use existing decision
            let (_, idx) = &self.decision_stack[self.current_depth - 1];
            return ready[*idx % ready.len()];
        }

        // New decision point - start with first option
        self.decision_stack.push((ready.to_vec(), 0));
        ready[0]
    }

    fn on_yield(&mut self, _task: TaskId) {}

    fn on_decision(&mut self, _chosen: TaskId, _ready: &[TaskId]) {}

    fn reset(&mut self) {
        self.current_depth = 0;
        // Don't reset decision_stack - we want to continue exploration
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dfs_new() {
        let dfs = DFSStrategy::new(100);
        assert_eq!(dfs.max_depth(), 100);
        assert_eq!(dfs.schedules_explored(), 0);
        assert!(!dfs.is_exhausted());
    }

    #[test]
    fn test_dfs_single_task() {
        let mut dfs = DFSStrategy::new(10);
        let chosen = dfs.select(&[1]);
        assert_eq!(chosen, 1);
    }

    #[test]
    fn test_dfs_explores_first() {
        let mut dfs = DFSStrategy::new(10);
        
        // First selection should pick first option
        let chosen = dfs.select(&[1, 2, 3]);
        assert_eq!(chosen, 1);
    }

    #[test]
    fn test_dfs_backtrack() {
        let mut dfs = DFSStrategy::new(10);
        
        // First run
        assert_eq!(dfs.select(&[1, 2]), 1);
        dfs.schedule_complete();
        
        // After backtrack, should try index 1
        dfs.reset();
        assert_eq!(dfs.select(&[1, 2]), 2);
    }

    #[test]
    fn test_dfs_exhaustion() {
        let mut dfs = DFSStrategy::new(2);
        
        // Explore all combinations for 2 tasks, 2 decisions
        // Should be 2^2 = 4 schedules max
        for _ in 0..10 {
            if dfs.is_exhausted() {
                break;
            }
            dfs.select(&[1, 2]);
            dfs.select(&[1, 2]);
            dfs.schedule_complete();
            dfs.reset();
        }
        
        assert!(dfs.schedules_explored() > 0);
    }

    #[test]
    fn test_dfs_max_depth_limit() {
        let mut dfs = DFSStrategy::new(2);
        
        // Make more decisions than max depth
        dfs.select(&[1, 2]);
        dfs.select(&[1, 2]);
        dfs.select(&[1, 2]); // Beyond max depth
        
        // Should not crash, just picks first
    }

    #[test]
    fn test_dfs_deterministic_replay() {
        let mut dfs1 = DFSStrategy::new(10);
        let mut dfs2 = DFSStrategy::new(10);
        
        // Both should make same choices initially
        assert_eq!(dfs1.select(&[1, 2, 3]), dfs2.select(&[1, 2, 3]));
        assert_eq!(dfs1.select(&[4, 5]), dfs2.select(&[4, 5]));
    }
}
