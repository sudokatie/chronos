//! Visualization module for test execution
//!
//! Provides tools for visualizing test runs:
//! - Timeline view of events
//! - Message sequence diagrams
//! - HTML report generation

mod report;
mod sequence;
mod timeline;

pub use report::{Report, ReportConfig};
pub use sequence::SequenceDiagram;
pub use timeline::{Timeline, TimelineBuilder, TimelineEntry, TaskInfo};
