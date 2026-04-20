//! Feature-flag source — boolean gates for opt-in code paths.
//!
//! Port of Claude Code's `getFeatureValue_CACHED_MAY_BE_STALE` +
//! GrowthBook-backed flag lookups, stripped down to the local case.
//! OC doesn't have a remote flag backend; every flag defaults to
//! `false` unless explicitly turned on via [`StaticFlags::set`] or the
//! `OPENCLAUDIA_FEATURE_<NAME>` environment variable.
//!
//! Flag names are expected to be snake_case. The env-var lookup
//! uppercases the name and prepends `OPENCLAUDIA_FEATURE_`, so
//! `ultrathink_enabled` reads `OPENCLAUDIA_FEATURE_ULTRATHINK_ENABLED`.

use std::collections::HashMap;

/// Trait for resolving feature flags. Separate from
/// [`crate::services::AnalyticsSink`] so a backend that wires one
/// doesn't need to implement the other.
pub trait FeatureFlagSource: Send + Sync {
    /// Resolve `name` → `true` / `false`. Unknown flags default to
    /// `false` — matches Claude Code's "opt-in" semantic for GB-backed
    /// flags.
    fn is_enabled(&self, name: &str) -> bool;
}

/// Default implementation backed by a `HashMap<String, bool>` +
/// environment-variable fallback. `set` writes are intentionally
/// build-time / startup-time — no lock overhead on the `is_enabled`
/// hot path at the cost of needing `&mut self` for mutation. Wrap in
/// `Arc<RwLock<>>` if you need concurrent updates.
#[derive(Debug, Default, Clone)]
pub struct StaticFlags {
    entries: HashMap<String, bool>,
}

impl StaticFlags {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Pre-seed a flag. Called at startup / test setup.
    pub fn set(&mut self, name: &str, value: bool) {
        self.entries.insert(name.to_string(), value);
    }

    /// Chainable alias for `set` — convenient in test fixtures.
    #[must_use]
    pub fn with(mut self, name: &str, value: bool) -> Self {
        self.set(name, value);
        self
    }

    /// Resolve via env var: `OPENCLAUDIA_FEATURE_<UPPER>` where
    /// truthy = `1` / `true` / `on` / `yes` (case-insensitive).
    /// Any other value → `false`. Missing env var → `None`, letting
    /// the caller fall through to the map.
    fn env_override(name: &str) -> Option<bool> {
        let upper = name.to_ascii_uppercase();
        let key = format!("OPENCLAUDIA_FEATURE_{upper}");
        let raw = std::env::var(&key).ok()?;
        Some(matches!(
            raw.to_ascii_lowercase().as_str(),
            "1" | "true" | "on" | "yes"
        ))
    }
}

impl FeatureFlagSource for StaticFlags {
    fn is_enabled(&self, name: &str) -> bool {
        // Env var wins — operators / CI can override the compiled
        // defaults without a rebuild. Matches the precedence used
        // elsewhere (MAX_THINKING_TOKENS, CLAUDE_CODE_EFFORT_LEVEL).
        if let Some(env) = Self::env_override(name) {
            return env;
        }
        self.entries.get(name).copied().unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    /// `StaticFlags::env_override` reads an env var → tests that
    /// touch it must serialize. Shared across the module.
    fn env_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    struct EnvGuard {
        key: String,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            unsafe {
                std::env::set_var(key, value);
            }
            Self {
                key: key.to_string(),
                previous,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.previous {
                    Some(v) => std::env::set_var(&self.key, v),
                    None => std::env::remove_var(&self.key),
                }
            }
        }
    }

    #[test]
    fn unknown_flag_defaults_false() {
        let _lock = env_lock();
        let flags = StaticFlags::new();
        assert!(!flags.is_enabled("missing"));
    }

    #[test]
    fn explicit_set_wins_over_default() {
        let _lock = env_lock();
        let flags = StaticFlags::new().with("fast_path", true);
        assert!(flags.is_enabled("fast_path"));
    }

    #[test]
    fn env_var_overrides_map_entry() {
        let _lock = env_lock();
        let flags = StaticFlags::new().with("beta_feature", false);
        // With map=false, no env override yet: should be false.
        assert!(!flags.is_enabled("beta_feature"));

        let _g = EnvGuard::set("OPENCLAUDIA_FEATURE_BETA_FEATURE", "1");
        assert!(flags.is_enabled("beta_feature"));
    }

    #[test]
    fn env_var_accepts_truthy_variants() {
        let _lock = env_lock();
        let flags = StaticFlags::new();
        for truthy in ["1", "true", "on", "yes", "TRUE", "Yes", "ON"] {
            let _g = EnvGuard::set("OPENCLAUDIA_FEATURE_GATE", truthy);
            assert!(
                flags.is_enabled("gate"),
                "expected '{truthy}' to count as truthy"
            );
        }
    }

    #[test]
    fn env_var_rejects_non_truthy_variants() {
        let _lock = env_lock();
        let flags = StaticFlags::new();
        for falsy in ["0", "false", "off", "no", "random"] {
            let _g = EnvGuard::set("OPENCLAUDIA_FEATURE_GATE", falsy);
            assert!(
                !flags.is_enabled("gate"),
                "expected '{falsy}' to count as falsy"
            );
        }
    }

    #[test]
    fn env_var_falls_through_to_map_when_unset() {
        let _lock = env_lock();
        // Ensure no ambient env var exists before the test runs.
        unsafe {
            std::env::remove_var("OPENCLAUDIA_FEATURE_MAP_ONLY");
        }
        let flags = StaticFlags::new().with("map_only", true);
        assert!(flags.is_enabled("map_only"));
    }
}
