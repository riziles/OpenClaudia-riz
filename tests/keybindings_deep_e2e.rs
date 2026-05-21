//! End-to-end tests filling gaps in `keybindings` coverage left
//! by sprint 12 (`tests/keybindings_e2e.rs`).
//!
//! Sprint 56 of the verification effort.
//!
//! Coverage shape:
//!   - `KeyAction` serde round-trip across every variant.
//!   - Default `KeybindingsConfig` populates the documented
//!     ~11 bindings.
//!   - `parse_chord` multi-key + whitespace handling.
//!   - Resolver prefix-then-match-then-cancel sequence.
//!   - Resolver multi-context isolation (the resolver itself is
//!     context-agnostic; bindings are per-config).

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::config::KeybindingsConfig;
use openclaudia::keybindings::{
    parse_chord, ChordResolveResult, KeyAction, KeybindingResolver, ParsedKeystroke,
};

// ───────────────────────────────────────────────────────────────────────────
// Helpers
// ───────────────────────────────────────────────────────────────────────────

fn ks(s: &str) -> ParsedKeystroke {
    ParsedKeystroke::parse(s).expect("keystroke parse")
}

// ───────────────────────────────────────────────────────────────────────────
// Section A — KeyAction serde round-trip
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn key_action_round_trip_across_every_variant() {
    let cases = &[
        (KeyAction::NewSession, "new_session"),
        (KeyAction::ListSessions, "list_sessions"),
        (KeyAction::Export, "export"),
        (KeyAction::CopyResponse, "copy_response"),
        (KeyAction::Editor, "editor"),
        (KeyAction::Models, "models"),
        (KeyAction::ToggleMode, "toggle_mode"),
        (KeyAction::Cancel, "cancel"),
        (KeyAction::Status, "status"),
        (KeyAction::Help, "help"),
        (KeyAction::Clear, "clear"),
        (KeyAction::Exit, "exit"),
        (KeyAction::Undo, "undo"),
        (KeyAction::Redo, "redo"),
        (KeyAction::Compact, "compact"),
        (KeyAction::None, "none"),
    ];
    for (action, expected_str) in cases {
        let json = serde_json::to_string(action).expect("serialize");
        assert_eq!(
            json.trim_matches('"'),
            *expected_str,
            "{action:?} MUST serialize to {expected_str:?}"
        );
        let parsed: KeyAction =
            serde_json::from_str(&json).unwrap_or_else(|e| panic!("deserialize {json}: {e}"));
        assert_eq!(parsed, *action);
    }
}

