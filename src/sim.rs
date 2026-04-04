//! User-facing simulation APIs.
//!
//! This module provides the public interface for writing simulation tests.
//! All non-determinism is controlled through these APIs.

use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;


use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use crate::network::{NetworkConfig, NetworkSim};
use crate::recording::{Event, Header, RecordingWriter};
use crate::runtime::TaskHandle;
use crate::time::{Clock, Instant, TimerId, TimerWheel};
use crate::{NodeId, TaskId};

thread_local! {
    static SIM_CONTEXT: RefCell<Option<Arc<SimContextInner>>> = RefCell::new(None);
}

/// A spawned task stored in the simulation context.
struct SpawnedTask {
    id: TaskId,
    future: Mutex<Pin<Box<dyn Future<Output = ()> + Send>>>,
    handle: TaskHandle,
    waker: Mutex<Option<Waker>>,
}

/// Replay state for feeding recorded values back during execution.
#[allow(dead_code)]
#[derive(Default)]
struct ReplayState {
    /// Whether we're in replay mode.
    active: bool,
    /// Recorded random values to feed back.
    random_values: Vec<u64>,
    /// Current index into random_values.
    random_index: usize,
    /// Recorded schedule decisions (chosen task, ready tasks).
    schedule_decisions: Vec<(TaskId, Vec<TaskId>)>,
    /// Current index into schedule_decisions.
    schedule_index: usize,
    /// Verify that execution matches recording.
    verify: bool,
    /// Mismatches found during verification.
    mismatches: Vec<String>,
}

/// Internal simulation state.
struct SimContextInner {
    clock: Clock,
    timers: Mutex<TimerWheel>,
    rng: Mutex<StdRng>,
    network: Mutex<NetworkSim>,
    seed: u64,
    current_task: Mutex<TaskId>,
    recorder: Mutex<Option<RecordingWriter>>,
    /// Registered always-assertions
    always_assertions: Mutex<Vec<AlwaysAssertion>>,
    /// Registered eventually-assertions
    eventually_assertions: Mutex<Vec<EventuallyAssertion>>,
    /// Virtual filesystem
    vfs: Mutex<VirtualFs>,
    /// Pending wakers for timer-based sleeps
    timer_wakers: Mutex<HashMap<TimerId, Waker>>,
    /// Spawned tasks
    spawned_tasks: Mutex<Vec<Arc<SpawnedTask>>>,
    /// Next task ID
    next_task_id: Mutex<TaskId>,
    /// Replay state (if replaying from recording).
    replay: Mutex<ReplayState>,
}

struct AlwaysAssertion {
    condition: Box<dyn Fn() -> bool + Send + Sync>,
    message: String,
    failed: bool,
}

struct EventuallyAssertion {
    condition: Box<dyn Fn() -> bool + Send + Sync>,
    message: String,
    satisfied: bool,
    deadline: Option<Instant>,
}

struct VirtualFs {
    files: HashMap<std::path::PathBuf, Vec<u8>>,
    read_fail_rate: f64,
    write_fail_rate: f64,
}

impl VirtualFs {
    fn new() -> Self {
        Self {
            files: HashMap::new(),
            read_fail_rate: 0.0,
            write_fail_rate: 0.0,
        }
    }
}

/// Simulation context holding all controlled state.
#[derive(Clone)]
pub struct SimContext {
    inner: Arc<SimContextInner>,
}

impl SimContext {
    /// Create a new simulation context with the given seed.
    pub fn new(seed: u64) -> Self {
        let network_config = NetworkConfig::default();
        Self {
            inner: Arc::new(SimContextInner {
                clock: Clock::new(),
                timers: Mutex::new(TimerWheel::new()),
                rng: Mutex::new(StdRng::seed_from_u64(seed)),
                network: Mutex::new(NetworkSim::new(network_config, seed)),
                seed,
                current_task: Mutex::new(0),
                recorder: Mutex::new(None),
                always_assertions: Mutex::new(Vec::new()),
                eventually_assertions: Mutex::new(Vec::new()),
                vfs: Mutex::new(VirtualFs::new()),
                timer_wakers: Mutex::new(HashMap::new()),
                spawned_tasks: Mutex::new(Vec::new()),
                next_task_id: Mutex::new(0),
                replay: Mutex::new(ReplayState::default()),
            }),
        }
    }

    /// Create a simulation context with recording enabled.
    pub fn with_recording(seed: u64, path: &str) -> Self {
        let ctx = Self::new(seed);
        let header = Header::new(seed, 1); // 1 = Random strategy
        if let Ok(writer) = RecordingWriter::new(path, header) {
            *ctx.inner.recorder.lock().unwrap() = Some(writer);
        }
        ctx
    }

