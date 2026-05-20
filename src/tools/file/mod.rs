mod edit;
mod list;
mod notebook;
mod read;
mod write;

pub use edit::execute_edit_file;
pub use list::execute_list_files;
#[allow(unused_imports)] // used by tests in tools::mod
pub use notebook::{execute_notebook_edit, source_to_line_array};
#[allow(unused_imports)] // used by tests in tools::mod
pub use read::{
    detect_file_type, parse_page_range, read_image_file, read_notebook_file, read_text_file,
    FileType, ImageKind,
};
pub use write::execute_write_file;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

/// Maximum number of entries in the read tracker, per session, before
/// the oldest write is evicted from the front of the list. Per-session so
/// a noisy session cannot evict another session's reads. Matches the
/// previous global ceiling.
const READ_TRACKER_MAX_ENTRIES: usize = 10_000;

/// Tracks which files have been read, bucketed per session id.
///
/// Each session id (set via `crate::tools::SessionIdGuard`) has its own
/// write-recency list of canonicalized paths: every `mark_read` removes
/// any prior occurrence and pushes the path to the end of the list, so the
/// path with the oldest *write* (not oldest *use*) sits at the front and
/// is evicted first. `edit_file` will fail if the file hasn't been read
/// first **in the same session**. Without an active guard the bucket
/// falls back to the shared default key so the chat REPL and legacy
/// tests keep working out of the box.
///
/// crosslink #986: the previous doc-comment called this an "LRU" list,
/// which is ambiguous — true LRU bumps the entry on read too. Here, only
/// `mark_read` touches the order; `has_been_read` is read-only and does
/// not affect eviction. The naming is now "write-recency" / "insertion-
/// recency" to match the actual semantics.
///
/// crosslink #440 phase 1: session isolation lives inside this
/// singleton (keyed by the thread-local session id), not yet threaded
/// through `ToolContext`. Phase 2 (follow-up issue) will own the
/// tracker on `ChatSession` / `ToolContext` directly.
pub static READ_TRACKER: LazyLock<ReadFileTracker> = LazyLock::new(ReadFileTracker::new);

pub struct ReadFileTracker {
    /// Per-session write-recency lists. Key is the session id from the
    /// thread-local guard (or the shared default key when no guard is
    /// active). Most-recently-WRITTEN paths sit at the end; over
    /// [`READ_TRACKER_MAX_ENTRIES`] in a bucket evicts from the front.
    /// `has_been_read` does not promote — see crosslink #986.
    buckets: Mutex<HashMap<String, Vec<PathBuf>>>,
}

impl ReadFileTracker {
    fn new() -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// Mark a file as having been read in the **current session**.
    /// Moves to end (most recent) if already tracked. Other sessions'
    /// buckets are untouched.
    pub(crate) fn mark_read(&self, path: &Path) {
        let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let key = super::todo::current_session_key();
        if let Ok(mut buckets) = self.buckets.lock() {
            let files = buckets.entry(key).or_default();
            files.retain(|p| p != &resolved);
            files.push(resolved);
            if files.len() > READ_TRACKER_MAX_ENTRIES {
                let excess = files.len() - READ_TRACKER_MAX_ENTRIES;
                files.drain(..excess);
            }
        }
    }

    /// Check whether a file has been read in the **current session**.
    /// A read in another session does not satisfy this check.
    pub(crate) fn has_been_read(&self, path: &Path) -> bool {
        let check_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let key = super::todo::current_session_key();
        self.buckets
            .lock()
            .ok()
            .is_some_and(|buckets| buckets.get(&key).is_some_and(|f| f.contains(&check_path)))
    }

    /// Clear every session's bucket. Used by tests and at
    /// session-start by `crate::tools::reset_read_tracker`. A
    /// per-session `clear()` is intentionally deferred to phase 2
    /// (follow-up issue): until `ToolContext` owns the tracker there
    /// is no caller that has a session id without the thread-local
    /// guard, so adding it now would be dead code rejected by clippy.
    pub(crate) fn clear_all(&self) {
        if let Ok(mut buckets) = self.buckets.lock() {
            buckets.clear();
        }
    }
}

