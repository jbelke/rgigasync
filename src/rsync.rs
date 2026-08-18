//! Invoking rsync, with retries and POSIX-correct option handling.

use std::ffi::OsString;
use std::io::Write;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::time::Duration;

use tempfile::NamedTempFile;

use crate::batch::Batch;
use crate::error::{Error, Result};

/// Flags `rgigasync` always passes.
///
/// `-lptgoD` is `-a` minus `-r`: recursion is our job, not rsync's, since we
/// hand it an explicit list. `--no-implied-dirs` stops rsync from re-sending
/// attributes for every parent directory of every file in every batch, and
/// `--from0` lets the file list carry filenames containing newlines.
const BASE_ARGS: &[&str] = &["-lptgoD", "--no-implied-dirs", "--from0"];

/// rsync's "some files vanished before they could be transferred" exit code.
/// Routine on a live tree and not a reason to retry or fail the run.
const EXIT_VANISHED: i32 = 24;

/// Exit codes that will never succeed on a second attempt.
///
/// Retrying these is not merely useless, it is actively harmful: a misspelled
/// option would otherwise sit through five 90-second sleeps before reporting
/// the typo. Retries exist for flaky networks, not for bad arguments.
const FATAL_EXITS: &[(i32, &str)] = &[
    (1, "syntax or usage error"),
    (2, "protocol incompatibility"),
    (4, "requested action not supported"),
];

/// A configured rsync invoker.
#[derive(Debug, Clone)]
pub struct Rsync {
    /// Binary to execute. Overridable so tests can inject a stub and operators
    /// can point at a newer rsync than the one on `PATH`.
    pub binary: String,
    /// User-supplied options, already split into argv words.
    pub options: Vec<String>,
    pub max_attempts: u32,
    pub retry_delay: Duration,
    /// Print the command instead of running it.
    pub dry_run: bool,
}

impl Default for Rsync {
    fn default() -> Self {
        Self {
            binary: "rsync".to_string(),
            options: Vec::new(),
            max_attempts: 5,
            retry_delay: Duration::from_secs(90),
            dry_run: false,
        }
    }
}

/// Split a user-supplied rsync option string into argv words.
///
/// This is `shell_words`, not `split_whitespace`: the latter turns
/// `--exclude='*.tmp'` into a single argument with literal quotes, which rsync
/// accepts and then silently fails to match anything against.
pub fn split_options(input: &str) -> Result<Vec<String>> {
    shell_words::split(input).map_err(|source| Error::RsyncOptions {
        input: input.to_string(),
        source,
    })
}

/// Deletion flags, none of which rsync can honour under `--files-from`.
///
/// `--files-from` turns off recursion, and rsync only deletes inside the
/// directories it recurses into, so every one of these parses cleanly, runs
/// without complaint, and removes nothing. Accepting them would advertise a
/// mirror guarantee the tool cannot keep.
const DELETE_FLAGS: &[&str] = &[
    "--delete",
    "--del",
    "--delete-before",
    "--delete-during",
    "--delete-delay",
    "--delete-after",
    "--delete-excluded",
    "--delete-missing-args",
];

/// Reject deletion flags rather than ignoring them.
pub fn reject_delete_flags(options: &[String]) -> Result<()> {
    for opt in options {
        // Match the bare flag and any `--delete-during=…` style spelling.
        let name = opt.split('=').next().unwrap_or(opt);
        if let Some(found) = DELETE_FLAGS.iter().find(|f| **f == name) {
            return Err(Error::DeleteUnsupported((*found).to_string()));
        }
    }
    Ok(())
}

/// Flags that silently change what metadata survives a transfer.
///
/// `rsync` on `PATH` is not one program: macOS ships openrsync, which reports
/// itself as "2.6.9 compatible" and rejects `--xattrs` and `--acls` outright.
/// Asking for `-aHAX` there does not fail loudly, it just produces a copy that
/// has quietly lost its extended attributes — the worst possible outcome for
/// something whose whole job is fidelity.
const FIDELITY_FLAGS: &[(&str, Option<char>)] = &[
    ("--xattrs", Some('X')),
    ("--acls", Some('A')),
    ("--hard-links", Some('H')),
    ("--fileflags", None),
    ("--crtimes", None),
];

impl Rsync {
    /// First line of `rsync --version`, or a description of why it could not run.
    #[must_use]
    pub fn version_banner(&self) -> String {
        match Command::new(&self.binary).arg("--version").output() {
            Ok(out) => String::from_utf8_lossy(&out.stdout)
                .lines()
                .next()
                .unwrap_or("(no version output)")
                .trim()
                .to_string(),
            Err(err) => format!("(could not run {}: {err})", self.binary),
        }
    }

