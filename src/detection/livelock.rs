use std::collections::HashMap;

use crate::TaskId;

/// Tracks task progress for livelock detection.
#[derive(Debug, Clone)]
pub struct ProgressTracker {
    /// Steps since last meaningful progress per task
    steps_without_progress: HashMap<TaskId, u64>,
    /// Total steps taken per task
    total_steps: HashMap<TaskId, u64>,
    /// Threshold for considering a task stuck
    threshold: u64,
}

impl ProgressTracker {
    /// Create a new progress tracker.
    pub fn new(threshold: u64) -> Self {
        Self {
            steps_without_progress: HashMap::new(),
            total_steps: HashMap::new(),
            threshold,
        }
    }

    /// Create with default threshold (1000 steps).
    pub fn with_default_threshold() -> Self {
        Self::new(1000)
    }

    /// Record that a task took a step.
    pub fn record_step(&mut self, task: TaskId) {
        *self.steps_without_progress.entry(task).or_insert(0) += 1;
        *self.total_steps.entry(task).or_insert(0) += 1;
    }

    /// Record that a task made meaningful progress (resets counter).
    pub fn record_progress(&mut self, task: TaskId) {
        self.steps_without_progress.insert(task, 0);
    }

    /// Remove a task (e.g., when it completes).
    pub fn remove_task(&mut self, task: TaskId) {
        self.steps_without_progress.remove(&task);
        self.total_steps.remove(&task);
    }

    /// Get steps without progress for a task.
    pub fn steps_without_progress(&self, task: TaskId) -> u64 {
        self.steps_without_progress.get(&task).copied().unwrap_or(0)
    }

    /// Get total steps for a task.
    pub fn total_steps(&self, task: TaskId) -> u64 {
        self.total_steps.get(&task).copied().unwrap_or(0)
    }

    /// Check if a specific task appears stuck.
    pub fn is_stuck(&self, task: TaskId) -> bool {
        self.steps_without_progress(task) >= self.threshold
    }

    /// Get all tasks that appear stuck.
    pub fn stuck_tasks(&self) -> Vec<TaskId> {
        self.steps_without_progress
            .iter()
            .filter(|(_, &steps)| steps >= self.threshold)
            .map(|(&task, _)| task)
            .collect()
    }

    /// Set the threshold.
    pub fn set_threshold(&mut self, threshold: u64) {
        self.threshold = threshold;
    }

    /// Get the threshold.
    pub fn threshold(&self) -> u64 {
        self.threshold
    }
}

/// Livelock detector that monitors multiple tasks.
#[derive(Debug)]
pub struct LivelockDetector {
    tracker: ProgressTracker,
}

impl LivelockDetector {
    /// Create a new livelock detector.
    pub fn new(threshold: u64) -> Self {
        Self {
            tracker: ProgressTracker::new(threshold),
        }
    }

    /// Create with default threshold.
    pub fn with_default_threshold() -> Self {
        Self::new(1000)
    }

    /// Record that a task took a step (no meaningful progress).
    pub fn task_step(&mut self, task: TaskId) {
        self.tracker.record_step(task);
    }

    /// Record that a task made meaningful progress.
    pub fn task_progress(&mut self, task: TaskId) {
        self.tracker.record_progress(task);
    }

    /// Record that a task completed.
    pub fn task_completed(&mut self, task: TaskId) {
        self.tracker.remove_task(task);
    }

    /// Check for livelock. Returns stuck tasks if found.
    pub fn check(&self) -> Option<Vec<TaskId>> {
        let stuck = self.tracker.stuck_tasks();
        if stuck.is_empty() {
            None
        } else {
            Some(stuck)
        }
    }

    /// Check if a specific task is stuck.
    pub fn is_stuck(&self, task: TaskId) -> bool {
        self.tracker.is_stuck(task)
    }

    /// Get progress info for a task.
    pub fn task_info(&self, task: TaskId) -> (u64, u64) {
        (
            self.tracker.steps_without_progress(task),
            self.tracker.total_steps(task),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tracker_new() {
        let tracker = ProgressTracker::new(100);
        assert_eq!(tracker.threshold(), 100);
    }

    #[test]
    fn test_tracker_record_step() {
        let mut tracker = ProgressTracker::new(10);
        
        tracker.record_step(1);
        tracker.record_step(1);
        tracker.record_step(1);
        
        assert_eq!(tracker.steps_without_progress(1), 3);
        assert_eq!(tracker.total_steps(1), 3);
    }

    #[test]
    fn test_tracker_record_progress() {
        let mut tracker = ProgressTracker::new(10);
        
        tracker.record_step(1);
        tracker.record_step(1);
        tracker.record_progress(1);
        
        assert_eq!(tracker.steps_without_progress(1), 0);
        assert_eq!(tracker.total_steps(1), 2);
    }

    #[test]
    fn test_tracker_is_stuck() {
        let mut tracker = ProgressTracker::new(5);
        
        for _ in 0..4 {
            tracker.record_step(1);
        }
        assert!(!tracker.is_stuck(1));
        
        tracker.record_step(1);
        assert!(tracker.is_stuck(1));
    }

    #[test]
    fn test_tracker_stuck_tasks() {
        let mut tracker = ProgressTracker::new(3);
        
        for _ in 0..3 {
            tracker.record_step(1);
            tracker.record_step(2);
        }
        
        // Both should be stuck
        let stuck = tracker.stuck_tasks();
        assert_eq!(stuck.len(), 2);
        
        // Make task 1 progress
        tracker.record_progress(1);
        let stuck = tracker.stuck_tasks();
        assert_eq!(stuck.len(), 1);
        assert!(stuck.contains(&2));
    }

    #[test]
    fn test_tracker_remove_task() {
        let mut tracker = ProgressTracker::new(5);
        
        tracker.record_step(1);
        tracker.remove_task(1);
        
        assert_eq!(tracker.steps_without_progress(1), 0);
        assert_eq!(tracker.total_steps(1), 0);
    }

    #[test]
    fn test_detector_no_livelock() {
        let mut detector = LivelockDetector::new(10);
        
        for _ in 0..5 {
            detector.task_step(1);
        }
        
        assert!(detector.check().is_none());
    }

    #[test]
    fn test_detector_livelock() {
        let mut detector = LivelockDetector::new(10);
        
        for _ in 0..10 {
            detector.task_step(1);
        }
        
        let stuck = detector.check();
        assert!(stuck.is_some());
        assert!(stuck.unwrap().contains(&1));
    }

    #[test]
    fn test_detector_progress_resets() {
        let mut detector = LivelockDetector::new(10);
        
        for _ in 0..9 {
            detector.task_step(1);
        }
        
        detector.task_progress(1);
        
        for _ in 0..9 {
            detector.task_step(1);
        }
        
        // Still not stuck because progress was made
        assert!(detector.check().is_none());
    }

    #[test]
    fn test_detector_task_completed() {
        let mut detector = LivelockDetector::new(5);
        
        for _ in 0..5 {
            detector.task_step(1);
        }
        
        assert!(detector.check().is_some());
        
        detector.task_completed(1);
        assert!(detector.check().is_none());
    }
}
