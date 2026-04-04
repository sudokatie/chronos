//! Point-to-point network link simulation with latency and failures.

use std::collections::VecDeque;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use super::latency::LatencyModel;
use super::message::{InFlightMessage, Message};
use crate::time::Instant;

/// A simulated network link between two nodes.
///
/// Models latency, packet loss, packet duplication, bandwidth limits, and reordering.
#[derive(Debug)]
pub struct Link {
    /// Latency model for this link.
    latency: LatencyModel,
    /// Messages currently in flight.
    in_flight: VecDeque<InFlightMessage>,
    /// Probability of dropping a message (0.0 to 1.0).
    drop_rate: f64,
    /// Probability of duplicating a message (0.0 to 1.0).
    duplicate_rate: f64,
    /// Probability of reordering messages (0.0 to 1.0).
    reorder_rate: f64,
    /// Bandwidth limit in bytes per second (0 = unlimited).
    bandwidth_bps: u64,
    /// Bytes sent in current time window.
    bytes_in_window: u64,
    /// Start of current bandwidth window.
    window_start: Option<Instant>,
    /// Random number generator for deterministic simulation.
    rng: StdRng,
    /// Seed for reset.
    seed: u64,
}

impl Link {
    /// Creates a new link with the given latency model and RNG seed.
    pub fn new(latency: LatencyModel, seed: u64) -> Self {
        Self {
            latency,
            in_flight: VecDeque::new(),
            drop_rate: 0.0,
            duplicate_rate: 0.0,
            reorder_rate: 0.0,
            bandwidth_bps: 0,
            bytes_in_window: 0,
            window_start: None,
            rng: StdRng::seed_from_u64(seed),
            seed,
        }
    }

    /// Creates a link with default latency (1ms fixed).
    pub fn with_seed(seed: u64) -> Self {
        Self::new(LatencyModel::default(), seed)
    }

    /// Sets the reorder rate (probability of reordering each message).
    pub fn set_reorder_rate(&mut self, rate: f64) {
        self.reorder_rate = rate.clamp(0.0, 1.0);
    }

    /// Returns the current reorder rate.
    pub fn reorder_rate(&self) -> f64 {
        self.reorder_rate
    }

    /// Sets the bandwidth limit in bytes per second (0 = unlimited).
    pub fn set_bandwidth(&mut self, bytes_per_second: u64) {
        self.bandwidth_bps = bytes_per_second;
    }

    /// Returns the current bandwidth limit.
    pub fn bandwidth(&self) -> u64 {
        self.bandwidth_bps
    }

    /// Enqueues a message for delivery through this link.
    ///
    /// The message may be dropped based on `drop_rate`, may be
    /// duplicated based on `duplicate_rate`, and may be reordered
    /// based on `reorder_rate`. Bandwidth limits are also enforced.
    pub fn enqueue(&mut self, msg: Message, now: Instant) {
        // Check for drop
        if self.rng.gen::<f64>() < self.drop_rate {
            return; // Message dropped
        }

        // Check bandwidth limit
        if self.bandwidth_bps > 0 {
            let msg_size = msg.size() as u64;
            
            // Reset window if needed (1 second window)
            let window_duration = std::time::Duration::from_secs(1);
            if let Some(start) = self.window_start {
                if now.duration_since(start).unwrap_or_default() >= window_duration {
                    self.window_start = Some(now);
                    self.bytes_in_window = 0;
                }
            } else {
                self.window_start = Some(now);
            }

            // Check if we exceed bandwidth
            if self.bytes_in_window + msg_size > self.bandwidth_bps {
                // Queue delay: wait until next window
                let extra_delay = window_duration;
                let delay = self.latency.sample(&mut self.rng) + extra_delay;
                let deliver_at = now.saturating_add(delay);
                self.in_flight.push_back(InFlightMessage::new(msg, deliver_at));
                return;
            }
            
            self.bytes_in_window += msg_size;
        }

        // Calculate delivery time
        let mut delay = self.latency.sample(&mut self.rng);
        
        // Apply reordering - add random extra delay
        if self.rng.gen::<f64>() < self.reorder_rate {
            let extra = self.latency.sample(&mut self.rng);
            delay += extra;
        }
        
        let deliver_at = now.saturating_add(delay);

        // Enqueue the message
        self.in_flight.push_back(InFlightMessage::new(msg.clone(), deliver_at));

        // Check for duplicate
        if self.rng.gen::<f64>() < self.duplicate_rate {
            let dup_delay = self.latency.sample(&mut self.rng);
            let dup_deliver_at = now.saturating_add(dup_delay);
            self.in_flight.push_back(InFlightMessage::new(msg, dup_deliver_at));
        }
    }

