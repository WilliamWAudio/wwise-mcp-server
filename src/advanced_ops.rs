// GWwiseAgent Advanced Operations Module — © 2025-2026 william.wang
// 动态 WAAPI 自省 + SoundBank / Profiler / 远程连接 / UI 自动化域

use crate::waapi::{SubscriptionInfo, WaapiClient};
use serde_json::{json, Value};

/// WAAPI 调用失败时附加的排查提示。
/// 不同 Wwise 版本的接口集合不同，引导调用方先自省而不是反复猜参数。
fn uri_failure(uri: &str, err: &str) -> String {
    format!(
        "WAAPI_ERROR [{}]: {}\n\
         Hint: this URI may not exist in the connected Wwise version. \
         Call waapi_list_functions to list the URIs this Wwise actually exposes, \
         then waapi_get_schema to get the exact argument structure before retrying.",
        uri, err
    )
}

fn str_vec(args: &Value, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::trim))
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

// ───────────────────────── 动态 WAAPI 自省 ─────────────────────────

/// 列出当前连接的 Wwise 实例真实暴露的 WAAPI 函数 URI。
/// 可选 filter 做子串过滤（如 "profiler"、"soundbank"），避免一次返回上百条。
pub async fn waapi_list_functions(
    waapi: &WaapiClient,
    filter: Option<&str>,
) -> Result<Value, String> {
    let uri = "ak.wwise.waapi.getFunctions";
    let result = waapi
        .call(uri, json!({}), None)
        .await
        .map_err(|e| uri_failure(uri, &e))?;

    let all: Vec<String> = result
        .get("functions")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let needle = filter.map(str::to_lowercase).unwrap_or_default();
    let matched: Vec<String> = if needle.is_empty() {
        all.clone()
    } else {
        all.iter()
            .filter(|f| f.to_lowercase().contains(&needle))
            .cloned()
            .collect()
    };

    Ok(json!({
        "total_available": all.len(),
        "filter": filter.unwrap_or(""),
        "matched": matched.len(),
        "functions": matched,
    }))
}

/// 列出当前 Wwise 实例支持的订阅 topic。
pub async fn waapi_list_topics(
    waapi: &WaapiClient,
    filter: Option<&str>,
) -> Result<Value, String> {
    let uri = "ak.wwise.waapi.getTopics";
    let result = waapi
        .call(uri, json!({}), None)
        .await
        .map_err(|e| uri_failure(uri, &e))?;

    let all: Vec<String> = result
        .get("topics")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let needle = filter.map(str::to_lowercase).unwrap_or_default();
    let matched: Vec<String> = if needle.is_empty() {
        all.clone()
    } else {
        all.iter()
            .filter(|t| t.to_lowercase().contains(&needle))
            .cloned()
            .collect()
    };

    Ok(json!({
        "total_available": all.len(),
        "filter": filter.unwrap_or(""),
        "matched": matched.len(),
        "topics": matched,
    }))
}

/// 取某个 WAAPI URI 的参数 / 返回值 JSON Schema。
/// 这是替代"凭记忆猜参数"的正确做法：先取 schema，再按 schema 构造调用。
pub async fn waapi_get_schema(waapi: &WaapiClient, target_uri: &str) -> Result<Value, String> {
    let uri = "ak.wwise.waapi.getSchema";
    let schema = waapi
        .call(uri, json!({ "uri": target_uri }), None)
        .await
        .map_err(|e| uri_failure(uri, &e))?;

    Ok(json!({
        "uri": target_uri,
        "schema": schema,
    }))
}

// ───────────────────────── SoundBank 域 ─────────────────────────

