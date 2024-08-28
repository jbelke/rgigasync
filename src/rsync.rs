use std::process::Command as ProcessCommand;
use tempfile::NamedTempFile;

pub fn run_rsync(batch_file: &NamedTempFile, src_dir: &str, dest_dir: &str, rsync_options: &str) {
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
