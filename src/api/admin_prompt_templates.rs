// 管理端系统提示词模板包：切换启用包、编辑分段提示词文件。
use crate::config::Config;
use crate::i18n;
use crate::state::AppState;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::{routing::delete, routing::get, routing::post, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const DEFAULT_PACK_ID: &str = "default";
const DEFAULT_PACKS_ROOT: &str = "./data/prompt_templates";
const PROMPTS_ROOT_ENV: &str = "WUNDER_PROMPTS_ROOT";

const SYSTEM_SEGMENTS: &[(&str, &str)] = &[
    ("role", "role.txt"),
    ("engineering", "engineering.txt"),
    ("tools_protocol", "tools_protocol.txt"),
    ("skills_protocol", "skills_protocol.txt"),
    ("memory", "memory.txt"),
    ("extra", "extra.txt"),
];

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/admin/prompt_templates", get(list_prompt_templates))
        .route(
            "/wunder/admin/prompt_templates/active",
            post(set_active_prompt_template),
        )
        .route(
            "/wunder/admin/prompt_templates/file",
            get(get_prompt_template_file).put(update_prompt_template_file),
        )
        .route(
            "/wunder/admin/prompt_templates/packs",
            post(create_prompt_template_pack),
        )
        .route(
            "/wunder/admin/prompt_templates/packs/{pack_id}",
            delete(delete_prompt_template_pack),
        )
}

fn error_response(status: StatusCode, message: String) -> Response {
    crate::api::errors::error_response(status, message)
}

fn normalize_pack_id(raw: Option<&str>) -> String {
    let cleaned = raw.unwrap_or("").trim();
    if cleaned.is_empty() {
        return DEFAULT_PACK_ID.to_string();
    }
    cleaned.to_string()
}

fn validate_pack_id(pack_id: &str) -> Result<(), Response> {
    let cleaned = pack_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.param_required"),
        ));
    }
    if cleaned.eq_ignore_ascii_case(DEFAULT_PACK_ID) {
        return Ok(());
    }
    if cleaned.len() > 64 {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "pack_id too long".to_string(),
        ));
    }
    if !cleaned
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "pack_id contains invalid characters".to_string(),
        ));
    }
    Ok(())
}

fn normalize_locale(raw: Option<&str>) -> String {
    let cleaned = raw.unwrap_or("").trim().to_ascii_lowercase();
    if cleaned.starts_with("en") {
        "en".to_string()
    } else if cleaned.starts_with("zh") {
        "zh".to_string()
    } else {
        let system = i18n::get_language().to_ascii_lowercase();
        if system.starts_with("en") {
            "en".to_string()
        } else {
            "zh".to_string()
        }
    }
}

fn resolve_packs_root(config: &Config) -> PathBuf {
    let root = config.prompt_templates.root.trim();
    let selected = if root.is_empty() { DEFAULT_PACKS_ROOT } else { root };
    let path = PathBuf::from(selected);
    if path.is_absolute() {
        path
    } else {
        resolve_prompts_root().join(path)
    }
}

fn resolve_pack_root(config: &Config, pack_id: &str) -> PathBuf {
    if pack_id.trim().eq_ignore_ascii_case(DEFAULT_PACK_ID) {
        return resolve_prompts_root();
    }
    resolve_packs_root(config).join(pack_id.trim())
}

fn resolve_prompts_root() -> PathBuf {
    let root = std::env::var(PROMPTS_ROOT_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    normalize_prompts_root(root)
}

fn normalize_prompts_root(root: PathBuf) -> PathBuf {
    if root.join("prompts").is_dir() {
        return root;
    }
    let looks_like_prompts_dir = root
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.eq_ignore_ascii_case("prompts"))
        .unwrap_or(false);
    if looks_like_prompts_dir && (root.join("zh").is_dir() || root.join("en").is_dir()) {
        if let Some(parent) = root.parent() {
            return parent.to_path_buf();
        }
    }
    root
}

fn resolve_segment_file_name(key: &str) -> Option<&'static str> {
    SYSTEM_SEGMENTS
        .iter()
        .find(|(segment_key, _)| segment_key.eq_ignore_ascii_case(key.trim()))
        .map(|(_, file)| *file)
}

fn resolve_segment_path(pack_root: &Path, locale: &str, key: &str) -> Result<PathBuf, Response> {
    let Some(file_name) = resolve_segment_file_name(key) else {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            format!("unknown segment key: {key}"),
        ));
    };
    Ok(pack_root.join(format!(
        "prompts/{locale}/system/{file_name}"
    )))
}

