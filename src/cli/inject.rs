//! Fault injection command for testing failure scenarios.

use std::path::PathBuf;
use std::time::Duration;

use clap::Args;
use serde_json;

use crate::network::Fault;
use crate::Result;

/// Arguments for the inject command.
#[derive(Args, Debug)]
pub struct InjectArgs {
    /// Test binary to run with fault injection.
    pub test_binary: String,

    /// Fault specification (can be repeated).
    /// 
    /// Formats:
    ///   network:partition:node1,node2:5s
    ///   network:drop:10%
    ///   network:delay:50ms-200ms
    ///   disk:error:read:5%
    ///   crash:node1:after:100ms
    #[arg(long, short = 'f', value_name = "SPEC")]
    pub fault: Vec<String>,

    /// Random seed for reproducibility.
    #[arg(long, short = 's')]
    pub seed: Option<u64>,

    /// Verbose output.
    #[arg(long, short = 'v')]
    pub verbose: bool,

    /// Record execution to file.
    #[arg(long, short = 'r')]
    pub record: Option<PathBuf>,
}

/// Parsed fault specification.
#[derive(Debug, Clone)]
pub enum FaultSpec {
    /// Network partition between groups.
    Partition {
        groups: Vec<Vec<u32>>,
        duration: Option<Duration>,
    },
    /// Drop packets with given probability.
    Drop { rate: f64 },
    /// Add delay to packets.
    Delay { min: Duration, max: Duration },
    /// Duplicate packets with given probability.
    Duplicate { rate: f64 },
    /// Disk read error rate.
    DiskReadError { rate: f64 },
    /// Disk write error rate.
    DiskWriteError { rate: f64 },
    /// Crash a node after delay.
    Crash { node: u32, after: Duration },
    /// Heal all faults.
    Heal,
}

impl FaultSpec {
    /// Parse a fault specification string.
    pub fn parse(spec: &str) -> Result<Self> {
        let parts: Vec<&str> = spec.split(':').collect();
        
        if parts.is_empty() {
            return Err(crate::Error::ConfigError {
                message: "empty fault specification".to_string(),
            });
        }

        match parts[0] {
            "network" => Self::parse_network_fault(&parts[1..]),
            "disk" => Self::parse_disk_fault(&parts[1..]),
            "crash" => Self::parse_crash_fault(&parts[1..]),
            "heal" => Ok(FaultSpec::Heal),
            other => Err(crate::Error::ConfigError {
                message: format!("unknown fault type: {}", other),
            }),
        }
    }

    fn parse_network_fault(parts: &[&str]) -> Result<Self> {
        if parts.is_empty() {
            return Err(crate::Error::ConfigError {
                message: "network fault requires subtype".to_string(),
            });
        }

        match parts[0] {
            "partition" => {
                if parts.len() < 2 {
                    return Err(crate::Error::ConfigError {
                        message: "partition requires node groups".to_string(),
                    });
                }
                
                let groups_str = parts[1];
                let groups: Vec<Vec<u32>> = groups_str
                    .split('|')
                    .map(|g| {
                        g.split(',')
                            .filter_map(|s| s.trim().parse().ok())
                            .collect()
                    })
                    .collect();
                
                let duration = parts.get(2).and_then(|s| parse_duration(s).ok());
                
                Ok(FaultSpec::Partition { groups, duration })
            }
            "drop" => {
                let rate = parts.get(1)
                    .ok_or_else(|| crate::Error::ConfigError {
                        message: "drop requires rate".to_string(),
                    })?;
                let rate = parse_percentage(rate)?;
                Ok(FaultSpec::Drop { rate })
            }
            "delay" => {
                let range = parts.get(1)
                    .ok_or_else(|| crate::Error::ConfigError {
                        message: "delay requires range".to_string(),
                    })?;
                let (min, max) = parse_duration_range(range)?;
                Ok(FaultSpec::Delay { min, max })
            }
            "duplicate" => {
                let rate = parts.get(1)
                    .ok_or_else(|| crate::Error::ConfigError {
                        message: "duplicate requires rate".to_string(),
                    })?;
                let rate = parse_percentage(rate)?;
                Ok(FaultSpec::Duplicate { rate })
            }
            other => Err(crate::Error::ConfigError {
                message: format!("unknown network fault: {}", other),
            }),
        }
    }

    fn parse_disk_fault(parts: &[&str]) -> Result<Self> {
        if parts.len() < 3 {
            return Err(crate::Error::ConfigError {
                message: "disk fault requires: error:read|write:rate".to_string(),
            });
        }

        if parts[0] != "error" {
            return Err(crate::Error::ConfigError {
                message: format!("unknown disk fault: {}", parts[0]),
            });
        }

        let rate = parse_percentage(parts[2])?;

        match parts[1] {
            "read" => Ok(FaultSpec::DiskReadError { rate }),
            "write" => Ok(FaultSpec::DiskWriteError { rate }),
            other => Err(crate::Error::ConfigError {
                message: format!("unknown disk operation: {}", other),
            }),
        }
    }