/// 生成 SoundBank：可选先写 inclusions，再触发生成，最后汇总结果。
/// soundbanks 为空时生成全部 SoundBank。
#[allow(clippy::too_many_arguments)]
pub async fn generate_soundbanks(
    waapi: &WaapiClient,
    soundbanks: &[String],
    platforms: &[String],
    languages: &[String],
    inclusions_object_ids: &[String],
    inclusion_filter: &[String],
    rebuild: bool,
    write_to_disk: bool,
) -> Result<Value, String> {
    let mut details: Vec<String> = Vec::new();

    // 阶段一：按需写 inclusions。只有同时给了 SoundBank 和对象才执行。
    if !inclusions_object_ids.is_empty() {
        if soundbanks.is_empty() {
            return Err(
                "TOOL_ERROR: inclusions_object_ids requires at least one entry in 'soundbanks' \
                 so the tool knows which SoundBank to add them to."
                    .to_string(),
            );
        }

        let filter: Vec<String> = if inclusion_filter.is_empty() {
            vec![
                "events".to_string(),
                "structures".to_string(),
                "media".to_string(),
            ]
        } else {
            inclusion_filter.to_vec()
        };

        let inclusions: Vec<Value> = inclusions_object_ids
            .iter()
            .map(|id| json!({ "object": id, "filter": filter }))
            .collect();

        for bank in soundbanks {
            let uri = "ak.wwise.core.soundbank.setInclusions";
            match waapi
                .call(
                    uri,
                    json!({
                        "soundbank": bank,
                        "operation": "add",
                        "inclusions": inclusions,
                    }),
                    None,
                )
                .await
            {
                Ok(_) => details.push(format!(
                    "inclusions: added {} object(s) to '{}'",
                    inclusions_object_ids.len(),
                    bank
                )),
                Err(e) => return Err(uri_failure(uri, &e)),
            }
        }
    }

    // 阶段二：触发生成。
    let mut gen_args = json!({
        "rebuildSoundBanks": rebuild,
        "writeToDisk": write_to_disk,
    });

    if !soundbanks.is_empty() {
        let banks: Vec<Value> = soundbanks
            .iter()
            .map(|b| {
                // GUID 用 id，其余当作名字
                if b.starts_with('{') && b.ends_with('}') {
                    json!({ "id": b })
                } else {
                    json!({ "name": b })
                }
            })
            .collect();
        gen_args["soundbanks"] = Value::Array(banks);
    }
    if !platforms.is_empty() {
        gen_args["platforms"] = json!(platforms);
    }
    if !languages.is_empty() {
        gen_args["languages"] = json!(languages);
    }

    let uri = "ak.wwise.core.soundbank.generate";
    let result = waapi
        .call(uri, gen_args.clone(), None)
        .await
        .map_err(|e| uri_failure(uri, &e))?;

    details.push(format!(
        "generate: {} bank(s) requested, rebuild={}, write_to_disk={}",
        if soundbanks.is_empty() {
            "all".to_string()
        } else {
            soundbanks.len().to_string()
        },
        rebuild,
        write_to_disk
    ));

    Ok(json!({
        "requested": gen_args,
        "details": details,
        "result": result,
    }))
}

/// 查询某个 SoundBank 当前的 inclusions。
pub async fn get_soundbank_inclusions(
    waapi: &WaapiClient,
    soundbank: &str,
) -> Result<Value, String> {
    let uri = "ak.wwise.core.soundbank.getInclusions";
    let result = waapi
        .call(uri, json!({ "soundbank": soundbank }), None)
        .await
        .map_err(|e| uri_failure(uri, &e))?;

    Ok(json!({
        "soundbank": soundbank,
        "inclusions": result.get("inclusions").cloned().unwrap_or(json!([])),
    }))
}

// ───────────────────────── Profiler 域 ─────────────────────────

/// Wwise 认可的 profiler 数据类型枚举（取自 profilerDataType schema）。
/// 注意这里没有 "busses"——总线计量数据属于 "meter"。
const PROFILER_DATA_TYPES: &[&str] = &[
    "cpu",
    "memory",
    "stream",
    "voices",
    "listener",
    "obstructionOcclusion",
    "markersNotification",
    "soundbanks",
    "loadedMedia",
    "preparedObjects",
    "preparedGameSyncs",
    "interactiveMusic",
    "streamingDevice",
    "meter",
    "auxiliarySends",
    "apiCalls",
    "spatialAudio",
    "spatialAudioRaycasting",
    "voiceInspector",
    "audioObjects",
    "gameSyncs",
];

/// 把口语化的数据类型名归一到 Wwise 枚举值。
fn canonical_profiler_data_type(input: &str) -> Option<&'static str> {
    let key = input.trim().to_lowercase();
    let aliased = match key.as_str() {
        "bus" | "buses" | "busses" | "metering" | "meters" => "meter",
        "performance" | "perf" | "performancemonitor" => "cpu",
        "voice" => "voices",
        "streams" => "stream",
        "audioobject" | "audio_objects" => "audioobjects",
        "gamesync" | "game_syncs" => "gamesyncs",
        other => other,
    };
    PROFILER_DATA_TYPES
        .iter()
        .find(|canon| canon.to_lowercase() == aliased)
        .copied()
}

