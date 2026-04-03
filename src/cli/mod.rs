mod run;
mod explore;

pub use run::{RunArgs, run_command};
pub use explore::{ExploreArgs, explore_command};

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
}
