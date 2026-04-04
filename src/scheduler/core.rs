//! Core scheduler for managing task execution order.

use std::collections::{HashMap, HashSet, VecDeque};

use tracing::trace;

use crate::runtime::{BlockReason, WakeNotifier};
use crate::TaskId;

use super::context_bound::ContextBoundStrategy;
use super::dfs::DFSStrategy;
use super::pct::PCTStrategy;
use super::random::RandomStrategy;
use super::strategy::{FifoStrategy, ScheduleStrategy, Strategy};

/// The core scheduler that manages task states and selection.
pub struct Scheduler {
    /// Tasks that are ready to run.
    ready: VecDeque<TaskId>,
    /// Set of ready task IDs for O(1) lookup.
    ready_set: HashSet<TaskId>,
    /// Tasks that are blocked and why.
    blocked: HashMap<TaskId, BlockReason>,
    /// All known task IDs.
    all_tasks: HashSet<TaskId>,
    /// Scheduling strategy.
    strategy: Box<dyn ScheduleStrategy>,
    /// Next task ID to assign.
    next_id: TaskId,
}

impl Scheduler {
    /// Creates a new scheduler with the given strategy.
    pub fn new(strategy: Strategy) -> Self {
        let strategy_impl: Box<dyn ScheduleStrategy> = match strategy {
            Strategy::Fifo => Box::new(FifoStrategy::new()),
            Strategy::Random { seed } => {
                Box::new(RandomStrategy::new(seed))
            }
            Strategy::DepthFirst { max_depth } => {
                Box::new(DFSStrategy::new(max_depth))
            }
            Strategy::PCT { seed, bug_depth } => {
                Box::new(PCTStrategy::new(seed, bug_depth))
            }
            Strategy::ContextBound { max_preemptions, seed } => {
                Box::new(ContextBoundStrategy::new(max_preemptions, seed))
            }
        };

        Self {
            ready: VecDeque::new(),
            ready_set: HashSet::new(),
            blocked: HashMap::new(),
            all_tasks: HashSet::new(),
            strategy: strategy_impl,
            next_id: 0,
        }
    }

    /// Creates a scheduler with FIFO strategy.
    pub fn fifo() -> Self {
        Self::new(Strategy::Fifo)
    }

    /// Adds a new task and returns its ID.
    pub fn add_task(&mut self) -> TaskId {
        let id = self.next_id;
        self.next_id += 1;
        self.all_tasks.insert(id);
        self.ready.push_back(id);
        self.ready_set.insert(id);
        id
    }

    /// Removes a task from the scheduler.
    pub fn remove_task(&mut self, id: TaskId) {
        self.all_tasks.remove(&id);
        self.ready.retain(|&t| t != id);
        self.ready_set.remove(&id);
        self.blocked.remove(&id);
    }

    /// Marks a task as ready to run.
    pub fn mark_ready(&mut self, id: TaskId) {
        if self.all_tasks.contains(&id) && !self.ready_set.contains(&id) {
            self.blocked.remove(&id);
            self.ready.push_back(id);
            self.ready_set.insert(id);
        }
    }

    /// Marks a task as blocked with the given reason.
    pub fn mark_blocked(&mut self, id: TaskId, reason: BlockReason) {
        if self.all_tasks.contains(&id) {
            self.ready.retain(|&t| t != id);
            self.ready_set.remove(&id);
            self.blocked.insert(id, reason);
        }
    }

    /// Returns the next task to run, or None if no tasks are ready.
    pub fn select_next(&mut self) -> Option<TaskId> {
        if self.ready.is_empty() {
            trace!("no ready tasks");
            return None;
        }

        let ready_vec: Vec<TaskId> = self.ready.iter().copied().collect();
        let chosen = self.strategy.select(&ready_vec);

        trace!(chosen, ready = ?ready_vec, "schedule decision");

        // Remove chosen from ready queue
        self.ready.retain(|&t| t != chosen);
        self.ready_set.remove(&chosen);

        self.strategy.on_decision(chosen, &ready_vec);

        Some(chosen)
    }

    /// Returns true if any tasks are ready.
    pub fn has_ready(&self) -> bool {
        !self.ready.is_empty()
    }

    /// Returns true if all tasks are blocked (potential deadlock).
    pub fn all_blocked(&self) -> bool {
        self.ready.is_empty() && !self.blocked.is_empty()
    }

    /// Returns the number of ready tasks.
    pub fn ready_count(&self) -> usize {
        self.ready.len()
    }