    /// Create a simulation context in replay mode from a recording.
    pub fn with_replay(path: &str, verify: bool) -> crate::Result<Self> {
        use crate::recording::{RecordingReader, EventPayload};
        
        let mut reader = RecordingReader::open(path)?;
        let seed = reader.seed();
        let ctx = Self::new(seed);
        
        // Load events and populate replay state
        let mut random_values = Vec::new();
        let mut schedule_decisions = Vec::new();
        
        while let Some(event) = reader.next_event()? {
            match event.payload {
                EventPayload::RandomGen { result } => {
                    random_values.push(result);
                }
                EventPayload::ScheduleDecision { chosen, ready } => {
                    schedule_decisions.push((chosen, ready));
                }
                _ => {}
            }
        }
        
        // Enable replay mode
        {
            let mut replay = ctx.inner.replay.lock().unwrap();
            replay.active = true;
            replay.random_values = random_values;
            replay.schedule_decisions = schedule_decisions;
            replay.verify = verify;
        }
        
        Ok(ctx)
    }

    /// Check if we're in replay mode.
    pub fn is_replay(&self) -> bool {
        self.inner.replay.lock().unwrap().active
    }

    /// Get any verification mismatches found during replay.
    pub fn replay_mismatches(&self) -> Vec<String> {
        self.inner.replay.lock().unwrap().mismatches.clone()
    }

    /// Get the next recorded random value (for replay mode).
    #[allow(dead_code)]
    fn next_replay_random(&self) -> Option<u64> {
        let mut replay = self.inner.replay.lock().unwrap();
        if !replay.active {
            return None;
        }
        if replay.random_index < replay.random_values.len() {
            let value = replay.random_values[replay.random_index];
            replay.random_index += 1;
            Some(value)
        } else {
            None
        }
    }

    /// Get the next recorded schedule decision (for replay mode).
    #[allow(dead_code)]
    fn next_replay_schedule(&self) -> Option<(TaskId, Vec<TaskId>)> {
        let mut replay = self.inner.replay.lock().unwrap();
        if !replay.active {
            return None;
        }
        if replay.schedule_index < replay.schedule_decisions.len() {
            let decision = replay.schedule_decisions[replay.schedule_index].clone();
            replay.schedule_index += 1;
            Some(decision)
        } else {
            None
        }
    }

    /// Record a mismatch during verification.
    #[allow(dead_code)]
    fn record_mismatch(&self, msg: String) {
        let mut replay = self.inner.replay.lock().unwrap();
        if replay.verify {
            replay.mismatches.push(msg);
        }
    }

    /// Get the seed used for this context.
    pub fn seed(&self) -> u64 {
        self.inner.seed
    }

    /// Get the current simulated time.
    pub fn now(&self) -> Instant {
        self.inner.clock.now()
    }

    /// Advance simulated time by the given duration.
    pub fn advance_time(&self, duration: Duration) {
        self.inner.clock.advance(duration);
        self.fire_timers();
        self.tick_network();
        self.check_assertions();
    }

    /// Advance time to the next scheduled event.
    pub fn advance_to_next_event(&self) -> bool {
        let next_timer = self.inner.timers.lock().unwrap().next_deadline();
        let next_network = self.inner.network.lock().unwrap().next_event_time();
        
        let next = match (next_timer, next_network) {
            (Some(t), Some(n)) => Some(t.min(n)),
            (Some(t), None) => Some(t),
            (None, Some(n)) => Some(n),
            (None, None) => None,
        };

        if let Some(deadline) = next {
            let now = self.inner.clock.now();
            if deadline > now {
                self.inner.clock.advance_to(deadline);
                self.fire_timers();
                self.tick_network();
                self.check_assertions();
                return true;
            }
        }
        false
    }

    fn fire_timers(&self) {
        let now = self.inner.clock.now();
        let wakers = self.inner.timers.lock().unwrap().fire_expired(now);
        
        // Wake all expired timer futures
        let mut timer_wakers = self.inner.timer_wakers.lock().unwrap();
        for waker in wakers {
            waker.wake();
        }
        // Clean up fired timers
        timer_wakers.retain(|_, _| true); // Will be cleaned by cancel
    }

    fn tick_network(&self) {
        let now = self.inner.clock.now();
        self.inner.network.lock().unwrap().tick(now);
    }

