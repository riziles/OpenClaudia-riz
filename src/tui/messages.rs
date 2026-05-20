//! Scrollable message list for the TUI.

use std::str::FromStr;
use std::time::Instant;

use ratatui::{
    prelude::*,
    widgets::{Paragraph, Wrap},
};

// ─── MessageKind ────────────────────────────────────────────────────────────

/// The semantic kind of a [`DisplayMessage`].
///
/// Encodes what type of content the message carries and how it should be
/// rendered. Replaces the previous twin-boolean `(is_error, is_thinking)` flags
/// and the stringly-typed `role` field — invalid state combinations are now
/// unrepresentable at the type level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageKind {
    /// A message typed by the human user.
    User,
    /// A completed assistant response.
    Assistant,
    /// A collapsed thinking summary (e.g. "Thought for 1.2s").
    Thinking,
    /// An informational system message (no error).
    SystemInfo,
    /// An error-level system message.
    SystemError,
    /// Tool invocation header — the tool started but has not yet returned.
    ToolStart {
        /// Tool name shown in the header line.
        name: String,
    },
    /// Tool completed successfully.
    ToolOk {
        /// Tool name shown in the result line.
        name: String,
    },
    /// Tool completed with a failure / non-zero exit.
    ToolErr {
        /// Tool name shown in the result line.
        name: String,
    },
}

impl MessageKind {
    /// Returns `true` when this kind renders with error styling.
    #[must_use]
    pub const fn is_error(&self) -> bool {
        matches!(self, Self::SystemError | Self::ToolErr { .. })
    }

    /// Returns `true` when this kind carries thinking content.
    #[must_use]
    pub const fn is_thinking(&self) -> bool {
        matches!(self, Self::Thinking)
    }

    /// Borrow the tool name when available, without allocating.
    #[must_use]
    pub const fn tool_name(&self) -> Option<&str> {
        match self {
            Self::ToolStart { name } | Self::ToolOk { name } | Self::ToolErr { name } => {
                Some(name.as_str())
            }
            _ => None,
        }
    }
}

// ─── Role ───────────────────────────────────────────────────────────────────

/// Wire-format role for session messages.
///
/// Used exclusively at the serde boundary (session JSON persistence and
/// resume). Internal code compares against enum variants — no string literals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    System,
    Tool,
}

impl Role {
    /// Return the lowercase wire-format string for this role.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::System => "system",
            Self::Tool => "tool",
        }
    }
}

/// Parse a role from a wire-format string.
///
/// Unknown roles round-trip to `System` rather than failing, matching
/// the previous fallback behaviour of the `_ =>` arm.
impl FromStr for Role {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "user" => Self::User,
            "assistant" => Self::Assistant,
            "tool" => Self::Tool,
            _ => Self::System,
        })
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ─── Mode ───────────────────────────────────────────────────────────────────

/// The agent operating mode.
///
/// Replaces `TuiSession.mode: String` and `App.mode: String`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum Mode {
    /// Full-access mode — the agent can write files, run commands, etc.
    #[default]
    Build,
    /// Read-only suggestions-only mode.
    Plan,
}

impl Mode {
    /// Return the display name used in the status bar and system messages.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Build => "Build",
            Self::Plan => "Plan",
        }
    }

    /// Return a one-line description shown in `/mode` system messages.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::Build => "Full access — can make changes",
            Self::Plan => "Read-only — suggestions only",
        }
    }

    /// Toggle between `Build` and `Plan`.
    #[must_use]
    pub const fn toggled(self) -> Self {
        match self {
            Self::Build => Self::Plan,
            Self::Plan => Self::Build,
        }
    }
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Mode {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "Plan" => Self::Plan,
            _ => Self::Build,
        })
    }
}

// ─── EffortLevel ────────────────────────────────────────────────────────────

/// The reasoning-effort level forwarded to the provider.
///
/// Replaces `App.effort_level: String` and `ApiTurnParams.effort_level: String`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EffortLevel {
    Low,
    Medium,
    High,
}

impl EffortLevel {
    /// Return the lowercase wire string expected by provider adapters.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    /// Return the Unicode bullet symbol used in the status bar.
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Low => "\u{25CB}",
            Self::Medium => "\u{25D0}",
            Self::High => "\u{25CF}",
        }
    }

    /// Cycle through Low → Medium → High → Low.
    #[must_use]
    pub const fn cycled(self) -> Self {
        match self {
            Self::Low => Self::Medium,
            Self::Medium => Self::High,
            Self::High => Self::Low,
        }
    }
}

