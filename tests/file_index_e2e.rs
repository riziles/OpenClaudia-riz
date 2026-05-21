//! End-to-end tests for `FileIndex` walk + search behaviour against
//! a real on-disk tempdir tree.
//!
//! Sprint 16 of the verification effort. `src/tools/file_index.rs`
//! has 8 unit tests but no integration coverage that drives the
//! walker against the adversarial filesystem topologies it has to
//! survive:
//!
//!   - **Symlink cycles** (crosslink #920) — `a/b → a` MUST not
//!     stack-overflow or infinite-loop. The walker uses a canonical-
//!     path visited set + an iterative `VecDeque` to enforce
//!     termination.
//!   - **Pathologically deep trees** — depth > `MAX_WALK_DEPTH`
//!     (64) MUST not panic; deeper files are simply excluded.
//!   - **Skip-list** — `.git`, `node_modules`, `target`,
//!     `__pycache__`, `dist`, `build`, and any hidden dir
//!     (`.foo`) all skipped.
//!   - **Search scoring** — fuzzy subsequence match, no-match
//!     returns empty, first-char bonus + path-boundary bonus,
//!     case-insensitivity, limit truncation.
//!   - **Edge cases** — empty query yields empty; non-existent
//!     root yields empty index; unicode filenames round-trip.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::tools::file_index::FileIndex;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

// ───────────────────────────────────────────────────────────────────────────
// Helpers
// ───────────────────────────────────────────────────────────────────────────

fn touch(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("mkdir parent");
    }
    fs::write(path, b"").expect("touch");
}

// ───────────────────────────────────────────────────────────────────────────
// Section A — walker basics
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn build_indexes_files_relative_to_root() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path();
    touch(&root.join("a.rs"));
    touch(&root.join("src/main.rs"));
    touch(&root.join("src/lib.rs"));
    touch(&root.join("docs/README.md"));

    let index = FileIndex::build(root);
    // Search for "rs" must find at least the three .rs files.
    let hits = index.search("rs", 100);
    let paths: Vec<&str> = hits.iter().map(|h| h.path.as_str()).collect();
    for expected in &["a.rs", "src/main.rs", "src/lib.rs"] {
        // Path separators may be platform-specific on Windows, but
        // we're on linux per the harness.
        assert!(
            paths
                .iter()
                .any(|p| p.ends_with(*expected) || p == expected),
            "search 'rs' missing expected file {expected:?}; got {paths:?}"
        );
    }
}

#[test]
fn build_skips_documented_ignore_dirs() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path();
    touch(&root.join("kept.rs"));
    // Every one of these must be silently skipped.
    for ignored in &[
        ".git",
        "node_modules",
        "target",
        "__pycache__",
        "dist",
        "build",
    ] {
        touch(&root.join(format!("{ignored}/secret.rs")));
    }
    // A hidden dir starting with `.` must also be skipped.
    touch(&root.join(".hidden/secret.rs"));

    let index = FileIndex::build(root);
    let hits = index.search("secret", 100);
    assert!(
        hits.is_empty(),
        "files under ignored/hidden dirs MUST NOT be indexed; got {hits:?}"
    );
    // But the legitimately-placed file must still be visible.
    let visible = index.search("kept", 10);
    assert!(!visible.is_empty(), "non-ignored file must be indexed");
}

#[cfg(unix)]
#[test]
fn walker_terminates_on_symlink_cycle() {
    // crosslink #920: a self-referential symlink `inner/loop → ../`
    // would have stack-overflowed the recursive walker. Build the
    // cycle and assert the walker terminates AND indexes the real
    // files at the canonical paths.
    let dir = tempdir().expect("tempdir");
    let root = dir.path();
    touch(&root.join("real.rs"));
    touch(&root.join("inner/nested.rs"));
    std::os::unix::fs::symlink(root, root.join("inner/loop")).expect("symlink cycle");

    // If this hangs / OOMs / stack-overflows, the test runner kills
    // it via the harness timeout. Successfully returning IS the pass.
    let index = FileIndex::build(root);
    let nested = index.search("nested", 10);
    assert!(
        !nested.is_empty(),
        "indexed files inside the cycled dir must still be searchable"
    );
    let real = index.search("real", 10);
    assert!(!real.is_empty(), "root-level files must still be indexed");
}

