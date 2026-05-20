//! Typed errors for file I/O and on-disk serialization.
//!
//! Replaces the `map_err(|e| e.to_string())` and `map_err(|e| format!(...))`
//! pattern that previously littered file-touching code paths. Stringly-typed
//! errors lose the source chain ([`std::io::ErrorKind`], `serde` line/column
//! information) and prevent programmatic discrimination between e.g.
//! `NotFound`, `PermissionDenied`, and `AlreadyExists`. See crosslink #492.
//!
//! Callers gain three benefits over the old pattern:
//!
//! 1. The underlying `io::Error` / `serde_json::Error` / `serde_yaml::Error`
//!    is preserved via `#[source]`, so `Display` chains expose the cause and
//!    consumers can downcast or match.
//! 2. Every variant carries the offending [`PathBuf`], so the rendered error
//!    always says *which* file failed (the old pattern routinely dropped this).
//! 3. [`FileError::io_kind`] surfaces the [`std::io::ErrorKind`] without forcing
//!    the consumer to know about the inner type — enough to distinguish the
//!    common cases (missing, permission denied) in a render or retry path.
//!
//! Helpers [`read_file`], [`write_file`], [`read_json`], [`read_yaml`],
//! [`write_json_pretty`], and [`create_dir_all`] are provided for the
//! ergonomically common case where callers want the typed error for free
//! without writing the `.map_err(...)` themselves.

use std::path::{Path, PathBuf};

use thiserror::Error;

/// Errors raised by file I/O and on-disk parse/serialize operations.
///
/// All variants carry the [`PathBuf`] that was being operated on, so the
/// rendered message always names the offending file. The original
/// [`std::io::Error`] / `serde` error is preserved via `#[source]`.
#[derive(Debug, Error)]
pub enum FileError {
    /// Raw I/O failure (`open`/`read`/`write`/`rename`/`create_dir`).
    #[error("I/O error on {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// JSON serialization or deserialization failure.
    #[error("JSON error on {}: {source}", path.display())]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    /// YAML serialization or deserialization failure.
    #[error("YAML error on {}: {source}", path.display())]
    Yaml {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },

    /// File content decoded as UTF-8 was not valid UTF-8.
    #[error("UTF-8 decoding error on {}: {source}", path.display())]
    Utf8 {
        path: PathBuf,
        #[source]
        source: std::string::FromUtf8Error,
    },

    /// File or directory failed a precondition check (symlink, missing
    /// parent, wrong owner, etc.) before any I/O was attempted.
    #[error("invalid file state on {}: {reason}", path.display())]
    Invalid { path: PathBuf, reason: String },
}

impl FileError {
    /// Surface the inner [`std::io::ErrorKind`] when the error is an
    /// [`FileError::Io`]. Returns `None` for the parse/decoding variants
    /// — callers that branch on `NotFound`/`PermissionDenied` don't need
    /// to know the inner type.
    #[must_use]
    pub fn io_kind(&self) -> Option<std::io::ErrorKind> {
        if let Self::Io { source, .. } = self {
            Some(source.kind())
        } else {
            None
        }
    }

    /// Return the [`Path`] that was being operated on when the error
    /// occurred. Always present — every variant carries one.
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            Self::Io { path, .. }
            | Self::Json { path, .. }
            | Self::Yaml { path, .. }
            | Self::Utf8 { path, .. }
            | Self::Invalid { path, .. } => path.as_path(),
        }
    }
}

