//! End-to-end tests for the enterprise `PolicyEnforcer`:
//! model-allowlist, token caps, per-tool invocation caps with
//! per-session isolation.
//!
//! Sprint 36 of the verification effort.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::services::policy::{
    EnterprisePolicy, PolicyDecision, PolicyEnforcer, PolicyError,
};
use std::collections::HashSet;

// ───────────────────────────────────────────────────────────────────────────
// Helpers
// ───────────────────────────────────────────────────────────────────────────

fn policy_with_model_allowlist(models: &[&str]) -> EnterprisePolicy {
    EnterprisePolicy {
        model_allowlist: models.iter().map(|s| (*s).to_string()).collect(),
        ..EnterprisePolicy::default()
    }
}

fn policy_with_request_cap(cap: usize) -> EnterprisePolicy {
    EnterprisePolicy {
        max_request_tokens: Some(cap),
        ..EnterprisePolicy::default()
    }
}

fn policy_with_session_cap(cap: usize) -> EnterprisePolicy {
    EnterprisePolicy {
        max_session_tokens: Some(cap),
        ..EnterprisePolicy::default()
    }
}

fn policy_with_tool_cap(tool: &str, cap: usize) -> EnterprisePolicy {
    let mut tool_caps = std::collections::HashMap::new();
    tool_caps.insert(tool.to_string(), cap);
    EnterprisePolicy {
        tool_caps,
        ..EnterprisePolicy::default()
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section A — model-allowlist
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn empty_allowlist_admits_every_model() {
    let policy = EnterprisePolicy::default();
    assert!(
        policy.model_allowlist.is_empty(),
        "default policy must have empty allowlist"
    );
    // Every model passes when no allowlist is configured.
    for model in &["claude-3-opus", "gpt-4", "random-model-xyz"] {
        assert!(
            policy.check_model(model).is_ok(),
            "no allowlist must admit {model}"
        );
    }
}

#[test]
fn populated_allowlist_admits_listed_models_only() {
    let policy =
        policy_with_model_allowlist(&["claude-3-5-sonnet-20241022", "claude-3-opus-20240229"]);
    // Listed → admit.
    for model in &["claude-3-5-sonnet-20241022", "claude-3-opus-20240229"] {
        assert!(
            policy.check_model(model).is_ok(),
            "{model} must be admitted"
        );
    }
    // Unlisted → deny with ModelDenied carrying the model name.
    let outcome = policy.check_model("gpt-4o");
    let Err(PolicyError::ModelDenied { model }) = outcome else {
        panic!("unlisted model must error ModelDenied; got {outcome:?}");
    };
    assert_eq!(model, "gpt-4o", "error must echo the offending model name");
}

#[test]
fn allowlist_match_is_case_sensitive() {
    // The allowlist uses HashSet<String> exact match — case
    // counts. This is the documented contract; pin it so a
    // future change to case-folding surfaces.
    let policy = policy_with_model_allowlist(&["Claude-3-Opus"]);
    assert!(
        policy.check_model("Claude-3-Opus").is_ok(),
        "exact-case match must admit"
    );
    let outcome = policy.check_model("claude-3-opus");
    assert!(
        outcome.is_err(),
        "case mismatch MUST deny (allowlist is case-sensitive); got {outcome:?}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — request-token cap
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn no_request_cap_admits_any_size() {
    let policy = EnterprisePolicy::default();
    // Even a billion tokens passes when no cap configured.
    assert!(policy.check_request_tokens(1_000_000_000).is_ok());
}

#[test]
fn request_cap_admits_at_or_below_cap() {
    let policy = policy_with_request_cap(1000);
    assert!(policy.check_request_tokens(0).is_ok());
    assert!(policy.check_request_tokens(500).is_ok());
    assert!(
        policy.check_request_tokens(1000).is_ok(),
        "exactly-cap must admit"
    );
}

#[test]
fn request_cap_refuses_above_cap() {
    let policy = policy_with_request_cap(1000);
    let outcome = policy.check_request_tokens(1001);
    let Err(PolicyError::TokenCapExceeded {
        estimated,
        cap,
        scope,
    }) = outcome
    else {
        panic!("over-cap must error TokenCapExceeded; got {outcome:?}");
    };
    assert_eq!(estimated, 1001);
    assert_eq!(cap, 1000);
    assert_eq!(scope, "request", "scope must be 'request'");
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — session-token cap
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn session_cap_refuses_above_cap_with_correct_scope() {
    let policy = policy_with_session_cap(50_000);
    let outcome = policy.check_session_tokens(50_001);
    let Err(PolicyError::TokenCapExceeded {
        estimated,
        cap,
        scope,
    }) = outcome
    else {
        panic!("over-session-cap must error; got {outcome:?}");
    };
    assert_eq!(estimated, 50_001);
    assert_eq!(cap, 50_000);
    assert_eq!(scope, "session", "scope must be 'session'");
}

#[test]
fn session_cap_at_exact_value_admits() {
    let policy = policy_with_session_cap(1_000_000);
    assert!(
        policy.check_session_tokens(1_000_000).is_ok(),
        "exactly-cap must admit (strict greater-than refuses)"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — PolicyEnforcer tool-cap counters
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn no_tool_cap_admits_unlimited_invocations() {
    let enforcer = PolicyEnforcer::new(EnterprisePolicy::default());
    for _ in 0..100 {
        assert_eq!(
            enforcer.evaluate_tool_call("session-1", "bash"),
            PolicyDecision::Allow,
            "uncapped tool MUST always allow"
        );
        enforcer.record_tool_invocation("session-1", "bash");
    }
}

#[test]
fn tool_cap_admits_until_count_reaches_cap_then_denies() {
    let enforcer = PolicyEnforcer::new(policy_with_tool_cap("bash", 3));
    // First 3 invocations: Allow (cap is exclusive — i.e. count<cap).
    for i in 0..3 {
        let decision = enforcer.evaluate_tool_call("s1", "bash");
        assert_eq!(
            decision,
            PolicyDecision::Allow,
            "call #{i}: under-cap MUST allow"
        );
        enforcer.record_tool_invocation("s1", "bash");
    }
    // 4th invocation: Deny.
    let decision = enforcer.evaluate_tool_call("s1", "bash");
    assert_eq!(
        decision,
        PolicyDecision::Deny,
        "4th call (count == cap) MUST deny"
    );
}

#[test]
fn check_and_record_tool_returns_tool_cap_error_carrying_consumed() {
    let enforcer = PolicyEnforcer::new(policy_with_tool_cap("edit_file", 2));
    // Two successful calls.
    enforcer
        .check_and_record_tool("s1", "edit_file")
        .expect("1st call");
    enforcer
        .check_and_record_tool("s1", "edit_file")
        .expect("2nd call");
    // Third call: ToolCapExceeded with cap=2, consumed=2.
    let outcome = enforcer.check_and_record_tool("s1", "edit_file");
    let Err(PolicyError::ToolCapExceeded {
        tool,
        cap,
        consumed,
    }) = outcome
    else {
        panic!("over-cap MUST error; got {outcome:?}");
    };
    assert_eq!(tool, "edit_file");
    assert_eq!(cap, 2);
    assert_eq!(consumed, 2, "consumed reports the count AT refusal time");
}

#[test]
fn evaluate_does_not_consume_budget_so_deny_is_idempotent() {
    let enforcer = PolicyEnforcer::new(policy_with_tool_cap("bash", 1));
    // First call consumes the budget.
    enforcer
        .check_and_record_tool("s1", "bash")
        .expect("1st call");
    // Now repeated evaluate calls all return Deny without
    // changing state — evaluate is pure.
    for _ in 0..5 {
        assert_eq!(
            enforcer.evaluate_tool_call("s1", "bash"),
            PolicyDecision::Deny,
            "evaluate MUST be repeatable + pure"
        );
    }
}

#[test]
fn per_session_tool_counters_are_isolated() {
    let enforcer = PolicyEnforcer::new(policy_with_tool_cap("bash", 1));
    // Session A consumes its one budget.
    enforcer
        .check_and_record_tool("session-A", "bash")
        .expect("A 1st");
    // Session A: next call denied.
    assert!(enforcer.check_and_record_tool("session-A", "bash").is_err());
    // Session B starts fresh — its budget is independent.
    enforcer
        .check_and_record_tool("session-B", "bash")
        .expect("B 1st (independent budget)");
}

#[test]
fn reset_session_clears_only_that_session_counters() {
    let enforcer = PolicyEnforcer::new(policy_with_tool_cap("bash", 1));
    enforcer.check_and_record_tool("A", "bash").expect("A 1st");
    enforcer.check_and_record_tool("B", "bash").expect("B 1st");
    assert!(enforcer.check_and_record_tool("A", "bash").is_err());
    assert!(enforcer.check_and_record_tool("B", "bash").is_err());

    // Reset A only.
    enforcer.reset_session("A");
    // A can call again; B is still capped.
    enforcer
        .check_and_record_tool("A", "bash")
        .expect("A post-reset");
    assert!(
        enforcer.check_and_record_tool("B", "bash").is_err(),
        "B must remain capped after A's reset"
    );
}

#[test]
fn per_tool_counters_are_independent_within_one_session() {
    let mut tool_caps = std::collections::HashMap::new();
    tool_caps.insert("bash".to_string(), 1);
    tool_caps.insert("edit_file".to_string(), 1);
    let policy = EnterprisePolicy {
        tool_caps,
        ..EnterprisePolicy::default()
    };
    let enforcer = PolicyEnforcer::new(policy);
    // bash + edit_file budgets are independent.
    enforcer
        .check_and_record_tool("s", "bash")
        .expect("bash 1st");
    enforcer
        .check_and_record_tool("s", "edit_file")
        .expect("edit 1st");
    // Both now exhausted; the other tool isn't affected.
    assert!(enforcer.check_and_record_tool("s", "bash").is_err());
    assert!(enforcer.check_and_record_tool("s", "edit_file").is_err());
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — YAML round-trip of the policy block
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn enterprise_policy_yaml_round_trip_preserves_every_field() {
    let yaml = r"
max_request_tokens: 32000
max_session_tokens: 500000
tool_caps:
  bash: 50
  edit_file: 100
model_allowlist:
  - claude-3-5-sonnet-20241022
  - claude-3-opus-20240229
";
    let policy: EnterprisePolicy = serde_yaml::from_str(yaml).expect("yaml parses");
    assert_eq!(policy.max_request_tokens, Some(32000));
    assert_eq!(policy.max_session_tokens, Some(500_000));
    assert_eq!(policy.tool_caps.get("bash"), Some(&50));
    assert_eq!(policy.tool_caps.get("edit_file"), Some(&100));
    let expected: HashSet<String> = [
        "claude-3-5-sonnet-20241022".to_string(),
        "claude-3-opus-20240229".to_string(),
    ]
    .into_iter()
    .collect();
    assert_eq!(policy.model_allowlist, expected);
}

#[test]
fn empty_yaml_policy_block_yields_all_defaults_off() {
    let policy: EnterprisePolicy = serde_yaml::from_str("{}").expect("empty parses");
    assert_eq!(policy.max_request_tokens, None);
    assert_eq!(policy.max_session_tokens, None);
    assert!(policy.tool_caps.is_empty());
    assert!(policy.model_allowlist.is_empty());
    // And every check is a no-op with this configuration.
    assert!(policy.check_model("any-model").is_ok());
    assert!(policy.check_request_tokens(usize::MAX).is_ok());
    assert!(policy.check_session_tokens(usize::MAX).is_ok());
}
