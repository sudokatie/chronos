//! Cluster simulation for distributed systems testing.

mod happens_before;
pub mod node;

pub use happens_before::{HappensBeforeGraph, HBEvent, VectorClock};
pub use node::{Node, NodeState, Message, Query, MessageHandler, ByteMessage, EchoHandler, InternalMessage};

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::network::NetworkSim;
use crate::time::{Clock, Instant};
use crate::{EventId, NodeId};

/// A simulated cluster of nodes.
pub struct Cluster {
    nodes: Vec<Node>,
    network: NetworkSim,
    clock: Clock,
    partitions: Vec<HashSet<NodeId>>,
    happens_before: HappensBeforeGraph,
    seed: u64,
    /// Clock skew rates per node (1.0 = normal speed).
    clock_skews: HashMap<NodeId, f64>,
    /// Accumulated clock offsets per node (in nanoseconds).
    clock_offsets: HashMap<NodeId, i64>,
    /// Base time when skew was last applied (for calculating drift).
    skew_base_times: HashMap<NodeId, u64>,
}

impl Cluster {
    /// Create a new cluster with the given number of nodes.
    pub fn new(size: usize) -> Self {
        Self::with_seed(size, 0)
    }

    /// Create a new cluster with a specific random seed.
    pub fn with_seed(size: usize, seed: u64) -> Self {
        let nodes: Vec<Node> = (0..size as NodeId).map(Node::new).collect();
        let mut network = NetworkSim::with_seed(seed);
        
        // Connect all nodes to each other
        for i in 0..size as NodeId {
            network.add_node(i);
            for j in (i + 1)..size as NodeId {
                network.connect(i, j);
            }
        }
        
        let clock = Clock::new();

        Self {
            nodes,
            network,
            clock,
            partitions: Vec::new(),
            happens_before: HappensBeforeGraph::new(),
            seed,
            clock_skews: HashMap::new(),
            clock_offsets: HashMap::new(),
            skew_base_times: HashMap::new(),
        }
    }

    /// Get the number of nodes in the cluster.
    pub fn size(&self) -> usize {
        self.nodes.len()
    }

