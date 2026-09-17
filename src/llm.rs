// wwise-mcp — © 2025-2026 william.wang. All rights reserved.
// 精简 shim：开源版不包含 LLM 客户端，仅保留 tools.rs 依赖的调试日志辅助函数。

use std::io::Write;

pub fn debug_log(msg: &str) {
    let log_path = std::env::temp_dir().join("gwise-agent-debug.log");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let _ = writeln!(f, "[{}] {}", ts, msg);
    }
}
