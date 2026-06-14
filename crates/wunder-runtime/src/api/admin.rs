// 管理端 API：配置更新、监控查询、知识库与技能管理等。
use crate::auth;
use crate::config::Config;
use crate::i18n;
use crate::llm;
use crate::services::default_agent_sync::{DEFAULT_AGENT_ID_ALIAS, PRESET_TEMPLATE_USER_ID};
use crate::state::AppState;
use crate::user_store::UserStore;
use crate::{
    org_units,
    storage::{ExternalLinkRecord, OrgUnitRecord, UserAccountRecord},
};
use axum::extract::State;
use axum::http::{HeaderMap as AxumHeaderMap, StatusCode};
use axum::response::Response;
use axum::{routing::get, routing::post, Json, Router};
use chrono::{Local, TimeZone, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

mod channel_admin;
mod gateway_admin;
mod identity_admin;
mod integration_admin;
mod knowledge_admin;
mod monitor_admin;
mod resource_admin;

pub(crate) use integration_admin::{
    ensure_admin_skill_editable, is_admin_skill_editable, resolve_admin_skill_root,
    resolve_admin_skill_spec,
};

const MAX_ORG_UNIT_LEVEL: i32 = 4;
pub(super) const DEFAULT_TEST_USER_PASSWORD: &str = "Test@123456";
pub(super) const DEFAULT_TEST_USER_PREFIX: &str = "test_user";
pub(super) const MAX_TEST_USERS_PER_UNIT: i64 = 200;
pub(super) const TEST_USER_CLEANUP_BATCH_SIZE: i64 = 200;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .merge(integration_admin::router())
        .merge(channel_admin::router())
        .merge(gateway_admin::router())
        .merge(monitor_admin::router())
        .merge(knowledge_admin::router())
        .merge(identity_admin::router())
        .merge(resource_admin::router())
        .route(
            "/wunder/admin/llm",
            get(admin_llm_get).post(admin_llm_update),
        )
        .route(
            "/wunder/admin/llm/context_window",
            post(admin_llm_context_window),
        )
        .route("/wunder/admin/llm/tts_voices", post(admin_llm_tts_voices))
        .route(
            "/wunder/admin/system",
            get(admin_system_get).post(admin_system_update),
        )
        .route(
            "/wunder/admin/server",
            get(admin_server_get).post(admin_server_update),
        )
        .route("/wunder/admin/security", get(admin_security_get))
}

async fn admin_llm_get(State(state): State<Arc<AppState>>) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    Ok(Json(json!({ "llm": config.llm })))
}

async fn admin_llm_update(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<LlmUpdateRequest>,
) -> Result<Json<Value>, Response> {
    let updated = state
        .config_store
        .update(|config| {
            config.llm = payload.llm.clone();
        })
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "llm": updated.llm })))
}

async fn admin_llm_context_window(
    Json(payload): Json<LlmContextProbeRequest>,
) -> Result<Json<Value>, Response> {
    let model = payload.model.trim();
    let provider = llm::normalize_provider(payload.provider.as_deref());
    let inline_base = payload.base_url.trim();
    let base_url = if inline_base.is_empty() {
        llm::provider_default_base_url(&provider).unwrap_or("")
    } else {
        inline_base
    };
    if base_url.is_empty() || model.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.base_url_or_model_required"),
        ));
    }
    if !llm::is_openai_compatible_provider(&provider) {
        return Ok(Json(json!({
            "max_context": Value::Null,
            "message": i18n::t("probe.provider_unsupported")
        })));
    }

    let timeout_s = payload.timeout_s.unwrap_or(15);
    let timeout_s = if timeout_s == 0 { 15 } else { timeout_s };
    let api_key = payload.api_key.as_deref().unwrap_or("");
    let result = llm::probe_openai_context_window(base_url, api_key, model, timeout_s).await;
    let payload = match result {
        Ok(Some(value)) => json!({ "max_context": value, "message": i18n::t("probe.success") }),
        Ok(None) => json!({ "max_context": Value::Null, "message": i18n::t("probe.no_context") }),
        Err(err) => {
            let message = i18n::t_with_params(
                "probe.failed",
                &HashMap::from([("detail".to_string(), err.to_string())]),
            );
            json!({ "max_context": Value::Null, "message": message })
        }
    };
    Ok(Json(payload))
}

