use crate::api::admin::{
    ensure_unit_scope, error_response, resolve_admin_actor, DEFAULT_AGENT_ID_ALIAS,
    PRESET_TEMPLATE_USER_ID,
};
use crate::config::{PresetCustomizable, UserAgentPresetConfig};
use crate::core::blocking;
use crate::services::companions::{
    content_hash, delete_global_companion, export_global_companion, import_global_companion,
    list_global_companions, load_global_companion, load_global_companion_spritesheet,
    update_global_companion,
};
use crate::services::default_agent_sync::{self, load_effective_default_agent_record};
use crate::services::inner_visible::build_worker_card;
use crate::services::preset_worker_cards;
use crate::services::user_agent_presets::{
    self, find_preset_by_id, normalize_agent_approval_mode, normalize_agent_status,
    normalize_preset_questions, normalize_tool_list, resolve_preset_id, PresetSyncMode,
};
use crate::services::worker_card_settings::{
    canonicalize_preset_config, collect_configured_skill_names, customizable_payload,
    normalize_preset_icon_color, normalize_preset_icon_name, normalize_preset_icon_parts,
    normalize_preset_icon_payload,
};
use crate::state::AppState;
use axum::extract::{DefaultBodyLimit, Multipart, Path as AxumPath, Query, State};
use axum::http::{HeaderMap as AxumHeaderMap, HeaderValue as AxumHeaderValue, StatusCode};
use axum::response::Response;
use axum::{routing::get, routing::post, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use tracing::warn;

const MAX_COMPANION_UPLOAD_BYTES: usize = 24 * 1024 * 1024;
/// Frozen contract limits for the preset-binding list (`§12.2.1` 6)).
const BINDING_MAX_PAGE_SIZE: i64 = 100;
const BINDING_DEFAULT_PAGE_SIZE: i64 = 20;

pub(super) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/wunder/admin/preset_agents",
            get(admin_preset_agents_list).post(admin_preset_agents_update),
        )
        .route(
            "/wunder/admin/preset_agents/{preset_id}/worker_card",
            get(admin_preset_agent_worker_card),
        )
        .route(
            "/wunder/admin/preset_agents/{preset_id}/bindings",
            get(admin_preset_bindings_list),
        )
        .route(
            "/wunder/admin/preset_agents/bindings",
            post(admin_preset_bindings_mutate),
        )
        .route(
            "/wunder/admin/preset_agents/sync",
            post(admin_preset_agents_sync),
        )
        .route("/wunder/admin/agent_avatars", get(admin_agent_avatars_list))
        .route(
            "/wunder/admin/companions",
            get(admin_companions_list)
                .post(admin_companions_import)
                .layer(DefaultBodyLimit::max(MAX_COMPANION_UPLOAD_BYTES)),
        )
        .route(
            "/wunder/admin/companions/{id}",
            get(admin_companion_get)
                .patch(admin_companion_update)
                .delete(admin_companion_delete),
        )
        .route(
            "/wunder/admin/companions/{id}/spritesheet",
            get(admin_companion_spritesheet),
        )
        .route(
            "/wunder/admin/companions/{id}/package",
            get(admin_companion_export),
        )
}

