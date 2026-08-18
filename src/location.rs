//! Distinguishing local paths from rsync's remote endpoint syntax.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Where one side of the transfer lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    Local(PathBuf),
    /// An `rsync://…`, `host:path` or `host::path` spec, passed through verbatim.
    Remote(String),
}

impl Location {
    /// Classify a command-line endpoint using rsync's own rules.
    ///
    /// A colon makes the spec remote only when it appears before the first
    /// slash, so `/mnt/a:b/c` is local while `host:/srv` is not.
    #[must_use]
    pub fn parse(spec: &str) -> Self {
        if spec.starts_with("rsync://") {
            return Location::Remote(spec.to_string());
        }
        match (spec.find(':'), spec.find('/')) {
            (Some(colon), Some(slash)) if colon < slash => Location::Remote(spec.to_string()),
            (Some(_), None) => Location::Remote(spec.to_string()),
            _ => Location::Local(PathBuf::from(spec)),
        }
    }

    #[must_use]
    pub fn is_remote(&self) -> bool {
        matches!(self, Location::Remote(_))
    }
}

/// Resolve the source endpoint, which must be an existing local directory.
///
/// Returns the canonical path with a trailing slash, the form rsync expects for
/// the root of a `--files-from` list.
pub fn resolve_source(spec: &str) -> Result<(PathBuf, String)> {
    let path = match Location::parse(spec) {
        Location::Local(path) => path,
        // rsync cannot enumerate a remote tree for us, so a remote source is
        // simply not something this tool can batch.
        Location::Remote(spec) => return Err(Error::SourceNotADirectory(PathBuf::from(spec))),
    };

    let canonical = std::fs::canonicalize(&path).map_err(|source| Error::ResolvePath {
        path: path.clone(),
        source,
    })?;
    if !canonical.is_dir() {
        return Err(Error::SourceNotADirectory(canonical));
    }

    let arg = format!("{}/", canonical.display());
    Ok((canonical, arg))
}

/// Resolve the destination endpoint.
///
/// Remote destinations are passed through untouched — canonicalising them is
/// what used to break `user@host:/srv/backup`. A missing local destination is
/// created, matching what plain rsync does.
pub fn resolve_destination(spec: &str) -> Result<String> {
    let path = match Location::parse(spec) {
        Location::Remote(spec) => return Ok(spec),
        Location::Local(path) => path,
    };

    if !path.exists() {
        std::fs::create_dir_all(&path).map_err(|source| Error::ResolvePath {
            path: path.clone(),
            source,
        })?;
    }

    let canonical = std::fs::canonicalize(&path).map_err(|source| Error::ResolvePath {
        path: path.clone(),
        source,
    })?;
    if !canonical.is_dir() {
        return Err(Error::DestinationNotADirectory(canonical));
    }

    Ok(format!("{}/", canonical.display()))
}

/// True when `dest` sits inside `src`, which would make the sync feed on itself.
#[must_use]
pub fn is_nested(src: &Path, dest: &Path) -> bool {
    dest.starts_with(src)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_paths_are_local() {
        for spec in ["/srv/data", "./rel", "data", "/mnt/a b/c"] {
            assert_eq!(
                Location::parse(spec),
                Location::Local(PathBuf::from(spec)),
                "{spec} should be local"
            );
        }
    }

    #[test]
    fn host_specs_are_remote() {
        for spec in [
            "user@host:/srv",
            "host:/srv",
            "host::module/path",
            "rsync://host/module",
        ] {
            assert!(Location::parse(spec).is_remote(), "{spec} should be remote");
        }
    }

    #[test]
    fn a_colon_after_a_slash_is_part_of_the_path_not_a_host() {
        // rsync's own rule; `/mnt/backup:2024/` is a directory, not a host.
        assert_eq!(
            Location::parse("/mnt/backup:2024/x"),
            Location::Local(PathBuf::from("/mnt/backup:2024/x"))
        );
    }

    #[test]
    fn remote_destinations_pass_through_untouched() {
        let spec = "user@host:/home/user/dest/";
        assert_eq!(resolve_destination(spec).unwrap(), spec);
    }

    #[test]
    fn remote_sources_are_rejected_because_they_cannot_be_enumerated() {
        assert!(matches!(
            resolve_source("host:/srv"),
            Err(Error::SourceNotADirectory(_))
        ));
    }

    #[test]
    fn a_missing_local_destination_is_created() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("nested").join("dest");
        let resolved = resolve_destination(dest.to_str().unwrap()).unwrap();
        assert!(dest.is_dir());
        assert!(resolved.ends_with('/'));
    }

    #[test]
    fn a_destination_that_is_a_file_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("not-a-dir");
        std::fs::write(&file, b"x").unwrap();
        assert!(matches!(
            resolve_destination(file.to_str().unwrap()),
            Err(Error::DestinationNotADirectory(_))
        ));
    }

    #[test]
    fn source_resolves_to_a_canonical_path_with_a_trailing_slash() {
        let tmp = tempfile::tempdir().unwrap();
        let (path, arg) = resolve_source(tmp.path().to_str().unwrap()).unwrap();
        assert!(path.is_absolute());
        assert!(arg.ends_with('/'), "rsync wants the source root, got {arg}");
    }

    #[test]
    fn missing_source_is_an_error() {
        assert!(matches!(
            resolve_source("/definitely/does/not/exist/anywhere"),
            Err(Error::ResolvePath { .. })
        ));
    }

    #[test]
    fn nesting_detects_a_destination_inside_the_source() {
        assert!(is_nested(
            Path::new("/srv/data"),
            Path::new("/srv/data/backup")
        ));
        assert!(!is_nested(Path::new("/srv/data"), Path::new("/srv/other")));
    }
}
