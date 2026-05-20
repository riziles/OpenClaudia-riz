//! `InProcessTeammateTask` — schema for a worker teammate dispatched
//! inside the leader process (crosslink #607).
//!
//! Where Claude Code spawns a child shell for a subagent, `OpenClaudia`
//! supports an in-process variant: the worker runs on the same tokio
//! runtime as the leader, sharing the provider client and tool
//! registry. This wrapper records the task's lifecycle plus the
//! coordinator-side knobs needed to police it:
//!
//! - a token budget (consumed during execution),
//! - a `nested_dispatch` flag that records whether the worker is
//!   itself allowed to spawn further in-process teammates.
//!
//! The state machine matches the dream-task convention: terminal
//! Done/Failed/Cancelled, with idempotent re-entry on equal payload.

use serde::{Deserialize, Serialize};

use crate::subagent::AgentType;

use super::TaskTransitionError;

/// State of an in-process teammate task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InProcessTeammateState {
    /// Created, not yet scheduled.
    Pending,
    /// Dispatched to a teammate; budget is being consumed.
    Running,
    /// Finished successfully — payload is the worker's final output.
    Done(String),
    /// Finished with an error — payload is the error message.
    Failed(String),
    /// Cancelled by the leader — payload is the reason.
    Cancelled(String),
}

impl InProcessTeammateState {
    /// Static label used in error messages.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::Running => "Running",
            Self::Done(_) => "Done",
            Self::Failed(_) => "Failed",
            Self::Cancelled(_) => "Cancelled",
        }
    }

    /// `true` for terminal states.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Done(_) | Self::Failed(_) | Self::Cancelled(_))
    }
}

/// Errors an in-process-teammate transition can surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum InProcessTeammateTaskError {
    /// Illegal state-machine edge.
    #[error(transparent)]
    InvalidTransition(#[from] TaskTransitionError),
    /// Budget overdraw — caller asked to consume more tokens than the
    /// remaining budget allows.
    #[error("budget exceeded: requested {requested}, remaining {remaining}")]
    BudgetExceeded {
        /// Tokens the caller wanted to consume.
        requested: u32,
        /// Tokens left in the budget at the time of the request.
        remaining: u32,
    },
}

/// Coordinator wrapper for an in-process teammate dispatch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InProcessTeammateTask {
    /// Opaque id.
    pub id: String,
    /// Agent type the leader requested.
    pub subagent_type: AgentType,
    /// Free-form prompt handed verbatim to the worker.
    pub prompt: String,
    /// Token budget cap at construction.
    budget_total: u32,
    /// Tokens consumed so far.
    budget_used: u32,
    /// Whether this worker may itself spawn nested in-process
    /// teammates. Defaults to `false` — a single layer of dispatch is
    /// the common CC-parity contract.
    pub nested_dispatch: bool,
    state: InProcessTeammateState,
}

