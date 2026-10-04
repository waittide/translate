//! 工程状态 Actor：单写者持有 [`Project`]，通过消息通道对外提供查询/修改，
//! 同时维护一份只读快照供 Web/SSE 无锁读取。

use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use domain::{
    language::Language,
    llm::LlmUsage,
    project::{ItemCounts, ItemStatus, Project, ProjectConfig, ProjectInfo, ProjectObject, RunStatus},
};
use parking_lot::RwLock;
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::{ExportMode, ServiceError, event::ServiceEvent, writer};

/// 只读工程快照（体积小，适合高频读取）。
#[derive(Debug, Clone)]
pub struct ProjectViewData {
    pub info:           ProjectInfo,
    pub counts:         ItemCounts,
    pub project_config: ProjectConfig,
}

impl ProjectViewData {
    fn from_project(project: &Project) -> Self {
        Self {
            info:           project.info.clone(),
            counts:         project.counts(),
            project_config: project.project_config.clone(),
        }
    }
}

/// 条目行数据（服务层返回，Web 层再组装成视图）。
#[derive(Debug, Clone)]
pub struct ItemData {
    pub file:   String,
    pub line:   u64,
    pub status: ItemStatus,
    pub source: String,
    pub target: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ItemPageData {
    pub total: usize,
    pub items: Vec<ItemData>,
}

/// 从 waiting 取出的一个待翻译条目。
#[derive(Debug, Clone)]
pub struct TakenItem {
    pub file:            String,
    pub line:            u64,
    pub retry_times:     u64,
    pub source:          String,
    pub source_language: Language,
    pub target_language: Language,
}

/// 一轮翻译开始前从 Actor 取到的运行参数。
#[derive(Debug, Clone)]
pub struct RunMeta {
    pub waiting_total:   usize,
    pub concurrency:     usize,
    pub timeout:         Duration,
    pub accumulated_ms:  u64,
    pub source_language: Language,
    pub target_language: Language,
}

/// 一次批量取 waiting 的结果。
#[derive(Debug, Clone)]
pub struct TakeBatch {
    pub items:         Vec<TakenItem>,
    pub backoff_until: Option<u64>,
}

/// 失败/错误处理后的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompleteOutcome {
    /// 是否重新放回 waiting；false 表示已转为 failure。
    pub requeued: bool,
}

/// 工程 Actor 的对外句柄。
#[derive(Clone)]
pub struct ProjectHandle {
    tx:       mpsc::Sender<ProjectMsg>,
    snapshot: Arc<RwLock<ProjectViewData>>,
}

impl ProjectHandle {
    /// 启动一个工程 Actor。Actor 拥有 `Project`，所有修改都经消息队列串行执行。
    pub fn spawn(project: Project, events: broadcast::Sender<ServiceEvent>) -> Self {
        let (tx, rx) = mpsc::channel(256);
        let snapshot = Arc::new(RwLock::new(ProjectViewData::from_project(&project)));
        tokio::spawn(actor_loop(project, rx, events, snapshot.clone()));
        Self { tx, snapshot }
    }

    /// 无锁读取最新快照。
    pub fn view_snapshot(&self) -> ProjectViewData {
        self.snapshot.read().clone()
    }

    /// 查询工程配置（从快照读取，无锁）。
    pub fn project_config(&self) -> ProjectConfig {
        self.view_snapshot().project_config
    }

    /// 获取完整 Project 克隆（测试/特殊场景用；不要在热路径调用）。
    pub async fn snapshot_project(&self) -> Result<Project, ServiceError> {
        self.call(|reply| ProjectMsg::SnapshotProject { reply })
            .await
    }

    /// 分页/过滤查询条目。
    pub async fn items(&self, status: Option<&str>, offset: usize, limit: usize) -> Result<ItemPageData, ServiceError> {
        self.call(|reply| ProjectMsg::Items {
            status: status.map(str::to_owned),
            offset,
            limit,
            reply,
        })
        .await
    }