    fn check_assertions(&self) {
        // Check always-assertions
        let mut always = self.inner.always_assertions.lock().unwrap();
        for assertion in always.iter_mut() {
            if !assertion.failed && !(assertion.condition)() {
                assertion.failed = true;
                panic!("chronos::assert_always failed: {}", assertion.message);
            }
        }

        // Check eventually-assertions for timeout
        let now = self.inner.clock.now();
        let mut eventually = self.inner.eventually_assertions.lock().unwrap();
        for assertion in eventually.iter_mut() {
            if !assertion.satisfied {
                if (assertion.condition)() {
                    assertion.satisfied = true;
                } else if let Some(deadline) = assertion.deadline {
                    if now >= deadline {
                        panic!("chronos::assert_eventually timed out: {}", assertion.message);
                    }
                }
            }
        }
    }

    /// Record an event if recording is enabled.
    pub fn record_event(&self, event: Event) {
        if let Some(ref mut writer) = *self.inner.recorder.lock().unwrap() {
            let _ = writer.write_event(&event);
        }
    }

    /// Get current task ID.
    pub fn current_task(&self) -> TaskId {
        *self.inner.current_task.lock().unwrap()
    }

    /// Set current task ID.
    pub fn set_current_task(&self, task_id: TaskId) {
        *self.inner.current_task.lock().unwrap() = task_id;
    }

    /// Install this context as the current thread-local context.
    pub fn install(&self) {
        SIM_CONTEXT.with(|ctx| {
            *ctx.borrow_mut() = Some(self.inner.clone());
        });
    }

    /// Remove the current thread-local context.
    pub fn uninstall() {
        SIM_CONTEXT.with(|ctx| {
            *ctx.borrow_mut() = None;
        });
    }

    /// Get the network simulator.
    pub fn network(&self) -> &Mutex<NetworkSim> {
        &self.inner.network
    }

    /// Get the timer wheel.
    pub fn timers(&self) -> &Mutex<TimerWheel> {
        &self.inner.timers
    }

    /// Finish recording and flush to disk.
    pub fn finish_recording(&self) {
        if let Some(writer) = self.inner.recorder.lock().unwrap().take() {
            let _ = writer.finish();
        }
    }
}

impl Drop for SimContext {
    fn drop(&mut self) {
        // Only finish if we're the last reference
        if Arc::strong_count(&self.inner) == 1 {
            self.finish_recording();
        }
    }
}

/// Check if we're running inside a simulation.
pub fn is_simulation() -> bool {
    SIM_CONTEXT.with(|ctx| ctx.borrow().is_some())
}

/// Check if we're in replay mode.
pub fn is_replay() -> bool {
    try_with_context(|ctx| ctx.replay.lock().unwrap().active).unwrap_or(false)
}

/// Get any replay verification mismatches.
pub fn replay_mismatches() -> Vec<String> {
    try_with_context(|ctx| ctx.replay.lock().unwrap().mismatches.clone()).unwrap_or_default()
}

fn with_context<T, F: FnOnce(&SimContextInner) -> T>(f: F) -> T {
    SIM_CONTEXT.with(|ctx| {
        let ctx = ctx.borrow();
        let ctx = ctx.as_ref().expect("not running in simulation context");
        f(ctx)
    })
}

fn try_with_context<T, F: FnOnce(&SimContextInner) -> T>(f: F) -> Option<T> {
    SIM_CONTEXT.with(|ctx| {
        ctx.borrow().as_ref().map(|c| f(c))
    })
}

/// Spawn a task in the current simulation context.
///
/// Returns a handle that can be used to track task completion.
pub fn spawn_task<F>(future: F) -> TaskHandle
where
    F: Future<Output = ()> + Send + 'static,
{
    with_context(|ctx| {
        // Get next task ID
        let task_id = {
            let mut next_id = ctx.next_task_id.lock().unwrap();
            let id = *next_id;
            *next_id += 1;
            id
        };

        // Create task handle
        let handle = TaskHandle::new(task_id);

        // Create spawned task
        let task = Arc::new(SpawnedTask {
            id: task_id,
            future: Mutex::new(Box::pin(future)),
            handle: handle.clone(),
            waker: Mutex::new(None),
        });

        // Record spawn event
        let timestamp = ctx.clock.now().as_nanos();
        let parent = *ctx.current_task.lock().unwrap();
        if let Some(ref mut writer) = *ctx.recorder.lock().unwrap() {
            let _ = writer.write_event(&crate::recording::Event::task_spawn(
                task_id, parent, format!("task_{}", task_id), timestamp
            ));
        }

        // Add to spawned tasks
        ctx.spawned_tasks.lock().unwrap().push(task);

        handle
    })
}

