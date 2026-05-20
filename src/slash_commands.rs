//! Canonical list of slash commands the chat seams expose.
//!
//! This is the single source of truth referenced by both:
//! - the CLI's `slash_help()` printer (long-form list)
//! - the TUI `HelpOverlay` cheatsheet (compact scrollable overlay)
//!
//! Per crosslink #499, the two seams previously hand-maintained parallel
//! lists that drifted (TUI listed ~15 commands; CLI listed ~50). Both now
//! iterate [`SLASH_COMMANDS`] for their "Slash commands" section so adding
//! or renaming a command is a single edit here.
//!
//! Entries are grouped by section to preserve the visual structure both
//! seams render. The TUI overlay flattens to one "Slash commands" pane;
//! the CLI printer renders each section as its own heading.
//!
//! Out of scope for this table:
//! - Keybindings (TUI-only; see `tui/components/help.rs`)
//! - Shell `!cmd`, note `#text`, file `@path` syntax (CLI-only)
//! - `/plugin-name:command` open-ended plugin dispatch (registry bypass)
//!
//! When a new slash command is added to `CommandRegistry`, append it here
//! with a one-line description. The doctest below enforces non-emptiness.

/// A single slash-command entry: invocation form + one-line description.
///
/// `invocation` is the user-visible string including the leading `/` and
/// any synopsis (e.g. `"/model <name>"`). It is shown verbatim — the
/// renderers do not parse it.
#[derive(Debug, Clone, Copy)]
pub struct SlashCommand {
    /// Invocation as the user would type it, including leading `/`.
    pub invocation: &'static str,
    /// Short, single-line description rendered next to the invocation.
    pub description: &'static str,
}

/// A named group of related commands. Section titles are stable strings
/// rendered by both the CLI printer and the TUI overlay.
#[derive(Debug, Clone, Copy)]
pub struct SlashSection {
    /// Heading shown above the group (e.g. `"Slash Commands"`).
    pub title: &'static str,
    /// Commands in this section, in display order.
    pub commands: &'static [SlashCommand],
}

const fn cmd(invocation: &'static str, description: &'static str) -> SlashCommand {
    SlashCommand {
        invocation,
        description,
    }
}

/// Core slash commands — the always-available set the registry dispatches.
const CORE: &[SlashCommand] = &[
    cmd("/help, /?", "Show this help message"),
    cmd("/new, /clear", "Start a new conversation"),
    cmd("/sessions", "List saved sessions"),
    cmd("/continue <n>", "Continue session number n"),
    cmd("/export", "Export conversation to markdown"),
    cmd("/compact", "Summarize old messages to save context"),
    cmd("/editor", "Open $EDITOR for composing message"),
    cmd("/undo", "Undo last message exchange"),
    cmd("/redo", "Redo last undone exchange"),
    cmd("/exit, /quit", "Exit the chat"),
    cmd("/history", "Show conversation history"),
    cmd("/model", "Show current model and provider"),
    cmd("/model list", "List available models for current provider"),
    cmd("/model <name>", "Switch to a different model"),
    cmd("/copy", "Copy last assistant response to clipboard"),
    cmd("/init", "Initialize project config with auto-detection"),
    cmd("/review", "Review uncommitted git changes"),
    cmd(
        "/commit",
        "Stage changes and commit with auto-generated message",
    ),
    cmd("/commit-push-pr", "Commit, push, and create a pull request"),
    cmd(
        "/review <branch>",
        "Compare current branch against <branch>",
    ),
    cmd("/status", "Show session status (model, tokens, etc.)"),
    cmd("/connect", "Configure API keys for providers"),
    cmd("/theme", "List available color themes"),
    cmd("/theme <name>", "Switch to a color theme"),
    cmd("/mode", "Show current behavioral mode and list presets"),
    cmd(
        "/mode <preset>",
        "Switch behavioral mode (create/extend/safe/refactor/...)",
    ),
    cmd("/plan", "Toggle between Build and Plan modes"),
    cmd("/vim", "Toggle vim mode (show mode indicator in prompt)"),
    cmd("/effort [level]", "Set effort level (low/medium/high)"),
    cmd("/keybindings", "Show configured keyboard shortcuts"),
    cmd("/rename <title>", "Rename the current session"),
    cmd("/version", "Show version and system information"),
    cmd("/debug", "Show debug info (paths, env vars, config)"),
    cmd("/find <query>", "Fuzzy-find files in the project"),
    cmd("/doctor", "Run inline diagnostics"),
    cmd("/config", "Show current configuration"),
    cmd("/config path", "Show config file locations"),
    cmd("/cost", "Show session cost estimate"),
    cmd("/context", "Show context window usage breakdown"),
    cmd("/login", "Check authentication status"),
    cmd("/logout", "Show how to clear credentials"),
];

