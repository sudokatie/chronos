mod node;

pub use node::{Node, NodeState};

use std::collections::HashSet;

use crate::network::NetworkSim;
use crate::time::{Clock, Instant};
use crate::NodeId;

/// A simulated cluster of nodes.
pub struct Cluster {
    nodes: Vec<Node>,
    network: NetworkSim,
    clock: Clock,
    partitions: Vec<HashSet<NodeId>>,
}

impl Cluster {
    /// Create a new cluster with the given number of nodes.
    pub fn new(size: usize) -> Self {
        Self::with_seed(size, 0)
    }

    /// Create a new cluster with a specific random seed.
    pub fn with_seed(size: usize, seed: u64) -> Self {
        let nodes: Vec<Node> = (0..size as NodeId).map(Node::new).collect();
        let network = NetworkSim::with_seed(seed);
        let clock = Clock::new();

        Self {
            nodes,
            network,
            clock,
            partitions: Vec::new(),
        }
    }

    /// Get the number of nodes in the cluster.
    pub fn size(&self) -> usize {
        self.nodes.len()
    }

    /// Get a reference to a node by ID.
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id as usize)
    }

    /// Get a mutable reference to a node by ID.
    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.get_mut(id as usize)
    }

    /// Get the current simulated time.
    pub fn now(&self) -> Instant {
        self.clock.now()
    }

    /// Advance the simulated clock.
    pub fn advance_time(&mut self, duration: std::time::Duration) {
        self.clock.advance(duration);
        self.network.tick(self.clock.now());
        self.deliver_messages();
    }

    /// Advance time to a specific instant.
    pub fn advance_to(&mut self, instant: Instant) {
        self.clock.advance_to(instant);
        self.network.tick(self.clock.now());
        self.deliver_messages();
    }

    /// Deliver pending network messages to nodes.
    fn deliver_messages(&mut self) {
        let now = self.clock.now();
        
        // Collect outgoing messages from all nodes
        for node in &mut self.nodes {
            if node.is_running() {
                for msg in node.drain_outbox() {
                    let _ = self.network.send(msg.from, msg.to, msg.data.clone(), now);
                }
            }
        }

        // Deliver incoming messages
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
    fn can_communicate(&self, from: NodeId, to: NodeId) -> bool {
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
    }

    /// Heal all network partitions.
    pub fn heal_partition(&mut self) {
        self.partitions.clear();
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

    /// Run until the cluster stabilizes or max iterations reached.
    pub fn run_until_stable(&mut self, max_iterations: usize) -> bool {
        for _ in 0..max_iterations {
            if self.is_stable() {
                return true;
            }
            self.advance_time(std::time::Duration::from_millis(1));
        }
        false
    }

    /// Get count of running nodes.
    pub fn running_count(&self) -> usize {
        self.nodes.iter().filter(|n| n.is_running()).count()
    }

    /// Get count of crashed nodes.
    pub fn crashed_count(&self) -> usize {
        self.nodes.iter().filter(|n| n.is_crashed()).count()
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
    }
}
