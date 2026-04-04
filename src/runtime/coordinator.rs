//! Runtime coordinator that ties together scheduling, time, and network.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use tracing::{debug, warn, instrument};

use crate::detection::{DeadlockDetector, LivelockDetector, RaceDetector, DataRace};
use crate::network::{NetworkConfig, NetworkSim};
use crate::recording::{Event, EventPayload, Header, RecordingReader, RecordingWriter};
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
    /// Enable recording.
    pub recording_enabled: bool,
    /// Recording output path.
    pub recording_path: Option<String>,
    /// Replay from recording file.
    pub replay_path: Option<String>,
    /// Verify replay matches recording.
    pub replay_verify: bool,
    /// Enable deadlock detection.
    pub deadlock_detection: bool,
    /// Enable livelock detection.
    pub livelock_detection: bool,
    /// Livelock threshold (steps without progress).
    pub livelock_threshold: u64,
    /// Enable data race detection.
    pub race_detection: bool,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            strategy: Strategy::Fifo,
            seed: 0,
            network: NetworkConfig::default(),
            max_time: Duration::from_secs(60),
            recording_enabled: false,
            recording_path: None,
            replay_path: None,
            replay_verify: false,
            deadlock_detection: true,
            livelock_detection: true,
            livelock_threshold: 10000,
            race_detection: false,
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
    /// Livelock detected.
    Livelock(Vec<TaskId>),
    /// Data race detected.
    RaceDetected,
}

/// Replay state for enforcing recorded schedules.
#[allow(dead_code)]
#[derive(Debug, Default)]
struct ReplayState {
    /// Whether replay mode is active.
    active: bool,
    /// Verify execution matches recording.
    verify: bool,
    /// Recorded schedule decisions: (chosen_task, ready_tasks).
    schedule_decisions: Vec<(TaskId, Vec<TaskId>)>,
    /// Current index into schedule_decisions.
    schedule_index: usize,
    /// Recorded random values.
    random_values: Vec<u64>,
    /// Current random value index.
    random_index: usize,
    /// Mismatches found during verification.
    mismatches: Vec<String>,
}

impl ReplayState {
    /// Load replay state from a recording file.
    fn load(path: &str, verify: bool) -> Result<Self> {
        let mut reader = RecordingReader::open(path)?;
        let mut state = Self {
            active: true,
            verify,
            schedule_decisions: Vec::new(),
            schedule_index: 0,
            random_values: Vec::new(),
            random_index: 0,
            mismatches: Vec::new(),
        };

        while let Some(event) = reader.next_event()? {
            match event.payload {
                EventPayload::ScheduleDecision { chosen, ready } => {
                    state.schedule_decisions.push((chosen, ready));
                }
                EventPayload::RandomGen { result } => {
                    state.random_values.push(result);
                }
                _ => {}
            }
        }

        Ok(state)
    }

    /// Get the next recorded schedule decision.
    fn next_schedule(&mut self) -> Option<(TaskId, Vec<TaskId>)> {
        if self.schedule_index < self.schedule_decisions.len() {
            let decision = self.schedule_decisions[self.schedule_index].clone();
            self.schedule_index += 1;
            Some(decision)
        } else {
            None
        }
    }

    /// Record a mismatch during verification.
    fn record_mismatch(&mut self, msg: String) {
        if self.verify {
            self.mismatches.push(msg);
        }
    }
}

/// The main runtime coordinator.
pub struct Runtime {
    /// Task scheduler.
    scheduler: Arc<Mutex<Scheduler>>,
    /// Active tasks.
    tasks: HashMap<TaskId, Task>,
    /// Task names for recording.
    task_names: HashMap<TaskId, String>,
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
    /// Recording writer (if enabled).
    recorder: Option<RecordingWriter>,
    /// Replay state (if replaying).
    replay: Option<ReplayState>,
    /// Deadlock detector.
    deadlock_detector: DeadlockDetector,
    /// Livelock detector.
    livelock_detector: LivelockDetector,
    /// Race detector (optional).
    race_detector: Option<RaceDetector>,
    /// Total steps taken (for livelock detection).
    total_steps: u64,
}