/// Trait-style helpers so callers can write
/// `std::fs::read(...).map_err(FileError::with_path(&path))?` instead of
/// hand-rolling a closure. Kept as inherent associated functions to avoid
/// a public extension trait surface.
impl FileError {
    /// Build an `Io` variant closure for the given path.
    pub fn with_path(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> Self {
        let path = path.into();
        move |source| Self::Io { path, source }
    }

    /// Build a `Json` variant closure for the given path.
    pub fn json_with_path(path: impl Into<PathBuf>) -> impl FnOnce(serde_json::Error) -> Self {
        let path = path.into();
        move |source| Self::Json { path, source }
    }

    /// Build a `Yaml` variant closure for the given path.
    pub fn yaml_with_path(path: impl Into<PathBuf>) -> impl FnOnce(serde_yaml::Error) -> Self {
        let path = path.into();
        move |source| Self::Yaml { path, source }
    }
}

/// Read a file's contents as a UTF-8 string, returning a typed [`FileError`]
/// that names the path on failure.
///
/// # Errors
/// Returns [`FileError::Io`] if the file cannot be read.
pub fn read_file(path: impl AsRef<Path>) -> Result<String, FileError> {
    let path = path.as_ref();
    std::fs::read_to_string(path).map_err(FileError::with_path(path))
}

/// Write `contents` to `path`, returning a typed [`FileError`].
///
/// # Errors
/// Returns [`FileError::Io`] if the file cannot be written.
pub fn write_file(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> Result<(), FileError> {
    let path = path.as_ref();
    std::fs::write(path, contents).map_err(FileError::with_path(path))
}

/// Create `path` and all parent directories as needed.
///
/// # Errors
/// Returns [`FileError::Io`] on any underlying filesystem failure.
pub fn create_dir_all(path: impl AsRef<Path>) -> Result<(), FileError> {
    let path = path.as_ref();
    std::fs::create_dir_all(path).map_err(FileError::with_path(path))
}

/// Read and parse a JSON file into `T`.
///
/// # Errors
/// Returns [`FileError::Io`] if the file cannot be read, or
/// [`FileError::Json`] if the contents fail to deserialize.
pub fn read_json<T: serde::de::DeserializeOwned>(path: impl AsRef<Path>) -> Result<T, FileError> {
    let path = path.as_ref();
    let s = read_file(path)?;
    serde_json::from_str(&s).map_err(FileError::json_with_path(path))
}

/// Read and parse a YAML file into `T`.
///
/// # Errors
/// Returns [`FileError::Io`] if the file cannot be read, or
/// [`FileError::Yaml`] if the contents fail to deserialize.
pub fn read_yaml<T: serde::de::DeserializeOwned>(path: impl AsRef<Path>) -> Result<T, FileError> {
    let path = path.as_ref();
    let s = read_file(path)?;
    serde_yaml::from_str(&s).map_err(FileError::yaml_with_path(path))
}

/// Serialize `value` as pretty JSON and write it to `path`.
///
/// # Errors
/// Returns [`FileError::Json`] if serialization fails or
/// [`FileError::Io`] if the file cannot be written.
pub fn write_json_pretty<T: serde::Serialize>(
    path: impl AsRef<Path>,
    value: &T,
) -> Result<(), FileError> {
    let path = path.as_ref();
    let json = serde_json::to_string_pretty(value).map_err(FileError::json_with_path(path))?;
    write_file(path, json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{self, ErrorKind};
    use std::path::PathBuf;

    /// Spec — `FileError::Io` preserves the inner `io::ErrorKind` so callers
    /// can branch on `NotFound` / `PermissionDenied` etc. without restringing.
    #[test]
    fn io_variant_preserves_error_kind_not_found() {
        let io_err = io::Error::new(ErrorKind::NotFound, "missing");
        let err = FileError::Io {
            path: PathBuf::from("/nope/here"),
            source: io_err,
        };
        assert_eq!(err.io_kind(), Some(ErrorKind::NotFound));
    }

    /// Spec — `io_kind()` returns `None` for non-Io variants so callers
    /// distinguish parse failure from underlying-filesystem failure.
    #[test]
    fn io_kind_returns_none_for_non_io_variants() {
        let json_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
        let err = FileError::Json {
            path: PathBuf::from("/tmp/x.json"),
            source: json_err,
        };
        assert!(err.io_kind().is_none());
    }

    /// Spec — `Display` impl always names the offending file path.
    /// The old stringly-typed pattern routinely dropped this information,
    /// which is the regression #492 calls out.
    #[test]
    fn display_includes_path() {
        let io_err = io::Error::new(ErrorKind::PermissionDenied, "nope");
        let err = FileError::Io {
            path: PathBuf::from("/etc/protected.yaml"),
            source: io_err,
        };
        let msg = err.to_string();
        assert!(
            msg.contains("/etc/protected.yaml"),
            "Display must include path, got: {msg}"
        );
    }

    /// Spec — `with_path` closure produces an `Io` variant tagged with
    /// the right path and preserves the source error kind. This is the
    /// hot helper code paths use in place of `.map_err(|e| e.to_string())`.
    #[test]
    fn with_path_helper_builds_correct_io_variant() {
        let io_err = io::Error::new(ErrorKind::AlreadyExists, "dup");
        let map = FileError::with_path("/var/lib/openclaudia/state.json");
        let err = map(io_err);
        match err {
            FileError::Io { path, source } => {
                assert_eq!(path, PathBuf::from("/var/lib/openclaudia/state.json"));
                assert_eq!(source.kind(), ErrorKind::AlreadyExists);
            }
            other => panic!("expected Io variant, got {other:?}"),
        }
    }

    /// Spec — `read_file` on a missing path returns a typed `Io` variant
    /// whose `io_kind()` reports `NotFound`. Exercises a real call site
    /// to prove the typed variant survives end-to-end through the helper,
    /// not just in synthesized test errors.
    #[test]
    fn read_file_propagates_typed_not_found() {
        let p = PathBuf::from("/this/path/definitely/does/not/exist/openclaudia/x.json");
        let err = read_file(&p).expect_err("missing path must error");
        assert_eq!(
            err.io_kind(),
            Some(ErrorKind::NotFound),
            "expected NotFound from typed FileError, got: {err}"
        );
        assert_eq!(err.path(), p.as_path());
    }

    /// Spec — `read_json` on a syntactically invalid file returns the
    /// `Json` variant (not `Io`) and the path is preserved.
    #[test]
    fn read_json_returns_typed_json_variant_on_parse_failure() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path().join("bad.json");
        std::fs::write(&p, "{ this is not json").unwrap();
        let err = read_json::<serde_json::Value>(&p).expect_err("must fail");
        assert!(
            matches!(err, FileError::Json { .. }),
            "expected Json variant, got: {err:?}"
        );
        assert_eq!(err.path(), p.as_path());
    }
}
