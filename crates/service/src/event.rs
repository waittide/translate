//! 引擎运行时事件：web 层经 SSE 转发给前端。

use domain::project::{ItemStatus, RunStatus};
use serde::Serialize;

/// 引擎运行时事件。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ServiceEvent {
    /// 单条目翻译完成（失败条目的失败原因可能来自单次尝试，条目可能仍在重试队列）。
    Item {
        file:   String,
        line:   u64,
        status: ItemStatus,
        /// 原文：行不在前端已加载页时，供其直接插入渲染
        source: String,
        /// success 时译文
        target: Option<String>,
        /// failure 时原因
        reason: Option<String>,
    },
    /// 运行状态切换。
    Status { status: RunStatus },
}

impl ServiceEvent {
    /// SSE 事件名：与 `#[serde(tag = "event")]` 的 rename 保持一致（改动需同步）。
    pub fn event_name(&self) -> &'static str {
        match self {
            ServiceEvent::Item { .. } => "item",
            ServiceEvent::Status { .. } => "status",
        }
    }
}
