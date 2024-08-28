use clap::{Arg, Command};
use walkdir::WalkDir;
use rayon::prelude::*;
use rayon::ThreadPoolBuilder;
use std::process::Command as ProcessCommand;
use tempfile::NamedTempFile;
use std::io::Write;
use std::fs::metadata;
use std::env;
use dotenv::dotenv;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use std::thread;

fn main() {
    // Load .env vars
    dotenv().ok();

    let num_threads = env::var("RAYON_NUM_THREADS")
        .unwrap_or_else(|_| "0".to_string()) // Default to 0 if not set
        .parse::<usize>()
        .unwrap_or(0); // Default to 0 if parsing fails

    // Configure the number of threads for rayon
    if num_threads > 0 {
        ThreadPoolBuilder::new().num_threads(num_threads).build_global().unwrap();
    }

    let file_feedback_count = env::var("FILE_FEEDBACK_COUNT")
        .unwrap_or_else(|_| "1000000".to_string()) // Default to 1 million files
        .parse::<u64>()
        .unwrap_or(1_000_000); // Default to 1 million if parsing fails

    let time_feedback_interval = env::var("TIME_FEEDBACK_INTERVAL")
        .unwrap_or_else(|_| "120".to_string()) // Default to 120 seconds (2 minutes)
        .parse::<u64>()
        .unwrap_or(120); // Default to 2 minutes if parsing fails

    let matches = Command::new("rgigasync")
        .version("1.0")
        .author("Josh Belke <joshbelke@gmail.com>")
        .about("Tool that enables rsync to mirror enormous directory trees")
        .arg(Arg::new("parallel")
            .long("parallel")
            .help("Enable parallel processing for faster execution")
            .action(clap::ArgAction::SetTrue))  // Use SetTrue for a flag
        .arg(Arg::new("rsync-options")
            .help("Options to pass to rsync")
            .required(true)
            .index(1))
        .arg(Arg::new("src-dir")
            .help("Source directory")
            .required(true)
            .index(2))
        .arg(Arg::new("target-dir")
            .help("Target directory")
            .required(true)
            .index(3))
        .arg(Arg::new("run-size-mb")
            .help("Run size in megabytes")
            .required(false)
            .default_value("256")
            .index(4))
        .arg(Arg::new("parallel")
            .long("parallel")
            .help("Set the number of threads for parallel processing (0 for all available CPUs)")
            .value_parser(clap::value_parser!(usize))) // Use value_parser for arguments that take a value
        .get_matches();

    let rsync_options = matches.get_one::<String>("rsync-options").unwrap();
    let src_dir = std::fs::canonicalize(matches.get_one::<String>("src-dir").unwrap())
        .expect("Failed to resolve source directory")
        .to_string_lossy()
        .to_string();
    let target_dir = std::fs::canonicalize(matches.get_one::<String>("target-dir").unwrap())
        .expect("Failed to resolve target directory")
        .to_string_lossy()
        .to_string();
    let run_size_mb = matches.get_one::<String>("run-size-mb").unwrap();
    let run_size: u64 = run_size_mb.parse::<u64>().expect("Invalid run size") * 1024 * 1024;
    let enable_parallel = matches.get_flag("parallel");

    // Validate directories
    if !Path::new(&src_dir).is_dir() {
        eprintln!("Source directory invalid: {}", src_dir);
        std::process::exit(2);
    }
    if !Path::new(&target_dir).is_dir() {
        eprintln!("Target directory invalid: {}", target_dir);
        std::process::exit(2);
    }

    // Debug output
    println!("Backing up:");
    println!("  Source directory: {}/", src_dir);
    println!("  Target directory: {}/", target_dir);
    println!("  Rsync options:    {}", rsync_options);
    println!("  Run size in Mb:   {}", run_size_mb);
    println!("  Parallelization:  {}", enable_parallel);
    println!("  Command:");
    println!("    gigasync --run-size '{}' '{}' '{}'", run_size_mb, src_dir, target_dir);
    println!(" ");

    // Launch a background thread for the progress indicator
    let start_time = Instant::now();
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_secs(time_feedback_interval)); // Update every 2 minutes
            let elapsed = start_time.elapsed().as_secs();
            println!("Process running for {} minutes", elapsed / 60);
        }
    });

    // Execute gigasync with the provided arguments
    env::set_var("RSYNC_OPTIONS", rsync_options);
    run_gigasync(run_size, &src_dir, &target_dir, rsync_options, enable_parallel, file_feedback_count, time_feedback_interval);
}

