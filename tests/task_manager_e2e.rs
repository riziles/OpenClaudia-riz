//! End-to-end tests for `TaskManager` session-side lifecycle:
//! create → status transitions → dependency edges → delete.
//!
//! Sprint 43 of the verification effort.
//!
//! `tests/coordinator_e2e.rs` (sprint 13) covers the coordinator
//! queue; this file covers the session-level `TaskManager` that
//! drives the model-visible todo list.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::session::{Task, TaskManager, TaskStatus, TaskUpdateParams, TaskUpdateStatus};

// ───────────────────────────────────────────────────────────────────────────
// Helpers
// ───────────────────────────────────────────────────────────────────────────

fn add(mgr: &mut TaskManager, subject: &str) -> String {
    mgr.create_task(subject.to_string(), String::new(), None)
        .id
        .clone()
}

fn status_of(mgr: &TaskManager, id: &str) -> Option<TaskStatus> {
    mgr.get_task(id).map(|t| t.status.clone())
}

fn update_status<'m>(
    mgr: &'m mut TaskManager,
    id: &str,
    s: TaskUpdateStatus,
) -> Result<Option<&'m Task>, String> {
    mgr.update_task(
        id,
        TaskUpdateParams {
            status: Some(s),
            ..TaskUpdateParams::default()
        },
    )
}

// ───────────────────────────────────────────────────────────────────────────
// Section A — TaskUpdateStatus::parse
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn parse_accepts_every_documented_status_string() {
    assert_eq!(
        TaskUpdateStatus::parse("pending"),
        Some(TaskUpdateStatus::Pending)
    );
    assert_eq!(
        TaskUpdateStatus::parse("in_progress"),
        Some(TaskUpdateStatus::InProgress)
    );
    assert_eq!(
        TaskUpdateStatus::parse("completed"),
        Some(TaskUpdateStatus::Completed)
    );
    assert_eq!(
        TaskUpdateStatus::parse("deleted"),
        Some(TaskUpdateStatus::Deleted)
    );
}

