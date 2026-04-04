//! Network simulation coordinator.

use std::collections::{HashMap, VecDeque};

use tracing::{debug, trace};

use super::fault::{Fault, FaultSchedule, FaultState};
use super::latency::LatencyModel;
use super::link::Link;
use super::message::Message;
use crate::time::Instant;
use crate::{NodeId, Result};

/// Configuration for network simulation.
#[derive(Clone, Debug)]
pub struct NetworkConfig {
    /// Default latency model for new links.
    pub latency: LatencyModel,
    /// Default drop rate for new links.
    pub drop_rate: f64,
    /// Default duplicate rate for new links.
    pub duplicate_rate: f64,
    /// Default reorder rate for new links.
    pub reorder_rate: f64,
    /// Default bandwidth limit in bytes per second (0 = unlimited).
    pub bandwidth_bps: u64,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            latency: LatencyModel::default(),
            drop_rate: 0.0,
            duplicate_rate: 0.0,
            reorder_rate: 0.0,
            bandwidth_bps: 0,
        }
    }
}

/// Network simulator coordinating links, faults, and message delivery.
#[derive(Debug)]
pub struct NetworkSim {
    /// Links between node pairs (bidirectional stored as two entries).
    links: HashMap<(NodeId, NodeId), Link>,
    /// Message inboxes per node.
    inboxes: HashMap<NodeId, VecDeque<Message>>,
    /// Current fault state.
    fault_state: FaultState,
    /// Scheduled faults.
    fault_schedule: FaultSchedule,
    /// Configuration.
    config: NetworkConfig,
    /// Random seed.
    seed: u64,
    /// Link counter for unique seeds.
    link_counter: u64,
}

impl NetworkSim {
    /// Creates a new network simulator.
    pub fn new(config: NetworkConfig, seed: u64) -> Self {
        Self {
            links: HashMap::new(),
            inboxes: HashMap::new(),
            fault_state: FaultState::new(),
            fault_schedule: FaultSchedule::new(),
            config,
            seed,
            link_counter: 0,
        }
    }

    /// Creates a network simulator with default config.
    pub fn with_seed(seed: u64) -> Self {
        Self::new(NetworkConfig::default(), seed)
    }

    /// Connects two nodes with bidirectional links.
    pub fn connect(&mut self, a: NodeId, b: NodeId) {
        // Create unique seeds for each link direction
        let seed_a = self.seed.wrapping_add(self.link_counter);
        self.link_counter += 1;
        let seed_b = self.seed.wrapping_add(self.link_counter);
        self.link_counter += 1;

        let mut link_a = Link::new(self.config.latency.clone(), seed_a);
        link_a.set_drop_rate(self.config.drop_rate);
        link_a.set_duplicate_rate(self.config.duplicate_rate);
        link_a.set_reorder_rate(self.config.reorder_rate);
        link_a.set_bandwidth(self.config.bandwidth_bps);

        let mut link_b = Link::new(self.config.latency.clone(), seed_b);
        link_b.set_drop_rate(self.config.drop_rate);
        link_b.set_duplicate_rate(self.config.duplicate_rate);
        link_b.set_reorder_rate(self.config.reorder_rate);
        link_b.set_bandwidth(self.config.bandwidth_bps);

        self.links.insert((a, b), link_a);
        self.links.insert((b, a), link_b);

        // Ensure inboxes exist
        self.inboxes.entry(a).or_default();
        self.inboxes.entry(b).or_default();
    }

    /// Adds a node to the network (creates inbox).
    pub fn add_node(&mut self, node: NodeId) {
        self.inboxes.entry(node).or_default();
    }

    /// Sends a message from one node to another.
    pub fn send(&mut self, from: NodeId, to: NodeId, data: Vec<u8>, now: Instant) -> Result<()> {
        // Check partition
        if !self.fault_state.can_communicate(from, to) {
            trace!(from, to, "message dropped: partitioned");
            return Ok(()); // Silently drop - partitioned
        }
        
        trace!(from, to, bytes = data.len(), "sending message");

        // Get or create link
        let link = self.links.get_mut(&(from, to)).ok_or_else(|| {
            crate::error::Error::NodeNotFound(to)
        })?;

        // Apply fault modifiers to link
        let base_drop = link.drop_rate();
        let fault_drop = self.fault_state.drop_rate();
        link.set_drop_rate((base_drop + fault_drop).min(1.0));

        let base_dup = link.duplicate_rate();
        let fault_dup = self.fault_state.duplicate_rate();
        link.set_duplicate_rate((base_dup + fault_dup).min(1.0));

        // Create and enqueue message
        let msg = Message::new(from, to, data, now);
        link.enqueue(msg, now);

        // Restore original rates
        link.set_drop_rate(base_drop);
        link.set_duplicate_rate(base_dup);

        Ok(())
    }