    /// Fidelity flags the caller asked for that this rsync does not implement.
    ///
    /// Probing beats parsing the version string: it asks the actual binary.
    #[must_use]
    pub fn unsupported_options(&self) -> Vec<&'static str> {
        FIDELITY_FLAGS
            .iter()
            .filter(|(long, short)| self.requests(long, *short))
            .filter(|(long, _)| !self.accepts(long))
            .map(|(long, _)| *long)
            .collect()
    }

    /// Whether the user's options ask for `long`, either spelled out or bundled
    /// into a short cluster such as `-aHAX`.
    fn requests(&self, long: &str, short: Option<char>) -> bool {
        self.options.iter().any(|opt| {
            if opt == long {
                return true;
            }
            match short {
                Some(c) => opt.starts_with('-') && !opt.starts_with("--") && opt.contains(c),
                None => false,
            }
        })
    }

    fn accepts(&self, flag: &str) -> bool {
        Command::new(&self.binary)
            .arg(flag)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    /// Write `batch` to a NUL-separated temporary list and transfer it.
    pub fn transfer(&self, batch: &Batch, src: &str, dest: &str) -> Result<()> {
        let list = write_file_list(batch)?;
        self.run_with_list(list.path(), src, dest)
    }

    /// Run rsync against an existing `--files-from` list, retrying on failure.
    pub fn run_with_list(&self, list: &Path, src: &str, dest: &str) -> Result<()> {
        if self.dry_run {
            eprintln!("[dry-run] would run: {}", self.render(list, src, dest));
            return Ok(());
        }

        let mut attempt = 1;
        loop {
            let status = self.spawn(list, src, dest)?;

            if status.success() {
                return Ok(());
            }
            if status.code() == Some(EXIT_VANISHED) {
                eprintln!("rsync: some files vanished during transfer (exit 24); continuing");
                return Ok(());
            }

            if let Some((_, reason)) = status
                .code()
                .and_then(|c| FATAL_EXITS.iter().find(|(code, _)| *code == c))
            {
                return Err(Error::RsyncFailed {
                    status: format!("{} ({reason}; not retryable)", describe(status)),
                    attempts: attempt,
                });
            }

            if attempt >= self.max_attempts {
                return Err(Error::RsyncFailed {
                    status: describe(status),
                    attempts: attempt,
                });
            }

            eprintln!(
                "rsync {} (attempt {attempt}/{}); retrying in {}s",
                describe(status),
                self.max_attempts,
                self.retry_delay.as_secs()
            );
            std::thread::sleep(self.retry_delay);
            attempt += 1;
        }
    }

    fn spawn(&self, list: &Path, src: &str, dest: &str) -> Result<ExitStatus> {
        Command::new(&self.binary)
            .args(self.argv(list, src, dest))
            .stdin(Stdio::null())
            .status()
            .map_err(|source| Error::RsyncSpawn {
                binary: self.binary.clone(),
                source,
            })
    }

    /// The full argument vector, options first so they apply to the positional
    /// source and destination that follow.
    fn argv(&self, list: &Path, src: &str, dest: &str) -> Vec<OsString> {
        let mut argv: Vec<OsString> = BASE_ARGS.iter().map(OsString::from).collect();
        argv.extend(self.options.iter().map(OsString::from));
        argv.push(OsString::from("--files-from"));
        argv.push(list.as_os_str().to_os_string());
        argv.push(OsString::from(src));
        argv.push(OsString::from(dest));
        argv
    }

    fn render(&self, list: &Path, src: &str, dest: &str) -> String {
        let words = std::iter::once(self.binary.clone())
            .chain(
                self.argv(list, src, dest)
                    .iter()
                    .map(|a| a.to_string_lossy().into_owned()),
            )
            .collect::<Vec<_>>();
        shell_words::join(words.iter().map(String::as_str))
    }
}

fn describe(status: ExitStatus) -> String {
    status.code().map_or_else(
        || "was killed by a signal".to_string(),
        |c| format!("exited with code {c}"),
    )
}