/// 决定这次采集要开启哪些数据类型。
/// 关键点：根据 include_* 开关自动补齐必需的类型，
/// 否则用户会拿到一份全空的采集结果而不知道为什么。
fn resolve_profiler_data_types(
    requested: &[String],
    include_voices: bool,
    include_busses: bool,
    include_performance: bool,
) -> Result<Vec<String>, String> {
    fn push(resolved: &mut Vec<String>, value: &str) {
        if !resolved.iter().any(|existing| existing == value) {
            resolved.push(value.to_string());
        }
    }

    let mut resolved: Vec<String> = Vec::new();

    for raw in requested {
        match canonical_profiler_data_type(raw) {
            Some(canon) => push(&mut resolved, canon),
            None => {
                return Err(format!(
                    "TOOL_ERROR: '{}' is not a valid profiler data type. Valid values: {}. \
                     Note that bus metering data uses 'meter', not 'busses'.",
                    raw,
                    PROFILER_DATA_TYPES.join(", ")
                ))
            }
        }
    }

    if include_voices {
        push(&mut resolved, "voices");
    }
    if include_busses {
        push(&mut resolved, "meter");
    }
    if include_performance {
        push(&mut resolved, "cpu");
    }

    if resolved.is_empty() {
        push(&mut resolved, "voices");
        push(&mut resolved, "meter");
    }

    Ok(resolved)
}

/// 一次完整的 Profiler 采样：开数据类型 → 起采集 → 等待 → 读游标 →
/// 抓 voices / busses / 性能计数器 → 停采集 → 汇总。
pub async fn profiler_capture(
    waapi: &WaapiClient,
    duration_ms: u64,
    data_types: &[String],
    include_voices: bool,
    include_busses: bool,
    include_performance: bool,
) -> Result<Value, String> {
    let duration_ms = duration_ms.clamp(100, 60_000);
    let mut details: Vec<String> = Vec::new();

    let types = resolve_profiler_data_types(
        data_types,
        include_voices,
        include_busses,
        include_performance,
    )?;

    // 开启需要的数据类型，否则采集下来是空的。
    // 注意：schema 里的键是 "enable"，不是 "enabled"。
    let enable_uri = "ak.wwise.core.profiler.enableProfilerData";
    let payload: Vec<Value> = types
        .iter()
        .map(|t| json!({ "dataType": t, "enable": true }))
        .collect();
    waapi
        .call(enable_uri, json!({ "dataTypes": payload }), None)
        .await
        .map_err(|e| uri_failure(enable_uri, &e))?;
    details.push(format!("enabled data types: {}", types.join(", ")));

    let start_uri = "ak.wwise.core.profiler.startCapture";
    waapi
        .call(start_uri, json!({}), None)
        .await
        .map_err(|e| uri_failure(start_uri, &e))?;
    details.push("capture started".to_string());

    tokio::time::sleep(std::time::Duration::from_millis(duration_ms)).await;

    // 取采集游标时间，后续所有查询都以这个时间点为准。
    let cursor_uri = "ak.wwise.core.profiler.getCursorTime";
    let cursor = waapi
        .call(cursor_uri, json!({ "cursor": "capture" }), None)
        .await
        .ok()
        .and_then(|v| v.get("return").and_then(|t| t.as_i64()))
        .unwrap_or(0);
    details.push(format!("capture cursor time = {} ms", cursor));

    let mut voices = Value::Null;
    let mut busses = Value::Null;
    let mut performance = Value::Null;

    if include_voices {
        let uri = "ak.wwise.core.profiler.getVoices";
        match waapi.call(uri, json!({ "time": cursor }), None).await {
            Ok(v) => voices = v.get("return").cloned().unwrap_or(v),
            Err(e) => details.push(format!("getVoices failed: {}", e)),
        }
    }
    if include_busses {
        let uri = "ak.wwise.core.profiler.getBusses";
        match waapi.call(uri, json!({ "time": cursor }), None).await {
            Ok(v) => busses = v.get("return").cloned().unwrap_or(v),
            Err(e) => details.push(format!("getBusses failed: {}", e)),
        }
    }
    if include_performance {
        let uri = "ak.wwise.core.profiler.getPerformanceMonitor";
        match waapi.call(uri, json!({ "time": cursor }), None).await {
            Ok(v) => performance = v.get("return").cloned().unwrap_or(v),
            Err(e) => details.push(format!("getPerformanceMonitor failed: {}", e)),
        }
    }

    let stop_uri = "ak.wwise.core.profiler.stopCapture";
    match waapi.call(stop_uri, json!({}), None).await {
        Ok(_) => details.push("capture stopped".to_string()),
        Err(e) => details.push(format!("stopCapture failed: {}", e)),
    }

    let voice_count = voices.as_array().map(|a| a.len()).unwrap_or(0);
    let bus_count = busses.as_array().map(|a| a.len()).unwrap_or(0);

    let mut result = json!({
        "duration_ms": duration_ms,
        "enabled_data_types": types,
        "cursor_time_ms": cursor,
        "voice_count": voice_count,
        "bus_count": bus_count,
        "voices": voices,
        "busses": busses,
        "performance": performance,
        "details": details,
    });

    // 空结果几乎总是"没有音频在播"，而不是工具坏了。明确说出来，
    // 避免调用方反复重试同一次采集。
    if voice_count == 0 && bus_count == 0 {
        result["note"] = json!(
            "No voices or busses were active during the capture window. This is expected unless \
             audio is actually playing: either connect to a running game (remote_connect) or \
             audition something in Wwise, then capture again. Do not retry blindly."
        );
    }

    Ok(result)
}