async fn admin_preset_agents_list(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, Response> {
    let items = admin_preset_agent_items(&state).await?;
    Ok(Json(json!({ "data": { "items": items } })))
}

async fn admin_preset_agents_update(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<PresetAgentsUpdateRequest>,
) -> Result<Json<Value>, Response> {
    let current = state.config_store.get().await;
    let skill_name_keys = collect_configured_skill_names(&current);
    let existing_items =
        match preset_worker_cards::load_effective_preset_configs(&current, &skill_name_keys) {
            Ok(items) => items,
            Err(err) => {
                warn!("failed to load preset worker cards before admin save: {err}");
                current.user_agents.presets.clone()
            }
        };
    let normalized = normalize_preset_agents(&existing_items, payload.items, &skill_name_keys)?;
    let persisted_to_assets =
        preset_worker_cards::persist_preset_configs(&current, &normalized, &skill_name_keys)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if persisted_to_assets {
        if !current.user_agents.presets.is_empty() {
            state
                .config_store
                .update(|config| {
                    config.user_agents.presets.clear();
                })
                .await
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        }
    } else {
        let next_presets = normalized.clone();
        state
            .config_store
            .update(move |config| {
                config.user_agents.presets = next_presets;
            })
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
    let items = admin_preset_agent_items(&state).await?;
    Ok(Json(json!({ "data": { "items": items } })))
}

async fn admin_preset_agent_items(state: &AppState) -> Result<Vec<Value>, Response> {
    let config = state.config_store.get().await;
    let skill_name_keys = collect_configured_skill_names(&config);
    let configured = preset_worker_cards::load_effective_preset_configs_with_updated_at(
        &config,
        &skill_name_keys,
    )
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let preset_ids = configured
        .iter()
        .map(|item| item.preset.preset_id.clone())
        .collect::<Vec<_>>();
    // One grouped query for every preset instead of a per-preset count.
    let bound_users = state
        .user_store
        .count_preset_bound_users(&preset_ids)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .into_iter()
        .collect::<HashMap<_, _>>();
    let mut items = Vec::with_capacity(configured.len() + 1);
    items.push(admin_default_preset_agent_payload(state).await?);
    items.extend(configured.iter().filter_map(|item| {
        preset_agent_payload(
            &item.preset,
            &skill_name_keys,
            bound_users
                .get(&item.preset.preset_id)
                .copied()
                .unwrap_or(0),
            item.updated_at,
        )
    }));
    Ok(items)
}

async fn admin_preset_agent_worker_card(
    State(state): State<Arc<AppState>>,
    AxumPath(preset_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let cleaned = preset_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "preset_id is required".to_string(),
        ));
    }
    let config = state.config_store.get().await;
    let skill_name_keys = collect_configured_skill_names(&config);
    if cleaned.eq_ignore_ascii_case(DEFAULT_AGENT_ID_ALIAS) {
        let record = load_effective_default_agent_record(&state, PRESET_TEMPLATE_USER_ID)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        let document = build_worker_card(&record, &skill_name_keys);
        let filename = preset_worker_cards::export_file_name_for_default_agent(&record);
        return Ok(Json(json!({
            "data": {
                "filename": filename,
                "document": document,
            }
        })));
    }
    let presets = preset_worker_cards::load_effective_preset_configs(&config, &skill_name_keys)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let preset = presets
        .into_iter()
        .find(|item| item.preset_id == cleaned)
        .ok_or_else(|| {
            error_response(StatusCode::NOT_FOUND, "preset agent not found".to_string())
        })?;
    let document =
        preset_worker_cards::worker_card_document_from_preset_config(&preset, &skill_name_keys)
            .ok_or_else(|| {
                error_response(
                    StatusCode::BAD_REQUEST,
                    "failed to build preset worker card".to_string(),
                )
            })?;
    let filename = preset_worker_cards::export_file_name_for_preset(&preset);
    Ok(Json(json!({
        "data": {
            "filename": filename,
            "document": document,
        }
    })))
}

async fn admin_default_preset_agent_payload(state: &AppState) -> Result<Value, Response> {
    // Expose the template user's default agent as a special preset item for admin UI editing.
    let record = load_effective_default_agent_record(state, PRESET_TEMPLATE_USER_ID)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let icon =
        crate::services::worker_card_settings::normalize_icon_payload(record.icon.as_deref());
    let (icon_name, icon_color) = normalize_preset_icon_parts(Some(&icon));
    Ok(json!({
        "preset_id": DEFAULT_AGENT_ID_ALIAS,
        "revision": 1,
        "name": record.name.trim(),
        "description": record.description.trim(),
        "system_prompt": record.system_prompt.trim(),
        "preview_skill": record.preview_skill,
        "model_name": Value::Null,
        "icon": icon,
        "icon_name": icon_name,
        "icon_color": icon_color,
        "tool_names": normalize_tool_list(record.tool_names),
        "declared_tool_names": normalize_tool_list(record.declared_tool_names),
        "declared_skill_names": normalize_tool_list(record.declared_skill_names),
        "preset_questions": normalize_preset_questions(record.preset_questions),
        "approval_mode": normalize_agent_approval_mode(Some(&record.approval_mode)),
        "status": normalize_agent_status(Some(&record.status)),
        "is_default_agent": true,
        // The default agent is the user's own agent when no preset governs it.
        "bound_users": 0,
        "customizable": customizable_payload(&PresetCustomizable::ALL),
        "updated_at": record.updated_at,
    }))
}

