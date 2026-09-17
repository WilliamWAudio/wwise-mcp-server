// GWwiseAgent Tools Module — © 2025-2026 william.wang

use serde_json::{json, Value};
use std::collections::HashSet;

use crate::llm::debug_log;
use crate::waapi::WaapiClient;

/// 根据工具名和参数生成一句可读的操作描述（用于确认弹窗）
pub fn describe_wwise_action(tool_name: &str, arguments: &str) -> String {
    let args: Value = match serde_json::from_str(arguments) {
        Ok(v) => v,
        Err(_) => return format!("执行：{}", tool_name),
    };
    match tool_name {
        "batch_delete_unused_descendants" => {
            let count = args
                .get("object_ids")
                .and_then(|v| v.as_array())
                .map(|arr| arr.len())
                .unwrap_or(0);
            let preview_only = args
                .get("preview_only")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if preview_only {
                format!("预检删除 {} 个结构下未使用的子对象", count)
            } else {
                format!("删除 {} 个结构下未使用的子对象", count)
            }
        }
        "batch_smart_delete" => {
            let count = args
                .get("object_ids")
                .and_then(|v| v.as_array())
                .map(|arr| arr.len())
                .unwrap_or(0);
            let preview_only = args
                .get("preview_only")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if preview_only {
                format!("预检智能删除 {} 个对象", count)
            } else {
                format!("智能删除 {} 个对象", count)
            }
        }
        "batch_delete" => {
            let count = args
                .get("object_ids")
                .and_then(|v| v.as_array())
                .map(|arr| arr.len())
                .unwrap_or(0);
            format!("删除 {} 个对象", count)
        }
        "cleanup_unused_originals_files" => {
            let preview_only = args
                .get("preview_only")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let root = args
                .get("originals_root")
                .and_then(|v| v.as_str())
                .unwrap_or("工程 Originals");
            if preview_only {
                format!("预览未使用 Originals 文件：{}", root)
            } else {
                format!("清理未使用 Originals 文件：{}", root)
            }
        }
        "batch_copy_missing_descendants" | "batch_move_missing_descendants" => {
            let source = args
                .get("source_parent_name")
                .and_then(|v| v.as_str())
                .unwrap_or("Skill_Hit");
            let target = args
                .get("target_parent_name")
                .and_then(|v| v.as_str())
                .unwrap_or("Skill_Release");
            let subtree = args
                .get("subtree_name")
                .and_then(|v| v.as_str())
                .unwrap_or("GeneralSkills");
            let preview_only = args
                .get("preview_only")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let action = if tool_name == "batch_move_missing_descendants" {
                "Move"
            } else {
                "Copy"
            };
            if preview_only {
                format!(
                    "Preview {} missing descendants from {}\\{} to {}\\{}",
                    action.to_lowercase(),
                    source,
                    subtree,
                    target,
                    subtree
                )
            } else {
                format!(
                    "{} missing descendants from {}\\{} to {}\\{}",
                    action, source, subtree, target, subtree
                )
            }
        }
        "batch_sync_output_bus_by_relative_path" => {
            let preview_only = args
                .get("preview_only")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let count = args
                .get("target_object_ids")
                .or_else(|| args.get("object_ids"))
                .and_then(|v| v.as_array())
                .map(|arr| arr.len())
                .unwrap_or(0);
            if preview_only {
                format!(
                    "Preview OutputBus sync for {} target objects by relative path",
                    count
                )
            } else {
                format!(
                    "Sync OutputBus for {} target objects by relative path",
                    count
                )
            }
        }
        "waapi_query" => {
            let waql = args.get("waql").and_then(|v| v.as_str()).unwrap_or("");
            let short = if waql.len() > 60 {
                format!("{}...", &waql[..60])
            } else {
                waql.to_string()
            };
            format!("执行 WAQL 查询：{}", short)
        }
        "list_local_audio_files" => {
            let dir = args.get("directory").and_then(|v| v.as_str()).unwrap_or("");
            format!("读取本地目录：{}", dir)
        }
        "waapi_call" => {
            let uri = args.get("uri").and_then(|v| v.as_str()).unwrap_or("");
            let inner = args
                .get("args")
                .or(args.get("parameters"))
                .cloned()
                .unwrap_or(Value::Null);
            let obj = inner.as_object().or_else(|| args.as_object());
            let get = |k: &str| {
                obj.and_then(|o| o.get(k))
                    .and_then(|v| v.as_str())
                    .map(String::from)
            };
            let path_last = |s: &str| s.rsplit(&['\\', '/'][..]).next().unwrap_or(s).to_string();
            let get_ref_name = |k: &str| {
                obj.and_then(|o| o.get(k)).and_then(|v| {
                    v.get("name")
                        .and_then(|n| n.as_str())
                        .map(String::from)
                        .or_else(|| v.get("path").and_then(|p| p.as_str()).map(path_last))
                        .or_else(|| v.as_str().map(String::from))
                })
            };
            let get_arr_names = |k: &str| {
                obj.and_then(|o| o.get(k))
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| {
                                v.as_str()
                                    .map(String::from)
                                    .or_else(|| {
                                        v.get("name").and_then(|n| n.as_str()).map(String::from)
                                    })
                                    .or_else(|| {
                                        v.get("path")
                                            .and_then(|p| p.as_str())
                                            .map(|s| s.rsplit('\\').next().unwrap_or(s).into())
                                    })
                            })
                            .take(3)
                            .collect::<Vec<_>>()
                    })
            };
            let desc = match uri {
                "ak.wwise.core.object.create" => {
                    let name = get("name").unwrap_or_else(|| "未命名".into());
                    let obj_type = get("type").unwrap_or_else(|| "Object".into());
                    format!("创建 {}：{}", obj_type, name)
                }
                "ak.wwise.core.object.delete" => {
                    let target = get_ref_name("object")
                        .or_else(|| get("object"))
                        .or_else(|| get("name"))
                        .unwrap_or_else(|| "指定对象".into());
                    format!("删除对象：{}", target)
                }
                "ak.wwise.core.soundbank.setInclusions" => {
                    let bank = get_ref_name("soundbank")
                        .or_else(|| get("soundbank"))
                        .or_else(|| get("name"))
                        .unwrap_or_else(|| "Bank".into());
                    if let Some(names) = get_arr_names("object") {
                        if names.is_empty() {
                            format!("设置 Bank「{}」的包含项", bank)
                        } else {
                            format!("将「{}」等放入 Bank「{}」", names.join("」「"), bank)
                        }
                    } else {
                        format!("设置 Bank「{}」的包含项", bank)
                    }
                }
                "ak.wwise.core.audio.import" => {
                    let name = get("objectPath")
                        .or_else(|| get("name"))
                        .unwrap_or_else(|| "音频".into());
                    format!("导入音频：{}", name)
                }
                "ak.wwise.core.object.setName" => {
                    let new_name = get("value").unwrap_or_else(|| "".into());
                    if new_name.is_empty() {
                        "重命名对象".into()
                    } else {
                        format!("重命名为：「{}」", new_name)
                    }
                }
                "ak.wwise.core.object.setProperty" => {
                    let prop = get("property").unwrap_or_else(|| "?".into());
                    let val = obj
                        .and_then(|o| o.get("value"))
                        .map(|v| {
                            if v.is_f64() || v.is_i64() {
                                v.to_string()
                            } else if let Some(s) = v.as_str() {
                                s.to_string()
                            } else if v.is_boolean() {
                                v.as_bool().map(|b| b.to_string()).unwrap_or_default()
                            } else {
                                v.to_string()
                            }
                        })
                        .unwrap_or_else(|| "?".into());
                    let obj_raw = get_ref_name("object").or_else(|| get("object"));
                    let is_guid = obj_raw.as_ref().map_or(false, |s| {
                        (s.contains('-') && s.len() > 30) || s.starts_with('{')
                    });
                    let obj_label = obj_raw.as_ref().map(|s| {
                        if s.contains('\\') || s.contains('/') {
                            path_last(s)
                        } else {
                            s.clone()
                        }
                    });
                    if is_guid || obj_label.as_ref().map_or(true, |s| s.is_empty()) {
                        format!("属性「{}」→ {}", prop, val)
                    } else {
                        format!(
                            "对象「{}」：属性「{}」→ {}",
                            obj_label.unwrap_or_default(),
                            prop,
                            val
                        )
                    }
                }
                _ if uri.contains("create") => {
                    let name = get("name").unwrap_or_else(|| "对象".into());
                    format!("创建：{}", name)
                }
                _ if uri.contains("delete") => get("object")
                    .map(|o| format!("删除：{}", o))
                    .unwrap_or_else(|| "删除对象".into()),
                _ => format!("执行 WAAPI：{}", uri),
            };
            desc
        }
        "wwise_get_info" => "查询 Wwise 版本与当前工程信息".to_string(),
        "transport_play" => {
            let object = args.get("object").and_then(|v| v.as_str()).unwrap_or("对象");
            format!("试听：{}", object)
        }
        "transport_control" => {
            let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("stop");
            format!("传输控制：{}", action)
        }
        "transport_list" => "列出当前试听传输".to_string(),
        "generate_soundbanks" => "生成 SoundBank".to_string(),
        "profiler_capture" => "采集 Profiler 数据".to_string(),
        "ui_get_selection" => "读取 Wwise 当前选中对象".to_string(),
        "waapi_list_functions" => "列出当前 Wwise 暴露的 WAAPI 接口".to_string(),
        _ => format!("执行：{}", tool_name),
    }
}

