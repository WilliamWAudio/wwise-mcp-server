// 对着正在运行的 Wwise 做端到端探测。Wwise 没开时跳过，不让 CI 红。
// 这些用例锁的是真实 URI / 返回形状，正是开源项目按文档硬编码会踩的坑。

use gwwise_agent_lib::advanced_ops;
use gwwise_agent_lib::tools::execute_tool;
use gwwise_agent_lib::waapi::WaapiClient;
use serde_json::Value;

async fn connected_client() -> Option<WaapiClient> {
    let client = WaapiClient::new();
    match tokio::time::timeout(
        std::time::Duration::from_secs(8),
        client.connect("127.0.0.1", 8080),
    )
    .await
    {
        Ok(Ok(())) => Some(client),
        _ => {
            eprintln!("skipping live Wwise test: WAAPI not reachable on 127.0.0.1:8080");
            None
        }
    }
}

#[tokio::test]
async fn live_get_info_reports_a_version() {
    let Some(waapi) = connected_client().await else {
        return;
    };
    let info = advanced_ops::wwise_get_info(&waapi)
        .await
        .expect("getInfo should succeed while Wwise is running");
    assert_eq!(info["connected"], true);
    let serialized = info.to_string();
    assert!(
        serialized.contains("version") || serialized.contains("Version") || serialized.contains("displayName"),
        "expected version fields in {}",
        serialized
    );
}

#[tokio::test]
async fn live_list_functions_exposes_transport_and_soundbank() {
    let Some(waapi) = connected_client().await else {
        return;
    };
    let listed = advanced_ops::waapi_list_functions(&waapi, Some("transport"))
        .await
        .expect("list functions");
    let functions = listed["functions"].as_array().cloned().unwrap_or_default();
    let names: Vec<String> = functions
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    assert!(
        names.iter().any(|n| n.contains("transport")),
        "expected transport URIs, got {:?}",
        names
    );
}

#[tokio::test]
async fn live_get_schema_for_enable_profiler_uses_enable_not_enabled() {
    let Some(waapi) = connected_client().await else {
        return;
    };
    let schema = advanced_ops::waapi_get_schema(
        &waapi,
        "ak.wwise.core.profiler.enableProfilerData",
    )
    .await
    .expect("schema");
    let text = schema.to_string();
    assert!(
        text.contains("\"enable\""),
        "schema must document the 'enable' key: {}",
        text
    );
    assert!(
        !text.contains("\"enabled\""),
        "schema should not use 'enabled': {}",
        text
    );
}

#[tokio::test]
async fn live_ui_get_selection_returns_count() {
    let Some(waapi) = connected_client().await else {
        return;
    };
    let sel = advanced_ops::ui_get_selection(&waapi, &[])
        .await
        .expect("selection");
    assert!(sel.get("count").and_then(Value::as_u64).is_some());
    assert!(sel.get("objects").is_some());
}

#[tokio::test]
async fn live_query_sounds_is_truncated_or_complete_but_valid_json() {
    let Some(waapi) = connected_client().await else {
        return;
    };
    let raw = execute_tool(
        &waapi,
        "waapi_query",
        r#"{"waql":"$ from type Sound","return_fields":["id","name","path","type"]}"#,
    )
    .await
    .expect("query");
    assert!(raw.contains("WAQL_SUCCESS") || raw.contains("_truncated") || raw.contains("return"));
}

#[tokio::test]
async fn live_transport_list_is_well_formed() {
    let Some(waapi) = connected_client().await else {
        return;
    };
    let listed = advanced_ops::transport_list(&waapi)
        .await
        .expect("transport list");
    assert!(listed.get("count").is_some());
    assert!(listed.get("transports").is_some());
}

