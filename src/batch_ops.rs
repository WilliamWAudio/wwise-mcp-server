// GWwiseAgent Batch Operations Module — © 2025-2026 william.wang
// 高级批处理接口 - 将 WAAPITools 中的成熟方法转换为可直接调用的接口

use crate::waapi::WaapiClient;
use chrono::Local;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

/// 批量重命名操作结果
#[derive(Debug, Clone)]
pub struct BatchRenameResult {
    pub total: usize,
    pub success: usize,
    pub failed: usize,
    pub details: Vec<String>,
}

impl BatchRenameResult {
    pub fn to_json(&self) -> Value {
        json!({
            "total": self.total,
            "success": self.success,
            "failed": self.failed,
            "details": self.details
        })
    }
}

fn matches_recursive_filter(object_type: Option<&str>, filter_type: &str) -> bool {
    filter_type == "*" || filter_type.is_empty() || object_type == Some(filter_type)
}

fn split_object_refs(object_refs: &[String]) -> (Vec<String>, Vec<String>) {
    let mut ids = Vec::new();
    let mut paths = Vec::new();

    for object_ref in object_refs {
        let trimmed = object_ref.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.contains('\\') || trimmed.contains('/') {
            paths.push(trimmed.to_string());
        } else {
            ids.push(trimmed.to_string());
        }
    }

    (ids, paths)
}

fn path_depth(path: &str) -> usize {
    path.chars().filter(|c| *c == '\\' || *c == '/').count()
}

fn order_ids_by_depth(
    object_ids: Vec<String>,
    depth_by_id: &HashMap<String, usize>,
) -> Vec<String> {
    let mut indexed = object_ids.into_iter().enumerate().collect::<Vec<_>>();
    indexed.sort_by(|(left_index, left_id), (right_index, right_id)| {
        let left_depth = depth_by_id.get(left_id).copied().unwrap_or(0);
        let right_depth = depth_by_id.get(right_id).copied().unwrap_or(0);
        right_depth
            .cmp(&left_depth)
            .then_with(|| left_index.cmp(right_index))
    });
    indexed.into_iter().map(|(_, id)| id).collect()
}

async fn order_object_ids_for_delete(waapi: &WaapiClient, object_ids: Vec<String>) -> Vec<String> {
    if object_ids.len() <= 1 {
        return object_ids;
    }

    let query_args = json!({
        "from": { "id": object_ids.clone() }
    });
    let query_options = json!({
        "return": ["id", "path"]
    });

    let response = match waapi
        .call("ak.wwise.core.object.get", query_args, Some(query_options))
        .await
    {
        Ok(response) => response,
        Err(_) => return object_ids,
    };

    let mut depth_by_id = HashMap::new();
    if let Some(objects) = response.get("return").and_then(|r| r.as_array()) {
        for obj in objects {
            let Some(id) = obj.get("id").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(path) = obj.get("path").and_then(|v| v.as_str()) else {
                continue;
            };
            depth_by_id.insert(id.to_string(), path_depth(path));
        }
    }

    order_ids_by_depth(object_ids, &depth_by_id)
}

async fn get_objects_by_refs(
    waapi: &WaapiClient,
    object_refs: &[String],
    return_fields: &[&str],
) -> Result<Vec<Value>, String> {
    let (id_refs, path_refs) = split_object_refs(object_refs);
    let mut objects = Vec::new();
    let mut seen = HashSet::new();

    for (key, refs) in [("id", id_refs), ("path", path_refs)] {
        if refs.is_empty() {
            continue;
        }

        let query_args = match key {
            "id" => json!({
                "from": { "id": refs }
            }),
            "path" => json!({
                "from": { "path": refs }
            }),
            _ => continue,
        };
        let query_options = json!({
            "return": return_fields
        });

        let response = waapi
            .call("ak.wwise.core.object.get", query_args, Some(query_options))
            .await?;

        if let Some(found_objects) = response.get("return").and_then(|r| r.as_array()) {
            for obj in found_objects {
                if let Some(id) = obj.get("id").and_then(|v| v.as_str()) {
                    if seen.insert(id.to_string()) {
                        objects.push(obj.clone());
                    }
                } else {
                    objects.push(obj.clone());
                }
            }
        }
    }

    Ok(objects)
}

fn normalize_filesystem_path(path_str: &str) -> PathBuf {
    let sep = std::path::MAIN_SEPARATOR;
    let normalized: String = path_str
        .chars()
        .map(|c| if c == '/' || c == '\\' { sep } else { c })
        .collect();
    PathBuf::from(normalized)
}

fn path_to_display_string(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\")
}

fn compact_object_info(obj: &Value) -> Value {
    json!({
        "id": obj.get("id").and_then(|v| v.as_str()).unwrap_or_default(),
        "name": obj.get("name").and_then(|v| v.as_str()).unwrap_or_default(),
        "type": obj.get("type").and_then(|v| v.as_str()).unwrap_or_default(),
        "path": obj.get("path").and_then(|v| v.as_str()).unwrap_or_default()
    })
}

fn limit_preview(items: Vec<Value>, limit: usize) -> (Vec<Value>, usize) {
    let total = items.len();
    let preview = items.into_iter().take(limit).collect::<Vec<_>>();
    let hidden_count = total.saturating_sub(preview.len());
    (preview, hidden_count)
}

fn contains_text(haystack: &str, needle: &str, case_sensitive: bool) -> bool {
    if needle.is_empty() {
        return true;
    }

    if case_sensitive {
        haystack.contains(needle)
    } else {
        haystack.to_lowercase().contains(&needle.to_lowercase())
    }
}

fn matches_name_filters(
    name: &str,
    name_contains: Option<&str>,
    exclude_name_contains: &[String],
    case_sensitive: bool,
) -> bool {
    if let Some(include) = name_contains {
        if !contains_text(name, include, case_sensitive) {
            return false;
        }
    }

    !exclude_name_contains
        .iter()
        .filter(|value| !value.trim().is_empty())
        .any(|value| contains_text(name, value, case_sensitive))
}

fn normalize_filename_key(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let path = Path::new(trimmed);
    let stem = path
        .file_stem()
        .or_else(|| path.file_name())
        .and_then(|v| v.to_str())
        .unwrap_or(trimmed);

    stem.trim().to_lowercase()
}

fn normalize_extension_set(extensions: &[String]) -> HashSet<String> {
    extensions
        .iter()
        .map(|ext| ext.trim().trim_start_matches('.').to_lowercase())
        .filter(|ext| !ext.is_empty())
        .collect()
}

async fn get_project_file_path(waapi: &WaapiClient) -> Result<String, String> {
    let args = json!({
        "from": {
            "ofType": ["Project"]
        }
    });
    let options = json!({
        "return": ["filePath"]
    });

    let result = waapi
        .call("ak.wwise.core.object.get", args, Some(options))
        .await
        .map_err(|e| format!("无法获取 Wwise 工程路径: {}", e))?;

    result
        .get("return")
        .and_then(|r| r.as_array())
        .and_then(|arr| arr.first())
        .and_then(|obj| obj.get("filePath"))
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .ok_or_else(|| "无法从 Wwise 工程中解析 filePath".to_string())
}

async fn get_project_dir(waapi: &WaapiClient) -> Result<PathBuf, String> {
    let project_file_path = get_project_file_path(waapi).await?;
    let project_path = normalize_filesystem_path(&project_file_path);
    project_path
        .parent()
        .map(PathBuf::from)
        .ok_or_else(|| format!("无法解析工程目录: {}", project_file_path))
}

async fn query_references_to_object(
    waapi: &WaapiClient,
    object_id: &str,
    reference_types: &[String],
) -> Result<Vec<Value>, String> {
    let mut transform = vec![json!({ "select": ["referencesTo"] })];
    if !reference_types.is_empty() {
        transform.push(json!({
            "where": ["type:isIn", reference_types]
        }));
    }

    let args = json!({
        "from": { "id": [object_id] },
        "transform": transform
    });
    let options = json!({
        "return": ["id", "name", "path", "type"]
    });

    let result = waapi
        .call("ak.wwise.core.object.get", args, Some(options))
        .await?;

    Ok(result
        .get("return")
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default())
}

fn extract_object_id(obj: &Value) -> Option<String> {
    obj.get("id")
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

fn extract_parent_id(obj: &Value) -> Option<String> {
    obj.get("parent").and_then(|parent| {
        parent
            .get("id")
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .or_else(|| parent.as_str().map(str::to_string))
    })
}

fn extract_inclusion_flag(obj: &Value) -> Option<bool> {
    obj.get("Inclusion")
        .or_else(|| obj.get("inclusion"))
        .and_then(|value| value.as_bool())
}

fn extract_string_field<'a>(obj: &'a Value, key: &str) -> Option<&'a str> {
    obj.get(key)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn normalize_source_path_value(value: &str) -> String {
    value.trim().replace('/', "\\").to_lowercase()
}

fn extract_source_path_snapshot(obj: &Value) -> Vec<(String, String)> {
    [
        "originalWavFilePath",
        "sound:originalWavFilePath",
        "sound:convertedWemFilePath",
    ]
    .into_iter()
    .filter_map(|key| {
        extract_string_field(obj, key)
            .map(|value| (key.to_string(), normalize_source_path_value(value)))
    })
    .collect()
}

fn merge_source_path_fields(target: &mut Value, source: &Value) {
    let Some(target_obj) = target.as_object_mut() else {
        return;
    };

    for key in [
        "originalWavFilePath",
        "sound:originalWavFilePath",
        "sound:convertedWemFilePath",
    ] {
        if let Some(value) = source.get(key).cloned() {
            target_obj.insert(key.to_string(), value);
        }
    }
}

async fn enrich_objects_with_source_paths(
    waapi: &WaapiClient,
    objects_by_id: &mut HashMap<String, Value>,
) -> Result<(), String> {
    let mut refs = objects_by_id
        .iter()
        .filter_map(|(object_id, object)| {
            object
                .get("type")
                .and_then(|value| value.as_str())
                .filter(|object_type| matches!(*object_type, "Sound" | "AudioFileSource"))
                .map(|_| object_id.clone())
        })
        .collect::<Vec<_>>();
    refs.sort();

    if refs.is_empty() {
        return Ok(());
    }

    let enriched_objects = get_objects_by_refs(
        waapi,
        &refs,
        &[
            "id",
            "originalWavFilePath",
            "sound:originalWavFilePath",
            "sound:convertedWemFilePath",
        ],
    )
    .await?;

    for enriched in enriched_objects {
        let Some(object_id) = extract_object_id(&enriched) else {
            continue;
        };
        if let Some(existing) = objects_by_id.get_mut(&object_id) {
            merge_source_path_fields(existing, &enriched);
        }
    }

    Ok(())
}

fn derive_audio_source_activity(
    objects_by_id: &HashMap<String, Value>,
    parent_by_id: &HashMap<String, Option<String>>,
) -> HashMap<String, bool> {
    let mut activity_by_id = HashMap::new();

    for (object_id, object) in objects_by_id {
        let Some(object_type) = object.get("type").and_then(|value| value.as_str()) else {
            continue;
        };
        if object_type != "AudioFileSource" {
            continue;
        }

        let Some(parent_id) = parent_by_id.get(object_id).cloned().flatten() else {
            continue;
        };
        let Some(parent) = objects_by_id.get(&parent_id) else {
            continue;
        };

        let child_paths = extract_source_path_snapshot(object);
        let parent_paths = extract_source_path_snapshot(parent);
        if child_paths.is_empty() || parent_paths.is_empty() {
            continue;
        }

        let parent_by_key = parent_paths.into_iter().collect::<HashMap<_, _>>();
        let matches_parent = child_paths.into_iter().any(|(key, child_value)| {
            parent_by_key
                .get(&key)
                .map(|parent_value| parent_value == &child_value)
                .unwrap_or(false)
        });

        activity_by_id.insert(object_id.clone(), matches_parent);
    }

    activity_by_id
}

fn filter_external_references(
    references: Vec<Value>,
    analyzed_ids: &HashSet<String>,
) -> Vec<Value> {
    references
        .into_iter()
        .filter(|reference| {
            extract_object_id(reference)
                .map(|id| !analyzed_ids.contains(&id))
                .unwrap_or(true)
        })
        .collect()
}

fn collect_descendants_from_children(
    object_id: &str,
    children_by_id: &HashMap<String, Vec<String>>,
    protected_ids: &mut HashSet<String>,
) {
    let mut to_process = vec![object_id.to_string()];
    while let Some(current_id) = to_process.pop() {
        if !protected_ids.insert(current_id.clone()) {
            continue;
        }

        if let Some(children) = children_by_id.get(&current_id) {
            to_process.extend(children.iter().cloned());
        }
    }
}

fn collect_protected_ids(
    directly_used_ids: &HashSet<String>,
    parent_by_id: &HashMap<String, Option<String>>,
    children_by_id: &HashMap<String, Vec<String>>,
) -> HashSet<String> {
    let mut protected_ids = HashSet::new();

    for object_id in directly_used_ids {
        collect_descendants_from_children(object_id, children_by_id, &mut protected_ids);

        let mut current_id = Some(object_id.clone());
        while let Some(id) = current_id {
            if !protected_ids.insert(id.clone()) {
                current_id = parent_by_id.get(&id).cloned().flatten();
                continue;
            }
            current_id = parent_by_id.get(&id).cloned().flatten();
        }
    }

    protected_ids
}

fn matches_any_object_type(object_type: Option<&str>, candidate_types: &[String]) -> bool {
    candidate_types.is_empty()
        || candidate_types
            .iter()
            .any(|candidate| candidate.is_empty() || candidate == "*")
        || object_type
            .map(|value| candidate_types.iter().any(|candidate| candidate == value))
            .unwrap_or(false)
}

fn is_top_level_candidate(
    object_id: &str,
    candidate_ids: &HashSet<String>,
    parent_by_id: &HashMap<String, Option<String>>,
) -> bool {
    let mut current_id = parent_by_id.get(object_id).cloned().flatten();
    while let Some(id) = current_id {
        if candidate_ids.contains(&id) {
            return false;
        }
        current_id = parent_by_id.get(&id).cloned().flatten();
    }
    true
}

async fn get_descendants_for_roots(
    waapi: &WaapiClient,
    root_ids: &[String],
) -> Result<(Vec<Value>, usize), String> {
    if root_ids.is_empty() {
        return Ok((Vec::new(), 0));
    }

    let args = json!({
        "from": { "id": root_ids },
        "transform": [{ "select": ["descendants"] }]
    });

    for return_fields in [
        vec!["id", "name", "path", "type", "parent", "Inclusion"],
        vec!["id", "name", "path", "type", "parent", "inclusion"],
        vec!["id", "name", "path", "type", "parent"],
    ] {
        let options = json!({
            "return": return_fields
        });

        let result = match waapi
            .call("ak.wwise.core.object.get", args.clone(), Some(options))
            .await
        {
            Ok(result) => result,
            Err(error) => {
                if return_fields.len() == 5 {
                    return Err(error);
                }
                continue;
            }
        };

        let items = result
            .get("return")
            .and_then(|r| r.as_array())
            .cloned()
            .unwrap_or_default();
        let inclusion_flag_count = items
            .iter()
            .filter(|item| extract_inclusion_flag(item).is_some())
            .count();
        return Ok((items, inclusion_flag_count));
    }

    Ok((Vec::new(), 0))
}

#[derive(Debug, Clone)]
struct SyncNode {
    id: String,
    name: String,
    object_type: String,
    path: String,
    relative_path: String,
    parent_relative_path: Option<String>,
    depth: usize,
}

fn get_value_string<'a>(obj: &'a Value, key: &str) -> Option<&'a str> {
    obj.get(key).and_then(|v| v.as_str())
}

fn get_object_id(obj: &Value) -> Option<&str> {
    get_value_string(obj, "id")
}

fn get_object_type(obj: &Value) -> &str {
    get_value_string(obj, "type").unwrap_or_default()
}

fn split_wwise_path(path: &str) -> Vec<&str> {
    path.split(|c| c == '\\' || c == '/')
        .filter(|part| !part.is_empty())
        .collect()
}

fn normalize_wwise_relative_path(path: &str, case_sensitive: bool) -> String {
    let normalized = split_wwise_path(path).join("\\");
    if case_sensitive {
        normalized
    } else {
        normalized.to_lowercase()
    }
}

fn relative_path_from_base(base_path: &str, object_path: &str) -> Option<String> {
    let base_parts = split_wwise_path(base_path);
    let object_parts = split_wwise_path(object_path);

    if object_parts.len() <= base_parts.len() {
        return None;
    }

    for (left, right) in base_parts.iter().zip(object_parts.iter()) {
        if !left.eq_ignore_ascii_case(right) {
            return None;
        }
    }

    Some(object_parts[base_parts.len()..].join("\\"))
}

fn parent_relative_path(relative_path: &str) -> Option<String> {
    let mut parts = split_wwise_path(relative_path);
    if parts.len() <= 1 {
        return None;
    }
    parts.pop();
    Some(parts.join("\\"))
}

