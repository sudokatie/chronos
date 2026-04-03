use std::path::PathBuf;

use clap::Args;

use crate::Result;

/// Scheduling strategy options.
#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
pub enum Strategy {
    /// First-in-first-out scheduling.
    Fifo,
    /// Random scheduling with seed.
    #[default]
    Random,
    /// Probabilistic Concurrency Testing.
    Pct,
}

/// Arguments for the run command.
#[derive(Args, Debug)]
pub struct RunArgs {
    /// Test binary to run.
    pub test_binary: String,

    /// Random seed for reproducibility.
    #[arg(long, short = 's')]
    pub seed: Option<u64>,

    /// Scheduling strategy.
    #[arg(long, default_value = "random")]
    pub strategy: Strategy,

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
}

/// Configuration for a simulation run.
#[derive(Debug, Clone)]
pub struct RunConfig {
    pub test_binary: String,
    pub seed: u64,
    pub strategy: Strategy,
    pub iterations: u32,
    pub timeout_secs: Option<u64>,
    pub record_path: Option<PathBuf>,
    pub replay_path: Option<PathBuf>,
    pub verbose: bool,
}

impl From<RunArgs> for RunConfig {
    fn from(args: RunArgs) -> Self {
        Self {
            seed: args.effective_seed(),
            test_binary: args.test_binary,
            strategy: args.strategy,
            iterations: args.iterations,
            timeout_secs: args.timeout,
            record_path: args.record,
            replay_path: args.replay,
            verbose: args.verbose,
        }
    }
}

/// Result of a simulation run.
#[derive(Debug)]
pub struct RunResult {
    pub iterations_run: u32,
    pub bugs_found: u32,
    pub seed_used: u64,
}

/// Execute the run command.
pub fn run_command(args: RunArgs) -> Result<RunResult> {
    let config = RunConfig::from(args);
    
    if config.verbose {
        eprintln!("Running {} with seed {}", config.test_binary, config.seed);
        eprintln!("Strategy: {:?}", config.strategy);
        eprintln!("Iterations: {}", config.iterations);
    }

    // TODO: Actually execute the test binary
    // For now, return a placeholder result
    Ok(RunResult {
        iterations_run: config.iterations,
        bugs_found: 0,
        seed_used: config.seed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::parse_from;

    #[test]
    fn test_run_args_defaults() {
        let cli = parse_from(["chronos", "run", "my_test"]);
        if let crate::cli::Commands::Run(args) = cli.command {
            assert_eq!(args.test_binary, "my_test");
            assert!(args.seed.is_none());
            assert_eq!(args.iterations, 1);
            assert!(!args.verbose);
        }
    }

    #[test]
    fn test_run_args_seed() {
        let cli = parse_from(["chronos", "run", "test", "--seed", "12345"]);
        if let crate::cli::Commands::Run(args) = cli.command {
            assert_eq!(args.seed, Some(12345));
        }
    }

    #[test]
    fn test_run_args_strategy() {
        let cli = parse_from(["chronos", "run", "test", "--strategy", "pct"]);
        if let crate::cli::Commands::Run(args) = cli.command {
            assert!(matches!(args.strategy, Strategy::Pct));
        }
    }

    #[test]
    fn test_run_args_iterations() {
        let cli = parse_from(["chronos", "run", "test", "-n", "100"]);
        if let crate::cli::Commands::Run(args) = cli.command {
            assert_eq!(args.iterations, 100);
        }
    }

    #[test]
    fn test_run_args_record() {
        let cli = parse_from(["chronos", "run", "test", "--record", "out.chrn"]);
        if let crate::cli::Commands::Run(args) = cli.command {
            assert_eq!(args.record, Some(PathBuf::from("out.chrn")));
        }
    }

    #[test]
    fn test_run_args_verbose() {
        let cli = parse_from(["chronos", "run", "test", "-v"]);
        if let crate::cli::Commands::Run(args) = cli.command {
            assert!(args.verbose);
        }
    }

    #[test]
    fn test_effective_seed_provided() {
        let cli = parse_from(["chronos", "run", "test", "--seed", "999"]);
        if let crate::cli::Commands::Run(args) = cli.command {
            assert_eq!(args.effective_seed(), 999);
        }
    }

    #[test]
    fn test_effective_seed_random() {
        let cli = parse_from(["chronos", "run", "test"]);
        if let crate::cli::Commands::Run(args) = cli.command {
            let seed = args.effective_seed();
            assert!(seed > 0); // Should be non-zero
        }
    }

    #[test]
    fn test_run_config_from_args() {
        let cli = parse_from(["chronos", "run", "test", "--seed", "42", "-n", "10"]);
        if let crate::cli::Commands::Run(args) = cli.command {
            let config = RunConfig::from(args);
            assert_eq!(config.seed, 42);
            assert_eq!(config.iterations, 10);
        }
    }

    #[test]
    fn test_run_command_basic() {
        let cli = parse_from(["chronos", "run", "test", "--seed", "1"]);
        if let crate::cli::Commands::Run(args) = cli.command {
            let result = run_command(args).unwrap();
            assert_eq!(result.seed_used, 1);
            assert_eq!(result.bugs_found, 0);
        }
    }
}