    /// 导出译文。
    pub async fn export(&self, mode: ExportMode, output_dir: &str) -> Result<usize, ServiceError> {
        self.call(|reply| ProjectMsg::Export {
            mode,
            output_dir: output_dir.to_string(),
            reply,
        })
        .await
    }

    /// 人工修改成功条目译文。
    pub async fn update_target(&self, file: &str, line: u64, target: String) -> Result<(), ServiceError> {
        self.call(|reply| ProjectMsg::UpdateTarget {
            file: file.to_string(),
            line,
            target,
            reply,
        })
        .await
    }

    /// 重置条目回 waiting。
    pub async fn reset_item(&self, file: &str, line: u64) -> Result<(), ServiceError> {
        self.call(|reply| ProjectMsg::ResetItem {
            file: file.to_string(),
            line,
            reply,
        })
        .await
    }

    /// 失败条目移回 waiting。
    pub async fn retry_failed(&self, filter: Option<(&str, u64)>) -> Result<usize, ServiceError> {
        self.call(|reply| ProjectMsg::RetryFailed {
            filter: filter.map(|(f, l)| (f.to_string(), l)),
            reply,
        })
        .await
    }

    /// 更新工程配置。
    pub async fn update_config(&self, config: ProjectConfig) -> Result<(), ServiceError> {
        self.call(|reply| ProjectMsg::UpdateConfig { config, reply })
            .await
    }

    /// 落盘（引擎节流保存用）；同时更新累计运行时长。
    pub async fn save(&self, elapsed_ms: u64) -> Result<(), ServiceError> {
        self.call(|reply| ProjectMsg::Save { elapsed_ms, reply })
            .await
    }

    pub(crate) async fn prepare_run(&self) -> Result<Option<RunMeta>, ServiceError> {
        self.call(|reply| ProjectMsg::PrepareRun { reply }).await
    }

    pub(crate) async fn take_retryable(&self, now: u64, max: usize) -> Result<TakeBatch, ServiceError> {
        self.call(|reply| ProjectMsg::TakeRetryable { now, max, reply })
            .await
    }

    pub(crate) async fn complete_success(
        &self,
        file: &str,
        line: u64,
        target: String,
        usage: LlmUsage,
    ) -> Result<(), ServiceError> {
        self.call(|reply| ProjectMsg::CompleteSuccess {
            file: file.to_string(),
            line,
            target,
            usage,
            reply,
        })
        .await
    }

    pub(crate) async fn complete_failure(
        &self,
        file: &str,
        line: u64,
        retry_count: u64,
        reason: String,
        now: u64,
    ) -> Result<CompleteOutcome, ServiceError> {
        self.call(|reply| ProjectMsg::CompleteFailure {
            file: file.to_string(),
            line,
            retry_count,
            reason,
            now,
            reply,
        })
        .await
    }

    pub(crate) async fn complete_panic(&self) -> Result<(), ServiceError> {
        self.call(|reply| ProjectMsg::CompletePanic { reply }).await
    }

    pub(crate) async fn finish_run(&self, cancelled: bool, elapsed_ms: u64) -> Result<(), ServiceError> {
        self.call(|reply| ProjectMsg::FinishRun {
            cancelled,
            elapsed_ms,
            reply,
        })
        .await
    }

    async fn call<R, F>(&self, make: F) -> Result<R, ServiceError>
    where
        R: Send + 'static,
        F: FnOnce(oneshot::Sender<Result<R, ServiceError>>) -> ProjectMsg,
    {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(make(tx))
            .await
            .map_err(|_| ServiceError::TaskPanic("工程 actor 已关闭".to_string()))?;
        rx.await
            .map_err(|_| ServiceError::TaskPanic("工程 actor 已关闭".to_string()))?
    }
}