async fn admin_llm_tts_voices(
    Json(payload): Json<LlmContextProbeRequest>,
) -> Result<Json<Value>, Response> {
    let model = payload.model.trim();
    let provider = llm::normalize_provider(payload.provider.as_deref());
    let inline_base = payload.base_url.trim();
    let base_url = if inline_base.is_empty() {
        llm::provider_default_base_url(&provider).unwrap_or("")
    } else {
        inline_base
    };
    if base_url.is_empty() || model.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.base_url_or_model_required"),
        ));
    }
    if !llm::is_openai_compatible_provider(&provider) {
        return Ok(Json(json!({
            "voices": [],
            "message": i18n::t("probe.provider_unsupported")
        })));
    }

    let timeout_s = payload.timeout_s.unwrap_or(15).max(5);
    let api_key = payload.api_key.as_deref().unwrap_or("");
    let result =
        crate::multimodal_models::probe_tts_voices(base_url, api_key, model, timeout_s).await;
    let response = match result {
        Ok(voices) if !voices.is_empty() => {
            json!({ "voices": voices, "message": i18n::t("probe.success") })
        }
        Ok(_) => json!({ "voices": [], "message": i18n::t("probe.no_context") }),
        Err(err) => {
            let message = i18n::t_with_params(
                "probe.failed",
                &HashMap::from([("detail".to_string(), err.to_string())]),
            );
            json!({ "voices": [], "message": message })
        }
    };
    Ok(Json(response))
}

