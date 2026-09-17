// 任务路由契约：用户用自然语言描述的任务，知识库必须点名正确的超级接口。
// 这锁的是"模型会走哪条路"，不是 WAAPI 本身。

fn knowledge() -> String {
    let defs = gwwise_agent_lib::knowledge::tools_definition().to_string();
    format!(
        "{}\n{}",
        gwwise_agent_lib::knowledge::system_prompt(),
        defs
    )
}

/// 公开源码构建不包含完整知识库（base.txt 随官方 Release 二进制分发）。
/// 此时路由契约无从谈起，直接跳过，保证贡献者构建也全绿。
fn full_knowledge_available() -> bool {
    gwwise_agent_lib::knowledge::system_prompt().len() > 5_000
}

fn assert_routes(tool: &str, context_needles: &[&str]) {
    if !full_knowledge_available() {
        eprintln!("skipping routing check: built without the full knowledge base");
        return;
    }
    let hay = knowledge();
    assert!(
        hay.contains(tool),
        "knowledge/defs does not mention the expected tool '{}'",
        tool
    );
    for needle in context_needles {
        assert!(
            hay.to_lowercase().contains(&needle.to_lowercase()),
            "knowledge around '{}' should mention '{}'",
            tool,
            needle
        );
    }
}

macro_rules! route {
    ($test:ident, $tool:expr $(, $needle:expr)* $(,)?) => {
        #[test]
        fn $test() {
            assert_routes($tool, &[$($needle),*]);
        }
    };
}

route!(replace_audio_routes_to_batch_replace, "batch_replace_audio_by_name", "Sound", "originals");
route!(languages_before_import, "get_project_languages", "language");
route!(create_missing_wwise_path, "create_path_if_not_exists", "Folder");
route!(unused_descendants_cleanup, "batch_delete_unused_descendants", "unused");
route!(skill_copy_missing, "batch_copy_missing_descendants", "Skill");
route!(skill_move_missing, "batch_move_missing_descendants", "move");
route!(output_bus_sync, "batch_sync_output_bus_by_relative_path", "OutputBus");
route!(originals_filesystem_cleanup, "cleanup_unused_originals_files", "Originals");
route!(introspect_functions, "waapi_list_functions", "guess");
route!(introspect_schema, "waapi_get_schema", "schema");
route!(introspect_topics, "waapi_list_topics", "topic");
route!(soundbank_generate, "generate_soundbanks", "SoundBank");
route!(soundbank_inclusions, "get_soundbank_inclusions", "inclusion");
route!(profiler_session, "profiler_capture", "profiler");
route!(profiler_busses_are_meter, "profiler_capture", "meter");
route!(remote_discover, "remote_list_consoles", "host");
route!(remote_connect, "remote_connect", "remote");
route!(ui_selection, "ui_get_selection", "selection");
route!(ui_commands, "ui_list_commands", "command");
route!(subscribe_before_generate, "waapi_wait_for_event", "generationDone");
route!(do_not_poll_events, "waapi_poll_events", "wait");
route!(truncated_results_use_total, "_truncated", "TOTAL");
route!(no_chat_tool_spam, "CONVERSATIONAL", "NO tools");
route!(health_check, "wwise_get_info", "getInfo");
route!(audition_play, "transport_play", "audition");
route!(audition_stop, "transport_control", "stop");
route!(never_guess_project_getinfo, "ak.wwise.core.project.getInfo", "do NOT");
route!(waql_for_discovery, "waapi_query", "WAQL");
route!(batch_preferred_over_loops, "batch_replace_audio_by_name", "CRITICAL");
route!(children_by_type, "batch_get_children_by_type", "Sound");
route!(unsubscribe_when_done, "waapi_unsubscribe", "clean");
route!(profiler_enable_key_is_not_enabled, "enableProfilerData", "NOT `enabled`");
