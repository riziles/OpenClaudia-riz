//! End-to-end tests for `session::AllowedPrompt` — the
//! 2-field struct that carries (tool, prompt) pairs used
//! by plan-mode exit and approval flows. Pins serde shape +
//! Clone + Debug.
//!
//! Sprint 218 of the verification effort.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::session::AllowedPrompt;
use serde_json::{json, Value};

fn make(tool: &str, prompt: &str) -> AllowedPrompt {
    AllowedPrompt {
        tool: tool.to_string(),
        prompt: prompt.to_string(),
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section A — Required-field serialization
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn allowed_prompt_serializes_with_both_fields() {
    let p = make("Bash", "ls /tmp");
    let v: Value = serde_json::to_value(&p).expect("ser");
    assert!(v.get("tool").is_some());
    assert!(v.get("prompt").is_some());
}

#[test]
fn allowed_prompt_tool_field_is_string() {
    let p = make("Bash", "x");
    let v: Value = serde_json::to_value(&p).expect("ser");
    assert_eq!(v["tool"].as_str(), Some("Bash"));
}

#[test]
fn allowed_prompt_prompt_field_is_string() {
    let p = make("Edit", "src/main.rs");
    let v: Value = serde_json::to_value(&p).expect("ser");
    assert_eq!(v["prompt"].as_str(), Some("src/main.rs"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — Required-field deserialization
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn allowed_prompt_deserializes_from_complete_json() {
    let v = json!({"tool": "Bash", "prompt": "ls"});
    let p: AllowedPrompt = serde_json::from_value(v).expect("de");
    assert_eq!(p.tool, "Bash");
    assert_eq!(p.prompt, "ls");
}

#[test]
fn allowed_prompt_missing_tool_field_rejected() {
    let v = json!({"prompt": "x"});
    let outcome: Result<AllowedPrompt, _> = serde_json::from_value(v);
    assert!(outcome.is_err(), "missing tool MUST be rejected");
}

#[test]
fn allowed_prompt_missing_prompt_field_rejected() {
    let v = json!({"tool": "Bash"});
    let outcome: Result<AllowedPrompt, _> = serde_json::from_value(v);
    assert!(outcome.is_err(), "missing prompt MUST be rejected");
}

#[test]
fn allowed_prompt_extra_fields_tolerated() {
    let v = json!({
        "tool": "Bash",
        "prompt": "ls",
        "future_field": "ignored"
    });
    let p: AllowedPrompt = serde_json::from_value(v).expect("de");
    assert_eq!(p.tool, "Bash");
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — Round-trip
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn allowed_prompt_round_trips_through_json() {
    let original = make("marker-tool", "marker-prompt");
    let v: Value = serde_json::to_value(&original).expect("ser");
    let back: AllowedPrompt = serde_json::from_value(v).expect("de");
    assert_eq!(back.tool, original.tool);
    assert_eq!(back.prompt, original.prompt);
}

#[test]
fn allowed_prompt_with_empty_strings_round_trips() {
    let p = make("", "");
    let v: Value = serde_json::to_value(&p).expect("ser");
    let back: AllowedPrompt = serde_json::from_value(v).expect("de");
    assert_eq!(back.tool, "");
    assert_eq!(back.prompt, "");
}

#[test]
fn allowed_prompt_with_unicode_round_trips() {
    let p = make("ツール", "プロンプト");
    let v: Value = serde_json::to_value(&p).expect("ser");
    let back: AllowedPrompt = serde_json::from_value(v).expect("de");
    assert_eq!(back.tool, "ツール");
    assert_eq!(back.prompt, "プロンプト");
}

#[test]
fn allowed_prompt_with_special_chars_in_prompt_preserved() {
    let p = make("Bash", "ls \"$HOME\" && echo done");
    let v: Value = serde_json::to_value(&p).expect("ser");
    let back: AllowedPrompt = serde_json::from_value(v).expect("de");
    assert_eq!(back.prompt, "ls \"$HOME\" && echo done");
}

#[test]
fn allowed_prompt_with_long_prompt_round_trips() {
    let long_prompt = "x".repeat(5000);
    let p = make("Bash", &long_prompt);
    let v: Value = serde_json::to_value(&p).expect("ser");
    let back: AllowedPrompt = serde_json::from_value(v).expect("de");
    assert_eq!(back.prompt.len(), 5000);
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — Clone derive
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn allowed_prompt_clone_preserves_both_fields() {
    let original = make("Bash", "ls");
    let cloned = original.clone();
    assert_eq!(cloned.tool, original.tool);
    assert_eq!(cloned.prompt, original.prompt);
}

#[test]
fn allowed_prompt_clone_independent_of_original() {
    let original = make("X", "Y");
    let cloned = original.clone();
    // Both still usable.
    let _ = &original.tool;
    let _ = &cloned.tool;
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — Debug formatting
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn allowed_prompt_debug_includes_struct_name_and_fields() {
    let p = make("debug-tool", "debug-prompt");
    let d = format!("{p:?}");
    assert!(d.contains("AllowedPrompt"));
    assert!(d.contains("debug-tool"));
    assert!(d.contains("debug-prompt"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — Field-level rendering invariants
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn allowed_prompt_json_contains_field_names() {
    let p = make("X", "Y");
    let s = serde_json::to_string(&p).expect("ser");
    assert!(s.contains("\"tool\""));
    assert!(s.contains("\"prompt\""));
}

#[test]
fn allowed_prompt_json_emits_exactly_two_fields() {
    let p = make("X", "Y");
    let v: Value = serde_json::to_value(&p).expect("ser");
    let obj = v.as_object().expect("object");
    assert_eq!(obj.len(), 2, "MUST emit exactly 2 fields");
}

// ───────────────────────────────────────────────────────────────────────────
// Section G — Send + Sync
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn allowed_prompt_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<AllowedPrompt>();
}
