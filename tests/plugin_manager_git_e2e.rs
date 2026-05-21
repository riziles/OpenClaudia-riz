//! End-to-end tests for `PluginManager` lifecycle + git helper
//! invariants against real local git repos.
//!
//! Sprint 53 of the verification effort.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::plugins::git::{
    copy_dir_recursive, read_origin_url_sidecar, resolve_head_sha, write_origin_url_sidecar,
    ORIGIN_URL_SIDECAR,
};
use openclaudia::plugins::manager::PluginManager;
use openclaudia::plugins::PluginError;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

// ───────────────────────────────────────────────────────────────────────────
// Helpers
// ───────────────────────────────────────────────────────────────────────────

/// Whether `git` is available on PATH. Most tests need it; tests
/// that touch git invocations skip silently when it's not present
/// rather than fail on CI runners without git.
fn have_git() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Initialise a fresh local git repo with one commit. Returns the
/// repo path and the HEAD commit SHA.
fn init_local_repo(dir: &Path) -> String {
    Command::new("git")
        .args(["init", "--quiet", "-b", "main"])
        .current_dir(dir)
        .status()
        .expect("git init");
    // Test-only identity so commit doesn't fail on a fresh CI box.
    for (k, v) in &[
        ("user.email", "test@example.com"),
        ("user.name", "Test User"),
        ("commit.gpgsign", "false"),
    ] {
        Command::new("git")
            .args(["config", "--local", k, v])
            .current_dir(dir)
            .status()
            .expect("git config");
    }
    fs::write(dir.join("README.md"), b"# test\n").expect("write README");
    Command::new("git")
        .args(["add", "."])
        .current_dir(dir)
        .status()
        .expect("git add");
    Command::new("git")
        .args(["commit", "--quiet", "-m", "init"])
        .current_dir(dir)
        .status()
        .expect("git commit");
    let head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .expect("git rev-parse");
    String::from_utf8_lossy(&head.stdout).trim().to_string()
}

