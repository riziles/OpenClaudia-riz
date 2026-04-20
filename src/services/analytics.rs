//! Analytics sink — where lifecycle + usage events get recorded.
//!
//! Port of Claude Code's `services/analytics/` layer. OC keeps this
//! minimal: a typed [`AnalyticsEvent`] enum covering the events we
//! actually emit, and two default impls:
//!
//! - [`NoopAnalytics`]: discards every event. Used in tests and
//!   headless invocations where recording would skew results or leak
//!   user data. This is the `ServiceRegistry` default.
//! - [`TracingAnalytics`]: forwards each event as a `tracing::info!`
//!   span. Lets operators turn analytics on via `RUST_LOG` without
//!   hauling in a full telemetry library.
//!
//! Callers invoke via `ServiceRegistry::analytics().record(...)` —
//! the sink lives behind an `Arc<dyn AnalyticsSink>` so a test can
//! substitute a recording impl without changing the call sites.

/// Structured event variants. New fields or new variants land here
/// so the type system forces every sink to handle them — a stringly-
/// typed event bag would silently drop unknown events.
#[derive(Debug, Clone)]
pub enum AnalyticsEvent {
    /// A new session started. Payload: session id.
    SessionStart { session_id: String },
    /// A session ended. Payload: session id + message count.
    SessionEnd { session_id: String, messages: usize },
    /// A tool ran. Payload: tool name + success bit.
    ToolUsed { tool: String, success: bool },
    /// User-facing prompt submitted. Payload: char length of the
    /// final prompt text (not the text itself — that's PII).
    PromptSubmitted { prompt_chars: usize },
    /// Context was compacted. Payload: trigger + tokens freed.
    ContextCompacted {
        trigger: &'static str,
        tokens_freed: usize,
    },
    /// API request sent to the provider. Payload: provider string +
    /// model name (no secrets; headers are logged elsewhere).
    ApiRequest { provider: String, model: String },
    /// An expected thinking burst happened. Payload: budget hint
    /// used. Mirrors Claude Code's `tengu_thinking` analytics event.
    ThinkingEmitted { budget: u32 },
}

/// Sink trait. Single required method — [`AnalyticsSink::record`].
/// Implementors decide what to do with events. Must be `Send + Sync`
/// so the `Arc<dyn AnalyticsSink>` can cross thread / task boundaries.
pub trait AnalyticsSink: Send + Sync {
    /// Record a single event. Must not panic; misbehaving sinks
    /// shouldn't bring down the caller. Sink impls that can fail
    /// (network IO, etc.) should buffer + log-on-failure internally.
    fn record(&self, event: AnalyticsEvent);
}

/// No-op sink — the `ServiceRegistry` default. Exists so callers can
/// record unconditionally without a `Some(sink)` check.
pub struct NoopAnalytics;

impl AnalyticsSink for NoopAnalytics {
    fn record(&self, _event: AnalyticsEvent) {
        // Intentionally empty — the Claude Code `NoopSink` equivalent.
    }
}

/// Forwards events to the `tracing` subscriber as structured fields
/// under a stable target (`openclaudia::analytics`) so operators can
/// filter them via `RUST_LOG=openclaudia::analytics=info` without
/// flipping every tracing span on.
pub struct TracingAnalytics;

impl AnalyticsSink for TracingAnalytics {
    fn record(&self, event: AnalyticsEvent) {
        match event {
            AnalyticsEvent::SessionStart { session_id } => {
                tracing::info!(
                    target: "openclaudia::analytics",
                    event = "session_start",
                    session_id = %session_id
                );
            }
            AnalyticsEvent::SessionEnd {
                session_id,
                messages,
            } => {
                tracing::info!(
                    target: "openclaudia::analytics",
                    event = "session_end",
                    session_id = %session_id,
                    messages
                );
            }
            AnalyticsEvent::ToolUsed { tool, success } => {
                tracing::info!(
                    target: "openclaudia::analytics",
                    event = "tool_used",
                    tool = %tool,
                    success
                );
            }
            AnalyticsEvent::PromptSubmitted { prompt_chars } => {
                tracing::info!(
                    target: "openclaudia::analytics",
                    event = "prompt_submitted",
                    prompt_chars
                );
            }
            AnalyticsEvent::ContextCompacted {
                trigger,
                tokens_freed,
            } => {
                tracing::info!(
                    target: "openclaudia::analytics",
                    event = "context_compacted",
                    trigger,
                    tokens_freed
                );
            }
            AnalyticsEvent::ApiRequest { provider, model } => {
                tracing::info!(
                    target: "openclaudia::analytics",
                    event = "api_request",
                    provider = %provider,
                    model = %model
                );
            }
            AnalyticsEvent::ThinkingEmitted { budget } => {
                tracing::info!(
                    target: "openclaudia::analytics",
                    event = "thinking_emitted",
                    budget
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct Recording {
        events: Mutex<Vec<AnalyticsEvent>>,
    }

    impl AnalyticsSink for Recording {
        fn record(&self, event: AnalyticsEvent) {
            self.events.lock().unwrap().push(event);
        }
    }

    #[test]
    fn noop_sink_discards_events() {
        // Compiles if the trait object is Send + Sync (required for
        // Arc<dyn AnalyticsSink>). Record panics aren't a concern —
        // NoopAnalytics has no body.
        let sink: Box<dyn AnalyticsSink> = Box::new(NoopAnalytics);
        sink.record(AnalyticsEvent::SessionStart {
            session_id: "x".to_string(),
        });
    }

    #[test]
    fn tracing_sink_handles_all_variants() {
        // Exhaustive record call per variant: a missing arm in
        // TracingAnalytics::record would fail to compile. Running
        // each event here catches runtime panics (slice OOB,
        // unwrap on untrusted fields) even though most variants are
        // trivial.
        let sink = TracingAnalytics;
        sink.record(AnalyticsEvent::SessionStart {
            session_id: "s".to_string(),
        });
        sink.record(AnalyticsEvent::SessionEnd {
            session_id: "s".to_string(),
            messages: 3,
        });
        sink.record(AnalyticsEvent::ToolUsed {
            tool: "bash".to_string(),
            success: true,
        });
        sink.record(AnalyticsEvent::PromptSubmitted { prompt_chars: 42 });
        sink.record(AnalyticsEvent::ContextCompacted {
            trigger: "auto",
            tokens_freed: 1000,
        });
        sink.record(AnalyticsEvent::ApiRequest {
            provider: "anthropic".to_string(),
            model: "claude-opus-4-6".to_string(),
        });
        sink.record(AnalyticsEvent::ThinkingEmitted { budget: 31_999 });
    }

    #[test]
    fn recording_sink_captures_in_order() {
        let rec = Recording {
            events: Mutex::new(Vec::new()),
        };
        rec.record(AnalyticsEvent::SessionStart {
            session_id: "a".to_string(),
        });
        rec.record(AnalyticsEvent::ToolUsed {
            tool: "read_file".to_string(),
            success: false,
        });
        let events = rec.events.lock().unwrap();
        assert_eq!(events.len(), 2);
        match &events[0] {
            AnalyticsEvent::SessionStart { session_id } => assert_eq!(session_id, "a"),
            other => panic!("expected SessionStart, got {other:?}"),
        }
        match &events[1] {
            AnalyticsEvent::ToolUsed { tool, success } => {
                assert_eq!(tool, "read_file");
                assert!(!success);
            }
            other => panic!("expected ToolUsed, got {other:?}"),
        }
    }
}
