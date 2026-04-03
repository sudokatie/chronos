//! Integration tests for Chronos.

use std::time::Duration;

use chronos::cluster::Cluster;
use chronos::config::Config;
use chronos::detection::{DeadlockDetector, LivelockDetector};
use chronos::recording::{Event, Header, RecordingReader, RecordingWriter};
use chronos::time::Instant;

/// Test basic cluster creation and time advancement.
#[test]
fn test_cluster_basic() {
    let mut cluster = Cluster::new(3);
    
    assert_eq!(cluster.size(), 3);
    assert_eq!(cluster.running_count(), 3);
    
    cluster.advance_time(Duration::from_secs(1));
    assert_eq!(cluster.now().as_nanos(), 1_000_000_000);
}

/// Test cluster partitioning.
#[test]
fn test_cluster_partition() {
    let mut cluster = Cluster::new(4);
    
    // Create partition: {0, 1} and {2, 3}
    cluster.partition(&[&[0, 1], &[2, 3]]);
    
    // Crash and restart node
    cluster.crash_node(1);
    assert!(cluster.node(1).unwrap().is_crashed());
    
    cluster.restart_node(1);
    assert!(cluster.node(1).unwrap().is_running());
    
    // Heal partition
    cluster.heal_partition();
}

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

/// Test deadlock detection.
#[test]
fn test_deadlock_detection() {
    let mut detector = DeadlockDetector::new();
    
    // No deadlock initially
    detector.task_waiting(1, 2);
    detector.task_waiting(2, 3);
    assert!(detector.check().is_none());
    
    // Create deadlock
    detector.task_waiting(3, 1);
    let cycle = detector.check();
    assert!(cycle.is_some());
    
    // Break deadlock
    detector.task_completed(2);
    assert!(detector.check().is_none());
}

/// Test livelock detection.
#[test]
fn test_livelock_detection() {
    let mut detector = LivelockDetector::new(100);
    
    // Not stuck yet
    for _ in 0..50 {
        detector.task_step(1);
    }
    assert!(detector.check().is_none());
    
    // Now stuck
    for _ in 0..50 {
        detector.task_step(1);
    }
    assert!(detector.check().is_some());
    
    // Progress resets counter
    detector.task_progress(1);
    assert!(detector.check().is_none());
}

/// Test configuration loading.
#[test]
fn test_config_loading() {
    let toml = r#"
        [scheduler]
        strategy = "pct"
        seed = 12345
        pct_depth = 5

        [network]
        latency_ms = 50
        drop_rate = 0.01

        [detection]
        deadlock_detection = true
        livelock_threshold = 500
    "#;
    
    let config = Config::from_str(toml).unwrap();
    
    assert_eq!(config.scheduler.strategy, "pct");
    assert_eq!(config.scheduler.seed, 12345);
    assert_eq!(config.network.latency_ms, 50);
    assert_eq!(config.detection.livelock_threshold, 500);
}

/// Test config save and load.
#[test]
fn test_config_persistence() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chronos.toml");
    
    let mut config = Config::default();
    config.scheduler.seed = 99999;
    config.network.drop_rate = 0.05;
    
    config.save(&path).unwrap();
    
    let loaded = Config::load(&path).unwrap();
    assert_eq!(loaded.scheduler.seed, 99999);
    assert_eq!(loaded.network.drop_rate, 0.05);
}

/// Test CLI argument parsing for run command.
#[test]
fn test_cli_run_parsing() {
    use chronos::cli::parse_from;
    
    let cli = parse_from([
        "chronos", "run", "my_test",
        "--seed", "42",
        "--strategy", "pct",
        "-n", "100",
        "-v"
    ]);
    
    match cli.command {
        chronos::cli::Commands::Run(args) => {
            assert_eq!(args.test_binary, "my_test");
            assert_eq!(args.seed, Some(42));
            assert_eq!(args.iterations, 100);
            assert!(args.verbose);
        }
        _ => panic!("expected Run command"),
    }
}

/// Test CLI argument parsing for explore command.
#[test]
fn test_cli_explore_parsing() {
    use chronos::cli::parse_from;
    
    let cli = parse_from([
        "chronos", "explore", "my_test",
        "--depth", "200",
        "-j", "4",
        "--seed", "123"
    ]);
    
    match cli.command {
        chronos::cli::Commands::Explore(args) => {
            assert_eq!(args.test_binary, "my_test");
            assert_eq!(args.depth, 200);
            assert_eq!(args.threads, 4);
            assert_eq!(args.seed, Some(123));
        }
        _ => panic!("expected Explore command"),
    }
}

/// Test time instant arithmetic.
#[test]
fn test_time_arithmetic() {
    let t1 = Instant::from_nanos(1_000_000_000);
    let t2 = Instant::from_nanos(2_000_000_000);
    
    let duration = t2.duration_since(t1).unwrap();
    assert_eq!(duration.as_secs(), 1);
    
    let t3 = t1.saturating_add(Duration::from_secs(5));
    assert_eq!(t3.as_nanos(), 6_000_000_000);
}

/// Test full simulation flow.
#[test]
fn test_simulation_flow() {
    // Create cluster
    let mut cluster = Cluster::with_seed(3, 42);
    
    // Run for a while
    for _ in 0..10 {
        cluster.advance_time(Duration::from_millis(100));
    }
    
    assert_eq!(cluster.now().as_nanos(), 1_000_000_000);
    
    // Inject fault
    cluster.crash_node(1);
    assert_eq!(cluster.running_count(), 2);
    
    // Partition
    cluster.partition(&[&[0], &[2]]);
    
    // Continue simulation
    cluster.advance_time(Duration::from_secs(1));
    
    // Heal and restart
    cluster.heal_partition();
    cluster.restart_node(1);
    assert_eq!(cluster.running_count(), 3);
}

/// Test detection integration.
#[test]
fn test_detection_integration() {
    let mut deadlock = DeadlockDetector::new();
    let mut livelock = LivelockDetector::new(50);
    
    // Simulate some tasks
    deadlock.task_waiting(1, 2);
    livelock.task_step(1);
    
    // No issues yet
    assert!(deadlock.check().is_none());
    assert!(livelock.check().is_none());
    
    // Task 2 completes, releases 1
    deadlock.task_completed(2);
    livelock.task_progress(1);
    
    assert!(deadlock.check().is_none());
    assert!(livelock.check().is_none());
}
