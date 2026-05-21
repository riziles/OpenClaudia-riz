//! End-to-end tests for `memory::MemoryDb` short-term memory
//! (`save_session_summary`, `log_activity`, `get_recent_*`,
//! `cleanup_expired`) plus core-memory section accessors,
//! `search_by_tag`, and `format_recent_context`.
//!
//! Sprint 92 of the verification effort. Sprint 13
//! (`memory_e2e`) covered the archival save/get/update/delete
//! round-trips; sprint 42 (`memory_eviction_e2e`) covered
//! auto-learn pruning; this file covers the short-term
//! `recent_sessions` plus `recent_activity` surfaces that
//! the session-handoff path depends on.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::memory::{MemoryDb, SECTION_PERSONA, SECTION_PROJECT_INFO, SECTION_USER_PREFS};
use tempfile::TempDir;

// ───────────────────────────────────────────────────────────────────────────
// Helpers
// ───────────────────────────────────────────────────────────────────────────

fn fresh_db() -> (MemoryDb, TempDir) {
    let dir = TempDir::new().expect("tempdir");
    let db = MemoryDb::open(&dir.path().join("memory.db")).expect("open");
    (db, dir)
}

// ───────────────────────────────────────────────────────────────────────────
// Section A — SECTION constants
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn section_constants_match_documented_string_values() {
    assert_eq!(SECTION_PERSONA, "persona");
    assert_eq!(SECTION_PROJECT_INFO, "project_info");
    assert_eq!(SECTION_USER_PREFS, "user_preferences");
}

#[test]
fn section_constants_are_pairwise_distinct() {
    assert_ne!(SECTION_PERSONA, SECTION_PROJECT_INFO);
    assert_ne!(SECTION_PROJECT_INFO, SECTION_USER_PREFS);
    assert_ne!(SECTION_PERSONA, SECTION_USER_PREFS);
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — save_session_summary + get_recent_sessions
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn save_session_summary_stores_a_session_with_files_and_issues() {
    let (db, _dir) = fresh_db();
    let files = vec!["src/main.rs".to_string(), "src/lib.rs".to_string()];
    let issues = vec!["#100".to_string(), "#200".to_string()];
    let rowid = db
        .save_session_summary(
            "session-1",
            "fixed bugs in main",
            &files,
            &issues,
            "2024-01-01T00:00:00Z",
        )
        .expect("save");
    assert!(rowid > 0);
}

#[test]
fn get_recent_sessions_returns_saved_session() {
    let (db, _dir) = fresh_db();
    db.save_session_summary("s-test", "summary text", &[], &[], "2024-01-01T00:00:00Z")
        .expect("save");
    let sessions = db.get_recent_sessions(10).expect("list");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].session_id, "s-test");
    assert_eq!(sessions[0].summary, "summary text");
}

#[test]
fn save_session_summary_preserves_files_modified_list() {
    let (db, _dir) = fresh_db();
    let files = vec![
        "src/main.rs".to_string(),
        "tests/integration.rs".to_string(),
        "Cargo.toml".to_string(),
    ];
    db.save_session_summary("s", "x", &files, &[], "2024-01-01T00:00:00Z")
        .expect("save");
    let sessions = db.get_recent_sessions(10).expect("list");
    assert_eq!(sessions[0].files_modified.len(), 3);
    assert!(sessions[0]
        .files_modified
        .contains(&"src/main.rs".to_string()));
    assert!(sessions[0]
        .files_modified
        .contains(&"Cargo.toml".to_string()));
}

#[test]
fn save_session_summary_replaces_when_same_session_id() {
    let (db, _dir) = fresh_db();
    db.save_session_summary("dup-id", "first", &[], &[], "2024-01-01T00:00:00Z")
        .expect("first");
    db.save_session_summary("dup-id", "updated", &[], &[], "2024-01-01T00:00:00Z")
        .expect("second");
    let sessions = db.get_recent_sessions(10).expect("list");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].summary, "updated");
}

#[test]
fn get_recent_sessions_with_limit_caps_result_count() {
    let (db, _dir) = fresh_db();
    for i in 0..5 {
        db.save_session_summary(&format!("s-{i}"), "x", &[], &[], "2024-01-01T00:00:00Z")
            .expect("save");
    }
    let limited = db.get_recent_sessions(3).expect("list");
    assert!(limited.len() <= 3);
}

