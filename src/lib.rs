// wwise-mcp — The most complete Wwise MCP server
// © 2025-2026 william.wang. All rights reserved.
// Licensed under the MIT License.

pub mod advanced_ops;
pub mod batch_ops;
pub mod knowledge;
pub mod llm;
pub mod tools;
pub mod waapi;

/// 作者署名（公开水印）。
pub const AUTHOR: &str = "william.wang";

const GWA_SIGNATURE: &str = concat!(
    "GWwiseAgent v",
    env!("CARGO_PKG_VERSION"),
    " | author:william.wang",
    " | built:",
    env!("GWA_BUILD_TS"),
);

/// 公开签名字符串（版本 + 作者 + 构建时间）。
pub fn signature() -> &'static str {
    GWA_SIGNATURE
}

// 隐藏水印：XOR(0x5A) 编码的作者信息。即使可见字符串被剥离/替换，
// 仍可通过解码此字节序列证明原始作者身份。
static WATERMARK_ENC: [u8; 62] = [
    29, 13, 45, 51, 41, 63, 27, 61, 63, 52, 46, 119, 53, 40, 51, 61, 51, 52, 59, 54, 119, 59, 47,
    46, 50, 53, 40, 96, 45, 51, 54, 54, 51, 59, 55, 116, 45, 59, 52, 61, 96, 29, 59, 40, 63, 52,
    59, 9, 53, 47, 52, 62, 96, 104, 106, 104, 111, 119, 104, 106, 104, 108,
];

/// 解码隐藏水印。用于验证二进制归属。
/// 密钥经过 black_box，阻止编译器常量折叠——确保二进制里保留的是
/// XOR 编码字节而非解码后的明文。
pub fn hidden_watermark() -> String {
    let key = std::hint::black_box(0x5Au8);
    let enc = std::hint::black_box(&WATERMARK_ENC);
    let decoded: Vec<u8> = enc.iter().map(|b| b ^ key).collect();
    String::from_utf8(decoded).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    #[test]
    fn author_constant_is_correct() {
        assert_eq!(crate::AUTHOR, "william.wang");
    }

    #[test]
    fn signature_embeds_author_and_version() {
        let sig = crate::signature();
        assert!(sig.contains("william.wang"), "signature: {}", sig);
        assert!(sig.contains(env!("CARGO_PKG_VERSION")), "signature: {}", sig);
    }

    #[test]
    fn hidden_watermark_decodes_to_author_identity() {
        let wm = crate::hidden_watermark();
        assert!(wm.contains("william.wang"), "watermark: {}", wm);
        assert!(wm.contains("GWwiseAgent-original-author"), "watermark: {}", wm);
    }
}
