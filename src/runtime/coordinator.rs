//! Runtime coordinator that ties together scheduling, time, and network.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use crate::network::{NetworkConfig, NetworkSim};
use crate::scheduler::{Scheduler, Strategy};
use crate::time::{Clock, TimerWheel};
use crate::{Result, TaskId};

use super::task::{Task, TaskHandle};
use super::waker::{create_waker, WakeNotifier};

/// Configuration for the runtime.
#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    /// Scheduling strategy.
    pub strategy: Strategy,
    /// Random seed for determinism.
    pub seed: u64,
    /// Network configuration.
    pub network: NetworkConfig,
    /// Maximum simulation time before timeout.
    pub max_time: Duration,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            strategy: Strategy::Fifo,
            seed: 0,
            network: NetworkConfig::default(),
            max_time: Duration::from_secs(60),
        }
    }
}

/// Result of a single simulation step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepResult {
    /// A task was polled.
    TaskPolled(TaskId),
    /// Time was advanced by the given duration.
    TimeAdvanced(Duration),
    /// No progress could be made (deadlock or complete).
    NoProgress,
    /// All tasks completed.
    Complete,
}

/// The main runtime coordinator.
pub struct Runtime {
    /// Task scheduler.
    scheduler: Arc<Mutex<Scheduler>>,
    /// Active tasks.
    tasks: HashMap<TaskId, Task>,
    /// Virtual clock.
    clock: Clock,
    /// Timer wheel for sleep operations.
    timers: TimerWheel,
    /// Network simulator.
    network: NetworkSim,
    /// Configuration.
    config: RuntimeConfig,
    /// Next task ID.
    next_task_id: TaskId,
}

impl Runtime {
    /// Creates a new runtime with the given configuration.
    pub fn new(config: RuntimeConfig) -> Self {
        Self {
            scheduler: Arc::new(Mutex::new(Scheduler::new(config.strategy.clone()))),
            tasks: HashMap::new(),
            clock: Clock::new(),
            timers: TimerWheel::new(),
            network: NetworkSim::new(config.network.clone(), config.seed),
            config,
            next_task_id: 0,
        }
    }

    /// Creates a runtime with default configuration.
    pub fn with_seed(seed: u64) -> Self {
        let mut config = RuntimeConfig::default();
        config.seed = seed;
        Self::new(config)
    }

    /// Spawns a new task.
    pub fn spawn<F>(&mut self, future: F) -> TaskHandle
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let task_id = self.next_task_id;
        self.next_task_id += 1;

        let (task, handle) = Task::new(task_id, future);
        self.tasks.insert(task_id, task);

        // Add to scheduler
        if let Ok(mut scheduler) = self.scheduler.lock() {
            scheduler.add_task();
        }

