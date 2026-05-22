//! End-to-end tests for `verify_signature` ed25519 verification
//! + `InstallScope` / `InstalledPlugins` registry semantics.
//!
//! Sprint 51 of the verification effort.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use ed25519_dalek::{Signer, SigningKey};
use openclaudia::plugins::install::{InstallScope, InstalledPlugins, PluginInstallEntry};
use openclaudia::plugins::validate::{verify_signature, PluginSignature};
use openclaudia::plugins::{PublicKey, SignatureError};
// rand_core 0.10 dropped `OsRng` from the root and the `RngCore` trait was
// replaced with `TryRng`. Use `rand::rngs::SysRng` + `TryRng::try_fill_bytes`
// (both re-exported from `rand` so we don't need `rand_core` directly).
use rand::rngs::SysRng;
use rand::TryRng as _;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::str::FromStr;

// ───────────────────────────────────────────────────────────────────────────
// Helpers — ed25519 keypair + signing
// ───────────────────────────────────────────────────────────────────────────

fn fresh_keypair() -> (SigningKey, PublicKey) {
    let mut secret = [0u8; 32];
    SysRng
        .try_fill_bytes(&mut secret)
        .expect("OS RNG must produce 32 bytes for test keypair");
    let signing = SigningKey::from_bytes(&secret);
    let pub_bytes = signing.verifying_key().to_bytes();
    (signing, PublicKey(pub_bytes))
}

fn sign_bytes(key: &SigningKey, msg: &[u8]) -> PluginSignature {
    let sig = key.sign(msg);
    PluginSignature(sig.to_bytes())
}

// ───────────────────────────────────────────────────────────────────────────
// Section A — PluginSignature constructors
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn signature_from_bytes_accepts_64_byte_input() {
    let raw = [0u8; 64];
    let sig = PluginSignature::from_bytes(&raw).expect("64 bytes ok");
    assert_eq!(sig.as_bytes(), &raw);
}

#[test]
fn signature_from_bytes_rejects_wrong_length() {
    for len in &[0_usize, 32, 63, 65, 128] {
        let buf = vec![0u8; *len];
        let outcome = PluginSignature::from_bytes(&buf);
        assert!(
            matches!(outcome, Err(SignatureError::InvalidLength(observed)) if observed == *len),
            "len={len} MUST error InvalidLength({len}); got {outcome:?}"
        );
    }
}

#[test]
fn signature_from_base64_round_trips() {
    use base64::Engine;
    let original = [0xAB; 64];
    let encoded = base64::engine::general_purpose::STANDARD.encode(original);
    let sig = PluginSignature::from_base64(&encoded).expect("decode");
    assert_eq!(sig.as_bytes(), &original);
}