    fn parse_crash_fault(parts: &[&str]) -> Result<Self> {
        if parts.len() < 3 {
            return Err(crate::Error::ConfigError {
                message: "crash fault requires: node:after:duration".to_string(),
            });
        }

        let node: u32 = parts[0].parse().map_err(|_| crate::Error::ConfigError {
            message: format!("invalid node id: {}", parts[0]),
        })?;

        if parts[1] != "after" {
            return Err(crate::Error::ConfigError {
                message: "expected 'after' keyword".to_string(),
            });
        }

        let after = parse_duration(parts[2])?;

        Ok(FaultSpec::Crash { node, after })
    }

    /// Convert to a network Fault.
    pub fn to_network_fault(&self) -> Option<Fault> {
        match self {
            FaultSpec::Partition { groups, .. } => {
                Some(Fault::partition(groups.clone()))
            }
            FaultSpec::Drop { rate } => Some(Fault::drop(*rate)),
            FaultSpec::Delay { min, max } => Some(Fault::delay(*min, *max)),
            FaultSpec::Duplicate { rate } => Some(Fault::duplicate(*rate)),
            FaultSpec::Heal => Some(Fault::heal()),
            _ => None,
        }
    }
}

fn parse_duration(s: &str) -> Result<Duration> {
    let s = s.trim();
    
    if let Some(ms) = s.strip_suffix("ms") {
        let val: u64 = ms.parse().map_err(|_| crate::Error::ConfigError {
            message: format!("invalid duration: {}", s),
        })?;
        return Ok(Duration::from_millis(val));
    }
    
    if let Some(us) = s.strip_suffix("us") {
        let val: u64 = us.parse().map_err(|_| crate::Error::ConfigError {
            message: format!("invalid duration: {}", s),
        })?;
        return Ok(Duration::from_micros(val));
    }
    
    if let Some(ns) = s.strip_suffix("ns") {
        let val: u64 = ns.parse().map_err(|_| crate::Error::ConfigError {
            message: format!("invalid duration: {}", s),
        })?;
        return Ok(Duration::from_nanos(val));
    }
    
    if let Some(secs) = s.strip_suffix('s') {
        let val: u64 = secs.parse().map_err(|_| crate::Error::ConfigError {
            message: format!("invalid duration: {}", s),
        })?;
        return Ok(Duration::from_secs(val));
    }

    // Default to milliseconds
    let val: u64 = s.parse().map_err(|_| crate::Error::ConfigError {
        message: format!("invalid duration: {}", s),
    })?;
    Ok(Duration::from_millis(val))
}

fn parse_duration_range(s: &str) -> Result<(Duration, Duration)> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 2 {
        return Err(crate::Error::ConfigError {
            message: format!("invalid duration range: {}", s),
        });
    }
    
    let min = parse_duration(parts[0])?;
    let max = parse_duration(parts[1])?;
    
    Ok((min, max))
}

fn parse_percentage(s: &str) -> Result<f64> {
    let s = s.trim();
    
    if let Some(pct) = s.strip_suffix('%') {
        let val: f64 = pct.parse().map_err(|_| crate::Error::ConfigError {
            message: format!("invalid percentage: {}", s),
        })?;
        return Ok(val / 100.0);
    }
    
    // Assume it's already a decimal
    let val: f64 = s.parse().map_err(|_| crate::Error::ConfigError {
        message: format!("invalid rate: {}", s),
    })?;
    
    Ok(val.clamp(0.0, 1.0))
}

/// Result of fault injection run.
#[derive(Debug)]
pub struct InjectResult {
    pub faults_applied: Vec<FaultSpec>,
    pub bugs_found: u32,
    pub seed: u64,
    pub test_passed: bool,
    pub output: String,
}