// ───────────────────────── 健康检查 / 工程信息 ─────────────────────────

/// 取 Wwise 版本与当前工程信息。相当于别的 MCP 里的 ping + get_project_info。
pub async fn wwise_get_info(waapi: &WaapiClient) -> Result<Value, String> {
    let info_uri = "ak.wwise.core.getInfo";
    let info = waapi
        .call(info_uri, json!({}), None)
        .await
        .map_err(|e| uri_failure(info_uri, &e))?;

    let project = waapi
        .call(
            "ak.wwise.core.object.get",
            json!({ "from": { "ofType": ["Project"] } }),
            Some(json!({ "return": ["name", "filePath", "id"] })),
        )
        .await
        .ok()
        .and_then(|v| {
            v.get("return")
                .and_then(|r| r.as_array())
                .and_then(|arr| arr.first())
                .cloned()
        });

    Ok(json!({
        "wwise": info,
        "project": project,
        "connected": true,
    }))
}

// ───────────────────────── Transport / 试听 ─────────────────────────

const TRANSPORT_ACTIONS: &[&str] = &["play", "stop", "pause", "playStop", "playDirectly"];

fn canonical_transport_action(input: &str) -> Option<&'static str> {
    match input.trim().to_lowercase().as_str() {
        "play" | "start" | "preview" => Some("play"),
        "stop" | "halt" => Some("stop"),
        "pause" => Some("pause"),
        "playstop" | "play_stop" | "toggle" => Some("playStop"),
        "playdirectly" | "play_directly" | "direct" => Some("playDirectly"),
        _ => None,
    }
}

/// 为指定对象创建 transport、prepare，并立刻播放。
pub async fn transport_play(
    waapi: &WaapiClient,
    object: &str,
    game_object: Option<u64>,
) -> Result<Value, String> {
    if object.trim().is_empty() {
        return Err(
            "TOOL_ERROR: Missing 'object' — pass a GUID, path, or type:name of the Event/Sound to audition."
                .to_string(),
        );
    }

    let mut create_args = json!({ "object": object });
    if let Some(go) = game_object {
        create_args["gameObject"] = json!(go);
    }

    let create_uri = "ak.wwise.core.transport.create";
    let created = waapi
        .call(create_uri, create_args, None)
        .await
        .map_err(|e| uri_failure(create_uri, &e))?;

    let transport_id = created
        .get("transport")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            format!(
                "TOOL_ERROR: transport.create did not return a transport id. Got: {}",
                created
            )
        })?;

    let play_uri = "ak.wwise.core.transport.executeAction";
    if let Err(e) = waapi
        .call(
            play_uri,
            json!({ "transport": transport_id, "action": "play" }),
            None,
        )
        .await
    {
        // 播放失败时清理刚创建的 transport，避免留下孤儿
        let _ = waapi
            .call(
                "ak.wwise.core.transport.destroy",
                json!({ "transport": transport_id }),
                None,
            )
            .await;
        return Err(uri_failure(play_uri, &e));
    }

    Ok(json!({
        "transport": transport_id,
        "object": object,
        "action": "play",
        "note": "Playback started. Call transport_control with action=stop when finished, \
                 or transport_list to see active transports.",
    }))
}

