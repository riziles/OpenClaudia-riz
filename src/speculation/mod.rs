//! Speculation engine — crosslink #166.
//!
//! Provides a framework for **speculative execution**: pre-running likely tool
//! calls before the model finishes its turn so results are available without
//! blocking the user interaction loop.
//!
//! # Architecture (phased rollout)
//!
//! - **Phase 1 (this commit)**: trait surface + `NoOpSpeculationEngine` +
//!   integration hook in `pipeline::run_turn`. Nothing executes speculatively
//!   yet; the hook is a zero-cost no-op. Tests validate the contract.
//! - **Phase 2** (tracked in follow-up issue): `OverlaySpeculationEngine` —
//!   an in-memory snapshot of the relevant FS paths ("overlay") pre-populated
//!   before the model turn starts; speculative tool calls write into the
//!   overlay rather than the real FS. Requires the overlay-filesystem
//!   subsystem and a prediction heuristic.
//! - **Phase 3**: acceptance workflow — when the model's actual tool call
//!   matches the prediction, promote the overlay result without re-running
//!   the tool. When it doesn't match, discard the overlay silently.
//!
//! # Design invariants
//!
//! 1. The `SpeculationEngine` trait is `Send + Sync + 'static` so engines can
//!    live in an `Arc` shared across async task boundaries.
//! 2. `predict` is **pure** (no side-effects on call); side-effects happen only
//!    inside `submit_result` (recording hit/miss metrics) and the engine's
//!    internal async worker.
//! 3. A prediction is identified by a `PredictionId` newtype — opaque to
//!    callers, avoids stringly-typed IDs.
//! 4. `SpeculationHint` carries the minimum information the engine needs:
//!    the last N model messages plus the pending tool-call list (may be empty
//!    on the first message of a turn).
//! 5. The no-op implementation satisfies the full trait contract: every method
//!    is a constant-time no-op that returns the neutral value for its type.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;

// ─── Newtype identifiers ─────────────────────────────────────────────────────

/// Opaque identifier for a single speculation prediction.
///
/// The engine assigns these; callers carry them through to `submit_result`
/// so the engine can record hit/miss statistics without exposing its internal
/// accounting.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PredictionId(String);

impl PredictionId {
    /// Create from a raw string (engine-internal use only).
    #[must_use]
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    /// Borrow the underlying string for logging.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PredictionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ─── Input: what the engine sees ─────────────────────────────────────────────

/// A summary of one recent message, carrying only what the engine needs to
/// decide what to speculate about.
///
/// Full message bodies can be large; `SpeculationHint` intentionally avoids
/// including the raw `serde_json::Value` to keep `predict` allocations small.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSummary {
    /// "user" | "assistant" | "tool"
    pub role: String,
    /// First 256 bytes of the text content (enough for pattern-matching).
    pub content_prefix: String,
    /// Tool name if this is a tool-result message; `None` otherwise.
    pub tool_name: Option<String>,
}

/// Context passed to [`SpeculationEngine::predict`] at the start of a model turn.
///
/// The engine reads this to decide which (if any) tool call is worth
/// pre-running speculatively before the model responds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeculationHint {
    /// The last few turns of conversation (most-recent last).
    ///
    /// Capped at [`SpeculationConfig::context_depth`] entries to bound the
    /// cost of cloning into the engine.
    pub recent_messages: Vec<MessageSummary>,

    /// Tool calls already queued in the current partial response (may be
    /// empty at the start of a fresh turn).
    pub pending_tool_names: Vec<String>,
}

// ─── Output: what the engine predicts ────────────────────────────────────────

/// A single speculative prediction: the engine believes the model will call
/// `tool_name` with `predicted_args` in the next turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeculationPrediction {
    /// Opaque identifier assigned by the engine.
    pub id: PredictionId,
    /// Tool the engine expects the model to call.
    pub tool_name: String,
    /// Best-guess JSON arguments (may be partial or approximate).
    pub predicted_args: serde_json::Value,
    /// Engine's confidence score in [0.0, 1.0].
    ///
    /// Callers may use this to decide whether to actually pre-run the
    /// tool (Phase 2+). A score below a configured threshold should skip
    /// speculative execution even if a prediction exists.
    pub confidence: f32,
}

// ─── Feedback: what actually happened ────────────────────────────────────────