/// Execute the inject command.
pub fn inject_command(args: InjectArgs) -> Result<InjectResult> {
    use std::process::Command;
    
    let seed = args.seed.unwrap_or_else(|| {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(42)
    });

    if args.verbose {
        eprintln!("Fault injection: {}", args.test_binary);
        eprintln!("Seed: {}", seed);
    }

    let mut faults_applied = Vec::new();
    let mut fault_env_specs = Vec::new();

    for fault_str in &args.fault {
        let fault_spec = FaultSpec::parse(fault_str)?;
        
        if args.verbose {
            eprintln!("  Fault: {:?}", fault_spec);
        }
        
        fault_env_specs.push(fault_str.clone());
        faults_applied.push(fault_spec);
    }

    // Build the fault specification string for environment variable
    let faults_json = serde_json::to_string(&fault_env_specs)
        .unwrap_or_else(|_| "[]".to_string());

    // Run the test binary with fault injection environment variables
    let mut cmd = Command::new(&args.test_binary);
    
    // Set environment variables
    cmd.env("CHRONOS_SEED", seed.to_string());
    cmd.env("CHRONOS_FAULTS", &faults_json);
    cmd.env("CHRONOS_FAULT_INJECTION", "1");
    
    // Pass individual fault specs as numbered env vars for simpler parsing
    for (i, spec) in fault_env_specs.iter().enumerate() {
        cmd.env(format!("CHRONOS_FAULT_{}", i), spec);
    }
    cmd.env("CHRONOS_FAULT_COUNT", fault_env_specs.len().to_string());

    if let Some(ref record_path) = args.record {
        cmd.env("CHRONOS_RECORD", record_path.to_string_lossy().to_string());
    }

    if args.verbose {
        eprintln!("\nRunning with faults...");
    }

    let output = cmd.output().map_err(crate::Error::Io)?;
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined_output = format!("{}\n{}", stdout, stderr);
    
    let test_passed = output.status.success();
    let bugs_found = if test_passed { 0 } else { 1 };

    if args.verbose {
        if !stdout.is_empty() {
            eprintln!("stdout:\n{}", stdout);
        }
        if !stderr.is_empty() {
            eprintln!("stderr:\n{}", stderr);
        }
    }

    // Print summary
    println!();
    println!("=== Fault Injection Results ===");
    println!("Test: {}", args.test_binary);
    println!("Seed: {}", seed);
    println!("Faults applied: {}", faults_applied.len());
    for (i, fault) in faults_applied.iter().enumerate() {
        println!("  {}. {:?}", i + 1, fault);
    }
    println!();
    
    if test_passed {
        println!("Result: PASS (test succeeded despite faults)");
    } else {
        println!("Result: FAIL (bug found!)");
        println!("Exit code: {:?}", output.status.code());
        
        // Extract failure info
        for line in combined_output.lines() {
            if line.contains("assertion") || line.contains("panic") || line.contains("FAIL") {
                println!("  {}", line);
            }
        }
        
        println!();
        println!("Replay: chronos inject {} --seed {} {}", 
            args.test_binary, 
            seed,
            args.fault.iter().map(|f| format!("-f \"{}\"", f)).collect::<Vec<_>>().join(" ")
        );
    }

    Ok(InjectResult {
        faults_applied,
        bugs_found,
        seed,
        test_passed,
        output: combined_output,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_partition() {
        let spec = FaultSpec::parse("network:partition:0,1|2,3").unwrap();
        match spec {
            FaultSpec::Partition { groups, .. } => {
                assert_eq!(groups.len(), 2);
                assert_eq!(groups[0], vec![0, 1]);
                assert_eq!(groups[1], vec![2, 3]);
            }
            _ => panic!("expected partition"),
        }
    }

    #[test]
    fn test_parse_drop() {
        let spec = FaultSpec::parse("network:drop:10%").unwrap();
        match spec {
            FaultSpec::Drop { rate } => {
                assert!((rate - 0.1).abs() < 0.001);
            }
            _ => panic!("expected drop"),
        }
    }

    #[test]
    fn test_parse_delay() {
        let spec = FaultSpec::parse("network:delay:50ms-200ms").unwrap();
        match spec {
            FaultSpec::Delay { min, max } => {
                assert_eq!(min, Duration::from_millis(50));
                assert_eq!(max, Duration::from_millis(200));
            }
            _ => panic!("expected delay"),
        }
    }

    #[test]
    fn test_parse_disk_error() {
        let spec = FaultSpec::parse("disk:error:read:5%").unwrap();
        match spec {
            FaultSpec::DiskReadError { rate } => {
                assert!((rate - 0.05).abs() < 0.001);
            }
            _ => panic!("expected disk read error"),
        }
    }

    #[test]
    fn test_parse_crash() {
        let spec = FaultSpec::parse("crash:1:after:100ms").unwrap();
        match spec {
            FaultSpec::Crash { node, after } => {
                assert_eq!(node, 1);
                assert_eq!(after, Duration::from_millis(100));
            }
            _ => panic!("expected crash"),
        }
    }

    #[test]
    fn test_parse_heal() {
        let spec = FaultSpec::parse("heal").unwrap();
        assert!(matches!(spec, FaultSpec::Heal));
    }

    #[test]
    fn test_parse_duration_units() {
        assert_eq!(parse_duration("100ms").unwrap(), Duration::from_millis(100));
        assert_eq!(parse_duration("5s").unwrap(), Duration::from_secs(5));
        assert_eq!(parse_duration("1000us").unwrap(), Duration::from_micros(1000));
        assert_eq!(parse_duration("50").unwrap(), Duration::from_millis(50));
    }

    #[test]
    fn test_parse_percentage() {
        assert!((parse_percentage("10%").unwrap() - 0.1).abs() < 0.001);
        assert!((parse_percentage("0.5").unwrap() - 0.5).abs() < 0.001);
        assert!((parse_percentage("100%").unwrap() - 1.0).abs() < 0.001);
    }
}