impl InProcessTeammateTask {
    /// Build a new pending task with the supplied budget.
    #[must_use]
    pub fn new(subagent_type: AgentType, prompt: impl Into<String>, budget_tokens: u32) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            subagent_type,
            prompt: prompt.into(),
            budget_total: budget_tokens,
            budget_used: 0,
            nested_dispatch: false,
            state: InProcessTeammateState::Pending,
        }
    }

    /// Enable nested dispatch on this worker. Chainable.
    #[must_use]
    pub const fn with_nested_dispatch(mut self) -> Self {
        self.nested_dispatch = true;
        self
    }

    /// Current state.
    #[must_use]
    pub const fn state(&self) -> &InProcessTeammateState {
        &self.state
    }

    /// Total budget configured at construction.
    #[must_use]
    pub const fn budget_total(&self) -> u32 {
        self.budget_total
    }

    /// Tokens already consumed.
    #[must_use]
    pub const fn budget_used(&self) -> u32 {
        self.budget_used
    }

    /// Tokens remaining (`total − used`, saturating at zero so a stale
    /// metric from the worker can't underflow the counter).
    #[must_use]
    pub const fn budget_remaining(&self) -> u32 {
        self.budget_total.saturating_sub(self.budget_used)
    }

    /// Consume `tokens` from the budget while running.
    ///
    /// # Errors
    ///
    /// Returns [`InProcessTeammateTaskError::BudgetExceeded`] if the
    /// request would overdraw, leaving the counter unchanged. Returns
    /// [`InProcessTeammateTaskError::InvalidTransition`] if the task
    /// is not Running.
    pub fn consume_tokens(&mut self, tokens: u32) -> Result<(), InProcessTeammateTaskError> {
        if !matches!(self.state, InProcessTeammateState::Running) {
            return Err(TaskTransitionError::new(
                self.state.label(),
                "consume_tokens (requires Running)",
            )
            .into());
        }
        let remaining = self.budget_remaining();
        if tokens > remaining {
            return Err(InProcessTeammateTaskError::BudgetExceeded {
                requested: tokens,
                remaining,
            });
        }
        // `tokens <= remaining <= u32::MAX − budget_used` so the add
        // cannot overflow.
        self.budget_used += tokens;
        Ok(())
    }

    /// Transition Pending → Running.
    ///
    /// Idempotent if already Running.
    ///
    /// # Errors
    ///
    /// Returns [`InProcessTeammateTaskError::InvalidTransition`] from
    /// any terminal state.
    pub fn start(&mut self) -> Result<(), InProcessTeammateTaskError> {
        match &self.state {
            InProcessTeammateState::Pending | InProcessTeammateState::Running => {
                self.state = InProcessTeammateState::Running;
                Ok(())
            }
            other => Err(TaskTransitionError::new(other.label(), "Running").into()),
        }
    }

    /// Transition Running → Done with output.
    ///
    /// Idempotent on Done with equal payload.
    ///
    /// # Errors
    ///
    /// Returns [`InProcessTeammateTaskError::InvalidTransition`] from
    /// any non-Running, non-Done state, or from Done with a different
    /// payload.
    pub fn complete(
        &mut self,
        output: impl Into<String>,
    ) -> Result<(), InProcessTeammateTaskError> {
        let output = output.into();
        match &self.state {
            InProcessTeammateState::Running => {
                self.state = InProcessTeammateState::Done(output);
                Ok(())
            }
            InProcessTeammateState::Done(existing) if *existing == output => Ok(()),
            other => Err(TaskTransitionError::new(other.label(), "Done").into()),
        }
    }

    /// Transition Running → Failed with an error message.
    ///
    /// Idempotent on Failed with equal payload.
    ///
    /// # Errors
    ///
    /// Returns [`InProcessTeammateTaskError::InvalidTransition`] from
    /// any non-Running, non-Failed state, or from Failed with a
    /// different payload.
    pub fn fail(&mut self, error: impl Into<String>) -> Result<(), InProcessTeammateTaskError> {
        let error = error.into();
        match &self.state {
            InProcessTeammateState::Running => {
                self.state = InProcessTeammateState::Failed(error);
                Ok(())
            }
            InProcessTeammateState::Failed(existing) if *existing == error => Ok(()),
            other => Err(TaskTransitionError::new(other.label(), "Failed").into()),
        }
    }

    /// Transition any non-terminal state → Cancelled.
    ///
    /// # Errors
    ///
    /// Returns [`InProcessTeammateTaskError::InvalidTransition`] from
    /// Done or Failed; idempotent on Cancelled with equal reason.
    pub fn cancel(&mut self, reason: impl Into<String>) -> Result<(), InProcessTeammateTaskError> {
        let reason = reason.into();
        match &self.state {
            InProcessTeammateState::Pending | InProcessTeammateState::Running => {
                self.state = InProcessTeammateState::Cancelled(reason);
                Ok(())
            }
            InProcessTeammateState::Cancelled(existing) if *existing == reason => Ok(()),
            other => Err(TaskTransitionError::new(other.label(), "Cancelled").into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_with_token_consumption() {
        let mut task = InProcessTeammateTask::new(AgentType::Explore, "search", 1000);
        assert_eq!(task.budget_total(), 1000);
        assert_eq!(task.budget_remaining(), 1000);
        assert!(!task.nested_dispatch);

        task.start().unwrap();
        task.consume_tokens(300).unwrap();
        assert_eq!(task.budget_used(), 300);
        assert_eq!(task.budget_remaining(), 700);
        task.consume_tokens(200).unwrap();
        task.complete("found 3 files").unwrap();
        assert!(task.state().is_terminal());
    }

    #[test]
    fn budget_exceeded_is_returned_not_silently_clamped() {
        let mut task = InProcessTeammateTask::new(AgentType::Plan, "plan", 100);
        task.start().unwrap();
        task.consume_tokens(80).unwrap();
        let err = task.consume_tokens(30).expect_err("overdraw must error");
        assert!(matches!(
            err,
            InProcessTeammateTaskError::BudgetExceeded {
                requested: 30,
                remaining: 20
            }
        ));
        // Counter must not have advanced.
        assert_eq!(task.budget_used(), 80);
    }

    #[test]
    fn consume_tokens_rejects_when_not_running() {
        let mut task = InProcessTeammateTask::new(AgentType::Plan, "p", 100);
        let err = task
            .consume_tokens(1)
            .expect_err("consume on Pending must error");
        assert!(matches!(
            err,
            InProcessTeammateTaskError::InvalidTransition(_)
        ));
    }

    #[test]
    fn nested_dispatch_flag_roundtrips() {
        let task =
            InProcessTeammateTask::new(AgentType::GeneralPurpose, "p", 1).with_nested_dispatch();
        assert!(task.nested_dispatch);
        let json = serde_json::to_string(&task).unwrap();
        let back: InProcessTeammateTask = serde_json::from_str(&json).unwrap();
        assert!(back.nested_dispatch);
        assert_eq!(back.budget_total(), 1);
    }

    #[test]
    fn cancel_then_complete_is_rejected() {
        let mut task = InProcessTeammateTask::new(AgentType::Plan, "p", 1);
        task.start().unwrap();
        task.cancel("user").unwrap();
        let err = task
            .complete("late")
            .expect_err("complete after cancel must error");
        assert!(matches!(
            err,
            InProcessTeammateTaskError::InvalidTransition(_)
        ));
    }

    #[test]
    fn complete_is_idempotent_on_equal_payload() {
        let mut task = InProcessTeammateTask::new(AgentType::Plan, "p", 1);
        task.start().unwrap();
        task.complete("ok").unwrap();
        task.complete("ok").expect("idempotent on equal payload");
    }
}
