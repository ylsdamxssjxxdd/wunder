use crate::api::admin::{
    build_unit_map, build_user_activity_series_map, empty_user_activity_series, ensure_unit_scope,
    ensure_user_scope, error_response, external_link_payload, filter_units_by_scope,
    next_unit_sort_order, normalize_external_link_icon, normalize_external_link_levels,
    normalize_leader_ids, normalize_optional_id, normalize_optional_tool_access_list,
    normalize_tool_access_list, normalize_user_email, normalize_user_roles, normalize_user_status,
    now_ts, org_unit_payload, permission_denied, resolve_admin_actor, DEFAULT_TEST_USER_PASSWORD,
    DEFAULT_TEST_USER_PREFIX, MAX_TEST_USERS_PER_UNIT, TEST_USER_CLEANUP_BATCH_SIZE,
};
use crate::i18n;
use crate::org_units;
use crate::state::AppState;
use crate::storage::{ExternalLinkRecord, OrgUnitRecord, UserAccountRecord};
use crate::user_store::UserStore;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap as AxumHeaderMap, StatusCode};
use axum::response::Response;
use axum::{routing::delete, routing::get, routing::patch, routing::post, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use url::Url;
use uuid::Uuid;

const ORG_UNIT_NAME_SEPARATOR: &str = " / ";
const MAX_ORG_UNIT_LEVEL: i32 = 4;

pub(super) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/wunder/admin/org_units",
            get(admin_org_units_list).post(admin_org_units_create),
        )
        .route(
            "/wunder/admin/org_units/import",
            post(admin_org_units_import),
        )
        .route(
            "/wunder/admin/org_units/{unit_id}",
            patch(admin_org_units_update).delete(admin_org_units_delete),
        )
        .route(
            "/wunder/admin/external_links",
            get(admin_external_links_list).post(admin_external_links_upsert),
        )
        .route(
            "/wunder/admin/external_links/{link_id}",
            delete(admin_external_links_delete),
        )
        .route(
            "/wunder/admin/user_accounts",
            get(admin_user_accounts_list).post(admin_user_accounts_create),
        )
        .route(
            "/wunder/admin/user_accounts/test/seed",
            post(admin_user_accounts_seed),
        )
        .route(
            "/wunder/admin/user_accounts/test/cleanup",
            post(admin_user_accounts_cleanup),
        )
        .route(
            "/wunder/admin/user_accounts/{user_id}",
            patch(admin_user_accounts_update).delete(admin_user_accounts_delete),
        )
        .route(
            "/wunder/admin/user_accounts/{user_id}/password",
            post(admin_user_accounts_reset_password),
        )
        .route(
            "/wunder/admin/user_accounts/{user_id}/token_adjustment",
            post(admin_user_accounts_token_adjustment),
        )
        .route(
            "/wunder/admin/user_accounts/{user_id}/logout",
            post(admin_user_accounts_force_logout),
        )
        .route(
            "/wunder/admin/user_accounts/{user_id}/login_token",
            post(admin_user_accounts_login_token),
        )
        .route(
            "/wunder/admin/user_accounts/{user_id}/tool_access",
            get(admin_user_accounts_tool_access_get).put(admin_user_accounts_tool_access_update),
        )
        .route(
            "/wunder/admin/user_accounts/{user_id}/agent_access",
            get(admin_user_accounts_agent_access_get).put(admin_user_accounts_agent_access_update),
        )
        .route(
            "/wunder/admin/users/throughput/cleanup",
            post(admin_users_cleanup_throughput),
        )
        .route("/wunder/admin/users", get(admin_users))
        .route(
            "/wunder/admin/users/{user_id}/sessions",
            get(admin_user_sessions),
        )
        .route("/wunder/admin/users/{user_id}", delete(admin_user_delete))
}

async fn admin_org_units_list(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
) -> Result<Json<Value>, Response> {
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    let filtered = filter_units_by_scope(units, actor.scope_unit_ids.as_ref());
    let tree = org_units::build_unit_tree(&filtered);
    let items = filtered.iter().map(org_unit_payload).collect::<Vec<_>>();
    Ok(Json(json!({ "data": { "items": items, "tree": tree } })))
}

async fn admin_org_units_create(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    Json(payload): Json<OrgUnitCreateRequest>,
) -> Result<Json<Value>, Response> {
    let name = payload.name.trim();
    if name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    let parent_id = normalize_optional_id(payload.parent_id.as_deref());
    if actor.scope_unit_ids.is_some() {
        let Some(parent_id) = parent_id.as_deref() else {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("error.org_unit_parent_required"),
            ));
        };
        ensure_unit_scope(&actor, Some(parent_id))?;
    }
    let parent = parent_id
        .as_ref()
        .and_then(|id| units.iter().find(|unit| unit.unit_id == *id));
    if parent_id.is_some() && parent.is_none() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("error.org_unit_not_found"),
        ));
    }
    let level = parent.map_or(1, |parent| parent.level + 1);
    if level > MAX_ORG_UNIT_LEVEL {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.org_unit_level_exceeded"),
        ));
    }
    let sort_order = payload
        .sort_order
        .unwrap_or_else(|| next_unit_sort_order(&units, parent_id.as_deref()));
    let leader_ids = normalize_leader_ids(payload.leader_ids);
    let unit_id = format!("unit_{}", Uuid::new_v4().simple());
    let path = parent
        .map(|parent| format!("{}/{}", parent.path, unit_id))
        .unwrap_or_else(|| unit_id.clone());
    let path_name = parent
        .map(|parent| format!("{}{}{}", parent.path_name, ORG_UNIT_NAME_SEPARATOR, name))
        .unwrap_or_else(|| name.to_string());
    let now = now_ts();
    let record = OrgUnitRecord {
        unit_id: unit_id.clone(),
        parent_id: parent_id.clone(),
        name: name.to_string(),
        level,
        path,
        path_name,
        sort_order,
        leader_ids,
        created_at: now,
        updated_at: now,
    };
    state
        .user_store
        .upsert_org_unit(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": org_unit_payload(&record) })))
}

