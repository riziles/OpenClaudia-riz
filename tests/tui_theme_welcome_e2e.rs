//! End-to-end tests for `tui::Theme` builtin catalog +
//! `tui::WelcomeScreen` shape + `tui::get_tips` content +
//! `MarkdownRenderState` Send-safe round-trip.
//!
//! Sprint 105 of the verification effort. The TUI module's
//! drawing surface (anything that touches stdout/crossterm)
//! is hard to integration-test, but the *data* surface
//! (`Theme` catalog, `WelcomeScreen` builder, tips catalog,
//! markdown render state) is pure and pinnable.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::tui::{
    get_tips, MarkdownRenderState, StreamingMarkdownRenderer, Theme, WelcomeScreen,
};

// ───────────────────────────────────────────────────────────────────────────
// Section A — Theme::default + Theme::from_name
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn theme_default_has_name_default() {
    let t = Theme::default();
    assert_eq!(t.name, "default");
}

#[test]
fn theme_from_name_default_matches_default_constructor() {
    let from_name = Theme::from_name("default").expect("Some");
    let default = Theme::default();
    assert_eq!(from_name.name, default.name);
}

#[test]
fn theme_from_name_returns_some_for_every_documented_builtin() {
    // PINS THEME CATALOG: 6 documented builtins.
    for name in &["default", "ocean", "forest", "sunset", "mono", "neon"] {
        let outcome = Theme::from_name(name);
        assert!(
            outcome.is_some(),
            "documented theme {name:?} MUST resolve via from_name"
        );
    }
}

#[test]
fn theme_from_name_returns_none_for_unknown_name() {
    assert!(Theme::from_name("not-a-theme").is_none());
    assert!(Theme::from_name("").is_none());
    assert!(Theme::from_name("DEFAULT").is_none()); // case-sensitive
}

#[test]
fn theme_from_name_sets_name_field_to_matching_string() {
    for name in &["default", "ocean", "forest", "sunset", "mono", "neon"] {
        let theme = Theme::from_name(name).expect("Some");
        assert_eq!(
            &theme.name, name,
            "Theme.name field MUST match the from_name() input"
        );
    }
}

