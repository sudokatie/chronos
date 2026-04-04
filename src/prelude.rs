//! Convenient re-exports for Chronos users.
//!
//! ```rust,ignore
//! use chronos::prelude::*;
//! ```

// Core types
pub use crate::error::Error;
pub use crate::{EventId, MessageId, NodeId, Result, TaskId};

// Time
pub use crate::time::{Clock, Instant, TimerId, TimerWheel};
pub use std::time::Duration;

// Cluster and nodes
pub use crate::cluster::{Cluster, Node, NodeState};
pub use crate::cluster::node::{Message, Query, MessageHandler, ByteMessage, EchoHandler};

// Network
pub use crate::network::{
    Fault, FaultSchedule, FaultState, LatencyModel, Link, Message as NetMessage, 
    NetworkConfig, NetworkSim,
};

// Scheduler
pub use crate::scheduler::{Scheduler, Strategy};

// Runtime
pub use crate::runtime::{Runtime, RuntimeConfig, StepResult, TaskHandle};

// Detection
pub use crate::detection::{DeadlockDetector, LivelockDetector, ProgressTracker, WaitGraph,
    RaceDetector, DataRace, MemoryAccess, AccessType};

// Recording
pub use crate::recording::{Event, EventPayload, EventType, Header, RecordingReader, RecordingWriter};

// Config
pub use crate::config::Config;

// Simulation APIs
pub use crate::sim::{self, SimContext};
pub use crate::sim::time::{now, sleep, sleep_until, yield_now, advance};
pub use crate::sim::random::{gen, gen_range, gen_u64, fill_bytes, shuffle, choose, chance, seed};

// Network and filesystem - also available at crate root
pub use crate::net::{Endpoint, Connection, partition, heal, can_communicate};
pub use crate::fs::{read, write, exists, remove, list, reset as reset_fs, 
    set_read_failure_rate, set_write_failure_rate};

// Top-level convenience functions
pub use crate::{spawn, is_simulation};

// Assertions
pub use crate::assertions::{assert, assert_always, assert_eventually};
pub use crate::assertions::assert_eventually_within;
pub use crate::assertions::verify_all as verify_assertions;