/// Run all spawned tasks until they complete or block.
///
/// This is called internally by the simulation to make progress.
pub fn run_spawned_tasks() {
    with_context(|ctx| {
        let tasks = ctx.spawned_tasks.lock().unwrap().clone();
        
        for task in tasks {
            if task.handle.is_complete() {
                continue;
            }

            // Set current task
            *ctx.current_task.lock().unwrap() = task.id;

            // Create waker
            let task_clone = task.clone();
            let waker = futures::task::waker(Arc::new(TaskWaker { task: task_clone }));
            *task.waker.lock().unwrap() = Some(waker.clone());

            let mut cx = Context::from_waker(&waker);
            let mut future = task.future.lock().unwrap();

            match future.as_mut().poll(&mut cx) {
                Poll::Ready(()) => {
                    task.handle.mark_complete();
                    
                    // Record completion
                    let timestamp = ctx.clock.now().as_nanos();
                    if let Some(ref mut writer) = *ctx.recorder.lock().unwrap() {
                        let _ = writer.write_event(&crate::recording::Event::task_complete(
                            task.id, timestamp
                        ));
                    }
                }
                Poll::Pending => {
                    // Task yielded - record it
                    let timestamp = ctx.clock.now().as_nanos();
                    if let Some(ref mut writer) = *ctx.recorder.lock().unwrap() {
                        let _ = writer.write_event(&crate::recording::Event::task_yield(
                            task.id, timestamp
                        ));
                    }
                }
            }
        }

        // Reset current task
        *ctx.current_task.lock().unwrap() = 0;
    });
}

/// Waker implementation for spawned tasks.
struct TaskWaker {
    task: Arc<SpawnedTask>,
}

impl futures::task::ArcWake for TaskWaker {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        // Task was woken - it will be polled again in the next run_spawned_tasks call
        let _ = arc_self.task.id; // Just reference to avoid unused warning
    }
}

/// Check all registered assertions (called from runtime coordinator).
///
/// This checks always-assertions and eventually-assertion timeouts.
/// Panics if any always-assertion fails or eventually-assertion times out.
pub fn check_assertions() {
    try_with_context(|ctx| {
        // Check always-assertions
        let mut always = ctx.always_assertions.lock().unwrap();
        for assertion in always.iter_mut() {
            if !assertion.failed && !(assertion.condition)() {
                assertion.failed = true;
                panic!("chronos::assert_always failed: {}", assertion.message);
            }
        }

        // Check eventually-assertions for timeout
        let now = ctx.clock.now();
        let mut eventually = ctx.eventually_assertions.lock().unwrap();
        for assertion in eventually.iter_mut() {
            if !assertion.satisfied {
                if (assertion.condition)() {
                    assertion.satisfied = true;
                } else if let Some(deadline) = assertion.deadline {
                    if now >= deadline {
                        panic!("chronos::assert_eventually timed out: {}", assertion.message);
                    }
                }
            }
        }
    });
}

// ============================================================================
// Time APIs
// ============================================================================

/// Time control APIs.
pub mod time {
    use super::*;

    /// Returns the current simulated time.
    pub fn now() -> Instant {
        with_context(|ctx| {
            let result = ctx.clock.now();
            // Record event
            let task_id = *ctx.current_task.lock().unwrap();
            if let Some(ref mut writer) = *ctx.recorder.lock().unwrap() {
                let _ = writer.write_event(&Event::time_query(task_id, result.as_nanos(), result.as_nanos()));
            }
            result
        })
    }

    /// Returns the elapsed time since simulation start.
    pub fn elapsed() -> Duration {
        with_context(|ctx| ctx.clock.elapsed())
    }

    /// A future that completes after the given duration of simulated time.
    pub fn sleep(duration: Duration) -> Sleep {
        let deadline = now().saturating_add(duration);
        Sleep::new(deadline)
    }

    /// A future that completes at the given instant.
    pub fn sleep_until(deadline: Instant) -> Sleep {
        Sleep::new(deadline)
    }

    /// Future returned by `sleep` and `sleep_until`.
    pub struct Sleep {
        deadline: Instant,
        timer_id: Option<TimerId>,
    }

    impl Sleep {
        fn new(deadline: Instant) -> Self {
            Self {
                deadline,
                timer_id: None,
            }
        }
    }

    impl Future for Sleep {
        type Output = ();

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            let current = now();
            if current >= self.deadline {
                // Cancel timer if registered
                if let Some(timer_id) = self.timer_id.take() {
                    let _ = try_with_context(|ctx| {
                        ctx.timers.lock().unwrap().cancel(timer_id);
                        ctx.timer_wakers.lock().unwrap().remove(&timer_id);
                    });
                }
                return Poll::Ready(());
            }

