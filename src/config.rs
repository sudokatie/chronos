//! Configuration loading and validation.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::Result;

/// Main configuration for Chronos.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Scheduling configuration.
    pub scheduler: SchedulerConfig,
    /// Network simulation configuration.
    pub network: NetworkConfig,
    /// Fault injection configuration.
    pub faults: FaultsConfig,
    /// Detection configuration.
    pub detection: DetectionConfig,
    /// Recording configuration.
    pub recording: RecordingConfig,
}

impl Config {
    /// Load configuration from a TOML file.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        Self::from_str(&contents)
    }

    /// Parse configuration from a TOML string.
    pub fn from_str(s: &str) -> Result<Self> {
        let config: Config = toml::from_str(s)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        config.validate()?;
        Ok(config)
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Result<()> {
        self.scheduler.validate()?;
        self.network.validate()?;
        self.faults.validate()?;
        self.detection.validate()?;
        Ok(())
    }

    /// Save configuration to a TOML file.
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let contents = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, contents)?;
        Ok(())
    }
}

/// Scheduler configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SchedulerConfig {
    /// Default scheduling strategy.
    pub strategy: String,
    /// Random seed (0 = use system time).
    pub seed: u64,
    /// Number of iterations for exploration.
    pub iterations: u32,
    /// PCT depth parameter.
    pub pct_depth: u32,
    /// Maximum steps before timeout.
    pub max_steps: u64,
    /// Timeout in seconds (simulated time).
    pub timeout_secs: u64,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            strategy: "random".to_string(),
            seed: 0,
            iterations: 1000,
            pct_depth: 3,
            max_steps: 1_000_000,
            timeout_secs: 60,
        }
    }
}

impl SchedulerConfig {
    fn validate(&self) -> Result<()> {
        let valid_strategies = ["fifo", "random", "pct", "dfs", "context_bound"];
        if !valid_strategies.contains(&self.strategy.as_str()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid strategy: {}. Valid: {:?}", self.strategy, valid_strategies),
            ).into());
        }
        if self.pct_depth == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "pct_depth must be > 0",
            ).into());
        }
        if self.iterations == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "iterations must be > 0",
            ).into());
        }
        Ok(())
    }

    /// Get the timeout as a Duration.
    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs)
    }
}

/// Network simulation configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkConfig {
    /// Latency configuration.
    pub latency: LatencyConfig,
    /// Packet drop rate (0.0 - 1.0).
    pub drop_rate: f64,
    /// Packet duplicate rate (0.0 - 1.0).
    pub duplicate_rate: f64,
    /// Maximum in-flight messages.
    pub max_in_flight: usize,
    /// Bandwidth limit in bytes per second (0 = unlimited).
    pub bandwidth_bps: u64,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            latency: LatencyConfig::default(),
            drop_rate: 0.0,
            duplicate_rate: 0.0,
            max_in_flight: 10000,
            bandwidth_bps: 0,
        }
    }
}

impl NetworkConfig {
    fn validate(&self) -> Result<()> {
        if self.drop_rate < 0.0 || self.drop_rate > 1.0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "drop_rate must be between 0.0 and 1.0",
            ).into());
        }
        if self.duplicate_rate < 0.0 || self.duplicate_rate > 1.0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "duplicate_rate must be between 0.0 and 1.0",
            ).into());
        }
        self.latency.validate()?;
        Ok(())
    }
}

/// Latency configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum LatencyConfig {
    /// Fixed latency.
    Fixed { ms: u64 },
    /// Uniform distribution.
    Uniform { min_ms: u64, max_ms: u64 },
    /// Normal distribution.
    Normal { mean_ms: u64, stddev_ms: u64 },
    /// Bimodal distribution.
    Bimodal { fast_ms: u64, slow_ms: u64, slow_pct: f64 },
}

impl Default for LatencyConfig {
    fn default() -> Self {
        Self::Uniform { min_ms: 1, max_ms: 10 }
    }
}

impl LatencyConfig {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Fixed { .. } => Ok(()),
            Self::Uniform { min_ms, max_ms } => {
                if min_ms > max_ms {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "latency min_ms must be <= max_ms",
                    ).into());
                }
                Ok(())
            }
            Self::Normal { .. } => Ok(()),
            Self::Bimodal { slow_pct, .. } => {
                if *slow_pct < 0.0 || *slow_pct > 1.0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "slow_pct must be between 0.0 and 1.0",
                    ).into());
                }
                Ok(())
            }
        }
    }

    /// Convert to a crate::network::LatencyModel.
    pub fn to_latency_model(&self) -> crate::network::LatencyModel {
        match self {
            Self::Fixed { ms } => crate::network::LatencyModel::Fixed(Duration::from_millis(*ms)),
            Self::Uniform { min_ms, max_ms } => crate::network::LatencyModel::Uniform {
                min: Duration::from_millis(*min_ms),
                max: Duration::from_millis(*max_ms),
            },
            Self::Normal { mean_ms, stddev_ms } => crate::network::LatencyModel::Normal {
                mean: Duration::from_millis(*mean_ms),
                stddev: Duration::from_millis(*stddev_ms),
            },
            Self::Bimodal { fast_ms, slow_ms, slow_pct } => crate::network::LatencyModel::Bimodal {
                fast: Duration::from_millis(*fast_ms),
                slow: Duration::from_millis(*slow_ms),
                slow_pct: *slow_pct,
            },
        }
    }
}

