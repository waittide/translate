//! LLM 客户端，按 [`ApiStyle`] 分发到不同 API 风格：
//! - [`message`]: Anthropic Messages 风格 `<base_url>/messages`
//! - [`response`]: OpenAI Responses 风格 `<base_url>/responses`
//! - [`completion`]: OpenAI Chat Completions 风格 `<base_url>/chat/completions`
//!
//! 各风格协议差异（请求/响应结构、鉴权 header）封装在对应模块的 [`StyleClient`] 实现里。

use std::{collections::BTreeMap, time::Duration};

use domain::{
    language::Language,
    llm::{ApiStyle, LlmConfig, LlmResponse, LlmUsage},
};
use reqwest::StatusCode;
use serde::Deserialize;
use thiserror::Error;

mod completion;
mod message;
mod response;

mod protocol;

/// 单次请求超时：批量翻译可能耗时较长。
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

/// 具体 API 风格的客户端接口：协议差异（请求结构、鉴权、响应提取）在实现内。
/// 注意：async fn 使 trait 无法用作 dyn 对象，分发走 [`ApiClient`] 枚举聚合。
pub(crate) trait StyleClient: Send + Sync {
    async fn translate(
        &self,
        config: &LlmConfig,
        http: &reqwest::Client,
        source: Language,
        target: Language,
        segments: BTreeMap<u64, String>,
    ) -> Result<LlmResponse, LlmError>;

    async fn models(&self, config: &LlmConfig, http: &reqwest::Client) -> Result<Vec<String>, LlmError>;
}

/// 具体风格客户端聚合：按 [`ApiStyle`] 静态分发。
pub(crate) enum ApiClient {
    Message(message::Client),
    Response(response::Client),
    Completion(completion::Client),
}

impl ApiClient {
    async fn translate(
        &self,
        config: &LlmConfig,
        http: &reqwest::Client,
        source: Language,
        target: Language,
        segments: BTreeMap<u64, String>,
    ) -> Result<LlmResponse, LlmError> {
        match self {
            ApiClient::Message(c) => c.translate(config, http, source, target, segments).await,
            ApiClient::Response(c) => c.translate(config, http, source, target, segments).await,
            ApiClient::Completion(c) => c.translate(config, http, source, target, segments).await,
        }
    }

    async fn models(&self, config: &LlmConfig, http: &reqwest::Client) -> Result<Vec<String>, LlmError> {
        match self {
            ApiClient::Message(c) => c.models(config, http).await,
            ApiClient::Response(c) => c.models(config, http).await,
            ApiClient::Completion(c) => c.models(config, http).await,
        }
    }
}

/// LLM 请求错误。
#[derive(Debug, Error)]
pub enum LlmError {
    #[error("http 请求失败: {0}")]
    Http(#[from] reqwest::Error),
    #[error("api 返回 {status}: {body}")]
    Api { status: StatusCode, body: String },
    #[error("解析响应失败: {0}")]
    Parse(String),
    #[error("请求超时")]
    Timeout,
}

/// 统一的 LLM 客户端，按 [`ApiStyle`] 分发到具体 API 风格。
pub struct LlmClient {
    config: LlmConfig,
    http:   reqwest::Client,
    style:  ApiClient,
}

#[derive(Deserialize)]
struct ModelsResponse {
    data: Vec<Model>,
}

#[derive(Deserialize)]
struct Model {
    id: String,
}

impl LlmClient {
    pub fn new(config: LlmConfig) -> Self {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("构建 HTTP 客户端失败");
        let style = match config.api_style {
            ApiStyle::Message => ApiClient::Message(message::Client),
            ApiStyle::Response => ApiClient::Response(response::Client),
            ApiStyle::Completion => ApiClient::Completion(completion::Client),
        };
        Self { config, http, style }
    }

    /// 单次翻译请求。`timeout` 为 None 时不限时（单发场景）；批量任务传入约束。
    pub async fn translate(
        &self,
        source: Language,
        target: Language,
        segments: BTreeMap<u64, String>,
        timeout: Option<Duration>,
    ) -> Result<LlmResponse, LlmError> {
        if segments.is_empty() {
            return Err(LlmError::Parse("上下文为空".into()));
        }
        let fut = self
            .style
            .translate(&self.config, &self.http, source, target, segments);
        match timeout {
            Some(t) => tokio::time::timeout(t, fut)
                .await
                .map_err(|_| LlmError::Timeout)?,
            None => fut.await,
        }
    }

    /// 模型列表。
    pub async fn models(&self) -> Result<Vec<String>, LlmError> {
        self.style.models(&self.config, &self.http).await
    }
}

/// token 用量换算：两种命名（prompt/completion、input/output）统一为 [`LlmUsage`]。
pub(crate) fn usage_from(input: Option<u64>, output: Option<u64>) -> LlmUsage {
    LlmUsage {
        input:  input.unwrap_or(0),
        output: output.unwrap_or(0),
    }
}

pub(crate) fn parse_err(err: serde_json::Error, text: &str) -> LlmError {
    let snippet: String = text.chars().take(500).collect();
    LlmError::Parse(format!("{err}: {snippet}"))
}

/// 发送请求、校验状态码并反序列化响应；`body` 为 None 时不带请求体（如 GET /models）。
pub(crate) async fn fetch<T: serde::de::DeserializeOwned>(
    req: reqwest::RequestBuilder,
    body: Option<&impl serde::Serialize>,
) -> Result<T, LlmError> {
    let resp = match body {
        Some(b) => req.json(b).send().await?,
        None => req.send().await?,
    };
    let status = resp.status();
    let text = resp.text().await?;
    if !status.is_success() {
        return Err(LlmError::Api { status, body: text });
    }
    serde_json::from_str(&text).map_err(|e| parse_err(e, &text))
}

#[cfg(test)]
mod tests {

    use super::*;

    fn config(base_url: String) -> LlmConfig {
        LlmConfig {
            api_style: ApiStyle::Completion,
            base_url,
            api_key: "k".into(),
            model: "m".into(),
            temperature: None,
            top_p: None,
            top_k: None,
        }
    }

    #[tokio::test]
    async fn lists_models() {
        let (addr, _rx) =
            test_support::spawn(r#"{"object":"list","data":[{"id":"model-a"},{"id":"model-b"}]}"#, 200).await;
        let client = LlmClient::new(config(addr));
        assert_eq!(
            client.models().await.unwrap(),
            vec!["model-a".to_string(), "model-b".to_string()]
        );
    }

    #[tokio::test]
    async fn models_failure_returns_error() {
        let (addr, _rx) = test_support::spawn(r#"{"error":"boom"}"#, 500).await;
        let client = LlmClient::new(config(addr));
        assert!(client.models().await.is_err());
    }
}
