//! Virtual instant representing a point in simulated time.

use std::ops::{Add, Sub};
use std::time::Duration;

/// A point in simulated time, measured in nanoseconds since simulation start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Instant(pub(crate) u64);

impl Instant {
    /// Creates a new instant at the given nanoseconds.
    pub const fn from_nanos(nanos: u64) -> Self {
        Self(nanos)
    }

    /// Returns the number of nanoseconds since simulation start.
    pub const fn as_nanos(&self) -> u64 {
        self.0
    }

    /// Returns the duration elapsed from this instant to another.
    /// Returns None if `later` is before `self`.
    pub fn duration_since(&self, earlier: Instant) -> Option<Duration> {
        self.0.checked_sub(earlier.0).map(Duration::from_nanos)
    }

    /// Adds a duration to this instant, returning None on overflow.
    pub fn checked_add(&self, duration: Duration) -> Option<Instant> {
        self.0.checked_add(duration.as_nanos() as u64).map(Instant)
    }

    /// Subtracts a duration from this instant, returning None on underflow.
    pub fn checked_sub(&self, duration: Duration) -> Option<Instant> {
        self.0.checked_sub(duration.as_nanos() as u64).map(Instant)
    }

    /// Saturating addition of a duration.
    pub fn saturating_add(&self, duration: Duration) -> Instant {
        Instant(self.0.saturating_add(duration.as_nanos() as u64))
    }

    /// Saturating subtraction of a duration.
    pub fn saturating_sub(&self, duration: Duration) -> Instant {
        Instant(self.0.saturating_sub(duration.as_nanos() as u64))
    }
}

impl Add<Duration> for Instant {
    type Output = Instant;

    fn add(self, rhs: Duration) -> Self::Output {
        self.checked_add(rhs).expect("instant overflow")
    }
}

impl Sub<Duration> for Instant {
    type Output = Instant;

    fn sub(self, rhs: Duration) -> Self::Output {
        self.checked_sub(rhs).expect("instant underflow")
    }
}

impl Sub<Instant> for Instant {
    type Output = Duration;

    fn sub(self, rhs: Instant) -> Self::Output {
        self.duration_since(rhs).expect("instant underflow")
    }
}

impl std::fmt::Display for Instant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let nanos = self.0;
        if nanos >= 1_000_000_000 {
            write!(f, "{:.3}s", nanos as f64 / 1_000_000_000.0)
        } else if nanos >= 1_000_000 {
            write!(f, "{:.3}ms", nanos as f64 / 1_000_000.0)
        } else if nanos >= 1_000 {
            write!(f, "{:.3}μs", nanos as f64 / 1_000.0)
        } else {
            write!(f, "{}ns", nanos)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instant_creation() {
        let i = Instant::from_nanos(1000);
        assert_eq!(i.as_nanos(), 1000);
    }

    #[test]
    fn test_instant_default() {
        let i = Instant::default();
        assert_eq!(i.as_nanos(), 0);
    }

    #[test]
    fn test_instant_ordering() {
        let a = Instant::from_nanos(100);
        let b = Instant::from_nanos(200);
        assert!(a < b);
        assert!(b > a);
        assert_eq!(a, Instant::from_nanos(100));
    }

    #[test]
    fn test_duration_since() {
        let earlier = Instant::from_nanos(100);
        let later = Instant::from_nanos(300);
        assert_eq!(later.duration_since(earlier), Some(Duration::from_nanos(200)));
        assert_eq!(earlier.duration_since(later), None);
    }

    #[test]
    fn test_checked_add() {
        let i = Instant::from_nanos(100);
        assert_eq!(i.checked_add(Duration::from_nanos(50)), Some(Instant::from_nanos(150)));
        assert_eq!(Instant::from_nanos(u64::MAX).checked_add(Duration::from_nanos(1)), None);
    }

    #[test]
    fn test_checked_sub() {
        let i = Instant::from_nanos(100);
        assert_eq!(i.checked_sub(Duration::from_nanos(50)), Some(Instant::from_nanos(50)));
        assert_eq!(i.checked_sub(Duration::from_nanos(200)), None);
    }

    #[test]
    fn test_add_duration() {
        let i = Instant::from_nanos(100);
        let result = i + Duration::from_nanos(50);
        assert_eq!(result.as_nanos(), 150);
    }

    #[test]
    fn test_sub_duration() {
        let i = Instant::from_nanos(100);
        let result = i - Duration::from_nanos(50);
        assert_eq!(result.as_nanos(), 50);
    }

    #[test]
    fn test_sub_instant() {
        let a = Instant::from_nanos(100);
        let b = Instant::from_nanos(300);
        assert_eq!(b - a, Duration::from_nanos(200));
    }

    #[test]
    fn test_saturating_add() {
        let i = Instant::from_nanos(u64::MAX - 10);
        let result = i.saturating_add(Duration::from_nanos(100));
        assert_eq!(result.as_nanos(), u64::MAX);
    }

    #[test]
    fn test_saturating_sub() {
        let i = Instant::from_nanos(10);
        let result = i.saturating_sub(Duration::from_nanos(100));
        assert_eq!(result.as_nanos(), 0);
    }

    #[test]
    fn test_display_nanos() {
        let i = Instant::from_nanos(500);
        assert_eq!(format!("{}", i), "500ns");
    }

    #[test]
    fn test_display_micros() {
        let i = Instant::from_nanos(5_500);
        assert!(format!("{}", i).contains("μs"));
    }

    #[test]
    fn test_display_millis() {
        let i = Instant::from_nanos(5_500_000);
        assert!(format!("{}", i).contains("ms"));
    }

    #[test]
    fn test_display_seconds() {
        let i = Instant::from_nanos(1_500_000_000);
        assert!(format!("{}", i).contains("s"));
    }
}