async fn admin_preset_agents_sync(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    Json(payload): Json<PresetAgentsSyncRequest>,
) -> Result<Json<Value>, Response> {
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    let unit_scope = match payload.scope_unit_id.as_deref().map(str::trim) {
        Some(unit_id) if !unit_id.is_empty() => {
            ensure_unit_scope(&actor, Some(unit_id))?;
            Some(vec![unit_id.to_string()])
        }
        _ => actor.scope_unit_ids.as_ref().map(|ids| {
            let mut items = ids.iter().cloned().collect::<Vec<_>>();
            items.sort();
            items
        }),
    };
    let mode = if payload.mode.as_deref() == Some("force") {
        PresetSyncMode::Force
    } else {
        PresetSyncMode::Safe
    };
    let dry_run = payload.dry_run.unwrap_or(false);
    let preset_id = payload.preset_id.trim();
    if preset_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "preset_id is required".to_string(),
        ));
    }
    let summary = if preset_id.eq_ignore_ascii_case(DEFAULT_AGENT_ID_ALIAS) {
        default_agent_sync::sync_default_agent_across_users(
            &state,
            mode,
            unit_scope.as_deref(),
            dry_run,
        )
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    } else {
        let preset = find_preset_by_id(&state, preset_id)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        user_agent_presets::sync_preset_across_users(
            &state,
            &preset,
            mode,
            unit_scope.as_deref(),
            dry_run,
        )
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    };
    Ok(Json(json!({
        "data": {
            "preset_id": preset_id,
            "mode": match mode {
                PresetSyncMode::Safe => "safe",
                PresetSyncMode::Force => "force",
            },
            "dry_run": dry_run,
            "affected_users": summary.affected_users,
            "updated_agents": summary.updated_agents,
            "skipped_customized": summary.skipped_customized,
            "created_agents": summary.created_agents,
        }
    })))
}

/// `GET /admin/preset_agents/{preset_id}/bindings` — paginated bound users.
///
/// `page_size` is capped at [`BINDING_MAX_PAGE_SIZE`]; the page and its count
/// come from one joined query (no per-user lookups).
async fn admin_preset_bindings_list(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    AxumPath(preset_id): AxumPath<String>,
    Query(query): Query<PresetBindingsQuery>,
) -> Result<Json<Value>, Response> {
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    let unit_scope = actor.scope_unit_ids.as_ref().map(|ids| {
        let mut items = ids.iter().cloned().collect::<Vec<_>>();
        items.sort();
        items
    });
    let preset_id = preset_id.trim();
    if preset_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "preset_id is required".to_string(),
        ));
    }
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query
        .page_size
        .unwrap_or(BINDING_DEFAULT_PAGE_SIZE)
        .clamp(1, BINDING_MAX_PAGE_SIZE);
    let offset = (page - 1).saturating_mul(page_size);
    let keyword = query
        .keyword
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let (rows, total) = state
        .user_store
        .list_preset_bound_agents(preset_id, keyword, unit_scope.as_deref(), offset, page_size)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let items = rows
        .into_iter()
        .map(|row| {
            let customized = user_agent_presets::customized_field_names(&row.record);
            json!({
                "user_id": row.user_id,
                "username": row.username,
                "agent_id": row.record.agent_id,
                "customized": customized,
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "data": {
            "total": total,
            "items": items,
        }
    })))
}

/// `POST /admin/preset_agents/bindings` — bind / rebind / unbind.
///
/// `action=bind` ensures every listed user has exactly one instance bound to
/// `preset_id`; `action=unbind` requires `new_preset_id` (解绑即迁移).
async fn admin_preset_bindings_mutate(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    Json(payload): Json<PresetBindingsRequest>,
) -> Result<Json<Value>, Response> {
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let _actor = resolve_admin_actor(&state, &headers, true, &units)?;
    let preset_id = payload.preset_id.trim();
    if preset_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "preset_id is required".to_string(),
        ));
    }
    let user_ids = payload
        .user_ids
        .iter()
        .map(|user_id| user_id.trim().to_string())
        .filter(|user_id| !user_id.is_empty())
        .collect::<Vec<_>>();
    if user_ids.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "user_ids is required".to_string(),
        ));
    }
    let action = payload.action.trim();
    let unbind = match action {
        "bind" => false,
        "unbind" => true,
        other => {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                format!("unsupported action: {other}"),
            ))
        }
    };
    let new_preset_id = payload
        .new_preset_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if unbind && new_preset_id.is_none() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "new_preset_id is required for action=unbind".to_string(),
        ));
    }
    let target_preset_id = if unbind {
        new_preset_id.unwrap_or_default()
    } else {
        preset_id
    };
    let target = find_preset_by_id(&state, target_preset_id)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let require_current = unbind.then_some(preset_id);
    let outcome =
        user_agent_presets::apply_preset_binding(&state, &target, &user_ids, require_current)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "preset_id": target_preset_id,
            "affected_users": outcome.affected_users,
            "created_agents": outcome.created_agents,
            "rebound_agents": outcome.rebound_agents,
        }
    })))
}

