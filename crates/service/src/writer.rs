//! 手动导出：纯译文 / 双语对照，不修改 `.trans`。

use std::{
    fs,
    path::{Path, PathBuf},
};

use domain::project::{Project, ProjectObject};

use crate::{ExportMode, ServiceError};

/// 导出译文文件到 `output_dir`；返回写出的文件数（空工程返回 0）。
pub(crate) fn export(project: &Project, output_dir: &str, mode: ExportMode) -> Result<usize, ServiceError> {
    let mut count = 0usize;
    for path in project.info.object.paths() {
        let text = fs::read_to_string(path).map_err(|source| ServiceError::Io {
            path: PathBuf::from(path),
            source,
        })?;
        let out_path = output_path(project, path, output_dir);
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|source| ServiceError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let mut out = String::new();
        // 键 = 实际行号（1-based，含空行）；空行原样输出且不查译文
        for (k, line) in text.lines().enumerate() {
            let k = k as u64 + 1;
            if line.trim().is_empty() {
                out.push_str(line);
            } else if let Some(item) = project.success_items.get(path).and_then(|m| m.get(&k)) {
                match mode {
                    ExportMode::TranslatedOnly => out.push_str(&item.target),
                    // 原文以文件行内容为准（与 source_items 同源）
                    ExportMode::Bilingual => {
                        out.push_str(line);
                        out.push('\t');
                        out.push_str(&item.target);
                    }
                }
            } else {
                // waiting / failure：输出原文
                out.push_str(line);
            }
            out.push('\n');
        }
        fs::write(&out_path, out).map_err(|source| ServiceError::Io {
            path: out_path.clone(),
            source,
        })?;
        count += 1;
    }
    Ok(count)
}

/// 输出路径：`File` → `<文件名>.translated.txt`；`Folder` → 相对子路径 + `.translated.txt`。
/// 带 `.translated.txt` 后缀，绝不覆盖原文文件。
fn output_path(project: &Project, path: &str, output_dir: &str) -> PathBuf {
    let rel = match &project.info.object {
        ProjectObject::File { .. } => Path::new(path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string()),
        ProjectObject::Folder { path: folder, .. } => Path::new(path)
            .strip_prefix(Path::new(folder))
            .unwrap_or(Path::new(path))
            .to_string_lossy()
            .into_owned(),
    };
    PathBuf::from(output_dir).join(format!("{rel}.translated.txt"))
}