fn sync_path_key(relative_path: &str, case_sensitive: bool) -> String {
    normalize_wwise_relative_path(relative_path, case_sensitive)
}

fn sync_typed_key(relative_path: &str, object_type: &str, case_sensitive: bool) -> String {
    format!(
        "{}|{}",
        sync_path_key(relative_path, case_sensitive),
        if case_sensitive {
            object_type.to_string()
        } else {
            object_type.to_lowercase()
        }
    )
}

fn object_name_matches(obj: &Value, expected: &str, case_sensitive: bool) -> bool {
    let Some(name) = get_value_string(obj, "name") else {
        return false;
    };
    if case_sensitive {
        name == expected
    } else {
        name.eq_ignore_ascii_case(expected)
    }
}

fn object_type_matches(obj: &Value, expected: &str, case_sensitive: bool) -> bool {
    if expected.trim().is_empty() || expected == "*" {
        return true;
    }
    let object_type = get_object_type(obj);
    if case_sensitive {
        object_type == expected
    } else {
        object_type.eq_ignore_ascii_case(expected)
    }
}

fn path_has_anchor_before_leaf(
    path: &str,
    anchor_name: &str,
    case_sensitive: bool,
) -> (bool, bool) {
    let parts = split_wwise_path(path);
    if parts.len() < 2 {
        return (false, false);
    }

    let leaf_index = parts.len() - 1;
    let matches_anchor = |value: &str| {
        if case_sensitive {
            value == anchor_name
        } else {
            value.eq_ignore_ascii_case(anchor_name)
        }
    };

    let direct_parent_matches = matches_anchor(parts[leaf_index - 1]);
    let ancestor_matches = parts[..leaf_index].iter().any(|part| matches_anchor(part));
    (ancestor_matches, direct_parent_matches)
}

fn find_sync_anchor(
    objects: &[Value],
    subtree_name: &str,
    subtree_type: &str,
    anchor_parent_name: &str,
    case_sensitive: bool,
    label: &str,
) -> Result<Value, String> {
    let mut ancestor_matches = Vec::new();
    let mut direct_matches = Vec::new();

    for obj in objects {
        if !object_name_matches(obj, subtree_name, case_sensitive)
            || !object_type_matches(obj, subtree_type, case_sensitive)
        {
            continue;
        }

        let Some(path) = get_value_string(obj, "path") else {
            continue;
        };
        let (has_anchor, direct_parent_matches) =
            path_has_anchor_before_leaf(path, anchor_parent_name, case_sensitive);
        if !has_anchor {
            continue;
        }

        if direct_parent_matches {
            direct_matches.push(obj.clone());
        }
        ancestor_matches.push(obj.clone());
    }

    let selected = if direct_matches.len() == 1 {
        direct_matches.first().cloned()
    } else if direct_matches.is_empty() && ancestor_matches.len() == 1 {
        ancestor_matches.first().cloned()
    } else {
        None
    };

    if let Some(anchor) = selected {
        return Ok(anchor);
    }

    let matches = if direct_matches.is_empty() {
        ancestor_matches
    } else {
        direct_matches
    };
    let preview = matches
        .into_iter()
        .take(10)
        .map(|obj| compact_object_info(&obj))
        .collect::<Vec<_>>();

    if preview.is_empty() {
        Err(format!(
            "Could not find {} subtree '{}' under anchor '{}'",
            label, subtree_name, anchor_parent_name
        ))
    } else {
        Err(format!(
            "Found multiple possible {} subtrees for '{}' under anchor '{}'. Please provide explicit source_root_id/target_root_id. Candidates: {}",
            label,
            subtree_name,
            anchor_parent_name,
            serde_json::to_string_pretty(&preview).unwrap_or_default()
        ))
    }
}

async fn get_objects_with_descendants_for_sync(
    waapi: &WaapiClient,
    object_refs: &[String],
) -> Result<Vec<Value>, String> {
    let mut objects = get_objects_by_refs(
        waapi,
        object_refs,
        &["id", "name", "path", "type", "parent"],
    )
    .await?;

    let root_ids = objects
        .iter()
        .filter_map(|obj| get_object_id(obj).map(String::from))
        .collect::<Vec<_>>();

    if root_ids.is_empty() {
        return Ok(objects);
    }

    let args = json!({
        "from": { "id": root_ids },
        "transform": [{ "select": ["descendants"] }]
    });
    let options = json!({
        "return": ["id", "name", "path", "type", "parent"]
    });

    let response = waapi
        .call("ak.wwise.core.object.get", args, Some(options))
        .await?;

    if let Some(descendants) = response.get("return").and_then(|r| r.as_array()) {
        objects.extend(descendants.iter().cloned());
    }

    Ok(objects)
}

async fn get_descendants_for_sync(
    waapi: &WaapiClient,
    root_id: &str,
) -> Result<Vec<Value>, String> {
    let args = json!({
        "from": { "id": [root_id] },
        "transform": [{ "select": ["descendants"] }]
    });
    let options = json!({
        "return": ["id", "name", "path", "type", "parent"]
    });

    let response = waapi
        .call("ak.wwise.core.object.get", args, Some(options))
        .await?;

    Ok(response
        .get("return")
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default())
}

fn resolve_single_object_ref(objects: Vec<Value>, label: &str) -> Result<Value, String> {
    if objects.len() == 1 {
        return Ok(objects.into_iter().next().unwrap());
    }

    let preview = objects
        .into_iter()
        .take(10)
        .map(|obj| compact_object_info(&obj))
        .collect::<Vec<_>>();

    if preview.is_empty() {
        Err(format!("Could not resolve {}", label))
    } else {
        Err(format!(
            "Resolved multiple objects for {}. Please pass a single GUID or exact path. Candidates: {}",
            label,
            serde_json::to_string_pretty(&preview).unwrap_or_default()
        ))
    }
}

fn build_sync_nodes(objects: &[Value], base_path: &str, case_sensitive: bool) -> Vec<SyncNode> {
    let mut nodes = Vec::new();

    for obj in objects {
        let Some(id) = get_object_id(obj) else {
            continue;
        };
        let Some(name) = get_value_string(obj, "name") else {
            continue;
        };
        let Some(path) = get_value_string(obj, "path") else {
            continue;
        };
        let Some(relative_path) = relative_path_from_base(base_path, path) else {
            continue;
        };
        let depth = split_wwise_path(&relative_path).len();
        if depth == 0 {
            continue;
        }

        let normalized_relative_path = if case_sensitive {
            relative_path.clone()
        } else {
            split_wwise_path(&relative_path).join("\\")
        };

        nodes.push(SyncNode {
            id: id.to_string(),
            name: name.to_string(),
            object_type: get_object_type(obj).to_string(),
            path: path.to_string(),
            parent_relative_path: parent_relative_path(&normalized_relative_path),
            relative_path: normalized_relative_path,
            depth,
        });
    }

    nodes.sort_by(|left, right| {
        left.depth
            .cmp(&right.depth)
            .then_with(|| left.relative_path.cmp(&right.relative_path))
    });
    nodes
}

fn sync_node_preview(node: &SyncNode, target_parent_id: Option<&str>) -> Value {
    json!({
        "id": node.id.clone(),
        "name": node.name.clone(),
        "type": node.object_type.clone(),
        "path": node.path.clone(),
        "relative_path": node.relative_path.clone(),
        "target_parent_id": target_parent_id.unwrap_or_default()
    })
}

pub async fn batch_copy_missing_descendants(
    waapi: &WaapiClient,
    object_refs: Vec<String>,
    source_parent_name: &str,
    target_parent_name: &str,
    subtree_name: &str,
    subtree_type: &str,
    source_root_ref: Option<&str>,
    target_root_ref: Option<&str>,
    match_type: bool,
    case_sensitive: bool,
    preview_only: bool,
    on_name_conflict: &str,
    sync_output_bus: bool,
) -> Result<Value, String> {
    let subtree_name = if subtree_name.trim().is_empty() {
        "GeneralSkills"
    } else {
        subtree_name.trim()
    };
    let source_parent_name = if source_parent_name.trim().is_empty() {
        "Skill_Hit"
    } else {
        source_parent_name.trim()
    };
    let target_parent_name = if target_parent_name.trim().is_empty() {
        "Skill_Release"
    } else {
        target_parent_name.trim()
    };
    let subtree_type = if subtree_type.trim().is_empty() {
        "ActorMixer"
    } else {
        subtree_type.trim()
    };
    let on_name_conflict = if on_name_conflict.trim().is_empty() {
        "fail"
    } else {
        on_name_conflict.trim()
    };

    let (source_root, target_root) = if let (Some(source_ref), Some(target_ref)) =
        (source_root_ref, target_root_ref)
    {
        let source = resolve_single_object_ref(
            get_objects_by_refs(
                waapi,
                &[source_ref.to_string()],
                &["id", "name", "path", "type", "parent"],
            )
            .await?,
            "source_root_id",
        )?;
        let target = resolve_single_object_ref(
            get_objects_by_refs(
                waapi,
                &[target_ref.to_string()],
                &["id", "name", "path", "type", "parent"],
            )
            .await?,
            "target_root_id",
        )?;
        (source, target)
    } else {
        if object_refs.is_empty() {
            return Err(
                "Missing object_ids. Pass the selected root object, or provide source_root_id and target_root_id."
                    .to_string(),
            );
        }
        let scope_objects = get_objects_with_descendants_for_sync(waapi, &object_refs)
            .await
            .map_err(|e| format!("Failed to scan selected scope: {}", e))?;
        let source = find_sync_anchor(
            &scope_objects,
            subtree_name,
            subtree_type,
            source_parent_name,
            case_sensitive,
            "source",
        )?;
        let target = find_sync_anchor(
            &scope_objects,
            subtree_name,
            subtree_type,
            target_parent_name,
            case_sensitive,
            "target",
        )?;
        (source, target)
    };

    let source_root_id = get_object_id(&source_root)
        .ok_or_else(|| "Resolved source root has no id".to_string())?
        .to_string();
    let target_root_id = get_object_id(&target_root)
        .ok_or_else(|| "Resolved target root has no id".to_string())?
        .to_string();
    let source_root_path = get_value_string(&source_root, "path")
        .ok_or_else(|| "Resolved source root has no path".to_string())?
        .to_string();
    let target_root_path = get_value_string(&target_root, "path")
        .ok_or_else(|| "Resolved target root has no path".to_string())?
        .to_string();

    let source_descendants = get_descendants_for_sync(waapi, &source_root_id)
        .await
        .map_err(|e| format!("Failed to query source descendants: {}", e))?;
    let target_descendants = get_descendants_for_sync(waapi, &target_root_id)
        .await
        .map_err(|e| format!("Failed to query target descendants: {}", e))?;

    let source_nodes = build_sync_nodes(&source_descendants, &source_root_path, case_sensitive);
    let target_nodes = build_sync_nodes(&target_descendants, &target_root_path, case_sensitive);

    let mut target_by_path = HashMap::new();
    let mut target_typed_keys = HashSet::new();
    for node in &target_nodes {
        target_by_path.insert(
            sync_path_key(&node.relative_path, case_sensitive),
            node.clone(),
        );
        target_typed_keys.insert(sync_typed_key(
            &node.relative_path,
            &node.object_type,
            case_sensitive,
        ));
    }

    let mut missing_nodes = Vec::new();
    let mut existing_count = 0usize;
    for node in &source_nodes {
        let exists = if match_type {
            target_typed_keys.contains(&sync_typed_key(
                &node.relative_path,
                &node.object_type,
                case_sensitive,
            ))
        } else {
            target_by_path.contains_key(&sync_path_key(&node.relative_path, case_sensitive))
        };

        if exists {
            existing_count += 1;
        } else {
            missing_nodes.push(node.clone());
        }
    }

    let missing_path_keys = missing_nodes
        .iter()
        .map(|node| sync_path_key(&node.relative_path, case_sensitive))
        .collect::<HashSet<_>>();

    let mut copy_candidates: Vec<(SyncNode, String)> = Vec::new();
    let mut covered_by_ancestor = Vec::new();
    let mut missing_parent = Vec::new();

    for node in missing_nodes {
        let mut ancestor_rel = node.parent_relative_path.clone();
        let mut is_covered = false;
        while let Some(rel) = ancestor_rel {
            let key = sync_path_key(&rel, case_sensitive);
            if missing_path_keys.contains(&key) {
                is_covered = true;
                break;
            }
            ancestor_rel = parent_relative_path(&rel);
        }

        if is_covered {
            covered_by_ancestor.push(node);
            continue;
        }

        let target_parent_id = match &node.parent_relative_path {
            Some(parent_rel) => target_by_path
                .get(&sync_path_key(parent_rel, case_sensitive))
                .map(|parent| parent.id.clone()),
            None => Some(target_root_id.clone()),
        };

        if let Some(target_parent_id) = target_parent_id {
            copy_candidates.push((node, target_parent_id));
        } else {
            missing_parent.push(node);
        }
    }

    let copy_candidate_preview = copy_candidates
        .iter()
        .take(50)
        .map(|(node, target_parent_id)| sync_node_preview(node, Some(target_parent_id)))
        .collect::<Vec<_>>();
    let covered_preview = covered_by_ancestor
        .iter()
        .take(20)
        .map(|node| sync_node_preview(node, None))
        .collect::<Vec<_>>();
    let missing_parent_preview = missing_parent
        .iter()
        .take(20)
        .map(|node| sync_node_preview(node, None))
        .collect::<Vec<_>>();

    let mut copied = Vec::new();
    let mut failed = Vec::new();

    if !preview_only && !copy_candidates.is_empty() {
        waapi
            .call("ak.wwise.core.undo.beginGroup", json!({}), None)
            .await
            .map_err(|e| format!("Failed to begin Wwise undo group: {}", e))?;

        for (node, target_parent_id) in &copy_candidates {
            let copy_args = json!({
                "object": node.id,
                "parent": target_parent_id,
                "onNameConflict": on_name_conflict
            });

            match waapi
                .call("ak.wwise.core.object.copy", copy_args, None)
                .await
            {
                Ok(response) => {
                    copied.push(json!({
                        "source": sync_node_preview(node, Some(target_parent_id)),
                        "waapi_result": response
                    }));
                }
                Err(error) => {
                    failed.push(json!({
                        "source": sync_node_preview(node, Some(target_parent_id)),
                        "error": error
                    }));
                }
            }
        }

        waapi
            .call(
                "ak.wwise.core.undo.endGroup",
                json!({
                    "displayName": format!("Copy missing descendants to {}", subtree_name)
                }),
                None,
            )
            .await
            .ok();
    }

    let mut copied_object_map = Vec::new();
    let mut copied_object_ids = Vec::new();
    let mut copied_top_level_object_ids = Vec::new();

    if !preview_only {
        let target_descendants_after = get_descendants_for_sync(waapi, &target_root_id)
            .await
            .map_err(|e| format!("Failed to query target descendants after copy: {}", e))?;
        let target_nodes_after =
            build_sync_nodes(&target_descendants_after, &target_root_path, case_sensitive);
        let mut target_after_by_path = HashMap::new();
        let mut target_after_by_typed_path = HashMap::new();
        for node in target_nodes_after {
            target_after_by_path.insert(
                sync_path_key(&node.relative_path, case_sensitive),
                node.clone(),
            );
            target_after_by_typed_path.insert(
                sync_typed_key(&node.relative_path, &node.object_type, case_sensitive),
                node,
            );
        }

        let mut expected_nodes = copy_candidates
            .iter()
            .map(|(node, _)| node.clone())
            .collect::<Vec<_>>();
        expected_nodes.extend(covered_by_ancestor.iter().cloned());

        let top_level_keys = copy_candidates
            .iter()
            .map(|(node, _)| {
                if match_type {
                    sync_typed_key(&node.relative_path, &node.object_type, case_sensitive)
                } else {
                    sync_path_key(&node.relative_path, case_sensitive)
                }
            })
            .collect::<HashSet<_>>();

        for source_node in expected_nodes {
            let target_node = if match_type {
                target_after_by_typed_path.get(&sync_typed_key(
                    &source_node.relative_path,
                    &source_node.object_type,
                    case_sensitive,
                ))
            } else {
                target_after_by_path.get(&sync_path_key(&source_node.relative_path, case_sensitive))
            };

            if let Some(target_node) = target_node {
                copied_object_ids.push(target_node.id.clone());
                let key = if match_type {
                    sync_typed_key(
                        &source_node.relative_path,
                        &source_node.object_type,
                        case_sensitive,
                    )
                } else {
                    sync_path_key(&source_node.relative_path, case_sensitive)
                };
                if top_level_keys.contains(&key) {
                    copied_top_level_object_ids.push(target_node.id.clone());
                }
                copied_object_map.push(json!({
                    "relative_path": source_node.relative_path,
                    "source_id": source_node.id,
                    "source_name": source_node.name,
                    "source_type": source_node.object_type,
                    "source_path": source_node.path,
                    "target_id": target_node.id,
                    "target_name": target_node.name,
                    "target_type": target_node.object_type,
                    "target_path": target_node.path
                }));
            }
        }
    }

    let output_bus_sync_result =
        if sync_output_bus && !preview_only && !copied_object_ids.is_empty() {
            Some(
                batch_sync_output_bus_by_relative_path(
                    waapi,
                    &source_root_id,
                    &target_root_id,
                    copied_object_ids.clone(),
                    false,
                    case_sensitive,
                    false,
                )
                .await
                .map_err(|e| {
                    format!(
                        "Copied objects were created, but output bus sync failed: {}",
                        e
                    )
                })?,
            )
        } else {
            None
        };

    let copied_preview = copied.iter().take(30).cloned().collect::<Vec<_>>();
    let failed_preview = failed.iter().take(30).cloned().collect::<Vec<_>>();
    let copied_object_map_preview = copied_object_map
        .iter()
        .take(50)
        .cloned()
        .collect::<Vec<_>>();
    let hidden_copy_candidate_count = copy_candidates
        .len()
        .saturating_sub(copy_candidate_preview.len());
    let hidden_covered_by_missing_ancestor_count = covered_by_ancestor
        .len()
        .saturating_sub(covered_preview.len());
    let hidden_missing_parent_count = missing_parent
        .len()
        .saturating_sub(missing_parent_preview.len());
    let hidden_copied_count = copied.len().saturating_sub(copied_preview.len());
    let hidden_failed_count = failed.len().saturating_sub(failed_preview.len());
    let hidden_copied_object_map_count = copied_object_map
        .len()
        .saturating_sub(copied_object_map_preview.len());

    Ok(json!({
        "source_root": compact_object_info(&source_root),
        "target_root": compact_object_info(&target_root),
        "settings": {
            "source_parent_name": source_parent_name,
            "target_parent_name": target_parent_name,
            "subtree_name": subtree_name,
            "subtree_type": subtree_type,
            "match_type": match_type,
            "case_sensitive": case_sensitive,
            "preview_only": preview_only,
            "on_name_conflict": on_name_conflict,
            "sync_output_bus": sync_output_bus
        },
        "scanned_source_descendant_count": source_nodes.len(),
        "scanned_target_descendant_count": target_nodes.len(),
        "existing_count": existing_count,
        "missing_count": copy_candidates.len() + covered_by_ancestor.len() + missing_parent.len(),
        "copy_candidate_count": copy_candidates.len(),
        "copy_candidates_preview": copy_candidate_preview,
        "hidden_copy_candidate_count": hidden_copy_candidate_count,
        "covered_by_missing_ancestor_count": covered_by_ancestor.len(),
        "covered_by_missing_ancestor_preview": covered_preview,
        "hidden_covered_by_missing_ancestor_count": hidden_covered_by_missing_ancestor_count,
        "missing_parent_count": missing_parent.len(),
        "missing_parent_preview": missing_parent_preview,
        "hidden_missing_parent_count": hidden_missing_parent_count,
        "copied_count": copied.len(),
        "copied_preview": copied_preview,
        "hidden_copied_count": hidden_copied_count,
        "copied_top_level_object_ids": copied_top_level_object_ids,
        "copied_object_ids": copied_object_ids,
        "copied_object_map_count": copied_object_map.len(),
        "copied_object_map": copied_object_map,
        "copied_object_map_preview": copied_object_map_preview,
        "hidden_copied_object_map_count": hidden_copied_object_map_count,
        "output_bus_sync_result": output_bus_sync_result,
        "failed_count": failed.len(),
        "failed_preview": failed_preview,
        "hidden_failed_count": hidden_failed_count,
        "executed": !preview_only
    }))
}