fn normalize_string_list(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

fn normalize_optional_config_string(value: String) -> Option<String> {
    let cleaned = value.trim().to_string();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

fn build_system_settings_payload(config: &Config) -> Value {
    let exec_policy_mode = config
        .security
        .exec_policy_mode
        .clone()
        .unwrap_or_else(|| "allow".to_string());
    json!({
        "server": {
            "max_active_sessions": config.server.max_active_sessions,
            "stream_chunk_size": config.server.stream_chunk_size,
        },
        "security": {
            "api_key": config.api_key(),
            "external_auth_key": config.external_auth_key(),
            "external_embed_preset_agent_name": config.external_embed_preset_agent_name(),
            "external_embed_jwt_secret": config.external_embed_jwt_secret(),
            "external_embed_jwt_user_id_claim": config.external_embed_jwt_user_id_claim(),
            "allow_commands": config.security.allow_commands.clone(),
            "allow_paths": config.security.allow_paths.clone(),
            "deny_globs": config.security.deny_globs.clone(),
            "exec_policy_mode": exec_policy_mode,
        },
        "observability": {
            "log_level": config.observability.log_level.clone(),
            "monitor_event_limit": config.observability.monitor_event_limit,
            "monitor_payload_max_chars": config.observability.monitor_payload_max_chars,
            "monitor_drop_event_types": config.observability.monitor_drop_event_types.clone(),
        },
        "cors": {
            "allow_origins": config.cors.allow_origins.clone(),
            "allow_methods": config.cors.allow_methods.clone(),
            "allow_headers": config.cors.allow_headers.clone(),
            "allow_credentials": config.cors.allow_credentials,
        },
        "onlyoffice": {
            "enabled": config.onlyoffice.enabled,
            "document_server_url": config.onlyoffice.document_server_url.clone().unwrap_or_default(),
            "internal_document_server_url": config.onlyoffice.internal_document_server_url.clone().unwrap_or_default(),
            "api_url": config.onlyoffice.api_url.clone().unwrap_or_default(),
            "public_base_url": config.onlyoffice.public_base_url.clone().unwrap_or_default(),
            "jwt_secret": config.onlyoffice.jwt_secret.clone().unwrap_or_default(),
            "jwt_header": config.onlyoffice.jwt_header.clone(),
            "token_ttl_s": config.onlyoffice.token_ttl_s,
            "request_timeout_s": config.onlyoffice.request_timeout_s,
            "max_download_bytes": config.onlyoffice.max_download_bytes,
        },
        "drawio": {
            "enabled": config.drawio.enabled(),
            "editor_url": config.drawio.editor_url.clone().unwrap_or_default(),
            "max_file_bytes": config.drawio.max_file_bytes,
        },
        "ragflow": {
            "base_url": config.ragflow.base_url.clone(),
            "api_key": config.ragflow.api_key.clone().unwrap_or_default(),
            "timeout_s": config.ragflow.timeout_s,
        },
        "firecrawl": {
            "provider": config.tools.web.fetch.provider(),
            "api_key": config.tools.web.fetch.firecrawl.api_key().unwrap_or_default(),
            "base_url": config.tools.web.fetch.firecrawl.base_url(),
            "timeout_secs": config.tools.web.fetch.firecrawl.timeout_secs,
            "only_main_content": config.tools.web.fetch.firecrawl.only_main_content,
            "max_age_ms": config.tools.web.fetch.firecrawl.max_age_ms,
            "proxy": config.tools.web.fetch.firecrawl.proxy.clone(),
            "store_in_cache": config.tools.web.fetch.firecrawl.store_in_cache,
        }
    })
}

async fn admin_system_get(State(state): State<Arc<AppState>>) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    Ok(Json(build_system_settings_payload(&config)))
}

async fn admin_system_update(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SystemUpdateRequest>,
) -> Result<Json<Value>, Response> {
    let has_updates = payload.server.is_some()
        || payload.security.is_some()
        || payload.observability.is_some()
        || payload.cors.is_some()
        || payload.onlyoffice.is_some()
        || payload.drawio.is_some()
        || payload.ragflow.is_some()
        || payload.firecrawl.is_some();
    if !has_updates {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.param_required"),
        ));
    }
    if let Some(server) = payload.server.as_ref() {
        if let Some(max_active_sessions) = server.max_active_sessions {
            if max_active_sessions == 0 {
                return Err(error_response(
                    StatusCode::BAD_REQUEST,
                    i18n::t("error.max_active_sessions_invalid"),
                ));
            }
        }
    }
    if let Some(security) = payload.security.as_ref() {
        if let Some(mode) = security.exec_policy_mode.as_ref() {
            let cleaned = mode.trim().to_lowercase();
            if !cleaned.is_empty() && !matches!(cleaned.as_str(), "allow" | "audit" | "enforce") {
                return Err(error_response(
                    StatusCode::BAD_REQUEST,
                    i18n::t("error.exec_policy_mode_invalid"),
                ));
            }
        }
    }
    let updated = state
        .config_store
        .update(|config| {
            if let Some(server) = payload.server {
                if let Some(max_active_sessions) = server.max_active_sessions {
                    config.server.max_active_sessions = max_active_sessions;
                }
                if let Some(stream_chunk_size) = server.stream_chunk_size {
                    config.server.stream_chunk_size = stream_chunk_size;
                }
            }
            if let Some(security) = payload.security {
                if let Some(api_key) = security.api_key {
                    let cleaned = api_key.trim().to_string();
                    if cleaned.is_empty() {
                        config.security.api_key = None;
                    } else {
                        config.security.api_key = Some(cleaned);
                    }
                }
                if let Some(external_auth_key) = security.external_auth_key {
                    let cleaned = external_auth_key.trim().to_string();
                    if cleaned.is_empty() {
                        config.security.external_auth_key = None;
                    } else {
                        config.security.external_auth_key = Some(cleaned);
                    }
                }
                if let Some(agent_name) = security.external_embed_preset_agent_name {
                    let cleaned = agent_name.trim().to_string();
                    if cleaned.is_empty() {
                        config.security.external_embed_preset_agent_name = None;
                    } else {
                        config.security.external_embed_preset_agent_name = Some(cleaned);
                    }
                }
                if let Some(jwt_secret) = security.external_embed_jwt_secret {
                    let cleaned = jwt_secret.trim().to_string();
                    if cleaned.is_empty() {
                        config.security.external_embed_jwt_secret = None;
                    } else {
                        config.security.external_embed_jwt_secret = Some(cleaned);
                    }
                }
                if let Some(user_id_claim) = security.external_embed_jwt_user_id_claim {
                    let cleaned = user_id_claim.trim().to_string();
                    if cleaned.is_empty() {
                        config.security.external_embed_jwt_user_id_claim = None;
                    } else {
                        config.security.external_embed_jwt_user_id_claim = Some(cleaned);
                    }
                }
                if let Some(allow_commands) = security.allow_commands {
                    config.security.allow_commands = normalize_string_list(allow_commands);
                }
                if let Some(allow_paths) = security.allow_paths {
                    config.security.allow_paths = normalize_string_list(allow_paths);
                }
                if let Some(deny_globs) = security.deny_globs {
                    config.security.deny_globs = normalize_string_list(deny_globs);
                }
                if let Some(exec_policy_mode) = security.exec_policy_mode {
                    let cleaned = exec_policy_mode.trim().to_lowercase();
                    if cleaned.is_empty() {
                        config.security.exec_policy_mode = None;
                    } else {
                        config.security.exec_policy_mode = Some(cleaned);
                    }
                }
            }
            if let Some(observability) = payload.observability {
                if let Some(log_level) = observability.log_level {
                    config.observability.log_level = log_level.trim().to_string();
                }
                if let Some(monitor_event_limit) = observability.monitor_event_limit {
                    config.observability.monitor_event_limit = monitor_event_limit;
                }
                if let Some(monitor_payload_max_chars) = observability.monitor_payload_max_chars {
                    config.observability.monitor_payload_max_chars = monitor_payload_max_chars;
                }
                if let Some(drop_event_types) = observability.monitor_drop_event_types {
                    config.observability.monitor_drop_event_types =
                        normalize_string_list(drop_event_types);
                }
            }
            if let Some(cors) = payload.cors {
                if let Some(allow_origins) = cors.allow_origins {
                    config.cors.allow_origins = Some(normalize_string_list(allow_origins));
                }
                if let Some(allow_methods) = cors.allow_methods {
                    config.cors.allow_methods = Some(normalize_string_list(allow_methods));
                }
                if let Some(allow_headers) = cors.allow_headers {
                    config.cors.allow_headers = Some(normalize_string_list(allow_headers));
                }
                if let Some(allow_credentials) = cors.allow_credentials {
                    config.cors.allow_credentials = Some(allow_credentials);
                }
            }
            if let Some(onlyoffice) = payload.onlyoffice {
                if let Some(enabled) = onlyoffice.enabled {
                    config.onlyoffice.enabled = enabled;
                }
                if let Some(document_server_url) = onlyoffice.document_server_url {
                    config.onlyoffice.document_server_url =
                        normalize_optional_config_string(document_server_url);
                }
                if let Some(internal_document_server_url) = onlyoffice.internal_document_server_url
                {
                    config.onlyoffice.internal_document_server_url =
                        normalize_optional_config_string(internal_document_server_url);
                }
                if let Some(api_url) = onlyoffice.api_url {
                    config.onlyoffice.api_url = normalize_optional_config_string(api_url);
                }
                if let Some(public_base_url) = onlyoffice.public_base_url {
                    config.onlyoffice.public_base_url =
                        normalize_optional_config_string(public_base_url);
                }
                if let Some(jwt_secret) = onlyoffice.jwt_secret {
                    config.onlyoffice.jwt_secret = normalize_optional_config_string(jwt_secret);
                }
                if let Some(jwt_header) = onlyoffice.jwt_header {
                    let cleaned = jwt_header.trim();
                    config.onlyoffice.jwt_header = if cleaned.is_empty() {
                        "Authorization".to_string()
                    } else {
                        cleaned.to_string()
                    };
                }
                if let Some(token_ttl_s) = onlyoffice.token_ttl_s {
                    config.onlyoffice.token_ttl_s = token_ttl_s.clamp(60, 24 * 60 * 60);
                }
                if let Some(request_timeout_s) = onlyoffice.request_timeout_s {
                    config.onlyoffice.request_timeout_s = request_timeout_s.clamp(5, 300);
                }
                if let Some(max_download_bytes) = onlyoffice.max_download_bytes {
                    config.onlyoffice.max_download_bytes =
                        max_download_bytes.clamp(1024, 1024 * 1024 * 1024);
                }
            }
            if let Some(drawio) = payload.drawio {
                if let Some(enabled) = drawio.enabled {
                    config.drawio.enabled = enabled;
                }
                if let Some(editor_url) = drawio.editor_url {
                    config.drawio.editor_url = normalize_optional_config_string(editor_url);
                }
                if let Some(max_file_bytes) = drawio.max_file_bytes {
                    config.drawio.max_file_bytes = max_file_bytes.clamp(1024, 200 * 1024 * 1024);
                }
            }
            if let Some(ragflow) = payload.ragflow {
                if let Some(base_url) = ragflow.base_url {
                    let cleaned = base_url.trim();
                    config.ragflow.base_url = cleaned.trim_end_matches('/').to_string();
                }
                if let Some(api_key) = ragflow.api_key {
                    config.ragflow.api_key = normalize_optional_config_string(api_key);
                }
                if let Some(timeout_s) = ragflow.timeout_s {
                    config.ragflow.timeout_s = timeout_s.clamp(1, 600);
                }
            }
            if let Some(firecrawl) = payload.firecrawl {
                if let Some(provider) = firecrawl.provider {
                    config.tools.web.fetch.provider =
                        match provider.trim().to_ascii_lowercase().as_str() {
                            "firecrawl" => "firecrawl".to_string(),
                            "auto" => "auto".to_string(),
                            _ => "direct".to_string(),
                        };
                }
                if let Some(api_key) = firecrawl.api_key {
                    config.tools.web.fetch.firecrawl.api_key =
                        normalize_optional_config_string(api_key);
                }
                if let Some(base_url) = firecrawl.base_url {
                    let cleaned = base_url.trim();
                    config.tools.web.fetch.firecrawl.base_url = if cleaned.is_empty() {
                        "https://api.firecrawl.dev".to_string()
                    } else {
                        cleaned.trim_end_matches('/').to_string()
                    };
                }
                if let Some(timeout_secs) = firecrawl.timeout_secs {
                    config.tools.web.fetch.firecrawl.timeout_secs = timeout_secs.clamp(1, 180);
                }
                if let Some(only_main_content) = firecrawl.only_main_content {
                    config.tools.web.fetch.firecrawl.only_main_content = only_main_content;
                }
                if let Some(max_age_ms) = firecrawl.max_age_ms {
                    config.tools.web.fetch.firecrawl.max_age_ms = max_age_ms.min(86_400_000);
                }
                if let Some(proxy) = firecrawl.proxy {
                    config.tools.web.fetch.firecrawl.proxy =
                        match proxy.trim().to_ascii_lowercase().as_str() {
                            "basic" => "basic".to_string(),
                            "stealth" => "stealth".to_string(),
                            _ => "auto".to_string(),
                        };
                }
                if let Some(store_in_cache) = firecrawl.store_in_cache {
                    config.tools.web.fetch.firecrawl.store_in_cache = store_in_cache;
                }
            }
        })
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(build_system_settings_payload(&updated)))
}

