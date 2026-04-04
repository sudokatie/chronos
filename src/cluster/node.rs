//! Node abstraction for cluster simulation.

use std::any::Any;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::sync::Arc;

use crate::network::Message as NetMessage;
use crate::runtime::TaskHandle;
use crate::time::Instant;
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

/// A message that can be sent between nodes.
pub trait Message: Send + Sync + 'static {
    /// The response type for this message.
    type Response: Send + 'static;
    
    /// Serialize to bytes.
    fn to_bytes(&self) -> Vec<u8>;
    
    /// Deserialize from bytes.
    fn from_bytes(bytes: &[u8]) -> Option<Self> where Self: Sized;
}

/// A query that can be sent to a node.
pub trait Query: Send + Sync + 'static {
    /// The result type for this query.
    type Result: Send + 'static;
    
    /// Execute the query against node state.
    fn execute(&self, state: &dyn Any) -> Self::Result;
}

/// Handler for incoming messages.
pub trait MessageHandler: Send + Sync {
    /// Handle an incoming message and return response bytes.
    fn handle(&self, from: NodeId, data: &[u8]) -> Option<Vec<u8>>;
}

/// Internal message with metadata.
#[derive(Debug, Clone)]
pub struct InternalMessage {
    pub from: NodeId,
    pub to: NodeId,
    pub data: Vec<u8>,
    pub timestamp: Instant,
    pub request_id: u64,
}

/// Pending request waiting for response.
struct PendingRequest {
    response_tx: std::sync::mpsc::Sender<Vec<u8>>,
}

/// A simulated node in the cluster.
pub struct Node {
    id: NodeId,
    state: NodeState,
    inbox: VecDeque<InternalMessage>,
    outbox: VecDeque<InternalMessage>,
    /// User-defined state
    user_state: Option<Box<dyn Any + Send + Sync>>,
    /// Message handler
    handler: Option<Arc<dyn MessageHandler>>,
    /// Spawned tasks
    tasks: Vec<TaskHandle>,
    /// Pending requests
    pending_requests: HashMap<u64, PendingRequest>,
    /// Next request ID
    next_request_id: u64,
}

impl std::fmt::Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node")
            .field("id", &self.id)
            .field("state", &self.state)
            .field("inbox_len", &self.inbox.len())
            .field("outbox_len", &self.outbox.len())
            .field("tasks", &self.tasks.len())
            .finish()
    }
}

