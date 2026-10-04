//! 翻译工程模型：`.trans` 工程文件（JSON）的数据结构与加载/保存。
//!
//! 条目状态互斥（success/failure/waiting）由 [`Project`] 的状态迁移方法维护；
//! 字段公开以支持序列化与直接构造，状态不变量由调用方保证（`.trans` 为外部输入，
//! [`Project::load`] 时容忍任意状态）。

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{language::Language, llm::LlmUsage};

/// 条目集合：外层键 = 文件路径（[`ProjectObject::Folder`] `file_paths` 中的完整路径；单文件为其自身路径），
/// 内层键 = 实际行号（1-based，含空行；空行记录在 [`Project::ignore_items`]）。
pub type ItemMap<T> = BTreeMap<String, BTreeMap<u64, T>>;

/// 业务配置，随工程文件持久化。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProjectConfig {
    /// 同时启动几个翻译
    pub task_limit:      u64,
    pub retry_limit:     u64,
    /// 单次请求超时，毫秒
    pub task_timeout_ms: u64,
    /// 使用的 provider 名，对应全局配置中 providers 表的键
    pub provider:        String,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            task_limit:      4,
            retry_limit:     3,
            task_timeout_ms: 300_000,
            provider:        String::new(),
        }
    }
}

/// 翻译成功条目：译文（原文见 [`Project::source_items`] 同键）。
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct SuccessItem {
    pub target: String,
}

/// 翻译失败条目：最后一次失败原因（原文见 [`Project::source_items`] 同键）。
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct FailureItem {
    pub reason: String,
}

/// 等待翻译的条目：重试元数据（原文见 [`Project::source_items`] 同键）。
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct WaitingItem {
    /// 已重试次数
    pub retry_times:      u64,
    /// 下次可重试时间戳（Unix 毫秒）；0 表示立即可试
    pub next_retry_at_ms: u64,
}

/// 单条目翻译状态（由条目表实时派生，不持久化）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    Success,
    Failure,
    /// 已从 waiting 取出、请求在途（仅内存态）
    Translating,
    Waiting,
}

impl ItemStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ItemStatus::Success => "success",
            ItemStatus::Failure => "failure",
            ItemStatus::Translating => "translating",
            ItemStatus::Waiting => "waiting",
        }
    }
}

/// 各状态条目计数（由条目表实时派生，不持久化）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ItemCounts {
    /// 可翻译行数（source_items 行数，不含 ignore）
    pub total:       u64,
    /// 忽略行数（空行/手动忽略）
    pub ignore:      u64,
    pub success:     u64,
    pub failure:     u64,
    /// 请求在途条数（仅内存态）
    pub translating: u64,
    pub waiting:     u64,
}

/// 翻译对象：单个文件或文件夹（其下所有 txt 文件）。
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ProjectObject {
    File {
        path: String,
    },
    Folder {
        path:       String,
        file_paths: BTreeSet<String>,
    },
}

impl ProjectObject {
    /// 参与翻译的全部文件路径：File 为其自身；Folder 为 file_paths（字典序）。
    pub fn paths(&self) -> Vec<&String> {
        match self {
            ProjectObject::File { path } => vec![path],
            ProjectObject::Folder { file_paths, .. } => file_paths.iter().collect(),
        }
    }
}

/// 翻译运行状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RunStatus {
    #[default]
    NotStarted,
    Running,
    Paused,
    Completed,
}

/// 工程元信息。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProjectInfo {
    pub original_name:   String,
    pub translate_name:  Option<String>,
    pub object:          ProjectObject,
    pub source_language: Language,
    pub target_language: Language,
    /// 累计运行时长，毫秒
    pub running_time_ms: u64,
    pub token_usage:     LlmUsage,
    /// 翻译运行状态
    pub run_status:      RunStatus,
}

/// 一个翻译工程：翻译对象的元信息 + 完整原文列表 + 各状态条目。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Project {
    pub project_config: ProjectConfig,
    pub info:           ProjectInfo,
    /// 完整原文列表（权威全表，含所有非空行；空行见 [`Project::ignore_items`]）
    pub source_items:   ItemMap<String>,
    /// 无需翻译的行（空行/纯空白行；后续手动忽略也记于此）
    pub ignore_items:   ItemMap<String>,
    pub success_items:  ItemMap<SuccessItem>,
    pub failure_items:  ItemMap<FailureItem>,
    pub waiting_items:  ItemMap<WaitingItem>,
    /// 请求在途条目（已从 waiting 取出、尚未返回）；仅内存态，不持久化。
    #[serde(skip, default)]
    pub in_flight:      ItemMap<()>,
}