async fn admin_server_get(State(state): State<Arc<AppState>>) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    Ok(Json(json!({
        "server": {
            "max_active_sessions": config.server.max_active_sessions
        }
    })))
}

async fn admin_security_get(State(state): State<Arc<AppState>>) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let api_key = config.api_key();
    Ok(Json(json!({
        "security": {
            "api_key": api_key
        }
    })))
}

async fn admin_server_update(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ServerUpdateRequest>,
) -> Result<Json<Value>, Response> {
    if let Some(max_active_sessions) = payload.max_active_sessions {
        if max_active_sessions == 0 {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("error.max_active_sessions_invalid"),
            ));
        }
    }
    if payload.max_active_sessions.is_none() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.param_required"),
        ));
    }
    let updated = state
        .config_store
        .update(|config| {
            if let Some(max_active_sessions) = payload.max_active_sessions {
                config.server.max_active_sessions = max_active_sessions;
            }
        })
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "server": {
            "max_active_sessions": updated.server.max_active_sessions
        }
    })))
}

pub(super) fn resolve_monitor_session_agent_name(
    state: &AppState,
    user_id: &str,
    agent_id: &str,
) -> Result<Option<String>, Response> {
    let cleaned_user = user_id.trim();
    if cleaned_user.is_empty() {
        return Ok(None);
    }
    let cleaned_agent = agent_id.trim();
    let is_default_agent = cleaned_agent.is_empty()
        || cleaned_agent.eq_ignore_ascii_case(DEFAULT_AGENT_ID_ALIAS)
        || cleaned_agent.eq_ignore_ascii_case("default");
    if is_default_agent {
        let record = crate::user_store::build_default_agent_record_from_storage(
            state.storage.as_ref(),
            cleaned_user,
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        let name = record.name.trim();
        return if name.is_empty() {
            Ok(None)
        } else {
            Ok(Some(name.to_string()))
        };
    }
    let record = state
        .user_store
        .get_user_agent(cleaned_user, cleaned_agent)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(record.and_then(|item| {
        let name = item.name.trim();
        if name.is_empty() {
            None
        } else {
            Some(name.to_string())
        }
    }))
}

pub(super) fn format_ts(ts: f64) -> String {
    if ts <= 0.0 {
        return String::new();
    }
    let secs = ts.trunc() as i64;
    let nanos = ((ts.fract()) * 1_000_000_000.0) as u32;
    match Local.timestamp_opt(secs, nanos).single() {
        Some(dt) => dt.to_rfc3339(),
        None => String::new(),
    }
}

pub(super) fn empty_user_activity_series(days: i64) -> Vec<Value> {
    let safe_days = days.max(1) as usize;
    let today = Local::now().date_naive();
    (0..safe_days)
        .map(|offset| {
            let day = today - chrono::Duration::days((safe_days - 1 - offset) as i64);
            json!({
                "date": day.format("%Y-%m-%d").to_string(),
                "tokens": 0_i64,
            })
        })
        .collect()
}

pub(super) fn build_user_activity_series_map(
    state: &AppState,
    user_ids: &[String],
    days: i64,
) -> HashMap<String, Vec<Value>> {
    if user_ids.is_empty() {
        return HashMap::new();
    }
    let safe_days = days.clamp(1, 31);
    let today = Local::now().date_naive();
    let start_day = today - chrono::Duration::days(safe_days.saturating_sub(1));
    let since_time = start_day
        .and_hms_opt(0, 0, 0)
        .and_then(|dt| Local.from_local_datetime(&dt).single())
        .map(|dt| dt.timestamp() as f64)
        .unwrap_or_else(now_ts);

    let mut result = user_ids
        .iter()
        .map(|user_id| (user_id.clone(), empty_user_activity_series(safe_days)))
        .collect::<HashMap<_, _>>();

    for user_id in user_ids {
        let records = state.monitor.load_records_by_user(
            user_id,
            None,
            Some(since_time),
            safe_days.saturating_mul(24),
        );
        if records.is_empty() {
            continue;
        }
        let mut day_buckets = HashMap::<String, i64>::new();
        for record in records {
            let updated_time = record
                .get("updated_time")
                .and_then(Value::as_f64)
                .or_else(|| record.get("ended_time").and_then(Value::as_f64))
                .or_else(|| record.get("start_time").and_then(Value::as_f64))
                .unwrap_or(0.0);
            if updated_time <= 0.0 {
                continue;
            }
            let Some(day) = Local.timestamp_opt(updated_time as i64, 0).single() else {
                continue;
            };
            let day_key = day.format("%Y-%m-%d").to_string();
            let tokens = record
                .get("consumed_tokens")
                .and_then(Value::as_i64)
                .or_else(|| record.get("context_tokens_peak").and_then(Value::as_i64))
                .or_else(|| record.get("context_tokens").and_then(Value::as_i64))
                .unwrap_or(0)
                .max(0);
            let entry = day_buckets.entry(day_key).or_insert(0);
            *entry = entry.saturating_add(tokens);
        }
        if let Some(series) = result.get_mut(user_id) {
            for point in series.iter_mut() {
                let date_key = point
                    .get("date")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let tokens = day_buckets.get(&date_key).copied().unwrap_or(0);
                *point = json!({
                    "date": date_key,
                    "tokens": tokens,
                });
            }
        }
    }

    result
}

pub(super) fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}

