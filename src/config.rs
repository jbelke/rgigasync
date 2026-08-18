//! Environment-derived defaults.
//!
//! Every value here is only a *default*: command-line flags win. Reading the
//! environment through an injected lookup keeps this unit-testable without
//! mutating the process environment, which is global state shared by every
//! test thread.

use std::time::Duration;

/// Defaults sourced from `.env` / the process environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Rayon worker count; `0` means "one per core".
    pub num_threads: usize,
    /// Emit a scan progress line every N entries.
    pub file_feedback_count: u64,
    /// Emit a scan progress line at least this often.
    pub time_feedback_interval: Duration,
    /// rsync executable to run.
    pub rsync_binary: String,
    /// Total attempts per batch, including the first.
    pub max_attempts: u32,
    /// Delay between attempts.
    pub retry_delay: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            num_threads: 0,
            file_feedback_count: 1_000_000,
            time_feedback_interval: Duration::from_secs(120),
            rsync_binary: "rsync".to_string(),
            max_attempts: 5,
            retry_delay: Duration::from_secs(90),
        }
    }
}

impl Config {
    /// Load `.env` (if present in the working directory) and read the environment.
    #[must_use]
    pub fn from_env() -> Self {
        let _ = dotenvy::dotenv();
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// Build a config from an arbitrary key lookup.
    #[must_use]
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let defaults = Self::default();
        let parse = |keys: &[&str]| -> Option<String> { keys.iter().find_map(|k| lookup(k)) };

        Self {
            num_threads: parse(&["RGIGASYNC_NUM_THREADS", "RAYON_NUM_THREADS"])
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.num_threads),
            file_feedback_count: parse(&["RGIGASYNC_FILE_FEEDBACK_COUNT", "FILE_FEEDBACK_COUNT"])
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.file_feedback_count),
            time_feedback_interval: parse(&[
                "RGIGASYNC_TIME_FEEDBACK_INTERVAL",
                "TIME_FEEDBACK_INTERVAL",
            ])
            .and_then(|v| v.parse().ok())
            .map_or(defaults.time_feedback_interval, Duration::from_secs),
            rsync_binary: parse(&["RGIGASYNC_RSYNC"]).unwrap_or(defaults.rsync_binary),
            max_attempts: parse(&["RGIGASYNC_RETRIES"])
                .and_then(|v| v.parse().ok())
                .filter(|n| *n > 0)
                .unwrap_or(defaults.max_attempts),
            retry_delay: parse(&["RGIGASYNC_RETRY_DELAY_SECS"])
                .and_then(|v| v.parse().ok())
                .map_or(defaults.retry_delay, Duration::from_secs),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn config_from(pairs: &[(&str, &str)]) -> Config {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        Config::from_lookup(move |k| map.get(k).cloned())
    }

    #[test]
    fn an_empty_environment_yields_the_documented_defaults() {
        assert_eq!(config_from(&[]), Config::default());
    }

    #[test]
    fn reads_every_setting() {
        let config = config_from(&[
            ("RGIGASYNC_NUM_THREADS", "8"),
            ("RGIGASYNC_FILE_FEEDBACK_COUNT", "500"),
            ("RGIGASYNC_TIME_FEEDBACK_INTERVAL", "30"),
            ("RGIGASYNC_RSYNC", "/opt/homebrew/bin/rsync"),
            ("RGIGASYNC_RETRIES", "3"),
            ("RGIGASYNC_RETRY_DELAY_SECS", "5"),
        ]);
        assert_eq!(
            config,
            Config {
                num_threads: 8,
                file_feedback_count: 500,
                time_feedback_interval: Duration::from_secs(30),
                rsync_binary: "/opt/homebrew/bin/rsync".to_string(),
                max_attempts: 3,
                retry_delay: Duration::from_secs(5),
            }
        );
    }

    #[test]
    fn legacy_unprefixed_names_still_work() {
        let config = config_from(&[
            ("RAYON_NUM_THREADS", "4"),
            ("FILE_FEEDBACK_COUNT", "7"),
            ("TIME_FEEDBACK_INTERVAL", "11"),
        ]);
        assert_eq!(config.num_threads, 4);
        assert_eq!(config.file_feedback_count, 7);
        assert_eq!(config.time_feedback_interval, Duration::from_secs(11));
    }

    #[test]
    fn the_prefixed_name_wins_over_the_legacy_one() {
        let config = config_from(&[("RGIGASYNC_NUM_THREADS", "2"), ("RAYON_NUM_THREADS", "9")]);
        assert_eq!(config.num_threads, 2);
    }

    #[test]
    fn garbage_values_fall_back_to_defaults_instead_of_aborting() {
        let config = config_from(&[
            ("RGIGASYNC_NUM_THREADS", "banana"),
            ("RGIGASYNC_RETRY_DELAY_SECS", "-1"),
        ]);
        assert_eq!(config.num_threads, Config::default().num_threads);
        assert_eq!(config.retry_delay, Config::default().retry_delay);
    }

    #[test]
    fn zero_retries_is_rejected_because_one_attempt_is_the_floor() {
        assert_eq!(
            config_from(&[("RGIGASYNC_RETRIES", "0")]).max_attempts,
            Config::default().max_attempts
        );
    }
}
