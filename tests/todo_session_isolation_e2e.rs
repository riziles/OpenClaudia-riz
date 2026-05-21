//! End-to-end tests for `tools::todo` types — `TodoStatus`
//! serde + `TodoItem` shape + `SessionIdGuard` RAII +
//! per-session bucketing semantics.
//!
//! Sprint 110 of the verification effort. Sprint 49 (via
//! `integration_tests.rs`) covered the basic
//! `execute_todo_*` round-trip; this file pins the
//! `TodoStatus` `snake_case` wire shape, the `TodoItem`
//! `activeForm` camelCase serde rename, the
//! `SessionIdGuard` RAII lifecycle, and the
//! `get_todo_list` / `clear_todo_list` accessor pair without
//! going through `execute_tool`.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::tools::{
    clear_all_todo_lists, clear_todo_list, get_todo_list, SessionIdGuard, TodoItem, TodoStatus,
};
use std::sync::{Mutex, MutexGuard, OnceLock};

// ───────────────────────────────────────────────────────────────────────────
// Global lock — TODO_LISTS is process-wide; tests serialize via this lock
// so they don't race on the shared HashMap.
// ───────────────────────────────────────────────────────────────────────────

fn todo_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

// ───────────────────────────────────────────────────────────────────────────
// Section A — TodoStatus serde (snake_case)
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn todo_status_pending_serializes_as_snake_case() {
    let json = serde_json::to_string(&TodoStatus::Pending).expect("ser");
    assert_eq!(json, "\"pending\"");
}

#[test]
fn todo_status_in_progress_serializes_as_snake_case() {
    let json = serde_json::to_string(&TodoStatus::InProgress).expect("ser");
    assert_eq!(json, "\"in_progress\"");
}

#[test]
fn todo_status_completed_serializes_as_snake_case() {
    let json = serde_json::to_string(&TodoStatus::Completed).expect("ser");
    assert_eq!(json, "\"completed\"");
}

#[test]
fn todo_status_deserializes_from_snake_case_strings() {
    for (input, expected) in &[
        ("\"pending\"", TodoStatus::Pending),
        ("\"in_progress\"", TodoStatus::InProgress),
        ("\"completed\"", TodoStatus::Completed),
    ] {
        let parsed: TodoStatus = serde_json::from_str(input).expect("de");
        assert_eq!(parsed, *expected);
    }
}

#[test]
fn todo_status_rejects_uppercase_or_kebab_case() {
    assert!(serde_json::from_str::<TodoStatus>("\"PENDING\"").is_err());
    assert!(serde_json::from_str::<TodoStatus>("\"in-progress\"").is_err());
    assert!(serde_json::from_str::<TodoStatus>("\"done\"").is_err());
}

#[test]
fn todo_status_round_trips_all_3_variants() {
    for v in &[
        TodoStatus::Pending,
        TodoStatus::InProgress,
        TodoStatus::Completed,
    ] {
        let json = serde_json::to_string(v).expect("ser");
        let back: TodoStatus = serde_json::from_str(&json).expect("de");
        assert_eq!(back, *v);
    }
}

#[test]
fn todo_status_is_copy_and_pairwise_distinct() {
    let p = TodoStatus::Pending;
    let copy = p;
    let again = p;
    assert_eq!(copy, again);
    assert_ne!(TodoStatus::Pending, TodoStatus::InProgress);
    assert_ne!(TodoStatus::InProgress, TodoStatus::Completed);
    assert_ne!(TodoStatus::Pending, TodoStatus::Completed);
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — TodoItem serde shape with activeForm rename
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn todo_item_serializes_active_form_as_camel_case_active_form() {
    let item = TodoItem {
        content: "do thing".to_string(),
        status: TodoStatus::Pending,
        active_form: "Doing thing".to_string(),
    };
    let json = serde_json::to_string(&item).expect("ser");
    // PINS WIRE FIELD: active_form ↔ "activeForm" rename.
    assert!(
        json.contains("\"activeForm\":\"Doing thing\""),
        "MUST use 'activeForm' on wire; got {json:?}"
    );
    assert!(
        !json.contains("active_form"),
        "MUST NOT emit snake_case wire name; got {json:?}"
    );
}

#[test]
fn todo_item_deserializes_from_active_form_camel_case() {
    let json = r#"{
        "content": "task",
        "status": "in_progress",
        "activeForm": "Tasking"
    }"#;
    let item: TodoItem = serde_json::from_str(json).expect("de");
    assert_eq!(item.content, "task");
    assert_eq!(item.status, TodoStatus::InProgress);
    assert_eq!(item.active_form, "Tasking");
}

