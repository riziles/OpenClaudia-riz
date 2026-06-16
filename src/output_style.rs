//! Output style customization for response formatting.
//!
//! Loads style definitions from markdown files in `.openclaudia/output-style.md`
//! or `~/.openclaudia/output-style.md`. The style content is injected into the
//! system prompt to customize how the model formats responses.

use std::io;
use std::path::{Path, PathBuf};

use crate::file_error::{self, FileError};

/// Load the active output style, if any.
/// Checks project-level first, then user-level.
///
/// Read errors other than `NotFound` (e.g. permission denied, encoding) are
/// logged at WARN with the file path and error message, then treated as
/// "no style configured" so the caller can continue without a style. A
/// missing file (`NotFound`) is the normal "no style" path and stays silent.
#[must_use]
pub fn load_output_style() -> Option<String> {
    let project_style = PathBuf::from(".openclaudia/output-style.md");
    if project_style.exists() {
        return read_style(&project_style);
    }

    if let Some(home) = dirs::home_dir() {
        let user_style = home.join(".openclaudia/output-style.md");
        if user_style.exists() {
            return read_style(&user_style);
        }
    }

    None
}

fn read_style(path: &Path) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(content) => {
            let trimmed = content.trim();
            if trimmed.is_empty() {
                None
            } else {
                // Crosslink #828: the output-style file content is
                // user-provided (and may be repo-committed, so a hostile
                // contributor in a multi-author project can plant it).
                // It is interpolated VERBATIM into the system prompt, so
                // a `</output_style>` injection plus sibling
                // instructions would escape the style block and steer
                // the model. `xml_escape_for_prompt` neutralises the
                // three bytes (`<`, `>`, `&`) that can close the
                // surrounding tag — markdown formatting and ASCII
                // English remain untouched.
                Some(crate::memory::xml_escape_for_prompt(trimmed).into_owned())
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => {
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "Failed to read output-style file; treating as no style configured"
            );
            None
        }
    }
}

/// Get a list of built-in style presets
#[must_use]
pub fn builtin_styles() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "concise",
            "Be extremely concise. Lead with the answer. No filler, no preamble. One sentence when possible.",
        ),
        (
            "detailed",
            "Provide thorough, detailed explanations. Include examples and edge cases. Use headers for organization.",
        ),
        (
            "minimal",
            "Respond with the absolute minimum text needed. No greetings, no sign-offs, no explanations unless asked.",
        ),
        (
            "educational",
            "Explain concepts step by step. Use analogies. Highlight key terms. Suitable for learning.",
        ),
        (
            "code-only",
            "When asked to write code, respond with ONLY the code. No explanations before or after unless specifically asked.",
        ),
    ]
}

/// Save a style to the project output-style file.
///
/// # Errors
///
/// Returns [`FileError::Io`] if the directory cannot be created or the file
/// cannot be written. The returned error carries the offending path and the
/// underlying `io::ErrorKind` for programmatic discrimination — see #492.
pub fn save_output_style(content: &str) -> Result<(), FileError> {
    let dir = PathBuf::from(".openclaudia");
    file_error::create_dir_all(&dir)?;
    let path = dir.join("output-style.md");
    file_error::write_file(&path, content)
}

