//! Tests for recording and replay functionality.

use chronos::recording::{Event, EventPayload, Header, RecordingReader, RecordingWriter};
use chronos::runtime::Runtime;
use chronos::cli::ReplayExecutor;
use tempfile::tempdir;

#[test]
fn test_runtime_with_recording() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.chrn");
    
    // Run with recording
    {
        let mut rt = Runtime::with_recording(42, path.to_str().unwrap());
        rt.spawn_named(async {}, "task1");
        rt.spawn_named(async {}, "task2");
        rt.run_until_stable().unwrap();
    }
    
    // Recording should exist and be valid
    assert!(path.exists());
    
    let reader = RecordingReader::open(&path).unwrap();
    assert_eq!(reader.seed(), 42);
    
    let events: Vec<_> = reader.events().collect::<chronos::Result<Vec<_>>>().unwrap();
    assert!(!events.is_empty());
    
    // Should have spawn and complete events
    let spawn_count = events.iter().filter(|e| matches!(e.payload, EventPayload::TaskSpawn { .. })).count();
    let complete_count = events.iter().filter(|e| matches!(e.payload, EventPayload::TaskComplete)).count();
    
    assert_eq!(spawn_count, 2);
    assert_eq!(complete_count, 2);
}

#[test]
fn test_recording_captures_schedule_decisions() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.chrn");
    
    {
        let mut rt = Runtime::with_recording(42, path.to_str().unwrap());
        // Spawn multiple tasks to force scheduling decisions
        for i in 0..5 {
            rt.spawn_named(async {}, &format!("task{}", i));
        }
        rt.run_until_stable().unwrap();
    }
    
    let reader = RecordingReader::open(&path).unwrap();
    let events: Vec<_> = reader.events().collect::<chronos::Result<Vec<_>>>().unwrap();
    
    // Should have schedule decisions
    let schedule_count = events.iter()
        .filter(|e| matches!(e.payload, EventPayload::ScheduleDecision { .. }))
        .count();
    
    assert!(schedule_count > 0);
}

#[test]
fn test_replay_executor_load() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.chrn");
    
    // Create a recording
    {
        let header = Header::new(12345, 1);
        let mut writer = RecordingWriter::new(&path, header).unwrap();
        
        writer.write_event(&Event::task_spawn(1, 0, "main".to_string(), 0)).unwrap();
        writer.write_event(&Event::random_gen(1, 100, 42)).unwrap();
        writer.write_event(&Event::schedule_decision(0, 200, 1, vec![1, 2])).unwrap();
        writer.write_event(&Event::task_complete(1, 300)).unwrap();
        
        writer.finish().unwrap();
    }
    
    // Load with replay executor
    let mut executor = ReplayExecutor::new(&path).unwrap();
    assert_eq!(executor.seed(), 12345);
    
    executor.load_events().unwrap();
    assert_eq!(executor.event_count(), 4);
    
    // State should have recorded values
    let state = executor.state();
    assert_eq!(state.random_values.len(), 1);
    assert_eq!(state.random_values[0], 42);
    assert_eq!(state.schedule_decisions.len(), 1);
}

#[test]
fn test_replay_executor_step() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.chrn");
    
    // Create a recording
    {
        let header = Header::new(42, 1);
        let mut writer = RecordingWriter::new(&path, header).unwrap();
        
        writer.write_event(&Event::task_spawn(1, 0, "t1".to_string(), 0)).unwrap();
        writer.write_event(&Event::task_spawn(2, 0, "t2".to_string(), 50)).unwrap();
        writer.write_event(&Event::task_complete(1, 100)).unwrap();
        writer.write_event(&Event::task_complete(2, 150)).unwrap();
        
        writer.finish().unwrap();
    }
    
    let mut executor = ReplayExecutor::new(&path).unwrap();
    executor.load_events().unwrap();
    
    // Step through events
    let e1 = executor.step().unwrap();
    assert_eq!(e1.task_id, 1);
    
    let e2 = executor.step().unwrap();
    assert_eq!(e2.task_id, 2);
    
    let e3 = executor.step().unwrap();
    assert_eq!(e3.task_id, 1);
    
    let e4 = executor.step().unwrap();
    assert_eq!(e4.task_id, 2);
    
    // No more events
    assert!(executor.step().is_none());
}

#[test]
fn test_replay_executor_reset() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.chrn");
    
    {
        let header = Header::new(42, 1);
        let mut writer = RecordingWriter::new(&path, header).unwrap();
        writer.write_event(&Event::task_spawn(1, 0, "t".to_string(), 0)).unwrap();
        writer.write_event(&Event::task_complete(1, 100)).unwrap();
        writer.finish().unwrap();
    }
    
    let mut executor = ReplayExecutor::new(&path).unwrap();
    executor.load_events().unwrap();
    
    executor.step();
    executor.step();
    assert_eq!(executor.state().event_index, 2);
    
    executor.reset();
    assert_eq!(executor.state().event_index, 0);
    
    // Can step through again
    assert!(executor.step().is_some());
}

