//! End-to-end tests filling gaps in `RulesEngine` coverage left
//! by `tests/rules_context_e2e.rs` (sprint 14).
//!
//! Sprint 40 of the verification effort.
//!
//! Coverage shape:
//!   - LANGUAGES table — every documented (lang, extension) pair
//!     resolves both ways at the engine boundary.
//!   - Filename parsing — case-insensitivity, hyphenated form,
//!     `always`/`global`/`all` global aliases.
//!   - `get_rules_for_files` — file-path → extension extraction.
//!   - Reload semantics — file removal as well as addition.
//!   - Edge cases — empty rules dir, non-md files ignored.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::rules::RulesEngine;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

// ───────────────────────────────────────────────────────────────────────────
// Helpers
// ───────────────────────────────────────────────────────────────────────────

fn write_rule(dir: &Path, filename: &str, content: &str) {
    fs::create_dir_all(dir).expect("mkdir parent");
    fs::write(dir.join(filename), content).expect("write rule");
}

// ───────────────────────────────────────────────────────────────────────────
// Section A — LANGUAGES table coverage (forward direction)
// ───────────────────────────────────────────────────────────────────────────

/// Documented (language, extension) pairs. Mirrors `src/rules.rs::LANGUAGES`.
/// If a new language is added, this table must update to keep coverage.
const LANG_EXT_PAIRS: &[(&str, &str)] = &[
    ("rust", "rs"),
    ("python", "py"),
    ("python", "pyw"),
    ("javascript", "js"),
    ("javascript", "mjs"),
    ("javascript", "cjs"),
    ("typescript", "ts"),
    ("tsx", "tsx"),
    ("jsx", "jsx"),
    ("go", "go"),
    ("java", "java"),
    ("kotlin", "kt"),
    ("swift", "swift"),
    ("c", "c"),
    ("c", "h"),
    ("cpp", "cpp"),
    ("cpp", "hpp"),
    ("csharp", "cs"),
    ("ruby", "rb"),
    ("php", "php"),
    ("scala", "scala"),
    ("elixir", "ex"),
    ("erlang", "erl"),
    ("haskell", "hs"),
    ("clojure", "clj"),
    ("lua", "lua"),
    ("r", "r"),
    ("julia", "jl"),
    ("dart", "dart"),
    ("zig", "zig"),
    ("nim", "nim"),
    ("vlang", "v"),
    ("sql", "sql"),
    ("shell", "sh"),
    ("yaml", "yml"),
    ("json", "json"),
    ("toml", "toml"),
    ("xml", "xml"),
    ("html", "html"),
    ("css", "css"),
    ("markdown", "md"),
];