impl std::fmt::Display for EffortLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for EffortLevel {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "low" => Self::Low,
            "high" => Self::High,
            _ => Self::Medium,
        })
    }
}

// ─── DisplayMessage ──────────────────────────────────────────────────────────

/// A single display message in the conversation.
///
/// The `kind` field carries all the semantic information that was previously
/// split across `role: String`, `is_error: bool`, `is_thinking: bool`, and
/// `tool_name: Option<String>`.
#[derive(Debug, Clone)]
pub struct DisplayMessage {
    pub kind: MessageKind,
    pub content: String,
}

impl DisplayMessage {
    /// Convenience constructor for a `SystemInfo` message.
    #[must_use]
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            kind: MessageKind::SystemInfo,
            content: content.into(),
        }
    }

    /// Convenience constructor for a `SystemError` message.
    #[must_use]
    pub fn error(content: impl Into<String>) -> Self {
        Self {
            kind: MessageKind::SystemError,
            content: content.into(),
        }
    }

    /// Convenience constructor for a `User` message.
    #[must_use]
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            kind: MessageKind::User,
            content: content.into(),
        }
    }

    /// Convenience constructor for an `Assistant` message.
    #[must_use]
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            kind: MessageKind::Assistant,
            content: content.into(),
        }
    }
}

// ─── MessageList ────────────────────────────────────────────────────────────

/// Scrollable message list with streaming support.
pub struct MessageList {
    pub messages: Vec<DisplayMessage>,
    pub scroll_offset: u16,
    pub streaming_text: String,
    pub is_streaming: bool,
    /// True while thinking/reasoning deltas are arriving for the current
    /// response, before any regular text has streamed in.
    pub is_thinking_now: bool,
    /// When the current thinking block started. Used to render elapsed
    /// seconds next to the `∴ Thinking…` indicator.
    thinking_start: Option<Instant>,
    /// Hidden accumulator for the full thinking stream — not rendered,
    /// but kept so callers could persist it alongside the assistant turn.
    pub thinking_buffer: String,
}

