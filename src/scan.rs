//! Walking the source tree into a flat, batchable list of entries.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use walkdir::WalkDir;

/// What kind of thing an [`Entry`] points at.
///
/// The distinction matters because only regular files contribute bytes to a
/// batch, while directories and symlinks still have to be listed so that rsync
/// recreates them (an empty directory is invisible to a file-only listing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
}

/// One path to hand to rsync, relative to the source root.
///
/// The path is kept as a [`PathBuf`] rather than a `String` because filenames
/// on POSIX systems are arbitrary byte strings; lossy UTF-8 conversion would
/// silently rename files at the destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Path relative to the source root, as written into `--files-from`.
    pub rel_path: PathBuf,
    /// Size in bytes; always 0 for directories and symlinks.
    pub size: u64,
    pub kind: EntryKind,
}

/// Result of walking the source tree.
#[derive(Debug, Default)]
pub struct Scan {
    pub entries: Vec<Entry>,
    /// Entries that could not be read (vanished mid-walk, permission denied).
    pub skipped: Vec<String>,
}

impl Scan {
    #[must_use]
    pub fn total_bytes(&self) -> u64 {
        self.entries
            .iter()
            .fold(0u64, |acc, e| acc.saturating_add(e.size))
    }

    #[must_use]
    pub fn count_of(&self, kind: EntryKind) -> usize {
        self.entries.iter().filter(|e| e.kind == kind).count()
    }
}

/// How often the scan should report progress while walking.
#[derive(Debug, Clone, Copy)]
pub struct ScanFeedback {
    /// Emit a line every N entries. Zero disables count-based feedback.
    pub every_entries: u64,
    /// Emit a line at least this often. Zero disables time-based feedback.
    pub every: Duration,
}

impl Default for ScanFeedback {
    fn default() -> Self {
        Self {
            every_entries: 1_000_000,
            every: Duration::from_secs(120),
        }
    }
}

