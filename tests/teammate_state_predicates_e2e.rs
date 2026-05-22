//! End-to-end tests for `coordinator::TeammateState` —
//! exhaustive matrix of the `is_alive` and `is_available`
//! predicates across all 4 variants (Spawning / Running /
//! Idle / Dead), plus mutual-exclusion relationships and
//! the documented Dead payload semantics.
//!
//! Sprint 179 of the verification effort. Sprint 21
//! covered Teammate transitions; this file pins the
//! predicate truth table at the state-enum layer
//! independent of the Teammate lifecycle owner.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::coordinator::TeammateState;

// ───────────────────────────────────────────────────────────────────────────
// Section A — is_alive truth table
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn spawning_is_alive() {
    assert!(TeammateState::Spawning.is_alive());
}

#[test]
fn running_is_alive() {
    assert!(TeammateState::Running.is_alive());
}

#[test]
fn idle_is_alive() {
    assert!(TeammateState::Idle.is_alive());
}

#[test]
fn dead_is_not_alive() {
    // PINS DOC: Dead is the only terminal state.
    let state = TeammateState::Dead("crashed".to_string());
    assert!(!state.is_alive(), "Dead MUST NOT be alive");
}

#[test]
fn dead_with_empty_message_still_not_alive() {
    let state = TeammateState::Dead(String::new());
    assert!(!state.is_alive());
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — is_available truth table
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn only_idle_is_available() {
    // PINS DOC: Idle is the ONLY state that accepts new work.
    assert!(TeammateState::Idle.is_available());
}

#[test]
fn spawning_is_not_available_yet() {
    assert!(!TeammateState::Spawning.is_available());
}

#[test]
fn running_is_not_available_busy() {
    assert!(!TeammateState::Running.is_available());
}

#[test]
fn dead_is_not_available() {
    let state = TeammateState::Dead("oops".to_string());
    assert!(!state.is_available());
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — Predicate relationships
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn is_available_implies_is_alive() {
    // PINS LOGIC: if a state is available it must also be alive.
    // (Available is strictly stronger than alive.)
    let states = [
        TeammateState::Spawning,
        TeammateState::Running,
        TeammateState::Idle,
        TeammateState::Dead("x".to_string()),
    ];
    for state in &states {
        if state.is_available() {
            assert!(
                state.is_alive(),
                "available MUST imply alive; state {state:?} violates"
            );
        }
    }
}

#[test]
fn dead_is_neither_alive_nor_available() {
    let state = TeammateState::Dead("error".to_string());
    assert!(!state.is_alive());
    assert!(!state.is_available());
}

#[test]
fn alive_states_count_is_3_out_of_4_variants() {
    // PINS DOC: 3 alive states (Spawning, Running, Idle) + 1
    // terminal (Dead) = 4 total.
    let states = [
        TeammateState::Spawning,
        TeammateState::Running,
        TeammateState::Idle,
        TeammateState::Dead("x".to_string()),
    ];
    let alive_count = states.iter().filter(|s| s.is_alive()).count();
    assert_eq!(alive_count, 3, "PINS: exactly 3/4 variants are alive");
}

#[test]
fn available_states_count_is_1_out_of_4_variants() {
    let states = [
        TeammateState::Spawning,
        TeammateState::Running,
        TeammateState::Idle,
        TeammateState::Dead("x".to_string()),
    ];
    let available_count = states.iter().filter(|s| s.is_available()).count();
    assert_eq!(
        available_count, 1,
        "PINS: exactly 1/4 variants are available (Idle)"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — Dead payload semantics
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn dead_carries_string_payload_documented_in_variant() {
    let state = TeammateState::Dead("connection reset".to_string());
    match state {
        TeammateState::Dead(reason) => assert_eq!(reason, "connection reset"),
        _ => panic!("MUST be Dead"),
    }
}

#[test]
fn dead_with_unicode_reason_preserved() {
    let state = TeammateState::Dead("日本語エラー 🎉".to_string());
    match state {
        TeammateState::Dead(reason) => assert_eq!(reason, "日本語エラー 🎉"),
        _ => panic!(),
    }
}

#[test]
fn dead_with_long_reason_preserved() {
    let long = "x".repeat(1000);
    let state = TeammateState::Dead(long.clone());
    match state {
        TeammateState::Dead(reason) => {
            assert_eq!(reason.len(), 1000);
            assert_eq!(reason, long);
        }
        _ => panic!(),
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — Predicates are pure const-fn
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn predicates_yield_same_result_on_repeated_calls() {
    let state = TeammateState::Running;
    for _ in 0..5 {
        assert!(state.is_alive());
        assert!(!state.is_available());
    }
}

#[test]
fn predicate_results_do_not_depend_on_dead_payload_content() {
    let dead_a = TeammateState::Dead(String::new());
    let dead_b = TeammateState::Dead("any message".to_string());
    let dead_c = TeammateState::Dead("日本語".to_string());

    assert_eq!(dead_a.is_alive(), dead_b.is_alive());
    assert_eq!(dead_b.is_alive(), dead_c.is_alive());
    assert_eq!(dead_a.is_available(), dead_b.is_available());
    assert!(!dead_a.is_alive());
    assert!(!dead_a.is_available());
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — Clone preserves variant + payload
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn clone_preserves_spawning() {
    let original = TeammateState::Spawning;
    let cloned = original.clone();
    // PINS CLONE: original still usable after clone (deep copy).
    assert!(cloned.is_alive());
    assert!(original.is_alive());
    assert!(!cloned.is_available());
}

#[test]
fn clone_preserves_idle_state() {
    let original = TeammateState::Idle;
    let cloned = original.clone();
    assert!(cloned.is_alive());
    assert!(original.is_alive());
    assert!(cloned.is_available());
    assert!(original.is_available());
}

#[test]
fn clone_preserves_dead_payload() {
    let original = TeammateState::Dead("payload-marker-179".to_string());
    let cloned = original.clone();
    match (&cloned, &original) {
        (TeammateState::Dead(r1), TeammateState::Dead(r2)) => {
            assert_eq!(r1, "payload-marker-179");
            assert_eq!(r2, "payload-marker-179");
        }
        _ => panic!("BOTH MUST be Dead"),
    }
}
