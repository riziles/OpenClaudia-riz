//! End-to-end tests for `coordinator::teammate::TransitionError`
//! — Display format ("invalid teammate state transition: {from} → {to}"),
//! the `from` and `to` `&'static str` discriminant-name fields,
//! and the labels each `TeammateState` variant produces.
//!
//! Sprint 204 of the verification effort. Sprint 21 / 192
//! covered the state-machine semantics; this file pins the
//! error's diagnostic surface specifically.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::coordinator::{Teammate, TeammateState, TransitionError};
use openclaudia::subagent::AgentType;
use std::path::PathBuf;

fn fresh() -> Teammate {
    Teammate::new(AgentType::Explore, 0, "s", PathBuf::from("/tmp/x.json"))
}

fn force_to_dead(tm: &mut Teammate) {
    tm.try_transition_to(TeammateState::Running).expect("ok");
    tm.try_transition_to(TeammateState::Dead("err".to_string()))
        .expect("ok");
}

// ───────────────────────────────────────────────────────────────────────────
// Section A — Display format
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn display_uses_documented_template_with_unicode_arrow() {
    // PINS TEMPLATE: "invalid teammate state transition: {from} → {to}".
    let mut tm = fresh();
    let err = tm.try_transition_to(TeammateState::Idle).unwrap_err();
    let s = err.to_string();
    assert!(s.starts_with("invalid teammate state transition:"));
    assert!(s.contains('→'), "MUST use unicode arrow; got {s:?}");
}

#[test]
fn display_for_spawning_to_idle_shows_both_labels() {
    let mut tm = fresh();
    let err = tm.try_transition_to(TeammateState::Idle).unwrap_err();
    let s = err.to_string();
    assert!(s.contains("Spawning"));
    assert!(s.contains("Idle"));
}

#[test]
fn display_for_dead_to_running_shows_both_labels() {
    let mut tm = fresh();
    force_to_dead(&mut tm);
    let err = tm.try_transition_to(TeammateState::Running).unwrap_err();
    let s = err.to_string();
    assert!(s.contains("Dead"));
    assert!(s.contains("Running"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — from/to fields are &'static str discriminant names
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn from_field_captures_spawning_label_on_illegal_jump() {
    let mut tm = fresh();
    let err = tm.try_transition_to(TeammateState::Idle).unwrap_err();
    assert_eq!(err.from, "Spawning");
}

#[test]
fn to_field_captures_target_idle_label_on_illegal_jump() {
    let mut tm = fresh();
    let err = tm.try_transition_to(TeammateState::Idle).unwrap_err();
    assert_eq!(err.to, "Idle");
}

#[test]
fn from_field_captures_dead_label_after_terminal_transition() {
    let mut tm = fresh();
    force_to_dead(&mut tm);
    let err = tm.try_transition_to(TeammateState::Running).unwrap_err();
    assert_eq!(err.from, "Dead");
}

#[test]
fn to_field_captures_running_label_on_dead_to_running_attempt() {
    let mut tm = fresh();
    force_to_dead(&mut tm);
    let err = tm.try_transition_to(TeammateState::Running).unwrap_err();
    assert_eq!(err.to, "Running");
}

#[test]
fn to_field_captures_dead_label_when_dead_to_dead_attempted() {
    let mut tm = fresh();
    force_to_dead(&mut tm);
    let err = tm
        .try_transition_to(TeammateState::Dead("again".to_string()))
        .unwrap_err();
    // PINS: Dead → Dead is also illegal (terminal state).
    assert_eq!(err.to, "Dead");
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — TeammateState::Dead label strips the inner String
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn dead_label_omits_inner_reason_string() {
    // PINS DOC: TransitionError carries just the discriminant
    // name "Dead", not "Dead(reason here)".
    let mut tm = fresh();
    tm.try_transition_to(TeammateState::Running).expect("ok");
    tm.try_transition_to(TeammateState::Dead(
        "very long descriptive crash reason that should NOT leak".to_string(),
    ))
    .expect("ok");
    let err = tm.try_transition_to(TeammateState::Running).unwrap_err();
    assert_eq!(err.from, "Dead", "Dead reason MUST NOT leak into label");
    assert!(!err.from.contains("crash"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — Clone + Debug
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn transition_error_is_clone() {
    let mut tm = fresh();
    let err = tm.try_transition_to(TeammateState::Idle).unwrap_err();
    let cloned = err.clone();
    assert_eq!(cloned.from, err.from);
    assert_eq!(cloned.to, err.to);
}

#[test]
fn transition_error_debug_includes_field_names() {
    let mut tm = fresh();
    let err = tm.try_transition_to(TeammateState::Idle).unwrap_err();
    let d = format!("{err:?}");
    assert!(d.contains("TransitionError"));
    assert!(d.contains("from"));
    assert!(d.contains("to"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — Error trait integration
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn transition_error_implements_std_error_trait() {
    let mut tm = fresh();
    let err = tm.try_transition_to(TeammateState::Idle).unwrap_err();
    let _: &dyn std::error::Error = &err;
}

#[test]
fn transition_error_is_send_sync() {
    // PINS: must be Send + Sync for async propagation.
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<TransitionError>();
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — Determinism
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn same_illegal_transition_yields_identical_error_each_time() {
    // PINS: identical illegal transitions on different teammates
    // yield identical error from/to (since the state is the same).
    let mut tm1 = fresh();
    let mut tm2 = fresh();
    let e1 = tm1.try_transition_to(TeammateState::Idle).unwrap_err();
    let e2 = tm2.try_transition_to(TeammateState::Idle).unwrap_err();
    assert_eq!(e1.from, e2.from);
    assert_eq!(e1.to, e2.to);
    assert_eq!(e1.to_string(), e2.to_string());
}
