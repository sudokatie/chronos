use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use flate2::write::GzEncoder;
use flate2::Compression;

use crate::recording::{Event, Header};
use crate::Result;

/// Writer type enum to handle both compressed and uncompressed output.
enum WriterInner {
    Plain(BufWriter<File>),
    Compressed(GzEncoder<BufWriter<File>>),
}

impl Write for WriterInner {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            WriterInner::Plain(w) => w.write(buf),
            WriterInner::Compressed(w) => w.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            WriterInner::Plain(w) => w.flush(),
            WriterInner::Compressed(w) => w.flush(),
        }
    }
}

/// Writer for recording execution traces to a file.
pub struct RecordingWriter {
    writer: WriterInner,
    event_count: u64,
    compressed: bool,
}

impl RecordingWriter {
    /// Create a new recording writer at the given path.
    pub fn new<P: AsRef<Path>>(path: P, header: Header) -> Result<Self> {
        Self::with_compression(path, header, false)
    }

    /// Create a new recording writer with optional compression.
    pub fn with_compression<P: AsRef<Path>>(path: P, header: Header, compress: bool) -> Result<Self> {
        let file = File::create(path)?;
        let buf_writer = BufWriter::new(file);
        
        let mut writer = if compress {
            WriterInner::Compressed(GzEncoder::new(buf_writer, Compression::default()))
        } else {
            WriterInner::Plain(buf_writer)
        };

        // Write header
        let header_bytes = bincode::serialize(&header)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        writer.write_all(&header_bytes)?;

        Ok(Self {
            writer,
            event_count: 0,
            compressed: compress,
        })
    }

    /// Create a compressed recording writer.
    pub fn compressed<P: AsRef<Path>>(path: P, header: Header) -> Result<Self> {
        Self::with_compression(path, header, true)
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

    /// Check if compression is enabled.
    pub fn is_compressed(&self) -> bool {
        self.compressed
    }

    /// Finish writing and flush to disk.
    pub fn finish(self) -> Result<u64> {
        let count = self.event_count;
        
        match self.writer {
            WriterInner::Plain(mut w) => {
                w.flush()?;
            }
            WriterInner::Compressed(w) => {
                // finish() consumes the encoder and flushes
                w.finish()?;
            }
        }
        
        Ok(count)
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

    #[test]
    fn test_compressed_writer() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.chrn.gz");

        let header = Header::new(42, 1);
        let mut writer = RecordingWriter::compressed(&path, header).unwrap();
        
        assert!(writer.is_compressed());

        // Write many events to see compression benefit
        for i in 0..1000 {
            let event = Event::task_yield(i % 10, i as u64 * 100);
            writer.write_event(&event).unwrap();
        }

        let count = writer.finish().unwrap();
        assert_eq!(count, 1000);

        // File should exist and be compressed (smaller than uncompressed would be)
        let metadata = std::fs::metadata(&path).unwrap();
        assert!(metadata.len() > 0);
    }

    #[test]
    fn test_with_compression_flag() {
        let dir = tempdir().unwrap();
        
        // Test with compression = false
        let path1 = dir.path().join("test1.chrn");
        let writer1 = RecordingWriter::with_compression(&path1, Header::new(42, 1), false).unwrap();
        assert!(!writer1.is_compressed());
        writer1.finish().unwrap();

        // Test with compression = true
        let path2 = dir.path().join("test2.chrn.gz");
        let writer2 = RecordingWriter::with_compression(&path2, Header::new(42, 1), true).unwrap();
        assert!(writer2.is_compressed());
        writer2.finish().unwrap();
    }
}
