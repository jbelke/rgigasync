use std::process::ExitCode;
use std::time::Duration;

use clap::Parser;

use rgigasync::batch::Limits;
use rgigasync::error::Error;
use rgigasync::location;
use rgigasync::rsync::{self, Rsync};
use rgigasync::scan::ScanFeedback;
use rgigasync::sync::{self, Job};
use rgigasync::{Args, Config, Summary};

fn main() -> ExitCode {
    let args = Args::parse();
    let config = Config::from_env();

    match run(&args, &config) {
        Ok(summary) if summary.is_complete() => ExitCode::SUCCESS,
        Ok(summary) => {
            eprintln!(
                "warning: {} entr(ies) were unreadable and did not transfer",
                summary.skipped.len()
            );
            ExitCode::from(1)
        }
        Err(err) => {
            eprintln!("error: {err}");
            let mut source = std::error::Error::source(&err);
            while let Some(cause) = source {
                eprintln!("  caused by: {cause}");
                source = cause.source();
            }
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            ExitCode::from(err.exit_code() as u8)
        }
    }
}

fn run(args: &Args, config: &Config) -> Result<Summary, Error> {
    let max_bytes = args.run_size_bytes().ok_or(if args.run_size_mb == 0 {
        Error::ZeroBatchSize
    } else {
        Error::BatchSizeOverflow(args.run_size_mb)
    })?;

    let options = rsync::split_options(&args.rsync_options)?;
    rsync::reject_delete_flags(&options)?;

    let (src_path, src) = location::resolve_source(&args.src_dir)?;
    let dest = location::resolve_destination(&args.target_dir)?;

    if let location::Location::Local(dest_path) = location::Location::parse(&dest) {
        if location::is_nested(&src_path, &dest_path) {
            eprintln!(
                "warning: destination {} is inside the source tree; rsync may copy its own output",
                dest_path.display()
            );
        }
    }

    let threads = args.jobs.unwrap_or(config.num_threads);
    if threads > 0 {
        // Failure here just means a pool already exists (e.g. under a test
        // harness); the default pool is a fine fallback.
        if let Err(err) = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build_global()
        {
            eprintln!("warning: using the default thread pool: {err}");
        }
    }

    let job = Job {
        src,
        dest,
        limits: Limits {
            max_bytes,
            max_entries: args.max_files_per_batch,
        },
        feedback: ScanFeedback {
            every_entries: config.file_feedback_count,
            every: config.time_feedback_interval,
        },
        rsync: Rsync {
            binary: config.rsync_binary.clone(),
            options,
            max_attempts: args.retries.unwrap_or(config.max_attempts).max(1),
            retry_delay: args
                .retry_delay
                .map_or(config.retry_delay, Duration::from_secs),
            dry_run: args.dry_run,
        },
        parallel: args.parallel,
    };

    if !args.quiet {
        eprintln!("using {}: {}", job.rsync.binary, job.rsync.version_banner());
    }
    for flag in job.rsync.unsupported_options() {
        eprintln!(
            "warning: {} does not support {flag}; that metadata will NOT be copied",
            job.rsync.binary
        );
    }

    let quiet = args.quiet;
    sync::run(&job, &move |line: &str| {
        if !quiet {
            eprintln!("{line}");
        }
    })
}
