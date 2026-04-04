//! Replay command for reproducing recorded executions.

use std::collections::HashMap;
use std::path::PathBuf;

use clap::Args;

use crate::recording::{Event, EventPayload, RecordingReader};
use crate::{Result, TaskId};

use super::output::{print_header, print_kv, print_info, print_warning};

/// Arguments for the replay command.
#[derive(Args, Debug)]
pub struct ReplayArgs {
    /// Recording file to replay.
    pub recording: PathBuf,

    /// Step through execution interactively.
    #[arg(long, short = 'i')]
    pub interactive: bool,

    /// Print each event as it's replayed.
    #[arg(long, short = 'v')]
    pub verbose: bool,

    /// Stop at specific timestamp (nanoseconds).
    #[arg(long)]
    pub stop_at: Option<u64>,

    /// Stop at specific event number.
    #[arg(long)]
    pub stop_event: Option<usize>,

    /// Show only events for specific task.
    #[arg(long, short = 't')]
    pub task: Option<u32>,

    /// Verify determinism by comparing with another recording.
    #[arg(long)]
    pub verify: Option<PathBuf>,

    /// Export timeline to JSON.
    #[arg(long)]
    pub export_json: Option<PathBuf>,
}

/// Current state during replay.
#[derive(Debug)]
pub struct ReplayState {
    /// Current event index.
    pub event_index: usize,
    /// Current simulated time.
    pub current_time: u64,
    /// Active tasks.
    pub active_tasks: HashMap<TaskId, TaskState>,
    /// Recorded random values to feed back.
    pub random_values: Vec<u64>,
    /// Random value index.
    pub random_index: usize,
    /// Recorded time queries.
    pub time_queries: Vec<u64>,
    /// Time query index.
    pub time_index: usize,
    /// Network events for replay.
    pub network_events: Vec<NetworkEvent>,
    /// Network event index.
    pub network_index: usize,
    /// Schedule decisions for verification.
    pub schedule_decisions: Vec<(u32, Vec<u32>)>,
    /// Schedule decision index.
    pub schedule_index: usize,
}

#[derive(Debug, Clone)]
pub struct TaskState {
    pub name: String,
    pub spawned_at: u64,
    pub completed_at: Option<u64>,
    pub parent: u32,
}

#[derive(Debug, Clone)]
pub struct NetworkEvent {
    pub timestamp: u64,
    pub event_type: NetworkEventType,
}

#[derive(Debug, Clone)]
pub enum NetworkEventType {
    Send { task_id: u32, dst: u32, data: Vec<u8> },
    Recv { task_id: u32, src: u32, data: Vec<u8> },
}

impl ReplayState {
    fn new() -> Self {
        Self {
            event_index: 0,
            current_time: 0,
            active_tasks: HashMap::new(),
            random_values: Vec::new(),
            random_index: 0,
            time_queries: Vec::new(),
            time_index: 0,
            network_events: Vec::new(),
            network_index: 0,
            schedule_decisions: Vec::new(),
            schedule_index: 0,
        }
    }

    /// Get the next random value from the recording.
    pub fn next_random(&mut self) -> Option<u64> {
        if self.random_index < self.random_values.len() {
            let value = self.random_values[self.random_index];
            self.random_index += 1;
            Some(value)
        } else {
            None
        }
    }

    /// Get the next time query result from the recording.
    pub fn next_time(&mut self) -> Option<u64> {
        if self.time_index < self.time_queries.len() {
            let value = self.time_queries[self.time_index];
            self.time_index += 1;
            Some(value)
        } else {
            None
        }
    }

    /// Get the next network send event.
    pub fn next_send(&mut self) -> Option<(u32, Vec<u8>)> {
        while self.network_index < self.network_events.len() {
            let event = &self.network_events[self.network_index];
            self.network_index += 1;
            
            if let NetworkEventType::Send { dst, data, .. } = &event.event_type {
                return Some((*dst, data.clone()));
            }
        }
        None
    }

    /// Verify a schedule decision matches the recording.
    pub fn verify_decision(&mut self, chosen: u32, _ready: &[u32]) -> bool {
        if self.schedule_index < self.schedule_decisions.len() {
            let (expected_chosen, _expected_ready) = &self.schedule_decisions[self.schedule_index];
            self.schedule_index += 1;
            *expected_chosen == chosen
        } else {
            true // No more recorded decisions
        }
    }
}

/// Executor for replaying a recording.
pub struct ReplayExecutor {
    reader: RecordingReader,
    state: ReplayState,
    events: Vec<Event>,
    seed: u64,
    strategy: u8,
}

