//! End-to-end tests for `coordinator::task_queue::TaskQueue`
//! state-machine gaps that sprint 27's `coordinator_e2e` left
//! uncovered.
//!
//! Sprint 75 of the verification effort. Sprint 27 covered
//! `submit` + simple `next_ready` paths + cycle detection; this
//! file covers the `len`/`is_empty` accessors, `get`/`get_mut`
//! lookups, multi-hop dependency unblocking, the
//! `TaskState::Failed` semantics (does NOT unblock
//! dependents — only `Done` does), `Task::new` defaults,
//! `Task::depends_on` builder, and the `add_dependency`
//! `UnknownTask` arm.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::coordinator::task_queue::{Task, TaskId, TaskQueue, TaskQueueError, TaskState};
use openclaudia::subagent::AgentType;

// ───────────────────────────────────────────────────────────────────────────
// Helpers
// ───────────────────────────────────────────────────────────────────────────

fn submit(q: &mut TaskQueue, label: &str) -> TaskId {
    q.submit(Task::new(AgentType::Explore, label))
        .expect("submit")
}

fn submit_with_deps(q: &mut TaskQueue, label: &str, deps: Vec<TaskId>) -> TaskId {
    q.submit(Task::new(AgentType::Explore, label).depends_on(deps))
        .expect("submit")
}

fn mark_done(q: &mut TaskQueue, id: TaskId, output: &str) {
    q.get_mut(id).expect("present").state = TaskState::Done(output.to_string());
}

fn mark_failed(q: &mut TaskQueue, id: TaskId, reason: &str) {
    q.get_mut(id).expect("present").state = TaskState::Failed(reason.to_string());
}

/// Fabricate a `TaskId` that almost-certainly isn't in any queue.
/// `TaskId`'s inner field is private, so we construct via serde —
/// `TaskId` derives `Serialize`/`Deserialize` over the inner `u64`
/// directly.
fn fake_id(n: u64) -> TaskId {
    serde_json::from_value(serde_json::json!(n)).expect("TaskId from u64")
}

// ───────────────────────────────────────────────────────────────────────────
// Section A — Task::new defaults + depends_on builder
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn task_new_starts_pending_with_placeholder_id() {
    let t = Task::new(AgentType::Plan, "do thing");
    assert_eq!(t.id, fake_id(0), "placeholder id MUST be 0 (sentinel)");
    assert_eq!(t.subagent_type, AgentType::Plan);
    assert_eq!(t.prompt, "do thing");
    assert!(t.depends_on.is_empty());
    assert!(t.assigned_to.is_none());
    assert!(matches!(t.state, TaskState::Pending));
}

#[test]
fn task_depends_on_builder_sets_dependency_list() {
    let deps = vec![fake_id(1), fake_id(2), fake_id(3)];
    let t = Task::new(AgentType::Explore, "x").depends_on(deps.clone());
    assert_eq!(t.depends_on, deps);
}

