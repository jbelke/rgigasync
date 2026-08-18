//! End-to-end batching behaviour, verified against a recording stub rsync.

#![cfg(unix)]

mod support;

use std::time::Duration;

use rgigasync::batch::Limits;
use rgigasync::rsync::Rsync;
use rgigasync::scan::ScanFeedback;
use rgigasync::sync::{self, Job, Summary};

use support::{FakeRsync, SourceTree};

fn job(src: &str, fake: &FakeRsync, max_bytes: u64, max_entries: usize, parallel: bool) -> Job {
    Job {
        src: src.to_string(),
        dest: "/dest/".to_string(),
        limits: Limits {
            max_bytes,
            max_entries,
        },
        feedback: ScanFeedback::default(),
        rsync: Rsync {
            binary: fake.binary(),
            options: vec!["-a".to_string()],
            max_attempts: 5,
            retry_delay: Duration::ZERO,
            dry_run: false,
        },
        parallel,
    }
}

fn run(job: &Job) -> Summary {
    sync::run(job, &|_| {}).expect("sync should succeed against the stub")
}

#[test]
fn every_entry_reaches_rsync_exactly_once() {
    let tree = SourceTree::fixture();
    let fake = FakeRsync::new();
    // A batch limit far below the tree size forces many batches.
    let summary = run(&job(&tree.src_arg(), &fake, 1_024, 0, false));

    assert_eq!(fake.all_paths(), tree.expected);
    let with_dupes = fake.all_paths_with_duplicates();
    assert_eq!(
        with_dupes.len(),
        tree.expected.len(),
        "no path may be sent twice: {with_dupes:?}"
    );
    assert!(summary.is_complete());
    assert_eq!(summary.batches, fake.invocations());
}

#[test]
fn empty_directories_are_included_so_the_mirror_is_faithful() {
    let tree = SourceTree::fixture();
    let fake = FakeRsync::new();
    run(&job(&tree.src_arg(), &fake, 1 << 30, 0, false));

    assert!(
        fake.all_paths().contains("empty dir"),
        "a file-only listing silently drops empty directories"
    );
}

#[test]
fn a_single_batch_is_used_when_the_tree_fits() {
    let tree = SourceTree::fixture();
    let fake = FakeRsync::new();
    let summary = run(&job(&tree.src_arg(), &fake, 1 << 30, 0, false));

    assert_eq!(summary.batches, 1);
    assert_eq!(fake.invocations(), 1);
}

#[test]
fn no_batch_exceeds_the_entry_limit() {
    let tree = SourceTree::fixture();
    let fake = FakeRsync::new();
    run(&job(&tree.src_arg(), &fake, 1 << 30, 2, false));

    for batch in fake.batches() {
        assert!(batch.len() <= 2, "batch of {} exceeds the cap", batch.len());
    }
    assert_eq!(fake.all_paths(), tree.expected);
}

#[test]
fn parallel_and_serial_transfer_exactly_the_same_set() {
    let tree = SourceTree::fixture();

    let serial_fake = FakeRsync::new();
    let serial = run(&job(&tree.src_arg(), &serial_fake, 2_048, 0, false));

    let parallel_fake = FakeRsync::new();
    let parallel = run(&job(&tree.src_arg(), &parallel_fake, 2_048, 0, true));

    assert_eq!(serial_fake.all_paths(), parallel_fake.all_paths());
    assert_eq!(serial.batches, parallel.batches);
    assert_eq!(serial.bytes, parallel.bytes);
    assert_eq!(
        parallel_fake.all_paths_with_duplicates().len(),
        tree.expected.len(),
        "concurrency must not duplicate work"
    );
}

#[test]
fn parallel_actually_overlaps_rsync_invocations() {
    // The point of --parallel is concurrent transfers. Holding a lock across
    // the rsync call would leave the peak at one and make the flag a no-op.
    let tree = SourceTree::fixture();
    let fake = FakeRsync::with_behaviour(0, 0.3);

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap();
    pool.install(|| run(&job(&tree.src_arg(), &fake, 1, 1, true)));

    assert!(fake.invocations() >= 4, "need several batches to overlap");
    assert!(
        fake.peak_concurrency() >= 2,
        "expected concurrent rsync processes, peak was {}",
        fake.peak_concurrency()
    );
}

#[test]
fn serial_never_overlaps() {
    let tree = SourceTree::fixture();
    let fake = FakeRsync::with_behaviour(0, 0.05);
    run(&job(&tree.src_arg(), &fake, 1, 1, false));

    assert_eq!(fake.peak_concurrency(), 1);
}

#[test]
fn a_failing_batch_is_retried_and_then_succeeds() {
    let tree = SourceTree::fixture();
    let fake = FakeRsync::with_behaviour(2, 0.0);
    let summary = run(&job(&tree.src_arg(), &fake, 1 << 30, 0, false));

    assert_eq!(summary.batches, 1);
    assert_eq!(fake.invocations(), 3, "two failures then one success");
}

