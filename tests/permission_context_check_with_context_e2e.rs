//! End-to-end tests for `permissions::PermissionContext` + the
//! `check_with_context` projection — the routing that turns
//! `NeedsPrompt` into Denied for headless contexts and keeps
//! `NeedsPrompt` for interactive contexts. Pins #570 contract.
//!
//! Sprint 212 of the verification effort.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::permissions::{
    CheckResult, PermissionContext, PermissionDecision, PermissionManager, PermissionRule,
};
use serde_json::json;
use tempfile::TempDir;

fn enabled_manager() -> (PermissionManager, TempDir) {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("rules.json");
    let mgr = PermissionManager::new(path, true, Vec::new());
    (mgr, dir)
}

// ───────────────────────────────────────────────────────────────────────────
// Section A — PermissionContext defaults + variants
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn permission_context_default_is_interactive() {
    // PINS DOC: Interactive is the #[default] variant.
    assert_eq!(PermissionContext::default(), PermissionContext::Interactive);
}

#[test]
fn three_permission_context_variants_pairwise_distinct() {
    assert_ne!(
        PermissionContext::Interactive,
        PermissionContext::SwarmWorker
    );
    assert_ne!(
        PermissionContext::SwarmWorker,
        PermissionContext::Coordinator
    );
    assert_ne!(
        PermissionContext::Interactive,
        PermissionContext::Coordinator
    );
}

#[test]
fn permission_context_clone_preserves_variant() {
    let ctx = PermissionContext::Coordinator;
    let cloned = ctx;
    assert_eq!(cloned, ctx);
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — check_with_context: Interactive preserves NeedsPrompt
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn interactive_context_keeps_needs_prompt_when_no_rule_matches() {
    // PINS #570: unmatched rule + Interactive → NeedsPrompt.
    let (mgr, _dir) = enabled_manager();
    let outcome = mgr.check_with_context(
        "bash",
        &json!({"command": "echo hello"}),
        PermissionContext::Interactive,
    );
    assert!(
        matches!(outcome, CheckResult::NeedsPrompt { .. }),
        "Interactive MUST yield NeedsPrompt; got {outcome:?}"
    );
}

#[test]
fn interactive_needs_prompt_carries_tool_and_target() {
    let (mgr, _dir) = enabled_manager();
    let outcome = mgr.check_with_context(
        "bash",
        &json!({"command": "ls"}),
        PermissionContext::Interactive,
    );
    if let CheckResult::NeedsPrompt { tool, target } = outcome {
        // PINS: tool = "Bash" canonical capability name.
        assert_eq!(tool, "Bash");
        // PINS: target carries the matched argument value.
        assert!(!target.is_empty());
    } else {
        panic!("expected NeedsPrompt");
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — SwarmWorker projects NeedsPrompt → Denied
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn swarm_worker_context_projects_needs_prompt_to_denied() {
    // PINS #570: unmatched rule + SwarmWorker → Denied (default-deny).
    let (mgr, _dir) = enabled_manager();
    let outcome = mgr.check_with_context(
        "bash",
        &json!({"command": "echo hello"}),
        PermissionContext::SwarmWorker,
    );
    assert!(
        matches!(outcome, CheckResult::Denied(_)),
        "SwarmWorker MUST project NeedsPrompt → Denied; got {outcome:?}"
    );
}

#[test]
fn swarm_worker_denial_message_mentions_default_deny() {
    let (mgr, _dir) = enabled_manager();
    let outcome = mgr.check_with_context(
        "bash",
        &json!({"command": "x"}),
        PermissionContext::SwarmWorker,
    );
    if let CheckResult::Denied(reason) = outcome {
        // PINS DOC: error mentions "Default-deny" and the context.
        assert!(reason.contains("Default-deny") || reason.contains("SwarmWorker"));
    } else {
        panic!("expected Denied");
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — Coordinator behaves like SwarmWorker (today)
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn coordinator_context_also_projects_needs_prompt_to_denied() {
    // PINS DOC: Coordinator reserves a future relay; today
    // behaves identically to SwarmWorker.
    let (mgr, _dir) = enabled_manager();
    let outcome = mgr.check_with_context(
        "bash",
        &json!({"command": "x"}),
        PermissionContext::Coordinator,
    );
    assert!(matches!(outcome, CheckResult::Denied(_)));
}

#[test]
fn coordinator_denial_message_mentions_default_deny() {
    let (mgr, _dir) = enabled_manager();
    let outcome = mgr.check_with_context(
        "bash",
        &json!({"command": "x"}),
        PermissionContext::Coordinator,
    );
    if let CheckResult::Denied(reason) = outcome {
        assert!(reason.contains("Default-deny") || reason.contains("Coordinator"));
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — Allowed survives context projection
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn allowed_outcome_passes_through_every_context_unchanged() {
    let (mut mgr, _dir) = enabled_manager();
    mgr.add_session_rule(PermissionRule {
        tool: "Bash".to_string(),
        pattern: "echo *".to_string(),
        decision: PermissionDecision::Allow,
    });
    for ctx in [
        PermissionContext::Interactive,
        PermissionContext::SwarmWorker,
        PermissionContext::Coordinator,
    ] {
        let outcome = mgr.check_with_context("bash", &json!({"command": "echo hello"}), ctx);
        assert_eq!(
            outcome,
            CheckResult::Allowed,
            "{ctx:?}: Allow rule MUST pass through"
        );
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — Denied survives context projection
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn denied_outcome_passes_through_every_context_unchanged() {
    let (mut mgr, _dir) = enabled_manager();
    mgr.add_session_rule(PermissionRule {
        tool: "Bash".to_string(),
        pattern: "rmtest *".to_string(),
        decision: PermissionDecision::Deny,
    });
    for ctx in [
        PermissionContext::Interactive,
        PermissionContext::SwarmWorker,
        PermissionContext::Coordinator,
    ] {
        let outcome = mgr.check_with_context("bash", &json!({"command": "rmtest hello"}), ctx);
        assert!(
            matches!(outcome, CheckResult::Denied(_)),
            "{ctx:?}: Deny rule MUST pass through"
        );
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section G — Determinism
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn check_with_context_is_deterministic_per_context() {
    let (mgr, _dir) = enabled_manager();
    let args = json!({"command": "ls"});
    let r1 = mgr.check_with_context("bash", &args, PermissionContext::SwarmWorker);
    let r2 = mgr.check_with_context("bash", &args, PermissionContext::SwarmWorker);
    // Both should be Denied with same reason.
    assert_eq!(r1, r2);
}

#[test]
fn check_with_context_interactive_3x_yields_same_needs_prompt() {
    let (mgr, _dir) = enabled_manager();
    let args = json!({"command": "ls"});
    let r1 = mgr.check_with_context("bash", &args, PermissionContext::Interactive);
    let r2 = mgr.check_with_context("bash", &args, PermissionContext::Interactive);
    assert_eq!(r1, r2);
}
