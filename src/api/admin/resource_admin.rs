use crate::api::admin::{
    ensure_unit_scope, error_response, resolve_admin_actor, DEFAULT_AGENT_ID_ALIAS,
    PRESET_TEMPLATE_USER_ID,
};
use crate::config::UserAgentPresetConfig;
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
    canonicalize_preset_config, collect_configured_skill_names, normalize_preset_icon_color,
    normalize_preset_icon_name, normalize_preset_icon_parts, normalize_preset_icon_payload,
};
use crate::state::AppState;
use axum::extract::{DefaultBodyLimit, Multipart, Path as AxumPath, State};
use axum::http::{HeaderMap as AxumHeaderMap, HeaderValue as AxumHeaderValue, StatusCode};
use axum::response::Response;
use axum::{routing::get, routing::post, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use tracing::warn;

const MAX_COMPANION_UPLOAD_BYTES: usize = 24 * 1024 * 1024;

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
    let configured = preset_worker_cards::load_effective_preset_configs(&config, &skill_name_keys)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let mut items = Vec::with_capacity(configured.len() + 1);
    items.push(admin_default_preset_agent_payload(state).await?);
    items.extend(
        configured
            .iter()
            .filter_map(|preset| preset_agent_payload(preset, &skill_name_keys)),
    );
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
        let document = build_worker_card(&record, None, None, &skill_name_keys);
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
        "sandbox_container_id": crate::storage::normalize_sandbox_container_id(record.sandbox_container_id),
        "tool_names": normalize_tool_list(record.tool_names),
        "declared_tool_names": normalize_tool_list(record.declared_tool_names),
        "declared_skill_names": normalize_tool_list(record.declared_skill_names),
        "preset_questions": normalize_preset_questions(record.preset_questions),
        "approval_mode": normalize_agent_approval_mode(Some(&record.approval_mode)),
        "status": normalize_agent_status(Some(&record.status)),
        "is_default_agent": true,
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
    if payload
        .preset_id
        .trim()
        .eq_ignore_ascii_case(DEFAULT_AGENT_ID_ALIAS)
    {
        let template = admin_default_preset_agent_payload(&state).await?;
        let summary = default_agent_sync::sync_default_agent_across_users(
            &state,
            mode,
            unit_scope.as_deref(),
            payload.dry_run.unwrap_or(false),
        )
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        return Ok(Json(json!({
            "data": {
                "preset": {
                    "preset_id": DEFAULT_AGENT_ID_ALIAS,
                    "name": template.get("name").and_then(Value::as_str).unwrap_or(""),
                    "revision": 1,
                },
                "mode": match mode {
                    PresetSyncMode::Safe => "safe",
                    PresetSyncMode::Force => "force",
                },
                "dry_run": payload.dry_run.unwrap_or(false),
                "summary": {
                    "total_users": summary.total_users,
                    "linked_users": summary.linked_users,
                    "missing_users": summary.missing_users,
                    "up_to_date_agents": summary.up_to_date_agents,
                    "stale_agents": summary.stale_agents,
                    "safe_update_agents": summary.safe_update_agents,
                    "overridden_agents": summary.overridden_agents,
                    "force_update_agents": summary.force_update_agents,
                    "created_agents": summary.created_agents,
                    "updated_agents": summary.updated_agents,
                    "rebound_agents": summary.rebound_agents,
                }
            }
        })));
    }
    let preset = find_preset_by_id(&state, &payload.preset_id)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let summary = user_agent_presets::sync_preset_across_users(
        &state,
        &preset,
        mode,
        unit_scope.as_deref(),
        payload.dry_run.unwrap_or(false),
    )
    .await
    .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "preset": {
                "preset_id": preset.preset_id,
                "name": preset.name,
                "revision": preset.revision,
            },
            "mode": match mode {
                PresetSyncMode::Safe => "safe",
                PresetSyncMode::Force => "force",
            },
            "dry_run": payload.dry_run.unwrap_or(false),
            "summary": {
                "total_users": summary.total_users,
                "linked_users": summary.linked_users,
                "missing_users": summary.missing_users,
                "up_to_date_agents": summary.up_to_date_agents,
                "stale_agents": summary.stale_agents,
                "safe_update_agents": summary.safe_update_agents,
                "overridden_agents": summary.overridden_agents,
                "force_update_agents": summary.force_update_agents,
                "created_agents": summary.created_agents,
                "updated_agents": summary.updated_agents,
                "rebound_agents": summary.rebound_agents,
            }
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
    let item = tokio::task::spawn_blocking(move || import_global_companion(&filename, &data))
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
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
        sandbox_container_id,
        tool_names,
        declared_tool_names,
        declared_skill_names,
        visible_unit_ids,
        preset_questions,
        approval_mode,
        status,
        ..
    } = normalized;
    Some(json!({
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
        "sandbox_container_id": crate::storage::normalize_sandbox_container_id(sandbox_container_id),
        "tool_names": normalize_tool_list(tool_names),
        "declared_tool_names": normalize_tool_list(declared_tool_names),
        "declared_skill_names": normalize_tool_list(declared_skill_names),
        "visible_unit_ids": normalize_tool_list(visible_unit_ids),
        "preset_questions": normalize_preset_questions(preset_questions),
        "approval_mode": normalize_agent_approval_mode(Some(&approval_mode)),
        "status": normalize_agent_status(Some(&status)),
        "is_default_agent": false,
    }))
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
                sandbox_container_id: crate::storage::normalize_sandbox_container_id(
                    item.sandbox_container_id.unwrap_or(1),
                ),
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
    sandbox_container_id: Option<i32>,
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
