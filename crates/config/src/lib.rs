//! 全局配置 `~/.config/translate/config.toml` 加载。

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use domain::llm::LlmConfig;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 默认配置文件路径 `~/.config/translate/config.toml`。
pub fn default_path() -> PathBuf {
    dirs::config_dir()
        .map(|p| p.join("translate").join("config.toml"))
        .unwrap_or_else(|| PathBuf::from("config.toml"))
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    8080
}

/// Web 服务监听配置。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
        }
    }
}

/// 全局配置：`[providers.<name>]` 表为 LLM provider 定义；`[server]` 为 web 服务监听。
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Config {
    #[serde(default)]
    pub providers:       BTreeMap<String, LlmConfig>,
    #[serde(default)]
    pub server:          ServerConfig,
    /// 最近打开/创建的工程（`.trans` 绝对路径），供 web 历史列表。
    #[serde(default)]
    pub recent_projects: BTreeSet<String>,
}

impl Config {
    /// 从默认路径 [`default_path`] 加载。
    pub fn load() -> Result<Self, ConfigError> {
        Self::load_from(&default_path())
    }

    pub fn load_from(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        toml::from_str(&text).map_err(|source| ConfigError::Toml {
            path: path.to_path_buf(),
            source,
        })
    }

    /// 原子写：先写唯一临时文件再 `fs::rename` 覆盖，避免写一半的损坏文件。
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        let text = toml::to_string(self).map_err(|source| ConfigError::TomlSer {
            path: path.to_path_buf(),
            source,
        })?;
        domain::persist::atomic_write(path, &format!("{text}\n")).map_err(|source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        })
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("配置文件读写失败: {path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("解析配置文件失败: {path}: {source}")]
    Toml { path: PathBuf, source: toml::de::Error },
    #[error("序列化配置文件失败: {path}: {source}")]
    TomlSer { path: PathBuf, source: toml::ser::Error },
}

#[cfg(test)]
mod tests {
    use domain::llm::ApiStyle;

    use super::*;

    fn config() -> Config {
        Config {
            providers:       BTreeMap::from([(
                "claude".to_string(),
                LlmConfig {
                    api_style:   ApiStyle::Message,
                    base_url:    "http://127.0.0.1:13000/v1".to_string(),
                    api_key:     "k".to_string(),
                    model:       "claude-fable-5".to_string(),
                    temperature: Some(0.3),
                    top_p:       None,
                    top_k:       None,
                },
            )]),
            server:          ServerConfig::default(),
            recent_projects: BTreeSet::from(["/tmp/a.trans".to_string()]),
        }
    }

    #[test]
    fn missing_sections_default_to_empty() {
        let text = r#"
[server]
host = "127.0.0.1"
port = 18081
"#;
        let cfg: Config = toml::from_str(text).unwrap();
        assert!(cfg.providers.is_empty());
        assert!(cfg.recent_projects.is_empty());
        assert_eq!(cfg.server.port, 18081);
    }

    #[test]
    fn recent_projects_roundtrip() {
        let text = toml::to_string(&config()).unwrap();
        let cfg: Config = toml::from_str(&text).unwrap();
        assert_eq!(cfg.recent_projects, BTreeSet::from(["/tmp/a.trans".to_string()]));
    }

    #[test]
    fn missing_server_section_defaults_to_loopback() {
        let text = r#"
[providers.claude]
api_style = "message"
base_url = "http://127.0.0.1:13000/v1"
api_key = "k"
model = "m"
"#;
        let cfg: Config = toml::from_str(text).unwrap();
        assert_eq!(cfg.server.host, "127.0.0.1");
        assert_eq!(cfg.server.port, 8080);
    }

    #[test]
    fn server_only_section_defaults_providers() {
        let text = r#"
[server]
host = "127.0.0.1"
port = 18081
"#;
        let cfg: Config = toml::from_str(text).unwrap();
        assert!(cfg.providers.is_empty());
        assert_eq!(cfg.server.port, 18081);
    }

    #[test]
    fn save_then_load_roundtrip() {
        let path = std::env::temp_dir().join(format!("config-test-save-{}.toml", std::process::id()));
        config().save(&path).unwrap();
        let cfg = Config::load_from(&path).unwrap();
        assert_eq!(cfg.providers["claude"].model, "claude-fable-5");
        assert_eq!(cfg.server.port, 8080);
        assert!(!PathBuf::from(format!("{}.tmp", path.display())).exists());
        let leftovers: Vec<_> = std::fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(&format!("config-test-save-{}.toml.", std::process::id()))
            })
            .collect();
        assert!(leftovers.is_empty(), "不应残留临时文件: {leftovers:?}");
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn parses_toml_with_lowercase_api_style() {
        let text = r#"
[providers.claude]
api_style = "message"
base_url = "http://127.0.0.1:13000/v1"
api_key = "k"
model = "claude-fable-5"
temperature = 0.3
"#;
        let cfg: Config = toml::from_str(text).unwrap();
        let p = &cfg.providers["claude"];
        assert_eq!(p.api_style, ApiStyle::Message);
        assert_eq!(p.model, "claude-fable-5");
        assert_eq!(p.temperature, Some(0.3));
        assert_eq!(p.top_p, None);
    }

    #[test]
    fn roundtrip_toml() {
        let text = toml::to_string(&config()).unwrap();
        let cfg: Config = toml::from_str(&text).unwrap();
        assert_eq!(cfg.providers["claude"].api_style, ApiStyle::Message);
        assert_eq!(cfg.providers["claude"].base_url, "http://127.0.0.1:13000/v1");
    }

    #[test]
    fn load_from_file() {
        let path = std::env::temp_dir().join(format!("config-test-{}-1.toml", std::process::id()));
        std::fs::write(&path, toml::to_string(&config()).unwrap()).unwrap();
        let cfg = Config::load_from(&path).unwrap();
        assert_eq!(cfg.providers.len(), 1);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn missing_file_is_io_error() {
        let path = std::env::temp_dir().join(format!("config-test-{}-2.toml", std::process::id()));
        let err = Config::load_from(&path).unwrap_err();
        assert!(matches!(err, ConfigError::Io { .. }), "{err}");
    }

    #[test]
    fn invalid_toml_is_parse_error() {
        let path = std::env::temp_dir().join(format!("config-test-{}-3.toml", std::process::id()));
        std::fs::write(&path, "not = [valid").unwrap();
        let err = Config::load_from(&path).unwrap_err();
        assert!(matches!(err, ConfigError::Toml { .. }), "{err}");
        std::fs::remove_file(&path).unwrap();
    }
}