/// 对一个（或全部）transport 执行 play/stop/pause。stop 时可顺带销毁。
pub async fn transport_control(
    waapi: &WaapiClient,
    action: &str,
    transport_id: Option<u64>,
    destroy: bool,
) -> Result<Value, String> {
    let action = canonical_transport_action(action).ok_or_else(|| {
        format!(
            "TOOL_ERROR: '{}' is not a valid transport action. Valid values: {}.",
            action,
            TRANSPORT_ACTIONS.join(", ")
        )
    })?;

    let mut args = json!({ "action": action });
    if let Some(id) = transport_id {
        args["transport"] = json!(id);
    }

    let uri = "ak.wwise.core.transport.executeAction";
    waapi
        .call(uri, args, None)
        .await
        .map_err(|e| uri_failure(uri, &e))?;

    let mut destroyed_count: u64 = 0;
    if destroy && action == "stop" {
        let destroy_uri = "ak.wwise.core.transport.destroy";
        if let Some(id) = transport_id {
            waapi
                .call(destroy_uri, json!({ "transport": id }), None)
                .await
                .map_err(|e| uri_failure(destroy_uri, &e))?;
            destroyed_count = 1;
        } else {
            // 未指定 transport：停止全部后逐个销毁，防止残留孤儿 transport
            let list_uri = "ak.wwise.core.transport.getList";
            let listed = waapi
                .call(list_uri, json!({}), None)
                .await
                .map_err(|e| uri_failure(list_uri, &e))?;
            let ids: Vec<u64> = listed
                .get("list")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|t| t.get("transport").and_then(Value::as_u64))
                        .collect()
                })
                .unwrap_or_default();
            for id in ids {
                waapi
                    .call(destroy_uri, json!({ "transport": id }), None)
                    .await
                    .map_err(|e| uri_failure(destroy_uri, &e))?;
                destroyed_count += 1;
            }
        }
    }

    Ok(json!({
        "action": action,
        "transport": transport_id,
        "destroyed": destroyed_count > 0,
        "destroyed_count": destroyed_count,
        "applied_to_all": transport_id.is_none(),
    }))
}

/// 列出当前活跃的 transport。
pub async fn transport_list(waapi: &WaapiClient) -> Result<Value, String> {
    let uri = "ak.wwise.core.transport.getList";
    let result = waapi
        .call(uri, json!({}), None)
        .await
        .map_err(|e| uri_failure(uri, &e))?;

    let list = result
        .get("list")
        .cloned()
        .or_else(|| result.get("return").cloned())
        .unwrap_or(json!([]));
    let count = list.as_array().map(|a| a.len()).unwrap_or(0);

    Ok(json!({
        "count": count,
        "transports": list,
    }))
}

// ───────────────────────── 远程连接域 ─────────────────────────

/// 列出局域网内可连接的游戏实例 / devkit。
pub async fn remote_list_consoles(waapi: &WaapiClient) -> Result<Value, String> {
    let uri = "ak.wwise.core.remote.getAvailableConsoles";
    let result = waapi
        .call(uri, json!({}), None)
        .await
        .map_err(|e| uri_failure(uri, &e))?;

    let consoles = result.get("consoles").cloned().unwrap_or(json!([]));
    let count = consoles.as_array().map(|a| a.len()).unwrap_or(0);

    Ok(json!({
        "count": count,
        "consoles": consoles,
    }))
}

/// 连接到远程游戏实例。host 可以是 IP，也可以配合 app_name 精确匹配。
pub async fn remote_connect(
    waapi: &WaapiClient,
    host: &str,
    app_name: Option<&str>,
    command_port: Option<u64>,
) -> Result<Value, String> {
    let mut args = json!({ "host": host });
    if let Some(name) = app_name {
        if !name.is_empty() {
            args["appName"] = json!(name);
        }
    }
    if let Some(port) = command_port {
        args["commandPort"] = json!(port);
    }

    let uri = "ak.wwise.core.remote.connect";
    waapi
        .call(uri, args.clone(), None)
        .await
        .map_err(|e| uri_failure(uri, &e))?;

    let status = waapi
        .call("ak.wwise.core.remote.getConnectionStatus", json!({}), None)
        .await
        .unwrap_or(json!({}));

    Ok(json!({
        "connected_to": args,
        "status": status,
    }))
}

/// 断开远程连接。
pub async fn remote_disconnect(waapi: &WaapiClient) -> Result<Value, String> {
    let uri = "ak.wwise.core.remote.disconnect";
    waapi
        .call(uri, json!({}), None)
        .await
        .map_err(|e| uri_failure(uri, &e))?;
    Ok(json!({ "disconnected": true }))
}

// ───────────────────────── UI 自动化域 ─────────────────────────