    /// Delivers all messages that should arrive by the given time.
    ///
    /// Returns the delivered messages and removes them from the in-flight queue.
    pub fn deliver(&mut self, now: Instant) -> Vec<Message> {
        let mut delivered = Vec::new();
        
        // Partition: keep messages not yet ready, return those that are
        let mut still_in_flight = VecDeque::new();
        
        for inflight in self.in_flight.drain(..) {
            if inflight.deliver_at <= now {
                delivered.push(inflight.msg);
            } else {
                still_in_flight.push_back(inflight);
            }
        }
        
        self.in_flight = still_in_flight;
        delivered
    }

    /// Returns messages that would be delivered at exactly the given time.
    pub fn peek_deliverable(&self, now: Instant) -> Vec<&Message> {
        self.in_flight
            .iter()
            .filter(|m| m.deliver_at <= now)
            .map(|m| &m.msg)
            .collect()
    }

    /// Sets the drop rate (probability of dropping each message).
    pub fn set_drop_rate(&mut self, rate: f64) {
        self.drop_rate = rate.clamp(0.0, 1.0);
    }

    /// Returns the current drop rate.
    pub fn drop_rate(&self) -> f64 {
        self.drop_rate
    }

    /// Sets the duplicate rate (probability of duplicating each message).
    pub fn set_duplicate_rate(&mut self, rate: f64) {
        self.duplicate_rate = rate.clamp(0.0, 1.0);
    }

    /// Returns the current duplicate rate.
    pub fn duplicate_rate(&self) -> f64 {
        self.duplicate_rate
    }

    /// Sets the latency model.
    pub fn set_latency(&mut self, latency: LatencyModel) {
        self.latency = latency;
    }

    /// Returns a reference to the latency model.
    pub fn latency(&self) -> &LatencyModel {
        &self.latency
    }

    /// Returns the number of messages currently in flight.
    pub fn in_flight_count(&self) -> usize {
        self.in_flight.len()
    }

    /// Returns true if no messages are in flight.
    pub fn is_empty(&self) -> bool {
        self.in_flight.is_empty()
    }

    /// Returns the next delivery time, if any messages are in flight.
    pub fn next_delivery_time(&self) -> Option<Instant> {
        self.in_flight.iter().map(|m| m.deliver_at).min()
    }

