use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// LLM API 风格：协议差异决定请求/响应结构。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ApiStyle {
    /// Anthropic Messages：`<base_url>/messages`
    Message,
    /// OpenAI Responses：`<base_url>/responses`
    Response,
    /// OpenAI Chat Completions：`<base_url>/chat/completions`
    Completion,
}

/// LLM provider 配置（config.toml 的 `[providers.<name>]` 表）。
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct LlmConfig {
    pub api_style:   ApiStyle,
    pub base_url:    String,
    pub api_key:     String,
    pub model:       String,
    /// 采样温度，浮点；OpenAI 为 0-2、Anthropic 为 0-1，按 API 原生语义透传。
    pub temperature: Option<f64>,
    /// 采样概率阈值（nucleus sampling），0-1 浮点，按 API 原生语义透传。
    pub top_p:       Option<f64>,
    /// 采样候选数，正整数；仅 Anthropic Messages 风格支持。
    pub top_k:       Option<u16>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
pub struct LlmUsage {
    pub input:  u64,
    pub output: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LlmResponse {
    /// 译文编号段：键与请求一致，值为对应编号的译文。
    pub segments: BTreeMap<u64, String>,
    pub usage:    LlmUsage,
}
