use super::{build_model_tool_success_with_hint, context::ToolContext};
use crate::i18n;
use crate::memory::{build_agent_memory_owner, normalize_agent_memory_scope, MemoryStore};
use crate::services::memory_fragments::{
    compact_memory_id_for_model, MemoryFragmentInput, MemoryFragmentListOptions,
    MemoryFragmentStore,
};
use crate::storage::MemoryFragmentRecord;
use anyhow::{anyhow, Result};
use chrono::{Local, NaiveDateTime, TimeZone};
use serde::Deserialize;
use serde_json::{json, Value};

const MAX_MEMORY_RECALL_LIMIT: usize = 30;

#[derive(Debug, Deserialize)]
struct MemoryManagerArgs {
    action: String,
    #[serde(default)]
    memory_id: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tag: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    related_memory_id: Option<String>,
    #[serde(default)]
    memory_time: Option<String>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    order: Option<String>,
}

fn normalize_memory_manager_action(raw: &str) -> String {
    let cleaned = raw.trim().to_lowercase();
    match cleaned.as_str() {
        "list" => "list".to_string(),
        "get" | "read" | "detail" => "get".to_string(),
        "add" | "create" | "append" => "add".to_string(),
        "update" | "upsert" => "update".to_string(),
        "delete" | "remove" => "remove".to_string(),
        "clear" | "reset" => "clear".to_string(),
        "recall" | "search" | "query" | "retrieve" => "search".to_string(),
        _ => String::new(),
    }
}

fn normalize_memory_record_id(payload: &MemoryManagerArgs) -> String {
    payload
        .memory_id
        .as_deref()
        .or(payload.id.as_deref())
        .or(payload.session_id.as_deref())
        .unwrap_or("")
        .trim()
        .to_string()
}

fn normalize_memory_content(payload: &MemoryManagerArgs) -> String {
    payload.content.as_deref().unwrap_or("").trim().to_string()
}

fn normalize_memory_query(payload: &MemoryManagerArgs) -> String {
    payload
        .query
        .as_deref()
        .or(payload.content.as_deref())
        .unwrap_or("")
        .trim()
        .to_string()
}

fn normalize_memory_title(payload: &MemoryManagerArgs) -> String {
    payload.title.as_deref().unwrap_or("").trim().to_string()
}

fn normalize_memory_category(payload: &MemoryManagerArgs) -> Option<String> {
    let cleaned = payload
        .tag
        .as_deref()
        .or(payload.category.as_deref())
        .unwrap_or("")
        .trim()
        .to_string();
    (!cleaned.is_empty()).then_some(cleaned)
}

fn normalize_related_memory_id(payload: &MemoryManagerArgs) -> Option<String> {
    let cleaned = payload
        .related_memory_id
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_string();
    (!cleaned.is_empty()).then_some(cleaned)
}

fn derive_memory_title(title: &str, content: &str) -> Option<String> {
    let clean_title = title.trim();
    if !clean_title.is_empty() {
        return Some(clean_title.to_string());
    }

    let clean_content = content.trim();
    for separator in ['\u{FF1A}', ':'] {
        if let Some((left, right)) = clean_content.split_once(separator) {
            let candidate_title = left.trim();
            if !candidate_title.is_empty()
                && !right.trim().is_empty()
                && candidate_title.chars().count() <= 48
            {
                return Some(candidate_title.to_string());
            }
        }
    }

    if !clean_content.is_empty() && clean_content.chars().count() <= 80 {
        return Some(clean_content.to_string());
    }

    None
}

fn normalize_memory_time(payload: &MemoryManagerArgs) -> Option<f64> {
    let cleaned = payload.memory_time.as_deref().unwrap_or("").trim();
    if cleaned.is_empty() {
        return None;
    }
    chrono::DateTime::parse_from_rfc3339(cleaned)
        .map(|value| value.timestamp() as f64)
        .ok()
        .or_else(|| {
            NaiveDateTime::parse_from_str(cleaned, "%Y-%m-%d %H:%M:%S")
                .ok()
                .and_then(|value| Local.from_local_datetime(&value).single())
                .map(|value| value.timestamp() as f64)
        })
        .or_else(|| {
            NaiveDateTime::parse_from_str(cleaned, "%Y-%m-%d %H:%M")
                .ok()
                .and_then(|value| Local.from_local_datetime(&value).single())
                .map(|value| value.timestamp() as f64)
        })
}

fn build_memory_fact_key(category: Option<&str>, title: Option<&str>, memory_id: &str) -> String {
    let category_prefix = category
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("tool-note");
    let title_suffix = title
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .chars()
                .map(|ch| if ch.is_whitespace() { '_' } else { ch })
                .take(40)
                .collect::<String>()
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| memory_id.trim().to_string());
    format!("{category_prefix}::{title_suffix}")
}

