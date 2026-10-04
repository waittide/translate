//! HTTP 接口层：工程管理、运行控制、条目操作、SSE 实时流、provider 配置。

pub mod fs;
pub mod projects;
pub mod providers;
pub mod sse;

use std::{path::PathBuf, sync::Arc};

use axum::{
    Json, Router,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use config::Config;
use domain::{
    llm::LlmUsage,
    project::{ItemCounts, ProjectConfig, ProjectInfo},
};
use parking_lot::RwLock;
use service::{
    Service, ServiceError, TranslateReport,
    event::ServiceEvent,
    project::{ProjectHandle, ProjectViewData},
};
use tokio::{
    sync::{Mutex, broadcast},
    task::JoinHandle,
    time::Instant,
};
use tokio_util::sync::CancellationToken;

/// 每秒 token 消耗速率。
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct TokenRate {
    pub input:  f64,
    pub output: f64,
}

/// 运行中翻译任务句柄。
pub struct RunHandle {
    pub(crate) cancel: CancellationToken,
    pub(crate) task:   JoinHandle<Result<TranslateReport, ServiceError>>,
}

/// token 速率跟踪槽位。
struct RateSlot {
    /// 上次累计用量
    last_usage: LlmUsage,
    /// 上次采样时刻
    last_at:    Instant,
    /// 当前速率
    rate:       TokenRate,
}

/// 全局共享状态：单活跃工程 + provider 配置 + 事件总线。
/// `config` 与 [`Service`] 共享同一实例（单一事实源），provider 变更自动生效。
pub struct AppState {
    pub(crate) service:     Arc<Service>,
    pub(crate) config:      Arc<RwLock<Config>>,
    pub(crate) config_path: PathBuf,
    pub(crate) current:     Mutex<Option<ProjectHandle>>,
    pub(crate) events:      broadcast::Sender<ServiceEvent>,
    pub(crate) run:         Mutex<Option<RunHandle>>,
    /// token 速率跟踪
    pub(crate) rate:        Mutex<RateSlot>,
}

impl AppState {
    pub fn new(config: Config) -> Arc<Self> {
        Self::with_config_path(config, config::default_path())
    }

    /// 指定配置文件路径（测试注入临时路径，避免污染真实配置）。
    pub fn with_config_path(config: Config, config_path: PathBuf) -> Arc<Self> {
        let config = Arc::new(RwLock::new(config));
        Arc::new(Self {
            service: Arc::new(Service::new(config.clone())),
            config,
            config_path,
            current: Mutex::new(None),
            events: broadcast::channel(256).0,
            run: Mutex::new(None),
            rate: Mutex::new(RateSlot {
                last_usage: LlmUsage::default(),
                last_at:    Instant::now(),
                rate:       TokenRate::default(),
            }),
        })
    }

    /// 当前工程句柄（若有）。
    pub async fn current_handle(&self) -> Option<ProjectHandle> {
        self.current.lock().await.clone()
    }

    /// 等待旧任务结束并清空句柄（start/resume 前调用，保证引擎不并发操作同一工程）。
    pub async fn ensure_idle(&self) {
        let old = self.run.lock().await.take();
        if let Some(h) = old {
            let _ = h.task.await;
        }
    }

    /// 启动一轮翻译任务；运行中检查与句柄设置在同一把锁内，避免并发 start 双启动。
    pub async fn spawn_run(&self, project: ProjectHandle) -> Result<(), ApiError> {
        let mut run = self.run.lock().await;
        if run.as_ref().is_some_and(|h| !h.task.is_finished()) {
            return Err(ApiError::Conflict("已有运行中的任务".to_string()));
        }
        let cancel = CancellationToken::new();
        let service = self.service.clone();
        let cancel_task = cancel.clone();
        let task = tokio::spawn(async move { service.translate_all(project, cancel_task).await });
        *run = Some(RunHandle { cancel, task });
        Ok(())
    }

    /// 用当前累计值更新 token 速率（SSE 定时器调用；锁内执行）。
    pub async fn update_rate(&self, data: &ProjectViewData) {
        let now = Instant::now();
        let mut slot = self.rate.lock().await;
        let dt = now.duration_since(slot.last_at).as_secs_f64();
        if dt >= 0.5 {
            let input = ((data.info.token_usage.input as f64 - slot.last_usage.input as f64) / dt).max(0.0);
            let output = ((data.info.token_usage.output as f64 - slot.last_usage.output as f64) / dt).max(0.0);
            *slot = RateSlot {
                last_usage: data.info.token_usage,
                last_at:    now,
                rate:       TokenRate { input, output },
            };
        }
    }

    /// 当前 token 速率快照。
    pub async fn rate_snapshot(&self) -> TokenRate {
        self.rate.lock().await.rate
    }

    /// 配置持久化到状态内路径（默认 `~/.config/translate/config.toml`）。
    pub async fn save_config(&self) -> Result<(), ApiError> {
        let cfg = self.config.read().clone();
        cfg.save(&self.config_path)
            .map_err(|e| ApiError::Internal(e.to_string()))
    }
}

/// 工程详情视图（统一响应结构）。运行态字段在 `info` 内，不重复外提。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProjectView {
    pub info:           ProjectInfo,
    pub counts:         ItemCounts,
    pub token_rate:     TokenRate,
    pub project_config: ProjectConfig,
}

