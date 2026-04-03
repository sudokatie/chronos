use std::collections::VecDeque;

use crate::network::Message;
use crate::NodeId;

/// State of a simulated node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeState {
    /// Node is running normally
    Running,
    /// Node has crashed
    Crashed,
    /// Node is paused (for debugging)
    Paused,
}

/// A simulated node in the cluster.
#[derive(Debug)]
pub struct Node {
    id: NodeId,
    state: NodeState,
    inbox: VecDeque<Message>,
    outbox: VecDeque<Message>,
}

impl Node {
    /// Create a new node with the given ID.
    pub fn new(id: NodeId) -> Self {
        Self {
            id,
            state: NodeState::Running,
            inbox: VecDeque::new(),
            outbox: VecDeque::new(),
        }
    }

    /// Get the node's ID.
    pub fn id(&self) -> NodeId {
        self.id
    }

    /// Get the node's current state.
    pub fn state(&self) -> NodeState {
        self.state
    }

    /// Check if the node is running.
    pub fn is_running(&self) -> bool {
        self.state == NodeState::Running
    }

    /// Check if the node is crashed.
    pub fn is_crashed(&self) -> bool {
        self.state == NodeState::Crashed
    }

    /// Crash the node, clearing its state.
    pub fn crash(&mut self) {
        self.state = NodeState::Crashed;
        self.inbox.clear();
        self.outbox.clear();
    }

    /// Restart a crashed node.
    pub fn restart(&mut self) {
        if self.state == NodeState::Crashed {
            self.state = NodeState::Running;
        }
    }

    /// Pause the node.
    pub fn pause(&mut self) {
        if self.state == NodeState::Running {
            self.state = NodeState::Paused;
        }
    }

    /// Resume a paused node.
    pub fn resume(&mut self) {
        if self.state == NodeState::Paused {
            self.state = NodeState::Running;
        }
    }

    /// Enqueue a message to the node's inbox.
    pub fn enqueue(&mut self, msg: Message) {
        if self.state == NodeState::Running {
            self.inbox.push_back(msg);
        }
    }

    /// Dequeue a message from the node's inbox.
    pub fn dequeue(&mut self) -> Option<Message> {
        if self.state == NodeState::Running {
            self.inbox.pop_front()
        } else {
            None
        }
    }

    /// Send a message (add to outbox).
    pub fn send(&mut self, msg: Message) {
        if self.state == NodeState::Running {
            self.outbox.push_back(msg);
        }
    }

    /// Take all messages from the outbox.
    pub fn drain_outbox(&mut self) -> Vec<Message> {
        self.outbox.drain(..).collect()
    }

    /// Check if the node has pending messages.
    pub fn has_pending(&self) -> bool {
        !self.inbox.is_empty() || !self.outbox.is_empty()
    }

    /// Get the number of pending inbox messages.
    pub fn inbox_len(&self) -> usize {
        self.inbox.len()
    }

    /// Get the number of pending outbox messages.
    pub fn outbox_len(&self) -> usize {
        self.outbox.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::Instant;

    fn test_message(from: NodeId, to: NodeId) -> Message {
        Message::new(from, to, vec![1, 2, 3], crate::time::Instant::from_nanos(0))
    }

    #[test]
    fn test_node_new() {
        let node = Node::new(1);
        assert_eq!(node.id(), 1);
        assert_eq!(node.state(), NodeState::Running);
        assert!(node.is_running());
    }

    #[test]
    fn test_node_crash() {
        let mut node = Node::new(1);
        node.enqueue(test_message(0, 1));
        
        node.crash();
        
        assert!(node.is_crashed());
        assert_eq!(node.inbox_len(), 0);
    }

    #[test]
    fn test_node_restart() {
        let mut node = Node::new(1);
        node.crash();
        node.restart();
        
        assert!(node.is_running());
    }

    #[test]
    fn test_node_pause_resume() {
        let mut node = Node::new(1);
        
        node.pause();
        assert_eq!(node.state(), NodeState::Paused);
        
        node.resume();
        assert!(node.is_running());
    }

    #[test]
    fn test_node_enqueue_dequeue() {
        let mut node = Node::new(1);
        let msg = test_message(0, 1);
        
        node.enqueue(msg.clone());
        assert_eq!(node.inbox_len(), 1);
        
        let received = node.dequeue().unwrap();
        assert_eq!(received.from, msg.from);
        assert_eq!(node.inbox_len(), 0);
    }

    #[test]
    fn test_crashed_node_drops_messages() {
        let mut node = Node::new(1);
        node.crash();
        
        node.enqueue(test_message(0, 1));
        assert_eq!(node.inbox_len(), 0);
        
        assert!(node.dequeue().is_none());
    }

    #[test]
    fn test_node_send_drain() {
        let mut node = Node::new(1);
        
        node.send(test_message(1, 2));
        node.send(test_message(1, 3));
        
        assert_eq!(node.outbox_len(), 2);
        
        let messages = node.drain_outbox();
        assert_eq!(messages.len(), 2);
        assert_eq!(node.outbox_len(), 0);
    }

    #[test]
    fn test_crashed_node_no_send() {
        let mut node = Node::new(1);
        node.crash();
        
        node.send(test_message(1, 2));
        assert_eq!(node.outbox_len(), 0);
    }
}