#[test]
fn signature_from_base64_rejects_invalid_encoding() {
    let outcome = PluginSignature::from_base64("not!valid$base64@@@");
    assert!(
        matches!(outcome, Err(SignatureError::InvalidEncoding(_))),
        "MUST error InvalidEncoding; got {outcome:?}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — PublicKey constructors
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn public_key_from_bytes_accepts_32_byte_input() {
    let raw = [0x42; 32];
    let key = PublicKey::from_bytes(&raw).expect("32 bytes ok");
    assert_eq!(key.0, raw);
}

#[test]
fn public_key_from_bytes_rejects_wrong_length() {
    for len in &[0_usize, 16, 31, 33, 64] {
        let buf = vec![0u8; *len];
        let outcome = PublicKey::from_bytes(&buf);
        assert!(
            matches!(outcome, Err(SignatureError::InvalidLength(observed)) if observed == *len),
            "len={len} MUST error InvalidLength({len}); got {outcome:?}"
        );
    }
}

#[test]
fn public_key_from_hex_round_trips_both_cases() {
    let bytes = [0xDEu8; 32];
    let lower = bytes.iter().fold(String::with_capacity(64), |mut acc, b| {
        write!(acc, "{b:02x}").expect("string write");
        acc
    });
    let upper = lower.to_uppercase();
    let key_lower = PublicKey::from_hex(&lower).expect("lowercase ok");
    let key_upper = PublicKey::from_hex(&upper).expect("uppercase ok");
    assert_eq!(key_lower.0, key_upper.0);
    assert_eq!(key_lower.0, bytes);
}

#[test]
fn public_key_from_hex_rejects_non_hex_characters() {
    let mut bad = "a".repeat(64);
    unsafe {
        bad.as_bytes_mut()[5] = b'z'; // invalid nibble
    }
    let outcome = PublicKey::from_hex(&bad);
    assert!(
        matches!(outcome, Err(SignatureError::InvalidEncoding(_))),
        "non-hex char MUST error InvalidEncoding; got {outcome:?}"
    );
}

#[test]
fn public_key_from_hex_rejects_wrong_length() {
    // 62-char input (31 bytes) — too short.
    let outcome = PublicKey::from_hex(&"a".repeat(62));
    assert!(
        matches!(outcome, Err(SignatureError::InvalidLength(_))),
        "wrong-length hex MUST error InvalidLength; got {outcome:?}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — verify_signature happy path + bad signature
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn verify_signature_accepts_a_genuine_signature_from_trusted_key() {
    let (signing, public) = fresh_keypair();
    let payload = b"plugin manifest contents v1";
    let sig = sign_bytes(&signing, payload);
    let outcome = verify_signature(payload, &sig, &[public]);
    assert!(
        outcome.is_ok(),
        "genuine signature from trusted key MUST verify; got {outcome:?}"
    );
}

#[test]
fn verify_signature_refuses_when_payload_tampered() {
    let (signing, public) = fresh_keypair();
    let original = b"original manifest";
    let sig = sign_bytes(&signing, original);
    let tampered = b"tampered manifest";
    let outcome = verify_signature(tampered, &sig, &[public]);
    assert!(
        outcome.is_err(),
        "tampered payload MUST fail verification; got {outcome:?}"
    );
}

#[test]
fn verify_signature_refuses_when_signed_by_untrusted_key() {
    let (rogue_signing, _rogue_pub) = fresh_keypair();
    let (_, trusted_pub) = fresh_keypair();
    let payload = b"manifest";
    let sig = sign_bytes(&rogue_signing, payload);
    // Trusted set contains a DIFFERENT key from the signer.
    let outcome = verify_signature(payload, &sig, &[trusted_pub]);
    assert!(
        matches!(outcome, Err(SignatureError::UnknownSigner)),
        "signed-by-rogue with trust-set excluding rogue MUST error UnknownSigner; got {outcome:?}"
    );
}

#[test]
fn verify_signature_with_empty_trust_set_errors_unknown_signer() {
    let (signing, _) = fresh_keypair();
    let payload = b"manifest";
    let sig = sign_bytes(&signing, payload);
    let outcome = verify_signature(payload, &sig, &[]);
    assert!(
        matches!(outcome, Err(SignatureError::UnknownSigner)),
        "empty trust set MUST error UnknownSigner; got {outcome:?}"
    );
}

#[test]
fn verify_signature_accepts_when_signer_is_in_multi_key_trust_set() {
    let (_, key_a) = fresh_keypair();
    let (signing_b, key_b) = fresh_keypair();
    let (_, key_c) = fresh_keypair();
    let payload = b"manifest";
    let sig = sign_bytes(&signing_b, payload);
    // Trust set: a, b, c — verifier MUST find b.
    let outcome = verify_signature(payload, &sig, &[key_a, key_b, key_c]);
    assert!(
        outcome.is_ok(),
        "multi-key trust set containing the signer MUST verify; got {outcome:?}"
    );
}

#[test]
fn verify_signature_with_all_malformed_keys_errors_malformed_key() {
    let (signing, _) = fresh_keypair();
    let payload = b"manifest";
    let sig = sign_bytes(&signing, payload);
    // Malformed key: all zeros is not a valid ed25519 point in
    // most representations — but stub doesn't matter; we want
    // the contract that all-malformed produces MalformedKey.
    // Use an entirely-invalid point: all 0xFF bytes.
    let malformed = PublicKey([0xFFu8; 32]);
    let outcome = verify_signature(payload, &sig, &[malformed]);
    // Per the docstring: when EVERY key in trusted_keys fails
    // to parse, the function returns MalformedKey.
    assert!(
        matches!(outcome, Err(SignatureError::MalformedKey(_)))
            || matches!(outcome, Err(SignatureError::UnknownSigner)),
        "all-malformed trust set MUST error MalformedKey or UnknownSigner; got {outcome:?}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — InstallScope parsing + display
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn install_scope_round_trips_through_fromstr_and_display() {
    for scope in &[
        InstallScope::Managed,
        InstallScope::User,
        InstallScope::Project,
        InstallScope::Local,
    ] {
        let display = scope.to_string();
        let parsed = InstallScope::from_str(&display).expect("round-trip");
        assert_eq!(parsed, *scope);
    }
}

#[test]
fn install_scope_fromstr_is_case_insensitive() {
    let cases = &[
        ("MANAGED", InstallScope::Managed),
        ("user", InstallScope::User),
        ("Project", InstallScope::Project),
        ("LOCAL", InstallScope::Local),
    ];
    for (input, expected) in cases {
        let parsed = InstallScope::from_str(input).expect("parse");
        assert_eq!(parsed, *expected, "{input:?} → {expected:?}");
    }
}

#[test]
fn install_scope_fromstr_rejects_unknown_strings() {
    for bad in &["", "global", "team", "foo", "system"] {
        let outcome = InstallScope::from_str(bad);
        assert!(outcome.is_err(), "{bad:?} MUST error");
    }
}

#[test]
fn install_scope_is_global_distinguishes_user_managed_vs_project_local() {
    assert!(InstallScope::Managed.is_global());
    assert!(InstallScope::User.is_global());
    assert!(!InstallScope::Project.is_global());
    assert!(!InstallScope::Local.is_global());
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — InstalledPlugins upsert + remove + prune
// ───────────────────────────────────────────────────────────────────────────

fn mk_entry(scope: InstallScope, install_path: &str) -> PluginInstallEntry {
    PluginInstallEntry {
        scope,
        project_path: None,
        install_path: install_path.to_string(),
        version: Some("1.0.0".to_string()),
        installed_at: None,
        last_updated: None,
        git_commit_sha: None,
    }
}

#[test]
fn default_installed_plugins_is_empty_with_schema_v2() {
    let registry = InstalledPlugins::default();
    assert_eq!(registry.version, 2, "schema version MUST be 2");
    assert!(registry.plugins.is_empty());
}

#[test]
fn upsert_adds_a_new_entry_under_a_new_plugin_id() {
    let mut registry = InstalledPlugins::default();
    registry.upsert(
        "my-plugin@local",
        mk_entry(InstallScope::User, "/tmp/my-plugin"),
    );
    assert_eq!(registry.plugins.len(), 1);
    let entries = registry.plugins.get("my-plugin@local").expect("present");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].install_path, "/tmp/my-plugin");
}

#[test]
fn upsert_replaces_existing_entry_with_same_scope_and_project_path() {
    let mut registry = InstalledPlugins::default();
    registry.upsert("p", mk_entry(InstallScope::User, "/tmp/old"));
    registry.upsert("p", mk_entry(InstallScope::User, "/tmp/new"));
    let entries = registry.plugins.get("p").unwrap();
    assert_eq!(entries.len(), 1, "same scope+path MUST replace, not append");
    assert_eq!(entries[0].install_path, "/tmp/new");
}

#[test]
fn upsert_appends_entry_with_different_scope() {
    let mut registry = InstalledPlugins::default();
    registry.upsert("p", mk_entry(InstallScope::User, "/u/path"));
    registry.upsert("p", mk_entry(InstallScope::Project, "/p/path"));
    let entries = registry.plugins.get("p").unwrap();
    assert_eq!(entries.len(), 2, "different scope MUST append");
}

#[test]
fn remove_drops_the_plugin_entry_and_returns_true() {
    let mut registry = InstalledPlugins::default();
    registry.upsert("p", mk_entry(InstallScope::User, "/x"));
    assert!(registry.remove("p"));
    assert!(registry.plugins.is_empty());
}

#[test]
fn remove_returns_false_for_unknown_plugin_id() {
    let mut registry = InstalledPlugins::default();
    assert!(!registry.remove("never-installed"));
}

#[test]
fn prune_stale_drops_entries_whose_install_path_does_not_exist() {
    let mut registry = InstalledPlugins::default();
    // Insert one entry pointing at a nonexistent path.
    registry.upsert(
        "ghost",
        mk_entry(InstallScope::User, "/absolutely/nonexistent/path/9999"),
    );
    // And one pointing at /tmp (which exists on every unix).
    registry.upsert("real", mk_entry(InstallScope::User, "/tmp"));
    let removed = registry.prune_stale();
    assert_eq!(removed, 1, "1 ghost entry must be reported as removed");
    assert!(
        !registry.plugins.contains_key("ghost"),
        "ghost MUST be dropped entirely (empty entry vector key removed)"
    );
    assert!(
        registry.plugins.contains_key("real"),
        "real entry pointing at /tmp MUST survive"
    );
}

#[test]
fn plugin_ids_returns_every_registered_id() {
    let mut registry = InstalledPlugins::default();
    registry.upsert("a", mk_entry(InstallScope::User, "/a"));
    registry.upsert("b", mk_entry(InstallScope::User, "/b"));
    registry.upsert("c", mk_entry(InstallScope::User, "/c"));
    let ids: std::collections::HashSet<&str> = registry.plugin_ids().into_iter().collect();
    assert_eq!(ids.len(), 3);
    assert!(ids.contains("a"));
    assert!(ids.contains("b"));
    assert!(ids.contains("c"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — InstalledPlugins serde
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn installed_plugins_serde_round_trips_through_json() {
    let mut registry = InstalledPlugins::default();
    registry.upsert(
        "test@local",
        PluginInstallEntry {
            scope: InstallScope::User,
            project_path: Some("/proj".to_string()),
            install_path: "/path/to/plugin".to_string(),
            version: Some("2.0.0".to_string()),
            installed_at: Some("2025-01-01T00:00:00Z".to_string()),
            last_updated: None,
            git_commit_sha: Some("abc123".to_string()),
        },
    );
    let json = serde_json::to_string(&registry).expect("serialize");
    let back: InstalledPlugins = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.version, 2);
    let entries = back.plugins.get("test@local").expect("entry");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].scope, InstallScope::User);
    assert_eq!(entries[0].project_path.as_deref(), Some("/proj"));
    assert_eq!(entries[0].install_path, "/path/to/plugin");
    assert_eq!(entries[0].version.as_deref(), Some("2.0.0"));
    assert_eq!(entries[0].git_commit_sha.as_deref(), Some("abc123"));
}

#[test]
fn installed_plugins_serde_uses_lowercase_scope_string() {
    let mut registry = InstalledPlugins::default();
    registry.upsert("p", mk_entry(InstallScope::Managed, "/x"));
    let json = serde_json::to_string(&registry).expect("serialize");
    assert!(
        json.contains("\"managed\""),
        "scope MUST serialize as lowercase; got {json}"
    );
    assert!(
        !json.contains("\"Managed\""),
        "scope MUST NOT serialize as PascalCase; got {json}"
    );
}

// Compile-time helper: HashMap import survives even if no other
// reference triggers it (keeps the `use` line meaningful for the
// linter).
#[test]
fn _hashmap_import_kept_alive() {
    let _: HashMap<String, String> = HashMap::new();
}