#[test]
fn test_replay_state_next_random() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.chrn");
    
    {
        let header = Header::new(42, 1);
        let mut writer = RecordingWriter::new(&path, header).unwrap();
        writer.write_event(&Event::random_gen(1, 0, 100)).unwrap();
        writer.write_event(&Event::random_gen(1, 50, 200)).unwrap();
        writer.write_event(&Event::random_gen(1, 100, 300)).unwrap();
        writer.finish().unwrap();
    }
    
    let mut executor = ReplayExecutor::new(&path).unwrap();
    executor.load_events().unwrap();
    
    let state = executor.state_mut();
    
    assert_eq!(state.next_random(), Some(100));
    assert_eq!(state.next_random(), Some(200));
    assert_eq!(state.next_random(), Some(300));
    assert_eq!(state.next_random(), None);
}

#[test]
fn test_runtime_replay_mode() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.chrn");
    
    // First run - record
    {
        let mut rt = Runtime::with_recording(42, path.to_str().unwrap());
        rt.spawn_named(async {}, "task");
        rt.run_until_stable().unwrap();
    }
    
    // Second run - replay
    {
        let rt = Runtime::with_replay(path.to_str().unwrap(), true).unwrap();
        assert!(rt.is_replay());
        assert_eq!(rt.seed(), 42);
    }
}

#[test]
fn test_recording_compressed_replay() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.chrn.gz");
    
    // Create compressed recording
    {
        let header = Header::new(99, 2);
        let mut writer = RecordingWriter::compressed(&path, header).unwrap();
        
        for i in 0..100 {
            writer.write_event(&Event::task_yield(i % 5, i as u64 * 10)).unwrap();
        }
        
        writer.finish().unwrap();
    }
    
    // Read and verify
    let reader = RecordingReader::open(&path).unwrap();
    assert!(reader.is_compressed());
    assert_eq!(reader.seed(), 99);
    
    let events: Vec<_> = reader.events().collect::<chronos::Result<Vec<_>>>().unwrap();
    assert_eq!(events.len(), 100);
}

#[test]
fn test_deterministic_replay_seeds() {
    let dir = tempdir().unwrap();
    let path1 = dir.path().join("run1.chrn");
    let path2 = dir.path().join("run2.chrn");
    
    // Run twice with same seed
    for path in [&path1, &path2] {
        let mut rt = Runtime::with_recording(12345, path.to_str().unwrap());
        for i in 0..3 {
            rt.spawn_named(async {}, &format!("t{}", i));
        }
        rt.run_until_stable().unwrap();
    }
    
    // Recordings should have same events
    let reader1 = RecordingReader::open(&path1).unwrap();
    let reader2 = RecordingReader::open(&path2).unwrap();
    
    let events1: Vec<_> = reader1.events().collect::<chronos::Result<Vec<_>>>().unwrap();
    let events2: Vec<_> = reader2.events().collect::<chronos::Result<Vec<_>>>().unwrap();
    
    assert_eq!(events1.len(), events2.len());
    
    // Event types and task IDs should match
    for (e1, e2) in events1.iter().zip(events2.iter()) {
        assert_eq!(e1.event_type, e2.event_type);
        assert_eq!(e1.task_id, e2.task_id);
    }
}

#[test]
fn test_recording_captures_all_event_types() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.chrn");
    
    {
        let header = Header::new(42, 1);
        let mut writer = RecordingWriter::new(&path, header).unwrap();
        
        // Write all event types
        writer.write_event(&Event::task_spawn(1, 0, "main".to_string(), 0)).unwrap();
        writer.write_event(&Event::task_yield(1, 100)).unwrap();
        writer.write_event(&Event::time_query(1, 150, 12345)).unwrap();
        writer.write_event(&Event::random_gen(1, 200, 99999)).unwrap();
        writer.write_event(&Event::net_send(1, 250, 2, vec![1, 2, 3])).unwrap();
        writer.write_event(&Event::net_recv(1, 300, 2, vec![4, 5, 6])).unwrap();
        writer.write_event(&Event::schedule_decision(0, 350, 1, vec![1, 2, 3])).unwrap();
        writer.write_event(&Event::fault_injected(0, 400, 1, 5)).unwrap();
        writer.write_event(&Event::task_complete(1, 500)).unwrap();
        
        writer.finish().unwrap();
    }
    
    let reader = RecordingReader::open(&path).unwrap();
    let events: Vec<_> = reader.events().collect::<chronos::Result<Vec<_>>>().unwrap();
    
    assert_eq!(events.len(), 9);
    
    // Verify all types are captured correctly
    use chronos::recording::EventType;
    assert_eq!(events[0].event_type, EventType::TaskSpawn);
    assert_eq!(events[1].event_type, EventType::TaskYield);
    assert_eq!(events[2].event_type, EventType::TimeQuery);
    assert_eq!(events[3].event_type, EventType::RandomGen);
    assert_eq!(events[4].event_type, EventType::NetSend);
    assert_eq!(events[5].event_type, EventType::NetRecv);
    assert_eq!(events[6].event_type, EventType::ScheduleDecision);
    assert_eq!(events[7].event_type, EventType::FaultInjected);
    assert_eq!(events[8].event_type, EventType::TaskComplete);
}
