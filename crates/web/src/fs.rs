//! 文件浏览接口：为前端"浏览"按钮提供目录列表。
//!
//! 浏览器受安全沙箱限制拿不到本地绝对路径，原生文件选择器无法把路径交给
//! 后端读取，因此由服务端列举目录、前端以文件浏览器弹窗选择。

use std::{cmp::Ordering, path::PathBuf};

use axum::{Json, extract::Query};
use serde::{Deserialize, Serialize};

use crate::ApiError;

/// 列表查询参数；缺省时从用户主目录开始。
#[derive(Deserialize)]
pub struct ListQuery {
    pub path: Option<String>,
}

/// 目录条目类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    File,
    Dir,
}

#[derive(Serialize)]
pub struct EntryView {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: EntryKind,
}

#[derive(Serialize)]
pub struct ListReply {
    /// 规范化后的当前目录绝对路径
    pub path:    String,
    /// 上级目录；根目录为 null
    pub parent:  Option<String>,
    /// 目录条目，目录在前、文件在后，各自按名称字典序
    pub entries: Vec<EntryView>,
}

/// 展开 `~` 前缀；其余路径原样返回（约定为绝对路径）。
fn expand(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        return dirs::home_dir()
            .map(|home| home.join(rest))
            .unwrap_or_else(|| PathBuf::from(path));
    }
    if path == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(path));
    }
    PathBuf::from(path)
}

/// 目录列表；`path` 必须是存在的目录，否则 400。
pub async fn list(Query(query): Query<ListQuery>) -> Result<Json<ListReply>, ApiError> {
    let path = match query.path {
        Some(p) => expand(&p),
        None => dirs::home_dir().ok_or_else(|| ApiError::Internal("无法确定起始目录".into()))?,
    };

    let md = std::fs::metadata(&path).map_err(|e| ApiError::BadRequest(format!("无法访问 {}: {e}", path.display())))?;
    if !md.is_dir() {
        return Err(ApiError::BadRequest(format!("{} 不是目录", path.display())));
    }

    let mut entries: Vec<EntryView> = Vec::new();
    for entry in
        std::fs::read_dir(&path).map_err(|e| ApiError::BadRequest(format!("无法读取目录 {}: {e}", path.display())))?
    {
        let entry = entry.map_err(|e| ApiError::BadRequest(format!("无法读取目录 {}: {e}", path.display())))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let kind = match entry.file_type() {
            Ok(t) if t.is_dir() => EntryKind::Dir,
            _ => EntryKind::File,
        };
        entries.push(EntryView { name, kind });
    }

    entries.sort_by(|a, b| match (&a.kind, &b.kind) {
        (EntryKind::Dir, EntryKind::File) => Ordering::Less,
        (EntryKind::File, EntryKind::Dir) => Ordering::Greater,
        _ => a.name.cmp(&b.name),
    });

    let parent = path.parent().map(|p| p.to_string_lossy().into_owned());
    Ok(Json(ListReply {
        path: path.to_string_lossy().into_owned(),
        parent,
        entries,
    }))
}