/// Fault injection configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FaultsConfig {
    /// Enable fault injection.
    pub enabled: bool,
    /// Scheduled faults.
    pub schedule: Vec<ScheduledFault>,
}

impl FaultsConfig {
    fn validate(&self) -> Result<()> {
        for (i, fault) in self.schedule.iter().enumerate() {
            fault.validate().map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("fault schedule[{}]: {}", i, e),
                )
            })?;
        }
        Ok(())
    }

    /// Convert to a FaultSchedule.
    pub fn to_fault_schedule(&self) -> crate::network::FaultSchedule {
        let mut schedule = crate::network::FaultSchedule::new();
        if !self.enabled {
            return schedule;
        }
        for sf in &self.schedule {
            let instant = crate::time::Instant::from_nanos(sf.at_secs as u64 * 1_000_000_000);
            if let Some(fault) = sf.to_fault() {
                schedule.add(instant, fault);
            }
        }
        schedule
    }
}

/// A scheduled fault from config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledFault {
    /// Time in seconds when fault triggers.
    pub at_secs: f64,
    /// Fault type.
    pub fault: String,
    /// Node groups for partition (e.g., [[0, 1], [2, 3]]).
    #[serde(default)]
    pub nodes: Vec<Vec<u32>>,
    /// Target node for crash/restart.
    #[serde(default)]
    pub node: Option<u32>,
    /// Drop rate for network:drop fault.
    #[serde(default)]
    pub rate: Option<f64>,
    /// Min delay for network:delay fault.
    #[serde(default)]
    pub min_ms: Option<u64>,
    /// Max delay for network:delay fault.
    #[serde(default)]
    pub max_ms: Option<u64>,
    /// Clock skew rate for clock:skew fault.
    #[serde(default)]
    pub skew_rate: Option<f64>,
    /// Clock jump delta in milliseconds.
    #[serde(default)]
    pub jump_ms: Option<i64>,
    /// Duration for crash:after (milliseconds).
    #[serde(default)]
    pub after_ms: Option<u64>,
}

impl ScheduledFault {
    fn validate(&self) -> Result<()> {
        let valid_faults = ["partition", "heal", "drop", "delay", "duplicate", 
                           "crash", "restart", "clock_skew", "clock_jump",
                           "disk_read_error", "disk_write_error", "full_disk", "corrupt"];
        if !valid_faults.contains(&self.fault.as_str()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid fault type: {}. Valid: {:?}", self.fault, valid_faults),
            ).into());
        }
        if self.at_secs < 0.0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "at_secs must be >= 0",
            ).into());
        }
        Ok(())
    }

    /// Convert to a Fault enum.
    pub fn to_fault(&self) -> Option<crate::network::Fault> {
        match self.fault.as_str() {
            "partition" => Some(crate::network::Fault::Partition { 
                groups: self.nodes.clone() 
            }),
            "heal" => Some(crate::network::Fault::Heal),
            "drop" => Some(crate::network::Fault::Drop { 
                rate: self.rate.unwrap_or(0.1) 
            }),
            "delay" => Some(crate::network::Fault::Delay {
                min: Duration::from_millis(self.min_ms.unwrap_or(10)),
                max: Duration::from_millis(self.max_ms.unwrap_or(100)),
            }),
            "duplicate" => Some(crate::network::Fault::Duplicate {
                rate: self.rate.unwrap_or(0.1),
            }),
            "clock_skew" => {
                let node = self.node.unwrap_or(0);
                Some(crate::network::Fault::ClockSkew {
                    node,
                    rate: self.skew_rate.unwrap_or(1.5),
                })
            }
            "clock_jump" => {
                let node = self.node.unwrap_or(0);
                let delta = self.jump_ms.unwrap_or(1000) * 1_000_000; // ms to nanos
                Some(crate::network::Fault::ClockJump { node, delta })
            }
            "crash" => {
                let node = self.node.unwrap_or(0);
                Some(crate::network::Fault::Crash { node })
            }
            "restart" => {
                let node = self.node.unwrap_or(0);
                Some(crate::network::Fault::Restart { node })
            }
            "disk_read_error" => Some(crate::network::Fault::DiskReadError {
                rate: self.rate.unwrap_or(0.05),
            }),
            "disk_write_error" => Some(crate::network::Fault::DiskWriteError {
                rate: self.rate.unwrap_or(0.05),
            }),
            "full_disk" => Some(crate::network::Fault::FullDisk),
            "corrupt" => Some(crate::network::Fault::Corrupt {
                rate: self.rate.unwrap_or(0.01),
            }),
            _ => None,
        }
    }
}

