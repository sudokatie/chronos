//! Tests for the recording and replay system.

use chronos::recording::{Event, EventPayload, EventType, Header, RecordingReader, RecordingWriter, MAGIC, VERSION};
use tempfile::tempdir;

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
fn test_event_types() {
    // Test all event constructors
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
    
    assert_eq!(events.len(), 9);
    
    // Verify each has correct type
    assert_eq!(events[0].event_type, EventType::TaskSpawn);
    assert_eq!(events[1].event_type, EventType::TaskYield);
    assert_eq!(events[2].event_type, EventType::TaskComplete);
    assert_eq!(events[3].event_type, EventType::TimeQuery);
    assert_eq!(events[4].event_type, EventType::RandomGen);
    assert_eq!(events[5].event_type, EventType::NetSend);
    assert_eq!(events[6].event_type, EventType::NetRecv);
    assert_eq!(events[7].event_type, EventType::ScheduleDecision);
    assert_eq!(events[8].event_type, EventType::FaultInjected);
}

#[test]
fn test_recording_roundtrip() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.chrn");
    
    let header = Header::new(12345, 1);
    let events = vec![
        Event::task_spawn(1, 0, "main".to_string(), 0),
        Event::task_yield(1, 100),
        Event::random_gen(1, 200, 42),
        Event::task_complete(1, 300),
    ];
    
    // Write
    {
        let mut writer = RecordingWriter::new(&path, header.clone()).unwrap();
        for event in &events {
            writer.write_event(event).unwrap();
        }
        writer.finish().unwrap();
    }
    
    // Read
    {
        let reader = RecordingReader::open(&path).unwrap();
        assert_eq!(reader.seed(), 12345);
        assert_eq!(reader.strategy(), 1);
        
        let read_events: Vec<Event> = reader
            .events()
            .collect::<chronos::Result<Vec<_>>>()
            .unwrap();
        
        assert_eq!(read_events, events);
    }
}

#[test]
fn test_recording_compressed() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.chrn.gz");
    
    let header = Header::new(42, 2);
    let events = vec![
        Event::task_spawn(1, 0, "test".to_string(), 0),
        Event::net_send(1, 100, 2, vec![1, 2, 3, 4, 5]),
        Event::task_complete(1, 200),
    ];
    
    // Write compressed
    {
        let mut writer = RecordingWriter::compressed(&path, header.clone()).unwrap();
        for event in &events {
            writer.write_event(event).unwrap();
        }
        writer.finish().unwrap();
    }
    
    // Read compressed
    {
        let reader = RecordingReader::open(&path).unwrap();
        assert!(reader.is_compressed());
        assert_eq!(reader.seed(), 42);
        
        let read_events: Vec<Event> = reader
            .events()
            .collect::<chronos::Result<Vec<_>>>()
            .unwrap();
        
        assert_eq!(read_events, events);
    }
}

#[test]
fn test_recording_many_events() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.chrn");
    
    let header = Header::new(99, 0);
    let events: Vec<Event> = (0..1000)
        .map(|i| Event::task_yield(i as u32 % 10, i as u64 * 100))
        .collect();
    
    // Write
    {
        let mut writer = RecordingWriter::new(&path, header).unwrap();
        for event in &events {
            writer.write_event(event).unwrap();
        }
        writer.finish().unwrap();
    }
    
    // Read
    {
        let reader = RecordingReader::open(&path).unwrap();
        let read_events: Vec<Event> = reader
            .events()
            .collect::<chronos::Result<Vec<_>>>()
            .unwrap();
        
        assert_eq!(read_events.len(), 1000);
        assert_eq!(read_events, events);
    }
}

#[test]
fn test_recording_empty() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.chrn");
    
    let header = Header::new(1, 0);
    
    // Write with no events
    {
        let writer = RecordingWriter::new(&path, header).unwrap();
        writer.finish().unwrap();
    }
    
    // Read
    {
        let mut reader = RecordingReader::open(&path).unwrap();
        assert!(reader.next_event().unwrap().is_none());
    }
}

#[test]
fn test_recording_header_access() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.chrn");
    
    let header = Header::new(999, 3);
    
    {
        let writer = RecordingWriter::new(&path, header.clone()).unwrap();
        writer.finish().unwrap();
    }
    
    {
        let reader = RecordingReader::open(&path).unwrap();
        let read_header = reader.header();
        
        assert_eq!(read_header.seed, 999);
        assert_eq!(read_header.strategy, 3);
        assert_eq!(read_header.magic, MAGIC);
        assert_eq!(read_header.version, VERSION);
    }
}

#[test]
fn test_recording_step_through() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.chrn");
    
    let header = Header::new(42, 1);
    let events = vec![
        Event::task_spawn(1, 0, "a".to_string(), 0),
        Event::task_spawn(2, 0, "b".to_string(), 100),
        Event::task_spawn(3, 0, "c".to_string(), 200),
    ];
    
    {
        let mut writer = RecordingWriter::new(&path, header).unwrap();
        for event in &events {
            writer.write_event(event).unwrap();
        }
        writer.finish().unwrap();
    }
    
    {
        let mut reader = RecordingReader::open(&path).unwrap();
        
        // Step through one at a time
        let e1 = reader.next_event().unwrap().unwrap();
        assert_eq!(e1.task_id, 1);
        
        let e2 = reader.next_event().unwrap().unwrap();
        assert_eq!(e2.task_id, 2);
        
        let e3 = reader.next_event().unwrap().unwrap();
        assert_eq!(e3.task_id, 3);
        
        assert!(reader.next_event().unwrap().is_none());
    }
}

#[test]
fn test_event_serialization_roundtrip() {
    let events = vec![
        Event::task_spawn(1, 0, "test".to_string(), 0),
        Event::net_send(2, 100, 3, vec![1, 2, 3, 4, 5, 6, 7, 8]),
        Event::schedule_decision(0, 200, 5, vec![1, 2, 3, 4, 5]),
    ];
    
    for event in events {
        let bytes = bincode::serialize(&event).unwrap();
        let decoded: Event = bincode::deserialize(&bytes).unwrap();
        assert_eq!(event, decoded);
    }
}