impl Project {
    pub fn load(path: &Path) -> Result<Self, ProjectError> {
        let text = std::fs::read_to_string(path).map_err(|source| ProjectError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        serde_json::from_str(&text).map_err(|source| ProjectError::Json {
            path: path.to_path_buf(),
            source,
        })
    }

    /// 原子写：先写唯一临时文件再 rename 覆盖，避免写一半的损坏文件。
    pub fn save(&self, path: &Path) -> Result<(), ProjectError> {
        let text = serde_json::to_string_pretty(self).map_err(|source| ProjectError::Json {
            path: path.to_path_buf(),
            source,
        })?;
        crate::persist::atomic_write(path, &format!("{text}\n")).map_err(|source| ProjectError::Io {
            path: path.to_path_buf(),
            source,
        })
    }

    /// `.trans` 路径：对象路径去扩展名 + `.trans`（与对象同级）。
    pub fn trans_path(&self) -> PathBuf {
        let p = match &self.info.object {
            ProjectObject::File { path } | ProjectObject::Folder { path, .. } => PathBuf::from(path),
        };
        p.with_extension("trans")
    }

    /// 原子写工程文件到默认 `.trans` 路径。
    pub fn save_trans(&self) -> Result<(), ProjectError> {
        self.save(&self.trans_path())
    }

    /// 各条目表实时计数，避免持久化派生数据漂移。
    pub fn counts(&self) -> ItemCounts {
        ItemCounts {
            total:       self.source_items.values().map(|m| m.len() as u64).sum(),
            ignore:      self.ignore_items.values().map(|m| m.len() as u64).sum(),
            success:     self.success_items.values().map(|m| m.len() as u64).sum(),
            failure:     self.failure_items.values().map(|m| m.len() as u64).sum(),
            translating: self.in_flight.values().map(|m| m.len() as u64).sum(),
            waiting:     self.waiting_items.values().map(|m| m.len() as u64).sum(),
        }
    }

    /// 等待条目总数。
    pub fn waiting_count(&self) -> usize {
        self.waiting_items.values().map(|m| m.len()).sum()
    }

    /// 条目原文；不在 source_items 时返回 None。
    pub fn source_of(&self, file: &str, line: u64) -> Option<&str> {
        self.source_items
            .get(file)
            .and_then(|m| m.get(&line))
            .map(String::as_str)
    }

    /// 成功条目译文。
    pub fn target_of(&self, file: &str, line: u64) -> Option<&str> {
        self.success_items
            .get(file)
            .and_then(|m| m.get(&line))
            .map(|it| it.target.as_str())
    }

    /// 失败条目原因。
    pub fn failure_reason_of(&self, file: &str, line: u64) -> Option<&str> {
        self.failure_items
            .get(file)
            .and_then(|m| m.get(&line))
            .map(|it| it.reason.as_str())
    }

    /// 单条目状态；不在任何状态表（或不在 source_items）时返回 None。
    pub fn item_status(&self, file: &str, line: u64) -> Option<ItemStatus> {
        if self
            .success_items
            .get(file)
            .is_some_and(|m| m.contains_key(&line))
        {
            Some(ItemStatus::Success)
        } else if self
            .failure_items
            .get(file)
            .is_some_and(|m| m.contains_key(&line))
        {
            Some(ItemStatus::Failure)
        } else if self
            .in_flight
            .get(file)
            .is_some_and(|m| m.contains_key(&line))
        {
            Some(ItemStatus::Translating)
        } else if self
            .waiting_items
            .get(file)
            .is_some_and(|m| m.contains_key(&line))
        {
            Some(ItemStatus::Waiting)
        } else {
            None
        }
    }

    /// 第一个可立即尝试的等待条目 `(file, line)`（`next_retry_at_ms <= now`）。
    pub fn waiting_retryable_at(&self, now: u64) -> Option<(String, u64)> {
        self.waiting_items.iter().find_map(|(file, m)| {
            m.iter()
                .find(|(_, it)| it.next_retry_at_ms <= now)
                .map(|(line, _)| (file.clone(), *line))
        })
    }

    /// 全部等待条目中最早可重试时间戳。
    pub fn earliest_retry_at(&self) -> Option<u64> {
        self.waiting_items
            .values()
            .flat_map(|m| m.values())
            .map(|it| it.next_retry_at_ms)
            .min()
    }

    /// 取走一个等待条目（同时清掉可能变空的内层表），返回 `(retry_times, 原文)`。
    /// 不变量：waiting ⊆ source_items，故原文必然可取；违反时 panic 以便尽早暴露。
    pub fn take_waiting(&mut self, file: &str, line: u64) -> Option<(u64, String)> {
        let inner = self.waiting_items.get_mut(file)?;
        let item = inner.remove(&line)?;
        if inner.is_empty() {
            self.waiting_items.remove(file);
        }
        let source = self
            .source_items
            .get(file)
            .and_then(|m| m.get(&line))
            .expect("waiting 条目必然同时存在于 source_items")
            .clone();
        Some((item.retry_times, source))
    }

    /// 写入成功条目（条目已从 waiting 取走）。
    pub fn record_success(&mut self, file: &str, line: u64, target: String) {
        self.success_items
            .entry(file.to_string())
            .or_default()
            .insert(line, SuccessItem { target });
    }

    /// 写入失败条目（重试超限）。
    pub fn record_failure(&mut self, file: &str, line: u64, reason: String) {
        self.failure_items
            .entry(file.to_string())
            .or_default()
            .insert(line, FailureItem { reason });
    }

    /// 条目回 waiting 队列（失败重试），携带重试次数与下次可试时间。
    pub fn requeue(&mut self, file: &str, line: u64, retry_times: u64, next_retry_at_ms: u64) {
        self.waiting_items
            .entry(file.to_string())
            .or_default()
            .insert(
                line,
                WaitingItem {
                    retry_times,
                    next_retry_at_ms,
                },
            );
    }

    /// 修改成功条目的译文（人工校正）；条目不在 success 时返回 false。
    pub fn update_target(&mut self, file: &str, line: u64, target: String) -> bool {
        match self
            .success_items
            .get_mut(file)
            .and_then(|m| m.get_mut(&line))
        {
            Some(item) => {
                item.target = target;
                true
            }
            None => false,
        }
    }

    /// 把 success/failure 条目移回 waiting（重置重试元数据）；不在两者中时返回 false。
    pub fn move_to_waiting(&mut self, file: &str, line: u64) -> bool {
        let removed = self
            .success_items
            .get_mut(file)
            .and_then(|m| m.remove(&line))
            .is_some()
            || self
                .failure_items
                .get_mut(file)
                .and_then(|m| m.remove(&line))
                .is_some();
        if removed {
            if self.success_items.get(file).is_some_and(|m| m.is_empty()) {
                self.success_items.remove(file);
            }
            if self.failure_items.get(file).is_some_and(|m| m.is_empty()) {
                self.failure_items.remove(file);
            }
            self.waiting_items
                .entry(file.to_string())
                .or_default()
                .insert(line, WaitingItem::default());
        }
        removed
    }
}

#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("工程文件读写失败: {path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("解析工程文件失败: {path}: {source}")]
    Json { path: PathBuf, source: serde_json::Error },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> Project {
        Project {
            project_config: ProjectConfig::default(),
            info:           ProjectInfo {
                original_name:   "example.txt".to_string(),
                translate_name:  None,
                object:          ProjectObject::Folder {
                    path:       "/tmp/example".to_string(),
                    file_paths: BTreeSet::from(["/tmp/example/a.txt".to_string()]),
                },
                source_language: Language::ZhHans,
                target_language: Language::Ja,
                running_time_ms: 1234,
                token_usage:     LlmUsage {
                    input:  100,
                    output: 50,
                },
                run_status:      RunStatus::NotStarted,
            },
            source_items:   BTreeMap::from([(
                "/tmp/example/a.txt".to_string(),
                BTreeMap::from([
                    (1, "广告".to_string()),
                    (2, "你好".to_string()),
                    (3, "再见".to_string()),
                ]),
            )]),
            ignore_items:   BTreeMap::from([(
                "/tmp/example/a.txt".to_string(),
                BTreeMap::from([(1, "广告".to_string())]),
            )]),
            success_items:  BTreeMap::from([(
                "/tmp/example/a.txt".to_string(),
                BTreeMap::from([(
                    2,
                    SuccessItem {
                        target: "こんにちは".to_string(),
                    },
                )]),
            )]),
            failure_items:  BTreeMap::new(),
            waiting_items:  BTreeMap::from([(
                "/tmp/example/a.txt".to_string(),
                BTreeMap::from([(3, WaitingItem::default())]),
            )]),
            in_flight:      BTreeMap::new(),
        }
    }