#[cfg(unix)]
#[test]
fn walker_survives_pathologically_deep_tree() {
    // crosslink #920: MAX_WALK_DEPTH is 64. Build a 200-deep chain
    // and assert the walker doesn't panic; deeper files just don't
    // make it into the index (which is the documented contract).
    let dir = tempdir().expect("tempdir");
    let root = dir.path().to_path_buf();
    let mut current = root.clone();
    for _ in 0..200 {
        current = current.join("d");
    }
    fs::create_dir_all(&current).expect("deep mkdir");
    fs::write(current.join("deep.rs"), b"").expect("deep touch");
    // Also plant a shallow file so we can verify SOMETHING was indexed.
    fs::write(root.join("shallow.rs"), b"").expect("shallow touch");

    let index = FileIndex::build(&root);
    let shallow = index.search("shallow", 10);
    assert!(
        !shallow.is_empty(),
        "shallow file at depth 0 must be indexed even with deep sibling tree"
    );
    // We don't assert anything about `deep.rs` — beyond
    // MAX_WALK_DEPTH it's allowed to be excluded.
}

#[test]
fn build_on_nonexistent_root_returns_empty_index() {
    let dir = tempdir().expect("tempdir");
    let nope = dir.path().join("never-existed");
    let index = FileIndex::build(&nope);
    // No files; any search returns empty.
    let hits = index.search("anything", 10);
    assert!(
        hits.is_empty(),
        "search against empty index must return no hits; got {hits:?}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — search scoring
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn search_empty_query_returns_empty_results() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path();
    touch(&root.join("a.rs"));
    touch(&root.join("b.rs"));
    let index = FileIndex::build(root);
    let hits = index.search("", 100);
    assert!(
        hits.is_empty(),
        "empty query must yield empty results; got {hits:?}"
    );
}

#[test]
fn search_matches_subsequence_not_just_substring() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path();
    touch(&root.join("src/foo_bar.rs"));
    let index = FileIndex::build(root);
    // "fbr" matches subsequence f...b...r in foo_bar.
    let hits = index.search("fbr", 10);
    assert!(
        !hits.is_empty(),
        "subsequence query 'fbr' must match 'foo_bar.rs'; got {hits:?}"
    );
}

#[test]
fn search_returns_no_hits_for_unrelated_query() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path();
    touch(&root.join("alpha.rs"));
    let index = FileIndex::build(root);
    let hits = index.search("xyz", 10);
    assert!(
        hits.is_empty(),
        "no-match query must return empty; got {hits:?}"
    );
}

#[test]
fn search_is_case_insensitive() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path();
    touch(&root.join("CamelCase.rs"));
    let index = FileIndex::build(root);
    let lower = index.search("camelcase", 10);
    let upper = index.search("CAMELCASE", 10);
    let mixed = index.search("CaMeL", 10);
    assert!(!lower.is_empty(), "lowercase must match");
    assert!(!upper.is_empty(), "uppercase must match");
    assert!(!mixed.is_empty(), "mixed-case must match");
}

#[test]
fn search_limit_caps_result_count() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path();
    for i in 0..20 {
        touch(&root.join(format!("alpha_{i}.rs")));
    }
    let index = FileIndex::build(root);
    let hits = index.search("alpha", 5);
    assert!(
        hits.len() <= 5,
        "limit=5 must cap result count; got {} hits",
        hits.len()
    );
}

#[test]
fn search_results_are_sorted_by_descending_score() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path();
    // Both files contain "tool" but the prefix match should score
    // higher than the buried match.
    touch(&root.join("tool.rs"));
    touch(&root.join("src/internal/has_a_tool_buried.rs"));
    let index = FileIndex::build(root);
    let hits = index.search("tool", 10);
    assert!(hits.len() >= 2, "must find both files");
    // Scores monotonically non-increasing.
    for win in hits.windows(2) {
        assert!(
            win[0].score >= win[1].score,
            "results must be sorted by descending score; got {:?}",
            hits.iter()
                .map(|h| (h.path.as_str(), h.score))
                .collect::<Vec<_>>()
        );
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — unicode + edge cases
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn unicode_filenames_round_trip_through_index() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path();
    touch(&root.join("世界.rs"));
    touch(&root.join("café.md"));
    let index = FileIndex::build(root);
    // Search by ASCII substring should still find the multi-byte
    // sibling via the path containing "rs"/"md".
    let rs_hits = index.search("rs", 10);
    assert!(
        rs_hits.iter().any(|h| h.path.contains("世界")),
        "unicode filename must be searchable via its ASCII suffix; got {rs_hits:?}"
    );
}

#[test]
fn query_longer_than_any_path_yields_no_hits() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path();
    touch(&root.join("x.rs"));
    let index = FileIndex::build(root);
    let hits = index.search("a-very-long-query-that-could-not-possibly-match-x.rs", 10);
    assert!(
        hits.is_empty(),
        "query longer than path must yield no hits; got {hits:?}"
    );
}