impl MessageList {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            messages: Vec::new(),
            scroll_offset: 0,
            streaming_text: String::new(),
            is_streaming: false,
            is_thinking_now: false,
            thinking_start: None,
            thinking_buffer: String::new(),
        }
    }

    /// Record a thinking-delta chunk. The text is accumulated into a
    /// hidden buffer (for session persistence) and the `∴ Thinking…`
    /// indicator is activated; the text itself is intentionally not
    /// rendered — matching Claude Code's collapsed thinking UX.
    pub fn push_thinking(&mut self, text: &str) {
        if self.thinking_start.is_none() {
            self.thinking_start = Some(Instant::now());
        }
        self.is_thinking_now = true;
        self.thinking_buffer.push_str(text);
    }

    /// Finalize the current thinking block: replace the live indicator
    /// with a collapsed `∴ Thought for X.Xs` header message and reset
    /// the timer. Safe to call repeatedly — no-op when not thinking.
    pub fn finish_thinking(&mut self) {
        if !self.is_thinking_now {
            return;
        }
        let duration = self
            .thinking_start
            .map_or(0.0, |start| start.elapsed().as_secs_f64());
        self.messages.push(DisplayMessage {
            kind: MessageKind::Thinking,
            content: format!("Thought for {duration:.1}s"),
        });
        self.is_thinking_now = false;
        self.thinking_start = None;
        self.thinking_buffer.clear();
        self.scroll_to_bottom();
    }

    /// Remove the last `count` messages from the display list.
    ///
    /// Saturates at zero — passing a `count` larger than the current length
    /// truncates the entire list rather than panicking, and `count == 0` is a
    /// no-op.
    pub fn pop_last(&mut self, count: usize) {
        self.messages
            .truncate(self.messages.len().saturating_sub(count));
    }

    /// Number of messages in the display list.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.messages.len()
    }

    /// Returns `true` if there are no messages in the display list.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    pub fn add(&mut self, msg: DisplayMessage) {
        self.messages.push(msg);
        self.scroll_to_bottom();
    }

    pub fn append_streaming(&mut self, text: &str) {
        self.streaming_text.push_str(text);
        self.is_streaming = true;
    }

    pub fn finish_streaming(&mut self) {
        if !self.streaming_text.is_empty() {
            self.messages.push(DisplayMessage {
                kind: MessageKind::Assistant,
                content: std::mem::take(&mut self.streaming_text),
            });
        }
        self.is_streaming = false;
    }

    pub const fn scroll_up(&mut self, n: u16) {
        self.scroll_offset = self.scroll_offset.saturating_add(n);
    }

    pub const fn scroll_down(&mut self, n: u16) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    pub const fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    /// Append styled lines for the welcome banner system message.
    fn append_welcome_lines<'a>(out: &mut Vec<Line<'a>>, content: &'a str) {
        for line in content.lines() {
            let styled = if line.starts_with("OpenClaudia v") {
                Line::from(vec![
                    Span::styled(
                        "OpenClaudia",
                        Style::default()
                            .fg(Color::Rgb(147, 112, 219))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        &line["OpenClaudia".len()..],
                        Style::default().fg(Color::Rgb(218, 165, 32)),
                    ),
                ])
            } else if line.starts_with("Provider:") {
                Line::from(Span::styled(
                    line,
                    Style::default().fg(Color::Rgb(147, 112, 219)),
                ))
            } else if line.starts_with("Model:") {
                Line::from(Span::styled(
                    line,
                    Style::default().fg(Color::Rgb(218, 165, 32)),
                ))
            } else if line.starts_with("Welcome") {
                Line::from(Span::styled(
                    line,
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ))
            } else {
                Line::from(Span::styled(line, Style::default().fg(Color::DarkGray)))
            };
            out.push(styled);
        }
    }

    /// Append rendered lines for a system-role message to `out`.
    fn append_system_lines<'a>(out: &mut Vec<Line<'a>>, msg: &'a DisplayMessage) {
        if msg.content.contains("OpenClaudia v") {
            Self::append_welcome_lines(out, &msg.content);
        } else {
            for line in msg.content.lines() {
                out.push(Line::from(Span::styled(
                    format!("  {line}"),
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                )));
            }
        }
        out.push(Line::from(""));
    }

    /// Append rendered lines for a tool-result message to `out`.
    fn append_tool_lines<'a>(out: &mut Vec<Line<'a>>, msg: &'a DisplayMessage) {
        let tool_name = msg.kind.tool_name().unwrap_or("tool");
        if msg.kind.is_error() {
            out.push(Line::from(Span::styled(
                format!("  \u{2717} {tool_name}"),
                Style::default().fg(Color::Red),
            )));
        } else {
            out.push(Line::from(Span::styled(
                format!("  \u{2713} {tool_name}"),
                Style::default().fg(Color::Green),
            )));
        }
        let preview = if msg.content.len() > 200 {
            format!("{}...", crate::tools::safe_truncate(&msg.content, 197))
        } else {
            msg.content.clone()
        };
        for line in preview.lines().take(5) {
            out.push(Line::from(Span::styled(
                format!("    {line}"),
                Style::default().fg(Color::DarkGray),
            )));
        }
        out.push(Line::from(""));
    }

    /// Append rendered lines for a single message to `out`.
    fn append_message_lines<'a>(out: &mut Vec<Line<'a>>, msg: &'a DisplayMessage) {
        match &msg.kind {
            MessageKind::SystemInfo | MessageKind::SystemError => {
                Self::append_system_lines(out, msg);
            }
            MessageKind::User => {
                out.push(Line::from(Span::styled(
                    "\u{203A} user",
                    Style::default()
                        .fg(Color::Rgb(100, 180, 255))
                        .add_modifier(Modifier::BOLD),
                )));
                for line in msg.content.lines() {
                    out.push(Line::from(format!("  {line}")));
                }
                out.push(Line::from(""));
            }
            MessageKind::Assistant => {
                out.push(Line::from(Span::styled(
                    "\u{23BF} Claudia",
                    Style::default()
                        .fg(Color::Rgb(147, 112, 219))
                        .add_modifier(Modifier::BOLD),
                )));
                for line in msg.content.lines() {
                    out.push(Line::from(format!("  {line}")));
                }
                out.push(Line::from(""));
            }
            MessageKind::Thinking => {
                out.push(Line::from(Span::styled(
                    format!("  \u{2234} {}", msg.content),
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                )));
                out.push(Line::from(""));
            }
            MessageKind::ToolStart { .. }
            | MessageKind::ToolOk { .. }
            | MessageKind::ToolErr { .. } => {
                Self::append_tool_lines(out, msg);
            }
        }
    }

    /// Build ratatui Lines for rendering.
    fn build_lines(&self) -> Vec<Line<'_>> {
        let mut lines: Vec<Line> = Vec::new();

        for msg in &self.messages {
            Self::append_message_lines(&mut lines, msg);
        }

        // Live thinking indicator (while thinking deltas are arriving)
        if self.is_thinking_now {
            let elapsed = self
                .thinking_start
                .map_or(0.0, |s| s.elapsed().as_secs_f64());
            lines.push(Line::from(Span::styled(
                format!("  \u{2234} Thinking\u{2026} ({elapsed:.1}s)"),
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            )));
            lines.push(Line::from(""));
        }

        // Streaming content
        if self.is_streaming && !self.streaming_text.is_empty() {
            lines.push(Line::from(Span::styled(
                "\u{23BF} Claudia",
                Style::default()
                    .fg(Color::Rgb(147, 112, 219))
                    .add_modifier(Modifier::BOLD),
            )));
            for line in self.streaming_text.lines() {
                lines.push(Line::from(format!("  {line}")));
            }
            // Cursor indicator
            lines.push(Line::from(Span::styled(
                "  \u{2588}",
                Style::default().fg(Color::Rgb(147, 112, 219)),
            )));
        }

        lines
    }

    /// Render the message list into a frame area.
    /// Content is anchored to the bottom — empty space is at the top, not below.
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let mut lines = self.build_lines();
        #[allow(clippy::cast_possible_truncation)] // line count bounded by terminal height
        let total = lines.len() as u16;
        let visible = area.height;

        // Pad the top with empty lines so content anchors to the bottom
        if total < visible {
            let pad = (visible - total) as usize;
            let mut padded = vec![Line::from(""); pad];
            padded.append(&mut lines);
            lines = padded;
        }

        #[allow(clippy::cast_possible_truncation)]
        let total = lines.len() as u16;
        let scroll = if total > visible {
            (total - visible).saturating_sub(self.scroll_offset)
        } else {
            0
        };

        let paragraph = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0));

        frame.render_widget(paragraph, area);
    }
}

