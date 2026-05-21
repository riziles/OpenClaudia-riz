//! End-to-end tests for `rules::RulesEngine` public accessors
//! (`all_rules`, `rules_dir`) + `Rule` struct shape +
//! `extract_extensions_from_tool_input` unknown-tool path +
//! edge cases at the loader boundary.
//!
//! Sprint 97 of the verification effort. Sprint 43
//! (`rules_engine_deep_e2e`) covered the per-language
//! matching matrix and the filename-prefix parser; sprint 22
//! (`rules_context_e2e`) covered the context-injection
//! integration; this file pins the accessor surface
//! (`all_rules` + `rules_dir`) and the unknown-tool /
//! edge-case behaviour of `extract_extensions_from_tool_input`.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::rules::{extract_extensions_from_tool_input, Rule, RulesEngine};
use serde_json::json;
use std::fs;
use tempfile::TempDir;

// ───────────────────────────────────────────────────────────────────────────
// Helpers
// ───────────────────────────────────────────────────────────────────────────

fn write_rule(dir: &std::path::Path, filename: &str, content: &str) {
    fs::write(dir.join(filename), content).expect("write");
}

// ───────────────────────────────────────────────────────────────────────────
// Section A — RulesEngine::rules_dir accessor
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn rules_dir_returns_path_supplied_at_construction() {
    let tmp = TempDir::new().expect("tempdir");
    let rules_path = tmp.path().join("custom-rules");
    let engine = RulesEngine::new(&rules_path);
    assert_eq!(engine.rules_dir(), rules_path.as_path());
}

#[test]
fn rules_dir_preserves_path_across_reload() {
    let tmp = TempDir::new().expect("tempdir");
    let rules_path = tmp.path().join("rules");
    fs::create_dir(&rules_path).expect("mkdir");
    let mut engine = RulesEngine::new(&rules_path);
    let before = engine.rules_dir().to_path_buf();
    engine.reload();
    assert_eq!(engine.rules_dir(), before.as_path());
}

