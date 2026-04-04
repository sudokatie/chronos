//! Systematic schedule exploration command.

use std::collections::{HashSet, VecDeque};
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant as StdInstant};

use clap::Args;

use crate::Result;

use super::output::{print_header, print_kv, print_info, print_warning, print_error, 
    print_progress, clear_progress};

/// Arguments for the explore command.
#[derive(Args, Debug)]
pub struct ExploreArgs {
    /// Test binary to explore.
    pub test_binary: String,

    /// Max exploration depth for DFS.
    #[arg(long, short = 'd', default_value = "100")]
    pub depth: usize,

    /// Number of parallel exploration workers.
    #[arg(long, short = 'j', default_value = "1")]
    pub threads: usize,

    /// Directory to save/resume exploration state.
    #[arg(long, short = 'c')]
    pub checkpoint: Option<PathBuf>,

    /// Write bug reports to this file.
    #[arg(long, short = 'r')]
    pub report: Option<PathBuf>,

    /// Starting random seed.
    #[arg(long, short = 's', default_value = "0")]
    pub seed: u64,

    /// Maximum number of schedules to explore.
    #[arg(long, short = 'n')]
    pub max_schedules: Option<u64>,

    /// Timeout per schedule (seconds).
    #[arg(long, short = 't', default_value = "60")]
    pub timeout: u64,

    /// Stop after finding this many bugs.
    #[arg(long, default_value = "1")]
    pub max_bugs: u32,

    /// Verbose output.
    #[arg(long, short = 'v')]
    pub verbose: bool,

    /// Use PCT strategy instead of random.
    #[arg(long)]
    pub pct: bool,

    /// PCT bug depth parameter.
    #[arg(long, default_value = "3")]
    pub pct_depth: usize,

    /// Run as cargo test.
    #[arg(long)]
    pub cargo: bool,
}

/// A bug found during exploration.
#[derive(Debug, Clone)]
pub struct Bug {
    pub seed: u64,
    pub schedule_id: u64,
    pub description: String,
    pub trace: Vec<String>,
}

/// Exploration state that can be checkpointed.
#[derive(Debug, Clone)]
pub struct ExplorationState {
    /// Seeds that have been explored.
    pub explored_seeds: HashSet<u64>,
    /// Seeds that found bugs.
    pub bug_seeds: HashSet<u64>,
    /// Next seed to try.
    pub next_seed: u64,
    /// Total schedules explored.
    pub schedules_explored: u64,
}

impl ExplorationState {
    fn new(start_seed: u64) -> Self {
        Self {
            explored_seeds: HashSet::new(),
            bug_seeds: HashSet::new(),
            next_seed: start_seed,
            schedules_explored: 0,
        }
    }

    fn load(path: &PathBuf) -> Result<Self> {
        let file = File::open(path).map_err(crate::Error::Io)?;
        let reader = BufReader::new(file);
        
        let mut state = Self::new(0);
        
        for line in reader.lines() {
            let line = line.map_err(crate::Error::Io)?;
            let parts: Vec<&str> = line.split('=').collect();
            if parts.len() != 2 {
                continue;
            }
            
            match parts[0] {
                "next_seed" => {
                    state.next_seed = parts[1].parse().unwrap_or(0);
                }
                "schedules_explored" => {
                    state.schedules_explored = parts[1].parse().unwrap_or(0);
                }
                "explored" => {
                    for seed_str in parts[1].split(',') {
                        if let Ok(seed) = seed_str.parse() {
                            state.explored_seeds.insert(seed);
                        }
                    }
                }
                "bugs" => {
                    for seed_str in parts[1].split(',') {
                        if let Ok(seed) = seed_str.parse() {
                            state.bug_seeds.insert(seed);
                        }
                    }
                }
                _ => {}
            }
        }
        
        Ok(state)
    }

