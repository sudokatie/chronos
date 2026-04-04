//! Tests for the time simulation system.

use std::time::Duration;
use chronos::time::{Clock, Instant, TimerWheel};

#[test]
fn test_clock_starts_at_zero() {
    let clock = Clock::new();
    assert_eq!(clock.now().as_nanos(), 0);
}

#[test]
fn test_clock_advance() {
    let clock = Clock::new();
    
    clock.advance(Duration::from_secs(1));
    assert_eq!(clock.now().as_nanos(), 1_000_000_000);
    
    clock.advance(Duration::from_millis(500));
    assert_eq!(clock.now().as_nanos(), 1_500_000_000);
}

#[test]
fn test_clock_advance_to() {
    let clock = Clock::new();
    
    clock.advance_to(Instant::from_nanos(5_000_000_000));
    assert_eq!(clock.now().as_nanos(), 5_000_000_000);
}

#[test]
#[should_panic(expected = "cannot advance clock backwards")]
fn test_clock_advance_to_past_panics() {
    let clock = Clock::new();
    clock.advance(Duration::from_secs(10));
    clock.advance_to(Instant::from_nanos(5_000_000_000));
}

#[test]
fn test_clock_elapsed() {
    let clock = Clock::new();
    clock.advance(Duration::from_millis(500));
    
    assert_eq!(clock.elapsed(), Duration::from_millis(500));
}

#[test]
fn test_instant_arithmetic() {
    let t1 = Instant::from_nanos(1_000_000_000);
    let t2 = Instant::from_nanos(2_500_000_000);
    
    let duration = t2.duration_since(t1).unwrap();
    assert_eq!(duration, Duration::from_millis(1500));
    
    let t3 = t1.saturating_add(Duration::from_secs(5));
    assert_eq!(t3.as_nanos(), 6_000_000_000);
}

#[test]
fn test_instant_saturating_add() {
    let t = Instant::from_nanos(u64::MAX - 100);
    let result = t.saturating_add(Duration::from_nanos(200));
    
    // Should saturate at max
    assert_eq!(result.as_nanos(), u64::MAX);
}

#[test]
fn test_instant_ordering() {
    let t1 = Instant::from_nanos(100);
    let t2 = Instant::from_nanos(200);
    
    assert!(t1 < t2);
    assert!(t2 > t1);
    assert!(t1 <= t1);
    assert!(t1 == Instant::from_nanos(100));
}

#[test]
fn test_timer_wheel_empty() {
    let wheel = TimerWheel::new();
    
    assert!(wheel.is_empty());
    assert_eq!(wheel.len(), 0);
    assert_eq!(wheel.next_deadline(), None);
}

#[test]
fn test_timer_wheel_schedule() {
    let mut wheel = TimerWheel::new();
    
    let waker = futures::task::noop_waker();
    
    let id1 = wheel.schedule(Instant::from_nanos(100), waker.clone());
    let id2 = wheel.schedule(Instant::from_nanos(50), waker.clone());
    
    assert_eq!(wheel.len(), 2);
    assert_eq!(wheel.next_deadline(), Some(Instant::from_nanos(50)));
    
    assert_ne!(id1, id2);
}

#[test]
fn test_timer_wheel_fire_expired() {
    let mut wheel = TimerWheel::new();
    let waker = futures::task::noop_waker();
    
    wheel.schedule(Instant::from_nanos(100), waker.clone());
    wheel.schedule(Instant::from_nanos(200), waker.clone());
    wheel.schedule(Instant::from_nanos(300), waker.clone());
    
    // Fire timers up to 150ns
    let fired = wheel.fire_expired(Instant::from_nanos(150));
    assert_eq!(fired.len(), 1);
    assert_eq!(wheel.len(), 2);
    
    // Fire remaining
    let fired = wheel.fire_expired(Instant::from_nanos(300));
    assert_eq!(fired.len(), 2);
    assert!(wheel.is_empty());
}

#[test]
fn test_timer_wheel_cancel() {
    let mut wheel = TimerWheel::new();
    let waker = futures::task::noop_waker();
    
    let id = wheel.schedule(Instant::from_nanos(100), waker);
    
    assert!(wheel.cancel(id));
    assert!(wheel.is_empty());
    
    // Cancel again should return false
    assert!(!wheel.cancel(id));
}

#[test]
fn test_timer_wheel_multiple_same_deadline() {
    let mut wheel = TimerWheel::new();
    let waker = futures::task::noop_waker();
    
    wheel.schedule(Instant::from_nanos(100), waker.clone());
    wheel.schedule(Instant::from_nanos(100), waker.clone());
    wheel.schedule(Instant::from_nanos(100), waker.clone());
    
    let fired = wheel.fire_expired(Instant::from_nanos(100));
    assert_eq!(fired.len(), 3);
}

#[test]
fn test_clock_set() {
    let clock = Clock::new();
    clock.advance(Duration::from_secs(10));
    
    // Can set backwards with set()
    clock.set(Instant::from_nanos(5_000_000_000));
    assert_eq!(clock.now().as_nanos(), 5_000_000_000);
}

#[test]
fn test_clock_clone() {
    let clock = Clock::new();
    clock.advance(Duration::from_secs(5));
    
    let cloned = clock.clone();
    assert_eq!(cloned.now().as_nanos(), 5_000_000_000);
    
    // Advancing original doesn't affect clone
    clock.advance(Duration::from_secs(1));
    assert_eq!(clock.now().as_nanos(), 6_000_000_000);
    assert_eq!(cloned.now().as_nanos(), 5_000_000_000);
}
