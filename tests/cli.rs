//! The command-line surface: parsing, validation and exit codes.

#![cfg(unix)]

mod support;

use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;

use support::{FakeRsync, SourceTree};

fn rgigasync(fake: &FakeRsync) -> Command {
    let mut cmd = Command::cargo_bin("rgigasync").unwrap();
    // Point at the stub and neutralise any .env the developer happens to have.
    cmd.env("RGIGASYNC_RSYNC", fake.binary())
        .env("RGIGASYNC_RETRY_DELAY_SECS", "0")
        .env("RGIGASYNC_NUM_THREADS", "2");
    cmd
}

#[test]
fn version_reports_the_crate_version_not_a_hardcoded_string() {
    Command::cargo_bin("rgigasync")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn help_documents_the_positional_interface_and_exit_codes() {
    Command::cargo_bin("rgigasync")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("RSYNC_OPTIONS"))
        .stdout(contains("SRC_DIR"))
        .stdout(contains("TARGET_DIR"))
        .stdout(contains("EXIT CODES"));
}

#[test]
fn missing_arguments_are_a_usage_error() {
    Command::cargo_bin("rgigasync")
        .unwrap()
        .assert()
        .failure()
        .stderr(contains("required"));
}

#[test]
fn the_documented_invocation_works() {
    let tree = SourceTree::fixture();
    let dest = tempfile::tempdir().unwrap();
    let fake = FakeRsync::new();

    rgigasync(&fake)
        .arg("--")
        .arg("-av")
        .arg(tree.path())
        .arg(dest.path())
        .assert()
        .success();

    assert_eq!(fake.all_paths(), tree.expected);
}

#[test]
fn the_positional_batch_size_is_honoured() {
    let tree = SourceTree::fixture();
    let dest = tempfile::tempdir().unwrap();
    let fake = FakeRsync::new();

    // The fixture holds ~6 KiB, so 1 MiB is one batch...
    rgigasync(&fake)
        .args(["--", "-a"])
        .arg(tree.path())
        .arg(dest.path())
        .arg("1")
        .assert()
        .success();
    assert_eq!(fake.invocations(), 1);
}

#[test]
fn a_missing_source_exits_two_and_says_which_path() {
    let dest = tempfile::tempdir().unwrap();
    let fake = FakeRsync::new();

    rgigasync(&fake)
        .args(["--", "-a", "/no/such/source/tree"])
        .arg(dest.path())
        .assert()
        .code(2)
        .stderr(contains("/no/such/source/tree"));
}

#[test]
fn a_source_that_is_a_file_exits_two() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("regular.txt");
    std::fs::write(&file, b"x").unwrap();
    let dest = tempfile::tempdir().unwrap();
    let fake = FakeRsync::new();

    rgigasync(&fake)
        .args(["--", "-a"])
        .arg(&file)
        .arg(dest.path())
        .assert()
        .code(2);
}

#[test]
fn a_zero_batch_size_is_rejected_rather_than_dividing_the_tree_into_nothing() {
    let tree = SourceTree::fixture();
    let dest = tempfile::tempdir().unwrap();
    let fake = FakeRsync::new();

    rgigasync(&fake)
        .args(["--", "-a"])
        .arg(tree.path())
        .arg(dest.path())
        .arg("0")
        .assert()
        .code(2)
        .stderr(contains("at least 1 MiB"));
}

#[test]
fn an_unbalanced_quote_in_the_options_exits_two() {
    let tree = SourceTree::fixture();
    let dest = tempfile::tempdir().unwrap();
    let fake = FakeRsync::new();

    rgigasync(&fake)
        .args(["--", "-a --exclude='unterminated"])
        .arg(tree.path())
        .arg(dest.path())
        .assert()
        .code(2)
        .stderr(contains("rsync options"));
}

#[test]
fn rsync_failing_every_attempt_exits_three() {
    let tree = SourceTree::fixture();
    let dest = tempfile::tempdir().unwrap();
    let fake = FakeRsync::with_behaviour(100, 0.0);

    rgigasync(&fake)
        .args(["--retries", "2", "--"])
        .arg("-a")
        .arg(tree.path())
        .arg(dest.path())
        .assert()
        .code(3)
        .stderr(contains("rsync"));
    assert_eq!(fake.invocations(), 2);
}

#[test]
fn a_missing_local_destination_is_created() {
    let tree = SourceTree::fixture();
    let parent = tempfile::tempdir().unwrap();
    let dest = parent.path().join("does/not/exist/yet");
    let fake = FakeRsync::new();

    rgigasync(&fake)
        .args(["--", "-a"])
        .arg(tree.path())
        .arg(&dest)
        .assert()
        .success();

    assert!(dest.is_dir());
}

#[test]
fn a_remote_destination_is_passed_through_instead_of_being_resolved_locally() {
    // Canonicalising the destination is what used to make `user@host:/path`
    // fail before rsync was ever reached.
    let tree = SourceTree::fixture();
    let fake = FakeRsync::new();

    rgigasync(&fake)
        .args(["--", "-avz -e ssh"])
        .arg(tree.path())
        .arg("user@example.invalid:/home/user/dest/")
        .assert()
        .success();

    assert_eq!(
        fake.first_args().last().unwrap(),
        "user@example.invalid:/home/user/dest/"
    );
}

#[test]
fn dry_run_reports_the_command_without_running_it() {
    let tree = SourceTree::fixture();
    let dest = tempfile::tempdir().unwrap();
    let fake = FakeRsync::new();

    rgigasync(&fake)
        .args(["--dry-run", "--"])
        .arg("-a")
        .arg(tree.path())
        .arg(dest.path())
        .assert()
        .success()
        .stderr(contains("[dry-run]"));

    assert_eq!(fake.invocations(), 0);
}

#[test]
fn quiet_suppresses_progress_but_not_the_dry_run_plan() {
    let tree = SourceTree::fixture();
    let dest = tempfile::tempdir().unwrap();
    let fake = FakeRsync::new();

    let out = rgigasync(&fake)
        .args(["--quiet", "--"])
        .arg("-a")
        .arg(tree.path())
        .arg(dest.path())
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).into_owned();
    assert!(stderr.is_empty(), "expected silence, got: {stderr}");
}

#[test]
fn the_resolved_rsync_binary_is_named_on_startup() {
    // On a stock mac `rsync` is openrsync, which cannot carry xattrs; the
    // operator needs to see which binary they actually got.
    let tree = SourceTree::fixture();
    let dest = tempfile::tempdir().unwrap();
    let fake = FakeRsync::new();

    rgigasync(&fake)
        .args(["--", "-a"])
        .arg(tree.path())
        .arg(dest.path())
        .assert()
        .success()
        .stderr(contains("using ").and(contains(fake.binary())));
}

#[test]
fn delete_is_rejected_rather_than_silently_doing_nothing() {
    // rsync accepts --delete under --files-from and then deletes nothing,
    // because --files-from disables the recursion deletion walks. Advertising
    // a mirror that never prunes is worse than refusing the flag.
    let tree = SourceTree::fixture();
    let dest = tempfile::tempdir().unwrap();
    let fake = FakeRsync::new();

    rgigasync(&fake)
        .args(["--", "-av --delete"])
        .arg(tree.path())
        .arg(dest.path())
        .assert()
        .code(2)
        .stderr(contains("--delete").and(contains("--files-from")));

    assert_eq!(fake.invocations(), 0, "nothing should transfer");
}