    /// Receives the next message for a node, if any.
    pub fn recv(&mut self, node: NodeId) -> Option<Message> {
        self.inboxes.get_mut(&node)?.pop_front()
    }

    /// Peeks at the next message for a node without removing it.
    pub fn peek(&self, node: NodeId) -> Option<&Message> {
        self.inboxes.get(&node)?.front()
    }

    /// Returns the number of pending messages in a node's inbox.
    pub fn inbox_len(&self, node: NodeId) -> usize {
        self.inboxes.get(&node).map(|i| i.len()).unwrap_or(0)
    }

    /// Advances the simulation, delivering ready messages.
    pub fn tick(&mut self, now: Instant) {
        // Apply scheduled faults
        for (instant, fault) in self.fault_schedule.take_faults_until(now) {
            debug!(?fault, at_ns = instant.as_nanos(), "applying fault");
            self.fault_state.apply(&fault);
        }

        // Deliver ready messages from all links
        for ((from, to), link) in &mut self.links {
            let delivered = link.deliver(now);
            for msg in delivered {
                // Re-check partition at delivery time
                if self.fault_state.can_communicate(*from, *to) {
                    if let Some(inbox) = self.inboxes.get_mut(to) {
                        inbox.push_back(msg);
                    }
                }
            }
        }
    }

    /// Schedules a fault to occur at the given time.
    pub fn schedule_fault(&mut self, at: Instant, fault: Fault) {
        self.fault_schedule.add(at, fault);
    }

    /// Immediately applies a partition.
    pub fn partition(&mut self, groups: Vec<Vec<NodeId>>) {
        self.fault_state.apply(&Fault::partition(groups));
    }

    /// Removes all partitions and faults.
    pub fn heal(&mut self) {
        self.fault_state.apply(&Fault::heal());
    }

    /// Returns true if two nodes can currently communicate.
    pub fn can_communicate(&self, a: NodeId, b: NodeId) -> bool {
        self.fault_state.can_communicate(a, b)
    }

    /// Returns the next time something will happen (delivery or fault).
    pub fn next_event_time(&self) -> Option<Instant> {
        let next_delivery = self.links.values()
            .filter_map(|l| l.next_delivery_time())
            .min();
        
        let next_fault = self.fault_schedule.next_fault_time(Instant::from_nanos(0));
        
        match (next_delivery, next_fault) {
            (Some(d), Some(f)) => Some(d.min(f)),
            (Some(d), None) => Some(d),
            (None, Some(f)) => Some(f),
            (None, None) => None,
        }
    }

    /// Returns the total number of messages in flight across all links.
    pub fn in_flight_count(&self) -> usize {
        self.links.values().map(|l| l.in_flight_count()).sum()
    }

