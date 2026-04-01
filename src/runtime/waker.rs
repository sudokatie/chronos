//! Custom waker implementation for scheduler integration.

use std::cell::RefCell;
use std::sync::{Arc, Weak};
use std::task::{RawWaker, RawWakerVTable, Waker};

use crate::TaskId;

/// Trait for types that can be notified when a task should wake up.
pub trait WakeNotifier: Send + Sync {
    /// Called when a task should be marked as ready to run.
    fn notify_ready(&self, task_id: TaskId);
}

/// Data stored in the waker.
struct WakerData {
    task_id: TaskId,
    notifier: Weak<dyn WakeNotifier>,
}

const VTABLE: RawWakerVTable = RawWakerVTable::new(
    clone_waker,
    wake,
    wake_by_ref,
    drop_waker,
);

/// Creates a waker that will notify the given notifier when woken.
pub fn create_waker(task_id: TaskId, notifier: &Arc<dyn WakeNotifier>) -> Waker {
    let data = Box::new(WakerData {
        task_id,
        notifier: Arc::downgrade(notifier),
    });
    let raw = RawWaker::new(Box::into_raw(data) as *const (), &VTABLE);
    unsafe { Waker::from_raw(raw) }
}

unsafe fn clone_waker(data: *const ()) -> RawWaker {
    let original = &*(data as *const WakerData);
    let cloned = Box::new(WakerData {
        task_id: original.task_id,
        notifier: original.notifier.clone(),
    });
    RawWaker::new(Box::into_raw(cloned) as *const (), &VTABLE)
}

unsafe fn wake(data: *const ()) {
    let data = Box::from_raw(data as *mut WakerData);
    wake_impl(&data);
    // Box is dropped here
}

unsafe fn wake_by_ref(data: *const ()) {
    let data = &*(data as *const WakerData);
    wake_impl(data);
}

fn wake_impl(data: &WakerData) {
    if let Some(notifier) = data.notifier.upgrade() {
        notifier.notify_ready(data.task_id);
    }
    // If notifier is gone, silently ignore - scheduler was dropped
}

unsafe fn drop_waker(data: *const ()) {
    let _ = Box::from_raw(data as *mut WakerData);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    struct TestNotifier {
        ready_called: AtomicBool,
        last_task_id: AtomicU32,
    }

    impl TestNotifier {
        fn new() -> Self {
            Self {
                ready_called: AtomicBool::new(false),
                last_task_id: AtomicU32::new(0),
            }
        }

        fn was_called(&self) -> bool {
            self.ready_called.load(Ordering::SeqCst)
        }

        fn last_id(&self) -> TaskId {
            self.last_task_id.load(Ordering::SeqCst)
        }

        fn reset(&self) {
            self.ready_called.store(false, Ordering::SeqCst);
        }
    }

    impl WakeNotifier for TestNotifier {
        fn notify_ready(&self, task_id: TaskId) {
            self.ready_called.store(true, Ordering::SeqCst);
            self.last_task_id.store(task_id, Ordering::SeqCst);
        }
    }

    #[test]
    fn test_waker_creation() {
        let notifier: Arc<dyn WakeNotifier> = Arc::new(TestNotifier::new());
        let waker = create_waker(42, &notifier);
        // Just verify it doesn't panic
        drop(waker);
    }

    #[test]
    fn test_waker_wake() {
        let notifier = Arc::new(TestNotifier::new());
        let notifier_dyn: Arc<dyn WakeNotifier> = notifier.clone();
        let waker = create_waker(42, &notifier_dyn);

        assert!(!notifier.was_called());
        waker.wake();
        assert!(notifier.was_called());
        assert_eq!(notifier.last_id(), 42);
    }

    #[test]
    fn test_waker_wake_by_ref() {
        let notifier = Arc::new(TestNotifier::new());
        let notifier_dyn: Arc<dyn WakeNotifier> = notifier.clone();
        let waker = create_waker(42, &notifier_dyn);

        assert!(!notifier.was_called());
        waker.wake_by_ref();
        assert!(notifier.was_called());
        assert_eq!(notifier.last_id(), 42);

        // Can call again
        notifier.reset();
        waker.wake_by_ref();
        assert!(notifier.was_called());
    }

    #[test]
    fn test_waker_clone() {
        let notifier = Arc::new(TestNotifier::new());
        let notifier_dyn: Arc<dyn WakeNotifier> = notifier.clone();
        let waker1 = create_waker(42, &notifier_dyn);
        let waker2 = waker1.clone();

        // Both should work
        waker1.wake_by_ref();
        assert!(notifier.was_called());
        assert_eq!(notifier.last_id(), 42);

        notifier.reset();
        waker2.wake();
        assert!(notifier.was_called());
    }

    #[test]
    fn test_waker_with_dead_notifier() {
        let notifier: Arc<dyn WakeNotifier> = Arc::new(TestNotifier::new());
        let waker = create_waker(42, &notifier);
        
        // Drop the notifier
        drop(notifier);

        // Should not panic when woken
        waker.wake();
    }

    #[test]
    fn test_waker_drop() {
        let notifier: Arc<dyn WakeNotifier> = Arc::new(TestNotifier::new());
        let waker = create_waker(42, &notifier);
        let waker2 = waker.clone();
        
        drop(waker);
        drop(waker2);
        // Should not leak or crash
    }

    #[test]
    fn test_multiple_tasks() {
        let notifier = Arc::new(TestNotifier::new());
        let notifier_dyn: Arc<dyn WakeNotifier> = notifier.clone();
        
        let waker1 = create_waker(1, &notifier_dyn);
        let waker2 = create_waker(2, &notifier_dyn);
        let waker3 = create_waker(3, &notifier_dyn);

        waker1.wake_by_ref();
        assert_eq!(notifier.last_id(), 1);

        waker2.wake_by_ref();
        assert_eq!(notifier.last_id(), 2);

        waker3.wake_by_ref();
        assert_eq!(notifier.last_id(), 3);
    }
}