impl ReplayExecutor {
    /// Create a new replay executor from a recording file.
    pub fn new(path: &PathBuf) -> Result<Self> {
        let reader = RecordingReader::open(path)?;
        let seed = reader.seed();
        let strategy = reader.strategy();
        
        Ok(Self {
            reader,
            state: ReplayState::new(),
            events: Vec::new(),
            seed,
            strategy,
        })
    }

    /// Load all events from the recording.
    pub fn load_events(&mut self) -> Result<()> {
        while let Some(event) = self.reader.next_event()? {
            // Index events for replay
            match &event.payload {
                EventPayload::RandomGen { result } => {
                    self.state.random_values.push(*result);
                }
                EventPayload::TimeQuery { result } => {
                    self.state.time_queries.push(*result);
                }
                EventPayload::NetSend { dst, data } => {
                    self.state.network_events.push(NetworkEvent {
                        timestamp: event.timestamp,
                        event_type: NetworkEventType::Send {
                            task_id: event.task_id,
                            dst: *dst,
                            data: data.clone(),
                        },
                    });
                }
                EventPayload::NetRecv { src, data } => {
                    self.state.network_events.push(NetworkEvent {
                        timestamp: event.timestamp,
                        event_type: NetworkEventType::Recv {
                            task_id: event.task_id,
                            src: *src,
                            data: data.clone(),
                        },
                    });
                }
                EventPayload::ScheduleDecision { chosen, ready } => {
                    self.state.schedule_decisions.push((*chosen, ready.clone()));
                }
                EventPayload::TaskSpawn { parent, name } => {
                    self.state.active_tasks.insert(event.task_id, TaskState {
                        name: name.clone(),
                        spawned_at: event.timestamp,
                        completed_at: None,
                        parent: *parent,
                    });
                }
                EventPayload::TaskComplete => {
                    if let Some(task) = self.state.active_tasks.get_mut(&event.task_id) {
                        task.completed_at = Some(event.timestamp);
                    }
                }
                _ => {}
            }
            
            self.events.push(event);
        }
        
        Ok(())
    }

    /// Get the seed from the recording.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Get the strategy from the recording.
    pub fn strategy(&self) -> u8 {
        self.strategy
    }

    /// Get the current replay state.
    pub fn state(&self) -> &ReplayState {
        &self.state
    }

    /// Get mutable state for feeding values.
    pub fn state_mut(&mut self) -> &mut ReplayState {
        &mut self.state
    }

    /// Get the total number of events.
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// Get event by index.
    pub fn event(&self, index: usize) -> Option<&Event> {
        self.events.get(index)
    }

    /// Step to the next event.
    pub fn step(&mut self) -> Option<&Event> {
        if self.state.event_index < self.events.len() {
            let event = &self.events[self.state.event_index];
            self.state.event_index += 1;
            self.state.current_time = event.timestamp;
            Some(event)
        } else {
            None
        }
    }

    /// Reset replay to beginning.
    pub fn reset(&mut self) {
        self.state.event_index = 0;
        self.state.current_time = 0;
        self.state.random_index = 0;
        self.state.time_index = 0;
        self.state.network_index = 0;
        self.state.schedule_index = 0;
    }

    /// Get all events.
    pub fn events(&self) -> &[Event] {
        &self.events
    }
}

/// Result of replay.
#[derive(Debug)]
pub struct ReplayResult {
    pub events_replayed: usize,
    pub final_time: u64,
    pub tasks_spawned: usize,
    pub tasks_completed: usize,
    pub verification_passed: Option<bool>,
}