    /// Resets the network state.
    pub fn reset(&mut self) {
        for link in self.links.values_mut() {
            link.reset();
        }
        for inbox in self.inboxes.values_mut() {
            inbox.clear();
        }
        self.fault_state = FaultState::new();
        self.fault_schedule = FaultSchedule::new();
        self.link_counter = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_new_network() {
        let net = NetworkSim::with_seed(42);
        assert_eq!(net.in_flight_count(), 0);
    }

    #[test]
    fn test_connect_and_send() {
        let mut net = NetworkSim::with_seed(42);
        net.connect(1, 2);

        let now = Instant::from_nanos(0);
        net.send(1, 2, vec![1, 2, 3], now).unwrap();

        assert_eq!(net.in_flight_count(), 1);
    }

    #[test]
    fn test_send_tick_recv() {
        let mut config = NetworkConfig::default();
        config.latency = LatencyModel::fixed(Duration::from_millis(10));
        
        let mut net = NetworkSim::new(config, 42);
        net.connect(1, 2);

        let now = Instant::from_nanos(0);
        net.send(1, 2, vec![42], now).unwrap();

        // Not yet delivered
        net.tick(Instant::from_nanos(5_000_000));
        assert!(net.recv(2).is_none());

        // Now delivered
        net.tick(Instant::from_nanos(10_000_000));
        let msg = net.recv(2).unwrap();
        assert_eq!(msg.data, vec![42]);
        assert_eq!(msg.from, 1);
        assert_eq!(msg.to, 2);
    }

    #[test]
    fn test_partition_blocks_communication() {
        let mut config = NetworkConfig::default();
        config.latency = LatencyModel::fixed(Duration::from_millis(1));
        
        let mut net = NetworkSim::new(config, 42);
        net.connect(1, 2);
        net.connect(2, 3);

        // Partition: [1, 2] and [3]
        net.partition(vec![vec![1, 2], vec![3]]);

        let now = Instant::from_nanos(0);
        
        // 1 -> 2 should work (same group)
        net.send(1, 2, vec![1], now).unwrap();
        
        // 2 -> 3 should be dropped (different groups)
        net.send(2, 3, vec![2], now).unwrap();

        net.tick(Instant::from_nanos(2_000_000));

        assert!(net.recv(2).is_some()); // 1->2 delivered
        assert!(net.recv(3).is_none()); // 2->3 dropped
    }

    #[test]
    fn test_heal_restores_communication() {
        let mut config = NetworkConfig::default();
        config.latency = LatencyModel::fixed(Duration::from_millis(1));
        
        let mut net = NetworkSim::new(config, 42);
        net.connect(1, 2);

        net.partition(vec![vec![1], vec![2]]);
        assert!(!net.can_communicate(1, 2));

        net.heal();
        assert!(net.can_communicate(1, 2));
    }

    #[test]
    fn test_scheduled_fault() {
        let mut config = NetworkConfig::default();
        config.latency = LatencyModel::fixed(Duration::from_millis(1));
        
        let mut net = NetworkSim::new(config, 42);
        net.connect(1, 2);

        // Schedule partition at 5ms
        net.schedule_fault(
            Instant::from_nanos(5_000_000),
            Fault::partition(vec![vec![1], vec![2]]),
        );

        assert!(net.can_communicate(1, 2));

        // Tick past the fault time
        net.tick(Instant::from_nanos(6_000_000));

        assert!(!net.can_communicate(1, 2));
    }

    #[test]
    fn test_multiple_messages() {
        let mut config = NetworkConfig::default();
        config.latency = LatencyModel::fixed(Duration::from_millis(1));
        
        let mut net = NetworkSim::new(config, 42);
        net.connect(1, 2);

        let now = Instant::from_nanos(0);
        net.send(1, 2, vec![1], now).unwrap();
        net.send(1, 2, vec![2], now).unwrap();
        net.send(1, 2, vec![3], now).unwrap();

        net.tick(Instant::from_nanos(2_000_000));

        assert_eq!(net.inbox_len(2), 3);
        assert_eq!(net.recv(2).unwrap().data, vec![1]);
        assert_eq!(net.recv(2).unwrap().data, vec![2]);
        assert_eq!(net.recv(2).unwrap().data, vec![3]);
    }

    #[test]
    fn test_bidirectional_communication() {
        let mut config = NetworkConfig::default();
        config.latency = LatencyModel::fixed(Duration::from_millis(1));
        
        let mut net = NetworkSim::new(config, 42);
        net.connect(1, 2);

        let now = Instant::from_nanos(0);
        net.send(1, 2, vec![1], now).unwrap();
        net.send(2, 1, vec![2], now).unwrap();

        net.tick(Instant::from_nanos(2_000_000));

        assert!(net.recv(2).is_some());
        assert!(net.recv(1).is_some());
    }

    #[test]
    fn test_reset() {
        let mut net = NetworkSim::with_seed(42);
        net.connect(1, 2);
        net.send(1, 2, vec![1], Instant::from_nanos(0)).unwrap();
        net.partition(vec![vec![1], vec![2]]);

        net.reset();

        assert_eq!(net.in_flight_count(), 0);
        assert!(net.can_communicate(1, 2));
    }

    #[test]
    fn test_peek() {
        let mut config = NetworkConfig::default();
        config.latency = LatencyModel::fixed(Duration::from_millis(1));
        
        let mut net = NetworkSim::new(config, 42);
        net.connect(1, 2);
        net.send(1, 2, vec![42], Instant::from_nanos(0)).unwrap();
        net.tick(Instant::from_nanos(2_000_000));

        // Peek doesn't remove
        assert!(net.peek(2).is_some());
        assert!(net.peek(2).is_some());
        assert_eq!(net.inbox_len(2), 1);

        // Recv removes
        net.recv(2);
        assert!(net.peek(2).is_none());
    }
}
