//! End-to-end tests for `mcp` wire-shape data types —
//! `McpTool`, `McpResource`, `McpCapabilities`,
//! `ToolsCapability`, `McpServerInfo`, `McpTransportKind`.
//!
//! Sprint 123 of the verification effort. Sprint 79 covered
//! `InProcessTransport` + `McpError`; this file pins the
//! wire-shape data types that cross the MCP JSON-RPC
//! boundary (tools/list response shape, resources/list,
//! initialize response capabilities + serverInfo).

#![allow(clippy::missing_panics_doc)]
#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use openclaudia::mcp::{
    McpCapabilities, McpResource, McpServerInfo, McpTool, McpTransportKind, ToolsCapability,
};
use serde_json::json;

// ───────────────────────────────────────────────────────────────────────────
// Section A — McpTool serde shape
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn mcp_tool_required_name_field_round_trips() {
    let tool = McpTool {
        name: "bash".to_string(),
        description: None,
        input_schema: None,
    };
    let json_str = serde_json::to_string(&tool).expect("ser");
    let back: McpTool = serde_json::from_str(&json_str).expect("de");
    assert_eq!(back.name, "bash");
    assert!(back.description.is_none());
    assert!(back.input_schema.is_none());
}

#[test]
fn mcp_tool_with_description_and_input_schema_round_trips() {
    let tool = McpTool {
        name: "read".to_string(),
        description: Some("Read a file".to_string()),
        input_schema: Some(json!({
            "type": "object",
            "properties": {"path": {"type": "string"}},
            "required": ["path"]
        })),
    };
    let json_str = serde_json::to_string(&tool).expect("ser");
    let back: McpTool = serde_json::from_str(&json_str).expect("de");
    assert_eq!(back.name, tool.name);
    assert_eq!(back.description, tool.description);
    assert_eq!(back.input_schema, tool.input_schema);
}

#[test]
fn mcp_tool_input_schema_wire_field_is_camel_case() {
    let tool = McpTool {
        name: "x".to_string(),
        description: None,
        input_schema: Some(json!({"type": "object"})),
    };
    let json_str = serde_json::to_string(&tool).expect("ser");
    // PINS WIRE: input_schema ↔ inputSchema rename.
    assert!(
        json_str.contains("\"inputSchema\""),
        "MUST use camelCase inputSchema on wire; got {json_str:?}"
    );
}

#[test]
fn mcp_tool_deserializes_from_camel_case_input_schema() {
    let json = r#"{
        "name": "bash",
        "description": "Run a shell command",
        "inputSchema": {"type": "object"}
    }"#;
    let tool: McpTool = serde_json::from_str(json).expect("de");
    assert_eq!(tool.name, "bash");
    assert!(tool.input_schema.is_some());
}

#[test]
fn mcp_tool_deserializes_with_only_name() {
    let json = r#"{"name": "minimal"}"#;
    let tool: McpTool = serde_json::from_str(json).expect("de");
    assert_eq!(tool.name, "minimal");
    assert!(tool.description.is_none());
    assert!(tool.input_schema.is_none());
}

#[test]
fn mcp_tool_clone_preserves_all_fields() {
    let original = McpTool {
        name: "x".to_string(),
        description: Some("d".to_string()),
        input_schema: Some(json!({"k": "v"})),
    };
    let cloned = original.clone();
    assert_eq!(cloned.name, original.name);
    assert_eq!(cloned.description, original.description);
    assert_eq!(cloned.input_schema, original.input_schema);
}

// ───────────────────────────────────────────────────────────────────────────
// Section B — McpResource serde shape
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn mcp_resource_required_fields_round_trip() {
    let r = McpResource {
        uri: "file:///x".to_string(),
        name: "file-x".to_string(),
        description: None,
        mime_type: None,
    };
    let json_str = serde_json::to_string(&r).expect("ser");
    let back: McpResource = serde_json::from_str(&json_str).expect("de");
    assert_eq!(back.uri, "file:///x");
    assert_eq!(back.name, "file-x");
}

#[test]
fn mcp_resource_full_shape_round_trips() {
    let r = McpResource {
        uri: "file:///doc.txt".to_string(),
        name: "Documentation".to_string(),
        description: Some("Project documentation".to_string()),
        mime_type: Some("text/plain".to_string()),
    };
    let json_str = serde_json::to_string(&r).expect("ser");
    let back: McpResource = serde_json::from_str(&json_str).expect("de");
    assert_eq!(back.description, r.description);
    assert_eq!(back.mime_type, r.mime_type);
}

#[test]
fn mcp_resource_mime_type_wire_field_is_camel_case() {
    let r = McpResource {
        uri: "x".to_string(),
        name: "n".to_string(),
        description: None,
        mime_type: Some("text/plain".to_string()),
    };
    let json_str = serde_json::to_string(&r).expect("ser");
    // PINS WIRE: mime_type ↔ mimeType rename.
    assert!(
        json_str.contains("\"mimeType\""),
        "MUST use camelCase mimeType on wire; got {json_str:?}"
    );
}

#[test]
fn mcp_resource_deserializes_from_camel_case_mime_type() {
    let json = r#"{
        "uri": "file:///x",
        "name": "doc",
        "mimeType": "application/json"
    }"#;
    let r: McpResource = serde_json::from_str(json).expect("de");
    assert_eq!(r.mime_type.as_deref(), Some("application/json"));
}

