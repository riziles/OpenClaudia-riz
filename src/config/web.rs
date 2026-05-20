//! Web-fetch tool configuration — preapproved domain list.
//!
//! See crosslink #603. Claude Code ships a built-in list of ~30 domains
//! whose fetches do not require an interactive permission prompt
//! (documentation hosts, package indexes, well-known reference sites).
//! `OpenClaudia` previously had no equivalent — every fetch fell through
//! to the same prompt path regardless of destination, including obvious
//! documentation lookups that no user would object to.
//!
//! This module exposes [`WebFetchConfig`] holding the user-facing
//! `preapproved_domains` list (with a sensible default that mirrors CC's
//! shipped list), and [`is_preapproved`] which subdomain-matches a URL
//! against that list. The permission layer consults `is_preapproved`
//! before opening a prompt so allowlisted hosts skip the round-trip.

use serde::Deserialize;

/// Web-fetch tool configuration.
///
/// `preapproved_domains` is a host allowlist — a URL whose host is equal
/// to, or a subdomain of, any entry is treated as preapproved and bypasses
/// the interactive permission prompt. Match semantics are identical to
/// the [`crate::tools`] web-search `allowed_domains` filter
/// (`docs.python.org` matches both the exact host and `foo.docs.python.org`).
///
/// Defaults to [`default_preapproved_domains`] — roughly 30 well-known
/// documentation / package / reference sites. Users can override with an
/// empty list to disable the preapproval shortcut entirely, or extend it
/// with additional hosts.
///
/// See crosslink #603.
#[derive(Debug, Deserialize, Clone)]
pub struct WebFetchConfig {
    #[serde(default = "default_preapproved_domains")]
    pub preapproved_domains: Vec<String>,
}

impl Default for WebFetchConfig {
    fn default() -> Self {
        Self {
            preapproved_domains: default_preapproved_domains(),
        }
    }
}

/// Default preapproved-domain list shipped with `OpenClaudia`.
///
/// Mirrors CC's bundled allowlist: documentation sites for the major
/// languages and frameworks an agent commonly reads while debugging,
/// plus package indexes (npm/PyPI/crates.io/Go) and the canonical
/// reference hosts (MDN, Stack Overflow, GitHub). Every entry must be
/// a bare host — schemes, paths, ports, and `www.` prefixes are stripped
/// before matching.
///
/// Hosts are sourced from CC's `webFetch_PREAPPROVED_HOSTS` constant and
/// trimmed to the categories where the agent's typical "fetch docs"
/// flows live. The list is intentionally short — adding a host requires
/// thinking about whether arbitrary content under that host should be
/// fetchable without a prompt.
#[must_use]
pub fn default_preapproved_domains() -> Vec<String> {
    vec![
        // Language references
        "docs.python.org".to_string(),
        "doc.rust-lang.org".to_string(),
        "docs.rs".to_string(),
        "go.dev".to_string(),
        "pkg.go.dev".to_string(),
        "nodejs.org".to_string(),
        "developer.mozilla.org".to_string(),
        "ruby-doc.org".to_string(),
        "docs.oracle.com".to_string(),
        // Framework / library docs
        "reactjs.org".to_string(),
        "react.dev".to_string(),
        "vuejs.org".to_string(),
        "angular.io".to_string(),
        "svelte.dev".to_string(),
        "nextjs.org".to_string(),
        "tailwindcss.com".to_string(),
        // Cloud / infra docs
        "docs.aws.amazon.com".to_string(),
        "cloud.google.com".to_string(),
        "learn.microsoft.com".to_string(),
        "docs.docker.com".to_string(),
        "kubernetes.io".to_string(),
        // AI / model provider docs
        "docs.anthropic.com".to_string(),
        "platform.openai.com".to_string(),
        "ai.google.dev".to_string(),
        "huggingface.co".to_string(),
        // Package indexes / source forges
        "pypi.org".to_string(),
        "npmjs.com".to_string(),
        "crates.io".to_string(),
        "github.com".to_string(),
        "gitlab.com".to_string(),
        // Reference / Q&A
        "stackoverflow.com".to_string(),
        "wikipedia.org".to_string(),
    ]
}

/// True when `url`'s host matches an entry in `preapproved`.
///
/// A match is either an exact host equality with one of the entries, or
/// a subdomain of it. Mirrors the subdomain match used by web-search
/// `allowed_domains` filtering so users only have one mental model of
/// how domain lists behave.
///
/// Unparseable URLs return `false` (fail-closed for unrecognised input).
#[must_use]
pub fn is_preapproved(url: &str, preapproved: &[String]) -> bool {
    let Some(host) = host_of(url) else {
        return false;
    };
    preapproved.iter().any(|d| domain_matches(&host, d))
}

