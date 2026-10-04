//! web 接口集成测试：创建工程 → 运行 → 实时事件 → 暂停/恢复 → 重试 → provider CRUD。

use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU32, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use config::{Config, ServerConfig};
use domain::llm::{ApiStyle, LlmConfig};
use tokio_stream::StreamExt;
use web::{AppState, router};

/// 进程内递增序号：临时目录名唯一性来源（并行测试互不碰撞）。
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// 临时目录：`<tag>-<pid>-<seq>`，进程内唯一。
fn unique_dir(tag: &str) -> PathBuf {
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("{tag}-{}-{seq}", std::process::id()))
}

fn llm_config(base_url: String) -> LlmConfig {
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

/// 起服务 + 注入临时配置文件路径；返回 base_url。
async fn spawn_server(providers: BTreeMap<String, LlmConfig>) -> (String, PathBuf) {
    let dir = unique_dir("web-test");
    std::fs::create_dir_all(&dir).unwrap();
    let config_path = dir.join("config.toml");
    let state = AppState::with_config_path(
        Config {
            providers,
            server: ServerConfig::default(),
            recent_projects: Default::default(),
        },
        config_path.clone(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router(state)).await.unwrap();
    });
    (format!("http://{addr}"), config_path)
}

fn temp_file(name: &str, content: &str) -> (PathBuf, String) {
    let dir = unique_dir("web-test-file");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    (dir, path.to_string_lossy().into_owned())
}

