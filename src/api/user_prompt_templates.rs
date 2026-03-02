use crate::api::user_context::resolve_user;
use crate::i18n;
use crate::services::user_prompt_templates::{
    self, normalize_locale, normalize_pack_id, DEFAULT_PACK_ID, SYSTEM_SEGMENTS,
};
use crate::state::AppState;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::{routing::delete, routing::get, routing::post, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/prompt_templates", get(list_prompt_templates))
        .route(
            "/wunder/prompt_templates/active",
            post(set_active_prompt_template),
        )
        .route(
            "/wunder/prompt_templates/file",
            get(get_prompt_template_file).put(update_prompt_template_file),
        )
        .route(
            "/wunder/prompt_templates/packs",
            post(create_prompt_template_pack),
        )
        .route(
            "/wunder/prompt_templates/packs/{pack_id}",
            delete(delete_prompt_template_pack),
        )
}

fn error_response(status: StatusCode, message: String) -> Response {
    crate::api::errors::error_response(status, message)
}

async fn list_prompt_templates(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
    let config = state.config_store.get().await;
    let active = user_prompt_templates::load_user_active_pack_id(&config, &user_id);
    let packs_root = user_prompt_templates::resolve_user_packs_root(&config, &user_id);
    let system_pack_id = user_prompt_templates::resolve_system_active_pack_id(&config);
    let system_pack_root =
        user_prompt_templates::resolve_system_pack_root(&config, &system_pack_id);

    let mut packs = vec![json!({
        "id": DEFAULT_PACK_ID,
        "is_default": true,
        "readonly": true,
        "sync_pack_id": system_pack_id,
        "path": system_pack_root.to_string_lossy(),
    })];

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
            if user_prompt_templates::validate_pack_id(&name).is_err() {
                continue;
            }
            packs.push(json!({
                "id": name,
                "is_default": false,
                "readonly": false,
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
            "default_sync_pack_id": user_prompt_templates::resolve_system_active_pack_id(&config),
        }
    })))
}

#[derive(Deserialize)]
struct SetActivePromptTemplateRequest {
    active: Option<String>,
}