async fn list_prompt_templates(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let active = normalize_pack_id(Some(&config.prompt_templates.active));
    let packs_root = resolve_packs_root(&config);
    let default_root = resolve_prompts_root();

    let mut packs = Vec::new();
    packs.push(json!({
        "id": DEFAULT_PACK_ID,
        "is_default": true,
        "path": default_root.to_string_lossy(),
    }));

    if let Ok(mut dir) = tokio::fs::read_dir(&packs_root).await {
        while let Ok(Some(entry)) = dir.next_entry().await {
            let Ok(meta) = entry.metadata().await else {
                continue;
            };
            if !meta.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().trim().to_string();
            if name.is_empty() || name.eq_ignore_ascii_case(DEFAULT_PACK_ID) {
                continue;
            }
            packs.push(json!({
                "id": name,
                "is_default": false,
                "path": entry.path().to_string_lossy(),
            }));
        }
    }

    packs.sort_by(|a, b| {
        let a_id = a.get("id").and_then(Value::as_str).unwrap_or("");
        let b_id = b.get("id").and_then(Value::as_str).unwrap_or("");
        a_id.to_lowercase().cmp(&b_id.to_lowercase())
    });

    let segments = SYSTEM_SEGMENTS
        .iter()
        .map(|(key, file_name)| {
            json!({
                "key": key,
                "file": file_name,
            })
        })
        .collect::<Vec<_>>();

    Ok(Json(json!({
        "data": {
            "active": active,
            "packs_root": packs_root.to_string_lossy(),
            "packs": packs,
            "segments": segments,
        }
    })))
}

#[derive(Deserialize)]
struct SetActivePromptTemplateRequest {
    active: Option<String>,
}

async fn set_active_prompt_template(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SetActivePromptTemplateRequest>,
) -> Result<Json<Value>, Response> {
    let mut pack_id = normalize_pack_id(payload.active.as_deref());
    validate_pack_id(&pack_id)?;
    if !pack_id.eq_ignore_ascii_case(DEFAULT_PACK_ID) {
        let config = state.config_store.get().await;
        let pack_root = resolve_pack_root(&config, &pack_id);
        if !pack_root.is_dir() {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                "prompt template pack not found".to_string(),
            ));
        }
    } else {
        pack_id = DEFAULT_PACK_ID.to_string();
    }

    let updated = state
        .config_store
        .update(|config| {
            config.prompt_templates.active = pack_id.clone();
        })
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    Ok(Json(json!({
        "ok": true,
        "data": {
            "active": normalize_pack_id(Some(&updated.prompt_templates.active)),
        }
    })))
}

#[derive(Deserialize)]
struct PromptTemplateFileQuery {
    pack_id: Option<String>,
    locale: Option<String>,
    key: String,
}

async fn get_prompt_template_file(
    State(state): State<Arc<AppState>>,
    Query(query): Query<PromptTemplateFileQuery>,
) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let pack_id = normalize_pack_id(
        query
            .pack_id
            .as_deref()
            .or(Some(config.prompt_templates.active.as_str())),
    );
    validate_pack_id(&pack_id)?;
    let locale = normalize_locale(query.locale.as_deref());
    let key = query.key.trim();
    if key.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.param_required"),
        ));
    }

    let pack_root = resolve_pack_root(&config, &pack_id);
    let path = resolve_segment_path(&pack_root, &locale, key)?;
    let exists = path.is_file();

    let (content, fallback_used) = if exists {
        (
            tokio::fs::read_to_string(&path).await.unwrap_or_default(),
            false,
        )
    } else if !pack_id.eq_ignore_ascii_case(DEFAULT_PACK_ID) {
        let fallback_root = resolve_pack_root(&config, DEFAULT_PACK_ID);
        let fallback_path = resolve_segment_path(&fallback_root, &locale, key)?;
        (
            tokio::fs::read_to_string(&fallback_path).await.unwrap_or_default(),
            true,
        )
    } else {
        (String::new(), false)
    };

    Ok(Json(json!({
        "data": {
            "pack_id": pack_id,
            "locale": locale,
            "key": key,
            "path": path.to_string_lossy(),
            "exists": exists,
            "fallback_used": fallback_used,
            "content": content,
        }
    })))
}

#[derive(Deserialize)]
struct UpdatePromptTemplateFileRequest {
    pack_id: Option<String>,
    locale: Option<String>,
    key: String,
    content: String,
}