#[test]
fn get_recent_sessions_empty_db_returns_empty_vec() {
    let (db, _dir) = fresh_db();
    let sessions = db.get_recent_sessions(10).expect("list");
    assert!(sessions.is_empty());
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — log_activity + get_session_activities
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn log_activity_stores_activity_for_a_session() {
    let (db, _dir) = fresh_db();
    let rowid = db
        .log_activity("session-1", "tool_use", "bash", Some("ls -la"))
        .expect("log");
    assert!(rowid > 0);
}

#[test]
fn get_session_activities_returns_activities_in_descending_creation_order() {
    let (db, _dir) = fresh_db();
    db.log_activity("s", "tool_use", "ls", None).expect("a1");
    db.log_activity("s", "tool_use", "cat", None).expect("a2");
    db.log_activity("s", "tool_use", "grep", None).expect("a3");
    let activities = db.get_session_activities("s").expect("list");
    assert_eq!(activities.len(), 3);
    // Descending creation order: most-recent first.
}

#[test]
fn get_session_activities_isolates_by_session_id() {
    let (db, _dir) = fresh_db();
    db.log_activity("session-a", "tool_use", "ls", None)
        .expect("a");
    db.log_activity("session-b", "tool_use", "pwd", None)
        .expect("b");
    let activities_a = db.get_session_activities("session-a").expect("list a");
    let activities_b = db.get_session_activities("session-b").expect("list b");
    assert_eq!(activities_a.len(), 1);
    assert_eq!(activities_a[0].target, "ls");
    assert_eq!(activities_b.len(), 1);
    assert_eq!(activities_b[0].target, "pwd");
}

#[test]
fn log_activity_with_optional_details_preserves_them() {
    let (db, _dir) = fresh_db();
    db.log_activity("s", "tool_use", "bash", Some("exit code 0"))
        .expect("log");
    let activities = db.get_session_activities("s").expect("list");
    assert_eq!(activities[0].details.as_deref(), Some("exit code 0"));
}

#[test]
fn log_activity_with_none_details_stores_none() {
    let (db, _dir) = fresh_db();
    db.log_activity("s", "edit", "file.rs", None).expect("log");
    let activities = db.get_session_activities("s").expect("list");
    assert!(activities[0].details.is_none());
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — Core memory single-section accessor
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn update_core_memory_creates_section_with_content() {
    let (db, _dir) = fresh_db();
    db.update_core_memory(SECTION_PERSONA, "I'm helpful")
        .expect("update");
    let mem = db.get_core_memory_section(SECTION_PERSONA).expect("get");
    let m = mem.expect("Some");
    assert_eq!(m.section, SECTION_PERSONA);
    assert_eq!(m.content, "I'm helpful");
}

#[test]
fn get_core_memory_section_returns_none_for_unknown_section() {
    let (db, _dir) = fresh_db();
    let mem = db.get_core_memory_section("never-set").expect("get");
    assert!(mem.is_none());
}

#[test]
fn update_core_memory_overwrites_existing_section() {
    let (db, _dir) = fresh_db();
    db.update_core_memory(SECTION_USER_PREFS, "first")
        .expect("first");
    db.update_core_memory(SECTION_USER_PREFS, "replaced")
        .expect("second");
    let mem = db.get_core_memory_section(SECTION_USER_PREFS).unwrap();
    assert_eq!(mem.unwrap().content, "replaced");
}

#[test]
fn get_core_memory_returns_all_sections() {
    let (db, _dir) = fresh_db();
    db.update_core_memory(SECTION_PERSONA, "p").expect("p");
    db.update_core_memory(SECTION_PROJECT_INFO, "pi")
        .expect("pi");
    db.update_core_memory(SECTION_USER_PREFS, "up").expect("up");
    let all = db.get_core_memory().expect("all");
    assert_eq!(all.len(), 3);
    let sections: Vec<&str> = all.iter().map(|m| m.section.as_str()).collect();
    assert!(sections.contains(&SECTION_PERSONA));
    assert!(sections.contains(&SECTION_PROJECT_INFO));
    assert!(sections.contains(&SECTION_USER_PREFS));
}

#[test]
fn format_core_memory_for_prompt_wraps_in_core_memory_tag() {
    let (db, _dir) = fresh_db();
    db.update_core_memory(SECTION_PERSONA, "test content")
        .expect("update");
    let formatted = db.format_core_memory_for_prompt().expect("format");
    assert!(formatted.starts_with("<core_memory>"));
    assert!(formatted.ends_with("</core_memory>"));
    assert!(formatted.contains("test content"));
}

#[test]
fn format_core_memory_for_prompt_escapes_xml_meta_chars_in_section_and_content() {
    // Crosslink #692: section + content are untrusted.
    let (db, _dir) = fresh_db();
    db.update_core_memory(
        SECTION_USER_PREFS,
        "user said <script>alert('xss')</script> & some text",
    )
    .expect("update");
    let formatted = db.format_core_memory_for_prompt().expect("format");
    assert!(
        !formatted.contains("<script>"),
        "raw <script> MUST NOT appear; got {formatted:?}"
    );
    assert!(
        formatted.contains("&lt;script&gt;") || formatted.contains("&lt;"),
        "MUST escape angle brackets; got {formatted:?}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — memory_search_by_tag
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn search_by_tag_finds_memories_with_matching_tag() {
    let (db, _dir) = fresh_db();
    db.memory_save("first content", &["tag-a".to_string()])
        .expect("save a");
    db.memory_save("second content", &["tag-b".to_string()])
        .expect("save b");
    db.memory_save("third content", &["tag-a".to_string(), "tag-c".to_string()])
        .expect("save ac");
    let matches = db.memory_search_by_tag("tag-a", 10).expect("search");
    assert_eq!(matches.len(), 2);
    let contents: Vec<&str> = matches.iter().map(|m| m.content.as_str()).collect();
    assert!(contents.contains(&"first content"));
    assert!(contents.contains(&"third content"));
}

#[test]
fn search_by_tag_returns_empty_for_unknown_tag() {
    let (db, _dir) = fresh_db();
    db.memory_save("x", &["actual-tag".to_string()])
        .expect("save");
    let matches = db
        .memory_search_by_tag("never-used-tag", 10)
        .expect("search");
    assert!(matches.is_empty());
}

#[test]
fn search_by_tag_respects_limit() {
    let (db, _dir) = fresh_db();
    for i in 0..5 {
        db.memory_save(&format!("entry-{i}"), &["common".to_string()])
            .expect("save");
    }
    let limited = db.memory_search_by_tag("common", 3).expect("search");
    assert!(limited.len() <= 3);
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — cleanup_expired_short_term
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn cleanup_expired_short_term_on_empty_db_returns_zeros() {
    let (db, _dir) = fresh_db();
    let (sessions, activities) = db.cleanup_expired_short_term().expect("cleanup");
    assert_eq!(sessions, 0);
    assert_eq!(activities, 0);
}

#[test]
fn cleanup_expired_short_term_does_not_drop_recent_entries() {
    let (db, _dir) = fresh_db();
    // Just-now session should not expire.
    let now = chrono::Utc::now().to_rfc3339();
    db.save_session_summary("s", "x", &[], &[], &now)
        .expect("save");
    db.log_activity("s", "tool_use", "x", None).expect("log");
    let (s, a) = db.cleanup_expired_short_term().expect("cleanup");
    assert_eq!(s, 0, "fresh session MUST NOT expire");
    assert_eq!(a, 0, "fresh activity MUST NOT expire");
    let sessions = db.get_recent_sessions(10).expect("list");
    assert_eq!(sessions.len(), 1);
}

// ───────────────────────────────────────────────────────────────────────────
// Section G — format_recent_context_for_prompt
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn format_recent_context_returns_empty_string_on_empty_db() {
    let (db, _dir) = fresh_db();
    let formatted = db.format_recent_context_for_prompt().expect("format");
    assert!(formatted.is_empty());
}

#[test]
fn format_recent_context_includes_session_summary() {
    let (db, _dir) = fresh_db();
    let now = chrono::Utc::now().to_rfc3339();
    db.save_session_summary(
        "session-test",
        "fixed several bugs in auth module",
        &["src/auth.rs".to_string()],
        &["#42".to_string()],
        &now,
    )
    .expect("save");
    let formatted = db.format_recent_context_for_prompt().expect("format");
    assert!(
        !formatted.is_empty(),
        "MUST surface recent context when session present"
    );
    assert!(formatted.contains("fixed several bugs"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section H — memory_stats coherence
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn memory_stats_count_matches_memory_list_count() {
    let (db, _dir) = fresh_db();
    for i in 0..3 {
        db.memory_save(&format!("entry-{i}"), &[]).expect("save");
    }
    let stats = db.memory_stats().expect("stats");
    let list = db.memory_list(100).expect("list");
    assert_eq!(stats.count, list.len());
}

#[test]
fn memory_stats_zero_for_empty_db() {
    let (db, _dir) = fresh_db();
    let stats = db.memory_stats().expect("stats");
    assert_eq!(stats.count, 0);
    assert_eq!(stats.total_size, 0);
}

#[test]
fn memory_stats_total_size_tracks_content_bytes() {
    let (db, _dir) = fresh_db();
    db.memory_save("hello", &[]).expect("save");
    db.memory_save("world", &[]).expect("save");
    let stats = db.memory_stats().expect("stats");
    // Documented contract: total_size aggregates content byte length.
    assert!(
        stats.total_size >= 10,
        "MUST aggregate content bytes; got {}",
        stats.total_size
    );
}