        handle
    }

    /// Performs a single simulation step.
    pub fn step(&mut self) -> StepResult {
        // 1. Deliver network messages
        self.network.tick(self.clock.now());

        // 2. Fire expired timers
        let wakers = self.timers.fire_expired(self.clock.now());
        for waker in wakers {
            waker.wake();
        }

        // 3. Check if all tasks are complete
        if self.tasks.values().all(|t| t.is_complete()) {
            return StepResult::Complete;
        }

        // 4. Try to select a ready task
        let task_id = {
            let mut scheduler = self.scheduler.lock().unwrap();
            scheduler.next()
        };

        if let Some(task_id) = task_id {
            // 5. Poll the task
            if let Some(task) = self.tasks.get_mut(&task_id) {
                let notifier: Arc<dyn WakeNotifier> = self.scheduler.clone();
                let waker = create_waker(task_id, &notifier);
                let mut cx = Context::from_waker(&waker);

                match task.poll(&mut cx) {
                    Poll::Ready(()) => {
                        // Task completed
                        let mut scheduler = self.scheduler.lock().unwrap();
                        scheduler.remove_task(task_id);
                    }
                    Poll::Pending => {
                        // Task yielded - mark ready again for next step
                        let mut scheduler = self.scheduler.lock().unwrap();
                        scheduler.mark_ready(task_id);
                    }
                }
            }

            return StepResult::TaskPolled(task_id);
        }

        // 6. No ready tasks - try to advance time
        if let Some(next_time) = self.next_event_time() {
            let current = self.clock.now();
            if next_time > current {
                let advance = next_time.duration_since(current).unwrap_or_default();
                self.clock.advance(advance);
                return StepResult::TimeAdvanced(advance);
            }
        }

        StepResult::NoProgress
    }

    /// Runs until all tasks complete or reach a stable blocked state.
    pub fn run_until_stable(&mut self) -> Result<()> {
        loop {
            match self.step() {
                StepResult::Complete => return Ok(()),
                StepResult::NoProgress => {
                    // Check for deadlock
                    if !self.tasks.values().all(|t| t.is_complete()) {
                        return Err(crate::error::Error::Deadlock {
                            cycle: self.tasks.keys().copied().collect(),
                        });
                    }
                    return Ok(());
                }
                _ => continue,
            }
        }
    }

    /// Runs for the given duration of simulated time.
    pub fn run_for(&mut self, duration: Duration) -> Result<()> {
        let deadline = self.clock.now().saturating_add(duration);
        
        while self.clock.now() < deadline {
            match self.step() {
                StepResult::Complete => return Ok(()),
                StepResult::NoProgress => break,
                _ => {}
            }
        }
        
        Ok(())
    }

    /// Returns the current simulated time.
    pub fn now(&self) -> crate::time::Instant {
        self.clock.now()
    }

    /// Returns a reference to the clock.
    pub fn clock(&self) -> &Clock {
        &self.clock
    }

    /// Returns a mutable reference to the network.
    pub fn network(&mut self) -> &mut NetworkSim {
        &mut self.network
    }

    /// Returns the number of active tasks.
    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    /// Returns the number of completed tasks.
    pub fn completed_count(&self) -> usize {
        self.tasks.values().filter(|t| t.is_complete()).count()
    }

    /// Returns the next time an event will occur.
    fn next_event_time(&self) -> Option<crate::time::Instant> {
        let timer_time = self.timers.next_deadline();
        let network_time = self.network.next_event_time();

        match (timer_time, network_time) {
            (Some(t), Some(n)) => Some(t.min(n)),
            (Some(t), None) => Some(t),
            (None, Some(n)) => Some(n),
            (None, None) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_runtime() {
        let rt = Runtime::with_seed(42);
        assert_eq!(rt.task_count(), 0);
    }

    #[test]
    fn test_spawn_task() {
        let mut rt = Runtime::with_seed(42);
        let handle = rt.spawn(async {});
        
        assert_eq!(rt.task_count(), 1);
        assert!(!handle.is_complete());
    }

    #[test]
    fn test_step_completes_immediate_task() {
        let mut rt = Runtime::with_seed(42);
        let handle = rt.spawn(async {});

        let result = rt.step();
        assert!(matches!(result, StepResult::TaskPolled(_)));
        assert!(handle.is_complete());
    }

    #[test]
    fn test_run_until_stable_empty() {
        let mut rt = Runtime::with_seed(42);
        let result = rt.run_until_stable();
        assert!(result.is_ok());
    }

    #[test]
    fn test_run_until_stable_single_task() {
        let mut rt = Runtime::with_seed(42);
        let handle = rt.spawn(async {});

        let result = rt.run_until_stable();
        assert!(result.is_ok());
        assert!(handle.is_complete());
    }

    #[test]
    fn test_multiple_tasks() {
        let mut rt = Runtime::with_seed(42);
        let h1 = rt.spawn(async {});
        let h2 = rt.spawn(async {});
        let h3 = rt.spawn(async {});

        rt.run_until_stable().unwrap();

        assert!(h1.is_complete());
        assert!(h2.is_complete());
        assert!(h3.is_complete());
    }

    #[test]
    fn test_completed_count() {
        let mut rt = Runtime::with_seed(42);
        rt.spawn(async {});
        rt.spawn(async {});

        assert_eq!(rt.completed_count(), 0);
        rt.step();
        assert_eq!(rt.completed_count(), 1);
        rt.step();
        assert_eq!(rt.completed_count(), 2);
    }

    #[test]
    fn test_run_for() {
        let mut rt = Runtime::with_seed(42);
        rt.spawn(async {});

        rt.run_for(Duration::from_secs(1)).unwrap();
        assert!(rt.now().as_nanos() >= 0);
    }

    #[test]
    fn test_clock_access() {
        let rt = Runtime::with_seed(42);
        assert_eq!(rt.clock().now().as_nanos(), 0);
    }
}