impl Default for MessageList {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── MessageKind tests ────────────────────────────────────────────────────

    #[test]
    fn message_kind_is_error_only_for_error_variants() {
        assert!(!MessageKind::User.is_error());
        assert!(!MessageKind::Assistant.is_error());
        assert!(!MessageKind::Thinking.is_error());
        assert!(!MessageKind::SystemInfo.is_error());
        assert!(!MessageKind::ToolStart {
            name: "bash".into()
        }
        .is_error());
        assert!(!MessageKind::ToolOk {
            name: "bash".into()
        }
        .is_error());

        assert!(MessageKind::SystemError.is_error());
        assert!(MessageKind::ToolErr {
            name: "bash".into()
        }
        .is_error());
    }

    #[test]
    fn message_kind_is_thinking_only_for_thinking_variant() {
        assert!(MessageKind::Thinking.is_thinking());
        assert!(!MessageKind::User.is_thinking());
        assert!(!MessageKind::SystemError.is_thinking());
    }

    #[test]
    fn message_kind_tool_name_accessor() {
        assert_eq!(
            MessageKind::ToolStart {
                name: "read_file".into()
            }
            .tool_name(),
            Some("read_file")
        );
        assert_eq!(
            MessageKind::ToolOk {
                name: "write_file".into()
            }
            .tool_name(),
            Some("write_file")
        );
        assert_eq!(
            MessageKind::ToolErr {
                name: "bash".into()
            }
            .tool_name(),
            Some("bash")
        );
        assert_eq!(MessageKind::User.tool_name(), None);
        assert_eq!(MessageKind::SystemInfo.tool_name(), None);
    }

    // ── Role tests ───────────────────────────────────────────────────────────

    #[test]
    fn role_round_trip_via_as_str_and_from_str() {
        for (wire, expected) in [
            ("user", Role::User),
            ("assistant", Role::Assistant),
            ("system", Role::System),
            ("tool", Role::Tool),
        ] {
            let parsed: Role = wire.parse().unwrap();
            assert_eq!(parsed, expected, "parse failed for {wire}");
            assert_eq!(parsed.as_str(), wire, "as_str mismatch for {wire}");
        }
    }

    #[test]
    fn role_unknown_input_falls_back_to_system() {
        let r: Role = "thinking".parse().unwrap();
        assert_eq!(r, Role::System);
        let r2: Role = "".parse().unwrap();
        assert_eq!(r2, Role::System);
    }

    #[test]
    fn role_display_matches_as_str() {
        for role in [Role::User, Role::Assistant, Role::System, Role::Tool] {
            assert_eq!(role.to_string(), role.as_str());
        }
    }

    // ── Mode tests ───────────────────────────────────────────────────────────

    #[test]
    fn mode_toggle_is_involution() {
        assert_eq!(Mode::Build.toggled(), Mode::Plan);
        assert_eq!(Mode::Plan.toggled(), Mode::Build);
    }

    #[test]
    fn mode_round_trip_from_str() {
        assert_eq!("Build".parse::<Mode>().unwrap(), Mode::Build);
        assert_eq!("Plan".parse::<Mode>().unwrap(), Mode::Plan);
        // Unknown falls back to Build
        assert_eq!("unknown".parse::<Mode>().unwrap(), Mode::Build);
    }