/// 轮询当前工程直到 run_status 达到期望值。
async fn wait_status(base: &str, expected: &str, timeout: Duration) -> serde_json::Value {
    let deadline = Instant::now() + timeout;
    loop {
        let resp = reqwest::get(format!("{base}/api/projects/current"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200, "GET current 失败");
        let v: serde_json::Value = resp.json().await.unwrap();
        if v["info"]["run_status"].as_str() == Some(expected) {
            return v;
        }
        assert!(
            Instant::now() < deadline,
            "等待 {expected} 超时，当前 {:?}",
            v["info"]["run_status"]
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// 创建工程；消费 SSE 事件流直到 `done`（携带 ProjectView）或 `error`。
async fn create_project(base: &str, path: &str) -> serde_json::Value {
    let body = serde_json::json!({
        "object": {"type": "file", "path": path},
        "source": "zh-Hans",
        "target": "ja",
        "project_config": {
            "task_limit": 2,
            "retry_limit": 1,
            "task_timeout_ms": 5000,
            "provider": "test"
        }
    });
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/projects"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "创建工程失败: {}", resp.text().await.unwrap());
    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    loop {
        match tokio::time::timeout(Duration::from_secs(10), stream.next()).await {
            Ok(Some(Ok(chunk))) => buf.push_str(&String::from_utf8_lossy(&chunk)),
            other => panic!("创建流未返回 done 事件: {other:?}，已收到: {buf}"),
        }
        for block in buf.split("\n\n") {
            if block.contains("event: done") {
                for line in block.lines() {
                    if let Some(data) = line.strip_prefix("data: ") {
                        return serde_json::from_str(data).unwrap();
                    }
                }
            }
            if block.contains("event: error") {
                panic!("创建工程失败: {block}");
            }
        }
    }
}

async fn run_action(base: &str, action: &str) -> reqwest::StatusCode {
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/projects/current/run"))
        .json(&serde_json::json!({ "action": action }))
        .send()
        .await
        .unwrap();
    resp.status()
}

#[tokio::test]
async fn create_run_items_and_sse() {
    let (addr, _rx) = test_support::spawn(
        r#"{"choices":[{"message":{"content":"{\"0\":\"〇\",\"1\":\"こんにちは\",\"2\":\"さようなら\"}"}}],"usage":{"prompt_tokens":5,"completion_tokens":7}}"#,
        200,
    )
    .await;
    let (base, _cfg_path) = spawn_server(BTreeMap::from([("test".to_string(), llm_config(addr))])).await;
    let (_dir, path) = temp_file("a.txt", "你好\n再见\n");

    create_project(&base, &path).await;
    assert_eq!(run_action(&base, "start").await, 200);

    // SSE：连接后应持续收到事件，progress 每 1s 合成一次
    let resp = reqwest::get(format!("{base}/api/projects/current/events"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    for _ in 0..4 {
        match tokio::time::timeout(Duration::from_secs(2), stream.next()).await {
            Ok(Some(Ok(chunk))) => buf.push_str(&String::from_utf8_lossy(&chunk)),
            _ => break,
        }
        if buf.contains("event: progress") {
            break;
        }
    }
    assert!(buf.contains("event: progress"), "未收到 progress 事件: {buf}");

    let view = wait_status(&base, "completed", Duration::from_secs(10)).await;
    // 文件名条目（line 0）+ 2 行 → 3 条成功
    assert_eq!(view["counts"]["success"], 3);
    assert_eq!(view["info"]["token_usage"]["input"], 15); // 3 请求 × 5

    // 条目列表（按 file, line 排序，line 0 文件名在前）
    let resp = reqwest::get(format!("{base}/api/projects/current/items"))
        .await
        .unwrap();
    let page: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(page["total"], 3);
    assert_eq!(page["items"][0]["line"], 0);
    assert_eq!(page["items"][0]["source"], "a.txt");
    let line1 = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["line"] == 1)
        .unwrap();
    assert_eq!(line1["status"], "success");
    assert_eq!(line1["target"], "こんにちは");
}

#[tokio::test]
async fn pause_then_resume_continues() {
    // 慢 LLM：300ms 响应，保证暂停时 in-flight 排空可观察
    let (addr, _rx) = test_support::spawn_handler(|req| async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        let user = serde_json::from_str::<serde_json::Value>(&req.body).unwrap()["messages"][1]["content"]
            .as_str()
            .unwrap()
            .to_string();
        let id: u64 = serde_json::from_str::<serde_json::Value>(&user)
            .unwrap()
            .as_object()
            .unwrap()
            .keys()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        (
            200,
            format!(r#"{{"choices":[{{"message":{{"content":"{{\"{id}\":\"译文{id}\"}}"}}}}],"usage":null}}"#),
        )
    })
    .await;
    let (base, _cfg_path) = spawn_server(BTreeMap::from([("test".to_string(), llm_config(addr))])).await;
    let (_dir, path) = temp_file(
        "a.txt",
        &(1..=6)
            .map(|i| format!("行{i}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );

    create_project(&base, &path).await;
    assert_eq!(run_action(&base, "start").await, 200);
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(run_action(&base, "pause").await, 200);
    wait_status(&base, "paused", Duration::from_secs(5)).await;

    assert_eq!(run_action(&base, "resume").await, 200);
    let view = wait_status(&base, "completed", Duration::from_secs(10)).await;
    assert_eq!(view["counts"]["success"], 7); // 6 行 + 文件名条目
}

#[tokio::test]
async fn retry_all_moves_failures_and_reruns() {
    // 前 6 次请求失败（3 条 × 2 次尝试，retry_limit=1），之后成功
    let calls = Arc::new(AtomicU32::new(0));
    let (addr, _rx) = test_support::spawn_handler({
        let calls = calls.clone();
        move |_| {
            let calls = calls.clone();
            async move {
                let n = calls.fetch_add(1, Ordering::SeqCst);
                // 建工程不调用 LLM；n=0..5 为 3 条（文件名 + 2 行）× 2 次尝试
                if n < 6 {
                    (500, r#"{"error":"boom"}"#.to_string())
                } else {
                    (
                        200,
                        r#"{"choices":[{"message":{"content":"{\"0\":\"〇\",\"1\":\"こんにちは\",\"2\":\"さようなら\"}"}}],"usage":null}"#.to_string(),
                    )
                }
            }
        }
    })
    .await;
    let (base, _cfg_path) = spawn_server(BTreeMap::from([("test".to_string(), llm_config(addr))])).await;
    let (_dir, path) = temp_file("a.txt", "你好\n再见\n");

    create_project(&base, &path).await;
    assert_eq!(run_action(&base, "start").await, 200);
    let view = wait_status(&base, "completed", Duration::from_secs(10)).await;
    assert_eq!(view["counts"]["failure"], 3);

    // 单条重试 + 全部重试
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/projects/current/retry/item"))
        .json(&serde_json::json!({ "file": path, "line": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/projects/current/retry"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Completed + waiting 非空可再次 start
    assert_eq!(run_action(&base, "start").await, 200);
    let view = wait_status(&base, "completed", Duration::from_secs(10)).await;
    assert_eq!(view["counts"]["success"], 3);
    assert_eq!(view["counts"]["failure"], 0);
}

#[tokio::test]
async fn patch_item_updates_target_and_reset_retranslates() {
    let (addr, _rx) = test_support::spawn(
        r#"{"choices":[{"message":{"content":"{\"1\":\"こんにちは\"}"}}],"usage":null}"#,
        200,
    )
    .await;
    let (base, _cfg_path) = spawn_server(BTreeMap::from([("test".to_string(), llm_config(addr))])).await;
    let (_dir, path) = temp_file("a.txt", "你好\n");

    create_project(&base, &path).await;
    assert_eq!(run_action(&base, "start").await, 200);
    wait_status(&base, "completed", Duration::from_secs(10)).await;

    // 修改译文
    let resp = reqwest::Client::new()
        .patch(format!("{base}/api/projects/current/items"))
        .json(&serde_json::json!({ "file": path, "line": 1, "target": "おはよう" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let resp = reqwest::get(format!("{base}/api/projects/current/items"))
        .await
        .unwrap();
    let page: serde_json::Value = resp.json().await.unwrap();
    let line1 = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["line"] == 1)
        .unwrap();
    assert_eq!(line1["target"], "おはよう");

    // 重置重译 → 重新运行
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/projects/current/items/reset"))
        .json(&serde_json::json!({ "file": path, "line": 1 }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(run_action(&base, "start").await, 200);
    let view = wait_status(&base, "completed", Duration::from_secs(10)).await;
    assert_eq!(view["counts"]["waiting"], 0);
    assert_eq!(view["counts"]["success"], 1);
}

#[tokio::test]
async fn load_normalizes_stale_running_status() {
    // 模拟运行中进程退出: .trans 残留 run_status = running
    let (addr, _rx) = test_support::spawn(
        r#"{"choices":[{"message":{"content":"{\"1\":\"x\"}"}}],"usage":null}"#,
        200,
    )
    .await;
    let (base, _cfg_path) = spawn_server(BTreeMap::from([("test".to_string(), llm_config(addr))])).await;
    let (dir, path) = temp_file("a.txt", "你好\n");
    create_project(&base, &path).await;
    let trans_path = dir.join("a.trans");
    let mut p: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&trans_path).unwrap()).unwrap();
    p["info"]["run_status"] = serde_json::json!("running");
    std::fs::write(&trans_path, serde_json::to_string_pretty(&p).unwrap()).unwrap();

    // 载入 → 归一为 paused 并落盘
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/projects/load"))
        .json(&serde_json::json!({ "trans_path": trans_path.to_string_lossy() }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let view: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(view["info"]["run_status"], "paused");
    let saved: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&trans_path).unwrap()).unwrap();
    assert_eq!(saved["info"]["run_status"], "paused");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn recent_projects_recorded_on_create_and_listed() {
    let (addr, _rx) = test_support::spawn(
        r#"{"choices":[{"message":{"content":"{\"1\":\"x\"}"}}],"usage":null}"#,
        200,
    )
    .await;
    let (base, cfg_path) = spawn_server(BTreeMap::from([("test".to_string(), llm_config(addr))])).await;
    let (_dir, path) = temp_file("a.txt", "你好\n");
    create_project(&base, &path).await;

    // 已持久化到 config.toml
    let saved: Config = config::Config::load_from(&cfg_path).unwrap();
    let trans = path.replace(".txt", ".trans");
    assert!(saved.recent_projects.contains(&trans));

    // GET /api/projects 返回历史列表
    let resp = reqwest::get(format!("{base}/api/projects")).await.unwrap();
    let v: serde_json::Value = resp.json().await.unwrap();
    assert!(
        v["recent"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p == &serde_json::Value::String(trans.clone())),
        "{v}"
    );
}

#[tokio::test]
async fn providers_crud_and_models() {
    let (addr, _rx) = test_support::spawn(r#"{"object":"list","data":[{"id":"model-a"}]}"#, 200).await;
    let (base, cfg_path) = spawn_server(BTreeMap::new()).await;

    // 新建
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/providers"))
        .json(&serde_json::json!({
            "name": "x",
            "config": {
                "api_style": "completion",
                "base_url": addr,
                "api_key": "k",
                "model": "m"
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    // 已持久化
    let saved: Config = config::Config::load_from(&cfg_path).unwrap();
    assert!(saved.providers.contains_key("x"));

    // models 查询
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/providers/x/models"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let models: Vec<String> = resp.json().await.unwrap();
    assert_eq!(models, vec!["model-a"]);

    // 更新
    let resp = reqwest::Client::new()
        .put(format!("{base}/api/providers/x"))
        .json(&serde_json::json!({
            "config": {
                "api_style": "completion",
                "base_url": addr,
                "api_key": "k2",
                "model": "m2"
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let saved: Config = config::Config::load_from(&cfg_path).unwrap();
    assert_eq!(saved.providers["x"].model, "m2");

    // 删除
    let resp = reqwest::Client::new()
        .delete(format!("{base}/api/providers/x"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let saved: Config = config::Config::load_from(&cfg_path).unwrap();
    assert!(!saved.providers.contains_key("x"));

    // 热更新回归：API 新建 provider 后立即可用于创建工程（无需重启）
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/providers"))
        .json(&serde_json::json!({
            "name": "fresh",
            "config": {"api_style": "completion", "base_url": addr, "api_key": "k", "model": "m"}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let (_dir, path) = temp_file("hot.txt", "你好\n");
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/projects"))
        .json(&serde_json::json!({
            "object": {"type": "file", "path": path},
            "source": "zh-Hans", "target": "ja",
            "project_config": {"task_limit": 1, "retry_limit": 1, "task_timeout_ms": 5000, "provider": "fresh"}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "热更新 provider 创建工程失败: {}",
        resp.text().await.unwrap()
    );
}
