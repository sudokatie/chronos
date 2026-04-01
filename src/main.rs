//! Chronos CLI - Deterministic simulation testing.

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "chronos")]
#[command(about = "Deterministic simulation testing for distributed systems")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a simulation with the specified configuration
    Run {
        /// Path to the configuration file
        #[arg(short, long, default_value = "chronos.toml")]
        config: String,

        /// Random seed for the simulation
        #[arg(short, long)]
        seed: Option<u64>,

        /// Record the execution for replay
        #[arg(short, long)]
        record: Option<String>,
    },

    /// Replay a recorded execution
    Replay {
        /// Path to the recording file
        recording: String,
    },

    /// Explore the state space with random schedules
    Explore {
        /// Path to the configuration file
        #[arg(short, long, default_value = "chronos.toml")]
        config: String,

        /// Number of iterations to run
        #[arg(short, long, default_value = "100")]
        iterations: u32,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Run { config, seed, record } => {
            println!("Running simulation with config: {config}");
            if let Some(s) = seed {
                println!("  seed: {s}");
            }
            if let Some(r) = record {
                println!("  recording to: {r}");
            }
            // TODO: implement run
        }
        Commands::Replay { recording } => {
            println!("Replaying: {recording}");
            // TODO: implement replay
        }
        Commands::Explore { config, iterations } => {
            println!("Exploring with config: {config}, iterations: {iterations}");
            // TODO: implement explore
        }
    }
}