pub(super) struct AdminActor {
    pub(super) scope_unit_ids: Option<HashSet<String>>,
}

pub(super) fn resolve_admin_actor(
    state: &AppState,
    headers: &AxumHeaderMap,
    allow_leader: bool,
    units: &[OrgUnitRecord],
) -> Result<AdminActor, Response> {
    if let Some(token) = auth::extract_bearer_token(headers) {
        if let Ok(Some(user)) = state.user_store.authenticate_token(&token) {
            if UserStore::is_admin(&user) {
                return Ok(AdminActor {
                    scope_unit_ids: None,
                });
            }
            if allow_leader {
                let roots = org_units::resolve_leader_root_ids(&user.user_id, units);
                if roots.is_empty() {
                    return Err(permission_denied());
                }
                let scope = org_units::collect_descendant_unit_ids(units, &roots);
                return Ok(AdminActor {
                    scope_unit_ids: Some(scope),
                });
            }
            return Err(permission_denied());
        }
    }
    Ok(AdminActor {
        scope_unit_ids: None,
    })
}

pub(super) fn ensure_unit_scope(actor: &AdminActor, unit_id: Option<&str>) -> Result<(), Response> {
    let Some(scope) = actor.scope_unit_ids.as_ref() else {
        return Ok(());
    };
    let cleaned = unit_id
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());
    if let Some(unit_id) = cleaned {
        if scope.contains(unit_id) {
            return Ok(());
        }
    }
    Err(permission_denied())
}