async fn admin_org_units_update(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    AxumPath(unit_id): AxumPath<String>,
    Json(payload): Json<OrgUnitUpdateRequest>,
) -> Result<Json<Value>, Response> {
    let cleaned = unit_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.param_required"),
        ));
    }
    let mut units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    let target_index = units
        .iter()
        .position(|unit| unit.unit_id == cleaned)
        .ok_or_else(|| {
            error_response(StatusCode::NOT_FOUND, i18n::t("error.org_unit_not_found"))
        })?;
    let target = units[target_index].clone();
    ensure_unit_scope(&actor, Some(&target.unit_id))?;

    let name = payload
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(target.name.as_str())
        .to_string();
    let parent_override = if payload.parent_id.is_some() {
        normalize_optional_id(payload.parent_id.as_deref())
    } else {
        None
    };
    let parent_id = if payload.parent_id.is_some() {
        parent_override
    } else {
        target.parent_id.clone()
    };
    if parent_id.as_deref() == Some(cleaned) {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.org_unit_cycle_not_allowed"),
        ));
    }
    if actor.scope_unit_ids.is_some() {
        if parent_id.is_none() {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("error.org_unit_parent_required"),
            ));
        }
        ensure_unit_scope(&actor, parent_id.as_deref())?;
    }
    let parent = parent_id
        .as_ref()
        .and_then(|id| units.iter().find(|unit| unit.unit_id == *id));
    if parent_id.is_some() && parent.is_none() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("error.org_unit_not_found"),
        ));
    }
    if let Some(parent) = parent {
        if parent.path == target.path || parent.path.starts_with(&format!("{}/", target.path)) {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("error.org_unit_cycle_not_allowed"),
            ));
        }
    }

    let parent_changed = parent_id != target.parent_id;
    let name_changed = name != target.name;
    let sort_order = if let Some(sort_order) = payload.sort_order {
        sort_order
    } else if parent_changed {
        next_unit_sort_order(&units, parent_id.as_deref())
    } else {
        target.sort_order
    };
    let leader_ids = if payload.leader_ids.is_some() {
        normalize_leader_ids(payload.leader_ids)
    } else {
        target.leader_ids.clone()
    };

    let now = now_ts();
    let mut updated_units = Vec::new();

    if parent_changed || name_changed {
        let old_path = target.path.clone();
        let old_path_name = target.path_name.clone();
        let old_level = target.level;
        let (new_level, new_path, new_path_name) = match parent {
            Some(parent) => {
                let level = parent.level + 1;
                if level > MAX_ORG_UNIT_LEVEL {
                    return Err(error_response(
                        StatusCode::BAD_REQUEST,
                        i18n::t("error.org_unit_level_exceeded"),
                    ));
                }
                (
                    level,
                    format!("{}/{}", parent.path, target.unit_id),
                    format!("{}{}{}", parent.path_name, ORG_UNIT_NAME_SEPARATOR, name),
                )
            }
            None => (1, target.unit_id.clone(), name.clone()),
        };
        let level_delta = new_level - old_level;
        let max_level = units
            .iter()
            .filter(|unit| unit.path == old_path || unit.path.starts_with(&format!("{old_path}/")))
            .map(|unit| unit.level)
            .max()
            .unwrap_or(old_level);
        if max_level + level_delta > MAX_ORG_UNIT_LEVEL {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("error.org_unit_level_exceeded"),
            ));
        }
        for unit in units.iter_mut() {
            if unit.path == old_path || unit.path.starts_with(&format!("{old_path}/")) {
                let suffix = unit.path.strip_prefix(&old_path).unwrap_or("");
                let suffix_name = unit.path_name.strip_prefix(&old_path_name).unwrap_or("");
                unit.path = format!("{new_path}{suffix}");
                unit.path_name = format!("{new_path_name}{suffix_name}");
                unit.level = (unit.level + level_delta).max(1);
                unit.updated_at = now;
                if unit.unit_id == cleaned {
                    unit.name = name.clone();
                    unit.parent_id = parent_id.clone();
                    unit.sort_order = sort_order;
                    unit.leader_ids = leader_ids.clone();
                }
                updated_units.push(unit.clone());
            }
        }
    } else {
        let mut updated = target.clone();
        updated.sort_order = sort_order;
        updated.leader_ids = leader_ids;
        updated.updated_at = now;
        updated_units.push(updated);
    }

    for unit in &updated_units {
        state
            .user_store
            .upsert_org_unit(unit)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
    let response_unit = updated_units
        .iter()
        .find(|unit| unit.unit_id == cleaned)
        .cloned()
        .unwrap_or_else(|| target.clone());
    Ok(Json(json!({ "data": org_unit_payload(&response_unit) })))
}

async fn admin_org_units_delete(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    AxumPath(unit_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let cleaned = unit_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.param_required"),
        ));
    }
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    ensure_unit_scope(&actor, Some(cleaned))?;
    if units
        .iter()
        .any(|unit| unit.parent_id.as_deref() == Some(cleaned))
    {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.org_unit_has_children"),
        ));
    }
    let unit_ids = vec![cleaned.to_string()];
    let (users, _) = state
        .user_store
        .list_users(None, Some(&unit_ids), 0, 1)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if !users.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.org_unit_has_users"),
        ));
    }
    state
        .user_store
        .delete_org_unit(cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": { "unit_id": cleaned } })))
}

async fn admin_org_units_import(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    Json(payload): Json<OrgUnitImportRequest>,
) -> Result<Json<Value>, Response> {
    let existing_units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, false, &existing_units)?;
    if actor.scope_unit_ids.is_some() {
        return Err(permission_denied());
    }
    let cleaned_units = payload.units;
    if cleaned_units.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "import units is empty".to_string(),
        ));
    }
    let now = now_ts();
    let imported_units = build_import_org_unit_records(&cleaned_units, now)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let imported_unit_ids = imported_units
        .iter()
        .map(|item| item.unit_id.clone())
        .collect::<HashSet<_>>();
    let root_units = imported_units
        .iter()
        .filter(|item| item.parent_id.is_none())
        .cloned()
        .collect::<Vec<_>>();
    let fallback_root = root_units
        .iter()
        .min_by(|left, right| {
            left.sort_order
                .cmp(&right.sort_order)
                .then_with(|| left.name.cmp(&right.name))
        })
        .ok_or_else(|| {
            error_response(
                StatusCode::BAD_REQUEST,
                "imported org units missing root".to_string(),
            )
        })?;
    let preferred_root_name = payload
        .migrate_user_root_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let preferred_root_unit_id = normalize_optional_id(payload.migrate_user_unit_id.as_deref());
    let migrate_target = root_units
        .iter()
        .find(|item| preferred_root_name == Some(item.name.as_str()))
        .cloned()
        .or_else(|| {
            preferred_root_unit_id.as_ref().and_then(|unit_id| {
                root_units
                    .iter()
                    .find(|item| item.unit_id == *unit_id)
                    .cloned()
            })
        })
        .unwrap_or_else(|| fallback_root.clone());

    let (all_users, _) = state
        .user_store
        .list_users(None, None, 0, i64::MAX / 4)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let mut migrated_user_count = 0_i64;
    for user in &all_users {
        let current_unit_id = user.unit_id.as_deref().map(str::trim).unwrap_or("");
        if current_unit_id.is_empty() {
            continue;
        }
        if imported_unit_ids.contains(current_unit_id) {
            continue;
        }
        let mut next_user = user.clone();
        next_user.unit_id = Some(migrate_target.unit_id.clone());
        next_user.updated_at = now;
        state
            .user_store
            .update_user(&next_user)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        migrated_user_count += 1;
    }

    for unit in &existing_units {
        let _ = state.user_store.delete_org_unit(&unit.unit_id);
    }
    for unit in &imported_units {
        state
            .user_store
            .upsert_org_unit(unit)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
    let tree = org_units::build_unit_tree(&imported_units);
    let items = imported_units
        .iter()
        .map(org_unit_payload)
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "data": {
            "items": items,
            "tree": tree,
            "imported_count": imported_units.len(),
            "migrated_user_count": migrated_user_count,
            "migrate_user_unit_id": migrate_target.unit_id,
            "migrate_user_root_name": migrate_target.name
        }
    })))
}

