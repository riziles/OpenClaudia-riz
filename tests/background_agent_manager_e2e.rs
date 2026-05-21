//! End-to-end tests for `subagent::BackgroundAgentManager`
//! lifecycle — `register` / `get` / `finish` / `fail` /
//! `increment_turns` / `list` / `remove` / `gc` /
//! `cleanup_finished`, plus `BackgroundAgent` field
//! accessibility.
//!
//! Sprint 126 of the verification effort. Sprint 60
//! covered `AgentType` parsing; sprint 125 covered the
//! tool-definition wire shape; this file pins the
//! in-memory background-agent registry used to track
//! `run_in_background: true` task invocations.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::subagent::{AgentType, BackgroundAgentManager};
use std::sync::atomic::Ordering;

// ───────────────────────────────────────────────────────────────────────────
// Section A — Constructor + empty state
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn manager_new_starts_with_empty_agent_list() {
    let mgr = BackgroundAgentManager::new();
    assert!(mgr.list().is_empty());
}

#[test]
fn manager_get_on_unknown_id_returns_none() {
    let mgr = BackgroundAgentManager::new();
    assert!(mgr.get("nonexistent-id").is_none());
}

#[test]
fn manager_remove_on_unknown_id_returns_none() {
    let mgr = BackgroundAgentManager::new();
    assert!(mgr.remove("nonexistent-id").is_none());
}

#[test]
fn manager_gc_on_empty_returns_zero_evicted() {
    let mgr = BackgroundAgentManager::new();
    let evicted = mgr.gc();
    assert_eq!(evicted, 0);
}