enum ProjectMsg {
    SnapshotProject {
        reply: oneshot::Sender<Result<Project, ServiceError>>,
    },
    Items {
        status: Option<String>,
        offset: usize,
        limit:  usize,
        reply:  oneshot::Sender<Result<ItemPageData, ServiceError>>,
    },
    Export {
        mode:       ExportMode,
        output_dir: String,
        reply:      oneshot::Sender<Result<usize, ServiceError>>,
    },
    UpdateTarget {
        file:   String,
        line:   u64,
        target: String,
        reply:  oneshot::Sender<Result<(), ServiceError>>,
    },
    ResetItem {
        file:  String,
        line:  u64,
        reply: oneshot::Sender<Result<(), ServiceError>>,
    },
    RetryFailed {
        filter: Option<(String, u64)>,
        reply:  oneshot::Sender<Result<usize, ServiceError>>,
    },
    UpdateConfig {
        config: ProjectConfig,
        reply:  oneshot::Sender<Result<(), ServiceError>>,
    },
    Save {
        elapsed_ms: u64,
        reply:      oneshot::Sender<Result<(), ServiceError>>,
    },
    PrepareRun {
        reply: oneshot::Sender<Result<Option<RunMeta>, ServiceError>>,
    },
    TakeRetryable {
        now:   u64,
        max:   usize,
        reply: oneshot::Sender<Result<TakeBatch, ServiceError>>,
    },
    CompleteSuccess {
        file:   String,
        line:   u64,
        target: String,
        usage:  LlmUsage,
        reply:  oneshot::Sender<Result<(), ServiceError>>,
    },
    CompleteFailure {
        file:        String,
        line:        u64,
        retry_count: u64,
        reason:      String,
        now:         u64,
        reply:       oneshot::Sender<Result<CompleteOutcome, ServiceError>>,
    },
    CompletePanic {
        reply: oneshot::Sender<Result<(), ServiceError>>,
    },
    FinishRun {
        cancelled:  bool,
        elapsed_ms: u64,
        reply:      oneshot::Sender<Result<(), ServiceError>>,
    },
}