/// Snapshot of the project root, captured the first time [`resolve_path`] runs.
///
/// Pinned at startup so that later `cd`s (via the worktree tool, shell
/// commands, etc.) cannot move the jail underneath us.
///
/// crosslink #981: when `current_dir` or `canonicalize` fail (process started
/// in a deleted directory, FUSE EIO, etc.) the previous fallback was a bare
/// `PathBuf::from(".")` — a relative path. Every subsequent
/// `path_is_within(canonical, &PROJECT_ROOT)` would then compare a fully
/// canonicalized absolute path against `"."` and reject every file silently,
/// breaking the entire tool subsystem with no visible error. Surface the
/// failure: a `warn!` records the underlying cause and we fall back to the
/// absolute filesystem-root `/` so the jail is conservatively wide-open
/// rather than uniformly closed — operators see broken behaviour and an
/// explicit warning instead of a silent dead harness. The follow-up cleanup
/// (panic-on-startup) is tracked separately; this is the smallest fix that
/// removes the silent-dead-harness mode.
static PROJECT_ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
    match std::env::current_dir().and_then(|cwd| cwd.canonicalize()) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(
                error = %e,
                "PROJECT_ROOT could not be resolved at startup (current_dir/canonicalize failed); \
                 file-tool jail will fall back to the filesystem root '/'. crosslink #981",
            );
            // Use a path that exists and is a real directory so containment
            // checks at least return *something* deterministic. `/` matches
            // every absolute path; the operator will see this in logs and
            // can correct it. Better than `"."`, which silently broke
            // every path comparison.
            #[cfg(unix)]
            {
                PathBuf::from("/")
            }
            #[cfg(not(unix))]
            {
                PathBuf::from("\\")
            }
        }
    }
});

/// Process temp directory, canonicalized.
static TEMP_ROOT: LazyLock<Option<PathBuf>> =
    LazyLock::new(|| std::env::temp_dir().canonicalize().ok());

/// Returns `true` when the path jail is in force.
///
/// `OPENCLAUDIA_ALLOW_OUT_OF_ROOT=1` opts out of the project-root + temp-dir
/// containment requirement. crosslink #982: emit a single `tracing::warn!`
/// the first time we observe the variable in the disabled state so an
/// operator who set the flag "just for one test" and forgot has a paper
/// trail in the logs. We deliberately do not warn on every call (the file
/// tools call `resolve_path` per operation); the once-per-process latch
/// keeps the log signal-rich without rate-limiting the file subsystem.
fn strict_mode() -> bool {
    let on = !matches!(std::env::var("OPENCLAUDIA_ALLOW_OUT_OF_ROOT"), Ok(ref v) if v == "1");
    if !on {
        use std::sync::atomic::{AtomicBool, Ordering};
        static WARNED: AtomicBool = AtomicBool::new(false);
        if !WARNED.swap(true, Ordering::SeqCst) {
            tracing::warn!(
                env = "OPENCLAUDIA_ALLOW_OUT_OF_ROOT",
                "file-path jail DISABLED: OPENCLAUDIA_ALLOW_OUT_OF_ROOT=1 is set; \
                 file tools may read/write outside the project root. crosslink #982",
            );
        }
    }
    on
}

fn path_is_within(canonical: &Path, root: &Path) -> bool {
    canonical == root || canonical.starts_with(root)
}

fn resolve_path(path: &str) -> Result<PathBuf, String> {
    let p = Path::new(path);
    let absolute = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| format!("Cannot resolve relative path (no working directory): {e}"))?
            .join(p)
    };
    if absolute
        .components()
        .any(|c| c == std::path::Component::ParentDir)
    {
        return Err(format!("Path traversal not allowed: '{path}'"));
    }
    let canonical = if let Ok(c) = absolute.canonicalize() {
        c
    } else {
        let mut ancestor = absolute.as_path();
        let mut suffix_components: Vec<&std::ffi::OsStr> = Vec::new();
        let canonical_ancestor = loop {
            if let Ok(c) = ancestor.canonicalize() {
                break c;
            }
            let file_name = ancestor.file_name().ok_or_else(|| {
                format!("Cannot resolve any ancestor of '{path}' — reached filesystem root")
            })?;
            suffix_components.push(file_name);
            ancestor = ancestor
                .parent()
                .ok_or_else(|| format!("Cannot resolve parent while walking up '{path}'"))?;
        };
        let mut built = canonical_ancestor;
        for comp in suffix_components.iter().rev() {
            built.push(comp);
        }
        built
    };
    if strict_mode() {
        let in_project = path_is_within(&canonical, &PROJECT_ROOT);
        let in_temp = TEMP_ROOT
            .as_ref()
            .is_some_and(|t| path_is_within(&canonical, t));
        if !in_project && !in_temp {
            return Err(format!(
                "Path '{path}' resolves to '{}' which is outside the project root ('{}') \
                 and outside the process temp directory. Set \
                 OPENCLAUDIA_ALLOW_OUT_OF_ROOT=1 to disable this jail (not recommended).",
                canonical.display(),
                PROJECT_ROOT.display(),
            ));
        }
    }
    Ok(canonical)
}

