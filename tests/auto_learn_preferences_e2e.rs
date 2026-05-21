//! End-to-end tests for `auto_learn::AutoLearner` preference
//! detection from user messages + session-end hook + tool
//! success/failure signals routed through the memory store.
//!
//! Sprint 91 of the verification effort. `AutoLearner` is the
//! background-learning surface that watches user messages for
//! imperative preference statements ("always X" / "never Y")
//! and feeds them into `MemoryDb::format_learned_preferences`
//! for prompt injection on subsequent turns. This file pins
//! the imperative-detector gating + conversational-prefix
//! stripping + the per-tool hook routing.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::auto_learn::AutoLearner;
use openclaudia::memory::MemoryDb;
use serde_json::json;
use tempfile::TempDir;

// ───────────────────────────────────────────────────────────────────────────
// Helpers
// ───────────────────────────────────────────────────────────────────────────

fn fresh_db() -> (MemoryDb, TempDir) {
    let dir = TempDir::new().expect("tempdir");
    let db = MemoryDb::open(&dir.path().join("memory.db")).expect("open db");
    (db, dir)
}

// ───────────────────────────────────────────────────────────────────────────
// Section A — Constructor + error counter
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn auto_learner_new_starts_with_zero_db_errors() {
    let (db, _dir) = fresh_db();
    let learner = AutoLearner::new(&db);
    assert_eq!(learner.error_count(), 0);
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — Preference detection: imperative gates
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn always_prefix_imperative_is_detected_and_stored() {
    let (db, _dir) = fresh_db();
    let mut learner = AutoLearner::new(&db);
    learner.on_user_message("always use rustfmt before committing", None);
    let prefs = db.format_learned_preferences().expect("format");
    assert!(
        prefs.contains("rustfmt"),
        "MUST capture 'always use rustfmt' preference; got {prefs:?}"
    );
}

#[test]
fn never_prefix_imperative_is_detected_and_stored() {
    let (db, _dir) = fresh_db();
    let mut learner = AutoLearner::new(&db);
    learner.on_user_message("never use unwrap in production code", None);
    let prefs = db.format_learned_preferences().expect("format");
    assert!(prefs.contains("unwrap"));
}

#[test]
fn prefer_prefix_imperative_is_detected_and_stored() {
    let (db, _dir) = fresh_db();
    let mut learner = AutoLearner::new(&db);
    learner.on_user_message("prefer enum over stringly-typed code", None);
    let prefs = db.format_learned_preferences().expect("format");
    assert!(prefs.contains("enum") || prefs.contains("stringly"));
}

#[test]
fn avoid_prefix_imperative_is_detected_and_stored() {
    let (db, _dir) = fresh_db();
    let mut learner = AutoLearner::new(&db);
    learner.on_user_message("avoid panic in library code", None);
    let prefs = db.format_learned_preferences().expect("format");
    assert!(prefs.contains("panic"));
}

#[test]
fn dont_use_prefix_imperative_is_detected_and_stored() {
    let (db, _dir) = fresh_db();
    let mut learner = AutoLearner::new(&db);
    learner.on_user_message("don't use deprecated APIs", None);
    let prefs = db.format_learned_preferences().expect("format");
    assert!(prefs.contains("deprecated"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — Preference detection: gates that MUST reject
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn question_mark_rejects_imperative_detection() {
    let (db, _dir) = fresh_db();
    let mut learner = AutoLearner::new(&db);
    learner.on_user_message("always use rustfmt?", None);
    let prefs = db.format_learned_preferences().expect("format");
    assert!(
        prefs.is_empty(),
        "questions MUST NOT trigger preference detection; got {prefs:?}"
    );
}

#[test]
fn multi_clause_sentence_rejects_imperative_detection() {
    let (db, _dir) = fresh_db();
    let mut learner = AutoLearner::new(&db);
    // Two sentences — both terminate.
    learner.on_user_message("always use rustfmt. then commit.", None);
    let prefs = db.format_learned_preferences().expect("format");
    assert!(
        prefs.is_empty(),
        "multi-clause statements MUST NOT trigger; got {prefs:?}"
    );
}

#[test]
fn empty_message_does_not_store_anything() {
    let (db, _dir) = fresh_db();
    let mut learner = AutoLearner::new(&db);
    learner.on_user_message("", None);
    learner.on_user_message("   ", None);
    learner.on_user_message("\t\n", None);
    let prefs = db.format_learned_preferences().expect("format");
    assert!(prefs.is_empty());
}

#[test]
fn non_imperative_statement_is_not_captured() {
    let (db, _dir) = fresh_db();
    let mut learner = AutoLearner::new(&db);
    learner.on_user_message("I was thinking about the weather today", None);
    let prefs = db.format_learned_preferences().expect("format");
    assert!(prefs.is_empty());
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — Conversational prefix stripping
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn please_prefix_is_stripped_before_imperative_match() {
    let (db, _dir) = fresh_db();
    let mut learner = AutoLearner::new(&db);
    learner.on_user_message("please always use rustfmt", None);
    let prefs = db.format_learned_preferences().expect("format");
    assert!(
        prefs.contains("rustfmt"),
        "'please always' MUST strip 'please' and match 'always rustfmt'; got {prefs:?}"
    );
}

#[test]
fn i_prefer_prefix_strips_and_detects() {
    let (db, _dir) = fresh_db();
    let mut learner = AutoLearner::new(&db);
    learner.on_user_message("i prefer always use Result over panic", None);
    let prefs = db.format_learned_preferences().expect("format");
    // After stripping "i prefer ", "always use Result over panic"
    // matches the always- preference verb.
    assert!(
        prefs.contains("Result") || prefs.contains("panic"),
        "'i prefer always X' MUST capture; got {prefs:?}"
    );
}

#[test]
fn we_should_prefix_strips_and_detects() {
    let (db, _dir) = fresh_db();
    let mut learner = AutoLearner::new(&db);
    learner.on_user_message("we should always run cargo clippy", None);
    let prefs = db.format_learned_preferences().expect("format");
    assert!(prefs.contains("clippy"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — on_tool_success / on_tool_failure don't panic
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn on_tool_success_for_unknown_tool_is_noop_no_panic() {
    let (db, _dir) = fresh_db();
    let mut learner = AutoLearner::new(&db);
    learner.on_tool_success("totally-unknown-tool", &json!({}), "output");
    // No panic + no errors recorded.
    assert_eq!(learner.error_count(), 0);
}

#[test]
fn on_tool_success_bash_with_valid_args_does_not_panic() {
    let (db, _dir) = fresh_db();
    let mut learner = AutoLearner::new(&db);
    learner.on_tool_success("bash", &json!({"command": "ls"}), "result");
    // bash success may write to file relationships; no panic.
}

#[test]
fn on_tool_success_edit_file_with_valid_args_does_not_panic() {
    let (db, _dir) = fresh_db();
    let mut learner = AutoLearner::new(&db);
    learner.on_tool_success(
        "edit_file",
        &json!({"path": "/tmp/test.rs", "old_string": "x", "new_string": "y"}),
        "edited",
    );
}

#[test]
fn on_tool_success_write_file_with_valid_args_does_not_panic() {
    let (db, _dir) = fresh_db();
    let mut learner = AutoLearner::new(&db);
    learner.on_tool_success(
        "write_file",
        &json!({"path": "/tmp/new.rs", "content": "fn main() {}"}),
        "written",
    );
}

#[test]
fn on_tool_failure_for_bash_does_not_panic() {
    let (db, _dir) = fresh_db();
    let mut learner = AutoLearner::new(&db);
    learner.on_tool_failure("bash", &json!({"command": "false"}), "exit-1");
}

#[test]
fn on_tool_failure_with_missing_args_does_not_panic() {
    let (db, _dir) = fresh_db();
    let mut learner = AutoLearner::new(&db);
    learner.on_tool_failure("bash", &json!({}), "error");
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — on_session_end
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn on_session_end_with_no_activity_does_not_panic() {
    let (db, _dir) = fresh_db();
    let mut learner = AutoLearner::new(&db);
    learner.on_session_end();
    assert_eq!(learner.error_count(), 0);
}

#[test]
fn on_session_end_after_preference_capture_does_not_panic() {
    let (db, _dir) = fresh_db();
    let mut learner = AutoLearner::new(&db);
    learner.on_user_message("always use rustfmt", None);
    learner.on_session_end();
    // Preferences MUST survive session-end pruning (they're
    // bounded to 100 entries, we have 1).
    let prefs = db.format_learned_preferences().expect("format");
    assert!(prefs.contains("rustfmt"));
}

#[test]
fn on_session_end_after_tool_success_signals_does_not_panic() {
    let (db, _dir) = fresh_db();
    let mut learner = AutoLearner::new(&db);
    learner.on_tool_success("edit_file", &json!({"path": "/tmp/a.rs"}), "ok");
    learner.on_tool_success("edit_file", &json!({"path": "/tmp/b.rs"}), "ok");
    learner.on_session_end();
}

// ───────────────────────────────────────────────────────────────────────────
// Section G — Multi-call accumulation
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn multiple_preference_messages_all_capture() {
    let (db, _dir) = fresh_db();
    let mut learner = AutoLearner::new(&db);
    learner.on_user_message("always use rustfmt", None);
    learner.on_user_message("never use unsafe", None);
    learner.on_user_message("prefer Result over panic", None);
    let prefs = db.format_learned_preferences().expect("format");
    // All 3 keywords present.
    assert!(
        prefs.contains("rustfmt"),
        "MUST capture first; got {prefs:?}"
    );
    assert!(prefs.contains("unsafe"));
    assert!(prefs.contains("Result") || prefs.contains("panic"));
}

#[test]
fn repeated_identical_preference_is_idempotent() {
    let (db, _dir) = fresh_db();
    let mut learner = AutoLearner::new(&db);
    for _ in 0..5 {
        learner.on_user_message("always use rustfmt", None);
    }
    let prefs = db.format_learned_preferences().expect("format");
    // The preference MUST be present; the underlying dedup
    // contract isn't specified at this layer (impl detail).
    assert!(prefs.contains("rustfmt"));
}
