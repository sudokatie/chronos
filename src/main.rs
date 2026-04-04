//! Chronos CLI - Deterministic simulation testing.

use chronos::cli::{
    self, analyze_command, explore_command, inject_command, replay_command, run_command, Commands,
};

fn main() {
    let cli = cli::parse();

    let result = match cli.command {
        Commands::Run(args) => {
            match run_command(args) {
                Ok(result) => {
                    println!();
                    println!("=== Run Result ===");
                    println!("Iterations: {}", result.iterations_run);
                    println!("Bugs found: {}", result.bugs_found);
                    println!("Seed: {}", result.seed_used);
                    
                    if result.bugs_found > 0 {
                        std::process::exit(1);
                    }
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }
        Commands::Explore(args) => {
            match explore_command(args) {
                Ok(result) => {
                    // explore_command already prints detailed output
                    if !result.bugs_found.is_empty() {
                        std::process::exit(1);
                    }
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }
        Commands::Inject(args) => {
            match inject_command(args) {
                Ok(result) => {
                    println!();
                    println!("=== Injection Result ===");
                    println!("Faults applied: {}", result.faults_applied.len());
                    println!("Bugs found: {}", result.bugs_found);
                    println!("Seed: {}", result.seed);
                    
                    if result.bugs_found > 0 {
                        std::process::exit(1);
                    }
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }
        Commands::Analyze(args) => {
            match analyze_command(args) {
                Ok(_result) => {
                    // analyze_command already prints output
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }
        Commands::Replay(args) => {
            match replay_command(args) {
                Ok(_result) => {
                    // replay_command already prints output
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
