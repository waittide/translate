//! 示例：连接本地 OpenAI 兼容翻译服务（vLLM / one-api / new-api 等）。
//!
//! ```sh
//! # 模型名缺省时自动取 `/models` 的第一个
//! cargo run -p llm --example translate [模型名]
//! cargo run -p llm --example translate deepseek-v4-flash
//! ```
//!
//! kind 按模型自动选择：`claude*` 走 Anthropic Messages（`/v1/messages`），
//! 其余走 OpenAI Chat Completions（`/v1/chat/completions`）。也可手动指定。

use std::collections::BTreeMap;

use anyhow::Result;
use domain::{
    language::Language,
    llm::{ApiStyle, LlmConfig},
};
use llm::LlmClient;

fn config(model: &str) -> LlmConfig {
    // 按模型家族选择 API 风格；自己的服务端若统一支持某一风格，直接写死对应 api_style 即可。
    let api_style = if model.starts_with("claude") {
        ApiStyle::Message
    } else {
        ApiStyle::Completion
    };
    LlmConfig {
        api_style,
        base_url: "http://127.0.0.1:13000/v1".to_string(),
        // 本地网关密钥；也可用环境变量 TRANSLATE_API_KEY 覆盖。
        api_key: std::env::var("TRANSLATE_API_KEY").unwrap_or_else(|_| "ahsisnwk1234".to_string()),
        model: model.to_string(),
        temperature: Some(0.3),
        top_p: None,
        top_k: None,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // 模型名：优先命令行参数，否则从 /models 自动选择。
    let arg_model = std::env::args().nth(1).unwrap_or_default();
    let probe = LlmClient::new(config(""));
    let models = probe.models().await?;
    println!("可用模型: {models:?}");
    let model = if arg_model.is_empty() {
        models
            .first()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("未取到模型：请用命令行参数指定模型名"))?
    } else {
        arg_model
    };
    println!("使用模型: {model}\n");

    let client = LlmClient::new(config(&model));

    let segments = BTreeMap::from([
        (1, "今天天气真好，我们出去走走吧。".to_string()),
        (2, "好啊，去河边怎么样？".to_string()),
        (3, "听你的。".to_string()),
    ]);
    let resp = client
        .translate(Language::ZhHans, Language::Ja, segments, None)
        .await?;
    println!("{:#?}", resp);
    Ok(())
}
