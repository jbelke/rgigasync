use clap::{Arg, Command};
use walkdir::WalkDir;
use std::process::Command as ProcessCommand;
use tempfile::NamedTempFile;
use std::io::Write;
use std::fs::metadata;
use std::env;
use std::path::Path;

fn main() {
    let matches = Command::new("rgigasync")
        .version("1.0")
        .author("Your Name <youremail@example.com>")
        .about("Tool that enables rsync to mirror enormous directory trees")
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
        .get_matches();

    let rsync_options = matches.get_one::<String>("rsync-options").unwrap();
    let src_dir = std::fs::canonicalize(matches.get_one::<String>("src-dir").unwrap()).expect("Failed to resolve source directory").to_string_lossy().to_string();
    let target_dir = std::fs::canonicalize(matches.get_one::<String>("target-dir").unwrap()).expect("Failed to resolve target directory").to_string_lossy().to_string();
    let run_size_mb = matches.get_one::<String>("run-size-mb").unwrap();
    let run_size: u64 = run_size_mb.parse::<u64>().expect("Invalid run size") * 1024 * 1024;

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
    println!("  Command:");
    println!("    gigasync --run-size '{}' '{}' '{}'", run_size_mb, src_dir, target_dir);
    println!(" ");

    // Execute gigasync with the provided arguments
    env::set_var("RSYNC_OPTIONS", rsync_options);
    run_gigasync(run_size, &src_dir, &target_dir, rsync_options);
}

fn run_gigasync(run_size: u64, src_dir: &str, dest_dir: &str, rsync_options: &str) {
    let mut batch_file = NamedTempFile::new().expect("Failed to create temporary file");
    let mut batch_size = 0;

    for entry in WalkDir::new(src_dir) {
        let entry = entry.expect("Failed to access directory entry");
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        let rel_path = path.strip_prefix(src_dir).unwrap().to_string_lossy().to_string();
        let file_size = metadata(path).expect("Failed to get file metadata").len();

        if batch_size + file_size > run_size {
            run_rsync(&batch_file, src_dir, dest_dir, rsync_options);
            batch_file = NamedTempFile::new().expect("Failed to create temporary file");
            batch_size = 0;
        }

        writeln!(batch_file, "{}", rel_path).expect("Failed to write to temporary file");
        batch_size += file_size;
    }

    if batch_size > 0 {
        run_rsync(&batch_file, src_dir, dest_dir, rsync_options);
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