/// Execute the replay command.
pub fn replay_command(args: ReplayArgs) -> Result<ReplayResult> {
    print_header("Chronos Replay");
    print_kv("Recording:", args.recording.display());

    let mut executor = ReplayExecutor::new(&args.recording)?;
    
    print_kv("Seed:", executor.seed());
    print_kv("Strategy:", executor.strategy());
    
    executor.load_events()?;
    
    print_kv("Total events:", executor.event_count());
    println!();

    // Optional verification against another recording
    let verification_passed = if let Some(ref verify_path) = args.verify {
        Some(verify_recordings(&args.recording, verify_path)?)
    } else {
        None
    };

    // Export to JSON if requested
    if let Some(ref json_path) = args.export_json {
        export_to_json(&executor, json_path)?;
        print_info(&format!("Timeline exported to {:?}", json_path));
    }

    // Replay events
    let mut events_replayed = 0;
    let mut tasks_spawned = 0;
    let mut tasks_completed = 0;

    if args.interactive {
        // Interactive mode
        println!("Interactive replay. Commands: (n)ext, (q)uit, (j)ump <n>, (s)tatus");
        
        loop {
            print!("chronos> ");
            std::io::Write::flush(&mut std::io::stdout()).ok();
            
            let mut input = String::new();
            if std::io::stdin().read_line(&mut input).is_err() {
                break;
            }
            
            let input = input.trim();
            
            match input {
                "n" | "next" | "" => {
                    if let Some(event) = executor.step() {
                        print_event(event, args.verbose);
                        events_replayed += 1;
                        
                        match &event.payload {
                            EventPayload::TaskSpawn { .. } => tasks_spawned += 1,
                            EventPayload::TaskComplete => tasks_completed += 1,
                            _ => {}
                        }
                    } else {
                        println!("End of recording.");
                        break;
                    }
                }
                "q" | "quit" => break,
                "s" | "status" => {
                    let state = executor.state();
                    println!("Event: {}/{}", state.event_index, executor.event_count());
                    println!("Time: {}ns", state.current_time);
                    println!("Active tasks: {}", state.active_tasks.len());
                }
                cmd if cmd.starts_with("j ") || cmd.starts_with("jump ") => {
                    let parts: Vec<&str> = cmd.split_whitespace().collect();
                    if parts.len() == 2 {
                        if let Ok(target) = parts[1].parse::<usize>() {
                            executor.reset();
                            for _ in 0..target {
                                if executor.step().is_none() {
                                    break;
                                }
                            }
                            println!("Jumped to event {}", executor.state().event_index);
                        }
                    }
                }
                _ => println!("Unknown command: {}", input),
            }
        }
    } else {
        // Non-interactive replay
        while let Some(event) = executor.step() {
            // Check stop conditions
            if let Some(stop_at) = args.stop_at {
                if event.timestamp > stop_at {
                    print_info(&format!("Stopped at timestamp {}", stop_at));
                    break;
                }
            }
            
            if let Some(stop_event) = args.stop_event {
                if events_replayed >= stop_event {
                    print_info(&format!("Stopped at event {}", stop_event));
                    break;
                }
            }

            // Filter by task if specified
            if let Some(task_filter) = args.task {
                if event.task_id != task_filter {
                    continue;
                }
            }

            if args.verbose {
                print_event(event, true);
            }

            events_replayed += 1;
            
            match &event.payload {
                EventPayload::TaskSpawn { .. } => tasks_spawned += 1,
                EventPayload::TaskComplete => tasks_completed += 1,
                _ => {}
            }
        }
    }

    let state = executor.state();
    
    println!();
    print_header("Replay Summary");
    print_kv("Events replayed:", events_replayed);
    print_kv("Final time:", format!("{}ns", state.current_time));
    print_kv("Tasks spawned:", tasks_spawned);
    print_kv("Tasks completed:", tasks_completed);
    
    if let Some(passed) = verification_passed {
        print_kv("Verification:", if passed { "PASSED" } else { "FAILED" });
    }

    Ok(ReplayResult {
        events_replayed,
        final_time: state.current_time,
        tasks_spawned,
        tasks_completed,
        verification_passed,
    })
}

fn print_event(event: &Event, _verbose: bool) {
    let time_str = format!("{:>12}ns", event.timestamp);
    let task_str = format!("task {:>3}", event.task_id);
    
    let desc = match &event.payload {
        EventPayload::TaskSpawn { parent, name } => {
            format!("SPAWN {} (parent={})", name, parent)
        }
        EventPayload::TaskYield => "YIELD".to_string(),
        EventPayload::TaskComplete => "COMPLETE".to_string(),
        EventPayload::TimeQuery { result } => {
            format!("TIME -> {}ns", result)
        }
        EventPayload::RandomGen { result } => {
            format!("RANDOM -> {}", result)
        }
        EventPayload::NetSend { dst, data } => {
            format!("SEND -> node {} ({} bytes)", dst, data.len())
        }
        EventPayload::NetRecv { src, data } => {
            format!("RECV <- node {} ({} bytes)", src, data.len())
        }
        EventPayload::ScheduleDecision { chosen, ready } => {
            format!("SCHEDULE chose {} from {:?}", chosen, ready)
        }
        EventPayload::FaultInjected { fault_type, target } => {
            format!("FAULT type={} target={}", fault_type, target)
        }
    };

    println!("[{}] {} {}", time_str, task_str, desc);
}

