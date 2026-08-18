//! Orchestration: scan, plan, transfer.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use rayon::prelude::*;

use crate::batch::{self, Batch, Limits};
use crate::error::Result;
use crate::rsync::Rsync;
use crate::scan::{self, EntryKind, ScanFeedback};

/// Everything the run needs, already validated.
#[derive(Debug)]
pub struct Job {
    /// Canonical source directory with a trailing slash.
    pub src: String,
    /// Destination as rsync should see it.
    pub dest: String,
    pub limits: Limits,
    pub feedback: ScanFeedback,
    pub rsync: Rsync,
    pub parallel: bool,
}

/// What a completed run moved.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Summary {
    pub files: usize,
    pub dirs: usize,
    pub symlinks: usize,
    pub bytes: u64,
    pub batches: usize,
    /// Entries the scan could not read; a non-empty list means a partial mirror.
    pub skipped: Vec<String>,
}

impl Summary {
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.skipped.is_empty()
    }
}

/// Scan the source, plan batches, and transfer them.
///
/// `report` receives human-readable progress; it is called from several
/// threads when `parallel` is set, hence the `Sync` bound.
pub fn run(job: &Job, report: &(dyn Fn(&str) + Sync)) -> Result<Summary> {
    let started = Instant::now();

    report("scanning source tree...");
    let scanned = scan::scan(
        std::path::Path::new(job.src.trim_end_matches('/')),
        job.feedback,
        |msg| report(msg),
    );

    let mut summary = Summary {
        files: scanned.count_of(EntryKind::File),
        dirs: scanned.count_of(EntryKind::Dir),
        symlinks: scanned.count_of(EntryKind::Symlink),
        bytes: scanned.total_bytes(),
        batches: 0,
        skipped: scanned.skipped,
    };

    let batches = batch::plan(scanned.entries, job.limits);
    summary.batches = batches.len();

    report(&format!(
        "{}, {}, {} ({}) in {}",
        plural(summary.files, "file"),
        plural(summary.dirs, "directory"),
        plural(summary.symlinks, "symlink"),
        human_bytes(summary.bytes),
        plural(summary.batches, "batch"),
    ));

    if batches.is_empty() {
        report("nothing to transfer");
        return Ok(summary);
    }

    let done = AtomicUsize::new(0);
    let total = batches.len();
    let transfer = |b: &Batch| -> Result<()> {
        job.rsync.transfer(b, &job.src, &job.dest)?;
        let n = done.fetch_add(1, Ordering::Relaxed) + 1;
        report(&format!(
            "batch {n}/{total} done ({}, {})",
            plural(b.len(), "entry"),
            human_bytes(b.bytes())
        ));
        Ok(())
    };

    if job.parallel {
        batches.par_iter().try_for_each(transfer)?;
    } else {
        batches.iter().try_for_each(transfer)?;
    }

    report(&format!(
        "transferred {} in {} ({})",
        human_bytes(summary.bytes),
        human_duration(started.elapsed()),
        plural(summary.batches, "batch"),
    ));

    Ok(summary)
}

/// `1 file` / `2 files`, with `-y` -> `-ies` for words like "directory".
fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        return format!("1 {noun}");
    }
    match noun.strip_suffix('y') {
        Some(stem) => format!("{count} {stem}ies"),
        None if noun.ends_with("ch") || noun.ends_with('s') => format!("{count} {noun}es"),
        None => format!("{count} {noun}s"),
    }
}

#[must_use]
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 7] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    #[allow(clippy::cast_precision_loss)]
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[must_use]
pub fn human_duration(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}h{m:02}m{s:02}s")
    } else if m > 0 {
        format!("{m}m{s:02}s")
    } else {
        format!("{:.1}s", elapsed.as_secs_f64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pluralisation_covers_the_nouns_actually_used() {
        assert_eq!(plural(1, "file"), "1 file");
        assert_eq!(plural(0, "file"), "0 files");
        assert_eq!(plural(2, "directory"), "2 directories");
        assert_eq!(plural(1, "directory"), "1 directory");
        assert_eq!(plural(3, "batch"), "3 batches");
        assert_eq!(plural(2, "entry"), "2 entries");
    }

    #[test]
    fn byte_sizes_scale_to_readable_units() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1024), "1.0 KiB");
        assert_eq!(human_bytes(1024 * 1024 * 3 / 2), "1.5 MiB");
        assert_eq!(
            human_bytes(u64::MAX),
            "16.0 EiB",
            "the table must not run out of units"
        );
    }

    #[test]
    fn durations_read_as_time_not_seconds() {
        assert_eq!(human_duration(Duration::from_secs_f64(1.26)), "1.3s");
        assert_eq!(human_duration(Duration::from_secs(75)), "1m15s");
        assert_eq!(human_duration(Duration::from_secs(3725)), "1h02m05s");
    }
}