/// Remove the project output-style file.
///
/// # Errors
///
/// Returns [`FileError::Io`] if the file exists but cannot be removed.
pub fn clear_output_style() -> Result<(), FileError> {
    let path = PathBuf::from(".openclaudia/output-style.md");
    if path.exists() {
        std::fs::remove_file(&path).map_err(FileError::with_path(&path))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_builtin_styles() {
        let styles = builtin_styles();
        assert!(styles.len() >= 4);
        assert!(styles.iter().any(|(name, _)| *name == "concise"));
    }

    #[test]
    fn test_load_style_nonexistent() {
        // Should return None when no style file exists (may or may not depending on env)
        let _ = load_output_style();
    }

    /// In-memory writer used to capture tracing output emitted during a test.
    /// Cloning shares the buffer (Arc<Mutex<…>>), so the writer handed to the
    /// subscriber writes to the same buffer the test inspects after the fact.
    #[derive(Clone, Default)]
    struct BufWriter(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for BufWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for BufWriter {
        type Writer = Self;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    /// `NotFound` is the normal "no style configured" path and MUST stay silent —
    /// no WARN/ERROR log lines should be emitted when the file simply isn't there.
    #[test]
    fn read_style_notfound_returns_none_silently() {
        let buf = BufWriter::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(buf.clone())
            .with_max_level(tracing::Level::WARN)
            .without_time()
            .finish();

        let result = tracing::subscriber::with_default(subscriber, || {
            let missing = std::path::PathBuf::from(
                "/nonexistent-openclaudia-test-path/definitely/not/here.md",
            );
            read_style(&missing)
        });

        assert!(result.is_none(), "NotFound must yield None");
        let captured = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert!(
            captured.is_empty(),
            "NotFound must not emit any log output, got: {captured}"
        );
    }

    /// A non-NotFound read error (here: permission denied on a 0o000 file)
    /// must log at WARN with the file path + error message, and still return
    /// None so the caller can continue without an output style.
    #[cfg(unix)]
    #[test]
    fn read_style_permission_denied_logs_warn_and_returns_none() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("output-style.md");
        std::fs::write(&path, "some style content").expect("write fixture");
        // Strip all permission bits so read_to_string fails with PermissionDenied.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).expect("chmod 000");

        // If the test runs as root, permission bits are bypassed and the
        // PermissionDenied branch is unreachable — skip rather than assert
        // a false invariant.
        if nix_is_root() {
            return;
        }

        let buf = BufWriter::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(buf.clone())
            .with_max_level(tracing::Level::WARN)
            .without_time()
            .finish();

        let result = tracing::subscriber::with_default(subscriber, || read_style(&path));

        // Restore perms so tempdir cleanup succeeds.
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));

        assert!(
            result.is_none(),
            "PermissionDenied must yield None so the caller continues without a style"
        );
        let captured = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert!(
            captured.contains("WARN"),
            "expected a WARN log line, got: {captured}"
        );
        assert!(
            captured.contains("output-style.md"),
            "WARN log should mention the file path, got: {captured}"
        );
    }

    #[cfg(unix)]
    fn nix_is_root() -> bool {
        // SAFETY: getuid is a thread-safe libc call with no preconditions.
        unsafe { libc::getuid() == 0 }
    }

    /// Spec — `clear_output_style` propagates a typed [`FileError::Io`] (not
    /// a stringly-typed error) so callers can branch on
    /// [`std::io::ErrorKind`]. Regression guard for crosslink #492.
    ///
    /// Pointing `.openclaudia/output-style.md` at an entry whose parent is a
    /// regular file (not a directory) forces a deterministic `io::Error` on
    /// `remove_file` that the typed variant must preserve through to the
    /// caller, instead of being flattened to a `String`.
    #[test]
    fn clear_output_style_returns_typed_io_error_with_path() {
        use std::io::ErrorKind;

        // Drive `clear_output_style` from a tempdir so we don't touch the
        // user's real `.openclaudia/`. The function reads `.openclaudia/...`
        // relative to the process cwd, so we chdir into the tempdir first.
        let _cwd_lock = crate::tools::testutil::process_cwd_lock();
        let dir = tempfile::tempdir().expect("tempdir");
        let prev_cwd = std::env::current_dir().expect("cwd");
        // NOTE: process-wide cwd mutation. Hold the shared cwd lock so
        // cwd-sensitive tests cannot initialize global path state from this
        // tempdir while this test is running.
        std::env::set_current_dir(dir.path()).expect("chdir");

        // Create `.openclaudia` as a FILE (not a directory). Then the
        // implementation's `path.exists()` returns false for the
        // not-actually-present `output-style.md`, so we instead force a
        // failure by making `.openclaudia/output-style.md` itself a
        // permission-denied target: easiest cross-platform reproduction is
        // to make the path resolve to a non-empty directory and call
        // `remove_file` on it (returns `IsADirectory` or `PermissionDenied`
        // depending on platform, both of which are `io::Error` variants).
        let dot = dir.path().join(".openclaudia");
        std::fs::create_dir_all(&dot).unwrap();
        let target = dot.join("output-style.md");
        std::fs::create_dir_all(&target).unwrap(); // make the leaf a dir!

        let result = clear_output_style();

        // Restore cwd before any assertion so a failure doesn't poison other
        // tests sharing the process.
        std::env::set_current_dir(prev_cwd).expect("restore cwd");

        let err = result.expect_err("removing a directory via remove_file must fail");
        // The typed variant — not a String — must come through.
        let kind = err
            .io_kind()
            .expect("must be the Io variant, not Json/Yaml");
        assert!(
            matches!(
                kind,
                ErrorKind::IsADirectory
                    | ErrorKind::PermissionDenied
                    | ErrorKind::Other
                    | ErrorKind::InvalidInput
            ),
            "expected an io::Error from remove_file-on-dir, got: {err}"
        );
        // And the path is carried through end-to-end.
        assert!(
            err.path().ends_with("output-style.md"),
            "FileError must carry the offending path, got: {}",
            err.path().display()
        );
    }
}