async fn set_active_prompt_template(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<SetActivePromptTemplateRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
    let config = state.config_store.get().await;
    let pack_id = normalize_pack_id(payload.active.as_deref());
    user_prompt_templates::validate_pack_id(&pack_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err))?;
    if !pack_id.eq_ignore_ascii_case(DEFAULT_PACK_ID) {
        let pack_root = user_prompt_templates::resolve_user_pack_root(&config, &user_id, &pack_id);
        if !pack_root.is_dir() {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                "prompt template pack not found".to_string(),
            ));
        }
    }
    user_prompt_templates::save_user_active_pack_id(&config, &user_id, &pack_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err))?;
    crate::prompting::bump_system_prompt_templates_revision();
    Ok(Json(json!({
        "ok": true,
        "data": {
            "active": pack_id,
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
    headers: axum::http::HeaderMap,
    Query(query): Query<PromptTemplateFileQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
    let config = state.config_store.get().await;
    let active_pack_id = user_prompt_templates::load_user_active_pack_id(&config, &user_id);
    let pack_id = normalize_pack_id(query.pack_id.as_deref().or(Some(active_pack_id.as_str())));
    user_prompt_templates::validate_pack_id(&pack_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err))?;
    let locale = normalize_locale(query.locale.as_deref());
    let key = query.key.trim();
    if key.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.param_required"),
        ));
    }

    let system_pack_id = user_prompt_templates::resolve_system_active_pack_id(&config);
    let system_pack_root =
        user_prompt_templates::resolve_system_pack_root(&config, &system_pack_id);
    let (path, exists, fallback_used, content, readonly, source_pack_id) = if pack_id
        .eq_ignore_ascii_case(DEFAULT_PACK_ID)
    {
        let path = user_prompt_templates::resolve_segment_path(&system_pack_root, &locale, key)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err))?;
        let exists = path.is_file();
        let content = tokio::fs::read_to_string(&path).await.unwrap_or_default();
        (path, exists, false, content, true, system_pack_id)
    } else {
        let pack_root = user_prompt_templates::resolve_user_pack_root(&config, &user_id, &pack_id);
        if !pack_root.is_dir() {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                "prompt template pack not found".to_string(),
            ));
        }
        let path = user_prompt_templates::resolve_segment_path(&pack_root, &locale, key)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err))?;
        let exists = path.is_file();
        if exists {
            let content = tokio::fs::read_to_string(&path).await.unwrap_or_default();
            (path, true, false, content, false, pack_id.clone())
        } else {
            let fallback_path =
                user_prompt_templates::resolve_segment_path(&system_pack_root, &locale, key)
                    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err))?;
            let content = tokio::fs::read_to_string(&fallback_path)
                .await
                .unwrap_or_default();
            (path, false, true, content, false, system_pack_id)
        }
    };

    Ok(Json(json!({
        "data": {
            "pack_id": pack_id,
            "locale": locale,
            "key": key,
            "path": path.to_string_lossy(),
            "exists": exists,
            "fallback_used": fallback_used,
            "readonly": readonly,
            "source_pack_id": source_pack_id,
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
    headers: axum::http::HeaderMap,
    Json(payload): Json<UpdatePromptTemplateFileRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
    let config = state.config_store.get().await;
    let active_pack_id = user_prompt_templates::load_user_active_pack_id(&config, &user_id);
    let pack_id = normalize_pack_id(payload.pack_id.as_deref().or(Some(active_pack_id.as_str())));
    user_prompt_templates::validate_pack_id(&pack_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err))?;
    if pack_id.eq_ignore_ascii_case(DEFAULT_PACK_ID) {
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
    let pack_root = user_prompt_templates::resolve_user_pack_root(&config, &user_id, &pack_id);
    if !pack_root.is_dir() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "prompt template pack not found".to_string(),
        ));
    }
    let path = user_prompt_templates::resolve_segment_path(&pack_root, &locale, key)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err))?;
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
    headers: axum::http::HeaderMap,
    Json(payload): Json<CreatePromptTemplatePackRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
    let config = state.config_store.get().await;
    let pack_id = payload.pack_id.trim().to_string();
    user_prompt_templates::validate_pack_id(&pack_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err))?;
    if pack_id.eq_ignore_ascii_case(DEFAULT_PACK_ID) {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "cannot create default pack".to_string(),
        ));
    }
    let pack_root = user_prompt_templates::resolve_user_pack_root(&config, &user_id, &pack_id);
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
    user_prompt_templates::validate_pack_id(&copy_from)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err))?;
    let source_root = if copy_from.eq_ignore_ascii_case(DEFAULT_PACK_ID) {
        let system_pack_id = user_prompt_templates::resolve_system_active_pack_id(&config);
        user_prompt_templates::resolve_system_pack_root(&config, &system_pack_id)
    } else {
        let root = user_prompt_templates::resolve_user_pack_root(&config, &user_id, &copy_from);
        if !root.is_dir() {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                "copy_from pack not found".to_string(),
            ));
        }
        root
    };

    for locale in ["zh", "en"] {
        for (key, _) in SYSTEM_SEGMENTS {
            let src_path = user_prompt_templates::resolve_segment_path(&source_root, locale, key)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err))?;
            let content = tokio::fs::read_to_string(&src_path)
                .await
                .unwrap_or_default();
            let dst_path = user_prompt_templates::resolve_segment_path(&pack_root, locale, key)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err))?;
            if let Some(parent) = dst_path.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            }
            tokio::fs::write(&dst_path, content)
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
    headers: axum::http::HeaderMap,
    AxumPath(pack_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let user_id = resolved.user.user_id.clone();
    let config = state.config_store.get().await;
    user_prompt_templates::validate_pack_id(&pack_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err))?;
    if pack_id.trim().eq_ignore_ascii_case(DEFAULT_PACK_ID) {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "cannot delete default pack".to_string(),
        ));
    }
    let pack_root = user_prompt_templates::resolve_user_pack_root(&config, &user_id, &pack_id);
    if !pack_root.is_dir() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "prompt template pack not found".to_string(),
        ));
    }
    tokio::fs::remove_dir_all(&pack_root)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;

    let active = user_prompt_templates::load_user_active_pack_id(&config, &user_id);
    if active.eq_ignore_ascii_case(pack_id.trim()) {
        user_prompt_templates::save_user_active_pack_id(&config, &user_id, DEFAULT_PACK_ID)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err))?;
    }
    crate::prompting::bump_system_prompt_templates_revision();
    Ok(Json(json!({
        "ok": true,
        "data": {
            "pack_id": pack_id.trim(),
        }
    })))
}