    #[test]
    fn waiting_item_serializes_retry_fields() {
        let json = serde_json::to_value(WaitingItem::default()).unwrap();
        assert_eq!(json, serde_json::json!({"retry_times": 0, "next_retry_at_ms": 0}));
    }

    #[test]
    fn nested_items_serialize_with_file_line_keys() {
        // 工程文件整体序列化：外层文件路径、内层行号
        let p = project();
        let value = serde_json::to_value(&p).unwrap();
        assert_eq!(value["source_items"]["/tmp/example/a.txt"]["1"], "广告");
        assert_eq!(
            value["success_items"]["/tmp/example/a.txt"]["2"]["target"],
            "こんにちは"
        );
        assert_eq!(value["waiting_items"]["/tmp/example/a.txt"]["3"]["retry_times"], 0);
    }

    #[test]
    fn object_serializes_with_type_tag() {
        let json = serde_json::to_value(&project().info.object).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"type": "folder", "path": "/tmp/example", "file_paths": ["/tmp/example/a.txt"]})
        );
    }

    #[test]
    fn counts_derived_from_maps() {
        let c = project().counts();
        assert_eq!(
            c,
            ItemCounts {
                total:       3,
                ignore:      1,
                success:     1,
                failure:     0,
                translating: 0,
                waiting:     1,
            }
        );
    }

    #[test]
    fn roundtrip_json() {
        let json = serde_json::to_string(&project()).unwrap();
        let p: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(p.info.original_name, "example.txt");
        assert_eq!(p.info.object, project().info.object);
        assert_eq!(p.source_items, project().source_items);
        assert_eq!(p.success_items, project().success_items);
        assert_eq!(p.waiting_items, project().waiting_items);
        assert_eq!(p.project_config.task_limit, 4);
    }

    #[test]
    fn save_then_load() {
        let path = std::env::temp_dir().join(format!("project-test-{}.trans", std::process::id()));
        project().save(&path).unwrap();
        let p = Project::load(&path).unwrap();
        assert_eq!(p.info.target_language, Language::Ja);
        assert_eq!(p.info.running_time_ms, 1234);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn missing_file_is_io_error() {
        let path = std::env::temp_dir().join(format!("project-test-{}-2.trans", std::process::id()));
        let err = Project::load(&path).unwrap_err();
        assert!(matches!(err, ProjectError::Io { .. }), "{err}");
    }

    #[test]
    fn invalid_json_is_parse_error() {
        let path = std::env::temp_dir().join(format!("project-test-{}-3.trans", std::process::id()));
        std::fs::write(&path, "{not json").unwrap();
        let err = Project::load(&path).unwrap_err();
        assert!(matches!(err, ProjectError::Json { .. }), "{err}");
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn run_status_serializes_lowercase() {
        assert_eq!(serde_json::to_value(RunStatus::Running).unwrap(), "running");
        assert_eq!(
            serde_json::from_value::<RunStatus>(serde_json::json!("paused")).unwrap(),
            RunStatus::Paused
        );
        assert_eq!(RunStatus::default(), RunStatus::NotStarted);
    }

    #[test]
    fn save_is_atomic_no_tmp_leftover() {
        let dir = std::env::temp_dir().join(format!("project-test-atomic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("p.trans");
        project().save(&path).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("\"run_status\""));
        let p = Project::load(&path).unwrap();
        assert_eq!(p.info.run_status, RunStatus::NotStarted);
        assert!(!PathBuf::from(format!("{}.tmp", path.display())).exists());
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "不应残留临时文件: {leftovers:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn item_status_derived_from_maps() {
        let p = project();
        assert_eq!(p.item_status("/tmp/example/a.txt", 1), None); // 不在任何状态表
        assert_eq!(p.item_status("/tmp/example/a.txt", 2), Some(ItemStatus::Success));
        assert_eq!(p.item_status("/tmp/example/a.txt", 3), Some(ItemStatus::Waiting));
    }

    #[test]
    fn take_waiting_returns_retry_and_source() {
        let mut p = project();
        let (retry, source) = p.take_waiting("/tmp/example/a.txt", 3).unwrap();
        assert_eq!(retry, 0);
        assert_eq!(source, "再见");
        assert!(p.waiting_items.is_empty());
        assert_eq!(p.waiting_count(), 0);
        assert!(p.take_waiting("/tmp/example/a.txt", 3).is_none());
    }

    #[test]
    fn move_to_waiting_moves_success_or_failure() {
        let mut p = project();
        assert!(p.move_to_waiting("/tmp/example/a.txt", 2)); // success → waiting
        assert_eq!(p.item_status("/tmp/example/a.txt", 2), Some(ItemStatus::Waiting));
        assert!(p.success_items.is_empty());
        assert!(!p.move_to_waiting("/tmp/example/a.txt", 99)); // 不存在
    }

    #[test]
    fn requeue_and_record_keep_state_consistent() {
        let mut p = project();
        // 失败重试路径：行 1（waiting → failure）
        p.requeue("/tmp/example/a.txt", 1, 1, 123);
        assert_eq!(p.item_status("/tmp/example/a.txt", 1), Some(ItemStatus::Waiting));
        p.record_failure("/tmp/example/a.txt", 1, "原因".to_string());
        assert_eq!(p.item_status("/tmp/example/a.txt", 1), Some(ItemStatus::Failure));
        assert_eq!(p.failure_reason_of("/tmp/example/a.txt", 1), Some("原因"));
        // 成功路径：行 2（waiting → success）
        p.record_success("/tmp/example/a.txt", 2, "译文".to_string());
        assert_eq!(p.item_status("/tmp/example/a.txt", 2), Some(ItemStatus::Success));
        assert_eq!(p.target_of("/tmp/example/a.txt", 2), Some("译文"));
    }
}
