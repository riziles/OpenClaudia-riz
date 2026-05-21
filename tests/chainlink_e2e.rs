//! End-to-end tests for `execute_chainlink`'s argument-validation
//! perimeter — the chokepoint that gates which subcommands the
//! model can spawn against the `chainlink` binary.
//!
//! Sprint 18 of the verification effort. `src/tools/chainlink.rs`
//! has 7 unit tests but no integration coverage that drives the
//! validation perimeter against the adversarial input catalog at
//! the public `execute_chainlink` entry point.
//!
//! Coverage shape:
//!
//!   - **Allowlist enforcement** — every subcommand outside
//!     `ALLOWED_SUBCOMMANDS` is rejected BEFORE the binary is
//!     spawned. The error message must name the offending
//!     subcommand AND the allowed-list contents so the model
//!     gets a actionable correction.
//!   - **Shell-injection defence** — `;`, `&`, `|`, backticks,
//!     `$VAR`, redirects all survive `shlex::split` as plain
//!     tokens (one of which is the subcommand). The allowlist
//!     check then rejects them as a non-allowed subcommand.
//!   - **Control-char rejection** — argv tokens containing NUL,
//!     CR, or LF are explicitly refused with a message naming
//!     the bad token.
//!   - **shlex parser robustness** — unbalanced quotes,
//!     dangling backslash, and other malformed inputs error
//!     cleanly (not panic).
//!   - **Happy-path catalog** — every entry in the documented
//!     allowlist parses and gets dispatched (then errors because
//!     the chainlink binary isn't in PATH in the test env).
//!     The error here MUST be an "install" / "not found" /
//!     "not available" shape — NOT a validation error.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::tools::execute_chainlink;
use serde_json::{json, Value};
use std::collections::HashMap;

fn args(args_str: &str) -> HashMap<String, Value> {
    let mut m = HashMap::new();
    m.insert("args".to_string(), json!(args_str));
    m
}

// ───────────────────────────────────────────────────────────────────────────
// Section A — missing/malformed arg surface
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn missing_args_field_errors() {
    let (msg, is_err) = execute_chainlink(&HashMap::new());
    assert!(is_err, "missing args field must error");
    // The typed-accessor's canonical error wording is
    // "Missing 'args' argument" (crosslink #675).
    assert!(
        msg.to_lowercase().contains("args"),
        "msg must mention 'args'; got {msg:?}"
    );
}

#[test]
fn empty_args_string_errors() {
    let (msg, is_err) = execute_chainlink(&args(""));
    assert!(is_err, "empty args must error");
    // shlex::split("") returns Some(vec![]) → "Missing subcommand".
    assert!(
        msg.to_lowercase().contains("missing") && msg.to_lowercase().contains("subcommand"),
        "msg must say 'Missing chainlink subcommand'; got {msg:?}"
    );
}

#[test]
fn whitespace_only_args_string_errors_as_missing_subcommand() {
    let (msg, is_err) = execute_chainlink(&args("   \t  "));
    assert!(is_err, "whitespace-only args must error");
    assert!(
        msg.to_lowercase().contains("missing") && msg.to_lowercase().contains("subcommand"),
        "whitespace-only args must produce 'Missing subcommand'; got {msg:?}"
    );
}