/// Scan the agent-avatars directory and return a list of available avatar keys.
async fn admin_agent_avatars_list(
    State(_state): State<Arc<AppState>>,
) -> Result<Json<Value>, Response> {
    let avatar_dir = Path::new("frontend/src/assets/agent-avatars");
    if !avatar_dir.is_dir() {
        return Ok(Json(json!({
            "data": {
                "keys": [],
                "extension_map": {}
            }
        })));
    }

    let mut keys: Vec<String> = Vec::new();
    let mut extension_map: serde_json::Map<String, Value> = serde_json::Map::new();

    let read_dir = std::fs::read_dir(avatar_dir)
        .map_err(|err| error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;

    for entry in read_dir.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if extension.is_empty()
            || !matches!(extension.to_lowercase().as_str(), "png" | "jpg" | "jpeg")
        {
            continue;
        }
        // Extract key like "avatar-000" from "avatar-000.png"
        let key = file_name.replace(&format!(".{}", extension), "");
        if !key.starts_with("avatar-") {
            continue;
        }
        if !keys.contains(&key) {
            keys.push(key.clone());
        }
        // Record preferred extension (png > jpg > jpeg)
        let existing = extension_map
            .get(&key)
            .and_then(Value::as_str)
            .unwrap_or("");
        if extension.eq_ignore_ascii_case("png") || existing.is_empty() {
            extension_map.insert(key, Value::String(extension.to_lowercase()));
        }
    }

    keys.sort_by(|left, right| {
        let left_num = left
            .strip_prefix("avatar-")
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);
        let right_num = right
            .strip_prefix("avatar-")
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);
        left_num.cmp(&right_num)
    });

    Ok(Json(json!({
        "data": {
            "keys": keys,
            "extension_map": extension_map
        }
    })))
}

async fn admin_companions_list(
    State(_state): State<Arc<AppState>>,
) -> Result<Json<Value>, Response> {
    let items = list_global_companions()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": { "items": items } })))
}

async fn admin_companion_get(
    State(_state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let Some(item) = load_global_companion(&id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    else {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "companion not found".to_string(),
        ));
    };
    Ok(Json(json!({ "data": item })))
}

