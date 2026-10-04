//! 工程与运行控制 handler。

use std::{convert::Infallible, sync::Arc, time::Duration};

use axum::{
    Json,
    extract::{Query, State},
    response::sse::{Event, Sse},
};
use domain::{
    language::Language,
    project::{ItemStatus, Project, ProjectConfig, ProjectObject, RunStatus},
};
use serde::{Deserialize, Serialize};
use tokio_stream::{Stream, StreamExt};

use crate::{ApiError, AppState, ProjectView, project_view};

/// 创建工程请求体。
#[derive(Deserialize)]
pub struct CreateProject {
    pub object:         ProjectObject,
    pub source:         Language,
    pub target:         Language,
    pub project_config: ProjectConfig,
}

/// 载入历史工程请求体。
#[derive(Deserialize)]
pub struct LoadProject {
    pub trans_path: String,
}

/// 运行控制动作。
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunAction {
    Start,
    Pause,
    Resume,
}

#[derive(Deserialize)]
pub struct RunRequest {
    pub action: RunAction,
}

/// 条目查询参数。
#[derive(Deserialize)]
pub struct ItemsQuery {
    pub status: Option<String>,
    pub offset: Option<usize>,
    pub limit:  Option<usize>,
}

/// 条目视图。
#[derive(Serialize)]
pub struct ItemView {
    pub file:   String,
    pub line:   u64,
    pub status: ItemStatus,
    pub source: String,
    pub target: Option<String>,
    pub reason: Option<String>,
}

#[derive(Serialize)]
pub struct ItemsPage {
    pub total: usize,
    pub items: Vec<ItemView>,
}

/// 条目定位（file 含路径分隔符，走 body 避免 URL 编码）。
#[derive(Deserialize)]
pub struct ItemRef {
    pub file: String,
    pub line: u64,
}

#[derive(Deserialize)]
pub struct PatchItem {
    pub file:   String,
    pub line:   u64,
    pub target: String,
}