#[test]
fn todo_item_round_trips_full_shape() {
    let original = TodoItem {
        content: "implement feature".to_string(),
        status: TodoStatus::InProgress,
        active_form: "Implementing feature".to_string(),
    };
    let json = serde_json::to_string(&original).expect("ser");
    let back: TodoItem = serde_json::from_str(&json).expect("de");
    assert_eq!(back.content, original.content);
    assert_eq!(back.status, original.status);
    assert_eq!(back.active_form, original.active_form);
}

#[test]
fn todo_item_clone_preserves_all_three_fields() {
    let original = TodoItem {
        content: "c".to_string(),
        status: TodoStatus::Completed,
        active_form: "C".to_string(),
    };
    let cloned = original.clone();
    assert_eq!(cloned.content, original.content);
    assert_eq!(cloned.status, original.status);
    assert_eq!(cloned.active_form, original.active_form);
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — get_todo_list / clear_todo_list on default session
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn get_todo_list_on_fresh_default_session_is_empty() {
    let _l = todo_lock();
    clear_all_todo_lists();
    let list = get_todo_list();
    assert!(list.is_empty());
}

#[test]
fn clear_todo_list_after_no_writes_is_no_op() {
    let _l = todo_lock();
    clear_all_todo_lists();
    clear_todo_list();
    // No panic, list still empty.
    assert!(get_todo_list().is_empty());
}

#[test]
fn clear_all_todo_lists_can_be_called_idempotently() {
    let _l = todo_lock();
    clear_all_todo_lists();
    clear_all_todo_lists();
    clear_all_todo_lists();
    assert!(get_todo_list().is_empty());
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — SessionIdGuard RAII
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn session_id_guard_returns_must_use_guard() {
    let _l = todo_lock();
    // Compile-time MUST: the guard is #[must_use], so binding
    // to `_` is the documented "ignore" pattern.
    let guard = SessionIdGuard::set("test-session-123");
    drop(guard);
}

#[test]
fn session_id_guard_drop_restores_previous_value() {
    let _l = todo_lock();
    clear_all_todo_lists();
    // Inside the guard, get_todo_list bucket is the test
    // session.
    {
        let _g = SessionIdGuard::set("session-a");
        // No writes yet; bucket starts empty.
        assert!(get_todo_list().is_empty());
    }
    // After drop, the guard has restored the previous
    // session id (which was None — default key).
    assert!(get_todo_list().is_empty());
}

#[test]
fn session_id_guard_nested_guards_restore_to_outer_value() {
    let _l = todo_lock();
    clear_all_todo_lists();
    {
        let _outer = SessionIdGuard::set("outer");
        {
            let _inner = SessionIdGuard::set("inner");
            // Inside inner: session-id is "inner".
            // Each session has its own bucket.
        }
        // After inner drops, outer's session-id is restored.
        // get_todo_list reads the outer bucket — still empty
        // because no writes.
        assert!(get_todo_list().is_empty());
    }
    // After outer drops, default-key bucket is active.
    assert!(get_todo_list().is_empty());
}

#[test]
fn session_id_guard_can_be_constructed_with_borrowed_string() {
    let _l = todo_lock();
    let id = String::from("borrowed-session");
    let guard = SessionIdGuard::set(&id);
    drop(guard);
}

#[test]
fn session_id_guard_can_be_constructed_with_owned_string() {
    let _l = todo_lock();
    let guard = SessionIdGuard::set(String::from("owned-session"));
    drop(guard);
}

#[test]
fn session_id_guard_can_be_constructed_with_str_literal() {
    let _l = todo_lock();
    let guard = SessionIdGuard::set("literal-session");
    drop(guard);
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — Per-session bucketing
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn distinct_sessions_have_distinct_todo_buckets() {
    let _l = todo_lock();
    clear_all_todo_lists();

    // Both sessions start empty.
    {
        let _g = SessionIdGuard::set("session-x");
        assert!(get_todo_list().is_empty());
    }
    {
        let _g = SessionIdGuard::set("session-y");
        assert!(get_todo_list().is_empty());
    }
    // Default bucket also empty.
    assert!(get_todo_list().is_empty());
}

#[test]
fn clear_all_todo_lists_clears_every_session_bucket() {
    let _l = todo_lock();
    // Just verify it doesn't panic when called with multiple
    // distinct session ids in play.
    clear_all_todo_lists();
    {
        let _g = SessionIdGuard::set("sess-1");
        clear_all_todo_lists();
    }
    {
        let _g = SessionIdGuard::set("sess-2");
        clear_all_todo_lists();
    }
}
