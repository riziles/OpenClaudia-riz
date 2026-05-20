//! In-process MCP transport (crosslink #622).
//!
//! Stdio and HTTP transports cover the two ways CC talks to *external*
//! MCP servers, but Claude Code also exposes an **in-process** path:
//! when the harness ships a server implementation directly (e.g. a
//! built-in knowledge server, a fixtures server in tests), there is no
//! reason to serialise the request, pipe it through a child process,
//! and parse it back. The in-process transport calls a Rust handler
//! directly inside the same task.
//!
//! Shape:
//!
//! * [`McpServerCallable`] — async trait every in-process MCP server
//!   implements. One method (`call`) that returns a JSON-RPC-style
//!   `Result<Value, McpError>`.
//! * [`InProcessTransport`] — thin [`McpTransport`] adapter that
//!   wraps an `Arc<dyn McpServerCallable>` and forwards requests to
//!   the handler. Implements `close` as a no-op (there is no child
//!   process to reap).
//!
//! The transport is intentionally not a full server: it does not own
//! tool definitions or capabilities — those still come from the
//! standard `initialize`/`tools/list` handshake. That keeps the
//! manager-side code (`McpManager`) agnostic to whether a given
//! server is in-process or out-of-process.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::mcp::{McpError, McpTransport};

/// Trait every in-process MCP server implements.
///
/// Sync API surface deliberately mirrors `McpTransport::request` so
/// the adapter is a near-zero-cost forwarder. Implementors get the
/// method name and an optional `params` value, and return a JSON-RPC
/// result body (or an [`McpError`]).
#[async_trait]
pub trait McpServerCallable: Send + Sync {
    /// Handle a single MCP request.
    ///
    /// # Errors
    ///
    /// Implementors return [`McpError`] for anything the manager
    /// would otherwise see over the wire (`Protocol`, `ToolNotFound`,
    /// `Timeout`, etc.). The transport does not synthesise errors of
    /// its own — whatever the callable returns is what the manager
    /// observes.
    async fn call(&self, method: &str, params: Option<Value>) -> Result<Value, McpError>;
}

/// `McpTransport` adapter for an in-process server.
pub struct InProcessTransport {
    server: Arc<dyn McpServerCallable>,
}

impl InProcessTransport {
    /// Wrap an `Arc<dyn McpServerCallable>` as a transport.
    ///
    /// `Arc` is taken by value (not constructed here) so the caller
    /// can keep their own reference and inspect the server's internal
    /// state from tests.
    #[must_use]
    pub const fn new(server: Arc<dyn McpServerCallable>) -> Self {
        Self { server }
    }
}

#[async_trait]
impl McpTransport for InProcessTransport {
    async fn request(&self, method: &str, params: Option<Value>) -> Result<Value, McpError> {
        self.server.call(method, params).await
    }

    async fn close(&self) -> Result<(), McpError> {
        // Nothing to reap — in-process server's lifetime is bound to
        // the `Arc` held by the transport. Dropping the transport
        // releases this reference; the server stays alive as long as
        // any other clone exists.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Tiny test double that counts calls and echoes the params back.
    struct EchoServer {
        calls: AtomicUsize,
    }

    impl EchoServer {
        const fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl McpServerCallable for EchoServer {
        async fn call(&self, method: &str, params: Option<Value>) -> Result<Value, McpError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if method == "fail" {
                return Err(McpError::Protocol("test failure".into()));
            }
            Ok(json!({"method": method, "params": params}))
        }
    }

    #[tokio::test]
    async fn transport_forwards_request_to_callable() {
        let server = Arc::new(EchoServer::new());
        let transport = InProcessTransport::new(server.clone());

        let resp = transport
            .request("tools/list", Some(json!({"foo": 1})))
            .await
            .unwrap();
        assert_eq!(resp["method"], "tools/list");
        assert_eq!(resp["params"]["foo"], 1);
        assert_eq!(server.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn transport_propagates_callable_errors_verbatim() {
        let server = Arc::new(EchoServer::new());
        let transport = InProcessTransport::new(server);

        let err = transport.request("fail", None).await.unwrap_err();
        assert!(
            matches!(err, McpError::Protocol(ref m) if m == "test failure"),
            "transport must not wrap callable errors: got {err:?}",
        );
    }

    #[tokio::test]
    async fn close_is_noop() {
        let server = Arc::new(EchoServer::new());
        let transport = InProcessTransport::new(server.clone());
        transport.close().await.unwrap();
        // After close, the server is still callable because the
        // transport never owned the OS-level resource — calls go
        // straight through to the callable.
        transport.request("ping", None).await.unwrap();
        assert_eq!(server.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn transport_is_object_safe_behind_trait_object() {
        // Compile-time check: McpTransport must remain object-safe
        // after #622, so the manager can keep using
        // `Box<dyn McpTransport>` uniformly across stdio / http /
        // in-process.
        let server = Arc::new(EchoServer::new());
        let transport: Box<dyn McpTransport> = Box::new(InProcessTransport::new(server));
        let resp = transport.request("ok", None).await.unwrap();
        assert_eq!(resp["method"], "ok");
    }
}