/// Whether the model's actual tool call matched the prediction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PredictionOutcome {
    /// Tool name and args matched closely enough to reuse the result.
    Hit,
    /// Tool name matched but args diverged — result discarded.
    PartialMiss,
    /// Tool name did not match — prediction was wrong.
    Miss,
    /// No tool call was made (model replied without calling a tool).
    NoToolCall,
}

/// Outcome of one model turn, fed back to the engine via
/// [`SpeculationEngine::submit_result`] for learning/statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActualOutcome {
    /// Which prediction this feedback is for.
    pub prediction_id: PredictionId,
    /// How closely the prediction matched reality.
    pub outcome: PredictionOutcome,
    /// The tool name the model actually called (or `None` if no tool call).
    pub actual_tool_name: Option<String>,
}

// ─── Configuration ────────────────────────────────────────────────────────────

/// Run-time configuration for the speculation engine.
///
/// Passed to the engine factory so the engine can tune its behaviour without
/// needing to reach into the global `AppConfig`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeculationConfig {
    /// Whether the engine is allowed to actually pre-run tools.
    ///
    /// `false` makes every engine behave like `NoOpSpeculationEngine`
    /// regardless of which implementation is in use.
    pub enabled: bool,

    /// Minimum confidence before a prediction is acted on (Phase 2+).
    ///
    /// Range [0.0, 1.0]. The default (0.7) means the engine must be at least
    /// 70% confident before it starts a speculative execution.
    pub confidence_threshold: f32,

    /// How many recent messages to include in each `SpeculationHint`.
    ///
    /// Larger values give the engine more context but cost more cloning work.
    pub context_depth: usize,

    /// Maximum number of concurrent speculative executions.
    ///
    /// Phase 2+ honours this limit. Phase 1 (no-op) ignores it.
    pub max_concurrent: usize,
}

impl Default for SpeculationConfig {
    fn default() -> Self {
        Self {
            enabled: false, // disabled by default until Phase 2 lands
            confidence_threshold: 0.70,
            context_depth: 6,
            max_concurrent: 2,
        }
    }
}

// ─── Engine trait ─────────────────────────────────────────────────────────────

/// Core contract for a speculation engine.
///
/// Implementors examine `SpeculationHint` data and optionally pre-run
/// tool calls in a sandboxed overlay. All methods must be callable from
/// an async context (the integration hook in `pipeline::run_turn` is async).
///
/// # Thread safety
///
/// The bound `Send + Sync + 'static` is required so an engine can live
/// behind `Arc<dyn SpeculationEngine>` and be shared across the TUI event
/// loop and the pipeline worker tasks.
pub trait SpeculationEngine: Send + Sync + 'static {
    /// Examine the current conversation hint and optionally predict the
    /// next tool call.
    ///
    /// Returning `None` means the engine declines to speculate for this
    /// turn (e.g., low confidence, disabled, or the hint doesn't match
    /// any known pattern).
    ///
    /// This call must be **fast** (≪ 1 ms) — it runs on the hot path
    /// of every model turn. Expensive work (I/O, HTTP) belongs in the
    /// engine's background worker, not here.
    fn predict(&self, hint: &SpeculationHint) -> Option<SpeculationPrediction>;

    /// Record the actual outcome of a turn so the engine can update its
    /// internal hit/miss statistics.
    ///
    /// Called once per turn, regardless of whether `predict` returned `Some`.
    /// When `predict` returned `None`, pass a synthetic `ActualOutcome` with
    /// `outcome = PredictionOutcome::NoToolCall` and `prediction_id` set to
    /// any sentinel (the engine must handle this gracefully).
    fn submit_result(&self, outcome: &ActualOutcome);

    /// Returns `true` if the engine has at least one speculative execution
    /// in flight.
    ///
    /// The pipeline integration hook polls this after the model responds
    /// to decide whether to wait for the overlay result (Phase 2+) or
    /// proceed immediately.
    fn has_pending(&self) -> bool;

    /// Human-readable name for this engine (used in tracing / diagnostics).
    fn name(&self) -> &'static str;

    /// Current hit rate over all recorded turns, in [0.0, 1.0].
    ///
    /// Returns `None` if no turns have been recorded yet.
    ///
    /// Primarily for metrics / TUI display; not on the hot path.
    fn hit_rate(&self) -> Option<f32>;
}

// ─── No-op implementation ─────────────────────────────────────────────────────

