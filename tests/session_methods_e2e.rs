//! End-to-end tests for `Session` per-instance methods +
//! `SessionView` zero-copy accessors + `SessionMode` + the
//! `TurnMetrics` ring eviction at `MAX_TURN_METRICS`.
//!
//! Sprint 86 of the verification effort. Sprint 27 covered
//! `SessionManager` persistence + load + cleanup; this file
//! covers the per-`Session` mutators (`touch`,
//! `increment_requests`, `add_tokens`, `record_turn_estimate`,
//! `record_actual_usage`, `complete_task`, `set_handoff_notes`,
//! `generate_handoff`) and the `SessionView` read-only
//! accessors that the rest of the codebase prefers over
//! cloning.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::session::{Session, SessionMode, TokenUsage, MAX_TURN_METRICS};

// ───────────────────────────────────────────────────────────────────────────
// Section A — Constructors
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn new_initializer_mode_is_initializer_with_no_parent() {
    let s = Session::new_initializer();
    assert_eq!(s.mode, SessionMode::Initializer);
    assert!(s.parent_session_id.is_none());
    assert_eq!(s.request_count, 0);
    assert_eq!(s.total_tokens(), 0);
    assert!(s.turn_metrics.is_empty());
    assert_eq!(s.total_turns, 0);
}

#[test]
fn new_coding_records_parent_id_and_coding_mode() {
    let s = Session::new_coding("parent-uuid-abc");
    assert_eq!(s.mode, SessionMode::Coding);
    assert_eq!(s.parent_session_id.as_deref(), Some("parent-uuid-abc"));
    assert_eq!(s.request_count, 0);
}

#[test]
fn fresh_sessions_have_distinct_uuids() {
    let a = Session::new_initializer();
    let b = Session::new_initializer();
    assert_ne!(a.id, b.id);
}

