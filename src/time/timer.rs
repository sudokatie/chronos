//! Timer wheel for scheduling wake-ups at specific instants.

use std::collections::BTreeMap;
use std::task::Waker;

use super::Instant;

/// Unique identifier for a timer.
pub type TimerId = u64;

/// A timer wheel that tracks deadlines and their associated wakers.
///
/// Uses a BTreeMap to keep deadlines sorted, allowing efficient
/// lookup of the next deadline and firing of expired timers.
#[derive(Debug)]
pub struct TimerWheel {
    /// Map from deadline to (timer_id, waker) pairs.
    deadlines: BTreeMap<Instant, Vec<(TimerId, Waker)>>,
    /// Next timer ID to assign.
    next_id: TimerId,
    /// Map from timer ID to its deadline (for efficient cancel).
    id_to_deadline: std::collections::HashMap<TimerId, Instant>,
}

impl TimerWheel {
    /// Creates a new empty timer wheel.
    pub fn new() -> Self {
        Self {
            deadlines: BTreeMap::new(),
            next_id: 0,
            id_to_deadline: std::collections::HashMap::new(),
        }
    }

    /// Schedules a waker to be called at the given deadline.
    ///
    /// Returns a TimerId that can be used to cancel the timer.
    pub fn schedule(&mut self, deadline: Instant, waker: Waker) -> TimerId {
        let id = self.next_id;
        self.next_id += 1;

        self.deadlines
            .entry(deadline)
            .or_insert_with(Vec::new)
            .push((id, waker));
        self.id_to_deadline.insert(id, deadline);

        id
    }

    /// Cancels a previously scheduled timer.
    ///
    /// Returns true if the timer was found and cancelled,
    /// false if it was already fired or never existed.
    pub fn cancel(&mut self, id: TimerId) -> bool {
        let Some(deadline) = self.id_to_deadline.remove(&id) else {
            return false;
        };
        
        let Some(timers) = self.deadlines.get_mut(&deadline) else {
            return false;
        };
        
        let original_len = timers.len();
        timers.retain(|(timer_id, _)| *timer_id != id);
        let cancelled = timers.len() < original_len;
        
        // Remove the deadline entry if no more timers
        if timers.is_empty() {
            self.deadlines.remove(&deadline);
        }
        
        cancelled
    }

    /// Returns the next deadline, or None if no timers are scheduled.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.deadlines.keys().next().copied()
    }

    /// Fires all timers that have expired (deadline <= now).
    ///
    /// Returns the wakers that should be called.
    /// The timers are removed from the wheel.
    pub fn fire_expired(&mut self, now: Instant) -> Vec<Waker> {
        let mut wakers = Vec::new();
        
        // Collect all deadlines that have passed
        let expired: Vec<Instant> = self
            .deadlines
            .range(..=now)
            .map(|(k, _)| *k)
            .collect();

        for deadline in expired {
            if let Some(timers) = self.deadlines.remove(&deadline) {
                for (id, waker) in timers {
                    self.id_to_deadline.remove(&id);
                    wakers.push(waker);
                }
            }
        }

        wakers
    }

    /// Returns the number of pending timers.
    pub fn len(&self) -> usize {
        self.id_to_deadline.len()
    }

    /// Returns true if no timers are scheduled.
    pub fn is_empty(&self) -> bool {
        self.id_to_deadline.is_empty()
    }
}

