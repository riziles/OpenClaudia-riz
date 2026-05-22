//! End-to-end tests for `tools::install_global_path_constraints`
//! plus `clear_global_path_constraints` plus
//! `check_bash_path_against_global` global-slot lifecycle (#594) —
//! install/clear/replace and the no-constraints-installed
//! default behaviour.
//!
//! Sprint 185 of the verification effort. Sprint 24 covered
//! `PathConstraints` instance methods directly; this file pins
//! the global slot lifecycle distinct from per-instance
//! checks.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::tools::{
    check_bash_path_against_global, clear_global_path_constraints, install_global_path_constraints,
    PathConstraints,
};
use std::sync::{Mutex, MutexGuard, OnceLock};
use tempfile::TempDir;

// The global slot is process-wide, so concurrent test threads
// would race. Serialize through this mutex.
fn slot_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

// ───────────────────────────────────────────────────────────────────────────
// Section A — Empty slot defaults to Ok
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn check_against_empty_slot_returns_ok_for_any_command() {
    let _l = slot_lock();
    clear_global_path_constraints();
    // No constraints installed → every command passes.
    assert!(check_bash_path_against_global("ls /tmp").is_ok());
    assert!(check_bash_path_against_global("cat /etc/passwd").is_ok());
    assert!(check_bash_path_against_global("rm -rf /").is_ok());
}

#[test]
fn check_against_empty_slot_returns_ok_for_empty_command() {
    let _l = slot_lock();
    clear_global_path_constraints();
    assert!(check_bash_path_against_global("").is_ok());
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — Install + check
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn install_then_check_accepts_paths_inside_root() {
    let _l = slot_lock();
    let dir = TempDir::new().expect("tempdir");
    let pc = PathConstraints::new([dir.path().to_path_buf()]);
    install_global_path_constraints(pc);

    let inside = format!("ls {}", dir.path().display());
    assert!(
        check_bash_path_against_global(&inside).is_ok(),
        "path inside root MUST pass"
    );
    clear_global_path_constraints();
}

#[test]
fn install_then_check_rejects_paths_outside_root() {
    let _l = slot_lock();
    let dir = TempDir::new().expect("tempdir");
    let pc = PathConstraints::new([dir.path().to_path_buf()]);
    install_global_path_constraints(pc);

    let outside = check_bash_path_against_global("cat /etc/passwd");
    assert!(
        outside.is_err(),
        "/etc/passwd is outside tempdir root, MUST be rejected"
    );
    clear_global_path_constraints();
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — Clear + recheck
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn clear_after_install_restores_no_constraint_default() {
    let _l = slot_lock();
    let dir = TempDir::new().expect("tempdir");
    let pc = PathConstraints::new([dir.path().to_path_buf()]);
    install_global_path_constraints(pc);
    // Confirm gate is active.
    assert!(check_bash_path_against_global("cat /etc/passwd").is_err());

    // Now clear.
    clear_global_path_constraints();
    // Gate is back off — any command accepted.
    assert!(
        check_bash_path_against_global("cat /etc/passwd").is_ok(),
        "clear MUST restore default no-constraints state"
    );
}

#[test]
fn clear_on_already_empty_slot_is_safe() {
    let _l = slot_lock();
    clear_global_path_constraints();
    clear_global_path_constraints();
    // Double-clear no panic.
    assert!(check_bash_path_against_global("any").is_ok());
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — Re-install replaces previous
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn re_install_replaces_previous_constraints() {
    let _l = slot_lock();
    let dir_a = TempDir::new().expect("tempdir a");
    let dir_b = TempDir::new().expect("tempdir b");

    // Install with root A.
    install_global_path_constraints(PathConstraints::new([dir_a.path().to_path_buf()]));
    let cmd_b = format!("cat {}/file.txt", dir_b.path().display());
    assert!(
        check_bash_path_against_global(&cmd_b).is_err(),
        "B path outside root A MUST be rejected"
    );

    // Replace with root B — now B is allowed, A is not.
    install_global_path_constraints(PathConstraints::new([dir_b.path().to_path_buf()]));
    assert!(
        check_bash_path_against_global(&cmd_b).is_ok(),
        "B path inside new root B MUST be accepted"
    );

    clear_global_path_constraints();
}

#[test]
fn install_with_multiple_roots_allows_any_root() {
    let _l = slot_lock();
    let dir_a = TempDir::new().expect("a");
    let dir_b = TempDir::new().expect("b");

    install_global_path_constraints(PathConstraints::new([
        dir_a.path().to_path_buf(),
        dir_b.path().to_path_buf(),
    ]));

    let cmd_a = format!("cat {}/x", dir_a.path().display());
    let cmd_b = format!("cat {}/y", dir_b.path().display());
    assert!(check_bash_path_against_global(&cmd_a).is_ok());
    assert!(check_bash_path_against_global(&cmd_b).is_ok());

    clear_global_path_constraints();
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — Empty roots = unrestricted
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn install_with_empty_roots_allows_everything() {
    // PINS DOC: PathConstraints::check_command short-circuits
    // Ok when roots is empty, even when installed globally.
    let _l = slot_lock();
    install_global_path_constraints(PathConstraints::new::<_, std::path::PathBuf>(
        std::iter::empty(),
    ));
    assert!(check_bash_path_against_global("cat /etc/passwd").is_ok());
    clear_global_path_constraints();
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — PathConstraints::roots accessor
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn roots_accessor_returns_supplied_paths() {
    let dir = TempDir::new().expect("tempdir");
    let pc = PathConstraints::new([dir.path().to_path_buf()]);
    let roots = pc.roots();
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0], dir.path().to_path_buf());
}

#[test]
fn roots_accessor_on_empty_is_empty_slice() {
    let pc = PathConstraints::new::<_, std::path::PathBuf>(std::iter::empty());
    assert!(pc.roots().is_empty());
}

#[test]
fn roots_accessor_returns_all_supplied_in_order() {
    let dir_a = TempDir::new().expect("a");
    let dir_b = TempDir::new().expect("b");
    let dir_c = TempDir::new().expect("c");
    let pc = PathConstraints::new([
        dir_a.path().to_path_buf(),
        dir_b.path().to_path_buf(),
        dir_c.path().to_path_buf(),
    ]);
    assert_eq!(pc.roots().len(), 3);
}

#[test]
fn roots_accessor_skips_empty_and_keeps_nonempty() {
    // PINS DOC: empty roots are filtered out by new().
    let dir = TempDir::new().expect("d");
    let pc = PathConstraints::new([
        std::path::PathBuf::new(),
        dir.path().to_path_buf(),
        std::path::PathBuf::new(),
    ]);
    // Only the non-empty path survives.
    assert_eq!(pc.roots().len(), 1);
}

// ───────────────────────────────────────────────────────────────────────────
// Section G — Idempotency
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn repeated_install_and_clear_cycles_terminate_cleanly() {
    let _l = slot_lock();
    let dir = TempDir::new().expect("d");
    for _ in 0..5 {
        install_global_path_constraints(PathConstraints::new([dir.path().to_path_buf()]));
        assert!(check_bash_path_against_global("cat /etc/passwd").is_err());
        clear_global_path_constraints();
        assert!(check_bash_path_against_global("cat /etc/passwd").is_ok());
    }
}
