//! Timeline view generation for test execution

use crate::recording::{Event, EventPayload};
use std::collections::HashMap;

/// Task info for timeline display
#[derive(Debug, Clone)]
pub struct TaskInfo {
    /// Task ID
    pub id: u32,
    /// Task name
    pub name: String,
    /// Parent task ID
    pub parent: u32,
    /// Spawn time (nanos)
    pub spawn_time: u64,
    /// Complete time (nanos), None if still running
    pub complete_time: Option<u64>,
}

/// Timeline entry for display
#[derive(Debug, Clone)]
pub struct TimelineEntry {
    /// Timestamp (nanos)
    pub timestamp: u64,
    /// Task ID
    pub task_id: u32,
    /// Event type name
    pub event_type: String,
    /// Human-readable description
    pub description: String,
    /// CSS class for styling
    pub css_class: String,
}

/// Timeline builder from events
pub struct TimelineBuilder {
    tasks: HashMap<u32, TaskInfo>,
    entries: Vec<TimelineEntry>,
    failures: Vec<TimelineEntry>,
}

impl TimelineBuilder {
    /// Create a new timeline builder
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
            entries: Vec::new(),
            failures: Vec::new(),
        }
    }

    /// Add an event to the timeline
    pub fn add_event(&mut self, event: &Event) {
        let entry = self.event_to_entry(event);
        
        // Track failures separately
        if entry.css_class == "failure" {
            self.failures.push(entry.clone());
        }
        
        self.entries.push(entry);
        
        // Track task lifecycle
        match &event.payload {
            EventPayload::TaskSpawn { parent, name } => {
                self.tasks.insert(event.task_id, TaskInfo {
                    id: event.task_id,
                    name: name.clone(),
                    parent: *parent,
                    spawn_time: event.timestamp,
                    complete_time: None,
                });
            }
            EventPayload::TaskComplete => {
                if let Some(task) = self.tasks.get_mut(&event.task_id) {
                    task.complete_time = Some(event.timestamp);
                }
            }
            _ => {}
        }
    }

    fn event_to_entry(&self, event: &Event) -> TimelineEntry {
        let task_name = self.tasks.get(&event.task_id)
            .map(|t| t.name.as_str())
            .unwrap_or("unknown");
        
        let (event_type, description, css_class) = match &event.payload {
            EventPayload::TaskSpawn { parent, name } => (
                "spawn".to_string(),
                format!("Task '{}' spawned by task {}", name, parent),
                "spawn".to_string(),
            ),
            EventPayload::TaskYield => (
                "yield".to_string(),
                format!("Task '{}' yielded", task_name),
                "yield".to_string(),
            ),
            EventPayload::TaskComplete => (
                "complete".to_string(),
                format!("Task '{}' completed", task_name),
                "complete".to_string(),
            ),
            EventPayload::TimeQuery { result } => (
                "time".to_string(),
                format!("Task '{}' queried time: {}ns", task_name, result),
                "time".to_string(),
            ),
            EventPayload::RandomGen { result } => (
                "random".to_string(),
                format!("Task '{}' generated random: {}", task_name, result),
                "random".to_string(),
            ),
            EventPayload::NetSend { dst, data } => (
                "send".to_string(),
                format!("Task '{}' sent {} bytes to task {}", task_name, data.len(), dst),
                "network".to_string(),
            ),
            EventPayload::NetRecv { src, data } => (
                "recv".to_string(),
                format!("Task '{}' received {} bytes from task {}", task_name, data.len(), src),
                "network".to_string(),
            ),
            EventPayload::ScheduleDecision { chosen, ready } => (
                "schedule".to_string(),
                format!("Scheduler chose task {} from {} ready", chosen, ready.len()),
                "schedule".to_string(),
            ),
            EventPayload::FaultInjected { fault_type, target } => (
                "fault".to_string(),
                format!("Fault type {} injected on task {}", fault_type, target),
                "failure".to_string(),
            ),
        };
        
        TimelineEntry {
            timestamp: event.timestamp,
            task_id: event.task_id,
            event_type,
            description,
            css_class,
        }
    }

    /// Build the timeline
    pub fn build(self) -> Timeline {
        Timeline {
            tasks: self.tasks,
            entries: self.entries,
            failures: self.failures,
        }
    }
}

impl Default for TimelineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Completed timeline
pub struct Timeline {
    /// Task information
    pub tasks: HashMap<u32, TaskInfo>,
    /// All timeline entries
    pub entries: Vec<TimelineEntry>,
    /// Failure entries only
    pub failures: Vec<TimelineEntry>,
}

impl Timeline {
    /// Get duration of the timeline in nanoseconds
    pub fn duration_ns(&self) -> u64 {
        self.entries.iter()
            .map(|e| e.timestamp)
            .max()
            .unwrap_or(0)
    }

    /// Get number of events
    pub fn event_count(&self) -> usize {
        self.entries.len()
    }

    /// Get number of tasks
    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    /// Get failure count
    pub fn failure_count(&self) -> usize {
        self.failures.len()
    }

    /// Check if timeline has failures
    pub fn has_failures(&self) -> bool {
        !self.failures.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::Event;

    #[test]
    fn empty_timeline() {
        let builder = TimelineBuilder::new();
        let timeline = builder.build();
        
        assert_eq!(timeline.event_count(), 0);
        assert_eq!(timeline.task_count(), 0);
        assert_eq!(timeline.duration_ns(), 0);
    }

    #[test]
    fn single_task_lifecycle() {
        let mut builder = TimelineBuilder::new();
        
        builder.add_event(&Event::task_spawn(1, 0, "main".to_string(), 0));
        builder.add_event(&Event::task_yield(1, 100));
        builder.add_event(&Event::task_complete(1, 200));
        
        let timeline = builder.build();
        
        assert_eq!(timeline.event_count(), 3);
        assert_eq!(timeline.task_count(), 1);
        assert_eq!(timeline.duration_ns(), 200);
        
        let task = timeline.tasks.get(&1).unwrap();
        assert_eq!(task.name, "main");
        assert_eq!(task.spawn_time, 0);
        assert_eq!(task.complete_time, Some(200));
    }

    #[test]
    fn failure_tracking() {
        let mut builder = TimelineBuilder::new();
        
        builder.add_event(&Event::task_spawn(1, 0, "main".to_string(), 0));
        builder.add_event(&Event::fault_injected(0, 100, 1, 1));
        
        let timeline = builder.build();
        
        assert!(timeline.has_failures());
        assert_eq!(timeline.failure_count(), 1);
    }

    #[test]
    fn network_events() {
        let mut builder = TimelineBuilder::new();
        
        builder.add_event(&Event::task_spawn(1, 0, "sender".to_string(), 0));
        builder.add_event(&Event::task_spawn(2, 0, "receiver".to_string(), 0));
        builder.add_event(&Event::net_send(1, 100, 2, vec![1, 2, 3]));
        builder.add_event(&Event::net_recv(2, 150, 1, vec![1, 2, 3]));
        
        let timeline = builder.build();
        
        assert_eq!(timeline.event_count(), 4);
        assert_eq!(timeline.task_count(), 2);
    }

    #[test]
    fn entry_descriptions() {
        let mut builder = TimelineBuilder::new();
        builder.add_event(&Event::task_spawn(1, 0, "test".to_string(), 0));
        
        let timeline = builder.build();
        let entry = &timeline.entries[0];
        
        assert_eq!(entry.event_type, "spawn");
        assert!(entry.description.contains("test"));
        assert_eq!(entry.css_class, "spawn");
    }
}
