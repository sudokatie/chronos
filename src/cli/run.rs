//! Run command for executing simulation tests.

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant as StdInstant};

use clap::Args;

use crate::scheduler::Strategy;
use crate::Result;

use super::output::{print_success, print_failure, print_info, print_error, TraceEntry};

/// Scheduling strategy options.
#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
pub enum StrategyArg {
    /// First-in-first-out scheduling.
    Fifo,
    /// Random scheduling with seed.
    #[default]
    Random,
    /// Probabilistic Concurrency Testing.
    Pct,
    /// Depth-first search.
    Dfs,
    /// Context-bounded scheduling.
    ContextBound,
}

impl StrategyArg {
    /// Convert to a Strategy with the given seed.
    pub fn to_strategy(self, seed: u64) -> Strategy {
        match self {
            StrategyArg::Fifo => Strategy::Fifo,
            StrategyArg::Random => Strategy::Random { seed },
            StrategyArg::Pct => Strategy::PCT { seed, bug_depth: 3 },
            StrategyArg::Dfs => Strategy::DepthFirst { max_depth: 100 },
            StrategyArg::ContextBound => Strategy::ContextBound { max_preemptions: 3, seed },
        }
    }

    /// Get the name of the strategy.
    pub fn name(&self) -> &'static str {
        match self {
            StrategyArg::Fifo => "FIFO",
            StrategyArg::Random => "Random",
            StrategyArg::Pct => "PCT",
            StrategyArg::Dfs => "DFS",
            StrategyArg::ContextBound => "ContextBound",
        }
    }
}

/// Arguments for the run command.
#[derive(Args, Debug)]
pub struct RunArgs {
    /// Test binary or test name to run.
    pub test_binary: String,

    /// Random seed for reproducibility.
    #[arg(long, short = 's')]
    pub seed: Option<u64>,

    /// Scheduling strategy.
    #[arg(long, default_value = "random")]
    pub strategy: StrategyArg,

    /// Number of iterations.
    #[arg(long, short = 'n', default_value = "1")]
    pub iterations: u32,

    /// Max simulated time (seconds) before timeout.
    #[arg(long, short = 't')]
    pub timeout: Option<u64>,

    /// Record execution to file.
    #[arg(long, short = 'r')]
    pub record: Option<PathBuf>,

    /// Replay from recorded file.
    #[arg(long)]
    pub replay: Option<PathBuf>,

    /// Show scheduling decisions.
    #[arg(long, short = 'v')]
    pub verbose: bool,

    /// PCT bug depth parameter.
    #[arg(long, default_value = "3")]
    pub pct_depth: usize,

    /// Max preemptions for context-bound.
    #[arg(long, default_value = "3")]
    pub max_preemptions: usize,

    /// DFS max depth.
    #[arg(long, default_value = "100")]
    pub dfs_depth: usize,

    /// Run as cargo test (prepends "cargo test --").
    #[arg(long)]
    pub cargo: bool,

    /// Additional arguments to pass to the test binary.
    #[arg(last = true)]
    pub args: Vec<String>,
}

impl RunArgs {
    /// Get the effective seed (provided or random).
    pub fn effective_seed(&self) -> u64 {
        self.seed.unwrap_or_else(|| {
            use std::time::{SystemTime, UNIX_EPOCH};
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(42)
        })
    }

    /// Get the strategy with custom parameters.
    pub fn to_strategy(&self, seed: u64) -> Strategy {
        match self.strategy {
            StrategyArg::Fifo => Strategy::Fifo,
            StrategyArg::Random => Strategy::Random { seed },
            StrategyArg::Pct => Strategy::PCT { seed, bug_depth: self.pct_depth },
            StrategyArg::Dfs => Strategy::DepthFirst { max_depth: self.dfs_depth },
            StrategyArg::ContextBound => Strategy::ContextBound { 
                max_preemptions: self.max_preemptions, 
                seed 
            },
        }
    }
}

