//! ACP (Agent Client Protocol) server configuration.
//!
//! Closes crosslink #717 — the agentic loop in [`crate::acp::AcpServer`]
//! previously hard-coded its iteration ceiling. This module exposes the
//! knob as a typed config value so the cap is discoverable, configurable
//! at runtime via either YAML or env var, and testable without
//! recompiling.
//!
//! ## Why this isn't a field on [`crate::config::AppConfig`]
//!
//! Adding a field to `AppConfig` would force every in-tree struct
//! literal that constructs an `AppConfig` (test fixtures, subagent
//! scaffolding, vdd transport tests) to be updated in lockstep. Several
//! of those sites live in modules the present change-set is forbidden
//! to touch. So `AcpConfig` is loaded lazily on first use from the
//! optional `acp:` block of `.openclaudia/config.yaml` plus a single
//! env-var override — same configurability surface, no schema break.

use serde::Deserialize;

/// Default iteration ceiling for the ACP prompt → tool-call → re-prompt
/// loop. Matches the previous hard-coded value so existing deployments
/// see no behavioural change after this module lands.
const DEFAULT_MAX_ITERATIONS: u32 = 50;

/// Env-var override for [`AcpConfig::max_iterations`]. A non-empty,
/// parseable `u32` wins over both the default and any value read from
/// the YAML config file.
pub const MAX_ITERATIONS_ENV_VAR: &str = "OPENCLAUDIA_ACP_MAX_ITERATIONS";

const fn default_max_iterations() -> u32 {
    DEFAULT_MAX_ITERATIONS
}

/// ACP server configuration.
///
/// All fields default to the values previously hard-coded in
/// [`crate::acp::AcpServer::run_prompt_loop`] so omitting the section
/// from `config.yaml` reproduces today's behaviour exactly.
#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub struct AcpConfig {
    /// Maximum number of provider request iterations within a single
    /// ACP prompt. Each tool call consumes one iteration; the loop
    /// returns `"end_turn"` as soon as the model stops issuing tool
    /// calls. The cap exists as a safety belt against runaway loops —
    /// a model that never decides to stop. Configurable so operators
    /// running long-horizon agents can raise it without forking.
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,
}

impl Default for AcpConfig {
    fn default() -> Self {
        Self {
            max_iterations: DEFAULT_MAX_ITERATIONS,
        }
    }
}

impl AcpConfig {
    /// Resolve the runtime [`AcpConfig`] from (in order of precedence):
    ///
    /// 1. The [`MAX_ITERATIONS_ENV_VAR`] env var, if set to a parseable `u32`.
    /// 2. The `acp:` block of `.openclaudia/config.yaml`, if present.
    /// 3. [`AcpConfig::default`].
    ///
    /// Errors from individual sources are silently downgraded to the
    /// next source — this lookup is on the per-prompt hot path and must
    /// not panic when the operator's config is malformed. The caller
    /// receives a usable cap in every case.
    #[must_use]
    pub fn load() -> Self {
        let mut cfg = Self::load_from_yaml().unwrap_or_default();
        if let Ok(raw) = std::env::var(MAX_ITERATIONS_ENV_VAR) {
            if let Ok(parsed) = raw.trim().parse::<u32>() {
                if parsed > 0 {
                    cfg.max_iterations = parsed;
                }
            }
        }
        cfg
    }

    /// Read the `acp:` block out of `.openclaudia/config.yaml` if the
    /// file exists. Returns `None` when the file is missing, unreadable,
    /// or carries no `acp:` block — the caller falls back to defaults.
    fn load_from_yaml() -> Option<Self> {
        let path = std::path::PathBuf::from(".openclaudia/config.yaml");
        let raw = std::fs::read_to_string(&path).ok()?;
        let root: serde_yaml::Value = serde_yaml::from_str(&raw).ok()?;
        let acp = root.get("acp")?;
        serde_yaml::from_value(acp.clone()).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_max_iterations_matches_previous_hard_coded_value() {
        // crosslink #717: the cap was `50` as a local literal before this
        // module existed. Default must match exactly so config-less
        // deployments see no behavioural change.
        assert_eq!(AcpConfig::default().max_iterations, 50);
    }

    #[test]
    fn empty_yaml_yields_default() {
        let cfg: AcpConfig = serde_yaml::from_str("{}").expect("valid yaml");
        assert_eq!(cfg.max_iterations, 50);
    }

    #[test]
    fn deserialises_custom_max_iterations_from_yaml() {
        let yaml = "max_iterations: 200\n";
        let cfg: AcpConfig = serde_yaml::from_str(yaml).expect("valid yaml");
        assert_eq!(cfg.max_iterations, 200);
    }

    #[test]
    fn env_var_constant_is_namespaced_under_openclaudia_acp() {
        // Guard against accidental rename: the env var is documented in
        // the module docstring above. Keep the OPENCLAUDIA_ prefix so
        // it aligns with the prefix used by the main config builder.
        assert_eq!(MAX_ITERATIONS_ENV_VAR, "OPENCLAUDIA_ACP_MAX_ITERATIONS");
    }
}