pub(super) fn ensure_user_scope(
    actor: &AdminActor,
    record: &UserAccountRecord,
) -> Result<(), Response> {
    ensure_unit_scope(actor, record.unit_id.as_deref())
}

pub(super) fn filter_units_by_scope(
    units: Vec<OrgUnitRecord>,
    scope: Option<&HashSet<String>>,
) -> Vec<OrgUnitRecord> {
    match scope {
        Some(scope) => units
            .into_iter()
            .filter(|unit| scope.contains(&unit.unit_id))
            .collect(),
        None => units,
    }
}

pub(super) fn build_unit_map(units: &[OrgUnitRecord]) -> HashMap<String, OrgUnitRecord> {
    units
        .iter()
        .map(|unit| (unit.unit_id.clone(), unit.clone()))
        .collect()
}

pub(super) fn org_unit_payload(record: &OrgUnitRecord) -> Value {
    json!({
        "unit_id": record.unit_id,
        "parent_id": record.parent_id,
        "name": record.name,
        "level": record.level,
        "path": record.path,
        "path_name": record.path_name,
        "sort_order": record.sort_order,
        "leader_ids": record.leader_ids,
        "created_at": record.created_at,
        "updated_at": record.updated_at,
    })
}

pub(super) fn external_link_payload(record: &ExternalLinkRecord) -> Value {
    json!({
        "link_id": record.link_id,
        "title": record.title,
        "description": record.description,
        "url": record.url,
        "icon": record.icon,
        "allowed_levels": record.allowed_levels,
        "sort_order": record.sort_order,
        "enabled": record.enabled,
        "created_at": record.created_at,
        "updated_at": record.updated_at,
    })
}

