// GWwise MCP Server — Wwise WAAPI tools for Cursor / Claude / any MCP client
// © 2025-2026 william.wang. All rights reserved.
//
// Protocol: JSON-RPC 2.0 over stdio (newline-delimited)
// Launch:   gwwise-mcp-server [--wwise-host HOST] [--wwise-port PORT]

use gwwise_agent_lib::knowledge;
use gwwise_agent_lib::tools::execute_tool;
use gwwise_agent_lib::waapi::WaapiClient;

use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::sync::Arc;
use tokio::sync::Mutex;

const SERVER_NAME: &str = "gwwise-mcp-server";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
const PROTOCOL_VERSION: &str = "2024-11-05";
const KNOWLEDGE_BASE_URI: &str = "gwwise://knowledge/base";
const KNOWLEDGE_QUICKSTART_URI: &str = "gwwise://knowledge/quickstart";
const PROMPT_NAME: &str = "gwwise_agent";

// ───────────────────────── CLI arg parsing ─────────────────────────

struct Args {
    wwise_host: String,
    wwise_port: u16,
}

fn parse_args() -> Args {
    let args: Vec<String> = std::env::args().collect();
    let mut host = "127.0.0.1".to_string();
    let mut port: u16 = 8080;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--wwise-host" => {
                if i + 1 < args.len() {
                    host = args[i + 1].clone();
                    i += 1;
                }
            }
            "--wwise-port" => {
                if i + 1 < args.len() {
                    port = args[i + 1].parse().unwrap_or(8080);
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    Args {
        wwise_host: host,
        wwise_port: port,
    }
}

// ───────────────────────── Logging (stderr only) ─────────────────────────

macro_rules! log {
    ($($arg:tt)*) => {
        eprintln!("[gwwise-mcp] {}", format!($($arg)*))
    };
}

// ───────────────────────── JSON-RPC helpers ─────────────────────────

fn jsonrpc_result(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn jsonrpc_error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn send(msg: &Value) {
    let s = serde_json::to_string(msg).unwrap_or_default();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let _ = out.write_all(s.as_bytes());
    let _ = out.write_all(b"\n");
    let _ = out.flush();
}

fn mcp_instructions() -> String {
    knowledge::system_prompt()
}

fn mcp_quickstart() -> String {
    [
        "GWwise MCP quickstart:",
        "- Respond in 简体中文. Keep Wwise object names, WAAPI URIs, and property names exact.",
        "- Never fabricate GUIDs. Query Wwise first and only use IDs returned by tools.",
        "- Prefer high-level batch_* tools over manual WAAPI loops for batch work.",
        "- For selected-subtree unused/redundant cleanup, use batch_delete_unused_descendants.",
        "- For Skill_Hit/Skill_Release GeneralSkills copy-missing tasks, use batch_copy_missing_descendants.",
        "- For move/迁移/搬过去 missing descendants, use batch_move_missing_descendants, not copy+delete.",
        "- Keep sync_output_bus=true unless the user explicitly wants target-side inherited routing.",
        "- For filesystem Originals cleanup, use cleanup_unused_originals_files.",
    ]
    .join("\n")
}

// ───────────────────────── Tool schemas (MCP format) ─────────────────────────

fn tool_schemas() -> Value {
    let defs = knowledge::tools_definition();
    let defs = defs.as_array().cloned().unwrap_or_default();

    let mut tools = Vec::new();

    for def in defs {
        if let Some(func) = def.get("function") {
            let name = func.get("name").cloned().unwrap_or_default();
            let description = func.get("description").cloned().unwrap_or_default();
            let input_schema = func.get("parameters").cloned().unwrap_or(json!({
                "type": "object",
                "properties": {}
            }));

            tools.push(json!({
                "name": name,
                "description": description,
                "inputSchema": input_schema
            }));
        }
    }

    // Add wwise_connect
    tools.push(json!({
        "name": "wwise_connect",
        "description": "Connect or reconnect to Wwise. Call this if tools return connection errors, or to connect to a different Wwise instance.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "host": {
                    "type": "string",
                    "description": "Wwise host. Default: 127.0.0.1"
                },
                "port": {
                    "type": "integer",
                    "description": "Wwise WAAPI port. Default: 8080. The server will also try nearby ports (8081, 8082, etc.)"
                }
            }
        }
    }));

    json!({
        "tools": tools
    })
}