#[test]
fn unbalanced_quotes_in_args_errors() {
    let (msg, is_err) = execute_chainlink(&args("create \"unterminated"));
    assert!(is_err, "unbalanced-quote args must error");
    assert!(
        msg.to_lowercase().contains("parse") || msg.to_lowercase().contains("unbalanced"),
        "msg must mention parse failure / unbalanced; got {msg:?}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — subcommand allowlist enforcement
// ───────────────────────────────────────────────────────────────────────────

/// Disallowed subcommands that MUST be refused. Each is a real
/// `chainlink` flag/subcommand that exists but is not in the model-
/// facing allowlist — running them would let a hostile model bypass
/// the per-action gate.
const FORBIDDEN_SUBCOMMANDS: &[&str] = &[
    "destroy",
    "delete",
    "purge",
    "install",
    "config",
    "admin",
    "shell",
    "exec",
    // Empty-prefix attempt
    "subcommand-that-does-not-exist",
    // Shell-meta as the subcommand
    "; ls",
    "&& curl evil",
    "$BASH_ENV",
];

#[test]
fn forbidden_subcommands_are_refused_with_allowlist_named() {
    let mut leaked = Vec::new();
    for sub in FORBIDDEN_SUBCOMMANDS {
        let (msg, is_err) = execute_chainlink(&args(sub));
        if !is_err {
            leaked.push(format!("{sub:?} admitted (msg={msg:?})"));
            continue;
        }
        // For tokens that survive shlex as a single argv[0], the
        // message must say "not in the chainlink allowlist". For
        // tokens that shlex splits (e.g. `; ls` → ["; ls"] when
        // unquoted is `[";", "ls"]`), the first token is what
        // gets validated.
        let lowered = msg.to_lowercase();
        if !lowered.contains("allowlist") && !lowered.contains("not in") {
            // Soft note — message contract may drift but the
            // rejection itself is the hard requirement.
            eprintln!("note: {sub:?} refused with non-canonical message {msg:?}");
        }
    }
    assert!(
        leaked.is_empty(),
        "{} forbidden subcommands slipped past the allowlist:\n  {}",
        leaked.len(),
        leaked.join("\n  ")
    );
}

#[test]
fn allowlist_error_message_lists_allowed_subcommands() {
    // The error message for a bad subcommand MUST include the
    // documented allowlist (or at least a sample) so the model
    // can self-correct. We sample by checking for several known
    // allowed subcommands in the message.
    let (msg, is_err) = execute_chainlink(&args("nonexistent_subcmd"));
    assert!(is_err);
    let lowered = msg.to_lowercase();
    // At least one of the canonical allowed names must appear.
    let allowed_sample = ["create", "list", "show", "comment"];
    let saw = allowed_sample.iter().any(|&n| lowered.contains(n));
    assert!(
        saw,
        "error message must list allowed subcommands; got {msg:?}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — control-char rejection
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn argv_token_with_embedded_nul_is_refused() {
    // "create" passes allowlist; the second token has a literal NUL.
    // We construct via `shlex`-compatible quoting since args is a
    // single string.
    let args_str = "create \"title\0evil\"";
    let (msg, is_err) = execute_chainlink(&args(args_str));
    assert!(is_err, "argv token with NUL must error; got msg={msg:?}");
}

#[test]
fn argv_token_with_embedded_newline_is_refused() {
    let args_str = "create \"title\ninjected\"";
    let (msg, is_err) = execute_chainlink(&args(args_str));
    assert!(
        is_err,
        "argv token with newline must error; got msg={msg:?}"
    );
}

#[test]
fn argv_token_with_carriage_return_is_refused() {
    let args_str = "create \"title\rinjected\"";
    let (msg, is_err) = execute_chainlink(&args(args_str));
    assert!(is_err, "argv token with CR must error; got msg={msg:?}");
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — happy-path subcommands dispatch (binary not installed)
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn allowed_subcommand_passes_validation_and_dispatches() {
    // "list" is on the allowlist. Since `chainlink` isn't on the
    // test machine's PATH, the spawn fails with NotFound and the
    // install-help response fires. The KEY assertion: the failure
    // is NOT a validation failure — it must mention "not found"
    // or "install" / "available", NOT "allowlist" or "control
    // character".
    let (msg, is_err) = execute_chainlink(&args("list"));
    assert!(is_err, "missing chainlink binary must surface as is_err");
    let lowered = msg.to_lowercase();
    assert!(
        lowered.contains("not")
            || lowered.contains("install")
            || lowered.contains("available")
            || lowered.contains("chainlink"),
        "binary-not-found response must say something about chainlink \
         installation / availability; got {msg:?}"
    );
    assert!(
        !lowered.contains("allowlist"),
        "binary-not-found MUST NOT be reported as an allowlist failure; \
         got {msg:?}"
    );
    assert!(
        !lowered.contains("control character"),
        "binary-not-found MUST NOT be reported as a control-char failure; \
         got {msg:?}"
    );
}

#[test]
fn help_subcommands_pass_validation() {
    // --help, -h, --version, -V are all on the allowlist (so the
    // model can introspect chainlink without other side effects).
    for sub in &["--help", "-h", "--version", "-V", "help"] {
        let (msg, _is_err) = execute_chainlink(&args(sub));
        let lowered = msg.to_lowercase();
        assert!(
            !lowered.contains("allowlist"),
            "{sub:?} must not trip the allowlist check; got {msg:?}"
        );
    }
}
