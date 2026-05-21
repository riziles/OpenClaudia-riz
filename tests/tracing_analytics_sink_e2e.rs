//! End-to-end tests for `services::TracingAnalytics` sink —
//! no-panic for every documented `AnalyticsEvent` variant,
//! Send+Sync trait object dispatch, and trait-object equality
//! with `NoopAnalytics`.
//!
//! Sprint 113 of the verification effort. Sprint 38
//! (`feature_flags_analytics_e2e`) covered `NoopAnalytics`
//! plus a recording test sink; this file pins the
//! `TracingAnalytics` dispatch (every event variant routes
//! through without panic), the Send+Sync bound, and the
//! `Arc<dyn AnalyticsSink>` erasure used by `ServiceRegistry`.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::services::{AnalyticsEvent, AnalyticsSink, NoopAnalytics, TracingAnalytics};
use std::sync::Arc;

// ───────────────────────────────────────────────────────────────────────────
// Section A — TracingAnalytics dispatches every event variant without panic
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn tracing_analytics_records_session_start_without_panic() {
    let sink = TracingAnalytics;
    sink.record(AnalyticsEvent::SessionStart {
        session_id: "test-session-1".to_string(),
    });
}

#[test]
fn tracing_analytics_records_session_end_without_panic() {
    let sink = TracingAnalytics;
    sink.record(AnalyticsEvent::SessionEnd {
        session_id: "test-session-2".to_string(),
        messages: 42,
    });
}

#[test]
fn tracing_analytics_records_tool_used_success_without_panic() {
    let sink = TracingAnalytics;
    sink.record(AnalyticsEvent::ToolUsed {
        tool: "bash".to_string(),
        success: true,
    });
}

#[test]
fn tracing_analytics_records_tool_used_failure_without_panic() {
    let sink = TracingAnalytics;
    sink.record(AnalyticsEvent::ToolUsed {
        tool: "edit_file".to_string(),
        success: false,
    });
}

#[test]
fn tracing_analytics_records_prompt_submitted_without_panic() {
    let sink = TracingAnalytics;
    sink.record(AnalyticsEvent::PromptSubmitted { prompt_chars: 1234 });
}

#[test]
fn tracing_analytics_records_context_compacted_without_panic() {
    let sink = TracingAnalytics;
    sink.record(AnalyticsEvent::ContextCompacted {
        trigger: "auto",
        tokens_freed: 50_000,
    });
}

#[test]
fn tracing_analytics_records_api_request_without_panic() {
    let sink = TracingAnalytics;
    sink.record(AnalyticsEvent::ApiRequest {
        provider: "anthropic".to_string(),
        model: "claude-sonnet-4-5".to_string(),
    });
}

#[test]
fn tracing_analytics_records_thinking_emitted_without_panic() {
    let sink = TracingAnalytics;
    sink.record(AnalyticsEvent::ThinkingEmitted { budget: 8000 });
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — All 7 documented event variants — exhaustive matrix
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn tracing_analytics_handles_all_7_event_variants_in_sequence() {
    let sink = TracingAnalytics;
    let events = vec![
        AnalyticsEvent::SessionStart {
            session_id: "s".to_string(),
        },
        AnalyticsEvent::SessionEnd {
            session_id: "s".to_string(),
            messages: 0,
        },
        AnalyticsEvent::ToolUsed {
            tool: "t".to_string(),
            success: true,
        },
        AnalyticsEvent::PromptSubmitted { prompt_chars: 0 },
        AnalyticsEvent::ContextCompacted {
            trigger: "manual",
            tokens_freed: 0,
        },
        AnalyticsEvent::ApiRequest {
            provider: "p".to_string(),
            model: "m".to_string(),
        },
        AnalyticsEvent::ThinkingEmitted { budget: 0 },
    ];
    for event in events {
        sink.record(event);
    }
}

#[test]
fn tracing_analytics_handles_extreme_token_counts_without_overflow() {
    let sink = TracingAnalytics;
    sink.record(AnalyticsEvent::ContextCompacted {
        trigger: "auto",
        tokens_freed: usize::MAX,
    });
    sink.record(AnalyticsEvent::PromptSubmitted {
        prompt_chars: usize::MAX,
    });
    sink.record(AnalyticsEvent::ThinkingEmitted { budget: u32::MAX });
}

#[test]
fn tracing_analytics_handles_empty_string_payloads() {
    let sink = TracingAnalytics;
    sink.record(AnalyticsEvent::SessionStart {
        session_id: String::new(),
    });
    sink.record(AnalyticsEvent::ToolUsed {
        tool: String::new(),
        success: true,
    });
    sink.record(AnalyticsEvent::ApiRequest {
        provider: String::new(),
        model: String::new(),
    });
}

#[test]
fn tracing_analytics_handles_unicode_string_payloads() {
    let sink = TracingAnalytics;
    sink.record(AnalyticsEvent::SessionStart {
        session_id: "セッション-1".to_string(),
    });
    sink.record(AnalyticsEvent::ApiRequest {
        provider: "anthropic".to_string(),
        model: "クロード-5".to_string(),
    });
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — Trait-object Send + Sync compile-time + runtime
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn tracing_analytics_is_send_compile_time_check() {
    fn assert_send<T: Send>() {}
    assert_send::<TracingAnalytics>();
}

#[test]
fn tracing_analytics_is_sync_compile_time_check() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<TracingAnalytics>();
}

#[test]
fn analytics_sink_trait_object_is_send_for_arc_dispatch() {
    // The whole point of Send + Sync on AnalyticsSink is to
    // enable Arc<dyn AnalyticsSink> across thread boundaries.
    let sink: Arc<dyn AnalyticsSink> = Arc::new(TracingAnalytics);
    let cloned = Arc::clone(&sink);
    std::thread::spawn(move || {
        cloned.record(AnalyticsEvent::SessionStart {
            session_id: "from-thread".to_string(),
        });
    })
    .join()
    .expect("thread completes");
    sink.record(AnalyticsEvent::SessionEnd {
        session_id: "main".to_string(),
        messages: 1,
    });
}

#[test]
fn noop_analytics_is_drop_in_replacement_for_tracing_via_trait_object() {
    let sinks: Vec<Arc<dyn AnalyticsSink>> =
        vec![Arc::new(NoopAnalytics), Arc::new(TracingAnalytics)];
    for sink in &sinks {
        sink.record(AnalyticsEvent::ToolUsed {
            tool: "test".to_string(),
            success: true,
        });
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — Box<dyn AnalyticsSink> + Arc<dyn AnalyticsSink>
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn tracing_analytics_can_be_boxed_as_trait_object() {
    let sink: Box<dyn AnalyticsSink> = Box::new(TracingAnalytics);
    sink.record(AnalyticsEvent::PromptSubmitted { prompt_chars: 50 });
}

#[test]
fn tracing_analytics_supports_concurrent_arc_clone_dispatch() {
    let sink: Arc<dyn AnalyticsSink> = Arc::new(TracingAnalytics);
    let mut handles = Vec::new();
    for i in 0..4 {
        let sink_clone = Arc::clone(&sink);
        handles.push(std::thread::spawn(move || {
            sink_clone.record(AnalyticsEvent::ToolUsed {
                tool: format!("tool-{i}"),
                success: i % 2 == 0,
            });
        }));
    }
    for h in handles {
        h.join().expect("thread joins");
    }
}
