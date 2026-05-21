//! End-to-end tests for `MarketplaceManifest` source-variant
//! deserialization + `ZipCache` content-addressed archive store.
//!
//! Sprint 50 of the verification effort.

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::plugins::zip_cache::{sha256_hex, CacheEntry, ZipCache, ZipCacheError};
use openclaudia::plugins::{MarketplaceManifest, MarketplacePlugin, PluginSource, PluginSourceDef};
use std::collections::BTreeMap;
use tempfile::TempDir;

// ───────────────────────────────────────────────────────────────────────────
// Section A — PluginSource variants
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn plugin_source_as_path_string_parses_as_path_variant() {
    let json = r#""./relative/path""#;
    let source: PluginSource = serde_json::from_str(json).expect("parse");
    match source {
        PluginSource::Path(p) => assert_eq!(p, "./relative/path"),
        PluginSource::Structured(s) => panic!("expected Path, got Structured({s:?})"),
    }
}

#[test]
fn plugin_source_as_npm_object_parses_with_required_package_field() {
    let json = r#"{"source": "npm", "package": "@scope/my-plugin", "version": "^1.0.0"}"#;
    let source: PluginSource = serde_json::from_str(json).expect("parse");
    match source {
        PluginSource::Structured(PluginSourceDef::Npm(npm)) => {
            assert_eq!(npm.package, "@scope/my-plugin");
            assert_eq!(npm.version.as_deref(), Some("^1.0.0"));
            assert!(npm.registry.is_none());
        }
        other => panic!("expected Npm, got {other:?}"),
    }
}

#[test]
fn plugin_source_npm_rejects_typo_via_deny_unknown_fields() {
    // `packagee` typo MUST error rather than default `package`
    // to None and silently fail at install time.
    let json = r#"{"source": "npm", "packagee": "wrong-key"}"#;
    let outcome: Result<PluginSource, _> = serde_json::from_str(json);
    assert!(
        outcome.is_err(),
        "NPM source with typo'd field MUST error; got {outcome:?}"
    );
}

#[test]
fn plugin_source_as_pip_object_parses() {
    let json = r#"{"source": "pip", "package": "anthropic-plugin", "version": "1.2.3"}"#;
    let source: PluginSource = serde_json::from_str(json).expect("parse");
    match source {
        PluginSource::Structured(PluginSourceDef::Pip(pip)) => {
            assert_eq!(pip.package, "anthropic-plugin");
            assert_eq!(pip.version.as_deref(), Some("1.2.3"));
        }
        other => panic!("expected Pip, got {other:?}"),
    }
}

#[test]
fn plugin_source_as_url_object_parses_with_git_ref() {
    let json = r#"{"source": "url", "url": "https://github.com/u/repo.git", "ref": "main"}"#;
    let source: PluginSource = serde_json::from_str(json).expect("parse");
    match source {
        PluginSource::Structured(PluginSourceDef::Url(url)) => {
            assert_eq!(url.url, "https://github.com/u/repo.git");
            assert_eq!(url.git_ref.as_deref(), Some("main"));
        }
        other => panic!("expected Url, got {other:?}"),
    }
}

#[test]
fn plugin_source_as_github_object_parses_with_owner_repo_form() {
    let json = r#"{"source": "github", "repo": "openai/gpt-utils", "ref": "v1.0"}"#;
    let source: PluginSource = serde_json::from_str(json).expect("parse");
    match source {
        PluginSource::Structured(PluginSourceDef::GitHub(gh)) => {
            assert_eq!(gh.repo, "openai/gpt-utils");
            assert_eq!(gh.git_ref.as_deref(), Some("v1.0"));
        }
        other => panic!("expected GitHub, got {other:?}"),
    }
}

