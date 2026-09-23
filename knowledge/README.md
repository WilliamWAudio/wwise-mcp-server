# Knowledge directory

- `defs.json` — the OpenAI-function-calling definitions for all 45 tools (open source).
- `base.txt` — **not included in this repository.** This is the tuned Wwise knowledge base (battle-tested WAAPI rules, batch-tool routing guidance, and error-recovery strategies) that gets embedded into official release binaries and served to your AI client via MCP `instructions`.

## What this means for source builds

Building from source works fully — all 45 tools function identically. The build script simply embeds a minimal fallback prompt instead of the full knowledge base, and knowledge-dependent tests skip automatically.

For the complete experience (the knowledge base is what teaches your AI *when and how* to use these tools well), download the official binary from [Releases](../../../releases/latest).

The knowledge base is proprietary: © 2025-2026 william.wang, all rights reserved. See [LICENSE](../LICENSE).
