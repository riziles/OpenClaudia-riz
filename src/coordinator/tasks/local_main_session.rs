//! `LocalMainSessionTask` — schema-only state machine that tracks the
//! foreground/background lifecycle of the leader agent's own session
//! (crosslink #609).
//!
//! Claude Code's "main" session can be:
//!
//! - **Foreground** — the user is actively typing into it; TUI is
//!   focused on this session.
//! - **Background** — the user has explicitly handed the session off
//!   (e.g. `/background` slash command) and the leader keeps working
//!   while the TUI shows something else. The session is still
//!   process-attached.
//! - **Detached** — the leader process has exited but the session
//!   record persists for later resumption (`claudia resume`). This is
//!   terminal for the live state machine; resuming creates a fresh
//!   session task.
//!
//! Transitions follow the CC parity diagram (`local_main_session.ts`):
//!
//! ```text
//!     Foreground ⇄ Background ──► Detached
//! ```
//!
//! Every transition is idempotent — re-entering the current state is
//! a no-op. There are no error variants other than
//! [`super::TaskTransitionError`] for the illegal Detached → \* edges.

use serde::{Deserialize, Serialize};

use super::TaskTransitionError;

/// Foreground/background/detached state of the leader session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionBackgroundState {
    /// User is actively driving this session.
    Foreground,
    /// Session is process-attached but running in the background.
    Background,
    /// Process exited; session record persists for resume.
    Detached,
}

impl SessionBackgroundState {
    /// Static label used in error messages.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Foreground => "Foreground",
            Self::Background => "Background",
            Self::Detached => "Detached",
        }
    }

    /// `true` when no further transitions are allowed.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Detached)
    }
}

/// Coordinator-visible record of the leader session.
///
/// Holds the opaque session id plus the current background state.
/// Other session metadata (cwd, model selection, etc.) lives in
/// `session::Session`; this wrapper only tracks what the coordinator
/// needs for routing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalMainSessionTask {
    /// Opaque session id — matches `session::Session::id`.
    pub session_id: String,
    state: SessionBackgroundState,
}

impl LocalMainSessionTask {
    /// Build a new session task in the Foreground state — the
    /// canonical "user just started the TUI" entrypoint.
    #[must_use]
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            state: SessionBackgroundState::Foreground,
        }
    }

    /// Build a new session task in an explicit state. Useful when
    /// rehydrating from persisted state on resume.
    #[must_use]
    pub fn with_state(session_id: impl Into<String>, state: SessionBackgroundState) -> Self {
        Self {
            session_id: session_id.into(),
            state,
        }
    }

    /// Current state.
    #[must_use]
    pub const fn state(&self) -> SessionBackgroundState {
        self.state
    }

    /// Move into Foreground (idempotent).
    ///
    /// # Errors
    ///
    /// Returns [`TaskTransitionError`] from Detached.
    pub fn to_foreground(&mut self) -> Result<(), TaskTransitionError> {
        match self.state {
            SessionBackgroundState::Foreground | SessionBackgroundState::Background => {
                self.state = SessionBackgroundState::Foreground;
                Ok(())
            }
            SessionBackgroundState::Detached => {
                Err(TaskTransitionError::new(self.state.label(), "Foreground"))
            }
        }
    }

    /// Move into Background (idempotent).
    ///
    /// # Errors
    ///
    /// Returns [`TaskTransitionError`] from Detached.
    pub fn to_background(&mut self) -> Result<(), TaskTransitionError> {
        match self.state {
            SessionBackgroundState::Foreground | SessionBackgroundState::Background => {
                self.state = SessionBackgroundState::Background;
                Ok(())
            }
            SessionBackgroundState::Detached => {
                Err(TaskTransitionError::new(self.state.label(), "Background"))
            }
        }
    }

    /// Move into Detached (terminal, idempotent).
    ///
    /// Any non-terminal state can detach; Detached → Detached is a
    /// no-op. There is no edge out of Detached.
    pub const fn detach(&mut self) {
        self.state = SessionBackgroundState::Detached;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_session_starts_in_foreground() {
        let task = LocalMainSessionTask::new("sess-1");
        assert_eq!(task.state(), SessionBackgroundState::Foreground);
        assert_eq!(task.session_id, "sess-1");
    }

    #[test]
    fn foreground_to_background_and_back() {
        let mut task = LocalMainSessionTask::new("s");
        task.to_background().unwrap();
        assert_eq!(task.state(), SessionBackgroundState::Background);
        task.to_foreground().unwrap();
        assert_eq!(task.state(), SessionBackgroundState::Foreground);
    }

    #[test]
    fn transitions_are_idempotent() {
        let mut task = LocalMainSessionTask::new("s");
        task.to_foreground().expect("foreground → foreground");
        task.to_background().unwrap();
        task.to_background().expect("background → background");
        task.detach();
        task.detach(); // idempotent — no panic, still Detached.
        assert_eq!(task.state(), SessionBackgroundState::Detached);
    }

    #[test]
    fn detached_rejects_further_transitions() {
        let mut task = LocalMainSessionTask::new("s");
        task.detach();
        let err = task
            .to_foreground()
            .expect_err("detached → foreground must reject");
        assert_eq!(err.from, "Detached");
        assert_eq!(err.to, "Foreground");

        let err = task
            .to_background()
            .expect_err("detached → background must reject");
        assert_eq!(err.from, "Detached");
        assert_eq!(err.to, "Background");
    }

    #[test]
    fn detached_is_terminal_flag_matches_state_machine() {
        assert!(SessionBackgroundState::Detached.is_terminal());
        assert!(!SessionBackgroundState::Foreground.is_terminal());
        assert!(!SessionBackgroundState::Background.is_terminal());
    }

    #[test]
    fn serde_roundtrip_preserves_state() {
        let mut original = LocalMainSessionTask::new("sess-42");
        original.to_background().unwrap();
        let json = serde_json::to_string(&original).unwrap();
        assert!(
            json.contains("\"background\""),
            "kebab-case serialization expected: {json}",
        );
        let back: LocalMainSessionTask = serde_json::from_str(&json).unwrap();
        assert_eq!(original, back);
    }
}