            // Register with timer wheel if not already
            if self.timer_id.is_none() {
                let timer_id = with_context(|ctx| {
                    let id = ctx.timers.lock().unwrap().schedule(self.deadline, cx.waker().clone());
                    ctx.timer_wakers.lock().unwrap().insert(id, cx.waker().clone());
                    id
                });
                self.timer_id = Some(timer_id);
            }

            Poll::Pending
        }
    }

    impl Drop for Sleep {
        fn drop(&mut self) {
            if let Some(timer_id) = self.timer_id.take() {
                let _ = try_with_context(|ctx| {
                    ctx.timers.lock().unwrap().cancel(timer_id);
                    ctx.timer_wakers.lock().unwrap().remove(&timer_id);
                });
            }
        }
    }

    /// Yield control to the scheduler without advancing time.
    pub async fn yield_now() {
        YieldNow { yielded: false }.await
    }

    struct YieldNow {
        yielded: bool,
    }

    impl Future for YieldNow {
        type Output = ();

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            if self.yielded {
                Poll::Ready(())
            } else {
                self.yielded = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }

    /// Advance simulation time (for test harnesses).
    pub fn advance(duration: Duration) {
        with_context(|ctx| {
            ctx.clock.advance(duration);
            // Fire timers
            let now = ctx.clock.now();
            let wakers = ctx.timers.lock().unwrap().fire_expired(now);
            for waker in wakers {
                waker.wake();
            }
            // Tick network
            ctx.network.lock().unwrap().tick(now);
        });
    }
}

// ============================================================================
// Random APIs
// ============================================================================

/// Deterministic random number generation.
pub mod random {
    use super::*;

    /// Generate a random value using the simulation's seeded RNG.
    pub fn gen<T>() -> T
    where
        rand::distributions::Standard: rand::distributions::Distribution<T>,
    {
        with_context(|ctx| {
            let mut rng = ctx.rng.lock().unwrap();
            let value: T = rng.gen();
            value
        })
    }

    /// Generate a random u64 and record it.
    /// In replay mode, returns the recorded value instead.
    pub fn gen_u64() -> u64 {
        with_context(|ctx| {
            // Check if we're in replay mode
            let replay_value = {
                let mut replay = ctx.replay.lock().unwrap();
                if replay.active && replay.random_index < replay.random_values.len() {
                    let value = replay.random_values[replay.random_index];
                    replay.random_index += 1;
                    Some(value)
                } else {
                    None
                }
            };

            if let Some(value) = replay_value {
                // In replay mode - return recorded value
                // Optionally verify it matches what RNG would produce
                let verify = ctx.replay.lock().unwrap().verify;
                if verify {
                    let mut rng = ctx.rng.lock().unwrap();
                    let fresh: u64 = rng.gen();
                    if fresh != value {
                        ctx.replay.lock().unwrap().mismatches.push(
                            format!("Random mismatch: expected {}, got {}", value, fresh)
                        );
                    }
                }
                value
            } else {
                // Normal mode - generate and record
                let mut rng = ctx.rng.lock().unwrap();
                let value: u64 = rng.gen();
                
                // Record event
                let task_id = *ctx.current_task.lock().unwrap();
                let timestamp = ctx.clock.now().as_nanos();
                if let Some(ref mut writer) = *ctx.recorder.lock().unwrap() {
                    let _ = writer.write_event(&Event::random_gen(task_id, timestamp, value));
                }
                
                value
            }
        })
    }

    /// Generate a random value in the given range.
    pub fn gen_range<T, R>(range: R) -> T
    where
        T: rand::distributions::uniform::SampleUniform,
        R: rand::distributions::uniform::SampleRange<T>,
    {
        with_context(|ctx| {
            let mut rng = ctx.rng.lock().unwrap();
            rng.gen_range(range)
        })
    }

    /// Generate random bytes.
    pub fn fill_bytes(dest: &mut [u8]) {
        with_context(|ctx| {
            let mut rng = ctx.rng.lock().unwrap();
            rng.fill(dest);
        })
    }

    /// Shuffle a slice randomly.
    pub fn shuffle<T>(slice: &mut [T]) {
        use rand::seq::SliceRandom;
        with_context(|ctx| {
            let mut rng = ctx.rng.lock().unwrap();
            slice.shuffle(&mut *rng);
        })
    }