#[derive(Serialize)]
pub struct CountReply {
    pub retried: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportModeBody {
    TranslatedOnly,
    Bilingual,
}

#[derive(Deserialize)]
pub struct ExportRequest {
    pub mode:       ExportModeBody,
    pub output_dir: String,
}

#[derive(Serialize)]
pub struct ExportReply {
    pub files: usize,
}

/// 当前工程视图（或 null）+ 最近打开/创建的工程列表。
pub async fn list(State(st): State<Arc<AppState>>) -> Result<Json<serde_json::Value>, ApiError> {
    let current = match st.current_handle().await {
        Some(project) => {
            let rate = st.rate_snapshot().await;
            Some(project_view(&project.view_snapshot(), rate))
        }
        None => None,
    };
    let recent: Vec<String> = st.config.read().recent_projects.iter().cloned().collect();
    Ok(Json(serde_json::json!({
        "current": current,
        "recent": recent,
    })))
}

/// 创建工程并设为当前；响应为 SSE 事件流：`progress` 上报扫描进度，
/// 完成后发 `done`（携带 ProjectView），失败发 `error`。
pub async fn create(
    State(st): State<Arc<AppState>>,
    Json(body): Json<CreateProject>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    st.ensure_idle().await;
    let (tx, rx) = tokio::sync::mpsc::channel::<Event>(64);
    tokio::spawn(async move {
        let mut on_progress = |phase: service::CreateProgress| {
            let service::CreateProgress::Scanned { scanned, total } = phase;
            let event = Event::default()
                .event("progress")
                .data(serde_json::json!({ "scanned": scanned, "total": total }).to_string());
            let _ = tx.try_send(event);
        };
        let project = match st
            .service
            .create_project(
                body.object,
                body.source,
                body.target,
                body.project_config,
                &mut on_progress,
            )
            .await
        {
            Ok(p) => p,
            Err(e) => {
                let _ = tx.try_send(error_event(e.to_string()));
                return;
            }
        };
        let trans = project.trans_path().to_string_lossy().into_owned();
        let handle = service::project::ProjectHandle::spawn(project, st.events.clone());
        *st.current.lock().await = Some(handle.clone());
        {
            let mut cfg = st.config.write();
            cfg.recent_projects.insert(trans);
        }
        if let Err(e) = st.save_config().await {
            let _ = tx.try_send(error_event(e.to_string()));
            return;
        }
        let rate = st.rate_snapshot().await;
        let view = project_view(&handle.view_snapshot(), rate);
        let _ = tx.try_send(
            Event::default()
                .event("done")
                .data(serde_json::to_string(&view).unwrap_or_default()),
        );
    });
    Ok(Sse::new(
        tokio_stream::wrappers::ReceiverStream::new(rx).map(Ok::<_, Infallible>),
    ))
}

fn error_event(message: String) -> Event {
    Event::default()
        .event("error")
        .data(serde_json::json!({ "error": message }).to_string())
}

/// 载入历史 `.trans` 工程并设为当前。
pub async fn load(
    State(st): State<Arc<AppState>>,
    Json(body): Json<LoadProject>,
) -> Result<Json<ProjectView>, ApiError> {
    st.ensure_idle().await;
    let mut project =
        Project::load(std::path::Path::new(&body.trans_path)).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    // 运行状态是内存态：进程退出后 .trans 可能残留 Running，但没有对应任务；
    // 归一为可恢复的 Paused 并落盘，避免界面误显示"运行中/暂停"。
    if project.info.run_status == RunStatus::Running {
        project.info.run_status = RunStatus::Paused;
        project
            .save_trans()
            .map_err(|e| ApiError::Internal(e.to_string()))?;
    }
    let trans = project.trans_path().to_string_lossy().into_owned();
    let handle = service::project::ProjectHandle::spawn(project, st.events.clone());
    *st.current.lock().await = Some(handle.clone());
    {
        let mut cfg = st.config.write();
        cfg.recent_projects.insert(trans);
    }
    st.save_config().await?;
    let rate = st.rate_snapshot().await;
    Ok(Json(project_view(&handle.view_snapshot(), rate)))
}

/// 当前工程详情。
pub async fn current(State(st): State<Arc<AppState>>) -> Result<Json<ProjectView>, ApiError> {
    let project = st.current_handle().await.ok_or(ApiError::NoProject)?;
    let rate = st.rate_snapshot().await;
    Ok(Json(project_view(&project.view_snapshot(), rate)))
}

/// 运行控制：start / pause / resume。
pub async fn run(State(st): State<Arc<AppState>>, Json(body): Json<RunRequest>) -> Result<Json<ProjectView>, ApiError> {
    let project = st.current_handle().await.ok_or(ApiError::NoProject)?;
    match body.action {
        RunAction::Start => {
            st.ensure_idle().await;
            let snapshot = project.view_snapshot();
            let (status, waiting) = (snapshot.info.run_status, snapshot.counts.waiting);
            if matches!(status, RunStatus::Running | RunStatus::Paused) {
                return Err(ApiError::Conflict(format!(
                    "工程状态为 {status:?}，运行中/暂停中不能启动"
                )));
            }
            if waiting == 0 {
                return Err(ApiError::Conflict("没有等待翻译的条目".to_string()));
            }
            st.spawn_run(project.clone()).await?;
            // 引擎异步启动：等其进入 Running/Completed 再返回，避免响应携带未开始的陈旧状态
            for _ in 0..200 {
                let status = project.view_snapshot().info.run_status;
                if status != RunStatus::NotStarted {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
        RunAction::Pause => {
            let run = st.run.lock().await;
            let Some(handle) = run.as_ref() else {
                return Err(ApiError::Conflict("没有运行中的任务".to_string()));
            };
            handle.cancel.cancel();
        }
        RunAction::Resume => {
            st.ensure_idle().await;
            let status = project.view_snapshot().info.run_status;
            if status != RunStatus::Paused {
                return Err(ApiError::Conflict(format!("工程状态为 {status:?}，仅暂停状态可恢复")));
            }
            st.spawn_run(project.clone()).await?;
        }
    }
    let rate = st.rate_snapshot().await;
    Ok(Json(project_view(&project.view_snapshot(), rate)))
}

/// 条目列表（分页 + 状态过滤）。
pub async fn items(
    State(st): State<Arc<AppState>>,
    Query(query): Query<ItemsQuery>,
) -> Result<Json<ItemsPage>, ApiError> {
    let project = st.current_handle().await.ok_or(ApiError::NoProject)?;
    let page = project
        .items(
            query.status.as_deref(),
            query.offset.unwrap_or(0),
            query.limit.unwrap_or(usize::MAX),
        )
        .await?;
    Ok(Json(ItemsPage {
        total: page.total,
        items: page
            .items
            .into_iter()
            .map(|item| ItemView {
                file:   item.file,
                line:   item.line,
                status: item.status,
                source: item.source,
                target: item.target,
                reason: item.reason,
            })
            .collect(),
    }))
}

/// 修改成功条目的译文。
pub async fn patch_item(
    State(st): State<Arc<AppState>>,
    Json(body): Json<PatchItem>,
) -> Result<Json<ProjectView>, ApiError> {
    let project = st.current_handle().await.ok_or(ApiError::NoProject)?;
    project
        .update_target(&body.file, body.line, body.target)
        .await?;
    let rate = st.rate_snapshot().await;
    Ok(Json(project_view(&project.view_snapshot(), rate)))
}

/// 重置指定条目回 waiting 重新翻译。
pub async fn reset_item(
    State(st): State<Arc<AppState>>,
    Json(body): Json<ItemRef>,
) -> Result<Json<ProjectView>, ApiError> {
    let project = st.current_handle().await.ok_or(ApiError::NoProject)?;
    project.reset_item(&body.file, body.line).await?;
    let rate = st.rate_snapshot().await;
    Ok(Json(project_view(&project.view_snapshot(), rate)))
}

/// 全部失败条目移回 waiting。
pub async fn retry_all(State(st): State<Arc<AppState>>) -> Result<Json<CountReply>, ApiError> {
    let project = st.current_handle().await.ok_or(ApiError::NoProject)?;
    let retried = project.retry_failed(None).await?;
    Ok(Json(CountReply { retried }))
}

/// 单条失败条目移回 waiting。
pub async fn retry_item(
    State(st): State<Arc<AppState>>,
    Json(body): Json<ItemRef>,
) -> Result<Json<CountReply>, ApiError> {
    let project = st.current_handle().await.ok_or(ApiError::NoProject)?;
    let retried = project.retry_failed(Some((&body.file, body.line))).await?;
    Ok(Json(CountReply { retried }))
}

/// 更新当前工程的运行配置（task_limit 等）；修改立即落盘。
pub async fn update_config(
    State(st): State<Arc<AppState>>,
    Json(body): Json<ProjectConfig>,
) -> Result<Json<ProjectView>, ApiError> {
    let project = st.current_handle().await.ok_or(ApiError::NoProject)?;
    if body.task_limit == 0 {
        return Err(ApiError::BadRequest("并发任务数必须 ≥ 1".to_string()));
    }
    if body.task_timeout_ms < 1000 {
        return Err(ApiError::BadRequest("单次请求超时必须 ≥ 1000ms".to_string()));
    }
    if !st.config.read().providers.contains_key(&body.provider) {
        return Err(ApiError::BadRequest(format!("provider 未配置: {}", body.provider)));
    }
    project.update_config(body).await?;
    let rate = st.rate_snapshot().await;
    Ok(Json(project_view(&project.view_snapshot(), rate)))
}

/// 导出译文文件。
pub async fn export(
    State(st): State<Arc<AppState>>,
    Json(body): Json<ExportRequest>,
) -> Result<Json<ExportReply>, ApiError> {
    let project = st.current_handle().await.ok_or(ApiError::NoProject)?;
    let mode = match body.mode {
        ExportModeBody::TranslatedOnly => service::ExportMode::TranslatedOnly,
        ExportModeBody::Bilingual => service::ExportMode::Bilingual,
    };
    let files = project.export(mode, &body.output_dir).await?;
    Ok(Json(ExportReply { files }))
}