/// If a Value is a string that parses as a JSON object/array, decode it.
fn unwrap_stringified_json(val: &Value) -> Value {
    match val {
        Value::String(s) => {
            let trimmed = s.trim();
            if (trimmed.starts_with('{') && trimmed.ends_with('}'))
                || (trimmed.starts_with('[') && trimmed.ends_with(']'))
            {
                serde_json::from_str(trimmed).unwrap_or_else(|_| val.clone())
            } else {
                val.clone()
            }
        }
        Value::Object(map) => {
            let fixed: serde_json::Map<String, Value> = map
                .iter()
                .map(|(k, v)| (k.clone(), unwrap_stringified_json(v)))
                .collect();
            Value::Object(fixed)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(unwrap_stringified_json).collect()),
        other => other.clone(),
    }
}

fn fix_malformed_guids(val: &Value) -> Value {
    match val {
        Value::String(s) => {
            let t = s.trim();
            let hex_dash = |c: char| c.is_ascii_hexdigit() || c == '-';
            let looks_like_guid = |s: &str| {
                s.len() >= 36
                    && s.chars().all(hex_dash)
                    && s.chars().filter(|&c| c == '-').count() == 4
            };
            if t.starts_with('{') && t.ends_with('}') && looks_like_guid(&t[1..t.len() - 1]) {
                return val.clone();
            }
            let stripped = t.trim_start_matches('{').trim_end_matches('}');
            if looks_like_guid(stripped) {
                Value::String(format!("{{{}}}", stripped))
            } else {
                val.clone()
            }
        }
        Value::Object(map) => {
            let fixed: serde_json::Map<String, Value> = map
                .iter()
                .map(|(k, v)| (k.clone(), fix_malformed_guids(v)))
                .collect();
            Value::Object(fixed)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(fix_malformed_guids).collect()),
        other => other.clone(),
    }
}

fn extract_waapi_args(args: &Value) -> Value {
    let raw = if let Some(a) = args.get("args") {
        if a.is_object() {
            a.clone()
        } else {
            _extract_fallback(args)
        }
    } else if let Some(a) = args.get("parameters") {
        if a.is_object() {
            a.clone()
        } else {
            _extract_fallback(args)
        }
    } else {
        _extract_fallback(args)
    };
    let unwrapped = unwrap_stringified_json(&raw);
    let mut result = fix_malformed_guids(&unwrapped);
    for key in &["from", "transform"] {
        if result.get(*key).is_none() {
            if let Some(v) = args.get(*key) {
                result[*key] = unwrap_stringified_json(v);
            }
        }
    }
    result
}

fn normalized_key(key: &str) -> String {
    key.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn extract_ref_string(val: &Value) -> Option<String> {
    match val {
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Value::Object(map) => map
            .get("id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(String::from)
            .or_else(|| {
                map.get("path")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.trim().is_empty())
                    .map(String::from)
            })
            .or_else(|| {
                map.get("name")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.trim().is_empty())
                    .map(String::from)
            }),
        _ => None,
    }
}

fn normalize_action_type_value(val: &Value) -> Option<Value> {
    if val.is_i64() || val.is_u64() {
        return Some(val.clone());
    }
    if let Some(s) = val.as_str() {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return None;
        }
        if let Ok(num) = trimmed.parse::<i64>() {
            return Some(Value::from(num));
        }
        let compact = trimmed
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .flat_map(|c| c.to_lowercase())
            .collect::<String>();
        let action_type = match compact.as_str() {
            "play" => Some(1),
            "stop" => Some(2),
            "pause" => Some(7),
            "resume" => Some(9),
            "break" => Some(34),
            "seek" => Some(36),
            _ => None,
        }?;
        return Some(Value::from(action_type));
    }
    None
}

fn normalize_action_target_value(val: &Value) -> Option<Value> {
    extract_ref_string(val).map(Value::String)
}

fn normalize_action_create_args(call_args: &mut Value) {
    let Some(obj) = call_args.as_object_mut() else {
        return;
    };
    let action_type_key = "@ActionType";
    let target_key = "@Target";

    let mut action_alias: Option<(String, Value)> = None;
    let mut target_alias: Option<(String, Value)> = None;

    let keys = obj.keys().cloned().collect::<Vec<_>>();
    for key in keys {
        if key == action_type_key || key == target_key {
            continue;
        }
        let Some(value) = obj.get(&key).cloned() else {
            continue;
        };
        let normalized = normalized_key(&key);
        let matches_action_type = matches!(normalized.as_str(), "wg" | "x" | "actiontype")
            || normalized.ends_with("actiontype");
        let matches_target =
            matches!(normalized.as_str(), "wh" | "y" | "target") || normalized.ends_with("target");

        if action_alias.is_none() && matches_action_type {
            if let Some(normalized_value) = normalize_action_type_value(&value) {
                action_alias = Some((key.clone(), normalized_value));
            }
        }
        if target_alias.is_none() && matches_target {
            if let Some(normalized_value) = normalize_action_target_value(&value) {
                target_alias = Some((key.clone(), normalized_value));
            }
        }
    }

    if let Some(value) = obj.get(action_type_key).cloned() {
        if let Some(normalized_value) = normalize_action_type_value(&value) {
            obj.insert(action_type_key.into(), normalized_value);
        }
    } else if let Some((alias_key, normalized_value)) = action_alias {
        debug_log(&format!(
            "[waapi_call] normalized Action alias {} -> {}",
            alias_key, action_type_key
        ));
        obj.insert(action_type_key.into(), normalized_value);
        obj.remove(&alias_key);
    }

    if let Some(value) = obj.get(target_key).cloned() {
        if let Some(normalized_value) = normalize_action_target_value(&value) {
            obj.insert(target_key.into(), normalized_value);
        }
    } else if let Some((alias_key, normalized_value)) = target_alias {
        debug_log(&format!(
            "[waapi_call] normalized Action alias {} -> {}",
            alias_key, target_key
        ));
        obj.insert(target_key.into(), normalized_value);
        obj.remove(&alias_key);
    }
}

fn normalize_waapi_call_args(uri: &str, call_args: &mut Value) {
    if uri != "ak.wwise.core.object.create" {
        return;
    }

    let is_action = call_args
        .get("type")
        .and_then(|v| v.as_str())
        .map(|s| s.eq_ignore_ascii_case("Action"))
        .unwrap_or(false);

    if is_action {
        normalize_action_create_args(call_args);
    }
}

fn build_object_create_args(
    parent: &str,
    child: &serde_json::Map<String, Value>,
) -> Result<Value, String> {
    let child_type = child
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or("object.set emulation requires each child to include a string 'type' field")?;
    let child_name = child
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or("object.set emulation requires each child to include a string 'name' field")?;

    let mut args = serde_json::Map::new();
    args.insert("parent".into(), Value::String(parent.to_string()));
    args.insert("type".into(), Value::String(child_type.to_string()));
    args.insert("name".into(), Value::String(child_name.to_string()));
    args.insert(
        "onNameConflict".into(),
        child
            .get("onNameConflict")
            .cloned()
            .unwrap_or_else(|| Value::String("rename".into())),
    );

    for (key, value) in child {
        match key.as_str() {
            "type" | "name" | "onNameConflict" | "children" | "import" | "object" => {}
            _ if value.is_object() || value.is_array() => {}
            _ => {
                args.insert(key.clone(), value.clone());
            }
        }
    }

    Ok(Value::Object(args))
}

async fn emulate_object_set(waapi: &WaapiClient, call_args: &Value) -> Result<Value, String> {
    let objects = call_args
        .get("objects")
        .and_then(|v| v.as_array())
        .ok_or("object.set emulation requires an 'objects' array")?;

    let mut created = Vec::new();
    let mut pending: Vec<(String, Vec<Value>)> = Vec::new();

    for root in objects {
        let root_obj = root
            .as_object()
            .ok_or("object.set emulation requires each root entry to be an object")?;
        let parent = root_obj.get("object").and_then(extract_ref_string).ok_or(
            "object.set emulation requires each root entry to include an 'object' parent reference",
        )?;
        let children = root_obj
            .get("children")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        pending.push((parent, children));
    }

    while let Some((parent, children)) = pending.pop() {
        for child in children {
            let child_obj = child
                .as_object()
                .ok_or("object.set emulation requires each child entry to be an object")?;

            if child_obj.get("import").is_some() {
                return Err(
                    "object.set emulation does not support inline 'import'. Use audio.import or batch_replace_audio_by_name instead."
                        .into(),
                );
            }

            let create_args = build_object_create_args(&parent, child_obj)?;
            let child_type = child_obj
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("Object");
            let child_name = child_obj
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Unnamed");

            let result = waapi
                .call("ak.wwise.core.object.create", create_args, None)
                .await
                .map_err(|e| {
                    format!(
                        "object.set emulation failed while creating {} '{}': {}",
                        child_type, child_name, e
                    )
                })?;

            let child_ref = result
                .get("id")
                .and_then(|v| v.as_str())
                .map(String::from)
                .or_else(|| {
                    result
                        .get("name")
                        .and_then(|v| v.as_str())
                        .map(String::from)
                })
                .ok_or_else(|| {
                    format!(
                        "object.set emulation created {} '{}' but did not receive an id/name back",
                        child_type, child_name
                    )
                })?;

            created.push(serde_json::json!({
                "id": result.get("id").cloned().unwrap_or(Value::Null),
                "name": result.get("name").cloned().unwrap_or_else(|| Value::String(child_name.to_string())),
                "type": child_type,
                "parent": parent,
            }));

            if let Some(grandchildren) = child_obj.get("children").and_then(|v| v.as_array()) {
                if !grandchildren.is_empty() {
                    pending.push((child_ref, grandchildren.clone()));
                }
            }
        }
    }

    Ok(serde_json::json!({
        "emulated": true,
        "createdCount": created.len(),
        "created": created,
    }))
}

