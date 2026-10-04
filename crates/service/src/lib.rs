//! 业务逻辑层：扫描翻译对象建工程、滑动窗口并发翻译、手动导出。

mod engine;
pub mod event;
pub mod project;
mod writer;

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use config::Config;
use domain::{
    language::Language,
    llm::{LlmConfig, LlmUsage},
    project::{ItemMap, Project, ProjectConfig, ProjectError, ProjectInfo, ProjectObject, RunStatus, WaitingItem},
};
use llm::LlmClient;
use parking_lot::Mutex;
use thiserror::Error;
use tokio_util::sync::CancellationToken;

/// 导出模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportMode {
    /// 只导出译文；未成功条目保留原文
    TranslatedOnly,
    /// 每行 `原文\t译文`；未成功条目仅原文
    Bilingual,
}

/// 建工程阶段的进度回调事件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateProgress {
    /// 已扫描到第 `scanned` 行（`total` 为全部文件累计行数）
    Scanned { scanned: u64, total: u64 },
}

/// 业务层错误。
#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("provider 未配置: {0}")]
    ProviderNotFound(String),
    #[error("IO 失败: {path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("工程序列化失败: {path}: {source}")]
    Json { path: PathBuf, source: serde_json::Error },
    #[error("扫描翻译对象失败: {0}")]
    Scan(String),
    #[error("条目不存在: {file}:{line}")]
    ItemNotFound { file: String, line: u64 },
    #[error("翻译任务执行失败: {0}")]
    TaskPanic(String), // 仅 JoinSet 任务 panic 冒泡；单条失败进重试
}

impl From<ProjectError> for ServiceError {
    fn from(err: ProjectError) -> Self {
        match err {
            ProjectError::Io { path, source } => ServiceError::Io { path, source },
            ProjectError::Json { path, source } => ServiceError::Json { path, source },
        }
    }
}

/// 一次 [`Service::translate_all`] 调用的结果汇总。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TranslateReport {
    /// 本次调用实际发起的请求次数（含重试）
    pub attempts:  usize,
    pub succeeded: usize,
    /// 本次调用中转 failure 的条目数
    pub failed:    usize,
    pub cancelled: bool,
}

/// 翻译服务：与 web 层共享同一份全局配置（单一事实源），
/// provider 配置变更后客户端缓存按配置比对自动重建。
pub struct Service {
    /// 共享全局配置（与 web 层同一实例）
    config:  Arc<parking_lot::RwLock<Config>>,
    /// provider 名 → (构建时配置, 客户端缓存)
    clients: Mutex<BTreeMap<String, (LlmConfig, Arc<LlmClient>)>>,
}

impl Service {
    /// 共享全局配置；客户端按需构建并缓存（配置变更后自动重建）。
    pub fn new(config: Arc<parking_lot::RwLock<Config>>) -> Self {
        Self {
            config,
            clients: Mutex::new(BTreeMap::new()),
        }
    }

    /// 取 provider 客户端：配置读取与缓存检查各自持锁，无嵌套加锁；同步路径，锁不跨 await。
    /// 缓存条目携带构建时配置，与当前配置不一致时重建（热更新自动生效）。
    pub fn client(&self, name: &str) -> Result<Arc<LlmClient>, ServiceError> {
        let config = self
            .config
            .read()
            .providers
            .get(name)
            .cloned()
            .ok_or_else(|| ServiceError::ProviderNotFound(name.to_string()))?;
        let mut clients = self.clients.lock();
        if let Some((cached, client)) = clients.get(name) {
            if cached == &config {
                return Ok(client.clone());
            }
        }
        let client = Arc::new(LlmClient::new(config.clone()));
        clients.insert(name.to_string(), (config, client.clone()));
        Ok(client)
    }