#[test]
fn rules_dir_nonexistent_path_still_returned_verbatim() {
    let engine = RulesEngine::new("/nonexistent/path/that/never/existed");
    assert_eq!(
        engine.rules_dir(),
        std::path::Path::new("/nonexistent/path/that/never/existed")
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — RulesEngine::all_rules accessor
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn all_rules_returns_empty_slice_for_nonexistent_dir() {
    let engine = RulesEngine::new("/totally/missing/rules");
    assert!(engine.all_rules().is_empty());
}

#[test]
fn all_rules_returns_empty_slice_for_empty_dir() {
    let tmp = TempDir::new().expect("tempdir");
    let rules_path = tmp.path().join("rules");
    fs::create_dir(&rules_path).expect("mkdir");
    let engine = RulesEngine::new(&rules_path);
    assert!(engine.all_rules().is_empty());
}

#[test]
fn all_rules_lists_loaded_rules_for_global_and_lang_specific() {
    let tmp = TempDir::new().expect("tempdir");
    let rules_path = tmp.path().join("rules");
    fs::create_dir(&rules_path).expect("mkdir");
    write_rule(&rules_path, "always.md", "Global content");
    write_rule(&rules_path, "rust.md", "Rust content");
    write_rule(&rules_path, "python.md", "Python content");

    let engine = RulesEngine::new(&rules_path);
    let all = engine.all_rules();
    assert_eq!(all.len(), 3);
    let names: Vec<&str> = all.iter().map(|r| r.name.as_str()).collect();
    assert!(names.contains(&"always"));
    assert!(names.contains(&"rust"));
    assert!(names.contains(&"python"));
}

#[test]
fn all_rules_returns_borrowed_slice_not_owned() {
    // Compile-time: all_rules returns &[Rule], not Vec<Rule>.
    let tmp = TempDir::new().expect("tempdir");
    let rules_path = tmp.path().join("rules");
    fs::create_dir(&rules_path).expect("mkdir");
    write_rule(&rules_path, "always.md", "Global");
    let engine = RulesEngine::new(&rules_path);
    let slice: &[Rule] = engine.all_rules();
    assert_eq!(slice.len(), 1);
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — Rule struct shape
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn rule_has_three_pub_fields_name_content_languages() {
    let r = Rule {
        name: "n".to_string(),
        content: "c".to_string(),
        languages: vec!["rust".to_string()],
    };
    assert_eq!(r.name, "n");
    assert_eq!(r.content, "c");
    assert_eq!(r.languages, vec!["rust".to_string()]);
}

#[test]
fn rule_clone_preserves_all_fields() {
    let original = Rule {
        name: "test".to_string(),
        content: "body".to_string(),
        languages: vec!["python".to_string(), "rust".to_string()],
    };
    let cloned = original.clone();
    assert_eq!(cloned.name, original.name);
    assert_eq!(cloned.content, original.content);
    assert_eq!(cloned.languages, original.languages);
}

#[test]
fn loaded_rule_for_global_file_has_empty_languages_vec() {
    let tmp = TempDir::new().expect("tempdir");
    let rules_path = tmp.path().join("rules");
    fs::create_dir(&rules_path).expect("mkdir");
    write_rule(&rules_path, "always.md", "Global body");
    let engine = RulesEngine::new(&rules_path);
    let rule = engine
        .all_rules()
        .iter()
        .find(|r| r.name == "always")
        .unwrap();
    assert!(
        rule.languages.is_empty(),
        "global rule MUST have empty languages"
    );
    assert_eq!(rule.content, "Global body");
}

#[test]
fn loaded_rule_for_language_file_has_populated_languages_vec() {
    let tmp = TempDir::new().expect("tempdir");
    let rules_path = tmp.path().join("rules");
    fs::create_dir(&rules_path).expect("mkdir");
    write_rule(&rules_path, "rust.md", "Rust body");
    let engine = RulesEngine::new(&rules_path);
    let rule = engine
        .all_rules()
        .iter()
        .find(|r| r.name == "rust")
        .unwrap();
    assert!(!rule.languages.is_empty());
    assert!(rule.languages.iter().any(|s| s == "rust"));
}

#[test]
fn loaded_rule_preserves_utf8_content_byte_exact() {
    let tmp = TempDir::new().expect("tempdir");
    let rules_path = tmp.path().join("rules");
    fs::create_dir(&rules_path).expect("mkdir");
    let body = "# 日本語の規則\n\n• Use emoji 🎉 sparingly";
    write_rule(&rules_path, "always.md", body);
    let engine = RulesEngine::new(&rules_path);
    let rule = engine
        .all_rules()
        .iter()
        .find(|r| r.name == "always")
        .unwrap();
    assert_eq!(rule.content, body);
}

#[test]
fn loaded_rule_preserves_empty_content_file() {
    let tmp = TempDir::new().expect("tempdir");
    let rules_path = tmp.path().join("rules");
    fs::create_dir(&rules_path).expect("mkdir");
    write_rule(&rules_path, "always.md", "");
    let engine = RulesEngine::new(&rules_path);
    let rule = engine.all_rules().iter().find(|r| r.name == "always");
    // An empty-file rule MAY be loaded or skipped depending
    // on impl; both are reasonable. Pin which: if loaded,
    // content is empty; if not loaded, the engine still has
    // 0 rules.
    if let Some(r) = rule {
        assert_eq!(r.content, "");
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — extract_extensions_from_tool_input unknown tools
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn extract_extensions_unknown_tool_returns_empty_vec() {
    for tool in &["bash", "unknown_tool", "WebFetch", "ToolThatDoesntExist"] {
        let exts = extract_extensions_from_tool_input(tool, &json!({"file_path": "/tmp/foo.rs"}));
        assert!(
            exts.is_empty(),
            "{tool:?} MUST NOT extract extensions (not a file-related tool)"
        );
    }
}

#[test]
fn extract_extensions_write_with_missing_file_path_returns_empty() {
    let exts = extract_extensions_from_tool_input("Write", &json!({}));
    assert!(exts.is_empty());
}

#[test]
fn extract_extensions_edit_with_extensionless_path_returns_empty() {
    let exts =
        extract_extensions_from_tool_input("Edit", &json!({"file_path": "/tmp/no-extension-here"}));
    assert!(exts.is_empty());
}

#[test]
fn extract_extensions_read_with_dotfile_extracts_no_extension_per_path_extension() {
    // ".bashrc" — Path::extension returns None for dotfiles.
    let exts =
        extract_extensions_from_tool_input("Read", &json!({"file_path": "/home/user/.bashrc"}));
    assert!(
        exts.is_empty(),
        "Path::extension on dotfile returns None; MUST be empty"
    );
}

#[test]
fn extract_extensions_glob_with_pattern_lacking_dot_returns_empty() {
    // Documented #796: pattern without .ext yields no extensions.
    for pattern in &["src/util", "**/file", "*", "src/**/*"] {
        let exts = extract_extensions_from_tool_input("Glob", &json!({"pattern": pattern}));
        assert!(
            exts.is_empty(),
            "pattern {pattern:?} MUST yield no extensions"
        );
    }
}

#[test]
fn extract_extensions_glob_with_explicit_dot_ext_captures_it() {
    let exts = extract_extensions_from_tool_input("Glob", &json!({"pattern": "src/**/*.toml"}));
    assert_eq!(exts, vec!["toml".to_string()]);
}

#[test]
fn extract_extensions_with_non_string_file_path_returns_empty() {
    let exts = extract_extensions_from_tool_input("Write", &json!({"file_path": 42}));
    assert!(exts.is_empty());
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — RulesEngine reload semantics
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn reload_picks_up_newly_added_rule_files() {
    let tmp = TempDir::new().expect("tempdir");
    let rules_path = tmp.path().join("rules");
    fs::create_dir(&rules_path).expect("mkdir");
    let mut engine = RulesEngine::new(&rules_path);
    assert!(engine.all_rules().is_empty());

    write_rule(&rules_path, "rust.md", "added later");
    engine.reload();
    assert_eq!(engine.all_rules().len(), 1);
    assert_eq!(engine.all_rules()[0].name, "rust");
}

#[test]
fn reload_picks_up_content_changes_to_existing_files() {
    let tmp = TempDir::new().expect("tempdir");
    let rules_path = tmp.path().join("rules");
    fs::create_dir(&rules_path).expect("mkdir");
    write_rule(&rules_path, "always.md", "first body");
    let mut engine = RulesEngine::new(&rules_path);
    assert_eq!(engine.all_rules()[0].content, "first body");

    write_rule(&rules_path, "always.md", "updated body");
    engine.reload();
    assert_eq!(engine.all_rules()[0].content, "updated body");
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — RulesEngine Clone
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn rules_engine_clone_preserves_loaded_rules_and_dir() {
    let tmp = TempDir::new().expect("tempdir");
    let rules_path = tmp.path().join("rules");
    fs::create_dir(&rules_path).expect("mkdir");
    write_rule(&rules_path, "rust.md", "body");
    let engine = RulesEngine::new(&rules_path);
    let cloned = engine.clone();
    assert_eq!(cloned.rules_dir(), engine.rules_dir());
    assert_eq!(cloned.all_rules().len(), engine.all_rules().len());
}
