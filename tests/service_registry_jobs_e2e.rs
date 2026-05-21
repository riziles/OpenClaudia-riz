//! End-to-end tests for `ServiceRegistry` builder methods +
//! `analytics_arc` / `flags_arc` shared-handle accessors +
//! `MemoryConsolidationJob` end-to-end against a real
//! `MemoryDb` + `PluginAutoupdateJob` outcome shape.
//!
//! Sprint 77 of the verification effort. Sprint 47 covered
//! `LspServerManager`, sprint 46 covered `JobScheduler` ticks
//! plus `MockRateLimit`; this file covers the `ServiceRegistry`
//! builder API plus the `MemoryConsolidationJob` body that
//! drives short-term prune plus archival dedup against a
//! tempdir-backed memory store.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::memory::MemoryDb;
use openclaudia::services::{
    AnalyticsEvent, AnalyticsSink, BackgroundJob, MemoryConsolidationJob, NoopAnalytics,
    PluginAutoupdateJob, ServiceRegistry, StaticFlags,
};
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

// ───────────────────────────────────────────────────────────────────────────
// Helpers
// ───────────────────────────────────────────────────────────────────────────

fn fresh_db() -> (Arc<MemoryDb>, TempDir) {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("memory.db");
    let db = MemoryDb::open(&path).expect("open db");
    (Arc::new(db), dir)
}

/// Recording sink that captures every event for assertion.
struct RecordingSink {
    events: Arc<Mutex<Vec<AnalyticsEvent>>>,
}

impl RecordingSink {
    fn new() -> (Arc<Self>, Arc<Mutex<Vec<AnalyticsEvent>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::new(Self {
            events: events.clone(),
        });
        (sink, events)
    }
}