pub async fn batch_move_missing_descendants(
    waapi: &WaapiClient,
    object_refs: Vec<String>,
    source_parent_name: &str,
    target_parent_name: &str,
    subtree_name: &str,
    subtree_type: &str,
    source_root_ref: Option<&str>,
    target_root_ref: Option<&str>,
    match_type: bool,
    case_sensitive: bool,
    preview_only: bool,
    on_name_conflict: &str,
    sync_output_bus: bool,
) -> Result<Value, String> {
    let subtree_name = if subtree_name.trim().is_empty() {
        "GeneralSkills"
    } else {
        subtree_name.trim()
    };
    let source_parent_name = if source_parent_name.trim().is_empty() {
        "Skill_Hit"
    } else {
        source_parent_name.trim()
    };
    let target_parent_name = if target_parent_name.trim().is_empty() {
        "Skill_Release"
    } else {
        target_parent_name.trim()
    };
    let subtree_type = if subtree_type.trim().is_empty() {
        "ActorMixer"
    } else {
        subtree_type.trim()
    };
    let on_name_conflict = if on_name_conflict.trim().is_empty() {
        "fail"
    } else {
        on_name_conflict.trim()
    };

    let (source_root, target_root) = if let (Some(source_ref), Some(target_ref)) =
        (source_root_ref, target_root_ref)
    {
        let source = resolve_single_object_ref(
            get_objects_by_refs(
                waapi,
                &[source_ref.to_string()],
                &["id", "name", "path", "type", "parent"],
            )
            .await?,
            "source_root_id",
        )?;
        let target = resolve_single_object_ref(
            get_objects_by_refs(
                waapi,
                &[target_ref.to_string()],
                &["id", "name", "path", "type", "parent"],
            )
            .await?,
            "target_root_id",
        )?;
        (source, target)
    } else {
        if object_refs.is_empty() {
            return Err(
                "Missing object_ids. Pass the selected root object, or provide source_root_id and target_root_id."
                    .to_string(),
            );
        }
        let scope_objects = get_objects_with_descendants_for_sync(waapi, &object_refs)
            .await
            .map_err(|e| format!("Failed to scan selected scope: {}", e))?;
        let source = find_sync_anchor(
            &scope_objects,
            subtree_name,
            subtree_type,
            source_parent_name,
            case_sensitive,
            "source",
        )?;
        let target = find_sync_anchor(
            &scope_objects,
            subtree_name,
            subtree_type,
            target_parent_name,
            case_sensitive,
            "target",
        )?;
        (source, target)
    };

    let source_root_id = get_object_id(&source_root)
        .ok_or_else(|| "Resolved source root has no id".to_string())?
        .to_string();
    let target_root_id = get_object_id(&target_root)
        .ok_or_else(|| "Resolved target root has no id".to_string())?
        .to_string();
    let source_root_path = get_value_string(&source_root, "path")
        .ok_or_else(|| "Resolved source root has no path".to_string())?
        .to_string();
    let target_root_path = get_value_string(&target_root, "path")
        .ok_or_else(|| "Resolved target root has no path".to_string())?
        .to_string();

    let source_descendants = get_descendants_for_sync(waapi, &source_root_id)
        .await
        .map_err(|e| format!("Failed to query source descendants: {}", e))?;
    let target_descendants = get_descendants_for_sync(waapi, &target_root_id)
        .await
        .map_err(|e| format!("Failed to query target descendants: {}", e))?;

    let source_nodes = build_sync_nodes(&source_descendants, &source_root_path, case_sensitive);
    let target_nodes = build_sync_nodes(&target_descendants, &target_root_path, case_sensitive);

    let mut target_by_path = HashMap::new();
    let mut target_typed_keys = HashSet::new();
    for node in &target_nodes {
        target_by_path.insert(
            sync_path_key(&node.relative_path, case_sensitive),
            node.clone(),
        );
        target_typed_keys.insert(sync_typed_key(
            &node.relative_path,
            &node.object_type,
            case_sensitive,
        ));
    }

    let mut missing_nodes = Vec::new();
    let mut existing_count = 0usize;
    for node in &source_nodes {
        let exists = if match_type {
            target_typed_keys.contains(&sync_typed_key(
                &node.relative_path,
                &node.object_type,
                case_sensitive,
            ))
        } else {
            target_by_path.contains_key(&sync_path_key(&node.relative_path, case_sensitive))
        };

        if exists {
            existing_count += 1;
        } else {
            missing_nodes.push(node.clone());
        }
    }

    let missing_path_keys = missing_nodes
        .iter()
        .map(|node| sync_path_key(&node.relative_path, case_sensitive))
        .collect::<HashSet<_>>();

    let mut move_candidates: Vec<(SyncNode, String)> = Vec::new();
    let mut covered_by_ancestor = Vec::new();
    let mut missing_parent = Vec::new();

    for node in missing_nodes {
        let mut ancestor_rel = node.parent_relative_path.clone();
        let mut is_covered = false;
        while let Some(rel) = ancestor_rel {
            let key = sync_path_key(&rel, case_sensitive);
            if missing_path_keys.contains(&key) {
                is_covered = true;
                break;
            }
            ancestor_rel = parent_relative_path(&rel);
        }

        if is_covered {
            covered_by_ancestor.push(node);
            continue;
        }

        let target_parent_id = match &node.parent_relative_path {
            Some(parent_rel) => target_by_path
                .get(&sync_path_key(parent_rel, case_sensitive))
                .map(|parent| parent.id.clone()),
            None => Some(target_root_id.clone()),
        };

        if let Some(target_parent_id) = target_parent_id {
            move_candidates.push((node, target_parent_id));
        } else {
            missing_parent.push(node);
        }
    }

    let move_candidate_preview = move_candidates
        .iter()
        .take(50)
        .map(|(node, target_parent_id)| sync_node_preview(node, Some(target_parent_id)))
        .collect::<Vec<_>>();
    let covered_preview = covered_by_ancestor
        .iter()
        .take(20)
        .map(|node| sync_node_preview(node, None))
        .collect::<Vec<_>>();
    let missing_parent_preview = missing_parent
        .iter()
        .take(20)
        .map(|node| sync_node_preview(node, None))
        .collect::<Vec<_>>();

    let source_bus_objects_by_id: HashMap<String, Value> =
        if sync_output_bus && !preview_only && !move_candidates.is_empty() {
            get_descendants_for_bus_sync(waapi, &source_root_id)
                .await
                .map_err(|e| format!("Failed to capture source OutputBus before move: {}", e))?
                .into_iter()
                .filter_map(|obj| get_object_id(&obj).map(|id| (id.to_string(), obj.clone())))
                .collect()
        } else {
            HashMap::new()
        };

    let mut moved = Vec::new();
    let mut failed = Vec::new();

    if !preview_only && !move_candidates.is_empty() {
        waapi
            .call("ak.wwise.core.undo.beginGroup", json!({}), None)
            .await
            .map_err(|e| format!("Failed to begin Wwise undo group: {}", e))?;

        for (node, target_parent_id) in &move_candidates {
            let move_args = json!({
                "object": node.id,
                "parent": target_parent_id,
                "onNameConflict": on_name_conflict
            });

            match waapi
                .call("ak.wwise.core.object.move", move_args, None)
                .await
            {
                Ok(response) => {
                    moved.push(json!({
                        "source": sync_node_preview(node, Some(target_parent_id)),
                        "waapi_result": response
                    }));
                }
                Err(error) => {
                    failed.push(json!({
                        "source": sync_node_preview(node, Some(target_parent_id)),
                        "error": error
                    }));
                }
            }
        }

        waapi
            .call(
                "ak.wwise.core.undo.endGroup",
                json!({
                    "displayName": format!("Move missing descendants to {}", subtree_name)
                }),
                None,
            )
            .await
            .ok();
    }

    let mut moved_object_map = Vec::new();
    let mut moved_object_ids = Vec::new();
    let mut moved_top_level_object_ids = Vec::new();
    let mut moved_pairs = Vec::new();

    if !preview_only {
        let target_descendants_after = get_descendants_for_sync(waapi, &target_root_id)
            .await
            .map_err(|e| format!("Failed to query target descendants after move: {}", e))?;
        let target_nodes_after =
            build_sync_nodes(&target_descendants_after, &target_root_path, case_sensitive);
        let mut target_after_by_path = HashMap::new();
        let mut target_after_by_typed_path = HashMap::new();
        for node in target_nodes_after {
            target_after_by_path.insert(
                sync_path_key(&node.relative_path, case_sensitive),
                node.clone(),
            );
            target_after_by_typed_path.insert(
                sync_typed_key(&node.relative_path, &node.object_type, case_sensitive),
                node,
            );
        }

        let mut expected_nodes = move_candidates
            .iter()
            .map(|(node, _)| node.clone())
            .collect::<Vec<_>>();
        expected_nodes.extend(covered_by_ancestor.iter().cloned());

        let top_level_keys = move_candidates
            .iter()
            .map(|(node, _)| {
                if match_type {
                    sync_typed_key(&node.relative_path, &node.object_type, case_sensitive)
                } else {
                    sync_path_key(&node.relative_path, case_sensitive)
                }
            })
            .collect::<HashSet<_>>();

        for source_node in expected_nodes {
            let target_node = if match_type {
                target_after_by_typed_path.get(&sync_typed_key(
                    &source_node.relative_path,
                    &source_node.object_type,
                    case_sensitive,
                ))
            } else {
                target_after_by_path.get(&sync_path_key(&source_node.relative_path, case_sensitive))
            };

            if let Some(target_node) = target_node {
                moved_object_ids.push(target_node.id.clone());
                let key = if match_type {
                    sync_typed_key(
                        &source_node.relative_path,
                        &source_node.object_type,
                        case_sensitive,
                    )
                } else {
                    sync_path_key(&source_node.relative_path, case_sensitive)
                };
                if top_level_keys.contains(&key) {
                    moved_top_level_object_ids.push(target_node.id.clone());
                }
                moved_pairs.push((source_node.clone(), target_node.clone()));
                moved_object_map.push(json!({
                    "relative_path": source_node.relative_path,
                    "source_id": source_node.id,
                    "source_name": source_node.name,
                    "source_type": source_node.object_type,
                    "source_path_before_move": source_node.path,
                    "target_id": target_node.id,
                    "target_name": target_node.name,
                    "target_type": target_node.object_type,
                    "target_path": target_node.path
                }));
            }
        }
    }

    let output_bus_sync_result = if sync_output_bus && !preview_only && !moved_pairs.is_empty() {
        Some(
            sync_moved_output_bus_from_snapshot(
                waapi,
                &source_root,
                &target_root,
                &target_root_id,
                &source_bus_objects_by_id,
                &moved_pairs,
                case_sensitive,
                false,
            )
            .await
            .map_err(|e| {
                format!(
                    "Moved objects were placed, but output bus sync failed: {}",
                    e
                )
            })?,
        )
    } else {
        None
    };

    let moved_preview = moved.iter().take(30).cloned().collect::<Vec<_>>();
    let failed_preview = failed.iter().take(30).cloned().collect::<Vec<_>>();
    let moved_object_map_preview = moved_object_map
        .iter()
        .take(50)
        .cloned()
        .collect::<Vec<_>>();
    let hidden_move_candidate_count = move_candidates
        .len()
        .saturating_sub(move_candidate_preview.len());
    let hidden_covered_by_missing_ancestor_count = covered_by_ancestor
        .len()
        .saturating_sub(covered_preview.len());
    let hidden_missing_parent_count = missing_parent
        .len()
        .saturating_sub(missing_parent_preview.len());
    let hidden_moved_count = moved.len().saturating_sub(moved_preview.len());
    let hidden_failed_count = failed.len().saturating_sub(failed_preview.len());
    let hidden_moved_object_map_count = moved_object_map
        .len()
        .saturating_sub(moved_object_map_preview.len());

    Ok(json!({
        "source_root": compact_object_info(&source_root),
        "target_root": compact_object_info(&target_root),
        "settings": {
            "source_parent_name": source_parent_name,
            "target_parent_name": target_parent_name,
            "subtree_name": subtree_name,
            "subtree_type": subtree_type,
            "match_type": match_type,
            "case_sensitive": case_sensitive,
            "preview_only": preview_only,
            "on_name_conflict": on_name_conflict,
            "sync_output_bus": sync_output_bus
        },
        "scanned_source_descendant_count": source_nodes.len(),
        "scanned_target_descendant_count": target_nodes.len(),
        "existing_count": existing_count,
        "missing_count": move_candidates.len() + covered_by_ancestor.len() + missing_parent.len(),
        "move_candidate_count": move_candidates.len(),
        "move_candidates_preview": move_candidate_preview,
        "hidden_move_candidate_count": hidden_move_candidate_count,
        "covered_by_missing_ancestor_count": covered_by_ancestor.len(),
        "covered_by_missing_ancestor_preview": covered_preview,
        "hidden_covered_by_missing_ancestor_count": hidden_covered_by_missing_ancestor_count,
        "missing_parent_count": missing_parent.len(),
        "missing_parent_preview": missing_parent_preview,
        "hidden_missing_parent_count": hidden_missing_parent_count,
        "moved_count": moved.len(),
        "moved_preview": moved_preview,
        "hidden_moved_count": hidden_moved_count,
        "moved_top_level_object_ids": moved_top_level_object_ids,
        "moved_object_ids": moved_object_ids,
        "moved_object_map_count": moved_object_map.len(),
        "moved_object_map": moved_object_map,
        "moved_object_map_preview": moved_object_map_preview,
        "hidden_moved_object_map_count": hidden_moved_object_map_count,
        "output_bus_sync_result": output_bus_sync_result,
        "failed_count": failed.len(),
        "failed_preview": failed_preview,
        "hidden_failed_count": hidden_failed_count,
        "executed": !preview_only
    }))
}