async fn admin_companion_spritesheet(
    State(_state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Response, Response> {
    let Some((mime, bytes)) = load_global_companion_spritesheet(&id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    else {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "companion not found".to_string(),
        ));
    };
    let mut response = Response::new(axum::body::Body::from(bytes.clone()));
    *response.status_mut() = StatusCode::OK;
    if let Ok(value) = AxumHeaderValue::from_str(&mime) {
        response
            .headers_mut()
            .insert(axum::http::header::CONTENT_TYPE, value);
    }
    if let Ok(value) = AxumHeaderValue::from_str(&bytes.len().to_string()) {
        response
            .headers_mut()
            .insert(axum::http::header::CONTENT_LENGTH, value);
    }
    Ok(response)
}

async fn admin_companions_import(
    State(_state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<Value>, Response> {
    let mut filename = String::new();
    let mut data = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    {
        if let Some(field_name) = field.name() {
            if field_name != "file" && field.file_name().is_none() {
                continue;
            }
        }
        filename = field.file_name().unwrap_or("companion.zip").to_string();
        data = field
            .bytes()
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
            .to_vec();
    }
    let checksum = content_hash(&data);
    let item = blocking::run_fs("api.admin.resource.import_companion", move || {
        import_global_companion(&filename, &data)
    })
    .await
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(
        json!({ "data": { "item": item, "sha256": checksum } }),
    ))
}

async fn admin_companion_update(
    State(_state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    Json(payload): Json<CompanionUpdateRequest>,
) -> Result<Json<Value>, Response> {
    let item = update_global_companion(
        &id,
        payload.display_name.as_deref().or(payload.name.as_deref()),
        payload.description.as_deref(),
    )
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": item })))
}

async fn admin_companion_delete(
    State(_state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let deleted = delete_global_companion(&id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": { "id": id, "deleted": deleted } })))
}

async fn admin_companion_export(
    State(_state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Response, Response> {
    let (filename, bytes) = export_global_companion(&id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let mut response = Response::new(axum::body::Body::from(bytes.clone()));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        AxumHeaderValue::from_static("application/zip"),
    );
    if let Ok(value) = AxumHeaderValue::from_str(&bytes.len().to_string()) {
        response
            .headers_mut()
            .insert(axum::http::header::CONTENT_LENGTH, value);
    }
    let disposition = format!(
        "attachment; filename=\"{}\"",
        filename
            .chars()
            .map(
                |ch| if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                    ch
                } else {
                    '_'
                }
            )
            .collect::<String>()
    );
    if let Ok(value) = AxumHeaderValue::from_str(&disposition) {
        response
            .headers_mut()
            .insert(axum::http::header::CONTENT_DISPOSITION, value);
    }
    Ok(response)
}

fn preset_agent_payload(
    record: &UserAgentPresetConfig,
    skill_name_keys: &HashSet<String>,
    bound_users: i64,
    updated_at: Option<f64>,
) -> Option<Value> {
    let preset_id = resolve_preset_id(&record.preset_id, &record.name);
    let normalized = canonicalize_preset_config(record, &preset_id, skill_name_keys)?;
    let UserAgentPresetConfig {
        revision,
        name,
        description,
        system_prompt,
        preview_skill,
        model_name,
        icon,
        icon_name,
        icon_color,
        tool_names,
        declared_tool_names,
        declared_skill_names,
        visible_unit_ids,
        preset_questions,
        approval_mode,
        status,
        customizable,
        ..
    } = normalized;
    let mut payload = json!({
        "preset_id": preset_id,
        "revision": revision.max(1),
        "name": name.trim(),
        "description": description.trim(),
        "system_prompt": system_prompt.trim(),
        "preview_skill": preview_skill,
        "model_name": user_agent_presets::normalize_optional_model_name(model_name.as_deref()),
        "icon": icon.unwrap_or_else(|| normalize_preset_icon_payload(None, Some(icon_name.as_str()), Some(icon_color.as_str()))),
        "icon_name": normalize_preset_icon_name(Some(icon_name.as_str())),
        "icon_color": normalize_preset_icon_color(Some(icon_color.as_str())),
        "tool_names": normalize_tool_list(tool_names),
        "declared_tool_names": normalize_tool_list(declared_tool_names),
        "declared_skill_names": normalize_tool_list(declared_skill_names),
        "visible_unit_ids": normalize_tool_list(visible_unit_ids),
        "preset_questions": normalize_preset_questions(preset_questions),
        "approval_mode": normalize_agent_approval_mode(Some(&approval_mode)),
        "status": normalize_agent_status(Some(&status)),
        "is_default_agent": false,
        "bound_users": bound_users,
        "customizable": customizable_payload(&customizable),
    });
    // Asset-backed presets report the worker-card mtime; nothing is invented
    // for config-declared presets without a file.
    if let Some(updated_at) = updated_at.filter(|value| value.is_finite()) {
        if let Value::Object(ref mut map) = payload {
            map.insert("updated_at".to_string(), json!(updated_at));
        }
    }
    Some(payload)
}

fn normalize_preset_agents(
    existing_items: &[UserAgentPresetConfig],
    items: Vec<PresetAgentUpsertItem>,
    skill_name_keys: &HashSet<String>,
) -> Result<Vec<UserAgentPresetConfig>, Response> {
    let existing_by_id = user_agent_presets::configs_by_preset_id(existing_items);
    let mut seen_names = HashSet::new();
    let mut seen_ids = HashSet::new();
    let mut output = Vec::with_capacity(items.len());
    for item in items {
        if item.preset_id.as_deref().is_some_and(|preset_id| {
            preset_id
                .trim()
                .eq_ignore_ascii_case(DEFAULT_AGENT_ID_ALIAS)
        }) {
            continue;
        }
        let cleaned_name = item.name.trim();
        if cleaned_name.is_empty() {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                "preset agent name is required".to_string(),
            ));
        }
        let dedupe_key = cleaned_name.to_ascii_lowercase();
        if !seen_names.insert(dedupe_key) {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                format!("duplicate preset agent name: {cleaned_name}"),
            ));
        }
        let preset_id =
            resolve_preset_id(item.preset_id.as_deref().unwrap_or_default(), cleaned_name);
        if !seen_ids.insert(preset_id.clone()) {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                format!("duplicate preset agent id: {preset_id}"),
            ));
        }
        let previous = existing_by_id
            .get(&preset_id)
            .and_then(|prev| canonicalize_preset_config(prev, &preset_id, skill_name_keys));
        let preview_skill = item.preview_skill.unwrap_or_else(|| {
            previous
                .as_ref()
                .map(|prev| prev.preview_skill)
                .unwrap_or(false)
        });
        // Containers are gone (single cloud root); keep whatever the preset had.
        let sandbox_container_id = previous
            .as_ref()
            .map(|prev| prev.sandbox_container_id)
            .unwrap_or_else(|| crate::storage::normalize_sandbox_container_id(1));
        let customizable = item
            .customizable
            .or_else(|| previous.as_ref().map(|prev| prev.customizable))
            .unwrap_or_default();
        let candidate = canonicalize_preset_config(
            &UserAgentPresetConfig {
                preset_id: preset_id.clone(),
                revision: previous.as_ref().map(|prev| prev.revision).unwrap_or(1),
                name: cleaned_name.to_string(),
                description: item.description.trim().to_string(),
                system_prompt: item.system_prompt.trim().to_string(),
                preview_skill,
                model_name: user_agent_presets::normalize_optional_model_name(
                    item.model_name.as_deref(),
                ),
                icon: Some(normalize_preset_icon_payload(
                    item.icon.as_deref(),
                    item.icon_name.as_deref(),
                    item.icon_color.as_deref(),
                )),
                icon_name: normalize_preset_icon_name(item.icon_name.as_deref()),
                icon_color: normalize_preset_icon_color(item.icon_color.as_deref()),
                sandbox_container_id,
                tool_names: normalize_tool_list(item.tool_names.unwrap_or_default()),
                declared_tool_names: normalize_tool_list(
                    item.declared_tool_names.unwrap_or_default(),
                ),
                declared_skill_names: normalize_tool_list(
                    item.declared_skill_names.unwrap_or_default(),
                ),
                visible_unit_ids: normalize_tool_list(item.visible_unit_ids.unwrap_or_default()),
                preset_questions: normalize_preset_questions(
                    item.preset_questions.unwrap_or_default(),
                ),
                approval_mode: normalize_agent_approval_mode(item.approval_mode.as_deref()),
                status: normalize_agent_status(item.status.as_deref()),
                customizable,
            },
            &preset_id,
            skill_name_keys,
        )
        .ok_or_else(|| {
            error_response(
                StatusCode::BAD_REQUEST,
                "preset agent name is required".to_string(),
            )
        })?;
        let revision_changed = previous.as_ref() != Some(&candidate);
        let revision = previous
            .map(|prev| {
                if revision_changed {
                    prev.revision.max(1) + 1
                } else {
                    prev.revision.max(1)
                }
            })
            .unwrap_or(1);
        output.push(UserAgentPresetConfig {
            revision,
            ..candidate
        });
    }
    Ok(output)
}

