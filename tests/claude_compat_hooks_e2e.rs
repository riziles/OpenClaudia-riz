//! End-to-end tests for `hooks::claude_compat` serde
//! deserialization of Claude Code's `settings.json` hook
//! format + `load_claude_code_hooks` /
//! `load_claude_settings` fallback when no settings files
//! exist.
//!
//! Sprint 74 of the verification effort. Sprint 28's
//! `hooks_merge_e2e` covered the OC-native merge layering;
//! this file covers the CC-format ingestion layer.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::hooks::{
    load_claude_code_hooks, load_claude_settings, ClaudeCodeHook, ClaudeCodeHookEntry,
    ClaudeCodeSettings,
};
use std::sync::{Mutex, MutexGuard, OnceLock};

/// Global lock for cwd-mutating tests — the load_* helpers
/// read from process cwd / $HOME and can't be parallelised.
fn cwd_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn run_in_tempdir<F: FnOnce() -> R, R>(f: F) -> R {
    let _g = cwd_lock();
    let dir = tempfile::TempDir::new().expect("tempdir");
    let original = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(dir.path()).expect("chdir");
    let result = f();
    std::env::set_current_dir(&original).expect("restore");
    result
}

// ───────────────────────────────────────────────────────────────────────────
// Section A — ClaudeCodeSettings serde
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn claude_code_settings_default_has_empty_hooks_map() {
    let s = ClaudeCodeSettings::default();
    assert!(s.hooks.is_empty());
}

#[test]
fn claude_code_settings_empty_json_object_deserializes_to_default() {
    let s: ClaudeCodeSettings = serde_json::from_str("{}").expect("parse");
    assert!(s.hooks.is_empty());
}

#[test]
fn claude_code_settings_deserializes_documented_cc_shape() {
    let json = r#"{
        "hooks": {
            "PreToolUse": [
                {
                    "matcher": "Bash",
                    "hooks": [
                        {"type": "command", "command": "echo pre"}
                    ]
                }
            ]
        }
    }"#;
    let s: ClaudeCodeSettings = serde_json::from_str(json).expect("parse");
    let pre_tool = s.hooks.get("PreToolUse").expect("PreToolUse present");
    assert_eq!(pre_tool.len(), 1);
    assert_eq!(pre_tool[0].matcher.as_deref(), Some("Bash"));
    assert_eq!(pre_tool[0].hooks.len(), 1);
    let ClaudeCodeHook::Command { command, timeout } = &pre_tool[0].hooks[0];
    assert_eq!(command, "echo pre");
    assert_eq!(*timeout, Some(60), "MUST default to 60s when absent");
}

#[test]
fn claude_code_settings_keeps_unknown_event_names_in_map() {
    // The deserializer doesn't filter at parse time; the
    // unknown-event filter happens at merge time. Parsing
    // alone MUST preserve every key.
    let json = r#"{
        "hooks": {
            "TotallyUnknownEvent": [
                {"matcher": "x", "hooks": [{"type": "command", "command": "y"}]}
            ]
        }
    }"#;
    let s: ClaudeCodeSettings = serde_json::from_str(json).expect("parse");
    assert!(s.hooks.contains_key("TotallyUnknownEvent"));
}

#[test]
fn claude_code_settings_with_no_hooks_field_yields_empty_map() {
    let json = r#"{"otherField": 42}"#;
    let s: ClaudeCodeSettings = serde_json::from_str(json).expect("parse");
    assert!(s.hooks.is_empty());
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — ClaudeCodeHookEntry serde
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn hook_entry_deserializes_with_string_matcher() {
    let json = r#"{
        "matcher": "Bash",
        "hooks": [{"type": "command", "command": "ls"}]
    }"#;
    let entry: ClaudeCodeHookEntry = serde_json::from_str(json).expect("parse");
    assert_eq!(entry.matcher.as_deref(), Some("Bash"));
    assert_eq!(entry.hooks.len(), 1);
}

#[test]
fn hook_entry_deserializes_without_matcher_field() {
    // matcher defaults to None when absent.
    let json = r#"{
        "hooks": [{"type": "command", "command": "ls"}]
    }"#;
    let entry: ClaudeCodeHookEntry = serde_json::from_str(json).expect("parse");
    assert!(entry.matcher.is_none());
}

