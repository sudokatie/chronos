use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use crate::recording::{Event, Header};
use crate::Result;

/// Writer for recording execution traces to a file.
pub struct RecordingWriter {
    writer: BufWriter<File>,
    event_count: u64,
}

impl RecordingWriter {
    /// Create a new recording writer at the given path.
    pub fn new<P: AsRef<Path>>(path: P, header: Header) -> Result<Self> {
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);

        // Write header
        let header_bytes = bincode::serialize(&header)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        writer.write_all(&header_bytes)?;

        Ok(Self {
            writer,
            event_count: 0,
        })
    }

    /// Write an event to the recording.
    pub fn write_event(&mut self, event: &Event) -> Result<()> {
        let event_bytes = bincode::serialize(event)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        // Write length prefix for framing
        let len = event_bytes.len() as u32;
        self.writer.write_all(&len.to_le_bytes())?;
        self.writer.write_all(&event_bytes)?;

        self.event_count += 1;
        Ok(())
    }

    /// Get the number of events written so far.
    pub fn event_count(&self) -> u64 {
        self.event_count
    }

    /// Finish writing and flush to disk.
    pub fn finish(mut self) -> Result<u64> {
        self.writer.flush()?;
        Ok(self.event_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::{Event, Header};
    use tempfile::tempdir;

    #[test]
    fn test_create_writer() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.chrn");

        let header = Header::new(42, 1);
        let writer = RecordingWriter::new(&path, header);
        assert!(writer.is_ok());
        assert!(path.exists());
    }

    #[test]
    fn test_write_single_event() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.chrn");

        let header = Header::new(42, 1);
        let mut writer = RecordingWriter::new(&path, header).unwrap();

        let event = Event::task_spawn(1, 0, "main".to_string(), 0);
        assert!(writer.write_event(&event).is_ok());
        assert_eq!(writer.event_count(), 1);
    }

    #[test]
    fn test_write_multiple_events() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.chrn");

        let header = Header::new(42, 1);
        let mut writer = RecordingWriter::new(&path, header).unwrap();

        for i in 0..100 {
            let event = Event::task_yield(i, i as u64 * 100);
            writer.write_event(&event).unwrap();
        }

        assert_eq!(writer.event_count(), 100);
    }

    #[test]
    fn test_finish_flushes() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.chrn");

        let header = Header::new(42, 1);
        let mut writer = RecordingWriter::new(&path, header).unwrap();

        let event = Event::task_spawn(1, 0, "test".to_string(), 0);
        writer.write_event(&event).unwrap();

        let count = writer.finish().unwrap();
        assert_eq!(count, 1);

        // File should have content
        let metadata = std::fs::metadata(&path).unwrap();
        assert!(metadata.len() > 0);
    }

    #[test]
    fn test_write_large_payload() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.chrn");

        let header = Header::new(42, 1);
        let mut writer = RecordingWriter::new(&path, header).unwrap();

        // Large network payload
        let data = vec![0u8; 64 * 1024]; // 64KB
        let event = Event::net_send(1, 1000, 2, data);
        assert!(writer.write_event(&event).is_ok());

        writer.finish().unwrap();
    }
}