async fn admin_external_links_list(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, Response> {
    let records = state
        .storage
        .list_external_links(true)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let items = records
        .iter()
        .map(external_link_payload)
        .collect::<Vec<_>>();
    Ok(Json(json!({ "data": { "items": items } })))
}

async fn admin_external_links_upsert(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ExternalLinkUpsertRequest>,
) -> Result<Json<Value>, Response> {
    let title = payload.title.trim();
    let description = payload.description.trim();
    let url = payload.url.trim();
    if title.is_empty() || url.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let parsed_url = Url::parse(url)
        .map_err(|_| error_response(StatusCode::BAD_REQUEST, "外链 URL 格式无效".to_string()))?;
    if parsed_url.scheme() != "http" && parsed_url.scheme() != "https" {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "外链 URL 仅支持 http/https".to_string(),
        ));
    }
    let link_id = payload
        .link_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .unwrap_or_else(|| format!("ext_{}", Uuid::new_v4().simple()));
    let now = now_ts();
    let existing = state
        .storage
        .get_external_link(&link_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let created_at = existing
        .as_ref()
        .map(|record| record.created_at)
        .unwrap_or(now);
    let allowed_levels = normalize_external_link_levels(payload.allowed_levels.unwrap_or_default());
    let sort_order = payload.sort_order.unwrap_or(0);
    let icon = normalize_external_link_icon(payload.icon.as_deref());
    let record = ExternalLinkRecord {
        link_id: link_id.clone(),
        title: title.to_string(),
        description: description.to_string(),
        url: parsed_url.to_string(),
        icon,
        allowed_levels,
        sort_order,
        enabled: payload.enabled.unwrap_or(true),
        created_at,
        updated_at: now,
    };
    state
        .storage
        .upsert_external_link(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": external_link_payload(&record) })))
}

async fn admin_external_links_delete(
    State(state): State<Arc<AppState>>,
    AxumPath(link_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let cleaned = link_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    state
        .storage
        .delete_external_link(cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": { "link_id": cleaned } })))
}

async fn admin_user_accounts_list(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    Query(query): Query<UserAccountListQuery>,
) -> Result<Json<Value>, Response> {
    const DEFAULT_ACTIVITY_DAYS: i64 = 7;
    const MAX_ACTIVITY_DAYS: i64 = 14;

    let keyword = query
        .keyword
        .as_deref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());
    let requested_unit_id = normalize_optional_id(query.unit_id.as_deref());
    let offset = query.offset.unwrap_or(0).max(0);
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let activity_days = query
        .activity_days
        .unwrap_or(DEFAULT_ACTIVITY_DAYS)
        .clamp(3, MAX_ACTIVITY_DAYS);
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    let scoped_unit_ids = actor.scope_unit_ids.as_ref().map(|set| {
        let mut items = set.iter().cloned().collect::<Vec<_>>();
        items.sort();
        items
    });
    let requested_unit_scope = requested_unit_id
        .as_ref()
        .map(|unit_id| vec![unit_id.clone()]);
    let query_unit_scope = match (scoped_unit_ids.as_deref(), requested_unit_scope.as_deref()) {
        (Some(scope), Some(requested)) => {
            let requested_set = requested.iter().collect::<HashSet<_>>();
            let mut merged = scope
                .iter()
                .filter(|unit_id| requested_set.contains(unit_id))
                .cloned()
                .collect::<Vec<_>>();
            merged.sort();
            Some(merged)
        }
        (Some(scope), None) => Some(scope.to_vec()),
        (None, Some(requested)) => Some(requested.to_vec()),
        (None, None) => None,
    };
    let (users, total) = state
        .user_store
        .list_users(keyword, query_unit_scope.as_deref(), offset, limit)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let today = UserStore::today_string();
    let presence_now = now_ts();
    let active_sessions = state.monitor.list_sessions(true);
    let mut active_map: HashMap<String, i64> = HashMap::new();
    for session in active_sessions {
        let user_id = session
            .get("user_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if user_id.is_empty() {
            continue;
        }
        let entry = active_map.entry(user_id.to_string()).or_insert(0);
        *entry += 1;
    }
    let unit_map = build_unit_map(&units);
    let presence_map = state
        .control
        .presence
        .user_snapshot_many(users.iter().map(|user| user.user_id.as_str()), presence_now);
    let activity_user_ids = users
        .iter()
        .map(|user| user.user_id.trim())
        .filter(|user_id| !user_id.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let activity_series_map =
        build_user_activity_series_map(&state, &activity_user_ids, activity_days);
    let items = users
        .into_iter()
        .map(|user| {
            let unit = user
                .unit_id
                .as_ref()
                .and_then(|unit_id| unit_map.get(unit_id));
            let profile = UserStore::to_profile_with_unit(&user, unit);
            let token_status = UserStore::effective_token_balance_status(
                &user,
                unit.map(|item| item.level),
                Some(today.as_str()),
            );
            let active_count = active_map.get(&profile.id).copied().unwrap_or(0);
            let (presence_online, presence_last_seen) = presence_map
                .get(profile.id.as_str())
                .map(|snapshot| (snapshot.online, Some(snapshot.last_seen_at)))
                .unwrap_or((false, None));
            let activity_series = activity_series_map
                .get(profile.id.as_str())
                .cloned()
                .unwrap_or_else(|| empty_user_activity_series(activity_days));
            let mut value = serde_json::to_value(profile).unwrap_or_else(|_| json!({}));
            if let Value::Object(ref mut map) = value {
                map.insert("active_sessions".to_string(), json!(active_count));
                map.insert("online".to_string(), json!(presence_online));
                map.insert("last_seen_at".to_string(), json!(presence_last_seen));
                map.insert("token_balance".to_string(), json!(token_status.balance));
                map.insert(
                    "token_granted_total".to_string(),
                    json!(token_status.granted_total),
                );
                map.insert(
                    "token_used_total".to_string(),
                    json!(token_status.used_total),
                );
                map.insert(
                    "daily_token_grant".to_string(),
                    json!(token_status.daily_grant),
                );
                map.insert(
                    "last_token_grant_date".to_string(),
                    json!(token_status.last_grant_date),
                );
                // Legacy aliases for existing admin clients during migration.
                map.insert("daily_quota".to_string(), json!(token_status.granted_total));
                map.insert(
                    "daily_quota_used".to_string(),
                    json!(token_status.used_total),
                );
                map.insert(
                    "daily_quota_remaining".to_string(),
                    json!(token_status.balance),
                );
                map.insert(
                    "daily_quota_date".to_string(),
                    json!(token_status.last_grant_date),
                );
                map.insert("activity_series".to_string(), Value::Array(activity_series));
            }
            value
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({ "data": { "total": total, "items": items } })))
}

async fn admin_user_accounts_create(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    Json(payload): Json<UserAccountCreateRequest>,
) -> Result<Json<Value>, Response> {
    let username = payload.username.trim();
    let password = payload.password.trim();
    if username.is_empty() || password.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    let unit_id = normalize_optional_id(payload.unit_id.as_deref());
    if actor.scope_unit_ids.is_some() {
        let Some(unit_id) = unit_id.as_deref() else {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("error.org_unit_required"),
            ));
        };
        ensure_unit_scope(&actor, Some(unit_id))?;
    }
    if let Some(unit_id) = unit_id.as_deref() {
        let exists = units.iter().any(|unit| unit.unit_id == unit_id);
        if !exists {
            return Err(error_response(
                StatusCode::NOT_FOUND,
                i18n::t("error.org_unit_not_found"),
            ));
        }
    }
    let status = normalize_user_status(payload.status.as_deref());
    let roles = normalize_user_roles(payload.roles);
    let email = normalize_user_email(payload.email);
    let record = state
        .user_store
        .create_user(
            username,
            email,
            password,
            None,
            unit_id,
            roles,
            &status,
            payload.is_demo,
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if let Err(err) =
        crate::services::user_agent_presets::ensure_user_agent_bootstrap(&state, &record).await
    {
        tracing::warn!(
            "failed to bootstrap user agents after admin user create for {}: {err}",
            record.user_id
        );
    }
    let unit = record
        .unit_id
        .as_ref()
        .and_then(|unit_id| units.iter().find(|unit| unit.unit_id == *unit_id));
    Ok(Json(json!({
        "data": UserStore::to_profile_with_unit(&record, unit)
    })))
}

async fn admin_user_accounts_seed(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    Json(payload): Json<UserAccountSeedRequest>,
) -> Result<Json<Value>, Response> {
    let per_unit = payload.per_unit.unwrap_or(0);
    if per_unit <= 0 || per_unit > MAX_TEST_USERS_PER_UNIT {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_seed_count_invalid"),
        ));
    }
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    let scoped_units = filter_units_by_scope(units, actor.scope_unit_ids.as_ref());
    let password_hash = UserStore::hash_password(DEFAULT_TEST_USER_PASSWORD)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let now = now_ts();
    let access_level = UserStore::normalize_access_level(None);
    let capacity = scoped_units.len().saturating_mul(per_unit.max(0) as usize);
    let mut existing_test_user_ids = HashSet::new();
    let mut max_seed_serial = 0_u64;
    let mut offset = 0;
    loop {
        let (batch, total) = state
            .user_store
            .list_users(
                Some(DEFAULT_TEST_USER_PREFIX),
                None,
                offset,
                TEST_USER_CLEANUP_BATCH_SIZE,
            )
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        if batch.is_empty() {
            break;
        }
        for record in batch {
            let user_id = record.user_id.trim();
            if user_id.is_empty() {
                continue;
            }
            existing_test_user_ids.insert(user_id.to_string());
            if let Some(serial) = parse_seed_test_user_serial(user_id) {
                max_seed_serial = max_seed_serial.max(serial);
            }
        }
        offset += TEST_USER_CLEANUP_BATCH_SIZE;
        if offset >= total {
            break;
        }
    }
    let mut next_seed_serial = max_seed_serial.saturating_add(1).max(1);
    let mut records = Vec::with_capacity(capacity);
    for unit in &scoped_units {
        let token_grant = UserStore::default_daily_token_grant_by_level(Some(unit.level));
        for _ in 0..per_unit {
            let username = loop {
                let candidate = format!("{DEFAULT_TEST_USER_PREFIX}_{next_seed_serial}");
                next_seed_serial = next_seed_serial.saturating_add(1);
                if existing_test_user_ids.insert(candidate.clone()) {
                    break candidate;
                }
                if next_seed_serial == u64::MAX {
                    return Err(error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        i18n::t("error.internal"),
                    ));
                }
            };
            records.push(UserAccountRecord {
                user_id: username.clone(),
                username,
                email: None,
                password_hash: password_hash.clone(),
                roles: vec!["user".to_string()],
                status: "active".to_string(),
                access_level: access_level.clone(),
                unit_id: Some(unit.unit_id.clone()),
                token_balance: token_grant,
                token_granted_total: token_grant,
                token_used_total: 0,
                last_token_grant_date: Some(UserStore::today_string()),
                experience_total: 0,
                is_demo: true,
                created_at: now,
                updated_at: now,
                last_login_at: None,
            });
        }
    }
    state
        .user_store
        .upsert_users(&records)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let created = records.len() as i64;
    Ok(Json(json!({
        "data": {
            "created": created,
            "unit_count": scoped_units.len(),
            "per_unit": per_unit,
            "password": DEFAULT_TEST_USER_PASSWORD,
        }
    })))
}

