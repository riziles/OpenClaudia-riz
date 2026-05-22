//! End-to-end tests for `services::PolicyEnforcer::evaluate_tool_call` —
//! the dry-run check that returns `Allow`/`Deny` without
//! mutating the counter. Pins the uncapped→Allow short-circuit,
//! the exactly-at-cap boundary, the per-session isolation,
//! and the count→evaluate→record→evaluate→re-eval pipeline.
//!
//! Sprint 200 milestone of the verification effort.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::services::policy::{EnterprisePolicy, PolicyDecision, PolicyEnforcer};
use std::collections::HashMap;

fn enforcer_with_cap(tool: &str, cap: usize) -> PolicyEnforcer {
    let mut tool_caps: HashMap<String, usize> = HashMap::new();
    tool_caps.insert(tool.to_string(), cap);
    let policy = EnterprisePolicy {
        tool_caps,
        ..EnterprisePolicy::default()
    };
    PolicyEnforcer::new(policy)
}

fn enforcer_no_caps() -> PolicyEnforcer {
    PolicyEnforcer::new(EnterprisePolicy::default())
}

// ───────────────────────────────────────────────────────────────────────────
// Section A — uncapped tools always allowed
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn evaluate_uncapped_tool_returns_allow() {
    // PINS: tool absent from tool_caps → Allow (uncapped).
    let e = enforcer_no_caps();
    assert_eq!(e.evaluate_tool_call("s1", "bash"), PolicyDecision::Allow);
}

#[test]
fn evaluate_uncapped_tool_returns_allow_after_many_invocations() {
    // PINS: many invocations don't matter if no cap configured.
    let e = enforcer_no_caps();
    for _ in 0..1000 {
        e.record_tool_invocation("s1", "bash");
    }
    assert_eq!(e.evaluate_tool_call("s1", "bash"), PolicyDecision::Allow);
}

