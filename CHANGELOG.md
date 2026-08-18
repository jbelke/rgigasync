# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-08-17

A correctness release. Several of the fixes below are silent-data-fidelity
bugs: the old code reported success while transferring less than it claimed.

### Fixed

- **A source tree containing only zero-byte files transferred nothing and
  exited 0.** The batch flush was gated on an accumulated byte count, and an
  empty file adds zero bytes, so neither the mid-walk flush nor the final flush
  ever fired. Reproduced deterministically against the previous release: five
  empty files produced zero rsync invocations; adding one non-empty byte
  produced one. Not a race — serial mode had it as badly as parallel.
- **Quoted rsync options were silently ignored.** Options were split on
  whitespace, so `--exclude='*.tmp'` reached rsync with the quotes attached,
  where it matched nothing and excluded nothing. Options are now split with
  POSIX shell rules.
- **Empty directories were never mirrored.** The walk kept only regular files,
  so a directory with no files in it simply did not exist at the destination.
  Directories and symlinks are now listed explicitly.
- **Symlinks were followed and copied as their targets**, duplicating whole
  subtrees and charging their bytes twice. They are now recorded as links.
- **`--parallel` ran nothing in parallel.** The batch mutex was held across the
  `rsync` call, so exactly one transfer ran at a time and every worker blocked
  on every file write — including through the 90-second retry sleeps. Batches
  now own their own file list and genuinely run concurrently.
- **A remote destination could never be reached.** `user@host:/path` was passed
  through `canonicalize()` and failed before rsync was invoked, despite being a
  documented example. Remote endpoints are now detected with rsync's own rules
  and passed through untouched.
- **Filenames containing newlines corrupted the transfer list.** The list is
  now NUL-separated and passed with `--from0`.
- **Filenames that are not valid UTF-8 were renamed at the destination.** Paths
  are carried as raw bytes rather than lossily converted to `String`.
- **A file vanishing mid-walk aborted the run** via `panic!` on `metadata()`.
  Unreadable entries are now skipped with a warning and reported in the exit
  code.
- **Failures panicked instead of reporting.** All errors are typed and mapped
  to documented exit codes.
- **`--version` reported a hardcoded `1.0`** regardless of the crate version.
- A single file larger than the batch size no longer produces a malformed
  batch; it gets a batch of its own.
- Batch accounting no longer resets a shared counter out from under concurrent
  writers, so batch sizes now actually respect the requested limit.
- Retries no longer sleep before a failure that cannot succeed. A missing rsync
  binary, and rsync's own deterministic exit codes (1 syntax/usage error,
  2 protocol incompatibility, 4 action not supported), are reported immediately
  rather than after five 90-second waits. A mistyped flag used to cost seven
  and a half minutes before saying so.

### Added

- `rgigasync` is now a library plus a thin binary, split into `scan`, `batch`,
  `rsync`, `location` and `sync` modules.
- Startup line naming the resolved rsync binary and its version, plus a
  capability probe that warns when the caller asks for metadata this rsync
  cannot carry — `warning: /usr/bin/rsync does not support --xattrs; that
  metadata will NOT be copied`. macOS ships openrsync as `/usr/bin/rsync`,
  which rejects `--xattrs` and `--acls`; the probe runs the binary rather than
  pattern-matching its version string, and understands bundled clusters such
  as `-aHAX`.
- `RGIGASYNC_RSYNC` selects which rsync to run.
- `--jobs`, `--max-files-per-batch`, `--dry-run`, `--retries`, `--retry-delay`
  and `--quiet` flags.
- Documented exit codes: `0` complete, `1` completed with skipped entries,
  `2` invalid arguments, `3` rsync failed after all retries.
- rsync exit code 24 ("some files vanished") is treated as success with a
  warning rather than triggering a retry storm.
- A missing local destination directory is created, matching plain rsync.
- Warning when the destination is nested inside the source.
- 92 tests: unit tests for batching, scanning, config and option handling;
  integration tests against a recording stub rsync; and end-to-end mirrors
  against a real rsync, verified against both GNU rsync 3.4.1 and openrsync.

### Changed

- **`--delete` and its variants are now rejected with a diagnostic instead of
  silently doing nothing.** `--files-from` disables the recursion rsync deletes
  from, so deletion flags parse cleanly, run without complaint, and remove
  nothing. Reproduced: plain `rsync -a --delete` removes a stale destination
  file; the batched form does not. Run a separate `rsync -a --delete` pass if
  you need pruning.
- Batches are planned up front from a completed scan rather than flushed from a
  shared buffer during the walk, which makes batch boundaries exact and
  reproducible.
- rsync options are now placed before the positional source and destination.
- CI runs fmt, clippy (`-D warnings`, with `clippy::pedantic`), tests on Linux
  and macOS, an MSRV build, and coverage — replacing two overlapping workflows.
- Configuration moved from `src/.env` to `.env.example` at the repository root.
  `.env` is read from the working directory, so the old location never loaded.

### Known limitations

- The scan materialises the whole tree before the first transfer starts, on the
  order of a gigabyte of RSS per 10 million entries. What is bounded is rsync's
  per-invocation file list, not rgigasync's own footprint.
- No benchmark yet shows batching beats a plain `rsync -aAX` on a tree that
  fits in memory. rsync 3.x has had incremental recursion enabled by default
  since 3.0.0 whenever both ends run 3.0.0 or newer, which narrows the memory
  argument considerably. Measure before assuming a speedup. The argument does
  still hold when incremental recursion is unavailable — notably with macOS's
  `/usr/bin/rsync`, which is openrsync 2.6.9-compat and therefore never
  negotiates it.
- A remote source cannot be batched, because the tree has to be enumerated
  locally.

### Removed

- `cobertura.xml`, a committed coverage artefact.
- The two placeholder tests, which asserted `2 + 2 == 4` and that clap parses a
  flag, while the coverage badge reported on them.

## [0.1.0]

Initial release.
