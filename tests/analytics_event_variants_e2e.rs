//! End-to-end tests for `services::AnalyticsEvent` —
//! the 7-variant event taxonomy with field accessors via
//! pattern-matching + Clone + Debug derives.
//!
//! Sprint 219 of the verification effort.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::services::AnalyticsEvent;

// ───────────────────────────────────────────────────────────────────────────
// Section A — SessionStart variant
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn session_start_carries_session_id() {
    let ev = AnalyticsEvent::SessionStart {
        session_id: "sess-219-marker".to_string(),
    };
    if let AnalyticsEvent::SessionStart { session_id } = &ev {
        assert_eq!(session_id, "sess-219-marker");
    } else {
        panic!("expected SessionStart");
    }
}

#[test]
fn session_start_with_empty_id_still_constructible() {
    let ev = AnalyticsEvent::SessionStart {
        session_id: String::new(),
    };
    assert!(matches!(ev, AnalyticsEvent::SessionStart { .. }));
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — SessionEnd variant (id + messages)
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn session_end_carries_both_fields() {
    let ev = AnalyticsEvent::SessionEnd {
        session_id: "s1".to_string(),
        messages: 42,
    };
    if let AnalyticsEvent::SessionEnd {
        session_id,
        messages,
    } = &ev
    {
        assert_eq!(session_id, "s1");
        assert_eq!(*messages, 42);
    } else {
        panic!("expected SessionEnd");
    }
}

#[test]
fn session_end_zero_messages_constructible() {
    let ev = AnalyticsEvent::SessionEnd {
        session_id: "x".to_string(),
        messages: 0,
    };
    assert!(matches!(ev, AnalyticsEvent::SessionEnd { messages: 0, .. }));
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — ToolUsed variant (tool + success bit)
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn tool_used_carries_tool_name_and_success_true() {
    let ev = AnalyticsEvent::ToolUsed {
        tool: "bash".to_string(),
        success: true,
    };
    if let AnalyticsEvent::ToolUsed { tool, success } = &ev {
        assert_eq!(tool, "bash");
        assert!(*success);
    }
}

#[test]
fn tool_used_carries_success_false() {
    let ev = AnalyticsEvent::ToolUsed {
        tool: "edit_file".to_string(),
        success: false,
    };
    if let AnalyticsEvent::ToolUsed { success, .. } = &ev {
        assert!(!*success);
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — PromptSubmitted variant (PII-safe char length)
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn prompt_submitted_carries_char_length_not_text() {
    // PINS DOC: PromptSubmitted carries prompt_chars (length), NOT
    // the actual prompt text (PII protection).
    let ev = AnalyticsEvent::PromptSubmitted { prompt_chars: 1500 };
    if let AnalyticsEvent::PromptSubmitted { prompt_chars } = &ev {
        assert_eq!(*prompt_chars, 1500);
    }
}

#[test]
fn prompt_submitted_with_zero_chars_constructible() {
    let ev = AnalyticsEvent::PromptSubmitted { prompt_chars: 0 };
    assert!(matches!(
        ev,
        AnalyticsEvent::PromptSubmitted { prompt_chars: 0 }
    ));
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — ContextCompacted variant
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn context_compacted_carries_trigger_and_tokens_freed() {
    let ev = AnalyticsEvent::ContextCompacted {
        trigger: "auto-threshold",
        tokens_freed: 12_000,
    };
    if let AnalyticsEvent::ContextCompacted {
        trigger,
        tokens_freed,
    } = &ev
    {
        assert_eq!(*trigger, "auto-threshold");
        assert_eq!(*tokens_freed, 12_000);
    }
}

#[test]
fn context_compacted_trigger_is_static_str() {
    // PINS DOC: trigger is &'static str, not String — caller passes
    // a documented enum-like constant.
    let ev = AnalyticsEvent::ContextCompacted {
        trigger: "manual",
        tokens_freed: 1000,
    };
    if let AnalyticsEvent::ContextCompacted { trigger, .. } = &ev {
        let _: &'static str = trigger;
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — ApiRequest variant
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn api_request_carries_provider_and_model() {
    let ev = AnalyticsEvent::ApiRequest {
        provider: "anthropic".to_string(),
        model: "claude-opus-4".to_string(),
    };
    if let AnalyticsEvent::ApiRequest { provider, model } = &ev {
        assert_eq!(provider, "anthropic");
        assert_eq!(model, "claude-opus-4");
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section G — ThinkingEmitted variant
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn thinking_emitted_carries_budget_u32() {
    let ev = AnalyticsEvent::ThinkingEmitted { budget: 8192 };
    if let AnalyticsEvent::ThinkingEmitted { budget } = &ev {
        assert_eq!(*budget, 8192);
    }
}

#[test]
fn thinking_emitted_with_u32_max_constructible() {
    let ev = AnalyticsEvent::ThinkingEmitted { budget: u32::MAX };
    assert!(matches!(ev, AnalyticsEvent::ThinkingEmitted { .. }));
}

// ───────────────────────────────────────────────────────────────────────────
// Section H — Clone derive
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn clone_session_start_preserves_id() {
    let original = AnalyticsEvent::SessionStart {
        session_id: "marker".to_string(),
    };
    let cloned = original.clone();
    // Both original and cloned still usable.
    if let AnalyticsEvent::SessionStart { session_id } = &original {
        assert_eq!(session_id, "marker");
    }
    if let AnalyticsEvent::SessionStart { session_id } = cloned {
        assert_eq!(session_id, "marker");
    }
}

#[test]
fn clone_tool_used_preserves_both_fields() {
    let original = AnalyticsEvent::ToolUsed {
        tool: "T".to_string(),
        success: true,
    };
    let cloned = original.clone();
    if let AnalyticsEvent::ToolUsed { tool, success } = &original {
        assert_eq!(tool, "T");
        assert!(*success);
    }
    if let AnalyticsEvent::ToolUsed { tool, success } = cloned {
        assert_eq!(tool, "T");
        assert!(success);
    }
}

#[test]
fn clone_context_compacted_preserves_static_trigger() {
    let original = AnalyticsEvent::ContextCompacted {
        trigger: "compact-marker",
        tokens_freed: 99,
    };
    let cloned = original.clone();
    if let AnalyticsEvent::ContextCompacted {
        trigger,
        tokens_freed,
    } = &original
    {
        assert_eq!(*trigger, "compact-marker");
        assert_eq!(*tokens_freed, 99);
    }
    if let AnalyticsEvent::ContextCompacted {
        trigger,
        tokens_freed,
    } = cloned
    {
        assert_eq!(trigger, "compact-marker");
        assert_eq!(tokens_freed, 99);
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section I — Debug formatting
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn debug_session_start_includes_variant_name() {
    let ev = AnalyticsEvent::SessionStart {
        session_id: "s1".to_string(),
    };
    let d = format!("{ev:?}");
    assert!(d.contains("SessionStart"));
    assert!(d.contains("s1"));
}

#[test]
fn debug_all_seven_variants_include_variant_name() {
    let cases: Vec<(AnalyticsEvent, &str)> = vec![
        (
            AnalyticsEvent::SessionStart {
                session_id: "x".into(),
            },
            "SessionStart",
        ),
        (
            AnalyticsEvent::SessionEnd {
                session_id: "x".into(),
                messages: 1,
            },
            "SessionEnd",
        ),
        (
            AnalyticsEvent::ToolUsed {
                tool: "x".into(),
                success: true,
            },
            "ToolUsed",
        ),
        (
            AnalyticsEvent::PromptSubmitted { prompt_chars: 1 },
            "PromptSubmitted",
        ),
        (
            AnalyticsEvent::ContextCompacted {
                trigger: "x",
                tokens_freed: 1,
            },
            "ContextCompacted",
        ),
        (
            AnalyticsEvent::ApiRequest {
                provider: "x".into(),
                model: "y".into(),
            },
            "ApiRequest",
        ),
        (
            AnalyticsEvent::ThinkingEmitted { budget: 1 },
            "ThinkingEmitted",
        ),
    ];
    for (ev, name) in cases {
        let d = format!("{ev:?}");
        assert!(d.contains(name), "Debug MUST contain {name}; got {d}");
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section J — Send + Sync
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn analytics_event_is_send_sync_for_async_dispatch() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<AnalyticsEvent>();
}
