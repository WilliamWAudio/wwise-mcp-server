// GWwiseAgent Knowledge Module — © 2025-2026 william.wang

include!(concat!(env!("OUT_DIR"), "/knowledge_enc.rs"));

fn xor_decrypt(data: &[u8], key: &[u8]) -> Vec<u8> {
    data.iter()
        .enumerate()
        .map(|(i, &b)| b ^ key[i % key.len()])
        .collect()
}

pub fn system_prompt() -> String {
    if ENC_PROMPT.is_empty() {
        return FALLBACK_PROMPT.to_string();
    }
    let decrypted = xor_decrypt(ENC_PROMPT, ENC_KEY);
    String::from_utf8(decrypted).expect("Invalid UTF-8 in decrypted prompt")
}

pub fn tools_definition() -> serde_json::Value {
    if ENC_DEFS.is_empty() {
        return serde_json::from_str(FALLBACK_DEFS).expect("Invalid fallback defs");
    }
    let decrypted = xor_decrypt(ENC_DEFS, ENC_KEY);
    let json_str = String::from_utf8(decrypted).expect("Invalid UTF-8 in decrypted defs");
    serde_json::from_str(&json_str).expect("Invalid JSON in decrypted defs")
}

const FALLBACK_PROMPT: &str = r#"You are GWwiseAgent, an AI assistant for Audiokinetic Wwise. Control Wwise via WAAPI tools.
Respond in 简体中文. Use English only for Wwise terms, URIs, object names.
Use waapi_query to discover objects before acting. Use waapi_call to execute WAAPI functions."#;

const FALLBACK_DEFS: &str = r#"[
  {"type":"function","function":{"name":"waapi_call","description":"Execute a WAAPI function.","parameters":{"type":"object","properties":{"uri":{"type":"string","description":"WAAPI URI"},"args":{"type":"object","description":"Arguments"},"options":{"type":"object","description":"Options"}},"required":["uri","args"]}}},
  {"type":"function","function":{"name":"waapi_query","description":"Query Wwise objects via WAQL.","parameters":{"type":"object","properties":{"waql":{"type":"string","description":"WAQL query"},"return_fields":{"type":"array","items":{"type":"string"},"description":"Fields to return"}},"required":["waql"]}}}
]"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_prompt_is_the_embedded_knowledge_base_not_the_tiny_fallback() {
        // 公开源码构建不含完整知识库（随官方 Release 二进制分发），跳过此断言。
        if ENC_PROMPT.is_empty() {
            eprintln!("skipping: built without the full knowledge base");
            return;
        }
        let prompt = system_prompt();
        assert!(
            prompt.len() > 10_000,
            "embedded prompt looks truncated: {} chars",
            prompt.len()
        );
        for marker in [
            "CRITICAL Rules",
            "waapi_list_functions",
            "waapi_get_schema",
            "batch_replace_audio_by_name",
            "transport_play",
            "_truncated",
        ] {
            assert!(
                prompt.contains(marker),
                "system prompt missing expected section: {}",
                marker
            );
        }
    }

    #[test]
    fn tools_definition_is_an_array_of_functions() {
        let defs = tools_definition();
        let arr = defs.as_array().expect("defs must be an array");
        assert!(arr.len() >= 40, "expected at least 40 tools, got {}", arr.len());
        for def in arr {
            let func = def.get("function").expect("each entry needs function");
            assert_eq!(def.get("type").and_then(|v| v.as_str()), Some("function"));
            assert!(func.get("name").and_then(|v| v.as_str()).unwrap_or("").len() > 2);
            assert_eq!(
                func.get("parameters").and_then(|p| p.get("type")).and_then(|t| t.as_str()),
                Some("object")
            );
        }
    }

    #[test]
    fn xor_decrypt_roundtrips_ascii() {
        let key = b"garena";
        let plain = b"hello-wwise";
        let enc: Vec<u8> = plain
            .iter()
            .enumerate()
            .map(|(i, &b)| b ^ key[i % key.len()])
            .collect();
        let dec = xor_decrypt(&enc, key);
        assert_eq!(dec, plain);
    }
}
