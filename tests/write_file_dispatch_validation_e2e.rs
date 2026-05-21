//! End-to-end tests for the `write_file` tool dispatched
//! through the registry — invalidation arms that fire
//! BEFORE any filesystem write.
//!
//! Sprint 142 of the verification effort. This file pins
//! the registry-dispatched validation paths for `write_file`:
//! missing path, wrong-type path, missing content, the
//! "must read before overwrite" gate (#968), and Edit/Write
//! permission-target wiring (#782).

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::tools::registry::{registry, ToolContext};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

fn cwd_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn dispatch_write(args: &HashMap<String, Value>) -> (String, bool) {
    let mut ctx = ToolContext {
        memory_db: None,
        app_config: None,
        task_mgr: None,
    };
    registry()
        .dispatch("write_file", args, &mut ctx)
        .expect("write_file must be registered")
}

fn args_with(entries: &[(&str, Value)]) -> HashMap<String, Value> {
    let mut m = HashMap::new();
    for (k, v) in entries {
        m.insert((*k).to_string(), v.clone());
    }
    m
}

// ───────────────────────────────────────────────────────────────────────────
// Section A — Missing/wrong-type path arg
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn missing_path_arg_errors_with_documented_message() {
    let (msg, is_err) = dispatch_write(&HashMap::new());
    assert!(is_err);
    assert!(
        msg.contains("Missing 'path' argument"),
        "MUST surface documented missing-path message; got {msg:?}"
    );
}

#[test]
fn path_arg_as_number_treated_as_missing() {
    let args = args_with(&[("path", json!(42)), ("content", json!("body"))]);
    let (msg, is_err) = dispatch_write(&args);
    assert!(is_err);
    assert!(
        msg.contains("Missing 'path' argument"),
        "non-string path MUST surface missing-path; got {msg:?}"
    );
}

#[test]
fn path_arg_as_array_treated_as_missing() {
    let args = args_with(&[("path", json!(["x"])), ("content", json!("body"))]);
    let (msg, is_err) = dispatch_write(&args);
    assert!(is_err);
    assert!(msg.contains("Missing 'path' argument"));
}

#[test]
fn path_arg_as_object_treated_as_missing() {
    let args = args_with(&[("path", json!({"k": "v"})), ("content", json!("body"))]);
    let (msg, is_err) = dispatch_write(&args);
    assert!(is_err);
    assert!(msg.contains("Missing 'path' argument"));
}

