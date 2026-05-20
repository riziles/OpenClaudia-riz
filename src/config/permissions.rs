use serde::Deserialize;

/// Tool permission system configuration.
///
/// Controls whether permission checks are performed before tool execution
/// and provides default allow-list patterns.
///
/// # Default posture
///
/// `enabled` defaults to `true` (deny-by-default, matching Claude Code's
/// always-on permission pipeline). A fresh installation with no
/// `permissions:` block in `config.yaml` will **prompt before every
/// destructive tool call**.
///
/// To opt out of the permission system entirely, set `enabled: false` in
/// your config. This is **not recommended** for production use; it is
/// equivalent to Claude Code's `bypassPermissions` mode and removes all
/// audit trails.
///
/// # Deprecation note
///
/// The `enabled` field is scheduled for removal. The long-term plan is to
/// make permissions always-on and replace opt-out with an explicit
/// `dangerously_disable_permissions: true`. See crosslink #282.
#[derive(Debug, Deserialize, Clone)]
pub struct PermissionsConfig {
    /// Enable the permission system.
    ///
    /// Defaults to `true` (deny-by-default). Set to `false` only to
    /// replicate the old allow-all behaviour; note that doing so also
    /// silences all persisted Deny rules.
    ///
    /// **Deprecated**: prefer leaving this unset (the default `true`)
    /// and use `dangerously_disable_permissions` when an explicit bypass
    /// is required. See crosslink #282.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Glob patterns that are pre-allowed without prompting.
    /// Patterns are matched against the tool's primary argument
    /// (command string for Bash, `file_path` for Edit/Write).
    #[serde(default)]
    pub default_allow: Vec<String>,
}

/// Returns the default value for `PermissionsConfig::enabled`.
///
/// `true` — permissions are on by default (deny-by-default posture).
/// Fixes crosslink #282: the previous `#[serde(default)]` on a `bool`
/// field silently defaulted to `false`, making a fresh install allow-all.
const fn default_enabled() -> bool {
    true
}

impl Default for PermissionsConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            default_allow: Vec::new(),
        }
    }
}

impl PermissionsConfig {
    /// Validate `default_allow` entries at config-load time
    /// (crosslink #938). Rejects:
    ///
    /// * **Empty** patterns — a zero-byte glob silently matches every
    ///   empty target argument and is almost always a YAML quoting bug.
    /// * **Bare `*` / `**`** — unbounded patterns disable the permission
    ///   system while *looking* enabled. Reject unless the operator
    ///   explicitly opted in via a `bypass-permissions` mode.
    /// * **NUL bytes / control chars** — these never appear in a real
    ///   tool argument and almost always come from a misencoded YAML.
    ///
    /// Also emits a WARN log when `default_allow` is non-empty but
    /// `enabled = false` — the entries would be ignored and the
    /// operator probably meant to enable the system.
    ///
    /// # Errors
    ///
    /// Returns `Err(String)` with a human-readable diagnostic when any
    /// pattern fails validation. The caller (`config::load_config`)
    /// surfaces this as `ConfigError::Message`.
    pub fn validate(&self) -> Result<(), String> {
        for (idx, pat) in self.default_allow.iter().enumerate() {
            if pat.is_empty() {
                return Err(format!(
                    "permissions.default_allow[{idx}]: empty pattern is invalid \
                     (use a real glob or remove the entry)"
                ));
            }
            if pat == "*" || pat == "**" {
                return Err(format!(
                    "permissions.default_allow[{idx}] = '{pat}': unbounded patterns \
                     would pre-allow every tool argument and effectively disable the \
                     permission system. Use a scoped glob (e.g. '/project/**' or 'git *')."
                ));
            }
            if pat
                .chars()
                .any(|c| c == '\0' || (c.is_control() && c != '\t'))
            {
                return Err(format!(
                    "permissions.default_allow[{idx}] = '{pat}': pattern contains \
                     NUL / control characters that no real tool argument carries"
                ));
            }
        }
        if !self.default_allow.is_empty() && !self.enabled {
            tracing::warn!(
                count = self.default_allow.len(),
                "permissions.default_allow has entries but permissions.enabled=false; \
                 entries will be ignored. Set enabled=true to honour them."
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_accepts_scoped_globs() {
        let cfg = PermissionsConfig {
            enabled: true,
            default_allow: vec!["/project/**".into(), "git *".into(), "*.rs".into()],
        };
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn validate_rejects_empty_pattern() {
        let cfg = PermissionsConfig {
            enabled: true,
            default_allow: vec!["ok".into(), String::new()],
        };
        let err = cfg.validate().expect_err("empty pattern must be rejected");
        assert!(err.contains("[1]"), "error must name the index: {err}");
        assert!(
            err.contains("empty pattern"),
            "error must mention emptiness: {err}"
        );
    }

    #[test]
    fn validate_rejects_unbounded_glob() {
        for unbounded in ["*", "**"] {
            let cfg = PermissionsConfig {
                enabled: true,
                default_allow: vec![unbounded.into()],
            };
            let err = cfg.validate().expect_err("unbounded glob must be rejected");
            assert!(
                err.contains("unbounded"),
                "error must mention 'unbounded': {err}"
            );
        }
    }

    #[test]
    fn validate_rejects_control_characters() {
        let cfg = PermissionsConfig {
            enabled: true,
            default_allow: vec!["foo\u{1}bar".into()],
        };
        let err = cfg.validate().expect_err("control chars must be rejected");
        assert!(
            err.contains("control"),
            "error must mention 'control': {err}"
        );
    }

    #[test]
    fn validate_default_is_ok() {
        // Default is empty default_allow, so nothing to validate.
        assert!(PermissionsConfig::default().validate().is_ok());
    }
}
