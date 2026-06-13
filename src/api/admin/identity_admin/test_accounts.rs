use crate::api::admin::{
    error_response, filter_units_by_scope, now_ts, resolve_admin_actor, DEFAULT_TEST_USER_PASSWORD,
    DEFAULT_TEST_USER_PREFIX, MAX_TEST_USERS_PER_UNIT, TEST_USER_CLEANUP_BATCH_SIZE,
};
use crate::i18n;
use crate::state::AppState;
use crate::storage::UserAccountRecord;
use crate::user_store::UserStore;
use axum::extract::State;
use axum::http::{HeaderMap as AxumHeaderMap, StatusCode};
use axum::response::Response;
use axum::{routing::post, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Arc;

pub(super) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/wunder/admin/user_accounts/test/seed",
            post(admin_user_accounts_seed),
        )
        .route(
            "/wunder/admin/user_accounts/test/cleanup",
            post(admin_user_accounts_cleanup),
        )
        .route(
            "/wunder/admin/users/throughput/cleanup",
            post(admin_users_cleanup_throughput),
        )
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

#[derive(Debug, Deserialize)]
struct UserAccountSeedRequest {
    #[serde(default)]
    per_unit: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
struct ThroughputUserCleanupRequest {
    prefix: Option<String>,
}