    fn save(&self, path: &PathBuf) -> Result<()> {
        let file = File::create(path).map_err(crate::Error::Io)?;
        let mut writer = BufWriter::new(file);
        
        writeln!(writer, "next_seed={}", self.next_seed).map_err(crate::Error::Io)?;
        writeln!(writer, "schedules_explored={}", self.schedules_explored).map_err(crate::Error::Io)?;
        
        // Only save recent explored seeds to avoid huge files
        let recent: Vec<_> = self.explored_seeds.iter().take(10000).collect();
        let explored_str: String = recent.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(",");
        writeln!(writer, "explored={}", explored_str).map_err(crate::Error::Io)?;
        
        let bugs_str: String = self.bug_seeds.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(",");
        writeln!(writer, "bugs={}", bugs_str).map_err(crate::Error::Io)?;
        
        writer.flush().map_err(crate::Error::Io)?;
        Ok(())
    }
}

/// Result of exploration.
#[derive(Debug)]
pub struct ExploreResult {
    pub schedules_explored: u64,
    pub bugs_found: Vec<Bug>,
    pub elapsed: Duration,
    pub interrupted: bool,
}

/// Execute the explore command.
pub fn explore_command(args: ExploreArgs) -> Result<ExploreResult> {
    let start = StdInstant::now();
    
    // Load or create exploration state
    let mut state = if let Some(ref checkpoint) = args.checkpoint {
        let checkpoint_file = checkpoint.join("state.chk");
        if checkpoint_file.exists() {
            print_info(&format!("Resuming from checkpoint: {:?}", checkpoint_file));
            ExplorationState::load(&checkpoint_file)?
        } else {
            std::fs::create_dir_all(checkpoint)?;
            ExplorationState::new(args.seed)
        }
    } else {
        ExplorationState::new(args.seed)
    };

    let max_schedules = args.max_schedules.unwrap_or(u64::MAX);
    let bugs = Arc::new(Mutex::new(Vec::new()));
    let explored = Arc::new(AtomicU64::new(state.schedules_explored));
    let stop_flag = Arc::new(AtomicBool::new(false));

    print_header("Chronos Schedule Exploration");
    print_kv("Test:", &args.test_binary);
    print_kv("Threads:", args.threads);
    print_kv("Max depth:", args.depth);
    print_kv("Timeout per schedule:", format!("{}s", args.timeout));
    print_kv("Starting seed:", args.seed);
    if args.pct {
        print_kv("Strategy:", format!("PCT (depth={})", args.pct_depth));
    } else {
        print_kv("Strategy:", "Random");
    }
    println!();

    // Set up signal handler for graceful shutdown
    let stop_flag_clone = stop_flag.clone();
    ctrlc::set_handler(move || {
        print_warning("Interrupted, finishing current schedules...");
        stop_flag_clone.store(true, Ordering::SeqCst);
    }).ok();

    // Create work queue
    let work_queue: Arc<Mutex<VecDeque<u64>>> = Arc::new(Mutex::new(VecDeque::new()));
    
    // Pre-populate work queue
    {
        let mut queue = work_queue.lock().unwrap();
        for i in 0..std::cmp::min(args.threads as u64 * 10, max_schedules) {
            let seed = state.next_seed + i;
            if !state.explored_seeds.contains(&seed) {
                queue.push_back(seed);
            }
        }
        state.next_seed += args.threads as u64 * 10;
    }

    // Spawn worker threads
    let handles: Vec<_> = (0..args.threads).map(|worker_id| {
        let test_binary = args.test_binary.clone();
        let work_queue = work_queue.clone();
        let bugs = bugs.clone();
        let explored = explored.clone();
        let stop_flag = stop_flag.clone();
        let timeout = args.timeout;
        let verbose = args.verbose;
        let max_bugs = args.max_bugs;
        let pct = args.pct;
        let pct_depth = args.pct_depth;
        let cargo = args.cargo;

        thread::spawn(move || {
            loop {
                // Check stop conditions
                if stop_flag.load(Ordering::SeqCst) {
                    break;
                }
                
                let found_bugs = bugs.lock().unwrap().len() as u32;
                if found_bugs >= max_bugs {
                    break;
                }

                // Get next seed
                let seed = {
                    let mut queue = work_queue.lock().unwrap();
                    queue.pop_front()
                };

                let seed = match seed {
                    Some(s) => s,
                    None => break, // No more work
                };

                // Run the test
                let result = run_single_schedule(
                    &test_binary,
                    seed,
                    timeout,
                    pct,
                    pct_depth,
                    cargo,
                );

                explored.fetch_add(1, Ordering::SeqCst);

                match result {
                    Ok(None) => {
                        // No bug found
                        if verbose {
                            eprintln!("  [worker {}] seed {} passed", worker_id, seed);
                        }
                    }
                    Ok(Some(bug)) => {
                        let mut bugs = bugs.lock().unwrap();
                        bugs.push(bug);
                        print_warning(&format!("Bug found! seed={}", seed));
                    }
                    Err(e) => {
                        if verbose {
                            print_error(&format!("Error at seed {}: {}", seed, e));
                        }
                    }
                }

                // Print progress
                let count = explored.load(Ordering::SeqCst);
                let bug_count = bugs.lock().unwrap().len();
                if count % 10 == 0 {
                    print_progress(count, max_schedules, bug_count);
                }
            }
        })
    }).collect();

    // Refill work queue while workers are running
    let work_queue_main = work_queue.clone();
    let explored_main = explored.clone();
    let stop_flag_main = stop_flag.clone();
    
    thread::spawn(move || {
        let mut next_seed = state.next_seed;
        loop {
            if stop_flag_main.load(Ordering::SeqCst) {
                break;
            }
            
            let current = explored_main.load(Ordering::SeqCst);
            if current >= max_schedules {
                break;
            }

            let queue_len = work_queue_main.lock().unwrap().len();
            if queue_len < args.threads * 2 {
                let mut queue = work_queue_main.lock().unwrap();
                for _ in 0..args.threads * 5 {
                    if next_seed < max_schedules + args.seed {
                        queue.push_back(next_seed);
                        next_seed += 1;
                    }
                }
            }

            thread::sleep(Duration::from_millis(100));
        }
    });

    // Wait for workers
    for handle in handles {
        handle.join().ok();
    }

    clear_progress();

    let elapsed = start.elapsed();
    let bugs_found = bugs.lock().unwrap().clone();
    let total_explored = explored.load(Ordering::SeqCst);

    // Save checkpoint if configured
    if let Some(ref checkpoint) = args.checkpoint {
        state.schedules_explored = total_explored;
        for bug in &bugs_found {
            state.bug_seeds.insert(bug.seed);
        }
        let checkpoint_file = checkpoint.join("state.chk");
        state.save(&checkpoint_file)?;
        print_info(&format!("Checkpoint saved to {:?}", checkpoint_file));
    }

    // Write bug report if configured
    if let Some(ref report_path) = args.report {
        write_bug_report(report_path, &args.test_binary, &bugs_found)?;
        print_info(&format!("Bug report written to {:?}", report_path));
    }

    // Print summary
    println!();
    print_header("Exploration Complete");
    print_kv("Schedules explored:", total_explored);
    print_kv("Bugs found:", bugs_found.len());
    print_kv("Time elapsed:", format!("{:.2}s", elapsed.as_secs_f64()));
    print_kv("Rate:", format!("{:.1} schedules/sec", total_explored as f64 / elapsed.as_secs_f64()));
    
    if !bugs_found.is_empty() {
        println!();
        println!("Bugs:");
        for (i, bug) in bugs_found.iter().enumerate() {
            println!("  {}. seed={} - {}", i + 1, bug.seed, bug.description);
            println!("     Replay: chronos run {} --seed {}", args.test_binary, bug.seed);
        }
    }

    Ok(ExploreResult {
        schedules_explored: total_explored,
        bugs_found,
        elapsed,
        interrupted: stop_flag.load(Ordering::SeqCst),
    })
}

