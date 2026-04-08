//! Basic integration tests for Chronos.

use std::time::Duration;

use chronos::cluster::Cluster;
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
    
    // Verify partition
    assert!(cluster.can_communicate(0, 1));
    assert!(!cluster.can_communicate(0, 2));
    
    // Crash and restart node
    cluster.crash_node(1);
    assert!(cluster.node(1).unwrap().is_crashed());
    
    cluster.restart_node(1);
    assert!(cluster.node(1).unwrap().is_running());
    
    // Heal partition
    cluster.heal_partition();
    assert!(cluster.can_communicate(0, 2));
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
