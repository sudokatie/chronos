//! Happens-before tracking for causal ordering analysis.
//!
//! Tracks the causal relationships between events in the simulation
//! using vector clocks.

use std::collections::HashMap;

use petgraph::graph::{DiGraph, NodeIndex};

use crate::{EventId, NodeId};

/// A vector clock for tracking causal ordering.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VectorClock {
    /// Clock value for each node.
    clocks: HashMap<NodeId, u64>,
}

impl VectorClock {
    /// Create a new empty vector clock.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the clock value for a node.
    pub fn get(&self, node: NodeId) -> u64 {
        self.clocks.get(&node).copied().unwrap_or(0)
    }

    /// Increment the clock for a node.
    pub fn increment(&mut self, node: NodeId) {
        *self.clocks.entry(node).or_insert(0) += 1;
    }

    /// Merge another clock into this one (point-wise maximum).
    pub fn merge(&mut self, other: &VectorClock) {
        for (&node, &value) in &other.clocks {
            let current = self.clocks.entry(node).or_insert(0);
            *current = (*current).max(value);
        }
    }

    /// Check if this clock happened before another.
    /// Returns true if all components of self are <= other, and at least one is <.
    pub fn happened_before(&self, other: &VectorClock) -> bool {
        let mut dominated = true;
        let mut strictly_less = false;

        // Check all nodes in self
        for (&node, &value) in &self.clocks {
            let other_value = other.get(node);
            if value > other_value {
                dominated = false;
                break;
            }
            if value < other_value {
                strictly_less = true;
            }
        }

        // Check nodes only in other
        if dominated {
            for (&node, &value) in &other.clocks {
                if !self.clocks.contains_key(&node) && value > 0 {
                    strictly_less = true;
                    break;
                }
            }
        }

        dominated && strictly_less
    }

    /// Check if this clock is concurrent with another.
    /// Events are concurrent if neither happened before the other.
    pub fn concurrent(&self, other: &VectorClock) -> bool {
        !self.happened_before(other) && !other.happened_before(self) && self != other
    }
}

/// An event in the happens-before graph.
#[derive(Debug, Clone)]
pub struct HBEvent {
    /// Unique event ID.
    pub id: EventId,
    /// Node that generated this event.
    pub node: NodeId,
    /// Event description.
    pub description: String,
    /// Vector clock at this event.
    pub clock: VectorClock,
}

/// Happens-before graph tracking causal relationships.
#[derive(Debug)]
pub struct HappensBeforeGraph {
    /// The actual graph.
    graph: DiGraph<EventId, ()>,
    /// Map from event ID to node index.
    event_to_node: HashMap<EventId, NodeIndex>,
    /// Map from event ID to event data.
    events: HashMap<EventId, HBEvent>,
    /// Current vector clock per node.
    node_clocks: HashMap<NodeId, VectorClock>,
    /// Next event ID.
    next_event_id: EventId,
}

impl HappensBeforeGraph {
    /// Create a new empty graph.
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            event_to_node: HashMap::new(),
            events: HashMap::new(),
            node_clocks: HashMap::new(),
            next_event_id: 0,
        }
    }

    /// Record a local event on a node.
    pub fn local_event(&mut self, node: NodeId, description: impl Into<String>) -> EventId {
        let id = self.next_event_id;
        self.next_event_id += 1;

        // Get or create clock for this node
        let clock = self.node_clocks.entry(node).or_default();
        clock.increment(node);
        let event_clock = clock.clone();

        // Add to graph
        let graph_node = self.graph.add_node(id);
        self.event_to_node.insert(id, graph_node);

        // Store event
        let event = HBEvent {
            id,
            node,
            description: description.into(),
            clock: event_clock,
        };
        self.events.insert(id, event);

        // Add edge from previous event on this node (if any)
        if id > 0 {
            // Find the most recent event on this node
            for prev_id in (0..id).rev() {
                if let Some(prev_event) = self.events.get(&prev_id) {
                    if prev_event.node == node {
                        if let (Some(&prev_node), Some(&curr_node)) = 
                            (self.event_to_node.get(&prev_id), self.event_to_node.get(&id)) 
                        {
                            self.graph.add_edge(prev_node, curr_node, ());
                        }
                        break;
                    }
                }
            }
        }

        id
    }

    /// Record a send event.
    pub fn send_event(&mut self, from_node: NodeId, description: impl Into<String>) -> EventId {
        self.local_event(from_node, description)
    }

    /// Record a receive event, linking it to the send event.
    pub fn recv_event(&mut self, to_node: NodeId, send_event_id: EventId, description: impl Into<String>) -> EventId {
        // First, merge the sender's clock into our clock
        if let Some(send_event) = self.events.get(&send_event_id) {
            let clock = self.node_clocks.entry(to_node).or_default();
            clock.merge(&send_event.clock);
        }

        let recv_id = self.local_event(to_node, description);

        // Add edge from send to receive
        if let (Some(&send_node), Some(&recv_node)) = 
            (self.event_to_node.get(&send_event_id), self.event_to_node.get(&recv_id)) 
        {
            self.graph.add_edge(send_node, recv_node, ());
        }

        recv_id
    }

    /// Check if event A happened before event B.
    pub fn happened_before(&self, a: EventId, b: EventId) -> bool {
        if a == b {
            return false;
        }

        // Use vector clocks for comparison
        match (self.events.get(&a), self.events.get(&b)) {
            (Some(event_a), Some(event_b)) => event_a.clock.happened_before(&event_b.clock),
            _ => false,
        }
    }

    /// Check if two events are concurrent (neither happened before the other).
    pub fn concurrent(&self, a: EventId, b: EventId) -> bool {
        if a == b {
            return false;
        }

        match (self.events.get(&a), self.events.get(&b)) {
            (Some(event_a), Some(event_b)) => event_a.clock.concurrent(&event_b.clock),
            _ => false,
        }
    }

    /// Get an event by ID.
    pub fn get_event(&self, id: EventId) -> Option<&HBEvent> {
        self.events.get(&id)
    }

    /// Get all events.
    pub fn events(&self) -> impl Iterator<Item = &HBEvent> {
        self.events.values()
    }

    /// Get the number of events.
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// Reset the graph.
    pub fn reset(&mut self) {
        self.graph = DiGraph::new();
        self.event_to_node.clear();
        self.events.clear();
        self.node_clocks.clear();
        self.next_event_id = 0;
    }
}

