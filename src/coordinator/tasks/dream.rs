//! `DreamTask` — speculative planning + memory consolidation
//! (crosslink #606).
//!
//! Claude Code's "dream" pass runs the leader agent against its own
//! transcript while no user task is active and writes the resulting
//! observations back into archival memory. `OpenClaudia`'s coordinator
//! tracks the schema for that pass even though the dispatch path is
//! still future-work, so that the type is available to consumers (TUI,
//! `/agents` view, hooks) that need to render or react to the state.
//!
//! State machine:
//!
//! ```text
//!     Pending ──► Consolidating ──► Done
//!                       │
//!                       └────────► Aborted
//! ```
//!
//! `Done` and `Aborted` are terminal. Re-entering the current state is
//! a no-op (idempotent). Every other edge returns a
//! [`super::TaskTransitionError`] wrapped in [`DreamTaskError`].

use serde::{Deserialize, Serialize};

use super::TaskTransitionError;

/// State of a dream pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DreamState {
    /// Created but not yet started.
    Pending,
    /// Actively consolidating memory (leader is "thinking").
    Consolidating,
    /// Finished successfully — payload is a human-readable summary of
    /// what was consolidated.
    Done(String),
    /// Cancelled or interrupted — payload is the reason.
    Aborted(String),
}

impl DreamState {
    /// Static human-readable label used in error messages.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::Consolidating => "Consolidating",
            Self::Done(_) => "Done",
            Self::Aborted(_) => "Aborted",
        }
    }

    /// `true` when no further transitions are allowed.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Done(_) | Self::Aborted(_))
    }
}

/// Errors a dream-task transition can surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum DreamTaskError {
    /// The requested transition is not legal from the current state.
    #[error(transparent)]
    InvalidTransition(#[from] TaskTransitionError),
}

/// Coordinator-visible dream pass.
///
/// Owns only state — the actual consolidation work lives in
/// `memory::archival` and is driven externally. The coordinator
/// records the lifecycle so observers (TUI, hooks) can render it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DreamTask {
    /// Opaque identifier — UUID-shaped string.
    pub id: String,
    /// Free-form prompt that triggered this dream pass.
    pub prompt: String,
    /// Current state. Read-only outside this module.
    state: DreamState,
}

impl DreamTask {
    /// Build a new pending dream task with the supplied prompt.
    #[must_use]
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            prompt: prompt.into(),
            state: DreamState::Pending,
        }
    }

    /// Borrow the current state.
    #[must_use]
    pub const fn state(&self) -> &DreamState {
        &self.state
    }

    /// Transition Pending → Consolidating.
    ///
    /// Idempotent if already Consolidating.
    ///
    /// # Errors
    ///
    /// Returns [`DreamTaskError::InvalidTransition`] if the task is
    /// already terminal (Done/Aborted).
    pub fn start(&mut self) -> Result<(), DreamTaskError> {
        match &self.state {
            DreamState::Pending | DreamState::Consolidating => {
                self.state = DreamState::Consolidating;
                Ok(())
            }
            other => Err(TaskTransitionError::new(other.label(), "Consolidating").into()),
        }
    }

    /// Transition Consolidating → Done with a summary payload.
    ///
    /// Idempotent if already Done **with the same payload**; differing
    /// payloads on a terminal state are rejected to catch accidental
    /// double-finish on different consolidation outputs.
    ///
    /// # Errors
    ///
    /// Returns [`DreamTaskError::InvalidTransition`] from Pending or
    /// Aborted, or from a Done state whose payload differs.
    pub fn finish(&mut self, summary: impl Into<String>) -> Result<(), DreamTaskError> {
        let summary = summary.into();
        match &self.state {
            DreamState::Consolidating => {
                self.state = DreamState::Done(summary);
                Ok(())
            }
            DreamState::Done(existing) if *existing == summary => Ok(()),
            other => Err(TaskTransitionError::new(other.label(), "Done").into()),
        }
    }

    /// Transition any non-Done state → Aborted with a reason.
    ///
    /// Idempotent on Aborted with matching reason. Rejected from Done
    /// (a successful pass cannot be retroactively aborted) and from a
    /// previous Aborted with a different reason.
    ///
    /// # Errors
    ///
    /// Returns [`DreamTaskError::InvalidTransition`] from Done, or
    /// from Aborted with a different reason.
    pub fn abort(&mut self, reason: impl Into<String>) -> Result<(), DreamTaskError> {
        let reason = reason.into();
        match &self.state {
            DreamState::Pending | DreamState::Consolidating => {
                self.state = DreamState::Aborted(reason);
                Ok(())
            }
            DreamState::Aborted(existing) if *existing == reason => Ok(()),
            other => Err(TaskTransitionError::new(other.label(), "Aborted").into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_pending_to_done() {
        let mut task = DreamTask::new("consolidate today's transcripts");
        assert!(matches!(task.state(), DreamState::Pending));
        task.start().unwrap();
        assert!(matches!(task.state(), DreamState::Consolidating));
        task.finish("merged 14 memories").unwrap();
        assert!(
            matches!(task.state(), DreamState::Done(s) if s == "merged 14 memories"),
            "state must carry the summary payload",
        );
        assert!(task.state().is_terminal());
    }

    #[test]
    fn start_idempotent_in_consolidating() {
        let mut task = DreamTask::new("p");
        task.start().unwrap();
        // Re-entering Consolidating must not error.
        task.start().expect("start must be idempotent");
        assert!(matches!(task.state(), DreamState::Consolidating));
    }

    #[test]
    fn abort_from_pending_is_allowed() {
        let mut task = DreamTask::new("p");
        task.abort("user cancelled").unwrap();
        assert!(matches!(task.state(), DreamState::Aborted(r) if r == "user cancelled"),);
    }

    #[test]
    fn cannot_finish_after_abort() {
        let mut task = DreamTask::new("p");
        task.start().unwrap();
        task.abort("oom").unwrap();
        let err = task
            .finish("late summary")
            .expect_err("done after abort must reject");
        let DreamTaskError::InvalidTransition(t) = err;
        assert_eq!(t.from, "Aborted");
        assert_eq!(t.to, "Done");
    }

    #[test]
    fn cannot_abort_after_done() {
        let mut task = DreamTask::new("p");
        task.start().unwrap();
        task.finish("ok").unwrap();
        let err = task
            .abort("too late")
            .expect_err("abort after done must reject");
        let DreamTaskError::InvalidTransition(t) = err;
        assert_eq!(t.from, "Done");
        assert_eq!(t.to, "Aborted");
    }

    #[test]
    fn serde_roundtrip_preserves_state() {
        let mut original = DreamTask::new("p");
        original.start().unwrap();
        original.finish("12 items").unwrap();

        let json = serde_json::to_string(&original).unwrap();
        let back: DreamTask = serde_json::from_str(&json).unwrap();
        assert_eq!(original, back);
        assert!(matches!(back.state(), DreamState::Done(s) if s == "12 items"));
    }
}