/// 取 Wwise 当前选中对象，默认带上 path / type / parent，省掉一次补查。
pub async fn ui_get_selection(
    waapi: &WaapiClient,
    return_fields: &[String],
) -> Result<Value, String> {
    let fields: Vec<String> = if return_fields.is_empty() {
        vec![
            "id".to_string(),
            "name".to_string(),
            "type".to_string(),
            "path".to_string(),
        ]
    } else {
        return_fields.to_vec()
    };

    let uri = "ak.wwise.ui.getSelectedObjects";
    let result = waapi
        .call(uri, json!({}), Some(json!({ "return": fields })))
        .await
        .map_err(|e| uri_failure(uri, &e))?;

    let objects = result.get("objects").cloned().unwrap_or(json!([]));
    let count = objects.as_array().map(|a| a.len()).unwrap_or(0);
    let first_id = objects
        .as_array()
        .and_then(|a| a.first())
        .and_then(|o| o.get("id"))
        .cloned()
        .unwrap_or(Value::Null);

    Ok(json!({
        "count": count,
        "first_id": first_id,
        "objects": objects,
    }))
}

/// 列出 Wwise UI 可执行的命令 ID（布局切换、Inspector 操作等都在这里）。
pub async fn ui_list_commands(
    waapi: &WaapiClient,
    filter: Option<&str>,
) -> Result<Value, String> {
    let uri = "ak.wwise.ui.commands.getCommands";
    let result = waapi
        .call(uri, json!({}), None)
        .await
        .map_err(|e| uri_failure(uri, &e))?;

    let all: Vec<String> = result
        .get("commands")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    v.as_str()
                        .map(String::from)
                        .or_else(|| v.get("id").and_then(|i| i.as_str()).map(String::from))
                })
                .collect()
        })
        .unwrap_or_default();

    let needle = filter.map(str::to_lowercase).unwrap_or_default();
    let matched: Vec<String> = if needle.is_empty() {
        all.clone()
    } else {
        all.iter()
            .filter(|c| c.to_lowercase().contains(&needle))
            .cloned()
            .collect()
    };

    Ok(json!({
        "total_available": all.len(),
        "filter": filter.unwrap_or(""),
        "matched": matched.len(),
        "commands": matched,
    }))
}

/// 执行一个 Wwise UI 命令，可带作用对象。
pub async fn ui_execute_command(
    waapi: &WaapiClient,
    command: &str,
    object_ids: &[String],
) -> Result<Value, String> {
    let mut args = json!({ "command": command });
    if !object_ids.is_empty() {
        args["objects"] = json!(object_ids);
    }

    let uri = "ak.wwise.ui.commands.execute";
    waapi
        .call(uri, args.clone(), None)
        .await
        .map_err(|e| uri_failure(uri, &e))?;

    Ok(json!({
        "executed": command,
        "object_count": object_ids.len(),
    }))
}

// ───────────────────────── WAAPI 订阅域 ─────────────────────────

/// 订阅一个 WAAPI topic。事件进入内部缓冲，之后用 waapi_poll_events 取。
pub async fn waapi_subscribe(
    waapi: &WaapiClient,
    topic: &str,
    return_fields: &[String],
) -> Result<Value, String> {
    if let Some(existing) = waapi.find_subscription_by_topic(topic).await {
        return Ok(json!({
            "subscription_id": existing,
            "topic": topic,
            "already_subscribed": true,
            "note": "This topic was already subscribed; reusing the existing subscription.",
        }));
    }

    let options = if return_fields.is_empty() {
        None
    } else {
        Some(json!({ "return": return_fields }))
    };

    let id = waapi
        .subscribe(topic, options)
        .await
        .map_err(|e| subscribe_failure(topic, &e))?;

    Ok(json!({
        "subscription_id": id,
        "topic": topic,
        "already_subscribed": false,
        "note": "Events are buffered from now on. Call waapi_poll_events to read them, \
                 or waapi_wait_for_event to block until the next one arrives.",
    }))
}

fn subscribe_failure(topic: &str, err: &str) -> String {
    format!(
        "WAAPI_SUBSCRIBE_ERROR [{}]: {}\n\
         Hint: only the topics reported by waapi_list_topics can be subscribed to, \
         and topic names are not the same as function URIs.",
        topic, err
    )
}

/// 取出缓冲的订阅事件。
pub async fn waapi_poll_events(
    waapi: &WaapiClient,
    subscription_id: Option<u64>,
    max_events: usize,
    consume: bool,
) -> Result<Value, String> {
    let max_events = max_events.clamp(1, 500);
    let events = waapi
        .drain_events(subscription_id, max_events, consume)
        .await;

    let subscriptions: Vec<Value> = waapi
        .list_subscriptions()
        .await
        .iter()
        .map(SubscriptionInfo::to_json)
        .collect();

    let mut result = json!({
        "returned": events.len(),
        "consumed": consume,
        "events": events,
        "active_subscriptions": subscriptions,
    });

    if events.is_empty() {
        result["note"] = json!(
            "No buffered events. Either nothing happened in Wwise yet, or you have not subscribed \
             to the relevant topic. Do not poll in a tight loop: use waapi_wait_for_event instead."
        );
    }

    Ok(result)
}