fn resource_schemas() -> Value {
    json!({
        "resources": [
            {
                "uri": KNOWLEDGE_BASE_URI,
                "name": "GWwiseAgent full Wwise knowledge base",
                "description": "Full embedded GWwiseAgent Wwise operating instructions and best practices. Built into the MCP server executable.",
                "mimeType": "text/plain"
            },
            {
                "uri": KNOWLEDGE_QUICKSTART_URI,
                "name": "GWwiseAgent MCP quickstart",
                "description": "Short high-priority rules for using GWwise MCP tools safely.",
                "mimeType": "text/plain"
            }
        ]
    })
}

fn read_resource(uri: &str) -> Option<Value> {
    let text = match uri {
        KNOWLEDGE_BASE_URI => mcp_instructions(),
        KNOWLEDGE_QUICKSTART_URI => mcp_quickstart(),
        _ => return None,
    };

    Some(json!({
        "contents": [
            {
                "uri": uri,
                "mimeType": "text/plain",
                "text": text
            }
        ]
    }))
}

fn prompt_schemas() -> Value {
    json!({
        "prompts": [
            {
                "name": PROMPT_NAME,
                "description": "Load GWwiseAgent Wwise operation rules and MCP tool best practices into the current conversation.",
                "arguments": []
            }
        ]
    })
}

fn get_prompt(name: &str) -> Option<Value> {
    if name != PROMPT_NAME {
        return None;
    }

    Some(json!({
        "description": "GWwiseAgent Wwise operation rules",
        "messages": [
            {
                "role": "user",
                "content": {
                    "type": "text",
                    "text": mcp_instructions()
                }
            }
        ]
    }))
}

// ───────────────────────── Request handling ─────────────────────────