const MEMORY: &[SlashCommand] = &[
    cmd("/memory", "Show auto-learning stats"),
    cmd("/memory patterns", "Show learned coding patterns"),
    cmd("/memory errors", "Show known error patterns"),
    cmd("/memory prefs", "Show learned preferences"),
    cmd("/memory files", "Show file co-edit relationships"),
    cmd(
        "/memory reset",
        "Reset all learned data (with confirmation)",
    ),
];

const ACTIVITY: &[SlashCommand] = &[
    cmd("/activity", "Show current session activities"),
    cmd("/activity sessions", "Show recent session summaries"),
    cmd("/activity files", "Show files modified this session"),
    cmd("/activity issues", "Show issues worked this session"),
];

const PLUGIN: &[SlashCommand] = &[
    cmd("/plugin", "List installed plugins"),
    cmd("/plugin install", "Install a plugin"),
    cmd("/plugin manage", "Manage installed plugins"),
    cmd("/plugin help", "Show all plugin commands"),
    cmd("/<plugin>:<cmd>", "Run a plugin command"),
];

const SKILLS: &[SlashCommand] = &[
    cmd("/skill", "List available skills"),
    cmd(
        "/skill <name>",
        "Invoke a skill (inject prompt as next message)",
    ),
];

/// All sections in the order they should render.
///
/// Iterated by both `slash_help()` (CLI printer) and `HelpOverlay`
/// (TUI cheatsheet). Adding a section here automatically shows up in
/// both seams.
pub const SLASH_SECTIONS: &[SlashSection] = &[
    SlashSection {
        title: "Slash Commands",
        commands: CORE,
    },
    SlashSection {
        title: "Memory Commands (auto-learning)",
        commands: MEMORY,
    },
    SlashSection {
        title: "Activity Commands",
        commands: ACTIVITY,
    },
    SlashSection {
        title: "Plugin Commands",
        commands: PLUGIN,
    },
    SlashSection {
        title: "Skill Commands",
        commands: SKILLS,
    },
];

/// Flat iterator over every command across every section.
///
/// Useful for the TUI overlay (one pane) and for tab-completion sources.
pub fn all_commands() -> impl Iterator<Item = &'static SlashCommand> {
    SLASH_SECTIONS.iter().flat_map(|s| s.commands.iter())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Guard: if someone empties the table by accident, this fails rather
    /// than shipping an empty help screen in both seams.
    #[test]
    fn sections_non_empty() {
        assert!(!SLASH_SECTIONS.is_empty());
        for section in SLASH_SECTIONS {
            assert!(!section.title.is_empty());
            assert!(
                !section.commands.is_empty(),
                "section {} has no commands",
                section.title
            );
            for c in section.commands {
                assert!(c.invocation.starts_with('/'));
                assert!(!c.description.is_empty());
            }
        }
    }

    /// Sanity: must include the universally expected commands. If any of
    /// these disappear the help table is almost certainly broken.
    #[test]
    fn table_includes_canonical_commands() {
        let invocations: Vec<&str> = all_commands().map(|c| c.invocation).collect();
        for canonical in [
            "/help, /?",
            "/new, /clear",
            "/exit, /quit",
            "/model",
            "/compact",
        ] {
            assert!(
                invocations.contains(&canonical),
                "canonical command {canonical} missing from SLASH_SECTIONS"
            );
        }
    }
}
