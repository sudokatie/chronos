//! Task representation and state tracking.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};

use crate::time::Instant;
use crate::{NodeId, TaskId};

/// A simulated task wrapping a future.
pub struct Task {
    id: TaskId,
    future: Pin<Box<dyn Future<Output = ()> + Send>>,
    state: TaskState,
    complete_flag: Arc<AtomicBool>,
}

impl Task {
    /// Creates a new task with the given ID and future.
    pub fn new<F>(id: TaskId, future: F) -> (Self, TaskHandle)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let complete_flag = Arc::new(AtomicBool::new(false));
        let handle = TaskHandle {
            id,
            complete: complete_flag.clone(),
        };
        let task = Self {
            id,
            future: Box::pin(future),
            state: TaskState::Ready,
            complete_flag,
        };
        (task, handle)
    }

    /// Returns the task's unique identifier.
    pub fn id(&self) -> TaskId {
        self.id
    }

    /// Returns the current task state.
    pub fn state(&self) -> &TaskState {
        &self.state
    }

    /// Sets the task state.
    pub fn set_state(&mut self, state: TaskState) {
        self.state = state;
    }

    /// Returns true if the task is ready to be polled.
    pub fn is_ready(&self) -> bool {
        matches!(self.state, TaskState::Ready)
    }

    /// Returns true if the task is blocked.
    pub fn is_blocked(&self) -> bool {
        matches!(self.state, TaskState::Blocked(_))
    }

    /// Returns true if the task has completed.
    pub fn is_complete(&self) -> bool {
        matches!(self.state, TaskState::Complete)
    }

    /// Polls the task's future.
    ///
    /// Updates state to Running during poll, then to Complete or back to Ready.
    pub fn poll(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        self.state = TaskState::Running;

        match self.future.as_mut().poll(cx) {
            Poll::Ready(()) => {
                self.state = TaskState::Complete;
                self.complete_flag.store(true, Ordering::SeqCst);
                Poll::Ready(())
            }
            Poll::Pending => {
                // State will be set by scheduler based on block reason
                self.state = TaskState::Ready;
                Poll::Pending
            }
        }
    }
}

impl std::fmt::Debug for Task {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Task")
            .field("id", &self.id)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

/// State of a simulated task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskState {
    /// Task is ready to run.
    Ready,
    /// Task is currently executing.
    Running,
    /// Task is blocked waiting for something.
    Blocked(BlockReason),
    /// Task has completed.
    Complete,
}

/// Reason a task is blocked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockReason {
    /// Blocked waiting for a specific time.
    Time(Instant),
    /// Blocked waiting for network from a specific node.
    Network(NodeId),
    /// Blocked on a channel operation.
    Channel,
    /// Blocked for a custom reason.
    Custom(String),
}

/// Handle to a spawned task.
#[derive(Clone)]
pub struct TaskHandle {
    id: TaskId,
    complete: Arc<AtomicBool>,
}

impl TaskHandle {
    /// Returns the task's unique identifier.
    pub fn id(&self) -> TaskId {
        self.id
    }

    /// Returns true if the task has completed.
    pub fn is_complete(&self) -> bool {
        self.complete.load(Ordering::SeqCst)
    }
}

impl std::fmt::Debug for TaskHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskHandle")
            .field("id", &self.id)
            .field("complete", &self.is_complete())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::task::{RawWaker, RawWakerVTable, Waker};

    fn noop_waker() -> Waker {
        const VTABLE: RawWakerVTable = RawWakerVTable::new(
            |_| RawWaker::new(std::ptr::null(), &VTABLE),
            |_| {},
            |_| {},
            |_| {},
        );
        unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) }
    }

    #[test]
    fn test_task_starts_ready() {
        let (task, _handle) = Task::new(1, async {});
        assert!(matches!(task.state(), TaskState::Ready));
        assert!(task.is_ready());
    }

    #[test]
    fn test_task_id() {
        let (task, handle) = Task::new(42, async {});
        assert_eq!(task.id(), 42);
        assert_eq!(handle.id(), 42);
    }

    #[test]
    fn test_poll_completes_immediate_future() {
        let (mut task, handle) = Task::new(1, async {});
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);

        assert!(!handle.is_complete());
        let result = task.poll(&mut cx);
        
        assert!(matches!(result, Poll::Ready(())));
        assert!(task.is_complete());
        assert!(handle.is_complete());
    }

    #[test]
    fn test_poll_pending_future() {
        use std::sync::atomic::AtomicUsize;
        
        let poll_count = Arc::new(AtomicUsize::new(0));
        let poll_count_clone = poll_count.clone();
        
        let future = std::future::poll_fn(move |_| {
            let count = poll_count_clone.fetch_add(1, Ordering::SeqCst);
            if count < 2 {
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        });

        let (mut task, handle) = Task::new(1, future);
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);

        // First poll - pending
        assert!(matches!(task.poll(&mut cx), Poll::Pending));
        assert!(!task.is_complete());
        assert!(!handle.is_complete());

        // Second poll - still pending
        assert!(matches!(task.poll(&mut cx), Poll::Pending));
        assert!(!task.is_complete());

        // Third poll - complete
        assert!(matches!(task.poll(&mut cx), Poll::Ready(())));
        assert!(task.is_complete());
        assert!(handle.is_complete());
    }

    #[test]
    fn test_set_state() {
        let (mut task, _) = Task::new(1, async {});
        
        task.set_state(TaskState::Blocked(BlockReason::Channel));
        assert!(task.is_blocked());
        assert!(matches!(task.state(), TaskState::Blocked(BlockReason::Channel)));
        
        task.set_state(TaskState::Ready);
        assert!(task.is_ready());
    }

    #[test]
    fn test_block_reasons() {
        let time_block = BlockReason::Time(Instant::from_nanos(1000));
        let network_block = BlockReason::Network(5);
        let channel_block = BlockReason::Channel;
        let custom_block = BlockReason::Custom("mutex".to_string());

        assert_eq!(time_block, BlockReason::Time(Instant::from_nanos(1000)));
        assert_ne!(time_block, BlockReason::Time(Instant::from_nanos(2000)));
        assert_ne!(network_block, channel_block);
        assert_eq!(custom_block, BlockReason::Custom("mutex".to_string()));
    }

    #[test]
    fn test_task_handle_clone() {
        let (mut task, handle1) = Task::new(1, async {});
        let handle2 = handle1.clone();

        assert_eq!(handle1.id(), handle2.id());
        assert!(!handle1.is_complete());
        assert!(!handle2.is_complete());

        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);
        task.poll(&mut cx);

        assert!(handle1.is_complete());
        assert!(handle2.is_complete());
    }

    #[test]
    fn test_task_debug() {
        let (task, handle) = Task::new(1, async {});
        let debug_str = format!("{:?}", task);
        assert!(debug_str.contains("Task"));
        assert!(debug_str.contains("id: 1"));
        
        let handle_debug = format!("{:?}", handle);
        assert!(handle_debug.contains("TaskHandle"));
    }
}
