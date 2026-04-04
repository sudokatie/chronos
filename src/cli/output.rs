//! Formatted output for CLI commands matching the spec.

use std::io::{self, Write};
use std::time::Duration;

/// ANSI color codes for terminal output.
#[allow(dead_code)]
pub mod colors {
    pub const RESET: &str = "\x1b[0m";
    pub const BOLD: &str = "\x1b[1m";
    pub const DIM: &str = "\x1b[2m";
    pub const GREEN: &str = "\x1b[32m";
    pub const RED: &str = "\x1b[31m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const BLUE: &str = "\x1b[34m";
    pub const CYAN: &str = "\x1b[36m";
}

/// Check if colors should be used (terminal detection).
pub fn use_colors() -> bool {
    std::env::var("NO_COLOR").is_err() && atty_check()
}

fn atty_check() -> bool {
    // Simple check - could use atty crate for better detection
    std::env::var("TERM").is_ok()
}

/// Format a duration for display.
pub fn format_duration(d: Duration) -> String {
    let nanos = d.as_nanos();
    if nanos >= 1_000_000_000 {
        format!("{:.2}s", d.as_secs_f64())
    } else if nanos >= 1_000_000 {
        format!("{:.2}ms", nanos as f64 / 1_000_000.0)
    } else if nanos >= 1_000 {
        format!("{:.2}μs", nanos as f64 / 1_000.0)
    } else {
        format!("{}ns", nanos)
    }
}

/// Format simulated time from nanoseconds.
pub fn format_sim_time(nanos: u64) -> String {
    if nanos >= 1_000_000_000 {
        format!("{:.2}s", nanos as f64 / 1_000_000_000.0)
    } else if nanos >= 1_000_000 {
        format!("{:.2}ms", nanos as f64 / 1_000_000.0)
    } else if nanos >= 1_000 {
        format!("{:.2}μs", nanos as f64 / 1_000.0)
    } else {
        format!("{}ns", nanos)
    }
}

/// Print a success result matching the spec format.
pub fn print_success(
    test_name: &str,
    strategy: &str,
    seed: u64,
    iterations: u32,
    schedules_explored: u32,
    simulated_time: Duration,
    real_time: Duration,
) {
    let colors = use_colors();
    
    let (green, bold, reset, dim) = if colors {
        (colors::GREEN, colors::BOLD, colors::RESET, colors::DIM)
    } else {
        ("", "", "", "")
    };

    println!("{}chronos:{} {}", bold, reset, test_name);
    println!("  strategy: {} {}(seed={}){}", strategy, dim, seed, reset);
    println!("  iterations: {}", iterations);
    println!("  schedules explored: {}", schedules_explored);
    println!("  simulated time: {} total", format_duration(simulated_time));
    println!("  real time: {}", format_duration(real_time));
    println!("  result: {}{}PASS{} (no bugs found)", bold, green, reset);
}

/// Print a failure result matching the spec format.
pub fn print_failure(
    test_name: &str,
    strategy: &str,
    seed: u64,
    iteration: u32,
    failure_reason: &str,
    trace: &[TraceEntry],
    replay_command: &str,
) {
    let colors = use_colors();
    
    let (red, bold, reset, dim, yellow) = if colors {
        (colors::RED, colors::BOLD, colors::RESET, colors::DIM, colors::YELLOW)
    } else {
        ("", "", "", "", "")
    };

    println!("{}chronos:{} {}", bold, reset, test_name);
    println!("  strategy: {} {}(seed={}){}", strategy, dim, seed, reset);
    println!("  iterations: {}", iteration);
    println!("  result: {}{}FAIL{} ({})", bold, red, reset, failure_reason);
    println!();
    println!("  {}Bug found at iteration {}:{}", yellow, iteration, reset);
    println!("    seed: {}, schedule_id: {}", seed, iteration);
    println!();
    println!("  Minimal trace:");
    
    for entry in trace {
        print!("    {}[{}]{} ", dim, format_sim_time(entry.time_ns), reset);
        
        if let Some(ref _fault) = entry.fault {
            print!("{}FAULT:{} ", yellow, reset);
        }
        
        if let Some(ref node) = entry.node {
            print!("{}: ", node);
        }
        
        println!("{}", entry.description);
    }
    
    println!();
    println!("  Replay: {}", replay_command);
}

/// A trace entry for failure output.
#[derive(Debug, Clone)]
pub struct TraceEntry {
    pub time_ns: u64,
    pub node: Option<String>,
    pub description: String,
    pub fault: Option<String>,
}

impl TraceEntry {
    pub fn new(time_ns: u64, description: impl Into<String>) -> Self {
        Self {
            time_ns,
            node: None,
            description: description.into(),
            fault: None,
        }
    }

