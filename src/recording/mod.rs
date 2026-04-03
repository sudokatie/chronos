mod format;
mod writer;

pub use format::{Event, EventPayload, EventType, Header, MAGIC, VERSION};
pub use writer::RecordingWriter;