fn expand_waapi_uri(uri: &str) -> String {
    if uri.starts_with("ak.wwise.") {
        return uri.to_string();
    }
    if uri.starts_with("ak.soundengine.") {
        return uri.to_string();
    }
    let known_prefixes = [
        ("object.", "ak.wwise.core.object."),
        ("soundbank.", "ak.wwise.core.soundbank."),
        ("audio.", "ak.wwise.core.audio."),
        ("soundengine.", "ak.soundengine."),
        ("profiler.", "ak.wwise.core.profiler."),
        ("ui.", "ak.wwise.ui."),
        ("project.", "ak.wwise.core.project."),
        ("core.", "ak.wwise.core."),
    ];
    for (short, full) in &known_prefixes {
        if uri.starts_with(short) {
            return format!("{}{}", full, &uri[short.len()..]);
        }
    }
    if !uri.contains('.') {
        return format!("ak.wwise.core.{}", uri);
    }
    uri.to_string()
}

fn uri_supports_options(uri: &str) -> bool {
    matches!(
        uri,
        "ak.wwise.core.object.get" | "ak.wwise.core.audio.import" | "ak.wwise.core.object.set"
    )
}

fn _extract_fallback(args: &Value) -> Value {
    let mut fallback = args.as_object().cloned().unwrap_or_default();
    fallback.remove("uri");
    fallback.remove("options");
    Value::Object(fallback)
}

async fn execute_batch_tool(
    waapi: &WaapiClient,
    tool_name: &str,
    args: &Value,
) -> Result<String, String> {
    use crate::batch_ops;

    debug_log(&format!("[batch_tool] name={}", tool_name));

    // --- Tools that do NOT require object_ids ---
    if tool_name == "batch_replace_audio_by_name" {
        let target_location = args
            .get("target_object_id")
            .and_then(|v| v.as_str())
            .ok_or("TOOL_ERROR: Missing 'target_object_id' field")?;
        let local_directory = args
            .get("local_directory")
            .and_then(|v| v.as_str())
            .ok_or("TOOL_ERROR: Missing 'local_directory' field")?;
        let language = args
            .get("language")
            .and_then(|v| v.as_str())
            .unwrap_or("SFX");
        let originals_subfolder = args
            .get("originals_subfolder")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let result = batch_ops::batch_replace_audio_by_name(
            waapi,
            target_location,
            local_directory,
            language,
            originals_subfolder,
        )
        .await?;

        return Ok(format_batch_result(tool_name, &result));
    }

    if matches!(
        tool_name,
        "batch_copy_missing_descendants" | "batch_move_missing_descendants"
    ) {
        let object_ids = args
            .get("object_ids")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let source_parent_name = args
            .get("source_parent_name")
            .and_then(|v| v.as_str())
            .unwrap_or("Skill_Hit");
        let target_parent_name = args
            .get("target_parent_name")
            .and_then(|v| v.as_str())
            .unwrap_or("Skill_Release");
        let subtree_name = args
            .get("subtree_name")
            .and_then(|v| v.as_str())
            .unwrap_or("GeneralSkills");
        let subtree_type = args
            .get("subtree_type")
            .and_then(|v| v.as_str())
            .unwrap_or("ActorMixer");
        let source_root_ref = args.get("source_root_id").and_then(|v| v.as_str());
        let target_root_ref = args.get("target_root_id").and_then(|v| v.as_str());
        let match_type = args
            .get("match_type")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let case_sensitive = args
            .get("case_sensitive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let preview_only = args
            .get("preview_only")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let sync_output_bus = args
            .get("sync_output_bus")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let on_name_conflict = args
            .get("on_name_conflict")
            .and_then(|v| v.as_str())
            .unwrap_or("fail");

        let result = if tool_name == "batch_move_missing_descendants" {
            batch_ops::batch_move_missing_descendants(
                waapi,
                object_ids,
                source_parent_name,
                target_parent_name,
                subtree_name,
                subtree_type,
                source_root_ref,
                target_root_ref,
                match_type,
                case_sensitive,
                preview_only,
                on_name_conflict,
                sync_output_bus,
            )
            .await?
        } else {
            batch_ops::batch_copy_missing_descendants(
                waapi,
                object_ids,
                source_parent_name,
                target_parent_name,
                subtree_name,
                subtree_type,
                source_root_ref,
                target_root_ref,
                match_type,
                case_sensitive,
                preview_only,
                on_name_conflict,
                sync_output_bus,
            )
            .await?
        };

        return Ok(format_json_tool_result(tool_name, &result));
    }

    if tool_name == "batch_sync_output_bus_by_relative_path" {
        let source_root_id = args
            .get("source_root_id")
            .or_else(|| args.get("source_root_ref"))
            .and_then(|v| v.as_str())
            .ok_or("TOOL_ERROR: Missing 'source_root_id' field")?;
        let target_root_id = args
            .get("target_root_id")
            .or_else(|| args.get("target_root_ref"))
            .and_then(|v| v.as_str())
            .ok_or("TOOL_ERROR: Missing 'target_root_id' field")?;
        let target_object_ids = args
            .get("target_object_ids")
            .or_else(|| args.get("object_ids"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let include_target_descendants = args
            .get("include_target_descendants")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let case_sensitive = args
            .get("case_sensitive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let preview_only = args
            .get("preview_only")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let result = batch_ops::batch_sync_output_bus_by_relative_path(
            waapi,
            source_root_id,
            target_root_id,
            target_object_ids,
            include_target_descendants,
            case_sensitive,
            preview_only,
        )
        .await?;

        return Ok(format_json_tool_result(tool_name, &result));
    }

    // --- Tools that require object_ids ---
    let mut object_ids = args
        .get("object_ids")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .ok_or("TOOL_ERROR: Missing or invalid 'object_ids' array")?;

    let enable_recursive = args
        .get("recursive")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if enable_recursive {
        let filter_type = args
            .get("filterType")
            .and_then(|v| v.as_str())
            .unwrap_or("*");

        object_ids = batch_ops::get_recursive_objects(waapi, object_ids, filter_type).await?;
    }

    debug_log(&format!(
        "[batch_tool] object_count={} recursive={}",
        object_ids.len(),
        enable_recursive
    ));

    let result = match tool_name {
        "batch_rename_lowercase" => batch_ops::batch_rename_to_lowercase(waapi, object_ids).await?,
        "batch_rename_titlecase" => batch_ops::batch_rename_to_titlecase(waapi, object_ids).await?,
        "batch_rename_uppercase" => batch_ops::batch_rename_to_uppercase(waapi, object_ids).await?,
        "batch_rename_find_replace" => {
            let find = args
                .get("find")
                .and_then(|v| v.as_str())
                .ok_or("TOOL_ERROR: Missing 'find' field")?;
            let replace = args
                .get("replace")
                .and_then(|v| v.as_str())
                .ok_or("TOOL_ERROR: Missing 'replace' field")?;
            batch_ops::batch_rename_find_replace(waapi, object_ids, find, replace).await?
        }
        "batch_rename_add_prefix" => {
            let prefix = args
                .get("prefix")
                .and_then(|v| v.as_str())
                .ok_or("TOOL_ERROR: Missing 'prefix' field")?;
            batch_ops::batch_rename_add_prefix(waapi, object_ids, prefix).await?
        }
        "batch_rename_add_suffix" => {
            let suffix = args
                .get("suffix")
                .and_then(|v| v.as_str())
                .ok_or("TOOL_ERROR: Missing 'suffix' field")?;
            batch_ops::batch_rename_add_suffix(waapi, object_ids, suffix).await?
        }
        "batch_delete" => batch_ops::batch_delete(waapi, object_ids).await?,
        "batch_smart_delete" => {
            let name_contains = args.get("name_contains").and_then(|v| v.as_str());
            let exclude_name_contains = args
                .get("exclude_name_contains")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let case_sensitive = args
                .get("case_sensitive")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let check_references = args
                .get("check_references")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let reference_types = args
                .get("reference_types")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let block_if_referenced = args
                .get("block_if_referenced")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let preview_only = args
                .get("preview_only")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let result = batch_ops::batch_smart_delete(
                waapi,
                object_ids,
                name_contains,
                &exclude_name_contains,
                case_sensitive,
                check_references,
                &reference_types,
                block_if_referenced,
                preview_only,
            )
            .await?;

            return Ok(format_json_tool_result(tool_name, &result));
        }
        "batch_delete_unused_descendants" => {
            let candidate_types = args
                .get("candidate_types")
                .or_else(|| args.get("type_list"))
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect::<Vec<_>>()
                })
                .or_else(|| {
                    args.get("filterType")
                        .and_then(|v| v.as_str())
                        .map(|value| vec![value.to_string()])
                })
                .unwrap_or_default();
            let include_root = args
                .get("include_root")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let check_references = args
                .get("check_references")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let reference_types = args
                .get("reference_types")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let preview_only = args
                .get("preview_only")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let result = batch_ops::batch_delete_unused_descendants(
                waapi,
                object_ids,
                &candidate_types,
                include_root,
                check_references,
                &reference_types,
                preview_only,
            )
            .await?;

            return Ok(format_json_tool_result(tool_name, &result));
        }
        "batch_convert_type" => {
            let target_type = args
                .get("target_type")
                .and_then(|v| v.as_str())
                .ok_or("TOOL_ERROR: Missing 'target_type' field")?;
            batch_ops::batch_convert_type(waapi, object_ids, target_type).await?
        }
        "batch_set_property" => {
            let property = args
                .get("property")
                .and_then(|v| v.as_str())
                .ok_or("TOOL_ERROR: Missing 'property' field")?;
            let value = args
                .get("value")
                .ok_or("TOOL_ERROR: Missing 'value' field")?;
            batch_ops::batch_set_property(waapi, object_ids, property, value.clone()).await?
        }
        "batch_move" => {
            let new_parent_id = args
                .get("new_parent_id")
                .and_then(|v| v.as_str())
                .ok_or("TOOL_ERROR: Missing 'new_parent_id' field")?;
            batch_ops::batch_move(waapi, object_ids, new_parent_id).await?
        }
        "batch_get_children_by_type" => {
            let type_list = args
                .get("type_list")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect::<Vec<_>>()
                })
                .ok_or("TOOL_ERROR: Missing or invalid 'type_list' array")?;
            let keep_self = args
                .get("keep_self")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let include_descendants = args
                .get("include_descendants")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);

            let result_ids = batch_ops::batch_get_children_by_type(
                waapi,
                object_ids,
                type_list.clone(),
                keep_self,
                include_descendants,
            )
            .await?;

            let output = serde_json::json!({
                "object_ids": result_ids.clone(),
                "count": result_ids.len(),
                "message": format!("找到 {} 个符合类型 {:?} 的对象", result_ids.len(), type_list)
            });

            return Ok(format!(
                "BATCH_SUCCESS [batch_get_children_by_type]:\n{}",
                serde_json::to_string_pretty(&output).unwrap_or_default()
            ));
        }
        _ => {
            return Err(format!("TOOL_ERROR: Unknown batch tool '{}'", tool_name));
        }
    };

    Ok(format_batch_result(tool_name, &result))
}