/// Configuration for a simulation run.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct RunConfig {
    pub test_binary: String,
    pub seed: u64,
    pub strategy: Strategy,
    pub strategy_name: String,
    pub iterations: u32,
    pub timeout: Duration,
    pub record_path: Option<PathBuf>,
    pub replay_path: Option<PathBuf>,
    pub verbose: bool,
    pub cargo: bool,
    pub extra_args: Vec<String>,
}

impl From<&RunArgs> for RunConfig {
    fn from(args: &RunArgs) -> Self {
        let seed = args.effective_seed();
        Self {
            seed,
            test_binary: args.test_binary.clone(),
            strategy: args.to_strategy(seed),
            strategy_name: args.strategy.name().to_string(),
            iterations: args.iterations,
            timeout: Duration::from_secs(args.timeout.unwrap_or(60)),
            record_path: args.record.clone(),
            replay_path: args.replay.clone(),
            verbose: args.verbose,
            cargo: args.cargo,
            extra_args: args.args.clone(),
        }
    }
}

/// Result of a simulation run.
#[derive(Debug)]
pub struct RunResult {
    pub iterations_run: u32,
    pub bugs_found: u32,
    pub seed_used: u64,
    pub schedules_explored: u32,
    pub simulated_time: Duration,
    pub real_time: Duration,
    pub failure_trace: Option<Vec<TraceEntry>>,
    pub failure_reason: Option<String>,
    pub exit_code: i32,
}

/// Execute the run command.
pub fn run_command(args: RunArgs) -> Result<RunResult> {
    let config = RunConfig::from(&args);
    
    if config.verbose {
        print_info(&format!("Running {} with seed {}", config.test_binary, config.seed));
        print_info(&format!("Strategy: {}", config.strategy_name));
        print_info(&format!("Iterations: {}", config.iterations));
    }

    // Check if we should run via cargo test or directly
    if config.cargo {
        return run_cargo_test(&config);
    }

    // Check if the binary exists
    let binary_path = PathBuf::from(&config.test_binary);
    if !binary_path.exists() && !config.test_binary.starts_with("cargo") {
        // Try running as cargo test
        if config.verbose {
            print_info("Binary not found, trying cargo test...");
        }
        return run_cargo_test(&config);
    }

    // Run the test binary with chronos environment variables
    run_binary(&config)
}