    pub fn with_node(mut self, node: impl Into<String>) -> Self {
        self.node = Some(node.into());
        self
    }

    pub fn with_fault(mut self, fault: impl Into<String>) -> Self {
        self.fault = Some(fault.into());
        self
    }
}

/// Print exploration progress.
pub fn print_progress(current: u64, total: u64, bugs_found: usize) {
    let colors = use_colors();
    let (dim, reset) = if colors {
        (colors::DIM, colors::RESET)
    } else {
        ("", "")
    };

    let pct = if total > 0 {
        (current as f64 / total as f64 * 100.0) as u32
    } else {
        0
    };

    eprint!("\r{}[{}/{}]{} {}% explored, {} bugs found", 
        dim, current, total, reset, pct, bugs_found);
    let _ = io::stderr().flush();
}

/// Clear progress line.
pub fn clear_progress() {
    eprint!("\r\x1b[K");
    let _ = io::stderr().flush();
}

/// Print a section header.
pub fn print_header(title: &str) {
    let colors = use_colors();
    let (bold, reset) = if colors {
        (colors::BOLD, colors::RESET)
    } else {
        ("", "")
    };
    
    println!();
    println!("{}=== {} ==={}", bold, title, reset);
}

/// Print a key-value pair.
pub fn print_kv(key: &str, value: impl std::fmt::Display) {
    let colors = use_colors();
    let (dim, reset) = if colors {
        (colors::DIM, colors::RESET)
    } else {
        ("", "")
    };
    
    println!("  {}{:20}{} {}", dim, key, reset, value);
}

/// Print an info message.
pub fn print_info(msg: &str) {
    let colors = use_colors();
    let (blue, reset) = if colors {
        (colors::BLUE, colors::RESET)
    } else {
        ("", "")
    };
    
    eprintln!("{}info:{} {}", blue, reset, msg);
}

/// Print a warning message.
pub fn print_warning(msg: &str) {
    let colors = use_colors();
    let (yellow, reset) = if colors {
        (colors::YELLOW, colors::RESET)
    } else {
        ("", "")
    };
    
    eprintln!("{}warn:{} {}", yellow, reset, msg);
}

/// Print an error message.
pub fn print_error(msg: &str) {
    let colors = use_colors();
    let (red, reset) = if colors {
        (colors::RED, colors::RESET)
    } else {
        ("", "")
    };
    
    eprintln!("{}error:{} {}", red, reset, msg);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration() {
        assert!(format_duration(Duration::from_nanos(500)).contains("ns"));
        assert!(format_duration(Duration::from_micros(500)).contains("μs"));
        assert!(format_duration(Duration::from_millis(500)).contains("ms"));
        assert!(format_duration(Duration::from_secs(5)).contains("s"));
    }

    #[test]
    fn test_format_sim_time() {
        assert!(format_sim_time(500).contains("ns"));
        assert!(format_sim_time(500_000).contains("μs"));
        assert!(format_sim_time(500_000_000).contains("ms"));
        assert!(format_sim_time(5_000_000_000).contains("s"));
    }

    #[test]
    fn test_trace_entry() {
        let entry = TraceEntry::new(1000, "test event")
            .with_node("node0")
            .with_fault("partition");
        
        assert_eq!(entry.time_ns, 1000);
        assert_eq!(entry.node, Some("node0".to_string()));
        assert_eq!(entry.fault, Some("partition".to_string()));
    }
}