fn extract_reference_id(obj: &Value, field: &str) -> Option<String> {
    obj.get(field)
        .and_then(|value| {
            value
                .get("id")
                .and_then(|id| id.as_str())
                .or_else(|| value.as_str())
        })
        .map(String::from)
}

fn extract_reference_name(obj: &Value, field: &str) -> Option<String> {
    obj.get(field)
        .and_then(|value| {
            value
                .get("name")
                .and_then(|name| name.as_str())
                .or_else(|| value.as_str())
        })
        .map(String::from)
}

async fn get_descendants_for_bus_sync(
    waapi: &WaapiClient,
    root_id: &str,
) -> Result<Vec<Value>, String> {
    let args = json!({
        "from": { "id": [root_id] },
        "transform": [{ "select": ["descendants"] }]
    });
    let options = json!({
        "return": [
            "id",
            "name",
            "path",
            "type",
            "parent",
            "OutputBus",
            "OverrideOutput"
        ]
    });

    let response = waapi
        .call("ak.wwise.core.object.get", args, Some(options))
        .await?;

    Ok(response
        .get("return")
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default())
}

fn bus_sync_preview(
    target_node: &SyncNode,
    source_node: &SyncNode,
    source_bus_id: &str,
    source_bus_name: Option<&str>,
    target_bus_id: Option<&str>,
    target_bus_name: Option<&str>,
) -> Value {
    json!({
        "relative_path": target_node.relative_path.clone(),
        "target_id": target_node.id.clone(),
        "target_name": target_node.name.clone(),
        "target_type": target_node.object_type.clone(),
        "target_path": target_node.path.clone(),
        "source_id": source_node.id.clone(),
        "source_name": source_node.name.clone(),
        "source_type": source_node.object_type.clone(),
        "source_path": source_node.path.clone(),
        "source_output_bus": {
            "id": source_bus_id,
            "name": source_bus_name.unwrap_or_default()
        },
        "target_output_bus_before": {
            "id": target_bus_id.unwrap_or_default(),
            "name": target_bus_name.unwrap_or_default()
        }
    })
}

async fn sync_moved_output_bus_from_snapshot(
    waapi: &WaapiClient,
    source_root: &Value,
    target_root: &Value,
    target_root_id: &str,
    source_bus_objects_by_id: &HashMap<String, Value>,
    moved_pairs: &[(SyncNode, SyncNode)],
    case_sensitive: bool,
    preview_only: bool,
) -> Result<Value, String> {
    let target_bus_objects = get_descendants_for_bus_sync(waapi, target_root_id)
        .await
        .map_err(|e| {
            format!(
                "Failed to query moved target descendants for OutputBus: {}",
                e
            )
        })?;
    let target_bus_objects_by_id = target_bus_objects
        .into_iter()
        .filter_map(|obj| get_object_id(&obj).map(|id| (id.to_string(), obj.clone())))
        .collect::<HashMap<_, _>>();

    let mut candidates = Vec::new();
    let mut unchanged = Vec::new();
    let mut skipped_no_source = Vec::new();
    let mut skipped_no_source_bus = Vec::new();
    let mut skipped_unsupported_target = Vec::new();

    for (source_node, target_node) in moved_pairs {
        let Some(source_obj) = source_bus_objects_by_id.get(&source_node.id) else {
            skipped_no_source.push(sync_node_preview(source_node, None));
            continue;
        };
        let Some(target_obj) = target_bus_objects_by_id.get(&target_node.id) else {
            skipped_unsupported_target.push(sync_node_preview(target_node, None));
            continue;
        };

        if target_obj.get("OutputBus").is_none() {
            skipped_unsupported_target.push(sync_node_preview(target_node, None));
            continue;
        }

        let Some(source_bus_id) = extract_reference_id(source_obj, "OutputBus") else {
            skipped_no_source_bus.push(sync_node_preview(source_node, None));
            continue;
        };
        let source_bus_name = extract_reference_name(source_obj, "OutputBus");
        let target_bus_id = extract_reference_id(target_obj, "OutputBus");
        let target_bus_name = extract_reference_name(target_obj, "OutputBus");

        if target_bus_id.as_deref() == Some(source_bus_id.as_str()) {
            unchanged.push(bus_sync_preview(
                target_node,
                source_node,
                &source_bus_id,
                source_bus_name.as_deref(),
                target_bus_id.as_deref(),
                target_bus_name.as_deref(),
            ));
            continue;
        }

        candidates.push((
            target_node.clone(),
            source_node.clone(),
            source_bus_id,
            source_bus_name,
            target_bus_id,
            target_bus_name,
        ));
    }

    let candidate_preview = candidates
        .iter()
        .take(50)
        .map(
            |(
                target_node,
                source_node,
                source_bus_id,
                source_bus_name,
                target_bus_id,
                target_bus_name,
            )| {
                bus_sync_preview(
                    target_node,
                    source_node,
                    source_bus_id,
                    source_bus_name.as_deref(),
                    target_bus_id.as_deref(),
                    target_bus_name.as_deref(),
                )
            },
        )
        .collect::<Vec<_>>();

    let mut synced = Vec::new();
    let mut failed = Vec::new();

    if !preview_only && !candidates.is_empty() {
        waapi
            .call("ak.wwise.core.undo.beginGroup", json!({}), None)
            .await
            .map_err(|e| format!("Failed to begin Wwise undo group: {}", e))?;

        for (
            target_node,
            source_node,
            source_bus_id,
            source_bus_name,
            target_bus_id,
            target_bus_name,
        ) in &candidates
        {
            let set_override = waapi
                .call(
                    "ak.wwise.core.object.setProperty",
                    json!({
                        "object": target_node.id,
                        "property": "OverrideOutput",
                        "value": true
                    }),
                    None,
                )
                .await;

            if let Err(error) = set_override {
                failed.push(json!({
                    "target_id": target_node.id,
                    "relative_path": target_node.relative_path,
                    "step": "set OverrideOutput",
                    "error": error
                }));
                continue;
            }

            let set_reference = waapi
                .call(
                    "ak.wwise.core.object.setReference",
                    json!({
                        "object": target_node.id,
                        "reference": "OutputBus",
                        "value": source_bus_id
                    }),
                    None,
                )
                .await;

            match set_reference {
                Ok(_) => synced.push(bus_sync_preview(
                    target_node,
                    source_node,
                    source_bus_id,
                    source_bus_name.as_deref(),
                    target_bus_id.as_deref(),
                    target_bus_name.as_deref(),
                )),
                Err(error) => failed.push(json!({
                    "target_id": target_node.id,
                    "relative_path": target_node.relative_path,
                    "step": "set OutputBus reference",
                    "error": error
                })),
            }
        }

        waapi
            .call(
                "ak.wwise.core.undo.endGroup",
                json!({
                    "displayName": "Sync OutputBus after moving missing descendants"
                }),
                None,
            )
            .await
            .ok();
    }

    let unchanged_preview = unchanged.iter().take(30).cloned().collect::<Vec<_>>();
    let skipped_no_source_preview = skipped_no_source
        .iter()
        .take(30)
        .cloned()
        .collect::<Vec<_>>();
    let skipped_no_source_bus_preview = skipped_no_source_bus
        .iter()
        .take(30)
        .cloned()
        .collect::<Vec<_>>();
    let skipped_unsupported_target_preview = skipped_unsupported_target
        .iter()
        .take(30)
        .cloned()
        .collect::<Vec<_>>();
    let synced_preview = synced.iter().take(30).cloned().collect::<Vec<_>>();
    let failed_preview = failed.iter().take(30).cloned().collect::<Vec<_>>();

    Ok(json!({
        "source_root": compact_object_info(source_root),
        "target_root": compact_object_info(target_root),
        "settings": {
            "case_sensitive": case_sensitive,
            "preview_only": preview_only,
            "target_filter_count": moved_pairs.len(),
            "source_bus_snapshot": true
        },
        "candidate_count": candidates.len(),
        "candidate_preview": candidate_preview,
        "hidden_candidate_count": candidates.len().saturating_sub(candidate_preview.len()),
        "synced_count": synced.len(),
        "synced_preview": synced_preview,
        "hidden_synced_count": synced.len().saturating_sub(synced_preview.len()),
        "failed_count": failed.len(),
        "failed_preview": failed_preview,
        "hidden_failed_count": failed.len().saturating_sub(failed_preview.len()),
        "unchanged_count": unchanged.len(),
        "unchanged_preview": unchanged_preview,
        "hidden_unchanged_count": unchanged.len().saturating_sub(unchanged_preview.len()),
        "skipped_no_source_count": skipped_no_source.len(),
        "skipped_no_source_preview": skipped_no_source_preview,
        "hidden_skipped_no_source_count": skipped_no_source.len().saturating_sub(skipped_no_source_preview.len()),
        "skipped_no_source_bus_count": skipped_no_source_bus.len(),
        "skipped_no_source_bus_preview": skipped_no_source_bus_preview,
        "hidden_skipped_no_source_bus_count": skipped_no_source_bus.len().saturating_sub(skipped_no_source_bus_preview.len()),
        "skipped_unsupported_target_count": skipped_unsupported_target.len(),
        "skipped_unsupported_target_preview": skipped_unsupported_target_preview,
        "hidden_skipped_unsupported_target_count": skipped_unsupported_target.len().saturating_sub(skipped_unsupported_target_preview.len()),
        "executed": !preview_only
    }))
}

pub async fn batch_sync_output_bus_by_relative_path(
    waapi: &WaapiClient,
    source_root_ref: &str,
    target_root_ref: &str,
    target_object_refs: Vec<String>,
    include_target_descendants: bool,
    case_sensitive: bool,
    preview_only: bool,
) -> Result<Value, String> {
    let source_root = resolve_single_object_ref(
        get_objects_by_refs(
            waapi,
            &[source_root_ref.to_string()],
            &["id", "name", "path", "type", "parent"],
        )
        .await?,
        "source_root_id",
    )?;
    let target_root = resolve_single_object_ref(
        get_objects_by_refs(
            waapi,
            &[target_root_ref.to_string()],
            &["id", "name", "path", "type", "parent"],
        )
        .await?,
        "target_root_id",
    )?;

    let source_root_id = get_object_id(&source_root)
        .ok_or_else(|| "Resolved source root has no id".to_string())?
        .to_string();
    let target_root_id = get_object_id(&target_root)
        .ok_or_else(|| "Resolved target root has no id".to_string())?
        .to_string();
    let source_root_path = get_value_string(&source_root, "path")
        .ok_or_else(|| "Resolved source root has no path".to_string())?
        .to_string();
    let target_root_path = get_value_string(&target_root, "path")
        .ok_or_else(|| "Resolved target root has no path".to_string())?
        .to_string();

    let source_objects = get_descendants_for_bus_sync(waapi, &source_root_id)
        .await
        .map_err(|e| format!("Failed to query source descendants for OutputBus: {}", e))?;
    let target_objects = get_descendants_for_bus_sync(waapi, &target_root_id)
        .await
        .map_err(|e| format!("Failed to query target descendants for OutputBus: {}", e))?;

    let source_nodes = build_sync_nodes(&source_objects, &source_root_path, case_sensitive);
    let target_nodes = build_sync_nodes(&target_objects, &target_root_path, case_sensitive);

    let source_objects_by_id = source_objects
        .into_iter()
        .filter_map(|obj| {
            let id = get_object_id(&obj)?.to_string();
            Some((id, obj))
        })
        .collect::<HashMap<_, _>>();
    let target_objects_by_id = target_objects
        .into_iter()
        .filter_map(|obj| {
            let id = get_object_id(&obj)?.to_string();
            Some((id, obj))
        })
        .collect::<HashMap<_, _>>();

    let mut source_by_path = HashMap::new();
    for node in &source_nodes {
        source_by_path.insert(
            sync_path_key(&node.relative_path, case_sensitive),
            node.clone(),
        );
    }

    let target_filter_ids = if target_object_refs.is_empty() {
        HashSet::new()
    } else if include_target_descendants {
        get_recursive_objects(waapi, target_object_refs, "*")
            .await?
            .into_iter()
            .collect::<HashSet<_>>()
    } else {
        get_objects_by_refs(waapi, &target_object_refs, &["id"])
            .await?
            .into_iter()
            .filter_map(|obj| get_object_id(&obj).map(String::from))
            .collect::<HashSet<_>>()
    };

    let mut candidates: Vec<(
        SyncNode,
        SyncNode,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = Vec::new();
    let mut unchanged = Vec::new();
    let mut skipped_no_source = Vec::new();
    let mut skipped_no_source_bus = Vec::new();
    let mut skipped_unsupported_target = Vec::new();

    for target_node in &target_nodes {
        if !target_filter_ids.is_empty() && !target_filter_ids.contains(&target_node.id) {
            continue;
        }

        let Some(source_node) =
            source_by_path.get(&sync_path_key(&target_node.relative_path, case_sensitive))
        else {
            skipped_no_source.push(sync_node_preview(target_node, None));
            continue;
        };

        let Some(source_obj) = source_objects_by_id.get(&source_node.id) else {
            skipped_no_source.push(sync_node_preview(target_node, None));
            continue;
        };
        let Some(target_obj) = target_objects_by_id.get(&target_node.id) else {
            skipped_unsupported_target.push(sync_node_preview(target_node, None));
            continue;
        };

        if target_obj.get("OutputBus").is_none() {
            skipped_unsupported_target.push(sync_node_preview(target_node, None));
            continue;
        }

        let Some(source_bus_id) = extract_reference_id(source_obj, "OutputBus") else {
            skipped_no_source_bus.push(sync_node_preview(source_node, None));
            continue;
        };
        let source_bus_name = extract_reference_name(source_obj, "OutputBus");
        let target_bus_id = extract_reference_id(target_obj, "OutputBus");
        let target_bus_name = extract_reference_name(target_obj, "OutputBus");

        if target_bus_id.as_deref() == Some(source_bus_id.as_str()) {
            unchanged.push(bus_sync_preview(
                target_node,
                source_node,
                &source_bus_id,
                source_bus_name.as_deref(),
                target_bus_id.as_deref(),
                target_bus_name.as_deref(),
            ));
            continue;
        }

        candidates.push((
            target_node.clone(),
            source_node.clone(),
            source_bus_id,
            source_bus_name,
            target_bus_id,
            target_bus_name,
        ));
    }

    let candidate_preview = candidates
        .iter()
        .take(50)
        .map(
            |(
                target_node,
                source_node,
                source_bus_id,
                source_bus_name,
                target_bus_id,
                target_bus_name,
            )| {
                bus_sync_preview(
                    target_node,
                    source_node,
                    source_bus_id,
                    source_bus_name.as_deref(),
                    target_bus_id.as_deref(),
                    target_bus_name.as_deref(),
                )
            },
        )
        .collect::<Vec<_>>();

    let mut synced = Vec::new();
    let mut failed = Vec::new();

    if !preview_only && !candidates.is_empty() {
        waapi
            .call("ak.wwise.core.undo.beginGroup", json!({}), None)
            .await
            .map_err(|e| format!("Failed to begin Wwise undo group: {}", e))?;

        for (
            target_node,
            source_node,
            source_bus_id,
            source_bus_name,
            target_bus_id,
            target_bus_name,
        ) in &candidates
        {
            let set_override = waapi
                .call(
                    "ak.wwise.core.object.setProperty",
                    json!({
                        "object": target_node.id,
                        "property": "OverrideOutput",
                        "value": true
                    }),
                    None,
                )
                .await;

            if let Err(error) = set_override {
                failed.push(json!({
                    "target_id": target_node.id,
                    "relative_path": target_node.relative_path,
                    "step": "set OverrideOutput",
                    "error": error
                }));
                continue;
            }

            let set_reference = waapi
                .call(
                    "ak.wwise.core.object.setReference",
                    json!({
                        "object": target_node.id,
                        "reference": "OutputBus",
                        "value": source_bus_id
                    }),
                    None,
                )
                .await;

            match set_reference {
                Ok(_) => synced.push(bus_sync_preview(
                    target_node,
                    source_node,
                    source_bus_id,
                    source_bus_name.as_deref(),
                    target_bus_id.as_deref(),
                    target_bus_name.as_deref(),
                )),
                Err(error) => failed.push(json!({
                    "target_id": target_node.id,
                    "relative_path": target_node.relative_path,
                    "step": "set OutputBus reference",
                    "error": error
                })),
            }
        }

        waapi
            .call(
                "ak.wwise.core.undo.endGroup",
                json!({
                    "displayName": "Sync OutputBus by relative path"
                }),
                None,
            )
            .await
            .ok();
    }

    let unchanged_preview = unchanged.iter().take(30).cloned().collect::<Vec<_>>();
    let skipped_no_source_preview = skipped_no_source
        .iter()
        .take(30)
        .cloned()
        .collect::<Vec<_>>();
    let skipped_no_source_bus_preview = skipped_no_source_bus
        .iter()
        .take(30)
        .cloned()
        .collect::<Vec<_>>();
    let skipped_unsupported_target_preview = skipped_unsupported_target
        .iter()
        .take(30)
        .cloned()
        .collect::<Vec<_>>();
    let synced_preview = synced.iter().take(30).cloned().collect::<Vec<_>>();
    let failed_preview = failed.iter().take(30).cloned().collect::<Vec<_>>();

    Ok(json!({
        "source_root": compact_object_info(&source_root),
        "target_root": compact_object_info(&target_root),
        "settings": {
            "include_target_descendants": include_target_descendants,
            "case_sensitive": case_sensitive,
            "preview_only": preview_only,
            "target_filter_count": target_filter_ids.len()
        },
        "candidate_count": candidates.len(),
        "candidate_preview": candidate_preview,
        "hidden_candidate_count": candidates.len().saturating_sub(candidate_preview.len()),
        "synced_count": synced.len(),
        "synced_preview": synced_preview,
        "hidden_synced_count": synced.len().saturating_sub(synced_preview.len()),
        "failed_count": failed.len(),
        "failed_preview": failed_preview,
        "hidden_failed_count": failed.len().saturating_sub(failed_preview.len()),
        "unchanged_count": unchanged.len(),
        "unchanged_preview": unchanged_preview,
        "hidden_unchanged_count": unchanged.len().saturating_sub(unchanged_preview.len()),
        "skipped_no_source_count": skipped_no_source.len(),
        "skipped_no_source_preview": skipped_no_source_preview,
        "hidden_skipped_no_source_count": skipped_no_source.len().saturating_sub(skipped_no_source_preview.len()),
        "skipped_no_source_bus_count": skipped_no_source_bus.len(),
        "skipped_no_source_bus_preview": skipped_no_source_bus_preview,
        "hidden_skipped_no_source_bus_count": skipped_no_source_bus.len().saturating_sub(skipped_no_source_bus_preview.len()),
        "skipped_unsupported_target_count": skipped_unsupported_target.len(),
        "skipped_unsupported_target_preview": skipped_unsupported_target_preview,
        "hidden_skipped_unsupported_target_count": skipped_unsupported_target.len().saturating_sub(skipped_unsupported_target_preview.len()),
        "executed": !preview_only
    }))
}

async fn get_ancestors_for_object(
    waapi: &WaapiClient,
    object_id: &str,
) -> Result<Vec<Value>, String> {
    let args = json!({
        "from": { "id": [object_id] },
        "transform": [{ "select": ["ancestors"] }]
    });
    let options = json!({
        "return": ["id", "name", "path", "type"]
    });

    let result = waapi
        .call("ak.wwise.core.object.get", args, Some(options))
        .await?;

    Ok(result
        .get("return")
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default())
}

fn collect_unused_original_files(
    root: &Path,
    allowed_extensions: &HashSet<String>,
    used_names: &HashSet<String>,
) -> Result<Vec<PathBuf>, String> {
    let mut dirs = vec![root.to_path_buf()];
    let mut unused = Vec::new();

    while let Some(current_dir) = dirs.pop() {
        let entries = fs::read_dir(&current_dir)
            .map_err(|e| format!("无法读取目录 {}: {}", current_dir.display(), e))?;

        for entry in entries {
            let entry = entry.map_err(|e| format!("读取目录项失败: {}", e))?;
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
                continue;
            }
            if !path.is_file() {
                continue;
            }

            let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
                continue;
            };
            let extension = extension.to_lowercase();
            if !allowed_extensions.contains(&extension) {
                continue;
            }

            let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let stem_key = normalize_filename_key(file_name);
            if stem_key.is_empty() || used_names.contains(&stem_key) {
                continue;
            }

            unused.push(path);
        }
    }

    unused.sort();
    Ok(unused)
}

