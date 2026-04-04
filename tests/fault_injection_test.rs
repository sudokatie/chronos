//! Tests for fault injection functionality.

use std::time::Duration;

use chronos::cluster::Cluster;
use chronos::network::{NetworkSim, NetworkConfig, LatencyModel, Fault};
use chronos::time::Instant;
use chronos::cli::FaultSpec;

#[test]
fn test_network_partition_fault() {
    let mut config = NetworkConfig::default();
    config.latency = LatencyModel::fixed(Duration::from_millis(1));
    
    let mut net = NetworkSim::new(config, 42);
    net.connect(0, 1);
    net.connect(1, 2);
    net.connect(0, 2);
    
    // Initially all can communicate
    assert!(net.can_communicate(0, 1));
    assert!(net.can_communicate(1, 2));
    assert!(net.can_communicate(0, 2));
    
    // Partition: {0, 1} | {2}
    net.partition(vec![vec![0, 1], vec![2]]);
    
    assert!(net.can_communicate(0, 1));
    assert!(!net.can_communicate(0, 2));
    assert!(!net.can_communicate(1, 2));
}

#[test]
fn test_scheduled_partition() {
    let mut config = NetworkConfig::default();
    config.latency = LatencyModel::fixed(Duration::from_millis(1));
    
    let mut net = NetworkSim::new(config, 42);
    net.connect(0, 1);
    
    // Schedule partition at 10ms
    net.schedule_fault(
        Instant::from_nanos(10_000_000),
        Fault::partition(vec![vec![0], vec![1]]),
    );
    
    // Schedule heal at 20ms
    net.schedule_fault(
        Instant::from_nanos(20_000_000),
        Fault::heal(),
    );
    
    // Before 10ms - can communicate
    assert!(net.can_communicate(0, 1));
    
    // At 15ms - partitioned
    net.tick(Instant::from_nanos(15_000_000));
    assert!(!net.can_communicate(0, 1));
    
    // At 25ms - healed
    net.tick(Instant::from_nanos(25_000_000));
    assert!(net.can_communicate(0, 1));
}

#[test]
fn test_drop_fault() {
    let mut config = NetworkConfig::default();
    config.latency = LatencyModel::fixed(Duration::from_millis(1));
    config.drop_rate = 1.0; // 100% drop
    
    let mut net = NetworkSim::new(config, 42);
    net.connect(0, 1);
    
    let now = Instant::from_nanos(0);
    
    // Send multiple messages
    for _ in 0..10 {
        net.send(0, 1, vec![1], now).unwrap();
    }
    
    net.tick(Instant::from_nanos(2_000_000));
    
    // All should be dropped
    assert!(net.recv(1).is_none());
}

#[test]
fn test_node_crash_fault() {
    let mut cluster = Cluster::new(3);
    
    assert!(cluster.node(0).unwrap().is_running());
    assert!(cluster.node(1).unwrap().is_running());
    assert!(cluster.node(2).unwrap().is_running());
    
    // Crash node 1
    cluster.crash_node(1);
    
    assert!(cluster.node(0).unwrap().is_running());
    assert!(cluster.node(1).unwrap().is_crashed());
    assert!(cluster.node(2).unwrap().is_running());
    
    // Restart node 1
    cluster.restart_node(1);
    
    assert!(cluster.node(1).unwrap().is_running());
}

#[test]
fn test_clock_skew_fault() {
    let mut cluster = Cluster::new(2);
    
    // Normal time
    cluster.advance_time(Duration::from_secs(1));
    assert_eq!(cluster.node_time(0).as_nanos(), 1_000_000_000);
    assert_eq!(cluster.node_time(1).as_nanos(), 1_000_000_000);
    
    // Apply 2x skew to node 0
    cluster.set_clock_skew(0, 2.0);
    
    // Advance another second
    cluster.advance_time(Duration::from_secs(1));
    
    // Node 0 sees more time due to skew
    assert!(cluster.node_time(0).as_nanos() > cluster.node_time(1).as_nanos());
}

#[test]
fn test_clock_jump_fault() {
    let mut cluster = Cluster::new(2);
    
    cluster.advance_time(Duration::from_secs(1));
    
    // Jump node 0 forward 10 seconds
    cluster.clock_jump_forward(0, Duration::from_secs(10));
    
    let node0_time = cluster.node_time(0).as_nanos();
    let node1_time = cluster.node_time(1).as_nanos();
    
    assert_eq!(node0_time - node1_time, 10_000_000_000);
}

