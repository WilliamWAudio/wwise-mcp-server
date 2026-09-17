// MCP 协议契约集成测试 — © 2025-2026 william.wang
//
// 直接启动编译产物 gwwise-mcp-server，用 JSON-RPC over stdio 验证：
// initialize / tools/list / resources / prompts 的返回是否符合 MCP 规范，
// 以及知识库是否真的随二进制分发。
// 这些用例不需要 Wwise 在运行。

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

/// 把一批 JSON-RPC 请求写进 server 的 stdin，收集所有响应。
fn rpc(requests: &[Value]) -> Vec<Value> {
    rpc_with_host(requests, "127.0.0.1")
}

/// 注意：WaapiClient::connect 在给定端口失败后会回退到 8080 等标准端口，
/// 所以要真正隔离"未连接 Wwise"的场景，只能换一个不可路由的 host。
fn rpc_with_host(requests: &[Value], host: &str) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_gwwise-mcp-server"))
        .args(["--wwise-host", host])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn gwwise-mcp-server");

    {
        let stdin = child.stdin.as_mut().expect("no stdin");
        for req in requests {
            writeln!(stdin, "{}", req).expect("failed to write request");
        }
        stdin.flush().ok();
    }
    // 关闭 stdin 让 server 读到 EOF 后退出
    drop(child.stdin.take());

    let stdout = child.stdout.take().expect("no stdout");
    let responses: Vec<Value> = BufReader::new(stdout)
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str::<Value>(&line).ok())
        .collect();

    let _ = child.wait();
    responses
}

/// 公开源码构建不含完整知识库（base.txt 随官方 Release 二进制分发）。
/// 依赖知识正文的断言在这种构建下跳过，保证贡献者构建全绿。
fn has_full_knowledge() -> bool {
    let base_txt = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("knowledge/base.txt");
    std::fs::metadata(&base_txt).map(|m| m.len() > 5_000).unwrap_or(false)
}

fn init_request(id: u64) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "integration-test", "version": "1" }
        }
    })
}

fn by_id(responses: &[Value], id: u64) -> &Value {
    responses
        .iter()
        .find(|r| r.get("id").and_then(Value::as_u64) == Some(id))
        .unwrap_or_else(|| panic!("no response with id {} in {:?}", id, responses))
}

#[test]
fn initialize_reports_protocol_and_capabilities() {
    let res = rpc(&[init_request(1)]);
    let result = &by_id(&res, 1)["result"];

    assert_eq!(result["protocolVersion"], "2024-11-05");
    assert!(result["capabilities"]["tools"].is_object());
    assert!(result["capabilities"]["resources"].is_object());
    assert!(result["capabilities"]["prompts"].is_object());
    assert_eq!(result["serverInfo"]["name"], "gwwise-mcp-server");
    assert!(
        !result["serverInfo"]["version"]
            .as_str()
            .unwrap_or("")
            .is_empty(),
        "serverInfo.version must not be empty"
    );
    // 数字水印：作者署名必须随 MCP 握手对所有客户端可见
    assert_eq!(
        result["serverInfo"]["author"], "william.wang",
        "serverInfo.author watermark must be present"
    );
}

/// 知识库必须随二进制分发：这是本 MCP 相对同类项目的核心差异，
/// 一旦 build.rs 的嵌入链路断了，这个用例会立刻失败。
#[test]
fn initialize_ships_embedded_knowledge_base() {
    if !has_full_knowledge() {
        eprintln!("skipping knowledge-distribution check: built without the full knowledge base");
        return;
    }

    let res = rpc(&[init_request(1)]);
    let instructions = by_id(&res, 1)["result"]["instructions"]
        .as_str()
        .expect("initialize must return instructions")
        .to_string();

    assert!(
        instructions.len() > 10_000,
        "embedded knowledge base looks truncated: {} chars",
        instructions.len()
    );
    for marker in [
        "CRITICAL Rules",
        "WAQL Reference",
        "waapi_list_functions",
        "waapi_get_schema",
    ] {
        assert!(
            instructions.contains(marker),
            "instructions missing expected section: {}",
            marker
        );
    }
}

#[test]
fn tools_list_exposes_every_definition_plus_connect() {
    let res = rpc(&[
        init_request(1),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} }),
    ]);
    let tools = by_id(&res, 2)["result"]["tools"]
        .as_array()
        .expect("tools/list must return an array")
        .clone();

    let defs = gwwise_agent_lib::knowledge::tools_definition();
    let expected = defs.as_array().expect("defs must be an array").len();

    // defs.json 里的全部工具 + 运行时注册的 wwise_connect
    assert_eq!(
        tools.len(),
        expected + 1,
        "tools/list should expose every definition plus wwise_connect"
    );

    let names: Vec<&str> = tools
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(names.contains(&"wwise_connect"));
}