async fn actor_loop(
    mut project: Project,
    mut rx: mpsc::Receiver<ProjectMsg>,
    events: broadcast::Sender<ServiceEvent>,
    snapshot: Arc<RwLock<ProjectViewData>>,
) {
    while let Some(msg) = rx.recv().await {
        match msg {
            ProjectMsg::SnapshotProject { reply } => {
                let _ = reply.send(Ok(project.clone()));
            }
            ProjectMsg::Items {
                status,
                offset,
                limit,
                reply,
            } => {
                let _ = reply.send(Ok(items_page(&project, status.as_deref(), offset, limit)));
            }
            ProjectMsg::Export {
                mode,
                output_dir,
                reply,
            } => {
                let _ = reply.send(writer::export(&project, &output_dir, mode));
            }
            ProjectMsg::UpdateTarget {
                file,
                line,
                target,
                reply,
            } => {
                let result = (|| {
                    if !project.update_target(&file, line, target.clone()) {
                        return Err(ServiceError::ItemNotFound {
                            file: file.clone(),
                            line,
                        });
                    }
                    if line == 0 && matches!(project.info.object, ProjectObject::File { .. }) {
                        project.info.translate_name = Some(target);
                    }
                    project.save_trans()?;
                    publish(&project, &snapshot);
                    Ok(())
                })();
                let _ = reply.send(result);
            }
            ProjectMsg::ResetItem { file, line, reply } => {
                let result = (|| {
                    if !project.move_to_waiting(&file, line) {
                        return Err(ServiceError::ItemNotFound { file, line });
                    }
                    project.save_trans()?;
                    publish(&project, &snapshot);
                    Ok(())
                })();
                let _ = reply.send(result);
            }
            ProjectMsg::RetryFailed { filter, reply } => {
                let result = (|| {
                    let mut keys: Vec<(String, u64)> = Vec::new();
                    for (file, m) in &project.failure_items {
                        for &line in m.keys() {
                            if filter.as_ref().is_none_or(|(f, l)| f == file && *l == line) {
                                keys.push((file.clone(), line));
                            }
                        }
                    }
                    let moved = keys
                        .iter()
                        .filter(|(file, line)| project.move_to_waiting(file, *line))
                        .count();
                    project.save_trans()?;
                    publish(&project, &snapshot);
                    Ok(moved)
                })();
                let _ = reply.send(result);
            }
            ProjectMsg::UpdateConfig { config, reply } => {
                project.project_config = config;
                let result = project.save_trans().map_err(Into::into).map(|_| {
                    publish(&project, &snapshot);
                });
                let _ = reply.send(result);
            }
            ProjectMsg::Save { elapsed_ms, reply } => {
                project.info.running_time_ms = elapsed_ms;
                let result = project.save_trans().map_err(Into::into).map(|_| {
                    publish(&project, &snapshot);
                });
                let _ = reply.send(result);
            }
            ProjectMsg::PrepareRun { reply } => {
                let result = prepare_run(&mut project, &events).map(|opt| {
                    if opt.is_some() {
                        publish(&project, &snapshot);
                    }
                    opt
                });
                let _ = reply.send(result);
            }
            ProjectMsg::TakeRetryable { now, max, reply } => {
                let batch = take_retryable(&mut project, now, max, &events);
                publish(&project, &snapshot);
                let _ = reply.send(Ok(batch));
            }
            ProjectMsg::CompleteSuccess {
                file,
                line,
                target,
                usage,
                reply,
            } => {
                let result = (|| {
                    if let Some(m) = project.in_flight.get_mut(&file) {
                        m.remove(&line);
                        if m.is_empty() {
                            project.in_flight.remove(&file);
                        }
                    }
                    project.record_success(&file, line, target.clone());
                    if line == 0 && matches!(project.info.object, ProjectObject::File { .. }) {
                        project.info.translate_name = Some(target.clone());
                    }
                    project.info.token_usage.input += usage.input;
                    project.info.token_usage.output += usage.output;
                    let _ = events.send(ServiceEvent::Item {
                        file: file.clone(),
                        line,
                        status: ItemStatus::Success,
                        source: project
                            .source_of(&file, line)
                            .map(str::to_owned)
                            .unwrap_or_default(),
                        target: Some(target),
                        reason: None,
                    });
                    publish(&project, &snapshot);
                    Ok(())
                })();
                let _ = reply.send(result);
            }
            ProjectMsg::CompleteFailure {
                file,
                line,
                retry_count,
                reason,
                now,
                reply,
            } => {
                if let Some(m) = project.in_flight.get_mut(&file) {
                    m.remove(&line);
                    if m.is_empty() {
                        project.in_flight.remove(&file);
                    }
                }
                let outcome = if retry_count + 1 > project.project_config.retry_limit {
                    project.record_failure(&file, line, reason.clone());
                    CompleteOutcome { requeued: false }
                } else {
                    project.requeue(&file, line, retry_count + 1, now + backoff_ms(retry_count + 1));
                    CompleteOutcome { requeued: true }
                };
                let _ = events.send(ServiceEvent::Item {
                    file: file.clone(),
                    line,
                    status: ItemStatus::Failure,
                    source: project
                        .source_of(&file, line)
                        .map(str::to_owned)
                        .unwrap_or_default(),
                    target: None,
                    reason: Some(reason),
                });
                publish(&project, &snapshot);
                let _ = reply.send(Ok(outcome));
            }
            ProjectMsg::CompletePanic { reply } => {
                project.in_flight.clear();
                project.info.run_status = RunStatus::Paused;
                let result = project.save_trans().map_err(Into::into).map(|_| {
                    publish(&project, &snapshot);
                });
                let _ = reply.send(result);
            }
            ProjectMsg::FinishRun {
                cancelled,
                elapsed_ms,
                reply,
            } => {
                project.info.running_time_ms = elapsed_ms;
                project.info.run_status = if cancelled {
                    RunStatus::Paused
                } else {
                    RunStatus::Completed
                };
                let status = project.info.run_status;
                let result = project.save_trans().map_err(Into::into).map(|_| {
                    let _ = events.send(ServiceEvent::Status { status });
                    publish(&project, &snapshot);
                });
                let _ = reply.send(result);
            }
        }
    }
}

