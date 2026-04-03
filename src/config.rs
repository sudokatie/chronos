use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::Result;

/// Main configuration for Chronos.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Scheduling configuration.
    pub scheduler: SchedulerConfig,
    /// Network simulation configuration.
    pub network: NetworkConfig,
    /// Detection configuration.
    pub detection: DetectionConfig,
    /// Recording configuration.
    pub recording: RecordingConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            scheduler: SchedulerConfig::default(),
            network: NetworkConfig::default(),
            detection: DetectionConfig::default(),
            recording: RecordingConfig::default(),
        }
    }
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
    /// PCT depth parameter.
    pub pct_depth: u32,
    /// Maximum steps before timeout.
    pub max_steps: u64,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            strategy: "random".to_string(),
            seed: 0,
            pct_depth: 3,
            max_steps: 1_000_000,
        }
    }
}

impl SchedulerConfig {
    fn validate(&self) -> Result<()> {
        let valid_strategies = ["fifo", "random", "pct"];
        if !valid_strategies.contains(&self.strategy.as_str()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid strategy: {}", self.strategy),
            ).into());
        }
        if self.pct_depth == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "pct_depth must be > 0",
            ).into());
        }
        Ok(())
    }
}

/// Network simulation configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkConfig {
    /// Base latency in milliseconds.
    pub latency_ms: u64,
    /// Latency jitter in milliseconds.
    pub jitter_ms: u64,
    /// Packet drop rate (0.0 - 1.0).
    pub drop_rate: f64,
    /// Packet duplicate rate (0.0 - 1.0).
    pub duplicate_rate: f64,
    /// Maximum in-flight messages.
    pub max_in_flight: usize,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            latency_ms: 10,
            jitter_ms: 5,
            drop_rate: 0.0,
            duplicate_rate: 0.0,
            max_in_flight: 10000,
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
        Ok(())
    }

    /// Get the base latency as a Duration.
    pub fn latency(&self) -> Duration {
        Duration::from_millis(self.latency_ms)
    }

    /// Get the jitter as a Duration.
    pub fn jitter(&self) -> Duration {
        Duration::from_millis(self.jitter_ms)
    }
}

/// Detection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DetectionConfig {
    /// Enable deadlock detection.
    pub deadlock_detection: bool,
    /// Enable livelock detection.
    pub livelock_detection: bool,
    /// Livelock threshold (steps without progress).
    pub livelock_threshold: u64,
}

impl Default for DetectionConfig {
    fn default() -> Self {
        Self {
            deadlock_detection: true,
            livelock_detection: true,
            livelock_threshold: 1000,
        }
    }
}

impl DetectionConfig {
    fn validate(&self) -> Result<()> {
        if self.livelock_detection && self.livelock_threshold == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "livelock_threshold must be > 0 when livelock_detection is enabled",
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
        assert_eq!(config.network.latency_ms, 10);
        assert!(config.detection.deadlock_detection);
    }

    #[test]
    fn test_config_from_str() {
        let toml = r#"
            [scheduler]
            strategy = "pct"
            seed = 42

            [network]
            latency_ms = 20
        "#;

        let config = Config::from_str(toml).unwrap();
        assert_eq!(config.scheduler.strategy, "pct");
        assert_eq!(config.scheduler.seed, 42);
        assert_eq!(config.network.latency_ms, 20);
    }

    #[test]
    fn test_config_partial() {
        let toml = r#"
            [scheduler]
            strategy = "fifo"
        "#;

        let config = Config::from_str(toml).unwrap();
        assert_eq!(config.scheduler.strategy, "fifo");
        // Defaults for unspecified values
        assert_eq!(config.network.latency_ms, 10);
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
    fn test_config_invalid_livelock_threshold() {
        let toml = r#"
            [detection]
            livelock_detection = true
            livelock_threshold = 0
        "#;

        let result = Config::from_str(toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_save_load() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("chronos.toml");

        let config = Config::default();
        config.save(&path).unwrap();

        let loaded = Config::load(&path).unwrap();
        assert_eq!(loaded.scheduler.strategy, config.scheduler.strategy);
        assert_eq!(loaded.network.latency_ms, config.network.latency_ms);
    }

    #[test]
    fn test_config_load_nonexistent() {
        let result = Config::load("/nonexistent/path.toml");
        assert!(result.is_err());
    }

    #[test]
    fn test_network_config_durations() {
        let config = NetworkConfig {
            latency_ms: 50,
            jitter_ms: 10,
            ..Default::default()
        };

        assert_eq!(config.latency(), Duration::from_millis(50));
        assert_eq!(config.jitter(), Duration::from_millis(10));
    }

    #[test]
    fn test_scheduler_config_validate_pct_depth() {
        let config = SchedulerConfig {
            pct_depth: 0,
            ..Default::default()
        };

        assert!(config.validate().is_err());
    }
}