impl Runtime {
    /// Creates a new runtime with the given configuration.
    pub fn new(config: RuntimeConfig) -> Self {
        let recorder = if config.recording_enabled {
            let path = config.recording_path.clone()
                .unwrap_or_else(|| format!("recording_{}.chrn", config.seed));
            let strategy_byte = match &config.strategy {
                Strategy::Fifo => 0,
                Strategy::Random { .. } => 1,
                Strategy::PCT { .. } => 2,
                Strategy::DepthFirst { .. } => 3,
                Strategy::ContextBound { .. } => 4,
            };
            let header = Header::new(config.seed, strategy_byte);
            RecordingWriter::new(&path, header).ok()
        } else {
            None
        };

        let replay = config.replay_path.as_ref().and_then(|path| {
            ReplayState::load(path, config.replay_verify).ok()
        });

        let race_detector = if config.race_detection {
            Some(RaceDetector::new())
        } else {
            None
        };

        Self {
            scheduler: Arc::new(Mutex::new(Scheduler::new(config.strategy.clone()))),
            tasks: HashMap::new(),
            task_names: HashMap::new(),
            clock: Clock::new(),
            timers: TimerWheel::new(),
            network: NetworkSim::new(config.network.clone(), config.seed),
            next_task_id: 0,
            recorder,
            replay,
            deadlock_detector: DeadlockDetector::new(),
            livelock_detector: LivelockDetector::new(config.livelock_threshold),
            race_detector,
            total_steps: 0,
            config,
        }
    }

    /// Creates a runtime with default configuration.
    pub fn with_seed(seed: u64) -> Self {
        Self::new(RuntimeConfig {
            seed,
            ..Default::default()
        })
    }

    /// Creates a runtime with recording enabled.
    pub fn with_recording(seed: u64, path: impl Into<String>) -> Self {
        Self::new(RuntimeConfig {
            seed,
            recording_enabled: true,
            recording_path: Some(path.into()),
            ..Default::default()
        })
    }

    /// Creates a runtime in replay mode.
    pub fn with_replay(path: impl Into<String>, verify: bool) -> Result<Self> {
        let path_str = path.into();
        let reader = RecordingReader::open(&path_str)?;
        let seed = reader.seed();
        
        Ok(Self::new(RuntimeConfig {
            seed,
            replay_path: Some(path_str),
            replay_verify: verify,
            ..Default::default()
        }))
    }

    /// Creates a runtime with detection features enabled.
    pub fn with_detection(seed: u64, race_detection: bool) -> Self {
        Self::new(RuntimeConfig {
            seed,
            deadlock_detection: true,
            livelock_detection: true,
            race_detection,
            ..Default::default()
        })
    }

    /// Check if we're in replay mode.
    pub fn is_replay(&self) -> bool {
        self.replay.as_ref().map(|r| r.active).unwrap_or(false)
    }

    /// Get any verification mismatches from replay.
    pub fn replay_mismatches(&self) -> Vec<String> {
        self.replay.as_ref()
            .map(|r| r.mismatches.clone())
            .unwrap_or_default()
    }

    /// Get detected data races.
    pub fn detected_races(&self) -> Vec<DataRace> {
        self.race_detector.as_ref()
            .map(|r| r.races().to_vec())
            .unwrap_or_default()
    }

    /// Record an event if recording is enabled.
    fn record_event(&mut self, event: Event) {
        if let Some(ref mut recorder) = self.recorder {
            let _ = recorder.write_event(&event);
        }
    }

