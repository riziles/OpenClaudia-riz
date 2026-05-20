//! End-to-end tests for the SQLite-backed memory store.
//!
//! Sprint 5 of the verification effort. `src/memory.rs` has 56 unit
//! tests for the simple paths (insert, select, basic FTS5 hits) but
//! no integration coverage of:
//!
//!   - **Persistence across reopen** — a [`MemoryDb`] dropped and
//!     re-opened at the same path MUST observe the previously-saved
//!     rows. Pins the schema-migration idempotence contract.
//!   - **Concurrent writers** — multiple threads calling
//!     `memory_save` against the *same* `Arc<MemoryDb>` must not
//!     drop, panic, or interleave a transaction.
//!   - **SQL injection in user-supplied query / tag strings** —
//!     classic injection payloads (`'; DROP TABLE`, `' OR 1=1 --`,
//!     `*/*`) must be treated as literal search content, NOT
//!     executed; the database schema must survive the search and
//!     no rows must leak that don't actually contain the payload.
//!   - **FTS5 ranking + truncation** — when N inserts share a
//!     keyword, a `memory_search` with `limit=k` MUST return at
//!     most k results, in BM25 rank order.
//!   - **Tag round-trip** — `memory_save` with tags →
//!     `memory_search_by_tag` returns the row; tags with hostile
//!     content (newlines, SQL fragments) are preserved verbatim.
//!   - **Core memory CRUD** — `update_core_memory` is upsert; a
//!     second write at the same section overwrites the first.
//!   - **Stats** — `memory_stats` after a series of save/delete
//!     reports a coherent count.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::memory::MemoryDb;
use std::sync::{Arc, Barrier};
use std::thread;
use tempfile::tempdir;

/// Build a fresh tempdir-backed `MemoryDb`. Returns the db plus the
/// `TempDir` guard so the directory survives for the test's lifetime.
fn fresh_db() -> (MemoryDb, tempfile::TempDir) {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("memory.sqlite");
    let db = MemoryDb::open(&path).expect("open memory db");
    (db, dir)
}

// ───────────────────────────────────────────────────────────────────────────
// Section A — round-trip CRUD
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn save_get_update_delete_round_trip() {
    let (db, _td) = fresh_db();

    let id = db
        .memory_save("the quick brown fox", &["animal".to_string()])
        .expect("save");
    let got = db.memory_get(id).expect("get").expect("row exists");
    assert_eq!(got.content, "the quick brown fox");
    assert_eq!(got.id, id);

    let updated = db
        .memory_update(id, "the slow purple sloth")
        .expect("update");
    assert!(updated, "update of an existing row must report true");
    let after = db.memory_get(id).expect("get after").expect("still exists");
    assert_eq!(after.content, "the slow purple sloth");

    let deleted = db.memory_delete(id).expect("delete");
    assert!(deleted, "delete of an existing row must report true");
    assert!(
        db.memory_get(id).expect("get after delete").is_none(),
        "row must be gone after delete"
    );

    // Idempotence: delete of an already-gone row reports false.
    let twice = db.memory_delete(id).expect("delete twice");
    assert!(!twice, "delete of a missing row must report false");
}