fn write_unused_originals_log(
    project_dir: &Path,
    originals_root: &Path,
    preview_only: bool,
    candidate_files: &[PathBuf],
    deleted_files: &[PathBuf],
    failed_files: &[(PathBuf, String)],
) -> Option<PathBuf> {
    let log_dir = project_dir.join("GWwiseAgent");
    fs::create_dir_all(&log_dir).ok()?;

    let timestamp = Local::now().format("%Y%m%d_%H%M%S");
    let log_path = log_dir.join(format!("unused_originals_cleanup_{}.txt", timestamp));

    let mut lines = vec![
        format!("Originals root: {}", path_to_display_string(originals_root)),
        format!(
            "Mode: {}",
            if preview_only {
                "preview_only"
            } else {
                "delete"
            }
        ),
        "建议在提交前重新生成所有 SoundBank 做一次验证。".to_string(),
        String::new(),
        format!("Candidates: {}", candidate_files.len()),
    ];

    for path in candidate_files {
        lines.push(format!("CANDIDATE {}", path_to_display_string(path)));
    }

    if !deleted_files.is_empty() {
        lines.push(String::new());
        lines.push(format!("Deleted: {}", deleted_files.len()));
        for path in deleted_files {
            lines.push(format!("DELETED {}", path_to_display_string(path)));
        }
    }

    if !failed_files.is_empty() {
        lines.push(String::new());
        lines.push(format!("Failed: {}", failed_files.len()));
        for (path, error) in failed_files {
            lines.push(format!(
                "FAILED {} :: {}",
                path_to_display_string(path),
                error
            ));
        }
    }

    fs::write(&log_path, lines.join("\n")).ok()?;
    Some(log_path)
}