#[test]
fn every_documented_language_extension_routes_to_its_language_rule() {
    let dir = TempDir::new().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    // Write one rule per language using the canonical name.
    let unique_langs: std::collections::HashSet<&str> =
        LANG_EXT_PAIRS.iter().map(|(l, _)| *l).collect();
    for lang in &unique_langs {
        write_rule(&rules_dir, &format!("{lang}.md"), &format!("# {lang} rule"));
    }
    let engine = RulesEngine::new(&rules_dir);

    // For every (lang, ext) pair, querying the engine with the
    // extension MUST return a rule list including the lang rule.
    for (lang, ext) in LANG_EXT_PAIRS {
        let rules = engine.get_rules_for_extensions(&[ext]);
        let names: Vec<&str> = rules.iter().map(|r| r.name.as_str()).collect();
        assert!(
            names.contains(lang),
            "extension {ext:?} MUST resolve to {lang:?} rule; got {names:?}"
        );
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — Filename parsing edge cases
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn always_global_and_all_filenames_are_global() {
    let dir = TempDir::new().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    write_rule(&rules_dir, "always.md", "x");
    write_rule(&rules_dir, "global.md", "y");
    write_rule(&rules_dir, "all.md", "z");
    let engine = RulesEngine::new(&rules_dir);

    // Query with a totally-unmatched extension; all 3 globals
    // must still appear.
    let rules = engine.get_rules_for_extensions(&["zzz"]);
    let names: Vec<&str> = rules.iter().map(|r| r.name.as_str()).collect();
    for expected in &["always", "global", "all"] {
        assert!(
            names.contains(expected),
            "{expected:?} MUST be a global rule; got {names:?}"
        );
    }
}

#[test]
fn hyphenated_language_prefix_is_recognized() {
    let dir = TempDir::new().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    write_rule(&rules_dir, "rust-memory.md", "rust mem");
    write_rule(&rules_dir, "python-async.md", "py async");
    let engine = RulesEngine::new(&rules_dir);

    // `rust-memory.md` MUST apply to `.rs` files.
    let rust_rules = engine.get_rules_for_extensions(&["rs"]);
    let rust_names: Vec<&str> = rust_rules.iter().map(|r| r.name.as_str()).collect();
    assert!(
        rust_names.contains(&"rust-memory"),
        "rust-memory must apply to .rs; got {rust_names:?}"
    );
    // …but NOT to `.py` files.
    let py_rules = engine.get_rules_for_extensions(&["py"]);
    let py_names: Vec<&str> = py_rules.iter().map(|r| r.name.as_str()).collect();
    assert!(
        !py_names.contains(&"rust-memory"),
        "rust-memory MUST NOT apply to .py; got {py_names:?}"
    );
    assert!(py_names.contains(&"python-async"));
}

#[test]
fn unknown_prefix_filename_classifies_as_global() {
    let dir = TempDir::new().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    // "security.md" doesn't start with any known language; the
    // parser classifies it as global so security rules apply
    // to every file.
    write_rule(&rules_dir, "security.md", "security");
    let engine = RulesEngine::new(&rules_dir);

    // Apply with no extensions — should still surface as global.
    let rules = engine.get_rules_for_extensions(&[]);
    let names: Vec<&str> = rules.iter().map(|r| r.name.as_str()).collect();
    assert!(
        names.contains(&"security"),
        "unknown-prefix rule must be global; got {names:?}"
    );
}

#[test]
fn filename_parsing_is_case_insensitive() {
    let dir = TempDir::new().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    // CamelCase + UPPERCASE forms.
    write_rule(&rules_dir, "Rust.md", "X");
    write_rule(&rules_dir, "PYTHON.md", "Y");
    let engine = RulesEngine::new(&rules_dir);

    let rust_rules = engine.get_rules_for_extensions(&["rs"]);
    let rust_names: Vec<&str> = rust_rules.iter().map(|r| r.name.as_str()).collect();
    assert!(
        rust_names.contains(&"Rust"),
        "Rust.md MUST classify as rust; got {rust_names:?}"
    );

    let py_rules = engine.get_rules_for_extensions(&["py"]);
    let py_names: Vec<&str> = py_rules.iter().map(|r| r.name.as_str()).collect();
    assert!(
        py_names.contains(&"PYTHON"),
        "PYTHON.md MUST classify as python; got {py_names:?}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — get_rules_for_files (path → extension chain)
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn get_rules_for_files_routes_by_extension() {
    let dir = TempDir::new().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    write_rule(&rules_dir, "rust.md", "rust");
    write_rule(&rules_dir, "go.md", "go");
    let engine = RulesEngine::new(&rules_dir);

    let rules = engine.get_rules_for_files(&["src/main.rs", "lib.go"]);
    let names: Vec<&str> = rules.iter().map(|r| r.name.as_str()).collect();
    assert!(names.contains(&"rust"));
    assert!(names.contains(&"go"));
}

#[test]
fn get_rules_for_files_ignores_extensionless_paths() {
    let dir = TempDir::new().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    write_rule(&rules_dir, "rust.md", "rust");
    write_rule(&rules_dir, "always.md", "always");
    let engine = RulesEngine::new(&rules_dir);

    // Path with no extension at all.
    let rules = engine.get_rules_for_files(&["Makefile", "Dockerfile"]);
    let names: Vec<&str> = rules.iter().map(|r| r.name.as_str()).collect();
    // Only the global "always" rule should apply.
    assert!(names.contains(&"always"));
    assert!(
        !names.contains(&"rust"),
        "extensionless paths must NOT trigger rust rule; got {names:?}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — non-.md files ignored
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn non_md_files_in_rules_dir_are_ignored() {
    let dir = TempDir::new().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    fs::create_dir_all(&rules_dir).expect("mkdir");
    fs::write(rules_dir.join("rust.md"), "rust").expect("write");
    // Various non-.md files that must NOT load as rules.
    fs::write(rules_dir.join("rust.txt"), "ignored").expect("txt");
    fs::write(rules_dir.join("python.json"), "{}").expect("json");
    fs::write(rules_dir.join("README"), "ignored").expect("no-ext");
    fs::write(rules_dir.join(".hidden.md"), "ignored?").expect("hidden");

    let engine = RulesEngine::new(&rules_dir);
    let all = engine.all_rules();
    let names: Vec<&str> = all.iter().map(|r| r.name.as_str()).collect();
    assert!(names.contains(&"rust"), "rust.md MUST load; got {names:?}");
    // The .txt / .json / no-extension files must NOT load.
    assert!(
        !names.contains(&"rust.txt"),
        "non-md file MUST NOT load; got {names:?}"
    );
    assert!(!names.iter().any(|n| n.contains(".json")));
    assert!(!names.contains(&"README"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — Reload removal semantics
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn reload_drops_rules_that_were_deleted_from_disk() {
    let dir = TempDir::new().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    write_rule(&rules_dir, "rust.md", "before");
    write_rule(&rules_dir, "python.md", "before");
    let mut engine = RulesEngine::new(&rules_dir);
    assert_eq!(engine.all_rules().len(), 2);

    // Delete one rule from disk.
    fs::remove_file(rules_dir.join("python.md")).expect("remove");
    // Without reload, the engine still has the cached entry.
    assert_eq!(engine.all_rules().len(), 2);
    // After reload, the deletion is observed.
    engine.reload();
    let names: Vec<&str> = engine.all_rules().iter().map(|r| r.name.as_str()).collect();
    assert_eq!(engine.all_rules().len(), 1);
    assert!(names.contains(&"rust"));
    assert!(
        !names.contains(&"python"),
        "deleted rule MUST be gone after reload; got {names:?}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — Edge cases on extension queries
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn empty_extension_list_returns_only_globals() {
    let dir = TempDir::new().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    write_rule(&rules_dir, "rust.md", "rust");
    write_rule(&rules_dir, "always.md", "always");
    let engine = RulesEngine::new(&rules_dir);

    let rules = engine.get_rules_for_extensions(&[]);
    let names: Vec<&str> = rules.iter().map(|r| r.name.as_str()).collect();
    // The global rule must appear; the language-specific rust
    // rule must NOT.
    assert!(names.contains(&"always"));
    assert!(
        !names.contains(&"rust"),
        "rust rule MUST NOT appear for empty extension list; got {names:?}"
    );
}

#[test]
fn unknown_extension_returns_only_globals() {
    let dir = TempDir::new().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    write_rule(&rules_dir, "rust.md", "rust");
    write_rule(&rules_dir, "always.md", "always");
    let engine = RulesEngine::new(&rules_dir);

    let rules = engine.get_rules_for_extensions(&["totally-fake-extension"]);
    let names: Vec<&str> = rules.iter().map(|r| r.name.as_str()).collect();
    assert!(names.contains(&"always"));
    assert!(!names.contains(&"rust"));
}

#[test]
fn nonexistent_rules_dir_yields_empty_engine() {
    let dir = TempDir::new().expect("tempdir");
    let nope = dir.path().join("never-existed");
    let engine = RulesEngine::new(&nope);
    assert!(
        engine.all_rules().is_empty(),
        "nonexistent rules dir MUST yield empty engine"
    );
    // And queries still work (return empty).
    assert!(engine.get_rules_for_extensions(&["rs"]).is_empty());
    assert!(engine.get_combined_rules(&["rs"]).is_empty());
}

// ───────────────────────────────────────────────────────────────────────────
// Section G — get_combined_rules header shape
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn combined_rules_separator_is_horizontal_rule_md() {
    let dir = TempDir::new().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    write_rule(&rules_dir, "rust.md", "rust body");
    write_rule(&rules_dir, "always.md", "always body");
    let engine = RulesEngine::new(&rules_dir);

    let combined = engine.get_combined_rules(&["rs"]);
    // Per-rule header format AND the `---` separator between
    // rules.
    assert!(
        combined.contains("## rust Rules"),
        "must contain `## rust Rules` header; got {combined:?}"
    );
    assert!(
        combined.contains("## always Rules"),
        "must contain `## always Rules` header; got {combined:?}"
    );
    assert!(
        combined.contains("---"),
        "must use --- as the inter-rule separator; got {combined:?}"
    );
}

#[test]
fn combined_rules_preserves_body_content_verbatim() {
    let dir = TempDir::new().expect("tempdir");
    let rules_dir = dir.path().join("rules");
    let body = "Body with `code` and **bold** and special chars: <>&\"";
    write_rule(&rules_dir, "rust.md", body);
    let engine = RulesEngine::new(&rules_dir);
    let combined = engine.get_combined_rules(&["rs"]);
    assert!(
        combined.contains(body),
        "rule body must round-trip verbatim through combined output; \
         got {combined:?}"
    );
}