    #[test]
    fn mode_display_matches_as_str() {
        assert_eq!(Mode::Build.to_string(), "Build");
        assert_eq!(Mode::Plan.to_string(), "Plan");
    }

    // ── EffortLevel tests ────────────────────────────────────────────────────

    #[test]
    fn effort_level_cycle_is_periodic() {
        assert_eq!(EffortLevel::Low.cycled(), EffortLevel::Medium);
        assert_eq!(EffortLevel::Medium.cycled(), EffortLevel::High);
        assert_eq!(EffortLevel::High.cycled(), EffortLevel::Low);
    }

    #[test]
    fn effort_level_round_trip_from_str() {
        assert_eq!("low".parse::<EffortLevel>().unwrap(), EffortLevel::Low);
        assert_eq!(
            "medium".parse::<EffortLevel>().unwrap(),
            EffortLevel::Medium
        );
        assert_eq!("high".parse::<EffortLevel>().unwrap(), EffortLevel::High);
        // Unknown falls back to Medium
        assert_eq!(
            "unknown".parse::<EffortLevel>().unwrap(),
            EffortLevel::Medium
        );
    }

    #[test]
    fn effort_level_display_matches_as_str() {
        for level in [EffortLevel::Low, EffortLevel::Medium, EffortLevel::High] {
            assert_eq!(level.to_string(), level.as_str());
        }
    }

    // ── MessageList integration tests ────────────────────────────────────────

    #[test]
    fn test_add_and_count() {
        let mut ml = MessageList::new();
        ml.add(DisplayMessage::user("hello"));
        assert_eq!(ml.messages.len(), 1);
    }

    #[test]
    fn test_streaming() {
        let mut ml = MessageList::new();
        ml.append_streaming("hello ");
        ml.append_streaming("world");
        assert!(ml.is_streaming);
        assert_eq!(ml.streaming_text, "hello world");
        ml.finish_streaming();
        assert!(!ml.is_streaming);
        assert_eq!(ml.messages.len(), 1);
        assert_eq!(ml.messages[0].content, "hello world");
        assert_eq!(ml.messages[0].kind, MessageKind::Assistant);
    }

    #[test]
    fn thinking_indicator_lifecycle() {
        let mut ml = MessageList::new();
        // No-op when not thinking.
        ml.finish_thinking();
        assert!(!ml.is_thinking_now);
        assert_eq!(ml.messages.len(), 0);

        // Deltas activate the indicator and accumulate hidden buffer.
        ml.push_thinking("first ");
        ml.push_thinking("second");
        assert!(ml.is_thinking_now);
        assert_eq!(ml.thinking_buffer, "first second");

        // Finalize emits a collapsed summary and clears state.
        ml.finish_thinking();
        assert!(!ml.is_thinking_now);
        assert!(ml.thinking_buffer.is_empty());
        assert_eq!(ml.messages.len(), 1);
        assert_eq!(ml.messages[0].kind, MessageKind::Thinking);
        assert!(ml.messages[0].content.starts_with("Thought for "));
    }

    #[test]
    fn pop_last_handles_saturating_and_zero_edges() {
        let mut ml = MessageList::new();
        ml.add(DisplayMessage::user("a"));
        ml.add(DisplayMessage::user("b"));
        ml.add(DisplayMessage::user("c"));
        assert_eq!(ml.messages.len(), 3);

        // Edge 1: count == 0 is a no-op (must not touch the list).
        ml.pop_last(0);
        assert_eq!(ml.messages.len(), 3, "count == 0 must leave list intact");

        // Edge 2: count > len saturates at zero (must not panic).
        ml.pop_last(usize::MAX);
        assert_eq!(
            ml.messages.len(),
            0,
            "count > len must truncate to empty without panicking"
        );

        // Sanity: a normal in-range count still works.
        ml.add(DisplayMessage::user("x"));
        ml.add(DisplayMessage::user("y"));
        ml.add(DisplayMessage::user("z"));
        ml.pop_last(2);
        assert_eq!(ml.messages.len(), 1);
        assert_eq!(ml.messages[0].content, "x");
    }

    #[test]
    fn test_scroll() {
        let mut ml = MessageList::new();
        ml.scroll_up(5);
        assert_eq!(ml.scroll_offset, 5);
        ml.scroll_down(3);
        assert_eq!(ml.scroll_offset, 2);
        ml.scroll_to_bottom();
        assert_eq!(ml.scroll_offset, 0);
    }
}