/// Lowercased host with any `www.` prefix stripped, or `None` when the URL
/// can't be parsed by `url::Url`. Mirrors `tools::web::host_of` so the two
/// allowlists share one normalisation pass. Duplicated here (rather than
/// re-exported) because `tools::web::host_of` is module-private — calling
/// across `tools` from `config` would force a public surface change that
/// is out of scope for #603.
fn host_of(url: &str) -> Option<String> {
    let host = url::Url::parse(url).ok()?.host_str()?.to_ascii_lowercase();
    let stripped = host.strip_prefix("www.").unwrap_or(&host).to_string();
    if stripped.is_empty() {
        None
    } else {
        Some(stripped)
    }
}

/// Same semantics as `tools::web::domain_matches`: `host` is allowed when
/// it equals `needle` or when `needle` is a parent of `host` by at least
/// one DNS label.
fn domain_matches(host: &str, needle: &str) -> bool {
    let needle = needle.trim_start_matches("www.").to_ascii_lowercase();
    if needle.is_empty() {
        return false;
    }
    host == needle || host.ends_with(&format!(".{needle}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_list_contains_expected_categories() {
        let defaults = default_preapproved_domains();
        // ~30 entries; tolerate ±2 if we tweak the list later.
        assert!(
            defaults.len() >= 25 && defaults.len() <= 40,
            "default list should be roughly 30 entries; got {}",
            defaults.len()
        );
        // Spot-check coverage across categories.
        assert!(defaults.iter().any(|d| d == "docs.python.org"));
        assert!(defaults.iter().any(|d| d == "docs.rs"));
        assert!(defaults.iter().any(|d| d == "developer.mozilla.org"));
        assert!(defaults.iter().any(|d| d == "github.com"));
        assert!(defaults.iter().any(|d| d == "docs.anthropic.com"));
    }

    #[test]
    fn config_default_populates_preapproved_list() {
        let cfg = WebFetchConfig::default();
        assert!(
            !cfg.preapproved_domains.is_empty(),
            "default config must ship a non-empty allowlist (#603)"
        );
        assert!(cfg.preapproved_domains.contains(&"crates.io".to_string()));
    }

    #[test]
    fn serde_default_matches_struct_default() {
        let cfg: WebFetchConfig = serde_json::from_str("{}").expect("deserialize {}");
        assert_eq!(
            cfg.preapproved_domains,
            default_preapproved_domains(),
            "serde-default empty object must produce shipped allowlist"
        );
    }

    #[test]
    fn serde_explicit_empty_list_disables_preapproval() {
        let cfg: WebFetchConfig =
            serde_json::from_str(r#"{"preapproved_domains": []}"#).expect("deserialize");
        assert!(
            cfg.preapproved_domains.is_empty(),
            "explicit empty list must be honoured (opt-out path)"
        );
    }

    #[test]
    fn serde_user_extended_list_overrides_default() {
        let cfg: WebFetchConfig =
            serde_json::from_str(r#"{"preapproved_domains": ["example.com", "foo.test"]}"#)
                .expect("deserialize");
        assert_eq!(cfg.preapproved_domains.len(), 2);
        assert!(cfg.preapproved_domains.contains(&"example.com".to_string()));
        assert!(cfg.preapproved_domains.contains(&"foo.test".to_string()));
    }

    #[test]
    fn is_preapproved_exact_host_match() {
        let list = vec!["docs.python.org".to_string()];
        assert!(is_preapproved("https://docs.python.org/3/", &list));
    }

    #[test]
    fn is_preapproved_subdomain_match() {
        let list = vec!["github.com".to_string()];
        assert!(is_preapproved("https://api.github.com/repos/foo", &list));
        assert!(is_preapproved("https://github.com/foo/bar", &list));
    }

    #[test]
    fn is_preapproved_sibling_domain_rejected() {
        let list = vec!["docs.python.org".to_string()];
        // Sibling: `python.org` is NOT a subdomain of `docs.python.org`.
        assert!(!is_preapproved("https://python.org/", &list));
        // Lookalike: `evildocs.python.org` is not a subdomain match.
        assert!(!is_preapproved("https://evildocs.python.org/", &list));
    }

    #[test]
    fn is_preapproved_unparseable_url_rejected() {
        let list = vec!["github.com".to_string()];
        assert!(!is_preapproved("not-a-url", &list));
        assert!(!is_preapproved("", &list));
    }

    #[test]
    fn is_preapproved_handles_www_prefix() {
        let list = vec!["wikipedia.org".to_string()];
        assert!(is_preapproved("https://www.wikipedia.org/wiki/X", &list));
        assert!(is_preapproved(
            "https://en.wikipedia.org/wiki/Rust",
            &list
        ));
    }
}
