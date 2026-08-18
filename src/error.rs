//! Error type for every fallible operation in the crate.

use std::path::PathBuf;

/// Everything that can go wrong during a sync.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("source directory does not exist or is not a directory: {0}")]
    SourceNotADirectory(PathBuf),

    #[error("destination exists but is not a directory: {0}")]
    DestinationNotADirectory(PathBuf),

    #[error("failed to resolve {path}: {source}")]
    ResolvePath {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("batch size must be at least 1 MiB")]
    ZeroBatchSize,

    #[error("batch size of {0} MiB overflows a 64-bit byte count")]
    BatchSizeOverflow(u64),

    #[error(
        "rsync {0} cannot work under --files-from batching, because --files-from \
disables the recursion that rsync deletes from; stale files at the destination \
would be silently kept. Run a separate `rsync -a --delete` pass over the whole \
tree afterwards."
    )]
    DeleteUnsupported(String),

    #[error("could not parse rsync options {input:?}: {source}")]
    RsyncOptions {
        input: String,
        #[source]
        source: shell_words::ParseError,
    },

    #[error("failed to run rsync binary {binary:?}: {source}")]
    RsyncSpawn {
        binary: String,
        #[source]
        source: std::io::Error,
    },

    #[error("rsync {status} after {attempts} attempt(s)")]
    RsyncFailed { status: String, attempts: u32 },

    #[error("io error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl Error {
    /// Process exit code this error should map to.
    ///
    /// `2` is reserved for usage/validation problems, `3` for a transfer that
    /// rsync itself refused to complete.
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::SourceNotADirectory(_)
            | Error::DestinationNotADirectory(_)
            | Error::ResolvePath { .. }
            | Error::ZeroBatchSize
            | Error::BatchSizeOverflow(_)
            | Error::DeleteUnsupported(_)
            | Error::RsyncOptions { .. } => 2,
            Error::RsyncSpawn { .. } | Error::RsyncFailed { .. } => 3,
            Error::Io { .. } => 1,
        }
    }
}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;