#[test]
fn fresh_session_legacy_persisted_total_tokens_is_none() {
    let s = Session::new_initializer();
    assert!(s.legacy_persisted_total_tokens().is_none());
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — touch + increment_requests
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn increment_requests_increments_counter_and_touches() {
    let mut s = Session::new_initializer();
    let before = s.updated_at;
    // Sleep briefly so timestamps differ.
    std::thread::sleep(std::time::Duration::from_millis(2));
    s.increment_requests();
    assert_eq!(s.request_count, 1);
    assert!(
        s.updated_at > before,
        "increment_requests MUST touch updated_at"
    );

    s.increment_requests();
    s.increment_requests();
    assert_eq!(s.request_count, 3);
}

#[test]
fn touch_updates_timestamp_idempotently() {
    let mut s = Session::new_initializer();
    let before = s.updated_at;
    std::thread::sleep(std::time::Duration::from_millis(2));
    s.touch();
    assert!(s.updated_at > before);
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — add_tokens routes through cumulative_usage
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn add_tokens_accumulates_as_input_tokens() {
    let mut s = Session::new_initializer();
    s.add_tokens(100);
    assert_eq!(s.cumulative_usage.input_tokens, 100);
    assert_eq!(s.cumulative_usage.output_tokens, 0);
    assert_eq!(s.total_tokens(), 100);
}

#[test]
fn add_tokens_multiple_calls_accumulate() {
    let mut s = Session::new_initializer();
    s.add_tokens(50);
    s.add_tokens(75);
    s.add_tokens(25);
    assert_eq!(s.cumulative_usage.input_tokens, 150);
    assert_eq!(s.total_tokens(), 150);
}

#[test]
fn total_tokens_is_input_plus_output_only() {
    // PINS DOCUMENTED CONTRACT: total_tokens excludes
    // cache_read and cache_write.
    let mut s = Session::new_initializer();
    s.cumulative_usage = TokenUsage {
        input_tokens: 100,
        output_tokens: 50,
        cache_read_tokens: 1000,
        cache_write_tokens: 500,
    };
    assert_eq!(s.total_tokens(), 150, "cache tokens MUST NOT count");
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — record_turn_estimate
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn record_turn_estimate_returns_monotonically_increasing_turn_numbers() {
    let mut s = Session::new_initializer();
    let n1 = s.record_turn_estimate(100, 10, 20, 30);
    let n2 = s.record_turn_estimate(200, 10, 20, 30);
    let n3 = s.record_turn_estimate(300, 10, 20, 30);
    assert_eq!(n1, 1);
    assert_eq!(n2, 2);
    assert_eq!(n3, 3);
}

#[test]
fn record_turn_estimate_pushes_metrics_into_ring() {
    let mut s = Session::new_initializer();
    s.record_turn_estimate(100, 10, 20, 30);
    assert_eq!(s.turn_metrics.len(), 1);
    let m = &s.turn_metrics[0];
    assert_eq!(m.turn_number, 1);
    assert_eq!(m.estimated_input_tokens, 100);
    assert_eq!(m.injected_context_tokens, 10);
    assert_eq!(m.system_prompt_tokens, 20);
    assert_eq!(m.tool_def_tokens, 30);
    assert!(m.actual_usage.is_none());
}

#[test]
fn record_turn_estimate_increments_total_turns_counter() {
    let mut s = Session::new_initializer();
    assert_eq!(s.total_turns, 0);
    s.record_turn_estimate(0, 0, 0, 0);
    s.record_turn_estimate(0, 0, 0, 0);
    s.record_turn_estimate(0, 0, 0, 0);
    assert_eq!(s.total_turns, 3);
}

#[test]
fn record_turn_estimate_evicts_oldest_at_capacity() {
    // PINS RING-CAP CONTRACT: turn_metrics caps at
    // MAX_TURN_METRICS; oldest entry evicted on each push
    // past cap. cumulative usage + total_turns are NOT
    // affected by eviction.
    let mut s = Session::new_initializer();
    // Fill ring to capacity.
    for _ in 0..MAX_TURN_METRICS {
        s.record_turn_estimate(100, 0, 0, 0);
    }
    assert_eq!(s.turn_metrics.len(), MAX_TURN_METRICS);
    let first_at_cap_turn = s.turn_metrics[0].turn_number;
    // Push one more.
    let next_turn = s.record_turn_estimate(999, 0, 0, 0);
    assert_eq!(s.turn_metrics.len(), MAX_TURN_METRICS, "ring caps at MAX");
    // Oldest entry evicted: new index 0 was previously index 1.
    assert!(
        s.turn_metrics[0].turn_number > first_at_cap_turn,
        "oldest entry MUST be evicted"
    );
    // Newest entry has the highest turn number.
    let last = s.turn_metrics.last().unwrap();
    assert_eq!(last.turn_number, next_turn);
    assert_eq!(last.estimated_input_tokens, 999);
    // total_turns reflects cumulative count (not evicted).
    assert_eq!(s.total_turns, u64::try_from(MAX_TURN_METRICS).unwrap() + 1);
}

#[test]
fn max_turn_metrics_constant_matches_documented_value() {
    assert_eq!(MAX_TURN_METRICS, 1_000);
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — record_actual_usage
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn record_actual_usage_attaches_to_latest_turn_metric() {
    let mut s = Session::new_initializer();
    s.record_turn_estimate(100, 0, 0, 0);
    let usage = TokenUsage {
        input_tokens: 95,
        output_tokens: 25,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
    };
    s.record_actual_usage(usage);
    let last = s.turn_metrics.last().expect("Some");
    let actual = last
        .actual_usage
        .as_ref()
        .expect("actual_usage MUST be set");
    assert_eq!(actual.input_tokens, 95);
    assert_eq!(actual.output_tokens, 25);
}

#[test]
fn record_actual_usage_accumulates_into_cumulative_usage() {
    let mut s = Session::new_initializer();
    s.record_turn_estimate(100, 0, 0, 0);
    s.record_actual_usage(TokenUsage {
        input_tokens: 95,
        output_tokens: 25,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
    });
    assert_eq!(s.cumulative_usage.input_tokens, 95);
    assert_eq!(s.cumulative_usage.output_tokens, 25);
    assert_eq!(s.total_tokens(), 120);
}

#[test]
fn record_actual_usage_accumulates_across_multiple_turns() {
    let mut s = Session::new_initializer();
    s.record_turn_estimate(0, 0, 0, 0);
    s.record_actual_usage(TokenUsage {
        input_tokens: 100,
        output_tokens: 50,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
    });
    s.record_turn_estimate(0, 0, 0, 0);
    s.record_actual_usage(TokenUsage {
        input_tokens: 200,
        output_tokens: 75,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
    });
    assert_eq!(s.cumulative_usage.input_tokens, 300);
    assert_eq!(s.cumulative_usage.output_tokens, 125);
    assert_eq!(s.total_tokens(), 425);
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — Progress mutators
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn complete_task_appends_to_completed_tasks_list() {
    let mut s = Session::new_initializer();
    s.complete_task("Task A");
    s.complete_task("Task B");
    assert_eq!(s.progress.completed_tasks, vec!["Task A", "Task B"]);
}

#[test]
fn add_modified_file_appends_to_files_modified() {
    let mut s = Session::new_initializer();
    s.add_modified_file("src/main.rs");
    s.add_modified_file("src/lib.rs");
    assert_eq!(s.progress.files_modified.len(), 2);
    assert!(s
        .progress
        .files_modified
        .contains(&"src/main.rs".to_string()));
}

#[test]
fn set_handoff_notes_overwrites_existing_notes() {
    let mut s = Session::new_initializer();
    s.set_handoff_notes("first notes");
    assert_eq!(s.progress.handoff_notes, "first notes");
    s.set_handoff_notes("replaced");
    assert_eq!(s.progress.handoff_notes, "replaced");
}

// ───────────────────────────────────────────────────────────────────────────
// Section G — generate_handoff content
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn handoff_header_includes_session_id_and_mode() {
    let s = Session::new_initializer();
    let handoff = s.generate_handoff();
    assert!(handoff.contains("Session Handoff"));
    assert!(handoff.contains(&s.id), "MUST include session id");
    assert!(
        handoff.contains("Initializer"),
        "MUST include mode label; got {handoff:?}"
    );
}

#[test]
fn handoff_renders_completed_tasks_section_when_present() {
    let mut s = Session::new_initializer();
    s.complete_task("Done thing A");
    s.complete_task("Done thing B");
    let handoff = s.generate_handoff();
    assert!(handoff.contains("Completed Tasks"));
    assert!(handoff.contains("Done thing A"));
    assert!(handoff.contains("Done thing B"));
}

#[test]
fn handoff_renders_files_modified_section_when_present() {
    let mut s = Session::new_initializer();
    s.add_modified_file("src/foo.rs");
    let handoff = s.generate_handoff();
    assert!(handoff.contains("Files Modified"));
    assert!(handoff.contains("src/foo.rs"));
}

#[test]
fn handoff_renders_notes_section_when_set() {
    let mut s = Session::new_initializer();
    s.set_handoff_notes("Keep going with feature X");
    let handoff = s.generate_handoff();
    assert!(handoff.contains("Notes for Next Session"));
    assert!(handoff.contains("Keep going with feature X"));
}

#[test]
fn handoff_omits_empty_sections() {
    let s = Session::new_initializer();
    let handoff = s.generate_handoff();
    // Fresh session: no completed/in-progress/pending/decisions/files.
    assert!(
        !handoff.contains("Completed Tasks"),
        "MUST omit empty section; got {handoff:?}"
    );
    assert!(!handoff.contains("In Progress"));
    assert!(!handoff.contains("Pending Tasks"));
    assert!(!handoff.contains("Files Modified"));
}

#[test]
fn handoff_includes_token_usage_when_nonzero() {
    let mut s = Session::new_initializer();
    s.add_tokens(500);
    let handoff = s.generate_handoff();
    assert!(handoff.contains("Token Usage"));
    assert!(handoff.contains("500"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section H — SessionView accessors
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn session_view_id_returns_underlying_id() {
    let s = Session::new_initializer();
    let view = s.view();
    assert_eq!(view.id(), s.id);
}

#[test]
fn session_view_parent_session_id_returns_inner_option() {
    let s = Session::new_coding("parent-x");
    let view = s.view();
    assert_eq!(view.parent_session_id(), Some("parent-x"));

    let init = Session::new_initializer();
    let init_view = init.view();
    assert!(init_view.parent_session_id().is_none());
}

#[test]
fn session_view_turn_metrics_returns_borrowed_slice() {
    let mut s = Session::new_initializer();
    s.record_turn_estimate(100, 0, 0, 0);
    s.record_turn_estimate(200, 0, 0, 0);
    let view = s.view();
    let metrics = view.turn_metrics();
    assert_eq!(metrics.len(), 2);
    assert_eq!(metrics[0].estimated_input_tokens, 100);
    assert_eq!(metrics[1].estimated_input_tokens, 200);
}

// ───────────────────────────────────────────────────────────────────────────
// Section I — SessionMode serde
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn session_mode_serde_uses_snake_case() {
    let init_json = serde_json::to_string(&SessionMode::Initializer).expect("ser");
    let cod_json = serde_json::to_string(&SessionMode::Coding).expect("ser");
    assert_eq!(init_json.trim_matches('"'), "initializer");
    assert_eq!(cod_json.trim_matches('"'), "coding");
}

#[test]
fn session_mode_round_trips() {
    for mode in &[SessionMode::Initializer, SessionMode::Coding] {
        let json = serde_json::to_string(mode).expect("ser");
        let back: SessionMode = serde_json::from_str(&json).expect("de");
        assert_eq!(back, *mode);
    }
}

#[test]
fn session_mode_variants_are_distinct() {
    assert_ne!(SessionMode::Initializer, SessionMode::Coding);
}

// ───────────────────────────────────────────────────────────────────────────
// Section J — Session serde round-trip
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn session_round_trips_through_json() {
    let mut s = Session::new_initializer();
    s.add_tokens(123);
    s.record_turn_estimate(50, 5, 10, 15);
    s.complete_task("done");
    s.set_handoff_notes("notes");
    let json = serde_json::to_string(&s).expect("ser");
    let back: Session = serde_json::from_str(&json).expect("de");
    assert_eq!(back.id, s.id);
    assert_eq!(back.mode, s.mode);
    assert_eq!(back.total_tokens(), 123);
    assert_eq!(back.turn_metrics.len(), 1);
    assert_eq!(back.progress.completed_tasks, vec!["done"]);
    assert_eq!(back.progress.handoff_notes, "notes");
}

#[test]
fn session_with_legacy_total_tokens_field_round_trips() {
    // Pre-#854 session JSONL still carries `total_tokens`;
    // deserializer accepts it and surfaces via
    // legacy_persisted_total_tokens.
    // Build legacy JSON by serializing a real Session +
    // splicing in the legacy total_tokens field.
    let s = Session::new_initializer();
    let mut value: serde_json::Value = serde_json::to_value(&s).expect("ser");
    value
        .as_object_mut()
        .unwrap()
        .insert("total_tokens".to_string(), serde_json::json!(9999));
    let json = value.to_string();
    let back: Session = serde_json::from_str(&json).expect("de");
    assert_eq!(back.legacy_persisted_total_tokens(), Some(9999));
    // Live cumulative_usage was zero in the source session
    // — total_tokens() (the derived getter) returns 0, NOT
    // 9999. Pinning the documented contract: legacy field
    // doesn't repopulate cumulative_usage.
    assert_eq!(back.total_tokens(), 0);
}
