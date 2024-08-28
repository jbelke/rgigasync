mod config;
mod args;
mod file_processing;
mod rsync;

fn main() {
    let config = config::Config::new();
    let args = args::Args::parse();

    if config.num_threads > 0 {
        rayon::ThreadPoolBuilder::new().num_threads(config.num_threads).build_global().unwrap();
    }

    let src_dir = std::fs::canonicalize(&args.src_dir)
        .expect("Failed to resolve source directory")
        .to_string_lossy()
        .to_string();

    let target_dir = std::fs::canonicalize(&args.target_dir)
        .expect("Failed to resolve target directory")
        .to_string_lossy()
        .to_string();

    if !std::path::Path::new(&src_dir).is_dir() {
        eprintln!("Source directory invalid: {}", src_dir);
        std::process::exit(2);
    }

    if !std::path::Path::new(&target_dir).is_dir() {
        eprintln!("Target directory invalid: {}", target_dir);
        std::process::exit(2);
    }

    println!("Starting the process...");
    file_processing::process_files(
        args.run_size_mb, 
        &src_dir, 
        &target_dir, 
        &args.rsync_options, 
        args.enable_parallel, 
        config.file_feedback_count, 
        config.time_feedback_interval
    );
}