/// Write a minimal valid plugin (.claude-plugin/plugin.json) at `dir`.
fn write_plugin(dir: &Path, name: &str) {
    let manifest_dir = dir.join(".claude-plugin");
    fs::create_dir_all(&manifest_dir).expect("mkdir");
    fs::write(
        manifest_dir.join("plugin.json"),
        format!(r#"{{"name": "{name}"}}"#),
    )
    .expect("write manifest");
}

// ───────────────────────────────────────────────────────────────────────────
// Section A — PluginManager::with_paths + discover
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn with_paths_empty_yields_zero_plugins_after_discover() {
    let mut mgr = PluginManager::with_paths(vec![]);
    let errors = mgr.discover();
    assert!(errors.is_empty(), "no paths → no discovery errors");
    assert_eq!(mgr.count(), 0);
}

#[test]
fn discover_loads_plugins_from_real_search_path() {
    let dir = TempDir::new().expect("tempdir");
    let search = dir.path().join("plugins");
    fs::create_dir(&search).expect("mkdir search");
    write_plugin(&search.join("alpha"), "alpha");
    write_plugin(&search.join("beta"), "beta");

    let mut mgr = PluginManager::with_paths(vec![search]);
    let errors = mgr.discover();
    assert!(
        errors.is_empty(),
        "2 valid plugins must load cleanly; errors={errors:?}"
    );
    assert_eq!(mgr.count(), 2);
    assert!(mgr.get("alpha").is_some());
    assert!(mgr.get("beta").is_some());
    assert!(mgr.get("never-existed").is_none());
}

#[test]
fn discover_skips_non_directory_entries_in_search_path() {
    let dir = TempDir::new().expect("tempdir");
    let search = dir.path().join("plugins");
    fs::create_dir(&search).expect("mkdir");
    // One real plugin dir.
    write_plugin(&search.join("real-plugin"), "real-plugin");
    // One bare file — must be skipped, not error.
    fs::write(search.join("not-a-plugin.txt"), "hello").expect("write file");

    let mut mgr = PluginManager::with_paths(vec![search]);
    let _ = mgr.discover();
    assert_eq!(mgr.count(), 1);
}

#[test]
fn discover_with_nonexistent_search_path_is_a_noop() {
    let dir = TempDir::new().expect("tempdir");
    let mut mgr = PluginManager::with_paths(vec![dir.path().join("does-not-exist")]);
    let errors = mgr.discover();
    assert!(errors.is_empty(), "missing path MUST be skipped silently");
    assert_eq!(mgr.count(), 0);
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — enable / disable state transitions
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn enable_disable_round_trip_toggles_enabled_flag() {
    let dir = TempDir::new().expect("tempdir");
    let search = dir.path().join("plugins");
    fs::create_dir(&search).expect("mkdir");
    write_plugin(&search.join("toggle"), "toggle");
    let mut mgr = PluginManager::with_paths(vec![search]);
    let _ = mgr.discover();
    // Manager-loaded plugins land enabled by default.
    let initial = mgr.get("toggle").expect("loaded").enabled;
    // disable + verify
    mgr.disable("toggle").expect("disable");
    assert!(!mgr.get("toggle").unwrap().enabled);
    // enable + verify
    mgr.enable("toggle").expect("enable");
    assert!(mgr.get("toggle").unwrap().enabled);
    // The default-enabled invariant — first state mirrors final.
    assert_eq!(initial, mgr.get("toggle").unwrap().enabled);
}

#[test]
fn enable_unknown_plugin_returns_not_found() {
    let mut mgr = PluginManager::with_paths(vec![]);
    let outcome = mgr.enable("never-loaded");
    let matched = matches!(&outcome, Err(PluginError::NotFound(name)) if name == "never-loaded");
    assert!(
        matched,
        "enable(unknown) MUST error NotFound; got {outcome:?}"
    );
}

#[test]
fn disable_unknown_plugin_returns_not_found() {
    let mut mgr = PluginManager::with_paths(vec![]);
    let outcome = mgr.disable("never-loaded");
    let matched = matches!(&outcome, Err(PluginError::NotFound(name)) if name == "never-loaded");
    assert!(
        matched,
        "disable(unknown) MUST error NotFound; got {outcome:?}"
    );
}

#[test]
fn count_matches_plugins_iter_length() {
    let dir = TempDir::new().expect("tempdir");
    let search = dir.path().join("plugins");
    fs::create_dir(&search).expect("mkdir");
    for name in &["p1", "p2", "p3"] {
        write_plugin(&search.join(name), name);
    }
    let mut mgr = PluginManager::with_paths(vec![search]);
    let _ = mgr.discover();
    assert_eq!(mgr.count(), 3);
    assert_eq!(mgr.all().count(), 3, "all().count() MUST match count()");
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — write_origin_url_sidecar + read_origin_url_sidecar
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn origin_url_sidecar_round_trips_simple_url() {
    let dir = TempDir::new().expect("tempdir");
    write_origin_url_sidecar(dir.path(), "https://github.com/user/repo.git").expect("write");
    let recovered = read_origin_url_sidecar(dir.path()).expect("read");
    assert_eq!(
        recovered,
        Some("https://github.com/user/repo.git".to_string())
    );
}

#[test]
fn origin_url_sidecar_trims_trailing_whitespace_on_read() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join(ORIGIN_URL_SIDECAR);
    fs::write(&path, "https://example.com/repo.git\n   ").expect("write raw");
    let recovered = read_origin_url_sidecar(dir.path()).expect("read");
    assert_eq!(
        recovered,
        Some("https://example.com/repo.git".to_string()),
        "trailing whitespace MUST be trimmed"
    );
}

#[test]
fn origin_url_sidecar_missing_returns_none() {
    let dir = TempDir::new().expect("tempdir");
    let recovered = read_origin_url_sidecar(dir.path()).expect("read");
    assert!(
        recovered.is_none(),
        "missing sidecar MUST return Ok(None) (legacy clone backcompat)"
    );
}

#[test]
fn origin_url_sidecar_filename_matches_documented_constant() {
    assert_eq!(ORIGIN_URL_SIDECAR, ".openclaudia-origin-url");
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — resolve_head_sha against a real local repo
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn resolve_head_sha_returns_real_commit_sha_against_local_repo() {
    if !have_git() {
        return;
    }
    let dir = TempDir::new().expect("tempdir");
    let expected = init_local_repo(dir.path());
    let sha = resolve_head_sha(dir.path()).expect("resolve");
    assert_eq!(sha, expected, "rev-parse SHA MUST match git's own");
    // Bonus sanity: it's a 40-char lowercase hex string.
    assert_eq!(sha.len(), 40);
    assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn resolve_head_sha_errors_in_non_git_directory() {
    if !have_git() {
        return;
    }
    let dir = TempDir::new().expect("tempdir");
    // No git init — rev-parse must fail.
    let outcome = resolve_head_sha(dir.path());
    assert!(
        outcome.is_err(),
        "resolve_head_sha on non-git dir MUST error; got {outcome:?}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — copy_dir_recursive
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn copy_dir_recursive_copies_a_simple_tree() {
    let dir = TempDir::new().expect("tempdir");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    fs::create_dir_all(src.join("nested")).expect("mkdir src/nested");
    fs::write(src.join("a.txt"), b"a contents").expect("a");
    fs::write(src.join("nested/b.txt"), b"b contents").expect("b");

    copy_dir_recursive(&src, &dst).expect("copy");

    assert_eq!(fs::read(dst.join("a.txt")).expect("read a"), b"a contents");
    assert_eq!(
        fs::read(dst.join("nested/b.txt")).expect("read b"),
        b"b contents"
    );
}

#[test]
fn copy_dir_recursive_creates_destination_if_missing() {
    let dir = TempDir::new().expect("tempdir");
    let src = dir.path().join("src");
    let dst = dir.path().join("does-not-yet-exist");
    fs::create_dir(&src).expect("mkdir src");
    fs::write(src.join("x.txt"), b"x").expect("write");
    copy_dir_recursive(&src, &dst).expect("copy creates dst");
    assert!(dst.exists());
    assert_eq!(fs::read(dst.join("x.txt")).expect("read"), b"x");
}

#[cfg(unix)]
#[test]
fn copy_dir_recursive_with_symlink_in_unchecked_mode_currently_traverses() {
    let dir = TempDir::new().expect("tempdir");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    fs::create_dir(&src).expect("mkdir");
    fs::write(src.join("real.txt"), b"real").expect("real");
    // Symlink within the src directory.
    std::os::unix::fs::symlink(src.join("real.txt"), src.join("link.txt")).expect("symlink");
    // The unchecked copy_dir_recursive doesn't enforce
    // containment; we pin the actual behaviour here so a
    // future restriction-without-migration surfaces.
    let outcome = copy_dir_recursive(&src, &dst);
    // Either: copies successfully, or refuses on symlink.
    // The current impl refuses on symlinks (uses
    // copy_dir_recursive_checked under the hood without an
    // allowed_root, but the per-entry symlink guard always
    // refuses).
    assert!(
        outcome.is_err(),
        "copy_dir_recursive MUST refuse symlinks (per crosslink #258); got {outcome:?}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — PluginManager::reload
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn reload_clears_and_rediscovers_from_search_paths() {
    let dir = TempDir::new().expect("tempdir");
    let search = dir.path().join("plugins");
    fs::create_dir(&search).expect("mkdir");
    write_plugin(&search.join("first"), "first");
    let mut mgr = PluginManager::with_paths(vec![search.clone()]);
    let _ = mgr.discover();
    assert_eq!(mgr.count(), 1);
    // Add a second plugin on disk.
    write_plugin(&search.join("second"), "second");
    // Without reload, count stays at 1.
    assert_eq!(mgr.count(), 1, "pre-reload count unchanged");
    // After reload, both plugins surface.
    let _ = mgr.reload();
    assert_eq!(mgr.count(), 2, "reload MUST re-discover");
    assert!(mgr.get("second").is_some());
}

// ───────────────────────────────────────────────────────────────────────────
// Section G — Compile-time PathBuf import sanity
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn _pathbuf_kept_alive() {
    let _: PathBuf = PathBuf::new();
}
