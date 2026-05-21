//! End-to-end tests for `file_error::FileError` variants +
//! `file_error::read/write/create_dir_all/read_json/read_yaml/write_json_pretty`
//! helpers + `output_style::builtin_styles` catalog.
//!
//! Sprint 65 of the verification effort. Covers two
//! library-side modules with thin/no direct E2E coverage:
//! `src/file_error.rs` and `src/output_style.rs`.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::file_error::{
    create_dir_all, read_file, read_json, read_yaml, write_file, write_json_pretty, FileError,
};
use openclaudia::output_style::builtin_styles;
use serde::{Deserialize, Serialize};
use std::io::ErrorKind;
use std::path::Path;
use tempfile::TempDir;

// ───────────────────────────────────────────────────────────────────────────
// Section A — FileError variant shape + accessors
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn io_variant_path_accessor_returns_carried_path() {
    let err = FileError::Io {
        path: "/etc/x".into(),
        source: std::io::Error::new(ErrorKind::NotFound, "x"),
    };
    assert_eq!(err.path(), Path::new("/etc/x"));
}

#[test]
fn invalid_variant_path_accessor_returns_carried_path() {
    let err = FileError::Invalid {
        path: "/etc/y".into(),
        reason: "symlink".to_string(),
    };
    assert_eq!(err.path(), Path::new("/etc/y"));
}

#[test]
fn io_kind_returns_inner_kind_for_io_variant() {
    for kind in &[
        ErrorKind::NotFound,
        ErrorKind::PermissionDenied,
        ErrorKind::AlreadyExists,
        ErrorKind::Other,
    ] {
        let err = FileError::Io {
            path: "/x".into(),
            source: std::io::Error::new(*kind, "x"),
        };
        assert_eq!(err.io_kind(), Some(*kind), "io_kind MUST surface {kind:?}");
    }
}

#[test]
fn io_kind_returns_none_for_json_yaml_utf8_invalid_variants() {
    let json = serde_json::from_str::<serde_json::Value>("bad").unwrap_err();
    let je = FileError::Json {
        path: "/x".into(),
        source: json,
    };
    assert!(je.io_kind().is_none());

    let yaml = serde_yaml::from_str::<serde_yaml::Value>("[unclosed").unwrap_err();
    let ye = FileError::Yaml {
        path: "/x".into(),
        source: yaml,
    };
    assert!(ye.io_kind().is_none());

    let inv = FileError::Invalid {
        path: "/x".into(),
        reason: "r".to_string(),
    };
    assert!(inv.io_kind().is_none());
}

#[test]
fn display_format_always_includes_path() {
    let cases: Vec<FileError> = vec![
        FileError::Io {
            path: "/io/path".into(),
            source: std::io::Error::new(ErrorKind::NotFound, "x"),
        },
        FileError::Invalid {
            path: "/inv/path".into(),
            reason: "bad".to_string(),
        },
    ];
    for err in cases {
        let display = err.to_string();
        let path = err.path().display().to_string();
        assert!(
            display.contains(&path),
            "display MUST include path {path:?}; got {display:?}"
        );
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — read_file / write_file round-trip
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn write_then_read_round_trips_string_contents() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("hello.txt");
    write_file(&path, "Hello, world!").expect("write");
    let back = read_file(&path).expect("read");
    assert_eq!(back, "Hello, world!");
}

#[test]
fn read_file_on_missing_path_returns_io_not_found_error() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("never-existed.txt");
    let err = read_file(&path).unwrap_err();
    assert_eq!(err.io_kind(), Some(ErrorKind::NotFound));
    assert_eq!(err.path(), &path);
}

#[test]
fn write_file_accepts_bytes_slice_input() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("bin.dat");
    let bytes: &[u8] = &[0xDE, 0xAD, 0xBE, 0xEF];
    write_file(&path, bytes).expect("write");
    let raw = std::fs::read(&path).expect("read");
    assert_eq!(raw, bytes);
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — create_dir_all
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn create_dir_all_creates_nested_directories() {
    let dir = TempDir::new().expect("tempdir");
    let nested = dir.path().join("a/b/c/d");
    assert!(!nested.exists(), "premise: not yet present");
    create_dir_all(&nested).expect("mkdir");
    assert!(nested.exists());
    assert!(nested.is_dir());
}

#[test]
fn create_dir_all_idempotent_on_already_existing_directory() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("sub");
    create_dir_all(&path).expect("first");
    create_dir_all(&path).expect("second MUST NOT error");
}

#[test]
fn create_dir_all_errors_when_blocked_by_a_file_at_target() {
    let dir = TempDir::new().expect("tempdir");
    let blocker = dir.path().join("imafile");
    std::fs::write(&blocker, "x").expect("write");
    // create_dir_all('blocker') MUST error because the path
    // exists but is a file.
    let outcome = create_dir_all(&blocker);
    assert!(
        outcome.is_err(),
        "create_dir_all on existing file MUST error; got {outcome:?}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — read_json / write_json_pretty round-trip
// ───────────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Sample {
    name: String,
    count: u32,
    tags: Vec<String>,
}

#[test]
fn write_json_pretty_then_read_json_round_trips_struct() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("sample.json");
    let original = Sample {
        name: "test".to_string(),
        count: 42,
        tags: vec!["a".to_string(), "b".to_string()],
    };
    write_json_pretty(&path, &original).expect("write");
    let back: Sample = read_json(&path).expect("read");
    assert_eq!(back, original);
}

