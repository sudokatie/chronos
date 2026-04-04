//! CLI commands for Chronos.

mod analyze;
mod explore;
mod inject;
mod output;
mod replay;
mod run;

pub use analyze::{analyze_command, AnalyzeArgs, AnalysisResult};
pub use explore::{explore_command, ExploreArgs, ExploreResult};
pub use inject::{inject_command, FaultSpec, InjectArgs, InjectResult};
pub use output::{print_success, print_failure, print_header, print_kv, print_info, print_warning, print_error, TraceEntry};
pub use replay::{replay_command, ReplayArgs, ReplayExecutor, ReplayResult};
pub use run::{run_command, RunArgs, RunResult};

use clap::{Parser, Subcommand};

/// Chronos - Deterministic simulation testing for distributed systems.
#[derive(Parser, Debug)]
#[command(name = "chronos")]
#[command(version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

/// Available commands.
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Run a test under simulation.
    Run(RunArgs),
    /// Systematic schedule exploration.
    Explore(ExploreArgs),
    /// Configure fault injection.
    Inject(InjectArgs),
    /// Analyze recorded executions.
    Analyze(AnalyzeArgs),
    /// Replay a recorded execution.
    Replay(ReplayArgs),
}

/// Parse CLI arguments.
pub fn parse() -> Cli {
    Cli::parse()
}

/// Parse CLI from a vector of strings (for testing).
pub fn parse_from<I, T>(args: I) -> Cli
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    Cli::parse_from(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_run() {
        let cli = parse_from(["chronos", "run", "test_binary"]);
        match cli.command {
            Commands::Run(args) => {
                assert_eq!(args.test_binary, "test_binary");
            }
            _ => panic!("expected Run command"),
        }
    }

    #[test]
    fn test_parse_explore() {
        let cli = parse_from(["chronos", "explore", "test_binary"]);
        match cli.command {
            Commands::Explore(args) => {
                assert_eq!(args.test_binary, "test_binary");
            }
            _ => panic!("expected Explore command"),
        }
    }

    #[test]
    fn test_parse_inject() {
        let cli = parse_from([
            "chronos", "inject", "test_binary",
            "-f", "network:drop:10%",
        ]);
        match cli.command {
            Commands::Inject(args) => {
                assert_eq!(args.test_binary, "test_binary");
                assert_eq!(args.fault.len(), 1);
            }
            _ => panic!("expected Inject command"),
        }
    }

    #[test]
    fn test_parse_analyze() {
        let cli = parse_from(["chronos", "analyze", "recording.chrn"]);
        match cli.command {
            Commands::Analyze(args) => {
                assert_eq!(args.recording.to_str().unwrap(), "recording.chrn");
            }
            _ => panic!("expected Analyze command"),
        }
    }

    #[test]
    fn test_parse_replay() {
        let cli = parse_from(["chronos", "replay", "recording.chrn"]);
        match cli.command {
            Commands::Replay(args) => {
                assert_eq!(args.recording.to_str().unwrap(), "recording.chrn");
            }
            _ => panic!("expected Replay command"),
        }
    }

    #[test]
    fn test_parse_inject_multiple_faults() {
        let cli = parse_from([
            "chronos", "inject", "test",
            "-f", "network:drop:10%",
            "-f", "network:delay:10ms-50ms",
            "-f", "crash:1:after:100ms",
        ]);
        match cli.command {
            Commands::Inject(args) => {
                assert_eq!(args.fault.len(), 3);
            }
            _ => panic!("expected Inject command"),
        }
    }
}