fn parse_seed_test_user_serial(user_id: &str) -> Option<u64> {
    let suffix = user_id
        .trim()
        .strip_prefix(DEFAULT_TEST_USER_PREFIX)?
        .strip_prefix('_')?;
    if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    suffix.parse::<u64>().ok().filter(|value| *value > 0)
}

fn is_seed_test_user(record: &UserAccountRecord) -> bool {
    if !record.is_demo {
        return false;
    }
    let cleaned = record.user_id.trim();
    cleaned.starts_with(DEFAULT_TEST_USER_PREFIX)
        && cleaned.as_bytes().get(DEFAULT_TEST_USER_PREFIX.len()) == Some(&b'_')
}

async fn admin_user_accounts_cleanup(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
) -> Result<Json<Value>, Response> {
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    let scoped_unit_ids = actor.scope_unit_ids.as_ref().map(|set| {
        let mut items = set.iter().cloned().collect::<Vec<_>>();
        items.sort();
        items
    });

    let mut offset = 0;
    let mut target_user_ids = Vec::new();
    loop {
        let (batch, total) = state
            .user_store
            .list_users(
                Some(DEFAULT_TEST_USER_PREFIX),
                scoped_unit_ids.as_deref(),
                offset,
                TEST_USER_CLEANUP_BATCH_SIZE,
            )
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        if batch.is_empty() {
            break;
        }
        target_user_ids.extend(
            batch
                .into_iter()
                .filter(is_seed_test_user)
                .map(|record| record.user_id),
        );
        offset += TEST_USER_CLEANUP_BATCH_SIZE;
        if offset >= total {
            break;
        }
    }
    target_user_ids.sort();
    target_user_ids.dedup();

    let matched_users = target_user_ids.len() as i64;
    let mut deleted_users = 0;
    let mut failed = Vec::new();
    let mut cancelled_sessions = 0;
    let mut deleted_sessions = 0;
    let mut deleted_chat_records = 0;
    let mut deleted_tool_records = 0;
    let mut workspace_deleted = 0;
    let mut user_tools_deleted = 0;

    for user_id in target_user_ids {
        if let Err(err) = crate::services::user_plaza::purge_owner_items(&state, &user_id) {
            failed.push(json!({
                "user_id": user_id,
                "error": format!("purge plaza assets failed: {err}"),
            }));
            continue;
        }
        match state.user_store.delete_user(&user_id) {
            Ok(affected) if affected > 0 => {
                deleted_users += affected;
            }
            Ok(_) => {
                continue;
            }
            Err(err) => {
                failed.push(json!({
                    "user_id": user_id,
                    "error": err.to_string(),
                }));
                continue;
            }
        }
        let _ = state.user_store.set_user_tool_access(&user_id, None);
        let _ = state.user_store.set_user_agent_access(&user_id, None, None);
        let monitor_result = state.monitor.purge_user_sessions(&user_id);
        cancelled_sessions += monitor_result.get("cancelled").copied().unwrap_or(0);
        deleted_sessions += monitor_result.get("deleted").copied().unwrap_or(0);
        let purge_result = state.workspace.purge_user_data(&user_id);
        deleted_chat_records += purge_result.chat_records;
        deleted_tool_records += purge_result.tool_records;
        if purge_result.workspace_deleted {
            workspace_deleted += 1;
        }
        let tool_root = state.user_tool_store.get_user_dir(&user_id);
        if tool_root.exists() && std::fs::remove_dir_all(&tool_root).is_ok() {
            user_tools_deleted += 1;
        }
    }

    Ok(Json(json!({
        "ok": true,
        "prefix": DEFAULT_TEST_USER_PREFIX,
        "matched": matched_users,
        "deleted_users": deleted_users,
        "failed": failed.len(),
        "failed_items": failed,
        "cancelled_sessions": cancelled_sessions,
        "deleted_sessions": deleted_sessions,
        "deleted_chat_records": deleted_chat_records,
        "deleted_tool_records": deleted_tool_records,
        "workspace_deleted": workspace_deleted,
        "user_tools_deleted": user_tools_deleted,
    })))
}

