//! End-to-end tests for `coordinator::Coordinator` —
//! the leader-side façade that owns `TaskQueue` +
//! `LeaderPermissionBridge` + teammate registry. Pins
//! the read-only accessors, the focused mutators (`submit`,
//! `add_dependency`), and the Phase-1 `dispatch` stub
//! (`CoordinatorError::NotImplemented`).
//!
//! Sprint 187 of the verification effort. Sprint 21
//! covered `TaskQueue`; this file pins the `Coordinator`
//! wrapper that the leader pipeline owns.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::coordinator::{Coordinator, CoordinatorError, Task};
use openclaudia::subagent::AgentType;

// ───────────────────────────────────────────────────────────────────────────
// Section A — new / Default
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn new_yields_empty_teammates() {
    let c = Coordinator::new();
    // PINS: fresh Coordinator has no teammates.
    assert!(c.teammates().is_empty());
    // Permission bridge is also idle.
    assert!(c.permission_bridge().is_idle());
}

#[test]
fn default_matches_new() {
    let new = Coordinator::new();
    let def = Coordinator::default();
    assert_eq!(new.teammates().len(), def.teammates().len());
}

#[test]
fn new_permission_bridge_is_idle() {
    let c = Coordinator::new();
    assert!(c.permission_bridge().is_idle());
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — submit
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn submit_returns_new_task_id() {
    let mut c = Coordinator::new();
    let id = c.submit(Task::new(AgentType::Explore, "x")).expect("ok");
    let _ = id;
}

#[test]
fn submit_increases_queue_via_focused_mutator() {
    let mut c = Coordinator::new();
    let id1 = c.submit(Task::new(AgentType::Explore, "a")).expect("a");
    let id2 = c.submit(Task::new(AgentType::Explore, "b")).expect("b");
    assert_ne!(id1, id2);
    // Queue accessor confirms both present.
    assert!(c.queue().get(id1).is_some());
    assert!(c.queue().get(id2).is_some());
}

#[test]
fn submit_propagates_cycle_error_from_underlying_queue() {
    // submit itself can't trigger a cycle; the cycle check is on
    // add_dependency. Verify submit is straight-through.
    let mut c = Coordinator::new();
    let _id = c.submit(Task::new(AgentType::Explore, "x")).expect("ok");
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — add_dependency
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn add_dependency_between_valid_tasks_succeeds() {
    let mut c = Coordinator::new();
    let a = c.submit(Task::new(AgentType::Explore, "a")).expect("a");
    let b = c.submit(Task::new(AgentType::Explore, "b")).expect("b");
    let outcome = c.add_dependency(a, b);
    assert!(outcome.is_ok());
}

#[test]
fn add_dependency_self_dep_returns_cycle_error() {
    use openclaudia::coordinator::TaskQueueError;
    let mut c = Coordinator::new();
    let id = c.submit(Task::new(AgentType::Explore, "x")).expect("ok");
    let outcome = c.add_dependency(id, id);
    assert!(matches!(outcome, Err(TaskQueueError::CycleDetected { .. })));
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — queue/teammates/permission_bridge accessors
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn queue_accessor_returns_read_only_view() {
    let mut c = Coordinator::new();
    let id = c.submit(Task::new(AgentType::Explore, "x")).expect("ok");
    let q = c.queue();
    assert!(q.get(id).is_some());
}

#[test]
fn teammates_accessor_returns_read_only_map() {
    let c = Coordinator::new();
    let t = c.teammates();
    // Fresh coordinator → empty.
    assert!(t.is_empty());
}

#[test]
fn permission_bridge_accessor_returns_read_only_bridge() {
    let c = Coordinator::new();
    let b = c.permission_bridge();
    assert!(b.is_idle());
    assert_eq!(b.pending_count(), 0);
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — dispatch Phase-1 stub
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn dispatch_returns_not_implemented_in_phase_1() {
    // PINS PHASE-1: dispatch is a stub that errors until Phase 2.
    let mut c = Coordinator::new();
    let outcome = c.dispatch();
    assert!(outcome.is_err());
    match outcome {
        Err(CoordinatorError::NotImplemented) => {}
        _ => panic!("MUST return NotImplemented; got {outcome:?}"),
    }
}

#[test]
fn dispatch_error_display_mentions_phase_2() {
    let mut c = Coordinator::new();
    let err = c.dispatch().unwrap_err();
    let s = err.to_string();
    assert!(
        s.contains("Phase 2") || s.contains("not implemented"),
        "MUST mention Phase 2 or not implemented; got {s}"
    );
}

#[test]
fn dispatch_does_not_advance_queue_state() {
    let mut c = Coordinator::new();
    let id = c.submit(Task::new(AgentType::Explore, "x")).expect("ok");
    // dispatch errors without touching queue.
    let _ = c.dispatch();
    // Queue still has the task.
    assert!(c.queue().get(id).is_some());
}

#[test]
fn dispatch_repeated_calls_return_same_not_implemented() {
    let mut c = Coordinator::new();
    for _ in 0..5 {
        let outcome = c.dispatch();
        assert!(matches!(outcome, Err(CoordinatorError::NotImplemented)));
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — CoordinatorError From conversions
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn coordinator_error_from_task_queue_error_via_from_derive() {
    // PINS DERIVE: #[from] TaskQueueError → CoordinatorError::Queue.
    use openclaudia::coordinator::TaskQueueError;
    let mut c = Coordinator::new();
    let id = c.submit(Task::new(AgentType::Explore, "x")).expect("ok");
    let inner_err: TaskQueueError = c.add_dependency(id, id).unwrap_err();
    // Convert via From.
    let co_err: CoordinatorError = inner_err.into();
    assert!(matches!(co_err, CoordinatorError::Queue(_)));
}

#[test]
fn coordinator_error_display_for_queue_variant_includes_inner() {
    use openclaudia::coordinator::TaskQueueError;
    let mut c = Coordinator::new();
    let id = c.submit(Task::new(AgentType::Explore, "x")).expect("ok");
    let inner: TaskQueueError = c.add_dependency(id, id).unwrap_err();
    let co_err: CoordinatorError = inner.into();
    let s = co_err.to_string();
    assert!(s.contains("task queue error") || s.contains("cycle"));
}
