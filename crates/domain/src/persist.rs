//! 原子文件写入：先写唯一临时文件再 `rename` 覆盖，避免写一半的损坏文件；
//! 临时文件名带进程内递增序号，多个写入者并发写同一目标时互不干扰。

use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

/// 进程内递增序号，保证并发写同一目标的临时文件名互不冲突。
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// 原子写：内容先写入 `<path>.<seq>.tmp`（同目录）再 rename 覆盖。
/// 父目录不存在时先创建（全新安装的配置目录等场景）。
pub fn atomic_write(path: &Path, content: &str) -> std::io::Result<()> {
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = PathBuf::from(format!("{}.{}.tmp", path.display(), seq));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 父目录不存在时必须先创建（全新安装时配置目录尚未生成）。
    #[test]
    fn creates_missing_parent_directory() {
        let dir = std::env::temp_dir().join(format!(
            "persist-test-{}-{}",
            std::process::id(),
            TMP_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let path = dir.join("nested").join("config.toml");
        atomic_write(&path, "value = 1\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "value = 1\n");
        assert!(leftover_tmp(&dir).is_empty(), "不应残留临时文件");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// 覆盖已有文件不残留临时文件。
    #[test]
    fn overwrite_leaves_no_tmp() {
        let dir = std::env::temp_dir().join(format!(
            "persist-test-{}-{}",
            std::process::id(),
            TMP_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "old").unwrap();
        atomic_write(&path, "new").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        assert!(leftover_tmp(&dir).is_empty(), "不应残留临时文件");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn leftover_tmp(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect()
    }
}