    /// Returns the number of blocked tasks.
    pub fn blocked_count(&self) -> usize {
        self.blocked.len()
    }

    /// Returns the total number of tasks.
    pub fn task_count(&self) -> usize {
        self.all_tasks.len()
    }

    /// Returns the block reason for a task, if blocked.
    pub fn block_reason(&self, id: TaskId) -> Option<&BlockReason> {
        self.blocked.get(&id)
    }

    /// Resets the scheduler for a new simulation run.
    pub fn reset(&mut self) {
        self.ready.clear();
        self.ready_set.clear();
        self.blocked.clear();
        self.all_tasks.clear();
        self.next_id = 0;
        self.strategy.reset();
    }
}

impl WakeNotifier for std::sync::Mutex<Scheduler> {
    fn notify_ready(&self, task_id: TaskId) {
        if let Ok(mut scheduler) = self.lock() {
            scheduler.mark_ready(task_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::Instant;

    #[test]
    fn test_new_scheduler() {
        let s = Scheduler::fifo();
        assert_eq!(s.ready_count(), 0);
        assert_eq!(s.blocked_count(), 0);
        assert_eq!(s.task_count(), 0);
    }

    #[test]
    fn test_add_task() {
        let mut s = Scheduler::fifo();
        let id1 = s.add_task();
        let id2 = s.add_task();
        let id3 = s.add_task();

        assert_eq!(id1, 0);
        assert_eq!(id2, 1);
        assert_eq!(id3, 2);
        assert_eq!(s.task_count(), 3);
        assert_eq!(s.ready_count(), 3);
    }

    #[test]
    fn test_next_fifo() {
        let mut s = Scheduler::fifo();
        s.add_task(); // 0
        s.add_task(); // 1
        s.add_task(); // 2

        assert_eq!(s.select_next(), Some(0));
        assert_eq!(s.select_next(), Some(1));
        assert_eq!(s.select_next(), Some(2));
        assert_eq!(s.select_next(), None);
    }

    #[test]
    fn test_mark_ready() {
        let mut s = Scheduler::fifo();
        let id = s.add_task();
        s.select_next(); // Remove from ready

        assert!(!s.has_ready());
        s.mark_ready(id);
        assert!(s.has_ready());
        assert_eq!(s.select_next(), Some(id));
    }

    #[test]
    fn test_mark_blocked() {
        let mut s = Scheduler::fifo();
        let id = s.add_task();

        s.mark_blocked(id, BlockReason::Channel);
        assert!(!s.has_ready());
        assert_eq!(s.blocked_count(), 1);
        assert!(matches!(s.block_reason(id), Some(BlockReason::Channel)));
    }

    #[test]
    fn test_mark_blocked_then_ready() {
        let mut s = Scheduler::fifo();
        let id = s.add_task();

        s.mark_blocked(id, BlockReason::Time(Instant::from_nanos(100)));
        assert_eq!(s.ready_count(), 0);
        assert_eq!(s.blocked_count(), 1);

        s.mark_ready(id);
        assert_eq!(s.ready_count(), 1);
        assert_eq!(s.blocked_count(), 0);
    }

    #[test]
    fn test_remove_task() {
        let mut s = Scheduler::fifo();
        let id = s.add_task();
        s.add_task();

        s.remove_task(id);
        assert_eq!(s.task_count(), 1);
        assert_eq!(s.ready_count(), 1);
    }

    #[test]
    fn test_all_blocked() {
        let mut s = Scheduler::fifo();
        let id = s.add_task();

        assert!(!s.all_blocked()); // One ready task

        s.mark_blocked(id, BlockReason::Channel);
        assert!(s.all_blocked()); // Now blocked

        s.mark_ready(id);
        assert!(!s.all_blocked()); // Ready again
    }

    #[test]
    fn test_reset() {
        let mut s = Scheduler::fifo();
        s.add_task();
        s.add_task();
        s.mark_blocked(0, BlockReason::Channel);

        s.reset();
        assert_eq!(s.task_count(), 0);
        assert_eq!(s.ready_count(), 0);
        assert_eq!(s.blocked_count(), 0);
    }

    #[test]
    fn test_no_double_ready() {
        let mut s = Scheduler::fifo();
        let id = s.add_task();

        // Try to mark ready when already ready
        s.mark_ready(id);
        s.mark_ready(id);
        
        assert_eq!(s.ready_count(), 1);
    }
}
