//! Hook merging utilities
//!
//! Functions for merging hook configurations from multiple sources,
//! deep-merging JSON settings files, and converting Claude Code hooks
//! to `OpenClaudia` format.

use crate::config::{Hook, HookEntry, HooksConfig};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tracing::{debug, warn};

use super::claude_compat::{ClaudeCodeHook, ClaudeCodeSettings};
use super::HookEvent;

/// Merge two `HooksConfig` structs, with `other` taking precedence.
/// Entries from `other` with the same matcher as an entry in `base` replace the base entry.
#[must_use]
pub fn merge_hooks_config(base: HooksConfig, other: HooksConfig) -> HooksConfig {
    let mut merged = base;

    dedup_hook_entries(&mut merged.session_start, other.session_start);
    dedup_hook_entries(&mut merged.session_end, other.session_end);
    dedup_hook_entries(&mut merged.pre_tool_use, other.pre_tool_use);
    dedup_hook_entries(&mut merged.post_tool_use, other.post_tool_use);
    dedup_hook_entries(&mut merged.user_prompt_submit, other.user_prompt_submit);
    dedup_hook_entries(&mut merged.stop, other.stop);
    dedup_hook_entries(&mut merged.pre_adversary_review, other.pre_adversary_review);
    dedup_hook_entries(
        &mut merged.post_adversary_review,
        other.post_adversary_review,
    );
    dedup_hook_entries(&mut merged.vdd_conflict, other.vdd_conflict);
    dedup_hook_entries(&mut merged.vdd_converged, other.vdd_converged);

    merged
}

/// Normalize a hook matcher key: treat `None` and `Some("")` identically so
/// callers that vary on this don't end up with shadow duplicates
/// (crosslink #339).
fn norm_matcher_key(m: Option<&String>) -> Option<String> {
    m.filter(|s| !s.is_empty()).cloned()
}

/// Extend `base` with entries from `other`, replacing any base entry whose
/// matcher matches an entry in `other` (later source wins).
///
/// Uses a `HashMap` keyed on the normalized matcher for O(n+m) merge
/// (vs the previous O(n*m) `Vec::retain` per entry — crosslink #339).
/// When `other` replaces a base entry, the replaced entry's command list
/// is logged at WARN so users can detect silent overrides.
fn dedup_hook_entries(base: &mut Vec<HookEntry>, other: Vec<HookEntry>) {
    if other.is_empty() {
        return;
    }

    // Build an index of base entries by normalized matcher -> position. O(n).
    let mut index: HashMap<Option<String>, usize> = base
        .iter()
        .enumerate()
        .map(|(i, e)| (norm_matcher_key(e.matcher.as_ref()), i))
        .collect();

    for entry in other {
        let key = norm_matcher_key(entry.matcher.as_ref());
        if let Some(&pos) = index.get(&key) {
            // Replacement: surface the dropped commands so silent overrides
            // are visible in the warn-level log stream.
            let replaced_commands: Vec<String> = base[pos]
                .hooks
                .iter()
                .map(|h| match h {
                    Hook::Command { command, .. } => command.clone(),
                    Hook::Prompt { prompt, .. } => format!("<prompt: {prompt}>"),
                    Hook::Model { model, prompt, .. } => format!("<model {model}: {prompt}>"),
                })
                .collect();
            warn!(
                matcher = ?key,
                replaced_commands = ?replaced_commands,
                "Hook entry replaced by later source (same matcher)"
            );
            base[pos] = entry;
        } else {
            index.insert(key, base.len());
            base.push(entry);
        }
    }
}

/// Merge a settings file into the accumulator using deep merge semantics.
///
/// - Objects merge recursively
/// - Arrays concatenate
/// - Scalars from the new file override
pub(crate) fn merge_settings_file(target: &mut Value, path: &Path) {
    match fs::read_to_string(path) {
        Ok(content) => match serde_json::from_str::<Value>(&content) {
            Ok(new_settings) => {
                deep_merge(target, &new_settings);
            }
            Err(e) => {
                warn!(path = ?path, error = %e, "Failed to parse settings file");
            }
        },
        Err(e) => {
            debug!(path = ?path, error = %e, "Could not read settings file");
        }
    }
}

/// Deep merge two JSON values.
///
/// - Objects: recursively merge keys
/// - Arrays: concatenate
/// - Scalars: `source` overrides `target`
pub(crate) fn deep_merge(target: &mut Value, source: &Value) {
    match (target, source) {
        (Value::Object(target_map), Value::Object(source_map)) => {
            for (key, source_val) in source_map {
                let entry = target_map.entry(key.clone()).or_insert(Value::Null);
                deep_merge(entry, source_val);
            }
        }
        (Value::Array(target_arr), Value::Array(source_arr)) => {
            target_arr.extend(source_arr.iter().cloned());
        }
        (target, source) => {
            *target = source.clone();
        }
    }
}