    /// Get the seed used for this cluster.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Get a reference to a node by ID.
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id as usize)
    }

    /// Get a mutable reference to a node by ID.
    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.get_mut(id as usize)
    }

    /// Get all nodes.
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// Get mutable access to all nodes.
    pub fn nodes_mut(&mut self) -> &mut [Node] {
        &mut self.nodes
    }

    /// Get the current simulated time (global/real time).
    pub fn now(&self) -> Instant {
        self.clock.now()
    }

    /// Get the current time as perceived by a specific node.
    /// 
    /// This takes into account any clock skew or jumps applied to the node.
    /// Nodes with clock skew will perceive time passing faster or slower.
    pub fn node_time(&self, node_id: NodeId) -> Instant {
        let global_nanos = self.clock.now().as_nanos();
        
        // Get clock skew (default 1.0 = normal)
        let skew = self.clock_skews.get(&node_id).copied().unwrap_or(1.0);
        
        // Get the base time when skew started
        let base = self.skew_base_times.get(&node_id).copied().unwrap_or(0);
        
        // Calculate skewed elapsed time since skew was applied
        let elapsed_since_base = global_nanos.saturating_sub(base);
        let skewed_elapsed = (elapsed_since_base as f64 * skew) as u64;
        
        // Get any clock offset (from jumps)
        let offset = self.clock_offsets.get(&node_id).copied().unwrap_or(0);
        
        // Calculate final node time
        let node_nanos = if offset >= 0 {
            base.saturating_add(skewed_elapsed).saturating_add(offset as u64)
        } else {
            base.saturating_add(skewed_elapsed).saturating_sub((-offset) as u64)
        };
        
        Instant::from_nanos(node_nanos)
    }

    /// Apply clock skew to a node.
    /// 
    /// A rate > 1.0 makes the node's clock run fast (perceives more time passing).
    /// A rate < 1.0 makes the node's clock run slow (perceives less time passing).
    /// A rate of 1.0 is normal.
    pub fn set_clock_skew(&mut self, node_id: NodeId, rate: f64) {
        let rate = rate.max(0.01); // Minimum 1% speed
        
        // Record the current global time as the base for this skew
        let now = self.clock.now().as_nanos();
        
        // If there was a previous skew, accumulate the offset
        if let Some(&old_rate) = self.clock_skews.get(&node_id) {
            if let Some(&old_base) = self.skew_base_times.get(&node_id) {
                let elapsed = now.saturating_sub(old_base);
                let skewed_elapsed = (elapsed as f64 * old_rate) as u64;
                let offset = self.clock_offsets.entry(node_id).or_insert(0);
                *offset += (skewed_elapsed as i64) - (elapsed as i64);
            }
        }
        
        self.clock_skews.insert(node_id, rate);
        self.skew_base_times.insert(node_id, now);
    }

    /// Get the clock skew rate for a node (1.0 if no skew).
    pub fn clock_skew(&self, node_id: NodeId) -> f64 {
        self.clock_skews.get(&node_id).copied().unwrap_or(1.0)
    }

    /// Apply a clock jump to a node.
    /// 
    /// Positive delta jumps the clock forward, negative jumps backward.
    /// The delta is in nanoseconds.
    pub fn clock_jump(&mut self, node_id: NodeId, delta_nanos: i64) {
        let offset = self.clock_offsets.entry(node_id).or_insert(0);
        *offset += delta_nanos;
    }

    /// Apply a clock jump forward by a duration.
    pub fn clock_jump_forward(&mut self, node_id: NodeId, duration: std::time::Duration) {
        self.clock_jump(node_id, duration.as_nanos() as i64);
    }

    /// Get the accumulated clock offset for a node (0 if no offset).
    pub fn clock_offset(&self, node_id: NodeId) -> i64 {
        self.clock_offsets.get(&node_id).copied().unwrap_or(0)
    }

    /// Clear all clock faults for a node, restoring normal time.
    pub fn clear_clock_faults(&mut self, node_id: NodeId) {
        self.clock_skews.remove(&node_id);
        self.clock_offsets.remove(&node_id);
        self.skew_base_times.remove(&node_id);
    }

    /// Clear all clock faults for all nodes.
    pub fn clear_all_clock_faults(&mut self) {
        self.clock_skews.clear();
        self.clock_offsets.clear();
        self.skew_base_times.clear();
    }

    /// Get a reference to the clock.
    pub fn clock(&self) -> &Clock {
        &self.clock
    }

    /// Get a reference to the network simulator.
    pub fn network(&self) -> &NetworkSim {
        &self.network
    }

    /// Get a mutable reference to the network simulator.
    pub fn network_mut(&mut self) -> &mut NetworkSim {
        &mut self.network
    }

    /// Get a reference to the happens-before graph.
    pub fn happens_before_graph(&self) -> &HappensBeforeGraph {
        &self.happens_before
    }

    /// Get a mutable reference to the happens-before graph.
    pub fn happens_before_graph_mut(&mut self) -> &mut HappensBeforeGraph {
        &mut self.happens_before
    }

    /// Record a local event for happens-before tracking.
    pub fn record_event(&mut self, node: NodeId, description: impl Into<String>) -> EventId {
        self.happens_before.local_event(node, description)
    }

    /// Record a send event.
    pub fn record_send(&mut self, from: NodeId, description: impl Into<String>) -> EventId {
        self.happens_before.send_event(from, description)
    }

    /// Record a receive event.
    pub fn record_recv(&mut self, to: NodeId, send_event: EventId, description: impl Into<String>) -> EventId {
        self.happens_before.recv_event(to, send_event, description)
    }

    /// Check if event A happened before event B.
    pub fn happened_before(&self, a: EventId, b: EventId) -> bool {
        self.happens_before.happened_before(a, b)
    }

    /// Check if two events are concurrent.
    pub fn concurrent(&self, a: EventId, b: EventId) -> bool {
        self.happens_before.concurrent(a, b)
    }

    /// Advance the simulated clock (sync version).
    pub fn advance_time(&mut self, duration: std::time::Duration) {
        self.clock.advance(duration);
        self.network.tick(self.clock.now());
        self.deliver_messages();
    }

    /// Advance the simulated clock (async version).
    pub fn advance_time_async(&mut self, duration: std::time::Duration) -> AdvanceTimeFuture<'_> {
        AdvanceTimeFuture {
            cluster: self,
            duration,
            done: false,
        }
    }

    /// Advance time to a specific instant (sync version).
    pub fn advance_to(&mut self, instant: Instant) {
        self.clock.advance_to(instant);
        self.network.tick(self.clock.now());
        self.deliver_messages();
    }

    /// Advance time to a specific instant (async version).
    pub fn advance_to_async(&mut self, instant: Instant) -> AdvanceToFuture<'_> {
        AdvanceToFuture {
            cluster: self,
            instant,
            done: false,
        }
    }

    /// Deliver pending network messages to nodes.
    fn deliver_messages(&mut self) {
        let now = self.clock.now();
        
        // Collect outgoing messages from all nodes (including responses)
        let mut outgoing: Vec<(NodeId, InternalMessage)> = Vec::new();
        for node in &mut self.nodes {
            if node.is_running() {
                for msg in node.drain_outbox() {
                    outgoing.push((node.id(), msg));
                }
            }
        }

        // Process outgoing messages
        for (_from, msg) in outgoing {
            // Check if this is a response to a pending request
            if msg.request_id > 0 {
                // Try to deliver as response first
                if let Some(target_node) = self.nodes.get_mut(msg.to as usize) {
                    if target_node.deliver_response(msg.request_id, msg.data.clone()) {
                        continue; // Response delivered directly
                    }
                }
            }
            
            // Otherwise, send through network
            if self.can_communicate(msg.from, msg.to) {
                let _ = self.network.send(msg.from, msg.to, msg.data, now);
            }
        }

        // Deliver incoming messages from network
        for i in 0..self.nodes.len() {
            let id = i as NodeId;
            if self.nodes[i].is_running() {
                while let Some(msg) = self.network.recv(id) {
                    // Check partition - only deliver if nodes can communicate
                    if self.can_communicate(msg.from, id) {
                        self.nodes[i].enqueue(msg);
                    }
                }
            }
        }
    }

    /// Check if two nodes can communicate (not partitioned).
    pub fn can_communicate(&self, from: NodeId, to: NodeId) -> bool {
        if self.partitions.is_empty() {
            return true;
        }

        // Nodes can communicate if they're in the same partition group
        for group in &self.partitions {
            if group.contains(&from) && group.contains(&to) {
                return true;
            }
        }

        false
    }

    /// Create a network partition.
    /// Each slice defines a group of nodes that can communicate with each other.
    pub fn partition(&mut self, groups: &[&[NodeId]]) {
        self.partitions = groups
            .iter()
            .map(|g| g.iter().copied().collect())
            .collect();
        
        // Also update network sim
        self.network.partition(groups.iter().map(|g| g.to_vec()).collect());
    }

    /// Heal all network partitions.
    pub fn heal_partition(&mut self) {
        self.partitions.clear();
        self.network.heal();
    }

    /// Crash a node.
    pub fn crash_node(&mut self, id: NodeId) {
        if let Some(node) = self.node_mut(id) {
            node.crash();
        }
    }

    /// Restart a crashed node.
    pub fn restart_node(&mut self, id: NodeId) {
        if let Some(node) = self.node_mut(id) {
            node.restart();
        }
    }

    /// Check if all nodes have stabilized (no pending work).
    pub fn is_stable(&self) -> bool {
        self.nodes.iter().all(|n| !n.has_pending()) && self.network.in_flight_count() == 0
    }

    /// Run until the cluster stabilizes or max iterations reached (sync version).
    pub fn run_until_stable(&mut self, max_iterations: usize) -> bool {
        for _ in 0..max_iterations {
            if self.is_stable() {
                return true;
            }
            self.advance_time(std::time::Duration::from_millis(1));
        }
        false
    }

    /// Run until stable (async version).
    pub fn run_until_stable_async(&mut self, max_iterations: usize) -> RunUntilStableFuture<'_> {
        RunUntilStableFuture {
            cluster: self,
            max_iterations,
            current_iteration: 0,
        }
    }

    /// Run for a duration (sync version).
    pub fn run_for(&mut self, duration: std::time::Duration) {
        let deadline = self.clock.now().saturating_add(duration);
        while self.clock.now() < deadline {
            self.advance_time(std::time::Duration::from_millis(1));
        }
    }

    /// Run for a duration (async version).
    pub fn run_for_async(&mut self, duration: std::time::Duration) -> RunForFuture<'_> {
        let deadline = self.clock.now().saturating_add(duration);
        RunForFuture {
            cluster: self,
            deadline,
        }
    }

    /// Get count of running nodes.
    pub fn running_count(&self) -> usize {
        self.nodes.iter().filter(|n| n.is_running()).count()
    }

    /// Get count of crashed nodes.
    pub fn crashed_count(&self) -> usize {
        self.nodes.iter().filter(|n| n.is_crashed()).count()
    }

    /// Process all pending messages on all nodes.
    pub fn process_messages(&mut self) {
        for node in &mut self.nodes {
            if node.is_running() {
                node.process_messages();
            }
        }
    }

    /// Reset the cluster state.
    pub fn reset(&mut self) {
        for node in &mut self.nodes {
            node.restart();
        }
        self.partitions.clear();
        self.network.reset();
        self.happens_before.reset();
        self.clock.set(Instant::from_nanos(0));
        self.clear_all_clock_faults();
    }
}