/// 找一个真实 Sound 对象（id, name），没有就返回 None（空工程时跳过相关用例）。
async fn first_sound(waapi: &WaapiClient) -> Option<(String, String)> {
    let raw = execute_tool(
        waapi,
        "waapi_query",
        r#"{"waql":"$ from type Sound take 1","return_fields":["id","name"]}"#,
    )
    .await
    .ok()?;
    let json_start = raw.find('{')?;
    let parsed: Value = serde_json::from_str(&raw[json_start..]).ok()?;
    let obj = parsed.get("return")?.as_array()?.first()?;
    Some((
        obj.get("id")?.as_str()?.to_string(),
        obj.get("name")?.as_str()?.to_string(),
    ))
}

/// 端到端验证订阅事件真实送达：订阅 nameChanged → 改名 → 等事件 → 改回 → 退订。
/// 全程零净改动（改名立即还原，不保存工程）。
#[tokio::test]
async fn live_subscription_delivers_name_changed_event() {
    let Some(waapi) = connected_client().await else {
        return;
    };
    let Some((id, original_name)) = first_sound(&waapi).await else {
        eprintln!("skipping: project has no Sound objects");
        return;
    };

    advanced_ops::waapi_subscribe(&waapi, "ak.wwise.core.object.nameChanged", &[])
        .await
        .expect("subscribe nameChanged");

    let temp_name = format!("{}_MCPLIVETEST", original_name);
    let rename = waapi
        .call(
            "ak.wwise.core.object.setName",
            serde_json::json!({ "object": id, "value": temp_name }),
            None,
        )
        .await;

    // 无论断言结果如何都先等事件、再还原名字，保证工程零净改动
    let event = advanced_ops::waapi_wait_for_event(
        &waapi,
        Some("ak.wwise.core.object.nameChanged"),
        None,
        5_000,
    )
    .await;

    let restore = waapi
        .call(
            "ak.wwise.core.object.setName",
            serde_json::json!({ "object": id, "value": original_name }),
            None,
        )
        .await;

    let _ = advanced_ops::waapi_unsubscribe(&waapi, None, true).await;

    rename.expect("setName to temp name should succeed");
    restore.expect("setName back to original must succeed");
    let event = event.expect("wait_for_event should not error");
    assert_eq!(
        event["timed_out"], false,
        "nameChanged event must arrive within 5s: {}",
        event
    );
    assert!(
        event["event"].to_string().contains("_MCPLIVETEST"),
        "event payload should carry the new name: {}",
        event
    );
}

/// 端到端验证试听链路：播放 → 列表可见 → 停止并销毁 → 无残留。
/// 会短暂出声（本来就是试听功能）。
#[tokio::test]
async fn live_transport_play_stop_destroy_roundtrip() {
    let Some(waapi) = connected_client().await else {
        return;
    };
    let Some((id, _)) = first_sound(&waapi).await else {
        eprintln!("skipping: project has no Sound objects");
        return;
    };

    let played = advanced_ops::transport_play(&waapi, &id, None)
        .await
        .expect("transport_play must succeed on a live Wwise");
    assert!(played.get("transport").and_then(Value::as_u64).is_some());

    let listed = advanced_ops::transport_list(&waapi)
        .await
        .expect("transport list");
    assert!(
        listed["count"].as_u64().unwrap_or(0) >= 1,
        "the new transport must appear in the list: {}",
        listed
    );

    let stopped = advanced_ops::transport_control(&waapi, "stop", None, true)
        .await
        .expect("stop + destroy all");
    assert_eq!(stopped["destroyed"], true, "must destroy transports: {}", stopped);

    let after = advanced_ops::transport_list(&waapi)
        .await
        .expect("transport list after destroy");
    assert_eq!(
        after["count"].as_u64().unwrap_or(99),
        0,
        "no orphan transports may remain: {}",
        after
    );
}

#[tokio::test]
async fn live_list_topics_includes_selection_changed() {
    let Some(waapi) = connected_client().await else {
        return;
    };
    let topics = advanced_ops::waapi_list_topics(&waapi, Some("selection"))
        .await
        .expect("topics");
    let text = topics.to_string();
    assert!(
        text.contains("selectionChanged"),
        "expected selectionChanged topic, got {}",
        text
    );
}
