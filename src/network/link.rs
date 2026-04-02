//! Point-to-point network link simulation with latency and failures.

use std::collections::VecDeque;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use super::latency::LatencyModel;
use super::message::{InFlightMessage, Message};
use crate::time::Instant;

/// A simulated network link between two nodes.
///
/// Models latency, packet loss, and packet duplication.
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
            rng: StdRng::seed_from_u64(seed),
            seed,
        }
    }

    /// Creates a link with default latency (1ms fixed).
    pub fn with_seed(seed: u64) -> Self {
        Self::new(LatencyModel::default(), seed)
    }

    /// Enqueues a message for delivery through this link.
    ///
    /// The message may be dropped based on `drop_rate`, and may be
    /// duplicated based on `duplicate_rate`.
    pub fn enqueue(&mut self, msg: Message, now: Instant) {
        // Check for drop
        if self.rng.gen::<f64>() < self.drop_rate {
            return; // Message dropped
        }

        // Calculate delivery time
        let delay = self.latency.sample(&mut self.rng);
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