    /// Resets the link state.
    pub fn reset(&mut self) {
        self.in_flight.clear();
        self.bytes_in_window = 0;
        self.window_start = None;
        self.rng = StdRng::seed_from_u64(self.seed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn make_msg(from: u32, to: u32) -> Message {
        Message::with_id(0, from, to, vec![], Instant::from_nanos(0))
    }

    #[test]
    fn test_new_link() {
        let link = Link::new(LatencyModel::fixed(Duration::from_millis(10)), 42);
        assert!(link.is_empty());
        assert_eq!(link.drop_rate(), 0.0);
        assert_eq!(link.duplicate_rate(), 0.0);
    }

    #[test]
    fn test_enqueue_and_deliver() {
        let mut link = Link::new(LatencyModel::fixed(Duration::from_millis(10)), 42);
        let msg = make_msg(1, 2);
        let now = Instant::from_nanos(0);

        link.enqueue(msg, now);
        assert_eq!(link.in_flight_count(), 1);

        // Not yet delivered
        let delivered = link.deliver(Instant::from_nanos(5_000_000)); // 5ms
        assert!(delivered.is_empty());
        assert_eq!(link.in_flight_count(), 1);

        // Now delivered
        let delivered = link.deliver(Instant::from_nanos(10_000_000)); // 10ms
        assert_eq!(delivered.len(), 1);
        assert!(link.is_empty());
    }

    #[test]
    fn test_100_percent_drop_rate() {
        let mut link = Link::new(LatencyModel::fixed(Duration::from_millis(1)), 42);
        link.set_drop_rate(1.0);

        for i in 0..10 {
            link.enqueue(make_msg(1, i), Instant::from_nanos(0));
        }

        assert!(link.is_empty());
    }

    #[test]
    fn test_0_percent_drop_rate() {
        let mut link = Link::new(LatencyModel::fixed(Duration::from_millis(1)), 42);
        link.set_drop_rate(0.0);

        for i in 0..10 {
            link.enqueue(make_msg(1, i), Instant::from_nanos(0));
        }

        assert_eq!(link.in_flight_count(), 10);
    }

    #[test]
    fn test_duplicate_rate() {
        let mut link = Link::new(LatencyModel::fixed(Duration::from_millis(1)), 42);
        link.set_duplicate_rate(1.0); // Always duplicate

        link.enqueue(make_msg(1, 2), Instant::from_nanos(0));

        // Should have original + duplicate = 2 messages
        assert_eq!(link.in_flight_count(), 2);
    }

    #[test]
    fn test_partial_drop_rate() {
        let mut link = Link::new(LatencyModel::fixed(Duration::from_millis(1)), 12345);
        link.set_drop_rate(0.5);

        for i in 0..1000 {
            link.enqueue(make_msg(1, i), Instant::from_nanos(0));
        }

        // Should have roughly half delivered (allow 20% variance)
        let count = link.in_flight_count();
        assert!(count > 400 && count < 600, "Count was {}", count);
    }

    #[test]
    fn test_next_delivery_time() {
        let mut link = Link::new(LatencyModel::fixed(Duration::from_millis(10)), 42);
        assert_eq!(link.next_delivery_time(), None);

        link.enqueue(make_msg(1, 2), Instant::from_nanos(0));
        assert_eq!(link.next_delivery_time(), Some(Instant::from_nanos(10_000_000)));
    }

    #[test]
    fn test_reset() {
        let mut link = Link::new(LatencyModel::fixed(Duration::from_millis(1)), 42);
        link.enqueue(make_msg(1, 2), Instant::from_nanos(0));
        assert!(!link.is_empty());

        link.reset();
        assert!(link.is_empty());
    }

    #[test]
    fn test_deterministic_with_same_seed() {
        let mut link1 = Link::new(LatencyModel::fixed(Duration::from_millis(1)), 42);
        let mut link2 = Link::new(LatencyModel::fixed(Duration::from_millis(1)), 42);
        
        link1.set_drop_rate(0.5);
        link2.set_drop_rate(0.5);

        for i in 0..100 {
            link1.enqueue(make_msg(1, i), Instant::from_nanos(0));
            link2.enqueue(make_msg(1, i), Instant::from_nanos(0));
        }

        assert_eq!(link1.in_flight_count(), link2.in_flight_count());
    }

    #[test]
    fn test_peek_deliverable() {
        let mut link = Link::new(LatencyModel::fixed(Duration::from_millis(10)), 42);
        link.enqueue(make_msg(1, 2), Instant::from_nanos(0));

        // Not yet deliverable
        let peek = link.peek_deliverable(Instant::from_nanos(5_000_000));
        assert!(peek.is_empty());

        // Now deliverable
        let peek = link.peek_deliverable(Instant::from_nanos(10_000_000));
        assert_eq!(peek.len(), 1);

        // Peek doesn't remove
        assert_eq!(link.in_flight_count(), 1);
    }
}
