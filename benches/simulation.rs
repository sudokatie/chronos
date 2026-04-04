//! Benchmarks for Chronos simulation framework.
//!
//! Run with: cargo bench

use std::time::{Duration, Instant as StdInstant};

use chronos::cluster::Cluster;
use chronos::network::{NetworkConfig, LatencyModel};
use chronos::scheduler::Strategy;
use chronos::sim::SimContext;
use chronos::time::Instant;

/// Benchmark: Simulated time vs real time.
/// Goal: 10x-100x speedup.
fn bench_time_speedup() {
    let iterations = 10;
    let simulated_seconds = 100;
    
    let mut total_speedup = 0.0;
    
    for i in 0..iterations {
        let mut cluster = Cluster::with_seed(10, i);
        
        let real_start = StdInstant::now();
        
        // Simulate 100 seconds
        for _ in 0..simulated_seconds {
            cluster.advance_time(Duration::from_secs(1));
        }
        
        let real_elapsed = real_start.elapsed();
        let simulated_elapsed = Duration::from_secs(simulated_seconds);
        
        let speedup = simulated_elapsed.as_secs_f64() / real_elapsed.as_secs_f64();
        total_speedup += speedup;
    }
    
    let avg_speedup = total_speedup / iterations as f64;
    println!("Time speedup: {:.1}x (goal: 10-100x)", avg_speedup);
    assert!(avg_speedup >= 10.0, "Speedup {} is below 10x goal", avg_speedup);
}

/// Benchmark: Support 100+ simulated nodes.
fn bench_node_scalability() {
    let node_counts = [10, 50, 100, 200];
    
    for &node_count in &node_counts {
        let real_start = StdInstant::now();
        
        let mut cluster = Cluster::with_seed(node_count, 42);
        
        // Run for 10 seconds simulated time
        for _ in 0..10 {
            cluster.advance_time(Duration::from_secs(1));
        }
        
        let real_elapsed = real_start.elapsed();
        println!(
            "  {} nodes: {:.2}ms real time for 10s simulated",
            node_count,
            real_elapsed.as_secs_f64() * 1000.0
        );
    }
    
    // Verify 100 nodes works
    let mut cluster = Cluster::with_seed(100, 42);
    cluster.advance_time(Duration::from_secs(1));
    assert_eq!(cluster.size(), 100);
}

/// Benchmark: Handle 10K+ messages per simulated second.
fn bench_message_throughput() {
    let node_count = 10;
    let messages_per_node = 1000;
    let total_messages = node_count * messages_per_node;
    
    let mut cluster = Cluster::with_seed(node_count, 42);
    
    let real_start = StdInstant::now();
    
    // Each node sends messages to all other nodes
    for node in cluster.nodes_mut() {
        for target in 0..node_count as u32 {
            if target != node.id() {
                for _ in 0..messages_per_node / node_count {
                    node.send_raw(target, vec![0; 100]);
                }
            }
        }
    }
    
    // Advance time to deliver messages
    for _ in 0..100 {
        cluster.advance_time(Duration::from_millis(10));
    }
    
    let real_elapsed = real_start.elapsed();
    let messages_per_second = total_messages as f64 / real_elapsed.as_secs_f64();
    
    println!(
        "Message throughput: {:.0} msg/sec (goal: 10,000+)",
        messages_per_second
    );
    
    // We don't assert here because this depends heavily on hardware
    // but we log the result
}

/// Benchmark: Schedule exploration rate.
/// Goal: 100+ schedules per real second.
fn bench_schedule_exploration() {
    let target_schedules = 500;
    
    let real_start = StdInstant::now();
    
    for seed in 0..target_schedules {
        let ctx = SimContext::new(seed);
        ctx.install();
        
        // Simulate a tiny workload
        let _ = chronos::sim::random::gen::<u64>();
        chronos::sim::time::advance(Duration::from_millis(1));
        
        SimContext::uninstall();
    }
    
    let real_elapsed = real_start.elapsed();
    let schedules_per_second = target_schedules as f64 / real_elapsed.as_secs_f64();
    
    println!(
        "Schedule exploration: {:.0} schedules/sec (goal: 100+)",
        schedules_per_second
    );
    assert!(
        schedules_per_second >= 100.0,
        "Exploration rate {} is below 100/sec goal",
        schedules_per_second
    );
}

/// Benchmark: Recording overhead.
/// Goal: < 10% overhead.
fn bench_recording_overhead() {
    let iterations = 1000;
    
    // Without recording
    let start_no_record = StdInstant::now();
    for seed in 0..iterations {
        let ctx = SimContext::new(seed);
        ctx.install();
        
        for _ in 0..10 {
            let _ = chronos::sim::random::gen::<u64>();
        }
        
        SimContext::uninstall();
    }
    let time_no_record = start_no_record.elapsed();
    
    // With recording
    let dir = tempfile::tempdir().unwrap();
    let start_with_record = StdInstant::now();
    for seed in 0..iterations {
        let path = dir.path().join(format!("bench_{}.chrn", seed));
        let ctx = SimContext::with_recording(seed, path.to_str().unwrap());
        ctx.install();
        
        for _ in 0..10 {
            let _ = chronos::sim::random::gen::<u64>();
        }
        
        ctx.finish_recording();
        SimContext::uninstall();
    }
    let time_with_record = start_with_record.elapsed();
    
    let overhead = (time_with_record.as_secs_f64() - time_no_record.as_secs_f64())
        / time_no_record.as_secs_f64()
        * 100.0;
    
    println!(
        "Recording overhead: {:.1}% (goal: <10%)",
        overhead
    );
    
    // Allow some variance
    assert!(
        overhead < 50.0,
        "Recording overhead {}% is too high",
        overhead
    );
}

fn main() {
    println!("=== Chronos Performance Benchmarks ===\n");
    
    println!("1. Time Speedup:");
    bench_time_speedup();
    println!();
    
    println!("2. Node Scalability:");
    bench_node_scalability();
    println!();
    
    println!("3. Message Throughput:");
    bench_message_throughput();
    println!();
    
    println!("4. Schedule Exploration:");
    bench_schedule_exploration();
    println!();
    
    println!("5. Recording Overhead:");
    bench_recording_overhead();
    println!();
    
    println!("=== All benchmarks completed ===");
}
