//! End-to-end tests for `config::AcpConfig` — default
//! values, YAML deserialization, and the
//! `max_iterations` safety-belt cap.
//!
//! Sprint 133 of the verification effort. Sprint 89 covered
//! `acp::AcpServer` IDE state; this file pins the
//! `AcpConfig` defaults + YAML round-trip + the
//! `max_iterations` runaway-loop safety belt.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::config::AcpConfig;

// ───────────────────────────────────────────────────────────────────────────
// Section A — Default values
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn acp_config_default_max_iterations_is_50() {
    // PINS DOCUMENTED DEFAULT: safety belt at 50 iterations
    // (each tool call consumes one). Raising means a runaway
    // model could spin longer — operators can opt in.
    let cfg = AcpConfig::default();
    assert_eq!(cfg.max_iterations, 50);
}

#[test]
fn acp_config_default_via_yaml_empty_object_matches_default_impl() {
    let cfg: AcpConfig = serde_yaml::from_str("{}").expect("parse empty");
    let default_cfg = AcpConfig::default();
    assert_eq!(cfg, default_cfg);
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — YAML deserialization
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn acp_config_yaml_with_explicit_max_iterations_overrides_default() {
    let yaml = "max_iterations: 100";
    let cfg: AcpConfig = serde_yaml::from_str(yaml).expect("parse");
    assert_eq!(cfg.max_iterations, 100);
}

#[test]
fn acp_config_yaml_with_zero_max_iterations_is_accepted() {
    // PINS DOC: 0 is accepted (no compile-time validation
    // that max_iterations > 0).
    let yaml = "max_iterations: 0";
    let cfg: AcpConfig = serde_yaml::from_str(yaml).expect("parse");
    assert_eq!(cfg.max_iterations, 0);
}

#[test]
fn acp_config_yaml_with_max_u32_value_is_accepted() {
    let yaml = format!("max_iterations: {}", u32::MAX);
    let cfg: AcpConfig = serde_yaml::from_str(&yaml).expect("parse");
    assert_eq!(cfg.max_iterations, u32::MAX);
}

#[test]
fn acp_config_yaml_with_negative_max_iterations_rejects() {
    // u32 cannot be negative — deserialization MUST error.
    let yaml = "max_iterations: -1";
    let outcome: Result<AcpConfig, _> = serde_yaml::from_str(yaml);
    assert!(outcome.is_err(), "negative max_iterations MUST be rejected");
}

#[test]
fn acp_config_yaml_with_value_above_u32_max_rejects() {
    let yaml = format!("max_iterations: {}", u64::from(u32::MAX) + 1);
    let outcome: Result<AcpConfig, _> = serde_yaml::from_str(&yaml);
    assert!(outcome.is_err(), "value > u32::MAX MUST be rejected");
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — Eq + Clone semantics
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn acp_config_eq_holds_for_identical_configs() {
    let a = AcpConfig { max_iterations: 75 };
    let b = AcpConfig { max_iterations: 75 };
    assert_eq!(a, b);
}

#[test]
fn acp_config_eq_distinguishes_different_values() {
    let a = AcpConfig { max_iterations: 50 };
    let b = AcpConfig {
        max_iterations: 100,
    };
    assert_ne!(a, b);
}

#[test]
fn acp_config_clone_preserves_field() {
    let original = AcpConfig { max_iterations: 42 };
    let cloned = original.clone();
    // PINS CLONE: both still usable after clone with the same field value.
    assert_eq!(cloned.max_iterations, 42);
    assert_eq!(original.max_iterations, 42);
    // Pinning: cloned and original are independent (clone gave separate
    // ownership). We don't compare addresses (that's not the semantic
    // contract Clone provides); we verify both are usable post-clone.
    assert_eq!(cloned.max_iterations, original.max_iterations);
}

#[test]
fn acp_config_debug_format_includes_field_name() {
    let cfg = AcpConfig { max_iterations: 33 };
    let dbg = format!("{cfg:?}");
    assert!(dbg.contains("max_iterations"));
    assert!(dbg.contains("33"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — Edge values
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn acp_config_low_max_iterations_value_1_accepted() {
    let cfg = AcpConfig { max_iterations: 1 };
    assert_eq!(cfg.max_iterations, 1);
}

#[test]
fn acp_config_high_max_iterations_value_1000_accepted() {
    let cfg = AcpConfig {
        max_iterations: 1000,
    };
    assert_eq!(cfg.max_iterations, 1000);
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — YAML parsing rejects garbage
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn acp_config_yaml_with_string_max_iterations_rejects() {
    let yaml = "max_iterations: \"not a number\"";
    let outcome: Result<AcpConfig, _> = serde_yaml::from_str(yaml);
    assert!(outcome.is_err());
}

#[test]
fn acp_config_yaml_with_extra_fields_ignored_or_errors() {
    // serde Deserialize default behavior is to allow extras —
    // verify either tolerance OR error (both shapes acceptable;
    // we just pin no-panic).
    let yaml = "\nmax_iterations: 25\nunknown_field: ignored\n";
    let outcome: Result<AcpConfig, _> = serde_yaml::from_str(yaml);
    // Either Ok (extras tolerated) or Err (deny_unknown_fields).
    // Both are documented options; no panic.
    if let Ok(cfg) = outcome {
        assert_eq!(cfg.max_iterations, 25);
    }
}