/// Run a single schedule and return any bug found.
fn run_single_schedule(
    test_binary: &str,
    seed: u64,
    timeout: u64,
    pct: bool,
    pct_depth: usize,
    cargo: bool,
) -> Result<Option<Bug>> {
    let mut cmd = if cargo {
        let mut c = Command::new("cargo");
        c.arg("test");
        c.arg(test_binary);
        c.arg("--");
        c.arg("--nocapture");
        c
    } else {
        Command::new(test_binary)
    };

    cmd.env("CHRONOS_SEED", seed.to_string());
    cmd.env("CHRONOS_TIMEOUT_SECS", timeout.to_string());
    
    if pct {
        cmd.env("CHRONOS_STRATEGY", "pct");
        cmd.env("CHRONOS_PCT_DEPTH", pct_depth.to_string());
    } else {
        cmd.env("CHRONOS_STRATEGY", "random");
    }

    let output = cmd.output().map_err(crate::Error::Io)?;

    if output.status.success() {
        Ok(None)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        
        // Extract failure description
        let mut description = "Test failed".to_string();
        let mut trace = Vec::new();
        
        for line in stderr.lines().chain(stdout.lines()) {
            if line.contains("assertion") || line.contains("panic") {
                description = line.to_string();
            }
            if line.contains("at ") || line.contains("thread") || line.contains("error") {
                trace.push(line.to_string());
            }
        }

        Ok(Some(Bug {
            seed,
            schedule_id: seed,
            description,
            trace,
        }))
    }
}