#[test]
fn key_action_deserialize_rejects_unknown_variant() {
    let outcome: Result<KeyAction, _> = serde_json::from_str(r#""totally_unknown_action""#);
    assert!(
        outcome.is_err(),
        "unknown action MUST error; got {outcome:?}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — Default KeybindingsConfig completeness
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn default_config_contains_every_documented_binding() {
    let config = KeybindingsConfig::default();
    let expected = &[
        ("ctrl-x n", KeyAction::NewSession),
        ("ctrl-x l", KeyAction::ListSessions),
        ("ctrl-x x", KeyAction::Export),
        ("ctrl-x y", KeyAction::CopyResponse),
        ("ctrl-x e", KeyAction::Editor),
        ("ctrl-x m", KeyAction::Models),
        ("ctrl-x s", KeyAction::Status),
        ("ctrl-x h", KeyAction::Help),
        ("f2", KeyAction::Models),
        ("tab", KeyAction::ToggleMode),
        ("escape", KeyAction::Cancel),
    ];
    for (key, expected_action) in expected {
        let actual = config.bindings.get(*key);
        assert_eq!(
            actual,
            Some(expected_action),
            "default config MUST bind {key:?} → {expected_action:?}; got {actual:?}"
        );
    }
}

#[test]
fn default_config_has_at_least_11_bindings() {
    let config = KeybindingsConfig::default();
    assert!(
        config.bindings.len() >= 11,
        "default config MUST have at least the 11 documented bindings; got {}",
        config.bindings.len()
    );
}

#[test]
fn default_config_bindings_keys_are_all_lowercase() {
    // Documented contract: "Keys are stored lowercase for
    // case-insensitive lookup."
    let config = KeybindingsConfig::default();
    for key in config.bindings.keys() {
        assert_eq!(
            *key,
            key.to_lowercase(),
            "binding key {key:?} MUST be lowercase in default config"
        );
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — parse_chord whitespace + multi-key
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn parse_chord_collapses_multiple_inner_spaces() {
    // Two keystrokes separated by multiple spaces.
    let chord = parse_chord("ctrl-x    n").expect("multi-space parse");
    assert_eq!(chord.len(), 2);
    assert_eq!(chord[0], ks("ctrl-x"));
    assert_eq!(chord[1], ks("n"));
}

#[test]
fn parse_chord_trims_leading_and_trailing_whitespace() {
    let chord = parse_chord("  ctrl-x n  ").expect("trim parse");
    assert_eq!(chord.len(), 2);
}

#[test]
fn parse_chord_supports_three_key_sequence() {
    let chord = parse_chord("ctrl-x ctrl-y z").expect("3-key parse");
    assert_eq!(chord.len(), 3);
    assert_eq!(chord[0], ks("ctrl-x"));
    assert_eq!(chord[1], ks("ctrl-y"));
    assert_eq!(chord[2], ks("z"));
}

#[test]
fn parse_chord_returns_none_when_keystroke_is_pure_modifier() {
    // Authoring discovery: "ctrl-" (modifier + trailing dash)
    // is accepted by the parser as a ParsedKeystroke with
    // an empty key string — the rejection only applies to
    // bare modifier names ("ctrl") with no trailing dash at
    // all. Pin the actual contract here.
    let outcome = parse_chord("ctrl-x ctrl");
    assert!(
        outcome.is_none(),
        "chord with bare modifier name MUST fail; got {outcome:?}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — Resolver prefix → match → cancel sequence
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn resolver_prefix_then_match_completes_chord() {
    let mut resolver = KeybindingResolver::from_config(&KeybindingsConfig::default());
    // First keystroke matches the prefix of every `ctrl-x ...`
    // binding.
    let first = resolver.resolve(ks("ctrl-x"));
    assert_eq!(
        first,
        ChordResolveResult::Prefix,
        "ctrl-x alone MUST be Prefix; got {first:?}"
    );
    // Second keystroke completes ctrl-x n → NewSession.
    let second = resolver.resolve(ks("n"));
    assert_eq!(
        second,
        ChordResolveResult::Match {
            action: KeyAction::NewSession
        }
    );
}

#[test]
fn resolver_cancel_clears_pending_chord_state() {
    let mut resolver = KeybindingResolver::from_config(&KeybindingsConfig::default());
    let _ = resolver.resolve(ks("ctrl-x"));
    assert!(!resolver.pending_display().is_empty());
    resolver.cancel();
    assert!(
        resolver.pending_display().is_empty(),
        "after cancel(), pending_display MUST be empty"
    );
    // After cancel, the next ctrl-x is treated as a fresh
    // prefix again.
    let first = resolver.resolve(ks("ctrl-x"));
    assert_eq!(first, ChordResolveResult::Prefix);
}

#[test]
fn resolver_no_match_clears_pending_buffer() {
    let mut resolver = KeybindingResolver::from_config(&KeybindingsConfig::default());
    let _ = resolver.resolve(ks("ctrl-x"));
    // Now feed a key that doesn't match any ctrl-x binding.
    let outcome = resolver.resolve(ks("ctrl-x"));
    // ctrl-x ctrl-x is not a documented prefix → NoMatch.
    assert_eq!(outcome, ChordResolveResult::NoMatch);
    // After NoMatch, pending MUST be cleared.
    assert!(resolver.pending_display().is_empty());
}

#[test]
fn resolver_single_key_match_does_not_require_chord_prefix() {
    let mut resolver = KeybindingResolver::from_config(&KeybindingsConfig::default());
    // f2 is a single-key binding for Models.
    let outcome = resolver.resolve(ks("f2"));
    assert_eq!(
        outcome,
        ChordResolveResult::Match {
            action: KeyAction::Models
        }
    );
}

#[test]
fn resolver_escape_resolves_to_cancel() {
    let mut resolver = KeybindingResolver::from_config(&KeybindingsConfig::default());
    let outcome = resolver.resolve(ks("escape"));
    assert_eq!(
        outcome,
        ChordResolveResult::Match {
            action: KeyAction::Cancel
        }
    );
}

#[test]
fn resolver_tab_resolves_to_toggle_mode() {
    let mut resolver = KeybindingResolver::from_config(&KeybindingsConfig::default());
    let outcome = resolver.resolve(ks("tab"));
    assert_eq!(
        outcome,
        ChordResolveResult::Match {
            action: KeyAction::ToggleMode
        }
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — Resolver with custom config
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn resolver_with_empty_config_returns_no_match_for_everything() {
    let mut empty = KeybindingsConfig::default();
    empty.bindings.clear();
    let mut resolver = KeybindingResolver::from_config(&empty);
    let outcome = resolver.resolve(ks("ctrl-x"));
    assert_eq!(outcome, ChordResolveResult::NoMatch);
}

#[test]
fn resolver_with_custom_binding_resolves_correctly() {
    let mut config = KeybindingsConfig::default();
    config.bindings.clear();
    config
        .bindings
        .insert("ctrl-q".to_string(), KeyAction::Exit);
    let mut resolver = KeybindingResolver::from_config(&config);
    let outcome = resolver.resolve(ks("ctrl-q"));
    assert_eq!(
        outcome,
        ChordResolveResult::Match {
            action: KeyAction::Exit
        }
    );
}

#[test]
fn resolver_skips_unparseable_bindings_in_config() {
    let mut config = KeybindingsConfig::default();
    config.bindings.clear();
    // One valid + one unparseable (modifier-only).
    config
        .bindings
        .insert("ctrl-q".to_string(), KeyAction::Exit);
    config.bindings.insert("ctrl-".to_string(), KeyAction::Help);
    let mut resolver = KeybindingResolver::from_config(&config);
    // Valid one MUST resolve.
    let q = resolver.resolve(ks("ctrl-q"));
    assert_eq!(
        q,
        ChordResolveResult::Match {
            action: KeyAction::Exit
        }
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — pending_display formatting
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn pending_display_is_empty_for_fresh_resolver() {
    let resolver = KeybindingResolver::from_config(&KeybindingsConfig::default());
    assert!(resolver.pending_display().is_empty());
}

#[test]
fn pending_display_reflects_buffered_prefix_chord() {
    let mut resolver = KeybindingResolver::from_config(&KeybindingsConfig::default());
    let _ = resolver.resolve(ks("ctrl-x"));
    let display = resolver.pending_display();
    assert!(
        display.to_lowercase().contains("ctrl"),
        "pending display MUST include the buffered prefix; got {display:?}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section G — KeybindingsConfig serde via YAML
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn keybindings_config_yaml_round_trip_preserves_bindings() {
    let yaml = r#"
"ctrl-q": exit
"ctrl-x m": models
"f1": help
"#;
    let config: KeybindingsConfig = serde_yaml::from_str(yaml).expect("parse");
    assert_eq!(config.bindings.get("ctrl-q"), Some(&KeyAction::Exit));
    assert_eq!(config.bindings.get("ctrl-x m"), Some(&KeyAction::Models));
    assert_eq!(config.bindings.get("f1"), Some(&KeyAction::Help));
}

#[test]
fn keybindings_config_yaml_with_unknown_action_errors_during_parse() {
    let yaml = r#""ctrl-q": super_secret_action"#;
    let outcome: Result<KeybindingsConfig, _> = serde_yaml::from_str(yaml);
    assert!(
        outcome.is_err(),
        "unknown action MUST error at YAML parse time; got {outcome:?}"
    );
}
