//! End-to-end tests for the `chainlink` tool dispatched
//! through the registry — pre-binary-spawn validation of
//! `args` (#265, #277, #675).
//!
//! Sprint 148 of the verification effort. Sprint 17
//! covered direct `execute_chainlink` calls plus the
//! allowlist matrix in sprint 102; this file pins the
//! registry-dispatched path so the wire-facing contract
//! matches.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::tools::registry::{registry, ToolContext};
use serde_json::{json, Value};
use std::collections::HashMap;

fn dispatch_chainlink(args: &HashMap<String, Value>) -> (String, bool) {
    let mut ctx = ToolContext {
        memory_db: None,
        app_config: None,
        task_mgr: None,
    };
    registry()
        .dispatch("chainlink", args, &mut ctx)
        .expect("chainlink must be registered")
}

fn args_with(entries: &[(&str, Value)]) -> HashMap<String, Value> {
    let mut m = HashMap::new();
    for (k, v) in entries {
        m.insert((*k).to_string(), v.clone());
    }
    m
}

// ───────────────────────────────────────────────────────────────────────────
// Section A — Missing / wrong-type args field
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn missing_args_field_returns_error() {
    let (msg, is_err) = dispatch_chainlink(&HashMap::new());
    assert!(is_err);
    // Some error MUST surface naming the missing field.
    assert!(
        msg.to_lowercase().contains("args") || msg.contains("missing"),
        "MUST surface missing-args error; got {msg:?}"
    );
}

#[test]
fn args_as_number_treated_as_missing() {
    let args = args_with(&[("args", json!(42))]);
    let (msg, is_err) = dispatch_chainlink(&args);
    assert!(is_err);
    assert!(msg.to_lowercase().contains("args") || msg.contains("missing"));
}

#[test]
fn args_as_array_treated_as_missing() {
    let args = args_with(&[("args", json!(["create", "x"]))]);
    let (msg, is_err) = dispatch_chainlink(&args);
    assert!(is_err);
    assert!(msg.to_lowercase().contains("args") || msg.contains("missing"));
}

#[test]
fn args_as_null_treated_as_missing() {
    let args = args_with(&[("args", Value::Null)]);
    let (msg, is_err) = dispatch_chainlink(&args);
    assert!(is_err);
    assert!(msg.to_lowercase().contains("args") || msg.contains("missing"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — Empty args string
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn empty_args_string_returns_missing_subcommand_error() {
    let args = args_with(&[("args", json!(""))]);
    let (msg, is_err) = dispatch_chainlink(&args);
    assert!(is_err);
    assert!(
        msg.contains("Missing chainlink subcommand"),
        "MUST surface documented empty-args message; got {msg:?}"
    );
}

#[test]
fn whitespace_only_args_string_returns_missing_subcommand_error() {
    let args = args_with(&[("args", json!("   \t  "))]);
    let (msg, is_err) = dispatch_chainlink(&args);
    assert!(is_err);
    assert!(
        msg.contains("Missing chainlink subcommand"),
        "MUST surface missing-subcommand; got {msg:?}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — Shlex unbalanced quotes
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn unbalanced_double_quote_returns_parse_error() {
    let args = args_with(&[("args", json!("create \"unclosed quote"))]);
    let (msg, is_err) = dispatch_chainlink(&args);
    assert!(is_err);
    assert!(
        msg.contains("Could not parse") || msg.contains("unbalanced"),
        "MUST surface shlex parse error; got {msg:?}"
    );
}

#[test]
fn unbalanced_single_quote_returns_parse_error() {
    let args = args_with(&[("args", json!("create 'unclosed"))]);
    let (msg, is_err) = dispatch_chainlink(&args);
    assert!(is_err);
    assert!(
        msg.contains("Could not parse") || msg.contains("unbalanced"),
        "MUST surface shlex parse error; got {msg:?}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — Subcommand allowlist (#265, #277)
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn unknown_subcommand_rejected_with_documented_message() {
    let args = args_with(&[("args", json!("not_a_real_subcommand"))]);
    let (msg, is_err) = dispatch_chainlink(&args);
    assert!(is_err);
    assert!(
        msg.contains("not in the chainlink allowlist"),
        "MUST surface allowlist refusal; got {msg:?}"
    );
    assert!(
        msg.contains("not_a_real_subcommand"),
        "MUST echo offending subcommand; got {msg:?}"
    );
    // Documented allowlist members MUST appear in error so model
    // can self-correct.
    assert!(
        msg.contains("create") && msg.contains("close") && msg.contains("list"),
        "MUST list allowed subcommands; got {msg:?}"
    );
}

#[test]
fn shell_metacharacter_subcommand_rejected() {
    // Shell-injection style subcommand: not in allowlist.
    let args = args_with(&[("args", json!("rm"))]);
    let (msg, is_err) = dispatch_chainlink(&args);
    assert!(is_err);
    assert!(msg.contains("not in the chainlink allowlist"));
}

#[test]
fn case_sensitive_allowlist_uppercase_create_rejected() {
    // PINS DOC: allowlist is case-sensitive.
    let args = args_with(&[("args", json!("CREATE \"x\""))]);
    let (msg, is_err) = dispatch_chainlink(&args);
    assert!(is_err);
    assert!(msg.contains("not in the chainlink allowlist"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — Control character rejection in argv tokens
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn argv_token_with_embedded_null_byte_rejected() {
    let args = args_with(&[("args", json!("create \"foo\0bar\""))]);
    let (msg, is_err) = dispatch_chainlink(&args);
    assert!(is_err);
    assert!(
        msg.contains("control character") || msg.contains("Rejected argv"),
        "MUST surface control-char rejection; got {msg:?}"
    );
}

#[test]
fn argv_token_with_embedded_newline_rejected() {
    let args = args_with(&[("args", json!("create \"foo\nbar\""))]);
    let (msg, is_err) = dispatch_chainlink(&args);
    assert!(is_err);
    assert!(
        msg.contains("control character") || msg.contains("Rejected argv"),
        "MUST surface control-char rejection; got {msg:?}"
    );
}

#[test]
fn argv_token_with_carriage_return_rejected() {
    let args = args_with(&[("args", json!("create \"foo\rbar\""))]);
    let (msg, is_err) = dispatch_chainlink(&args);
    assert!(is_err);
    assert!(msg.contains("control character") || msg.contains("Rejected argv"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — Allowlist coverage (canonical names pass validation)
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn all_documented_subcommands_pass_allowlist_gate() {
    // These reach the binary-spawn step. With chainlink unavailable
    // they error there, but MUST NOT error with allowlist message.
    let documented = [
        "create", "close", "reopen", "comment", "label", "unlabel", "list", "show", "search",
        "subissue", "relate", "block", "unblock", "session", "next",
    ];
    for sub in documented {
        let args = args_with(&[("args", json!(sub))]);
        let (msg, _is_err) = dispatch_chainlink(&args);
        assert!(
            !msg.contains("not in the chainlink allowlist"),
            "{sub} MUST pass allowlist gate; got {msg:?}"
        );
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section G — Forward-compat
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn chainlink_dispatch_never_panics_on_arbitrary_extra_args() {
    let args = args_with(&[
        ("args", json!("list")),
        ("extra", json!({"k": "v"})),
        ("nested", json!([1, 2, 3])),
    ]);
    let (_msg, _is_err) = dispatch_chainlink(&args);
}

#[test]
fn chainlink_tool_registered_in_registry() {
    assert!(registry().get("chainlink").is_some());
}
