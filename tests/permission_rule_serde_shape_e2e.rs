//! End-to-end tests for `permissions::PermissionRule` —
//! the 3-field serde shape (`tool` / `pattern` / `decision`)
//! with embedded `PermissionDecision`. Pins required-field
//! enforcement on deserialize + Clone preservation.
//!
//! Sprint 209 of the verification effort. Sprint 207
//! covered `PermissionDecision` wire; this file pins
//! `PermissionRule` envelope.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::permissions::{PermissionDecision, PermissionRule};
use serde_json::{json, Value};

fn rule(tool: &str, pattern: &str, decision: PermissionDecision) -> PermissionRule {
    PermissionRule {
        tool: tool.to_string(),
        pattern: pattern.to_string(),
        decision,
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section A — Required-field serialization
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn permission_rule_serializes_with_all_three_fields() {
    let r = rule("Bash", "ls *", PermissionDecision::Allow);
    let v: Value = serde_json::to_value(&r).expect("ser");
    assert!(v.get("tool").is_some());
    assert!(v.get("pattern").is_some());
    assert!(v.get("decision").is_some());
}

#[test]
fn permission_rule_tool_field_is_string() {
    let r = rule("Bash", "*", PermissionDecision::Allow);
    let v: Value = serde_json::to_value(&r).expect("ser");
    assert_eq!(v["tool"].as_str(), Some("Bash"));
}

#[test]
fn permission_rule_pattern_field_is_string() {
    let r = rule("Bash", "rm -rf *", PermissionDecision::Deny);
    let v: Value = serde_json::to_value(&r).expect("ser");
    assert_eq!(v["pattern"].as_str(), Some("rm -rf *"));
}

#[test]
fn permission_rule_decision_field_uses_snake_case_for_always_allow() {
    // PINS WIRE: nested decision serialization uses snake_case.
    let r = rule("Bash", "*", PermissionDecision::AlwaysAllow);
    let v: Value = serde_json::to_value(&r).expect("ser");
    assert_eq!(v["decision"].as_str(), Some("always_allow"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — Required-field deserialization
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn permission_rule_deserializes_from_complete_json() {
    let v = json!({
        "tool": "Bash",
        "pattern": "ls *",
        "decision": "allow"
    });
    let r: PermissionRule = serde_json::from_value(v).expect("de");
    assert_eq!(r.tool, "Bash");
    assert_eq!(r.pattern, "ls *");
    assert_eq!(r.decision, PermissionDecision::Allow);
}

#[test]
fn permission_rule_deserializes_always_allow_decision() {
    let v = json!({
        "tool": "Edit",
        "pattern": "/tmp/*",
        "decision": "always_allow"
    });
    let r: PermissionRule = serde_json::from_value(v).expect("de");
    assert_eq!(r.decision, PermissionDecision::AlwaysAllow);
}

#[test]
fn permission_rule_missing_tool_field_rejected() {
    let v = json!({
        "pattern": "*",
        "decision": "allow"
    });
    let outcome: Result<PermissionRule, _> = serde_json::from_value(v);
    assert!(outcome.is_err(), "missing tool MUST be rejected");
}

#[test]
fn permission_rule_missing_pattern_field_rejected() {
    let v = json!({
        "tool": "Bash",
        "decision": "allow"
    });
    let outcome: Result<PermissionRule, _> = serde_json::from_value(v);
    assert!(outcome.is_err(), "missing pattern MUST be rejected");
}

#[test]
fn permission_rule_missing_decision_field_rejected() {
    let v = json!({
        "tool": "Bash",
        "pattern": "*"
    });
    let outcome: Result<PermissionRule, _> = serde_json::from_value(v);
    assert!(outcome.is_err(), "missing decision MUST be rejected");
}

#[test]
fn permission_rule_with_invalid_decision_camel_case_rejected() {
    // PINS: nested decision still requires snake_case.
    let v = json!({
        "tool": "Bash",
        "pattern": "*",
        "decision": "alwaysAllow"
    });
    let outcome: Result<PermissionRule, _> = serde_json::from_value(v);
    assert!(outcome.is_err());
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — Round-trip across 3 decision variants
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn permission_rule_round_trip_with_allow_decision() {
    let original = rule("Bash", "echo *", PermissionDecision::Allow);
    let v: Value = serde_json::to_value(&original).expect("ser");
    let back: PermissionRule = serde_json::from_value(v).expect("de");
    assert_eq!(back.tool, original.tool);
    assert_eq!(back.pattern, original.pattern);
    assert_eq!(back.decision, original.decision);
}

#[test]
fn permission_rule_round_trip_with_deny_decision() {
    let original = rule("Write", "/etc/*", PermissionDecision::Deny);
    let v: Value = serde_json::to_value(&original).expect("ser");
    let back: PermissionRule = serde_json::from_value(v).expect("de");
    assert_eq!(back.decision, PermissionDecision::Deny);
}

#[test]
fn permission_rule_round_trip_with_always_allow_decision() {
    let original = rule("Edit", "*.md", PermissionDecision::AlwaysAllow);
    let v: Value = serde_json::to_value(&original).expect("ser");
    let back: PermissionRule = serde_json::from_value(v).expect("de");
    assert_eq!(back.decision, PermissionDecision::AlwaysAllow);
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — Edge-case field values
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn permission_rule_with_empty_pattern_round_trips() {
    let r = rule("Bash", "", PermissionDecision::Deny);
    let v: Value = serde_json::to_value(&r).expect("ser");
    let back: PermissionRule = serde_json::from_value(v).expect("de");
    assert_eq!(back.pattern, "");
}

#[test]
fn permission_rule_with_unicode_pattern_round_trips() {
    let r = rule("Edit", "日本語/*.md", PermissionDecision::Allow);
    let v: Value = serde_json::to_value(&r).expect("ser");
    let back: PermissionRule = serde_json::from_value(v).expect("de");
    assert_eq!(back.pattern, "日本語/*.md");
}

#[test]
fn permission_rule_with_special_chars_in_pattern_preserved() {
    // Glob meta-chars: * ? [ ]
    let r = rule("Bash", "[ab]?_*.log", PermissionDecision::Allow);
    let v: Value = serde_json::to_value(&r).expect("ser");
    let back: PermissionRule = serde_json::from_value(v).expect("de");
    assert_eq!(back.pattern, "[ab]?_*.log");
}

#[test]
fn permission_rule_with_long_pattern_round_trips() {
    let long = "x".repeat(1000);
    let r = rule("Bash", &long, PermissionDecision::Allow);
    let v: Value = serde_json::to_value(&r).expect("ser");
    let back: PermissionRule = serde_json::from_value(v).expect("de");
    assert_eq!(back.pattern.len(), 1000);
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — Clone derive
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn permission_rule_clone_preserves_all_three_fields() {
    let original = rule(
        "tool-clone-marker",
        "pattern-clone-marker",
        PermissionDecision::AlwaysAllow,
    );
    let cloned = original.clone();
    assert_eq!(cloned.tool, original.tool);
    assert_eq!(cloned.pattern, original.pattern);
    assert_eq!(cloned.decision, original.decision);
}

#[test]
fn permission_rule_clone_independent_of_original() {
    let original = rule("Bash", "*", PermissionDecision::Allow);
    let cloned = original.clone();
    // Both still usable.
    let _ = &original.tool;
    let _ = &cloned.tool;
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — Field-level value preservation
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn permission_rule_field_order_in_json_serialization_includes_all_three() {
    // PINS: not pinning order, but pinning that 3 fields are emitted.
    let r = rule("X", "Y", PermissionDecision::Allow);
    let s = serde_json::to_string(&r).expect("ser");
    assert!(s.contains("\"tool\""));
    assert!(s.contains("\"pattern\""));
    assert!(s.contains("\"decision\""));
}

#[test]
fn permission_rule_debug_includes_all_fields() {
    let r = rule("tool-dbg", "pattern-dbg", PermissionDecision::AlwaysAllow);
    let d = format!("{r:?}");
    assert!(d.contains("PermissionRule"));
    assert!(d.contains("tool-dbg"));
    assert!(d.contains("pattern-dbg"));
    assert!(d.contains("AlwaysAllow"));
}
