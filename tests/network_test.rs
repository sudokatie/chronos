//! Tests for the network simulation system.

use std::time::Duration;
use chronos::network::{NetworkSim, NetworkConfig, LatencyModel, Fault};
use chronos::time::Instant;

#[test]
fn test_network_new() {
    let net = NetworkSim::with_seed(42);
    assert_eq!(net.in_flight_count(), 0);
}

#[test]
fn test_network_connect_and_send() {
    let mut net = NetworkSim::with_seed(42);
    net.connect(1, 2);
    
    let now = Instant::from_nanos(0);
    net.send(1, 2, vec![1, 2, 3], now).unwrap();
    
    assert_eq!(net.in_flight_count(), 1);
}

#[test]
fn test_network_fixed_latency() {
    let mut config = NetworkConfig::default();
    config.latency = LatencyModel::fixed(Duration::from_millis(10));
    
    let mut net = NetworkSim::new(config, 42);
    net.connect(1, 2);
    
    let now = Instant::from_nanos(0);
    net.send(1, 2, vec![42], now).unwrap();
    
    // Not yet delivered
    net.tick(Instant::from_nanos(5_000_000));
    assert!(net.recv(2).is_none());
    
    // Now delivered
    net.tick(Instant::from_nanos(10_000_000));
    let msg = net.recv(2).unwrap();
    assert_eq!(msg.data, vec![42]);
}

#[test]
fn test_network_partition() {
    let mut config = NetworkConfig::default();
    config.latency = LatencyModel::fixed(Duration::from_millis(1));
    
    let mut net = NetworkSim::new(config, 42);
    net.connect(1, 2);
    net.connect(2, 3);
    
    // Partition: [1, 2] and [3]
    net.partition(vec![vec![1, 2], vec![3]]);
    
    let now = Instant::from_nanos(0);
    
    // 1 -> 2 should work (same group)
    net.send(1, 2, vec![1], now).unwrap();
    
    // 2 -> 3 should be dropped (different groups)
    net.send(2, 3, vec![2], now).unwrap();
    
    net.tick(Instant::from_nanos(2_000_000));
    
    assert!(net.recv(2).is_some()); // 1->2 delivered
    assert!(net.recv(3).is_none()); // 2->3 dropped
}

#[test]
fn test_network_heal() {
    let mut config = NetworkConfig::default();
    config.latency = LatencyModel::fixed(Duration::from_millis(1));
    
    let mut net = NetworkSim::new(config, 42);
    net.connect(1, 2);
    
    net.partition(vec![vec![1], vec![2]]);
    assert!(!net.can_communicate(1, 2));
    
    net.heal();
    assert!(net.can_communicate(1, 2));
}

#[test]
fn test_network_drop_rate() {
    let mut config = NetworkConfig::default();
    config.latency = LatencyModel::fixed(Duration::from_millis(1));
    config.drop_rate = 1.0; // Drop everything
    
    let mut net = NetworkSim::new(config, 42);
    net.connect(1, 2);
    
    let now = Instant::from_nanos(0);
    net.send(1, 2, vec![1], now).unwrap();
    
    net.tick(Instant::from_nanos(2_000_000));
    
    // Should be dropped
    assert!(net.recv(2).is_none());
}

#[test]
fn test_network_bidirectional() {
    let mut config = NetworkConfig::default();
    config.latency = LatencyModel::fixed(Duration::from_millis(1));
    
    let mut net = NetworkSim::new(config, 42);
    net.connect(1, 2);
    
    let now = Instant::from_nanos(0);
    net.send(1, 2, vec![1], now).unwrap();
    net.send(2, 1, vec![2], now).unwrap();
    
    net.tick(Instant::from_nanos(2_000_000));
    
    assert!(net.recv(2).is_some());
    assert!(net.recv(1).is_some());
}

#[test]
fn test_network_fifo_ordering() {
    let mut config = NetworkConfig::default();
    config.latency = LatencyModel::fixed(Duration::from_millis(1));
    
    let mut net = NetworkSim::new(config, 42);
    net.connect(1, 2);
    
    let now = Instant::from_nanos(0);
    net.send(1, 2, vec![1], now).unwrap();
    net.send(1, 2, vec![2], now).unwrap();
    net.send(1, 2, vec![3], now).unwrap();
    
    net.tick(Instant::from_nanos(2_000_000));
    
    assert_eq!(net.recv(2).unwrap().data, vec![1]);
    assert_eq!(net.recv(2).unwrap().data, vec![2]);
    assert_eq!(net.recv(2).unwrap().data, vec![3]);
}

#[test]
fn test_network_reset() {
    let mut net = NetworkSim::with_seed(42);
    net.connect(1, 2);
    net.send(1, 2, vec![1], Instant::from_nanos(0)).unwrap();
    net.partition(vec![vec![1], vec![2]]);
    
    net.reset();
    
    assert_eq!(net.in_flight_count(), 0);
    assert!(net.can_communicate(1, 2));
}

#[test]
fn test_latency_model_uniform() {
    use rand::SeedableRng;
    use rand::rngs::StdRng;
    
    let model = LatencyModel::uniform(
        Duration::from_millis(10),
        Duration::from_millis(20),
    );
    let mut rng = StdRng::seed_from_u64(42);
    
    for _ in 0..100 {
        let latency = model.sample(&mut rng);
        assert!(latency >= Duration::from_millis(10));
        assert!(latency < Duration::from_millis(20));
    }
}

#[test]
fn test_latency_model_bimodal() {
    use rand::SeedableRng;
    use rand::rngs::StdRng;
    
    let model = LatencyModel::bimodal(
        Duration::from_millis(1),
        Duration::from_millis(100),
        0.5,
    );
    let mut rng = StdRng::seed_from_u64(42);
    
    let samples: Vec<_> = (0..1000).map(|_| model.sample(&mut rng)).collect();
    
    let fast_count = samples.iter().filter(|&&d| d == Duration::from_millis(1)).count();
    let slow_count = samples.iter().filter(|&&d| d == Duration::from_millis(100)).count();
    
    // Both should be roughly 500, allow reasonable deviation
    assert!(fast_count > 400 && fast_count < 600);
    assert!(slow_count > 400 && slow_count < 600);
}

#[test]
fn test_network_scheduled_fault() {
    let mut config = NetworkConfig::default();
    config.latency = LatencyModel::fixed(Duration::from_millis(1));
    
    let mut net = NetworkSim::new(config, 42);
    net.connect(1, 2);
    
    // Schedule partition at 5ms
    net.schedule_fault(
        Instant::from_nanos(5_000_000),
        Fault::partition(vec![vec![1], vec![2]]),
    );
    
    assert!(net.can_communicate(1, 2));
    
    // Tick past the fault time
    net.tick(Instant::from_nanos(6_000_000));
    
    assert!(!net.can_communicate(1, 2));
}

#[test]
fn test_network_peek() {
    let mut config = NetworkConfig::default();
    config.latency = LatencyModel::fixed(Duration::from_millis(1));
    
    let mut net = NetworkSim::new(config, 42);
    net.connect(1, 2);
    net.send(1, 2, vec![42], Instant::from_nanos(0)).unwrap();
    net.tick(Instant::from_nanos(2_000_000));
    
    // Peek doesn't remove
    assert!(net.peek(2).is_some());
    assert!(net.peek(2).is_some());
    assert_eq!(net.inbox_len(2), 1);
    
    // Recv removes
    net.recv(2);
    assert!(net.peek(2).is_none());
}
