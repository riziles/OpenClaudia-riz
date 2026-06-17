//! Claude Code Compatibility Layer
//!
//! Types and functions for loading hooks from Claude Code's `.claude/settings.json`
//! format and converting them to `OpenClaudia`'s internal representation.

use crate::config::HooksConfig;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

use super::merge::{enforce_total_size, merge_claude_hooks, merge_settings_file};

/// Claude Code settings.json structure
#[derive(Debug, Deserialize, Default)]
pub struct ClaudeCodeSettings {
    #[serde(default)]
    pub hooks: HashMap<String, Vec<ClaudeCodeHookEntry>>,
}

/// Claude Code hook entry format
#[derive(Debug, Deserialize)]
pub struct ClaudeCodeHookEntry {
    #[serde(default)]
    pub matcher: Option<String>,
    #[serde(default)]
    pub hooks: Vec<ClaudeCodeHook>,
}

/// Claude Code hook definition
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum ClaudeCodeHook {
    #[serde(rename = "command")]
    Command {
        command: String,
        #[serde(default = "default_claude_timeout")]
        timeout: Option<u64>,
    },
}

#[allow(clippy::unnecessary_wraps)]
const fn default_claude_timeout() -> Option<u64> {
    Some(60)
}

/// Load hooks from Claude Code-compatible settings layers.
///
/// This is the runtime-facing helper used by the CLI, ACP, and proxy paths.
/// It intentionally routes through [`load_claude_code_hooks_layered`] so all
/// call sites honor the same user, project, project-local, and managed
/// settings precedence.
#[must_use]
pub fn load_claude_code_hooks() -> HooksConfig {
    let (config, _) = load_claude_code_hooks_layered();
    config
}

// ============================================================================
// Settings File Layering
// ============================================================================

/// Result of loading layered Claude settings
pub struct LayeredSettings {
    /// The merged settings value
    pub settings: Value,
    /// Allowed tools extracted from merged settings
    pub allowed_tools: Vec<String>,
    /// Path to managed (enterprise) settings if loaded
    pub managed_settings_path: Option<PathBuf>,
}

/// Load Claude settings from all layers, merging them in order.
///
/// Load order (later overrides earlier):
/// 1. `~/.claude/settings.json` (user global)
/// 2. `.claude/settings.json` (project, committed)
/// 3. `.claude/settings.local.json` (project, gitignored)
/// 4. System-level managed settings (enterprise)
///
/// Deep merge: arrays concatenate, objects merge recursively,
/// scalars from later files override.
pub fn load_claude_settings() -> LayeredSettings {
    let mut settings = Value::Object(serde_json::Map::default());

    // 1. User global settings
    if let Some(home) = dirs::home_dir() {
        let user_settings = home.join(".claude/settings.json");
        if user_settings.exists() {
            merge_settings_file(&mut settings, &user_settings);
            debug!(path = ?user_settings, "Loaded user-global Claude settings");
        }
    }

    // 2. Project settings (committed)
    let project_settings = Path::new(".claude/settings.json");
    if project_settings.exists() {
        merge_settings_file(&mut settings, project_settings);
        debug!(path = ?project_settings, "Loaded project Claude settings");
    }

    // 3. Project local settings (gitignored)
    let local_settings = Path::new(".claude/settings.local.json");
    if local_settings.exists() {
        merge_settings_file(&mut settings, local_settings);
        debug!(path = ?local_settings, "Loaded project-local Claude settings");
    }

    // 4. System-level managed settings (enterprise). Only Linux and macOS
    // have well-known managed locations; on other platforms this is None.
    let managed_path: Option<PathBuf> = {
        #[cfg(target_os = "linux")]
        {
            let p = Path::new("/etc/openclaudia/managed-settings.json");
            p.exists().then(|| p.to_path_buf())
        }
        #[cfg(target_os = "macos")]
        {
            let p = Path::new("/Library/Application Support/openclaudia/managed-settings.json");
            p.exists().then(|| p.to_path_buf())
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            None
        }
    };

    if let Some(path) = &managed_path {
        warn!(
            path = ?path,
            "Loading enterprise managed settings - these override all user and project settings"
        );
        merge_settings_file(&mut settings, path);
    }

    // Post-merge size guard: a hostile combination of the four
    // settings layers could still pump megabytes of data in even when
    // each individual file is fine. Fall back to an empty object
    // rather than handing the harness an oversized blob to walk
    // (crosslink #333).
    if let Err(e) = enforce_total_size(&settings) {
        tracing::error!(
            error = %e,
            "Merged Claude settings exceed size cap; falling back to empty settings"
        );
        settings = Value::Object(serde_json::Map::default());
    }

    // Extract allowedTools from merged settings
    let allowed_tools: Vec<String> = settings
        .get("allowedTools")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    if !allowed_tools.is_empty() {
        info!(
            count = allowed_tools.len(),
            "Extracted allowedTools from settings"
        );
    }

    LayeredSettings {
        settings,
        allowed_tools,
        managed_settings_path: managed_path,
    }
}

/// Load hooks from all layered settings files.
///
/// Uses the new 4-layer settings loading instead of the old 2-layer approach.
/// Returns merged `HooksConfig` with Claude Code hooks converted to `OpenClaudia` format.
pub fn load_claude_code_hooks_layered() -> (HooksConfig, LayeredSettings) {
    let layered = load_claude_settings();
    let mut config = HooksConfig::default();

    // Parse hooks from the merged settings
    if let Some(hooks_obj) = layered.settings.get("hooks") {
        if let Ok(settings) =
            serde_json::from_value::<ClaudeCodeSettings>(json!({ "hooks": hooks_obj }))
        {
            merge_claude_hooks(&mut config, &settings);
            info!("Loaded hooks from layered Claude settings");
        }
    }

    (config, layered)
}