#[test]
fn plugin_source_unknown_discriminator_errors() {
    let json = r#"{"source": "ftp", "url": "ftp://x"}"#;
    let outcome: Result<PluginSource, _> = serde_json::from_str(json);
    assert!(
        outcome.is_err(),
        "unknown source discriminator MUST error; got {outcome:?}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — MarketplaceManifest serde
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn marketplace_manifest_with_path_source_round_trips() {
    let json = r#"{
        "name": "test-marketplace",
        "owner": {"name": "Alice"},
        "plugins": [
            {
                "name": "local-plugin",
                "source": "./plugins/local"
            }
        ]
    }"#;
    let manifest: MarketplaceManifest = serde_json::from_str(json).expect("parse");
    assert_eq!(manifest.name, "test-marketplace");
    assert_eq!(manifest.owner.name, "Alice");
    assert_eq!(manifest.plugins.len(), 1);
    assert_eq!(manifest.plugins[0].name, "local-plugin");
    match &manifest.plugins[0].source {
        PluginSource::Path(p) => assert_eq!(p, "./plugins/local"),
        other @ PluginSource::Structured(_) => {
            panic!("expected Path source, got {other:?}")
        }
    }
}

#[test]
fn marketplace_plugin_strict_defaults_true() {
    let json = r#"{"name": "p", "source": "./x"}"#;
    let plugin: MarketplacePlugin = serde_json::from_str(json).expect("parse");
    assert!(
        plugin.strict,
        "strict MUST default to true (require-manifest)"
    );
}

#[test]
fn marketplace_plugin_explicit_strict_false_round_trips() {
    let json = r#"{"name": "p", "source": "./x", "strict": false, "description": "loose"}"#;
    let plugin: MarketplacePlugin = serde_json::from_str(json).expect("parse");
    assert!(!plugin.strict);
    assert_eq!(plugin.description.as_deref(), Some("loose"));
}

#[test]
fn marketplace_with_mixed_source_types_all_parse() {
    let json = r#"{
        "name": "mixed",
        "owner": {"name": "Mixed Co"},
        "plugins": [
            {"name": "p-npm", "source": {"source": "npm", "package": "a"}},
            {"name": "p-pip", "source": {"source": "pip", "package": "b"}},
            {"name": "p-url", "source": {"source": "url", "url": "https://x/y.git"}},
            {"name": "p-gh", "source": {"source": "github", "repo": "a/b"}},
            {"name": "p-path", "source": "./local"}
        ]
    }"#;
    let manifest: MarketplaceManifest = serde_json::from_str(json).expect("parse");
    assert_eq!(manifest.plugins.len(), 5);
    let kinds: Vec<&'static str> = manifest
        .plugins
        .iter()
        .map(|p| match &p.source {
            PluginSource::Path(_) => "path",
            PluginSource::Structured(PluginSourceDef::Npm(_)) => "npm",
            PluginSource::Structured(PluginSourceDef::Pip(_)) => "pip",
            PluginSource::Structured(PluginSourceDef::Url(_)) => "url",
            PluginSource::Structured(PluginSourceDef::GitHub(_)) => "github",
        })
        .collect();
    assert_eq!(kinds, vec!["npm", "pip", "url", "github", "path"]);
}