fn format_batch_result(tool_name: &str, result: &crate::batch_ops::BatchRenameResult) -> String {
    let details_summary = result
        .details
        .iter()
        .take(20)
        .map(|s| format!("  {}", s))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "BATCH_SUCCESS [{}]:\n成功: {}/{}\n失败: {}\n\n详情:\n{}\n{}",
        tool_name,
        result.success,
        result.total,
        result.failed,
        details_summary,
        if result.details.len() > 20 {
            format!("\n  ... 及更多 {} 条 ...", result.details.len() - 20)
        } else {
            String::new()
        }
    )
}

fn format_json_tool_result(tool_name: &str, result: &Value) -> String {
    format!(
        "TOOL_SUCCESS [{}]:\n{}",
        tool_name,
        serde_json::to_string_pretty(result).unwrap_or_default()
    )
}

fn is_waql_unsupported_error(error: &str) -> bool {
    let lower = error.to_lowercase();
    lower.contains("argument waql is unknown")
        || (lower.contains("waql") && lower.contains("schema_validation_failed"))
}

fn split_outside_quotes(input: &str, separator: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut in_quotes = false;
    let mut i = 0usize;

    while i < input.len() {
        let slice = &input[i..];
        let Some(ch) = slice.chars().next() else {
            break;
        };
        let ch_len = ch.len_utf8();

        if ch == '"' {
            in_quotes = !in_quotes;
            i += ch_len;
            continue;
        }

        if !in_quotes && input[i..].starts_with(separator) {
            parts.push(input[start..i].trim().to_string());
            i += separator.len();
            start = i;
            continue;
        }

        i += ch_len;
    }

    parts.push(input[start..].trim().to_string());
    parts
}

fn find_outside_quotes(input: &str, needle: &str) -> Option<usize> {
    let mut in_quotes = false;
    let mut i = 0usize;

    while i < input.len() {
        let slice = &input[i..];
        let Some(ch) = slice.chars().next() else {
            break;
        };
        let ch_len = ch.len_utf8();

        if ch == '"' {
            in_quotes = !in_quotes;
            i += ch_len;
            continue;
        }

        if !in_quotes && input[i..].starts_with(needle) {
            return Some(i);
        }

        i += ch_len;
    }

    None
}