/// 递归获取对象及其所有子对象
pub async fn get_recursive_objects(
    waapi: &WaapiClient,
    root_ids: Vec<String>,
    filter_type: &str,
) -> Result<Vec<String>, String> {
    let mut all_ids = Vec::new();
    let mut seen = HashSet::new();
    let mut to_process = Vec::new();

    let root_objects = get_objects_by_refs(waapi, &root_ids, &["id", "type"]).await?;
    for obj in root_objects {
        let Some(root_id) = obj.get("id").and_then(|id| id.as_str()) else {
            continue;
        };
        let root_id = root_id.to_string();
        let is_new = seen.insert(root_id.clone());
        if is_new {
            to_process.push(root_id.clone());
        }

        let should_include = obj
            .get("type")
            .and_then(|t| t.as_str())
            .map(|t| matches_recursive_filter(Some(t), filter_type))
            .unwrap_or(matches_recursive_filter(None, filter_type));

        if should_include && is_new {
            all_ids.push(root_id);
        }
    }

    while !to_process.is_empty() {
        let current_batch = to_process.drain(..).collect::<Vec<_>>();

        for obj_id in current_batch {
            // 获取当前对象的子对象
            let query_args = json!({
                "from": { "id": [&obj_id] },
                "transform": [{"select": ["children"]}]
            });

            let query_options = json!({
                "return": ["id", "type"]
            });

            if let Ok(response) = waapi
                .call("ak.wwise.core.object.get", query_args, Some(query_options))
                .await
            {
                if let Some(children) = response.get("return").and_then(|r| r.as_array()) {
                    for child in children {
                        if let Some(child_id) =
                            child.get("id").and_then(|id| id.as_str()).map(String::from)
                        {
                            let is_new = seen.insert(child_id.clone());
                            if is_new {
                                to_process.push(child_id.clone());
                            }

                            // 检查类型过滤
                            let should_include = if filter_type == "*" || filter_type.is_empty() {
                                true
                            } else {
                                child
                                    .get("type")
                                    .and_then(|t| t.as_str())
                                    .map(|t| matches_recursive_filter(Some(t), filter_type))
                                    .unwrap_or(false)
                            };

                            if should_include && is_new {
                                all_ids.push(child_id);
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(all_ids)
}

/// 批量重命名 - 转换为小写
///
/// # 参数
/// - `waapi`: WAAPI 客户端
/// - `object_ids`: 对象 ID 列表 (GUID 或路径)
///
/// # 返回
/// BatchRenameResult 包含操作统计
pub async fn batch_rename_to_lowercase(
    waapi: &WaapiClient,
    object_ids: Vec<String>,
) -> Result<BatchRenameResult, String> {
    batch_rename_impl(waapi, object_ids, |name| name.to_lowercase()).await
}

/// 批量重命名 - 转换为标题大小写
pub async fn batch_rename_to_titlecase(
    waapi: &WaapiClient,
    object_ids: Vec<String>,
) -> Result<BatchRenameResult, String> {
    batch_rename_impl(waapi, object_ids, |name| {
        name.split_whitespace()
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    })
    .await
}

/// 批量重命名 - 转换为大写
pub async fn batch_rename_to_uppercase(
    waapi: &WaapiClient,
    object_ids: Vec<String>,
) -> Result<BatchRenameResult, String> {
    batch_rename_impl(waapi, object_ids, |name| name.to_uppercase()).await
}

/// 批量重命名 - 查找替换
pub async fn batch_rename_find_replace(
    waapi: &WaapiClient,
    object_ids: Vec<String>,
    find: &str,
    replace: &str,
) -> Result<BatchRenameResult, String> {
    batch_rename_impl(waapi, object_ids, |name| name.replace(find, replace)).await
}

/// 批量重命名 - 添加前缀
pub async fn batch_rename_add_prefix(
    waapi: &WaapiClient,
    object_ids: Vec<String>,
    prefix: &str,
) -> Result<BatchRenameResult, String> {
    batch_rename_impl(waapi, object_ids, |name| format!("{}{}", prefix, name)).await
}

/// 批量重命名 - 添加后缀
pub async fn batch_rename_add_suffix(
    waapi: &WaapiClient,
    object_ids: Vec<String>,
    suffix: &str,
) -> Result<BatchRenameResult, String> {
    batch_rename_impl(waapi, object_ids, |name| format!("{}{}", name, suffix)).await
}

/// 内部实现：批量重命名核心逻辑
async fn batch_rename_impl<F>(
    waapi: &WaapiClient,
    object_ids: Vec<String>,
    rename_fn: F,
) -> Result<BatchRenameResult, String>
where
    F: Fn(String) -> String,
{
    if object_ids.is_empty() {
        return Ok(BatchRenameResult {
            total: 0,
            success: 0,
            failed: 0,
            details: vec!["没有要重命名的对象".into()],
        });
    }

    let mut result = BatchRenameResult {
        total: object_ids.len(),
        success: 0,
        failed: 0,
        details: Vec::new(),
    };

    // 开始撤销组 - 将所有操作作为一个原子事务
    let begin_undo = json!({});
    waapi
        .call("ak.wwise.core.undo.beginGroup", begin_undo, None)
        .await
        .map_err(|e| format!("无法开始撤销组: {}", e))?;

    // 处理每个对象
    for obj_id in &object_ids {
        // 先获取当前名称
        let get_args = json!({
            "from": { "id": [obj_id] }
        });

        let get_options = json!({
            "return": ["id", "name", "type"]
        });

        match waapi
            .call("ak.wwise.core.object.get", get_args, Some(get_options))
            .await
        {
            Ok(response) => {
                if let Some(objects) = response.get("return").and_then(|r| r.as_array()) {
                    if let Some(obj) = objects.first() {
                        if let Some(old_name) = obj.get("name").and_then(|n| n.as_str()) {
                            let new_name = rename_fn(old_name.to_string());

                            // 只有名称不同时才重命名
                            if new_name != old_name {
                                let rename_args = json!({
                                    "object": obj_id,
                                    "value": new_name
                                });

                                match waapi
                                    .call("ak.wwise.core.object.setName", rename_args, None)
                                    .await
                                {
                                    Ok(_) => {
                                        result.success += 1;
                                        result
                                            .details
                                            .push(format!("✓ {} → {}", old_name, new_name));
                                    }
                                    Err(e) => {
                                        result.failed += 1;
                                        result.details.push(format!("✗ {}：{}", old_name, e));
                                    }
                                }
                            } else {
                                result.details.push(format!("- {}：名称无变化", old_name));
                            }
                        }
                    }
                }
            }
            Err(e) => {
                result.failed += 1;
                result
                    .details
                    .push(format!("✗ 获取对象信息失败 ({}): {}", obj_id, e));
            }
        }
    }

    // 结束撤销组
    let end_undo = json!({
        "displayName": format!("批量重命名 (成功: {})", result.success)
    });
    waapi
        .call("ak.wwise.core.undo.endGroup", end_undo, None)
        .await
        .ok(); // 即使失败也继续

    Ok(result)
}

// ==================== 批量删除操作 ====================

/// 批量删除对象
pub async fn batch_delete(
    waapi: &WaapiClient,
    object_ids: Vec<String>,
) -> Result<BatchRenameResult, String> {
    if object_ids.is_empty() {
        return Ok(BatchRenameResult {
            total: 0,
            success: 0,
            failed: 0,
            details: vec!["没有要删除的对象".into()],
        });
    }

    let mut result = BatchRenameResult {
        total: object_ids.len(),
        success: 0,
        failed: 0,
        details: Vec::new(),
    };

    // 开始撤销组
    let begin_undo = json!({});
    waapi
        .call("ak.wwise.core.undo.beginGroup", begin_undo, None)
        .await
        .map_err(|e| format!("无法开始撤销组: {}", e))?;

    let ordered_ids = order_object_ids_for_delete(waapi, object_ids).await;

    for obj_id in &ordered_ids {
        let get_args = json!({
            "from": { "id": [obj_id] }
        });

        let get_options = json!({
            "return": ["name"]
        });

        let obj_name = match waapi
            .call("ak.wwise.core.object.get", get_args, Some(get_options))
            .await
        {
            Ok(response) => response
                .get("return")
                .and_then(|r| r.as_array())
                .and_then(|arr| arr.first())
                .and_then(|obj| obj.get("name").and_then(|n| n.as_str()))
                .unwrap_or("未知对象")
                .to_string(),
            Err(_) => "未知对象".to_string(),
        };

        let delete_args = json!({ "object": obj_id });
        match waapi
            .call("ak.wwise.core.object.delete", delete_args, None)
            .await
        {
            Ok(_) => {
                result.success += 1;
                result.details.push(format!("✓ 删除: {}", obj_name));
            }
            Err(e) => {
                result.failed += 1;
                result
                    .details
                    .push(format!("✗ 删除失败 ({}): {}", obj_name, e));
            }
        }
    }

    // 结束撤销组
    let end_undo = json!({
        "displayName": format!("批量删除 (成功: {})", result.success)
    });
    waapi
        .call("ak.wwise.core.undo.endGroup", end_undo, None)
        .await
        .ok();

    Ok(result)
}

// ==================== 批量转换类型操作 ====================

/// 批量转换对象类型
pub async fn batch_convert_type(
    waapi: &WaapiClient,
    object_ids: Vec<String>,
    target_type: &str,
) -> Result<BatchRenameResult, String> {
    if object_ids.is_empty() {
        return Ok(BatchRenameResult {
            total: 0,
            success: 0,
            failed: 0,
            details: vec!["没有要转换的对象".into()],
        });
    }

    let mut result = BatchRenameResult {
        total: object_ids.len(),
        success: 0,
        failed: 0,
        details: Vec::new(),
    };

    // 开始撤销组
    let begin_undo = json!({});
    waapi
        .call("ak.wwise.core.undo.beginGroup", begin_undo, None)
        .await
        .map_err(|e| format!("无法开始撤销组: {}", e))?;

    for obj_id in &object_ids {
        // 获取对象信息
        let get_args = json!({
            "from": { "id": [obj_id] },
            "transform": [{"select": ["parent"]}]
        });

        let get_options = json!({
            "return": ["id", "name", "type"]
        });

        match waapi
            .call("ak.wwise.core.object.get", get_args, Some(get_options))
            .await
        {
            Ok(response) => {
                let old_obj_name = response
                    .get("return")
                    .and_then(|r| r.as_array())
                    .and_then(|arr| {
                        arr.iter().find(|o| {
                            o.get("id")
                                .and_then(|id| id.as_str())
                                .map(|id| id == obj_id)
                                .unwrap_or(false)
                        })
                    })
                    .and_then(|o| o.get("name").and_then(|n| n.as_str()))
                    .unwrap_or("未知")
                    .to_string();

                // 查询父对象
                let parent_query = json!({
                    "from": { "id": [obj_id] },
                    "transform": [{"select": ["parent"]}]
                });

                let parent_options = json!({
                    "return": ["id"]
                });

                match waapi
                    .call(
                        "ak.wwise.core.object.get",
                        parent_query,
                        Some(parent_options),
                    )
                    .await
                {
                    Ok(parent_resp) => {
                        if let Some(parent_id) = parent_resp
                            .get("return")
                            .and_then(|r| r.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|p| p.get("id").and_then(|id| id.as_str()))
                        {
                            // 创建临时对象
                            let temp_name = format!("{}_Temp", old_obj_name);
                            let create_args = json!({
                                "parent": parent_id,
                                "type": target_type,
                                "name": temp_name,
                                "onNameConflict": "rename"
                            });

                            match waapi
                                .call("ak.wwise.core.object.create", create_args, None)
                                .await
                            {
                                Ok(create_resp) => {
                                    if let Some(temp_id) = create_resp
                                        .get("return")
                                        .and_then(|r| r.as_object())
                                        .and_then(|obj| obj.get("id").and_then(|id| id.as_str()))
                                    {
                                        // 移动所有子对象到新对象
                                        let children_query = json!({
                                            "from": { "id": [obj_id] },
                                            "transform": [{"select": ["children"]}]
                                        });

                                        let children_options = json!({
                                            "return": ["id"]
                                        });

                                        if let Ok(children_resp) = waapi
                                            .call(
                                                "ak.wwise.core.object.get",
                                                children_query,
                                                Some(children_options),
                                            )
                                            .await
                                        {
                                            if let Some(children) = children_resp
                                                .get("return")
                                                .and_then(|r| r.as_array())
                                            {
                                                for child in children {
                                                    if let Some(child_id) =
                                                        child.get("id").and_then(|id| id.as_str())
                                                    {
                                                        let move_args = json!({
                                                            "object": child_id,
                                                            "parent": temp_id,
                                                            "onNameConflict": "replace"
                                                        });
                                                        let _ = waapi
                                                            .call(
                                                                "ak.wwise.core.object.move",
                                                                move_args,
                                                                None,
                                                            )
                                                            .await;
                                                    }
                                                }
                                            }
                                        }

                                        // 删除原对象
                                        let delete_args = json!({ "object": obj_id });
                                        let _ = waapi
                                            .call("ak.wwise.core.object.delete", delete_args, None)
                                            .await;

                                        // 重命名新对象
                                        let rename_args = json!({
                                            "object": temp_id,
                                            "value": old_obj_name
                                        });
                                        match waapi
                                            .call("ak.wwise.core.object.setName", rename_args, None)
                                            .await
                                        {
                                            Ok(_) => {
                                                result.success += 1;
                                                result.details.push(format!(
                                                    "✓ {} 转换为 {}",
                                                    old_obj_name, target_type
                                                ));
                                            }
                                            Err(e) => {
                                                result.failed += 1;
                                                result.details.push(format!(
                                                    "✗ {} 重命名失败: {}",
                                                    old_obj_name, e
                                                ));
                                            }
                                        }
                                    } else {
                                        result.failed += 1;
                                        result
                                            .details
                                            .push(format!("✗ {} 创建临时对象失败", old_obj_name));
                                    }
                                }
                                Err(e) => {
                                    result.failed += 1;
                                    result.details.push(format!(
                                        "✗ {} 创建临时对象失败: {}",
                                        old_obj_name, e
                                    ));
                                }
                            }
                        }
                    }
                    Err(e) => {
                        result.failed += 1;
                        result
                            .details
                            .push(format!("✗ {} 查询父对象失败: {}", old_obj_name, e));
                    }
                }
            }
            Err(e) => {
                result.failed += 1;
                result.details.push(format!("✗ 获取对象信息失败: {}", e));
            }
        }
    }

    // 结束撤销组
    let end_undo = json!({
        "displayName": format!("批量转换类型 (成功: {})", result.success)
    });
    waapi
        .call("ak.wwise.core.undo.endGroup", end_undo, None)
        .await
        .ok();

    Ok(result)
}

// ==================== 批量属性设置操作 ====================

/// 批量设置对象属性
pub async fn batch_set_property(
    waapi: &WaapiClient,
    object_ids: Vec<String>,
    property: &str,
    value: Value,
) -> Result<BatchRenameResult, String> {
    if object_ids.is_empty() {
        return Ok(BatchRenameResult {
            total: 0,
            success: 0,
            failed: 0,
            details: vec!["没有要修改的对象".into()],
        });
    }

    let mut result = BatchRenameResult {
        total: object_ids.len(),
        success: 0,
        failed: 0,
        details: Vec::new(),
    };

    // 开始撤销组
    let begin_undo = json!({});
    waapi
        .call("ak.wwise.core.undo.beginGroup", begin_undo, None)
        .await
        .map_err(|e| format!("无法开始撤销组: {}", e))?;

    for obj_id in &object_ids {
        let set_args = json!({
            "object": obj_id,
            "property": property,
            "value": value
        });

        match waapi
            .call("ak.wwise.core.object.setProperty", set_args, None)
            .await
        {
            Ok(_) => {
                result.success += 1;
                result
                    .details
                    .push(format!("✓ 属性「{}」设为 {}", property, value));
            }
            Err(e) => {
                result.failed += 1;
                result.details.push(format!("✗ 设置属性失败: {}", e));
            }
        }
    }

    // 结束撤销组
    let end_undo = json!({
        "displayName": format!("批量设置属性 (成功: {})", result.success)
    });
    waapi
        .call("ak.wwise.core.undo.endGroup", end_undo, None)
        .await
        .ok();

    Ok(result)
}

// ==================== 批量移动操作 ====================

/// 批量移动对象到新的父级
pub async fn batch_move(
    waapi: &WaapiClient,
    object_ids: Vec<String>,
    new_parent_id: &str,
) -> Result<BatchRenameResult, String> {
    if object_ids.is_empty() {
        return Ok(BatchRenameResult {
            total: 0,
            success: 0,
            failed: 0,
            details: vec!["没有要移动的对象".into()],
        });
    }

    let mut result = BatchRenameResult {
        total: object_ids.len(),
        success: 0,
        failed: 0,
        details: Vec::new(),
    };

    // 开始撤销组
    let begin_undo = json!({});
    waapi
        .call("ak.wwise.core.undo.beginGroup", begin_undo, None)
        .await
        .map_err(|e| format!("无法开始撤销组: {}", e))?;

    for obj_id in &object_ids {
        let get_args = json!({
            "from": { "id": [obj_id] }
        });

        let get_options = json!({
            "return": ["name"]
        });

        let obj_name = match waapi
            .call("ak.wwise.core.object.get", get_args, Some(get_options))
            .await
        {
            Ok(response) => response
                .get("return")
                .and_then(|r| r.as_array())
                .and_then(|arr| arr.first())
                .and_then(|obj| obj.get("name").and_then(|n| n.as_str()))
                .unwrap_or("未知对象")
                .to_string(),
            Err(_) => "未知对象".to_string(),
        };

        let move_args = json!({
            "object": obj_id,
            "parent": new_parent_id,
            "onNameConflict": "rename"
        });

        match waapi
            .call("ak.wwise.core.object.move", move_args, None)
            .await
        {
            Ok(_) => {
                result.success += 1;
                result.details.push(format!("✓ 移动: {}", obj_name));
            }
            Err(e) => {
                result.failed += 1;
                result
                    .details
                    .push(format!("✗ 移动失败 ({}): {}", obj_name, e));
            }
        }
    }

    // 结束撤销组
    let end_undo = json!({
        "displayName": format!("批量移动 (成功: {})", result.success)
    });
    waapi
        .call("ak.wwise.core.undo.endGroup", end_undo, None)
        .await
        .ok();

    Ok(result)
}

/// 批量按类型过滤子对象
///
/// # 参数
/// - `waapi`: WAAPI 客户端
/// - `object_ids`: 对象 ID 列表（起点对象）
/// - `type_list`: 要过滤的类型列表，如 ["Sound", "ActorMixer"]
/// - `keep_self`: 是否包含自己（如果自己也符合类型）
/// - `include_descendants`: true 获取所有后代，false 仅获取直属子对象
///
/// # 返回
/// 返回所有符合类型的对象 ID 列表
pub async fn batch_get_children_by_type(
    waapi: &WaapiClient,
    object_ids: Vec<String>,
    type_list: Vec<String>,
    keep_self: bool,
    include_descendants: bool,
) -> Result<Vec<String>, String> {
    if object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut result_ids = Vec::new();

    // Step 1: 如果 keep_self 为 true，先过滤自己
    if keep_self && !type_list.is_empty() {
        let filter_args = json!({
            "from": { "id": object_ids.clone() },
            "transform": [
                { "where": ["type:isIn", type_list.clone()] }
            ]
        });

        let filter_options = json!({
            "return": ["id"]
        });

        match waapi
            .call(
                "ak.wwise.core.object.get",
                filter_args,
                Some(filter_options),
            )
            .await
        {
            Ok(response) => {
                if let Some(objects) = response.get("return").and_then(|r| r.as_array()) {
                    for obj in objects {
                        if let Some(id) = obj.get("id").and_then(|i| i.as_str()) {
                            result_ids.push(id.to_string());
                        }
                    }
                }
            }
            Err(_) => {} // 继续处理子对象
        }
    }

    // Step 2: 查询子对象
    let child_args = json!({
        "from": { "id": object_ids },
        "transform": [
            { "select": [if include_descendants { "descendants" } else { "children" }] },
            { "where": ["type:isIn", type_list] }
        ]
    });

    let child_options = json!({
        "return": ["id"]
    });

    match waapi
        .call("ak.wwise.core.object.get", child_args, Some(child_options))
        .await
    {
        Ok(response) => {
            if let Some(objects) = response.get("return").and_then(|r| r.as_array()) {
                for obj in objects {
                    if let Some(id) = obj.get("id").and_then(|i| i.as_str()) {
                        let id_str = id.to_string();
                        if !result_ids.contains(&id_str) {
                            result_ids.push(id_str);
                        }
                    }
                }
            }
        }
        Err(e) => {
            return Err(format!("批量获取子对象失败: {}", e));
        }
    }

    Ok(result_ids)
}

/// 批量按名称替换音频
///
/// 遍历目标对象下的所有 Sound 对象，在指定的本地目录中查找同名的 .wav/.aif/.ogg 文件。
/// 如果找到，则替换该 Sound 对象的内部 AudioFileSource。
///
/// # 参数
/// - `waapi`: WAAPI 客户端
/// - `target_object_id`: 目标对象 ID 或路径（如 "\Actor-Mixer Hierarchy\Weapons"）
/// - `local_directory`: 本地包含音频文件的文件夹路径
/// - `language`: 音频语言标签，如 "SFX", "Chinese", "English"
///
/// # 返回
/// BatchRenameResult 包含替换统计和详情
pub async fn batch_replace_audio_by_name(
    waapi: &WaapiClient,
    target_object_id: &str,
    local_directory: &str,
    language: &str,
    originals_subfolder: &str,
) -> Result<BatchRenameResult, String> {
    // 1. 读取本地目录，收集音频文件
    let path = std::path::Path::new(local_directory);
    if !path.exists() || !path.is_dir() {
        return Err(format!("本地目录不存在或不是文件夹: {}", local_directory));
    }

    let mut local_files = std::collections::HashMap::new();
    match std::fs::read_dir(path) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() {
                    if let Some(ext) = p.extension().and_then(|s| s.to_str()) {
                        let ext_lower = ext.to_lowercase();
                        if ext_lower == "wav"
                            || ext_lower == "aif"
                            || ext_lower == "aiff"
                            || ext_lower == "ogg"
                        {
                            if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                                local_files
                                    .insert(stem.to_string(), p.to_string_lossy().to_string());
                            }
                        }
                    }
                }
            }
        }
        Err(e) => return Err(format!("无法读取目录: {}", e)),
    }

    if local_files.is_empty() {
        return Err("本地目录中没有找到音频文件 (.wav, .aif, .ogg)".to_string());
    }

    // 2. 查询 Wwise 中 target_object_id 下的所有 Sound 对象
    let (id_refs, path_refs) = split_object_refs(&[target_object_id.to_string()]);
    let query_args = if !id_refs.is_empty() {
        json!({
            "from": { "id": id_refs },
            "transform": [
                { "select": ["descendants"] },
                { "where": ["type:isIn", ["Sound"]] }
            ]
        })
    } else {
        json!({
            "from": { "path": path_refs },
            "transform": [
                { "select": ["descendants"] },
                { "where": ["type:isIn", ["Sound"]] }
            ]
        })
    };
    let query_options = json!({
        "return": ["id", "name", "parent"]
    });

    let response = waapi
        .call("ak.wwise.core.object.get", query_args, Some(query_options))
        .await
        .map_err(|e| format!("查询 Sound 对象失败: {}", e))?;

    let sounds = response
        .get("return")
        .and_then(|r| r.as_array())
        .ok_or("查询结果格式错误")?;

    let mut imports = Vec::new();
    let mut matched_names = Vec::new();

    // 3. 匹配 Sound 名称与本地文件名
    for sound in sounds {
        if let (Some(name), Some(parent_obj)) = (
            sound.get("name").and_then(|n| n.as_str()),
            sound.get("parent").and_then(|p| p.as_object()),
        ) {
            if let Some(parent_id) = parent_obj.get("id").and_then(|id| id.as_str()) {
                if let Some(local_path) = local_files.get(name) {
                    imports.push(json!({
                        "audioFile": local_path,
                        "importLocation": parent_id,
                        "objectPath": format!("<Sound>{}", name)
                    }));
                    matched_names.push(name.to_string());
                }
            }
        }
    }

    if imports.is_empty() {
        return Ok(BatchRenameResult {
            total: 0,
            success: 0,
            failed: 0,
            details: vec!["没有找到名称匹配的 Sound 对象".to_string()],
        });
    }

    let mut result = BatchRenameResult {
        total: imports.len(),
        success: 0,
        failed: 0,
        details: Vec::new(),
    };

    // 4. 开始撤销组并执行导入
    let begin_undo = json!({});
    let _ = waapi
        .call("ak.wwise.core.undo.beginGroup", begin_undo, None)
        .await
        .map_err(|e| format!("无法开始撤销组: {}", e))?;

    for (import_entry, name) in imports.into_iter().zip(matched_names.into_iter()) {
        let import_args = json!({
            "importOperation": "replaceExisting",
            "default": {
                "importLanguage": language,
                "originalsSubFolder": originals_subfolder
            },
            "imports": [import_entry]
        });

        match waapi
            .call("ak.wwise.core.audio.import", import_args, None)
            .await
        {
            Ok(_) => {
                result.success += 1;
                result.details.push(format!("✓ 成功替换: {}", name));
            }
            Err(e) => {
                result.failed += 1;
                result.details.push(format!("✗ 替换失败 {}: {}", name, e));
            }
        }
    }

    let end_undo = json!({
        "displayName": format!("按名称批量替换音频 (成功: {})", result.success)
    });
    let _ = waapi
        .call("ak.wwise.core.undo.endGroup", end_undo, None)
        .await;

    Ok(result)
}

/// 获取 Wwise 工程的默认语言和支持的语言列表
///
/// # 返回
/// JSON 对象包含:
/// - `default_language`: 工程默认语言
/// - `project_path`: 工程路径
pub async fn batch_smart_delete(
    waapi: &WaapiClient,
    object_refs: Vec<String>,
    name_contains: Option<&str>,
    exclude_name_contains: &[String],
    case_sensitive: bool,
    check_references: bool,
    reference_types: &[String],
    block_if_referenced: bool,
    preview_only: bool,
) -> Result<Value, String> {
    let resolved_objects =
        get_objects_by_refs(waapi, &object_refs, &["id", "name", "path", "type"]).await?;

    let mut candidate_objects = Vec::new();
    let mut skipped_by_name = Vec::new();

    for obj in resolved_objects {
        let name = obj.get("name").and_then(|v| v.as_str()).unwrap_or_default();
        if matches_name_filters(name, name_contains, exclude_name_contains, case_sensitive) {
            candidate_objects.push(obj);
        } else {
            skipped_by_name.push(compact_object_info(&obj));
        }
    }

    let mut blocked_by_references = Vec::new();
    let mut warnings = Vec::new();
    let mut delete_candidates = Vec::new();

    for obj in candidate_objects {
        let references = if check_references {
            query_references_to_object(
                waapi,
                obj.get("id").and_then(|v| v.as_str()).unwrap_or_default(),
                reference_types,
            )
            .await?
        } else {
            Vec::new()
        };

        if references.is_empty() {
            delete_candidates.push(obj);
            continue;
        }

        let reference_preview = references
            .iter()
            .map(compact_object_info)
            .collect::<Vec<_>>();
        let (reference_preview, hidden_reference_count) = limit_preview(reference_preview, 20);
        let entry = json!({
            "object": compact_object_info(&obj),
            "reference_count": references.len(),
            "references_preview": reference_preview,
            "hidden_reference_count": hidden_reference_count
        });

        if block_if_referenced {
            blocked_by_references.push(entry);
        } else {
            warnings.push(entry);
            delete_candidates.push(obj);
        }
    }

    let delete_candidate_count = delete_candidates.len();
    let delete_candidate_ids = delete_candidates
        .iter()
        .filter_map(|obj| obj.get("id").and_then(|v| v.as_str()).map(String::from))
        .collect::<Vec<_>>();

    let delete_result = if preview_only || delete_candidate_ids.is_empty() {
        None
    } else {
        Some(batch_delete(waapi, delete_candidate_ids).await?.to_json())
    };

    let candidate_preview = delete_candidates
        .iter()
        .map(compact_object_info)
        .collect::<Vec<_>>();
    let (candidate_preview, hidden_candidate_count) = limit_preview(candidate_preview, 30);
    let blocked_count = blocked_by_references.len();
    let warning_count = warnings.len();
    let (skipped_by_name_preview, hidden_skipped_count) = limit_preview(skipped_by_name, 20);
    let (blocked_preview, hidden_blocked_count) = limit_preview(blocked_by_references, 20);
    let (warning_preview, hidden_warning_count) = limit_preview(warnings, 20);

    Ok(json!({
        "requested_count": object_refs.len(),
        "resolved_count": delete_candidate_count + blocked_count + warning_count + skipped_by_name_preview.len() + hidden_skipped_count,
        "filters": {
            "name_contains": name_contains,
            "exclude_name_contains": exclude_name_contains,
            "case_sensitive": case_sensitive,
            "check_references": check_references,
            "reference_types": reference_types,
            "block_if_referenced": block_if_referenced,
            "preview_only": preview_only
        },
        "delete_candidate_count": delete_candidate_count,
        "delete_candidates_preview": candidate_preview,
        "hidden_delete_candidate_count": hidden_candidate_count,
        "blocked_count": blocked_count,
        "blocked_preview": blocked_preview,
        "hidden_blocked_count": hidden_blocked_count,
        "warning_count": warning_count,
        "warning_preview": warning_preview,
        "hidden_warning_count": hidden_warning_count,
        "skipped_by_name_count": skipped_by_name_preview.len() + hidden_skipped_count,
        "skipped_by_name_preview": skipped_by_name_preview,
        "hidden_skipped_by_name_count": hidden_skipped_count,
        "delete_result": delete_result,
        "executed": !preview_only && delete_candidate_count > 0
    }))
}

pub async fn batch_delete_unused_descendants(
    waapi: &WaapiClient,
    root_refs: Vec<String>,
    candidate_types: &[String],
    include_root: bool,
    check_references: bool,
    reference_types: &[String],
    preview_only: bool,
) -> Result<Value, String> {
    let resolved_roots =
        get_objects_by_refs(waapi, &root_refs, &["id", "name", "path", "type", "parent"]).await?;

    let root_ids = resolved_roots
        .iter()
        .filter_map(extract_object_id)
        .collect::<Vec<_>>();
    let root_id_set = root_ids.iter().cloned().collect::<HashSet<_>>();

    let mut objects_by_id = HashMap::new();
    for root in &resolved_roots {
        if let Some(id) = extract_object_id(root) {
            objects_by_id.insert(id, root.clone());
        }
    }

    let (descendants, inclusion_flag_count) = get_descendants_for_roots(waapi, &root_ids).await?;
    let inclusion_flag_mode = inclusion_flag_count > 0;
    for descendant in descendants {
        if let Some(id) = extract_object_id(&descendant) {
            objects_by_id.entry(id).or_insert(descendant);
        }
    }
    enrich_objects_with_source_paths(waapi, &mut objects_by_id).await?;

    let analyzed_ids = objects_by_id.keys().cloned().collect::<HashSet<_>>();
    let scanned_object_count = analyzed_ids.len();

    let mut parent_by_id = HashMap::new();
    let mut children_by_id: HashMap<String, Vec<String>> = HashMap::new();
    for (object_id, object) in &objects_by_id {
        let parent_id = extract_parent_id(object);
        parent_by_id.insert(object_id.clone(), parent_id.clone());
        if let Some(parent_id) = parent_id {
            if analyzed_ids.contains(&parent_id) {
                children_by_id
                    .entry(parent_id)
                    .or_default()
                    .push(object_id.clone());
            }
        }
    }

    let mut usage_sources = Vec::new();
    let mut protected_ids = HashSet::new();
    let mut skipped_missing_inclusion = Vec::new();
    let audio_source_activity = derive_audio_source_activity(&objects_by_id, &parent_by_id);
    let inactive_audio_source_ids = audio_source_activity
        .iter()
        .filter_map(|(object_id, is_active)| (!*is_active).then_some(object_id.clone()))
        .collect::<HashSet<_>>();
    let candidate_ids = if inclusion_flag_mode {
        for object_id in &analyzed_ids {
            if !include_root && root_id_set.contains(object_id) {
                continue;
            }
            let Some(object) = objects_by_id.get(object_id) else {
                continue;
            };
            if extract_inclusion_flag(object).is_none() {
                match audio_source_activity.get(object_id).copied() {
                    Some(true) => {
                        protected_ids.insert(object_id.clone());
                        let mut entry = json!({
                            "object": compact_object_info(object),
                            "reason": "matches_parent_active_source"
                        });
                        if let Some(parent_id) = parent_by_id.get(object_id).cloned().flatten() {
                            if let Some(parent) = objects_by_id.get(&parent_id) {
                                entry["parent"] = compact_object_info(parent);
                            }
                        }
                        usage_sources.push(entry);
                    }
                    Some(false) => {}
                    None => skipped_missing_inclusion.push(compact_object_info(object)),
                }
            }
        }

        let candidate_ids = analyzed_ids
            .iter()
            .filter(|object_id| include_root || !root_id_set.contains(*object_id))
            .filter(|object_id| {
                objects_by_id
                    .get(*object_id)
                    .map(|object| match extract_inclusion_flag(object) {
                        Some(false) => true,
                        Some(true) => false,
                        None => audio_source_activity.get(*object_id).copied() == Some(false),
                    })
                    .unwrap_or(false)
            })
            .filter(|object_id| {
                objects_by_id
                    .get(*object_id)
                    .and_then(|object| object.get("type").and_then(|value| value.as_str()))
                    .map(|object_type| matches_any_object_type(Some(object_type), candidate_types))
                    .unwrap_or(matches_any_object_type(None, candidate_types))
            })
            .cloned()
            .collect::<HashSet<_>>();

        for object in objects_by_id.values() {
            if extract_inclusion_flag(object) == Some(true) {
                if let Some(object_id) = extract_object_id(object) {
                    protected_ids.insert(object_id);
                }
                usage_sources.push(json!({
                    "object": compact_object_info(object),
                    "reason": "inclusion_true"
                }));
            }
        }

        candidate_ids
    } else {
        let mut directly_used_ids = HashSet::new();

        if check_references {
            let mut sorted_ids = analyzed_ids.iter().cloned().collect::<Vec<_>>();
            sorted_ids.sort();

            for object_id in sorted_ids {
                let references =
                    query_references_to_object(waapi, &object_id, reference_types).await?;
                let external_references = filter_external_references(references, &analyzed_ids);
                if external_references.is_empty() {
                    continue;
                }

                directly_used_ids.insert(object_id.clone());

                if let Some(object) = objects_by_id.get(&object_id) {
                    let reference_preview = external_references
                        .iter()
                        .map(compact_object_info)
                        .collect::<Vec<_>>();
                    let (reference_preview, hidden_reference_count) =
                        limit_preview(reference_preview, 20);
                    usage_sources.push(json!({
                        "object": compact_object_info(object),
                        "reason": "externally_referenced",
                        "reference_count": external_references.len(),
                        "references_preview": reference_preview,
                        "hidden_reference_count": hidden_reference_count
                    }));
                }
            }

            for root in &resolved_roots {
                let Some(root_id) = extract_object_id(root) else {
                    continue;
                };
                if directly_used_ids.contains(&root_id) {
                    continue;
                }

                let ancestors = get_ancestors_for_object(waapi, &root_id).await?;
                let mut matched_ancestor_entry = None;

                for ancestor in ancestors {
                    let Some(ancestor_id) = extract_object_id(&ancestor) else {
                        continue;
                    };
                    let references =
                        query_references_to_object(waapi, &ancestor_id, reference_types).await?;
                    let external_references = filter_external_references(references, &analyzed_ids);
                    if external_references.is_empty() {
                        continue;
                    }

                    let reference_preview = external_references
                        .iter()
                        .map(compact_object_info)
                        .collect::<Vec<_>>();
                    let (reference_preview, hidden_reference_count) =
                        limit_preview(reference_preview, 20);
                    matched_ancestor_entry = Some(json!({
                        "object": compact_object_info(root),
                        "reason": "ancestor_referenced",
                        "source_ancestor": compact_object_info(&ancestor),
                        "reference_count": external_references.len(),
                        "references_preview": reference_preview,
                        "hidden_reference_count": hidden_reference_count
                    }));
                    break;
                }

                if let Some(entry) = matched_ancestor_entry {
                    directly_used_ids.insert(root_id);
                    usage_sources.push(entry);
                }
            }
        }

        protected_ids = collect_protected_ids(&directly_used_ids, &parent_by_id, &children_by_id);
        for object_id in &inactive_audio_source_ids {
            if !directly_used_ids.contains(object_id) {
                protected_ids.remove(object_id);
            }
        }

        analyzed_ids
            .iter()
            .filter(|object_id| include_root || !root_id_set.contains(*object_id))
            .filter(|object_id| !protected_ids.contains(*object_id))
            .filter(|object_id| {
                objects_by_id
                    .get(*object_id)
                    .and_then(|object| object.get("type").and_then(|value| value.as_str()))
                    .map(|object_type| matches_any_object_type(Some(object_type), candidate_types))
                    .unwrap_or(matches_any_object_type(None, candidate_types))
            })
            .cloned()
            .collect::<HashSet<_>>()
    };

    let mut delete_candidates = objects_by_id
        .iter()
        .filter_map(|(object_id, object)| {
            candidate_ids
                .contains(object_id)
                .then_some((object_id.clone(), object.clone()))
        })
        .filter(|(object_id, _)| is_top_level_candidate(object_id, &candidate_ids, &parent_by_id))
        .collect::<Vec<_>>();
    delete_candidates.sort_by(|(_, left), (_, right)| {
        left.get("path")
            .and_then(|value| value.as_str())
            .cmp(&right.get("path").and_then(|value| value.as_str()))
    });

    let delete_candidate_ids = delete_candidates
        .iter()
        .map(|(object_id, _)| object_id.clone())
        .collect::<Vec<_>>();
    let delete_candidate_preview = delete_candidates
        .iter()
        .map(|(_, object)| compact_object_info(object))
        .collect::<Vec<_>>();
    let (delete_candidate_preview, hidden_delete_candidate_count) =
        limit_preview(delete_candidate_preview, 30);

    let delete_result = if preview_only || delete_candidate_ids.is_empty() {
        None
    } else {
        Some(batch_delete(waapi, delete_candidate_ids).await?.to_json())
    };

    let (usage_sources_preview, hidden_usage_source_count) = limit_preview(usage_sources, 20);
    let (skipped_missing_inclusion_preview, hidden_skipped_missing_inclusion_count) =
        limit_preview(skipped_missing_inclusion, 20);
    let root_preview = resolved_roots
        .iter()
        .map(compact_object_info)
        .collect::<Vec<_>>();
    let (root_preview, hidden_root_count) = limit_preview(root_preview, 10);

    Ok(json!({
        "requested_root_count": root_refs.len(),
        "resolved_root_count": resolved_roots.len(),
        "roots_preview": root_preview,
        "hidden_root_count": hidden_root_count,
        "scanned_object_count": scanned_object_count,
        "filters": {
            "candidate_types": candidate_types,
            "include_root": include_root,
            "check_references": check_references,
            "reference_types": reference_types,
            "preview_only": preview_only,
            "used_inclusion_flag": inclusion_flag_mode,
            "inclusion_flag_coverage_count": inclusion_flag_count
        },
        "usage_source_count": usage_sources_preview.len() + hidden_usage_source_count,
        "usage_sources_preview": usage_sources_preview,
        "hidden_usage_source_count": hidden_usage_source_count,
        "skipped_missing_inclusion_count": skipped_missing_inclusion_preview.len() + hidden_skipped_missing_inclusion_count,
        "skipped_missing_inclusion_preview": skipped_missing_inclusion_preview,
        "hidden_skipped_missing_inclusion_count": hidden_skipped_missing_inclusion_count,
        "protected_count": protected_ids.len(),
        "delete_candidate_count": delete_candidate_preview.len() + hidden_delete_candidate_count,
        "delete_candidates_preview": delete_candidate_preview,
        "hidden_delete_candidate_count": hidden_delete_candidate_count,
        "delete_result": delete_result,
        "executed": !preview_only && !delete_candidates.is_empty()
    }))
}

pub async fn cleanup_unused_originals_files(
    waapi: &WaapiClient,
    originals_root_override: Option<&str>,
    preview_only: bool,
    extensions: &[String],
) -> Result<Value, String> {
    let project_dir = get_project_dir(waapi).await?;
    let project_file_path = get_project_file_path(waapi).await?;
    let originals_root = originals_root_override
        .map(normalize_filesystem_path)
        .unwrap_or_else(|| project_dir.join("Originals"));

    if !originals_root.exists() || !originals_root.is_dir() {
        return Err(format!(
            "Originals 目录不存在或不是目录: {}",
            path_to_display_string(&originals_root)
        ));
    }

    let normalized_extensions = {
        let provided = normalize_extension_set(extensions);
        if provided.is_empty() {
            ["wav", "aif", "aiff", "ogg", "akd"]
                .into_iter()
                .map(String::from)
                .collect::<HashSet<_>>()
        } else {
            provided
        }
    };

    let query_args = json!({
        "from": {
            "ofType": ["AudioFileSource"]
        }
    });
    let query_options = json!({
        "return": ["name", "path"]
    });
    let source_result = waapi
        .call("ak.wwise.core.object.get", query_args, Some(query_options))
        .await
        .map_err(|e| format!("无法获取 AudioFileSource 列表: {}", e))?;

    let mut used_names = HashSet::new();
    if let Some(items) = source_result.get("return").and_then(|r| r.as_array()) {
        for item in items {
            if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
                let key = normalize_filename_key(name);
                if !key.is_empty() {
                    used_names.insert(key);
                }
            }
        }
    }

    let candidate_files =
        collect_unused_original_files(&originals_root, &normalized_extensions, &used_names)?;

    let mut deleted_files = Vec::new();
    let mut failed_files = Vec::new();

    if !preview_only {
        for path in &candidate_files {
            if let Err(error) = fs::remove_file(path) {
                failed_files.push((path.clone(), error.to_string()));
            } else {
                deleted_files.push(path.clone());
            }
        }
    }

    let log_path = write_unused_originals_log(
        &project_dir,
        &originals_root,
        preview_only,
        &candidate_files,
        &deleted_files,
        &failed_files,
    )
    .map(|path| path_to_display_string(&path));

    let candidate_preview = candidate_files
        .iter()
        .take(50)
        .map(|path| {
            json!({
                "path": path_to_display_string(path),
                "extension": path.extension().and_then(|v| v.to_str()).unwrap_or_default(),
                "name": path.file_name().and_then(|v| v.to_str()).unwrap_or_default()
            })
        })
        .collect::<Vec<_>>();
    let failed_preview = failed_files
        .iter()
        .take(20)
        .map(|(path, error)| {
            json!({
                "path": path_to_display_string(path),
                "error": error
            })
        })
        .collect::<Vec<_>>();

    let mut extension_list = normalized_extensions.into_iter().collect::<Vec<_>>();
    extension_list.sort();

    Ok(json!({
        "project_file_path": project_file_path,
        "originals_root": path_to_display_string(&originals_root),
        "preview_only": preview_only,
        "extensions": extension_list,
        "used_audio_source_count": used_names.len(),
        "candidate_count": candidate_files.len(),
        "candidate_preview": candidate_preview,
        "hidden_candidate_count": candidate_files.len().saturating_sub(candidate_preview.len()),
        "deleted_count": deleted_files.len(),
        "failed_count": failed_files.len(),
        "failed_preview": failed_preview,
        "log_path": log_path
    }))
}

pub async fn get_project_languages(waapi: &WaapiClient) -> Result<serde_json::Value, String> {
    // 获取默认语言
    let default_lang_args = json!({
        "from": {
            "ofType": ["Project"]
        }
    });

    let default_lang_options = json!({
        "return": ["@DefaultLanguage"]
    });

    let default_lang_result = waapi
        .call(
            "ak.wwise.core.object.get",
            default_lang_args,
            Some(default_lang_options),
        )
        .await
        .map_err(|e| format!("无法获取默认语言: {}", e))?;

    let default_language = default_lang_result
        .get("return")
        .and_then(|r| r.as_array())
        .and_then(|arr| arr.first())
        .and_then(|obj| obj.get("@DefaultLanguage"))
        .and_then(|lang| lang.as_str())
        .map(String::from)
        .unwrap_or_else(|| "SFX".to_string());

    // 获取工程路径
    let project_path_args = json!({
        "from": {
            "ofType": ["Project"]
        }
    });

    let project_path_options = json!({
        "return": ["filePath"]
    });

    let project_path_result = waapi
        .call(
            "ak.wwise.core.object.get",
            project_path_args,
            Some(project_path_options),
        )
        .await
        .ok();

    let project_path = project_path_result
        .and_then(|res| {
            res.get("return")
                .and_then(|r| r.as_array())
                .and_then(|arr| arr.first())
                .and_then(|obj| obj.get("filePath"))
                .and_then(|path| path.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "未知".to_string());

    // 查询所有可用的语言（通过查询所有 Sound 对象的语言属性）
    let languages_query = json!({
        "from": {
            "ofType": ["Sound"]
        }
    });

    let languages_options = json!({
        "return": ["audioSource:language"]
    });

    let mut all_languages = std::collections::HashSet::new();
    all_languages.insert("SFX".to_string()); // 总是包含 SFX

    if let Ok(sounds_result) = waapi
        .call(
            "ak.wwise.core.object.get",
            languages_query,
            Some(languages_options),
        )
        .await
    {
        if let Some(sounds) = sounds_result.get("return").and_then(|r| r.as_array()) {
            for sound in sounds {
                if let Some(lang_obj) = sound
                    .get("audioSource:language")
                    .and_then(|l| l.as_object())
                {
                    if let Some(lang_name) = lang_obj.get("name").and_then(|n| n.as_str()) {
                        all_languages.insert(lang_name.to_string());
                    }
                }
            }
        }
    }

    let mut languages: Vec<String> = all_languages.into_iter().collect();
    languages.sort();

    Ok(json!({
        "default_language": default_language,
        "project_path": project_path,
        "available_languages": languages,
        "language_count": languages.len()
    }))
}

/// 沿路径递归创建不存在的对象结构
///
/// # 参数
/// - `waapi`: WAAPI 客户端
/// - `path`: 要创建的路径，如 `\Actor-Mixer Hierarchy\Combat\Weapons`
/// - `create_type`: 创建的容器类型，默认 "Folder"（也可用 "ActorMixer"）
///
/// # 返回
/// 最终创建或找到的对象 ID 和信息
pub async fn create_path_if_not_exists(
    waapi: &WaapiClient,
    path: &str,
    create_type: &str,
) -> Result<serde_json::Value, String> {
    // 分割路径
    let parts: Vec<&str> = path.split('\\').filter(|s| !s.is_empty()).collect();

    if parts.is_empty() {
        return Err("路径为空".to_string());
    }

    let mut current_path = String::new();
    let mut last_obj: Option<String> = None;
    let mut details: Vec<String> = Vec::new();

    // 逐级创建路径
    for (idx, part) in parts.iter().enumerate() {
        current_path.push('\\');
        current_path.push_str(part);

        // 检查当前路径是否存在
        let check_args = json!({
            "from": {
                "path": [&current_path]
            }
        });

        let check_options = json!({
            "return": ["id", "name", "type", "path"]
        });

        match waapi
            .call("ak.wwise.core.object.get", check_args, Some(check_options))
            .await
        {
            Ok(response) => {
                if let Some(objects) = response.get("return").and_then(|r| r.as_array()) {
                    if let Some(obj) = objects.first() {
                        if let Some(obj_id) = obj.get("id").and_then(|i| i.as_str()) {
                            last_obj = Some(obj_id.to_string());
                            details.push(format!("✓ 路径已存在: {}", current_path));
                        }
                    }
                } else {
                    // 路径不存在，需要创建
                    if idx == 0 {
                        // 第一级必须已存在（不能创建根）
                        return Err(format!("路径起点不存在: {}", part));
                    }

                    // 获取父对象
                    if let Some(parent_id) = &last_obj {
                        let create_args = json!({
                            "parent": parent_id,
                            "type": create_type,
                            "name": part,
                            "onNameConflict": "rename"
                        });

                        let create_options = json!({
                            "return": ["id", "name", "type", "path"]
                        });

                        match waapi
                            .call(
                                "ak.wwise.core.object.create",
                                create_args,
                                Some(create_options),
                            )
                            .await
                        {
                            Ok(create_response) => {
                                if let Some(created_obj) =
                                    create_response.get("return").and_then(|r| match r {
                                        serde_json::Value::Array(arr) => arr.first(),
                                        serde_json::Value::Object(_) => Some(r),
                                        _ => None,
                                    })
                                {
                                    if let Some(new_id) =
                                        created_obj.get("id").and_then(|i| i.as_str())
                                    {
                                        last_obj = Some(new_id.to_string());
                                        details.push(format!(
                                            "✓ 创建成功: {} ({} 类型)",
                                            current_path, create_type
                                        ));
                                    }
                                }
                            }
                            Err(e) => {
                                return Err(format!("创建 {} 失败: {}", current_path, e));
                            }
                        }
                    } else {
                        return Err("无法确定父对象".to_string());
                    }
                }
            }
            Err(_) => {
                // 路径不存在，需要创建
                if idx == 0 {
                    return Err(format!("路径起点不存在: {}", part));
                }

                if let Some(parent_id) = &last_obj {
                    let create_args = json!({
                        "parent": parent_id,
                        "type": create_type,
                        "name": part,
                        "onNameConflict": "rename"
                    });

                    let create_options = json!({
                        "return": ["id", "name", "type", "path"]
                    });

                    match waapi
                        .call(
                            "ak.wwise.core.object.create",
                            create_args,
                            Some(create_options),
                        )
                        .await
                    {
                        Ok(create_response) => {
                            if let Some(created_obj) =
                                create_response.get("return").and_then(|r| match r {
                                    serde_json::Value::Array(arr) => arr.first(),
                                    serde_json::Value::Object(_) => Some(r),
                                    _ => None,
                                })
                            {
                                if let Some(new_id) = created_obj.get("id").and_then(|i| i.as_str())
                                {
                                    last_obj = Some(new_id.to_string());
                                    details.push(format!(
                                        "✓ 创建成功: {} ({} 类型)",
                                        current_path, create_type
                                    ));
                                }
                            }
                        }
                        Err(e) => {
                            return Err(format!("创建 {} 失败: {}", current_path, e));
                        }
                    }
                }
            }
        }
    }

    Ok(json!({
        "path": path,
        "final_object_id": last_obj,
        "steps": details,
        "success": !details.is_empty()
    }))
}

#[cfg(test)]
mod tests {
    use super::{
        collect_protected_ids, derive_audio_source_activity, is_top_level_candidate,
        matches_any_object_type, matches_name_filters, matches_recursive_filter,
        normalize_extension_set, normalize_filename_key, order_ids_by_depth, parent_relative_path,
        path_depth, relative_path_from_base, sync_path_key, sync_typed_key,
    };
    use serde_json::json;
    use std::collections::{HashMap, HashSet};

    #[test]
    fn recursive_filter_matches_all_when_filter_empty_or_wildcard() {
        assert!(matches_recursive_filter(Some("Event"), "*"));
        assert!(matches_recursive_filter(Some("Folder"), ""));
        assert!(matches_recursive_filter(None, "*"));
        assert!(matches_recursive_filter(None, ""));
    }

    #[test]
    fn recursive_filter_only_includes_matching_types_for_specific_filter() {
        assert!(matches_recursive_filter(Some("Event"), "Event"));
        assert!(!matches_recursive_filter(Some("Folder"), "Event"));
        assert!(!matches_recursive_filter(None, "Event"));
    }

    #[test]
    fn delete_order_prefers_deeper_paths_before_parents() {
        let mut depth_by_id = HashMap::new();
        depth_by_id.insert(
            "root".to_string(),
            path_depth("\\Events\\Default Work Unit\\Root"),
        );
        depth_by_id.insert(
            "child".to_string(),
            path_depth("\\Events\\Default Work Unit\\Root\\Nested\\Child"),
        );
        depth_by_id.insert(
            "nested".to_string(),
            path_depth("\\Events\\Default Work Unit\\Root\\Nested"),
        );

        let ordered = order_ids_by_depth(
            vec!["root".into(), "child".into(), "nested".into()],
            &depth_by_id,
        );

        assert_eq!(ordered, vec!["child", "nested", "root"]);
    }

    #[test]
    fn name_filters_support_include_and_exclude() {
        assert!(matches_name_filters(
            "Weapon_Rifle_AK",
            Some("Rifle"),
            &["Tail".into()],
            false
        ));
        assert!(!matches_name_filters(
            "Weapon_Rifle_AK_Tail",
            Some("Rifle"),
            &["Tail".into()],
            false
        ));
        assert!(!matches_name_filters(
            "Weapon_SMG_MP40",
            Some("Rifle"),
            &[],
            false
        ));
    }

    #[test]
    fn normalize_filename_key_uses_lowercase_stem() {
        assert_eq!(normalize_filename_key("Gun\\SMG_MP40.WAV"), "smg_mp40");
        assert_eq!(
            normalize_filename_key(" Weapon_Rifle_AK "),
            "weapon_rifle_ak"
        );
    }

    #[test]
    fn normalize_extension_set_trims_and_strips_dots() {
        let set = normalize_extension_set(&[".WAV".into(), " akd ".into(), "".into()]);
        assert!(set.contains("wav"));
        assert!(set.contains("akd"));
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn sync_relative_path_is_derived_from_matching_base_path() {
        let base = "\\Actor-Mixer Hierarchy\\Skill_Hit\\GeneralSkills";
        let child = "\\Actor-Mixer Hierarchy\\Skill_Hit\\GeneralSkills\\A\\B";

        assert_eq!(
            relative_path_from_base(base, child),
            Some("A\\B".to_string())
        );
        assert_eq!(parent_relative_path("A\\B"), Some("A".to_string()));
        assert_eq!(parent_relative_path("A"), None);
    }

    #[test]
    fn sync_keys_can_ignore_case_and_optionally_include_type() {
        assert_eq!(sync_path_key("A\\Child", false), "a\\child");
        assert_eq!(sync_path_key("A\\Child", true), "A\\Child");
        assert_eq!(sync_typed_key("A\\Child", "Sound", false), "a\\child|sound");
    }

    #[test]
    fn object_type_matching_treats_wildcard_as_all_types() {
        assert!(matches_any_object_type(Some("Sound"), &[]));
        assert!(matches_any_object_type(Some("Sound"), &["*".into()]));
        assert!(matches_any_object_type(Some("Sound"), &["".into()]));
        assert!(matches_any_object_type(Some("Sound"), &["Sound".into()]));
        assert!(!matches_any_object_type(Some("Sound"), &["Event".into()]));
    }

    #[test]
    fn protected_ids_expand_from_used_root_to_entire_subtree() {
        let directly_used_ids = HashSet::from(["root".to_string()]);
        let parent_by_id = HashMap::from([
            ("root".to_string(), None),
            ("child".to_string(), Some("root".to_string())),
            ("leaf".to_string(), Some("child".to_string())),
        ]);
        let children_by_id = HashMap::from([
            ("root".to_string(), vec!["child".to_string()]),
            ("child".to_string(), vec!["leaf".to_string()]),
        ]);

        let protected_ids =
            collect_protected_ids(&directly_used_ids, &parent_by_id, &children_by_id);

        assert!(protected_ids.contains("root"));
        assert!(protected_ids.contains("child"));
        assert!(protected_ids.contains("leaf"));
        assert_eq!(protected_ids.len(), 3);
    }

    #[test]
    fn protected_ids_keep_ancestors_and_descendants_of_used_child() {
        let directly_used_ids = HashSet::from(["used_child".to_string()]);
        let parent_by_id = HashMap::from([
            ("root".to_string(), None),
            ("unused_sibling".to_string(), Some("root".to_string())),
            ("used_child".to_string(), Some("root".to_string())),
            ("used_leaf".to_string(), Some("used_child".to_string())),
        ]);
        let children_by_id = HashMap::from([
            (
                "root".to_string(),
                vec!["unused_sibling".to_string(), "used_child".to_string()],
            ),
            ("used_child".to_string(), vec!["used_leaf".to_string()]),
        ]);

        let protected_ids =
            collect_protected_ids(&directly_used_ids, &parent_by_id, &children_by_id);

        assert!(protected_ids.contains("root"));
        assert!(protected_ids.contains("used_child"));
        assert!(protected_ids.contains("used_leaf"));
        assert!(!protected_ids.contains("unused_sibling"));
    }

    #[test]
    fn top_level_candidate_skips_descendants_when_parent_also_deleted() {
        let parent_by_id = HashMap::from([
            ("root".to_string(), None),
            ("child".to_string(), Some("root".to_string())),
            ("leaf".to_string(), Some("child".to_string())),
            ("other".to_string(), Some("root".to_string())),
        ]);
        let candidate_ids =
            HashSet::from(["child".to_string(), "leaf".to_string(), "other".to_string()]);

        assert!(is_top_level_candidate(
            "child",
            &candidate_ids,
            &parent_by_id
        ));
        assert!(!is_top_level_candidate(
            "leaf",
            &candidate_ids,
            &parent_by_id
        ));
        assert!(is_top_level_candidate(
            "other",
            &candidate_ids,
            &parent_by_id
        ));
    }

    #[test]
    fn derive_audio_source_activity_marks_only_parent_matching_source_as_active() {
        let objects_by_id = HashMap::from([
            (
                "sound".to_string(),
                json!({
                    "id": "sound",
                    "type": "Sound",
                    "originalWavFilePath": r"E:\Project\Originals\SFX\Weapon\Active.wav",
                    "sound:originalWavFilePath": r"E:\Project\Originals\SFX\Weapon\Active.wav",
                    "sound:convertedWemFilePath": r"E:\Project\.cache\Windows\SFX\Weapon\Active_123.wem"
                }),
            ),
            (
                "active".to_string(),
                json!({
                    "id": "active",
                    "type": "AudioFileSource",
                    "parent": { "id": "sound" },
                    "originalWavFilePath": r"E:\Project\Originals\SFX\Weapon\Active.wav",
                    "sound:originalWavFilePath": r"E:\Project\Originals\SFX\Weapon\Active.wav",
                    "sound:convertedWemFilePath": r"E:\Project\.cache\Windows\SFX\Weapon\Active_123.wem"
                }),
            ),
            (
                "inactive".to_string(),
                json!({
                    "id": "inactive",
                    "type": "AudioFileSource",
                    "parent": { "id": "sound" },
                    "originalWavFilePath": r"E:\Project\Originals\SFX\Weapon\Inactive.wav",
                    "sound:originalWavFilePath": r"E:\Project\Originals\SFX\Weapon\Inactive.wav",
                    "sound:convertedWemFilePath": r"E:\Project\.cache\Windows\SFX\Weapon\Inactive_456.wem"
                }),
            ),
        ]);
        let parent_by_id = HashMap::from([
            ("sound".to_string(), None),
            ("active".to_string(), Some("sound".to_string())),
            ("inactive".to_string(), Some("sound".to_string())),
        ]);

        let activity = derive_audio_source_activity(&objects_by_id, &parent_by_id);

        assert_eq!(activity.get("active"), Some(&true));
        assert_eq!(activity.get("inactive"), Some(&false));
    }

    #[test]
    fn derive_audio_source_activity_ignores_sources_without_parent_paths() {
        let objects_by_id = HashMap::from([
            (
                "sound".to_string(),
                json!({
                    "id": "sound",
                    "type": "Sound"
                }),
            ),
            (
                "audio".to_string(),
                json!({
                    "id": "audio",
                    "type": "AudioFileSource",
                    "parent": { "id": "sound" },
                    "originalWavFilePath": r"E:\Project\Originals\SFX\Weapon\Test.wav"
                }),
            ),
        ]);
        let parent_by_id = HashMap::from([
            ("sound".to_string(), None),
            ("audio".to_string(), Some("sound".to_string())),
        ]);

        let activity = derive_audio_source_activity(&objects_by_id, &parent_by_id);

        assert!(activity.is_empty());
    }

    #[test]
    fn inactive_audio_source_can_escape_protected_parent_branch() {
        let directly_used_ids = HashSet::from(["sound".to_string()]);
        let parent_by_id = HashMap::from([
            ("sound".to_string(), None),
            ("active".to_string(), Some("sound".to_string())),
            ("inactive".to_string(), Some("sound".to_string())),
        ]);
        let children_by_id = HashMap::from([(
            "sound".to_string(),
            vec!["active".to_string(), "inactive".to_string()],
        )]);

        let mut protected_ids =
            collect_protected_ids(&directly_used_ids, &parent_by_id, &children_by_id);
        let inactive_audio_source_ids = HashSet::from(["inactive".to_string()]);
        for object_id in &inactive_audio_source_ids {
            if !directly_used_ids.contains(object_id) {
                protected_ids.remove(object_id);
            }
        }

        assert!(protected_ids.contains("sound"));
        assert!(protected_ids.contains("active"));
        assert!(!protected_ids.contains("inactive"));
    }
}
