//! Shared fixtures: a source tree builder and a recording stand-in for rsync.
#![allow(dead_code)]

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

/// Write `contents` as a unix executable at `path`.
///
/// `fs::write` followed by `chmod` can fail the next `exec` with `ETXTBSY`
/// ("Text file busy") on Linux, especially under coverage where tests overlap.
/// Writing a sibling, fsyncing, then renaming onto `path` closes that race.
#[cfg(unix)]
fn write_unix_executable(path: &Path, contents: &str) {
    use std::os::unix::fs::OpenOptionsExt;

    let tmp = path.with_extension("writing");
    {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o755)
            .open(&tmp)
            .unwrap();
        file.write_all(contents.as_bytes()).unwrap();
        file.sync_all().unwrap();
    }
    std::fs::rename(tmp, path).unwrap();
}

/// A stub `rsync` that records the `--files-from` list of every invocation.
///
/// Recording the real argument vector is the only way to assert what the tool
/// actually asked rsync to move: assertions against the destination tree alone
/// cannot tell a batch that was never sent from one that rsync merged.
pub struct FakeRsync {
    _dir: TempDir,
    pub binary: PathBuf,
    pub records: PathBuf,
    pub live: PathBuf,
}

impl FakeRsync {
    /// Build a stub that always succeeds.
    pub fn new() -> Self {
        Self::with_behaviour(0, 0.0)
    }

    /// Build a stub that fails its first `fail_times` invocations, then
    /// succeeds, sleeping `sleep_secs` on every call.
    ///
    /// The failure counter is shared process-wide, so only use `fail_times`
    /// with serial transfers.
    pub fn with_behaviour(fail_times: u32, sleep_secs: f32) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let records = dir.path().join("records");
        let live = dir.path().join("live");
        std::fs::create_dir_all(&records).unwrap();
        std::fs::create_dir_all(&live).unwrap();

        let binary = dir.path().join("fake-rsync");
        let counter = dir.path().join("attempts");
        let script = format!(
            r#"#!/bin/sh
set -e
records='{records}'
live='{live}'
counter='{counter}'

# Pull the --files-from value out of the argument vector.
list=''
prev=''
for a in "$@"; do
  if [ "$prev" = '--files-from' ]; then list="$a"; fi
  prev="$a"
done

# Capability probes (`--version`, `--xattrs --version`) carry no --files-from.
# Recording them as transfers would corrupt every invocation count.
if [ -z "$list" ]; then
  echo 'rsync  version 3.4.1  protocol version 32'
  exit 0
fi

# Record the arguments and the file list, one file per invocation so that
# concurrent invocations cannot interleave their writes.
id="$$-$(od -An -N4 -tu4 /dev/urandom | tr -d ' ')"
printf '%s\n' "$@" > "$records/$id.args"
cp "$list" "$records/$id.list"

# Track how many invocations are in flight at once.
: > "$live/$id"
ls "$live" | wc -l | tr -d ' ' > "$records/$id.concurrency"

if [ '{sleep_secs}' != '0' ]; then sleep {sleep_secs}; fi
rm -f "$live/$id"

n=$(cat "$counter" 2>/dev/null || echo 0)
n=$((n + 1))
echo "$n" > "$counter"
if [ "$n" -le {fail_times} ]; then
  echo "fake-rsync: simulated failure $n" >&2
  exit 12
fi
exit 0
"#,
            records = records.display(),
            live = live.display(),
            counter = counter.display(),
            sleep_secs = sleep_secs,
            fail_times = fail_times,
        );
        write_unix_executable(&binary, &script);

        Self {
            _dir: dir,
            binary,
            records,
            live,
        }
    }

    pub fn binary(&self) -> String {
        self.binary.to_string_lossy().into_owned()
    }

    fn record_files(&self, extension: &str) -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = std::fs::read_dir(&self.records)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == extension))
            .collect();
        found.sort();
        found
    }

    /// How many times rsync was invoked.
    pub fn invocations(&self) -> usize {
        self.record_files("list").len()
    }

    /// The `--files-from` list of each invocation, in no particular order.
    pub fn batches(&self) -> Vec<Vec<String>> {
        self.record_files("list")
            .iter()
            .map(|p| {
                std::fs::read(p)
                    .unwrap()
                    .split(|b| *b == 0)
                    .filter(|s| !s.is_empty())
                    .map(|s| String::from_utf8_lossy(s).into_owned())
                    .collect()
            })
            .collect()
    }

    /// Every path handed to rsync across all invocations, deduplicated.
    pub fn all_paths(&self) -> BTreeSet<String> {
        self.batches().into_iter().flatten().collect()
    }

    /// Every path handed to rsync, including duplicates.
    pub fn all_paths_with_duplicates(&self) -> Vec<String> {
        self.batches().into_iter().flatten().collect()
    }

    /// The argument vector of the first invocation.
    pub fn first_args(&self) -> Vec<String> {
        let path = self
            .record_files("args")
            .into_iter()
            .next()
            .expect("no invocation recorded");
        std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// The greatest number of rsync processes observed running at once.
    pub fn peak_concurrency(&self) -> usize {
        self.record_files("concurrency")
            .iter()
            .filter_map(|p| std::fs::read_to_string(p).ok())
            .filter_map(|s| s.trim().parse::<usize>().ok())
            .max()
            .unwrap_or(0)
    }
}

/// Build a source tree and report what was created.
pub struct SourceTree {
    pub dir: TempDir,
    pub expected: BTreeSet<String>,
}

impl SourceTree {
    /// A fixed tree covering files, nesting, an empty directory, a space in a
    /// name, and (on unix) a symlink.
    pub fn fixture() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut expected = BTreeSet::new();

        let add_dir = |rel: &str, expected: &mut BTreeSet<String>| {
            std::fs::create_dir_all(root.join(rel)).unwrap();
            expected.insert(rel.to_string());
        };
        add_dir("nested", &mut expected);
        add_dir("nested/deep", &mut expected);
        add_dir("empty dir", &mut expected);

        for (rel, size) in [
            ("top.txt", 10usize),
            ("nested/mid.bin", 1_000),
            ("nested/deep/leaf.dat", 5_000),
            ("nested/deep/tiny", 1),
            ("a file with spaces.txt", 20),
        ] {
            std::fs::write(root.join(rel), vec![b'x'; size]).unwrap();
            expected.insert(rel.to_string());
        }

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("top.txt", root.join("link-to-top")).unwrap();
            expected.insert("link-to-top".to_string());
        }

        Self { dir, expected }
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// The source argument rsync expects: canonical, with a trailing slash.
    pub fn src_arg(&self) -> String {
        format!("{}/", self.dir.path().canonicalize().unwrap().display())
    }
}

/// Recursively list a tree relative to its root, like [`SourceTree::expected`].
pub fn list_tree(root: &Path) -> BTreeSet<String> {
    walkdir::WalkDir::new(root)
        .min_depth(1)
        .into_iter()
        .filter_map(Result::ok)
        .map(|e| {
            e.path()
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}