    /// Spawns a new task.
    pub fn spawn<F>(&mut self, future: F) -> TaskHandle
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.spawn_named(future, "task")
    }

    /// Spawns a new task with a name.
    #[instrument(skip(self, future), fields(task_name = %name))]
    pub fn spawn_named<F>(&mut self, future: F, name: &str) -> TaskHandle
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let task_id = self.next_task_id;
        self.next_task_id += 1;

        debug!(task_id, "spawning task");

        let (task, handle) = Task::new(task_id, future);
        self.tasks.insert(task_id, task);
        self.task_names.insert(task_id, name.to_string());

        // Add to scheduler
        if let Ok(mut scheduler) = self.scheduler.lock() {
            scheduler.add_task();
        }

        // Record spawn event
        let timestamp = self.clock.now().as_nanos();
        self.record_event(Event::task_spawn(task_id, 0, name.to_string(), timestamp));

        handle
    }

    /// Select the next task to run, respecting replay mode.
    fn select_next_task(&mut self) -> Option<(TaskId, Vec<TaskId>)> {
        // Collect ready tasks
        let ready: Vec<TaskId> = (0..self.next_task_id)
            .filter(|id| {
                self.tasks.get(id)
                    .map(|t| t.is_ready())
                    .unwrap_or(false)
            })
            .collect();

        if ready.is_empty() {
            return None;
        }

        // In replay mode, use recorded decisions
        if let Some(ref mut replay) = self.replay {
            if replay.active {
                if let Some((recorded_chosen, recorded_ready)) = replay.next_schedule() {
                    // Verify the ready set matches if in verify mode
                    if replay.verify {
                        let mut ready_sorted = ready.clone();
                        let mut recorded_sorted = recorded_ready.clone();
                        ready_sorted.sort();
                        recorded_sorted.sort();
                        
                        if ready_sorted != recorded_sorted {
                            replay.record_mismatch(format!(
                                "Ready set mismatch at step {}: expected {:?}, got {:?}",
                                replay.schedule_index - 1, recorded_ready, ready
                            ));
                        }
                    }

                    // Use the recorded decision if the task is still ready
                    if ready.contains(&recorded_chosen) {
                        return Some((recorded_chosen, ready));
                    } else {
                        // Task not ready - record mismatch and fall through to scheduler
                        if replay.verify {
                            replay.record_mismatch(format!(
                                "Recorded task {} not ready at step {}",
                                recorded_chosen, replay.schedule_index - 1
                            ));
                        }
                    }
                }
            }
        }

        // Normal mode: use scheduler
        let mut scheduler = self.scheduler.lock().unwrap();
        scheduler.select_next().map(|chosen| (chosen, ready))
    }

    /// Performs a single simulation step.
    #[instrument(skip(self), level = "trace")]
    pub fn step(&mut self) -> StepResult {
        self.total_steps += 1;

        // 0. Check assertions (always-assertions checked every step)
        self.check_assertions();

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

        // 4. Check for livelock
        if self.config.livelock_detection {
            if let Some(stuck_tasks) = self.livelock_detector.check() {
                return StepResult::Livelock(stuck_tasks);
            }
        }

        // 5. Try to select a ready task (respects replay mode)
        let selection = self.select_next_task();

        if let Some((task_id, ready_tasks)) = selection {
            // Track livelock - task is taking a step
            self.livelock_detector.task_step(task_id);

            // Record schedule decision
            let timestamp = self.clock.now().as_nanos();
            self.record_event(Event::schedule_decision(0, timestamp, task_id, ready_tasks));

            // 6. Poll the task
            if let Some(task) = self.tasks.get_mut(&task_id) {
                let notifier: Arc<dyn WakeNotifier> = self.scheduler.clone();
                let waker = create_waker(task_id, &notifier);
                let mut cx = Context::from_waker(&waker);

                let poll_result = task.poll(&mut cx);
                let timestamp = self.clock.now().as_nanos();
                
                match poll_result {
                    Poll::Ready(()) => {
                        // Task completed - this is progress
                        self.livelock_detector.task_progress(task_id);
                        self.livelock_detector.task_completed(task_id);
                        self.deadlock_detector.task_completed(task_id);
                        
                        // Record completion
                        self.record_event(Event::task_complete(task_id, timestamp));
                        
                        let mut scheduler = self.scheduler.lock().unwrap();
                        scheduler.remove_task(task_id);
                    }
                    Poll::Pending => {
                        // Task yielded
                        self.record_event(Event::task_yield(task_id, timestamp));
                        
                        let mut scheduler = self.scheduler.lock().unwrap();
                        scheduler.mark_ready(task_id);
                    }
                }
            }

            return StepResult::TaskPolled(task_id);
        }

        // 7. No ready tasks - check for deadlock
        if self.config.deadlock_detection {
            if let Some(_cycle) = self.deadlock_detector.check() {
                // All tasks blocked with a cycle
                return StepResult::NoProgress;
            }
        }

        // 8. Try to advance time
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

    /// Record a memory access for race detection.
    pub fn record_memory_access(&mut self, task: TaskId, location: u64, is_write: bool) -> Option<DataRace> {
        if let Some(ref mut detector) = self.race_detector {
            let access_type = if is_write {
                crate::detection::AccessType::Write
            } else {
                crate::detection::AccessType::Read
            };
            detector.record_access(task, location, access_type, None)
        } else {
            None
        }
    }

    /// Record that a task is waiting for another (for deadlock detection).
    pub fn record_wait(&mut self, waiter: TaskId, holder: TaskId) {
        self.deadlock_detector.task_waiting(waiter, holder);
    }

    /// Record that a task is no longer waiting.
    pub fn record_release(&mut self, waiter: TaskId, holder: TaskId) {
        self.deadlock_detector.task_released(waiter, holder);
    }

    /// Record meaningful progress for livelock detection.
    pub fn record_progress(&mut self, task: TaskId) {
        self.livelock_detector.task_progress(task);
    }

    /// Runs until all tasks complete or reach a stable blocked state.
    pub fn run_until_stable(&mut self) -> Result<()> {
        let max_time = self.config.max_time;
        let start = self.clock.now();
        
        loop {
            // Check timeout
            if let Some(elapsed) = self.clock.now().duration_since(start) {
                if elapsed >= max_time {
                    return Err(crate::error::Error::Timeout {
                        simulated_nanos: self.clock.now().as_nanos(),
                    });
                }
            }

            match self.step() {
                StepResult::Complete => {
                    self.finish_recording();
                    return Ok(());
                }
                StepResult::NoProgress => {
                    // Check for deadlock
                    if !self.tasks.values().all(|t| t.is_complete()) {
                        return Err(crate::error::Error::Deadlock {
                            cycle: self.tasks.keys().copied().collect(),
                        });
                    }
                    self.finish_recording();
                    return Ok(());
                }
                StepResult::Livelock(tasks) => {
                    return Err(crate::error::Error::Livelock { tasks });
                }
                StepResult::RaceDetected => {
                    // Continue but race is logged
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
                StepResult::Complete => {
                    self.finish_recording();
                    return Ok(());
                }
                StepResult::NoProgress => break,
                StepResult::Livelock(tasks) => {
                    return Err(crate::error::Error::Livelock { tasks });
                }
                _ => {}
            }
        }
        
        self.finish_recording();
        Ok(())
    }

    /// Finish recording and flush to disk.
    fn finish_recording(&mut self) {
        if let Some(recorder) = self.recorder.take() {
            let _ = recorder.finish();
        }
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

    /// Returns the seed used for this runtime.
    pub fn seed(&self) -> u64 {
        self.config.seed
    }

    /// Returns the total number of steps taken.
    pub fn total_steps(&self) -> u64 {
        self.total_steps
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

    /// Check all registered assertions.
    fn check_assertions(&self) {
        // If running within a sim context, delegate to it
        if crate::sim::is_simulation() {
            crate::sim::check_assertions();
        }
    }

    /// Verify all eventually-assertions are satisfied.
    pub fn verify_assertions(&self) -> std::result::Result<(), String> {
        if crate::sim::is_simulation() {
            crate::assertions::verify_all()
        } else {
            Ok(())
        }
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.finish_recording();
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
    fn test_spawn_named_task() {
        let mut rt = Runtime::with_seed(42);
        let handle = rt.spawn_named(async {}, "my_task");
        
        assert_eq!(rt.task_count(), 1);
        assert_eq!(rt.task_names.get(&handle.id()), Some(&"my_task".to_string()));
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
        // Verify time was tracked
        assert!(rt.now().as_nanos() <= 1_000_000_000);
    }

    #[test]
    fn test_clock_access() {
        let rt = Runtime::with_seed(42);
        assert_eq!(rt.clock().now().as_nanos(), 0);
    }

    #[test]
    fn test_seed_access() {
        let rt = Runtime::with_seed(12345);
        assert_eq!(rt.seed(), 12345);
    }

    #[test]
    fn test_with_recording() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.chrn");
        
        let mut rt = Runtime::with_recording(42, path.to_str().unwrap());
        rt.spawn_named(async {}, "test_task");
        rt.run_until_stable().unwrap();
        
        // Recording should exist
        assert!(path.exists());
    }

    #[test]
    fn test_with_detection() {
        let mut rt = Runtime::with_detection(42, true);
        rt.spawn(async {});
        rt.run_until_stable().unwrap();
        
        // Should complete without issues
        assert_eq!(rt.detected_races().len(), 0);
    }

    #[test]
    fn test_is_replay() {
        let rt = Runtime::with_seed(42);
        assert!(!rt.is_replay());
    }

    #[test]
    fn test_total_steps() {
        let mut rt = Runtime::with_seed(42);
        rt.spawn(async {});
        
        assert_eq!(rt.total_steps(), 0);
        rt.step();
        assert_eq!(rt.total_steps(), 1);
    }

    #[test]
    fn test_record_progress() {
        let mut rt = Runtime::with_seed(42);
        rt.spawn(async {});
        
        // Should not panic
        rt.record_progress(0);
    }

    #[test]
    fn test_record_wait_release() {
        let mut rt = Runtime::with_seed(42);
        
        rt.record_wait(1, 2);
        rt.record_release(1, 2);
        // Should not panic or detect false deadlock
    }
}