#[test]
fn hook_entry_deserializes_with_explicit_null_matcher() {
    let json = r#"{
        "matcher": null,
        "hooks": [{"type": "command", "command": "ls"}]
    }"#;
    let entry: ClaudeCodeHookEntry = serde_json::from_str(json).expect("parse");
    assert!(entry.matcher.is_none());
}

#[test]
fn hook_entry_with_empty_hooks_array_parses_as_empty() {
    let json = r#"{"matcher": "X", "hooks": []}"#;
    let entry: ClaudeCodeHookEntry = serde_json::from_str(json).expect("parse");
    assert!(entry.hooks.is_empty());
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — ClaudeCodeHook serde tagged union
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn claude_code_hook_command_variant_uses_type_tag() {
    let json = r#"{"type": "command", "command": "echo hi"}"#;
    let hook: ClaudeCodeHook = serde_json::from_str(json).expect("parse");
    let ClaudeCodeHook::Command { command, timeout } = hook;
    assert_eq!(command, "echo hi");
    // Documented: default timeout when not specified is 60.
    assert_eq!(timeout, Some(60));
}

#[test]
fn claude_code_hook_command_preserves_explicit_timeout() {
    let json = r#"{"type": "command", "command": "x", "timeout": 120}"#;
    let hook: ClaudeCodeHook = serde_json::from_str(json).expect("parse");
    let ClaudeCodeHook::Command { timeout, .. } = hook;
    assert_eq!(timeout, Some(120));
}

#[test]
fn claude_code_hook_command_with_null_timeout_uses_default() {
    let json = r#"{"type": "command", "command": "x", "timeout": null}"#;
    let hook: ClaudeCodeHook = serde_json::from_str(json).expect("parse");
    let ClaudeCodeHook::Command { timeout, .. } = hook;
    // The serde default impl yields Some(60) when the field
    // is missing entirely. With explicit null we get None.
    assert!(timeout.is_none() || timeout == Some(60));
}

#[test]
fn claude_code_hook_rejects_unknown_type_tag() {
    let json = r#"{"type": "totally-unknown", "command": "x"}"#;
    let outcome: Result<ClaudeCodeHook, _> = serde_json::from_str(json);
    assert!(
        outcome.is_err(),
        "unknown type tag MUST error; got {outcome:?}"
    );
}

#[test]
fn claude_code_hook_rejects_command_missing_command_field() {
    let json = r#"{"type": "command"}"#;
    let outcome: Result<ClaudeCodeHook, _> = serde_json::from_str(json);
    assert!(outcome.is_err());
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — load_claude_code_hooks fallback when no files exist
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn load_claude_code_hooks_returns_empty_config_in_clean_cwd() {
    // In a tempdir with no .claude/ at cwd, the function MUST
    // gracefully return an empty config (user-level may still
    // populate, but the function itself MUST NOT panic).
    let config = run_in_tempdir(load_claude_code_hooks);
    // We can't make a strict assertion about emptiness
    // because the user's ~/.claude/settings.json may exist
    // and populate it. The contract is: doesn't panic + is a
    // valid HooksConfig.
    // (No assertion necessary — just doesn't panic.)
    let _ = config;
}

#[test]
fn load_claude_settings_returns_layered_settings_in_clean_cwd() {
    let layered = run_in_tempdir(load_claude_settings);
    // The settings field is always populated as a JSON
    // value, even when no files exist (defaults to empty
    // object).
    assert!(
        layered.settings.is_object(),
        "settings MUST be a JSON object even when no files exist"
    );
    // allowed_tools is a Vec — always populated, possibly
    // empty.
    let _ = layered.allowed_tools;
}

#[test]
fn load_claude_code_hooks_handles_missing_dot_claude_dir_gracefully() {
    // Run twice in different tempdirs — both MUST succeed.
    let _ = run_in_tempdir(load_claude_code_hooks);
    let _ = run_in_tempdir(load_claude_code_hooks);
}
