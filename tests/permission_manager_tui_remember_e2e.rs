//! End-to-end tests for `permissions::PermissionManager` TUI
//! remember/check pairs — `tui_remember_always_allowed` +
//! `tui_remember_always_denied` plus their `tui_is_always_*`
//! query counterparts. Pins per-session sticky-state lifecycle.
//!
//! Sprint 210 milestone of the verification effort. Sprint 50/etc.
//! covered PermissionManager.check; this file pins the TUI
//! always-allow / always-deny in-memory sets directly.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::permissions::PermissionManager;

// ───────────────────────────────────────────────────────────────────────────
// Section A — Default state (empty sets)
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn fresh_manager_has_no_always_allowed_tools() {
    let mgr = PermissionManager::unrestricted();
    assert!(!mgr.tui_is_always_allowed("Bash"));
    assert!(!mgr.tui_is_always_allowed("Edit"));
    assert!(!mgr.tui_is_always_allowed("anything"));
}

#[test]
fn fresh_manager_has_no_always_denied_tools() {
    let mgr = PermissionManager::unrestricted();
    assert!(!mgr.tui_is_always_denied("Bash"));
    assert!(!mgr.tui_is_always_denied("Edit"));
}

#[test]
fn fresh_manager_is_neither_allowed_nor_denied_for_any_tool() {
    let mgr = PermissionManager::unrestricted();
    for tool in ["Bash", "Edit", "Write", "Read", "ls", "x"] {
        assert!(!mgr.tui_is_always_allowed(tool));
        assert!(!mgr.tui_is_always_denied(tool));
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — Always-allow remember + check
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn remember_always_allowed_flips_predicate_to_true() {
    let mgr = PermissionManager::unrestricted();
    assert!(!mgr.tui_is_always_allowed("Bash"));
    mgr.tui_remember_always_allowed("Bash".to_string());
    assert!(mgr.tui_is_always_allowed("Bash"));
}

#[test]
fn remember_always_allowed_only_affects_remembered_tool() {
    let mgr = PermissionManager::unrestricted();
    mgr.tui_remember_always_allowed("Bash".to_string());
    // Bash is allowed.
    assert!(mgr.tui_is_always_allowed("Bash"));
    // Edit was NOT remembered — still false.
    assert!(!mgr.tui_is_always_allowed("Edit"));
}

#[test]
fn remember_always_allowed_is_idempotent_for_duplicate_calls() {
    let mgr = PermissionManager::unrestricted();
    mgr.tui_remember_always_allowed("Bash".to_string());
    mgr.tui_remember_always_allowed("Bash".to_string());
    mgr.tui_remember_always_allowed("Bash".to_string());
    assert!(mgr.tui_is_always_allowed("Bash"));
}

#[test]
fn remember_always_allowed_supports_multiple_tools() {
    let mgr = PermissionManager::unrestricted();
    mgr.tui_remember_always_allowed("Bash".to_string());
    mgr.tui_remember_always_allowed("Edit".to_string());
    mgr.tui_remember_always_allowed("Write".to_string());
    assert!(mgr.tui_is_always_allowed("Bash"));
    assert!(mgr.tui_is_always_allowed("Edit"));
    assert!(mgr.tui_is_always_allowed("Write"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — Always-deny remember + check
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn remember_always_denied_flips_predicate_to_true() {
    let mgr = PermissionManager::unrestricted();
    assert!(!mgr.tui_is_always_denied("Bash"));
    mgr.tui_remember_always_denied("Bash".to_string());
    assert!(mgr.tui_is_always_denied("Bash"));
}

#[test]
fn remember_always_denied_only_affects_remembered_tool() {
    let mgr = PermissionManager::unrestricted();
    mgr.tui_remember_always_denied("rm-marker".to_string());
    assert!(mgr.tui_is_always_denied("rm-marker"));
    assert!(!mgr.tui_is_always_denied("Bash"));
}

#[test]
fn remember_always_denied_is_idempotent_for_duplicate_calls() {
    let mgr = PermissionManager::unrestricted();
    mgr.tui_remember_always_denied("X".to_string());
    mgr.tui_remember_always_denied("X".to_string());
    assert!(mgr.tui_is_always_denied("X"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — Allow + Deny sets are independent
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn allowed_and_denied_sets_are_independent_sets() {
    let mgr = PermissionManager::unrestricted();
    mgr.tui_remember_always_allowed("Bash".to_string());
    // Bash is in allowed set; should NOT be in denied set.
    assert!(mgr.tui_is_always_allowed("Bash"));
    assert!(!mgr.tui_is_always_denied("Bash"));
}

#[test]
fn same_tool_can_be_in_both_sets_simultaneously() {
    // PINS: the two sets are NOT mutually exclusive — both
    // remember operations succeed independently. Resolution
    // logic in higher layers decides precedence.
    let mgr = PermissionManager::unrestricted();
    mgr.tui_remember_always_allowed("Bash".to_string());
    mgr.tui_remember_always_denied("Bash".to_string());
    assert!(mgr.tui_is_always_allowed("Bash"));
    assert!(mgr.tui_is_always_denied("Bash"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — Edge inputs
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn remember_with_empty_string_inserts_empty_marker() {
    let mgr = PermissionManager::unrestricted();
    mgr.tui_remember_always_allowed(String::new());
    // Empty key is its own entry.
    assert!(mgr.tui_is_always_allowed(""));
    // Non-empty unrelated keys still false.
    assert!(!mgr.tui_is_always_allowed("Bash"));
}

#[test]
fn remember_with_unicode_tool_name_round_trips() {
    let mgr = PermissionManager::unrestricted();
    let tool = "日本語ツール";
    mgr.tui_remember_always_allowed(tool.to_string());
    assert!(mgr.tui_is_always_allowed(tool));
}

#[test]
fn lookup_is_byte_exact_match_case_sensitive() {
    // PINS: HashSet::contains is byte-exact, so case mismatch
    // means the predicate returns false.
    let mgr = PermissionManager::unrestricted();
    mgr.tui_remember_always_allowed("Bash".to_string());
    assert!(mgr.tui_is_always_allowed("Bash"));
    assert!(!mgr.tui_is_always_allowed("bash"));
    assert!(!mgr.tui_is_always_allowed("BASH"));
}

#[test]
fn lookup_does_not_trim_whitespace() {
    let mgr = PermissionManager::unrestricted();
    mgr.tui_remember_always_allowed("Bash".to_string());
    assert!(!mgr.tui_is_always_allowed(" Bash "));
    assert!(!mgr.tui_is_always_allowed("Bash "));
    assert!(!mgr.tui_is_always_allowed(" Bash"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — Cross-tool isolation
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn remember_many_tools_then_query_each() {
    let mgr = PermissionManager::unrestricted();
    let tools = ["Bash", "Edit", "Write", "Read", "Glob", "Grep"];
    for tool in tools {
        mgr.tui_remember_always_allowed(tool.to_string());
    }
    for tool in tools {
        assert!(
            mgr.tui_is_always_allowed(tool),
            "{tool:?} MUST be remembered"
        );
    }
    // An unremembered tool stays false.
    assert!(!mgr.tui_is_always_allowed("NotRemembered"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section G — Determinism (read-only lookups)
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn is_always_allowed_is_deterministic_across_repeated_calls() {
    let mgr = PermissionManager::unrestricted();
    mgr.tui_remember_always_allowed("Bash".to_string());
    for _ in 0..10 {
        assert!(mgr.tui_is_always_allowed("Bash"));
        assert!(!mgr.tui_is_always_allowed("Edit"));
    }
}

#[test]
fn is_always_denied_is_deterministic_across_repeated_calls() {
    let mgr = PermissionManager::unrestricted();
    mgr.tui_remember_always_denied("rm".to_string());
    for _ in 0..10 {
        assert!(mgr.tui_is_always_denied("rm"));
        assert!(!mgr.tui_is_always_denied("ls"));
    }
}
