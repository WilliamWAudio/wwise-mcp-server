# wwise-mcp-server — The Most Complete Wwise MCP Server

> **Official repository / 官方仓库**: <https://github.com/WilliamWAudio/wwise-mcp-server> — releases, issues, and updates all live here.

**A Model Context Protocol (MCP) server for Audiokinetic Wwise** — 45 tools covering raw WAAPI calls, WAQL queries, 20 batch super-interfaces, dynamic API introspection, real-time event subscriptions, SoundBank generation, profiler capture, remote game connection, UI automation, and audio audition. Works with **Claude (Desktop / Code), Cursor, Codex, and any MCP client**.

> **Single self-contained executable.** No Python, no Node.js, no virtual environments, no dependencies — download one file and go. The full Wwise knowledge base (300+ lines of battle-tested WAAPI rules) ships inside the binary and is served to your AI client automatically.

## Why this one?

| | wwise-mcp-server (this project) | Typical Wwise MCP |
|---|---|---|
| Tools | **45** across 11 domains | 5–15, mostly raw WAAPI passthrough |
| Batch super-interfaces | **20** (atomic, single-undo, e.g. replace audio sources by name without creating new Sounds) | none |
| Wwise version compatibility | **Dynamic introspection** — queries the connected Wwise for its real schemas; verified on Wwise **2019 – 2025** | hardcoded parameters that break across versions |
| Event subscriptions | **Full WAMP subscribe/poll/wait**, auto-resubscribe on reconnect, ~15 ms delivery | none |
| Context efficiency | ~8K tokens for all 45 schemas + automatic truncation of oversized results | unbounded result dumps |
| Quality assurance | **175 automated tests** (unit + contract + protocol + live E2E against a running Wwise) | usually zero |
| Setup | **Download 1 exe** | Python 3.13 + venv + pip, or Node.js |

## Quick start

### 1. Download

Grab `gwwise-mcp-server.exe` from the [latest Release](../../releases/latest).

Or build from source (Rust 1.75+): `cargo build --release`. Source builds are fully functional (all 45 tools); official release binaries additionally embed the tuned knowledge base — see [Open core](#open-core).

### 2. Enable WAAPI in Wwise

Wwise → **Project > User Preferences → Enable Wwise Authoring API** (default port 8080).

### 3. Configure your MCP client

**Claude Desktop / Claude Code** (`claude_desktop_config.json` or `.mcp.json`):

```json
{
  "mcpServers": {
    "wwise": {
      "command": "C:/path/to/gwwise-mcp-server.exe"
    }
  }
}
```

**Cursor** (`.cursor/mcp.json`):

```json
{
  "mcpServers": {
    "wwise": {
      "command": "C:/path/to/gwwise-mcp-server.exe",
      "args": ["--wwise-port", "8080"]
    }
  }
}
```

**Codex CLI** (`~/.codex/config.toml`):

```toml
[mcp_servers.wwise]
command = "C:/path/to/gwwise-mcp-server.exe"
```

The server starts even if Wwise is not running — it connects automatically on the first tool call.

## Tool catalog (45 tools)

| Domain | Tools |
|---|---|
| Core | `waapi_call` (any WAAPI URI), `waapi_query` (WAQL), `wwise_connect` |
| Local files | `list_local_audio_files`, `create_path_if_not_exists`, `get_project_languages` |
| Batch rename | lowercase / titlecase / uppercase / find-replace / prefix / suffix |
| Batch structure | `batch_delete`, `batch_smart_delete` (reference preflight + preview), `batch_delete_unused_descendants`, `batch_convert_type`, `batch_set_property`, `batch_move`, `batch_get_children_by_type` |
| Structure sync | `batch_copy_missing_descendants`, `batch_move_missing_descendants`, `batch_sync_output_bus_by_relative_path` |
| Audio sources | `batch_replace_audio_by_name` (strict in-place replacement — never creates new Sounds), `cleanup_unused_originals_files` |
| Introspection | `waapi_list_functions`, `waapi_list_topics`, `waapi_get_schema` |
| SoundBank | `generate_soundbanks`, `get_soundbank_inclusions` |
| Profiler | `profiler_capture` (one-call sampling session) |
| Remote | `remote_list_consoles`, `remote_connect`, `remote_disconnect` |
| UI automation | `ui_get_selection`, `ui_list_commands`, `ui_execute_command` |
| Subscriptions | `waapi_subscribe`, `waapi_poll_events`, `waapi_wait_for_event`, `waapi_list_subscriptions`, `waapi_unsubscribe` |
| Health / audition | `wwise_get_info`, `transport_play`, `transport_control`, `transport_list` |

All batch operations are atomic: one undo step in Wwise (Ctrl+Z reverts everything).

## Example prompts

- *"List every Sound under the Weapons folder whose volume is below -10 dB"*
- *"Replace the audio sources of all Sounds under the selected object with the same-name wav files in C:\recordings — do not create new Sounds"*
- *"Rename every object under Enemies to lowercase, then generate the Combat SoundBank"*
- *"Subscribe to object name changes and tell me when anything gets renamed"*
- *"Audition the Play_Footstep event, then stop it"*

## Quality

Every release passes **175 automated tests**: unit, tool-definition contract, natural-language routing, MCP protocol compliance (spawning the real binary over stdio), and live end-to-end tests against running Wwise instances.

Verified on **Wwise 2019 – 2025**, with full end-to-end coverage (real event delivery, audition transport lifecycle, profiler capture) across many Wwise versions from 2019 to 2025. Dynamic WAAPI introspection keeps it working across Wwise versions without updates.

## Architecture

```
MCP client (Claude / Cursor / Codex)
        │  JSON-RPC 2.0 over stdio
gwwise-mcp-server  (single Rust binary, embedded knowledge base)
        │  WebSocket + WAMP
Wwise Authoring  (WAAPI, port 8080)
```

## Privacy & safety

- Talks only to your local Wwise (127.0.0.1:8080 by default) — no telemetry, no network calls to anywhere else.
- Destructive batch operations run reference preflight checks and support preview mode.
- Every batch change is one Wwise undo step.

## Open core

- **Source code (this repo): MIT.** Clone, build, modify, contribute — everything works, including all 45 tools and the full test suite (knowledge-dependent tests skip automatically in source builds).
- **Tuned knowledge base: proprietary, ships in official Release binaries.** 300+ lines of battle-tested WAAPI rules that teach your AI client *when and how* to use these tools well — served automatically via MCP `instructions`. This is why the official binary gives a noticeably smarter experience than a bare source build.

## License

Source code: MIT. Embedded knowledge base: proprietary, free to use via official binaries. See [LICENSE](LICENSE).

© 2025-2026 [william.wang](https://github.com/WilliamWAudio)
