//! Network latency models for realistic simulation.

use std::time::Duration;

use rand::Rng;
use rand_distr::{Distribution, Normal};

/// Model for network latency distribution.
#[derive(Clone, Debug)]
pub enum LatencyModel {
    /// Fixed latency for all messages.
    Fixed(Duration),
    /// Uniform distribution between min and max.
    Uniform { min: Duration, max: Duration },
    /// Normal distribution with mean and standard deviation.
    Normal { mean: Duration, stddev: Duration },
    /// Bimodal distribution (fast local / slow remote).
    Bimodal {
        fast: Duration,
        slow: Duration,
        /// Probability of slow path (0.0 to 1.0).
        slow_pct: f64,
    },
}

impl LatencyModel {
    /// Samples a latency from this model.
    pub fn sample(&self, rng: &mut impl Rng) -> Duration {
        match self {
            Self::Fixed(d) => *d,
            Self::Uniform { min, max } => {
                let min_nanos = min.as_nanos() as u64;
                let max_nanos = max.as_nanos() as u64;
                if min_nanos >= max_nanos {
                    return *min;
                }
                let nanos = rng.gen_range(min_nanos..max_nanos);
                Duration::from_nanos(nanos)
            }
            Self::Normal { mean, stddev } => {
                let mean_nanos = mean.as_nanos() as f64;
                let stddev_nanos = stddev.as_nanos() as f64;
                
                if stddev_nanos <= 0.0 {
                    return *mean;
                }

                let normal = Normal::new(mean_nanos, stddev_nanos).unwrap();
                let nanos = normal.sample(rng).max(0.0) as u64;
                Duration::from_nanos(nanos)
            }
            Self::Bimodal { fast, slow, slow_pct } => {
                if rng.gen::<f64>() < *slow_pct {
                    *slow
                } else {
                    *fast
                }
            }
        }
    }

    /// Creates a fixed latency model.
    pub fn fixed(latency: Duration) -> Self {
        Self::Fixed(latency)
    }

    /// Creates a uniform latency model.
    pub fn uniform(min: Duration, max: Duration) -> Self {
        Self::Uniform { min, max }
    }

    /// Creates a normal distribution latency model.
    pub fn normal(mean: Duration, stddev: Duration) -> Self {
        Self::Normal { mean, stddev }
    }

    /// Creates a bimodal latency model (e.g., local vs remote).
    pub fn bimodal(fast: Duration, slow: Duration, slow_pct: f64) -> Self {
        Self::Bimodal { fast, slow, slow_pct }
    }

    /// Creates a "LAN" latency model (1-5ms uniform).
    pub fn lan() -> Self {
        Self::uniform(Duration::from_millis(1), Duration::from_millis(5))
    }

    /// Creates a "WAN" latency model (50-150ms uniform).
    pub fn wan() -> Self {
        Self::uniform(Duration::from_millis(50), Duration::from_millis(150))
    }

    /// Creates a typical datacenter latency model.
    pub fn datacenter() -> Self {
        Self::bimodal(
            Duration::from_micros(100),  // Same rack
            Duration::from_millis(1),     // Cross-rack
            0.3,                          // 30% cross-rack
        )
    }
}

impl Default for LatencyModel {
    fn default() -> Self {
        Self::Fixed(Duration::from_millis(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn test_rng() -> StdRng {
        StdRng::seed_from_u64(12345)
    }

    #[test]
    fn test_fixed_latency() {
        let model = LatencyModel::fixed(Duration::from_millis(10));
        let mut rng = test_rng();
        
        for _ in 0..10 {
            assert_eq!(model.sample(&mut rng), Duration::from_millis(10));
        }
    }

    #[test]
    fn test_uniform_latency() {
        let model = LatencyModel::uniform(
            Duration::from_millis(10),
            Duration::from_millis(20),
        );
        let mut rng = test_rng();
        
        for _ in 0..100 {
            let latency = model.sample(&mut rng);
            assert!(latency >= Duration::from_millis(10));
            assert!(latency < Duration::from_millis(20));
        }
    }

    #[test]
    fn test_uniform_min_equals_max() {
        let model = LatencyModel::uniform(
            Duration::from_millis(10),
            Duration::from_millis(10),
        );
        let mut rng = test_rng();
        
        assert_eq!(model.sample(&mut rng), Duration::from_millis(10));
    }

    #[test]
    fn test_normal_latency() {
        let model = LatencyModel::normal(
            Duration::from_millis(50),
            Duration::from_millis(10),
        );
        let mut rng = test_rng();
        
        let samples: Vec<_> = (0..1000).map(|_| model.sample(&mut rng)).collect();
        
        // Mean should be roughly 50ms
        let mean_nanos: u64 = samples.iter().map(|d| d.as_nanos() as u64).sum::<u64>() / 1000;
        let mean = Duration::from_nanos(mean_nanos);
        
        // Allow 20% deviation from expected mean
        assert!(mean > Duration::from_millis(40));
        assert!(mean < Duration::from_millis(60));
    }

    #[test]
    fn test_normal_no_negative() {
        let model = LatencyModel::normal(
            Duration::from_millis(1),
            Duration::from_millis(10), // Large stddev relative to mean
        );
        let mut rng = test_rng();
        
        // Even with large stddev, latency should never be negative
        for _ in 0..100 {
            let latency = model.sample(&mut rng);
            assert!(latency >= Duration::ZERO);
        }
    }

    #[test]
    fn test_bimodal_latency() {
        let model = LatencyModel::bimodal(
            Duration::from_millis(1),   // fast
            Duration::from_millis(100), // slow
            0.5,                        // 50% slow
        );
        let mut rng = test_rng();
        
        let samples: Vec<_> = (0..1000).map(|_| model.sample(&mut rng)).collect();
        
        let fast_count = samples.iter().filter(|&&d| d == Duration::from_millis(1)).count();
        let slow_count = samples.iter().filter(|&&d| d == Duration::from_millis(100)).count();
        
        // Both should be roughly 500, allow 20% deviation
        assert!(fast_count > 400 && fast_count < 600);
        assert!(slow_count > 400 && slow_count < 600);
    }

    #[test]
    fn test_preset_models() {
        let mut rng = test_rng();
        
        // Just verify they don't panic
        let _ = LatencyModel::lan().sample(&mut rng);
        let _ = LatencyModel::wan().sample(&mut rng);
        let _ = LatencyModel::datacenter().sample(&mut rng);
    }

    #[test]
    fn test_default() {
        let model = LatencyModel::default();
        let mut rng = test_rng();
        assert_eq!(model.sample(&mut rng), Duration::from_millis(1));
    }
}
