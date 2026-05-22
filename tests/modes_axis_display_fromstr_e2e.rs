//! End-to-end tests for `modes::Agency` / `Quality` /
//! `Scope` / `Modifier` `Display` + `FromStr` round-trips,
//! plus alias acceptance (auto/collab/arch/prag/min/adj/
//! read-only/pacing).
//!
//! Sprint 191 of the verification effort. Sprint 130
//! covered `Preset` / `BehaviorMode`; this file pins the
//! per-axis enum `Display`/`FromStr` contracts directly.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::modes::{Agency, Modifier, Quality, Scope};
use std::str::FromStr;

// ───────────────────────────────────────────────────────────────────────────
// Section A — Agency Display + FromStr round-trip
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn agency_autonomous_displays_lowercase_and_round_trips() {
    let a = Agency::Autonomous;
    assert_eq!(a.to_string(), "autonomous");
    let back: Agency = a.to_string().parse().expect("parse");
    assert_eq!(back, a);
}

#[test]
fn agency_collaborative_displays_lowercase_and_round_trips() {
    let a = Agency::Collaborative;
    assert_eq!(a.to_string(), "collaborative");
    let back: Agency = a.to_string().parse().expect("parse");
    assert_eq!(back, a);
}

#[test]
fn agency_surgical_displays_lowercase_and_round_trips() {
    let a = Agency::Surgical;
    assert_eq!(a.to_string(), "surgical");
    let back: Agency = a.to_string().parse().expect("parse");
    assert_eq!(back, a);
}

#[test]
fn agency_auto_alias_parses_to_autonomous() {
    // PINS ALIAS: "auto" → Autonomous.
    let parsed = Agency::from_str("auto").expect("parse");
    assert_eq!(parsed, Agency::Autonomous);
}

#[test]
fn agency_collab_alias_parses_to_collaborative() {
    let parsed = Agency::from_str("collab").expect("parse");
    assert_eq!(parsed, Agency::Collaborative);
}

#[test]
fn agency_case_insensitive_uppercase_accepted() {
    let parsed = Agency::from_str("AUTONOMOUS").expect("parse");
    assert_eq!(parsed, Agency::Autonomous);
}

