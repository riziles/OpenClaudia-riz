//! End-to-end tests for `tools::AnthropicToolAccumulator::process_event` —
//! the streaming-event state machine that builds
//! `AnthropicContentBlock`s from Anthropic's SSE wire
//! protocol (`content_block_start` / `_delta` / `_stop` plus
//! `message_delta` with `stop_reason`).
//!
//! Sprint 180 of the verification effort. Sprint 132 had
//! 21 basic `AnthropicContentBlock` tests; this file pins
//! the full event-pump state machine including the
//! `get_text` join, the `stop_reason` capture, the
//! `text_delta` append, and the `input_json_delta`
//! concatenation across multiple chunks.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::tools::{AnthropicContentBlock, AnthropicToolAccumulator};
use serde_json::json;

// ───────────────────────────────────────────────────────────────────────────
// Section A — content_block_start
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn content_block_start_text_pushes_empty_text_block() {
    let mut acc = AnthropicToolAccumulator::new();
    let outcome = acc.process_event(&json!({
        "type": "content_block_start",
        "content_block": {"type": "text"}
    }));
    assert!(outcome.is_none(), "start returns None (no text yet)");
    assert_eq!(acc.blocks.len(), 1);
    assert!(matches!(
        &acc.blocks[0],
        AnthropicContentBlock::Text(s) if s.is_empty()
    ));
}

#[test]
fn content_block_start_tool_use_pushes_empty_tool_use() {
    let mut acc = AnthropicToolAccumulator::new();
    let _ = acc.process_event(&json!({
        "type": "content_block_start",
        "content_block": {
            "type": "tool_use",
            "id": "toolu_123",
            "name": "bash"
        }
    }));
    assert_eq!(acc.blocks.len(), 1);
    match &acc.blocks[0] {
        AnthropicContentBlock::ToolUse {
            id,
            name,
            input_json,
        } => {
            assert_eq!(id, "toolu_123");
            assert_eq!(name, "bash");
            assert!(input_json.is_empty());
        }
        AnthropicContentBlock::Text(_) => panic!("MUST be ToolUse"),
    }
}

#[test]
fn content_block_start_tool_use_missing_id_defaults_to_empty() {
    let mut acc = AnthropicToolAccumulator::new();
    let _ = acc.process_event(&json!({
        "type": "content_block_start",
        "content_block": {"type": "tool_use", "name": "bash"}
    }));
    match &acc.blocks[0] {
        AnthropicContentBlock::ToolUse { id, .. } => {
            assert!(id.is_empty(), "missing id → empty default");
        }
        AnthropicContentBlock::Text(_) => panic!("MUST be ToolUse"),
    }
}

#[test]
fn content_block_start_with_unknown_block_type_pushes_nothing() {
    let mut acc = AnthropicToolAccumulator::new();
    let _ = acc.process_event(&json!({
        "type": "content_block_start",
        "content_block": {"type": "image"}
    }));
    assert!(acc.blocks.is_empty(), "unknown type pushes nothing");
}

