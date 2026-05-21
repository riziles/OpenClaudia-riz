//! End-to-end tests for `services::background` jobs not covered
//! by sprint 77 — `AgentSummaryJob`, `PluginDelistingJob`,
//! `PluginAutoupdateJob` outcome shape + name + no-data behavior.
//!
//! Sprint 114 of the verification effort. Sprint 77
//! (`service_registry_jobs_e2e`) covered
//! `MemoryConsolidationJob` end-to-end + `PluginAutoupdateJob`
//! outcome shape; this file pins the other 2 documented
//! `BackgroundJob` impls (`AgentSummaryJob`,
//! `PluginDelistingJob`) — name accessor + empty-db no-op
//! behavior + `JobOutcome` shape preservation.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::memory::MemoryDb;
use openclaudia::services::{
    AgentSummaryJob, BackgroundJob, JobOutcome, MemoryConsolidationJob, PluginAutoupdateJob,
    PluginDelistingJob,
};
use std::sync::Arc;
use tempfile::TempDir;

// ───────────────────────────────────────────────────────────────────────────
// Helpers
// ───────────────────────────────────────────────────────────────────────────

fn fresh_db() -> (Arc<MemoryDb>, TempDir) {
    let dir = TempDir::new().expect("tempdir");
    let db = MemoryDb::open(&dir.path().join("memory.db")).expect("open db");
    (Arc::new(db), dir)
}

// ───────────────────────────────────────────────────────────────────────────
// Section A — Job name accessors are stable identifiers
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn memory_consolidation_job_name_is_memory_consolidation() {
    assert_eq!(MemoryConsolidationJob.name(), "memory_consolidation");
}

#[test]
fn plugin_autoupdate_job_name_is_plugin_autoupdate() {
    let job = PluginAutoupdateJob::new(Vec::new());
    assert_eq!(job.name(), "plugin_autoupdate");
}

#[test]
fn plugin_delisting_job_name_is_plugin_delisting_check() {
    // Authoring discovery: actual name is
    // "plugin_delisting_check", not "plugin_delisting".
    let job = PluginDelistingJob::new(Vec::new());
    assert_eq!(job.name(), "plugin_delisting_check");
}

#[test]
fn agent_summary_job_name_is_agent_summary() {
    assert_eq!(AgentSummaryJob.name(), "agent_summary");
}

#[test]
fn job_names_are_pairwise_distinct() {
    let names = [
        MemoryConsolidationJob.name(),
        PluginAutoupdateJob::new(Vec::new()).name(),
        PluginDelistingJob::new(Vec::new()).name(),
        AgentSummaryJob.name(),
    ];
    let mut sorted: Vec<&'static str> = names.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        names.len(),
        "job names MUST be pairwise distinct; got {} unique of {}",
        sorted.len(),
        names.len()
    );
}