/// Future for advancing time asynchronously.
pub struct AdvanceTimeFuture<'a> {
    cluster: &'a mut Cluster,
    duration: std::time::Duration,
    done: bool,
}

impl<'a> Future for AdvanceTimeFuture<'a> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
        if self.done {
            return Poll::Ready(());
        }
        let duration = self.duration;
        self.cluster.advance_time(duration);
        self.done = true;
        Poll::Ready(())
    }
}

/// Future for advancing to a specific instant.
pub struct AdvanceToFuture<'a> {
    cluster: &'a mut Cluster,
    instant: Instant,
    done: bool,
}

impl<'a> Future for AdvanceToFuture<'a> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
        if self.done {
            return Poll::Ready(());
        }
        let instant = self.instant;
        self.cluster.advance_to(instant);
        self.done = true;
        Poll::Ready(())
    }
}

/// Future for running until stable.
pub struct RunUntilStableFuture<'a> {
    cluster: &'a mut Cluster,
    max_iterations: usize,
    current_iteration: usize,
}

impl<'a> Future for RunUntilStableFuture<'a> {
    type Output = bool;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<bool> {
        if self.cluster.is_stable() {
            return Poll::Ready(true);
        }
        
        if self.current_iteration >= self.max_iterations {
            return Poll::Ready(false);
        }
        
        self.cluster.advance_time(std::time::Duration::from_millis(1));
        self.current_iteration += 1;
        
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

/// Future for running for a duration.
pub struct RunForFuture<'a> {
    cluster: &'a mut Cluster,
    deadline: Instant,
}

impl<'a> Future for RunForFuture<'a> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.cluster.clock.now() >= self.deadline {
            return Poll::Ready(());
        }
        
