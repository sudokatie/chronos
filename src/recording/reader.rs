use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use flate2::read::GzDecoder;

use crate::recording::{Event, Header};
use crate::Result;

/// Reader type enum to handle both compressed and uncompressed input.
enum ReaderInner {
    Plain(BufReader<File>),
    Compressed(BufReader<GzDecoder<File>>),
}

impl Read for ReaderInner {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            ReaderInner::Plain(r) => r.read(buf),
            ReaderInner::Compressed(r) => r.read(buf),
        }
    }
}

/// Reader for recording files.
pub struct RecordingReader {
    reader: ReaderInner,
    header: Header,
    compressed: bool,
}

impl RecordingReader {
    /// Open a recording file for reading.
    /// Automatically detects if the file is compressed based on extension or content.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        
        // Check if compressed based on extension or magic bytes
        let is_compressed = path.extension()
            .map(|ext| ext == "gz")
            .unwrap_or(false) || Self::is_gzip_file(path)?;

        if is_compressed {
            Self::open_compressed(path)
        } else {
            Self::open_plain(path)
        }
    }

    /// Check if a file starts with gzip magic bytes.
    fn is_gzip_file(path: &Path) -> Result<bool> {
        let mut file = File::open(path)?;
        let mut magic = [0u8; 2];
        if file.read_exact(&mut magic).is_ok() {
            // Gzip magic: 0x1f 0x8b
            Ok(magic == [0x1f, 0x8b])
        } else {
            Ok(false)
        }
    }

    /// Open an uncompressed recording file.
    fn open_plain<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(path)?;
        let mut reader = ReaderInner::Plain(BufReader::new(file));

        let header = Self::read_header(&mut reader)?;
        header.validate().map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e)
        })?;

        Ok(Self { 
            reader, 
            header,
            compressed: false,
        })
    }

    /// Open a compressed recording file.
    fn open_compressed<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(path)?;
        let decoder = GzDecoder::new(file);
        let mut reader = ReaderInner::Compressed(BufReader::new(decoder));

        let header = Self::read_header(&mut reader)?;
        header.validate().map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e)
        })?;

        Ok(Self { 
            reader, 
            header,
            compressed: true,
        })
    }

    fn read_header(reader: &mut ReaderInner) -> Result<Header> {
        let header: Header = bincode::deserialize_from(reader)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok(header)
    }

    /// Get the recording header.
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// Get the seed used in this recording.
    pub fn seed(&self) -> u64 {
        self.header.seed
    }

    /// Get the strategy used in this recording.
    pub fn strategy(&self) -> u8 {
        self.header.strategy
    }

    /// Check if the recording is compressed.
    pub fn is_compressed(&self) -> bool {
        self.compressed
    }

    /// Read the next event from the recording.
    pub fn next_event(&mut self) -> Result<Option<Event>> {
        // Read length prefix
        let mut len_buf = [0u8; 4];
        match self.reader.read_exact(&mut len_buf) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e.into()),
        }

        let len = u32::from_le_bytes(len_buf) as usize;

        // Read event data
        let mut event_buf = vec![0u8; len];
        self.reader.read_exact(&mut event_buf)?;

        let event: Event = bincode::deserialize(&event_buf)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        Ok(Some(event))
    }

    /// Create an iterator over all events.
    pub fn events(self) -> EventIterator {
        EventIterator { reader: self }
    }
}

/// Iterator over events in a recording.
pub struct EventIterator {
    reader: RecordingReader,
}

impl Iterator for EventIterator {
    type Item = Result<Event>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.reader.next_event() {
            Ok(Some(event)) => Some(Ok(event)),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::{Event, Header, RecordingWriter};
    use tempfile::tempdir;

    fn create_test_recording(path: &Path, events: &[Event]) -> Header {
        let header = Header::new(12345, 1);
        let mut writer = RecordingWriter::new(path, header.clone()).unwrap();
        for event in events {
            writer.write_event(event).unwrap();
        }
        writer.finish().unwrap();
        header
    }

    fn create_compressed_recording(path: &Path, events: &[Event]) -> Header {
        let header = Header::new(12345, 1);
        let mut writer = RecordingWriter::compressed(path, header.clone()).unwrap();
        for event in events {
            writer.write_event(event).unwrap();
        }
        writer.finish().unwrap();
        header
    }

    #[test]
    fn test_open_valid_recording() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.chrn");

        let header = create_test_recording(&path, &[]);

        let reader = RecordingReader::open(&path);
        assert!(reader.is_ok());

        let reader = reader.unwrap();
        assert_eq!(reader.seed(), header.seed);
        assert_eq!(reader.strategy(), header.strategy);
        assert!(!reader.is_compressed());
    }