#[test]
fn parse_rejects_unknown_status_strings() {
    for input in &[
        "",
        "PENDING",
        "InProgress",
        "done",
        "removed",
        "in-progress",
    ] {
        assert_eq!(
            TaskUpdateStatus::parse(input),
            None,
            "{input:?} MUST NOT parse"
        );
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — create_task
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn create_task_yields_pending_with_incrementing_ids() {
    let mut mgr = TaskManager::new();
    let id1 = add(&mut mgr, "first");
    let id2 = add(&mut mgr, "second");
    let id3 = add(&mut mgr, "third");
    assert_eq!(id1, "task-1");
    assert_eq!(id2, "task-2");
    assert_eq!(id3, "task-3");
    for id in [&id1, &id2, &id3] {
        assert_eq!(status_of(&mgr, id), Some(TaskStatus::Pending));
    }
}

#[test]
fn create_task_starts_with_no_dependencies() {
    let mut mgr = TaskManager::new();
    let id = add(&mut mgr, "alone");
    let task = mgr.get_task(&id).unwrap();
    assert!(task.blocks.is_empty());
    assert!(task.blocked_by.is_empty());
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — Single-InProgress invariant
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn second_in_progress_demotes_first_to_pending() {
    let mut mgr = TaskManager::new();
    let a = add(&mut mgr, "A");
    let b = add(&mut mgr, "B");

    update_status(&mut mgr, &a, TaskUpdateStatus::InProgress).expect("set A InProgress");
    assert_eq!(status_of(&mgr, &a), Some(TaskStatus::InProgress));

    // Now set B to InProgress — A MUST be demoted to Pending.
    update_status(&mut mgr, &b, TaskUpdateStatus::InProgress).expect("set B InProgress");
    assert_eq!(status_of(&mgr, &b), Some(TaskStatus::InProgress));
    assert_eq!(
        status_of(&mgr, &a),
        Some(TaskStatus::Pending),
        "A MUST be demoted when B transitions to InProgress"
    );
}

#[test]
fn current_task_returns_the_in_progress_one() {
    let mut mgr = TaskManager::new();
    let _a = add(&mut mgr, "A");
    let b = add(&mut mgr, "B");
    assert!(mgr.current_task().is_none(), "no in-progress task yet");
    update_status(&mut mgr, &b, TaskUpdateStatus::InProgress).expect("set");
    let current = mgr.current_task().expect("there is a current task");
    assert_eq!(current.id, b);
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — Blocked-by guard (crosslink #593)
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn in_progress_refused_when_blocker_not_completed() {
    let mut mgr = TaskManager::new();
    let upstream = add(&mut mgr, "upstream");
    let dependent = add(&mut mgr, "dependent");
    // Make `dependent` depend on `upstream`.
    mgr.update_task(
        &dependent,
        TaskUpdateParams {
            add_blocked_by: Some(vec![upstream.clone()]),
            ..TaskUpdateParams::default()
        },
    )
    .expect("add dep");

    // Upstream is still Pending — transitioning `dependent` to
    // InProgress MUST error.
    let outcome = update_status(&mut mgr, &dependent, TaskUpdateStatus::InProgress);
    let Err(msg) = outcome else {
        panic!("blocked-by must refuse InProgress; got Ok");
    };
    assert!(
        msg.contains(&upstream) && msg.contains("pending"),
        "error must name the offending upstream + its status; got {msg:?}"
    );

    // The dependent task MUST still be Pending.
    assert_eq!(status_of(&mgr, &dependent), Some(TaskStatus::Pending));
}

#[test]
fn in_progress_admitted_after_blocker_completes() {
    let mut mgr = TaskManager::new();
    let upstream = add(&mut mgr, "upstream");
    let dependent = add(&mut mgr, "dependent");
    mgr.update_task(
        &dependent,
        TaskUpdateParams {
            add_blocked_by: Some(vec![upstream.clone()]),
            ..TaskUpdateParams::default()
        },
    )
    .expect("add dep");
    // Complete the upstream.
    update_status(&mut mgr, &upstream, TaskUpdateStatus::Completed).expect("complete upstream");
    // Now `dependent` may transition.
    update_status(&mut mgr, &dependent, TaskUpdateStatus::InProgress)
        .expect("dependent → in_progress");
    assert_eq!(status_of(&mgr, &dependent), Some(TaskStatus::InProgress));
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — Delete
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn delete_removes_task_from_list() {
    let mut mgr = TaskManager::new();
    let a = add(&mut mgr, "A");
    let b = add(&mut mgr, "B");
    let out = update_status(&mut mgr, &a, TaskUpdateStatus::Deleted).expect("delete must succeed");
    assert!(
        out.is_none(),
        "Deleted variant MUST return Ok(None); got {:?}",
        out.map(|t| t.id.clone())
    );
    // A is gone.
    assert!(mgr.get_task(&a).is_none());
    // B is unaffected.
    assert!(mgr.get_task(&b).is_some());
    assert_eq!(mgr.list_tasks().len(), 1);
}

#[test]
fn delete_unknown_task_id_errors() {
    let mut mgr = TaskManager::new();
    let outcome = update_status(&mut mgr, "task-9999", TaskUpdateStatus::Deleted);
    let Err(msg) = outcome else {
        panic!("delete on unknown id MUST error");
    };
    assert!(
        msg.contains("task-9999"),
        "msg must name the id; got {msg:?}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — Dependency edges + reverse-edge sync
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn adding_blocks_edge_creates_reverse_blocked_by_edge() {
    let mut mgr = TaskManager::new();
    let a = add(&mut mgr, "A");
    let b = add(&mut mgr, "B");
    // A blocks B → B is blocked_by A.
    mgr.update_task(
        &a,
        TaskUpdateParams {
            add_blocks: Some(vec![b.clone()]),
            ..TaskUpdateParams::default()
        },
    )
    .expect("add blocks");
    let task_a = mgr.get_task(&a).unwrap();
    let task_b = mgr.get_task(&b).unwrap();
    assert!(task_a.blocks.contains(&b), "A.blocks must include B");
    assert!(
        task_b.blocked_by.contains(&a),
        "B.blocked_by MUST mirror A.blocks (symmetric)"
    );
}

#[test]
fn dependency_to_nonexistent_task_errors() {
    let mut mgr = TaskManager::new();
    let a = add(&mut mgr, "A");
    let outcome = mgr.update_task(
        &a,
        TaskUpdateParams {
            add_blocks: Some(vec!["task-9999".to_string()]),
            ..TaskUpdateParams::default()
        },
    );
    let Err(msg) = outcome else {
        panic!("nonexistent-dep MUST error");
    };
    assert!(
        msg.contains("task-9999"),
        "error must name the bad dep id; got {msg:?}"
    );
}

#[test]
fn dependency_to_self_errors() {
    let mut mgr = TaskManager::new();
    let a = add(&mut mgr, "A");
    let outcome = mgr.update_task(
        &a,
        TaskUpdateParams {
            add_blocks: Some(vec![a.clone()]),
            ..TaskUpdateParams::default()
        },
    );
    assert!(outcome.is_err(), "self-blocks MUST be refused");
}

// ───────────────────────────────────────────────────────────────────────────
// Section G — Field updates
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn subject_description_active_form_updates_round_trip() {
    let mut mgr = TaskManager::new();
    let id = add(&mut mgr, "old");
    mgr.update_task(
        &id,
        TaskUpdateParams {
            subject: Some("new subject".to_string()),
            description: Some("new desc".to_string()),
            active_form: Some("Doing thing".to_string()),
            ..TaskUpdateParams::default()
        },
    )
    .expect("update");

    let task = mgr.get_task(&id).unwrap();
    assert_eq!(task.subject, "new subject");
    assert_eq!(task.description, "new desc");
    assert_eq!(task.active_form.as_deref(), Some("Doing thing"));
}

#[test]
fn empty_update_params_leaves_task_unchanged() {
    let mut mgr = TaskManager::new();
    let id = add(&mut mgr, "stable");
    let before = mgr.get_task(&id).unwrap().clone();
    mgr.update_task(&id, TaskUpdateParams::default())
        .expect("noop update");
    let after = mgr.get_task(&id).unwrap();
    assert_eq!(before.subject, after.subject);
    assert_eq!(before.description, after.description);
    assert_eq!(before.status, after.status);
}

// ───────────────────────────────────────────────────────────────────────────
// Section H — TaskStatus serialization shape
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn task_status_display_matches_serde_snake_case() {
    let cases = &[
        (TaskStatus::Pending, "pending"),
        (TaskStatus::InProgress, "in_progress"),
        (TaskStatus::Completed, "completed"),
    ];
    for (status, expected) in cases {
        assert_eq!(format!("{status}"), *expected);
        let json = serde_json::to_string(status).expect("serialize");
        assert_eq!(json.trim_matches('"'), *expected);
    }
}

#[test]
fn task_serde_round_trip_preserves_all_fields() {
    let mut mgr = TaskManager::new();
    let id = add(&mut mgr, "round-trip");
    mgr.update_task(
        &id,
        TaskUpdateParams {
            description: Some("desc".to_string()),
            active_form: Some("doing".to_string()),
            ..TaskUpdateParams::default()
        },
    )
    .expect("update");
    let task = mgr.get_task(&id).unwrap().clone();

    let json = serde_json::to_string(&task).expect("serialize");
    let back: Task = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.id, task.id);
    assert_eq!(back.subject, task.subject);
    assert_eq!(back.description, task.description);
    assert_eq!(back.active_form, task.active_form);
    assert_eq!(back.status, task.status);
}

// ───────────────────────────────────────────────────────────────────────────
// Section I — list_tasks + format helpers
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn list_tasks_preserves_insertion_order() {
    let mut mgr = TaskManager::new();
    for name in &["first", "second", "third", "fourth"] {
        add(&mut mgr, name);
    }
    let listed = mgr.list_tasks();
    assert_eq!(listed.len(), 4);
    for (i, expected) in ["first", "second", "third", "fourth"].iter().enumerate() {
        assert_eq!(
            listed[i].subject, *expected,
            "tasks must surface in insertion order; pos {i}"
        );
    }
}

#[test]
fn format_task_summary_includes_id_and_subject() {
    let mut mgr = TaskManager::new();
    let id = add(&mut mgr, "implement feature X");
    let task = mgr.get_task(&id).unwrap();
    let summary = TaskManager::format_task_summary(task);
    assert!(
        summary.contains(&id),
        "summary must include id; got {summary:?}"
    );
    assert!(
        summary.contains("implement feature X"),
        "summary must include subject; got {summary:?}"
    );
}

#[test]
fn format_task_detail_includes_status_label() {
    let mut mgr = TaskManager::new();
    let id = add(&mut mgr, "task");
    let task = mgr.get_task(&id).unwrap();
    let detail = TaskManager::format_task_detail(task);
    assert!(
        detail.to_lowercase().contains("pending"),
        "detail must mention status; got {detail:?}"
    );
}