impl Node {
    /// Create a new node with the given ID.
    pub fn new(id: NodeId) -> Self {
        Self {
            id,
            state: NodeState::Running,
            inbox: VecDeque::new(),
            outbox: VecDeque::new(),
            user_state: None,
            handler: None,
            tasks: Vec::new(),
            pending_requests: HashMap::new(),
            next_request_id: 0,
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

    /// Check if the node is alive (running or paused).
    pub fn is_alive(&self) -> bool {
        self.state != NodeState::Crashed
    }

    /// Set user-defined state.
    pub fn set_state<S: Any + Send + Sync>(&mut self, state: S) {
        self.user_state = Some(Box::new(state));
    }

    /// Get user-defined state.
    pub fn get_state<S: Any + Send + Sync>(&self) -> Option<&S> {
        self.user_state.as_ref()?.downcast_ref()
    }

    /// Get mutable user-defined state.
    pub fn get_state_mut<S: Any + Send + Sync>(&mut self) -> Option<&mut S> {
        self.user_state.as_mut()?.downcast_mut()
    }

    /// Set message handler.
    pub fn set_handler<H: MessageHandler + 'static>(&mut self, handler: H) {
        self.handler = Some(Arc::new(handler));
    }

    /// Spawn a task on this node.
    ///
    /// The task will be executed within the simulation context.
    /// Returns a handle that can be used to track task completion.
    pub fn spawn<F>(&mut self, future: F) -> TaskHandle
    where
        F: Future<Output = ()> + Send + 'static,
    {
        // Use the simulation context's spawn if available
        if crate::sim::is_simulation() {
            let handle = crate::sim::spawn_task(future);
            self.tasks.push(handle.clone());
            handle
        } else {
            // Fallback: create placeholder handle
            let handle = TaskHandle::new(self.tasks.len() as u32);
            self.tasks.push(handle.clone());
            handle
        }
    }

    /// Send a message to another node and wait for response.
    ///
    /// This queues the message for delivery and waits for a response.
    /// The response will be delivered when the receiving node processes
    /// the message and sends a reply.
    pub async fn send<M: Message>(&mut self, to: NodeId, msg: M) -> Option<M::Response>
    where
        M::Response: for<'de> serde::Deserialize<'de>,
    {
        if self.state != NodeState::Running {
            return None;
        }

        let request_id = self.next_request_id;
        self.next_request_id += 1;

        let internal_msg = InternalMessage {
            from: self.id,
            to,
            data: msg.to_bytes(),
            timestamp: Instant::from_nanos(0), // Will be set by cluster
            request_id,
        };

        self.outbox.push_back(internal_msg);

        // Create a response channel
        let (tx, rx) = std::sync::mpsc::channel();
        self.pending_requests.insert(request_id, PendingRequest { response_tx: tx });

        // Yield to allow message delivery
        crate::sim::time::yield_now().await;

        // Try to receive response (non-blocking check)
        match rx.try_recv() {
            Ok(response_bytes) => {
                // Deserialize the response
                bincode::deserialize(&response_bytes).ok()
            }
            Err(_) => None,
        }
    }

    /// Send a message and wait for response with timeout.
    pub async fn send_with_timeout<M: Message>(
        &mut self, 
        to: NodeId, 
        msg: M,
        timeout: std::time::Duration,
    ) -> Option<M::Response>
    where
        M::Response: for<'de> serde::Deserialize<'de>,
    {
        if self.state != NodeState::Running {
            return None;
        }

        let request_id = self.next_request_id;
        self.next_request_id += 1;

        let internal_msg = InternalMessage {
            from: self.id,
            to,
            data: msg.to_bytes(),
            timestamp: Instant::from_nanos(0),
            request_id,
        };

        self.outbox.push_back(internal_msg);

        let (tx, rx) = std::sync::mpsc::channel();
        self.pending_requests.insert(request_id, PendingRequest { response_tx: tx });

        let deadline = crate::sim::time::now().saturating_add(timeout);
        
        // Poll for response until timeout
        while crate::sim::time::now() < deadline {
            if let Ok(response_bytes) = rx.try_recv() {
                self.pending_requests.remove(&request_id);
                return bincode::deserialize(&response_bytes).ok();
            }
            crate::sim::time::yield_now().await;
        }

        // Timeout - remove pending request
        self.pending_requests.remove(&request_id);
        None
    }

    /// Deliver a response to a pending request.
    pub fn deliver_response(&mut self, request_id: u64, response: Vec<u8>) -> bool {
        if let Some(pending) = self.pending_requests.remove(&request_id) {
            pending.response_tx.send(response).is_ok()
        } else {
            false
        }
    }

    /// Send raw bytes to another node.
    pub fn send_raw(&mut self, to: NodeId, data: Vec<u8>) {
        if self.state != NodeState::Running {
            return;
        }

        let request_id = self.next_request_id;
        self.next_request_id += 1;

        let internal_msg = InternalMessage {
            from: self.id,
            to,
            data,
            timestamp: Instant::from_nanos(0),
            request_id,
        };

        self.outbox.push_back(internal_msg);
    }

    /// Query local state.
    pub fn query<Q: Query>(&self, query: Q) -> Option<Q::Result> {
        if self.state != NodeState::Running {
            return None;
        }

        self.user_state.as_ref().map(|state| query.execute(state.as_ref()))
    }

    /// Crash the node, clearing its state.
    pub fn crash(&mut self) {
        self.state = NodeState::Crashed;
        self.inbox.clear();
        self.outbox.clear();
        self.pending_requests.clear();
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

    /// Enqueue an internal message to the node's inbox.
    pub fn enqueue_internal(&mut self, msg: InternalMessage) {
        if self.state == NodeState::Running {
            self.inbox.push_back(msg);
        }
    }

    /// Enqueue a network message to the node's inbox.
    pub fn enqueue(&mut self, msg: NetMessage) {
        if self.state == NodeState::Running {
            self.inbox.push_back(InternalMessage {
                from: msg.from,
                to: msg.to,
                data: msg.data,
                timestamp: msg.sent_at,
                request_id: 0,
            });
        }
    }

    /// Dequeue a message from the node's inbox.
    pub fn dequeue(&mut self) -> Option<InternalMessage> {
        if self.state == NodeState::Running {
            self.inbox.pop_front()
        } else {
            None
        }
    }

    /// Process incoming messages using the handler.
    pub fn process_messages(&mut self) {
        if self.state != NodeState::Running {
            return;
        }

        let handler = match &self.handler {
            Some(h) => h.clone(),
            None => return,
        };

        while let Some(msg) = self.inbox.pop_front() {
            if let Some(response) = handler.handle(msg.from, &msg.data) {
                // Send response back
                self.outbox.push_back(InternalMessage {
                    from: self.id,
                    to: msg.from,
                    data: response,
                    timestamp: msg.timestamp,
                    request_id: msg.request_id,
                });
            }
        }
    }

    /// Take all messages from the outbox (for cluster to deliver).
    pub fn drain_outbox(&mut self) -> Vec<InternalMessage> {
        self.outbox.drain(..).collect()
    }

    /// Convert outbox to network messages.
    pub fn drain_outbox_as_net(&mut self) -> Vec<NetMessage> {
        self.outbox.drain(..).map(|m| {
            NetMessage::new(m.from, m.to, m.data, m.timestamp)
        }).collect()
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

    /// Get the number of spawned tasks.
    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    /// Get completed task count.
    pub fn completed_task_count(&self) -> usize {
        self.tasks.iter().filter(|t| t.is_complete()).count()
    }
}

/// Simple byte message implementation.
#[derive(Debug, Clone)]
pub struct ByteMessage(pub Vec<u8>);

impl Message for ByteMessage {
    type Response = ByteMessage;

    fn to_bytes(&self) -> Vec<u8> {
        self.0.clone()
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        Some(ByteMessage(bytes.to_vec()))
    }
}

/// Echo handler that returns received messages.
pub struct EchoHandler;

impl MessageHandler for EchoHandler {
    fn handle(&self, _from: NodeId, data: &[u8]) -> Option<Vec<u8>> {
        Some(data.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_message(from: NodeId, to: NodeId) -> NetMessage {
        NetMessage::new(from, to, vec![1, 2, 3], Instant::from_nanos(0))
    }

    #[test]
    fn test_node_new() {
        let node = Node::new(1);
        assert_eq!(node.id(), 1);
        assert_eq!(node.state(), NodeState::Running);
        assert!(node.is_running());
        assert!(node.is_alive());
    }

    #[test]
    fn test_node_crash() {
        let mut node = Node::new(1);
        node.enqueue(test_message(0, 1));
        
        node.crash();
        
        assert!(node.is_crashed());
        assert!(!node.is_alive());
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
        assert!(node.is_alive());
        
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
    fn test_node_send_raw() {
        let mut node = Node::new(1);
        
        node.send_raw(2, vec![1, 2, 3]);
        node.send_raw(3, vec![4, 5, 6]);
        
        assert_eq!(node.outbox_len(), 2);
        
        let messages = node.drain_outbox();
        assert_eq!(messages.len(), 2);
        assert_eq!(node.outbox_len(), 0);
    }

    #[test]
    fn test_crashed_node_no_send() {
        let mut node = Node::new(1);
        node.crash();
        
        node.send_raw(2, vec![1, 2, 3]);
        assert_eq!(node.outbox_len(), 0);
    }

    #[test]
    fn test_node_user_state() {
        let mut node = Node::new(1);
        
        #[derive(Debug, PartialEq)]
        struct Counter(u32);
        
        node.set_state(Counter(42));
        
        assert_eq!(node.get_state::<Counter>(), Some(&Counter(42)));
        
        if let Some(state) = node.get_state_mut::<Counter>() {
            state.0 += 1;
        }
        
        assert_eq!(node.get_state::<Counter>(), Some(&Counter(43)));
    }

    #[test]
    fn test_node_query() {
        let mut node = Node::new(1);
        
        struct Value(i32);
        
        struct GetValue;
        impl Query for GetValue {
            type Result = i32;
            fn execute(&self, state: &dyn Any) -> i32 {
                state.downcast_ref::<Value>().map(|v| v.0).unwrap_or(0)
            }
        }
        
        node.set_state(Value(100));
        
        let result = node.query(GetValue);
        assert_eq!(result, Some(100));
    }

    #[test]
    fn test_echo_handler() {
        let mut node = Node::new(1);
        node.set_handler(EchoHandler);
        
        node.enqueue_internal(InternalMessage {
            from: 2,
            to: 1,
            data: vec![1, 2, 3],
            timestamp: Instant::from_nanos(0),
            request_id: 1,
        });
        
        node.process_messages();
        
        assert_eq!(node.outbox_len(), 1);
        let response = node.drain_outbox().pop().unwrap();
        assert_eq!(response.data, vec![1, 2, 3]);
        assert_eq!(response.to, 2);
    }

    #[test]
    fn test_node_spawn() {
        let mut node = Node::new(1);
        
        let _handle = node.spawn(async {});
        
        assert_eq!(node.task_count(), 1);
    }
}