        self.cluster.advance_time(std::time::Duration::from_millis(1));
        
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cluster_new() {
        let cluster = Cluster::new(3);
        assert_eq!(cluster.size(), 3);
        assert_eq!(cluster.running_count(), 3);
    }

    #[test]
    fn test_cluster_node_access() {
        let cluster = Cluster::new(3);
        
        assert!(cluster.node(0).is_some());
        assert!(cluster.node(1).is_some());
        assert!(cluster.node(2).is_some());
        assert!(cluster.node(3).is_none());
    }

    #[test]
    fn test_cluster_crash_restart() {
        let mut cluster = Cluster::new(3);
        
        cluster.crash_node(1);
        assert_eq!(cluster.running_count(), 2);
        assert_eq!(cluster.crashed_count(), 1);
        assert!(cluster.node(1).unwrap().is_crashed());
        
        cluster.restart_node(1);
        assert_eq!(cluster.running_count(), 3);
        assert!(cluster.node(1).unwrap().is_running());
    }

    #[test]
    fn test_cluster_partition() {
        let mut cluster = Cluster::new(3);
        
        // Partition: {0, 1} and {2}
        cluster.partition(&[&[0, 1], &[2]]);
        
        assert!(cluster.can_communicate(0, 1));
        assert!(cluster.can_communicate(1, 0));
        assert!(!cluster.can_communicate(0, 2));
        assert!(!cluster.can_communicate(2, 1));
    }