/// 每个工具都必须有非空描述和合法的 object 类型 inputSchema，
/// 否则 LLM 只能靠猜——这正是同类项目报错率高的根因。
#[test]
fn every_tool_has_description_and_object_schema() {
    let res = rpc(&[
        init_request(1),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} }),
    ]);
    let tools = by_id(&res, 2)["result"]["tools"].as_array().unwrap().clone();

    for tool in &tools {
        let name = tool["name"].as_str().unwrap_or("<unnamed>");
        assert!(!name.is_empty(), "tool with empty name");

        let desc = tool["description"].as_str().unwrap_or("");
        assert!(
            desc.len() >= 20,
            "tool '{}' has a too-short description ({} chars)",
            name,
            desc.len()
        );

        assert_eq!(
            tool["inputSchema"]["type"], "object",
            "tool '{}' inputSchema.type must be \"object\"",
            name
        );
        assert!(
            tool["inputSchema"]["properties"].is_object(),
            "tool '{}' inputSchema.properties must be an object",
            name
        );
    }
}

/// 新补的五个域必须真的暴露出来。
#[test]
fn tools_list_covers_all_advanced_domains() {
    let res = rpc(&[
        init_request(1),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} }),
    ]);
    let names: Vec<String> = by_id(&res, 2)["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str().map(String::from))
        .collect();

    for required in [
        // 动态自省
        "waapi_list_functions",
        "waapi_list_topics",
        "waapi_get_schema",
        // SoundBank
        "generate_soundbanks",
        "get_soundbank_inclusions",
        // Profiler
        "profiler_capture",
        // 远程连接
        "remote_list_consoles",
        "remote_connect",
        "remote_disconnect",
        // UI 自动化
        "ui_get_selection",
        "ui_list_commands",
        "ui_execute_command",
        // 订阅
        "waapi_subscribe",
        "waapi_poll_events",
        "waapi_wait_for_event",
        "waapi_list_subscriptions",
        "waapi_unsubscribe",
        // 健康检查 / 试听
        "wwise_get_info",
        "transport_play",
        "transport_control",
        "transport_list",
    ] {
        assert!(
            names.iter().any(|n| n == required),
            "missing advanced tool: {}",
            required
        );
    }
}

#[test]
fn tool_names_are_unique() {
    let res = rpc(&[
        init_request(1),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} }),
    ]);
    let mut names: Vec<String> = by_id(&res, 2)["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str().map(String::from))
        .collect();

    let before = names.len();
    names.sort();
    names.dedup();
    assert_eq!(before, names.len(), "duplicate tool names in tools/list");
}

#[test]
fn resources_and_prompts_expose_knowledge() {
    let res = rpc(&[
        init_request(1),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "resources/list", "params": {} }),
        json!({ "jsonrpc": "2.0", "id": 3, "method": "prompts/list", "params": {} }),
        json!({ "jsonrpc": "2.0", "id": 4, "method": "prompts/get", "params": { "name": "gwwise_agent" } }),
    ]);

    let resources = by_id(&res, 2)["result"]["resources"]
        .as_array()
        .expect("resources/list must return an array")
        .clone();
    assert!(
        resources
            .iter()
            .any(|r| r["uri"].as_str().unwrap_or("").starts_with("gwwise://")),
        "expected at least one gwwise:// knowledge resource"
    );

    let prompts = by_id(&res, 3)["result"]["prompts"].as_array().unwrap().clone();
    assert!(
        prompts.iter().any(|p| p["name"] == "gwwise_agent"),
        "expected the gwwise_agent prompt to be listed"
    );

    // prompts/get 必须真的带回知识正文，而不是空壳
    let messages = by_id(&res, 4)["result"]["messages"].as_array().unwrap().clone();
    let text = messages
        .iter()
        .filter_map(|m| m["content"]["text"].as_str())
        .collect::<String>();
    if has_full_knowledge() {
        assert!(
            text.len() > 10_000,
            "prompts/get returned only {} chars of knowledge",
            text.len()
        );
    } else {
        eprintln!("skipping knowledge-size check: built without the full knowledge base");
        assert!(!text.is_empty(), "prompts/get must still return the fallback prompt");
    }
}

#[test]
fn unknown_prompt_returns_jsonrpc_error() {
    let res = rpc(&[
        init_request(1),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "prompts/get", "params": { "name": "no_such_prompt" } }),
    ]);
    let resp = by_id(&res, 2);
    assert!(
        resp.get("error").is_some(),
        "unknown prompt must produce a JSON-RPC error, got: {}",
        resp
    );
}