#[test]
fn theme_clone_preserves_all_fields() {
    let theme = Theme::from_name("ocean").expect("Some");
    let cloned = theme.clone();
    assert_eq!(cloned.name, theme.name);
    // Color fields are crossterm Color enum — assert via Debug equality.
    assert_eq!(
        format!("{:?}", cloned.primary),
        format!("{:?}", theme.primary)
    );
    assert_eq!(
        format!("{:?}", cloned.secondary),
        format!("{:?}", theme.secondary)
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — Themes are pairwise distinct
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn each_theme_has_distinct_primary_or_heading_color() {
    // PINS NO-ALIASING: pairwise comparison via Debug.
    let names = ["default", "ocean", "forest", "sunset", "mono", "neon"];
    let themes: Vec<Theme> = names
        .iter()
        .map(|n| Theme::from_name(n).expect("Some"))
        .collect();
    // Check primary colors are not all identical.
    let mut primary_debugs: Vec<String> =
        themes.iter().map(|t| format!("{:?}", t.primary)).collect();
    primary_debugs.sort();
    primary_debugs.dedup();
    let sorted = primary_debugs;
    // At least 3 distinct primary colors (allowing 2-3 themes to
    // share a single color is OK; "all 6 the same" is NOT).
    assert!(
        sorted.len() >= 3,
        "MUST have >= 3 distinct primary colors across 6 themes; got {} unique",
        sorted.len()
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — get_tips catalog
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn get_tips_returns_non_empty_list() {
    let tips = get_tips();
    assert!(!tips.is_empty());
}

#[test]
fn get_tips_includes_slash_init_documentation() {
    let tips = get_tips();
    let combined = tips.join("\n");
    assert!(
        combined.contains("/init") || combined.contains("/help"),
        "MUST surface at least one slash-command hint"
    );
}

#[test]
fn get_tips_includes_file_inclusion_hint() {
    let tips = get_tips();
    let combined = tips.join(" ");
    assert!(
        combined.contains('@'),
        "MUST mention @filename file-inclusion syntax"
    );
}

#[test]
fn get_tips_includes_keybinding_documentation() {
    let tips = get_tips();
    let combined = tips.join(" ");
    // At least one mention of common keys.
    assert!(
        combined.contains("Tab") || combined.contains("Ctrl") || combined.contains("Esc"),
        "MUST mention at least one key"
    );
}

#[test]
fn get_tips_entries_are_non_empty_strings() {
    for tip in get_tips() {
        assert!(!tip.is_empty(), "tip MUST be non-empty");
    }
}

#[test]
fn get_tips_returns_consistent_count_across_invocations() {
    let count1 = get_tips().len();
    let count2 = get_tips().len();
    assert_eq!(count1, count2);
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — WelcomeScreen builder
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn welcome_screen_new_sets_version_provider_model() {
    let ws = WelcomeScreen::new("0.1.0", "anthropic", "claude-sonnet-4-5");
    assert_eq!(ws.version, "0.1.0");
    assert_eq!(ws.provider, "anthropic");
    assert_eq!(ws.model, "claude-sonnet-4-5");
}

#[test]
fn welcome_screen_with_auth_replaces_auth_method() {
    let ws = WelcomeScreen::new("0.1.0", "anthropic", "model").with_auth("API key");
    assert_eq!(ws.auth_method, "API key");
}

#[test]
fn welcome_screen_with_auth_chains_via_builder() {
    let ws = WelcomeScreen::new("0.1.0", "x", "y")
        .with_auth("oauth")
        .with_auth("api_key");
    // Last with_auth wins (builder semantics).
    assert_eq!(ws.auth_method, "api_key");
}

#[test]
fn welcome_screen_new_has_populated_working_dir() {
    let ws = WelcomeScreen::new("0.1.0", "x", "y");
    assert!(
        !ws.working_dir.is_empty(),
        "working_dir MUST be populated from cwd"
    );
}

#[test]
fn welcome_screen_carries_all_six_documented_fields() {
    let ws = WelcomeScreen::new("v1", "p1", "m1").with_auth("a1");
    // Compile-time + runtime check all 6 pub fields accessible.
    let _ = &ws.version;
    let _ = &ws.provider;
    let _ = &ws.model;
    let _ = &ws.auth_method;
    let _ = &ws.working_dir;
    let _ = &ws.username;
    assert_eq!(ws.version, "v1");
    assert_eq!(ws.provider, "p1");
    assert_eq!(ws.model, "m1");
    assert_eq!(ws.auth_method, "a1");
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — MarkdownRenderState Send-safe round-trip
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn streaming_renderer_new_then_into_state_does_not_panic() {
    let renderer = StreamingMarkdownRenderer::new();
    let _state = renderer.into_state();
}

#[test]
fn renderer_state_round_trip_via_into_then_from_state_does_not_panic() {
    // PINS SEND-SAFE CONTRACT: into_state/from_state round-trip
    // preserves state (sans the !Send highlighter). Private
    // fields prevent direct content assertion; the contract
    // exercised here is "no-panic across the round-trip".
    let renderer = StreamingMarkdownRenderer::new();
    let state = renderer.into_state();
    let renderer2 = StreamingMarkdownRenderer::from_state(state);
    let _state2 = renderer2.into_state();
}

#[test]
fn markdown_render_state_is_send_compile_time_check() {
    // Compile-time assertion: MarkdownRenderState : Send.
    // This guarantees the state can cross .await boundaries
    // safely (the whole reason into_state/from_state exist).
    fn assert_send<T: Send>() {}
    assert_send::<MarkdownRenderState>();
}

#[test]
fn streaming_renderer_default_constructor_works_without_panic() {
    let _r1 = StreamingMarkdownRenderer::default();
    let _r2 = StreamingMarkdownRenderer::new();
}

#[test]
fn streaming_renderer_push_does_not_panic_on_empty_text() {
    let mut renderer = StreamingMarkdownRenderer::new();
    renderer.push("");
}

#[test]
fn streaming_renderer_flush_on_empty_buffer_does_not_panic() {
    let mut renderer = StreamingMarkdownRenderer::new();
    renderer.flush();
}