    /// Choose a random element from a slice.
    pub fn choose<'a, T>(slice: &'a [T]) -> Option<&'a T> {
        use rand::seq::SliceRandom;
        with_context(|ctx| {
            let mut rng = ctx.rng.lock().unwrap();
            slice.choose(&mut *rng)
        })
    }

    /// Returns true with the given probability.
    pub fn chance(probability: f64) -> bool {
        gen::<f64>() < probability
    }

    /// Get the current seed.
    pub fn seed() -> u64 {
        with_context(|ctx| ctx.seed)
    }
}

// ============================================================================
// Network APIs
// ============================================================================

/// Simulated network APIs.
pub mod net {
    use super::*;
    

    /// A simulated network endpoint.
    #[derive(Debug, Clone)]
    pub struct Endpoint {
        node_id: NodeId,
    }

    impl Endpoint {
        /// Create a new endpoint for the given node.
        pub fn new(node_id: NodeId) -> Self {
            // Register node with network
            with_context(|ctx| {
                ctx.network.lock().unwrap().add_node(node_id);
            });
            Self { node_id }
        }

        /// Get the node ID for this endpoint.
        pub fn node_id(&self) -> NodeId {
            self.node_id
        }

        /// Connect to another endpoint.
        pub fn connect_to(&self, other: NodeId) {
            with_context(|ctx| {
                ctx.network.lock().unwrap().connect(self.node_id, other);
            });
        }

        /// Send data to another node.
        pub async fn send(&self, to: NodeId, data: Vec<u8>) -> crate::Result<()> {
            let now = time::now();
            let _data_len = data.len();
            
            with_context(|ctx| {
                let result = ctx.network.lock().unwrap().send(self.node_id, to, data.clone(), now);
                
                // Record event
                let task_id = *ctx.current_task.lock().unwrap();
                if let Some(ref mut writer) = *ctx.recorder.lock().unwrap() {
                    let _ = writer.write_event(&Event::net_send(task_id, now.as_nanos(), to, data));
                }
                
                result
            })?;

            // Yield to allow network simulation
            time::yield_now().await;
            Ok(())
        }

        /// Receive data from any node.
        pub async fn recv(&self) -> Option<(NodeId, Vec<u8>)> {
            loop {
                // Check for pending message
                let msg = with_context(|ctx| {
                    ctx.network.lock().unwrap().recv(self.node_id)
                });

                if let Some(msg) = msg {
                    // Record event
                    with_context(|ctx| {
                        let task_id = *ctx.current_task.lock().unwrap();
                        let timestamp = ctx.clock.now().as_nanos();
                        if let Some(ref mut writer) = *ctx.recorder.lock().unwrap() {
                            let _ = writer.write_event(&Event::net_recv(
                                task_id, timestamp, msg.from, msg.data.clone()
                            ));
                        }
                    });
                    return Some((msg.from, msg.data));
                }

                // Yield and wait for network tick
                time::yield_now().await;
                
                // Check if simulation should advance time
                if !is_simulation() {
                    return None;
                }
            }
        }

        /// Try to receive without blocking.
        pub fn try_recv(&self) -> Option<(NodeId, Vec<u8>)> {
            with_context(|ctx| {
                ctx.network.lock().unwrap().recv(self.node_id).map(|msg| {
                    // Record event
                    let task_id = *ctx.current_task.lock().unwrap();
                    let timestamp = ctx.clock.now().as_nanos();
                    if let Some(ref mut writer) = *ctx.recorder.lock().unwrap() {
                        let _ = writer.write_event(&Event::net_recv(
                            task_id, timestamp, msg.from, msg.data.clone()
                        ));
                    }
                    (msg.from, msg.data)
                })
            })
        }

        /// Check if there are pending messages.
        pub fn has_pending(&self) -> bool {
            with_context(|ctx| {
                ctx.network.lock().unwrap().inbox_len(self.node_id) > 0
            })
        }
    }

    /// A simulated TCP-like connection.
    pub struct Connection {
        local: NodeId,
        remote: NodeId,
        endpoint: Endpoint,
        connected: bool,
    }

    impl Connection {
        /// Connect to a remote node.
        pub async fn connect(local: NodeId, remote: NodeId) -> crate::Result<Self> {
            let endpoint = Endpoint::new(local);
            endpoint.connect_to(remote);
            
            time::yield_now().await;
            
            Ok(Self {
                local,
                remote,
                endpoint,
                connected: true,
            })
        }

