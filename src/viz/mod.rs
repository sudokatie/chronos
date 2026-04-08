//! Visualization module for test execution
//!
//! Provides tools for visualizing test runs:
//! - Timeline view of events
//! - Message sequence diagrams
//! - HTML report generation
//! - Schedule replay controls

mod replay;
mod report;
mod sequence;
mod timeline;

pub use replay::{ReplayController, ReplaySpeed, ReplayState, generate_replay_html};
pub use report::{Report, ReportConfig};
pub use sequence::SequenceDiagram;
pub use timeline::{Timeline, TimelineBuilder, TimelineEntry, TaskInfo};