impl Default for TimerWheel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::task::{RawWaker, RawWakerVTable};

    fn noop_waker() -> Waker {
        const VTABLE: RawWakerVTable = RawWakerVTable::new(
            |_| RawWaker::new(std::ptr::null(), &VTABLE),
            |_| {},
            |_| {},
            |_| {},
        );
        unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) }
    }

    fn tracking_waker(flag: Arc<AtomicBool>) -> Waker {
        struct TrackingWaker(Arc<AtomicBool>);
        
        impl std::task::Wake for TrackingWaker {
            fn wake(self: Arc<Self>) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        
        Waker::from(Arc::new(TrackingWaker(flag)))
    }

    #[test]
    fn test_new_timer_wheel() {
        let wheel = TimerWheel::new();
        assert!(wheel.is_empty());
        assert_eq!(wheel.len(), 0);
        assert_eq!(wheel.next_deadline(), None);
    }

    #[test]
    fn test_schedule_returns_unique_ids() {
        let mut wheel = TimerWheel::new();
        let id1 = wheel.schedule(Instant::from_nanos(100), noop_waker());
        let id2 = wheel.schedule(Instant::from_nanos(200), noop_waker());
        let id3 = wheel.schedule(Instant::from_nanos(100), noop_waker());
        
        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_next_deadline() {
        let mut wheel = TimerWheel::new();
        
        wheel.schedule(Instant::from_nanos(300), noop_waker());
        assert_eq!(wheel.next_deadline(), Some(Instant::from_nanos(300)));
        
        wheel.schedule(Instant::from_nanos(100), noop_waker());
        assert_eq!(wheel.next_deadline(), Some(Instant::from_nanos(100)));
        
        wheel.schedule(Instant::from_nanos(200), noop_waker());
        assert_eq!(wheel.next_deadline(), Some(Instant::from_nanos(100)));
    }

    #[test]
    fn test_fire_expired_returns_wakers() {
        let mut wheel = TimerWheel::new();
        
        let flag1 = Arc::new(AtomicBool::new(false));
        let flag2 = Arc::new(AtomicBool::new(false));
        
        wheel.schedule(Instant::from_nanos(100), tracking_waker(flag1.clone()));
        wheel.schedule(Instant::from_nanos(200), tracking_waker(flag2.clone()));
        
        let wakers = wheel.fire_expired(Instant::from_nanos(150));
        assert_eq!(wakers.len(), 1);
        
        // Wake and check
        for w in wakers {
            w.wake();
        }
        assert!(flag1.load(Ordering::SeqCst));
        assert!(!flag2.load(Ordering::SeqCst));
    }

    #[test]
    fn test_fire_expired_removes_entries() {
        let mut wheel = TimerWheel::new();
        
        wheel.schedule(Instant::from_nanos(100), noop_waker());
        wheel.schedule(Instant::from_nanos(200), noop_waker());
        assert_eq!(wheel.len(), 2);
        
        wheel.fire_expired(Instant::from_nanos(100));
        assert_eq!(wheel.len(), 1);
        
        wheel.fire_expired(Instant::from_nanos(200));
        assert_eq!(wheel.len(), 0);
        assert!(wheel.is_empty());
    }

    #[test]
    fn test_fire_expired_multiple_same_deadline() {
        let mut wheel = TimerWheel::new();
        
        wheel.schedule(Instant::from_nanos(100), noop_waker());
        wheel.schedule(Instant::from_nanos(100), noop_waker());
        wheel.schedule(Instant::from_nanos(100), noop_waker());
        
        let wakers = wheel.fire_expired(Instant::from_nanos(100));
        assert_eq!(wakers.len(), 3);
        assert!(wheel.is_empty());
    }

    #[test]
    fn test_cancel() {
        let mut wheel = TimerWheel::new();
        
        let id1 = wheel.schedule(Instant::from_nanos(100), noop_waker());
        let id2 = wheel.schedule(Instant::from_nanos(200), noop_waker());
        
        assert!(wheel.cancel(id1));
        assert_eq!(wheel.len(), 1);
        assert_eq!(wheel.next_deadline(), Some(Instant::from_nanos(200)));
        
        // Cancel again should return false
        assert!(!wheel.cancel(id1));
        
        // Cancel id2
        assert!(wheel.cancel(id2));
        assert!(wheel.is_empty());
    }

    #[test]
    fn test_cancel_after_fire() {
        let mut wheel = TimerWheel::new();
        
        let id = wheel.schedule(Instant::from_nanos(100), noop_waker());
        wheel.fire_expired(Instant::from_nanos(100));
        
        // Timer already fired, cancel should return false
        assert!(!wheel.cancel(id));
    }

    #[test]
    fn test_cancel_nonexistent() {
        let mut wheel = TimerWheel::new();
        assert!(!wheel.cancel(999));
    }
}
