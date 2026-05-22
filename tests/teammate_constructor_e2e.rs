//! End-to-end tests for `coordinator::Teammate::new` —
//! field initialization, color round-robin by ordinal,
//! `id` uniqueness, initial Spawning state, and
//! `try_transition_to` for the canonical
//! Spawning→Running→Idle→Running→Idle→Dead path.
//!
//! Sprint 192 of the verification effort. Sprint 21
//! covered transitions; this file pins the constructor
//! field-init guarantees + the color-by-ordinal contract.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::coordinator::{AgentColor, Teammate, TeammateState};
use openclaudia::subagent::AgentType;
use std::path::PathBuf;

fn fresh(ordinal: usize, session: &str) -> Teammate {
    Teammate::new(
        AgentType::Explore,
        ordinal,
        session,
        PathBuf::from("/tmp/transcript-192.json"),
    )
}

// ───────────────────────────────────────────────────────────────────────────
// Section A — Initial field state
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn new_yields_spawning_state() {
    let tm = fresh(0, "sess");
    assert!(matches!(tm.state, TeammateState::Spawning));
}

#[test]
fn new_with_explore_type_preserves_agent_type() {
    let tm = fresh(0, "sess");
    assert_eq!(tm.agent_type, AgentType::Explore);
}

#[test]
fn new_with_plan_type_preserves_agent_type() {
    let tm = Teammate::new(AgentType::Plan, 0, "sess", PathBuf::from("/tmp/t.json"));
    assert_eq!(tm.agent_type, AgentType::Plan);
}

#[test]
fn new_propagates_session_id_from_into_string() {
    let tm = fresh(0, "session-marker-192");
    assert_eq!(tm.session_id, "session-marker-192");
}

#[test]
fn new_propagates_transcript_path_verbatim() {
    let path = PathBuf::from("/var/log/teammate/x.json");
    let tm = Teammate::new(AgentType::Explore, 0, "s", path.clone());
    assert_eq!(tm.transcript_path, path);
}

#[test]
fn new_id_is_non_empty_uuid_like_string() {
    let tm = fresh(0, "s");
    let id_str = tm.id.as_str();
    assert!(!id_str.is_empty());
    // UUID v4 → 36-char string with 4 hyphens.
    assert_eq!(id_str.len(), 36);
    assert_eq!(id_str.matches('-').count(), 4);
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — Color round-robin by ordinal
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn new_ordinal_zero_yields_first_palette_color() {
    let tm = fresh(0, "s");
    assert_eq!(tm.color, AgentColor::for_index(0));
}

#[test]
fn new_ordinal_3_yields_4th_palette_color() {
    let tm = fresh(3, "s");
    assert_eq!(tm.color, AgentColor::for_index(3));
}

#[test]
fn new_ordinal_7_wraps_to_first_palette_color() {
    // PINS DOC: 7-color palette wraps after 7th teammate.
    let tm0 = fresh(0, "s");
    let tm7 = fresh(7, "s");
    assert_eq!(tm0.color, tm7.color);
}

#[test]
fn new_ordinal_huge_does_not_panic() {
    // PINS SAFETY: huge ordinal mod 7 still indexes safely.
    let tm = fresh(usize::MAX, "s");
    assert!(AgentColor::PALETTE.contains(&tm.color));
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — id uniqueness
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn distinct_teammates_have_distinct_ids_even_same_ordinal() {
    let tm1 = fresh(0, "s");
    let tm2 = fresh(0, "s");
    assert_ne!(tm1.id, tm2.id, "each new() MUST yield fresh UUID id");
}

#[test]
fn five_teammates_all_have_distinct_ids() {
    let mut ids: Vec<_> = (0..5).map(|_| fresh(0, "s").id).collect();
    ids.sort_by_key(|i| i.as_str().to_string());
    ids.dedup();
    assert_eq!(ids.len(), 5, "5 fresh teammates → 5 distinct ids");
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — Initial state is alive but not available
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn new_teammate_is_alive_but_not_available() {
    let tm = fresh(0, "s");
    assert!(tm.state.is_alive());
    assert!(
        !tm.state.is_available(),
        "Spawning is alive but NOT available (still booting)"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — try_transition_to: canonical happy path
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn spawning_to_running_succeeds() {
    let mut tm = fresh(0, "s");
    tm.try_transition_to(TeammateState::Running)
        .expect("Spawning → Running MUST succeed");
    assert!(matches!(tm.state, TeammateState::Running));
}

#[test]
fn running_to_idle_succeeds_after_first_running_transition() {
    let mut tm = fresh(0, "s");
    tm.try_transition_to(TeammateState::Running).expect("ok");
    tm.try_transition_to(TeammateState::Idle).expect("ok");
    assert!(matches!(tm.state, TeammateState::Idle));
    assert!(tm.state.is_available());
}

#[test]
fn idle_to_running_succeeds_for_subsequent_task() {
    let mut tm = fresh(0, "s");
    tm.try_transition_to(TeammateState::Running).expect("ok");
    tm.try_transition_to(TeammateState::Idle).expect("ok");
    tm.try_transition_to(TeammateState::Running)
        .expect("Idle → Running MUST succeed for next task");
}

#[test]
fn any_state_to_dead_is_terminal() {
    // From Running, transition to Dead is allowed.
    let mut tm = fresh(0, "s");
    tm.try_transition_to(TeammateState::Running).expect("ok");
    tm.try_transition_to(TeammateState::Dead("crashed".to_string()))
        .expect("Running → Dead MUST succeed");
    assert!(!tm.state.is_alive());
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — try_transition_to: illegal transitions rejected
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn dead_to_anything_is_rejected() {
    let mut tm = fresh(0, "s");
    tm.try_transition_to(TeammateState::Running).expect("ok");
    tm.try_transition_to(TeammateState::Dead("err".to_string()))
        .expect("ok");
    // From Dead, can NOT go to Running.
    let outcome = tm.try_transition_to(TeammateState::Running);
    assert!(
        outcome.is_err(),
        "Dead → Running MUST be illegal (terminal state)"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section G — Equality + Clone of TeammateId in Teammate
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn teammate_id_clone_via_struct_field_access() {
    let tm = fresh(0, "s");
    let cloned_id = tm.id.clone();
    assert_eq!(cloned_id, tm.id);
}
