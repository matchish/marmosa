use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoRebuildMode {
    None,
    MissingCheckpointsOnly,
    ForceFullRebuild,
}

#[derive(Debug, Clone)]
pub struct ProjectionOptions {
    pub polling_interval: Duration,
    pub batch_size: usize,
    pub rebuild_flush_interval: usize,
    pub rebuild_batch_size: usize,
    pub auto_rebuild: AutoRebuildMode,
    pub max_concurrent_rebuilds: usize,
}

impl Default for ProjectionOptions {
    fn default() -> Self {
        Self {
            polling_interval: Duration::from_secs(5),
            batch_size: 1_000,
            rebuild_flush_interval: 10_000,
            rebuild_batch_size: 5_000,
            auto_rebuild: AutoRebuildMode::MissingCheckpointsOnly,
            max_concurrent_rebuilds: 4,
        }
    }
}

pub struct ProjectionOptionsValidator;

impl ProjectionOptionsValidator {
    pub fn validate(options: &ProjectionOptions) -> Result<(), Vec<String>> {
        let mut failures = Vec::new();
        if options.polling_interval < Duration::from_millis(100) {
            failures.push("Polling interval must be at least 100ms".to_string());
        }
        if options.polling_interval > Duration::from_secs(3600) {
            failures.push("Polling interval must be at most 1 hour".to_string());
        }
        if options.batch_size < 1 {
            failures.push("Batch size must be at least 1".to_string());
        }
        if options.batch_size > 100_000 {
            failures.push("Batch size must be at most 100,000".to_string());
        }
        if options.max_concurrent_rebuilds < 1 {
            failures.push("Max concurrent rebuilds must be at least 1".to_string());
        }
        if options.max_concurrent_rebuilds > 64 {
            failures.push("Max concurrent rebuilds must be at most 64".to_string());
        }
        if options.rebuild_flush_interval < 100 {
            failures.push("Rebuild flush interval must be at least 100".to_string());
        }
        if options.rebuild_flush_interval > 1_000_000 {
            failures.push("Rebuild flush interval must be at most 1,000,000".to_string());
        }
        if options.rebuild_batch_size < 100 {
            failures.push("Rebuild batch size must be at least 100".to_string());
        }
        if options.rebuild_batch_size > 1_000_000 {
            failures.push("Rebuild batch size must be at most 1,000,000".to_string());
        }
        
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructor_sets_default_values() {
        let options = ProjectionOptions::default();
        assert_eq!(options.polling_interval, Duration::from_secs(5));
        assert_eq!(options.batch_size, 1000);
        assert_eq!(options.rebuild_flush_interval, 10_000);
        assert_eq!(options.rebuild_batch_size, 5_000);
        assert_eq!(options.auto_rebuild, AutoRebuildMode::MissingCheckpointsOnly);
        assert_eq!(options.max_concurrent_rebuilds, 4);
    }

    #[test]
    fn polling_interval_can_be_set() {
        let mut options = ProjectionOptions::default();
        let interval = Duration::from_secs(10);
        options.polling_interval = interval;
        assert_eq!(options.polling_interval, interval);
    }

    #[test]
    fn batch_size_can_be_set() {
        let options = ProjectionOptions {
            batch_size: 500,
            ..Default::default()
        };
        assert_eq!(options.batch_size, 500);
    }

    #[test]
    fn auto_rebuild_can_be_set() {
        let options = ProjectionOptions {
            auto_rebuild: AutoRebuildMode::ForceFullRebuild,
            ..Default::default()
        };
        assert_eq!(options.auto_rebuild, AutoRebuildMode::ForceFullRebuild);
    }

    #[test]
    fn max_concurrent_rebuilds_can_be_configured() {
        for value in [1, 2, 4, 8, 16] {
            let options = ProjectionOptions {
                max_concurrent_rebuilds: value,
                ..Default::default()
            };
            assert_eq!(options.max_concurrent_rebuilds, value);
        }
    }

    #[test]
    fn max_concurrent_rebuilds_can_be_set_to_one_for_sequential_rebuild() {
        let options = ProjectionOptions {
            max_concurrent_rebuilds: 1,
            ..Default::default()
        };
        assert_eq!(options.max_concurrent_rebuilds, 1);
    }

    #[test]
    fn rebuild_flush_interval_can_be_configured() {
        for value in [100, 1_000, 10_000, 50_000, 1_000_000] {
            let options = ProjectionOptions {
                rebuild_flush_interval: value,
                ..Default::default()
            };
            assert_eq!(options.rebuild_flush_interval, value);
        }
    }

    #[test]
    fn validate_valid_options_returns_success() {
        let options = ProjectionOptions {
            polling_interval: Duration::from_secs(5),
            batch_size: 1000,
            max_concurrent_rebuilds: 4,
            auto_rebuild: AutoRebuildMode::MissingCheckpointsOnly,
            ..Default::default()
        };
        assert!(ProjectionOptionsValidator::validate(&options).is_ok());
    }

    #[test]
    fn validate_polling_interval_too_low_returns_fail() {
        for ms in [0, 50] {
            let options = ProjectionOptions {
                polling_interval: Duration::from_millis(ms),
                ..Default::default()
            };
            let result = ProjectionOptionsValidator::validate(&options);
            assert!(result.is_err());
            assert!(result.unwrap_err().iter().any(|f| f.contains("at least 100ms")));
        }
    }

    #[test]
    fn validate_polling_interval_too_high_returns_fail() {
        let options = ProjectionOptions {
            polling_interval: Duration::from_secs(7200),
            ..Default::default()
        };
        let result = ProjectionOptionsValidator::validate(&options);
        assert!(result.is_err());
        assert!(result.unwrap_err().iter().any(|f| f.contains("at most 1 hour")));
    }

    #[test]
    fn validate_polling_interval_valid_range_returns_success() {
        for ms in [100, 1000, 5000, 3600000] {
            let options = ProjectionOptions {
                polling_interval: Duration::from_millis(ms),
                ..Default::default()
            };
            assert!(ProjectionOptionsValidator::validate(&options).is_ok());
        }
    }

    #[test]
    fn validate_batch_size_too_low_returns_fail() {
        let options = ProjectionOptions {
            batch_size: 0,
            ..Default::default()
        };
        let result = ProjectionOptionsValidator::validate(&options);
        assert!(result.is_err());
        assert!(result.unwrap_err().iter().any(|f| f.contains("at least 1")));
    }

    #[test]
    fn validate_batch_size_too_high_returns_fail() {
        let options = ProjectionOptions {
            batch_size: 100001,
            ..Default::default()
        };
        let result = ProjectionOptionsValidator::validate(&options);
        assert!(result.is_err());
        assert!(result.unwrap_err().iter().any(|f| f.contains("at most 100,000")));
    }

    #[test]
    fn validate_batch_size_valid_range_returns_success() {
        for size in [1, 50, 1000, 10000, 100000] {
            let options = ProjectionOptions {
                batch_size: size,
                ..Default::default()
            };
            assert!(ProjectionOptionsValidator::validate(&options).is_ok());
        }
    }

    #[test]
    fn validate_max_concurrent_rebuilds_too_low_returns_fail() {
        let options = ProjectionOptions {
            max_concurrent_rebuilds: 0,
            ..Default::default()
        };
        let result = ProjectionOptionsValidator::validate(&options);
        assert!(result.is_err());
        assert!(result.unwrap_err().iter().any(|f| f.contains("at least 1")));
    }

    #[test]
    fn validate_max_concurrent_rebuilds_too_high_returns_fail() {
        let options = ProjectionOptions {
            max_concurrent_rebuilds: 65,
            ..Default::default()
        };
        let result = ProjectionOptionsValidator::validate(&options);
        assert!(result.is_err());
        assert!(result.unwrap_err().iter().any(|f| f.contains("at most 64")));
    }

    #[test]
    fn validate_multiple_invalid_values_returns_all_errors() {
        let options = ProjectionOptions {
            polling_interval: Duration::from_millis(50),
            batch_size: 0,
            max_concurrent_rebuilds: 100,
            ..Default::default()
        };
        let result = ProjectionOptionsValidator::validate(&options);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().len(), 3);
    }