async fn admin_user_accounts_update(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    AxumPath(user_id): AxumPath<String>,
    Json(payload): Json<UserAccountUpdateRequest>,
) -> Result<Json<Value>, Response> {
    let cleaned = user_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    let mut record = state
        .user_store
        .get_user_by_id(cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.user_not_found")))?;
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    ensure_user_scope(&actor, &record)?;
    let unit_map = build_unit_map(&units);
    if let Some(email) = payload.email {
        record.email = normalize_user_email(Some(email));
    }
    if let Some(status) = payload.status {
        record.status = normalize_user_status(Some(&status));
    }
    if payload.unit_id.is_some() {
        let next_unit_id = normalize_optional_id(payload.unit_id.as_deref());
        if actor.scope_unit_ids.is_some() {
            let Some(unit_id) = next_unit_id.as_deref() else {
                return Err(error_response(
                    StatusCode::BAD_REQUEST,
                    i18n::t("error.org_unit_required"),
                ));
            };
            ensure_unit_scope(&actor, Some(unit_id))?;
        }
        if let Some(unit_id) = next_unit_id.as_deref() {
            if !unit_map.contains_key(unit_id) {
                return Err(error_response(
                    StatusCode::NOT_FOUND,
                    i18n::t("error.org_unit_not_found"),
                ));
            }
        }
        if next_unit_id != record.unit_id {
            record.unit_id = next_unit_id;
        }
    }
    if let Some(roles) = payload.roles {
        record.roles = normalize_user_roles(roles);
    }
    if let Some(token_balance) = payload.token_balance {
        record.token_balance = token_balance.max(0);
    }
    record.updated_at = now_ts();
    state
        .user_store
        .update_user(&record)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if let Err(err) = state.inner_visible.sync_user_state(&record.user_id).await {
        tracing::warn!("failed to sync user state after admin update: {err}");
    }
    let unit = record
        .unit_id
        .as_ref()
        .and_then(|unit_id| unit_map.get(unit_id));
    Ok(Json(json!({
        "data": UserStore::to_profile_with_unit(&record, unit)
    })))
}

async fn admin_user_accounts_reset_password(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    AxumPath(user_id): AxumPath<String>,
    Json(payload): Json<UserAccountPasswordResetRequest>,
) -> Result<Json<Value>, Response> {
    let cleaned = user_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    let password = payload.password.trim();
    if password.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let record = state
        .user_store
        .get_user_by_id(cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.user_not_found")))?;
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    ensure_user_scope(&actor, &record)?;
    state
        .user_store
        .set_password(cleaned, password)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(
        json!({ "ok": true, "message": i18n::t("message.updated") }),
    ))
}

async fn admin_user_accounts_token_adjustment(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    AxumPath(user_id): AxumPath<String>,
    Json(payload): Json<UserAccountTokenAdjustmentRequest>,
) -> Result<Json<Value>, Response> {
    let cleaned = user_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    let amount = payload.amount.max(0);
    if amount <= 0 {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "token amount must be greater than 0".to_string(),
        ));
    }
    let record = state
        .user_store
        .get_user_by_id(cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.user_not_found")))?;
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    ensure_user_scope(&actor, &record)?;
    if UserStore::is_admin(&record) {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "admin users do not use token balance limits".to_string(),
        ));
    }
    let today = UserStore::today_string();
    let unit = record
        .unit_id
        .as_ref()
        .and_then(|unit_id| units.iter().find(|item| item.unit_id == *unit_id));
    let daily_grant = UserStore::default_daily_token_grant_by_level(unit.map(|item| item.level));
    let action = payload.action.trim().to_ascii_lowercase();
    match action.as_str() {
        "grant" => {
            state
                .storage
                .grant_user_tokens(cleaned, today.as_str(), daily_grant, amount, now_ts())
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        }
        "deduct" => {
            let token_status = UserStore::effective_token_balance_status(
                &record,
                unit.map(|item| item.level),
                Some(today.as_str()),
            );
            if amount > token_status.balance {
                return Err(error_response(
                    StatusCode::BAD_REQUEST,
                    i18n::t("error.user_token_insufficient"),
                ));
            }
            state
                .storage
                .consume_user_tokens(cleaned, today.as_str(), daily_grant, amount)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        }
        _ => {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                "invalid token adjustment action".to_string(),
            ));
        }
    }
    let updated = state
        .user_store
        .get_user_by_id(cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.user_not_found")))?;
    let updated_unit = updated
        .unit_id
        .as_ref()
        .and_then(|unit_id| units.iter().find(|item| item.unit_id == *unit_id));
    Ok(Json(json!({
        "data": UserStore::to_profile_with_unit(&updated, updated_unit),
        "adjustment": {
            "action": action,
            "amount": amount,
        }
    })))
}

