//! End-to-end tests for `modes::Modifier` serde rename
//! attributes — the #830 disambiguation that prefixes
//! Debug/Methodical/Director with "modifier-" to avoid
//! collision with the same-named `Preset` variants.
//!
//! Sprint 194 of the verification effort. Sprint 191
//! covered FromStr/Display; this file pins the serde
//! JSON wire shape, especially the `modifier-*` rename
//! that lets a config carry both `Preset::Debug` and
//! `Modifier::Debug` unambiguously.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::modes::Modifier;
use serde_json::{json, Value};

// ───────────────────────────────────────────────────────────────────────────
// Section A — modifier-* rename for overlapping variants (#830)
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn modifier_debug_serializes_with_modifier_prefix() {
    // PINS #830: "modifier-debug" wire name (NOT "debug")
    // to disambiguate from Preset::Debug.
    let m = Modifier::Debug;
    let json: Value = serde_json::to_value(m).expect("ser");
    assert_eq!(
        json,
        json!("modifier-debug"),
        "PINS #830: Debug → 'modifier-debug' wire"
    );
}

#[test]
fn modifier_methodical_serializes_with_modifier_prefix() {
    let m = Modifier::Methodical;
    let json: Value = serde_json::to_value(m).expect("ser");
    assert_eq!(json, json!("modifier-methodical"));
}

#[test]
fn modifier_director_serializes_with_modifier_prefix() {
    let m = Modifier::Director;
    let json: Value = serde_json::to_value(m).expect("ser");
    assert_eq!(json, json!("modifier-director"));
}

#[test]
fn modifier_bare_debug_deserializes_back_correctly() {
    let json = json!("modifier-debug");
    let m: Modifier = serde_json::from_value(json).expect("de");
    assert_eq!(m, Modifier::Debug);
}

#[test]
fn modifier_bare_methodical_deserializes_back_correctly() {
    let json = json!("modifier-methodical");
    let m: Modifier = serde_json::from_value(json).expect("de");
    assert_eq!(m, Modifier::Methodical);
}

#[test]
fn modifier_bare_director_deserializes_back_correctly() {
    let json = json!("modifier-director");
    let m: Modifier = serde_json::from_value(json).expect("de");
    assert_eq!(m, Modifier::Director);
}

#[test]
fn modifier_debug_without_prefix_rejected_on_deserialize() {
    // PINS #830 SAFETY: bare "debug" MUST NOT deserialize to
    // Modifier (would create ambiguity with Preset::Debug).
    let json = json!("debug");
    let outcome: Result<Modifier, _> = serde_json::from_value(json);
    assert!(
        outcome.is_err(),
        "bare 'debug' MUST NOT match Modifier::Debug"
    );
}

#[test]
fn modifier_methodical_without_prefix_rejected_on_deserialize() {
    let json = json!("methodical");
    let outcome: Result<Modifier, _> = serde_json::from_value(json);
    assert!(outcome.is_err());
}

