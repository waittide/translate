//! provider 配置 handler：CRUD 写回 config.toml + models 查询。

use std::{collections::BTreeMap, sync::Arc};

use axum::{
    Json,
    extract::{Path, State},
};
use domain::llm::LlmConfig;
use serde::Deserialize;

use crate::{ApiError, AppState};

/// POST /api/providers 请求体。
#[derive(Deserialize)]
pub struct ProviderUpsert {
    pub name:   String,
    pub config: LlmConfig,
}

/// PUT /api/providers/:name 请求体。
#[derive(Deserialize)]
pub struct ProviderUpdate {
    pub config: LlmConfig,
}

/// provider 列表（原样回显，含 api_key）。
pub async fn list(State(st): State<Arc<AppState>>) -> Json<BTreeMap<String, LlmConfig>> {
    let cfg = st.config.read();
    Json(cfg.providers.clone())
}

/// 新建 provider；写入共享配置（Service 客户端缓存自动失效重建）并持久化。
pub async fn upsert(
    State(st): State<Arc<AppState>>,
    Json(body): Json<ProviderUpsert>,
) -> Result<Json<LlmConfig>, ApiError> {
    {
        let mut cfg = st.config.write();
        cfg.providers.insert(body.name.clone(), body.config.clone());
    }
    st.save_config().await?;
    Ok(Json(body.config))
}

/// 更新 provider；写入共享配置（Service 客户端缓存自动失效重建）并持久化。
pub async fn upsert_named(
    State(st): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<ProviderUpdate>,
) -> Result<Json<LlmConfig>, ApiError> {
    {
        let mut cfg = st.config.write();
        if !cfg.providers.contains_key(&name) {
            return Err(ApiError::NotFound(format!("provider 未配置: {name}")));
        }
        cfg.providers.insert(name.clone(), body.config.clone());
    }
    st.save_config().await?;
    Ok(Json(body.config))
}

/// 删除 provider；写入共享配置（Service 客户端缓存自动失效重建）并持久化。
pub async fn delete(
    State(st): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let removed = {
        let mut cfg = st.config.write();
        cfg.providers.remove(&name)
    };
    if removed.is_none() {
        return Err(ApiError::NotFound(format!("provider 未配置: {name}")));
    }
    st.save_config().await?;
    Ok(Json(serde_json::json!({ "removed": name })))
}

/// 用指定 provider 请求模型列表（复用 Service 的客户端缓存）。
pub async fn models(State(st): State<Arc<AppState>>, Path(name): Path<String>) -> Result<Json<Vec<String>>, ApiError> {
    let client = st.service.client(&name)?;
    let models = client
        .models()
        .await
        .map_err(|e| ApiError::BadRequest(format!("models 请求失败: {e}")))?;
    Ok(Json(models))
}

/// 用任意（未保存的草稿）配置请求模型列表：新建/编辑表单的模型下拉提示。
pub async fn models_by_config(Json(config): Json<LlmConfig>) -> Result<Json<Vec<String>>, ApiError> {
    let client = llm::LlmClient::new(config);
    let models = client
        .models()
        .await
        .map_err(|e| ApiError::BadRequest(format!("models 请求失败: {e}")))?;
    Ok(Json(models))
}
