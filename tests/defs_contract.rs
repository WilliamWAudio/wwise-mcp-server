// 工具契约：defs.json 里的每一个工具都必须有合法 schema，
// 且这次新增的域（自省/Bank/Profiler/远程/UI/订阅/试听）全部在清单里。

use serde_json::Value;

fn defs() -> Vec<Value> {
    gwwise_agent_lib::knowledge::tools_definition()
        .as_array()
        .cloned()
        .expect("defs must be an array")
}

fn names() -> Vec<String> {
    defs()
        .iter()
        .filter_map(|d| {
            d.get("function")
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str())
                .map(String::from)
        })
        .collect()
}

#[test]
fn every_definition_is_a_function_with_object_parameters() {
    for def in defs() {
        assert_eq!(def.get("type").and_then(|v| v.as_str()), Some("function"));
        let func = def.get("function").expect("missing function");
        let name = func.get("name").and_then(|v| v.as_str()).unwrap_or("");
        assert!(name.len() >= 3, "empty tool name");
        let desc = func.get("description").and_then(|v| v.as_str()).unwrap_or("");
        assert!(
            desc.len() >= 20,
            "tool '{}' description is too short ({} chars)",
            name,
            desc.len()
        );
        let params = func.get("parameters").expect("missing parameters");
        assert_eq!(params.get("type").and_then(|v| v.as_str()), Some("object"));
        assert!(params.get("properties").is_some());
    }
}

#[test]
fn required_fields_exist_in_properties() {
    for def in defs() {
        let func = &def["function"];
        let name = func["name"].as_str().unwrap_or("?");
        let params = &func["parameters"];
        let required = params
            .get("required")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let properties = params
            .get("properties")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        for req in required {
            let key = req.as_str().unwrap_or("");
            assert!(
                properties.contains_key(key),
                "tool '{}' requires '{}' but it is not in properties",
                name,
                key
            );
        }
    }
}

#[test]
fn tool_names_are_unique_in_defs() {
    let mut names = names();
    let before = names.len();
    names.sort();
    names.dedup();
    assert_eq!(before, names.len(), "duplicate names in defs.json");
}

macro_rules! requires_tool {
    ($test:ident, $name:expr) => {
        #[test]
        fn $test() {
            assert!(
                names().iter().any(|n| n == $name),
                "missing tool definition: {}",
                $name
            );
        }
    };
}

requires_tool!(has_waapi_call, "waapi_call");
requires_tool!(has_waapi_query, "waapi_query");
requires_tool!(has_waapi_list_functions, "waapi_list_functions");
requires_tool!(has_waapi_get_schema, "waapi_get_schema");
requires_tool!(has_waapi_list_topics, "waapi_list_topics");
requires_tool!(has_generate_soundbanks, "generate_soundbanks");
requires_tool!(has_get_soundbank_inclusions, "get_soundbank_inclusions");
requires_tool!(has_profiler_capture, "profiler_capture");
requires_tool!(has_remote_list_consoles, "remote_list_consoles");
requires_tool!(has_remote_connect, "remote_connect");
requires_tool!(has_remote_disconnect, "remote_disconnect");
requires_tool!(has_ui_get_selection, "ui_get_selection");
requires_tool!(has_ui_list_commands, "ui_list_commands");
requires_tool!(has_ui_execute_command, "ui_execute_command");
requires_tool!(has_waapi_subscribe, "waapi_subscribe");
requires_tool!(has_waapi_poll_events, "waapi_poll_events");
requires_tool!(has_waapi_wait_for_event, "waapi_wait_for_event");
requires_tool!(has_waapi_list_subscriptions, "waapi_list_subscriptions");
requires_tool!(has_waapi_unsubscribe, "waapi_unsubscribe");
requires_tool!(has_batch_replace_audio_by_name, "batch_replace_audio_by_name");
requires_tool!(has_batch_smart_delete, "batch_smart_delete");
requires_tool!(has_get_project_languages, "get_project_languages");
requires_tool!(has_wwise_get_info, "wwise_get_info");
requires_tool!(has_transport_play, "transport_play");
requires_tool!(has_transport_control, "transport_control");
requires_tool!(has_transport_list, "transport_list");
requires_tool!(has_list_local_audio_files, "list_local_audio_files");
requires_tool!(has_create_path_if_not_exists, "create_path_if_not_exists");
requires_tool!(has_cleanup_unused_originals_files, "cleanup_unused_originals_files");
requires_tool!(has_batch_get_children_by_type, "batch_get_children_by_type");
requires_tool!(has_batch_copy_missing_descendants, "batch_copy_missing_descendants");
requires_tool!(has_batch_move_missing_descendants, "batch_move_missing_descendants");
requires_tool!(has_batch_sync_output_bus_by_relative_path, "batch_sync_output_bus_by_relative_path");
requires_tool!(has_batch_delete_unused_descendants, "batch_delete_unused_descendants");
requires_tool!(has_batch_rename_find_replace, "batch_rename_find_replace");
requires_tool!(has_batch_set_property, "batch_set_property");
requires_tool!(has_batch_move, "batch_move");
requires_tool!(has_batch_convert_type, "batch_convert_type");