/// 阻塞等待下一条事件。topic 尚未订阅时会自动补订阅。
pub async fn waapi_wait_for_event(
    waapi: &WaapiClient,
    topic: Option<&str>,
    subscription_id: Option<u64>,
    timeout_ms: u64,
) -> Result<Value, String> {
    let timeout_ms = timeout_ms.clamp(100, 120_000);

    let (sub_id, topic_label, auto_subscribed) = match (subscription_id, topic) {
        (Some(id), _) => {
            let label = waapi
                .list_subscriptions()
                .await
                .iter()
                .find(|info| info.subscription_id == id)
                .map(|info| info.topic.clone())
                .unwrap_or_else(|| "<unknown>".to_string());
            (id, label, false)
        }
        (None, Some(t)) => match waapi.find_subscription_by_topic(t).await {
            Some(id) => (id, t.to_string(), false),
            None => {
                let id = waapi
                    .subscribe(t, None)
                    .await
                    .map_err(|e| subscribe_failure(t, &e))?;
                (id, t.to_string(), true)
            }
        },
        (None, None) => {
            return Err(
                "TOOL_ERROR: provide either 'topic' or 'subscription_id' to wait on.".to_string(),
            )
        }
    };

    let event = waapi.wait_for_event(sub_id, timeout_ms).await?;

    match event {
        Some(event) => Ok(json!({
            "timed_out": false,
            "subscription_id": sub_id,
            "topic": topic_label,
            "auto_subscribed": auto_subscribed,
            "event": event,
        })),
        None => Ok(json!({
            "timed_out": true,
            "subscription_id": sub_id,
            "topic": topic_label,
            "auto_subscribed": auto_subscribed,
            "event": Value::Null,
            "note": format!(
                "No '{}' event arrived within {} ms. The subscription is still active, so you can \
                 wait again or poll later. Report the timeout instead of retrying indefinitely.",
                topic_label, timeout_ms
            ),
        })),
    }
}

/// 列出当前活跃的订阅及各自的缓冲情况。
pub async fn waapi_list_subscriptions(waapi: &WaapiClient) -> Result<Value, String> {
    let subscriptions = waapi.list_subscriptions().await;
    let total_buffered: usize = subscriptions.iter().map(|info| info.buffered).sum();

    Ok(json!({
        "count": subscriptions.len(),
        "total_buffered_events": total_buffered,
        "subscriptions": subscriptions.iter().map(SubscriptionInfo::to_json).collect::<Vec<_>>(),
    }))
}

/// 退订。unsubscribe_all=true 时退订全部。
pub async fn waapi_unsubscribe(
    waapi: &WaapiClient,
    subscription_id: Option<u64>,
    unsubscribe_all: bool,
) -> Result<Value, String> {
    if unsubscribe_all {
        let mut removed = Vec::new();
        let mut failed = Vec::new();
        for info in waapi.list_subscriptions().await {
            match waapi.unsubscribe(info.subscription_id).await {
                Ok(()) => removed.push(json!({
                    "subscription_id": info.subscription_id,
                    "topic": info.topic,
                })),
                Err(e) => failed.push(format!("{} ({}): {}", info.subscription_id, info.topic, e)),
            }
        }
        return Ok(json!({
            "unsubscribed": removed.len(),
            "details": removed,
            "failed": failed,
        }));
    }

    let id = subscription_id.ok_or(
        "TOOL_ERROR: provide 'subscription_id', or set 'unsubscribe_all' to true.".to_string(),
    )?;

    waapi.unsubscribe(id).await?;
    Ok(json!({ "unsubscribed": 1, "subscription_id": id }))
}

// ───────────────────────── 参数解析（供 tools.rs 复用） ─────────────────────────