    #[test]
    fn test_open_compressed_recording() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.chrn.gz");

        let header = create_compressed_recording(&path, &[
            Event::task_spawn(1, 0, "main".to_string(), 0),
        ]);

        let reader = RecordingReader::open(&path);
        assert!(reader.is_ok());

        let reader = reader.unwrap();
        assert_eq!(reader.seed(), header.seed);
        assert!(reader.is_compressed());
    }

    #[test]
    fn test_read_single_event() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.chrn");

        let event = Event::task_spawn(1, 0, "main".to_string(), 0);
        create_test_recording(&path, &[event.clone()]);

        let mut reader = RecordingReader::open(&path).unwrap();
        let read_event = reader.next_event().unwrap();

        assert!(read_event.is_some());
        assert_eq!(read_event.unwrap(), event);

        // No more events
        assert!(reader.next_event().unwrap().is_none());
    }

    #[test]
    fn test_read_multiple_events() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.chrn");

        let events: Vec<Event> = (0..10)
            .map(|i| Event::task_yield(i, i as u64 * 100))
            .collect();
        create_test_recording(&path, &events);

        let mut reader = RecordingReader::open(&path).unwrap();
        for expected in &events {
            let read_event = reader.next_event().unwrap().unwrap();
            assert_eq!(&read_event, expected);
        }
        assert!(reader.next_event().unwrap().is_none());
    }

    #[test]
    fn test_read_compressed_events() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.chrn.gz");

        let events: Vec<Event> = (0..10)
            .map(|i| Event::task_yield(i, i as u64 * 100))
            .collect();
        create_compressed_recording(&path, &events);

        let mut reader = RecordingReader::open(&path).unwrap();
        assert!(reader.is_compressed());

        for expected in &events {
            let read_event = reader.next_event().unwrap().unwrap();
            assert_eq!(&read_event, expected);
        }
        assert!(reader.next_event().unwrap().is_none());
    }

    #[test]
    fn test_events_iterator() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.chrn");

        let events: Vec<Event> = (0..5)
            .map(|i| Event::task_complete(i, i as u64 * 100))
            .collect();
        create_test_recording(&path, &events);

        let reader = RecordingReader::open(&path).unwrap();
        let read_events: Vec<Event> = reader
            .events()
            .collect::<Result<Vec<_>>>()
            .unwrap();

        assert_eq!(read_events, events);
    }

    #[test]
    fn test_open_nonexistent_file() {
        let result = RecordingReader::open("/nonexistent/path.chrn");
        assert!(result.is_err());
    }

    #[test]
    fn test_roundtrip_all_event_types() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.chrn");

        let events = vec![
            Event::task_spawn(1, 0, "test".to_string(), 0),
            Event::task_yield(1, 100),
            Event::task_complete(1, 200),
            Event::time_query(1, 300, 999),
            Event::random_gen(1, 400, 42),
            Event::net_send(1, 500, 2, vec![1, 2, 3]),
            Event::net_recv(1, 600, 2, vec![4, 5, 6]),
            Event::schedule_decision(0, 700, 1, vec![1, 2, 3]),
            Event::fault_injected(0, 800, 1, 5),
        ];

        create_test_recording(&path, &events);

        let reader = RecordingReader::open(&path).unwrap();
        let read_events: Vec<Event> = reader
            .events()
            .collect::<Result<Vec<_>>>()
            .unwrap();

        assert_eq!(read_events, events);
    }

    #[test]
    fn test_compressed_roundtrip_all_event_types() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.chrn.gz");

        let events = vec![
            Event::task_spawn(1, 0, "test".to_string(), 0),
            Event::task_yield(1, 100),
            Event::task_complete(1, 200),
            Event::time_query(1, 300, 999),
            Event::random_gen(1, 400, 42),
            Event::net_send(1, 500, 2, vec![1, 2, 3]),
            Event::net_recv(1, 600, 2, vec![4, 5, 6]),
            Event::schedule_decision(0, 700, 1, vec![1, 2, 3]),
            Event::fault_injected(0, 800, 1, 5),
        ];

        create_compressed_recording(&path, &events);

        let reader = RecordingReader::open(&path).unwrap();
        assert!(reader.is_compressed());
        
        let read_events: Vec<Event> = reader
            .events()
            .collect::<Result<Vec<_>>>()
            .unwrap();

        assert_eq!(read_events, events);
    }
}
