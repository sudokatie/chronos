use serde::{Deserialize, Serialize};

/// Magic bytes for recording files
pub const MAGIC: [u8; 4] = *b"CHRN";

/// Current format version
pub const VERSION: u32 = 1;

/// Recording file header
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Header {
    /// Magic bytes (should be MAGIC)
    pub magic: [u8; 4],
    /// Format version
    pub version: u32,
    /// Random seed used for this run
    pub seed: u64,
    /// Scheduling strategy (0=FIFO, 1=Random, 2=PCT)
    pub strategy: u8,
    /// Real wall clock start time (unix timestamp nanos)
    pub timestamp: u64,
}

impl Header {
    /// Create a new header with the given parameters
    pub fn new(seed: u64, strategy: u8) -> Self {
        Self {
            magic: MAGIC,
            version: VERSION,
            seed,
            strategy,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0),
        }
    }

    /// Validate the header
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.magic != MAGIC {
            return Err("invalid magic bytes");
        }
        if self.version > VERSION {
            return Err("unsupported version");
        }
        Ok(())
    }
}

/// Event types for recording
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum EventType {
    TaskSpawn = 0x01,
    TaskYield = 0x02,
    TaskComplete = 0x03,
    TimeQuery = 0x04,
    RandomGen = 0x05,
    NetSend = 0x06,
    NetRecv = 0x07,
    ScheduleDecision = 0x08,
    FaultInjected = 0x09,
}

/// A recorded event
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Event {
    /// Type of event
    pub event_type: EventType,
    /// Task that generated this event
    pub task_id: u32,
    /// Simulated time when event occurred (nanos)
    pub timestamp: u64,
    /// Event-specific payload
    pub payload: EventPayload,
}

/// Event-specific data
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EventPayload {
    TaskSpawn {
        parent: u32,
        name: String,
    },
    TaskYield,
    TaskComplete,
    TimeQuery {
        result: u64,
    },
    RandomGen {
        result: u64,
    },
    NetSend {
        dst: u32,
        data: Vec<u8>,
    },
    NetRecv {
        src: u32,
        data: Vec<u8>,
    },
    ScheduleDecision {
        chosen: u32,
        ready: Vec<u32>,
    },
    FaultInjected {
        fault_type: u8,
        target: u32,
    },
}

impl Event {
    /// Create a task spawn event
    pub fn task_spawn(task_id: u32, parent: u32, name: String, timestamp: u64) -> Self {
        Self {
            event_type: EventType::TaskSpawn,
            task_id,
            timestamp,
            payload: EventPayload::TaskSpawn { parent, name },
        }
    }

    /// Create a task yield event
    pub fn task_yield(task_id: u32, timestamp: u64) -> Self {
        Self {
            event_type: EventType::TaskYield,
            task_id,
            timestamp,
            payload: EventPayload::TaskYield,
        }
    }

    /// Create a task complete event
    pub fn task_complete(task_id: u32, timestamp: u64) -> Self {
        Self {
            event_type: EventType::TaskComplete,
            task_id,
            timestamp,
            payload: EventPayload::TaskComplete,
        }
    }

    /// Create a time query event
    pub fn time_query(task_id: u32, timestamp: u64, result: u64) -> Self {
        Self {
            event_type: EventType::TimeQuery,
            task_id,
            timestamp,
            payload: EventPayload::TimeQuery { result },
        }
    }

    /// Create a random generation event
    pub fn random_gen(task_id: u32, timestamp: u64, result: u64) -> Self {
        Self {
            event_type: EventType::RandomGen,
            task_id,
            timestamp,
            payload: EventPayload::RandomGen { result },
        }
    }

    /// Create a network send event
    pub fn net_send(task_id: u32, timestamp: u64, dst: u32, data: Vec<u8>) -> Self {
        Self {
            event_type: EventType::NetSend,
            task_id,
            timestamp,
            payload: EventPayload::NetSend { dst, data },
        }
    }

    /// Create a network receive event
    pub fn net_recv(task_id: u32, timestamp: u64, src: u32, data: Vec<u8>) -> Self {
        Self {
            event_type: EventType::NetRecv,
            task_id,
            timestamp,
            payload: EventPayload::NetRecv { src, data },
        }
    }