/// Canonicalise a path that may not yet exist by walking the deepest
/// canonicalisable ancestor and rejoining the remaining suffix.
///
/// crosslink #969: this used to live as inline `match canonicalize(&p) {
/// Ok(c) => c, Err(_) => match p.parent() { ... } }` blocks in
/// `write.rs`, `edit.rs::canonicalise_edit_path`, and
/// `notebook.rs::preflight_and_open` — three near-identical copies with
/// drifted error messages. Centralised here so every file tool agrees on
/// the semantics. Returns the resolved [`PathBuf`] or a stringly-typed
/// error mentioning the original user-supplied path.
pub(super) fn canonicalize_or_walk_up(p: &Path, user_path: &str) -> Result<PathBuf, String> {
    if let Ok(c) = std::fs::canonicalize(p) {
        return Ok(c);
    }
    // Walk up the ancestor chain until we find a canonicalisable directory,
    // then rejoin the missing suffix. Supports `write_file` calling
    // `create_dir_all` later: e.g. `/tmp/X/a/b/c/file.txt` where only
    // `/tmp/X` exists today.
    let mut ancestor = p;
    let mut suffix: Vec<&std::ffi::OsStr> = Vec::new();
    loop {
        let file_name = ancestor.file_name().ok_or_else(|| {
            format!("Cannot resolve any ancestor of '{user_path}' — reached filesystem root")
        })?;
        suffix.push(file_name);
        let Some(parent) = ancestor.parent() else {
            return Err(format!("Invalid path: '{user_path}'"));
        };
        if let Ok(canon_parent) = std::fs::canonicalize(parent) {
            let mut built = canon_parent;
            for comp in suffix.iter().rev() {
                built.push(comp);
            }
            return Ok(built);
        }
        ancestor = parent;
    }
}

pub fn resolve_open_path(user_path: &str) -> Result<PathBuf, String> {
    let p = Path::new(user_path);
    let absolute = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| format!("Cannot resolve relative path (no working directory): {e}"))?
            .join(p)
    };
    if absolute
        .components()
        .any(|c| c == std::path::Component::ParentDir)
    {
        return Err(format!("Path traversal not allowed: '{user_path}'"));
    }
    let parent = absolute
        .parent()
        .ok_or_else(|| format!("Invalid path (no parent): '{user_path}'"))?;
    let leaf = absolute
        .file_name()
        .ok_or_else(|| format!("Invalid path (no leaf): '{user_path}'"))?;
    let canonical_parent = if let Ok(c) = parent.canonicalize() {
        c
    } else {
        let mut ancestor = parent;
        let mut suffix_components: Vec<&std::ffi::OsStr> = Vec::new();
        let canonical_ancestor = loop {
            if let Ok(c) = ancestor.canonicalize() {
                break c;
            }
            let name = ancestor.file_name().ok_or_else(|| {
                format!("Cannot resolve any ancestor of '{user_path}' — reached filesystem root")
            })?;
            suffix_components.push(name);
            ancestor = ancestor
                .parent()
                .ok_or_else(|| format!("Cannot resolve parent while walking up '{user_path}'"))?;
        };
        let mut built = canonical_ancestor;
        for comp in suffix_components.iter().rev() {
            built.push(comp);
        }
        built
    };
    let containment_probe = canonical_parent.join(leaf);
    if strict_mode() {
        let in_project = path_is_within(&containment_probe, &PROJECT_ROOT);
        let in_temp = TEMP_ROOT
            .as_ref()
            .is_some_and(|t| path_is_within(&containment_probe, t));
        if !in_project && !in_temp {
            return Err(format!(
                "Path '{user_path}' resolves to '{}' which is outside the project root ('{}') \
                 and outside the process temp directory. Set \
                 OPENCLAUDIA_ALLOW_OUT_OF_ROOT=1 to disable this jail (not recommended).",
                containment_probe.display(),
                PROJECT_ROOT.display(),
            ));
        }
    }
    Ok(canonical_parent.join(leaf))
}

