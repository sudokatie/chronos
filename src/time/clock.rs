//! Virtual clock for controlling simulated time.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use super::Instant;

/// A virtual clock for deterministic time simulation.
///
/// The clock starts at zero and only advances when explicitly told to.
/// This allows complete control over time in simulations.
#[derive(Debug)]
pub struct Clock {
    current_nanos: AtomicU64,
}

impl Clock {
    /// Creates a new clock starting at time zero.
    pub fn new() -> Self {
        Self {
            current_nanos: AtomicU64::new(0),
        }
    }

    /// Creates a new clock starting at the given instant.
    pub fn starting_at(instant: Instant) -> Self {
        Self {
            current_nanos: AtomicU64::new(instant.as_nanos()),
        }
    }

    /// Returns the current simulated time.
    pub fn now(&self) -> Instant {
        Instant::from_nanos(self.current_nanos.load(Ordering::SeqCst))
    }

    /// Advances the clock by the given duration.
    pub fn advance(&self, duration: Duration) {
        self.current_nanos
            .fetch_add(duration.as_nanos() as u64, Ordering::SeqCst);
    }

    /// Advances the clock to the given instant.
    ///
    /// # Panics
    /// Panics if the instant is in the past.
    pub fn advance_to(&self, instant: Instant) {
        let current = self.current_nanos.load(Ordering::SeqCst);
        let target = instant.as_nanos();
        assert!(
            target >= current,
            "cannot advance clock backwards: current={}, target={}",
            current,
            target
        );
        self.current_nanos.store(target, Ordering::SeqCst);
    }

    /// Sets the clock to the given instant, even if it's in the past.
    ///
    /// Use with caution - this can break time consistency assumptions.
    pub fn set(&self, instant: Instant) {
        self.current_nanos.store(instant.as_nanos(), Ordering::SeqCst);
    }

    /// Returns the elapsed time since simulation start.
    pub fn elapsed(&self) -> Duration {
        Duration::from_nanos(self.current_nanos.load(Ordering::SeqCst))
    }
}

impl Default for Clock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for Clock {
    fn clone(&self) -> Self {
        Self {
            current_nanos: AtomicU64::new(self.current_nanos.load(Ordering::SeqCst)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clock_starts_at_zero() {
        let clock = Clock::new();
        assert_eq!(clock.now().as_nanos(), 0);
    }

    #[test]
    fn test_clock_starting_at() {
        let clock = Clock::starting_at(Instant::from_nanos(1000));
        assert_eq!(clock.now().as_nanos(), 1000);
    }

    #[test]
    fn test_advance() {
        let clock = Clock::new();
        clock.advance(Duration::from_nanos(100));
        assert_eq!(clock.now().as_nanos(), 100);
    }

    #[test]
    fn test_advance_accumulates() {
        let clock = Clock::new();
        clock.advance(Duration::from_nanos(100));
        clock.advance(Duration::from_nanos(50));
        clock.advance(Duration::from_nanos(25));
        assert_eq!(clock.now().as_nanos(), 175);
    }

    #[test]
    fn test_advance_to() {
        let clock = Clock::new();
        clock.advance_to(Instant::from_nanos(500));
        assert_eq!(clock.now().as_nanos(), 500);
    }

    #[test]
    #[should_panic(expected = "cannot advance clock backwards")]
    fn test_advance_to_panics_on_past() {
        let clock = Clock::starting_at(Instant::from_nanos(100));
        clock.advance_to(Instant::from_nanos(50));
    }

    #[test]
    fn test_set() {
        let clock = Clock::starting_at(Instant::from_nanos(100));
        clock.set(Instant::from_nanos(50));
        assert_eq!(clock.now().as_nanos(), 50);
    }

    #[test]
    fn test_elapsed() {
        let clock = Clock::new();
        clock.advance(Duration::from_millis(100));
        assert_eq!(clock.elapsed(), Duration::from_millis(100));
    }

    #[test]
    fn test_clone() {
        let clock = Clock::new();
        clock.advance(Duration::from_nanos(100));
        let cloned = clock.clone();
        assert_eq!(cloned.now().as_nanos(), 100);
        
        // Advancing original doesn't affect clone
        clock.advance(Duration::from_nanos(50));
        assert_eq!(clock.now().as_nanos(), 150);
        assert_eq!(cloned.now().as_nanos(), 100);
    }

    #[test]
    fn test_default() {
        let clock = Clock::default();
        assert_eq!(clock.now().as_nanos(), 0);
    }
}
