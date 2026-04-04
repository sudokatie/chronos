//! Network message types for simulation.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::time::Instant;
use crate::NodeId;

/// Unique identifier for a message.
pub type MessageId = u64;

/// Global counter for generating unique message IDs.
static NEXT_MESSAGE_ID: AtomicU64 = AtomicU64::new(0);

/// Generates a new unique message ID.
pub fn next_message_id() -> MessageId {
    NEXT_MESSAGE_ID.fetch_add(1, Ordering::SeqCst)
}

/// Resets the message ID counter (for testing).
#[cfg(test)]
pub fn reset_message_ids() {
    NEXT_MESSAGE_ID.store(0, Ordering::SeqCst);
}

/// A network message between nodes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    /// Unique identifier for this message.
    pub id: MessageId,
    /// Source node.
    pub from: NodeId,
    /// Destination node.
    pub to: NodeId,
    /// Message payload.
    pub data: Vec<u8>,
    /// Simulated time when the message was sent.
    pub sent_at: Instant,
}

impl Message {
    /// Creates a new message with a unique ID.
    pub fn new(from: NodeId, to: NodeId, data: Vec<u8>, sent_at: Instant) -> Self {
        Self {
            id: next_message_id(),
            from,
            to,
            data,
            sent_at,
        }
    }

    /// Creates a message with a specific ID (for testing/replay).
    pub fn with_id(id: MessageId, from: NodeId, to: NodeId, data: Vec<u8>, sent_at: Instant) -> Self {
        Self {
            id,
            from,
            to,
            data,
            sent_at,
        }
    }

    /// Returns the size of the message payload in bytes.
    pub fn size(&self) -> usize {
        self.data.len()
    }

    /// Returns true if this is an empty message.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

/// A message in flight with scheduled delivery time.
#[derive(Clone, Debug)]
pub(super) struct InFlightMessage {
    /// The message being delivered.
    pub msg: Message,
    /// Simulated time when the message will be delivered.
    pub deliver_at: Instant,
}

impl InFlightMessage {
    /// Creates a new in-flight message.
    pub fn new(msg: Message, deliver_at: Instant) -> Self {
        Self { msg, deliver_at }
    }

    /// Returns the latency (delivery time - sent time).
    #[allow(dead_code)]
    pub fn latency(&self) -> std::time::Duration {
        self.deliver_at
            .duration_since(self.msg.sent_at)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_creation() {
        reset_message_ids();
        
        let msg = Message::new(1, 2, vec![1, 2, 3], Instant::from_nanos(100));
        
        assert_eq!(msg.from, 1);
        assert_eq!(msg.to, 2);
        assert_eq!(msg.data, vec![1, 2, 3]);
        assert_eq!(msg.sent_at, Instant::from_nanos(100));
    }

    #[test]
    fn test_message_size() {
        let msg = Message::with_id(0, 1, 2, vec![0; 100], Instant::from_nanos(0));
        assert_eq!(msg.size(), 100);
        
        let empty_msg = Message::with_id(0, 1, 2, vec![], Instant::from_nanos(0));
        assert_eq!(empty_msg.size(), 0);
        assert!(empty_msg.is_empty());
    }

    #[test]
    fn test_unique_message_ids() {
        reset_message_ids();
        
        let msg1 = Message::new(1, 2, vec![], Instant::from_nanos(0));
        let msg2 = Message::new(1, 2, vec![], Instant::from_nanos(0));
        let msg3 = Message::new(1, 2, vec![], Instant::from_nanos(0));
        
        assert_ne!(msg1.id, msg2.id);
        assert_ne!(msg2.id, msg3.id);
        assert_ne!(msg1.id, msg3.id);
    }

    #[test]
    fn test_message_clone() {
        let original = Message::with_id(42, 1, 2, vec![1, 2, 3], Instant::from_nanos(100));
        let cloned = original.clone();
        
        assert_eq!(original, cloned);
        assert_eq!(original.id, cloned.id);
        assert_eq!(original.data, cloned.data);
    }

    #[test]
    fn test_in_flight_message() {
        let msg = Message::with_id(0, 1, 2, vec![], Instant::from_nanos(100));
        let in_flight = InFlightMessage::new(msg, Instant::from_nanos(200));
        
        assert_eq!(in_flight.latency(), std::time::Duration::from_nanos(100));
    }

    #[test]
    fn test_with_id() {
        let msg = Message::with_id(999, 1, 2, vec![42], Instant::from_nanos(50));
        assert_eq!(msg.id, 999);
    }
}
