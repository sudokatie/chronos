//! Message sequence diagram generation

use crate::recording::{Event, EventPayload};
use std::collections::{HashMap, HashSet};

/// A message in the sequence diagram
#[derive(Debug, Clone)]
pub struct SequenceMessage {
    /// Source task ID
    pub from: u32,
    /// Destination task ID
    pub to: u32,
    /// Send timestamp
    pub send_time: u64,
    /// Receive timestamp (if received)
    pub recv_time: Option<u64>,
    /// Message size in bytes
    pub size: usize,
    /// Message label
    pub label: String,
}

/// Sequence diagram builder
pub struct SequenceDiagram {
    /// Tasks/participants in the diagram
    participants: HashMap<u32, String>,
    /// Messages between tasks
    messages: Vec<SequenceMessage>,
    /// Pending sends (not yet received)
    pending: HashMap<(u32, u32, usize), u64>, // (from, to, size) -> send_time
}

impl SequenceDiagram {
    /// Create a new sequence diagram
    pub fn new() -> Self {
        Self {
            participants: HashMap::new(),
            messages: Vec::new(),
            pending: HashMap::new(),
        }
    }

    /// Add an event
    pub fn add_event(&mut self, event: &Event) {
        match &event.payload {
            EventPayload::TaskSpawn { name, .. } => {
                self.participants.insert(event.task_id, name.clone());
            }
            EventPayload::NetSend { dst, data } => {
                // Record pending send
                let key = (event.task_id, *dst, data.len());
                self.pending.insert(key, event.timestamp);
            }
            EventPayload::NetRecv { src, data } => {
                // Match with pending send
                let key = (*src, event.task_id, data.len());
                let send_time = self.pending.remove(&key);
                
                self.messages.push(SequenceMessage {
                    from: *src,
                    to: event.task_id,
                    send_time: send_time.unwrap_or(0),
                    recv_time: Some(event.timestamp),
                    size: data.len(),
                    label: format!("{} bytes", data.len()),
                });
            }
            _ => {}
        }
    }

    /// Get all participants
    pub fn participants(&self) -> &HashMap<u32, String> {
        &self.participants
    }

    /// Get all messages
    pub fn messages(&self) -> &[SequenceMessage] {
        &self.messages
    }

    /// Get message count
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Get participant count
    pub fn participant_count(&self) -> usize {
        self.participants.len()
    }

    /// Get unique participant IDs involved in messages
    pub fn active_participants(&self) -> HashSet<u32> {
        let mut active = HashSet::new();
        for msg in &self.messages {
            active.insert(msg.from);
            active.insert(msg.to);
        }
        active
    }

    /// Calculate total bytes transferred
    pub fn total_bytes(&self) -> usize {
        self.messages.iter().map(|m| m.size).sum()
    }

    /// Get messages involving a specific task
    pub fn messages_for_task(&self, task_id: u32) -> Vec<&SequenceMessage> {
        self.messages.iter()
            .filter(|m| m.from == task_id || m.to == task_id)
            .collect()
    }

    /// Get average message latency
    pub fn avg_latency_ns(&self) -> Option<u64> {
        let latencies: Vec<u64> = self.messages.iter()
            .filter_map(|m| m.recv_time.map(|r| r.saturating_sub(m.send_time)))
            .collect();
        
        if latencies.is_empty() {
            None
        } else {
            Some(latencies.iter().sum::<u64>() / latencies.len() as u64)
        }
    }
}

impl Default for SequenceDiagram {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::Event;

    #[test]
    fn empty_diagram() {
        let diagram = SequenceDiagram::new();
        assert_eq!(diagram.message_count(), 0);
        assert_eq!(diagram.participant_count(), 0);
    }

    #[test]
    fn single_message() {
        let mut diagram = SequenceDiagram::new();
        
        diagram.add_event(&Event::task_spawn(1, 0, "sender".to_string(), 0));
        diagram.add_event(&Event::task_spawn(2, 0, "receiver".to_string(), 0));
        diagram.add_event(&Event::net_send(1, 100, 2, vec![1, 2, 3]));
        diagram.add_event(&Event::net_recv(2, 150, 1, vec![1, 2, 3]));
        
        assert_eq!(diagram.participant_count(), 2);
        assert_eq!(diagram.message_count(), 1);
        
        let msg = &diagram.messages()[0];
        assert_eq!(msg.from, 1);
        assert_eq!(msg.to, 2);
        assert_eq!(msg.send_time, 100);
        assert_eq!(msg.recv_time, Some(150));
        assert_eq!(msg.size, 3);
    }

    #[test]
    fn multiple_messages() {
        let mut diagram = SequenceDiagram::new();
        
        diagram.add_event(&Event::task_spawn(1, 0, "a".to_string(), 0));
        diagram.add_event(&Event::task_spawn(2, 0, "b".to_string(), 0));
        
        // Message 1: a -> b
        diagram.add_event(&Event::net_send(1, 100, 2, vec![1]));
        diagram.add_event(&Event::net_recv(2, 150, 1, vec![1]));
        
        // Message 2: b -> a
        diagram.add_event(&Event::net_send(2, 200, 1, vec![2, 3]));
        diagram.add_event(&Event::net_recv(1, 250, 2, vec![2, 3]));
        
        assert_eq!(diagram.message_count(), 2);
        assert_eq!(diagram.total_bytes(), 3);
    }

    #[test]
    fn active_participants() {
        let mut diagram = SequenceDiagram::new();
        
        diagram.add_event(&Event::task_spawn(1, 0, "a".to_string(), 0));
        diagram.add_event(&Event::task_spawn(2, 0, "b".to_string(), 0));
        diagram.add_event(&Event::task_spawn(3, 0, "c".to_string(), 0)); // No messages
        
        diagram.add_event(&Event::net_send(1, 100, 2, vec![1]));
        diagram.add_event(&Event::net_recv(2, 150, 1, vec![1]));
        
        let active = diagram.active_participants();
        assert!(active.contains(&1));
        assert!(active.contains(&2));
        assert!(!active.contains(&3));
    }

    #[test]
    fn average_latency() {
        let mut diagram = SequenceDiagram::new();
        
        diagram.add_event(&Event::task_spawn(1, 0, "a".to_string(), 0));
        diagram.add_event(&Event::task_spawn(2, 0, "b".to_string(), 0));
        
        // Latency: 50
        diagram.add_event(&Event::net_send(1, 100, 2, vec![1]));
        diagram.add_event(&Event::net_recv(2, 150, 1, vec![1]));
        
        // Latency: 100
        diagram.add_event(&Event::net_send(1, 200, 2, vec![1]));
        diagram.add_event(&Event::net_recv(2, 300, 1, vec![1]));
        
        assert_eq!(diagram.avg_latency_ns(), Some(75)); // (50 + 100) / 2
    }

    #[test]
    fn messages_for_task() {
        let mut diagram = SequenceDiagram::new();
        
        diagram.add_event(&Event::task_spawn(1, 0, "a".to_string(), 0));
        diagram.add_event(&Event::task_spawn(2, 0, "b".to_string(), 0));
        
        diagram.add_event(&Event::net_send(1, 100, 2, vec![1]));
        diagram.add_event(&Event::net_recv(2, 150, 1, vec![1]));
        
        let msgs = diagram.messages_for_task(1);
        assert_eq!(msgs.len(), 1);
    }
}