#[test]
fn marketplace_manifest_missing_required_fields_errors() {
    // owner is required.
    let no_owner = r#"{"name": "x", "plugins": []}"#;
    assert!(serde_json::from_str::<MarketplaceManifest>(no_owner).is_err());
    // name is required.
    let no_name = r#"{"owner": {"name": "x"}, "plugins": []}"#;
    assert!(serde_json::from_str::<MarketplaceManifest>(no_name).is_err());
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — sha256_hex helper
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn sha256_hex_of_empty_bytes_matches_known_constant() {
    // The SHA-256 of zero bytes is the well-known constant
    // e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855.
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn sha256_hex_of_well_known_input_matches_known_value() {
    // SHA-256("hello") = 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824.
    assert_eq!(
        sha256_hex(b"hello"),
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
}

#[test]
fn sha256_hex_output_is_64_lowercase_hex_chars() {
    let h = sha256_hex(b"any payload");
    assert_eq!(h.len(), 64, "SHA-256 hex MUST be 64 chars");
    assert!(
        h.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')),
        "output MUST be lowercase hex; got {h:?}"
    );
}

#[test]
fn sha256_hex_is_deterministic_for_same_input() {
    let h1 = sha256_hex(b"the same input");
    let h2 = sha256_hex(b"the same input");
    assert_eq!(h1, h2);
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — ZipCache::put + get_verified round-trip
// ───────────────────────────────────────────────────────────────────────────

fn fresh_cache() -> (ZipCache, TempDir) {
    let dir = TempDir::new().expect("tempdir");
    let cache = ZipCache::new(dir.path().join("cache"));
    (cache, dir)
}

#[test]
fn put_and_get_verified_round_trip() {
    let (cache, _tmp) = fresh_cache();
    let bytes = b"fake-zip-archive-bytes";
    let sha = sha256_hex(bytes);
    let entry = CacheEntry {
        sha256: sha.clone(),
        plugin_id: "test-plugin@local".to_string(),
        version: Some("1.0.0".to_string()),
        installed_at_unix: 1_700_000_000,
    };
    cache.put(entry, bytes).expect("put must succeed");
    let recovered = cache.get_verified(&sha).expect("get must succeed");
    assert_eq!(recovered, bytes);
}

#[test]
fn put_with_mismatched_sha256_errors_integrity_mismatch() {
    let (cache, _tmp) = fresh_cache();
    let actual_bytes = b"actual content";
    let wrong_sha = sha256_hex(b"different content");
    let entry = CacheEntry {
        sha256: wrong_sha.clone(),
        plugin_id: "p".to_string(),
        version: None,
        installed_at_unix: 0,
    };
    let outcome = cache.put(entry, actual_bytes);
    let Err(ZipCacheError::IntegrityMismatch { sha256, actual }) = outcome else {
        panic!("expected IntegrityMismatch; got {outcome:?}");
    };
    assert_eq!(sha256, wrong_sha);
    assert_eq!(actual, sha256_hex(actual_bytes));
}

#[test]
fn get_verified_missing_archive_returns_missing_error() {
    let (cache, _tmp) = fresh_cache();
    let fake_sha = sha256_hex(b"not in cache");
    let outcome = cache.get_verified(&fake_sha);
    let Err(ZipCacheError::Missing(sha)) = outcome else {
        panic!("expected Missing; got {outcome:?}");
    };
    assert_eq!(sha, fake_sha);
}

#[test]
fn get_verified_detects_post_write_tampering() {
    let (cache, _tmp) = fresh_cache();
    let original = b"original archive bytes";
    let sha = sha256_hex(original);
    let entry = CacheEntry {
        sha256: sha.clone(),
        plugin_id: "p".to_string(),
        version: None,
        installed_at_unix: 0,
    };
    cache.put(entry, original).expect("put");
    // Tamper with the on-disk archive (simulate swap attack).
    let archive_path = cache.archive_path(&sha);
    std::fs::write(&archive_path, b"tampered content").expect("tamper write");
    // get_verified MUST detect the mismatch.
    let outcome = cache.get_verified(&sha);
    let Err(ZipCacheError::IntegrityMismatch { sha256, actual }) = outcome else {
        panic!("expected IntegrityMismatch after tamper; got {outcome:?}");
    };
    assert_eq!(sha256, sha);
    assert_eq!(actual, sha256_hex(b"tampered content"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — ZipCache::contains
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn contains_returns_false_for_uncached_sha() {
    let (cache, _tmp) = fresh_cache();
    assert!(!cache.contains(&sha256_hex(b"absent")));
}

#[test]
fn contains_returns_true_after_put() {
    let (cache, _tmp) = fresh_cache();
    let bytes = b"present";
    let sha = sha256_hex(bytes);
    let entry = CacheEntry {
        sha256: sha.clone(),
        plugin_id: "p".to_string(),
        version: None,
        installed_at_unix: 0,
    };
    cache.put(entry, bytes).expect("put");
    assert!(cache.contains(&sha));
}

// ───────────────────────────────────────────────────────────────────────────
// Section F — ZipCache index round-trip
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn read_index_on_fresh_cache_returns_empty_map() {
    let (cache, _tmp) = fresh_cache();
    let index = cache.read_index().expect("read");
    assert!(index.is_empty());
}

#[test]
fn write_and_read_index_round_trips_entries() {
    let (cache, _tmp) = fresh_cache();
    let mut entries = BTreeMap::new();
    entries.insert(
        "sha-a".to_string(),
        CacheEntry {
            sha256: "sha-a".to_string(),
            plugin_id: "plugin-a".to_string(),
            version: Some("0.1.0".to_string()),
            installed_at_unix: 1_000,
        },
    );
    entries.insert(
        "sha-b".to_string(),
        CacheEntry {
            sha256: "sha-b".to_string(),
            plugin_id: "plugin-b".to_string(),
            version: None,
            installed_at_unix: 2_000,
        },
    );
    cache.write_index(&entries).expect("write");
    let back = cache.read_index().expect("read");
    assert_eq!(back, entries);
}

#[test]
fn put_upserts_into_existing_index() {
    let (cache, _tmp) = fresh_cache();
    // First put: 1 entry.
    let bytes_1 = b"first";
    let sha_1 = sha256_hex(bytes_1);
    cache
        .put(
            CacheEntry {
                sha256: sha_1.clone(),
                plugin_id: "p1".to_string(),
                version: None,
                installed_at_unix: 0,
            },
            bytes_1,
        )
        .expect("put 1");
    // Second put: distinct sha, must coexist in index.
    let bytes_2 = b"second";
    let sha_2 = sha256_hex(bytes_2);
    cache
        .put(
            CacheEntry {
                sha256: sha_2.clone(),
                plugin_id: "p2".to_string(),
                version: None,
                installed_at_unix: 0,
            },
            bytes_2,
        )
        .expect("put 2");
    let index = cache.read_index().expect("read");
    assert_eq!(index.len(), 2);
    assert!(index.contains_key(&sha_1));
    assert!(index.contains_key(&sha_2));
}

#[test]
fn archive_path_uses_sha256_zip_naming() {
    let (cache, _tmp) = fresh_cache();
    let sha = "abc123";
    let path = cache.archive_path(sha);
    assert!(path.ends_with("abc123.zip"));
}