#[test]
fn path_arg_as_null_treated_as_missing() {
    let args = args_with(&[("path", Value::Null), ("content", json!("body"))]);
    let (msg, is_err) = dispatch_write(&args);
    assert!(is_err);
    assert!(msg.contains("Missing 'path' argument"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — Relative path rejected by resolve_path
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn path_with_parent_dir_traversal_rejected_pre_write() {
    // AUTHORING DISCOVERY: resolve_path REJECTS `..` traversal
    // (Path::Component::ParentDir) but ACCEPTS relative paths
    // (resolves them to cwd-relative).
    // This pins the `..` traversal gate.
    let args = args_with(&[
        ("path", json!("/tmp/../etc/file.txt")),
        ("content", json!("body")),
    ]);
    let (msg, is_err) = dispatch_write(&args);
    assert!(is_err, "../-traversal path MUST be rejected");
    assert!(
        msg.contains("traversal") || msg.contains("Path"),
        "MUST surface path-traversal message; got {msg:?}"
    );
}

#[test]
fn relative_path_is_accepted_and_resolved_to_cwd_relative() {
    // AUTHORING DISCOVERY: relative paths are accepted —
    // resolve_path joins them to std::env::current_dir().
    // This is documented behavior (not a security hole).
    // The dispatch must NOT reject for relative-ness alone.
    let _l = cwd_lock();
    let dir = tempfile::TempDir::new().expect("tempdir");
    let prev_cwd = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(dir.path()).expect("cd");

    let args = args_with(&[
        ("path", json!("new_relative_file_xyz.txt")),
        ("content", json!("body")),
    ]);
    let (msg, is_err) = dispatch_write(&args);
    let _ = std::env::set_current_dir(&prev_cwd);
    assert!(
        !is_err,
        "relative path MUST be accepted (resolved to cwd); got error {msg:?}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — Missing content arg (for new file with valid path)
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn missing_content_arg_on_new_file_errors() {
    // Use a tempdir path so the file path validates successfully
    // and we reach the content check.
    let dir = tempfile::TempDir::new().expect("tempdir");
    let path = dir.path().join("new_unique_file_marker.txt");
    let args = args_with(&[("path", json!(path.to_str().unwrap()))]);
    let (msg, is_err) = dispatch_write(&args);
    assert!(is_err);
    assert!(
        msg.contains("Missing 'content' argument"),
        "MUST surface documented missing-content message; got {msg:?}"
    );
}

#[test]
fn content_arg_as_number_treated_as_missing() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let path = dir.path().join("test.txt");
    let args = args_with(&[
        ("path", json!(path.to_str().unwrap())),
        ("content", json!(42)),
    ]);
    let (msg, is_err) = dispatch_write(&args);
    assert!(is_err);
    assert!(
        msg.contains("Missing 'content' argument"),
        "non-string content MUST surface missing-content; got {msg:?}"
    );
}

#[test]
fn content_arg_as_array_treated_as_missing() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let path = dir.path().join("test.txt");
    let args = args_with(&[
        ("path", json!(path.to_str().unwrap())),
        ("content", json!(["line1", "line2"])),
    ]);
    let (msg, is_err) = dispatch_write(&args);
    assert!(is_err);
    assert!(msg.contains("Missing 'content' argument"));
}

#[test]
fn content_arg_as_null_treated_as_missing() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let path = dir.path().join("test.txt");
    let args = args_with(&[
        ("path", json!(path.to_str().unwrap())),
        ("content", Value::Null),
    ]);
    let (msg, is_err) = dispatch_write(&args);
    assert!(is_err);
    assert!(msg.contains("Missing 'content' argument"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — Argument-check ordering
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn missing_both_args_surfaces_path_error_first() {
    // PINS ORDER: path validation runs BEFORE content validation.
    // Missing both → path error message surfaces.
    let (msg, is_err) = dispatch_write(&HashMap::new());
    assert!(is_err);
    assert!(
        msg.contains("'path'"),
        "MUST surface path error before content; got {msg:?}"
    );
    assert!(
        !msg.contains("'content'"),
        "content error MUST NOT fire when path is missing; got {msg:?}"
    );
}

#[test]
fn traversal_path_with_missing_content_surfaces_traversal_error_first() {
    // PINS ORDER: path resolution runs BEFORE content check.
    // A `..` traversal MUST surface path error even if
    // content is missing.
    let args = args_with(&[("path", json!("/tmp/../etc/file.txt"))]);
    let (msg, is_err) = dispatch_write(&args);
    assert!(is_err);
    // Path error MUST fire; content-missing MUST NOT.
    assert!(
        !msg.contains("Missing 'content' argument"),
        "traversal-path error MUST fire before content check; got {msg:?}"
    );
    assert!(
        msg.contains("traversal") || msg.contains("Path"),
        "MUST surface path-traversal message; got {msg:?}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — Overwrite gate (#968): must read before overwriting
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn overwrite_existing_file_without_prior_read_errors() {
    // PINS #968: an existing file that has NOT been read via
    // read_file in this session cannot be overwritten — model
    // would be hallucinating prior content.
    let dir = tempfile::TempDir::new().expect("tempdir");
    let path = dir.path().join("existing_unique_marker.txt");
    // Pre-create file (existing) without going through read_file.
    std::fs::write(&path, "original content").expect("create");

    let args = args_with(&[
        ("path", json!(path.to_str().unwrap())),
        ("content", json!("new content")),
    ]);
    let (msg, is_err) = dispatch_write(&args);
    assert!(is_err, "overwrite without prior read MUST be refused");
    assert!(
        msg.contains("must read") && msg.contains("before overwriting"),
        "MUST surface #968 message; got {msg:?}"
    );
    // Error MUST hint at the corrective action.
    assert!(
        msg.contains("read_file"),
        "MUST suggest read_file; got {msg:?}"
    );
    // Original content MUST NOT be overwritten when gate fires.
    let preserved = std::fs::read_to_string(&path).expect("read");
    assert_eq!(
        preserved, "original content",
        "gate failure MUST preserve original file content"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — Empty content is valid (creates empty file)
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn empty_string_content_is_accepted_for_new_file() {
    // Empty content string is valid — creates empty file.
    let dir = tempfile::TempDir::new().expect("tempdir");
    let path = dir.path().join("empty_new.txt");
    let args = args_with(&[
        ("path", json!(path.to_str().unwrap())),
        ("content", json!("")),
    ]);
    let (_msg, is_err) = dispatch_write(&args);
    assert!(!is_err, "empty content for new file MUST succeed");
    assert!(path.exists(), "new file MUST be created");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
}

#[test]
fn unicode_content_writes_byte_exact() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let path = dir.path().join("unicode.txt");
    let content = "日本語コンテンツ 🎉";
    let args = args_with(&[
        ("path", json!(path.to_str().unwrap())),
        ("content", json!(content)),
    ]);
    let (_msg, is_err) = dispatch_write(&args);
    assert!(!is_err);
    let written = std::fs::read_to_string(&path).expect("read");
    assert_eq!(written, content, "unicode content MUST round-trip");
}