    #[test]
    fn validate_rebuild_flush_interval_too_low_returns_fail() {
        for interval in [0, 99] {
            let options = ProjectionOptions {
                rebuild_flush_interval: interval,
                ..Default::default()
            };
            let result = ProjectionOptionsValidator::validate(&options);
            assert!(result.is_err());
            assert!(result.unwrap_err().iter().any(|f| f.contains("at least 100")));
        }
    }

    #[test]
    fn validate_rebuild_flush_interval_too_high_returns_fail() {
        let options = ProjectionOptions {
            rebuild_flush_interval: 1_000_001,
            ..Default::default()
        };
        let result = ProjectionOptionsValidator::validate(&options);
        assert!(result.is_err());
        assert!(result.unwrap_err().iter().any(|f| f.contains("at most 1,000,000")));
    }

    #[test]
    fn validate_rebuild_batch_size_too_low_returns_fail() {
        for size in [0, 99] {
            let options = ProjectionOptions {
                rebuild_batch_size: size,
                ..Default::default()
            };
            let result = ProjectionOptionsValidator::validate(&options);
            assert!(result.is_err());
            assert!(result.unwrap_err().iter().any(|f| f.contains("at least 100")));
        }
    }

    #[test]
    fn validate_rebuild_batch_size_too_high_returns_fail() {
        let options = ProjectionOptions {
            rebuild_batch_size: 1_000_001,
            ..Default::default()
        };
        let result = ProjectionOptionsValidator::validate(&options);
        assert!(result.is_err());
        assert!(result.unwrap_err().iter().any(|f| f.contains("at most 1,000,000")));
    }
}
