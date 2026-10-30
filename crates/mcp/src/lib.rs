//! The MCP server (L5): JSON-RPC 2.0 messages in, responses out. Every registry command is a
//! tool, so agents build models with the same commands as the UI.
//!
//! The transport is the caller's: `structurecraft-cli mcp` reads newline-delimited messages on
//! stdin; the desktop app will bridge the same handler to its open session.

use serde_json::{Value, json};
use structurecraft_engine::{Session, registry};

pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// MCP tool names use `_` where command ids use `.` (`model.new` → `model_new`).
pub fn tool_name(command_id: &str) -> String {
    command_id.replace('.', "_")
}

fn command_id(tool: &str) -> Option<&'static str> {
    registry().iter().map(|c| c.id).find(|id| tool_name(id) == tool)
}

/// Handle one JSON-RPC message (a line of text). Returns the response to send, or `None` for a
/// notification.
pub fn handle_line(session: &mut Session, line: &str) -> Option<String> {
    let response = match serde_json::from_str::<Value>(line) {
        Ok(msg) => handle(session, &msg)?,
        Err(e) => error(Value::Null, -32700, &format!("parse error: {e}")),
    };
    Some(response.to_string())
}

pub fn handle(session: &mut Session, msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or_default();
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    // Notifications (no id) get no response.
    let id = id?;
    Some(match method {
        "initialize" => result(
            id,
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "structurecraft", "version": env!("CARGO_PKG_VERSION")},
            }),
        ),
        "ping" => result(id, json!({})),
        "tools/list" => {
            let tools: Vec<Value> =
                registry().iter().map(|c| json!({"name": tool_name(c.id), "description": c.summary, "inputSchema": (c.input_schema)()})).collect();
            result(id, json!({"tools": tools}))
        }
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
            let Some(command) = command_id(name) else {
                return Some(error(id, -32602, &format!("unknown tool `{name}`")));
            };
            let args = params.get("arguments").cloned().unwrap_or(Value::Null);
            let (text, is_error) = match session.execute(command, args) {
                Ok(out) => (out.to_string(), false),
                Err(e) => (e.to_string(), true),
            };
            result(id, json!({"content": [{"type": "text", "text": text}], "isError": is_error}))
        }
        other => error(id, -32601, &format!("method not found: {other}")),
    })
}

fn result(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_every_command_as_a_tool() {
        let mut s = Session::new();
        let out = handle(&mut s, &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})).unwrap();
        assert_eq!(out["result"]["tools"].as_array().unwrap().len(), registry().len());
    }

    #[test]
    fn calls_a_tool() {
        let mut s = Session::new();
        let call = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "model_new", "arguments": {"name": "Bridge"}}});
        let out = handle(&mut s, &call).unwrap();
        assert_eq!(out["result"]["isError"], false);
        assert_eq!(s.model.name(), "Bridge");
    }

    #[test]
    fn garbage_is_an_error_response() {
        let mut s = Session::new();
        let out: Value = serde_json::from_str(&handle_line(&mut s, "{not json").unwrap()).unwrap();
        assert_eq!(out["error"]["code"], -32700);
        assert!(handle_line(&mut s, r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
    }
}
