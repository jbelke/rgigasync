use dotenv::dotenv;
use std::env;

pub struct Config {
    pub num_threads: usize,
    pub file_feedback_count: u64,
    pub time_feedback_interval: u64,
}

impl Config {
    pub fn new() -> Self {
        dotenv().ok();

        let num_threads = env::var("RAYON_NUM_THREADS")
            .unwrap_or_else(|_| "0".to_string())
            .parse::<usize>()
            .unwrap_or(0);

        let file_feedback_count = env::var("FILE_FEEDBACK_COUNT")
            .unwrap_or_else(|_| "1000000".to_string())
            .parse::<u64>()
            .unwrap_or(1_000_000);

        let time_feedback_interval = env::var("TIME_FEEDBACK_INTERVAL")
            .unwrap_or_else(|_| "120".to_string())
            .parse::<u64>()
            .unwrap_or(120);

        Config {
            num_threads,
            file_feedback_count,
            time_feedback_interval,
        }
    }
}
