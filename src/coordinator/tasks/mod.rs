//! Coordinator-visible task wrappers — CC-parity types (crosslink
//! #606 / #607 / #609 / #611).
//!
//! These modules ship the **schema-only** state machines that Claude
//! Code carries for each kind of in-process work the leader agent can
//! observe:
//!
//! - [`dream::DreamTask`] (#606) — speculative planning / memory
//!   consolidation pass.
//! - [`in_process_teammate::InProcessTeammateTask`] (#607) — worker
//!   teammate dispatch with a token budget.
//! - [`local_main_session::LocalMainSessionTask`] (#609) —
//!   foreground/background lifecycle for the leader's own session.
//! - [`local_shell::LocalShellTask`] (#611) — coordinator-visible
//!   wrapper around a `BACKGROUND_SHELLS` shell with an agent owner.
//!
//! The wrappers are intentionally state-only: they do not own the
//! provider clients, MCP transports, or shell handles. Those live in
//! their respective subsystems and are looked up by id when the
//! coordinator actually wants to drive a transition.
//!
//! Every state machine in this module follows the same conventions:
//!
//! * Variants are exhaustive — a task either has a terminal state or
//!   is still in progress; there is no "unknown" sentinel.
//! * Transitions are idempotent (re-entering the current state is a
//!   no-op) but invalid transitions return a [`TaskTransitionError`].
//! * Every task type is `Serialize` + `Deserialize` so the coordinator
//!   can persist its registry.

pub mod dream;
pub mod in_process_teammate;
pub mod local_main_session;
pub mod local_shell;

pub use dream::{DreamState, DreamTask, DreamTaskError};
pub use in_process_teammate::{
    InProcessTeammateState, InProcessTeammateTask, InProcessTeammateTaskError,
};
pub use local_main_session::{LocalMainSessionTask, SessionBackgroundState};
pub use local_shell::{tasks_for_agent, LocalShellTask, LocalShellTaskState};

use serde::{Deserialize, Serialize};

/// Shared error variant for invalid state-machine transitions across
/// every coordinator task type.
///
/// We model "from → to is illegal" instead of bubbling raw strings so
/// callers can `match` on the offending edge and produce a meaningful
/// diagnostic without parsing log text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("invalid transition from {from} to {to}")]
pub struct TaskTransitionError {
    /// Human-readable name of the source state.
    pub from: String,
    /// Human-readable name of the target state.
    pub to: String,
}

impl TaskTransitionError {
    /// Build a transition error labelling the offending edge.
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
        }
    }
}