/// Serialise a batch into a NUL-separated `--files-from` list.
///
/// Paths are written as raw bytes on Unix so that filenames which are not
/// valid UTF-8 survive the round trip intact.
fn write_file_list(batch: &Batch) -> Result<NamedTempFile> {
    let mut file = NamedTempFile::new().map_err(|source| Error::Io {
        path: std::env::temp_dir(),
        source,
    })?;

    let path = file.path().to_path_buf();
    let mut buf = Vec::with_capacity(batch.len() * 64);
    for entry in &batch.entries {
        buf.extend_from_slice(&path_bytes(&entry.rel_path));
        buf.push(0);
    }

    let handle = file.as_file_mut();
    handle
        .write_all(&buf)
        .and_then(|()| handle.flush())
        .map_err(|source| Error::Io {
            path: path.clone(),
            source,
        })?;

    Ok(file)
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
fn path_bytes(path: &Path) -> Vec<u8> {
    path.to_string_lossy().into_owned().into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::{Entry, EntryKind};
    use std::path::PathBuf;

    fn batch(names: &[&str]) -> Batch {
        Batch {
            entries: names
                .iter()
                .map(|n| Entry {
                    rel_path: PathBuf::from(n),
                    size: 0,
                    kind: EntryKind::File,
                })
                .collect(),
        }
    }

    #[test]
    fn quoted_globs_survive_splitting() {
        // `split_whitespace` produces `--exclude='*.tmp'` with the quotes still
        // attached, which rsync happily accepts and then matches nothing with.
        assert_eq!(
            split_options("-av --exclude='*.tmp' --exclude=\"*.log\"").unwrap(),
            vec!["-av", "--exclude=*.tmp", "--exclude=*.log"]
        );
    }

    #[test]
    fn quoted_paths_containing_spaces_stay_one_argument() {
        assert_eq!(
            split_options("-e 'ssh -i /my key/id_rsa'").unwrap(),
            vec!["-e", "ssh -i /my key/id_rsa"]
        );
    }

    #[test]
    fn an_empty_option_string_produces_no_arguments() {
        assert!(split_options("").unwrap().is_empty());
        assert!(split_options("   ").unwrap().is_empty());
    }

    #[test]
    fn deletion_flags_are_rejected_because_they_would_be_silently_ignored() {
        for flag in [
            "--delete",
            "--del",
            "--delete-before",
            "--delete-during",
            "--delete-delay",
            "--delete-after",
            "--delete-excluded",
        ] {
            let options = split_options(&format!("-av {flag}")).unwrap();
            assert!(
                matches!(
                    reject_delete_flags(&options),
                    Err(Error::DeleteUnsupported(_))
                ),
                "{flag} should be rejected"
            );
        }
    }

    #[test]
    fn the_rejection_names_the_flag_and_explains_why() {
        let options = split_options("-av --delete").unwrap();
        let message = reject_delete_flags(&options).unwrap_err().to_string();
        assert!(message.contains("--delete"));
        assert!(message.contains("--files-from"));
        assert!(
            message.contains("rsync -a --delete"),
            "must say what to do instead"
        );
    }

    #[test]
    fn options_that_merely_look_like_deletion_flags_are_allowed() {
        // `--delete-excluded` is real, but these are not deletion requests.
        let options = split_options("-av --exclude=delete-me --info=del2").unwrap();
        assert!(reject_delete_flags(&options).is_ok());
    }

    #[test]
    fn an_unbalanced_quote_is_reported_rather_than_silently_mangled() {
        assert!(matches!(
            split_options("-av --exclude='oops"),
            Err(Error::RsyncOptions { .. })
        ));
    }

    #[test]
    fn the_file_list_is_nul_separated_raw_bytes() {
        let file = write_file_list(&batch(&["a.txt", "dir/b with space.txt"])).unwrap();
        let bytes = std::fs::read(file.path()).unwrap();
        assert_eq!(bytes, b"a.txt\0dir/b with space.txt\0");
    }

    #[test]
    fn a_filename_containing_a_newline_is_not_split_into_two_entries() {
        // This is exactly why the list is NUL-separated and --from0 is passed.
        let file = write_file_list(&batch(&["weird\nname.txt"])).unwrap();
        let bytes = std::fs::read(file.path()).unwrap();
        assert_eq!(bytes, b"weird\nname.txt\0");

        let entries: Vec<&[u8]> = bytes.split(|b| *b == 0).filter(|s| !s.is_empty()).collect();
        assert_eq!(
            entries,
            vec![&b"weird\nname.txt"[..]],
            "the newline must not act as a separator"
        );
    }

    #[test]
    fn user_options_precede_the_positional_source_and_destination() {
        let rsync = Rsync {
            options: vec!["-av".into(), "--delete".into()],
            ..Rsync::default()
        };
        let argv: Vec<String> = rsync
            .argv(Path::new("/tmp/list"), "/src/", "/dest/")
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();

        assert_eq!(
            argv,
            vec![
                "-lptgoD",
                "--no-implied-dirs",
                "--from0",
                "-av",
                "--delete",
                "--files-from",
                "/tmp/list",
                "/src/",
                "/dest/"
            ]
        );
    }

    #[test]
    fn a_missing_rsync_binary_is_a_spawn_error_not_a_panic() {
        let rsync = Rsync {
            binary: "/nonexistent/rsync-does-not-exist".to_string(),
            max_attempts: 1,
            retry_delay: Duration::ZERO,
            ..Rsync::default()
        };
        assert!(matches!(
            rsync.transfer(&batch(&["a"]), "/src/", "/dest/"),
            Err(Error::RsyncSpawn { .. })
        ));
    }

    #[test]
    fn a_usage_error_is_not_retried() {
        // exit 1 means rsync rejected the arguments; five 90-second sleeps
        // will not make a misspelled flag valid.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("always-usage-error");
        std::fs::write(&path, "#!/bin/sh\nexit 1\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();

        let rsync = Rsync {
            binary: path.to_string_lossy().into_owned(),
            max_attempts: 5,
            retry_delay: Duration::from_secs(90),
            ..Rsync::default()
        };

        let started = std::time::Instant::now();
        let err = rsync
            .transfer(&batch(&["a"]), "/src/", "/dest/")
            .unwrap_err();

        assert!(started.elapsed() < Duration::from_secs(5), "it slept");
        match err {
            Error::RsyncFailed { attempts, status } => {
                assert_eq!(attempts, 1, "a usage error must fail on the first attempt");
                assert!(status.contains("not retryable"), "{status}");
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn dry_run_never_spawns_anything() {
        let rsync = Rsync {
            binary: "/nonexistent/rsync-does-not-exist".to_string(),
            dry_run: true,
            ..Rsync::default()
        };
        assert!(rsync.transfer(&batch(&["a"]), "/src/", "/dest/").is_ok());
    }
}

#[cfg(all(test, unix))]
mod capability_tests {
    use super::*;

    /// Build a stub that accepts every flag except those in `rejects`.
    fn stub(rejects: &[&str]) -> (tempfile::TempDir, String) {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("stub-rsync");
        let cases = rejects
            .iter()
            .map(|f| format!("    {f}) exit 1 ;;"))
            .collect::<Vec<_>>()
            .join("\n");
        let script = format!(
            "#!/bin/sh\necho 'rsync version 9.9.9 protocol version 99'\nfor a in \"$@\"; do\n  case \"$a\" in\n{cases}\n  esac\ndone\nexit 0\n"
        );
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let binary = path.to_string_lossy().into_owned();
        (dir, binary)
    }

    fn rsync(binary: &str, options: &[&str]) -> Rsync {
        Rsync {
            binary: binary.to_string(),
            options: options.iter().map(|s| (*s).to_string()).collect(),
            ..Rsync::default()
        }
    }

    #[test]
    fn reports_the_version_of_the_binary_it_will_actually_run() {
        let (_dir, binary) = stub(&[]);
        assert_eq!(
            rsync(&binary, &[]).version_banner(),
            "rsync version 9.9.9 protocol version 99"
        );
    }

    #[test]
    fn a_missing_binary_yields_a_banner_instead_of_a_panic() {
        assert!(rsync("/no/such/rsync", &[])
            .version_banner()
            .contains("could not run"));
    }

    #[test]
    fn flags_the_binary_rejects_are_reported() {
        let (_dir, binary) = stub(&["--xattrs", "--acls"]);
        assert_eq!(
            rsync(&binary, &["--xattrs", "--acls", "--hard-links"]).unsupported_options(),
            vec!["--xattrs", "--acls"]
        );
    }

    #[test]
    fn a_bundled_short_cluster_is_recognised() {
        // `-aHAX` asks for hard links, ACLs and xattrs without ever spelling
        // them out; missing this is how the warning gets skipped in practice.
        let (_dir, binary) = stub(&["--xattrs", "--acls"]);
        let unsupported = rsync(&binary, &["-aHAX"]).unsupported_options();
        assert!(unsupported.contains(&"--xattrs"));
        assert!(unsupported.contains(&"--acls"));
        assert!(
            !unsupported.contains(&"--hard-links"),
            "the stub accepts -H"
        );
    }

    #[test]
    fn a_long_option_is_not_mistaken_for_a_short_cluster() {
        // `--exclude=X.tmp` contains an 'X' but is not asking for xattrs.
        let (_dir, binary) = stub(&["--xattrs"]);
        assert!(rsync(&binary, &["-av", "--exclude=X.tmp"])
            .unsupported_options()
            .is_empty());
    }

    #[test]
    fn nothing_is_reported_when_the_caller_asks_for_nothing_special() {
        let (_dir, binary) = stub(&["--xattrs", "--acls", "--fileflags"]);
        assert!(rsync(&binary, &["-av"]).unsupported_options().is_empty());
    }
}
