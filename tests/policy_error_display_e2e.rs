//! End-to-end tests for `services::policy::PolicyError`
//! `Display` strings. Pins each of the 3 variant message
//! templates exactly, plus error-source preservation via
//! `std::error::Error`.
//!
//! Sprint 201 of the verification effort. Sprint 113 / 199
//! covered the policy semantics; this file pins the
//! exact wire-level error messages operators see.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::services::policy::PolicyError;

// ───────────────────────────────────────────────────────────────────────────
// Section A — ModelDenied Display
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn model_denied_display_uses_documented_template() {
    let err = PolicyError::ModelDenied {
        model: "gpt-4o".to_string(),
    };
    let s = err.to_string();
    // PINS TEMPLATE: "model `<name>` is not in the enterprise allowlist".
    assert_eq!(s, "model `gpt-4o` is not in the enterprise allowlist");
}

#[test]
fn model_denied_display_wraps_model_in_backticks() {
    let err = PolicyError::ModelDenied {
        model: "claude-opus".to_string(),
    };
    let s = err.to_string();
    assert!(s.contains("`claude-opus`"), "MUST wrap model in backticks");
}

#[test]
fn model_denied_with_empty_model_does_not_panic() {
    let err = PolicyError::ModelDenied {
        model: String::new(),
    };
    let s = err.to_string();
    // Empty model still renders backticks-around-empty.
    assert!(s.contains("``"));
}

#[test]
fn model_denied_preserves_special_chars_in_template() {
    let err = PolicyError::ModelDenied {
        model: "model/with/slash".to_string(),
    };
    let s = err.to_string();
    assert!(s.contains("model/with/slash"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — TokenCapExceeded Display
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn token_cap_exceeded_request_scope_uses_documented_template() {
    let err = PolicyError::TokenCapExceeded {
        estimated: 7500,
        cap: 5000,
        scope: "request",
    };
    let s = err.to_string();
    // PINS TEMPLATE: "request exceeds policy token cap: <est> > <cap> (per-<scope>)".
    assert_eq!(
        s,
        "request exceeds policy token cap: 7500 > 5000 (per-request)"
    );
}

#[test]
fn token_cap_exceeded_session_scope_uses_per_session_suffix() {
    let err = PolicyError::TokenCapExceeded {
        estimated: 100_001,
        cap: 100_000,
        scope: "session",
    };
    let s = err.to_string();
    assert!(s.contains("(per-session)"));
    assert!(s.contains("100001"));
    assert!(s.contains("100000"));
}

#[test]
fn token_cap_exceeded_with_zero_cap_still_renders() {
    let err = PolicyError::TokenCapExceeded {
        estimated: 1,
        cap: 0,
        scope: "request",
    };
    let s = err.to_string();
    assert!(s.contains('1'));
    assert!(s.contains('0'));
}

#[test]
fn token_cap_exceeded_with_usize_max_renders_decimal_not_hex() {
    let err = PolicyError::TokenCapExceeded {
        estimated: usize::MAX,
        cap: 100,
        scope: "request",
    };
    let s = err.to_string();
    // No hex prefix; decimal rendering.
    assert!(!s.contains("0x"));
    assert!(s.contains("100"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — ToolCapExceeded Display
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn tool_cap_exceeded_display_uses_documented_template() {
    let err = PolicyError::ToolCapExceeded {
        tool: "bash".to_string(),
        cap: 10,
        consumed: 10,
    };
    let s = err.to_string();
    // PINS TEMPLATE: "tool `<name>` exceeded per-session cap of <cap>; consumed=<consumed>".
    assert_eq!(s, "tool `bash` exceeded per-session cap of 10; consumed=10");
}

#[test]
fn tool_cap_exceeded_carries_consumed_separately_from_cap() {
    let err = PolicyError::ToolCapExceeded {
        tool: "edit_file".to_string(),
        cap: 5,
        consumed: 7,
    };
    let s = err.to_string();
    assert!(s.contains("cap of 5"));
    assert!(s.contains("consumed=7"));
}

#[test]
fn tool_cap_exceeded_wraps_tool_name_in_backticks() {
    let err = PolicyError::ToolCapExceeded {
        tool: "custom_tool".to_string(),
        cap: 1,
        consumed: 1,
    };
    let s = err.to_string();
    assert!(s.contains("`custom_tool`"));
}

#[test]
fn tool_cap_exceeded_with_empty_tool_name_renders() {
    let err = PolicyError::ToolCapExceeded {
        tool: String::new(),
        cap: 1,
        consumed: 1,
    };
    let s = err.to_string();
    assert!(s.contains("``"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — Cross-variant distinctness
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn three_variants_have_distinct_display_prefixes() {
    let m = PolicyError::ModelDenied {
        model: "x".to_string(),
    }
    .to_string();
    let t = PolicyError::TokenCapExceeded {
        estimated: 1,
        cap: 1,
        scope: "request",
    }
    .to_string();
    let l = PolicyError::ToolCapExceeded {
        tool: "x".to_string(),
        cap: 1,
        consumed: 1,
    }
    .to_string();
    // Each variant has a distinct opening word.
    assert!(m.starts_with("model"));
    assert!(t.starts_with("request") || t.contains("policy token cap"));
    assert!(l.starts_with("tool"));
    // All three are pairwise distinct.
    assert_ne!(m, t);
    assert_ne!(t, l);
    assert_ne!(m, l);
}

#[test]
fn debug_format_includes_variant_name() {
    let err = PolicyError::ModelDenied {
        model: "x".to_string(),
    };
    let d = format!("{err:?}");
    assert!(d.contains("ModelDenied"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — Error trait integration
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn policy_error_implements_std_error_trait() {
    let err = PolicyError::ModelDenied {
        model: "x".to_string(),
    };
    // Verify it implements Error (statement-level coercion).
    let _: &dyn std::error::Error = &err;
}

#[test]
fn policy_error_is_send_sync_for_async_propagation() {
    // PINS DOC: PolicyError MUST be Send+Sync so it can cross
    // .await boundaries when bubbled up from async handlers.
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<PolicyError>();
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — Determinism
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn display_is_deterministic_across_repeated_calls() {
    let err = PolicyError::TokenCapExceeded {
        estimated: 100,
        cap: 50,
        scope: "request",
    };
    let s1 = err.to_string();
    let s2 = err.to_string();
    let s3 = err.to_string();
    assert_eq!(s1, s2);
    assert_eq!(s2, s3);
}