// ───────────────────────────────────────────────────────────────────────────
// Section C — McpCapabilities + ToolsCapability
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn mcp_capabilities_default_has_all_none_fields() {
    let caps = McpCapabilities::default();
    assert!(caps.tools.is_none());
    assert!(caps.resources.is_none());
    assert!(caps.prompts.is_none());
}

#[test]
fn mcp_capabilities_deserializes_empty_object_to_default() {
    let caps: McpCapabilities = serde_json::from_str("{}").expect("de");
    assert!(caps.tools.is_none());
    assert!(caps.resources.is_none());
    assert!(caps.prompts.is_none());
}

#[test]
fn mcp_capabilities_with_tools_subobject_deserializes() {
    let json = r#"{"tools": {"listChanged": true}}"#;
    let caps: McpCapabilities = serde_json::from_str(json).expect("de");
    let tools = caps.tools.expect("tools Some");
    assert!(tools.list_changed);
}

#[test]
fn mcp_capabilities_with_resources_and_prompts_preserves_arbitrary_value() {
    let json = r#"{
        "resources": {"subscribe": true},
        "prompts": {"listChanged": false}
    }"#;
    let caps: McpCapabilities = serde_json::from_str(json).expect("de");
    assert!(caps.resources.is_some());
    assert!(caps.prompts.is_some());
}

#[test]
fn tools_capability_default_has_list_changed_false() {
    let cap = ToolsCapability::default();
    assert!(!cap.list_changed);
}

#[test]
fn tools_capability_deserializes_with_list_changed_field_camel_case() {
    let json = r#"{"listChanged": true}"#;
    let cap: ToolsCapability = serde_json::from_str(json).expect("de");
    assert!(cap.list_changed);
}

#[test]
fn tools_capability_rejects_snake_case_list_changed() {
    // PINS CAMEL CASE: rename_all = "camelCase" — snake_case
    // input MAY be silently ignored (defaults to false).
    let json = r#"{"list_changed": true}"#;
    let cap: ToolsCapability = serde_json::from_str(json).expect("de");
    // snake_case field name not recognized → default false.
    assert!(
        !cap.list_changed,
        "snake_case input MUST NOT set list_changed"
    );
}

#[test]
fn tools_capability_empty_object_defaults_to_false() {
    let cap: ToolsCapability = serde_json::from_str("{}").expect("de");
    assert!(!cap.list_changed);
}

// ───────────────────────────────────────────────────────────────────────────
// Section D — McpServerInfo
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn mcp_server_info_required_name_field_deserializes() {
    let json = r#"{"name": "test-server"}"#;
    let info: McpServerInfo = serde_json::from_str(json).expect("de");
    assert_eq!(info.name, "test-server");
    assert!(info.version.is_none());
}

#[test]
fn mcp_server_info_with_version_round_trips() {
    let json = r#"{"name": "srv", "version": "1.2.3"}"#;
    let info: McpServerInfo = serde_json::from_str(json).expect("de");
    assert_eq!(info.name, "srv");
    assert_eq!(info.version.as_deref(), Some("1.2.3"));
}

#[test]
fn mcp_server_info_missing_name_field_errors() {
    let outcome: Result<McpServerInfo, _> = serde_json::from_str("{}");
    assert!(outcome.is_err(), "name is required");
}

#[test]
fn mcp_server_info_clone_preserves_fields() {
    let original = McpServerInfo {
        name: "srv".to_string(),
        version: Some("1.0".to_string()),
    };
    let cloned = original.clone();
    assert_eq!(cloned.name, original.name);
    assert_eq!(cloned.version, original.version);
}

// ───────────────────────────────────────────────────────────────────────────
// Section E — McpTransportKind enum + label
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn mcp_transport_kind_label_for_sse() {
    assert_eq!(McpTransportKind::Sse.label(), "Server-Sent Events");
}

#[test]
fn mcp_transport_kind_label_for_web_socket() {
    assert_eq!(McpTransportKind::WebSocket.label(), "WebSocket");
}

#[test]
fn mcp_transport_kind_label_for_streamable_http() {
    assert_eq!(McpTransportKind::StreamableHttp.label(), "Streamable HTTP");
}

#[test]
fn mcp_transport_kind_labels_are_pairwise_distinct() {
    let labels = [
        McpTransportKind::Sse.label(),
        McpTransportKind::WebSocket.label(),
        McpTransportKind::StreamableHttp.label(),
    ];
    let mut sorted: Vec<&str> = labels.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), 3);
}

#[test]
fn mcp_transport_kind_is_eq_and_clone() {
    let a = McpTransportKind::Sse;
    let cloned = a.clone();
    assert_eq!(a, cloned);
    assert_ne!(McpTransportKind::Sse, McpTransportKind::WebSocket);
}

#[test]
fn mcp_transport_kind_variants_compare_distinctly() {
    assert_ne!(McpTransportKind::Sse, McpTransportKind::StreamableHttp);
    assert_ne!(
        McpTransportKind::WebSocket,
        McpTransportKind::StreamableHttp
    );
}