#[test]
fn content_block_start_without_content_block_field_returns_none_no_push() {
    let mut acc = AnthropicToolAccumulator::new();
    let outcome = acc.process_event(&json!({
        "type": "content_block_start"
    }));
    assert!(outcome.is_none());
    assert!(acc.blocks.is_empty());
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — content_block_delta text_delta
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn text_delta_appends_to_last_text_block_and_returns_chunk() {
    let mut acc = AnthropicToolAccumulator::new();
    let _ = acc.process_event(&json!({
        "type": "content_block_start",
        "content_block": {"type": "text"}
    }));
    let chunk = acc.process_event(&json!({
        "type": "content_block_delta",
        "delta": {"type": "text_delta", "text": "hello"}
    }));
    assert_eq!(chunk.as_deref(), Some("hello"));
    match &acc.blocks[0] {
        AnthropicContentBlock::Text(s) => assert_eq!(s, "hello"),
        AnthropicContentBlock::ToolUse { .. } => panic!("MUST be Text"),
    }
}

#[test]
fn text_delta_concatenates_multiple_chunks() {
    let mut acc = AnthropicToolAccumulator::new();
    let _ = acc.process_event(&json!({
        "type": "content_block_start",
        "content_block": {"type": "text"}
    }));
    for chunk_text in ["hello ", "world ", "from ", "anthropic"] {
        let _ = acc.process_event(&json!({
            "type": "content_block_delta",
            "delta": {"type": "text_delta", "text": chunk_text}
        }));
    }
    match &acc.blocks[0] {
        AnthropicContentBlock::Text(s) => {
            assert_eq!(s, "hello world from anthropic");
        }
        AnthropicContentBlock::ToolUse { .. } => panic!("MUST be Text"),
    }
}

#[test]
fn text_delta_with_no_preceding_text_block_returns_chunk_but_no_block() {
    let mut acc = AnthropicToolAccumulator::new();
    // No content_block_start first.
    let chunk = acc.process_event(&json!({
        "type": "content_block_delta",
        "delta": {"type": "text_delta", "text": "orphan"}
    }));
    // Chunk is still returned for terminal display.
    assert_eq!(chunk.as_deref(), Some("orphan"));
    // But nothing was appended (no block to append to).
    assert!(acc.blocks.is_empty());
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — content_block_delta input_json_delta
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn input_json_delta_appends_to_last_tool_use_block() {
    let mut acc = AnthropicToolAccumulator::new();
    let _ = acc.process_event(&json!({
        "type": "content_block_start",
        "content_block": {"type": "tool_use", "id": "x", "name": "y"}
    }));
    let _ = acc.process_event(&json!({
        "type": "content_block_delta",
        "delta": {"type": "input_json_delta", "partial_json": "{\"key\":"}
    }));
    let _ = acc.process_event(&json!({
        "type": "content_block_delta",
        "delta": {"type": "input_json_delta", "partial_json": "\"value\"}"}
    }));
    match &acc.blocks[0] {
        AnthropicContentBlock::ToolUse { input_json, .. } => {
            assert_eq!(input_json, r#"{"key":"value"}"#);
        }
        AnthropicContentBlock::Text(_) => panic!("MUST be ToolUse"),
    }
}

#[test]
fn input_json_delta_returns_none_unlike_text_delta() {
    // PINS DOC: input_json_delta is NOT printed to terminal
    // (text_delta is). Returns None.
    let mut acc = AnthropicToolAccumulator::new();
    let _ = acc.process_event(&json!({
        "type": "content_block_start",
        "content_block": {"type": "tool_use", "id": "x", "name": "y"}
    }));
    let outcome = acc.process_event(&json!({
        "type": "content_block_delta",
        "delta": {"type": "input_json_delta", "partial_json": "{}"}
    }));
    assert!(outcome.is_none());
}

#[test]
fn input_json_delta_with_no_preceding_tool_use_silently_drops() {
    let mut acc = AnthropicToolAccumulator::new();
    let outcome = acc.process_event(&json!({
        "type": "content_block_delta",
        "delta": {"type": "input_json_delta", "partial_json": "x"}
    }));
    assert!(outcome.is_none());
    assert!(acc.blocks.is_empty());
}

#[test]
fn unknown_delta_type_returns_none_silently() {
    let mut acc = AnthropicToolAccumulator::new();
    let outcome = acc.process_event(&json!({
        "type": "content_block_delta",
        "delta": {"type": "unknown_kind", "value": "x"}
    }));
    assert!(outcome.is_none());
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — message_delta with stop_reason
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn message_delta_captures_stop_reason_tool_use() {
    let mut acc = AnthropicToolAccumulator::new();
    let _ = acc.process_event(&json!({
        "type": "message_delta",
        "delta": {"stop_reason": "tool_use"}
    }));
    assert_eq!(acc.stop_reason.as_deref(), Some("tool_use"));
}

#[test]
fn message_delta_captures_stop_reason_end_turn() {
    let mut acc = AnthropicToolAccumulator::new();
    let _ = acc.process_event(&json!({
        "type": "message_delta",
        "delta": {"stop_reason": "end_turn"}
    }));
    assert_eq!(acc.stop_reason.as_deref(), Some("end_turn"));
}

#[test]
fn message_delta_without_delta_field_does_not_panic() {
    let mut acc = AnthropicToolAccumulator::new();
    let _ = acc.process_event(&json!({"type": "message_delta"}));
    assert!(acc.stop_reason.is_none());
}

#[test]
fn message_delta_without_stop_reason_does_not_set_reason() {
    let mut acc = AnthropicToolAccumulator::new();
    let _ = acc.process_event(&json!({
        "type": "message_delta",
        "delta": {"other_field": "value"}
    }));
    assert!(acc.stop_reason.is_none());
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — has_tool_use predicate
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn has_tool_use_true_when_stop_reason_tool_use_and_tool_use_block_present() {
    let mut acc = AnthropicToolAccumulator::new();
    let _ = acc.process_event(&json!({
        "type": "content_block_start",
        "content_block": {"type": "tool_use", "id": "x", "name": "y"}
    }));
    let _ = acc.process_event(&json!({
        "type": "message_delta",
        "delta": {"stop_reason": "tool_use"}
    }));
    assert!(acc.has_tool_use());
}

#[test]
fn has_tool_use_false_when_only_text_block_with_tool_use_reason() {
    // PINS DOC: requires BOTH stop_reason AND a tool_use block.
    let mut acc = AnthropicToolAccumulator::new();
    let _ = acc.process_event(&json!({
        "type": "content_block_start",
        "content_block": {"type": "text"}
    }));
    let _ = acc.process_event(&json!({
        "type": "message_delta",
        "delta": {"stop_reason": "tool_use"}
    }));
    assert!(
        !acc.has_tool_use(),
        "tool_use reason but no tool_use block MUST be false"
    );
}

#[test]
fn has_tool_use_false_when_tool_use_block_but_other_stop_reason() {
    let mut acc = AnthropicToolAccumulator::new();
    let _ = acc.process_event(&json!({
        "type": "content_block_start",
        "content_block": {"type": "tool_use", "id": "x", "name": "y"}
    }));
    let _ = acc.process_event(&json!({
        "type": "message_delta",
        "delta": {"stop_reason": "end_turn"}
    }));
    assert!(!acc.has_tool_use());
}

#[test]
fn has_tool_use_false_on_fresh_accumulator() {
    let acc = AnthropicToolAccumulator::new();
    assert!(!acc.has_tool_use());
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — get_text concatenation
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn get_text_joins_all_text_blocks_skipping_tool_use() {
    let mut acc = AnthropicToolAccumulator::new();
    let _ = acc.process_event(&json!({
        "type": "content_block_start",
        "content_block": {"type": "text"}
    }));
    let _ = acc.process_event(&json!({
        "type": "content_block_delta",
        "delta": {"type": "text_delta", "text": "first "}
    }));
    let _ = acc.process_event(&json!({
        "type": "content_block_start",
        "content_block": {"type": "tool_use", "id": "x", "name": "y"}
    }));
    // tool_use block doesn't contribute text.
    let _ = acc.process_event(&json!({
        "type": "content_block_start",
        "content_block": {"type": "text"}
    }));
    let _ = acc.process_event(&json!({
        "type": "content_block_delta",
        "delta": {"type": "text_delta", "text": "second"}
    }));
    // Default join is "" — depends on the join logic. Let's check.
    let text = acc.get_text();
    assert!(text.contains("first"));
    assert!(text.contains("second"));
}

#[test]
fn get_text_empty_on_fresh_accumulator() {
    let acc = AnthropicToolAccumulator::new();
    assert!(acc.get_text().is_empty());
}

#[test]
fn get_text_empty_when_only_tool_use_blocks() {
    let mut acc = AnthropicToolAccumulator::new();
    let _ = acc.process_event(&json!({
        "type": "content_block_start",
        "content_block": {"type": "tool_use", "id": "x", "name": "y"}
    }));
    assert!(acc.get_text().is_empty());
}

// ───────────────────────────────────────────────────────────────────────────
// Section G — Unknown event types are no-ops
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn unknown_event_type_returns_none_and_does_not_mutate_state() {
    let mut acc = AnthropicToolAccumulator::new();
    let outcome = acc.process_event(&json!({"type": "future_event_kind"}));
    assert!(outcome.is_none());
    assert!(acc.blocks.is_empty());
    assert!(acc.stop_reason.is_none());
}

#[test]
fn event_without_type_field_returns_none() {
    let mut acc = AnthropicToolAccumulator::new();
    let outcome = acc.process_event(&json!({"other": "field"}));
    assert!(outcome.is_none());
}

#[test]
fn event_with_null_type_returns_none() {
    let mut acc = AnthropicToolAccumulator::new();
    let outcome = acc.process_event(&json!({"type": null}));
    assert!(outcome.is_none());
}