async fn handle_request(
    method: &str,
    id: &Value,
    params: &Value,
    waapi: &Arc<Mutex<WaapiClient>>,
    default_host: &str,
    default_port: u16,
) {
    match method {
        "initialize" => {
            send(&jsonrpc_result(
                id,
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": {
                        "tools": {},
                        "resources": {},
                        "prompts": {}
                    },
                    "serverInfo": {
                        "name": SERVER_NAME,
                        "version": SERVER_VERSION,
                        "author": gwwise_agent_lib::AUTHOR
                    },
                    "instructions": mcp_instructions()
                }),
            ));
        }

        "tools/list" => {
            send(&jsonrpc_result(id, tool_schemas()));
        }

        "resources/list" => {
            send(&jsonrpc_result(id, resource_schemas()));
        }

        "resources/read" => {
            let uri = params.get("uri").and_then(|v| v.as_str()).unwrap_or("");
            if let Some(result) = read_resource(uri) {
                send(&jsonrpc_result(id, result));
            } else {
                send(&jsonrpc_error(
                    id,
                    -32002,
                    &format!("Resource not found: {}", uri),
                ));
            }
        }

        "prompts/list" => {
            send(&jsonrpc_result(id, prompt_schemas()));
        }

        "prompts/get" => {
            let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if let Some(result) = get_prompt(name) {
                send(&jsonrpc_result(id, result));
            } else {
                send(&jsonrpc_error(
                    id,
                    -32602,
                    &format!("Unknown prompt: {}", name),
                ));
            }
        }

        "tools/call" => {
            let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

            match tool_name {
                "wwise_connect" => {
                    let host = arguments
                        .get("host")
                        .and_then(|v| v.as_str())
                        .unwrap_or(default_host);
                    let port = arguments
                        .get("port")
                        .and_then(|v| v.as_u64())
                        .map(|p| p as u16)
                        .unwrap_or(default_port);

                    let client = waapi.lock().await;
                    match client.connect(host, port).await {
                        Ok(()) => {
                            log!("Connected to Wwise at {}:{}", host, port);
                            send(&jsonrpc_result(
                                id,
                                json!({
                                    "content": [{ "type": "text", "text": format!("Connected to Wwise at {}:{}", host, port) }]
                                }),
                            ));
                        }
                        Err(e) => {
                            log!("Failed to connect: {}", e);
                            send(&jsonrpc_result(
                                id,
                                json!({
                                    "content": [{ "type": "text", "text": format!("Connection failed: {}", e) }],
                                    "isError": true
                                }),
                            ));
                        }
                    }
                }

                other_tool => {
                    let client = waapi.lock().await;

                    // Auto-connect if not connected
                    if !client.is_connected() {
                        log!("Not connected, attempting auto-connect...");
                        if let Err(e) = client.connect(default_host, default_port).await {
                            send(&jsonrpc_result(
                                id,
                                json!({
                                    "content": [{ "type": "text", "text": format!(
                                        "Not connected to Wwise and auto-connect failed: {}. Use wwise_connect tool to connect manually.", e
                                    )}],
                                    "isError": true
                                }),
                            ));
                            return;
                        }
                        log!("Auto-connected to Wwise");
                    }

                    let args_str = serde_json::to_string(&arguments).unwrap_or_default();
                    match execute_tool(&client, other_tool, &args_str).await {
                        Ok(result) => {
                            send(&jsonrpc_result(
                                id,
                                json!({
                                    "content": [{ "type": "text", "text": result }]
                                }),
                            ));
                        }
                        Err(e) => {
                            send(&jsonrpc_result(
                                id,
                                json!({
                                    "content": [{ "type": "text", "text": e }],
                                    "isError": true
                                }),
                            ));
                        }
                    }
                }
            }
        }

        // Notifications (no response needed)
        "notifications/initialized" | "notifications/cancelled" => {}

        _ => {
            if !id.is_null() {
                send(&jsonrpc_error(
                    id,
                    -32601,
                    &format!("Method not found: {}", method),
                ));
            }
        }
    }
}

// ───────────────────────── Main ─────────────────────────

#[tokio::main]
async fn main() {
    let args = parse_args();
    log!("Starting {} v{}", SERVER_NAME, SERVER_VERSION);
    log!("{}", gwwise_agent_lib::signature());
    // 引用隐藏水印，确保其字节序列编译进本二进制（版权保护）
    debug_assert!(gwwise_agent_lib::hidden_watermark().contains("william.wang"));
    let _ = std::hint::black_box(gwwise_agent_lib::hidden_watermark());
    log!(
        "Default Wwise target: {}:{}",
        args.wwise_host,
        args.wwise_port
    );

    let waapi = Arc::new(Mutex::new(WaapiClient::new()));

    // Try initial connection (non-blocking — server starts even if Wwise is down)
    {
        let client = waapi.lock().await;
        match client.connect(&args.wwise_host, args.wwise_port).await {
            Ok(()) => log!("Connected to Wwise"),
            Err(e) => log!(
                "Initial Wwise connection failed (will retry on first tool call): {}",
                e
            ),
        }
    }

    let stdin = io::stdin();
    let reader = stdin.lock();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let msg: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                log!(
                    "JSON parse error: {} | input: {}",
                    e,
                    &trimmed[..trimmed.len().min(200)]
                );
                continue;
            }
        };

        let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        let params = msg.get("params").cloned().unwrap_or(json!({}));

        handle_request(
            method,
            &id,
            &params,
            &waapi,
            &args.wwise_host,
            args.wwise_port,
        )
        .await;
    }

    log!("Stdin closed, shutting down");
}
