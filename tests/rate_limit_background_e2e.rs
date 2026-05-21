//! End-to-end tests for `MockRateLimit` state machine + the
//! `JobScheduler` tick / interval semantics.
//!
//! Sprint 46 of the verification effort.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::memory::MemoryDb;
use openclaudia::services::rate_limit_mock::MockResponse;
use openclaudia::services::{
    BackgroundJob, JobOutcome, JobScheduler, MockRateLimit, RateLimitMock,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

// ───────────────────────────────────────────────────────────────────────────
// Helpers
// ───────────────────────────────────────────────────────────────────────────

fn fresh_db() -> (Arc<MemoryDb>, TempDir) {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("memory.db");
    let db = Arc::new(MemoryDb::open(&path).expect("open"));
    (db, dir)
}

/// Test `BackgroundJob` that increments a shared counter on each
/// run and returns a customizable `JobOutcome`. Lets us pin tick
/// scheduling semantics without depending on production jobs'
/// side effects.
struct CountingJob {
    name: &'static str,
    run_count: Arc<AtomicUsize>,
    fail_on_call: Option<usize>,
}

impl CountingJob {
    fn new(name: &'static str) -> (Self, Arc<AtomicUsize>) {
        let counter = Arc::new(AtomicUsize::new(0));
        (
            Self {
                name,
                run_count: counter.clone(),
                fail_on_call: None,
            },
            counter,
        )
    }
}

impl BackgroundJob for CountingJob {
    fn name(&self) -> &'static str {
        self.name
    }

    fn run(&self, _db: &Arc<MemoryDb>) -> anyhow::Result<JobOutcome> {
        let n = self.run_count.fetch_add(1, Ordering::SeqCst) + 1;
        if Some(n) == self.fail_on_call {
            anyhow::bail!("intentional test failure on call {n}");
        }
        Ok(JobOutcome {
            job_name: self.name,
            records_pruned: n,
            records_deduped: 0,
        })
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section A — MockRateLimit dormant default
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn dormant_mock_returns_proceed_for_every_call() {
    let mock = MockRateLimit::dormant();
    for _ in 0..10 {
        assert_eq!(mock.next_response(), MockResponse::Proceed);
    }
}

#[test]
fn default_mock_equals_dormant() {
    let dormant = MockRateLimit::dormant();
    let default = MockRateLimit::default();
    // Both must return Proceed on first call.
    assert_eq!(dormant.next_response(), MockResponse::Proceed);
    assert_eq!(default.next_response(), MockResponse::Proceed);
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — throttle_next state machine
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn throttle_next_returns_throttle_then_proceeds() {
    let mock = MockRateLimit::dormant();
    mock.throttle_next(3, Duration::from_secs(5), "test throttle");

    // First 3 calls: Throttle with the configured retry_after + reason.
    for i in 0..3 {
        match mock.next_response() {
            MockResponse::Throttle {
                retry_after,
                reason,
            } => {
                assert_eq!(retry_after, Duration::from_secs(5), "call {i} retry_after");
                assert_eq!(reason, "test throttle", "call {i} reason");
            }
            MockResponse::Proceed => panic!("call {i} MUST be Throttle, got Proceed"),
        }
    }
    // 4th call onward: Proceed.
    assert_eq!(mock.next_response(), MockResponse::Proceed);
    assert_eq!(mock.next_response(), MockResponse::Proceed);
}

#[test]
fn throttle_next_with_zero_count_immediately_proceeds() {
    let mock = MockRateLimit::dormant();
    mock.throttle_next(0, Duration::from_secs(1), "no-op");
    assert_eq!(mock.next_response(), MockResponse::Proceed);
}

#[test]
fn throttle_next_carries_the_configured_reason_byte_exact() {
    let mock = MockRateLimit::dormant();
    let custom_reason = "rate-limited by upstream: try again in 30s (req-id: abc)";
    mock.throttle_next(1, Duration::from_secs(30), custom_reason);
    let MockResponse::Throttle { reason, .. } = mock.next_response() else {
        panic!("expected Throttle");
    };
    assert_eq!(reason, custom_reason);
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — kill switch + reset
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn set_enabled_false_suppresses_active_throttle() {
    let mock = MockRateLimit::dormant();
    mock.throttle_next(5, Duration::from_secs(1), "x");
    // Disable: next response MUST be Proceed despite remaining budget.
    mock.set_enabled(false);
    assert_eq!(mock.next_response(), MockResponse::Proceed);
}

#[test]
fn set_enabled_true_resumes_remaining_throttle_budget() {
    let mock = MockRateLimit::dormant();
    mock.throttle_next(2, Duration::from_secs(1), "x");
    mock.set_enabled(false);
    assert_eq!(mock.next_response(), MockResponse::Proceed);
    // Re-enable: budget MUST still be 2 (not consumed by suppressed calls).
    mock.set_enabled(true);
    assert!(matches!(
        mock.next_response(),
        MockResponse::Throttle { .. }
    ));
    assert!(matches!(
        mock.next_response(),
        MockResponse::Throttle { .. }
    ));
    assert_eq!(mock.next_response(), MockResponse::Proceed);
}

#[test]
fn reset_returns_mock_to_dormant_state() {
    let mock = MockRateLimit::dormant();
    mock.throttle_next(5, Duration::from_secs(1), "x");
    mock.record_call();
    mock.record_call();
    assert_eq!(mock.calls_recorded(), 2);
    mock.reset();
    // After reset: Proceed + counter zeroed.
    assert_eq!(mock.next_response(), MockResponse::Proceed);
    assert_eq!(mock.calls_recorded(), 0);
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — record_call counter
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn record_call_increments_counter_independently_of_throttle_state() {
    let mock = MockRateLimit::dormant();
    assert_eq!(mock.calls_recorded(), 0);
    for i in 1..=5 {
        mock.record_call();
        assert_eq!(mock.calls_recorded(), i);
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — JobScheduler tick semantics
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn scheduler_first_tick_runs_every_registered_job() {
    let (db, _tmp) = fresh_db();
    let mut sched = JobScheduler::new(db);
    let (job_a, counter_a) = CountingJob::new("job_a");
    let (job_b, counter_b) = CountingJob::new("job_b");
    sched.register(Arc::new(job_a), Duration::from_millis(1));
    sched.register(Arc::new(job_b), Duration::from_millis(1));

    let outcomes = sched.tick();
    assert_eq!(outcomes.len(), 2, "first tick MUST run both jobs");
    assert_eq!(counter_a.load(Ordering::SeqCst), 1);
    assert_eq!(counter_b.load(Ordering::SeqCst), 1);
}

#[test]
fn scheduler_second_tick_within_interval_does_not_re_run_jobs() {
    let (db, _tmp) = fresh_db();
    let mut sched = JobScheduler::new(db);
    let (job, counter) = CountingJob::new("job");
    // Long interval — second tick is well within it.
    sched.register(Arc::new(job), Duration::from_hours(1));

    let first = sched.tick();
    assert_eq!(first.len(), 1);
    assert_eq!(counter.load(Ordering::SeqCst), 1);

    // Immediate second tick — under interval, MUST NOT re-run.
    let second = sched.tick();
    assert!(
        second.is_empty(),
        "second tick within interval MUST NOT re-run; got outcomes={second:?}"
    );
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "counter MUST NOT advance"
    );
}

#[test]
fn scheduler_tick_after_interval_re_runs_due_jobs() {
    let (db, _tmp) = fresh_db();
    let mut sched = JobScheduler::new(db);
    let (job, counter) = CountingJob::new("job");
    // Very short interval — second tick after sleep is due.
    sched.register(Arc::new(job), Duration::from_millis(1));

    let _ = sched.tick();
    assert_eq!(counter.load(Ordering::SeqCst), 1);
    std::thread::sleep(Duration::from_millis(50));
    let second = sched.tick();
    assert_eq!(second.len(), 1, "tick after interval MUST re-run");
    assert_eq!(counter.load(Ordering::SeqCst), 2);
}

#[test]
fn scheduler_with_no_jobs_returns_empty_outcomes() {
    let (db, _tmp) = fresh_db();
    let mut sched = JobScheduler::new(db);
    let outcomes = sched.tick();
    assert!(outcomes.is_empty(), "no-jobs tick MUST yield empty Vec");
}

#[test]
fn scheduler_failing_job_does_not_crash_others() {
    let (db, _tmp) = fresh_db();
    let mut sched = JobScheduler::new(db);

    let (good_job, good_counter) = CountingJob::new("good");
    let (mut bad_job, bad_counter) = CountingJob::new("bad");
    // Make `bad_job` fail on its first call.
    bad_job.fail_on_call = Some(1);

    sched.register(Arc::new(bad_job), Duration::from_millis(1));
    sched.register(Arc::new(good_job), Duration::from_millis(1));

    let outcomes = sched.tick();
    // Only the good job's outcome appears.
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].job_name, "good");
    assert_eq!(good_counter.load(Ordering::SeqCst), 1);
    // The bad job DID try (counter advanced) — even though
    // it errored, the scheduler did not panic.
    assert_eq!(bad_counter.load(Ordering::SeqCst), 1);
}

#[test]
fn scheduler_outcomes_preserve_job_name_and_metrics() {
    let (db, _tmp) = fresh_db();
    let mut sched = JobScheduler::new(db);
    let (job, _) = CountingJob::new("metric-test");
    sched.register(Arc::new(job), Duration::from_millis(1));

    let outcomes = sched.tick();
    assert_eq!(outcomes.len(), 1);
    let outcome = &outcomes[0];
    assert_eq!(outcome.job_name, "metric-test");
    // CountingJob returns records_pruned = call number, 1 on
    // first run.
    assert_eq!(outcome.records_pruned, 1);
    assert_eq!(outcome.records_deduped, 0);
}

#[test]
fn scheduler_runs_jobs_in_registration_order() {
    let (db, _tmp) = fresh_db();
    let mut sched = JobScheduler::new(db);
    let (job_1, _) = CountingJob::new("first");
    let (job_2, _) = CountingJob::new("second");
    let (job_3, _) = CountingJob::new("third");
    sched.register(Arc::new(job_1), Duration::from_millis(1));
    sched.register(Arc::new(job_2), Duration::from_millis(1));
    sched.register(Arc::new(job_3), Duration::from_millis(1));

    let outcomes = sched.tick();
    assert_eq!(outcomes.len(), 3);
    assert_eq!(outcomes[0].job_name, "first");
    assert_eq!(outcomes[1].job_name, "second");
    assert_eq!(outcomes[2].job_name, "third");
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — independent intervals per job
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn scheduler_short_interval_re_runs_while_long_interval_waits() {
    let (db, _tmp) = fresh_db();
    let mut sched = JobScheduler::new(db);
    let (fast_job, fast_counter) = CountingJob::new("fast");
    let (slow_job, slow_counter) = CountingJob::new("slow");
    sched.register(Arc::new(fast_job), Duration::from_millis(1));
    sched.register(Arc::new(slow_job), Duration::from_hours(1));

    // First tick: both run.
    let _ = sched.tick();
    assert_eq!(fast_counter.load(Ordering::SeqCst), 1);
    assert_eq!(slow_counter.load(Ordering::SeqCst), 1);
    std::thread::sleep(Duration::from_millis(50));
    // Second tick: only fast runs.
    let outcomes = sched.tick();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].job_name, "fast");
    assert_eq!(fast_counter.load(Ordering::SeqCst), 2);
    assert_eq!(
        slow_counter.load(Ordering::SeqCst),
        1,
        "slow job MUST NOT re-run within its long interval"
    );
}