fn extract_quoted_literal(input: &str) -> Option<(String, &str)> {
    let trimmed = input.trim_start();
    if !trimmed.starts_with('"') {
        return None;
    }

    let mut escaped = false;
    for (index, ch) in trimmed.char_indices().skip(1) {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            let literal = trimmed[1..index].replace("\\\"", "\"");
            return Some((literal, &trimmed[index + 1..]));
        }
    }

    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LegacyWaqlSelect {
    None,
    Children,
    Descendants,
    Parent,
    Target,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum LegacyWaqlRoot {
    OfType(String),
    ObjectRef(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LegacyFilterOp {
    Contains,
    Equals,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LegacyFilterClause {
    field: String,
    op: LegacyFilterOp,
    value: String,
    negated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LegacyWaqlQuery {
    root: LegacyWaqlRoot,
    select: LegacyWaqlSelect,
    filters: Vec<Vec<LegacyFilterClause>>,
}

fn parse_legacy_filter_clause(input: &str) -> Result<LegacyFilterClause, String> {
    let mut clause = input.trim();
    let mut negated = false;
    if let Some(rest) = clause.strip_prefix("not ") {
        negated = true;
        clause = rest.trim();
    }

    let (field, value, op) = if let Some(index) = clause.find(" : ") {
        (
            clause[..index].trim(),
            clause[index + 3..].trim(),
            LegacyFilterOp::Contains,
        )
    } else if let Some(index) = clause.find(" = ") {
        (
            clause[..index].trim(),
            clause[index + 3..].trim(),
            LegacyFilterOp::Equals,
        )
    } else {
        return Err(format!("Unsupported legacy WAQL filter clause: {}", input));
    };

    let Some((value, rest)) = extract_quoted_literal(value) else {
        return Err(format!(
            "Legacy WAQL filter value must be quoted: {}",
            input
        ));
    };
    if !rest.trim().is_empty() {
        return Err(format!(
            "Unexpected trailing filter text in legacy WAQL clause: {}",
            input
        ));
    }

    Ok(LegacyFilterClause {
        field: field.to_string(),
        op,
        value,
        negated,
    })
}

fn parse_legacy_filters(input: &str) -> Result<Vec<Vec<LegacyFilterClause>>, String> {
    if input.trim().is_empty() {
        return Ok(Vec::new());
    }

    split_outside_quotes(input, " or ")
        .into_iter()
        .map(|group| {
            split_outside_quotes(&group, " and ")
                .into_iter()
                .filter(|part| !part.trim().is_empty())
                .map(|part| parse_legacy_filter_clause(&part))
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()
}

fn parse_legacy_waql_query(waql: &str) -> Result<LegacyWaqlQuery, String> {
    let trimmed = waql.trim();
    let trimmed = trimmed.strip_prefix('$').unwrap_or(trimmed).trim();

    let (root, mut rest) = if let Some(after) = trimmed.strip_prefix("from type ") {
        let type_name = after
            .split_whitespace()
            .next()
            .ok_or_else(|| format!("Missing type in WAQL query: {}", waql))?;
        (
            LegacyWaqlRoot::OfType(type_name.to_string()),
            after[type_name.len()..].trim(),
        )
    } else if let Some((object_ref, after)) = extract_quoted_literal(trimmed) {
        (LegacyWaqlRoot::ObjectRef(object_ref), after.trim())
    } else {
        return Err(format!(
            "This Wwise version does not support native WAQL, and the query could not be translated automatically: {}",
            waql
        ));
    };

    let mut select = LegacyWaqlSelect::None;
    if let Some(after_select) = rest.strip_prefix("select ") {
        let token_end = find_outside_quotes(after_select, " where ").unwrap_or(after_select.len());
        let token = after_select[..token_end].trim();
        select = match token {
            "children" => LegacyWaqlSelect::Children,
            "descendants" => LegacyWaqlSelect::Descendants,
            "parent" => LegacyWaqlSelect::Parent,
            "@Target" | "target" => LegacyWaqlSelect::Target,
            _ => {
                return Err(format!(
                    "Unsupported WAQL select clause for legacy fallback: {}",
                    token
                ))
            }
        };
        rest = after_select[token_end..].trim();
    }

    let filters = if let Some(after_where) = rest.strip_prefix("where ") {
        parse_legacy_filters(after_where)?
    } else if rest.is_empty() {
        Vec::new()
    } else {
        return Err(format!(
            "Unsupported WAQL structure for legacy fallback: {}",
            waql
        ));
    };

    Ok(LegacyWaqlQuery {
        root,
        select,
        filters,
    })
}

fn build_legacy_object_ref_args(object_ref: &str) -> Value {
    if object_ref.contains('\\') || object_ref.contains('/') {
        json!({ "from": { "path": [object_ref] } })
    } else {
        json!({ "from": { "id": [object_ref] } })
    }
}

fn ensure_legacy_fetch_fields(
    return_fields: &[String],
    filters: &[Vec<LegacyFilterClause>],
) -> Vec<String> {
    let mut fields = return_fields.to_vec();
    for required in ["id", "name", "path", "type"] {
        if !fields.iter().any(|field| field == required) {
            fields.push(required.to_string());
        }
    }
    for field in filters.iter().flatten().map(|clause| clause.field.as_str()) {
        if matches!(field, "name" | "path" | "type" | "category")
            && !fields.iter().any(|existing| existing == field)
        {
            fields.push(field.to_string());
        }
    }
    fields
}

fn legacy_filter_value_matches(item: &Value, clause: &LegacyFilterClause) -> bool {
    let actual = item
        .get(&clause.field)
        .and_then(|value| {
            value.as_str().map(str::to_string).or_else(|| {
                value
                    .as_i64()
                    .map(|number| number.to_string())
                    .or_else(|| value.as_u64().map(|number| number.to_string()))
                    .or_else(|| value.as_f64().map(|number| number.to_string()))
                    .or_else(|| value.as_bool().map(|flag| flag.to_string()))
            })
        })
        .unwrap_or_default();

    let matched = match clause.op {
        LegacyFilterOp::Contains => actual.to_lowercase().contains(&clause.value.to_lowercase()),
        LegacyFilterOp::Equals => actual.eq_ignore_ascii_case(&clause.value),
    };

    if clause.negated {
        !matched
    } else {
        matched
    }
}

fn apply_legacy_filters(items: Vec<Value>, filters: &[Vec<LegacyFilterClause>]) -> Vec<Value> {
    if filters.is_empty() {
        return items;
    }

    items
        .into_iter()
        .filter(|item| {
            filters.iter().any(|group| {
                group
                    .iter()
                    .all(|clause| legacy_filter_value_matches(item, clause))
            })
        })
        .collect()
}

fn project_result_fields(item: &Value, return_fields: &[String]) -> Value {
    let Some(object) = item.as_object() else {
        return item.clone();
    };

    let mut projected = serde_json::Map::new();
    for field in return_fields {
        if let Some(value) = object.get(field) {
            projected.insert(field.clone(), value.clone());
        }
    }
    Value::Object(projected)
}

fn collect_target_ids(result: &Value) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut ids = Vec::new();

    for row in result
        .get("return")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
    {
        let target = row.get("@Target");
        let target_iter: Vec<&Value> = match target {
            Some(Value::Array(items)) => items.iter().collect(),
            Some(value) => vec![value],
            None => Vec::new(),
        };

        for value in target_iter {
            if let Some(id) = value
                .get("id")
                .and_then(|v| v.as_str())
                .filter(|id| seen.insert((*id).to_string()))
            {
                ids.push(id.to_string());
            } else if let Some(id) = value.as_str().filter(|id| seen.insert((*id).to_string())) {
                ids.push(id.to_string());
            }
        }
    }

    ids
}

async fn execute_legacy_waql_query(
    waapi: &WaapiClient,
    waql: &str,
    return_fields: &[String],
) -> Result<Value, String> {
    let parsed = parse_legacy_waql_query(waql)?;
    let fetch_fields = ensure_legacy_fetch_fields(return_fields, &parsed.filters);

    let mut call_args = match &parsed.root {
        LegacyWaqlRoot::OfType(object_type) => json!({
            "from": { "ofType": [object_type] }
        }),
        LegacyWaqlRoot::ObjectRef(object_ref) => build_legacy_object_ref_args(object_ref),
    };

    match parsed.select {
        LegacyWaqlSelect::Children => {
            call_args["transform"] = json!([{ "select": ["children"] }]);
        }
        LegacyWaqlSelect::Descendants => {
            call_args["transform"] = json!([{ "select": ["descendants"] }]);
        }
        LegacyWaqlSelect::Parent => {
            call_args["transform"] = json!([{ "select": ["parent"] }]);
        }
        LegacyWaqlSelect::Target => {
            let target_refs = waapi
                .call(
                    "ak.wwise.core.object.get",
                    call_args,
                    Some(json!({ "return": ["@Target"] })),
                )
                .await
                .map_err(|error| {
                    format!(
                        "Legacy WAQL fallback failed while resolving @Target: {}",
                        error
                    )
                })?;

            let target_ids = collect_target_ids(&target_refs);
            if target_ids.is_empty() {
                return Ok(json!({ "return": [] }));
            }

            let result = waapi
                .call(
                    "ak.wwise.core.object.get",
                    json!({ "from": { "id": target_ids } }),
                    Some(json!({ "return": fetch_fields })),
                )
                .await
                .map_err(|error| {
                    format!(
                        "Legacy WAQL fallback failed while loading target objects: {}",
                        error
                    )
                })?;

            let items = result
                .get("return")
                .and_then(|value| value.as_array())
                .cloned()
                .unwrap_or_default();
            let filtered = apply_legacy_filters(items, &parsed.filters)
                .into_iter()
                .map(|item| project_result_fields(&item, return_fields))
                .collect::<Vec<_>>();
            return Ok(json!({ "return": filtered }));
        }
        LegacyWaqlSelect::None => {}
    }

    let result = waapi
        .call(
            "ak.wwise.core.object.get",
            call_args,
            Some(json!({ "return": fetch_fields })),
        )
        .await
        .map_err(|error| format!("Legacy WAQL fallback failed: {}", error))?;

    let items = result
        .get("return")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let filtered = apply_legacy_filters(items, &parsed.filters)
        .into_iter()
        .map(|item| project_result_fields(&item, return_fields))
        .collect::<Vec<_>>();

    Ok(json!({ "return": filtered }))
}

/// 单次工具返回的字符上限。超过就截断——一次"查全部 Sound"在中等工程上
/// 就能吐出五万多字符，不设闸会撑爆上下文窗口，也会白烧钱。
const MAX_TOOL_RESULT_CHARS: usize = 20_000;

/// 在 JSON 里找出最占体积的那个数组字段，截断它就能最有效地缩小结果。
fn largest_array_field(value: &Value) -> Option<(String, usize)> {
    value.as_object()?.iter().fold(None, |best, (key, val)| {
        let len = match val.as_array() {
            Some(arr) if arr.len() > 1 => arr.len(),
            _ => return best,
        };
        match best {
            Some((_, best_len)) if best_len >= len => best,
            _ => Some((key.clone(), len)),
        }
    })
}

/// 把超长的工具返回压到上限以内。
///
/// 关键在于"截断的同时把总量说清楚"：只截不报数，调用方会以为这就是全部，
/// 从而得出错误结论。所以这里一定要带上原始条目数和收窄查询的具体建议。
fn guard_result_size(tool_name: &str, text: String) -> String {
    if text.len() <= MAX_TOOL_RESULT_CHARS {
        return text;
    }

    // 返回格式是 "PREFIX:\n{json}"，先把前缀和 JSON 分开
    if let Some(split_at) = text.find("\n{") {
        let (header, body) = text.split_at(split_at + 1);
        if let Ok(parsed) = serde_json::from_str::<Value>(body) {
            if let Some((field, total)) = largest_array_field(&parsed) {
                let original: Vec<Value> =
                    parsed[&field].as_array().cloned().unwrap_or_default();

                // 逐步减半保留条数，直到整体塞进上限
                let mut keep = total;
                loop {
                    keep /= 2;

                    let mut candidate = parsed.clone();
                    candidate[&field] =
                        Value::Array(original.iter().take(keep).cloned().collect());
                    candidate["_truncated"] = json!(true);
                    candidate["_truncation_note"] = json!(format!(
                        "Result was too large for one response. Field '{}' held {} items; only the \
                         first {} are shown. The TOTAL count is {}: use it for your conclusions, \
                         and do not assume the listed items are everything. To see the rest, narrow \
                         the query: add a 'where' clause, scope it to a specific parent, or request \
                         fewer return fields.",
                        field, total, keep, total
                    ));

                    let rendered = format!(
                        "{}{}",
                        header,
                        serde_json::to_string_pretty(&candidate).unwrap_or_default()
                    );

                    if rendered.len() <= MAX_TOOL_RESULT_CHARS || keep == 0 {
                        debug_log(&format!(
                            "[execute_tool] {} result truncated: {} -> {} chars ({} of {} items)",
                            tool_name,
                            text.len(),
                            rendered.len(),
                            keep,
                            total
                        ));
                        return rendered;
                    }
                }
            }
        }
    }

    // 解析不出结构就硬截，但同样要讲明白发生了什么
    let mut clipped: String = text.chars().take(MAX_TOOL_RESULT_CHARS).collect();
    clipped.push_str(&format!(
        "\n\n[TRUNCATED] Output exceeded {} characters and was cut off. Narrow the request \
         (fewer objects, fewer return fields, or a more specific query) and try again.",
        MAX_TOOL_RESULT_CHARS
    ));
    clipped
}

pub async fn execute_tool(
    waapi: &WaapiClient,
    tool_name: &str,
    arguments: &str,
) -> Result<String, String> {
    execute_tool_inner(waapi, tool_name, arguments)
        .await
        .map(|text| guard_result_size(tool_name, text))
}

async fn execute_tool_inner(
    waapi: &WaapiClient,
    tool_name: &str,
    arguments: &str,
) -> Result<String, String> {
    let args: Value = serde_json::from_str(arguments).map_err(|e| {
        format!(
            "TOOL_ERROR: Invalid JSON arguments: {}. Raw input: {}",
            e, arguments
        )
    })?;

    debug_log(&format!(
        "[execute_tool] name={} args={}",
        tool_name,
        &arguments[..arguments.len().min(300)]
    ));

    // 检查是否为批处理工具
    if tool_name.starts_with("batch_") {
        return execute_batch_tool(waapi, tool_name, &args).await;
    }

    match tool_name {
        "waapi_call" => {
            let raw_uri = args
                .get("uri")
                .and_then(|v| v.as_str())
                .ok_or("TOOL_ERROR: Missing 'uri' field in waapi_call arguments")?;
            let uri = expand_waapi_uri(raw_uri);
            let uri = uri.as_str();
            let mut call_args = extract_waapi_args(&args);
            if let Some(t) = call_args.get("transform") {
                if t.is_object() {
                    let wrapped = Value::Array(vec![t.clone()]);
                    call_args["transform"] = wrapped;
                }
            }
            if matches!(uri, "ak.wwise.core.object.copy" | "ak.wwise.core.object.move") {
                if let Some(obj) = call_args.as_object_mut() {
                    obj.remove("name");
                }
            }
            normalize_waapi_call_args(uri, &mut call_args);
            if uri == "ak.wwise.core.object.set" {
                debug_log("[waapi_call] emulating ak.wwise.core.object.set via object.create calls");
                let result = emulate_object_set(waapi, &call_args).await?;
                let summary = serde_json::to_string_pretty(&result).unwrap_or_default();
                return Ok(format!("WAAPI_SUCCESS [{}]:\n{}", uri, summary));
            }
            let options = if uri_supports_options(uri) {
                args.get("options")
                    .or_else(|| call_args.get("options"))
                    .map(|v| unwrap_stringified_json(v))
            } else {
                None
            };
            if options.is_some() {
                if let Some(obj) = call_args.as_object_mut() {
                    obj.remove("options");
                }
            }

            debug_log(&format!("[waapi_call] uri={} call_args={}", uri, serde_json::to_string(&call_args).unwrap_or_default()));

            match waapi.call(uri, call_args.clone(), options.clone()).await {
                Ok(result) => {
                    let summary = serde_json::to_string_pretty(&result).unwrap_or_default();
                    Ok(format!("WAAPI_SUCCESS [{}]:\n{}", uri, summary))
                }
                Err(e) => {
                    let args_str = serde_json::to_string_pretty(&call_args).unwrap_or_default();
                    Err(format!(
                        "WAAPI_ERROR [{}]: {}\nArguments used:\n{}\nPlease analyze the error, fix the arguments or approach, and retry.",
                        uri, e, args_str
                    ))
                }
            }
        }
        "waapi_query" => {
            let waql = args
                .get("waql")
                .and_then(|v| v.as_str())
                .ok_or("TOOL_ERROR: Missing 'waql' field in waapi_query arguments")?;

            const VALID_RETURN_FIELDS: &[&str] = &[
                "name", "id", "path", "type", "shortId", "classId", "category",
                "filePath", "workunit", "parent", "owner", "isPlayable",
                "childrenCount", "totalSize", "mediaSize", "objectSize",
                "structureSize", "sound:convertedWemFilePath",
                "sound:originalWavFilePath", "music:transitionRoot",
                "music:playlistRoot", "notes",
            ];
            let return_fields = args
                .get("return_fields")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .filter(|f| VALID_RETURN_FIELDS.contains(&f.as_str()) || f.starts_with('@'))
                        .collect::<Vec<_>>()
                })
                .map(|v| if v.is_empty() { vec!["name".into(), "id".into(), "path".into(), "type".into()] } else { v })
                .unwrap_or_else(|| vec!["name".into(), "id".into(), "path".into(), "type".into()]);

            let call_args = json!({ "waql": waql });
            let options = json!({ "return": return_fields });

            match waapi.call("ak.wwise.core.object.get", call_args, Some(options)).await {
                Ok(result) => {
                    let summary = serde_json::to_string_pretty(&result).unwrap_or_default();
                    Ok(format!("WAQL_SUCCESS [{}]:\n{}", waql, summary))
                }
                Err(e) if is_waql_unsupported_error(&e) => {
                    debug_log(&format!(
                        "[waapi_query] native WAQL unsupported, falling back to legacy translation: {}",
                        waql
                    ));
                    let fallback_result = execute_legacy_waql_query(waapi, waql, &return_fields).await
                        .map_err(|fallback_error| {
                            format!(
                                "WAQL_ERROR: Query '{}' failed because this Wwise version does not support native WAQL, and the legacy fallback also failed: {}\nOriginal WAAPI error: {}\nCheck WAQL syntax or try a different query approach.",
                                waql, fallback_error, e
                            )
                        })?;
                    let summary = serde_json::to_string_pretty(&fallback_result).unwrap_or_default();
                    Ok(format!("WAQL_SUCCESS [{}]:\n{}", waql, summary))
                }
                Err(e) => {
                    Err(format!(
                        "WAQL_ERROR: Query '{}' failed: {}\nCheck WAQL syntax or try a different query approach.",
                        waql, e
                    ))
                }
            }
        }
        "list_local_audio_files" => {
            let dir_path = args
                .get("directory")
                .and_then(|v| v.as_str())
                .ok_or("TOOL_ERROR: Missing 'directory' field in list_local_audio_files arguments")?;

            let path = std::path::Path::new(dir_path);
            if !path.exists() || !path.is_dir() {
                return Err(format!("TOOL_ERROR: Directory does not exist or is not a directory: {}", dir_path));
            }

            let mut files = Vec::new();
            match std::fs::read_dir(path) {
                Ok(entries) => {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file() {
                            if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                                let ext_lower = ext.to_lowercase();
                                if ext_lower == "wav" || ext_lower == "aif" || ext_lower == "aiff" || ext_lower == "ogg" {
                                    if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                                        files.push(serde_json::json!({
                                            "name": name,
                                            "path": path.to_string_lossy().to_string()
                                        }));
                                    }
                                }
                            }
                        }
                    }
                    let result = serde_json::json!({
                        "directory": dir_path,
                        "audio_files": files,
                        "count": files.len()
                    });
                    Ok(format!("LOCAL_FILES_SUCCESS:\n{}", serde_json::to_string_pretty(&result).unwrap_or_default()))
                }
                Err(e) => Err(format!("TOOL_ERROR: Failed to read directory '{}': {}", dir_path, e)),
            }
        }
        "get_project_languages" => {
            match crate::batch_ops::get_project_languages(waapi).await {
                Ok(result) => {
                    let summary = serde_json::to_string_pretty(&result).unwrap_or_default();
                    Ok(format!("PROJECT_LANGUAGES_SUCCESS:\n{}", summary))
                }
                Err(e) => Err(format!("TOOL_ERROR: Failed to get project languages: {}", e)),
            }
        }
        "create_path_if_not_exists" => {
            let path = args
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or("TOOL_ERROR: Missing 'path' field")?;
            let create_type = args
                .get("create_type")
                .and_then(|v| v.as_str())
                .unwrap_or("Folder");

            match crate::batch_ops::create_path_if_not_exists(waapi, path, create_type).await {
                Ok(result) => {
                    let summary = serde_json::to_string_pretty(&result).unwrap_or_default();
                    Ok(format!("CREATE_PATH_SUCCESS:\n{}", summary))
                }
                Err(e) => Err(format!("TOOL_ERROR: {}", e)),
            }
        }
        "cleanup_unused_originals_files" => {
            let originals_root = args.get("originals_root").and_then(|v| v.as_str());
            let preview_only = args
                .get("preview_only")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let extensions = args
                .get("extensions")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            match crate::batch_ops::cleanup_unused_originals_files(
                waapi,
                originals_root,
                preview_only,
                &extensions,
            )
            .await
            {
                Ok(result) => Ok(format_json_tool_result(tool_name, &result)),
                Err(e) => Err(format!("TOOL_ERROR: {}", e)),
            }
        }
        // ───────── 动态 WAAPI 自省 ─────────
        "waapi_list_functions" => {
            let filter = args.get("filter").and_then(|v| v.as_str());
            match crate::advanced_ops::waapi_list_functions(waapi, filter).await {
                Ok(result) => Ok(format_json_tool_result(tool_name, &result)),
                Err(e) => Err(e),
            }
        }
        "waapi_list_topics" => {
            let filter = args.get("filter").and_then(|v| v.as_str());
            match crate::advanced_ops::waapi_list_topics(waapi, filter).await {
                Ok(result) => Ok(format_json_tool_result(tool_name, &result)),
                Err(e) => Err(e),
            }
        }
        "waapi_get_schema" => {
            let target = args
                .get("uri")
                .and_then(|v| v.as_str())
                .ok_or("TOOL_ERROR: Missing 'uri' field — pass the WAAPI URI you want the schema for")?;
            let target = expand_waapi_uri(target);
            match crate::advanced_ops::waapi_get_schema(waapi, &target).await {
                Ok(result) => Ok(format_json_tool_result(tool_name, &result)),
                Err(e) => Err(e),
            }
        }

        // ───────── SoundBank ─────────
        "generate_soundbanks" => {
            let soundbanks = crate::advanced_ops::arg_str_vec(&args, "soundbanks");
            let platforms = crate::advanced_ops::arg_str_vec(&args, "platforms");
            let languages = crate::advanced_ops::arg_str_vec(&args, "languages");
            let inclusions = crate::advanced_ops::arg_str_vec(&args, "inclusions_object_ids");
            let filter = crate::advanced_ops::arg_str_vec(&args, "inclusion_filter");
            let rebuild = args
                .get("rebuild")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let write_to_disk = args
                .get("write_to_disk")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);

            match crate::advanced_ops::generate_soundbanks(
                waapi,
                &soundbanks,
                &platforms,
                &languages,
                &inclusions,
                &filter,
                rebuild,
                write_to_disk,
            )
            .await
            {
                Ok(result) => Ok(format_json_tool_result(tool_name, &result)),
                Err(e) => Err(e),
            }
        }
        "get_soundbank_inclusions" => {
            let soundbank = args
                .get("soundbank")
                .and_then(|v| v.as_str())
                .ok_or("TOOL_ERROR: Missing 'soundbank' field (name or GUID)")?;
            match crate::advanced_ops::get_soundbank_inclusions(waapi, soundbank).await {
                Ok(result) => Ok(format_json_tool_result(tool_name, &result)),
                Err(e) => Err(e),
            }
        }

        // ───────── Profiler ─────────
        "profiler_capture" => {
            let duration_ms = args
                .get("duration_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(2000);
            let data_types = crate::advanced_ops::arg_str_vec(&args, "data_types");
            let include_voices = args
                .get("include_voices")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let include_busses = args
                .get("include_busses")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let include_performance = args
                .get("include_performance")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            match crate::advanced_ops::profiler_capture(
                waapi,
                duration_ms,
                &data_types,
                include_voices,
                include_busses,
                include_performance,
            )
            .await
            {
                Ok(result) => Ok(format_json_tool_result(tool_name, &result)),
                Err(e) => Err(e),
            }
        }

        // ───────── 健康检查 / 试听 ─────────
        "wwise_get_info" => match crate::advanced_ops::wwise_get_info(waapi).await {
            Ok(result) => Ok(format_json_tool_result(tool_name, &result)),
            Err(e) => Err(e),
        },
        "transport_play" => {
            let object = args
                .get("object")
                .and_then(|v| v.as_str())
                .ok_or("TOOL_ERROR: Missing 'object' — GUID, path, or type:name of the Event/Sound to audition")?;
            let game_object = args.get("game_object").and_then(|v| v.as_u64());
            match crate::advanced_ops::transport_play(waapi, object, game_object).await {
                Ok(result) => Ok(format_json_tool_result(tool_name, &result)),
                Err(e) => Err(e),
            }
        }
        "transport_control" => {
            let action = args
                .get("action")
                .and_then(|v| v.as_str())
                .ok_or("TOOL_ERROR: Missing 'action' — play, stop, pause, playStop, or playDirectly")?;
            let transport_id = args.get("transport_id").and_then(|v| v.as_u64());
            let destroy = args.get("destroy").and_then(|v| v.as_bool()).unwrap_or(false);
            match crate::advanced_ops::transport_control(waapi, action, transport_id, destroy)
                .await
            {
                Ok(result) => Ok(format_json_tool_result(tool_name, &result)),
                Err(e) => Err(e),
            }
        }
        "transport_list" => match crate::advanced_ops::transport_list(waapi).await {
            Ok(result) => Ok(format_json_tool_result(tool_name, &result)),
            Err(e) => Err(e),
        },

        // ───────── 远程连接 ─────────
        "remote_list_consoles" => match crate::advanced_ops::remote_list_consoles(waapi).await {
            Ok(result) => Ok(format_json_tool_result(tool_name, &result)),
            Err(e) => Err(e),
        },
        "remote_connect" => {
            let host = args
                .get("host")
                .and_then(|v| v.as_str())
                .ok_or("TOOL_ERROR: Missing 'host' field — call remote_list_consoles first to discover hosts")?;
            let app_name = args.get("app_name").and_then(|v| v.as_str());
            let command_port = args.get("command_port").and_then(|v| v.as_u64());
            match crate::advanced_ops::remote_connect(waapi, host, app_name, command_port).await {
                Ok(result) => Ok(format_json_tool_result(tool_name, &result)),
                Err(e) => Err(e),
            }
        }
        "remote_disconnect" => match crate::advanced_ops::remote_disconnect(waapi).await {
            Ok(result) => Ok(format_json_tool_result(tool_name, &result)),
            Err(e) => Err(e),
        },

        // ───────── UI 自动化 ─────────
        "ui_get_selection" => {
            let return_fields = crate::advanced_ops::arg_str_vec(&args, "return_fields");
            match crate::advanced_ops::ui_get_selection(waapi, &return_fields).await {
                Ok(result) => Ok(format_json_tool_result(tool_name, &result)),
                Err(e) => Err(e),
            }
        }
        "ui_list_commands" => {
            let filter = args.get("filter").and_then(|v| v.as_str());
            match crate::advanced_ops::ui_list_commands(waapi, filter).await {
                Ok(result) => Ok(format_json_tool_result(tool_name, &result)),
                Err(e) => Err(e),
            }
        }
        "ui_execute_command" => {
            let command = args
                .get("command")
                .and_then(|v| v.as_str())
                .ok_or("TOOL_ERROR: Missing 'command' field — call ui_list_commands to discover valid command IDs")?;
            let object_ids = crate::advanced_ops::arg_str_vec(&args, "object_ids");
            match crate::advanced_ops::ui_execute_command(waapi, command, &object_ids).await {
                Ok(result) => Ok(format_json_tool_result(tool_name, &result)),
                Err(e) => Err(e),
            }
        }

        // ───────── WAAPI 订阅 ─────────
        "waapi_subscribe" => {
            let topic = args
                .get("topic")
                .and_then(|v| v.as_str())
                .ok_or("TOOL_ERROR: Missing 'topic' field — call waapi_list_topics to see subscribable topics")?;
            let return_fields = crate::advanced_ops::arg_str_vec(&args, "return_fields");
            match crate::advanced_ops::waapi_subscribe(waapi, topic, &return_fields).await {
                Ok(result) => Ok(format_json_tool_result(tool_name, &result)),
                Err(e) => Err(e),
            }
        }
        "waapi_poll_events" => {
            let subscription_id = args.get("subscription_id").and_then(|v| v.as_u64());
            let max_events = args
                .get("max_events")
                .and_then(|v| v.as_u64())
                .unwrap_or(50) as usize;
            let consume = args
                .get("consume")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            match crate::advanced_ops::waapi_poll_events(waapi, subscription_id, max_events, consume)
                .await
            {
                Ok(result) => Ok(format_json_tool_result(tool_name, &result)),
                Err(e) => Err(e),
            }
        }
        "waapi_wait_for_event" => {
            let topic = args.get("topic").and_then(|v| v.as_str());
            let subscription_id = args.get("subscription_id").and_then(|v| v.as_u64());
            let timeout_ms = args
                .get("timeout_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(10_000);
            match crate::advanced_ops::waapi_wait_for_event(
                waapi,
                topic,
                subscription_id,
                timeout_ms,
            )
            .await
            {
                Ok(result) => Ok(format_json_tool_result(tool_name, &result)),
                Err(e) => Err(e),
            }
        }
        "waapi_list_subscriptions" => {
            match crate::advanced_ops::waapi_list_subscriptions(waapi).await {
                Ok(result) => Ok(format_json_tool_result(tool_name, &result)),
                Err(e) => Err(e),
            }
        }
        "waapi_unsubscribe" => {
            let subscription_id = args.get("subscription_id").and_then(|v| v.as_u64());
            let unsubscribe_all = args
                .get("unsubscribe_all")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            match crate::advanced_ops::waapi_unsubscribe(waapi, subscription_id, unsubscribe_all)
                .await
            {
                Ok(result) => Ok(format_json_tool_result(tool_name, &result)),
                Err(e) => Err(e),
            }
        }

        _ => Err(format!("TOOL_ERROR: Unknown tool '{}'. Call waapi_list_functions to discover what this Wwise version supports, or see the tool list exposed by this server.", tool_name)),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_legacy_filters, describe_wwise_action, expand_waapi_uri, guard_result_size,
        is_waql_unsupported_error, largest_array_field, normalize_waapi_call_args,
        parse_legacy_waql_query, uri_supports_options, execute_tool, MAX_TOOL_RESULT_CHARS,
    };
    use crate::waapi::WaapiClient;
    use serde_json::{json, Value};

    #[test]
    fn normalizes_common_action_aliases_to_canonical_fields() {
        let mut args = json!({
            "parent": "{EVENT-GUID}",
            "type": "Action",
            "name": "Play",
            "wg": 1,
            "wh": "{TARGET-GUID}"
        });

        normalize_waapi_call_args("ak.wwise.core.object.create", &mut args);

        assert_eq!(args.get("@ActionType").and_then(|v| v.as_i64()), Some(1));
        assert_eq!(
            args.get("@Target").and_then(|v| v.as_str()),
            Some("{TARGET-GUID}")
        );
        assert!(args.get("wg").is_none());
        assert!(args.get("wh").is_none());
    }

    #[test]
    fn normalizes_actiontype_strings_and_object_targets() {
        let mut args = json!({
            "parent": "{EVENT-GUID}",
            "type": "Action",
            "name": "Break",
            "T_ActionType": "Break",
            "T_Target": {
                "id": "{LOOP-GUID}",
                "name": "Weapon_Rifle_AK_Loop"
            }
        });

        normalize_waapi_call_args("ak.wwise.core.object.create", &mut args);

        assert_eq!(args.get("@ActionType").and_then(|v| v.as_i64()), Some(34));
        assert_eq!(
            args.get("@Target").and_then(|v| v.as_str()),
            Some("{LOOP-GUID}")
        );
        assert!(args.get("T_ActionType").is_none());
        assert!(args.get("T_Target").is_none());
    }

    #[test]
    fn leaves_non_action_object_create_requests_unchanged() {
        let mut args = json!({
            "parent": "\\Events\\Default Work Unit",
            "type": "Event",
            "name": "Play_Test",
            "x": 1,
            "y": "{TARGET-GUID}"
        });
        let original = args.clone();

        normalize_waapi_call_args("ak.wwise.core.object.create", &mut args);

        assert_eq!(args, original);
    }

    #[test]
    fn expands_soundengine_short_uris_to_ak_soundengine_namespace() {
        assert_eq!(
            expand_waapi_uri("soundengine.postEvent"),
            "ak.soundengine.postEvent"
        );
        assert_eq!(
            expand_waapi_uri("ak.soundengine.registerGameObj"),
            "ak.soundengine.registerGameObj"
        );
    }

    #[test]
    fn small_results_pass_through_untouched() {
        let text = "WAQL_SUCCESS [x]:\n{\n  \"return\": [1, 2, 3]\n}".to_string();
        assert_eq!(guard_result_size("waapi_query", text.clone()), text);
    }

    #[test]
    fn oversized_results_are_truncated_below_the_cap() {
        let items: Vec<Value> = (0..4000)
            .map(|i| json!({ "id": format!("{{GUID-{}}}", i), "name": format!("Sound_{}", i) }))
            .collect();
        let text = format!(
            "WAQL_SUCCESS [$ from type Sound]:\n{}",
            serde_json::to_string_pretty(&json!({ "return": items })).unwrap()
        );
        assert!(text.len() > MAX_TOOL_RESULT_CHARS);

        let guarded = guard_result_size("waapi_query", text);
        assert!(
            guarded.len() <= MAX_TOOL_RESULT_CHARS,
            "guarded output is still {} chars",
            guarded.len()
        );
    }

    /// 截断而不报总数会让模型以为看到的就是全部，必须把总量带出来。
    #[test]
    fn truncated_results_report_the_true_total() {
        let items: Vec<Value> = (0..4000)
            .map(|i| json!({ "id": i, "name": format!("Sound_{}", i) }))
            .collect();
        let text = format!(
            "WAQL_SUCCESS [q]:\n{}",
            serde_json::to_string_pretty(&json!({ "return": items })).unwrap()
        );

        let guarded = guard_result_size("waapi_query", text);
        let body = &guarded[guarded.find('{').unwrap()..];
        let parsed: Value = serde_json::from_str(body).expect("guarded output must stay valid JSON");

        assert_eq!(parsed["_truncated"], true);
        let note = parsed["_truncation_note"].as_str().unwrap_or("");
        assert!(note.contains("4000"), "note must state the real total: {}", note);
        assert!(note.contains("narrow") || note.contains("Narrow"));

        let returned = parsed["return"].as_array().unwrap().len();
        assert!(returned > 0 && returned < 4000);
    }

    #[test]
    fn truncation_targets_the_largest_array() {
        let value = json!({
            "small": [1, 2],
            "big": (0..50).collect::<Vec<u32>>(),
            "scalar": "x",
        });
        let (field, total) = largest_array_field(&value).expect("should find an array");
        assert_eq!(field, "big");
        assert_eq!(total, 50);
    }

    #[test]
    fn objects_without_arrays_have_no_truncation_target() {
        let value = json!({ "a": 1, "b": "text", "c": { "d": [1, 2, 3] } });
        assert!(largest_array_field(&value).is_none());
    }

    /// 结构解析不了时也要截断并说明，不能原样放行一个巨型字符串。
    #[test]
    fn unparseable_oversized_output_is_hard_truncated_with_a_note() {
        let text = format!("LOG_DUMP:\n{}", "x".repeat(MAX_TOOL_RESULT_CHARS * 2));
        let guarded = guard_result_size("waapi_call", text);

        assert!(guarded.contains("[TRUNCATED]"));
        assert!(guarded.len() < MAX_TOOL_RESULT_CHARS + 500);
    }

    #[test]
    fn detects_waql_unsupported_schema_errors() {
        let error = r#"ak.wwise.invalid_arguments: {"details":{"argumentName":"waql","typeUri":"ak.wwise.schema_validation_failed"},"message":"Argument waql is unknown."}"#;
        assert!(is_waql_unsupported_error(error));
    }

    #[test]
    fn parses_legacy_type_query_with_negated_path_filter() {
        let query = parse_legacy_waql_query(r#"$ from type Effect where not path : "Factory""#)
            .expect("query should parse");

        match query.root {
            super::LegacyWaqlRoot::OfType(ref object_type) => assert_eq!(object_type, "Effect"),
            _ => panic!("expected type root"),
        }
        assert_eq!(query.select, super::LegacyWaqlSelect::None);
        assert_eq!(query.filters.len(), 1);
        assert_eq!(query.filters[0].len(), 1);
        assert_eq!(query.filters[0][0].field, "path");
        assert!(query.filters[0][0].negated);
    }

    #[test]
    fn parses_legacy_object_query_with_descendants_and_type_filter() {
        let query =
            parse_legacy_waql_query(r#"$ "{GUID}" select descendants where type = "Sound""#)
                .expect("query should parse");

        match query.root {
            super::LegacyWaqlRoot::ObjectRef(ref object_ref) => assert_eq!(object_ref, "{GUID}"),
            _ => panic!("expected object root"),
        }
        assert_eq!(query.select, super::LegacyWaqlSelect::Descendants);
        assert_eq!(query.filters[0][0].field, "type");
    }

    #[test]
    fn applies_legacy_or_filters_case_insensitively() {
        let query =
            parse_legacy_waql_query(r#"$ from type Folder where name : "MP40" or name : "SMG""#)
                .expect("query should parse");
        let items = vec![
            json!({"name": "Weapon_SMG_MP40", "path": "\\Actor-Mixer Hierarchy", "type": "Folder"}),
            json!({"name": "Weapon_AR_AK", "path": "\\Actor-Mixer Hierarchy", "type": "Folder"}),
        ];

        let filtered = apply_legacy_filters(items, &query.filters);

        assert_eq!(filtered.len(), 1);
        assert_eq!(
            filtered[0].get("name").and_then(|v| v.as_str()),
            Some("Weapon_SMG_MP40")
        );
    }

    #[test]
    fn expands_short_object_and_profiler_uris() {
        assert_eq!(
            expand_waapi_uri("object.get"),
            "ak.wwise.core.object.get"
        );
        assert_eq!(
            expand_waapi_uri("profiler.getVoices"),
            "ak.wwise.core.profiler.getVoices"
        );
        assert_eq!(
            expand_waapi_uri("ui.getSelectedObjects"),
            "ak.wwise.ui.getSelectedObjects"
        );
        assert_eq!(
            expand_waapi_uri("soundbank.generate"),
            "ak.wwise.core.soundbank.generate"
        );
        assert_eq!(
            expand_waapi_uri("audio.import"),
            "ak.wwise.core.audio.import"
        );
        assert_eq!(
            expand_waapi_uri("project.saved"),
            "ak.wwise.core.project.saved"
        );
        assert_eq!(expand_waapi_uri("getInfo"), "ak.wwise.core.getInfo");
        assert_eq!(
            expand_waapi_uri("ak.wwise.core.object.get"),
            "ak.wwise.core.object.get"
        );
    }

    #[test]
    fn only_get_and_import_accept_options() {
        assert!(uri_supports_options("ak.wwise.core.object.get"));
        assert!(uri_supports_options("ak.wwise.core.audio.import"));
        assert!(uri_supports_options("ak.wwise.core.object.set"));
        assert!(!uri_supports_options("ak.wwise.core.getInfo"));
        assert!(!uri_supports_options("ak.wwise.core.object.create"));
        assert!(!uri_supports_options("ak.wwise.core.transport.executeAction"));
    }

    #[test]
    fn describe_query_and_info_actions() {
        assert!(describe_wwise_action("waapi_query", r#"{"waql":"$ from type Sound"}"#)
            .contains("Sound"));
        assert!(describe_wwise_action("wwise_get_info", "{}").contains("Wwise"));
        assert!(describe_wwise_action("transport_play", r#"{"object":"Event:Play_X"}"#)
            .contains("Play_X"));
        assert!(describe_wwise_action("transport_control", r#"{"action":"stop"}"#)
            .contains("stop"));
        assert!(describe_wwise_action("profiler_capture", "{}").contains("Profiler"));
        assert!(describe_wwise_action("generate_soundbanks", "{}").contains("SoundBank"));
    }

    #[test]
    fn describe_create_and_rename_waapi_calls() {
        let create = describe_wwise_action(
            "waapi_call",
            r#"{"uri":"ak.wwise.core.object.create","args":{"type":"Sound","name":"Footstep"}}"#,
        );
        assert!(create.contains("Sound"));
        assert!(create.contains("Footstep"));

        let rename = describe_wwise_action(
            "waapi_call",
            r#"{"uri":"ak.wwise.core.object.setName","args":{"value":"NewName"}}"#,
        );
        assert!(rename.contains("NewName"));
    }

    #[test]
    fn describe_falls_back_for_unknown_tool() {
        let desc = describe_wwise_action("not_a_real_tool", "{}");
        assert!(desc.contains("not_a_real_tool"));
    }

    #[tokio::test]
    async fn execute_tool_rejects_invalid_json() {
        let waapi = WaapiClient::new();
        let err = execute_tool(&waapi, "waapi_query", "not-json")
            .await
            .expect_err("invalid json must fail");
        assert!(err.contains("Invalid JSON"));
    }

    #[tokio::test]
    async fn execute_tool_rejects_unknown_tool() {
        let waapi = WaapiClient::new();
        let err = execute_tool(&waapi, "definitely_not_a_tool", "{}")
            .await
            .expect_err("unknown tool must fail");
        assert!(err.contains("Unknown tool"));
    }

    #[tokio::test]
    async fn missing_required_arguments_are_reported_without_wwise() {
        let waapi = WaapiClient::new();
        let cases = [
            ("waapi_call", "{}", "uri"),
            ("waapi_query", "{}", "waql"),
            ("waapi_get_schema", "{}", "uri"),
            ("get_soundbank_inclusions", "{}", "soundbank"),
            ("remote_connect", "{}", "host"),
            ("ui_execute_command", "{}", "command"),
            ("waapi_subscribe", "{}", "topic"),
            ("transport_play", "{}", "object"),
            ("transport_control", "{}", "action"),
            ("batch_replace_audio_by_name", "{}", "target_object_id"),
            ("list_local_audio_files", "{}", "directory"),
            ("create_path_if_not_exists", "{}", "path"),
        ];
        for (tool, args, needle) in cases {
            let err = execute_tool(&waapi, tool, args)
                .await
                .expect_err("missing required argument must fail");
            assert!(
                err.to_lowercase().contains(needle) || err.contains("Missing"),
                "tool {} error should mention '{}', got: {}",
                tool,
                needle,
                err
            );
        }
    }

    #[tokio::test]
    async fn list_local_audio_files_reads_matching_extensions() {
        let dir = std::env::temp_dir().join(format!("gwwise-audio-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Gun_Shot.wav"), b"RIFF").unwrap();
        std::fs::write(dir.join("readme.txt"), b"nope").unwrap();

        let waapi = WaapiClient::new();
        let args = json!({ "directory": dir.to_string_lossy() }).to_string();
        let ok = execute_tool(&waapi, "list_local_audio_files", &args)
            .await
            .expect("listing a real directory must succeed");
        assert!(ok.contains("Gun_Shot"));
        assert!(!ok.contains("readme.txt"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn list_local_audio_files_rejects_missing_directory() {
        let waapi = WaapiClient::new();
        let err = execute_tool(
            &waapi,
            "list_local_audio_files",
            r#"{"directory":"Z:\\this\\path\\does\\not\\exist\\gwwise"}"#,
        )
        .await
        .expect_err("missing dir must fail");
        assert!(err.contains("Failed to read directory") || err.contains("TOOL_ERROR"));
    }
}