/// A speculation engine that does nothing.
///
/// Every method is a constant-time no-op returning the neutral value for
/// its type. Used as the default when `SpeculationConfig::enabled` is
/// `false`, or as the Phase 1 default until a real engine lands.
///
/// # Why not just `Option<Arc<dyn SpeculationEngine>>`?
///
/// Having a concrete type that satisfies the trait means the pipeline
/// integration code is always the same shape — no `if let Some(engine)` at
/// the call site, no dead-code branches to test separately.
#[derive(Debug, Default)]
pub struct NoOpSpeculationEngine;

impl SpeculationEngine for NoOpSpeculationEngine {
    fn predict(&self, _hint: &SpeculationHint) -> Option<SpeculationPrediction> {
        None
    }

    fn submit_result(&self, _outcome: &ActualOutcome) {
        // nothing to record
    }

    fn has_pending(&self) -> bool {
        false
    }

    fn name(&self) -> &'static str {
        "no-op"
    }

    fn hit_rate(&self) -> Option<f32> {
        None
    }
}

// ─── Factory ─────────────────────────────────────────────────────────────────

/// Build the active `SpeculationEngine` from a `SpeculationConfig`.
///
/// Phase 1 always returns `NoOpSpeculationEngine` wrapped in an `Arc`.
/// Phase 2+ will pattern-match on `cfg` to select the `OverlaySpeculationEngine`
/// when `cfg.enabled` is `true`.
#[must_use]
pub fn build_engine(cfg: &SpeculationConfig) -> Arc<dyn SpeculationEngine> {
    if cfg.enabled {
        tracing::warn!(
            "SpeculationEngine: enabled=true but OverlaySpeculationEngine is not yet \
             implemented (Phase 2, crosslink #166 follow-up). Falling back to no-op."
        );
    }
    Arc::new(NoOpSpeculationEngine)
}

// ─── Pipeline integration hook ────────────────────────────────────────────────

/// Build a `SpeculationHint` from a slice of raw OpenAI-format messages.
///
/// This is a pure function so it can be unit-tested without a live engine.
/// Only the last `depth` messages are included; each is summarised to avoid
/// cloning large `Value` trees.
#[must_use]
pub fn build_hint_from_messages(messages: &[serde_json::Value], depth: usize) -> SpeculationHint {
    let start = messages.len().saturating_sub(depth);
    let recent_messages = messages[start..]
        .iter()
        .map(|m| {
            let role = m
                .get("role")
                .and_then(|r| r.as_str())
                .unwrap_or("unknown")
                .to_string();

            let content_prefix = m
                .get("content")
                .and_then(|c| c.as_str())
                .map(|s| {
                    // Cap at 256 bytes on a char boundary to avoid slicing mid-char.
                    let end = s
                        .char_indices()
                        .map(|(i, _)| i)
                        .take_while(|&i| i < 256)
                        .last()
                        .map_or(0, |i| {
                            // advance past the last char
                            let ch = s[i..].chars().next().map_or(0, char::len_utf8);
                            i + ch
                        });
                    s[..end].to_string()
                })
                .unwrap_or_default();

            let tool_name = m
                .get("tool_calls")
                .and_then(|tc| tc.get(0))
                .and_then(|tc| tc.get("function"))
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str())
                .map(str::to_string);

            MessageSummary {
                role,
                content_prefix,
                tool_name,
            }
        })
        .collect();

    SpeculationHint {
        recent_messages,
        pending_tool_names: Vec::new(),
    }
}