/// Merge Claude Code hooks into `OpenClaudia` `HooksConfig`
pub(crate) fn merge_claude_hooks(config: &mut HooksConfig, settings: &ClaudeCodeSettings) {
    for (event_name, entries) in &settings.hooks {
        let Some(event) = HookEvent::from_claude_code_name(event_name) else {
            warn!(event = %event_name, "Unknown Claude Code hook event, skipping");
            continue;
        };

        // Convert Claude Code entries to OpenClaudia format
        let converted_entries: Vec<HookEntry> = entries
            .iter()
            .map(|entry| {
                let hooks: Vec<Hook> = entry
                    .hooks
                    .iter()
                    .map(|h| match h {
                        ClaudeCodeHook::Command { command, timeout } => Hook::Command {
                            command: command.clone(),
                            // Claude Code settings files have no shell: field;
                            // default to the safe direct-spawn mode.
                            shell: false,
                            timeout: timeout.unwrap_or(60),
                        },
                    })
                    .collect();

                HookEntry {
                    matcher: entry.matcher.clone().filter(|m| !m.is_empty()),
                    hooks,
                }
            })
            .collect();

        // Append to the appropriate event list. Full Claude Code
        // hook-event coverage — anything Claude Code can configure in
        // settings.json reaches our HookEngine.
        match event {
            HookEvent::SessionStart => config.session_start.extend(converted_entries),
            HookEvent::SessionEnd => config.session_end.extend(converted_entries),
            HookEvent::PreToolUse => config.pre_tool_use.extend(converted_entries),
            HookEvent::PostToolUse => config.post_tool_use.extend(converted_entries),
            HookEvent::PostToolUseFailure => {
                config.post_tool_use_failure.extend(converted_entries);
            }
            HookEvent::UserPromptSubmit => config.user_prompt_submit.extend(converted_entries),
            HookEvent::Stop => config.stop.extend(converted_entries),
            HookEvent::SubagentStart => config.subagent_start.extend(converted_entries),
            HookEvent::SubagentStop => config.subagent_stop.extend(converted_entries),
            HookEvent::PreCompact => config.pre_compact.extend(converted_entries),
            HookEvent::PermissionRequest => {
                config.permission_request.extend(converted_entries);
            }
            HookEvent::Notification => config.notification.extend(converted_entries),
            // VDD events aren't in Claude Code's schema — merge path
            // never sees them (from_claude_code_name filters them out).
            HookEvent::PreAdversaryReview
            | HookEvent::PostAdversaryReview
            | HookEvent::VddConflict
            | HookEvent::VddConverged => {
                debug!(event = ?event, "VDD-specific event not expected in Claude Code settings");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Hook, HookEntry, HooksConfig};

    fn cmd_entry(matcher: Option<&str>, command: &str) -> HookEntry {
        HookEntry {
            matcher: matcher.map(str::to_owned),
            hooks: vec![Hook::Command {
                command: command.to_owned(),
                shell: false,
                timeout: 60,
            }],
        }
    }

    #[test]
    fn dedup_collides_on_normalized_none_and_empty() {
        // None and Some("") must collide (semantic identity, crosslink #339).
        let mut base = HooksConfig::default();
        base.pre_tool_use.push(cmd_entry(None, "base-cmd"));

        let mut other = HooksConfig::default();
        other
            .pre_tool_use
            .push(cmd_entry(Some(""), "other-cmd-empty-matcher"));

        let merged = merge_hooks_config(base, other);
        assert_eq!(
            merged.pre_tool_use.len(),
            1,
            "None and Some(\"\") matchers must be treated as the same key"
        );
        let Hook::Command { command, .. } = &merged.pre_tool_use[0].hooks[0] else {
            panic!("expected Hook::Command");
        };
        assert_eq!(command, "other-cmd-empty-matcher");
    }

    #[test]
    fn dedup_replaces_on_matcher_collision_later_source_wins() {
        let mut base = HooksConfig::default();
        base.pre_tool_use
            .push(cmd_entry(Some("Write"), "base-write"));
        base.pre_tool_use
            .push(cmd_entry(Some("Read"), "base-read"));

        let mut other = HooksConfig::default();
        other
            .pre_tool_use
            .push(cmd_entry(Some("Write"), "other-write"));

        let merged = merge_hooks_config(base, other);
        assert_eq!(merged.pre_tool_use.len(), 2);
        // Read entry preserved unchanged
        let read_e = merged
            .pre_tool_use
            .iter()
            .find(|e| e.matcher.as_deref() == Some("Read"))
            .expect("Read matcher preserved");
        let Hook::Command { command, .. } = &read_e.hooks[0] else {
            panic!("expected Hook::Command");
        };
        assert_eq!(command, "base-read");
        // Write entry replaced
        let write_e = merged
            .pre_tool_use
            .iter()
            .find(|e| e.matcher.as_deref() == Some("Write"))
            .expect("Write matcher present");
        let Hook::Command { command, .. } = &write_e.hooks[0] else {
            panic!("expected Hook::Command");
        };
        assert_eq!(command, "other-write");
    }

    #[test]
    fn dedup_preserves_non_colliding_entries_from_both_sides() {
        let mut base = HooksConfig::default();
        base.pre_tool_use.push(cmd_entry(Some("A"), "a"));
        base.pre_tool_use.push(cmd_entry(Some("B"), "b"));

        let mut other = HooksConfig::default();
        other.pre_tool_use.push(cmd_entry(Some("C"), "c"));

        let merged = merge_hooks_config(base, other);
        assert_eq!(merged.pre_tool_use.len(), 3);
        let matchers: Vec<&str> = merged
            .pre_tool_use
            .iter()
            .map(|e| e.matcher.as_deref().unwrap_or(""))
            .collect();
        assert!(matchers.contains(&"A"));
        assert!(matchers.contains(&"B"));
        assert!(matchers.contains(&"C"));
    }
}