impl Default for HappensBeforeGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vector_clock_new() {
        let vc = VectorClock::new();
        assert_eq!(vc.get(0), 0);
        assert_eq!(vc.get(1), 0);
    }

    #[test]
    fn test_vector_clock_increment() {
        let mut vc = VectorClock::new();
        vc.increment(0);
        vc.increment(0);
        vc.increment(1);
        
        assert_eq!(vc.get(0), 2);
        assert_eq!(vc.get(1), 1);
    }

    #[test]
    fn test_vector_clock_merge() {
        let mut vc1 = VectorClock::new();
        vc1.increment(0);
        vc1.increment(0);
        
        let mut vc2 = VectorClock::new();
        vc2.increment(1);
        vc2.increment(1);
        vc2.increment(1);
        
        vc1.merge(&vc2);
        
        assert_eq!(vc1.get(0), 2);
        assert_eq!(vc1.get(1), 3);
    }

    #[test]
    fn test_vector_clock_happened_before() {
        let mut vc1 = VectorClock::new();
        vc1.increment(0);
        
        let mut vc2 = VectorClock::new();
        vc2.increment(0);
        vc2.increment(0);
        
        assert!(vc1.happened_before(&vc2));
        assert!(!vc2.happened_before(&vc1));
    }

    #[test]
    fn test_vector_clock_concurrent() {
        let mut vc1 = VectorClock::new();
        vc1.increment(0);
        
        let mut vc2 = VectorClock::new();
        vc2.increment(1);
        
        assert!(vc1.concurrent(&vc2));
        assert!(vc2.concurrent(&vc1));
    }

    #[test]
    fn test_hb_graph_local_events() {
        let mut hb = HappensBeforeGraph::new();
        
        let e1 = hb.local_event(0, "event 1");
        let e2 = hb.local_event(0, "event 2");
        let e3 = hb.local_event(0, "event 3");
        
        // Sequential events on same node
        assert!(hb.happened_before(e1, e2));
        assert!(hb.happened_before(e2, e3));
        assert!(hb.happened_before(e1, e3));
        
        // Not concurrent
        assert!(!hb.concurrent(e1, e2));
    }

    #[test]
    fn test_hb_graph_concurrent_nodes() {
        let mut hb = HappensBeforeGraph::new();
        
        let e1 = hb.local_event(0, "node 0 event");
        let e2 = hb.local_event(1, "node 1 event");
        
        // Events on different nodes without communication are concurrent
        assert!(hb.concurrent(e1, e2));
        assert!(!hb.happened_before(e1, e2));
        assert!(!hb.happened_before(e2, e1));
    }

    #[test]
    fn test_hb_graph_send_recv() {
        let mut hb = HappensBeforeGraph::new();
        
        // Node 0 sends to node 1
        let send = hb.send_event(0, "send");
        let recv = hb.recv_event(1, send, "recv");
        
        // Send happens before receive
        assert!(hb.happened_before(send, recv));
        assert!(!hb.happened_before(recv, send));
        assert!(!hb.concurrent(send, recv));
    }

    #[test]
    fn test_hb_graph_causal_chain() {
        let mut hb = HappensBeforeGraph::new();
        
        // Node 0: e1 -> send
        let e1 = hb.local_event(0, "e1");
        let send = hb.send_event(0, "send");
        
        // Node 1: recv -> e2
        let recv = hb.recv_event(1, send, "recv");
        let e2 = hb.local_event(1, "e2");
        
        // Full causal chain
        assert!(hb.happened_before(e1, e2));
        assert!(hb.happened_before(send, recv));
        assert!(hb.happened_before(e1, recv));
    }

    #[test]
    fn test_hb_graph_reset() {
        let mut hb = HappensBeforeGraph::new();
        hb.local_event(0, "e1");
        hb.local_event(1, "e2");
        
        assert_eq!(hb.event_count(), 2);
        
        hb.reset();
        
        assert_eq!(hb.event_count(), 0);
    }
}