#[test]
fn modifier_director_without_prefix_rejected_on_deserialize() {
    let json = json!("director");
    let outcome: Result<Modifier, _> = serde_json::from_value(json);
    assert!(outcome.is_err());
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — Non-overlapping variants use bare lowercase
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn modifier_bold_serializes_as_bare_lowercase() {
    // PINS kebab-case: Bold → "bold" (no overlap with any Preset).
    let m = Modifier::Bold;
    let json: Value = serde_json::to_value(m).expect("ser");
    assert_eq!(json, json!("bold"));
}

#[test]
fn modifier_readonly_serializes_as_bare_lowercase() {
    let m = Modifier::Readonly;
    let json: Value = serde_json::to_value(m).expect("ser");
    assert_eq!(json, json!("readonly"));
}

#[test]
fn modifier_context_pacing_serializes_kebab_case() {
    // PINS kebab-case: ContextPacing → "context-pacing".
    let m = Modifier::ContextPacing;
    let json: Value = serde_json::to_value(m).expect("ser");
    assert_eq!(json, json!("context-pacing"));
}

#[test]
fn modifier_bold_deserializes_from_bare_lowercase() {
    let json = json!("bold");
    let m: Modifier = serde_json::from_value(json).expect("de");
    assert_eq!(m, Modifier::Bold);
}

#[test]
fn modifier_readonly_deserializes_from_bare_lowercase() {
    let json = json!("readonly");
    let m: Modifier = serde_json::from_value(json).expect("de");
    assert_eq!(m, Modifier::Readonly);
}

#[test]
fn modifier_context_pacing_deserializes_from_kebab_case() {
    let json = json!("context-pacing");
    let m: Modifier = serde_json::from_value(json).expect("de");
    assert_eq!(m, Modifier::ContextPacing);
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — Round-trip across all 6 variants
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn all_six_modifiers_round_trip_through_json() {
    let all = [
        Modifier::Bold,
        Modifier::Debug,
        Modifier::Methodical,
        Modifier::Director,
        Modifier::Readonly,
        Modifier::ContextPacing,
    ];
    for m in all {
        let json = serde_json::to_value(m).expect("ser");
        let back: Modifier = serde_json::from_value(json).expect("de");
        assert_eq!(back, m, "round-trip failed for {m:?}");
    }
}

#[test]
fn all_six_modifiers_have_distinct_wire_names() {
    let names: Vec<String> = [
        Modifier::Bold,
        Modifier::Debug,
        Modifier::Methodical,
        Modifier::Director,
        Modifier::Readonly,
        Modifier::ContextPacing,
    ]
    .iter()
    .map(|m| {
        serde_json::to_value(m)
            .unwrap()
            .as_str()
            .unwrap()
            .to_string()
    })
    .collect();
    let mut sorted = names;
    sorted.sort();
    let unique_count = sorted.windows(2).filter(|w| w[0] != w[1]).count() + 1;
    assert_eq!(unique_count, 6, "MUST have 6 distinct wire names");
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — Cross-shape isolation (Preset vs Modifier)
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn preset_debug_serializes_with_preset_prefix_distinct_from_modifier() {
    // Verify Preset::Debug serializes as "preset-debug" (or
    // similar) to disambiguate from Modifier::Debug.
    use openclaudia::modes::Preset;
    let preset_json = serde_json::to_value(Preset::Debug).expect("ser");
    let modifier_json = serde_json::to_value(Modifier::Debug).expect("ser");
    assert_ne!(
        preset_json, modifier_json,
        "PINS #830: Preset::Debug and Modifier::Debug MUST serialize differently"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — Robustness
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn modifier_unknown_string_rejected() {
    let json = json!("not_a_modifier_xyz");
    let outcome: Result<Modifier, _> = serde_json::from_value(json);
    assert!(outcome.is_err());
}

#[test]
fn modifier_empty_string_rejected() {
    let json = json!("");
    let outcome: Result<Modifier, _> = serde_json::from_value(json);
    assert!(outcome.is_err());
}

#[test]
fn modifier_uppercase_rejected_strict_serde() {
    // PINS DOC: serde rename_all = "kebab-case" is strict lowercase.
    let json = json!("BOLD");
    let outcome: Result<Modifier, _> = serde_json::from_value(json);
    assert!(outcome.is_err());
}

#[test]
fn modifier_with_underscore_rejected_on_deserialize() {
    // kebab-case uses hyphens, not underscores.
    let json = json!("context_pacing");
    let outcome: Result<Modifier, _> = serde_json::from_value(json);
    assert!(
        outcome.is_err(),
        "underscore form MUST NOT match kebab-case serde"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — Round-trip via to_string then serde
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn modifier_serde_and_display_agree_for_non_overlapping_variants() {
    // For non-overlapping variants, the Display string equals
    // the serde wire string.
    let pairs: &[(Modifier, &str)] = &[(Modifier::Bold, "bold"), (Modifier::Readonly, "readonly")];
    for (m, expected) in pairs {
        let display_str = m.to_string();
        let serde_str = serde_json::to_value(m)
            .unwrap()
            .as_str()
            .unwrap()
            .to_string();
        // Both should equal the expected wire name.
        assert_eq!(display_str, *expected);
        assert_eq!(serde_str, *expected);
    }
}