#[test]
fn manager_cleanup_finished_on_empty_returns_zero() {
    let mgr = BackgroundAgentManager::new();
    let removed = mgr.cleanup_finished();
    assert_eq!(removed, 0);
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — register lifecycle
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn register_returns_non_empty_id_string() {
    let mgr = BackgroundAgentManager::new();
    let id = mgr.register(AgentType::Explore, "find files");
    assert!(!id.is_empty());
}

#[test]
fn register_two_agents_yields_distinct_ids() {
    let mgr = BackgroundAgentManager::new();
    let id_a = mgr.register(AgentType::Explore, "a");
    let id_b = mgr.register(AgentType::Plan, "b");
    assert_ne!(id_a, id_b);
}

#[test]
fn register_then_get_returns_arc_with_documented_fields() {
    let mgr = BackgroundAgentManager::new();
    let id = mgr.register(AgentType::Plan, "design an API");
    let agent = mgr.get(&id).expect("present");
    assert_eq!(agent.id, id);
    assert_eq!(agent.agent_type, AgentType::Plan);
    assert_eq!(agent.task, "design an API");
    assert!(!agent.finished.load(Ordering::SeqCst));
    assert_eq!(agent.turns.load(Ordering::SeqCst), 0);
}

#[test]
fn register_with_id_inserts_entry_when_id_is_fresh() {
    let mgr = BackgroundAgentManager::new();
    let was_new = mgr.register_with_id(AgentType::Guide, "task", "fresh-id-123");
    assert!(was_new);
    let agent = mgr.get("fresh-id-123").expect("present");
    assert_eq!(agent.id, "fresh-id-123");
}

#[test]
fn register_with_id_returns_false_when_id_already_exists() {
    // PINS RESUME CONTRACT #582: duplicate-id register is
    // a no-op (NOT a replace) — used by the resume path.
    let mgr = BackgroundAgentManager::new();
    let was_new = mgr.register_with_id(AgentType::Explore, "first", "shared-id");
    assert!(was_new);
    let was_new_again = mgr.register_with_id(AgentType::Plan, "second", "shared-id");
    assert!(!was_new_again, "duplicate id MUST return false");
    // Original entry preserved.
    let agent = mgr.get("shared-id").expect("present");
    assert_eq!(agent.task, "first", "original task MUST be preserved");
    assert_eq!(
        agent.agent_type,
        AgentType::Explore,
        "original agent_type MUST be preserved"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — finish + fail
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn finish_sets_finished_flag_and_stores_result() {
    let mgr = BackgroundAgentManager::new();
    let id = mgr.register(AgentType::Explore, "task");
    mgr.finish(&id, "result body".to_string());
    let agent = mgr.get(&id).expect("present");
    assert!(agent.finished.load(Ordering::SeqCst));
    let observed = {
        let result = agent.result.lock().unwrap();
        result.clone()
    };
    assert_eq!(observed.as_deref(), Some("result body"));
}

#[test]
fn fail_sets_finished_flag_and_stores_error() {
    let mgr = BackgroundAgentManager::new();
    let id = mgr.register(AgentType::Explore, "task");
    mgr.fail(&id, "execution error".to_string());
    let agent = mgr.get(&id).expect("present");
    assert!(agent.finished.load(Ordering::SeqCst));
    let observed = {
        let error = agent.error.lock().unwrap();
        error.clone()
    };
    assert_eq!(observed.as_deref(), Some("execution error"));
}

#[test]
fn finish_on_unknown_id_is_silent_no_op() {
    let mgr = BackgroundAgentManager::new();
    // No panic on unknown id.
    mgr.finish("nonexistent", "result".to_string());
}

#[test]
fn fail_on_unknown_id_is_silent_no_op() {
    let mgr = BackgroundAgentManager::new();
    mgr.fail("nonexistent", "error".to_string());
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — increment_turns
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn increment_turns_returns_new_value_starting_at_1() {
    let mgr = BackgroundAgentManager::new();
    let id = mgr.register(AgentType::Explore, "task");
    let n = mgr.increment_turns(&id);
    assert_eq!(n, 1);
}

#[test]
fn increment_turns_is_monotonic_across_calls() {
    let mgr = BackgroundAgentManager::new();
    let id = mgr.register(AgentType::Explore, "task");
    let n1 = mgr.increment_turns(&id);
    let n2 = mgr.increment_turns(&id);
    let n3 = mgr.increment_turns(&id);
    assert_eq!(n1, 1);
    assert_eq!(n2, 2);
    assert_eq!(n3, 3);
}

#[test]
fn increment_turns_visible_via_get_atomic_counter() {
    let mgr = BackgroundAgentManager::new();
    let id = mgr.register(AgentType::Explore, "task");
    mgr.increment_turns(&id);
    mgr.increment_turns(&id);
    let agent = mgr.get(&id).expect("present");
    assert_eq!(agent.turns.load(Ordering::SeqCst), 2);
}

#[test]
fn increment_turns_on_unknown_id_returns_zero() {
    let mgr = BackgroundAgentManager::new();
    let n = mgr.increment_turns("nonexistent");
    // No-op: agent doesn't exist, no turn recorded.
    assert_eq!(n, 0);
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — list + remove
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn list_after_3_registers_returns_3_tuples() {
    let mgr = BackgroundAgentManager::new();
    mgr.register(AgentType::Explore, "a");
    mgr.register(AgentType::Plan, "b");
    mgr.register(AgentType::Guide, "c");
    let listed = mgr.list();
    assert_eq!(listed.len(), 3);
}

#[test]
fn list_tuple_carries_id_type_task_finished() {
    let mgr = BackgroundAgentManager::new();
    let id = mgr.register(AgentType::Plan, "design");
    let listed = mgr.list();
    assert_eq!(listed.len(), 1);
    let (got_id, got_type, got_task, got_finished) = &listed[0];
    assert_eq!(got_id, &id);
    assert_eq!(*got_type, AgentType::Plan);
    assert_eq!(got_task, "design");
    assert!(!*got_finished);
}

#[test]
fn list_reflects_finished_status_after_finish() {
    let mgr = BackgroundAgentManager::new();
    let id = mgr.register(AgentType::Explore, "task");
    mgr.finish(&id, "done".to_string());
    let listed = mgr.list();
    let (_, _, _, finished) = &listed[0];
    assert!(*finished);
}

#[test]
fn remove_returns_some_arc_for_existing_id() {
    let mgr = BackgroundAgentManager::new();
    let id = mgr.register(AgentType::Explore, "task");
    let removed = mgr.remove(&id);
    assert!(removed.is_some());
    // After remove, get returns None.
    assert!(mgr.get(&id).is_none());
}

#[test]
fn remove_returns_none_for_unknown_id() {
    let mgr = BackgroundAgentManager::new();
    let outcome = mgr.remove("nonexistent");
    assert!(outcome.is_none());
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — cleanup_finished
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn cleanup_finished_removes_only_finished_agents() {
    let mgr = BackgroundAgentManager::new();
    let id_running = mgr.register(AgentType::Explore, "running");
    let id_done = mgr.register(AgentType::Plan, "done");
    mgr.finish(&id_done, "result".to_string());
    let removed = mgr.cleanup_finished();
    assert_eq!(removed, 1);
    // Running agent still present, done one gone.
    assert!(mgr.get(&id_running).is_some());
    assert!(mgr.get(&id_done).is_none());
}

#[test]
fn cleanup_finished_with_only_running_agents_removes_nothing() {
    let mgr = BackgroundAgentManager::new();
    mgr.register(AgentType::Explore, "a");
    mgr.register(AgentType::Plan, "b");
    let removed = mgr.cleanup_finished();
    assert_eq!(removed, 0);
}

// ───────────────────────────────────────────────────────────────────────────
// Section G — Default impl
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn manager_default_equals_new_for_empty_state() {
    let default_mgr = BackgroundAgentManager::default();
    let new_mgr = BackgroundAgentManager::new();
    assert_eq!(default_mgr.list().len(), new_mgr.list().len());
}