pub(super) fn normalize_external_link_levels(levels: Vec<i32>) -> Vec<i32> {
    let mut items = levels
        .into_iter()
        .filter(|level| (1..=MAX_ORG_UNIT_LEVEL).contains(level))
        .collect::<Vec<_>>();
    items.sort_unstable();
    items.dedup();
    items
}

pub(super) fn normalize_external_link_icon(raw: Option<&str>) -> String {
    let cleaned = raw.unwrap_or_default().trim();
    if cleaned.is_empty() {
        return "fa-globe".to_string();
    }

    let mut icon_name = normalize_external_icon_name(cleaned);
    let mut icon_color = None;

    if cleaned.starts_with('{') {
        if let Ok(value) = serde_json::from_str::<Value>(cleaned) {
            if let Some(name) = value.get("name").and_then(Value::as_str) {
                icon_name = normalize_external_icon_name(name);
            }
            icon_color = value
                .get("color")
                .and_then(Value::as_str)
                .and_then(normalize_external_icon_color);
        }
    }

    if let Some(color) = icon_color {
        json!({
            "name": icon_name,
            "color": color,
        })
        .to_string()
    } else {
        icon_name
    }
}

fn normalize_external_icon_name(raw: &str) -> String {
    let normalized = raw
        .trim()
        .trim_start_matches("fa-solid")
        .trim_start_matches(' ')
        .trim();
    if normalized.is_empty() {
        return "fa-globe".to_string();
    }
    let icon = normalized
        .split_whitespace()
        .find(|part| part.starts_with("fa-"))
        .unwrap_or(normalized);
    if icon.starts_with("fa-") {
        icon.to_string()
    } else {
        "fa-globe".to_string()
    }
}

fn normalize_external_icon_color(raw: &str) -> Option<String> {
    let cleaned = raw.trim().trim_start_matches('#');
    let expanded = match cleaned.len() {
        3 if cleaned.chars().all(|ch| ch.is_ascii_hexdigit()) => {
            cleaned.chars().flat_map(|ch| [ch, ch]).collect::<String>()
        }
        6 if cleaned.chars().all(|ch| ch.is_ascii_hexdigit()) => cleaned.to_string(),
        _ => return None,
    };
    Some(format!("#{}", expanded.to_ascii_lowercase()))
}

pub(super) fn normalize_optional_id(raw: Option<&str>) -> Option<String> {
    raw.map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
}

pub(super) fn normalize_leader_ids(raw: Option<Vec<String>>) -> Vec<String> {
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for value in raw.unwrap_or_default() {
        let cleaned = value.trim();
        if cleaned.is_empty() {
            continue;
        }
        if seen.insert(cleaned.to_string()) {
            output.push(cleaned.to_string());
        }
    }
    output
}

pub(super) fn next_unit_sort_order(units: &[OrgUnitRecord], parent_id: Option<&str>) -> i64 {
    units
        .iter()
        .filter(|unit| unit.parent_id.as_deref() == parent_id)
        .map(|unit| unit.sort_order)
        .max()
        .unwrap_or(-1)
        + 1
}

pub(super) fn permission_denied() -> Response {
    error_response(StatusCode::FORBIDDEN, i18n::t("error.permission_denied"))
}

pub(super) fn normalize_user_status(value: Option<&str>) -> String {
    let cleaned = value.unwrap_or("active").trim();
    if cleaned.is_empty() {
        "active".to_string()
    } else {
        cleaned.to_string()
    }
}

pub(super) fn normalize_user_roles(raw: Vec<String>) -> Vec<String> {
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for role in raw {
        let name = role.trim();
        if name.is_empty() {
            continue;
        }
        if seen.insert(name.to_string()) {
            output.push(name.to_string());
        }
    }
    if output.is_empty() {
        output.push("user".to_string());
    }
    output
}

pub(super) fn normalize_user_email(value: Option<String>) -> Option<String> {
    value.and_then(|email| {
        let cleaned = email.trim();
        if cleaned.is_empty() {
            None
        } else {
            Some(cleaned.to_string())
        }
    })
}