fn verify_recordings(path1: &PathBuf, path2: &PathBuf) -> Result<bool> {
    let reader1 = RecordingReader::open(path1)?;
    let reader2 = RecordingReader::open(path2)?;

    // Check headers
    if reader1.seed() != reader2.seed() {
        print_warning("Seeds differ");
        return Ok(false);
    }

    // Compare events
    let events1: Vec<_> = reader1.events().collect::<std::result::Result<_, _>>()?;
    let events2: Vec<_> = reader2.events().collect::<std::result::Result<_, _>>()?;

    if events1.len() != events2.len() {
        print_warning(&format!(
            "Event count differs: {} vs {}",
            events1.len(),
            events2.len()
        ));
        return Ok(false);
    }

    for (i, (e1, e2)) in events1.iter().zip(events2.iter()).enumerate() {
        if e1.event_type != e2.event_type {
            print_warning(&format!("Event {} type differs", i));
            return Ok(false);
        }
        if e1.task_id != e2.task_id {
            print_warning(&format!("Event {} task_id differs", i));
            return Ok(false);
        }
        // Note: timestamps might differ slightly, so we don't compare them strictly
    }

    Ok(true)
}

fn export_to_json(executor: &ReplayExecutor, path: &PathBuf) -> Result<()> {
    use std::fs::File;
    use std::io::Write;

    let mut file = File::create(path).map_err(crate::Error::Io)?;
    
    writeln!(file, "{{").map_err(crate::Error::Io)?;
    writeln!(file, "  \"seed\": {},", executor.seed()).map_err(crate::Error::Io)?;
    writeln!(file, "  \"strategy\": {},", executor.strategy()).map_err(crate::Error::Io)?;
    writeln!(file, "  \"events\": [").map_err(crate::Error::Io)?;
    
    let events = executor.events();
    for (i, event) in events.iter().enumerate() {
        let comma = if i < events.len() - 1 { "," } else { "" };
        writeln!(
            file,
            "    {{\"type\": \"{:?}\", \"task\": {}, \"time\": {}}}{}",
            event.event_type, event.task_id, event.timestamp, comma
        ).map_err(crate::Error::Io)?;
    }
    
    writeln!(file, "  ]").map_err(crate::Error::Io)?;
    writeln!(file, "}}").map_err(crate::Error::Io)?;
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::{Header, RecordingWriter};
    use tempfile::tempdir;

    fn create_test_recording(path: &std::path::Path) {
        let header = Header::new(42, 1);
        let mut writer = RecordingWriter::new(path, header).unwrap();
        
        writer.write_event(&Event::task_spawn(1, 0, "main".to_string(), 0)).unwrap();
        writer.write_event(&Event::random_gen(1, 100, 12345)).unwrap();
        writer.write_event(&Event::time_query(1, 200, 200)).unwrap();
        writer.write_event(&Event::task_yield(1, 300)).unwrap();
        writer.write_event(&Event::task_complete(1, 400)).unwrap();
        
        writer.finish().unwrap();
    }

    #[test]
    fn test_replay_executor_new() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.chrn");
        create_test_recording(&path);
        
        let executor = ReplayExecutor::new(&path).unwrap();
        assert_eq!(executor.seed(), 42);
        assert_eq!(executor.strategy(), 1);
    }

    #[test]
    fn test_replay_executor_load_events() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.chrn");
        create_test_recording(&path);
        
        let mut executor = ReplayExecutor::new(&path).unwrap();
        executor.load_events().unwrap();
        
        assert_eq!(executor.event_count(), 5);
    }

    #[test]
    fn test_replay_executor_step() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.chrn");
        create_test_recording(&path);
        
        let mut executor = ReplayExecutor::new(&path).unwrap();
        executor.load_events().unwrap();
        
        let event = executor.step().unwrap();
        assert!(matches!(event.payload, EventPayload::TaskSpawn { .. }));
        
        let event = executor.step().unwrap();
        assert!(matches!(event.payload, EventPayload::RandomGen { .. }));
    }

    #[test]
    fn test_replay_state_next_random() {
        let mut state = ReplayState::new();
        state.random_values = vec![100, 200, 300];
        
        assert_eq!(state.next_random(), Some(100));
        assert_eq!(state.next_random(), Some(200));
        assert_eq!(state.next_random(), Some(300));
        assert_eq!(state.next_random(), None);
    }

    #[test]
    fn test_replay_executor_reset() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.chrn");
        create_test_recording(&path);
        
        let mut executor = ReplayExecutor::new(&path).unwrap();
        executor.load_events().unwrap();
        
        executor.step();
        executor.step();
        assert_eq!(executor.state().event_index, 2);
        
        executor.reset();
        assert_eq!(executor.state().event_index, 0);
    }
}
