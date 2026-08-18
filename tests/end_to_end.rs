//! Full mirrors driven by the real rsync, when one is available.

#![cfg(unix)]

mod support;

use std::time::Duration;

use rgigasync::batch::Limits;
use rgigasync::rsync::Rsync;
use rgigasync::scan::ScanFeedback;
use rgigasync::sync::{self, Job};

use support::{list_tree, SourceTree};

/// Locate a real rsync, or `None` if the platform has none.
fn real_rsync() -> Option<String> {
    let out = std::process::Command::new("sh")
        .arg("-c")
        .arg("command -v rsync")
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !path.is_empty()).then_some(path)
}

fn mirror(src: &str, dest: &str, binary: &str, max_bytes: u64, parallel: bool) -> sync::Summary {
    let job = Job {
        src: src.to_string(),
        dest: dest.to_string(),
        limits: Limits {
            max_bytes,
            max_entries: 0,
        },
        feedback: ScanFeedback::default(),
        rsync: Rsync {
            binary: binary.to_string(),
            options: vec!["-q".to_string()],
            max_attempts: 2,
            retry_delay: Duration::ZERO,
            dry_run: false,
        },
        parallel,
    };
    sync::run(&job, &|_| {}).expect("real rsync mirror should succeed")
}

#[test]
fn the_destination_tree_matches_the_source_tree() {
    let Some(rsync) = real_rsync() else {
        eprintln!("skipping: no rsync on PATH");
        return;
    };

    let tree = SourceTree::fixture();
    let dest = tempfile::tempdir().unwrap();
    let dest_arg = format!("{}/", dest.path().canonicalize().unwrap().display());

    // A tiny batch limit forces the tree across many rsync invocations, which
    // is the case a single-shot rsync would never exercise.
    mirror(&tree.src_arg(), &dest_arg, &rsync, 512, false);

    assert_eq!(list_tree(dest.path()), tree.expected);
}

#[test]
fn file_contents_survive_batching() {
    let Some(rsync) = real_rsync() else { return };

    let src = tempfile::tempdir().unwrap();
    for i in 0..25 {
        std::fs::write(src.path().join(format!("f{i}")), format!("contents of {i}")).unwrap();
    }
    let dest = tempfile::tempdir().unwrap();
    let src_arg = format!("{}/", src.path().canonicalize().unwrap().display());
    let dest_arg = format!("{}/", dest.path().canonicalize().unwrap().display());

    mirror(&src_arg, &dest_arg, &rsync, 16, false);

    for i in 0..25 {
        assert_eq!(
            std::fs::read_to_string(dest.path().join(format!("f{i}"))).unwrap(),
            format!("contents of {i}")
        );
    }
}

#[test]
fn an_empty_directory_is_recreated_at_the_destination() {
    let Some(rsync) = real_rsync() else { return };

    let src = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(src.path().join("a/b/empty")).unwrap();
    std::fs::write(src.path().join("a/file.txt"), b"x").unwrap();
    let dest = tempfile::tempdir().unwrap();
    let src_arg = format!("{}/", src.path().canonicalize().unwrap().display());
    let dest_arg = format!("{}/", dest.path().canonicalize().unwrap().display());

    mirror(&src_arg, &dest_arg, &rsync, 1 << 20, false);

    assert!(
        dest.path().join("a/b/empty").is_dir(),
        "an empty directory must survive the mirror"
    );
}

#[test]
fn a_symlink_is_recreated_as_a_symlink() {
    let Some(rsync) = real_rsync() else { return };

    let src = tempfile::tempdir().unwrap();
    std::fs::write(src.path().join("target.txt"), b"payload").unwrap();
    std::os::unix::fs::symlink("target.txt", src.path().join("link")).unwrap();
    let dest = tempfile::tempdir().unwrap();
    let src_arg = format!("{}/", src.path().canonicalize().unwrap().display());
    let dest_arg = format!("{}/", dest.path().canonicalize().unwrap().display());

    mirror(&src_arg, &dest_arg, &rsync, 1 << 20, false);

    let meta = std::fs::symlink_metadata(dest.path().join("link")).unwrap();
    assert!(meta.file_type().is_symlink(), "the link was dereferenced");
}

#[test]
fn a_filename_containing_a_space_round_trips() {
    let Some(rsync) = real_rsync() else { return };

    let src = tempfile::tempdir().unwrap();
    std::fs::write(src.path().join("a file with spaces.txt"), b"ok").unwrap();
    let dest = tempfile::tempdir().unwrap();
    let src_arg = format!("{}/", src.path().canonicalize().unwrap().display());
    let dest_arg = format!("{}/", dest.path().canonicalize().unwrap().display());

    mirror(&src_arg, &dest_arg, &rsync, 1 << 20, false);

    assert_eq!(
        std::fs::read_to_string(dest.path().join("a file with spaces.txt")).unwrap(),
        "ok"
    );
}

#[test]
fn a_filename_containing_a_newline_round_trips() {
    // The NUL-separated list plus --from0 exists for exactly this case; a
    // newline-separated list would split it into two bogus entries.
    let Some(rsync) = real_rsync() else { return };

    let src = tempfile::tempdir().unwrap();
    let weird = "line one\nline two.txt";
    if std::fs::write(src.path().join(weird), b"ok").is_err() {
        return; // some filesystems refuse the name
    }
    let dest = tempfile::tempdir().unwrap();
    let src_arg = format!("{}/", src.path().canonicalize().unwrap().display());
    let dest_arg = format!("{}/", dest.path().canonicalize().unwrap().display());

    mirror(&src_arg, &dest_arg, &rsync, 1 << 20, false);

    assert_eq!(list_tree(dest.path()), [weird.to_string()].into());
}

#[test]
fn parallel_batches_produce_the_same_tree_as_serial() {
    let Some(rsync) = real_rsync() else { return };

    let tree = SourceTree::fixture();
    let serial = tempfile::tempdir().unwrap();
    let parallel = tempfile::tempdir().unwrap();
    let serial_arg = format!("{}/", serial.path().canonicalize().unwrap().display());
    let parallel_arg = format!("{}/", parallel.path().canonicalize().unwrap().display());

    mirror(&tree.src_arg(), &serial_arg, &rsync, 512, false);
    mirror(&tree.src_arg(), &parallel_arg, &rsync, 512, true);

    assert_eq!(list_tree(serial.path()), list_tree(parallel.path()));
    assert_eq!(list_tree(parallel.path()), tree.expected);
}

#[test]
fn re_running_a_mirror_is_idempotent() {
    let Some(rsync) = real_rsync() else { return };

    let tree = SourceTree::fixture();
    let dest = tempfile::tempdir().unwrap();
    let dest_arg = format!("{}/", dest.path().canonicalize().unwrap().display());

    let first = mirror(&tree.src_arg(), &dest_arg, &rsync, 512, false);
    let after_first = list_tree(dest.path());
    let second = mirror(&tree.src_arg(), &dest_arg, &rsync, 512, false);

    assert_eq!(after_first, list_tree(dest.path()));
    assert_eq!(first.files, second.files);
    assert_eq!(first.bytes, second.bytes);
}
