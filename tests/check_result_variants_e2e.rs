//! End-to-end tests for `permissions::CheckResult` — the
//! 3-variant return value from permission gates: `Allowed`,
//! `Denied(String)`, `NeedsPrompt { tool, target }`. Pins
//! payload shapes, `PartialEq` derive, and Clone preservation.
//!
//! Sprint 208 of the verification effort. Sprint 50/207
//! covered `PermissionDecision`; this file pins `CheckResult`
//! independent of the decision enum.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::permissions::CheckResult;

// ───────────────────────────────────────────────────────────────────────────
// Section A — Allowed variant
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn allowed_variant_constructible_with_no_payload() {
    let r = CheckResult::Allowed;
    assert!(matches!(r, CheckResult::Allowed));
}

#[test]
fn two_allowed_instances_are_equal() {
    let a = CheckResult::Allowed;
    let b = CheckResult::Allowed;
    assert_eq!(a, b);
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — Denied(String) variant payload
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn denied_carries_string_reason() {
    let r = CheckResult::Denied("blocked by rule".to_string());
    if let CheckResult::Denied(msg) = &r {
        assert_eq!(msg, "blocked by rule");
    } else {
        panic!("MUST match Denied");
    }
}

#[test]
fn denied_with_empty_string_still_valid() {
    let r = CheckResult::Denied(String::new());
    assert!(matches!(r, CheckResult::Denied(s) if s.is_empty()));
}

#[test]
fn denied_with_unicode_message_preserves_bytes() {
    let r = CheckResult::Denied("拒否されました".to_string());
    if let CheckResult::Denied(msg) = &r {
        assert_eq!(msg, "拒否されました");
    }
}

#[test]
fn two_denied_with_same_reason_are_equal() {
    let a = CheckResult::Denied("x".to_string());
    let b = CheckResult::Denied("x".to_string());
    assert_eq!(a, b);
}

#[test]
fn two_denied_with_different_reasons_are_distinct() {
    let a = CheckResult::Denied("a".to_string());
    let b = CheckResult::Denied("b".to_string());
    assert_ne!(a, b);
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — NeedsPrompt struct-style payload
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn needs_prompt_carries_tool_and_target_fields() {
    let r = CheckResult::NeedsPrompt {
        tool: "Bash".to_string(),
        target: "ls /tmp".to_string(),
    };
    if let CheckResult::NeedsPrompt { tool, target } = &r {
        assert_eq!(tool, "Bash");
        assert_eq!(target, "ls /tmp");
    } else {
        panic!("MUST match NeedsPrompt");
    }
}

#[test]
fn needs_prompt_with_empty_tool_and_target_still_constructible() {
    let r = CheckResult::NeedsPrompt {
        tool: String::new(),
        target: String::new(),
    };
    if let CheckResult::NeedsPrompt { tool, target } = &r {
        assert!(tool.is_empty());
        assert!(target.is_empty());
    }
}

#[test]
fn needs_prompt_with_unicode_target_preserves_bytes() {
    let r = CheckResult::NeedsPrompt {
        tool: "Edit".to_string(),
        target: "ファイル.txt".to_string(),
    };
    if let CheckResult::NeedsPrompt { target, .. } = &r {
        assert_eq!(target, "ファイル.txt");
    }
}

#[test]
fn two_needs_prompt_with_same_fields_are_equal() {
    let a = CheckResult::NeedsPrompt {
        tool: "Bash".to_string(),
        target: "ls".to_string(),
    };
    let b = CheckResult::NeedsPrompt {
        tool: "Bash".to_string(),
        target: "ls".to_string(),
    };
    assert_eq!(a, b);
}

#[test]
fn two_needs_prompt_with_different_tools_are_distinct() {
    let a = CheckResult::NeedsPrompt {
        tool: "Bash".to_string(),
        target: "ls".to_string(),
    };
    let b = CheckResult::NeedsPrompt {
        tool: "Edit".to_string(),
        target: "ls".to_string(),
    };
    assert_ne!(a, b);
}

#[test]
fn two_needs_prompt_with_different_targets_are_distinct() {
    let a = CheckResult::NeedsPrompt {
        tool: "Bash".to_string(),
        target: "ls".to_string(),
    };
    let b = CheckResult::NeedsPrompt {
        tool: "Bash".to_string(),
        target: "rm".to_string(),
    };
    assert_ne!(a, b);
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — Cross-variant distinctness
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn three_variants_pairwise_distinct() {
    let a = CheckResult::Allowed;
    let d = CheckResult::Denied("x".to_string());
    let n = CheckResult::NeedsPrompt {
        tool: "x".to_string(),
        target: "y".to_string(),
    };
    assert_ne!(a, d);
    assert_ne!(d, n);
    assert_ne!(a, n);
}

#[test]
fn allowed_never_equals_denied_even_with_empty_string() {
    let a = CheckResult::Allowed;
    let d = CheckResult::Denied(String::new());
    assert_ne!(a, d);
}

#[test]
fn allowed_never_equals_needs_prompt_with_empty_fields() {
    let a = CheckResult::Allowed;
    let n = CheckResult::NeedsPrompt {
        tool: String::new(),
        target: String::new(),
    };
    assert_ne!(a, n);
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — Clone derive
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn allowed_clone_preserves_variant() {
    let a = CheckResult::Allowed;
    let cloned = a.clone();
    assert_eq!(a, cloned);
}

#[test]
fn denied_clone_preserves_reason() {
    let d = CheckResult::Denied("reason-marker-208".to_string());
    let cloned = d.clone();
    assert_eq!(d, cloned);
    if let CheckResult::Denied(msg) = &cloned {
        assert_eq!(msg, "reason-marker-208");
    }
}

#[test]
fn needs_prompt_clone_preserves_both_fields() {
    let n = CheckResult::NeedsPrompt {
        tool: "tool-marker".to_string(),
        target: "target-marker".to_string(),
    };
    let cloned = n.clone();
    assert_eq!(n, cloned);
    if let CheckResult::NeedsPrompt { tool, target } = &cloned {
        assert_eq!(tool, "tool-marker");
        assert_eq!(target, "target-marker");
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — Debug formatting
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn debug_for_allowed_includes_variant_name() {
    let d = format!("{:?}", CheckResult::Allowed);
    assert!(d.contains("Allowed"));
}

#[test]
fn debug_for_denied_includes_reason_text() {
    let d = format!("{:?}", CheckResult::Denied("payload-marker".to_string()));
    assert!(d.contains("Denied"));
    assert!(d.contains("payload-marker"));
}

#[test]
fn debug_for_needs_prompt_includes_field_names() {
    let d = format!(
        "{:?}",
        CheckResult::NeedsPrompt {
            tool: "T".to_string(),
            target: "G".to_string()
        }
    );
    assert!(d.contains("NeedsPrompt"));
    assert!(d.contains("tool"));
    assert!(d.contains("target"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section G — Send + Sync
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn check_result_is_send_sync_for_async_boundary_crossing() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<CheckResult>();
}
