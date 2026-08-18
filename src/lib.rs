//! `rgigasync` mirrors very large directory trees by driving `rsync` in
//! bounded batches.
//!
//! rsync builds its whole file list in memory before moving a byte, so a tree
//! with tens of millions of entries can exhaust RAM before the transfer even
//! starts, and any network hiccup throws away all of that work. `rgigasync`
//! enumerates the tree itself, splits it into batches bounded by size and
//! entry count, and feeds each batch to rsync through `--files-from`.
//!
//! The pipeline is three separable stages, each independently testable:
//!
//! 1. [`scan`] walks the source into a flat list of entries.
//! 2. [`batch`] groups that list into bounded [`batch::Batch`]es.
//! 3. [`rsync`] transfers one batch per rsync invocation, with retries.
//!
//! [`sync::run`] wires them together.
//!
//! ```no_run
//! use rgigasync::{batch::Limits, rsync::Rsync, scan::ScanFeedback, sync::{self, Job}};
//!
//! let job = Job {
//!     src: "/data/src/".into(),
//!     dest: "/data/dest/".into(),
//!     limits: Limits { max_bytes: 256 * 1024 * 1024, max_entries: 0 },
//!     feedback: ScanFeedback::default(),
//!     rsync: Rsync::default(),
//!     parallel: false,
//! };
//! let summary = sync::run(&job, &|line| eprintln!("{line}"))?;
//! assert!(summary.is_complete());
//! # Ok::<(), rgigasync::Error>(())
//! ```

pub mod args;
pub mod batch;
pub mod config;
pub mod error;
pub mod location;
pub mod rsync;
pub mod scan;
pub mod sync;

pub use args::Args;
pub use config::Config;
pub use error::{Error, Result};
pub use sync::Summary;
