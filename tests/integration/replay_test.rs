//! Replay integration tests.

use chronos::config::Config;
use chronos::recording::{Event, Header, RecordingReader, RecordingWriter};

/// Test recording roundtrip.
#[test]
fn test_recording_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.chrn");
    
    // Write recording
    let header = Header::new(42, 1);
    let mut writer = RecordingWriter::new(&path, header.clone()).unwrap();
    
    let events = vec![
        Event::task_spawn(1, 0, "main".to_string(), 0),
        Event::task_yield(1, 100),
        Event::net_send(1, 200, 2, vec![1, 2, 3]),
        Event::task_complete(1, 300),
    ];
    
    for event in &events {
        writer.write_event(event).unwrap();
    }
    writer.finish().unwrap();
    
    // Read recording
    let reader = RecordingReader::open(&path).unwrap();
    assert_eq!(reader.seed(), 42);
    
    let read_events: Vec<Event> = reader
        .events()
        .collect::<chronos::Result<Vec<_>>>()
        .unwrap();
    
    assert_eq!(read_events, events);
}

/// Test compressed recording roundtrip.
#[test]
fn test_compressed_recording_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.chrn.gz");
    
    // Write compressed recording
    let header = Header::new(12345, 2);
    let mut writer = RecordingWriter::compressed(&path, header.clone()).unwrap();
    
    let events: Vec<Event> = (0..100)
        .map(|i| Event::task_yield(i, i as u64 * 100))
        .collect();
    
    for event in &events {
        writer.write_event(event).unwrap();
    }
    writer.finish().unwrap();
    
    // Read compressed recording
    let reader = RecordingReader::open(&path).unwrap();
    assert!(reader.is_compressed());
    assert_eq!(reader.seed(), 12345);
    
    let read_events: Vec<Event> = reader
        .events()
        .collect::<chronos::Result<Vec<_>>>()
        .unwrap();
    
    assert_eq!(read_events, events);
}

/// Test config save and load.
#[test]
fn test_config_persistence() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chronos.toml");
    
    let mut config = Config::default();
    config.scheduler.seed = 99999;
    config.network.drop_rate = 0.05;
    config.network.reorder_rate = 0.02;
    config.recording.enabled = true;
    config.recording.compress = true;
    
    config.save(&path).unwrap();
    
    let loaded = Config::load(&path).unwrap();
    assert_eq!(loaded.scheduler.seed, 99999);
    assert_eq!(loaded.network.drop_rate, 0.05);
    assert_eq!(loaded.network.reorder_rate, 0.02);
    assert!(loaded.recording.enabled);
    assert!(loaded.recording.compress);
}

/// Test all event types roundtrip.
#[test]
fn test_all_event_types_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.chrn");
    
    let header = Header::new(42, 1);
    let mut writer = RecordingWriter::new(&path, header).unwrap();
    
    let events = vec![
        Event::task_spawn(1, 0, "main".to_string(), 0),
        Event::task_yield(1, 100),
        Event::task_complete(1, 200),
        Event::time_query(1, 300, 12345),
        Event::random_gen(1, 400, 99999),
        Event::net_send(1, 500, 2, vec![1, 2, 3, 4, 5]),
        Event::net_recv(1, 600, 2, vec![6, 7, 8]),
        Event::schedule_decision(0, 700, 1, vec![1, 2, 3]),
        Event::fault_injected(0, 800, 1, 5),
    ];
    
    for event in &events {
        writer.write_event(event).unwrap();
    }
    writer.finish().unwrap();
    
    let reader = RecordingReader::open(&path).unwrap();
    let read_events: Vec<Event> = reader
        .events()
        .collect::<chronos::Result<Vec<_>>>()
        .unwrap();
    
    assert_eq!(read_events, events);
}

/// Test config with faults.
#[test]
fn test_config_with_faults() {
    let toml = r#"
        [scheduler]
        strategy = "pct"
        seed = 12345
        pct_depth = 5

        [network]
        drop_rate = 0.01
        reorder_rate = 0.05

        [network.latency]
        type = "uniform"
        min_ms = 10
        max_ms = 50

        [faults]
        enabled = true

        [[faults.schedule]]
        at_secs = 5.0
        fault = "partition"
        nodes = [[0, 1], [2, 3]]

        [[faults.schedule]]
        at_secs = 10.0
        fault = "heal"

        [recording]
        enabled = true
        compress = true
    "#;
    
    let config = Config::parse_str(toml).unwrap();
    
    assert_eq!(config.scheduler.strategy, "pct");
    assert!(config.faults.enabled);
    assert_eq!(config.faults.schedule.len(), 2);
    assert!(config.recording.compress);
}