#[test]
fn job_names_use_snake_case() {
    for name in &[
        MemoryConsolidationJob.name(),
        PluginAutoupdateJob::new(Vec::new()).name(),
        PluginDelistingJob::new(Vec::new()).name(),
        AgentSummaryJob.name(),
    ] {
        assert!(
            name.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "job name {name:?} MUST be snake_case lowercase ASCII"
        );
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — AgentSummaryJob on empty DB
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn agent_summary_job_on_empty_db_returns_ok_outcome_with_zeros() {
    let (db, _dir) = fresh_db();
    let job = AgentSummaryJob;
    let outcome = job.run(&db).expect("run ok");
    // No archival rows → nothing to summarise. Outcome should
    // report 0 affected / 0 errors.
    assert_eq!(outcome.records_pruned, 0);
    assert_eq!(outcome.records_deduped, 0);
}

#[test]
fn agent_summary_job_on_db_with_only_unrelated_tags_is_noop() {
    let (db, _dir) = fresh_db();
    // Write rows tagged with something OTHER than subagent-task.
    db.memory_save("first", &["unrelated".to_string()])
        .expect("save");
    db.memory_save("second", &["other".to_string()])
        .expect("save");
    let job = AgentSummaryJob;
    let outcome = job.run(&db).expect("run ok");
    // No subagent-task rows → nothing folded.
    assert_eq!(outcome.records_pruned, 0);
}

#[test]
fn agent_summary_job_is_send_for_background_job_trait_object() {
    let job: Arc<dyn BackgroundJob> = Arc::new(AgentSummaryJob);
    assert_eq!(job.name(), "agent_summary");
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — PluginDelistingJob on empty DB
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn plugin_delisting_job_default_constructor_works() {
    let _job = PluginDelistingJob::new(Vec::new());
}

#[test]
fn plugin_delisting_job_on_empty_db_returns_ok() {
    let (db, _dir) = fresh_db();
    let job = PluginDelistingJob::new(Vec::new());
    let outcome = job.run(&db).expect("run ok");
    assert_eq!(outcome.records_deduped, 0);
}

#[test]
fn plugin_delisting_job_is_send_for_background_job_trait_object() {
    let job: Arc<dyn BackgroundJob> = Arc::new(PluginDelistingJob::new(Vec::new()));
    assert_eq!(job.name(), "plugin_delisting_check");
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — PluginAutoupdateJob on empty DB
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn plugin_autoupdate_job_default_constructor_works() {
    let _job = PluginAutoupdateJob::new(Vec::new());
}

#[test]
fn plugin_autoupdate_job_is_send_for_background_job_trait_object() {
    let job: Arc<dyn BackgroundJob> = Arc::new(PluginAutoupdateJob::new(Vec::new()));
    assert_eq!(job.name(), "plugin_autoupdate");
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — JobOutcome shape + PartialEq
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn job_outcome_carries_affected_and_errors_fields() {
    let outcome = JobOutcome {
        job_name: "test",
        records_pruned: 5,
        records_deduped: 1,
    };
    assert_eq!(outcome.records_pruned, 5);
    assert_eq!(outcome.records_deduped, 1);
}

#[test]
fn job_outcome_clone_preserves_fields() {
    let original = JobOutcome {
        job_name: "test",
        records_pruned: 100,
        records_deduped: 2,
    };
    let cloned = original.clone();
    assert_eq!(cloned.records_pruned, original.records_pruned);
    assert_eq!(cloned.records_deduped, original.records_deduped);
}

#[test]
fn job_outcome_struct_literal_construction_works() {
    let outcome = JobOutcome {
        job_name: "literal",
        records_pruned: 0,
        records_deduped: 0,
    };
    assert_eq!(outcome.job_name, "literal");
    assert_eq!(outcome.records_pruned, 0);
    assert_eq!(outcome.records_deduped, 0);
}

#[test]
fn job_outcome_eq_holds_for_identical_outcomes() {
    let a = JobOutcome {
        job_name: "test",
        records_pruned: 3,
        records_deduped: 0,
    };
    let b = JobOutcome {
        job_name: "test",
        records_pruned: 3,
        records_deduped: 0,
    };
    assert_eq!(a, b);
}

#[test]
fn job_outcome_eq_distinguishes_different_outcomes() {
    let a = JobOutcome {
        job_name: "test",
        records_pruned: 3,
        records_deduped: 0,
    };
    let b = JobOutcome {
        job_name: "test",
        records_pruned: 3,
        records_deduped: 1,
    };
    assert_ne!(a, b);
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — Send + Sync trait object dispatch
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn background_job_trait_is_send_compile_time_check() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    assert_send::<MemoryConsolidationJob>();
    assert_send::<AgentSummaryJob>();
    assert_send::<PluginAutoupdateJob>();
    assert_send::<PluginDelistingJob>();
    assert_sync::<MemoryConsolidationJob>();
    assert_sync::<AgentSummaryJob>();
    assert_sync::<PluginAutoupdateJob>();
    assert_sync::<PluginDelistingJob>();
}

#[test]
fn background_jobs_can_be_collected_into_homogeneous_vec_of_trait_objects() {
    let jobs: Vec<Arc<dyn BackgroundJob>> = vec![
        Arc::new(MemoryConsolidationJob),
        Arc::new(AgentSummaryJob),
        Arc::new(PluginAutoupdateJob::new(Vec::new())),
        Arc::new(PluginDelistingJob::new(Vec::new())),
    ];
    assert_eq!(jobs.len(), 4);
    // Each job has a distinct name accessible through the
    // trait object.
    let names: Vec<&str> = jobs.iter().map(|j| j.name()).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), 4);
}

#[test]
fn every_documented_job_can_run_against_fresh_db_without_panic() {
    let (db, _dir) = fresh_db();
    // Each job runs on an empty DB and returns Ok without
    // panic. This is the documented minimum contract for
    // every BackgroundJob impl.
    let jobs: Vec<Arc<dyn BackgroundJob>> = vec![
        Arc::new(MemoryConsolidationJob),
        Arc::new(AgentSummaryJob),
        Arc::new(PluginAutoupdateJob::new(Vec::new())),
        Arc::new(PluginDelistingJob::new(Vec::new())),
    ];
    for job in jobs {
        let _outcome = job.run(&db).expect("each job MUST succeed on empty db");
    }
}
