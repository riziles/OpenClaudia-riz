//! Shared test-only helpers for the `tools` module.
//!
//! Several tool modules (`worktree`, `cron`) previously defined their own
//! local `cwd_lock` helper, each backed by its own `OnceLock<Mutex<()>>`.
//! That meant a `worktree` test mutating `std::env::set_current_dir` did
//! NOT serialise against a `cron` test doing the same — the two locks
//! were independent, so under parallel `cargo test` execution they could
//! interleave and corrupt each other's CWD assumptions
//! (crosslink #945).
//!
//! Both modules now import [`process_cwd_lock`] from here. The single
//! `static LOCK` lives in this module, so any test in the workspace that
//! mutates process CWD is mutually exclusive with every other test that
//! does so — regardless of which `tools` submodule it lives in.

use std::sync::{Mutex, MutexGuard, OnceLock};

/// Process-wide lock for tests that mutate the current working directory
/// via [`std::env::set_current_dir`].
///
/// `set_current_dir` is process-global; concurrent tests that change it
/// (e.g. to control where `schedules.json` is written, or to observe a
/// worktree's CWD from a relative path) MUST hold this lock for the
/// duration of any block that depends on the CWD.
///
/// A poisoned lock is recovered transparently — a test that panicked
/// while holding the lock has already failed, and downstream tests can
/// still run sequentially under a guard that just re-acquires the inner
/// `()`.
pub fn process_cwd_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
