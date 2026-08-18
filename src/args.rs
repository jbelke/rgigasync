//! Command-line interface.

use clap::Parser;

const EXAMPLES: &str = "\
EXAMPLES:
    # Mirror verbosely
    rgigasync -- \"-av\" /Volumes/SrcDir/ /Users/me/DestDir/

    # 512 MiB batches, skipping files that already exist
    rgigasync -- \"-av --ignore-existing --info=progress2\" /src/ /dest/ 512

    # Quoted excludes are honoured (they are split with shell rules)
    rgigasync -- \"-av --exclude='*.tmp' --exclude='*.log'\" /src/ /dest/

    # Over ssh, to a remote destination
    rgigasync -- \"-avz -e ssh\" /src/ user@host:/home/user/dest/

    # Run four rsync transfers concurrently
    rgigasync --parallel --jobs 4 -- \"-a --info=progress2\" /src/ /dest/ 2048

EXIT CODES:
    0  everything transferred
    1  transfer finished but some entries were unreadable and skipped
    2  invalid arguments, source or destination
    3  rsync failed after all retries
";

/// Mirror enormous directory trees with rsync, one bounded batch at a time.
#[derive(Parser, Debug, Clone)]
#[command(name = "rgigasync", version, about, after_help = EXAMPLES)]
pub struct Args {
    /// Options forwarded to rsync, as a single quoted string.
    #[arg(allow_hyphen_values = true)]
    pub rsync_options: String,

    /// Source directory (must be local, so it can be enumerated).
    pub src_dir: String,

    /// Destination directory; may be `[user@]host:/path` or `rsync://…`.
    pub target_dir: String,

    /// Maximum size of each batch, in MiB.
    #[arg(default_value_t = 256, value_name = "MIB")]
    pub run_size_mb: u64,

    /// Run batches concurrently instead of one after another.
    #[arg(long)]
    pub parallel: bool,

    /// Concurrent rsync processes when --parallel is set (default: one per core).
    #[arg(long, value_name = "N")]
    pub jobs: Option<usize>,

    /// Cap entries per batch, bounding rsync's memory on trees of tiny files.
    #[arg(long, value_name = "N", default_value_t = 0)]
    pub max_files_per_batch: usize,

    /// Scan and plan batches, printing the rsync commands without running them.
    #[arg(long)]
    pub dry_run: bool,

    /// Attempts per batch, including the first.
    #[arg(long, value_name = "N")]
    pub retries: Option<u32>,

    /// Seconds to wait between attempts.
    #[arg(long, value_name = "SECONDS")]
    pub retry_delay: Option<u64>,

    /// Only report errors.
    #[arg(long, short)]
    pub quiet: bool,
}

impl Args {
    /// Batch size in bytes, or `None` if the MiB value is zero or overflows.
    #[must_use]
    pub fn run_size_bytes(&self) -> Option<u64> {
        if self.run_size_mb == 0 {
            return None;
        }
        self.run_size_mb.checked_mul(1024 * 1024)
    }
}
