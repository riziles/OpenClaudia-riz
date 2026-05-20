//! MCP `elicitation/create` handler (crosslink #613).
//!
//! The MCP spec defines `elicitation/create` as a *server-initiated*
//! interactive prompt: the server asks the host for additional information
//! (credentials, scopes, free-form clarification) and the host returns either
//! the supplied content or an explicit `cancel` / `decline` action. OC's
//! current MCP integration drives only tool calls, so we land the trait skeleton
//! plus the safe-by-default no-op implementation here. Concrete TUI/CLI
//! handlers will plug in later — this module is the dispatch seam they bind
//! against.
//!
//! ## Why a separate module
//!
//! `mcp.rs` is already large (≈3.5k lines) and carries the JSON-RPC transport
//! layer; elicitation is a *policy* concern (does the user want to allow this
//! prompt?) that should not be entangled with the transport. Keeping the
//! trait in its own file lets callers depend on the elicitation interface
//! without pulling the full MCP graph into their compile units.
//!
//! ## Safety default
//!
//! The provided [`NoopElicitationHandler`] always returns
//! [`ElicitationAction::Cancel`]. That is the conservative choice: a host
//! that has not opted in to interactive prompts should never silently leak
//! data to a server that asks for it, and `Cancel` is the action the MCP
//! spec defines as "the user is unavailable / the host declined to ask."
//! `Decline` is a stronger no — used when the server should treat the
//! response as a permanent refusal rather than a transient unavailability.
//! `Accept` is reserved for handlers that actually collect content.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// MCP elicitation action — the three-state response the server expects.
///
/// Matches the wire shape defined in the MCP spec for `elicitation/create`:
/// the response carries an `action` discriminator and, only when
/// `action == "accept"`, a `content` payload whose shape is dictated by the
/// server's `requestedSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ElicitationAction {
    /// The host gathered and is returning the requested content. The
    /// embedded `Value` MUST conform to the server-supplied schema; we do
    /// not validate it at this layer — that is the server's job.
    Accept(Value),
    /// The host explicitly refused the elicitation. The server should treat
    /// this as a permanent "no" for this prompt.
    Decline,
    /// The host could not (or chose not to) prompt the user — the user was
    /// unavailable, no interactive surface exists, or policy disallowed the
    /// prompt. This is the default response of [`NoopElicitationHandler`].
    Cancel,
}

/// Server-initiated request payload that the handler sees.
///
/// `message` is the human-readable prompt text from the server and
/// `requested_schema` is the JSON-Schema the server expects the response
/// `content` to satisfy when [`ElicitationAction::Accept`] is returned.
/// We carry both as opaque values because schema validation happens server-
/// side: this layer's responsibility is the user-facing decision, not
/// content well-formedness.
#[derive(Debug, Clone)]
pub struct ElicitationRequest {
    /// Prompt text the server wants the host to display.
    pub message: String,
    /// JSON Schema document the server expects the accept-content to match.
    /// Treated opaquely here — only the eventual interactive UI needs to
    /// understand it.
    pub requested_schema: Value,
    /// Name of the MCP server that originated the request — used for prompt
    /// attribution and policy decisions (e.g. "trust this server's prompts").
    pub server_name: String,
}

/// Trait every elicitation handler implements.
///
/// Implementations are kept behind `Box<dyn McpElicitationHandler>` in the
/// MCP wiring (forthcoming), so the trait must be object-safe: no generics
/// in method signatures, only `&self`/`&mut self` receivers.
///
/// # Errors
///
/// `handle` returns the action wrapped in `anyhow::Result` so an interactive
/// handler can surface I/O failures (closed stdin, broken pipe) without
/// having to invent a synthetic `Cancel`. Most production implementations
/// will absorb internal errors and downgrade them to `Cancel` — but
/// reporting up gives the caller the option to log the underlying cause.
#[async_trait]
pub trait McpElicitationHandler: Send + Sync {
    /// Display the prompt and return the user's decision.
    async fn handle(&self, req: ElicitationRequest) -> anyhow::Result<ElicitationAction>;
}

/// Safe default: never prompt, always [`ElicitationAction::Cancel`].
///
/// Used as the bootstrap implementation when no interactive surface is
/// wired up (e.g. headless tests, batch runs, daemon mode). Production
/// code that wants user prompts swaps this for a TUI- or CLI-aware
/// implementation.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopElicitationHandler;

#[async_trait]
impl McpElicitationHandler for NoopElicitationHandler {
    async fn handle(&self, req: ElicitationRequest) -> anyhow::Result<ElicitationAction> {
        tracing::debug!(
            server = %req.server_name,
            "MCP elicitation/create received in non-interactive host; returning Cancel"
        );
        Ok(ElicitationAction::Cancel)
    }
}

/// Render an [`ElicitationAction`] into the wire shape MCP expects for the
/// `elicitation/create` response.
///
/// Centralising the serialisation here keeps every transport (stdio, HTTP,
/// future SSE) on the same representation and lets unit tests pin the wire
/// format without instantiating a transport.
#[must_use]
pub fn action_to_response(action: &ElicitationAction) -> Value {
    match action {
        ElicitationAction::Accept(content) => {
            serde_json::json!({"action": "accept", "content": content})
        }
        ElicitationAction::Decline => serde_json::json!({"action": "decline"}),
        ElicitationAction::Cancel => serde_json::json!({"action": "cancel"}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req() -> ElicitationRequest {
        ElicitationRequest {
            message: "What is your favourite colour?".to_string(),
            requested_schema: serde_json::json!({"type": "string"}),
            server_name: "test-server".to_string(),
        }
    }

    #[tokio::test]
    async fn noop_handler_cancels() {
        let h = NoopElicitationHandler;
        let action = h.handle(req()).await.unwrap();
        assert_eq!(action, ElicitationAction::Cancel);
    }

    #[test]
    fn action_to_response_accept() {
        let value = serde_json::json!({"colour": "blue"});
        let wire = action_to_response(&ElicitationAction::Accept(value.clone()));
        assert_eq!(wire["action"], "accept");
        assert_eq!(wire["content"], value);
    }

    #[test]
    fn action_to_response_decline() {
        let wire = action_to_response(&ElicitationAction::Decline);
        assert_eq!(wire["action"], "decline");
        assert!(wire.get("content").is_none());
    }

    #[test]
    fn action_to_response_cancel() {
        let wire = action_to_response(&ElicitationAction::Cancel);
        assert_eq!(wire["action"], "cancel");
        assert!(wire.get("content").is_none());
    }

    /// Trait object safety regression — if a future method gets a generic
    /// parameter the trait will stop coercing to `dyn`. Pin it by coercing a
    /// concrete impl through a `&dyn` reference and round-tripping a call so
    /// the trait must remain object-safe AND callable through the vtable.
    #[tokio::test]
    async fn trait_is_object_safe() {
        let concrete = NoopElicitationHandler;
        let via_dyn: &dyn McpElicitationHandler = &concrete;
        let action = via_dyn.handle(req()).await.expect("dyn dispatch ok");
        assert_eq!(action, ElicitationAction::Cancel);
    }
}
