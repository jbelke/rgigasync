use walkdir::WalkDir;
use rayon::prelude::*;
use tempfile::NamedTempFile;
use std::fs::metadata;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, Duration};  // Include `Duration` from `std::time`
use std::io::Write;  // Include `Write` trait for writing to the file

pub fn process_files(
    run_size: u64, 
    src_dir: &str, 
    dest_dir: &str, 
    rsync_options: &str, 
    parallel: bool, 
    file_feedback_count: u64, 
    time_feedback_interval: u64
) {
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
        crate::rsync::run_rsync(&batch_file.lock().unwrap(), src_dir, dest_dir, rsync_options);
    }
}

fn process_file(
    entry: &walkdir::DirEntry, 
    src_dir: &str, 
    batch_file: &Arc<Mutex<NamedTempFile>>, 
    batch_size: &Arc<AtomicU64>, 
    run_size: u64, 
    dest_dir: &str, 
    rsync_options: &str, 
    file_count: &Arc<AtomicU64>, 
    start_time: &Instant, 
    file_feedback_count: u64, 
    time_feedback_interval: u64
) {
    let path = entry.path();
    let rel_path = path.strip_prefix(src_dir).unwrap().to_string_lossy().to_string();
    let file_size = metadata(path).expect("Failed to get file metadata").len();

    {
        let mut batch_file = batch_file.lock().expect("Failed to lock batch file");
        writeln!(batch_file, "{}", rel_path).expect("Failed to write to temporary file");
    }

    let current_size = batch_size.fetch_add(file_size, Ordering::SeqCst) + file_size;
    let current_file_count = file_count.fetch_add(1, Ordering::SeqCst) + 1;

    // Periodic feedback
    if current_file_count % file_feedback_count == 0 || start_time.elapsed() >= Duration::from_secs(time_feedback_interval) {
        println!("Processed {} files so far...", current_file_count);
    }

    if current_size >= run_size {
        batch_size.store(0, Ordering::SeqCst);
        let mut batch_file = batch_file.lock().expect("Failed to lock batch file");
        println!("Running rsync for batch of {} files...", current_file_count);
        crate::rsync::run_rsync(&batch_file, src_dir, dest_dir, rsync_options);
        *batch_file = NamedTempFile::new().expect("Failed to create temporary file");
    }
}