fn prepare_run(
    project: &mut Project,
    events: &broadcast::Sender<ServiceEvent>,
) -> Result<Option<RunMeta>, ServiceError> {
    let waiting_total = project.waiting_count();
    if waiting_total == 0 {
        return Ok(None);
    }
    let concurrency = (project.project_config.task_limit as usize)
        .min(waiting_total)
        .max(1);
    let timeout = Duration::from_millis(project.project_config.task_timeout_ms);
    let accumulated_ms = project.info.running_time_ms;
    let source_language = project.info.source_language;
    let target_language = project.info.target_language;
    project.info.run_status = RunStatus::Running;
    project.save_trans()?;
    let _ = events.send(ServiceEvent::Status {
        status: RunStatus::Running,
    });
    Ok(Some(RunMeta {
        waiting_total,
        concurrency,
        timeout,
        accumulated_ms,
        source_language,
        target_language,
    }))
}

fn take_retryable(project: &mut Project, now: u64, max: usize, events: &broadcast::Sender<ServiceEvent>) -> TakeBatch {
    let mut items = Vec::new();
    let mut backoff_until = None;
    for _ in 0..max {
        let Some((file, line)) = project.waiting_retryable_at(now) else {
            backoff_until = project.earliest_retry_at();
            break;
        };
        let (retry_times, source) = project
            .take_waiting(&file, line)
            .expect("刚找到的等待条目必然存在");
        project
            .in_flight
            .entry(file.clone())
            .or_default()
            .insert(line, ());
        let _ = events.send(ServiceEvent::Item {
            file: file.clone(),
            line,
            status: ItemStatus::Translating,
            source: source.clone(),
            target: None,
            reason: None,
        });
        items.push(TakenItem {
            file,
            line,
            retry_times,
            source,
            source_language: project.info.source_language,
            target_language: project.info.target_language,
        });
    }
    TakeBatch { items, backoff_until }
}

fn items_page(project: &Project, status: Option<&str>, offset: usize, limit: usize) -> ItemPageData {
    let mut all: Vec<ItemData> = Vec::new();
    for (file, m) in &project.source_items {
        for (&line, source) in m {
            let item_status = project
                .item_status(file, line)
                .unwrap_or(ItemStatus::Waiting);
            let (target, reason) = match item_status {
                ItemStatus::Success => (project.target_of(file, line).map(str::to_owned), None),
                ItemStatus::Failure => (None, project.failure_reason_of(file, line).map(str::to_owned)),
                ItemStatus::Translating | ItemStatus::Waiting => (None, None),
            };
            if status.is_some_and(|s| s != item_status.as_str()) {
                continue;
            }
            all.push(ItemData {
                file: file.clone(),
                line,
                status: item_status,
                source: source.clone(),
                target,
                reason,
            });
        }
    }
    let total = all.len();
    let offset = offset.min(total);
    let limit = limit.min(total - offset);
    ItemPageData {
        total,
        items: all.drain(offset..offset + limit).collect(),
    }
}

fn publish(project: &Project, snapshot: &Arc<RwLock<ProjectViewData>>) {
    *snapshot.write() = ProjectViewData::from_project(project);
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 指数退避：第 n 次尝试前等待 `min(1000 << (n-1), BACKOFF_CAP_MS)` 毫秒。
pub(crate) fn backoff_ms(attempt: u64) -> u64 {
    let shift = attempt.saturating_sub(1).min(5); // 1s, 2s, 4s, 8s, 16s, 30s
    (1000u64 << shift).min(30_000)
}