        /// Send data over the connection.
        pub async fn send(&self, data: &[u8]) -> crate::Result<()> {
            if !self.connected {
                return Err(crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::NotConnected,
                    "connection closed",
                )));
            }
            self.endpoint.send(self.remote, data.to_vec()).await
        }

        /// Receive data from the connection.
        pub async fn recv(&self) -> crate::Result<Vec<u8>> {
            if !self.connected {
                return Err(crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::NotConnected,
                    "connection closed",
                )));
            }
            
            loop {
                if let Some((from, data)) = self.endpoint.try_recv() {
                    if from == self.remote {
                        return Ok(data);
                    }
                }
                time::yield_now().await;
            }
        }

        /// Close the connection.
        pub fn close(&mut self) {
            self.connected = false;
        }

        /// Check if connected.
        pub fn is_connected(&self) -> bool {
            self.connected
        }

        /// Get local node ID.
        pub fn local(&self) -> NodeId {
            self.local
        }

        /// Get remote node ID.
        pub fn remote(&self) -> NodeId {
            self.remote
        }
    }

    /// Partition the network.
    pub fn partition(groups: &[&[NodeId]]) {
        with_context(|ctx| {
            let groups_vec: Vec<Vec<NodeId>> = groups.iter().map(|g| g.to_vec()).collect();
            ctx.network.lock().unwrap().partition(groups_vec);
        });
    }

    /// Heal all network partitions.
    pub fn heal() {
        with_context(|ctx| {
            ctx.network.lock().unwrap().heal();
        });
    }

    /// Check if two nodes can communicate.
    pub fn can_communicate(a: NodeId, b: NodeId) -> bool {
        with_context(|ctx| {
            ctx.network.lock().unwrap().can_communicate(a, b)
        })
    }
}

// ============================================================================
// Filesystem APIs
// ============================================================================

/// Simulated filesystem APIs.
pub mod fs {
    use super::*;
    use std::path::{Path, PathBuf};

    /// Read a file from the virtual filesystem.
    pub async fn read<P: AsRef<Path>>(path: P) -> crate::Result<Vec<u8>> {
        time::yield_now().await;
        
        with_context(|ctx| {
            let vfs = ctx.vfs.lock().unwrap();
            
            // Check for fault injection
            if vfs.read_fail_rate > 0.0 {
                drop(vfs);
                if random::chance(ctx.vfs.lock().unwrap().read_fail_rate) {
                    return Err(crate::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "simulated read error",
                    )));
                }
            }

            let vfs = ctx.vfs.lock().unwrap();
            vfs.files
                .get(path.as_ref())
                .cloned()
                .ok_or_else(|| {
                    crate::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("file not found: {:?}", path.as_ref()),
                    ))
                })
        })
    }

    /// Write data to a file in the virtual filesystem.
    pub async fn write<P: AsRef<Path>>(path: P, data: Vec<u8>) -> crate::Result<()> {
        time::yield_now().await;

        with_context(|ctx| {
            // Check for fault injection
            {
                let vfs = ctx.vfs.lock().unwrap();
                if vfs.write_fail_rate > 0.0 {
                    drop(vfs);
                    if random::chance(ctx.vfs.lock().unwrap().write_fail_rate) {
                        return Err(crate::Error::Io(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "simulated write error",
                        )));
                    }
                }
            }

            let mut vfs = ctx.vfs.lock().unwrap();
            vfs.files.insert(path.as_ref().to_path_buf(), data);
            Ok(())
        })
    }

    /// Check if a file exists.
    pub fn exists<P: AsRef<Path>>(path: P) -> bool {
        with_context(|ctx| {
            ctx.vfs.lock().unwrap().files.contains_key(path.as_ref())
        })
    }

    /// Remove a file.
    pub async fn remove<P: AsRef<Path>>(path: P) -> crate::Result<()> {
        time::yield_now().await;
        with_context(|ctx| {
            ctx.vfs.lock().unwrap().files.remove(path.as_ref());
        });
        Ok(())
    }

    /// List all files in the virtual filesystem.
    pub fn list() -> Vec<PathBuf> {
        with_context(|ctx| {
            ctx.vfs.lock().unwrap().files.keys().cloned().collect()
        })
    }

    /// Reset the virtual filesystem.
    pub fn reset() {
        let _ = try_with_context(|ctx| {
            let mut vfs = ctx.vfs.lock().unwrap();
            vfs.files.clear();
            vfs.read_fail_rate = 0.0;
            vfs.write_fail_rate = 0.0;
        });
    }

    /// Set read failure rate (0.0 - 1.0).
    pub fn set_read_failure_rate(rate: f64) {
        with_context(|ctx| {
            ctx.vfs.lock().unwrap().read_fail_rate = rate.clamp(0.0, 1.0);
        });
    }

    /// Set write failure rate (0.0 - 1.0).
    pub fn set_write_failure_rate(rate: f64) {
        with_context(|ctx| {
            ctx.vfs.lock().unwrap().write_fail_rate = rate.clamp(0.0, 1.0);
        });
    }
}