fn normalize_memory_list_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(30).clamp(1, 200)
}

fn normalize_memory_search_limit(limit: Option<i64>) -> usize {
    limit.unwrap_or(10).clamp(1, MAX_MEMORY_RECALL_LIMIT as i64) as usize
}

fn build_memory_index_item(record: &MemoryFragmentRecord) -> Value {
    json!({
        "memory_id": compact_memory_id_for_model(&record.memory_id),
        "title": &record.title_l0,
        "tag": &record.category,
        "updated_at": record.updated_at,
    })
}

fn build_memory_detail_item(record: &MemoryFragmentRecord) -> Value {
    json!({
        "memory_id": compact_memory_id_for_model(&record.memory_id),
        "title": &record.title_l0,
        "content": &record.content_l2,
        "tag": &record.category,
        "related_memory_id": record.supersedes_memory_id.as_ref().map(|value| compact_memory_id_for_model(value)),
        "memory_time": record.valid_from,
        "updated_at": record.updated_at,
    })
}

fn extract_matched_fields(reason: &Value) -> Vec<String> {
    reason
        .get("matched_fields")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .take(4)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn build_memory_search_snippet(fragment: &MemoryFragmentRecord) -> String {
    let content = fragment.content_l2.trim();
    if content.is_empty() {
        return String::new();
    }
    let mut chars = content.chars();
    let truncated = chars.by_ref().take(120).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn resolve_memory_record_id(
    fragment_store: &MemoryFragmentStore,
    user_id: &str,
    agent_id: Option<&str>,
    payload: &MemoryManagerArgs,
) -> Result<String> {
    resolve_memory_identifier_text(
        fragment_store,
        user_id,
        agent_id,
        &normalize_memory_record_id(payload),
    )
}

fn resolve_memory_identifier_text(
    fragment_store: &MemoryFragmentStore,
    user_id: &str,
    agent_id: Option<&str>,
    requested: &str,
) -> Result<String> {
    let memory_id = requested.trim().to_string();
    if memory_id.is_empty() {
        return Ok(String::new());
    }
    if fragment_store
        .get_fragment(user_id, agent_id, &memory_id)
        .is_some()
    {
        return Ok(memory_id);
    }
    let candidates = fragment_store.list_fragments(
        user_id,
        agent_id,
        MemoryFragmentListOptions {
            include_invalidated: true,
            limit: Some(200),
            ..Default::default()
        },
    );
    let mut matches = candidates
        .into_iter()
        .filter(|item| compact_memory_id_for_model(&item.memory_id) == memory_id)
        .map(|item| item.memory_id)
        .collect::<Vec<_>>();
    matches.sort();
    matches.dedup();
    match matches.len() {
        0 => Ok(memory_id),
        1 => Ok(matches.remove(0)),
        _ => Err(anyhow!(i18n::t("tool.memory_manager.not_found"))),
    }
}

fn resolve_related_memory_record_id(
    fragment_store: &MemoryFragmentStore,
    user_id: &str,
    agent_id: Option<&str>,
    payload: &MemoryManagerArgs,
) -> Result<Option<String>> {
    let Some(requested) = normalize_related_memory_id(payload) else {
        return Ok(None);
    };
    let resolved = resolve_memory_identifier_text(fragment_store, user_id, agent_id, &requested)?;
    if resolved.is_empty()
        || fragment_store
            .get_fragment(user_id, agent_id, &resolved)
            .is_none()
    {
        return Err(anyhow!(i18n::t("tool.memory_manager.not_found")));
    }
    Ok(Some(resolved))
}

fn build_memory_manager_success(
    action: &str,
    agent_scope: &str,
    data: Value,
    next_step_hint: Option<String>,
) -> Value {
    let state = "completed";
    let summary = match action {
        "list" => format!(
            "Listed {} memory entries.",
            data.get("count").and_then(Value::as_u64).unwrap_or(0)
        ),
        "get" => "Loaded a memory entry.".to_string(),
        "search" => format!(
            "Found {} memory entries.",
            data.get("count").and_then(Value::as_u64).unwrap_or(0)
        ),
        "add" => "Saved a memory entry.".to_string(),
        "update" => "Updated a memory entry.".to_string(),
        "remove" => format!(
            "Removed {} memory entries.",
            data.get("deleted").and_then(Value::as_i64).unwrap_or(0)
        ),
        "clear" => format!(
            "Cleared {} memory entries.",
            data.get("deleted").and_then(Value::as_i64).unwrap_or(0)
        ),
        _ => format!("memory_manage {action} completed."),
    };
    let mut payload = data;
    if let Some(map) = payload.as_object_mut() {
        map.insert(
            "agent_id".to_string(),
            Value::String(agent_scope.to_string()),
        );
    }
    build_model_tool_success_with_hint(action, state, summary, payload, next_step_hint)
}

fn normalize_memory_order_desc(order: Option<&str>) -> bool {
    let cleaned = order.unwrap_or("").trim().to_lowercase();
    if cleaned.is_empty() {
        return true;
    }
    !matches!(cleaned.as_str(), "asc" | "ascending")
}

fn clear_fragment_scope(
    fragment_store: &MemoryFragmentStore,
    user_id: &str,
    agent_id: Option<&str>,
) -> i64 {
    let mut deleted = 0i64;
    loop {
        let batch = fragment_store.list_fragments(
            user_id,
            agent_id,
            MemoryFragmentListOptions {
                include_invalidated: true,
                limit: Some(200),
                ..Default::default()
            },
        );
        if batch.is_empty() {
            break;
        }
        for item in &batch {
            if fragment_store.delete_fragment(user_id, agent_id, &item.memory_id) {
                deleted += 1;
            }
        }
        if batch.len() < 200 {
            break;
        }
    }
    deleted
}

pub(crate) async fn execute_memory_manager_tool(
    context: &ToolContext<'_>,
    args: &Value,
) -> Result<Value> {
    let payload: MemoryManagerArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let action = normalize_memory_manager_action(&payload.action);
    if action.is_empty() {
        return Err(anyhow!(i18n::t("tool.memory_manager.invalid_action")));
    }

    let agent_scope = normalize_agent_memory_scope(context.agent_id);
    let fragment_store = MemoryFragmentStore::new(context.storage.clone());

    let response = match action.as_str() {
        "list" => {
            let limit = normalize_memory_list_limit(payload.limit) as usize;
            let order_desc = normalize_memory_order_desc(payload.order.as_deref());
            let mut records = fragment_store.list_fragments(
                context.user_id,
                context.agent_id,
                MemoryFragmentListOptions {
                    query: payload.query.as_deref(),
                    include_invalidated: false,
                    limit: Some(limit),
                    ..Default::default()
                },
            );
            if !order_desc {
                records.reverse();
            }
            let items = records
                .iter()
                .map(build_memory_index_item)
                .collect::<Vec<_>>();
            build_memory_manager_success(
                action.as_str(),
                &agent_scope,
                json!({
                    "count": items.len(),
                    "items": items,
                }),
                Some(i18n::t("tool.memory_manager.note_new_sessions_only")),
            )
        }
        "get" => {
            let memory_id = resolve_memory_record_id(
                &fragment_store,
                context.user_id,
                context.agent_id,
                &payload,
            )?;
            if memory_id.is_empty() {
                return Err(anyhow!(i18n::t("tool.memory_manager.memory_id_required")));
            }
            let record = fragment_store
                .get_fragment(context.user_id, context.agent_id, &memory_id)
                .ok_or_else(|| anyhow!(i18n::t("tool.memory_manager.not_found")))?;
            build_memory_manager_success(
                action.as_str(),
                &agent_scope,
                json!({
                    "memory_id": compact_memory_id_for_model(&record.memory_id),
                    "item": build_memory_detail_item(&record),
                }),
                Some(i18n::t("tool.memory_manager.note_get_full_detail")),
            )
        }
        "add" => {
            let content = normalize_memory_content(&payload);
            if content.is_empty() {
                return Err(anyhow!(i18n::t("error.content_required")));
            }
            let title = normalize_memory_title(&payload);
            let category = normalize_memory_category(&payload);
            let related_memory_id = resolve_related_memory_record_id(
                &fragment_store,
                context.user_id,
                context.agent_id,
                &payload,
            )?;
            let memory_time = normalize_memory_time(&payload);
            let derived_title = derive_memory_title(&title, &content);
            let mut memory_id = normalize_memory_record_id(&payload);
            if memory_id.is_empty() {
                memory_id = compact_memory_id_for_model(&uuid::Uuid::new_v4().simple().to_string());
            } else if memory_id.chars().count() > 8 {
                memory_id = compact_memory_id_for_model(&memory_id);
            }
            let fact_key = build_memory_fact_key(
                category.as_deref(),
                derived_title
                    .as_deref()
                    .or((!title.is_empty()).then_some(title.as_str())),
                &memory_id,
            );
            let saved_record = fragment_store
                .save_fragment(
                    context.user_id,
                    context.agent_id,
                    MemoryFragmentInput {
                        memory_id: Some(memory_id.clone()),
                        source_session_id: Some(context.session_id.to_string()),
                        source_type: Some("memory_manager".to_string()),
                        category: Some(category.unwrap_or_else(|| "tool-note".to_string())),
                        title_l0: derived_title,
                        content_l2: Some(content),
                        fact_key: Some(fact_key),
                        supersedes_memory_id: related_memory_id,
                        valid_from: memory_time,
                        pinned: Some(false),
                        invalidated: Some(false),
                        ..Default::default()
                    },
                )
                .map_err(|err| anyhow!(format!("failed to save memory fragment: {err}")))?;
            build_memory_manager_success(
                action.as_str(),
                &agent_scope,
                json!({
                    "memory_id": compact_memory_id_for_model(&saved_record.memory_id),
                    "saved": true,
                }),
                Some(i18n::t("tool.memory_manager.note_new_sessions_only")),
            )
        }
        "update" => {
            let memory_id = resolve_memory_record_id(
                &fragment_store,
                context.user_id,
                context.agent_id,
                &payload,
            )?;
            if memory_id.is_empty() {
                return Err(anyhow!(i18n::t("tool.memory_manager.memory_id_required")));
            }
            let content = normalize_memory_content(&payload);
            if content.is_empty() {
                return Err(anyhow!(i18n::t("error.content_required")));
            }
            let title = normalize_memory_title(&payload);
            let category = normalize_memory_category(&payload);
            let related_memory_id = resolve_related_memory_record_id(
                &fragment_store,
                context.user_id,
                context.agent_id,
                &payload,
            )?;
            let memory_time = normalize_memory_time(&payload);
            let derived_title = derive_memory_title(&title, &content);
            let existing_fragment =
                fragment_store.get_fragment(context.user_id, context.agent_id, &memory_id);
            let fact_key = build_memory_fact_key(
                category.as_deref().or(existing_fragment
                    .as_ref()
                    .map(|item| item.category.as_str())),
                derived_title
                    .as_deref()
                    .or((!title.is_empty()).then_some(title.as_str()))
                    .or(existing_fragment
                        .as_ref()
                        .map(|item| item.title_l0.as_str())),
                &memory_id,
            );
            let updated_record = fragment_store
                .save_fragment(
                    context.user_id,
                    context.agent_id,
                    MemoryFragmentInput {
                        memory_id: Some(memory_id.clone()),
                        source_session_id: Some(context.session_id.to_string()),
                        source_type: Some("memory_manager".to_string()),
                        category,
                        title_l0: derived_title,
                        content_l2: Some(content),
                        fact_key: Some(fact_key),
                        supersedes_memory_id: related_memory_id,
                        valid_from: memory_time,
                        ..Default::default()
                    },
                )
                .map_err(|err| anyhow!(format!("failed to update memory fragment: {err}")))?;
            let owner_key = build_agent_memory_owner(context.user_id, context.agent_id);
            let memory_store = MemoryStore::new(context.storage.clone());
            cleanup_legacy_memory_record(&memory_store, &owner_key, &memory_id);
            build_memory_manager_success(
                action.as_str(),
                &agent_scope,
                json!({
                    "memory_id": compact_memory_id_for_model(&updated_record.memory_id),
                    "updated": true,
                }),
                Some(i18n::t("tool.memory_manager.note_new_sessions_only")),
            )
        }
        "remove" => {
            let memory_id = resolve_memory_record_id(
                &fragment_store,
                context.user_id,
                context.agent_id,
                &payload,
            )?;
            if memory_id.is_empty() {
                return Err(anyhow!(i18n::t("tool.memory_manager.memory_id_required")));
            }
            let fragment_deleted =
                fragment_store.delete_fragment(context.user_id, context.agent_id, &memory_id);
            let owner_key = build_agent_memory_owner(context.user_id, context.agent_id);
            let memory_store = MemoryStore::new(context.storage.clone());
            let legacy_deleted =
                cleanup_legacy_memory_record(&memory_store, &owner_key, &memory_id);
            let deleted = i64::from(fragment_deleted) + legacy_deleted;
            build_memory_manager_success(
                action.as_str(),
                &agent_scope,
                json!({
                    "memory_id": compact_memory_id_for_model(&memory_id),
                    "deleted": deleted,
                }),
                Some(i18n::t("tool.memory_manager.note_new_sessions_only")),
            )
        }
        "clear" => {
            let owner_key = build_agent_memory_owner(context.user_id, context.agent_id);
            let memory_store = MemoryStore::new(context.storage.clone());
            let deleted = clear_fragment_scope(&fragment_store, context.user_id, context.agent_id)
                + memory_store.clear_records(&owner_key);
            build_memory_manager_success(
                action.as_str(),
                &agent_scope,
                json!({
                    "deleted": deleted,
                }),
                Some(i18n::t("tool.memory_manager.note_new_sessions_only")),
            )
        }
        "search" => {
            let query = normalize_memory_query(&payload);
            let search_limit = normalize_memory_search_limit(payload.limit);
            let hits = fragment_store
                .recall_for_prompt(
                    Some(context.config),
                    context.user_id,
                    context.agent_id,
                    Some(context.session_id),
                    None,
                    (!query.is_empty()).then_some(query.as_str()),
                    Some(search_limit),
                )
                .await;
            let items = hits
                .into_iter()
                .map(|hit| {
                    let matched_fields = extract_matched_fields(&hit.reason_json);
                    let fragment = hit.fragment;
                    json!({
                        "memory_id": compact_memory_id_for_model(&fragment.memory_id),
                        "title": fragment.title_l0,
                        "tag": fragment.category,
                        "snippet": build_memory_search_snippet(&fragment),
                        "matched_in": matched_fields,
                        "updated_at": fragment.updated_at,
                    })
                })
                .collect::<Vec<_>>();
            build_memory_manager_success(
                action.as_str(),
                &agent_scope,
                json!({
                    "query": query,
                    "count": items.len(),
                    "items": items,
                }),
                Some(i18n::t("tool.memory_manager.note_recall_current_session")),
            )
        }
        _ => return Err(anyhow!(i18n::t("tool.memory_manager.invalid_action"))),
    };

    Ok(response)
}

fn cleanup_legacy_memory_record(
    memory_store: &MemoryStore,
    owner_key: &str,
    memory_id: &str,
) -> i64 {
    let cleaned = memory_id.trim();
    if cleaned.is_empty() {
        return 0;
    }
    let direct = memory_store.delete_record(owner_key, cleaned);
    if let Some(legacy_id) = cleaned.strip_prefix("legacy::") {
        return direct + memory_store.delete_record(owner_key, legacy_id);
    }
    direct
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a2a_store::A2aStore;
    use crate::config::Config;
    use crate::lsp::LspManager;
    use crate::services::tools::context::ToolContext;
    use crate::skills::SkillRegistry;
    use crate::storage::{SqliteStorage, StorageBackend};
    use crate::workspace::WorkspaceManager;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tempfile::tempdir;

    struct TestHarness {
        _dir: tempfile::TempDir,
        config: Config,
        storage: Arc<dyn StorageBackend>,
        workspace: Arc<WorkspaceManager>,
        lsp_manager: Arc<LspManager>,
        a2a_store: A2aStore,
        skills: SkillRegistry,
        http: reqwest::Client,
    }

    impl TestHarness {
        fn new() -> Self {
            let dir = tempdir().expect("tempdir");
            let db_path = dir.path().join("memory-manager-tool.db");
            let storage: Arc<dyn StorageBackend> =
                Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
            storage.ensure_initialized().expect("init storage");
            let config = Config::default();
            let workspace = Arc::new(WorkspaceManager::new(
                dir.path().to_string_lossy().as_ref(),
                storage.clone(),
                0,
                &HashMap::new(),
            ));
            let lsp_manager = LspManager::new(workspace.clone());
            Self {
                _dir: dir,
                config,
                storage,
                workspace,
                lsp_manager,
                a2a_store: A2aStore::default(),
                skills: SkillRegistry::default(),
                http: reqwest::Client::new(),
            }
        }

        fn context<'a>(
            &'a self,
            user_id: &'a str,
            session_id: &'a str,
            agent_id: Option<&'a str>,
        ) -> ToolContext<'a> {
            ToolContext {
                user_id,
                session_id,
                workspace_id: "workspace-test",
                agent_id,
                user_round: None,
                model_round: None,
                is_admin: false,
                storage: self.storage.clone(),
                orchestrator: None,
                monitor: None,
                beeroom_realtime: None,
                workspace: self.workspace.clone(),
                lsp_manager: self.lsp_manager.clone(),
                config: &self.config,
                a2a_store: &self.a2a_store,
                skills: &self.skills,
                gateway: None,
                user_world: None,
                cron_wake_signal: None,
                user_tool_manager: None,
                user_tool_bindings: None,
                user_tool_store: None,
                request_config_overrides: None,
                allow_roots: None,
                read_roots: None,
                command_sessions: None,
                event_emitter: None,
                http: &self.http,
            }
        }
    }

    #[test]
    fn normalize_action_supports_search_and_get_aliases() {
        assert_eq!(normalize_memory_manager_action("search"), "search");
        assert_eq!(normalize_memory_manager_action("recall"), "search");
        assert_eq!(normalize_memory_manager_action("query"), "search");
        assert_eq!(normalize_memory_manager_action("retrieve"), "search");
        assert_eq!(normalize_memory_manager_action("get"), "get");
        assert_eq!(normalize_memory_manager_action("read"), "get");
    }

    #[test]
    fn normalize_search_limit_clamps_range() {
        assert_eq!(normalize_memory_search_limit(None), 10);
        assert_eq!(normalize_memory_search_limit(Some(0)), 1);
        assert_eq!(normalize_memory_search_limit(Some(99)), 30);
    }

    #[test]
    fn derive_memory_title_prefers_explicit_or_prefix_title() {
        assert_eq!(
            derive_memory_title("User name", "User name: Zhou Huajian"),
            Some("User name".to_string())
        );
        assert_eq!(
            derive_memory_title("", "User name: Zhou Huajian"),
            Some("User name".to_string())
        );
    }

    #[tokio::test]
    async fn memory_manager_tool_uses_index_list_search_and_get_detail() {
        let harness = TestHarness::new();
        let context = harness.context("u1", "sess-idx", Some("agent-demo"));

        let add = execute_memory_manager_tool(
            &context,
            &json!({
                "action": "add",
                "memory_id": "pref-reply-language-v2",
                "title": "Reply language",
                "content": "Reply in Chinese by default unless the user explicitly asks for another language.",
                "tag": "response_preference"
            }),
        )
        .await
        .expect("add indexed memory");
        assert_eq!(
            add.pointer("/data/saved").and_then(Value::as_bool),
            Some(true)
        );
        let compact_id = compact_memory_id_for_model("pref-reply-language-v2");
        assert_eq!(
            add.pointer("/data/memory_id").and_then(Value::as_str),
            Some(compact_id.as_str())
        );

        let listed = execute_memory_manager_tool(&context, &json!({ "action": "list" }))
            .await
            .expect("list indexed memory");
        let list_item = listed
            .pointer("/data/items")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .cloned()
            .expect("list item");
        assert_eq!(
            list_item.get("memory_id").and_then(Value::as_str),
            Some(compact_id.as_str())
        );
        assert_eq!(
            list_item.get("title").and_then(Value::as_str),
            Some("Reply language")
        );
        assert!(list_item.get("summary").is_none());
        assert!(list_item.get("content").is_none());
        assert!(list_item.get("status").is_none());
        assert!(list_item.get("pinned").is_none());

        let searched = execute_memory_manager_tool(
            &context,
            &json!({
                "action": "search",
                "query": "Chinese"
            }),
        )
        .await
        .expect("search indexed memory");
        let search_item = searched
            .pointer("/data/items")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .cloned()
            .expect("search item");
        assert_eq!(
            search_item.get("title").and_then(Value::as_str),
            Some("Reply language")
        );
        assert!(search_item.get("snippet").and_then(Value::as_str).is_some());
        assert!(search_item
            .get("matched_in")
            .and_then(Value::as_array)
            .is_some());
        assert!(search_item.get("content").is_none());
        assert!(search_item.get("status").is_none());
        assert!(search_item.get("pinned").is_none());
        assert!(search_item.get("matched_terms").is_none());
        assert!(search_item.get("why").is_none());

        let loaded = execute_memory_manager_tool(
            &context,
            &json!({
                "action": "get",
                "memory_id": compact_id
            }),
        )
        .await
        .expect("get indexed memory");
        assert_eq!(
            loaded.pointer("/data/item/title").and_then(Value::as_str),
            Some("Reply language")
        );
        assert_eq!(
            loaded.pointer("/data/item/content").and_then(Value::as_str),
            Some(
                "Reply in Chinese by default unless the user explicitly asks for another language."
            )
        );
        assert!(loaded.pointer("/data/item/status").is_none());
        assert!(loaded.pointer("/data/item/pinned").is_none());
        assert!(loaded.pointer("/data/item/source_type").is_none());

        let removed = execute_memory_manager_tool(
            &context,
            &json!({
                "action": "remove",
                "memory_id": compact_id
            }),
        )
        .await
        .expect("remove indexed memory");
        assert_eq!(
            removed.pointer("/data/deleted").and_then(Value::as_i64),
            Some(1)
        );
    }

    #[tokio::test]
    async fn memory_manager_tool_defaults_to_default_agent_scope() {
        let harness = TestHarness::new();
        let context = harness.context("u1", "sess-2", None);

        let add = execute_memory_manager_tool(
            &context,
            &json!({
                "action": "add",
                "memory_id": "pref-tone-default",
                "content": "Prefer concise answers by default."
            }),
        )
        .await
        .expect("add default-scope memory");
        assert_eq!(
            add.pointer("/data/saved").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            add.pointer("/data/agent_id").and_then(Value::as_str),
            Some("__default__")
        );
        assert_eq!(
            add.pointer("/data/memory_id").and_then(Value::as_str),
            Some(compact_memory_id_for_model("pref-tone-default").as_str())
        );

        let fragment_store = MemoryFragmentStore::new(harness.storage.clone());
        let fragments = fragment_store.list_fragments(
            "u1",
            Some("__default__"),
            MemoryFragmentListOptions::default(),
        );
        assert_eq!(fragments.len(), 1);
        assert_eq!(fragments[0].memory_id, "preftone");
        assert_eq!(fragments[0].agent_id, "__default__");
    }

    #[tokio::test]
    async fn memory_manager_tool_supports_related_memory_and_time() {
        let harness = TestHarness::new();
        let context = harness.context("u1", "sess-3", Some("agent-demo"));

        execute_memory_manager_tool(
            &context,
            &json!({
                "action": "add",
                "memory_id": "profile-name",
                "title": "User name",
                "content": "The user's name is Zhou Huajian.",
                "tag": "profile"
            }),
        )
        .await
        .expect("add source memory");

        let add = execute_memory_manager_tool(
            &context,
            &json!({
                "action": "add",
                "memory_id": "display-name-v2",
                "title": "Preferred display name",
                "content": "Use Zhou Huajian as the user's preferred display name.",
                "tag": "profile",
                "related_memory_id": compact_memory_id_for_model("profile-name"),
                "memory_time": "2026-04-12T08:37:00+08:00"
            }),
        )
        .await
        .expect("add related memory");
        let compact_id = compact_memory_id_for_model("display-name-v2");
        assert_eq!(
            add.pointer("/data/memory_id").and_then(Value::as_str),
            Some(compact_id.as_str())
        );

        let fragment_store = MemoryFragmentStore::new(harness.storage.clone());
        let fragment = fragment_store
            .get_fragment("u1", Some("agent-demo"), &compact_id)
            .expect("get related fragment");
        assert_eq!(fragment.title_l0, "Preferred display name");
        assert_eq!(
            fragment.content_l2,
            "Use Zhou Huajian as the user's preferred display name."
        );
        assert_eq!(fragment.category, "profile");
        assert_eq!(
            fragment.supersedes_memory_id.as_deref(),
            Some(compact_memory_id_for_model("profile-name").as_str())
        );
        assert_eq!(fragment.valid_from, 1_775_954_220.0);
    }

    #[tokio::test]
    async fn memory_manager_tool_default_limits_keep_index_payload_compact() {
        let harness = TestHarness::new();
        let context = harness.context("u1", "sess-limit", Some("agent-demo"));

        for idx in 0..35 {
            execute_memory_manager_tool(
                &context,
                &json!({
                    "action": "add",
                    "memory_id": format!("ml{idx:06}"),
                    "title": format!("Memory #{idx:02}"),
                    "content": format!("Shared alpha detail {idx:02}")
                }),
            )
            .await
            .expect("seed memory fragment");
        }

        let listed = execute_memory_manager_tool(&context, &json!({ "action": "list" }))
            .await
            .expect("list default limit");
        let list_items = listed
            .pointer("/data/items")
            .and_then(Value::as_array)
            .cloned()
            .expect("list items");
        assert_eq!(
            listed.pointer("/data/count").and_then(Value::as_u64),
            Some(30)
        );
        assert_eq!(list_items.len(), 30);
        assert!(list_items.iter().all(|item| item.get("content").is_none()));
        assert!(list_items.iter().all(|item| item.get("summary").is_none()));
        assert!(list_items.iter().all(|item| item.get("status").is_none()));
        assert!(list_items.iter().all(|item| item.get("pinned").is_none()));

        let searched = execute_memory_manager_tool(
            &context,
            &json!({
                "action": "search",
                "query": "Shared alpha"
            }),
        )
        .await
        .expect("search default limit");
        let search_items = searched
            .pointer("/data/items")
            .and_then(Value::as_array)
            .cloned()
            .expect("search items");
        assert_eq!(
            searched.pointer("/data/count").and_then(Value::as_u64),
            Some(10)
        );
        assert_eq!(search_items.len(), 10);
        assert!(search_items
            .iter()
            .all(|item| item.get("content").is_none()));
        assert!(search_items.iter().all(|item| item.get("why").is_none()));
        assert!(search_items
            .iter()
            .all(|item| item.get("matched_terms").is_none()));
        assert!(search_items
            .iter()
            .all(|item| item.get("snippet").and_then(Value::as_str).is_some()));
    }

    #[tokio::test]
    async fn memory_manager_tool_alias_actions_return_canonical_action_names() {
        let harness = TestHarness::new();
        let context = harness.context("u1", "sess-alias", Some("agent-demo"));

        execute_memory_manager_tool(
            &context,
            &json!({
                "action": "add",
                "memory_id": "pref-language-alias",
                "title": "Reply language",
                "content": "Reply in Chinese by default unless the user explicitly asks for English."
            }),
        )
        .await
        .expect("add alias memory");

        let searched = execute_memory_manager_tool(
            &context,
            &json!({
                "action": "recall",
                "query": "Chinese"
            }),
        )
        .await
        .expect("search via recall alias");
        assert_eq!(
            searched.get("action").and_then(Value::as_str),
            Some("search")
        );
        assert!(searched.pointer("/data/items/0/content").is_none());
        assert!(searched.pointer("/data/items/0/why").is_none());

        let loaded = execute_memory_manager_tool(
            &context,
            &json!({
                "action": "read",
                "memory_id": compact_memory_id_for_model("pref-language-alias")
            }),
        )
        .await
        .expect("get via read alias");
        assert_eq!(loaded.get("action").and_then(Value::as_str), Some("get"));
        assert_eq!(
            loaded.pointer("/data/item/content").and_then(Value::as_str),
            Some("Reply in Chinese by default unless the user explicitly asks for English.")
        );

        let removed = execute_memory_manager_tool(
            &context,
            &json!({
                "action": "delete",
                "memory_id": compact_memory_id_for_model("pref-language-alias")
            }),
        )
        .await
        .expect("remove via delete alias");
        assert_eq!(
            removed.get("action").and_then(Value::as_str),
            Some("remove")
        );
        assert_eq!(
            removed.pointer("/data/deleted").and_then(Value::as_i64),
            Some(1)
        );
    }

    #[tokio::test]
    async fn memory_manager_tool_requires_memory_id_for_read_write_delete_actions() {
        let harness = TestHarness::new();
        let context = harness.context("u1", "sess-required", Some("agent-demo"));
        let expected = i18n::t("tool.memory_manager.memory_id_required");

        let get_err = execute_memory_manager_tool(&context, &json!({ "action": "get" }))
            .await
            .expect_err("get without memory_id should fail");
        assert_eq!(get_err.to_string(), expected);

        let update_err = execute_memory_manager_tool(
            &context,
            &json!({
                "action": "update",
                "content": "Reply in Chinese by default."
            }),
        )
        .await
        .expect_err("update without memory_id should fail");
        assert_eq!(update_err.to_string(), expected);

        let remove_err = execute_memory_manager_tool(&context, &json!({ "action": "remove" }))
            .await
            .expect_err("remove without memory_id should fail");
        assert_eq!(remove_err.to_string(), expected);
    }

    #[tokio::test]
    async fn memory_manager_tool_resolves_compact_ids_for_existing_long_records() {
        let harness = TestHarness::new();
        let context = harness.context("u1", "sess-long", Some("agent-demo"));
        let fragment_store = MemoryFragmentStore::new(harness.storage.clone());

        let saved = fragment_store
            .save_fragment(
                "u1",
                Some("agent-demo"),
                MemoryFragmentInput {
                    memory_id: Some("memory-id-long-12345".to_string()),
                    source_session_id: Some("sess-long".to_string()),
                    source_type: Some("manual".to_string()),
                    category: Some("note".to_string()),
                    title_l0: Some("Long id memory".to_string()),
                    content_l2: Some(
                        "A stored fragment that keeps a long internal id.".to_string(),
                    ),
                    fact_key: Some("note::long-id-memory".to_string()),
                    ..Default::default()
                },
            )
            .expect("seed long memory id");
        let compact_id = compact_memory_id_for_model(&saved.memory_id);
        assert_eq!(
            compact_id,
            compact_memory_id_for_model("memory-id-long-12345")
        );

        let listed = execute_memory_manager_tool(&context, &json!({ "action": "list" }))
            .await
            .expect("list long-id memory");
        assert_eq!(
            listed
                .pointer("/data/items/0/memory_id")
                .and_then(Value::as_str),
            Some(compact_id.as_str())
        );

        let loaded = execute_memory_manager_tool(
            &context,
            &json!({
                "action": "get",
                "memory_id": compact_id
            }),
        )
        .await
        .expect("load with compact id");
        assert_eq!(
            loaded.pointer("/data/item/title").and_then(Value::as_str),
            Some("Long id memory")
        );

        let removed = execute_memory_manager_tool(
            &context,
            &json!({
                "action": "remove",
                "memory_id": compact_id
            }),
        )
        .await
        .expect("remove with compact id");
        assert_eq!(
            removed.pointer("/data/deleted").and_then(Value::as_i64),
            Some(1)
        );
    }
}