#[test]
fn unknown_resource_returns_jsonrpc_error() {
    let res = rpc(&[
        init_request(1),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "resources/read", "params": { "uri": "gwwise://nope" } }),
    ]);
    assert!(
        by_id(&res, 2).get("error").is_some(),
        "unknown resource must produce a JSON-RPC error"
    );
}

/// Wwise 不可用时，工具调用必须返回结构化的 isError 结果，而不是崩掉或挂死。
/// 用 TEST-NET-1 (RFC 5737) 保证连不上，与本机是否开着 Wwise 无关。
#[test]
fn tool_call_without_wwise_degrades_gracefully() {
    let res = rpc_with_host(
        &[
            init_request(1),
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": { "name": "ui_get_selection", "arguments": {} }
            }),
        ],
        "192.0.2.1",
    );

    let result = &by_id(&res, 2)["result"];
    assert_eq!(
        result["isError"], true,
        "expected isError=true, got: {}",
        result
    );

    let text = result["content"][0]["text"].as_str().unwrap_or("");
    assert!(
        text.to_lowercase().contains("connect"),
        "error text should mention connecting to Wwise, got: {}",
        text
    );
}

/// 无论本机是否开着 Wwise，tools/call 都必须返回结构良好的 content 数组，
/// 既不能崩，也不能挂住不回。
#[test]
fn tool_call_always_returns_well_formed_content() {
    let res = rpc(&[
        init_request(1),
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": { "name": "ui_get_selection", "arguments": {} }
        }),
    ]);

    let result = &by_id(&res, 2)["result"];
    let content = result["content"]
        .as_array()
        .expect("tools/call result must contain a content array");
    assert_eq!(content.len(), 1, "expected exactly one content block");
    assert_eq!(content[0]["type"], "text");

    let text = content[0]["text"].as_str().unwrap_or("");
    assert!(!text.is_empty(), "content text must not be empty");

    // Wwise 在跑 → TOOL_SUCCESS；没在跑 → isError 且提示去连接。
    if result["isError"] == true {
        assert!(text.to_lowercase().contains("connect"));
    } else {
        assert!(
            text.starts_with("TOOL_SUCCESS"),
            "successful tool result should be tagged TOOL_SUCCESS, got: {}",
            text
        );
    }
}

/// 没有订阅时，查询订阅列表必须是干净的空结果而不是报错。
#[test]
fn listing_subscriptions_with_none_active_is_not_an_error() {
    let res = rpc(&[
        init_request(1),
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": { "name": "waapi_list_subscriptions", "arguments": {} }
        }),
    ]);

    let result = &by_id(&res, 2)["result"];
    if result["isError"] == true {
        // 本机没开 Wwise 时允许连接错误，但不能是崩溃
        let text = result["content"][0]["text"].as_str().unwrap_or("");
        assert!(text.to_lowercase().contains("connect"));
        return;
    }

    let text = result["content"][0]["text"].as_str().unwrap_or("");
    assert!(text.starts_with("TOOL_SUCCESS"));
    assert!(
        text.contains("\"count\": 0"),
        "a fresh session should report zero subscriptions, got: {}",
        text
    );
}

/// 等待事件时必须给出 topic 或 subscription_id，两个都不给要明确报错。
#[test]
fn waiting_without_topic_or_subscription_is_rejected() {
    let res = rpc(&[
        init_request(1),
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": { "name": "waapi_wait_for_event", "arguments": { "timeout_ms": 200 } }
        }),
    ]);

    let result = &by_id(&res, 2)["result"];
    assert_eq!(result["isError"], true);
    let text = result["content"][0]["text"].as_str().unwrap_or("");
    assert!(
        text.contains("topic") || text.to_lowercase().contains("connect"),
        "expected a message about the missing topic, got: {}",
        text
    );
}

/// 退订时既没给 id 也没要求全退，应报错而不是静默成功。
#[test]
fn unsubscribe_without_target_is_rejected() {
    let res = rpc(&[
        init_request(1),
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": { "name": "waapi_unsubscribe", "arguments": {} }
        }),
    ]);

    let result = &by_id(&res, 2)["result"];
    assert_eq!(result["isError"], true);
    let text = result["content"][0]["text"].as_str().unwrap_or("");
    assert!(
        text.contains("subscription_id") || text.to_lowercase().contains("connect"),
        "expected a message about the missing target, got: {}",
        text
    );
}

/// 未连接时也不能在参数缺失上崩溃：缺 required 字段应得到明确报错。
#[test]
fn tool_call_with_missing_required_arg_is_reported() {
    let res = rpc(&[
        init_request(1),
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": { "name": "waapi_get_schema", "arguments": {} }
        }),
    ]);
    let resp = by_id(&res, 2);
    let serialized = resp.to_string();
    assert!(
        serialized.contains("isError") || resp.get("error").is_some(),
        "missing required arg must surface as an error, got: {}",
        serialized
    );
}