#[test]
fn write_json_pretty_produces_human_readable_indentation() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("pretty.json");
    let value = serde_json::json!({"a": 1, "b": [2, 3]});
    write_json_pretty(&path, &value).expect("write");
    let raw = std::fs::read_to_string(&path).expect("read raw");
    // Pretty-printing inserts newlines.
    assert!(
        raw.contains('\n'),
        "pretty JSON MUST contain newlines; got {raw:?}"
    );
}

#[test]
fn read_json_on_invalid_json_returns_json_variant_with_path() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("bad.json");
    std::fs::write(&path, "not valid json {{{").expect("write");
    let err = read_json::<serde_json::Value>(&path).unwrap_err();
    assert!(
        matches!(err, FileError::Json { .. }),
        "MUST be Json variant; got {err:?}"
    );
    assert_eq!(err.path(), &path);
    assert!(err.io_kind().is_none());
}

#[test]
fn read_json_on_missing_path_returns_io_not_found() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("missing.json");
    let err = read_json::<serde_json::Value>(&path).unwrap_err();
    assert_eq!(err.io_kind(), Some(ErrorKind::NotFound));
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — read_yaml
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn read_yaml_round_trips_struct() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("sample.yaml");
    let yaml = r"
name: test
count: 7
tags:
  - one
  - two
";
    std::fs::write(&path, yaml).expect("write");
    let back: Sample = read_yaml(&path).expect("read");
    assert_eq!(back.name, "test");
    assert_eq!(back.count, 7);
    assert_eq!(back.tags, vec!["one".to_string(), "two".to_string()]);
}

#[test]
fn read_yaml_on_invalid_yaml_returns_yaml_variant_with_path() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("bad.yaml");
    std::fs::write(&path, "[unclosed").expect("write");
    let err = read_yaml::<serde_yaml::Value>(&path).unwrap_err();
    assert!(
        matches!(err, FileError::Yaml { .. }),
        "MUST be Yaml variant; got {err:?}"
    );
    assert_eq!(err.path(), &path);
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — FileError::with_path / json_with_path / yaml_with_path closures
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn with_path_builds_io_variant_with_captured_path() {
    let make = FileError::with_path("/captured");
    let raw = std::io::Error::other("boom");
    let err = make(raw);
    assert!(matches!(err, FileError::Io { .. }));
    assert_eq!(err.path(), Path::new("/captured"));
}

#[test]
fn json_with_path_builds_json_variant_with_captured_path() {
    let make = FileError::json_with_path("/cap-json");
    let raw = serde_json::from_str::<serde_json::Value>("bad").unwrap_err();
    let err = make(raw);
    assert!(matches!(err, FileError::Json { .. }));
    assert_eq!(err.path(), Path::new("/cap-json"));
}

#[test]
fn yaml_with_path_builds_yaml_variant_with_captured_path() {
    let make = FileError::yaml_with_path("/cap-yaml");
    let raw = serde_yaml::from_str::<serde_yaml::Value>("[unclosed").unwrap_err();
    let err = make(raw);
    assert!(matches!(err, FileError::Yaml { .. }));
    assert_eq!(err.path(), Path::new("/cap-yaml"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section G — output_style::builtin_styles catalog
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn builtin_styles_includes_all_documented_5_presets() {
    let styles = builtin_styles();
    let names: Vec<&str> = styles.iter().map(|(n, _)| *n).collect();
    for expected in &["concise", "detailed", "minimal", "educational", "code-only"] {
        assert!(
            names.contains(expected),
            "builtin style {expected:?} MUST be present; got {names:?}"
        );
    }
    assert!(
        styles.len() >= 5,
        "MUST have at least 5 documented presets; got {}",
        styles.len()
    );
}

#[test]
fn every_builtin_style_has_non_empty_prompt_text() {
    for (name, prompt) in builtin_styles() {
        assert!(
            !prompt.is_empty(),
            "style {name:?} MUST have non-empty prompt"
        );
        assert!(
            prompt.len() >= 20,
            "style {name:?} prompt MUST be substantive (>=20 chars); got {} chars",
            prompt.len()
        );
    }
}

#[test]
fn builtin_styles_names_are_pairwise_distinct() {
    let styles = builtin_styles();
    let mut names: Vec<&str> = styles.iter().map(|(n, _)| *n).collect();
    let n = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), n, "preset names MUST be pairwise distinct");
}

#[test]
fn builtin_styles_names_are_lowercase_kebab_friendly() {
    for (name, _) in builtin_styles() {
        assert_eq!(
            name,
            name.to_lowercase().as_str(),
            "preset name MUST be lowercase"
        );
        // Allow alphanumeric + dash.
        assert!(
            name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
            "preset name {name:?} MUST be kebab-friendly (alnum + dash)"
        );
    }
}