/// Run a test binary directly.
fn run_binary(config: &RunConfig) -> Result<RunResult> {
    let real_start = StdInstant::now();
    let total_simulated_nanos: u64 = 0;
    let mut bugs_found = 0;
    let mut failure_trace = None;
    let mut failure_reason = None;
    let mut schedules_explored = 0;
    let mut last_exit_code = 0;

    for iteration in 0..config.iterations {
        let iteration_seed = config.seed.wrapping_add(iteration as u64);
        
        // Build command
        let mut cmd = Command::new(&config.test_binary);
        
        // Set environment variables for the test
        cmd.env("CHRONOS_SEED", iteration_seed.to_string());
        cmd.env("CHRONOS_STRATEGY", &config.strategy_name);
        cmd.env("CHRONOS_ITERATION", iteration.to_string());
        cmd.env("CHRONOS_TIMEOUT_SECS", config.timeout.as_secs().to_string());
        
        if let Some(ref path) = config.record_path {
            cmd.env("CHRONOS_RECORD", format!("{}_{}.chrn", path.display(), iteration));
        }

        // Add extra args
        cmd.args(&config.extra_args);

        if config.verbose {
            print_info(&format!("Iteration {} (seed={})", iteration + 1, iteration_seed));
        }

        // Run the test
        let output = cmd.output();

        match output {
            Ok(output) => {
                last_exit_code = output.status.code().unwrap_or(-1);
                schedules_explored += 1;

                if !output.status.success() {
                    bugs_found += 1;
                    
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    
                    failure_reason = Some(format!(
                        "Test failed with exit code {}",
                        last_exit_code
                    ));
                    
                    // Extract trace from output
                    let mut trace = Vec::new();
                    for line in stderr.lines().chain(stdout.lines()) {
                        if line.contains("assertion") || line.contains("panic") || line.contains("FAIL") {
                            trace.push(TraceEntry::new(0, line.to_string()));
                        }
                    }
                    if trace.is_empty() {
                        trace.push(TraceEntry::new(0, format!("Exit code: {}", last_exit_code)));
                    }
                    failure_trace = Some(trace);

                    if config.verbose {
                        print_error(&format!("Bug found at iteration {} (seed={})", iteration + 1, iteration_seed));
                        if !stderr.is_empty() {
                            eprintln!("{}", stderr);
                        }
                    }

                    // Stop on first bug
                    break;
                } else if config.verbose {
                    print_info(&format!("Iteration {} passed", iteration + 1));
                }
            }
            Err(e) => {
                failure_reason = Some(format!("Failed to execute test: {}", e));
                failure_trace = Some(vec![TraceEntry::new(0, e.to_string())]);
                bugs_found += 1;
                break;
            }
        }
    }

    let real_time = real_start.elapsed();
    let simulated_time = Duration::from_nanos(total_simulated_nanos);

    // Print formatted output
    if bugs_found == 0 {
        print_success(
            &config.test_binary,
            &config.strategy_name,
            config.seed,
            config.iterations,
            schedules_explored,
            simulated_time,
            real_time,
        );
    } else if let (Some(ref trace), Some(ref reason)) = (&failure_trace, &failure_reason) {
        print_failure(
            &config.test_binary,
            &config.strategy_name,
            config.seed,
            schedules_explored,
            reason,
            trace,
            &format!("chronos run {} --seed {} --replay recording_{}.chrn", 
                config.test_binary, config.seed, config.seed),
        );
    }

    Ok(RunResult {
        iterations_run: config.iterations,
        bugs_found,
        seed_used: config.seed,
        schedules_explored,
        simulated_time,
        real_time,
        failure_trace,
        failure_reason,
        exit_code: last_exit_code,
    })
}

