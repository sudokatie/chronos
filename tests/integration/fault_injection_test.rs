//! Fault injection integration tests.

use std::time::Duration;

use chronos::cluster::Cluster;
use chronos::network::Fault;
use chronos::time::Instant;

/// Test scheduled partition fault via network.
#[test]
fn test_scheduled_partition() {
    let mut cluster = Cluster::with_seed(3, 42);
    
    // Initially all can communicate (network level)
    assert!(cluster.network().can_communicate(0, 1));
    assert!(cluster.network().can_communicate(0, 2));
    
    // Schedule partition at 5 seconds on the network
    cluster.network_mut().schedule_fault(
        Instant::from_nanos(5_000_000_000),
        Fault::partition(vec![vec![0, 1], vec![2]]),
    );
    
    // Run to just before partition
    cluster.advance_time(Duration::from_secs(4));
    assert!(cluster.network().can_communicate(0, 2));
    
    // Run past partition time (network ticks during advance_time)
    cluster.advance_time(Duration::from_secs(2));
    assert!(!cluster.network().can_communicate(0, 2));
}

/// Test partition via cluster API (direct, not scheduled).
#[test]
fn test_cluster_partition_direct() {
    let mut cluster = Cluster::with_seed(3, 42);
    
    // Initially all can communicate
    assert!(cluster.can_communicate(0, 1));
    assert!(cluster.can_communicate(0, 2));
    
    // Apply partition directly
    cluster.partition(&[&[0, 1], &[2]]);
    
    // Now partitioned
    assert!(cluster.can_communicate(0, 1));  // Same group
    assert!(!cluster.can_communicate(0, 2)); // Different groups
    
    // Heal
    cluster.heal_partition();
    assert!(cluster.can_communicate(0, 2));
}

/// Test crash and restart.
#[test]
fn test_crash_restart_flow() {
    let mut cluster = Cluster::new(3);
    
    // All running
    assert_eq!(cluster.running_count(), 3);
    
    // Crash node 1
    cluster.crash_node(1);
    assert!(cluster.node(1).unwrap().is_crashed());
    assert_eq!(cluster.running_count(), 2);
    
    // Advance time
    cluster.advance_time(Duration::from_secs(1));
    
    // Node should still be crashed
    assert!(cluster.node(1).unwrap().is_crashed());
    
    // Restart
    cluster.restart_node(1);
    assert!(cluster.node(1).unwrap().is_running());
    assert_eq!(cluster.running_count(), 3);
}

/// Test clock skew fault.
#[test]
fn test_clock_skew() {
    let mut cluster = Cluster::new(2);
    
    // Set node 0 to run at 2x speed
    cluster.set_clock_skew(0, 2.0);
    
    // Advance global time by 1 second
    cluster.advance_time(Duration::from_secs(1));
    
    // Global time is 1s
    assert_eq!(cluster.now().as_nanos(), 1_000_000_000);
    
    // Node 0 sees 2 seconds
    assert_eq!(cluster.node_time(0).as_nanos(), 2_000_000_000);
    
    // Node 1 sees 1 second
    assert_eq!(cluster.node_time(1).as_nanos(), 1_000_000_000);
}

/// Test clock jump fault.
#[test]
fn test_clock_jump() {
    let mut cluster = Cluster::new(2);
    
    // Jump node 0 forward by 5 seconds
    cluster.clock_jump_forward(0, Duration::from_secs(5));
    
    // Node 0 should be 5 seconds ahead
    assert_eq!(cluster.node_time(0).as_nanos(), 5_000_000_000);
    assert_eq!(cluster.node_time(1).as_nanos(), 0);
    
    // Advance time
    cluster.advance_time(Duration::from_secs(1));
    
    // Node 0 still 5 seconds ahead
    assert_eq!(cluster.node_time(0).as_nanos(), 6_000_000_000);
    assert_eq!(cluster.node_time(1).as_nanos(), 1_000_000_000);
}

/// Test combined faults.
#[test]
fn test_combined_faults() {
    let mut cluster = Cluster::new(4);
    
    // Partition
    cluster.partition(&[&[0, 1], &[2, 3]]);
    
    // Crash one node in each partition
    cluster.crash_node(1);
    cluster.crash_node(3);
    
    // Set clock skew on remaining nodes
    cluster.set_clock_skew(0, 0.5);  // Slow
    cluster.set_clock_skew(2, 1.5);  // Fast
    
    // Run simulation
    cluster.advance_time(Duration::from_secs(10));
    
    // Verify state
    assert_eq!(cluster.running_count(), 2);
    assert!(!cluster.can_communicate(0, 2));
    
    // Node 0 (slow) saw less time
    assert!(cluster.node_time(0).as_nanos() < cluster.now().as_nanos());
    
    // Node 2 (fast) saw more time
    assert!(cluster.node_time(2).as_nanos() > cluster.now().as_nanos());
}