/// Detection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DetectionConfig {
    /// Enable deadlock detection.
    pub deadlock: bool,
    /// Enable livelock detection.
    pub livelock: bool,
    /// Livelock threshold (steps without progress).
    pub livelock_threshold: u64,
    /// Enable data race detection.
    pub race_detection: bool,
}

impl Default for DetectionConfig {
    fn default() -> Self {
        Self {
            deadlock: true,
            livelock: true,
            livelock_threshold: 10000,
            race_detection: false,
        }
    }
}

impl DetectionConfig {
    fn validate(&self) -> Result<()> {
        if self.livelock && self.livelock_threshold == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "livelock_threshold must be > 0 when livelock is enabled",
            ).into());
        }
        Ok(())
    }
}

/// Recording configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordingConfig {
    /// Enable execution recording.
    pub enabled: bool,
    /// Output directory for recordings.
    pub output_dir: String,
    /// Compress recordings.
    pub compress: bool,
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            output_dir: "./recordings".to_string(),
            compress: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_config_defaults() {
        let config = Config::default();
        assert_eq!(config.scheduler.strategy, "random");
        assert!(!config.faults.enabled);
        assert!(config.detection.deadlock);
    }

    #[test]
    fn test_config_from_str() {
        let toml = r#"
            [scheduler]
            strategy = "pct"
            seed = 42
            iterations = 500

            [network.latency]
            type = "uniform"
            min_ms = 5
            max_ms = 20
        "#;

        let config = Config::from_str(toml).unwrap();
        assert_eq!(config.scheduler.strategy, "pct");
        assert_eq!(config.scheduler.seed, 42);
        assert_eq!(config.scheduler.iterations, 500);
    }

    #[test]
    fn test_config_with_faults() {
        let toml = r#"
            [faults]
            enabled = true

            [[faults.schedule]]
            at_secs = 5.0
            fault = "partition"
            nodes = [[0, 1], [2, 3]]

            [[faults.schedule]]
            at_secs = 10.0
            fault = "heal"

            [[faults.schedule]]
            at_secs = 15.0
            fault = "crash"
            node = 1
        "#;

        let config = Config::from_str(toml).unwrap();
        assert!(config.faults.enabled);
        assert_eq!(config.faults.schedule.len(), 3);
        assert_eq!(config.faults.schedule[0].fault, "partition");
        assert_eq!(config.faults.schedule[1].fault, "heal");
        assert_eq!(config.faults.schedule[2].fault, "crash");
    }

    #[test]
    fn test_config_latency_types() {
        let toml = r#"
            [network.latency]
            type = "bimodal"
            fast_ms = 1
            slow_ms = 100
            slow_pct = 0.3
        "#;

        let config = Config::from_str(toml).unwrap();
        match config.network.latency {
            LatencyConfig::Bimodal { fast_ms, slow_ms, slow_pct } => {
                assert_eq!(fast_ms, 1);
                assert_eq!(slow_ms, 100);
                assert!((slow_pct - 0.3).abs() < 0.001);
            }
            _ => panic!("expected bimodal"),
        }
    }

    #[test]
    fn test_config_invalid_strategy() {
        let toml = r#"
            [scheduler]
            strategy = "invalid"
        "#;

        let result = Config::from_str(toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_invalid_drop_rate() {
        let toml = r#"
            [network]
            drop_rate = 1.5
        "#;

        let result = Config::from_str(toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_invalid_fault_type() {
        let toml = r#"
            [faults]
            enabled = true

            [[faults.schedule]]
            at_secs = 1.0
            fault = "unknown_fault"
        "#;

        let result = Config::from_str(toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_save_load() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("chronos.toml");

        let mut config = Config::default();
        config.faults.enabled = true;
        config.faults.schedule.push(ScheduledFault {
            at_secs: 5.0,
            fault: "partition".to_string(),
            nodes: vec![vec![0, 1], vec![2]],
            node: None,
            rate: None,
            min_ms: None,
            max_ms: None,
            skew_rate: None,
            jump_ms: None,
            after_ms: None,
        });
        config.save(&path).unwrap();

        let loaded = Config::load(&path).unwrap();
        assert!(loaded.faults.enabled);
        assert_eq!(loaded.faults.schedule.len(), 1);
    }

    #[test]
    fn test_fault_schedule_conversion() {
        let mut config = FaultsConfig::default();
        config.enabled = true;
        config.schedule.push(ScheduledFault {
            at_secs: 5.0,
            fault: "drop".to_string(),
            nodes: vec![],
            node: None,
            rate: Some(0.25),
            min_ms: None,
            max_ms: None,
            skew_rate: None,
            jump_ms: None,
            after_ms: None,
        });

        let schedule = config.to_fault_schedule();
        assert!(!schedule.is_empty());
    }

    #[test]
    fn test_latency_model_conversion() {
        let config = LatencyConfig::Normal { mean_ms: 50, stddev_ms: 10 };
        let _model = config.to_latency_model();
    }
}