/// Integration hook: run after a model turn completes.
///
/// In Phase 1 this is a fast no-op (the `NoOpSpeculationEngine` returns
/// `None` from `predict` immediately). In Phase 2+ the engine will use
/// the hint to decide whether to pre-run the next predicted tool call.
///
/// Called by `pipeline::run_turn` at the end of every turn, before the
/// `TurnResult` is returned to the caller.
pub fn after_turn(
    engine: &dyn SpeculationEngine,
    messages: &[serde_json::Value],
    tool_names_called: &[String],
    config: &SpeculationConfig,
) {
    if !config.enabled {
        return;
    }

    let hint = build_hint_from_messages(messages, config.context_depth);
    let prediction = engine.predict(&hint);

    // Record outcome for the previous turn.
    // When no prediction was made, record a no-call sentinel.
    let prediction_id = prediction
        .as_ref()
        .map_or_else(|| PredictionId::new("__no_prediction__"), |p| p.id.clone());

    let outcome_kind = if tool_names_called.is_empty() {
        PredictionOutcome::NoToolCall
    } else if let Some(pred) = &prediction {
        if tool_names_called.contains(&pred.tool_name) {
            PredictionOutcome::Hit
        } else {
            PredictionOutcome::Miss
        }
    } else {
        PredictionOutcome::NoToolCall
    };

    engine.submit_result(&ActualOutcome {
        prediction_id,
        outcome: outcome_kind,
        actual_tool_name: tool_names_called.first().cloned(),
    });
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── NoOpSpeculationEngine contract ────────────────────────────────────────

    #[test]
    fn noop_predict_always_returns_none() {
        let engine = NoOpSpeculationEngine;
        let hint = SpeculationHint {
            recent_messages: vec![MessageSummary {
                role: "user".into(),
                content_prefix: "read this file".into(),
                tool_name: None,
            }],
            pending_tool_names: vec!["read_file".into()],
        };
        assert!(
            engine.predict(&hint).is_none(),
            "NoOp engine must never predict a tool call"
        );
    }

    #[test]
    fn noop_has_pending_always_false() {
        let engine = NoOpSpeculationEngine;
        assert!(
            !engine.has_pending(),
            "NoOp engine must never report pending speculations"
        );
    }

    #[test]
    fn noop_hit_rate_returns_none() {
        let engine = NoOpSpeculationEngine;
        assert!(
            engine.hit_rate().is_none(),
            "NoOp engine has no statistics to report"
        );
    }

    #[test]
    fn noop_submit_result_does_not_panic() {
        let engine = NoOpSpeculationEngine;
        // Should complete without panicking for all outcome variants.
        for outcome in &[
            PredictionOutcome::Hit,
            PredictionOutcome::PartialMiss,
            PredictionOutcome::Miss,
            PredictionOutcome::NoToolCall,
        ] {
            engine.submit_result(&ActualOutcome {
                prediction_id: PredictionId::new("test-id"),
                outcome: *outcome,
                actual_tool_name: Some("bash".into()),
            });
        }
    }

    #[test]
    fn noop_name_is_static_str() {
        let engine = NoOpSpeculationEngine;
        assert_eq!(engine.name(), "no-op");
    }

    // ── build_engine factory ──────────────────────────────────────────────────

    #[test]
    fn build_engine_disabled_returns_noop() {
        let cfg = SpeculationConfig {
            enabled: false,
            ..Default::default()
        };
        let engine = build_engine(&cfg);
        // The no-op engine predicts nothing even when enabled is false.
        let hint = SpeculationHint {
            recent_messages: vec![],
            pending_tool_names: vec![],
        };
        assert!(engine.predict(&hint).is_none());
    }

    #[test]
    fn build_engine_enabled_falls_back_to_noop_in_phase1() {
        // Phase 1: enabled=true still returns a no-op because the overlay
        // engine is not yet implemented.
        let cfg = SpeculationConfig {
            enabled: true,
            ..Default::default()
        };
        let engine = build_engine(&cfg);
        assert!(!engine.has_pending(), "Phase 1 engine must have no pending");
    }

    // ── SpeculationConfig defaults ────────────────────────────────────────────

    #[test]
    fn config_default_is_disabled() {
        let cfg = SpeculationConfig::default();
        assert!(!cfg.enabled, "Speculation must be disabled by default");
    }

    #[test]
    fn config_default_confidence_threshold_is_reasonable() {
        let cfg = SpeculationConfig::default();
        assert!(
            (0.0..=1.0).contains(&cfg.confidence_threshold),
            "confidence_threshold must be in [0.0, 1.0]"
        );
        assert!(
            cfg.confidence_threshold >= 0.5,
            "default threshold should be at least 0.5 to avoid noisy speculation"
        );
    }

    #[test]
    fn config_default_context_depth_nonzero() {
        let cfg = SpeculationConfig::default();
        assert!(cfg.context_depth > 0, "context_depth must be > 0");
    }

    // ── build_hint_from_messages ──────────────────────────────────────────────

    #[test]
    fn hint_depth_caps_message_count() {
        let messages: Vec<serde_json::Value> = (0..20)
            .map(|i| json!({ "role": "user", "content": format!("msg {i}") }))
            .collect();
        let hint = build_hint_from_messages(&messages, 6);
        assert_eq!(
            hint.recent_messages.len(),
            6,
            "hint must contain exactly `depth` messages"
        );
    }

    #[test]
    fn hint_fewer_messages_than_depth() {
        let messages = vec![
            json!({ "role": "user", "content": "hello" }),
            json!({ "role": "assistant", "content": "hi" }),
        ];
        let hint = build_hint_from_messages(&messages, 6);
        assert_eq!(
            hint.recent_messages.len(),
            2,
            "hint must not fabricate messages when fewer than depth exist"
        );
    }

    #[test]
    fn hint_content_prefix_caps_at_256_bytes() {
        // 300 ASCII 'x' characters — should be trimmed to 256.
        let long_str = "x".repeat(300);
        let messages = vec![json!({ "role": "user", "content": long_str })];
        let hint = build_hint_from_messages(&messages, 4);
        let prefix = &hint.recent_messages[0].content_prefix;
        assert!(
            prefix.len() <= 256,
            "content_prefix must be at most 256 bytes, got {}",
            prefix.len()
        );
        // Must still be non-empty
        assert!(
            !prefix.is_empty(),
            "non-empty content must produce non-empty prefix"
        );
    }

    #[test]
    fn hint_empty_messages_produces_empty_hint() {
        let hint = build_hint_from_messages(&[], 6);
        assert!(hint.recent_messages.is_empty());
        assert!(hint.pending_tool_names.is_empty());
    }

    #[test]
    fn hint_tool_name_extracted_from_tool_calls() {
        let messages = vec![json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [
                {
                    "id": "call_1",
                    "type": "function",
                    "function": { "name": "read_file", "arguments": "{}" }
                }
            ]
        })];
        let hint = build_hint_from_messages(&messages, 4);
        assert_eq!(
            hint.recent_messages[0].tool_name.as_deref(),
            Some("read_file"),
            "tool_name must be extracted from tool_calls[0].function.name"
        );
    }

    #[test]
    fn hint_role_preserved() {
        let messages = vec![
            json!({ "role": "system", "content": "You are helpful." }),
            json!({ "role": "user", "content": "do something" }),
            json!({ "role": "assistant", "content": "ok" }),
        ];
        let hint = build_hint_from_messages(&messages, 10);
        let roles: Vec<&str> = hint
            .recent_messages
            .iter()
            .map(|m| m.role.as_str())
            .collect();
        assert_eq!(roles, vec!["system", "user", "assistant"]);
    }

    // ── after_turn hook ───────────────────────────────────────────────────────

    #[test]
    fn after_turn_disabled_is_noop() {
        // When enabled=false, after_turn returns immediately without calling
        // predict or submit_result. Verified indirectly: the no-op engine is
        // stateless so there's nothing to assert, but it must not panic.
        let engine = NoOpSpeculationEngine;
        let cfg = SpeculationConfig {
            enabled: false,
            ..Default::default()
        };
        let messages = vec![json!({ "role": "user", "content": "test" })];
        after_turn(&engine, &messages, &[], &cfg);
    }

    #[test]
    fn after_turn_enabled_with_no_tool_calls() {
        let engine = NoOpSpeculationEngine;
        let cfg = SpeculationConfig {
            enabled: true,
            ..Default::default()
        };
        let messages = vec![json!({ "role": "user", "content": "test" })];
        // Should complete without panic even when tool_names_called is empty.
        after_turn(&engine, &messages, &[], &cfg);
    }

    #[test]
    fn after_turn_enabled_with_tool_calls() {
        let engine = NoOpSpeculationEngine;
        let cfg = SpeculationConfig {
            enabled: true,
            ..Default::default()
        };
        let messages = vec![json!({ "role": "user", "content": "list files" })];
        let tool_names = vec!["list_files".to_string()];
        after_turn(&engine, &messages, &tool_names, &cfg);
    }

    // ── PredictionId ──────────────────────────────────────────────────────────

    #[test]
    fn prediction_id_display_matches_inner() {
        let id = PredictionId::new("abc-123");
        assert_eq!(id.to_string(), "abc-123");
        assert_eq!(id.as_str(), "abc-123");
    }

    #[test]
    fn prediction_id_equality() {
        let a = PredictionId::new("x");
        let b = PredictionId::new("x");
        let c = PredictionId::new("y");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    // ── Trait object safety ───────────────────────────────────────────────────

    #[test]
    fn engine_is_object_safe_via_arc() {
        // If this compiles, the trait is object-safe and Arc-compatible.
        let engine: Arc<dyn SpeculationEngine> = Arc::new(NoOpSpeculationEngine);
        assert_eq!(engine.name(), "no-op");
        assert!(!engine.has_pending());
    }

    #[test]
    fn engine_arc_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Arc<dyn SpeculationEngine>>();
    }
}