fn run_gigasync(run_size: u64, src_dir: &str, dest_dir: &str, rsync_options: &str, parallel: bool, file_feedback_count: u64, time_feedback_interval: u64) {
    let batch_file = Arc::new(Mutex::new(NamedTempFile::new().expect("Failed to create temporary file")));
    let batch_size = Arc::new(AtomicU64::new(0));
    let file_count = Arc::new(AtomicU64::new(0));
    let start_time = Instant::now();

    let files: Vec<_> = WalkDir::new(src_dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
        .collect();

    if parallel {
        files.par_iter().for_each(|entry| {
            process_file(entry, src_dir, &batch_file, &batch_size, run_size, dest_dir, rsync_options, &file_count, &start_time, file_feedback_count, time_feedback_interval);
        });
    } else {
        files.iter().for_each(|entry| {
            process_file(entry, src_dir, &batch_file, &batch_size, run_size, dest_dir, rsync_options, &file_count, &start_time, file_feedback_count, time_feedback_interval);
        });
    }

    if batch_size.load(Ordering::SeqCst) > 0 {
        println!("Running final rsync for remaining files...");
        run_rsync(&batch_file.lock().unwrap(), src_dir, dest_dir, rsync_options);
    }
}

fn process_file(entry: &walkdir::DirEntry, src_dir: &str, batch_file: &Arc<Mutex<NamedTempFile>>, batch_size: &Arc<AtomicU64>, run_size: u64, dest_dir: &str, rsync_options: &str, file_count: &Arc<AtomicU64>, start_time: &Instant, file_feedback_count: u64, time_feedback_interval: u64) {
    let path = entry.path();
    let rel_path = path.strip_prefix(src_dir).unwrap().to_string_lossy().to_string();
    let file_size = metadata(path).expect("Failed to get file metadata").len();

    {
        let mut batch_file = batch_file.lock().expect("Failed to lock batch file");
        writeln!(batch_file, "{}", rel_path).expect("Failed to write to temporary file");
    }

    let current_size = batch_size.fetch_add(file_size, Ordering::SeqCst) + file_size;
    let current_file_count = file_count.fetch_add(1, Ordering::SeqCst) + 1;

    // Periodic feedback based on the .env settings
    if current_file_count % file_feedback_count == 0 || start_time.elapsed() >= Duration::from_secs(time_feedback_interval) {
        println!("Processed {} files so far...", current_file_count);
    }

    if current_size >= run_size {
        batch_size.store(0, Ordering::SeqCst);
        let mut batch_file = batch_file.lock().expect("Failed to lock batch file");
        println!("Running rsync for batch of {} files...", current_file_count);
        run_rsync(&batch_file, src_dir, dest_dir, rsync_options);
        *batch_file = NamedTempFile::new().expect("Failed to create temporary file");
    }
}

fn run_rsync(batch_file: &NamedTempFile, src_dir: &str, dest_dir: &str, rsync_options: &str) {
    let mut retries = 5;
    while retries > 0 {
        let status = ProcessCommand::new("rsync")
            .arg("-lptgoD")
            .arg("--no-implied-dirs")
            .arg("--files-from")
            .arg(batch_file.path())
            .arg(src_dir)
            .arg(dest_dir)
            .args(rsync_options.split_whitespace())
            .status()
            .expect("Failed to execute rsync");

        if status.success() {
            break;
        } else if retries == 1 {
            panic!("rsync failed after multiple retries");
        } else {
            eprintln!("rsync failed, retrying...");
            retries -= 1;
            std::thread::sleep(std::time::Duration::from_secs(90));
        }
    }
}