    /// 扫描翻译对象建工程（见设计规则）；返回新 Project，已落盘 `.trans`。
    /// 建工程时用指定 provider 把原名翻译为目标语言写入 [`ProjectInfo::translate_name`]，
    /// 翻译失败不阻塞建工程（保持 None）。
    /// 扫描与文件名翻译阶段通过 `on_progress` 上报进度（供 web 层流式转发给前端）。
    pub async fn create_project(
        &self,
        object: ProjectObject,
        source: Language,
        target: Language,
        project_config: ProjectConfig,
        mut on_progress: impl FnMut(CreateProgress) + Send,
    ) -> Result<Project, ServiceError> {
        // provider 必须已配置（建工程不发起 LLM 请求，仅校验；运行翻译时才用）
        self.client(&project_config.provider)?;

        let (original_name, object) = match object {
            ProjectObject::File { path } => {
                let name = file_name_of(&path);
                (name, ProjectObject::File { path })
            }
            ProjectObject::Folder { path, file_paths } => {
                let name = file_name_of(&path);
                let resolved: BTreeSet<String> = if file_paths.is_empty() {
                    scan_txt(&path)?.into_iter().collect()
                } else {
                    file_paths
                };
                (
                    name,
                    ProjectObject::Folder {
                        path,
                        file_paths: resolved,
                    },
                )
            }
        };

        // 单次读取每个文件并保留文本：统计总行数用于进度分母，再逐行构建条目
        let mut files: Vec<(String, String)> = Vec::new();
        let mut total_lines: u64 = 0;
        for path in object.paths() {
            let text = std::fs::read_to_string(path).map_err(|source| ServiceError::Io {
                path: PathBuf::from(path),
                source,
            })?;
            total_lines += text.lines().count() as u64;
            files.push((path.clone(), text));
        }

        let mut source_items: ItemMap<String> = BTreeMap::new();
        let mut ignore_items: ItemMap<String> = BTreeMap::new();
        let mut waiting_items: ItemMap<WaitingItem> = BTreeMap::new();
        let mut scanned: u64 = 0;
        // 时间节流：快速扫描时每 ~50ms 至多报一次，避免事件洪峰淹没 SSE 通道
        let mut last_report = std::time::Instant::now();
        let report_every = Duration::from_millis(50);
        for (path, text) in &files {
            // 键 = 实际行号（1-based，含空行）；空行/纯空白行无需翻译 → ignore
            for (k, line) in text.lines().enumerate() {
                let k = k as u64 + 1;
                scanned += 1;
                // 节流上报：每 64 行且距上次 ≥50ms；末尾补报保证 100%
                if scanned == total_lines || (scanned.is_multiple_of(64) && last_report.elapsed() >= report_every) {
                    on_progress(CreateProgress::Scanned {
                        scanned,
                        total: total_lines,
                    });
                    last_report = std::time::Instant::now();
                }
                if line.trim().is_empty() {
                    ignore_items
                        .entry(path.clone())
                        .or_default()
                        .insert(k, line.to_string());
                } else {
                    source_items
                        .entry(path.clone())
                        .or_default()
                        .insert(k, line.to_string());
                    waiting_items
                        .entry(path.clone())
                        .or_default()
                        .insert(k, WaitingItem::default());
                }
            }
        }

        // 文件名作为条目：`<file, 0>` = 文件名，由引擎在运行时像普通条目一样翻译。
        // 建工程阶段不发起任何 LLM 请求。
        for path in object.paths() {
            let name = file_name_of(path);
            source_items
                .entry(path.clone())
                .or_default()
                .insert(0, name.clone());
            waiting_items
                .entry(path.clone())
                .or_default()
                .insert(0, WaitingItem::default());
        }

        let info = ProjectInfo {
            original_name,
            translate_name: None,
            object: object.clone(),
            source_language: source,
            target_language: target,
            running_time_ms: 0,
            token_usage: LlmUsage::default(),
            run_status: RunStatus::NotStarted,
        };
        let project = Project {
            project_config,
            info,
            source_items,
            ignore_items,
            success_items: BTreeMap::new(),
            failure_items: BTreeMap::new(),
            waiting_items,
            in_flight: BTreeMap::new(),
        };
        project.save_trans()?;
        Ok(project)
    }

    /// 滑动窗口翻译直至 waiting 清空或取消；进度随 `.trans` 持久化。
    /// 工程状态由 [`project::ProjectHandle`] 对应的 Actor 单写者持有，
    /// 只读快照供 web/SSE 无锁读取，运行时事件经 broadcast 广播。
    pub async fn translate_all(
        &self,
        project: project::ProjectHandle,
        cancel: CancellationToken,
    ) -> Result<TranslateReport, ServiceError> {
        let provider = project.project_config().provider;
        let client = self.client(&provider)?;
        engine::run(client, project, cancel).await
    }

    /// 手动导出译文文件（不修改 .trans）；返回写出的文件数。
    pub fn export(&self, project: &Project, output_dir: &str, mode: ExportMode) -> Result<usize, ServiceError> {
        writer::export(project, output_dir, mode)
    }

    /// 把失败条目移回 waiting 重新排队；`filter` 为 `Some((file, line))` 时只重试单条。
    /// 返回移回的条目数；修改立即落盘。
    pub fn retry_failed(&self, project: &mut Project, filter: Option<(&str, u64)>) -> Result<usize, ServiceError> {
        let mut keys: Vec<(String, u64)> = Vec::new();
        for (file, m) in &project.failure_items {
            for &line in m.keys() {
                if filter.is_none_or(|(f, l)| f == file.as_str() && l == line) {
                    keys.push((file.clone(), line));
                }
            }
        }
        let moved = keys
            .iter()
            .filter(|(file, line)| project.move_to_waiting(file, *line))
            .count();
        project.save_trans()?;
        Ok(moved)
    }