pub fn arg_str_vec(args: &Value, key: &str) -> Vec<String> {
    str_vec(args, key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn str_vec_trims_and_drops_empty() {
        let args = json!({ "ids": ["  a  ", "", "b", "   "] });
        assert_eq!(str_vec(&args, "ids"), vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn str_vec_missing_key_is_empty() {
        let args = json!({});
        assert!(str_vec(&args, "ids").is_empty());
    }

    #[test]
    fn str_vec_ignores_non_string_entries() {
        let args = json!({ "ids": ["a", 1, null, true, "b"] });
        assert_eq!(str_vec(&args, "ids"), vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn bus_aliases_resolve_to_meter_data_type() {
        // "busses" 不是合法枚举值，必须归一到 "meter"
        for alias in ["busses", "buses", "Bus", "METERING"] {
            assert_eq!(canonical_profiler_data_type(alias), Some("meter"));
        }
    }

    #[test]
    fn performance_alias_resolves_to_cpu() {
        assert_eq!(canonical_profiler_data_type("performance"), Some("cpu"));
        assert_eq!(canonical_profiler_data_type(" perf "), Some("cpu"));
    }

    #[test]
    fn canonical_data_type_preserves_wwise_casing() {
        assert_eq!(canonical_profiler_data_type("audioobjects"), Some("audioObjects"));
        assert_eq!(canonical_profiler_data_type("gameSyncs"), Some("gameSyncs"));
    }

    #[test]
    fn unknown_data_type_is_rejected_with_valid_values() {
        let err = resolve_profiler_data_types(&["nonsense".to_string()], false, false, false)
            .expect_err("unknown data type must be rejected");
        assert!(err.contains("nonsense"));
        assert!(err.contains("meter"));
        assert!(err.contains("not 'busses'"));
    }

    #[test]
    fn include_flags_add_their_required_data_types() {
        let types = resolve_profiler_data_types(&[], true, true, true).unwrap();
        assert_eq!(types, vec!["voices", "meter", "cpu"]);
    }

    #[test]
    fn empty_request_falls_back_to_voices_and_meter() {
        let types = resolve_profiler_data_types(&[], false, false, false).unwrap();
        assert_eq!(types, vec!["voices", "meter"]);
    }

    #[test]
    fn resolved_data_types_are_deduplicated() {
        let types = resolve_profiler_data_types(
            &["voices".to_string(), "busses".to_string(), "meter".to_string()],
            true,
            true,
            false,
        )
        .unwrap();
        assert_eq!(types, vec!["voices", "meter"]);
    }

    #[test]
    fn uri_failure_mentions_introspection_path() {
        let msg = uri_failure("ak.wwise.core.profiler.getVoices", "boom");
        assert!(msg.contains("ak.wwise.core.profiler.getVoices"));
        assert!(msg.contains("boom"));
        assert!(msg.contains("waapi_list_functions"));
        assert!(msg.contains("waapi_get_schema"));
    }

    #[test]
    fn play_aliases_resolve_to_play() {
        for alias in ["play", "Play", "START", "preview"] {
            assert_eq!(canonical_transport_action(alias), Some("play"));
        }
    }

    #[test]
    fn stop_and_pause_aliases() {
        assert_eq!(canonical_transport_action("stop"), Some("stop"));
        assert_eq!(canonical_transport_action("halt"), Some("stop"));
        assert_eq!(canonical_transport_action("pause"), Some("pause"));
    }

    #[test]
    fn toggle_alias_resolves_to_play_stop() {
        assert_eq!(canonical_transport_action("toggle"), Some("playStop"));
        assert_eq!(canonical_transport_action("play_stop"), Some("playStop"));
        assert_eq!(canonical_transport_action("playStop"), Some("playStop"));
    }

    #[test]
    fn direct_play_alias() {
        assert_eq!(canonical_transport_action("direct"), Some("playDirectly"));
        assert_eq!(
            canonical_transport_action("play_directly"),
            Some("playDirectly")
        );
    }

    #[test]
    fn unknown_transport_action_is_rejected() {
        assert!(canonical_transport_action("rewind").is_none());
        assert!(canonical_transport_action("").is_none());
    }

    #[tokio::test]
    async fn transport_play_rejects_empty_object_before_connecting() {
        let waapi = WaapiClient::new();
        let err = transport_play(&waapi, "  ", None)
            .await
            .expect_err("empty object must fail");
        assert!(err.contains("object"));
    }

    #[tokio::test]
    async fn transport_control_rejects_unknown_action_before_connecting() {
        let waapi = WaapiClient::new();
        let err = transport_control(&waapi, "rewind", None, false)
            .await
            .expect_err("unknown action must fail");
        assert!(err.contains("rewind"));
        assert!(err.contains("playStop"));
    }

    #[tokio::test]
    async fn generate_soundbanks_requires_banks_when_adding_inclusions() {
        let waapi = WaapiClient::new();
        let err = generate_soundbanks(
            &waapi,
            &[],
            &[],
            &[],
            &["{GUID}".to_string()],
            &[],
            false,
            true,
        )
        .await
        .expect_err("inclusions without soundbanks must fail");
        assert!(err.contains("inclusions_object_ids"));
        assert!(err.contains("soundbanks"));
    }
}
