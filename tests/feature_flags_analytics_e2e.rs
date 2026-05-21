//! End-to-end tests for `StaticFlags` precedence + env-snapshot
//! semantics, and the `AnalyticsEvent` taxonomy via `NoopAnalytics`
//! and a custom test sink.
//!
//! Sprint 37 of the verification effort.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::services::{
    AnalyticsEvent, AnalyticsSink, FeatureFlagSource, NoopAnalytics, StaticFlags,
};
use std::sync::{Mutex, MutexGuard, OnceLock};

// ───────────────────────────────────────────────────────────────────────────
// Env-guard infrastructure
// ───────────────────────────────────────────────────────────────────────────

/// Tests in this file mutate `OPENCLAUDIA_FEATURE_*` env vars and
/// must serialize so a concurrent test doesn't observe a partial
/// state. Single `OnceLock<Mutex<()>>` shared across the binary.
fn env_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct EnvGuard {
    key: String,
    previous: Option<String>,
}

impl EnvGuard {
    fn set(key: &str, value: &str) -> Self {
        let previous = std::env::var(key).ok();
        // SAFETY: tests serialize via env_lock above.
        unsafe {
            std::env::set_var(key, value);
        }
        Self {
            key: key.to_string(),
            previous,
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        unsafe {
            match self.previous.take() {
                Some(v) => std::env::set_var(&self.key, v),
                None => std::env::remove_var(&self.key),
            }
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section A — default + set semantics
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn unset_flag_defaults_to_false() {
    let _l = env_lock();
    let flags = StaticFlags::new();
    assert!(!flags.is_enabled("absolutely_not_a_real_flag_xyz_9999"));
}

#[test]
fn set_flag_returns_explicit_value() {
    let _l = env_lock();
    let mut flags = StaticFlags::new();
    flags.set("opt_in_thing", true);
    assert!(flags.is_enabled("opt_in_thing"));
    flags.set("opt_in_thing", false);
    assert!(!flags.is_enabled("opt_in_thing"));
}

#[test]
fn with_chainable_alias_returns_set_value() {
    let _l = env_lock();
    let flags = StaticFlags::new().with("alpha", true).with("beta", false);
    assert!(flags.is_enabled("alpha"));
    assert!(!flags.is_enabled("beta"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — env-var precedence (env > set > default)
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn env_var_override_takes_precedence_over_explicit_set_false() {
    let _l = env_lock();
    let _guard = EnvGuard::set("OPENCLAUDIA_FEATURE_PRIORITY_TEST", "true");
    // env captured AT new() time per crosslink #843
    let mut flags = StaticFlags::new();
    flags.set("priority_test", false); // explicit FALSE
                                       // env override wins → returns true.
    assert!(
        flags.is_enabled("priority_test"),
        "env=true MUST override set=false"
    );
}

#[test]
fn env_var_override_takes_precedence_over_explicit_set_true() {
    let _l = env_lock();
    let _guard = EnvGuard::set("OPENCLAUDIA_FEATURE_PRIORITY_TEST2", "false");
    let mut flags = StaticFlags::new();
    flags.set("priority_test2", true);
    assert!(
        !flags.is_enabled("priority_test2"),
        "env=false MUST override set=true"
    );
}

#[test]
fn env_var_uppercases_flag_name() {
    let _l = env_lock();
    // Set OPENCLAUDIA_FEATURE_ULTRATHINK_ENABLED (uppercase suffix)
    // and query via the snake_case form.
    let _guard = EnvGuard::set("OPENCLAUDIA_FEATURE_ULTRATHINK_ENABLED", "1");
    let flags = StaticFlags::new();
    assert!(
        flags.is_enabled("ultrathink_enabled"),
        "snake_case query MUST resolve to uppercased env var"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — env truthy/falsy parsing
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn env_truthy_values_all_resolve_true() {
    for value in &["1", "true", "TRUE", "on", "ON", "yes", "Yes"] {
        let _l = env_lock();
        let _guard = EnvGuard::set("OPENCLAUDIA_FEATURE_TRUTHY_TEST", value);
        let flags = StaticFlags::new();
        assert!(
            flags.is_enabled("truthy_test"),
            "env value {value:?} MUST resolve as truthy"
        );
    }
}

#[test]
fn env_falsy_values_all_resolve_false() {
    for value in &[
        "0",
        "false",
        "FALSE",
        "off",
        "OFF",
        "no",
        "anything-else",
        "",
    ] {
        let _l = env_lock();
        let _guard = EnvGuard::set("OPENCLAUDIA_FEATURE_FALSY_TEST", value);
        let flags = StaticFlags::new();
        assert!(
            !flags.is_enabled("falsy_test"),
            "env value {value:?} MUST resolve as falsy"
        );
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — env snapshot at construction (crosslink #843)
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn env_changes_after_construction_do_not_affect_existing_flags() {
    let _l = env_lock();
    // Construct WITHOUT the env var set.
    let flags = StaticFlags::new();
    assert!(!flags.is_enabled("late_env_test"));
    // Now set the env var.
    let _guard = EnvGuard::set("OPENCLAUDIA_FEATURE_LATE_ENV_TEST", "true");
    // The already-constructed flags do NOT see the late env var
    // (the snapshot is taken at construction time).
    assert!(
        !flags.is_enabled("late_env_test"),
        "post-construction env change MUST NOT affect the snapshot"
    );
}

#[test]
fn reload_env_picks_up_late_changes() {
    let _l = env_lock();
    let mut flags = StaticFlags::new();
    assert!(!flags.is_enabled("reload_test"));
    let _guard = EnvGuard::set("OPENCLAUDIA_FEATURE_RELOAD_TEST", "yes");
    // Without reload, the flag stays at its captured-false state.
    assert!(!flags.is_enabled("reload_test"));
    // After reload_env, the env override is observed.
    flags.reload_env();
    assert!(
        flags.is_enabled("reload_test"),
        "reload_env MUST capture the late env change"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — AnalyticsSink: NoopAnalytics + custom test sink
// ───────────────────────────────────────────────────────────────────────────

/// Test sink that records every event it receives so the test
/// can pattern-match on shape + count.
#[derive(Default)]
struct CapturingSink {
    events: Mutex<Vec<AnalyticsEvent>>,
}

impl AnalyticsSink for CapturingSink {
    fn record(&self, event: AnalyticsEvent) {
        if let Ok(mut g) = self.events.lock() {
            g.push(event);
        }
    }
}

impl CapturingSink {
    fn snapshot(&self) -> Vec<AnalyticsEvent> {
        self.events.lock().unwrap().clone()
    }
}

#[test]
fn noop_analytics_silently_accepts_every_event_kind() {
    // The NoopAnalytics sink must not panic, allocate
    // observably, or otherwise misbehave on any event.
    let sink = NoopAnalytics;
    sink.record(AnalyticsEvent::SessionStart {
        session_id: "s1".to_string(),
    });
    sink.record(AnalyticsEvent::SessionEnd {
        session_id: "s1".to_string(),
        messages: 42,
    });
    sink.record(AnalyticsEvent::ToolUsed {
        tool: "bash".to_string(),
        success: true,
    });
    sink.record(AnalyticsEvent::PromptSubmitted { prompt_chars: 100 });
    sink.record(AnalyticsEvent::ContextCompacted {
        trigger: "auto",
        tokens_freed: 5000,
    });
    sink.record(AnalyticsEvent::ApiRequest {
        provider: "anthropic".to_string(),
        model: "claude-3-5-sonnet".to_string(),
    });
    sink.record(AnalyticsEvent::ThinkingEmitted { budget: 8000 });
}

#[test]
fn capturing_sink_round_trips_every_event_field_byte_exact() {
    let sink = CapturingSink::default();
    sink.record(AnalyticsEvent::SessionStart {
        session_id: "abc-123".to_string(),
    });
    sink.record(AnalyticsEvent::ToolUsed {
        tool: "edit_file".to_string(),
        success: false,
    });
    sink.record(AnalyticsEvent::PromptSubmitted { prompt_chars: 1234 });

    let captured = sink.snapshot();
    assert_eq!(captured.len(), 3);

    match &captured[0] {
        AnalyticsEvent::SessionStart { session_id } => assert_eq!(session_id, "abc-123"),
        other => panic!("expected SessionStart, got {other:?}"),
    }
    match &captured[1] {
        AnalyticsEvent::ToolUsed { tool, success } => {
            assert_eq!(tool, "edit_file");
            assert!(!success, "success flag must round-trip false");
        }
        other => panic!("expected ToolUsed, got {other:?}"),
    }
    match &captured[2] {
        AnalyticsEvent::PromptSubmitted { prompt_chars } => assert_eq!(*prompt_chars, 1234),
        other => panic!("expected PromptSubmitted, got {other:?}"),
    }
}

#[test]
fn prompt_submitted_event_carries_char_length_not_content() {
    // The PromptSubmitted event payload is documented to be
    // CHAR LENGTH ONLY — never the prompt text itself (PII).
    // We verify the variant only exposes `prompt_chars` and
    // no field that could carry text.
    //
    // This is a compile-time + Debug-format check: a future
    // change that adds a `prompt: String` field to the variant
    // would change the Debug shape and trip this assertion.
    let event = AnalyticsEvent::PromptSubmitted { prompt_chars: 500 };
    let debug = format!("{event:?}");
    assert!(
        debug.contains("prompt_chars"),
        "Debug must include prompt_chars; got {debug:?}"
    );
    assert!(
        debug.contains("500"),
        "Debug must include the char count; got {debug:?}"
    );
    // The Debug output MUST NOT contain a `prompt: ` text
    // field — that would be a PII leak by event design.
    // (We assert on a specific string shape; a Debug derive
    // would always include the field name.)
    assert!(
        !debug.contains("prompt:") && !debug.contains("prompt =") && !debug.contains("\"prompt\""),
        "Debug must NOT contain a `prompt:` field (PII leak); got {debug:?}"
    );
}

#[test]
fn analytics_sink_is_send_sync_for_arc_dispatch() {
    // The trait bound `AnalyticsSink: Send + Sync` is what
    // lets a single Arc<dyn AnalyticsSink> be cloned across
    // async tasks. A compile-time assertion via a generic
    // function confirms the bound is present.
    fn assert_send_sync<T: Send + Sync + ?Sized>() {}
    assert_send_sync::<dyn AnalyticsSink>();
    assert_send_sync::<NoopAnalytics>();
    assert_send_sync::<CapturingSink>();
}