    /// 重置指定条目（success/failure）回 waiting 重新翻译；修改立即落盘。
    pub fn reset_item(&self, project: &mut Project, file: &str, line: u64) -> Result<(), ServiceError> {
        if !project.move_to_waiting(file, line) {
            return Err(ServiceError::ItemNotFound {
                file: file.to_string(),
                line,
            });
        }
        project.save_trans()?;
        Ok(())
    }

    /// 修改成功条目的译文（人工校正）；修改立即落盘。
    pub fn update_target(
        &self,
        project: &mut Project,
        file: &str,
        line: u64,
        target: String,
    ) -> Result<(), ServiceError> {
        if !project.update_target(file, line, target.clone()) {
            return Err(ServiceError::ItemNotFound {
                file: file.to_string(),
                line,
            });
        }
        // 文件名条目（line 0）译文被人工修改 → 同步工程显示名（仅单文件对象）
        if line == 0 && matches!(project.info.object, ProjectObject::File { .. }) {
            project.info.translate_name = Some(target);
        }
        project.save_trans()?;
        Ok(())
    }
}

fn file_name_of(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

/// 递归扫描目录下全部 `*.txt` 文件，返回按字典序排序的完整路径。
fn scan_txt(dir: &str) -> Result<Vec<String>, ServiceError> {
    let mut out = Vec::new();
    let mut stack = vec![PathBuf::from(dir)];
    while let Some(d) = stack.pop() {
        let entries =
            std::fs::read_dir(&d).map_err(|source| ServiceError::Scan(format!("{}: {source}", d.display())))?;
        for entry in entries {
            let entry = entry.map_err(|source| ServiceError::Scan(format!("{}: {source}", d.display())))?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "txt") {
                out.push(path.to_string_lossy().into_owned());
            }
        }
    }
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        fs,
        path::PathBuf,
        sync::{
            Arc,
            atomic::{AtomicU32, Ordering},
        },
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    use domain::{
        llm::{ApiStyle, LlmConfig},
        project::{FailureItem, SuccessItem},
    };
    use parking_lot::RwLock;
    use tokio::sync::broadcast;
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::project::ProjectHandle;

    // ---------- helpers ----------

    fn provider_config(base_url: String) -> LlmConfig {
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

    fn service(base_url: String) -> Service {
        Service::new(Arc::new(RwLock::new(Config {
            providers:       BTreeMap::from([("test".to_string(), provider_config(base_url))]),
            server:          config::ServerConfig::default(),
            recent_projects: BTreeSet::new(),
        })))
    }

    fn project_config() -> ProjectConfig {
        ProjectConfig {
            task_limit:      4,
            retry_limit:     3,
            task_timeout_ms: 300_000,
            provider:        "test".to_string(),
        }
    }

    fn unique_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("service-test-{tag}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    async fn file_project(dir: &Path, name: &str, content: &str) -> (Service, Project, PathBuf) {
        let file = dir.join(name);
        fs::write(&file, content).unwrap();
        let svc = service("http://unused".into());
        let p = svc
            .create_project(
                ProjectObject::File {
                    path: file.to_string_lossy().into_owned(),
                },
                Language::ZhHans,
                Language::Ja,
                project_config(),
                |_| {},
            )
            .await
            .unwrap();
        (svc, p, file)
    }

    // ---------- create_project ----------

    #[tokio::test]
    async fn create_project_file_scans_lines_and_saves() {
        let dir = unique_dir("file");
        let (_svc, p, file) = file_project(&dir, "a.txt", "第一行\n\n   \n第二行\n").await;
        assert_eq!(p.info.original_name, "a.txt");
        assert_eq!(p.info.source_language, Language::ZhHans);
        assert_eq!(p.info.target_language, Language::Ja);
        let counts = p.counts();
        assert_eq!(counts.total, 3);
        assert_eq!(counts.ignore, 2);
        assert_eq!(counts.waiting, 3);
        assert_eq!(counts.success, 0);
        assert_eq!(p.info.run_status, RunStatus::NotStarted);
        // 键 = 实际行号（1-based，含空行）；line 0 = 文件名条目；空行/纯空白行 → ignore_items
        let path = file.to_str().unwrap();
        assert_eq!(p.source_items[path][&0], "a.txt");
        assert_eq!(p.source_items[path][&1], "第一行");
        assert_eq!(p.source_items[path][&4], "第二行");
        assert_eq!(p.ignore_items[path][&2], "");
        assert_eq!(p.ignore_items[path][&3], "   ");
        assert_eq!(p.waiting_items[path][&0].retry_times, 0);
        assert_eq!(p.waiting_items[path][&1].retry_times, 0);
        assert_eq!(p.waiting_items[path][&4].retry_times, 0);
        // full_source 与 waiting 行号一致（原文以 source_items 为权威）
        let src: Vec<_> = p.source_items[path].keys().copied().collect();
        let wait: Vec<_> = p.waiting_items[path].keys().copied().collect();
        assert_eq!(src, wait);
        // 已落盘 .trans
        let trans = file.with_extension("trans");
        assert!(trans.exists());
        let loaded = Project::load(&trans).unwrap();
        assert_eq!(
            serde_json::to_value(&loaded).unwrap(),
            serde_json::to_value(&p).unwrap()
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn create_project_folder_recursive_scan_and_indexes() {
        let dir = unique_dir("folder");
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("b.txt"), "乙1\n乙2\n").unwrap();
        fs::write(dir.join("sub").join("a.txt"), "甲1\n").unwrap();
        fs::write(dir.join("notes.md"), "忽略\n").unwrap();
        let svc = service("http://unused".into());
        let p = svc
            .create_project(
                ProjectObject::Folder {
                    path:       dir.to_string_lossy().into_owned(),
                    file_paths: Default::default(),
                },
                Language::ZhHans,
                Language::Ja,
                project_config(),
                |_| {},
            )
            .await
            .unwrap();
        // 字典序：b.txt < sub/a.txt
        let paths: Vec<_> = match &p.info.object {
            ProjectObject::Folder { file_paths, .. } => file_paths.iter().collect(),
            _ => unreachable!(),
        };
        assert_eq!(paths.len(), 2);
        assert!(paths[0].ends_with("/b.txt"));
        assert!(paths[1].ends_with("/sub/a.txt"));
        let b = dir.join("b.txt");
        let a = dir.join("sub").join("a.txt");
        assert_eq!(p.source_items[b.to_str().unwrap()][&1], "乙1");
        assert_eq!(p.source_items[b.to_str().unwrap()][&2], "乙2");
        assert_eq!(p.source_items[a.to_str().unwrap()][&1], "甲1");
        // 非 txt 忽略；不同文件同号行分离（外层键为文件路径）；每个文件含 line-0 文件名条目
        assert_eq!(p.counts().total, 5);
        assert!(dir.with_extension("trans").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn create_project_rejects_unknown_provider() {
        let svc = service("http://unused".into());
        let mut sc = project_config();
        sc.provider = "nope".into();
        let err = svc
            .create_project(
                ProjectObject::File { path: "x.txt".into() },
                Language::ZhHans,
                Language::Ja,
                sc,
                |_| {},
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::ProviderNotFound(_)), "{err}");
    }

    #[tokio::test]
    async fn create_project_name_becomes_line_zero_item() {
        let dir = unique_dir("name-item");
        let file = dir.join("a.txt");
        fs::write(&file, "你好\n").unwrap();
        // 建工程不发起任何 LLM 请求
        let svc = service("http://unused".into());
        let p = svc
            .create_project(
                ProjectObject::File {
                    path: file.to_string_lossy().into_owned(),
                },
                Language::ZhHans,
                Language::Ja,
                project_config(),
                |_| {},
            )
            .await
            .unwrap();
        // 文件名作为 `<file, 0>` 条目待翻译；translate_name 建工程时不翻译
        let path = file.to_str().unwrap();
        assert_eq!(p.source_items[path][&0], "a.txt");
        assert_eq!(p.waiting_items[path][&0].retry_times, 0);
        assert_eq!(p.info.translate_name, None);
        // 已随工程落盘
        let loaded = Project::load(&file.with_extension("trans")).unwrap();
        assert_eq!(loaded.source_items[path][&0], "a.txt");
        assert_eq!(loaded.info.translate_name, None);
        fs::remove_dir_all(&dir).unwrap();
    }

    // ---------- export ----------

    #[tokio::test]
    async fn export_translated_only_and_bilingual() {
        let dir = unique_dir("export");
        let (svc, mut p, file) = file_project(&dir, "a.txt", "你好\n\n再见\n").await;
        let path = file.to_str().unwrap().to_string();
        p.success_items.insert(
            path.clone(),
            BTreeMap::from([(
                1,
                SuccessItem {
                    target: "こんにちは".into(),
                },
            )]),
        );
        p.failure_items
            .insert(path, BTreeMap::from([(3, FailureItem { reason: "x".into() })]));
        let out = dir.join("out");
        let n = svc
            .export(&p, out.to_str().unwrap(), ExportMode::TranslatedOnly)
            .unwrap();
        assert_eq!(n, 1);
        // 成功行替换、失败行保留原文、空行原样
        assert_eq!(
            fs::read_to_string(out.join("a.txt.translated.txt")).unwrap(),
            "こんにちは\n\n再见\n"
        );
        let n = svc
            .export(&p, out.to_str().unwrap(), ExportMode::Bilingual)
            .unwrap();
        assert_eq!(n, 1);
        assert_eq!(
            fs::read_to_string(out.join("a.txt.translated.txt")).unwrap(),
            "你好\tこんにちは\n\n再见\n"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn export_folder_preserves_subdirs() {
        let dir = unique_dir("export-folder");
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("b.txt"), "乙\n").unwrap();
        fs::write(dir.join("sub").join("a.txt"), "甲\n").unwrap();
        let svc = service("http://unused".into());
        let p = svc
            .create_project(
                ProjectObject::Folder {
                    path:       dir.to_string_lossy().into_owned(),
                    file_paths: Default::default(),
                },
                Language::ZhHans,
                Language::Ja,
                project_config(),
                |_| {},
            )
            .await
            .unwrap();
        let out = dir.join("out");
        let n = svc
            .export(&p, out.to_str().unwrap(), ExportMode::TranslatedOnly)
            .unwrap();
        assert_eq!(n, 2);
        assert_eq!(fs::read_to_string(out.join("b.txt.translated.txt")).unwrap(), "乙\n");
        assert_eq!(
            fs::read_to_string(out.join("sub").join("a.txt.translated.txt")).unwrap(),
            "甲\n"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    // ---------- retry / reset / update ----------

    #[tokio::test]
    async fn retry_failed_moves_all_failures_to_waiting() {
        let dir = unique_dir("retry-all");
        let (_svc, mut p, file) = file_project(&dir, "a.txt", "你好\n再见\n行三\n").await;
        let path = file.to_str().unwrap().to_string();
        p.failure_items.insert(
            path.clone(),
            BTreeMap::from([
                (1, FailureItem { reason: "r1".into() }),
                (3, FailureItem { reason: "r2".into() }),
            ]),
        );
        p.success_items.insert(
            path.clone(),
            BTreeMap::from([(
                2,
                SuccessItem {
                    target: "こんにちは".into(),
                },
            )]),
        );
        // 模拟一轮翻译后的状态：waiting 已排空
        p.waiting_items.clear();
        let svc = service("http://unused".into());
        let n = svc.retry_failed(&mut p, None).unwrap();
        assert_eq!(n, 2);
        assert!(p.failure_items.is_empty());
        assert_eq!(p.waiting_items[&path][&1].retry_times, 0);
        assert_eq!(p.waiting_items[&path][&3].retry_times, 0);
        // success 不受影响
        assert_eq!(p.success_items[&path][&2].target, "こんにちは");
        // 已落盘
        let loaded = Project::load(&file.with_extension("trans")).unwrap();
        assert!(loaded.failure_items.is_empty());
        assert_eq!(loaded.waiting_items[&path].len(), 2);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn retry_failed_single_item_with_filter() {
        let dir = unique_dir("retry-one");
        let (_svc, mut p, file) = file_project(&dir, "a.txt", "你好\n再见\n行三\n").await;
        let path = file.to_str().unwrap().to_string();
        p.failure_items.insert(
            path.clone(),
            BTreeMap::from([
                (1, FailureItem { reason: "r1".into() }),
                (3, FailureItem { reason: "r2".into() }),
            ]),
        );
        // 模拟一轮翻译后的状态：waiting 已排空
        p.waiting_items.clear();
        let svc = service("http://unused".into());
        let n = svc.retry_failed(&mut p, Some((&path, 3))).unwrap();
        assert_eq!(n, 1);
        assert_eq!(p.failure_items[&path].len(), 1);
        assert!(p.failure_items[&path].contains_key(&1));
        assert_eq!(p.waiting_items[&path].len(), 1);
        assert!(p.waiting_items[&path].contains_key(&3));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn reset_item_moves_success_or_failure_to_waiting() {
        let dir = unique_dir("reset");
        let (_svc, mut p, file) = file_project(&dir, "a.txt", "你好\n再见\n行三\n").await;
        let path = file.to_str().unwrap().to_string();
        p.success_items.insert(
            path.clone(),
            BTreeMap::from([(
                2,
                SuccessItem {
                    target: "こんにちは".into(),
                },
            )]),
        );
        p.failure_items
            .insert(path.clone(), BTreeMap::from([(3, FailureItem { reason: "x".into() })]));
        let svc = service("http://unused".into());
        // success → waiting
        svc.reset_item(&mut p, &path, 2).unwrap();
        assert!(p.success_items.is_empty());
        assert!(p.waiting_items[&path].contains_key(&2));
        // failure → waiting
        svc.reset_item(&mut p, &path, 3).unwrap();
        assert!(p.failure_items.is_empty());
        assert!(p.waiting_items[&path].contains_key(&3));
        // 不存在
        let err = svc.reset_item(&mut p, &path, 99).unwrap_err();
        assert!(matches!(err, ServiceError::ItemNotFound { .. }), "{err}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn update_target_persists_and_rejects_non_success() {
        let dir = unique_dir("update");
        let (_svc, mut p, file) = file_project(&dir, "a.txt", "你好\n").await;
        let path = file.to_str().unwrap().to_string();
        p.success_items.insert(
            path.clone(),
            BTreeMap::from([(
                1,
                SuccessItem {
                    target: "こんにちは".into(),
                },
            )]),
        );
        let svc = service("http://unused".into());
        svc.update_target(&mut p, &path, 1, "おはよう".to_string())
            .unwrap();
        assert_eq!(p.success_items[&path][&1].target, "おはよう");
        let loaded = Project::load(&file.with_extension("trans")).unwrap();
        assert_eq!(loaded.success_items[&path][&1].target, "おはよう");
        // 非 success 条目报错
        let err = svc
            .update_target(&mut p, &path, 2, "x".to_string())
            .unwrap_err();
        assert!(matches!(err, ServiceError::ItemNotFound { .. }), "{err}");
        fs::remove_dir_all(&dir).unwrap();
    }

    // ---------- engine ----------

    #[tokio::test]
    async fn translate_all_success_and_usage() {
        let dir = unique_dir("success");
        let file = dir.join("a.txt");
        fs::write(&file, "你好\n再见\n").unwrap();
        let (addr, _rx) = test_support::spawn(
            r#"{"choices":[{"message":{"content":"{\"0\":\"〇\",\"1\":\"こんにちは\",\"2\":\"さようなら\"}"}}],"usage":{"prompt_tokens":10,"completion_tokens":20}}"#,
            200,
        )
        .await;
        let svc = service(addr);
        let p = svc
            .create_project(
                ProjectObject::File {
                    path: file.to_string_lossy().into_owned(),
                },
                Language::ZhHans,
                Language::Ja,
                project_config(),
                |_| {},
            )
            .await
            .unwrap();
        let handle = ProjectHandle::spawn(p, broadcast::channel(64).0);
        let report = svc
            .translate_all(handle.clone(), CancellationToken::new())
            .await
            .unwrap();
        let p = handle.snapshot_project().await.unwrap();
        assert_eq!(
            report,
            TranslateReport {
                attempts:  3,
                succeeded: 3,
                failed:    0,
                cancelled: false,
            }
        );
        let path = file.to_str().unwrap();
        assert_eq!(p.success_items[path][&0], SuccessItem { target: "〇".into() });
        assert_eq!(
            p.success_items[path][&1],
            SuccessItem {
                target: "こんにちは".into(),
            }
        );
        assert_eq!(p.success_items[path][&2].target, "さようなら");
        assert!(p.waiting_items.is_empty());
        assert!(p.failure_items.is_empty());
        assert_eq!(p.info.run_status, RunStatus::Completed);
        // 文件名条目（line 0）成功 → 工程显示名
        assert_eq!(p.info.translate_name.as_deref(), Some("〇"));
        assert_eq!(p.info.token_usage.input, 30);
        assert_eq!(p.info.token_usage.output, 60);
        // 计数与 map 一致
        let counts = p.counts();
        assert_eq!(counts.success, 3);
        assert_eq!(counts.waiting, 0);
        assert_eq!(counts.total, 3);
        // 落盘快照可重载
        let loaded = Project::load(&file.with_extension("trans")).unwrap();
        assert_eq!(loaded.info.run_status, RunStatus::Completed);
        assert_eq!(loaded.success_items[path][&1].target, "こんにちは");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn retry_exceeding_limit_moves_to_failure() {
        let dir = unique_dir("retry");
        let file = dir.join("a.txt");
        fs::write(&file, "你好\n").unwrap();
        let (addr, _rx) = test_support::spawn(r#"{"error":"boom"}"#, 500).await;
        let svc = service(addr);
        let mut sc = project_config();
        sc.retry_limit = 1;
        let p = svc
            .create_project(
                ProjectObject::File {
                    path: file.to_string_lossy().into_owned(),
                },
                Language::ZhHans,
                Language::Ja,
                sc,
                |_| {},
            )
            .await
            .unwrap();
        let handle = ProjectHandle::spawn(p, broadcast::channel(64).0);
        let report = svc
            .translate_all(handle.clone(), CancellationToken::new())
            .await
            .unwrap();
        let p = handle.snapshot_project().await.unwrap();
        // 两条（文件名 + 1 行）× 2 次尝试后超限转 failure
        assert_eq!(report.attempts, 4);
        assert_eq!(report.failed, 2);
        assert!(p.success_items.is_empty());
        assert!(p.waiting_items.is_empty());
        let f = &p.failure_items[file.to_str().unwrap()][&1];
        assert!(f.reason.contains("api 返回 500"), "{}", f.reason);
        assert_eq!(p.counts().failure, 2);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn retry_succeeds_on_second_attempt() {
        let dir = unique_dir("retry-ok");
        let file = dir.join("a.txt");
        fs::write(&file, "你好\n").unwrap();
        let calls = Arc::new(AtomicU32::new(0));
        let (addr, _rx) = test_support::spawn_handler({
            let calls = calls.clone();
            move |_| {
                let calls = calls.clone();
                async move {
                    let n = calls.fetch_add(1, Ordering::SeqCst);
                    // 建工程不调用 LLM；n=0/1 为第一条条目两次尝试，其后成功
                    if n < 2 {
                        (500, r#"{"error":"temporary"}"#.to_string())
                    } else {
                        (
                            200,
                            r#"{"choices":[{"message":{"content":"{\"0\":\"〇\",\"1\":\"こんにちは\"}"}}],"usage":{"prompt_tokens":3,"completion_tokens":4}}"#
                                .to_string(),
                        )
                    }
                }
            }
        })
        .await;
        let svc = service(addr);
        let p = svc
            .create_project(
                ProjectObject::File {
                    path: file.to_string_lossy().into_owned(),
                },
                Language::ZhHans,
                Language::Ja,
                project_config(),
                |_| {},
            )
            .await
            .unwrap();
        let handle = ProjectHandle::spawn(p, broadcast::channel(64).0);
        let report = svc
            .translate_all(handle.clone(), CancellationToken::new())
            .await
            .unwrap();
        let p = handle.snapshot_project().await.unwrap();
        // 第一条：n=0 失败 → n=1 失败 → n=2 成功；第二条（文件名）n=3 成功
        assert_eq!(report.attempts, 4);
        assert_eq!(report.succeeded, 2);
        assert_eq!(p.success_items[file.to_str().unwrap()][&1].target, "こんにちは");
        assert_eq!(p.success_items[file.to_str().unwrap()][&0].target, "〇");
        assert_eq!(p.info.token_usage.input, 6);
        assert!(p.failure_items.is_empty());
        assert!(p.waiting_items.is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn missing_reply_id_retries_with_reason() {
        let dir = unique_dir("missing");
        let file = dir.join("a.txt");
        fs::write(&file, "你好\n").unwrap();
        // 回复只含未知编号 → 无可用译文 → 重试
        let (addr, _rx) = test_support::spawn(
            r#"{"choices":[{"message":{"content":"{\"999\":\"x\"}"}}],"usage":null}"#,
            200,
        )
        .await;
        let svc = service(addr);
        let mut sc = project_config();
        sc.retry_limit = 1;
        let p = svc
            .create_project(
                ProjectObject::File {
                    path: file.to_string_lossy().into_owned(),
                },
                Language::ZhHans,
                Language::Ja,
                sc,
                |_| {},
            )
            .await
            .unwrap();
        let handle = ProjectHandle::spawn(p, broadcast::channel(64).0);
        let report = svc
            .translate_all(handle.clone(), CancellationToken::new())
            .await
            .unwrap();
        let p = handle.snapshot_project().await.unwrap();
        // 文件名 + 1 行：回复都不含对应编号 → 两条各重试后失败
        assert_eq!(report.failed, 2);
        assert!(
            p.failure_items[file.to_str().unwrap()][&1]
                .reason
                .contains("回复中没有可解析的译文"),
            "{}",
            p.failure_items[file.to_str().unwrap()][&1].reason
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn timeout_marks_retry_and_eventually_fails() {
        let dir = unique_dir("timeout");
        let file = dir.join("a.txt");
        fs::write(&file, "你好\n").unwrap();
        let (addr, _rx) = test_support::spawn_handler(|_| async move {
            tokio::time::sleep(Duration::from_millis(500)).await;
            (
                200,
                r#"{"choices":[{"message":{"content":"{\"1\":\"こんにちは\"}"}}],"usage":null}"#.to_string(),
            )
        })
        .await;
        let svc = service(addr);
        let mut sc = project_config();
        sc.task_timeout_ms = 50;
        sc.retry_limit = 1;
        let p = svc
            .create_project(
                ProjectObject::File {
                    path: file.to_string_lossy().into_owned(),
                },
                Language::ZhHans,
                Language::Ja,
                sc,
                |_| {},
            )
            .await
            .unwrap();
        let handle = ProjectHandle::spawn(p, broadcast::channel(64).0);
        let report = svc
            .translate_all(handle.clone(), CancellationToken::new())
            .await
            .unwrap();
        let p = handle.snapshot_project().await.unwrap();
        // 文件名 + 1 行：两条都超时，各重试后失败
        assert_eq!(report.failed, 2);
        assert!(
            p.failure_items[file.to_str().unwrap()][&1]
                .reason
                .contains("请求超时"),
            "{}",
            p.failure_items[file.to_str().unwrap()][&1].reason
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn cancel_pauses_and_drains_in_flight() {
        let dir = unique_dir("cancel");
        let file = dir.join("a.txt");
        let content = (1..=10)
            .map(|i| format!("行{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&file, &content).unwrap();
        // 回显请求中的编号，保证成功收尾
        let (addr, _rx) = test_support::spawn_handler(|req| async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            // id 在 messages[1].content（`{"1":"行1"}`）里
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
        let svc = Arc::new(service(addr));
        let mut sc = project_config();
        sc.task_limit = 4;
        let p = svc
            .create_project(
                ProjectObject::File {
                    path: file.to_string_lossy().into_owned(),
                },
                Language::ZhHans,
                Language::Ja,
                sc,
                |_| {},
            )
            .await
            .unwrap();
        let project = ProjectHandle::spawn(p, broadcast::channel(64).0);
        let cancel = CancellationToken::new();
        let task = {
            let svc = svc.clone();
            let cancel = cancel.clone();
            let project = project.clone();
            tokio::spawn(async move { svc.translate_all(project, cancel).await.unwrap() })
        };
        tokio::time::sleep(Duration::from_millis(100)).await;
        cancel.cancel();
        let report = task.await.unwrap();
        let p = project.snapshot_project().await.unwrap();
        assert!(report.cancelled);
        assert_eq!(report.attempts, 4);
        assert_eq!(report.succeeded, 4);
        // 4 条 in-flight 收尾成功；其余（含文件名条目）仍在 waiting
        let path = file.to_str().unwrap();
        assert_eq!(p.success_items[path].len(), 4);
        assert_eq!(p.waiting_items[path].len(), 7);
        assert_eq!(p.counts().waiting, 7);
        assert_eq!(p.info.run_status, RunStatus::Paused);
        // 快照为 Paused，可恢复续跑
        let loaded = Project::load(&file.with_extension("trans")).unwrap();
        assert_eq!(loaded.info.run_status, RunStatus::Paused);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn translate_all_empty_waiting_returns_zero_report() {
        let dir = unique_dir("empty");
        let (_svc, mut p, _file) = file_project(&dir, "a.txt", "你好\n").await;
        p.waiting_items.clear();
        let svc = service("http://unused".into());
        let handle = ProjectHandle::spawn(p, broadcast::channel(64).0);
        let report = svc
            .translate_all(handle.clone(), CancellationToken::new())
            .await
            .unwrap();
        let p = handle.snapshot_project().await.unwrap();
        assert_eq!(
            report,
            TranslateReport {
                attempts:  0,
                succeeded: 0,
                failed:    0,
                cancelled: false,
            }
        );
        // 不改 run_status
        assert_eq!(p.info.run_status, RunStatus::NotStarted);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn translate_all_broadcasts_item_and_status_events() {
        let dir = unique_dir("events");
        let file = dir.join("a.txt");
        fs::write(&file, "你好\n再见\n").unwrap();
        let (addr, _rx) = test_support::spawn(
            r#"{"choices":[{"message":{"content":"{\"0\":\"〇\",\"1\":\"こんにちは\",\"2\":\"さようなら\"}"}}],"usage":null}"#,
            200,
        )
        .await;
        let svc = service(addr);
        let p = svc
            .create_project(
                ProjectObject::File {
                    path: file.to_string_lossy().into_owned(),
                },
                Language::ZhHans,
                Language::Ja,
                project_config(),
                |_| {},
            )
            .await
            .unwrap();
        let (tx, mut rx) = broadcast::channel(64);
        let handle = ProjectHandle::spawn(p, tx.clone());
        svc.translate_all(handle.clone(), CancellationToken::new())
            .await
            .unwrap();
        let mut events = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            events.push(ev);
        }
        assert!(
            matches!(
                events.first(),
                Some(event::ServiceEvent::Status {
                    status: RunStatus::Running,
                })
            ),
            "{events:?}"
        );
        let items = events
            .iter()
            .filter(|e| matches!(e, event::ServiceEvent::Item { .. }))
            .count();
        // 3 条 ×（派发 translating + 完成 success）= 6 个条目事件
        assert_eq!(items, 6, "{events:?}");
        assert!(
            matches!(
                events.last(),
                Some(event::ServiceEvent::Status {
                    status: RunStatus::Completed,
                })
            ),
            "{events:?}"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn translate_all_unknown_provider_errors() {
        let dir = unique_dir("provider");
        let (_svc, mut p, _file) = file_project(&dir, "a.txt", "你好\n").await;
        p.project_config.provider = "nope".into();
        let svc = service("http://unused".into());
        let handle = ProjectHandle::spawn(p, broadcast::channel(64).0);
        let err = svc
            .translate_all(handle.clone(), CancellationToken::new())
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::ProviderNotFound(_)), "{err}");
        fs::remove_dir_all(&dir).unwrap();
    }
}