#[test]
fn task_depends_on_builder_is_chainable() {
    // Verify the &mut return path is composable in a single
    // statement.
    let t = Task::new(AgentType::Coordinator, "p").depends_on(vec![fake_id(7)]);
    assert_eq!(t.depends_on, vec![fake_id(7)]);
    assert_eq!(t.prompt, "p");
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — TaskQueue len / is_empty accessors
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn fresh_queue_has_len_zero_and_is_empty() {
    let q = TaskQueue::new();
    assert_eq!(q.len(), 0);
    assert!(q.is_empty());
}

#[test]
fn len_grows_with_each_submit() {
    let mut q = TaskQueue::new();
    submit(&mut q, "a");
    assert_eq!(q.len(), 1);
    submit(&mut q, "b");
    assert_eq!(q.len(), 2);
    submit(&mut q, "c");
    assert_eq!(q.len(), 3);
    assert!(!q.is_empty());
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — get / get_mut lookups
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn get_returns_some_for_existing_id() {
    let mut q = TaskQueue::new();
    let id = submit(&mut q, "test-task");
    let task = q.get(id).expect("MUST find existing task");
    assert_eq!(task.id, id);
    assert_eq!(task.prompt, "test-task");
}

#[test]
fn get_returns_none_for_unknown_id() {
    let q = TaskQueue::new();
    assert!(q.get(fake_id(9999)).is_none());
}

#[test]
fn get_mut_allows_state_mutation_in_place() {
    let mut q = TaskQueue::new();
    let id = submit(&mut q, "x");
    q.get_mut(id).expect("present").state = TaskState::Running;
    assert!(matches!(
        q.get(id).expect("present").state,
        TaskState::Running
    ));
}

#[test]
fn get_mut_returns_none_for_unknown_id() {
    let mut q = TaskQueue::new();
    assert!(q.get_mut(fake_id(9999)).is_none());
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — Multi-hop dependency unblocking
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn three_hop_chain_unblocks_progressively() {
    let mut q = TaskQueue::new();
    let a = submit(&mut q, "a");
    let b = submit_with_deps(&mut q, "b", vec![a]);
    let c = submit_with_deps(&mut q, "c", vec![b]);

    // Only a is ready initially.
    let first = q.next_ready().expect("a ready");
    assert_eq!(first.id, a);

    // Mark a done; now b becomes ready.
    mark_done(&mut q, a, "a-output");
    let second = q.next_ready().expect("b ready after a");
    assert_eq!(second.id, b);
    // Caller is responsible for marking the returned task
    // Running — next_ready doesn't auto-transition.
    second.state = TaskState::Running;

    // b Running → c blocked (next_ready only returns Pending).
    assert!(
        q.next_ready().is_none(),
        "c MUST still be blocked while b is running"
    );

    // Mark b done; c becomes ready.
    mark_done(&mut q, b, "b-output");
    let third = q.next_ready().expect("c ready after b");
    assert_eq!(third.id, c);
}

#[test]
fn task_with_multiple_deps_waits_for_all_to_complete() {
    let mut q = TaskQueue::new();
    let a = submit(&mut q, "a");
    let b = submit(&mut q, "b");
    let c = submit_with_deps(&mut q, "c", vec![a, b]);

    // Only a + b ready; c blocked.
    mark_done(&mut q, a, "a-out");
    // c still blocked (b not done).
    let next = q.next_ready();
    let next_id = next.expect("b should be ready").id;
    assert_eq!(next_id, b, "b ready before c");

    mark_done(&mut q, b, "b-out");
    let final_ready = q.next_ready().expect("c now ready");
    assert_eq!(final_ready.id, c);
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — TaskState::Failed semantics
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn failed_predecessor_does_not_unblock_dependent() {
    // PINS DOCUMENTED CONTRACT: next_ready checks for Done
    // specifically, NOT Failed. A failed predecessor leaves
    // the dependent stuck — the coordinator must decide
    // whether to abort or retry.
    let mut q = TaskQueue::new();
    let a = submit(&mut q, "a");
    let _b = submit_with_deps(&mut q, "b", vec![a]);

    mark_done(&mut q, a, "unused-because-we-flip-to-failed-next");
    // Actually mark failed for the test.
    mark_failed(&mut q, a, "simulated failure");

    let next = q.next_ready();
    assert!(
        next.is_none(),
        "Failed predecessor MUST NOT unblock dependent; got {:?}",
        next.map(|t| t.id)
    );
}

#[test]
fn task_state_failed_carries_reason_payload() {
    let mut q = TaskQueue::new();
    let id = submit(&mut q, "x");
    mark_failed(&mut q, id, "boom");
    let task = q.get(id).expect("present");
    let TaskState::Failed(reason) = &task.state else {
        panic!("expected Failed; got {:?}", task.state);
    };
    assert_eq!(reason, "boom");
}

#[test]
fn task_state_done_carries_output_payload() {
    let mut q = TaskQueue::new();
    let id = submit(&mut q, "x");
    mark_done(&mut q, id, "tool output text");
    let task = q.get(id).expect("present");
    let TaskState::Done(output) = &task.state else {
        panic!("expected Done");
    };
    assert_eq!(output, "tool output text");
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — add_dependency UnknownTask + edge cases
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn add_dependency_unknown_from_errors() {
    let mut q = TaskQueue::new();
    let real = submit(&mut q, "a");
    let outcome = q.add_dependency(fake_id(9999), real);
    assert_eq!(
        outcome,
        Err(TaskQueueError::UnknownTask {
            missing: fake_id(9999)
        })
    );
}

#[test]
fn add_dependency_unknown_to_errors() {
    let mut q = TaskQueue::new();
    let real = submit(&mut q, "a");
    let outcome = q.add_dependency(real, fake_id(9999));
    assert_eq!(
        outcome,
        Err(TaskQueueError::UnknownTask {
            missing: fake_id(9999)
        })
    );
}

#[test]
fn add_dependency_self_loop_is_detected_as_cycle() {
    let mut q = TaskQueue::new();
    let id = submit(&mut q, "a");
    let outcome = q.add_dependency(id, id);
    assert!(
        matches!(outcome, Err(TaskQueueError::CycleDetected { .. })),
        "self-loop MUST be cycle; got {outcome:?}"
    );
}

#[test]
fn add_dependency_long_chain_cycle_detected() {
    // a → b → c → d → e — try adding a → e back-edge (the
    // graph already has a → b → c → d → e via forward deps),
    // so adding a → e would not actually cycle; but adding
    // e → a should.
    let mut queue = TaskQueue::new();
    let task_a = submit(&mut queue, "a");
    let task_b = submit_with_deps(&mut queue, "b", vec![task_a]);
    let task_c = submit_with_deps(&mut queue, "c", vec![task_b]);
    let task_d = submit_with_deps(&mut queue, "d", vec![task_c]);
    let task_e = submit_with_deps(&mut queue, "e", vec![task_d]);
    // e already depends on a transitively (via d → c → b → a).
    // Adding a → e MUST cycle.
    let outcome = queue.add_dependency(task_a, task_e);
    assert!(
        matches!(outcome, Err(TaskQueueError::CycleDetected { .. })),
        "long-chain cycle MUST be detected; got {outcome:?}"
    );
}

#[test]
fn add_dependency_admits_new_edge_when_no_cycle_forms() {
    let mut q = TaskQueue::new();
    let a = submit(&mut q, "a");
    let b = submit(&mut q, "b"); // independent
                                 // Adding a → b is fine.
    q.add_dependency(a, b).expect("no cycle");
    // a now depends on b.
    let a_task = q.get(a).expect("present");
    assert!(a_task.depends_on.contains(&b));
}

// ───────────────────────────────────────────────────────────────────────────
// Section G — TaskId Display + Eq
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn task_id_display_renders_as_integer() {
    let id = fake_id(42);
    assert_eq!(format!("{id}"), "42");
}

#[test]
fn task_ids_with_same_value_compare_equal() {
    assert_eq!(fake_id(7), fake_id(7));
    assert_ne!(fake_id(7), fake_id(8));
}

#[test]
fn task_id_raw_accessor_returns_inner_u64() {
    let id = fake_id(123);
    assert_eq!(id.raw(), 123);
}

// ───────────────────────────────────────────────────────────────────────────
// Section H — TaskQueueError Display
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn unknown_task_error_display_mentions_missing_id() {
    let err = TaskQueueError::UnknownTask {
        missing: fake_id(42),
    };
    let msg = format!("{err}");
    assert!(msg.contains("42"));
    assert!(msg.to_lowercase().contains("not"));
}

#[test]
fn cycle_detected_error_display_mentions_both_endpoints() {
    let err = TaskQueueError::CycleDetected {
        from: fake_id(1),
        to: fake_id(2),
    };
    let msg = format!("{err}");
    assert!(msg.contains('1'));
    assert!(msg.contains('2'));
    assert!(msg.to_lowercase().contains("cycle"));
}