// ============================================================================
// Assertion APIs
// ============================================================================

/// Assertion APIs for simulation tests.
pub mod assertions {
    use super::*;

    /// Register an always-assertion that must hold throughout the simulation.
    pub fn register_always<F>(condition: F, message: &str)
    where
        F: Fn() -> bool + Send + Sync + 'static,
    {
        with_context(|ctx| {
            ctx.always_assertions.lock().unwrap().push(AlwaysAssertion {
                condition: Box::new(condition),
                message: message.to_string(),
                failed: false,
            });
        });
    }

    /// Register an eventually-assertion that must become true.
    pub fn register_eventually<F>(condition: F, message: &str, timeout: Option<Duration>)
    where
        F: Fn() -> bool + Send + Sync + 'static,
    {
        let deadline = timeout.map(|d| {
            with_context(|ctx| ctx.clock.now().saturating_add(d))
        });

        with_context(|ctx| {
            ctx.eventually_assertions.lock().unwrap().push(EventuallyAssertion {
                condition: Box::new(condition),
                message: message.to_string(),
                satisfied: false,
                deadline,
            });
        });
    }

    /// Check all eventually-assertions are satisfied.
    pub fn verify_eventually() -> Result<(), String> {
        with_context(|ctx| {
            let eventually = ctx.eventually_assertions.lock().unwrap();
            for assertion in eventually.iter() {
                if !assertion.satisfied && !(assertion.condition)() {
                    return Err(format!("assert_eventually not satisfied: {}", assertion.message));
                }
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sim_context() {
        let ctx = SimContext::new(42);
        ctx.install();
        
        assert!(is_simulation());
        assert_eq!(time::now(), Instant::from_nanos(0));
        
        SimContext::uninstall();
        assert!(!is_simulation());
    }

    #[test]
    fn test_random_deterministic() {
        let ctx = SimContext::new(12345);
        ctx.install();
        
        let v1: u64 = random::gen();
        let v2: u64 = random::gen();
        
        SimContext::uninstall();
        
        let ctx2 = SimContext::new(12345);
        ctx2.install();
        
        let v3: u64 = random::gen();
        let v4: u64 = random::gen();
        
        SimContext::uninstall();
        
        assert_eq!(v1, v3);
        assert_eq!(v2, v4);
    }

    #[test]
    fn test_random_range() {
        let ctx = SimContext::new(42);
        ctx.install();
        
        for _ in 0..100 {
            let v = random::gen_range(10..20);
            assert!(v >= 10 && v < 20);
        }
        
        SimContext::uninstall();
    }

    #[test]
    fn test_time_advance() {
        let ctx = SimContext::new(42);
        ctx.install();
        
        assert_eq!(time::now(), Instant::from_nanos(0));
        time::advance(Duration::from_secs(1));
        assert_eq!(time::now().as_nanos(), 1_000_000_000);
        
        SimContext::uninstall();
    }

    #[test]
    fn test_virtual_fs() {
        fs::reset();
        
        let ctx = SimContext::new(42);
        ctx.install();
        
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        
        rt.block_on(async {
            fs::write("test.txt", b"hello".to_vec()).await.unwrap();
            assert!(fs::exists("test.txt"));
            
            let data = fs::read("test.txt").await.unwrap();
            assert_eq!(data, b"hello");
            
            fs::remove("test.txt").await.unwrap();
            assert!(!fs::exists("test.txt"));
        });
        
        SimContext::uninstall();
        fs::reset();
    }

    #[test]
    fn test_network_endpoint() {
        let ctx = SimContext::new(42);
        ctx.install();
        
        let ep1 = net::Endpoint::new(1);
        let _ep2 = net::Endpoint::new(2);
        ep1.connect_to(2);
        
        assert!(net::can_communicate(1, 2));
        
        SimContext::uninstall();
    }

    #[test]
    fn test_network_partition() {
        let ctx = SimContext::new(42);
        ctx.install();
        
        let _ep1 = net::Endpoint::new(1);
        let _ep2 = net::Endpoint::new(2);
        
        net::partition(&[&[1], &[2]]);
        assert!(!net::can_communicate(1, 2));
        
        net::heal();
        assert!(net::can_communicate(1, 2));
        
        SimContext::uninstall();
    }

    #[test]
    fn test_random_seed() {
        let ctx = SimContext::new(99999);
        ctx.install();
        
        assert_eq!(random::seed(), 99999);
        
        SimContext::uninstall();
    }
}