    #[test]
    fn test_cluster_heal_partition() {
        let mut cluster = Cluster::new(3);
        
        cluster.partition(&[&[0, 1], &[2]]);
        assert!(!cluster.can_communicate(0, 2));
        
        cluster.heal_partition();
        assert!(cluster.can_communicate(0, 2));
    }

    #[test]
    fn test_cluster_advance_time() {
        let mut cluster = Cluster::new(2);
        
        assert_eq!(cluster.now(), Instant::from_nanos(0));
        
        cluster.advance_time(std::time::Duration::from_secs(1));
        assert_eq!(cluster.now().as_nanos(), 1_000_000_000);
    }

    #[test]
    fn test_cluster_is_stable() {
        let cluster = Cluster::new(2);
        assert!(cluster.is_stable());
    }

    #[test]
    fn test_cluster_with_seed() {
        let c1 = Cluster::with_seed(3, 42);
        let c2 = Cluster::with_seed(3, 42);
        
        assert_eq!(c1.size(), c2.size());
        assert_eq!(c1.seed(), c2.seed());
    }

    #[test]
    fn test_cluster_happens_before() {
        let mut cluster = Cluster::new(2);
        
        let e1 = cluster.record_event(0, "event 1");
        let e2 = cluster.record_event(0, "event 2");
        
        assert!(cluster.happened_before(e1, e2));
        assert!(!cluster.happened_before(e2, e1));
    }

    #[test]
    fn test_cluster_concurrent_events() {
        let mut cluster = Cluster::new(2);
        
        let e1 = cluster.record_event(0, "node 0 event");
        let e2 = cluster.record_event(1, "node 1 event");
        
        assert!(cluster.concurrent(e1, e2));
    }

    #[test]
    fn test_cluster_send_recv_causality() {
        let mut cluster = Cluster::new(2);
        
        let send = cluster.record_send(0, "send from 0");
        let recv = cluster.record_recv(1, send, "recv at 1");
        
        assert!(cluster.happened_before(send, recv));
    }

    #[test]
    fn test_cluster_reset() {
        let mut cluster = Cluster::new(3);
        
        cluster.crash_node(0);
        cluster.partition(&[&[1], &[2]]);
        cluster.advance_time(std::time::Duration::from_secs(1));
        cluster.record_event(1, "test");
        
        cluster.reset();
        
        assert_eq!(cluster.running_count(), 3);
        assert!(cluster.can_communicate(1, 2));
        assert_eq!(cluster.now(), Instant::from_nanos(0));
        assert_eq!(cluster.happens_before_graph().event_count(), 0);
    }

