//! Anthropic Messages 风格客户端（`<base_url>/messages`）。

use std::collections::BTreeMap;

use domain::{
    language::Language,
    llm::{LlmConfig, LlmResponse},
};
use serde::{Deserialize, Serialize};

use crate::{LlmError, StyleClient, fetch, protocol, usage_from};

/// Messages 风格客户端。
pub(crate) struct Client;

#[derive(Serialize)]
struct Request {
    model:       String,
    max_tokens:  u32,
    system:      String,
    messages:    Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p:       Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_k:       Option<u16>,
}

#[derive(Serialize)]
struct Message {
    role:    &'static str,
    content: String,
}

#[derive(Deserialize)]
struct Response {
    content: Vec<ContentBlock>,
    usage:   Option<Usage>,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
}

#[derive(Deserialize)]
struct Usage {
    input_tokens:  Option<u64>,
    output_tokens: Option<u64>,
}

impl StyleClient for Client {
    async fn translate(
        &self,
        config: &LlmConfig,
        http: &reqwest::Client,
        source: Language,
        target: Language,
        segments: BTreeMap<u64, String>,
    ) -> Result<LlmResponse, LlmError> {
        let body = Request {
            model:       config.model.clone(),
            max_tokens:  protocol::estimate_max_tokens(&segments),
            system:      protocol::build_system(source, target),
            messages:    vec![Message {
                role:    "user",
                content: protocol::build_user(&segments),
            }],
            temperature: config.temperature,
            top_p:       config.top_p,
            top_k:       config.top_k,
        };
        let url = format!("{}/messages", config.base_url);
        let parsed: Response = fetch(
            http.post(&url)
                .header("x-api-key", &config.api_key)
                .header("anthropic-version", "2023-06-01"),
            Some(&body),
        )
        .await?;

        let reply = parsed
            .content
            .iter()
            .filter(|b| b.kind == "text")
            .filter_map(|b| b.text.clone())
            .collect::<Vec<_>>()
            .join("\n");
        let segments = protocol::parse_reply(&segments, &reply)?;
        let usage = parsed
            .usage
            .map_or_else(Default::default, |u| usage_from(u.input_tokens, u.output_tokens));
        Ok(LlmResponse { segments, usage })
    }

    async fn models(&self, config: &LlmConfig, http: &reqwest::Client) -> Result<Vec<String>, LlmError> {
        let url = format!("{}/models", config.base_url);
        let parsed: crate::ModelsResponse = fetch(
            http.get(&url)
                .header("x-api-key", &config.api_key)
                .header("anthropic-version", "2023-06-01"),
            None::<&()>,
        )
        .await?;
        Ok(parsed.data.into_iter().map(|m| m.id).collect())
    }
}

#[cfg(test)]
mod tests {
    use domain::llm::ApiStyle;

    use super::*;

    fn config(base_url: String) -> LlmConfig {
        LlmConfig {
            api_style: ApiStyle::Message,
            base_url,
            api_key: "k".into(),
            model: "m".into(),
            temperature: Some(0.5),
            top_p: None,
            top_k: Some(3),
        }
    }

    fn segments() -> BTreeMap<u64, String> {
        BTreeMap::from([(1, "hello".into()), (2, "thanks".into())])
    }

    #[tokio::test]
    async fn translate_messages_style() {
        let (addr, mut rx) = test_support::spawn(
            r#"{"content":[{"type":"text","text":"{\"1\":\"こんにちは\",\"2\":\"ありがとう\"}"}],"usage":{"input_tokens":10,"output_tokens":20}}"#,
            200,
        )
        .await;
        let http = reqwest::Client::new();
        let out = Client
            .translate(&config(addr), &http, Language::ZhHans, Language::Ja, segments())
            .await
            .unwrap();

        let req = rx.recv().await.unwrap();
        assert_eq!(req.path, "/messages");
        assert_eq!(req.headers["x-api-key"], "k");
        assert_eq!(req.headers["anthropic-version"], "2023-06-01");
        let body: serde_json::Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(body["model"], "m");
        assert_eq!(body["max_tokens"], 1024);
        let system = body["system"].as_str().unwrap();
        assert!(system.starts_with("把以下文本从简体中文翻译成日语"));
        assert!(system.contains("JSON 对象"));
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], r#"{"1":"hello","2":"thanks"}"#);
        assert_eq!(body["temperature"], 0.5);
        assert_eq!(body["top_k"], 3);
        assert!(body.get("top_p").is_none());

        assert_eq!(out.segments.get(&1).unwrap(), "こんにちは");
        assert_eq!(out.segments.get(&2).unwrap(), "ありがとう");
        assert_eq!(out.usage.input, 10);
        assert_eq!(out.usage.output, 20);
    }

    #[tokio::test]
    async fn skips_non_text_blocks() {
        let (addr, _rx) = test_support::spawn(
            r#"{"content":[{"type":"thinking","text":"内部思考"},{"type":"text","text":"{\"1\":\"hi\"}"}],"usage":null}"#,
            200,
        )
        .await;
        let http = reqwest::Client::new();
        let out = Client
            .translate(&config(addr), &http, Language::ZhHans, Language::Ja, segments())
            .await
            .unwrap();
        assert_eq!(out.segments.get(&1).unwrap(), "hi");
        assert_eq!(out.usage.input, 0);
    }

    #[tokio::test]
    async fn api_error_carries_status() {
        let (addr, _rx) = test_support::spawn(r#"{"error":"overloaded"}"#, 429).await;
        let http = reqwest::Client::new();
        let err = Client
            .translate(&config(addr), &http, Language::ZhHans, Language::Ja, segments())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("429"), "{err}");
    }
}