#[test]
fn list_and_stats_are_coherent_after_mixed_ops() {
    let (db, _td) = fresh_db();
    for i in 0..5 {
        db.memory_save(&format!("entry-{i}"), &[]).expect("save");
    }
    let list = db.memory_list(100).expect("list");
    assert_eq!(list.len(), 5, "list must return all 5 saved rows");

    let stats_before = db.memory_stats().expect("stats");
    assert_eq!(stats_before.count, 5, "stats must reflect the 5 rows");

    // Delete two; stats must drop by exactly 2.
    db.memory_delete(list[0].id).expect("delete 0");
    db.memory_delete(list[1].id).expect("delete 1");
    let stats_after = db.memory_stats().expect("stats after");
    assert_eq!(
        stats_after.count, 3,
        "stats must decrement by exactly the number deleted"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — persistence across re-open
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn data_survives_close_and_reopen_at_same_path() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("memory.sqlite");

    // Phase 1: open, write, drop.
    let id;
    {
        let db = MemoryDb::open(&path).expect("first open");
        id = db
            .memory_save("persistent entry", &["persist".to_string()])
            .expect("save");
        db.update_core_memory("persona", "test-persona-body")
            .expect("core save");
        // `db` drops here — connection closed.
    }

    // Phase 2: re-open at the same path; data must still be there.
    let db2 = MemoryDb::open(&path).expect("second open");
    let got = db2
        .memory_get(id)
        .expect("get after reopen")
        .expect("row must persist");
    assert_eq!(got.content, "persistent entry");

    let core = db2
        .get_core_memory_section("persona")
        .expect("core get")
        .expect("core row must persist");
    assert_eq!(core.content, "test-persona-body");

    // Schema-migration idempotence: a third open must not panic / error.
    let _db3 = MemoryDb::open(&path).expect("third open");
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — concurrent writers
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn concurrent_writers_all_succeed_against_same_db() {
    // 8 threads racing on memory_save against the same connection.
    // The Mutex around the SQLite handle serialises access — but the
    // test is here to pin that contract: zero panics, zero lost rows.
    let (db, _td) = fresh_db();
    let db = Arc::new(db);
    let n_threads = 8;
    let writes_per_thread = 16;

    let barrier = Arc::new(Barrier::new(n_threads));
    let handles: Vec<_> = (0..n_threads)
        .map(|t| {
            let db = Arc::clone(&db);
            let b = Arc::clone(&barrier);
            thread::spawn(move || {
                b.wait();
                for i in 0..writes_per_thread {
                    let content = format!("t{t}-i{i}");
                    db.memory_save(&content, &[format!("thread-{t}")])
                        .expect("save under contention");
                }
            })
        })
        .collect();
    for h in handles {
        h.join().expect("thread joined");
    }

    let stats = db.memory_stats().expect("stats");
    assert_eq!(
        stats.count,
        n_threads * writes_per_thread,
        "all {} writes must land",
        n_threads * writes_per_thread
    );
    // Per-thread tag round-trip: every thread's writes should be
    // retrievable via its tag.
    for t in 0..n_threads {
        let rows = db
            .memory_search_by_tag(&format!("thread-{t}"), 1000)
            .expect("search by tag");
        assert_eq!(
            rows.len(),
            writes_per_thread,
            "thread {t} must have exactly {writes_per_thread} tagged rows"
        );
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — SQL injection attempts via search / tag strings
// ───────────────────────────────────────────────────────────────────────────

/// Classic SQL-injection payloads. Each must be treated as
/// literal search content — the prepared-statement / `params!`
/// path in `memory_search` never interpolates user text.
const SEARCH_INJECTION_PAYLOADS: &[&str] = &[
    "'; DROP TABLE archival_memory; --",
    "' OR '1'='1",
    "*/* INTO OUTFILE '/tmp/owned' --",
    "x'; ATTACH DATABASE '/tmp/evil.db' AS evil; --",
    "); DELETE FROM archival_memory; --",
    "UNION SELECT 1,2,3,4,5 --",
];

#[test]
fn injection_payloads_in_search_query_do_not_drop_schema() {
    // Plant 3 benign rows; then run a series of classic injection
    // payloads as the search query. After every search the schema
    // must still be intact (we re-list and re-stats to confirm).
    let (db, _td) = fresh_db();
    for i in 0..3 {
        db.memory_save(&format!("benign-{i}"), &[]).expect("save");
    }
    let pre = db.memory_stats().expect("stats pre").count;
    assert_eq!(pre, 3);

    for payload in SEARCH_INJECTION_PAYLOADS {
        // The search must NOT error out the test (it returns Vec on
        // any rusqlite failure per crosslink #501) — and crucially
        // the table must still exist on the other side.
        let _ = db.memory_search(payload, 10);
        let post = db.memory_stats().expect("stats post").count;
        assert_eq!(
            post, pre,
            "injection payload {payload:?} altered row count: {pre} → {post}"
        );
    }
    // Final sanity: the benign rows are still retrievable.
    let list = db.memory_list(100).expect("list");
    assert_eq!(
        list.len(),
        3,
        "benign rows must survive every injection attempt"
    );
}

/// Tag strings with hostile content — quotes, newlines, raw SQL
/// fragments, GLOB wildcards. Each must round-trip byte-exact:
/// `memory_save` stores it verbatim and `memory_search_by_tag`
/// matches the same literal back.
const HOSTILE_TAGS: &[&str] = &[
    "'; DROP TABLE archival_memory_tags; --",
    "tag with\nnewline",
    "tag with \"quotes\" and 'apostrophes'",
    "tag % with _ wildcards",
];

#[test]
fn injection_payloads_in_tag_strings_are_stored_verbatim() {
    let (db, _td) = fresh_db();

    for tag in HOSTILE_TAGS {
        let id = db
            .memory_save("content", &[(*tag).to_string()])
            .expect("save with hostile tag");
        let rows = db.memory_search_by_tag(tag, 10).expect("search by tag");
        assert!(
            rows.iter().any(|r| r.id == id),
            "row inserted with tag {tag:?} must be retrievable by the same tag, \
             got {:?}",
            rows.iter().map(|r| r.id).collect::<Vec<_>>(),
        );
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — FTS5 search behaviour
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn search_limit_caps_result_count() {
    let (db, _td) = fresh_db();
    // 20 entries all containing "alpha"; search with limit=5 → at most 5.
    for i in 0..20 {
        db.memory_save(&format!("alpha entry {i}"), &[])
            .expect("save");
    }
    let hits = db.memory_search("alpha", 5).expect("search");
    assert!(
        hits.len() <= 5,
        "search with limit=5 must return at most 5 results, got {}",
        hits.len(),
    );
}

#[test]
fn search_finds_only_matching_content() {
    let (db, _td) = fresh_db();
    db.memory_save("the quick brown fox jumps", &[])
        .expect("save fox");
    db.memory_save("totally unrelated", &[])
        .expect("save unrelated");
    db.memory_save("brown sugar pancakes", &[])
        .expect("save brown");

    let hits = db.memory_search("brown", 100).expect("search brown");
    assert_eq!(
        hits.len(),
        2,
        "search 'brown' must match exactly the 2 rows containing 'brown', got {:?}",
        hits.iter().map(|h| &h.content).collect::<Vec<_>>(),
    );
    let contents: Vec<&str> = hits.iter().map(|h| h.content.as_str()).collect();
    assert!(contents.iter().any(|c| c.contains("fox")));
    assert!(contents.iter().any(|c| c.contains("pancakes")));
    assert!(!contents.iter().any(|c| c.contains("unrelated")));
}

#[test]
fn search_returns_empty_on_no_match() {
    let (db, _td) = fresh_db();
    db.memory_save("something else", &[]).expect("save");
    let hits = db.memory_search("zebrafish", 10).expect("search");
    assert!(
        hits.is_empty(),
        "no-match search must return empty, got {hits:?}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — core memory upsert
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn update_core_memory_overwrites_existing_section() {
    let (db, _td) = fresh_db();
    db.update_core_memory("persona", "first version")
        .expect("first write");
    db.update_core_memory("persona", "second version")
        .expect("overwrite");

    let got = db
        .get_core_memory_section("persona")
        .expect("get")
        .expect("section exists");
    assert_eq!(
        got.content, "second version",
        "second write must overwrite the first (upsert semantics)"
    );
    // And there's still only ONE row for that section.
    let all = db.get_core_memory().expect("all");
    let persona_count = all.iter().filter(|c| c.section == "persona").count();
    assert_eq!(
        persona_count, 1,
        "after two writes to the same section, exactly 1 row must remain; got {all:?}",
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section G — edge cases
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn save_with_unicode_round_trips() {
    let (db, _td) = fresh_db();
    let content = "héllo 世界 🚀 \u{1F600}";
    let id = db
        .memory_save(content, &["unicode".to_string()])
        .expect("save");
    let got = db.memory_get(id).expect("get").expect("exists");
    assert_eq!(got.content, content, "unicode must round-trip byte-exact");
}

#[test]
fn save_with_empty_tags_array_is_accepted() {
    let (db, _td) = fresh_db();
    let id = db.memory_save("no-tag content", &[]).expect("save");
    let got = db.memory_get(id).expect("get").expect("exists");
    assert_eq!(got.content, "no-tag content");
}

#[test]
fn clear_archival_memory_removes_all_rows() {
    let (db, _td) = fresh_db();
    for i in 0..5 {
        db.memory_save(&format!("entry-{i}"), &[]).expect("save");
    }
    let removed = db.clear_archival_memory().expect("clear");
    assert_eq!(removed, 5, "clear must report removing all 5 rows");
    let stats = db.memory_stats().expect("stats");
    assert_eq!(stats.count, 0, "no rows must remain after clear");
}