/// Run via cargo test.
fn run_cargo_test(config: &RunConfig) -> Result<RunResult> {
    let real_start = StdInstant::now();
    let mut bugs_found = 0;
    let mut failure_trace = None;
    let mut failure_reason = None;
    let mut schedules_explored = 0;
    let mut last_exit_code = 0;

    for iteration in 0..config.iterations {
        let iteration_seed = config.seed.wrapping_add(iteration as u64);

        let mut cmd = Command::new("cargo");
        cmd.arg("test");
        cmd.arg(&config.test_binary);
        cmd.arg("--");
        cmd.arg("--nocapture");

        // Set chronos environment
        cmd.env("CHRONOS_SEED", iteration_seed.to_string());
        cmd.env("CHRONOS_STRATEGY", &config.strategy_name);
        cmd.env("CHRONOS_ITERATION", iteration.to_string());

        if let Some(ref path) = config.record_path {
            cmd.env("CHRONOS_RECORD", format!("{}_{}.chrn", path.display(), iteration));
        }

        cmd.args(&config.extra_args);

        if config.verbose {
            print_info(&format!("cargo test {} (seed={})", config.test_binary, iteration_seed));
        }

        let output = cmd.output();

        match output {
            Ok(output) => {
                last_exit_code = output.status.code().unwrap_or(-1);
                schedules_explored += 1;

                if !output.status.success() {
                    bugs_found += 1;
                    
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    
                    failure_reason = Some("cargo test failed".to_string());
                    
                    let mut trace = Vec::new();
                    for line in stderr.lines().chain(stdout.lines()) {
                        if line.contains("assertion") || line.contains("panic") 
                            || line.contains("FAILED") || line.contains("error[") {
                            trace.push(TraceEntry::new(0, line.to_string()));
                        }
                    }
                    if trace.is_empty() {
                        trace.push(TraceEntry::new(0, "Test failed".to_string()));
                    }
                    failure_trace = Some(trace);

                    break;
                }
            }
            Err(e) => {
                failure_reason = Some(format!("Failed to run cargo test: {}", e));
                failure_trace = Some(vec![TraceEntry::new(0, e.to_string())]);
                bugs_found += 1;
                break;
            }
        }
    }

    let real_time = real_start.elapsed();

    if bugs_found == 0 {
        print_success(
            &config.test_binary,
            &config.strategy_name,
            config.seed,
            config.iterations,
            schedules_explored,
            Duration::ZERO,
            real_time,
        );
    } else if let (Some(ref trace), Some(ref reason)) = (&failure_trace, &failure_reason) {
        print_failure(
            &config.test_binary,
            &config.strategy_name,
            config.seed,
            schedules_explored,
            reason,
            trace,
            &format!("chronos run {} --seed {} --cargo", config.test_binary, config.seed),
        );
    }

    Ok(RunResult {
        iterations_run: config.iterations,
        bugs_found,
        seed_used: config.seed,
        schedules_explored,
        simulated_time: Duration::ZERO,
        real_time,
        failure_trace,
        failure_reason,
        exit_code: last_exit_code,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strategy_conversion() {
        let seed = 42u64;
        assert!(matches!(StrategyArg::Fifo.to_strategy(seed), Strategy::Fifo));
        assert!(matches!(StrategyArg::Random.to_strategy(seed), Strategy::Random { .. }));
        assert!(matches!(StrategyArg::Pct.to_strategy(seed), Strategy::PCT { .. }));
        assert!(matches!(StrategyArg::Dfs.to_strategy(seed), Strategy::DepthFirst { .. }));
        assert!(matches!(StrategyArg::ContextBound.to_strategy(seed), Strategy::ContextBound { .. }));
    }

    #[test]
    fn test_strategy_names() {
        assert_eq!(StrategyArg::Fifo.name(), "FIFO");
        assert_eq!(StrategyArg::Random.name(), "Random");
        assert_eq!(StrategyArg::Pct.name(), "PCT");
        assert_eq!(StrategyArg::Dfs.name(), "DFS");
        assert_eq!(StrategyArg::ContextBound.name(), "ContextBound");
    }

    #[test]
    fn test_effective_seed_provided() {
        let args = RunArgs {
            test_binary: "test".to_string(),
            seed: Some(999),
            strategy: StrategyArg::Random,
            iterations: 1,
            timeout: None,
            record: None,
            replay: None,
            verbose: false,
            pct_depth: 3,
            max_preemptions: 3,
            dfs_depth: 100,
            cargo: false,
            args: vec![],
        };
        assert_eq!(args.effective_seed(), 999);
    }

    #[test]
    fn test_effective_seed_random() {
        let args = RunArgs {
            test_binary: "test".to_string(),
            seed: None,
            strategy: StrategyArg::Random,
            iterations: 1,
            timeout: None,
            record: None,
            replay: None,
            verbose: false,
            pct_depth: 3,
            max_preemptions: 3,
            dfs_depth: 100,
            cargo: false,
            args: vec![],
        };
        let seed = args.effective_seed();
        assert!(seed > 0);
    }

    #[test]
    fn test_run_config_from_args() {
        let args = RunArgs {
            test_binary: "my_test".to_string(),
            seed: Some(12345),
            strategy: StrategyArg::Pct,
            iterations: 100,
            timeout: Some(30),
            record: Some(PathBuf::from("out.chrn")),
            replay: None,
            verbose: true,
            pct_depth: 5,
            max_preemptions: 3,
            dfs_depth: 100,
            cargo: false,
            args: vec!["--extra".to_string()],
        };

        let config = RunConfig::from(&args);
        
        assert_eq!(config.test_binary, "my_test");
        assert_eq!(config.seed, 12345);
        assert_eq!(config.strategy_name, "PCT");
        assert_eq!(config.iterations, 100);
        assert_eq!(config.timeout, Duration::from_secs(30));
        assert!(config.verbose);
        assert_eq!(config.extra_args, vec!["--extra"]);
    }
}