/// Walk `root` and collect every file, directory and symlink beneath it.
///
/// Symlinks are never followed: they are recorded as symlinks so rsync can
/// recreate the link itself rather than duplicating its target. Unreadable
/// entries are recorded in [`Scan::skipped`] instead of aborting the walk —
/// a single permission-denied subdirectory should not kill a multi-hour sync.
pub fn scan(root: &Path, feedback: ScanFeedback, mut report: impl FnMut(&str)) -> Scan {
    let start = Instant::now();
    let mut last_report = start;
    let mut scanned: u64 = 0;
    let mut out = Scan::default();

    for result in WalkDir::new(root).min_depth(1) {
        let entry = match result {
            Ok(entry) => entry,
            Err(err) => {
                let path = err
                    .path()
                    .map_or_else(|| "<unknown>".to_string(), |p| p.display().to_string());
                report(&format!("warning: skipping {path}: {err}"));
                out.skipped.push(path);
                continue;
            }
        };

        let Ok(rel_path) = entry.path().strip_prefix(root) else {
            // Unreachable for a WalkDir rooted at `root`, but a lost entry is
            // strictly worse than a warning.
            let path = entry.path().display().to_string();
            report(&format!("warning: {path} is outside the source root"));
            out.skipped.push(path);
            continue;
        };

        let file_type = entry.file_type();
        let (kind, size) = if file_type.is_dir() {
            (EntryKind::Dir, 0)
        } else if file_type.is_symlink() {
            (EntryKind::Symlink, 0)
        } else {
            // The file may vanish between the readdir and this stat; that is
            // routine on a live tree, so skip it rather than panic.
            match entry.metadata() {
                Ok(meta) => (EntryKind::File, meta.len()),
                Err(err) => {
                    let path = entry.path().display().to_string();
                    report(&format!("warning: skipping {path}: {err}"));
                    out.skipped.push(path);
                    continue;
                }
            }
        };

        out.entries.push(Entry {
            rel_path: rel_path.to_path_buf(),
            size,
            kind,
        });

        scanned += 1;
        let by_count = feedback.every_entries > 0 && scanned % feedback.every_entries == 0;
        let by_time = !feedback.every.is_zero() && last_report.elapsed() >= feedback.every;
        if by_count || by_time {
            last_report = Instant::now();
            report(&format!(
                "scanned {scanned} entries in {:.0}s...",
                start.elapsed().as_secs_f64()
            ));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn quiet() -> impl FnMut(&str) {
        |_: &str| {}
    }

    fn names(scan: &Scan) -> BTreeSet<String> {
        scan.entries
            .iter()
            .map(|e| e.rel_path.to_string_lossy().into_owned())
            .collect()
    }

    fn fixture() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("nested/deep")).unwrap();
        std::fs::create_dir_all(root.join("empty")).unwrap();
        std::fs::write(root.join("top.txt"), b"hello").unwrap();
        std::fs::write(root.join("nested/deep/leaf.bin"), vec![0u8; 128]).unwrap();
        tmp
    }

    #[test]
    fn records_files_dirs_and_sizes() {
        let tmp = fixture();
        let scan = scan(tmp.path(), ScanFeedback::default(), quiet());

        assert_eq!(
            names(&scan),
            [
                "empty",
                "nested",
                "nested/deep",
                "nested/deep/leaf.bin",
                "top.txt"
            ]
            .into_iter()
            .map(String::from)
            .collect()
        );
        assert_eq!(scan.count_of(EntryKind::File), 2);
        assert_eq!(scan.count_of(EntryKind::Dir), 3);
        assert_eq!(scan.total_bytes(), 5 + 128);
        assert!(scan.skipped.is_empty());
    }

    #[test]
    fn empty_directories_are_listed_so_the_mirror_can_recreate_them() {
        let tmp = fixture();
        let scan = scan(tmp.path(), ScanFeedback::default(), quiet());
        let empty = scan
            .entries
            .iter()
            .find(|e| e.rel_path == Path::new("empty"))
            .expect("an empty directory must still appear in the list");
        assert_eq!(empty.kind, EntryKind::Dir);
        assert_eq!(empty.size, 0);
    }

    #[test]
    fn the_root_itself_is_never_emitted() {
        let tmp = fixture();
        let scan = scan(tmp.path(), ScanFeedback::default(), quiet());
        assert!(scan
            .entries
            .iter()
            .all(|e| !e.rel_path.as_os_str().is_empty()));
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_recorded_as_links_and_not_followed() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir(root.join("real")).unwrap();
        std::fs::write(root.join("real/f.txt"), b"data").unwrap();
        std::os::unix::fs::symlink(root.join("real"), root.join("link")).unwrap();

        let scan = scan(root, ScanFeedback::default(), quiet());

        let link = scan
            .entries
            .iter()
            .find(|e| e.rel_path == Path::new("link"))
            .expect("the symlink itself must be listed");
        assert_eq!(link.kind, EntryKind::Symlink);
        assert_eq!(
            link.size, 0,
            "a link must not be charged its target's bytes"
        );
        assert!(
            !names(&scan).contains("link/f.txt"),
            "following the link would duplicate the tree"
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_subdirectory_is_skipped_rather_than_aborting_the_walk() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("readable.txt"), b"ok").unwrap();
        let locked = root.join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::write(locked.join("hidden.txt"), b"secret").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

        let mut warnings = Vec::new();
        let scan = scan(root, ScanFeedback::default(), |m| {
            warnings.push(m.to_string());
        });

        // Restore before the TempDir drop tries to clean up.
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();

        if scan.skipped.is_empty() {
            // Running as root defeats the permission bits entirely.
            return;
        }
        assert!(
            names(&scan).contains("readable.txt"),
            "the walk must continue"
        );
        assert!(warnings.iter().any(|w| w.contains("skipping")));
    }

    #[cfg(unix)]
    #[test]
    fn filenames_that_are_not_valid_utf8_survive_the_scan() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let tmp = tempfile::tempdir().unwrap();
        let raw = OsStr::from_bytes(b"caf\xe9.txt"); // latin-1, not UTF-8
        if std::fs::write(tmp.path().join(raw), b"x").is_err() {
            // APFS and other filesystems enforce valid UTF-8 filenames and
            // reject this with EILSEQ; there is nothing to assert there.
            return;
        }

        let scan = scan(tmp.path(), ScanFeedback::default(), quiet());

        assert_eq!(scan.entries.len(), 1);
        assert_eq!(
            scan.entries[0].rel_path.as_os_str().as_bytes(),
            b"caf\xe9.txt",
            "lossy conversion would rename the file at the destination"
        );
    }

    #[test]
    fn count_based_feedback_fires_on_the_configured_interval() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..10 {
            std::fs::write(tmp.path().join(format!("f{i}")), b"x").unwrap();
        }

        let mut lines = Vec::new();
        scan(
            tmp.path(),
            ScanFeedback {
                every_entries: 4,
                every: Duration::ZERO,
            },
            |m| lines.push(m.to_string()),
        );
        assert_eq!(lines.len(), 2, "10 entries at every-4 => 2 reports");
    }

    #[test]
    fn zero_feedback_settings_disable_reporting() {
        let tmp = fixture();
        let mut lines = Vec::new();
        scan(
            tmp.path(),
            ScanFeedback {
                every_entries: 0,
                every: Duration::ZERO,
            },
            |m| lines.push(m.to_string()),
        );
        assert!(lines.is_empty());
    }
}