#[test]
fn agency_unknown_value_returns_error_with_documented_list() {
    let err = Agency::from_str("bogus_agency").unwrap_err();
    assert!(err.contains("unknown agency"));
    // PINS DOC: error lists 3 documented values.
    assert!(err.contains("autonomous"));
    assert!(err.contains("collaborative"));
    assert!(err.contains("surgical"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — Quality Display + FromStr
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn quality_3_variants_round_trip_lowercase() {
    for (variant, name) in &[
        (Quality::Architect, "architect"),
        (Quality::Pragmatic, "pragmatic"),
        (Quality::Minimal, "minimal"),
    ] {
        assert_eq!(variant.to_string(), *name);
        let parsed: Quality = name.parse().expect("parse");
        assert_eq!(parsed, *variant);
    }
}

#[test]
fn quality_arch_alias_parses_to_architect() {
    let parsed = Quality::from_str("arch").expect("parse");
    assert_eq!(parsed, Quality::Architect);
}

#[test]
fn quality_prag_alias_parses_to_pragmatic() {
    let parsed = Quality::from_str("prag").expect("parse");
    assert_eq!(parsed, Quality::Pragmatic);
}

#[test]
fn quality_min_alias_parses_to_minimal() {
    let parsed = Quality::from_str("min").expect("parse");
    assert_eq!(parsed, Quality::Minimal);
}

#[test]
fn quality_unknown_value_returns_documented_error() {
    let err = Quality::from_str("xyz").unwrap_err();
    assert!(err.contains("unknown quality"));
    assert!(err.contains("architect"));
    assert!(err.contains("pragmatic"));
    assert!(err.contains("minimal"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — Scope Display + FromStr
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn scope_3_variants_round_trip_lowercase() {
    for (variant, name) in &[
        (Scope::Unrestricted, "unrestricted"),
        (Scope::Adjacent, "adjacent"),
        (Scope::Narrow, "narrow"),
    ] {
        assert_eq!(variant.to_string(), *name);
        let parsed: Scope = name.parse().expect("parse");
        assert_eq!(parsed, *variant);
    }
}

#[test]
fn scope_adj_alias_parses_to_adjacent() {
    let parsed = Scope::from_str("adj").expect("parse");
    assert_eq!(parsed, Scope::Adjacent);
}

#[test]
fn scope_unknown_value_returns_documented_error() {
    let err = Scope::from_str("xyz").unwrap_err();
    assert!(err.contains("unknown scope"));
    assert!(err.contains("unrestricted"));
    assert!(err.contains("adjacent"));
    assert!(err.contains("narrow"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — Modifier Display + FromStr
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn modifier_bold_round_trips() {
    let m = Modifier::Bold;
    let s = m.to_string();
    let back: Modifier = s.parse().expect("parse");
    assert_eq!(back, m);
}

#[test]
fn modifier_readonly_with_hyphen_alias_parses() {
    // PINS ALIAS: "read-only" → Readonly.
    let parsed = Modifier::from_str("read-only").expect("parse");
    assert_eq!(parsed, Modifier::Readonly);
}

#[test]
fn modifier_readonly_without_hyphen_parses() {
    let parsed = Modifier::from_str("readonly").expect("parse");
    assert_eq!(parsed, Modifier::Readonly);
}

#[test]
fn modifier_context_pacing_with_hyphen_parses() {
    let parsed = Modifier::from_str("context-pacing").expect("parse");
    assert_eq!(parsed, Modifier::ContextPacing);
}

#[test]
fn modifier_pacing_alias_parses_to_context_pacing() {
    let parsed = Modifier::from_str("pacing").expect("parse");
    assert_eq!(parsed, Modifier::ContextPacing);
}

#[test]
fn modifier_underscore_replaced_with_hyphen_before_match() {
    // PINS NORM: input "read_only" normalised to "read-only".
    let parsed = Modifier::from_str("read_only").expect("parse");
    assert_eq!(parsed, Modifier::Readonly);
}

#[test]
fn modifier_case_insensitive() {
    let parsed = Modifier::from_str("BOLD").expect("parse");
    assert_eq!(parsed, Modifier::Bold);
}

#[test]
fn modifier_unknown_value_returns_error() {
    let err = Modifier::from_str("zzz_bogus").unwrap_err();
    assert!(!err.is_empty());
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — Defaults
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn agency_default_is_autonomous() {
    assert_eq!(Agency::default(), Agency::Autonomous);
}

#[test]
fn quality_default_is_pragmatic() {
    assert_eq!(Quality::default(), Quality::Pragmatic);
}

#[test]
fn scope_default_is_adjacent() {
    // PINS DOC: matches BehaviorMode::default().scope.
    assert_eq!(Scope::default(), Scope::Adjacent);
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — Cross-axis Display distinctness
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn three_agencies_have_distinct_display_strings() {
    let a = Agency::Autonomous.to_string();
    let c = Agency::Collaborative.to_string();
    let s = Agency::Surgical.to_string();
    assert_ne!(a, c);
    assert_ne!(c, s);
    assert_ne!(a, s);
}

#[test]
fn three_qualities_have_distinct_display_strings() {
    let a = Quality::Architect.to_string();
    let p = Quality::Pragmatic.to_string();
    let m = Quality::Minimal.to_string();
    assert_ne!(a, p);
    assert_ne!(p, m);
    assert_ne!(a, m);
}

#[test]
fn three_scopes_have_distinct_display_strings() {
    let u = Scope::Unrestricted.to_string();
    let a = Scope::Adjacent.to_string();
    let n = Scope::Narrow.to_string();
    assert_ne!(u, a);
    assert_ne!(a, n);
    assert_ne!(u, n);
}

// ───────────────────────────────────────────────────────────────────────────
// Section G — Whitespace and empty rejection
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn agency_empty_string_rejected() {
    let outcome = Agency::from_str("");
    assert!(outcome.is_err());
}

#[test]
fn quality_whitespace_only_rejected() {
    let outcome = Quality::from_str("   ");
    assert!(outcome.is_err());
}

#[test]
fn scope_with_trailing_whitespace_rejected() {
    // PINS: from_str does NOT trim.
    let outcome = Scope::from_str("narrow ");
    assert!(outcome.is_err());
}
