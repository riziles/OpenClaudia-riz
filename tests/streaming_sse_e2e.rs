//! End-to-end tests for the streaming SSE event-translation
//! pipeline: realistic Anthropic event sequences driven through
//! `AnthropicToolAccumulator` and verified end-to-end against the
//! accumulated text + finalized `tool_calls`.
//!
//! Sprint 33 of the verification effort.
//!
//! `tests/tools_e2e.rs` (sprint-2) pinned the basic chunk-boundary
//! invariants. This file exercises the FULL realistic event flow
//! the live Anthropic streaming API produces:
//!
//!   1. `message_start` (with `input_tokens`)
//!   2. `content_block_start` (text)
//!   3. N × `content_block_delta` (`text_delta`)
//!   4. `content_block_stop`
//!   5. `content_block_start` (`tool_use`)
//!   6. N × `content_block_delta` (`input_json_delta`)
//!   7. `content_block_stop`
//!   8. `message_delta` (with `stop_reason` + `output_tokens`)
//!   9. `message_stop`

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::proxy::extract_usage_from_sse_event;
use openclaudia::tools::AnthropicToolAccumulator;
use serde_json::{json, Value};

// ───────────────────────────────────────────────────────────────────────────
// Helpers
// ───────────────────────────────────────────────────────────────────────────

/// Drive a sequence of events through the accumulator and return
/// the printed-text fragments alongside the accumulator for
/// further assertions.
fn drive(events: &[Value]) -> (AnthropicToolAccumulator, Vec<String>) {
    let mut acc = AnthropicToolAccumulator::new();
    let mut printed = Vec::new();
    for event in events {
        if let Some(text) = acc.process_event(event) {
            printed.push(text);
        }
    }
    (acc, printed)
}

// ───────────────────────────────────────────────────────────────────────────
// Section A — realistic text-only stream
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn realistic_text_only_stream_accumulates_full_text() {
    let events = vec![
        json!({
            "type": "message_start",
            "message": {"usage": {"input_tokens": 25, "output_tokens": 0}}
        }),
        json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {"type": "text", "text": ""}
        }),
        json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": "Hello"}
        }),
        json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": ", "}
        }),
        json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": "world!"}
        }),
        json!({"type": "content_block_stop", "index": 0}),
        json!({
            "type": "message_delta",
            "delta": {"stop_reason": "end_turn"},
            "usage": {"output_tokens": 7}
        }),
        json!({"type": "message_stop"}),
    ];

    let (acc, printed) = drive(&events);
    assert_eq!(
        acc.get_text(),
        "Hello, world!",
        "accumulator must concatenate text deltas in order"
    );
    // Printed text MUST equal the concatenated deltas exactly.
    let joined: String = printed.into_iter().collect();
    assert_eq!(joined, "Hello, world!");
    assert!(
        !acc.has_tool_use(),
        "text-only stream must not flag tool_use"
    );
}

#[test]
fn empty_text_stream_yields_empty_accumulator() {
    let events = vec![
        json!({
            "type": "message_start",
            "message": {"usage": {"input_tokens": 5, "output_tokens": 0}}
        }),
        json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {"type": "text", "text": ""}
        }),
        json!({"type": "content_block_stop", "index": 0}),
        json!({
            "type": "message_delta",
            "delta": {"stop_reason": "end_turn"}
        }),
    ];
    let (acc, _) = drive(&events);
    assert_eq!(acc.get_text(), "");
    assert!(!acc.has_tool_use());
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — realistic tool-use stream
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn realistic_tool_use_stream_finalizes_tool_call_correctly() {
    // The model says "Let me check", then issues a bash tool call.
    let events = vec![
        json!({
            "type": "message_start",
            "message": {"usage": {"input_tokens": 50, "output_tokens": 0}}
        }),
        // Block 0: text
        json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {"type": "text", "text": ""}
        }),
        json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": "Let me check"}
        }),
        json!({"type": "content_block_stop", "index": 0}),
        // Block 1: tool_use (split JSON across 3 chunks)
        json!({
            "type": "content_block_start",
            "index": 1,
            "content_block": {
                "type": "tool_use",
                "id": "toolu_abc",
                "name": "bash"
            }
        }),
        json!({
            "type": "content_block_delta",
            "index": 1,
            "delta": {"type": "input_json_delta", "partial_json": "{\"comm"}
        }),
        json!({
            "type": "content_block_delta",
            "index": 1,
            "delta": {"type": "input_json_delta", "partial_json": "and\":\"l"}
        }),
        json!({
            "type": "content_block_delta",
            "index": 1,
            "delta": {"type": "input_json_delta", "partial_json": "s -la\"}"}
        }),
        json!({"type": "content_block_stop", "index": 1}),
        json!({
            "type": "message_delta",
            "delta": {"stop_reason": "tool_use"},
            "usage": {"output_tokens": 20}
        }),
        json!({"type": "message_stop"}),
    ];

    let (acc, printed) = drive(&events);
    // Text portion: "Let me check"
    assert_eq!(acc.get_text(), "Let me check");
    assert_eq!(printed.join(""), "Let me check");

    // Tool calls: one bash call with the fully-assembled JSON.
    assert!(
        acc.has_tool_use(),
        "stream with tool_use block MUST flag has_tool_use"
    );
    let tool_calls = acc.finalize_tool_calls();
    assert_eq!(tool_calls.len(), 1, "exactly one tool call");
    assert_eq!(tool_calls[0].id, "toolu_abc");
    assert_eq!(tool_calls[0].function.name, "bash");
    assert_eq!(
        tool_calls[0].function.arguments, r#"{"command":"ls -la"}"#,
        "input_json must concatenate across all chunks"
    );
}