async fn admin_user_accounts_force_logout(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    AxumPath(user_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let cleaned = user_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    let record = state
        .user_store
        .get_user_by_id(cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.user_not_found")))?;
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    ensure_user_scope(&actor, &record)?;

    let mut invalidated_at: f64 = 0.0;
    for scope in [
        UserStore::normalize_session_scope(Some("user_web")),
        UserStore::default_session_scope().to_string(),
    ] {
        let scope_invalidated_at = state
            .user_store
            .force_logout_user_scope(cleaned, &scope)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        invalidated_at = invalidated_at.max(scope_invalidated_at);
        state
            .control
            .auth_sessions
            .force_logout_user(cleaned, &scope)
            .await;
    }
    state.control.presence.force_user_offline(cleaned, now_ts());

    Ok(Json(json!({
        "data": {
            "ok": true,
            "user_id": cleaned,
            "session_scopes": ["user_web", "default"],
            "invalidated_at": invalidated_at,
        }
    })))
}

async fn admin_user_accounts_login_token(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    AxumPath(user_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let cleaned = user_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    let record = state
        .user_store
        .get_user_by_id(cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.user_not_found")))?;
    if record.status.trim().to_lowercase() != "active" {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "user disabled".to_string(),
        ));
    }
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    ensure_user_scope(&actor, &record)?;

    let session_scope = UserStore::normalize_session_scope(Some("user_web"));
    let session = state
        .user_store
        .issue_session_for_user_with_scope(record, &session_scope)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    state
        .control
        .auth_sessions
        .force_logout_user(&session.user.user_id, &session.token.session_scope)
        .await;
    let unit = session
        .user
        .unit_id
        .as_ref()
        .and_then(|unit_id| units.iter().find(|item| item.unit_id == *unit_id));

    Ok(Json(json!({
        "data": {
            "access_token": session.token.token,
            "session_scope": session.token.session_scope,
            "user": UserStore::to_profile_with_unit(&session.user, unit),
        }
    })))
}

async fn admin_user_accounts_tool_access_get(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    AxumPath(user_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let cleaned = user_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    let record = state
        .user_store
        .get_user_by_id(cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.user_not_found")))?;
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    ensure_user_scope(&actor, &record)?;
    let allowed = state
        .user_store
        .get_user_tool_access(cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let allowed_tools = allowed
        .as_ref()
        .and_then(|record| record.allowed_tools.clone());
    Ok(Json(json!({
        "data": { "allowed_tools": allowed_tools }
    })))
}

async fn admin_user_accounts_tool_access_update(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    AxumPath(user_id): AxumPath<String>,
    Json(payload): Json<UserAccountToolAccessRequest>,
) -> Result<Json<Value>, Response> {
    let cleaned = user_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    let record = state
        .user_store
        .get_user_by_id(cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.user_not_found")))?;
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    ensure_user_scope(&actor, &record)?;
    let allowed = normalize_optional_tool_access_list(payload.allowed_tools);
    state
        .user_store
        .set_user_tool_access(cleaned, allowed.as_ref())
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": { "allowed_tools": allowed }
    })))
}

async fn admin_user_accounts_agent_access_get(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    AxumPath(user_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let cleaned = user_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    let record = state
        .user_store
        .get_user_by_id(cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.user_not_found")))?;
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    ensure_user_scope(&actor, &record)?;
    let access = state
        .user_store
        .get_user_agent_access(cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let allowed_agent_ids = access
        .as_ref()
        .and_then(|record| record.allowed_agent_ids.clone());
    let blocked_agent_ids = access
        .as_ref()
        .map(|record| record.blocked_agent_ids.clone())
        .unwrap_or_default();
    Ok(Json(json!({
        "data": { "allowed_agent_ids": allowed_agent_ids, "blocked_agent_ids": blocked_agent_ids }
    })))
}

async fn admin_user_accounts_agent_access_update(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    AxumPath(user_id): AxumPath<String>,
    Json(payload): Json<UserAccountAgentAccessRequest>,
) -> Result<Json<Value>, Response> {
    let cleaned = user_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    let record = state
        .user_store
        .get_user_by_id(cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.user_not_found")))?;
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    ensure_user_scope(&actor, &record)?;
    let allowed = payload.allowed_agent_ids.map(normalize_tool_access_list);
    let blocked = payload.blocked_agent_ids.map(normalize_tool_access_list);
    state
        .user_store
        .set_user_agent_access(cleaned, allowed.as_ref(), blocked.as_ref())
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": { "allowed_agent_ids": allowed, "blocked_agent_ids": blocked }
    })))
}

async fn admin_user_accounts_delete(
    State(state): State<Arc<AppState>>,
    headers: AxumHeaderMap,
    AxumPath(user_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let cleaned = user_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    if UserStore::is_default_admin(cleaned) {
        return Err(error_response(
            StatusCode::FORBIDDEN,
            i18n::t("error.user_protected"),
        ));
    }
    let record = state
        .user_store
        .get_user_by_id(cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .ok_or_else(|| error_response(StatusCode::NOT_FOUND, i18n::t("error.user_not_found")))?;
    let units = state
        .user_store
        .list_org_units()
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let actor = resolve_admin_actor(&state, &headers, true, &units)?;
    ensure_user_scope(&actor, &record)?;
    crate::services::user_plaza::purge_owner_items(&state, cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let deleted_user = state
        .user_store
        .delete_user(cleaned)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let _ = state.user_store.set_user_tool_access(cleaned, None);
    let _ = state.user_store.set_user_agent_access(cleaned, None, None);
    let monitor_result = state.monitor.purge_user_sessions(cleaned);
    let purge_result = state.workspace.purge_user_data(cleaned);
    let tool_root = state.user_tool_store.get_user_dir(cleaned);
    let tool_dir_deleted = std::fs::remove_dir_all(&tool_root).is_ok();
    Ok(Json(json!({
        "ok": true,
        "message": i18n::t("message.user_deleted"),
        "deleted_user": deleted_user,
        "cancelled_sessions": monitor_result.get("cancelled").copied().unwrap_or(0),
        "deleted_sessions": monitor_result.get("deleted").copied().unwrap_or(0),
        "deleted_chat_records": purge_result.chat_records,
        "deleted_tool_records": purge_result.tool_records,
        "workspace_deleted": purge_result.workspace_deleted,
        "legacy_history_deleted": purge_result.legacy_history_deleted,
        "user_tools_deleted": tool_dir_deleted
    })))
}

async fn admin_users(State(state): State<Arc<AppState>>) -> Result<Json<Value>, Response> {
    #[derive(Default)]
    struct UserStats {
        active_sessions: i64,
        history_sessions: i64,
        total_sessions: i64,
        consumed_tokens: i64,
        chat_records: i64,
        tool_calls: i64,
        agent_ids: HashSet<String>,
    }

    state.monitor.warm_history(true);
    let sessions = state.monitor.list_sessions(false);
    let usage_stats = state.workspace.get_user_usage_stats();
    let active_statuses = HashSet::from(["running", "cancelling"]);
    let mut summary: HashMap<String, UserStats> = HashMap::new();

    for session in sessions {
        let user_id = session
            .get("user_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if user_id.is_empty() {
            continue;
        }
        let entry = summary.entry(user_id.to_string()).or_default();
        entry.total_sessions += 1;
        entry.consumed_tokens += session
            .get("consumed_tokens")
            .and_then(Value::as_i64)
            .unwrap_or_else(|| {
                session
                    .get("context_tokens_peak")
                    .or_else(|| session.get("context_tokens"))
                    .and_then(Value::as_i64)
                    .unwrap_or(0)
            });
        let agent_id = session
            .get("agent_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if agent_id.is_empty() {
            entry.agent_ids.insert("__default__".to_string());
        } else {
            entry.agent_ids.insert(agent_id.to_string());
        }
        let status = session.get("status").and_then(Value::as_str).unwrap_or("");
        if active_statuses.contains(status) {
            entry.active_sessions += 1;
        } else {
            entry.history_sessions += 1;
        }
    }

    for (user_id, stats) in usage_stats {
        let entry = summary.entry(user_id).or_default();
        entry.chat_records = *stats.get("chat_records").unwrap_or(&0);
        entry.tool_calls = *stats.get("tool_records").unwrap_or(&0);
    }

    for (user_id, stats) in summary.iter_mut() {
        if let Ok(agent_ids) = state.user_store.list_chat_session_agent_ids(user_id) {
            for agent_id in agent_ids {
                let agent_id = agent_id.trim();
                if !agent_id.is_empty() {
                    stats.agent_ids.insert(agent_id.to_string());
                }
            }
        }
        if let Ok(agents) = state.user_store.list_user_agents(user_id) {
            for agent in agents {
                let agent_id = agent.agent_id.trim();
                if !agent_id.is_empty() {
                    stats.agent_ids.insert(agent_id.to_string());
                }
            }
        }
        stats.agent_ids.insert("__default__".to_string());
    }

    let mut users = summary
        .into_iter()
        .map(|(user_id, stats)| {
            let agent_count = stats.agent_ids.len() as i64;
            json!({
                "user_id": user_id,
                "active_sessions": stats.active_sessions,
                "history_sessions": stats.history_sessions,
                "total_sessions": stats.total_sessions,
                "chat_records": stats.chat_records,
                "tool_calls": stats.tool_calls,
                "consumed_tokens": stats.consumed_tokens,
                "context_tokens": stats.consumed_tokens,
                "context_occupancy_tokens": stats.consumed_tokens,
                "agent_count": agent_count
            })
        })
        .collect::<Vec<_>>();
    users.sort_by(|a, b| {
        let left_active = a
            .get("active_sessions")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let right_active = b
            .get("active_sessions")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let left_total = a.get("total_sessions").and_then(Value::as_i64).unwrap_or(0);
        let right_total = b.get("total_sessions").and_then(Value::as_i64).unwrap_or(0);
        let left_id = a.get("user_id").and_then(Value::as_str).unwrap_or("");
        let right_id = b.get("user_id").and_then(Value::as_str).unwrap_or("");
        right_active
            .cmp(&left_active)
            .then_with(|| right_total.cmp(&left_total))
            .then_with(|| left_id.cmp(right_id))
    });
    Ok(Json(json!({ "users": users })))
}

async fn admin_user_sessions(
    State(state): State<Arc<AppState>>,
    AxumPath(user_id): AxumPath<String>,
    Query(query): Query<UserSessionsQuery>,
) -> Result<Json<Value>, Response> {
    let cleaned = user_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    let active_only = query.active_only.unwrap_or(false);
    let sessions = state
        .monitor
        .list_sessions(active_only)
        .into_iter()
        .filter(|session| session.get("user_id").and_then(Value::as_str) == Some(cleaned))
        .collect::<Vec<_>>();
    Ok(Json(json!({ "user_id": cleaned, "sessions": sessions })))
}

async fn admin_user_delete(
    State(state): State<Arc<AppState>>,
    AxumPath(user_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let cleaned = user_id.trim();
    if cleaned.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.user_id_required"),
        ));
    }
    if UserStore::is_default_admin(cleaned) {
        return Err(error_response(
            StatusCode::FORBIDDEN,
            i18n::t("error.user_protected"),
        ));
    }
    let monitor_result = state.monitor.purge_user_sessions(cleaned);
    let purge_result = state.workspace.purge_user_data(cleaned);
    Ok(Json(json!({
        "ok": true,
        "message": i18n::t("message.user_deleted"),
        "cancelled_sessions": monitor_result.get("cancelled").copied().unwrap_or(0),
        "deleted_sessions": monitor_result.get("deleted").copied().unwrap_or(0),
        "deleted_chat_records": purge_result.chat_records,
        "deleted_tool_records": purge_result.tool_records,
        "workspace_deleted": purge_result.workspace_deleted,
        "legacy_history_deleted": purge_result.legacy_history_deleted
    })))
}

fn normalize_throughput_prefix(prefix: Option<String>) -> String {
    let fallback = "throughput_user";
    let cleaned = prefix
        .as_deref()
        .unwrap_or(fallback)
        .trim()
        .trim_end_matches('-');
    if cleaned.is_empty() {
        fallback.to_string()
    } else {
        cleaned.to_string()
    }
}

fn is_throughput_user(user_id: &str, prefix: &str) -> bool {
    if prefix.is_empty() {
        return false;
    }
    let cleaned = user_id.trim();
    if cleaned.len() <= prefix.len() {
        return false;
    }
    cleaned.starts_with(prefix) && cleaned.as_bytes().get(prefix.len()) == Some(&b'-')
}

async fn admin_users_cleanup_throughput(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ThroughputUserCleanupRequest>,
) -> Result<Json<Value>, Response> {
    let prefix = normalize_throughput_prefix(payload.prefix);
    state.monitor.warm_history(true);
    let mut user_ids = HashSet::new();
    for session in state.monitor.list_sessions(false) {
        if let Some(user_id) = session.get("user_id").and_then(Value::as_str) {
            let cleaned = user_id.trim();
            if !cleaned.is_empty() {
                user_ids.insert(cleaned.to_string());
            }
        }
    }
    let usage_stats = state.workspace.get_user_usage_stats();
    user_ids.extend(usage_stats.keys().cloned());
    let mut throughput_users = user_ids
        .into_iter()
        .filter(|user_id| is_throughput_user(user_id, &prefix))
        .collect::<Vec<_>>();
    throughput_users.sort();

    let mut cancelled_sessions = 0;
    let mut deleted_sessions = 0;
    let mut deleted_storage = 0;
    let mut deleted_chat_records = 0;
    let mut deleted_tool_records = 0;
    let mut workspace_deleted = 0;
    for user_id in &throughput_users {
        let monitor_result = state.monitor.purge_user_sessions(user_id);
        cancelled_sessions += monitor_result.get("cancelled").copied().unwrap_or(0);
        deleted_sessions += monitor_result.get("deleted").copied().unwrap_or(0);
        deleted_storage += monitor_result.get("deleted_storage").copied().unwrap_or(0);
        let purge_result = state.workspace.purge_user_data(user_id);
        deleted_chat_records += purge_result.chat_records;
        deleted_tool_records += purge_result.tool_records;
        if purge_result.workspace_deleted {
            workspace_deleted += 1;
        }
    }

    Ok(Json(json!({
        "ok": true,
        "prefix": prefix,
        "users": throughput_users.len(),
        "cancelled_sessions": cancelled_sessions,
        "deleted_sessions": deleted_sessions,
        "deleted_storage": deleted_storage,
        "deleted_chat_records": deleted_chat_records,
        "deleted_tool_records": deleted_tool_records,
        "workspace_deleted": workspace_deleted
    })))
}

fn build_import_org_unit_records(
    roots: &[OrgUnitImportNode],
    now: f64,
) -> anyhow::Result<Vec<OrgUnitRecord>> {
    let mut output = Vec::new();
    for (index, root) in roots.iter().enumerate() {
        append_import_org_unit_records(root, None, &[], &[], index as i64, 1, now, &mut output)?;
    }
    Ok(output)
}

fn append_import_org_unit_records(
    node: &OrgUnitImportNode,
    parent_id: Option<String>,
    parent_path_ids: &[String],
    parent_path_names: &[String],
    sort_order: i64,
    level: i32,
    now: f64,
    output: &mut Vec<OrgUnitRecord>,
) -> anyhow::Result<()> {
    let name = node.name.trim();
    if name.is_empty() {
        return Err(anyhow::anyhow!("org unit name is empty"));
    }
    if level > MAX_ORG_UNIT_LEVEL {
        return Err(anyhow::anyhow!(
            "org unit level exceeds {MAX_ORG_UNIT_LEVEL}"
        ));
    }
    let mut path_names = parent_path_names.to_vec();
    path_names.push(name.to_string());
    let unit_id = format!(
        "unit_{}",
        Uuid::new_v5(&Uuid::NAMESPACE_URL, path_names.join("/").as_bytes()).simple()
    );
    let mut path_ids = parent_path_ids.to_vec();
    path_ids.push(unit_id.clone());
    let record = OrgUnitRecord {
        unit_id: unit_id.clone(),
        parent_id,
        name: name.to_string(),
        level,
        path: path_ids.join("/"),
        path_name: path_names.join(ORG_UNIT_NAME_SEPARATOR),
        sort_order,
        leader_ids: Vec::new(),
        created_at: now,
        updated_at: now,
    };
    output.push(record);
    for (index, child) in node.children.iter().enumerate() {
        append_import_org_unit_records(
            child,
            Some(unit_id.clone()),
            &path_ids,
            &path_names,
            index as i64,
            level + 1,
            now,
            output,
        )?;
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct OrgUnitCreateRequest {
    name: String,
    #[serde(default)]
    parent_id: Option<String>,
    #[serde(default)]
    sort_order: Option<i64>,
    #[serde(default)]
    leader_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct OrgUnitUpdateRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    parent_id: Option<String>,
    #[serde(default)]
    sort_order: Option<i64>,
    #[serde(default)]
    leader_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct OrgUnitImportRequest {
    units: Vec<OrgUnitImportNode>,
    #[serde(default)]
    migrate_user_unit_id: Option<String>,
    #[serde(default)]
    migrate_user_root_name: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct OrgUnitImportNode {
    name: String,
    #[serde(default)]
    children: Vec<OrgUnitImportNode>,
}

#[derive(Debug, Deserialize)]
struct ExternalLinkUpsertRequest {
    #[serde(default)]
    link_id: Option<String>,
    title: String,
    #[serde(default)]
    description: String,
    url: String,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    allowed_levels: Option<Vec<i32>>,
    #[serde(default)]
    sort_order: Option<i64>,
    #[serde(default)]
    enabled: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct UserAccountListQuery {
    #[serde(default)]
    keyword: Option<String>,
    #[serde(default)]
    unit_id: Option<String>,
    #[serde(default)]
    offset: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    activity_days: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct UserAccountCreateRequest {
    username: String,
    #[serde(default)]
    email: Option<String>,
    password: String,
    #[serde(default)]
    unit_id: Option<String>,
    #[serde(default)]
    roles: Vec<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    is_demo: bool,
}

#[derive(Debug, Deserialize)]
struct UserAccountSeedRequest {
    #[serde(default)]
    per_unit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct UserAccountUpdateRequest {
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    unit_id: Option<String>,
    #[serde(default)]
    roles: Option<Vec<String>>,
    #[serde(default)]
    token_balance: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct UserAccountPasswordResetRequest {
    password: String,
}

#[derive(Debug, Deserialize)]
struct UserAccountTokenAdjustmentRequest {
    action: String,
    amount: i64,
}

#[derive(Debug, Deserialize)]
struct UserAccountToolAccessRequest {
    #[serde(default)]
    allowed_tools: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct UserAccountAgentAccessRequest {
    #[serde(default)]
    allowed_agent_ids: Option<Vec<String>>,
    #[serde(default)]
    blocked_agent_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Default)]
struct UserSessionsQuery {
    active_only: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct ThroughputUserCleanupRequest {
    prefix: Option<String>,
}