pub(super) fn normalize_tool_access_list(raw: Vec<String>) -> Vec<String> {
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for name in raw {
        let cleaned = name.trim();
        if cleaned.is_empty() {
            continue;
        }
        let normalized = cleaned.to_string();
        if seen.insert(normalized.clone()) {
            output.push(normalized);
        }
    }
    output
}

pub(super) fn normalize_optional_tool_access_list(raw: Option<Vec<String>>) -> Option<Vec<String>> {
    raw.and_then(|values| {
        let normalized = normalize_tool_access_list(values);
        if normalized.is_empty() {
            None
        } else {
            Some(normalized)
        }
    })
}

pub(super) fn error_response(status: StatusCode, message: String) -> Response {
    crate::api::errors::error_response(status, message)
}

#[derive(Debug, Deserialize)]
struct LlmUpdateRequest {
    llm: crate::config::LlmConfig,
}

#[derive(Debug, Deserialize)]
struct LlmContextProbeRequest {
    #[serde(default)]
    provider: Option<String>,
    base_url: String,
    #[serde(default)]
    api_key: Option<String>,
    model: String,
    #[serde(default)]
    timeout_s: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct SystemUpdateRequest {
    #[serde(default)]
    server: Option<SystemServerUpdateRequest>,
    #[serde(default)]
    security: Option<SystemSecurityUpdateRequest>,
    #[serde(default)]
    observability: Option<SystemObservabilityUpdateRequest>,
    #[serde(default)]
    cors: Option<SystemCorsUpdateRequest>,
    #[serde(default)]
    onlyoffice: Option<SystemOnlyOfficeUpdateRequest>,
    #[serde(default)]
    drawio: Option<SystemDrawioUpdateRequest>,
    #[serde(default)]
    ragflow: Option<SystemRagflowUpdateRequest>,
    #[serde(default)]
    firecrawl: Option<SystemFirecrawlUpdateRequest>,
}

#[derive(Debug, Deserialize)]
struct SystemServerUpdateRequest {
    #[serde(default)]
    max_active_sessions: Option<usize>,
    #[serde(default)]
    stream_chunk_size: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct SystemSecurityUpdateRequest {
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    external_auth_key: Option<String>,
    #[serde(default)]
    external_embed_preset_agent_name: Option<String>,
    #[serde(default)]
    external_embed_jwt_secret: Option<String>,
    #[serde(default)]
    external_embed_jwt_user_id_claim: Option<String>,
    #[serde(default)]
    allow_commands: Option<Vec<String>>,
    #[serde(default)]
    allow_paths: Option<Vec<String>>,
    #[serde(default)]
    deny_globs: Option<Vec<String>>,
    #[serde(default)]
    exec_policy_mode: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SystemObservabilityUpdateRequest {
    #[serde(default)]
    log_level: Option<String>,
    #[serde(default)]
    monitor_event_limit: Option<i64>,
    #[serde(default)]
    monitor_payload_max_chars: Option<i64>,
    #[serde(default)]
    monitor_drop_event_types: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct SystemCorsUpdateRequest {
    #[serde(default)]
    allow_origins: Option<Vec<String>>,
    #[serde(default)]
    allow_methods: Option<Vec<String>>,
    #[serde(default)]
    allow_headers: Option<Vec<String>>,
    #[serde(default)]
    allow_credentials: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct SystemOnlyOfficeUpdateRequest {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    document_server_url: Option<String>,
    #[serde(default)]
    internal_document_server_url: Option<String>,
    #[serde(default)]
    api_url: Option<String>,
    #[serde(default)]
    public_base_url: Option<String>,
    #[serde(default)]
    jwt_secret: Option<String>,
    #[serde(default)]
    jwt_header: Option<String>,
    #[serde(default)]
    token_ttl_s: Option<u64>,
    #[serde(default)]
    request_timeout_s: Option<u64>,
    #[serde(default)]
    max_download_bytes: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct SystemDrawioUpdateRequest {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    editor_url: Option<String>,
    #[serde(default)]
    max_file_bytes: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct SystemRagflowUpdateRequest {
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    timeout_s: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct SystemFirecrawlUpdateRequest {
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    timeout_secs: Option<u64>,
    #[serde(default)]
    only_main_content: Option<bool>,
    #[serde(default)]
    max_age_ms: Option<u64>,
    #[serde(default)]
    proxy: Option<String>,
    #[serde(default)]
    store_in_cache: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ServerUpdateRequest {
    #[serde(default)]
    max_active_sessions: Option<usize>,
}
