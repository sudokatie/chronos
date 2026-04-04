//! Basic integration tests for Chronos.

use std::time::Duration;

use chronos::cluster::Cluster;
use chronos::time::Instant;
use chronos::runtime::{Runtime, StepResult};

#[test]
fn test_cluster_basic() {
    let mut cluster = Cluster::new(3);
    
    assert_eq!(cluster.size(), 3);
    assert_eq!(cluster.running_count(), 3);
    
    cluster.advance_time(Duration::from_secs(1));
    assert_eq!(cluster.now().as_nanos(), 1_000_000_000);
}

#[test]
fn test_cluster_with_seed() {
    let c1 = Cluster::with_seed(3, 42);
    let c2 = Cluster::with_seed(3, 42);
    
    assert_eq!(c1.size(), c2.size());
    assert_eq!(c1.seed(), c2.seed());
}

#[test]
fn test_cluster_node_access() {
    let cluster = Cluster::new(3);
    
    assert!(cluster.node(0).is_some());
    assert!(cluster.node(1).is_some());
    assert!(cluster.node(2).is_some());
    assert!(cluster.node(3).is_none());
}

#[test]
fn test_cluster_crash_restart() {
    let mut cluster = Cluster::new(3);
    
    cluster.crash_node(1);
    assert_eq!(cluster.running_count(), 2);
    assert_eq!(cluster.crashed_count(), 1);
    assert!(cluster.node(1).unwrap().is_crashed());
    
    cluster.restart_node(1);
    assert_eq!(cluster.running_count(), 3);
    assert!(cluster.node(1).unwrap().is_running());
}

#[test]
fn test_cluster_partition() {
    let mut cluster = Cluster::new(3);
    
    // Partition: {0, 1} and {2}
    cluster.partition(&[&[0, 1], &[2]]);
    
    assert!(cluster.can_communicate(0, 1));
    assert!(cluster.can_communicate(1, 0));
    assert!(!cluster.can_communicate(0, 2));
    assert!(!cluster.can_communicate(2, 1));
}

#[test]
fn test_cluster_heal_partition() {
    let mut cluster = Cluster::new(3);
    
    cluster.partition(&[&[0, 1], &[2]]);
    assert!(!cluster.can_communicate(0, 2));
    
    cluster.heal_partition();
    assert!(cluster.can_communicate(0, 2));
}

#[test]
fn test_cluster_advance_time() {
    let mut cluster = Cluster::new(2);
    
    assert_eq!(cluster.now(), Instant::from_nanos(0));
    
    cluster.advance_time(Duration::from_secs(1));
    assert_eq!(cluster.now().as_nanos(), 1_000_000_000);
}

#[test]
fn test_cluster_is_stable() {
    let cluster = Cluster::new(2);
    assert!(cluster.is_stable());
}

#[test]
fn test_cluster_happens_before() {
    let mut cluster = Cluster::new(2);
    
    let e1 = cluster.record_event(0, "event 1");
    let e2 = cluster.record_event(0, "event 2");
    
    assert!(cluster.happened_before(e1, e2));
    assert!(!cluster.happened_before(e2, e1));
}

#[test]
fn test_cluster_concurrent_events() {
    let mut cluster = Cluster::new(2);
    
    let e1 = cluster.record_event(0, "node 0 event");
    let e2 = cluster.record_event(1, "node 1 event");
    
    assert!(cluster.concurrent(e1, e2));
}

#[test]
fn test_cluster_send_recv_causality() {
    let mut cluster = Cluster::new(2);
    
    let send = cluster.record_send(0, "send from 0");
    let recv = cluster.record_recv(1, send, "recv at 1");
    
    assert!(cluster.happened_before(send, recv));
}

#[test]
fn test_cluster_reset() {
    let mut cluster = Cluster::new(3);
    
    cluster.crash_node(0);
    cluster.partition(&[&[1], &[2]]);
    cluster.advance_time(Duration::from_secs(1));
    cluster.record_event(1, "test");
    
    cluster.reset();
    
    assert_eq!(cluster.running_count(), 3);
    assert!(cluster.can_communicate(1, 2));
    assert_eq!(cluster.now(), Instant::from_nanos(0));
    assert_eq!(cluster.happens_before_graph().event_count(), 0);
}

#[test]
fn test_runtime_basic() {
    let mut rt = Runtime::with_seed(42);
    let handle = rt.spawn(async {});
    
    assert_eq!(rt.task_count(), 1);
    assert!(!handle.is_complete());
    
    rt.run_until_stable().unwrap();
    assert!(handle.is_complete());
}

#[test]
fn test_runtime_multiple_tasks() {
    let mut rt = Runtime::with_seed(42);
    let h1 = rt.spawn(async {});
    let h2 = rt.spawn(async {});
    let h3 = rt.spawn(async {});
    
    rt.run_until_stable().unwrap();
    
    assert!(h1.is_complete());
    assert!(h2.is_complete());
    assert!(h3.is_complete());
}

#[test]
fn test_runtime_step_by_step() {
    let mut rt = Runtime::with_seed(42);
    rt.spawn(async {});
    rt.spawn(async {});
    
    assert_eq!(rt.completed_count(), 0);
    
    let result = rt.step();
    assert!(matches!(result, StepResult::TaskPolled(_)));
    assert_eq!(rt.completed_count(), 1);
    
    let result = rt.step();
    assert!(matches!(result, StepResult::TaskPolled(_)));
    assert_eq!(rt.completed_count(), 2);
}

#[test]
fn test_runtime_run_for() {
    let mut rt = Runtime::with_seed(42);
    rt.spawn(async {});
    
    rt.run_for(Duration::from_secs(1)).unwrap();
}

#[test]
fn test_time_instant_arithmetic() {
    let t1 = Instant::from_nanos(1_000_000_000);
    let t2 = Instant::from_nanos(2_000_000_000);
    
    let duration = t2.duration_since(t1).unwrap();
    assert_eq!(duration.as_secs(), 1);
    
    let t3 = t1.saturating_add(Duration::from_secs(5));
    assert_eq!(t3.as_nanos(), 6_000_000_000);
}

#[test]
fn test_full_simulation_flow() {
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

#[test]
fn test_cluster_clock_skew() {
    let mut cluster = Cluster::new(2);
    
    // Default skew is 1.0
    assert_eq!(cluster.clock_skew(0), 1.0);
    
    // Set node 0 to run at 2x speed
    cluster.set_clock_skew(0, 2.0);
    
    // Advance global time by 1 second
    cluster.advance_time(Duration::from_secs(1));
    
    // Node 0 should see 2 seconds, node 1 sees 1 second
    let node0_time = cluster.node_time(0);
    let node1_time = cluster.node_time(1);
    
    assert_eq!(node0_time.as_nanos(), 2_000_000_000);
    assert_eq!(node1_time.as_nanos(), 1_000_000_000);
}

#[test]
fn test_cluster_clock_jump() {
    let mut cluster = Cluster::new(2);
    
    // Jump node 0 forward by 5 seconds
    cluster.clock_jump_forward(0, Duration::from_secs(5));
    
    // Node 0 should be 5 seconds ahead
    assert_eq!(cluster.node_time(0).as_nanos(), 5_000_000_000);
    assert_eq!(cluster.node_time(1).as_nanos(), 0);
}
