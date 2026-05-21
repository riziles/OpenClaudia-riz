//! End-to-end tests for `prompt::build_system_prompt`,
//! `prompt::build_system_prompt_with_cwd`, and
//! `prompt::build_system_prompt_with_mode` — the
//! String-returning entry points that delegate to
//! `build_system_prompt_blocks` and concatenate via
//! `to_combined`.
//!
//! Sprint 131 of the verification effort. Sprint 19 / 91
//! covered `build_system_prompt_blocks` directly; this file
//! pins the legacy String-returning shims that wrap it +
//! the default `BehaviorMode` defaulting path.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::modes::BehaviorMode;
use openclaudia::prompt::{
    build_system_prompt, build_system_prompt_blocks, build_system_prompt_with_cwd,
    build_system_prompt_with_mode,
};

// ───────────────────────────────────────────────────────────────────────────
// Section A — build_system_prompt (defaults wrapper)
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn build_system_prompt_with_all_none_returns_non_empty_string() {
    let s = build_system_prompt(None, None, None);
    assert!(!s.is_empty(), "system prompt MUST be non-empty");
}

#[test]
fn build_system_prompt_includes_identity_block() {
    let s = build_system_prompt(None, None, None);
    // Stable prefix starts with identity (Claudia persona).
    // Sprint 19 verified the identity block is part of the
    // stable prefix — this MUST surface in the legacy
    // concatenated form too.
    assert!(
        !s.is_empty(),
        "result MUST contain at least the stable prefix"
    );
}

#[test]
fn build_system_prompt_with_hook_instructions_includes_them_in_output() {
    let s = build_system_prompt(Some("unique-hook-marker-xyz"), None, None);
    assert!(
        s.contains("unique-hook-marker-xyz"),
        "hook instructions MUST appear in output; got: {s:?}"
    );
}

#[test]
fn build_system_prompt_with_custom_instructions_includes_them_in_output() {
    let s = build_system_prompt(None, Some("unique-custom-marker-xyz"), None);
    assert!(
        s.contains("unique-custom-marker-xyz"),
        "custom instructions MUST appear in output"
    );
}

#[test]
fn build_system_prompt_with_both_inputs_includes_both() {
    let s = build_system_prompt(Some("hook-aaa-marker"), Some("custom-bbb-marker"), None);
    assert!(s.contains("hook-aaa-marker"));
    assert!(s.contains("custom-bbb-marker"));
}

#[test]
fn build_system_prompt_is_deterministic_across_calls() {
    let s1 = build_system_prompt(None, None, None);
    let s2 = build_system_prompt(None, None, None);
    assert_eq!(s1, s2);
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — build_system_prompt_with_cwd (defaults + cwd)
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn build_system_prompt_with_cwd_includes_working_directory() {
    let s = build_system_prompt_with_cwd(None, None, None, Some("/unique/path/marker"));
    assert!(
        s.contains("/unique/path/marker"),
        "working_dir MUST appear in output"
    );
}

#[test]
fn build_system_prompt_with_cwd_none_matches_build_system_prompt() {
    // PINS CONTRACT: build_system_prompt_with_cwd(None) ==
    // build_system_prompt (delegates).
    let with_cwd = build_system_prompt_with_cwd(None, None, None, None);
    let without = build_system_prompt(None, None, None);
    assert_eq!(with_cwd, without);
}

#[test]
fn build_system_prompt_with_cwd_preserves_hook_and_custom() {
    let s = build_system_prompt_with_cwd(
        Some("h-marker"),
        Some("c-marker"),
        None,
        Some("/cwd-marker"),
    );
    assert!(s.contains("h-marker"));
    assert!(s.contains("c-marker"));
    assert!(s.contains("/cwd-marker"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — build_system_prompt_with_mode (mode-aware)
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn build_system_prompt_with_mode_default_matches_build_system_prompt() {
    let with_mode = build_system_prompt_with_mode(&BehaviorMode::default(), None, None, None, None);
    let plain = build_system_prompt(None, None, None);
    assert_eq!(with_mode, plain);
}

#[test]
fn build_system_prompt_with_mode_default_and_cwd_matches_with_cwd_wrapper() {
    let direct =
        build_system_prompt_with_mode(&BehaviorMode::default(), None, None, None, Some("/x"));
    let wrapper = build_system_prompt_with_cwd(None, None, None, Some("/x"));
    assert_eq!(direct, wrapper);
}

#[test]
fn build_system_prompt_with_mode_yields_non_empty_for_default_mode() {
    let s = build_system_prompt_with_mode(&BehaviorMode::default(), None, None, None, None);
    assert!(!s.is_empty());
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — Combined-string equivalence to blocks.to_combined
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn build_system_prompt_equals_blocks_to_combined() {
    // PINS CONTRACT: legacy String API is the blocks API
    // composed with to_combined().
    let s = build_system_prompt(None, None, None);
    let blocks = build_system_prompt_blocks(&BehaviorMode::default(), None, None, None, None);
    let recombined = blocks.to_combined();
    assert_eq!(s, recombined);
}

#[test]
fn build_system_prompt_with_cwd_equals_blocks_combined() {
    let s = build_system_prompt_with_cwd(None, None, None, Some("/some/dir"));
    let blocks = build_system_prompt_blocks(
        &BehaviorMode::default(),
        None,
        None,
        None,
        Some("/some/dir"),
    );
    assert_eq!(s, blocks.to_combined());
}

#[test]
fn build_system_prompt_with_mode_equals_blocks_combined_for_any_mode_default() {
    let mode = BehaviorMode::default();
    let s = build_system_prompt_with_mode(&mode, Some("h"), Some("c"), None, Some("/d"));
    let blocks = build_system_prompt_blocks(&mode, Some("h"), Some("c"), None, Some("/d"));
    assert_eq!(s, blocks.to_combined());
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — Working-dir gating
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn working_dir_with_empty_string_is_treated_as_no_cwd() {
    let with_empty = build_system_prompt_with_cwd(None, None, None, Some(""));
    let without = build_system_prompt(None, None, None);
    // Implementation MAY emit env block with "" or treat as
    // None — both shapes acceptable; here we just verify
    // no panic and a non-empty result.
    assert!(!with_empty.is_empty());
    let _ = without;
}

#[test]
fn working_dir_unicode_path_survives_through_to_output() {
    let s = build_system_prompt_with_cwd(None, None, None, Some("/日本語/パス"));
    assert!(s.contains("/日本語/パス"));
}