#[test]
fn test_clear_clock_faults() {
    let mut cluster = Cluster::new(2);
    
    cluster.set_clock_skew(0, 2.0);
    cluster.clock_jump_forward(0, Duration::from_secs(5));
    cluster.advance_time(Duration::from_secs(1));
    
    // Clear faults
    cluster.clear_clock_faults(0);
    
    assert_eq!(cluster.clock_skew(0), 1.0);
    assert_eq!(cluster.clock_offset(0), 0);
}

#[test]
fn test_fault_spec_parse_partition() {
    let spec = FaultSpec::parse("network:partition:0,1|2,3").unwrap();
    
    match spec {
        FaultSpec::Partition { groups, .. } => {
            assert_eq!(groups.len(), 2);
            assert_eq!(groups[0], vec![0, 1]);
            assert_eq!(groups[1], vec![2, 3]);
        }
        _ => panic!("expected partition"),
    }
}

#[test]
fn test_fault_spec_parse_drop() {
    let spec = FaultSpec::parse("network:drop:10%").unwrap();
    
    match spec {
        FaultSpec::Drop { rate } => {
            assert!((rate - 0.1).abs() < 0.001);
        }
        _ => panic!("expected drop"),
    }
}

#[test]
fn test_fault_spec_parse_delay() {
    let spec = FaultSpec::parse("network:delay:50ms-200ms").unwrap();
    
    match spec {
        FaultSpec::Delay { min, max } => {
            assert_eq!(min, Duration::from_millis(50));
            assert_eq!(max, Duration::from_millis(200));
        }
        _ => panic!("expected delay"),
    }
}

#[test]
fn test_fault_spec_parse_disk_read_error() {
    let spec = FaultSpec::parse("disk:error:read:5%").unwrap();
    
    match spec {
        FaultSpec::DiskReadError { rate } => {
            assert!((rate - 0.05).abs() < 0.001);
        }
        _ => panic!("expected disk read error"),
    }
}

#[test]
fn test_fault_spec_parse_disk_write_error() {
    let spec = FaultSpec::parse("disk:error:write:10%").unwrap();
    
    match spec {
        FaultSpec::DiskWriteError { rate } => {
            assert!((rate - 0.10).abs() < 0.001);
        }
        _ => panic!("expected disk write error"),
    }
}

#[test]
fn test_fault_spec_parse_crash() {
    let spec = FaultSpec::parse("crash:1:after:100ms").unwrap();
    
    match spec {
        FaultSpec::Crash { node, after } => {
            assert_eq!(node, 1);
            assert_eq!(after, Duration::from_millis(100));
        }
        _ => panic!("expected crash"),
    }
}

#[test]
fn test_fault_spec_parse_heal() {
    let spec = FaultSpec::parse("heal").unwrap();
    assert!(matches!(spec, FaultSpec::Heal));
}

#[test]
fn test_fault_spec_to_network_fault() {
    let spec = FaultSpec::parse("network:drop:25%").unwrap();
    let fault = spec.to_network_fault();
    
    assert!(fault.is_some());
}

#[test]
fn test_multiple_faults_sequence() {
    let mut cluster = Cluster::new(3);
    
    // Apply sequence of faults
    cluster.crash_node(0);
    assert_eq!(cluster.running_count(), 2);
    
    cluster.partition(&[&[1], &[2]]);
    assert!(!cluster.can_communicate(1, 2));
    
    cluster.restart_node(0);
    assert_eq!(cluster.running_count(), 3);
    
    cluster.heal_partition();
    assert!(cluster.can_communicate(1, 2));
}

#[test]
fn test_fault_isolation() {
    let mut cluster = Cluster::new(4);
    
    // Partition into multiple groups
    cluster.partition(&[&[0], &[1], &[2, 3]]);
    
    // Node 0 isolated
    assert!(!cluster.can_communicate(0, 1));
    assert!(!cluster.can_communicate(0, 2));
    assert!(!cluster.can_communicate(0, 3));
    
    // Node 1 isolated
    assert!(!cluster.can_communicate(1, 0));
    assert!(!cluster.can_communicate(1, 2));
    
    // Nodes 2 and 3 can communicate
    assert!(cluster.can_communicate(2, 3));
}