async fn update_prompt_template_file(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<UpdatePromptTemplateFileRequest>,
) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let pack_id = normalize_pack_id(
        payload
            .pack_id
            .as_deref()
            .or(Some(config.prompt_templates.active.as_str())),
    );
    validate_pack_id(&pack_id)?;
    if pack_id.trim().eq_ignore_ascii_case(DEFAULT_PACK_ID) {
        return Err(error_response(
            StatusCode::FORBIDDEN,
            "default prompt template pack is read-only".to_string(),
        ));
    }
    let locale = normalize_locale(payload.locale.as_deref());
    let key = payload.key.trim();
    if key.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.param_required"),
        ));
    }

    if !pack_id.eq_ignore_ascii_case(DEFAULT_PACK_ID) {
        let pack_root = resolve_pack_root(&config, &pack_id);
        if !pack_root.is_dir() {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                "prompt template pack not found".to_string(),
            ));
        }
    }

    let pack_root = resolve_pack_root(&config, &pack_id);
    let path = resolve_segment_path(&pack_root, &locale, key)?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
    tokio::fs::write(&path, payload.content)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    crate::prompting::bump_system_prompt_templates_revision();

    Ok(Json(json!({
        "ok": true,
        "data": {
            "pack_id": pack_id,
            "locale": locale,
            "key": key,
            "path": path.to_string_lossy(),
        }
    })))
}

#[derive(Deserialize)]
struct CreatePromptTemplatePackRequest {
    pack_id: String,
    copy_from: Option<String>,
}

async fn create_prompt_template_pack(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreatePromptTemplatePackRequest>,
) -> Result<Json<Value>, Response> {
    let pack_id = payload.pack_id.trim().to_string();
    validate_pack_id(&pack_id)?;
    if pack_id.eq_ignore_ascii_case(DEFAULT_PACK_ID) {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "cannot create default pack".to_string(),
        ));
    }
    let config = state.config_store.get().await;
    let pack_root = resolve_pack_root(&config, &pack_id);
    if pack_root.exists() {
        return Err(error_response(
            StatusCode::CONFLICT,
            "prompt template pack already exists".to_string(),
        ));
    }
    tokio::fs::create_dir_all(&pack_root)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    let copy_from = normalize_pack_id(payload.copy_from.as_deref());
    validate_pack_id(&copy_from)?;
    if !copy_from.eq_ignore_ascii_case(DEFAULT_PACK_ID) {
        let src_root = resolve_pack_root(&config, &copy_from);
        if !src_root.is_dir() {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                "copy_from pack not found".to_string(),
            ));
        }
    }

    let src_root = resolve_pack_root(&config, &copy_from);
    for locale in ["zh", "en"] {
        for (key, _) in SYSTEM_SEGMENTS {
            let src_path = resolve_segment_path(&src_root, locale, key)?;
            let content = tokio::fs::read_to_string(&src_path).await.unwrap_or_default();
            let dst_path = resolve_segment_path(&pack_root, locale, key)?;
            if let Some(parent) = dst_path.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            }
            tokio::fs::write(&dst_path, &content)
                .await
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        }
    }
    crate::prompting::bump_system_prompt_templates_revision();

    Ok(Json(json!({
        "ok": true,
        "data": {
            "pack_id": pack_id,
            "path": pack_root.to_string_lossy(),
            "copied_from": copy_from,
        }
    })))
}

async fn delete_prompt_template_pack(
    State(state): State<Arc<AppState>>,
    AxumPath(pack_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    validate_pack_id(&pack_id)?;
    if pack_id.trim().eq_ignore_ascii_case(DEFAULT_PACK_ID) {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "cannot delete default pack".to_string(),
        ));
    }
    let config = state.config_store.get().await;
    let pack_root = resolve_pack_root(&config, &pack_id);
    if !pack_root.is_dir() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "prompt template pack not found".to_string(),
        ));
    }
    tokio::fs::remove_dir_all(&pack_root)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    crate::prompting::bump_system_prompt_templates_revision();

    let active = normalize_pack_id(Some(&config.prompt_templates.active));
    if active.eq_ignore_ascii_case(pack_id.trim()) {
        let _ = state
            .config_store
            .update(|config| {
                config.prompt_templates.active = DEFAULT_PACK_ID.to_string();
            })
            .await;
    }

    Ok(Json(json!({
        "ok": true,
        "data": {
            "pack_id": pack_id.trim(),
        }
    })))
}