#[derive(Debug, Deserialize)]
struct PresetAgentsUpdateRequest {
    #[serde(default, alias = "presets")]
    items: Vec<PresetAgentUpsertItem>,
}

#[derive(Debug, Deserialize)]
struct PresetAgentUpsertItem {
    name: String,
    #[serde(default)]
    preset_id: Option<String>,
    #[serde(default)]
    description: String,
    #[serde(default)]
    system_prompt: String,
    #[serde(default)]
    preview_skill: Option<bool>,
    #[serde(default, alias = "modelName", alias = "model_name")]
    model_name: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    icon_name: Option<String>,
    #[serde(default)]
    icon_color: Option<String>,
    #[serde(default)]
    tool_names: Option<Vec<String>>,
    #[serde(default)]
    declared_tool_names: Option<Vec<String>>,
    #[serde(default)]
    declared_skill_names: Option<Vec<String>>,
    #[serde(default)]
    visible_unit_ids: Option<Vec<String>>,
    #[serde(default)]
    preset_questions: Option<Vec<String>>,
    #[serde(default)]
    approval_mode: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    customizable: Option<PresetCustomizable>,
}

#[derive(Debug, Deserialize)]
struct CompanionUpdateRequest {
    #[serde(default, alias = "displayName", alias = "display_name")]
    display_name: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PresetAgentsSyncRequest {
    preset_id: String,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    dry_run: Option<bool>,
    #[serde(default)]
    scope_unit_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct PresetBindingsQuery {
    #[serde(default)]
    page: Option<i64>,
    #[serde(default)]
    page_size: Option<i64>,
    #[serde(default)]
    keyword: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PresetBindingsRequest {
    preset_id: String,
    #[serde(default)]
    user_ids: Vec<String>,
    #[serde(default)]
    action: String,
    #[serde(default)]
    new_preset_id: Option<String>,
}