#[test]
fn evaluate_tool_absent_from_caps_returns_allow_even_with_other_caps() {
    // PINS: bash is uncapped; edit_file is capped → bash still Allow.
    let e = enforcer_with_cap("edit_file", 1);
    assert_eq!(e.evaluate_tool_call("s1", "bash"), PolicyDecision::Allow);
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — capped tools: below / at / over
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn evaluate_below_cap_returns_allow() {
    let e = enforcer_with_cap("bash", 5);
    // 0 invocations done < cap 5.
    assert_eq!(e.evaluate_tool_call("s1", "bash"), PolicyDecision::Allow);
}

#[test]
fn evaluate_at_cap_returns_deny() {
    // PINS BOUND: predicate is `count >= cap`, so exactly = cap denies.
    let e = enforcer_with_cap("bash", 3);
    e.record_tool_invocation("s1", "bash");
    e.record_tool_invocation("s1", "bash");
    e.record_tool_invocation("s1", "bash");
    // Now count == cap; next eval MUST deny.
    assert_eq!(e.evaluate_tool_call("s1", "bash"), PolicyDecision::Deny);
}

#[test]
fn evaluate_one_under_cap_returns_allow_then_one_more_denies() {
    let e = enforcer_with_cap("bash", 2);
    // 0 invocations.
    assert_eq!(e.evaluate_tool_call("s1", "bash"), PolicyDecision::Allow);
    e.record_tool_invocation("s1", "bash");
    // 1 < 2.
    assert_eq!(e.evaluate_tool_call("s1", "bash"), PolicyDecision::Allow);
    e.record_tool_invocation("s1", "bash");
    // 2 >= 2 → deny.
    assert_eq!(e.evaluate_tool_call("s1", "bash"), PolicyDecision::Deny);
}

#[test]
fn evaluate_zero_cap_immediately_denies() {
    // PINS EDGE: cap=0 means tool is effectively disabled.
    let e = enforcer_with_cap("bash", 0);
    assert_eq!(e.evaluate_tool_call("s1", "bash"), PolicyDecision::Deny);
}

#[test]
fn evaluate_one_cap_allows_first_then_denies() {
    let e = enforcer_with_cap("bash", 1);
    assert_eq!(e.evaluate_tool_call("s1", "bash"), PolicyDecision::Allow);
    e.record_tool_invocation("s1", "bash");
    assert_eq!(e.evaluate_tool_call("s1", "bash"), PolicyDecision::Deny);
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — evaluate is dry-run (no mutation)
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn evaluate_does_not_consume_budget() {
    // PINS DOC: evaluate is dry-run — repeated calls don't decrement.
    let e = enforcer_with_cap("bash", 1);
    // 100 dry-run evals, all return Allow.
    for _ in 0..100 {
        assert_eq!(
            e.evaluate_tool_call("s1", "bash"),
            PolicyDecision::Allow,
            "dry-run MUST NOT consume the budget"
        );
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — Per-session isolation
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn evaluate_session_a_deny_does_not_affect_session_b_allow() {
    let e = enforcer_with_cap("bash", 1);
    e.record_tool_invocation("session-A", "bash");
    assert_eq!(
        e.evaluate_tool_call("session-A", "bash"),
        PolicyDecision::Deny
    );
    // Session B is fresh.
    assert_eq!(
        e.evaluate_tool_call("session-B", "bash"),
        PolicyDecision::Allow
    );
}

#[test]
fn evaluate_with_empty_session_id_treated_as_distinct_session() {
    let e = enforcer_with_cap("bash", 1);
    e.record_tool_invocation("real-session", "bash");
    // Empty session id has its own counter.
    assert_eq!(e.evaluate_tool_call("", "bash"), PolicyDecision::Allow);
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — Per-tool isolation within a session
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn evaluate_tool_a_count_does_not_count_against_tool_b() {
    let mut tool_caps: HashMap<String, usize> = HashMap::new();
    tool_caps.insert("bash".to_string(), 1);
    tool_caps.insert("edit_file".to_string(), 1);
    let policy = EnterprisePolicy {
        tool_caps,
        ..EnterprisePolicy::default()
    };
    let e = PolicyEnforcer::new(policy);
    e.record_tool_invocation("s1", "bash");
    // bash is at cap, edit_file still fresh.
    assert_eq!(e.evaluate_tool_call("s1", "bash"), PolicyDecision::Deny);
    assert_eq!(
        e.evaluate_tool_call("s1", "edit_file"),
        PolicyDecision::Allow
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — Reset session restores Allow
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn reset_session_after_deny_restores_allow() {
    let e = enforcer_with_cap("bash", 1);
    e.record_tool_invocation("s1", "bash");
    assert_eq!(e.evaluate_tool_call("s1", "bash"), PolicyDecision::Deny);
    e.reset_session("s1");
    assert_eq!(e.evaluate_tool_call("s1", "bash"), PolicyDecision::Allow);
}

#[test]
fn reset_session_only_affects_named_session() {
    let e = enforcer_with_cap("bash", 1);
    e.record_tool_invocation("s1", "bash");
    e.record_tool_invocation("s2", "bash");
    e.reset_session("s1");
    // s1 is fresh.
    assert_eq!(e.evaluate_tool_call("s1", "bash"), PolicyDecision::Allow);
    // s2 is still at cap.
    assert_eq!(e.evaluate_tool_call("s2", "bash"), PolicyDecision::Deny);
}

// ───────────────────────────────────────────────────────────────────────────
// Section G — Determinism (read-only path)
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn evaluate_is_deterministic_across_repeated_calls() {
    let e = enforcer_with_cap("bash", 3);
    let d1 = e.evaluate_tool_call("s1", "bash");
    let d2 = e.evaluate_tool_call("s1", "bash");
    let d3 = e.evaluate_tool_call("s1", "bash");
    assert_eq!(d1, d2);
    assert_eq!(d2, d3);
    assert_eq!(d1, PolicyDecision::Allow);
}

#[test]
fn record_then_evaluate_changes_decision_at_cap_boundary() {
    // PINS PIPELINE: record × N → evaluate flips at N==cap.
    let e = enforcer_with_cap("bash", 4);
    for n in 0..4 {
        // BEFORE recording, we still have budget.
        assert_eq!(
            e.evaluate_tool_call("s1", "bash"),
            PolicyDecision::Allow,
            "at n={n} (count <{n}), MUST still allow"
        );
        e.record_tool_invocation("s1", "bash");
    }
    // After 4 records, count==4==cap, denies.
    assert_eq!(e.evaluate_tool_call("s1", "bash"), PolicyDecision::Deny);
}

// ───────────────────────────────────────────────────────────────────────────
// Section H — Cross-method invariant
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn check_and_record_then_evaluate_consistent_decisions() {
    let e = enforcer_with_cap("bash", 2);
    let outcome1 = e.check_and_record_tool("s1", "bash");
    assert!(outcome1.is_ok());
    // After 1 record, eval = Allow (1 < 2).
    assert_eq!(e.evaluate_tool_call("s1", "bash"), PolicyDecision::Allow);
    let outcome2 = e.check_and_record_tool("s1", "bash");
    assert!(outcome2.is_ok());
    // After 2 records, eval = Deny (2 >= 2).
    assert_eq!(e.evaluate_tool_call("s1", "bash"), PolicyDecision::Deny);
}