impl AnalyticsSink for RecordingSink {
    fn record(&self, event: AnalyticsEvent) {
        self.events.lock().expect("poison").push(event);
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section A — ServiceRegistry::noop + Default
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn noop_registry_returns_noop_analytics_and_static_flags() {
    let r = ServiceRegistry::noop();
    // Verify the analytics sink doesn't panic on event recording.
    r.analytics().record(AnalyticsEvent::SessionStart {
        session_id: "test".to_string(),
    });
    // Verify the flag source returns false for any unknown flag.
    assert!(!r.flags().is_enabled("any-flag-name"));
}

#[test]
fn default_registry_matches_noop_registry_shape() {
    let d = ServiceRegistry::default();
    // Same behavioral contract: noop analytics + StaticFlags.
    d.analytics().record(AnalyticsEvent::SessionStart {
        session_id: "x".to_string(),
    });
    assert!(!d.flags().is_enabled("x"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — Builder methods (with_analytics + with_flags)
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn with_analytics_swaps_the_sink_routed_through_analytics_accessor() {
    let (sink, captured) = RecordingSink::new();
    let registry = ServiceRegistry::noop().with_analytics(sink);
    registry
        .analytics()
        .record(AnalyticsEvent::PromptSubmitted { prompt_chars: 42 });
    let events = captured.lock().expect("poison").clone();
    assert_eq!(events.len(), 1);
    assert!(matches!(
        events[0],
        AnalyticsEvent::PromptSubmitted { prompt_chars: 42 }
    ));
}

#[test]
fn with_flags_swaps_the_flag_source() {
    let flags = StaticFlags::default().with("test-flag", true);
    let registry = ServiceRegistry::noop().with_flags(Arc::new(flags));
    assert!(registry.flags().is_enabled("test-flag"));
    assert!(!registry.flags().is_enabled("other-flag"));
}

#[test]
fn builder_methods_chain_in_a_single_expression() {
    let (sink, captured) = RecordingSink::new();
    let flags = StaticFlags::default().with("x", true);
    let registry = ServiceRegistry::noop()
        .with_analytics(sink)
        .with_flags(Arc::new(flags));
    assert!(registry.flags().is_enabled("x"));
    registry
        .analytics()
        .record(AnalyticsEvent::ThinkingEmitted { budget: 1000 });
    assert_eq!(captured.lock().expect("poison").len(), 1);
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — analytics_arc + flags_arc shared-ownership accessors
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn analytics_arc_returns_clone_of_underlying_arc() {
    let registry = ServiceRegistry::noop();
    let a1 = registry.analytics_arc();
    let a2 = registry.analytics_arc();
    // Both Arcs point at the same sink — clone count went up.
    assert!(
        Arc::strong_count(&a1) >= 2,
        "analytics_arc MUST share ownership; got refcount {}",
        Arc::strong_count(&a1)
    );
    // Both can record independently.
    a1.record(AnalyticsEvent::SessionStart {
        session_id: "a".to_string(),
    });
    a2.record(AnalyticsEvent::SessionStart {
        session_id: "b".to_string(),
    });
}

#[test]
fn flags_arc_returns_clone_of_underlying_arc() {
    let registry = ServiceRegistry::noop();
    let f1 = registry.flags_arc();
    let f2 = registry.flags_arc();
    assert!(Arc::strong_count(&f1) >= 2);
    assert!(!f1.is_enabled("x"));
    assert!(!f2.is_enabled("x"));
}

#[test]
fn analytics_arc_outlives_the_registry_via_shared_ownership() {
    let arc = {
        let registry = ServiceRegistry::noop();
        registry.analytics_arc()
    };
    // Registry has been dropped, but the Arc lives on.
    arc.record(AnalyticsEvent::SessionEnd {
        session_id: "post-drop".to_string(),
        messages: 0,
    });
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — plugin_mcp_registrations (no plugins wired)
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn plugin_mcp_registrations_is_empty_when_no_plugins_wired() {
    let registry = ServiceRegistry::noop();
    let regs = registry.plugin_mcp_registrations();
    assert!(regs.is_empty());
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — ServiceRegistry Debug impl
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn registry_debug_format_does_not_panic() {
    let registry = ServiceRegistry::noop();
    let debug = format!("{registry:?}");
    // The Debug impl uses type metadata strings (no actual
    // Arc<dyn Trait> Debug); minimum contract is non-panic +
    // non-empty.
    assert!(!debug.is_empty());
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — MemoryConsolidationJob end-to-end
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn memory_consolidation_job_name_is_documented_stable_label() {
    let job = MemoryConsolidationJob;
    assert_eq!(job.name(), "memory_consolidation");
}

#[test]
fn memory_consolidation_on_empty_db_returns_zero_metrics() {
    let (db, _dir) = fresh_db();
    let job = MemoryConsolidationJob;
    let outcome = job.run(&db).expect("run OK");
    assert_eq!(outcome.job_name, "memory_consolidation");
    assert_eq!(outcome.records_pruned, 0);
    assert_eq!(outcome.records_deduped, 0);
}

#[test]
fn memory_consolidation_dedups_identical_archival_entries() {
    let (db, _dir) = fresh_db();
    // Insert 3 archival rows with identical content.
    let content = "duplicate-content";
    db.memory_save(content, &[]).expect("save 1");
    db.memory_save(content, &[]).expect("save 2");
    db.memory_save(content, &[]).expect("save 3");
    // Also one distinct entry — must NOT be deduped.
    db.memory_save("unique-content", &[]).expect("save 4");

    let job = MemoryConsolidationJob;
    let outcome = job.run(&db).expect("run OK");
    assert_eq!(
        outcome.records_deduped, 2,
        "3 identical rows → 2 deduped (1 canonical kept); got {}",
        outcome.records_deduped
    );
}

#[test]
fn memory_consolidation_is_idempotent_on_repeat_runs() {
    let (db, _dir) = fresh_db();
    db.memory_save("dup", &[]).expect("1");
    db.memory_save("dup", &[]).expect("2");
    let job = MemoryConsolidationJob;
    let first = job.run(&db).expect("first run");
    assert_eq!(first.records_deduped, 1);
    let second = job.run(&db).expect("second run");
    assert_eq!(
        second.records_deduped, 0,
        "post-dedup nothing left to dedup"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section G — PluginAutoupdateJob outcome shape
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn plugin_autoupdate_job_name_is_documented_stable_label() {
    let job = PluginAutoupdateJob::new(vec![]);
    assert_eq!(job.name(), "plugin_autoupdate");
}

#[test]
fn plugin_autoupdate_with_empty_plugin_list_returns_zero_metrics() {
    let (db, _dir) = fresh_db();
    let job = PluginAutoupdateJob::new(vec![]);
    let outcome = job.run(&db).expect("run");
    assert_eq!(outcome.job_name, "plugin_autoupdate");
    assert_eq!(outcome.records_pruned, 0);
    assert_eq!(outcome.records_deduped, 0);
}

#[test]
fn plugin_autoupdate_with_plugins_does_not_panic() {
    let (db, _dir) = fresh_db();
    let plugins = vec![
        ("plugin-a".to_string(), Some("1.0.0".to_string())),
        ("plugin-b".to_string(), None),
    ];
    let job = PluginAutoupdateJob::new(plugins);
    // Phase 1: emits per-plugin trace events; doesn't update
    // any state. MUST not panic.
    let outcome = job.run(&db).expect("run");
    assert_eq!(outcome.job_name, "plugin_autoupdate");
}

// ───────────────────────────────────────────────────────────────────────────
// Section H — JobOutcome shape + Eq
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn job_outcome_with_same_values_compares_equal() {
    use openclaudia::services::JobOutcome;
    let a = JobOutcome {
        job_name: "x",
        records_pruned: 5,
        records_deduped: 3,
    };
    let b = JobOutcome {
        job_name: "x",
        records_pruned: 5,
        records_deduped: 3,
    };
    assert_eq!(a, b);
}

#[test]
fn noop_analytics_struct_directly_constructable() {
    // Verify the NoopAnalytics tuple struct can be made
    // directly (it's pub).
    let noop = NoopAnalytics;
    let sink: &dyn AnalyticsSink = &noop;
    sink.record(AnalyticsEvent::ApiRequest {
        provider: "test".to_string(),
        model: "model".to_string(),
    });
}
