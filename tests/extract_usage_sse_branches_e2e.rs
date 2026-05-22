//! End-to-end tests for `proxy::extract_usage_from_sse_event` —
//! all 3 branches: Anthropic `message_delta` (output-only),
//! Anthropic `message_start` (`input_tokens` plus
//! `cache_read_input_tokens` plus `cache_creation_input_tokens`),
//! and `OpenAI` final-chunk usage fallback. Pins documented
//! short-circuit behavior plus the require-positive-total
//! invariant.
//!
//! Sprint 172 of the verification effort. Sprint 18 had 5
//! basic tests; this file fills the cache-read/cache-write
//! branches + the `OpenAI` fallback + cross-branch isolation
//! + return-tuple cache propagation.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::proxy::extract_usage_from_sse_event;
use serde_json::json;

// ───────────────────────────────────────────────────────────────────────────
// Section A — Anthropic message_delta branch
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn message_delta_with_positive_output_tokens_returns_some() {
    let event = json!({
        "type": "message_delta",
        "usage": {"output_tokens": 42}
    });
    let usage = extract_usage_from_sse_event(&event).expect("Some");
    assert_eq!(usage.output_tokens, 42);
    // PINS DOC: message_delta carries only output; input=0.
    assert_eq!(usage.input_tokens, 0);
    assert_eq!(usage.cache_read_tokens, 0);
    assert_eq!(usage.cache_write_tokens, 0);
}

#[test]
fn message_delta_with_zero_output_tokens_returns_none() {
    // PINS DOC: zero output is treated as "no usage yet".
    let event = json!({
        "type": "message_delta",
        "usage": {"output_tokens": 0}
    });
    assert!(extract_usage_from_sse_event(&event).is_none());
}

#[test]
fn message_delta_with_missing_usage_field_returns_none() {
    let event = json!({"type": "message_delta"});
    assert!(extract_usage_from_sse_event(&event).is_none());
}

