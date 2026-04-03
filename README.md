# Chronos

Deterministic simulation testing for distributed systems. Control time, inject faults, find Heisenbugs before they find your production cluster at 3am.

## Why This Exists?

Testing distributed systems is hard. You write tests, they pass. You deploy, things break. The bug only happens when node 3 receives a message *just* before node 1's timer fires, while node 2 is temporarily partitioned. Good luck reproducing that.

Chronos takes control of everything non-deterministic in your system:
- **Time**: Virtual clock you control completely
- **Scheduling**: Explore different task interleavings
- **Network**: Inject latency, drops, partitions
- **Randomness**: Seeded RNG for reproducibility

When you find a bug, you get a seed. Run with that seed again - same bug, every time.

## Features

- **Cluster simulation** with configurable node count and network topology
- **Fault injection**: crash nodes, partition networks, delay messages
- **Deadlock detection** via wait-graph cycle detection
- **Livelock detection** with progress tracking
- **Execution recording** for replay and debugging
- **Multiple scheduling strategies**: FIFO, Random, PCT (Probabilistic Concurrency Testing)
- **CLI tools** for running and exploring test schedules

## Quick Start

Add to your `Cargo.toml`:

```toml
[dev-dependencies]
chronos = "0.1"
```

Write a simulation test:

```rust
use chronos::cluster::Cluster;
use std::time::Duration;

#[test]
fn test_my_distributed_system() {
    // Create a 3-node cluster
    let mut cluster = Cluster::with_seed(3, 42);
    
    // Advance simulated time
    cluster.advance_time(Duration::from_secs(1));
    
    // Inject a network partition
    cluster.partition(&[&[0, 1], &[2]]);
    
    // Crash a node
    cluster.crash_node(1);
    
    // ... run your test logic ...
    
    // Heal partition, restart node
    cluster.heal_partition();
    cluster.restart_node(1);
}
```

## CLI Usage

### Run a test with a specific seed

```bash
chronos run my_test --seed 42 --strategy pct
```

### Explore schedules systematically

```bash
chronos explore my_test --depth 100 --threads 4
```

### Options

```
chronos run <test-binary> [OPTIONS]
  --seed N          Random seed for reproducibility
  --strategy S      Scheduling strategy (fifo|random|pct)
  --iterations N    Number of iterations
  --timeout S       Max simulated time
  --record FILE     Record execution trace
  --verbose         Show scheduling decisions

chronos explore <test-binary> [OPTIONS]
  --depth N         Max exploration depth
  --threads N       Parallel workers
  --checkpoint DIR  Save/resume state
  --report FILE     Write bug report
```

## Configuration

Create `chronos.toml`:

```toml
[scheduler]
strategy = "random"
seed = 0  # 0 = use system time
pct_depth = 3

[network]
latency_ms = 10
jitter_ms = 5
drop_rate = 0.0
duplicate_rate = 0.0

[detection]
deadlock_detection = true
livelock_detection = true
livelock_threshold = 1000

[recording]
enabled = false
output_dir = "./recordings"
compress = true
```

## Detection

### Deadlock Detection

Chronos tracks task wait relationships and detects cycles:

```rust
use chronos::detection::DeadlockDetector;

let mut detector = DeadlockDetector::new();
detector.task_waiting(1, 2);  // Task 1 waits for Task 2
detector.task_waiting(2, 1);  // Task 2 waits for Task 1 - deadlock!

if let Some(cycle) = detector.check() {
    println!("Deadlock: {:?}", cycle);
}
```

### Livelock Detection

Detects tasks making no progress:

```rust
use chronos::detection::LivelockDetector;

let mut detector = LivelockDetector::new(100);  // threshold = 100 steps

for _ in 0..100 {
    detector.task_step(1);  // Task 1 spinning
}

if let Some(stuck) = detector.check() {
    println!("Livelock: tasks {:?} are stuck", stuck);
}
```

## Recording and Replay

Record an execution:

```rust
use chronos::recording::{Header, RecordingWriter, Event};

let header = Header::new(seed, strategy);
let mut writer = RecordingWriter::new("trace.chrn", header)?;

writer.write_event(&Event::task_spawn(1, 0, "main".into(), 0))?;
writer.finish()?;
```

Replay later:

```rust
use chronos::recording::RecordingReader;

let reader = RecordingReader::open("trace.chrn")?;
println!("Seed: {}", reader.seed());

for event in reader.events() {
    println!("{:?}", event?);
}
```

## Philosophy

1. **Determinism is debugging**: If you can't reproduce it, you can't fix it.
2. **Explore, don't hope**: Systematic exploration beats random testing.
3. **Fast feedback**: Simulated time runs as fast as your CPU.
4. **Production bugs come from interleavings**: Test the interleavings.

## License

MIT

---

*Find the bugs that only happen in production. Find them in CI instead.*