/// 由只读快照构建视图。
pub fn project_view(data: &ProjectViewData, rate: TokenRate) -> ProjectView {
    ProjectView {
        info:           data.info.clone(),
        counts:         data.counts,
        token_rate:     rate,
        project_config: data.project_config.clone(),
    }
}

/// API 错误。
#[derive(Debug)]
pub enum ApiError {
    NoProject,
    NotFound(String),
    BadRequest(String),
    Conflict(String),
    Internal(String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::NoProject => write!(f, "当前没有工程"),
            ApiError::NotFound(m) | ApiError::BadRequest(m) | ApiError::Conflict(m) | ApiError::Internal(m) => {
                write!(f, "{m}")
            }
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            ApiError::NoProject => (StatusCode::NOT_FOUND, "当前没有工程".to_string()),
            ApiError::NotFound(m) => (StatusCode::NOT_FOUND, m),
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            ApiError::Conflict(m) => (StatusCode::CONFLICT, m),
            ApiError::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, m),
        };
        (status, Json(serde_json::json!({ "error": msg }))).into_response()
    }
}

impl From<ServiceError> for ApiError {
    fn from(e: ServiceError) -> Self {
        match e {
            ServiceError::ItemNotFound { file, line } => ApiError::NotFound(format!("条目不存在: {file}:{line}")),
            ServiceError::ProviderNotFound(name) => ApiError::NotFound(format!("provider 未配置: {name}")),
            other => ApiError::Internal(other.to_string()),
        }
    }
}

/// 构建路由。
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/projects", get(projects::list).post(projects::create))
        .route("/api/projects/load", post(projects::load))
        .route("/api/projects/current", get(projects::current))
        .route("/api/projects/current/run", post(projects::run))
        .route(
            "/api/projects/current/items",
            get(projects::items).patch(projects::patch_item),
        )
        .route("/api/projects/current/items/reset", post(projects::reset_item))
        .route("/api/projects/current/config", put(projects::update_config))
        .route("/api/projects/current/retry", post(projects::retry_all))
        .route("/api/projects/current/retry/item", post(projects::retry_item))
        .route("/api/projects/current/export", post(projects::export))
        .route("/api/projects/current/events", get(sse::events))
        .route("/api/providers", get(providers::list).post(providers::upsert))
        .route("/api/providers/models", post(providers::models_by_config))
        .route(
            "/api/providers/{name}",
            put(providers::upsert_named).delete(providers::delete),
        )
        .route("/api/providers/{name}/models", post(providers::models))
        .route("/api/fs/list", get(fs::list))
        .with_state(state)
}