#[test]
fn message_delta_with_non_numeric_output_treated_as_zero() {
    let event = json!({
        "type": "message_delta",
        "usage": {"output_tokens": "not a number"}
    });
    assert!(extract_usage_from_sse_event(&event).is_none());
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — Anthropic message_start branch (input + cache)
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn message_start_with_input_tokens_only_returns_some_with_input() {
    let event = json!({
        "type": "message_start",
        "message": {
            "usage": {"input_tokens": 100}
        }
    });
    let usage = extract_usage_from_sse_event(&event).expect("Some");
    assert_eq!(usage.input_tokens, 100);
    // PINS DOC: message_start has no output_tokens (output
    // streams on subsequent chunks).
    assert_eq!(usage.output_tokens, 0);
}

#[test]
fn message_start_with_cache_read_propagates_to_output_struct() {
    let event = json!({
        "type": "message_start",
        "message": {
            "usage": {
                "input_tokens": 50,
                "cache_read_input_tokens": 1000
            }
        }
    });
    let usage = extract_usage_from_sse_event(&event).expect("Some");
    assert_eq!(usage.input_tokens, 50);
    // PINS WIRE: cache_read_input_tokens → cache_read_tokens.
    assert_eq!(usage.cache_read_tokens, 1000);
    assert_eq!(usage.cache_write_tokens, 0);
}

#[test]
fn message_start_with_cache_creation_propagates_to_write_field() {
    let event = json!({
        "type": "message_start",
        "message": {
            "usage": {
                "input_tokens": 50,
                "cache_creation_input_tokens": 500
            }
        }
    });
    let usage = extract_usage_from_sse_event(&event).expect("Some");
    assert_eq!(usage.cache_write_tokens, 500);
    assert_eq!(usage.cache_read_tokens, 0);
}

#[test]
fn message_start_with_only_cache_no_input_still_returns_some() {
    // PINS DOC: any of input / cache_read / cache_write > 0
    // qualifies. Pure-cache-hit chunk (input=0) still emits.
    let event = json!({
        "type": "message_start",
        "message": {
            "usage": {
                "input_tokens": 0,
                "cache_read_input_tokens": 800
            }
        }
    });
    let usage = extract_usage_from_sse_event(&event).expect("Some");
    assert_eq!(usage.input_tokens, 0);
    assert_eq!(usage.cache_read_tokens, 800);
}

#[test]
fn message_start_with_all_zeros_returns_none() {
    let event = json!({
        "type": "message_start",
        "message": {
            "usage": {
                "input_tokens": 0,
                "cache_read_input_tokens": 0,
                "cache_creation_input_tokens": 0
            }
        }
    });
    assert!(extract_usage_from_sse_event(&event).is_none());
}

#[test]
fn message_start_without_message_field_returns_none() {
    let event = json!({"type": "message_start"});
    assert!(extract_usage_from_sse_event(&event).is_none());
}

#[test]
fn message_start_with_message_but_no_usage_returns_none() {
    let event = json!({
        "type": "message_start",
        "message": {"id": "msg_123"}
    });
    assert!(extract_usage_from_sse_event(&event).is_none());
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — `OpenAI` final-chunk usage fallback
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn openai_final_chunk_with_usage_returns_some() {
    // PINS WIRE: `OpenAI` emits usage on the final chunk when
    // stream_options.include_usage is true.
    let event = json!({
        "id": "chatcmpl-xyz",
        "object": "chat.completion.chunk",
        "usage": {
            "prompt_tokens": 100,
            "completion_tokens": 50,
            "total_tokens": 150
        }
    });
    let usage = extract_usage_from_sse_event(&event).expect("Some");
    assert!(usage.input_tokens > 0 || usage.output_tokens > 0);
}

#[test]
fn openai_chunk_with_usage_zero_total_returns_none() {
    // PINS DOC: require positive total — a usage:{0,0,0} object
    // is still "no usage reported".
    let event = json!({
        "id": "chatcmpl-zero",
        "usage": {
            "prompt_tokens": 0,
            "completion_tokens": 0,
            "total_tokens": 0
        }
    });
    assert!(extract_usage_from_sse_event(&event).is_none());
}

#[test]
fn openai_chunk_with_usage_as_non_object_returns_none() {
    // PINS GUARD: usage field must be an object.
    let event = json!({
        "id": "chatcmpl-bad",
        "usage": "not an object"
    });
    assert!(extract_usage_from_sse_event(&event).is_none());
}

#[test]
fn openai_chunk_with_usage_as_null_returns_none() {
    let event = json!({
        "id": "chatcmpl-null",
        "usage": null
    });
    assert!(extract_usage_from_sse_event(&event).is_none());
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — Cross-branch isolation
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn no_type_field_falls_through_to_openai_branch() {
    // PINS PRIORITY: missing "type" → not Anthropic → `OpenAI`
    // fallback kicks in.
    let event = json!({
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 5,
            "total_tokens": 15
        }
    });
    let usage = extract_usage_from_sse_event(&event).expect("Some");
    assert!(usage.input_tokens > 0 || usage.output_tokens > 0);
}

#[test]
fn anthropic_message_delta_short_circuits_before_openai_branch() {
    // PINS ORDER: message_delta with positive output wins
    // even if a top-level "usage" object is also present.
    let event = json!({
        "type": "message_delta",
        "usage": {
            "output_tokens": 99,
            "prompt_tokens": 200
        }
    });
    let usage = extract_usage_from_sse_event(&event).expect("Some");
    assert_eq!(usage.output_tokens, 99);
}

#[test]
fn unrelated_event_type_returns_none() {
    let event = json!({
        "type": "ping",
        "data": {"counter": 5}
    });
    assert!(extract_usage_from_sse_event(&event).is_none());
}

#[test]
fn empty_object_event_returns_none() {
    let event = json!({});
    assert!(extract_usage_from_sse_event(&event).is_none());
}

#[test]
fn json_null_event_returns_none() {
    let event = serde_json::Value::Null;
    assert!(extract_usage_from_sse_event(&event).is_none());
}

#[test]
fn json_array_event_returns_none() {
    let event = json!([1, 2, 3]);
    assert!(extract_usage_from_sse_event(&event).is_none());
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — Combined cache_read + cache_write on message_start
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn message_start_full_cache_set_propagates_all_fields() {
    let event = json!({
        "type": "message_start",
        "message": {
            "usage": {
                "input_tokens": 25,
                "cache_read_input_tokens": 1500,
                "cache_creation_input_tokens": 750
            }
        }
    });
    let usage = extract_usage_from_sse_event(&event).expect("Some");
    assert_eq!(usage.input_tokens, 25);
    assert_eq!(usage.cache_read_tokens, 1500);
    assert_eq!(usage.cache_write_tokens, 750);
    assert_eq!(usage.output_tokens, 0);
}

#[test]
fn message_start_huge_token_count_no_overflow() {
    let event = json!({
        "type": "message_start",
        "message": {
            "usage": {"input_tokens": u64::MAX}
        }
    });
    let usage = extract_usage_from_sse_event(&event).expect("Some");
    assert_eq!(usage.input_tokens, u64::MAX);
}
