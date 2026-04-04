//! Task abstraction for the Chronos runtime.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};

use crate::time::Instant;
use crate::TaskId;

/// Reason a task is blocked.
#[derive(Debug, Clone, PartialEq)]
pub enum BlockReason {
    /// Blocked on time (sleep).
    Time(Instant),
    /// Blocked on channel.
    Channel,
    /// Blocked on network I/O.
    Network,
    /// Blocked on disk I/O.
    Disk,
    /// Blocked on another task.
    Task(TaskId),
    /// Blocked on lock/mutex.
    Lock,
    /// Custom reason.
    Other(String),
}

/// A handle to a spawned task.
#[derive(Clone)]
pub struct TaskHandle {
    id: TaskId,
    complete: Arc<AtomicBool>,
}

impl TaskHandle {
    /// Create a new task handle.
    pub fn new(id: TaskId) -> Self {
        Self {
            id,
            complete: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Create a task handle that's already complete.
    pub fn completed(id: TaskId) -> Self {
        let handle = Self::new(id);
        handle.complete.store(true, Ordering::Release);
        handle
    }

    /// Get the task ID.
    pub fn id(&self) -> TaskId {
        self.id
    }

    /// Check if the task has completed.
    pub fn is_complete(&self) -> bool {
        self.complete.load(Ordering::Acquire)
    }

    /// Mark the task as complete.
    pub fn mark_complete(&self) {
        self.complete.store(true, Ordering::Release);
    }

    /// Wait for the task to complete.
    pub async fn join(self) {
        JoinFuture { handle: self }.await
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

/// Future that completes when a task is done.
struct JoinFuture {
    handle: TaskHandle,
}

impl Future for JoinFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.handle.is_complete() {
            Poll::Ready(())
        } else {
            // Schedule a wake-up
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

/// Internal task state.
pub struct Task {
    id: TaskId,
    future: Pin<Box<dyn Future<Output = ()> + Send>>,
    handle: TaskHandle,
    ready: bool,
}

impl Task {
    /// Create a new task wrapping a future.
    pub fn new<F>(id: TaskId, future: F) -> (Self, TaskHandle)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let handle = TaskHandle::new(id);
        let task = Self {
            id,
            future: Box::pin(future),
            handle: handle.clone(),
            ready: true,
        };
        (task, handle)
    }

    /// Get the task ID.
    pub fn id(&self) -> TaskId {
        self.id
    }

    /// Poll the task.
    pub fn poll(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        let result = self.future.as_mut().poll(cx);
        if result.is_ready() {
            self.handle.mark_complete();
        }
        result
    }

    /// Check if the task is ready to be polled.
    pub fn is_ready(&self) -> bool {
        self.ready && !self.is_complete()
    }

    /// Check if the task has completed.
    pub fn is_complete(&self) -> bool {
        self.handle.is_complete()
    }

    /// Mark the task as ready.
    pub fn set_ready(&mut self, ready: bool) {
        self.ready = ready;
    }

    /// Get the task handle.
    pub fn handle(&self) -> &TaskHandle {
        &self.handle
    }
}

impl std::fmt::Debug for Task {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Task")
            .field("id", &self.id)
            .field("ready", &self.ready)
            .field("complete", &self.is_complete())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_handle_new() {
        let handle = TaskHandle::new(42);
        assert_eq!(handle.id(), 42);
        assert!(!handle.is_complete());
    }

    #[test]
    fn test_task_handle_completed() {
        let handle = TaskHandle::completed(42);
        assert!(handle.is_complete());
    }

    #[test]
    fn test_task_handle_mark_complete() {
        let handle = TaskHandle::new(1);
        assert!(!handle.is_complete());
        handle.mark_complete();
        assert!(handle.is_complete());
    }

    #[test]
    fn test_task_handle_clone() {
        let handle1 = TaskHandle::new(1);
        let handle2 = handle1.clone();
        
        handle1.mark_complete();
        assert!(handle2.is_complete());
    }

    #[test]
    fn test_task_new() {
        let (task, handle) = Task::new(1, async {});
        
        assert_eq!(task.id(), 1);
        assert!(task.is_ready());
        assert!(!task.is_complete());
        assert!(!handle.is_complete());
    }

    #[test]
    fn test_task_poll_completes() {
        let (mut task, handle) = Task::new(1, async {});
        
        let waker = futures::task::noop_waker();
        let mut cx = Context::from_waker(&waker);
        
        let result = task.poll(&mut cx);
        assert!(result.is_ready());
        assert!(task.is_complete());
        assert!(handle.is_complete());
    }

    #[test]
    fn test_task_set_ready() {
        let (mut task, _) = Task::new(1, async {});
        
        assert!(task.is_ready());
        task.set_ready(false);
        assert!(!task.is_ready());
        task.set_ready(true);
        assert!(task.is_ready());
    }

    #[tokio::test]
    async fn test_task_handle_join() {
        let handle = TaskHandle::completed(1);
        handle.join().await;
        // Should complete immediately
    }
}
