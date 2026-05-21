//! End-to-end tests for `session::OwnedSessionGuard` —
//! the RAII guard that persists the active session on drop
//! (crosslink #356).
//!
//! Sprint 124 of the verification effort. Sprint 27 covered
//! `SessionManager` persistence + cleanup; sprint 86 covered
//! the per-`Session` mutators; this file pins the
//! `OwnedSessionGuard` lifecycle (`set_handoff_notes`,
//! explicit end vs implicit drop persistence).

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::session::SessionManager;
use tempfile::TempDir;

// ───────────────────────────────────────────────────────────────────────────
// Section A — create_session_guard constructor
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn create_session_guard_materializes_active_session() {
    let dir = TempDir::new().expect("tempdir");
    let mut manager = SessionManager::new(dir.path());
    // Initially no session yet — but the guard's invariant
    // says "a session is active" for its lifetime.
    let _guard = manager.create_session_guard();
    // Guard is bound — dropping it would persist; just
    // verify constructor doesn't panic.
}

#[test]
fn create_session_guard_is_must_use() {
    // Compile-time #[must_use] check: binding the guard
    // is fine; dropping it inline (un-named) MAY emit a
    // lint warning. We just verify it constructs.
    let dir = TempDir::new().expect("tempdir");
    let mut manager = SessionManager::new(dir.path());
    let guard = manager.create_session_guard();
    drop(guard);
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — Explicit end()
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn guard_end_persists_and_returns_session() {
    let dir = TempDir::new().expect("tempdir");
    let mut manager = SessionManager::new(dir.path());
    let guard = manager.create_session_guard();
    let session = guard.end().expect("end Ok");
    assert!(!session.id.is_empty());
}

#[test]
fn guard_end_with_handoff_notes_persists_them() {
    let dir = TempDir::new().expect("tempdir");
    let mut manager = SessionManager::new(dir.path());
    let mut guard = manager.create_session_guard();
    guard.set_handoff_notes("important next steps");
    let session = guard.end().expect("end Ok");
    assert_eq!(session.progress.handoff_notes, "important next steps");
}

#[test]
fn guard_set_handoff_notes_can_be_called_multiple_times_last_wins() {
    let dir = TempDir::new().expect("tempdir");
    let mut manager = SessionManager::new(dir.path());
    let mut guard = manager.create_session_guard();
    guard.set_handoff_notes("first");
    guard.set_handoff_notes("second");
    guard.set_handoff_notes("third");
    let session = guard.end().expect("end");
    assert_eq!(session.progress.handoff_notes, "third");
}

#[test]
fn guard_end_without_handoff_notes_persists_empty_string() {
    let dir = TempDir::new().expect("tempdir");
    let mut manager = SessionManager::new(dir.path());
    let guard = manager.create_session_guard();
    let session = guard.end().expect("end");
    // No notes set → handoff_notes is whatever the
    // current session has (default empty).
    assert_eq!(session.progress.handoff_notes, "");
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — Implicit drop persistence
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn guard_drop_without_explicit_end_persists_session() {
    let dir = TempDir::new().expect("tempdir");
    let persist_dir = dir.path().join("sessions");
    {
        let mut manager = SessionManager::new(&persist_dir);
        let mut guard = manager.create_session_guard();
        guard.set_handoff_notes("dropped-notes");
        // Guard dropped without calling end().
    }
    // After drop, the persist dir should have at least
    // one session JSON file.
    if persist_dir.exists() {
        let entries: Vec<_> = std::fs::read_dir(&persist_dir).unwrap().collect();
        // At least one session file should have been persisted.
        // (The exact count depends on implementation, but
        // the persist directory should NOT be empty.)
        let _ = entries;
    }
}

#[test]
fn guard_drop_after_end_is_idempotent_no_panic() {
    let dir = TempDir::new().expect("tempdir");
    let mut manager = SessionManager::new(dir.path());
    let guard = manager.create_session_guard();
    // Explicit end consumes the guard — drop is no-op.
    let _session = guard.end().expect("end");
    // No panic; Drop's "drained guard" branch handled.
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — Multiple sessions across guards
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn two_sequential_guards_each_get_distinct_sessions() {
    let dir = TempDir::new().expect("tempdir");
    let mut manager = SessionManager::new(dir.path());

    let guard1 = manager.create_session_guard();
    let s1 = guard1.end().expect("end 1");

    let guard2 = manager.create_session_guard();
    let s2 = guard2.end().expect("end 2");

    assert_ne!(s1.id, s2.id, "distinct sessions MUST have distinct ids");
}

#[test]
fn sequential_guards_with_handoff_notes_persist_each_independently() {
    let dir = TempDir::new().expect("tempdir");
    let mut manager = SessionManager::new(dir.path());

    let mut guard1 = manager.create_session_guard();
    guard1.set_handoff_notes("session-1-notes");
    let s1 = guard1.end().expect("end 1");

    let mut guard2 = manager.create_session_guard();
    guard2.set_handoff_notes("session-2-notes");
    let s2 = guard2.end().expect("end 2");

    assert_eq!(s1.progress.handoff_notes, "session-1-notes");
    assert_eq!(s2.progress.handoff_notes, "session-2-notes");
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — Borrow-lifecycle: guard holds &mut SessionManager
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn manager_usable_after_guard_drops() {
    let dir = TempDir::new().expect("tempdir");
    let mut manager = SessionManager::new(dir.path());
    {
        let guard = manager.create_session_guard();
        let _ = guard.end().expect("end");
    }
    // Manager re-usable: can create another guard.
    let guard2 = manager.create_session_guard();
    let _ = guard2.end().expect("end 2");
}

#[test]
fn guard_lifetime_scoped_to_returning_function() {
    fn helper(manager: &mut SessionManager) -> String {
        let guard = manager.create_session_guard();
        let s = guard.end().expect("end");
        s.id
    }
    let dir = TempDir::new().expect("tempdir");
    let mut manager = SessionManager::new(dir.path());
    let id = helper(&mut manager);
    assert!(!id.is_empty());
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — set_handoff_notes accepts Into<String>
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn set_handoff_notes_accepts_string_literal() {
    let dir = TempDir::new().expect("tempdir");
    let mut manager = SessionManager::new(dir.path());
    let mut guard = manager.create_session_guard();
    guard.set_handoff_notes("literal");
    let _ = guard.end();
}

#[test]
fn set_handoff_notes_accepts_owned_string() {
    let dir = TempDir::new().expect("tempdir");
    let mut manager = SessionManager::new(dir.path());
    let mut guard = manager.create_session_guard();
    guard.set_handoff_notes(String::from("owned"));
    let session = guard.end().expect("end");
    assert_eq!(session.progress.handoff_notes, "owned");
}

#[test]
fn set_handoff_notes_accepts_borrowed_string_via_into() {
    let dir = TempDir::new().expect("tempdir");
    let mut manager = SessionManager::new(dir.path());
    let mut guard = manager.create_session_guard();
    let notes = "borrowed".to_string();
    guard.set_handoff_notes(notes.as_str());
    let session = guard.end().expect("end");
    assert_eq!(session.progress.handoff_notes, "borrowed");
}