#[test]
fn parallel_tool_use_blocks_each_get_their_own_call() {
    // Two parallel tool_use blocks at index 0 and 1 — each gets
    // its own tool_call entry.
    let events = vec![
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 10, "output_tokens": 0}}}),
        json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {"type": "tool_use", "id": "t1", "name": "read_file"}
        }),
        json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "input_json_delta", "partial_json": "{\"path\":\"/a\"}"}
        }),
        json!({"type": "content_block_stop", "index": 0}),
        json!({
            "type": "content_block_start",
            "index": 1,
            "content_block": {"type": "tool_use", "id": "t2", "name": "list_files"}
        }),
        json!({
            "type": "content_block_delta",
            "index": 1,
            "delta": {"type": "input_json_delta", "partial_json": "{\"path\":\"/b\"}"}
        }),
        json!({"type": "content_block_stop", "index": 1}),
        json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}}),
    ];

    let (acc, _) = drive(&events);
    let tool_calls = acc.finalize_tool_calls();
    assert_eq!(
        tool_calls.len(),
        2,
        "two parallel tool_use blocks → 2 calls"
    );

    let names: Vec<&str> = tool_calls
        .iter()
        .map(|t| t.function.name.as_str())
        .collect();
    assert!(names.contains(&"read_file"));
    assert!(names.contains(&"list_files"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — mixed text + tool streams
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn mixed_text_then_tool_then_text_preserves_both() {
    // Model emits: text, tool call, more text — get_text returns
    // the FULL text (both segments) and finalize_tool_calls
    // returns the one tool call.
    let events = vec![
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 30, "output_tokens": 0}}}),
        // Text block 0
        json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {"type": "text", "text": ""}
        }),
        json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": "PROLOGUE "}
        }),
        json!({"type": "content_block_stop", "index": 0}),
        // Tool block 1
        json!({
            "type": "content_block_start",
            "index": 1,
            "content_block": {"type": "tool_use", "id": "tx", "name": "bash"}
        }),
        json!({
            "type": "content_block_delta",
            "index": 1,
            "delta": {"type": "input_json_delta", "partial_json": "{\"command\":\"pwd\"}"}
        }),
        json!({"type": "content_block_stop", "index": 1}),
        // Text block 2
        json!({
            "type": "content_block_start",
            "index": 2,
            "content_block": {"type": "text", "text": ""}
        }),
        json!({
            "type": "content_block_delta",
            "index": 2,
            "delta": {"type": "text_delta", "text": "EPILOGUE"}
        }),
        json!({"type": "content_block_stop", "index": 2}),
        json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}}),
    ];

    let (acc, printed) = drive(&events);
    // get_text concatenates ALL text blocks.
    assert_eq!(acc.get_text(), "PROLOGUE EPILOGUE");
    // Printed sequence comes ONLY from text deltas.
    assert_eq!(printed.join(""), "PROLOGUE EPILOGUE");
    // One tool call.
    let tcs = acc.finalize_tool_calls();
    assert_eq!(tcs.len(), 1);
    assert_eq!(tcs[0].id, "tx");
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — unknown / malformed events ignored gracefully
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn unknown_event_type_returns_none_without_state_change() {
    let mut acc = AnthropicToolAccumulator::new();
    let outcome = acc.process_event(&json!({
        "type": "ping",
        "data": "totally-unrecognised-event"
    }));
    assert!(outcome.is_none());
    assert!(acc.get_text().is_empty());
    assert!(!acc.has_tool_use());
}

#[test]
fn event_missing_type_field_returns_none() {
    let mut acc = AnthropicToolAccumulator::new();
    let outcome = acc.process_event(&json!({"no_type": "field"}));
    assert!(outcome.is_none(), "no `type` field → None");
}

#[test]
fn content_block_delta_with_missing_delta_returns_none() {
    let mut acc = AnthropicToolAccumulator::new();
    let outcome = acc.process_event(&json!({"type": "content_block_delta", "index": 0}));
    assert!(outcome.is_none(), "missing `delta` field → None");
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — extract_usage_from_sse_event in concert with stream
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn usage_extracted_from_start_and_delta_through_full_stream() {
    let events = vec![
        json!({
            "type": "message_start",
            "message": {"usage": {"input_tokens": 100, "output_tokens": 0}}
        }),
        json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {"type": "text", "text": ""}
        }),
        json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": "ok"}
        }),
        json!({"type": "content_block_stop", "index": 0}),
        json!({
            "type": "message_delta",
            "delta": {"stop_reason": "end_turn"},
            "usage": {"output_tokens": 42}
        }),
    ];

    let mut total_input = 0u64;
    let mut total_output = 0u64;
    for event in &events {
        if let Some(usage) = extract_usage_from_sse_event(event) {
            total_input += usage.input_tokens;
            total_output += usage.output_tokens;
        }
    }
    assert_eq!(total_input, 100, "input_tokens from message_start");
    assert_eq!(total_output, 42, "output_tokens from message_delta");
}
