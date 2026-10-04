//! OpenAI Responses 风格客户端（`<base_url>/responses`）。

use std::collections::BTreeMap;

use domain::{
    language::Language,
    llm::{LlmConfig, LlmResponse},
};
use serde::{Deserialize, Serialize};

use crate::{LlmError, StyleClient, fetch, protocol, usage_from};

/// Responses 风格客户端。
pub(crate) struct Client;

#[derive(Serialize)]
struct Request {
    model:        String,
    instructions: String,
    input:        Vec<InputMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature:  Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p:        Option<f64>,
}

#[derive(Serialize)]
struct InputMessage {
    role:    &'static str,
    content: String,
}

#[derive(Deserialize)]
struct Response {
    output: Vec<OutputItem>,
    usage:  Option<Usage>,
}

#[derive(Deserialize)]
struct OutputItem {
    #[serde(rename = "type")]
    kind:    String,
    content: Option<Vec<ContentPart>>,
}

#[derive(Deserialize)]
struct ContentPart {
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
            model:        config.model.clone(),
            instructions: protocol::build_system(source, target),
            input:        vec![InputMessage {
                role:    "user",
                content: protocol::build_user(&segments),
            }],
            temperature:  config.temperature,
            top_p:        config.top_p,
        };
        let url = format!("{}/responses", config.base_url);
        let parsed: Response = fetch(http.post(&url).bearer_auth(&config.api_key), Some(&body)).await?;

        let reply = parsed
            .output
            .iter()
            .filter(|o| o.kind == "message")
            .filter_map(|o| o.content.as_ref())
            .flatten()
            .filter(|p| matches!(p.kind.as_str(), "output_text" | "text"))
            .filter_map(|p| p.text.clone())
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
        let parsed: crate::ModelsResponse = fetch(http.get(&url).bearer_auth(&config.api_key), None::<&()>).await?;
        Ok(parsed.data.into_iter().map(|m| m.id).collect())
    }
}

#[cfg(test)]
mod tests {
    use domain::llm::ApiStyle;

    use super::*;

    fn config(base_url: String) -> LlmConfig {
        LlmConfig {
            api_style: ApiStyle::Response,
            base_url,
            api_key: "k".into(),
            model: "m".into(),
            temperature: Some(0.2),
            top_p: None,
            top_k: None,
        }
    }

    fn segments() -> BTreeMap<u64, String> {
        BTreeMap::from([(1, "hello".into())])
    }

    #[tokio::test]
    async fn translate_responses_style() {
        let (addr, mut rx) = test_support::spawn(
            r#"{"output":[{"type":"reasoning","content":[]},{"type":"message","content":[{"type":"output_text","text":"{\"1\":\"こんにちは\"}"}]}],"usage":{"input_tokens":3,"output_tokens":4}}"#,
            200,
        )
        .await;
        let http = reqwest::Client::new();
        let out = Client
            .translate(&config(addr), &http, Language::ZhHans, Language::Ja, segments())
            .await
            .unwrap();

        let req = rx.recv().await.unwrap();
        assert_eq!(req.path, "/responses");
        assert_eq!(req.headers["authorization"], "Bearer k");
        let body: serde_json::Value = serde_json::from_str(&req.body).unwrap();
        assert_eq!(body["model"], "m");
        assert!(
            body["instructions"]
                .as_str()
                .unwrap()
                .contains("从简体中文翻译成日语")
        );
        assert_eq!(body["input"][0]["role"], "user");
        assert_eq!(body["input"][0]["content"], r#"{"1":"hello"}"#);
        assert_eq!(body["temperature"], 0.2);

        assert_eq!(out.segments.get(&1).unwrap(), "こんにちは");
        assert_eq!(out.usage.input, 3);
        assert_eq!(out.usage.output, 4);
    }
}
