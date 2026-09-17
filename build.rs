// wwise-mcp build script — © 2025-2026 william.wang
// Embeds the knowledge base (system prompt + tool definitions) into the binary
// so the MCP server ships as a single self-contained executable.

use std::io::Write;

fn derive_key(seed: &str) -> Vec<u8> {
    let mut key = vec![0u8; 32];
    let seed_bytes = seed.as_bytes();
    for (i, &b) in seed_bytes.iter().enumerate() {
        key[i % 32] ^= b;
        key[(i + 7) % 32] = key[(i + 7) % 32].wrapping_add(b).wrapping_mul(31);
    }
    for b in key.iter_mut() {
        if *b == 0 {
            *b = 0xAB;
        }
    }
    key
}

fn xor_encrypt(data: &[u8], key: &[u8]) -> Vec<u8> {
    data.iter()
        .enumerate()
        .map(|(i, &b)| b ^ key[i % key.len()])
        .collect()
}

fn main() {
    let build_ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    println!("cargo:rustc-env=GWA_BUILD_TS={}", build_ts);

    let prompt = std::fs::read("knowledge/base.txt").unwrap_or_default();
    let defs = std::fs::read("knowledge/defs.json").unwrap_or_default();

    if prompt.is_empty() {
        println!("cargo:warning=knowledge/base.txt not found or empty — building with fallback prompt");
    }
    if defs.is_empty() {
        println!("cargo:warning=knowledge/defs.json not found or empty — building with fallback tools");
    }

    let key = derive_key(&build_ts);
    let enc_prompt = xor_encrypt(&prompt, &key);
    let enc_defs = xor_encrypt(&defs, &key);

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let out_path = std::path::Path::new(&out_dir);

    std::fs::write(out_path.join("prompt.enc"), &enc_prompt).unwrap();
    std::fs::write(out_path.join("defs.enc"), &enc_defs).unwrap();
    std::fs::write(out_path.join("enc.key"), &key).unwrap();

    let mut f = std::fs::File::create(out_path.join("knowledge_enc.rs")).unwrap();
    writeln!(
        f,
        "const ENC_PROMPT: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/prompt.enc\"));"
    )
    .unwrap();
    writeln!(
        f,
        "const ENC_DEFS: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/defs.enc\"));"
    )
    .unwrap();
    writeln!(
        f,
        "const ENC_KEY: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/enc.key\"));"
    )
    .unwrap();

    println!("cargo:rerun-if-changed=knowledge/base.txt");
    println!("cargo:rerun-if-changed=knowledge/defs.json");
}