/// Write a bug report file.
fn write_bug_report(path: &PathBuf, test_binary: &str, bugs: &[Bug]) -> Result<()> {
    let file = File::create(path).map_err(crate::Error::Io)?;
    let mut writer = BufWriter::new(file);

    writeln!(writer, "# Chronos Bug Report").map_err(crate::Error::Io)?;
    writeln!(writer).map_err(crate::Error::Io)?;
    writeln!(writer, "Test: {}", test_binary).map_err(crate::Error::Io)?;
    writeln!(writer, "Bugs found: {}", bugs.len()).map_err(crate::Error::Io)?;
    writeln!(writer).map_err(crate::Error::Io)?;

    for (i, bug) in bugs.iter().enumerate() {
        writeln!(writer, "## Bug {}", i + 1).map_err(crate::Error::Io)?;
        writeln!(writer).map_err(crate::Error::Io)?;
        writeln!(writer, "- Seed: {}", bug.seed).map_err(crate::Error::Io)?;
        writeln!(writer, "- Description: {}", bug.description).map_err(crate::Error::Io)?;
        writeln!(writer, "- Replay: `chronos run {} --seed {}`", test_binary, bug.seed)
            .map_err(crate::Error::Io)?;
        writeln!(writer).map_err(crate::Error::Io)?;
        
        if !bug.trace.is_empty() {
            writeln!(writer, "### Trace").map_err(crate::Error::Io)?;
            writeln!(writer, "```").map_err(crate::Error::Io)?;
            for line in &bug.trace {
                writeln!(writer, "{}", line).map_err(crate::Error::Io)?;
            }
            writeln!(writer, "```").map_err(crate::Error::Io)?;
        }
        writeln!(writer).map_err(crate::Error::Io)?;
    }

    writer.flush().map_err(crate::Error::Io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exploration_state_new() {
        let state = ExplorationState::new(42);
        assert_eq!(state.next_seed, 42);
        assert!(state.explored_seeds.is_empty());
        assert!(state.bug_seeds.is_empty());
    }

    #[test]
    fn test_exploration_state_save_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.chk");
        
        let mut state = ExplorationState::new(100);
        state.explored_seeds.insert(1);
        state.explored_seeds.insert(2);
        state.bug_seeds.insert(5);
        state.schedules_explored = 50;
        
        state.save(&path).unwrap();
        
        let loaded = ExplorationState::load(&path).unwrap();
        assert_eq!(loaded.next_seed, 100);
        assert_eq!(loaded.schedules_explored, 50);
        assert!(loaded.explored_seeds.contains(&1));
        assert!(loaded.bug_seeds.contains(&5));
    }

    #[test]
    fn test_bug_structure() {
        let bug = Bug {
            seed: 42,
            schedule_id: 42,
            description: "assertion failed".to_string(),
            trace: vec!["line 1".to_string(), "line 2".to_string()],
        };
        
        assert_eq!(bug.seed, 42);
        assert_eq!(bug.description, "assertion failed");
        assert_eq!(bug.trace.len(), 2);
    }
}