#[test]
fn a_batch_that_never_succeeds_fails_the_run() {
    let tree = SourceTree::fixture();
    let fake = FakeRsync::with_behaviour(100, 0.0);
    let mut j = job(&tree.src_arg(), &fake, 1 << 30, 0, false);
    j.rsync.max_attempts = 3;

    let err = sync::run(&j, &|_| {}).expect_err("exhausted retries must surface");
    assert!(
        matches!(err, rgigasync::Error::RsyncFailed { attempts: 3, .. }),
        "unexpected error: {err}"
    );
    assert_eq!(fake.invocations(), 3);
}

#[test]
fn user_options_and_base_flags_both_reach_rsync() {
    let tree = SourceTree::fixture();
    let fake = FakeRsync::new();
    let mut j = job(&tree.src_arg(), &fake, 1 << 30, 0, false);
    j.rsync.options = rgigasync::rsync::split_options("-av --exclude='*.tmp'").unwrap();
    run(&j);

    let args = fake.first_args();
    for expected in ["-lptgoD", "--no-implied-dirs", "--from0", "-av"] {
        assert!(
            args.contains(&expected.to_string()),
            "missing {expected} in {args:?}"
        );
    }
    assert!(
        args.contains(&"--exclude=*.tmp".to_string()),
        "the glob must arrive unquoted: {args:?}"
    );
    assert_eq!(args.last().unwrap(), "/dest/");
}

#[test]
fn an_empty_source_transfers_nothing() {
    let empty = tempfile::tempdir().unwrap();
    let fake = FakeRsync::new();
    let src = format!("{}/", empty.path().canonicalize().unwrap().display());
    let summary = run(&job(&src, &fake, 1 << 30, 0, false));

    assert_eq!(summary.batches, 0);
    assert_eq!(fake.invocations(), 0, "an empty tree must not spawn rsync");
    assert_eq!(summary, Summary::default());
}

#[test]
fn the_summary_counts_match_the_tree() {
    let tree = SourceTree::fixture();
    let fake = FakeRsync::new();
    let summary = run(&job(&tree.src_arg(), &fake, 1 << 30, 0, false));

    assert_eq!(summary.files, 5);
    assert_eq!(summary.dirs, 3);
    assert_eq!(summary.symlinks, 1);
    assert_eq!(summary.bytes, 10 + 1_000 + 5_000 + 1 + 20);
    assert_eq!(
        summary.files + summary.dirs + summary.symlinks,
        tree.expected.len()
    );
}

#[test]
fn dry_run_plans_without_invoking_rsync() {
    let tree = SourceTree::fixture();
    let fake = FakeRsync::new();
    let mut j = job(&tree.src_arg(), &fake, 1_024, 0, false);
    j.rsync.dry_run = true;
    let summary = run(&j);

    assert!(summary.batches > 1, "planning still happens");
    assert_eq!(fake.invocations(), 0, "dry run must not execute anything");
}

#[test]
fn paths_are_relative_to_the_source_root() {
    let tree = SourceTree::fixture();
    let fake = FakeRsync::new();
    run(&job(&tree.src_arg(), &fake, 1 << 30, 0, false));

    let absolute: Vec<String> = fake
        .all_paths()
        .into_iter()
        .filter(|p| p.starts_with('/'))
        .collect();
    assert!(
        absolute.is_empty(),
        "--files-from entries must be relative: {absolute:?}"
    );
}

#[test]
fn a_tree_of_only_zero_byte_files_is_still_transferred() {
    // Regression: gating the final flush on an accumulated byte count means a
    // tree whose every file is empty accumulates nothing, transfers nothing,
    // and reports success.
    let dir = tempfile::tempdir().unwrap();
    for i in 0..5 {
        std::fs::write(dir.path().join(format!("empty{i}")), b"").unwrap();
    }
    let src = format!("{}/", dir.path().canonicalize().unwrap().display());

    let fake = FakeRsync::new();
    let summary = run(&job(&src, &fake, 256 * 1024 * 1024, 0, false));

    assert_eq!(summary.bytes, 0);
    assert_eq!(summary.files, 5);
    assert_eq!(fake.invocations(), 1, "zero bytes is not zero work");
    assert_eq!(fake.all_paths().len(), 5);
}

#[test]
fn a_tree_of_only_empty_directories_is_still_transferred() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("a/b/c")).unwrap();
    let src = format!("{}/", dir.path().canonicalize().unwrap().display());

    let fake = FakeRsync::new();
    let summary = run(&job(&src, &fake, 256 * 1024 * 1024, 0, false));

    assert_eq!(summary.dirs, 3);
    assert_eq!(
        fake.all_paths(),
        ["a", "a/b", "a/b/c"].map(String::from).into()
    );
}