    /// Create a schedule decision event
    pub fn schedule_decision(task_id: u32, timestamp: u64, chosen: u32, ready: Vec<u32>) -> Self {
        Self {
            event_type: EventType::ScheduleDecision,
            task_id,
            timestamp,
            payload: EventPayload::ScheduleDecision { chosen, ready },
        }
    }

    /// Create a fault injection event
    pub fn fault_injected(task_id: u32, timestamp: u64, fault_type: u8, target: u32) -> Self {
        Self {
            event_type: EventType::FaultInjected,
            task_id,
            timestamp,
            payload: EventPayload::FaultInjected { fault_type, target },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_new() {
        let header = Header::new(42, 1);
        assert_eq!(header.magic, MAGIC);
        assert_eq!(header.version, VERSION);
        assert_eq!(header.seed, 42);
        assert_eq!(header.strategy, 1);
        assert!(header.timestamp > 0);
    }

    #[test]
    fn test_header_validate() {
        let header = Header::new(42, 1);
        assert!(header.validate().is_ok());
    }

    #[test]
    fn test_header_validate_bad_magic() {
        let mut header = Header::new(42, 1);
        header.magic = *b"XXXX";
        assert_eq!(header.validate(), Err("invalid magic bytes"));
    }

    #[test]
    fn test_header_validate_bad_version() {
        let mut header = Header::new(42, 1);
        header.version = VERSION + 1;
        assert_eq!(header.validate(), Err("unsupported version"));
    }

    #[test]
    fn test_header_serialize_roundtrip() {
        let header = Header::new(12345, 2);
        let bytes = bincode::serialize(&header).unwrap();
        let decoded: Header = bincode::deserialize(&bytes).unwrap();
        assert_eq!(header, decoded);
    }

    #[test]
    fn test_event_task_spawn() {
        let event = Event::task_spawn(1, 0, "main".to_string(), 1000);
        assert_eq!(event.event_type, EventType::TaskSpawn);
        assert_eq!(event.task_id, 1);
        assert_eq!(event.timestamp, 1000);
        match event.payload {
            EventPayload::TaskSpawn { parent, name } => {
                assert_eq!(parent, 0);
                assert_eq!(name, "main");
            }
            _ => panic!("wrong payload type"),
        }
    }

    #[test]
    fn test_event_serialize_roundtrip() {
        let event = Event::task_spawn(1, 0, "test".to_string(), 5000);
        let bytes = bincode::serialize(&event).unwrap();
        let decoded: Event = bincode::deserialize(&bytes).unwrap();
        assert_eq!(event, decoded);
    }

    #[test]
    fn test_event_net_send_roundtrip() {
        let event = Event::net_send(1, 1000, 2, vec![1, 2, 3, 4]);
        let bytes = bincode::serialize(&event).unwrap();
        let decoded: Event = bincode::deserialize(&bytes).unwrap();
        assert_eq!(event, decoded);
    }

    #[test]
    fn test_event_schedule_decision_roundtrip() {
        let event = Event::schedule_decision(0, 500, 3, vec![1, 2, 3, 4]);
        let bytes = bincode::serialize(&event).unwrap();
        let decoded: Event = bincode::deserialize(&bytes).unwrap();
        assert_eq!(event, decoded);
    }

    #[test]
    fn test_all_event_types_serialize() {
        let events = vec![
            Event::task_spawn(1, 0, "t".to_string(), 0),
            Event::task_yield(1, 100),
            Event::task_complete(1, 200),
            Event::time_query(1, 300, 12345),
            Event::random_gen(1, 400, 99999),
            Event::net_send(1, 500, 2, vec![1, 2, 3]),
            Event::net_recv(1, 600, 2, vec![4, 5, 6]),
            Event::schedule_decision(0, 700, 1, vec![1, 2]),
            Event::fault_injected(0, 800, 1, 3),
        ];

        for event in events {
            let bytes = bincode::serialize(&event).unwrap();
            let decoded: Event = bincode::deserialize(&bytes).unwrap();
            assert_eq!(event, decoded);
        }
    }
}