    #[test]
    fn test_cluster_nodes_mut() {
        let mut cluster = Cluster::new(2);
        
        for node in cluster.nodes_mut() {
            node.send_raw(0, vec![1, 2, 3]);
        }
        
        // Both nodes should have outgoing messages
        assert!(cluster.nodes()[0].outbox_len() > 0 || cluster.nodes()[1].outbox_len() > 0);
    }

    #[test]
    fn test_clock_skew() {
        let mut cluster = Cluster::new(2);
        
        // Default skew is 1.0
        assert_eq!(cluster.clock_skew(0), 1.0);
        assert_eq!(cluster.clock_skew(1), 1.0);
        
        // Set node 0 to run at 2x speed
        cluster.set_clock_skew(0, 2.0);
        assert_eq!(cluster.clock_skew(0), 2.0);
        
        // Advance global time by 1 second
        cluster.advance_time(std::time::Duration::from_secs(1));
        
        // Node 0 should see 2 seconds, node 1 sees 1 second
        let node0_time = cluster.node_time(0);
        let node1_time = cluster.node_time(1);
        
        assert_eq!(node0_time.as_nanos(), 2_000_000_000);
        assert_eq!(node1_time.as_nanos(), 1_000_000_000);
    }

    #[test]
    fn test_clock_jump_forward() {
        let mut cluster = Cluster::new(2);
        
        // Jump node 0 forward by 5 seconds
        cluster.clock_jump_forward(0, std::time::Duration::from_secs(5));
        
        // Node 0 should be 5 seconds ahead
        let node0_time = cluster.node_time(0);
        let node1_time = cluster.node_time(1);
        
        assert_eq!(node0_time.as_nanos(), 5_000_000_000);
        assert_eq!(node1_time.as_nanos(), 0);
        
        // Advance global time
        cluster.advance_time(std::time::Duration::from_secs(1));
        
        // Node 0 should still be 5 seconds ahead
        assert_eq!(cluster.node_time(0).as_nanos(), 6_000_000_000);
        assert_eq!(cluster.node_time(1).as_nanos(), 1_000_000_000);
    }

    #[test]
    fn test_clock_jump_backward() {
        let mut cluster = Cluster::new(2);
        
        // Advance time first
        cluster.advance_time(std::time::Duration::from_secs(10));
        
        // Jump node 0 backward by 3 seconds
        cluster.clock_jump(0, -3_000_000_000);
        
        // Node 0 should be 3 seconds behind
        assert_eq!(cluster.node_time(0).as_nanos(), 7_000_000_000);
        assert_eq!(cluster.node_time(1).as_nanos(), 10_000_000_000);
    }

    #[test]
    fn test_clear_clock_faults() {
        let mut cluster = Cluster::new(2);
        
        cluster.set_clock_skew(0, 2.0);
        cluster.clock_jump_forward(0, std::time::Duration::from_secs(5));
        cluster.advance_time(std::time::Duration::from_secs(1));
        
        // Node 0 has skew and offset
        assert!(cluster.node_time(0).as_nanos() > cluster.node_time(1).as_nanos());
        
        // Clear faults
        cluster.clear_clock_faults(0);
        
        // Now node 0 should match global time
        assert_eq!(cluster.clock_skew(0), 1.0);
        assert_eq!(cluster.clock_offset(0), 0);
        assert_eq!(cluster.node_time(0).as_nanos(), cluster.now().as_nanos());
    }

    #[test]
    fn test_reset_clears_clock_faults() {
        let mut cluster = Cluster::new(2);
        
        cluster.set_clock_skew(0, 2.0);
        cluster.clock_jump_forward(1, std::time::Duration::from_secs(10));
        
        cluster.reset();
        
        assert_eq!(cluster.clock_skew(0), 1.0);
        assert_eq!(cluster.clock_offset(1), 0);
    }
}