pub fn execute_read_file(
    args: &std::collections::HashMap<String, serde_json::Value>,
) -> (String, bool) {
    let Some(path) = args.get("path").and_then(|v| v.as_str()) else {
        return ("Missing 'path' argument".to_string(), true);
    };

    let resolved = match resolve_path(path) {
        Ok(p) => p,
        Err(e) => return (e, true),
    };
    let resolved_str = resolved.to_string_lossy();

    READ_TRACKER.mark_read(&resolved);

    match detect_file_type(&resolved_str) {
        FileType::Image(kind) => read_image_file(&resolved_str, kind),
        FileType::Pdf => {
            let pages = args.get("pages").and_then(|v| v.as_str());
            read::read_pdf_file(&resolved_str, pages)
        }
        FileType::Notebook => read_notebook_file(&resolved_str),
        FileType::Text => read_text_file(&resolved_str, args),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    fn tracker_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn two_temp_paths() -> (
        tempfile::NamedTempFile,
        tempfile::NamedTempFile,
        PathBuf,
        PathBuf,
    ) {
        let a = tempfile::NamedTempFile::new().expect("tempfile a");
        let b = tempfile::NamedTempFile::new().expect("tempfile b");
        let pa = a.path().canonicalize().expect("canonicalize a");
        let pb = b.path().canonicalize().expect("canonicalize b");
        (a, b, pa, pb)
    }

    /// crosslink #440 phase 1: a read marked in session A is NOT
    /// visible in session B, despite the shared global tracker.
    #[test]
    fn read_tracker_isolates_marks_between_sessions() {
        let _lock = tracker_lock();
        READ_TRACKER.clear_all();
        let (_keep_a, _keep_b, path_a, path_b) = two_temp_paths();
        {
            let _g = crate::tools::SessionIdGuard::set("session-a-440");
            READ_TRACKER.mark_read(&path_a);
            assert!(READ_TRACKER.has_been_read(&path_a));
        }
        {
            let _g = crate::tools::SessionIdGuard::set("session-b-440");
            assert!(
                !READ_TRACKER.has_been_read(&path_a),
                "session-b must NOT see session-a's read"
            );
            assert!(!READ_TRACKER.has_been_read(&path_b));
            READ_TRACKER.mark_read(&path_b);
            assert!(READ_TRACKER.has_been_read(&path_b));
            assert!(
                !READ_TRACKER.has_been_read(&path_a),
                "session-a's read still invisible after session-b writes its own"
            );
        }
        {
            let _g = crate::tools::SessionIdGuard::set("session-a-440");
            assert!(
                READ_TRACKER.has_been_read(&path_a),
                "session-a's mark survives session-b activity"
            );
            assert!(
                !READ_TRACKER.has_been_read(&path_b),
                "session-a must NOT see session-b's read"
            );
        }
    }

    /// crosslink #440 phase 1: same-session mark-then-check round-trip.
    #[test]
    fn read_tracker_same_session_round_trip() {
        let _lock = tracker_lock();
        READ_TRACKER.clear_all();
        let _g = crate::tools::SessionIdGuard::set("session-round-trip-440");
        let (_keep, _keep_b, path_a, _path_b) = two_temp_paths();
        assert!(
            !READ_TRACKER.has_been_read(&path_a),
            "fresh session sees nothing"
        );
        READ_TRACKER.mark_read(&path_a);
        assert!(
            READ_TRACKER.has_been_read(&path_a),
            "round-trip works inside one session"
        );
        READ_TRACKER.mark_read(&path_a);
        assert!(READ_TRACKER.has_been_read(&path_a), "re-mark stays visible");
    }

    /// crosslink #440 phase 1: `clear_all()` wipes every session's bucket.
    #[test]
    fn read_tracker_clear_all_wipes_every_bucket() {
        let _lock = tracker_lock();
        READ_TRACKER.clear_all();
        let (_keep_a, _keep_b, path_a, path_b) = two_temp_paths();
        {
            let _g = crate::tools::SessionIdGuard::set("session-clear-a-440");
            READ_TRACKER.mark_read(&path_a);
        }
        {
            let _g = crate::tools::SessionIdGuard::set("session-clear-b-440");
            READ_TRACKER.mark_read(&path_b);
        }
        READ_TRACKER.clear_all();
        {
            let _g = crate::tools::SessionIdGuard::set("session-clear-a-440");
            assert!(
                !READ_TRACKER.has_been_read(&path_a),
                "clear_all wipes session-a's bucket"
            );
        }
        {
            let _g = crate::tools::SessionIdGuard::set("session-clear-b-440");
            assert!(
                !READ_TRACKER.has_been_read(&path_b),
                "clear_all wipes session-b's bucket"
            );
        }
    }
}
