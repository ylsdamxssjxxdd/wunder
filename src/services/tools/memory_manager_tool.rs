use super::context::ToolContext;
use crate::i18n;
use crate::memory::{build_agent_memory_owner, normalize_agent_memory_scope, MemoryStore};
use crate::services::memory_fragments::{
    MemoryFragmentInput, MemoryFragmentListOptions, MemoryFragmentStore,
};
use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

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
    summary: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
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
        "add" | "create" | "append" => "add".to_string(),
        "update" | "upsert" => "update".to_string(),
        "delete" | "remove" => "delete".to_string(),
        "clear" | "reset" => "clear".to_string(),
        "recall" | "search" | "query" | "retrieve" => "recall".to_string(),
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
    payload
        .content
        .as_deref()
        .or(payload.summary.as_deref())
        .unwrap_or("")
        .trim()
        .to_string()
}

fn normalize_memory_query(payload: &MemoryManagerArgs) -> String {
    payload
        .query
        .as_deref()
        .or(payload.content.as_deref())
        .or(payload.summary.as_deref())
        .unwrap_or("")
        .trim()
        .to_string()
}

fn normalize_memory_title(payload: &MemoryManagerArgs) -> String {
    payload.title.as_deref().unwrap_or("").trim().to_string()
}

fn normalize_memory_summary(payload: &MemoryManagerArgs) -> String {
    payload.summary.as_deref().unwrap_or("").trim().to_string()
}

fn normalize_memory_category(payload: &MemoryManagerArgs) -> Option<String> {
    let cleaned = payload.category.as_deref().unwrap_or("").trim().to_string();
    (!cleaned.is_empty()).then_some(cleaned)
}

fn normalize_memory_tags(payload: &MemoryManagerArgs) -> Option<Vec<String>> {
    let tags = payload
        .tags
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>();
    (!tags.is_empty()).then_some(tags)
}

fn derive_memory_title_and_summary(
    title: &str,
    summary: &str,
    content: &str,
) -> (Option<String>, Option<String>) {
    let clean_title = title.trim();
    let clean_summary = summary.trim();
    if !clean_title.is_empty() || !clean_summary.is_empty() {
        return (
            (!clean_title.is_empty()).then(|| clean_title.to_string()),
            (!clean_summary.is_empty()).then(|| clean_summary.to_string()),
        );
    }

    let clean_content = content.trim();
    for separator in ['：', ':'] {
        if let Some((left, right)) = clean_content.split_once(separator) {
            let candidate_title = left.trim();
            let candidate_summary = right.trim();
            if !candidate_title.is_empty()
                && !candidate_summary.is_empty()
                && candidate_title.chars().count() <= 48
            {
                return (
                    Some(candidate_title.to_string()),
                    Some(candidate_summary.to_string()),
                );
            }
        }
    }

    if !clean_content.is_empty() && clean_content.chars().count() <= 80 {
        return (Some(clean_content.to_string()), None);
    }

    (None, None)
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
    limit.unwrap_or(50).clamp(1, 200)
}

fn normalize_memory_recall_limit(limit: Option<i64>) -> usize {
    limit.unwrap_or(6).clamp(1, MAX_MEMORY_RECALL_LIMIT as i64) as usize
}

fn normalize_memory_order_desc(order: Option<&str>) -> bool {
    let cleaned = order.unwrap_or("").trim().to_lowercase();
    if cleaned.is_empty() {
        return true;
    }
    !matches!(cleaned.as_str(), "asc" | "ascending")
}

fn compact_recall_reason(reason: &Value, pinned: bool, query: &str) -> (Vec<String>, String) {
    let matched_terms = reason
        .get("matched_terms")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .take(3)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let matched_fields = reason
        .get("matched_fields")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .take(2)
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default();
    let mut parts = Vec::new();
    if !query.trim().is_empty() && !matched_terms.is_empty() {
        parts.push(format!("matched {}", matched_terms.join(", ")));
    }
    if !query.trim().is_empty() && !matched_fields.is_empty() {
        parts.push(format!("in {matched_fields}"));
    }
    if pinned {
        parts.push("pinned".to_string());
    }
    if parts.is_empty() {
        parts.push(if query.trim().is_empty() {
            "recent memory".to_string()
        } else {
            "keyword recall".to_string()
        });
    }
    (matched_terms, parts.join("; "))
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
                    include_invalidated: true,
                    limit: Some(limit),
                    ..Default::default()
                },
            );
            if !order_desc {
                records.reverse();
            }
            let items = records
                .into_iter()
                .map(|record| {
                    json!({
                        "memory_id": record.memory_id,
                        "title": record.title_l0,
                        "summary": record.summary_l1,
                        "content": record.content_l2,
                        "category": record.category,
                        "source_type": record.source_type,
                        "status": record.status,
                        "created_time_ts": record.created_at,
                        "updated_time_ts": record.updated_at,
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "action": action,
                "agent_id": agent_scope,
                "count": items.len(),
                "items": items,
                "note": i18n::t("tool.memory_manager.note_new_sessions_only"),
            })
        }
        "add" => {
            let content = normalize_memory_content(&payload);
            if content.is_empty() {
                return Err(anyhow!(i18n::t("error.content_required")));
            }
            let title = normalize_memory_title(&payload);
            let summary = normalize_memory_summary(&payload);
            let category = normalize_memory_category(&payload);
            let tags = normalize_memory_tags(&payload);
            let (derived_title, derived_summary) =
                derive_memory_title_and_summary(&title, &summary, &content);
            let mut memory_id = normalize_memory_record_id(&payload);
            if memory_id.is_empty() {
                memory_id = format!("mem_{}", Uuid::new_v4().simple());
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
                        summary_l1: derived_summary,
                        content_l2: Some(content),
                        fact_key: Some(fact_key),
                        tags,
                        pinned: Some(false),
                        invalidated: Some(false),
                        ..Default::default()
                    },
                )
                .map_err(|err| anyhow!(format!("failed to save memory fragment: {err}")))?;
            json!({
                "action": action,
                "agent_id": agent_scope,
                "memory_id": saved_record.memory_id,
                "saved": true,
                "note": i18n::t("tool.memory_manager.note_new_sessions_only"),
            })
        }
        "update" => {
            let memory_id = normalize_memory_record_id(&payload);
            if memory_id.is_empty() {
                return Err(anyhow!(i18n::t("error.content_required")));
            }
            let content = normalize_memory_content(&payload);
            if content.is_empty() {
                return Err(anyhow!(i18n::t("error.content_required")));
            }
            let title = normalize_memory_title(&payload);
            let summary = normalize_memory_summary(&payload);
            let category = normalize_memory_category(&payload);
            let tags = normalize_memory_tags(&payload);
            let (derived_title, derived_summary) =
                derive_memory_title_and_summary(&title, &summary, &content);
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
                        summary_l1: derived_summary,
                        content_l2: Some(content),
                        fact_key: Some(fact_key),
                        tags,
                        ..Default::default()
                    },
                )
                .map_err(|err| anyhow!(format!("failed to update memory fragment: {err}")))?;
            let owner_key = build_agent_memory_owner(context.user_id, context.agent_id);
            let memory_store = MemoryStore::new(context.storage.clone());
            cleanup_legacy_memory_record(&memory_store, &owner_key, &memory_id);
            json!({
                "action": action,
                "agent_id": agent_scope,
                "memory_id": updated_record.memory_id,
                "updated": true,
                "note": i18n::t("tool.memory_manager.note_new_sessions_only"),
            })
        }
        "delete" => {
            let memory_id = normalize_memory_record_id(&payload);
            if memory_id.is_empty() {
                return Err(anyhow!(i18n::t("error.content_required")));
            }
            let fragment_deleted =
                fragment_store.delete_fragment(context.user_id, context.agent_id, &memory_id);
            let owner_key = build_agent_memory_owner(context.user_id, context.agent_id);
            let memory_store = MemoryStore::new(context.storage.clone());
            let legacy_deleted =
                cleanup_legacy_memory_record(&memory_store, &owner_key, &memory_id);
            let deleted = i64::from(fragment_deleted) + legacy_deleted;
            json!({
                "action": action,
                "agent_id": agent_scope,
                "memory_id": memory_id,
                "deleted": deleted,
                "note": i18n::t("tool.memory_manager.note_new_sessions_only"),
            })
        }
        "clear" => {
            let owner_key = build_agent_memory_owner(context.user_id, context.agent_id);
            let memory_store = MemoryStore::new(context.storage.clone());
            let deleted = clear_fragment_scope(&fragment_store, context.user_id, context.agent_id)
                + memory_store.clear_records(&owner_key);
            json!({
                "action": action,
                "agent_id": agent_scope,
                "deleted": deleted,
                "note": i18n::t("tool.memory_manager.note_new_sessions_only"),
            })
        }
        "recall" => {
            let query = normalize_memory_query(&payload);
            let recall_limit = normalize_memory_recall_limit(payload.limit);
            let hits = fragment_store
                .recall_for_prompt(
                    Some(context.config),
                    context.user_id,
                    context.agent_id,
                    Some(context.session_id),
                    None,
                    (!query.is_empty()).then_some(query.as_str()),
                    Some(recall_limit),
                )
                .await;
            let items = hits
                .into_iter()
                .map(|hit| {
                    let fragment = hit.fragment;
                    let (matched_terms, why) =
                        compact_recall_reason(&hit.reason_json, fragment.pinned, &query);
                    json!({
                        "memory_id": fragment.memory_id,
                        "title": fragment.title_l0,
                        "summary": fragment.summary_l1,
                        "content": fragment.content_l2,
                        "category": fragment.category,
                        "source_type": fragment.source_type,
                        "tags": fragment.tags,
                        "pinned": fragment.pinned,
                        "status": fragment.status,
                        "updated_at": fragment.updated_at,
                        "matched_terms": matched_terms,
                        "why": why,
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "action": action,
                "agent_id": agent_scope,
                "query": query,
                "count": items.len(),
                "items": items,
                "note": i18n::t("tool.memory_manager.note_recall_current_session"),
            })
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
                event_emitter: None,
                http: &self.http,
            }
        }
    }

    #[test]
    fn normalize_action_supports_recall_aliases() {
        assert_eq!(normalize_memory_manager_action("recall"), "recall");
        assert_eq!(normalize_memory_manager_action("search"), "recall");
        assert_eq!(normalize_memory_manager_action("query"), "recall");
        assert_eq!(normalize_memory_manager_action("retrieve"), "recall");
    }

    #[test]
    fn normalize_recall_limit_clamps_range() {
        assert_eq!(normalize_memory_recall_limit(None), 6);
        assert_eq!(normalize_memory_recall_limit(Some(0)), 1);
        assert_eq!(normalize_memory_recall_limit(Some(99)), 30);
    }

    #[test]
    fn derive_memory_title_and_summary_prefers_structured_fields() {
        assert_eq!(
            derive_memory_title_and_summary("用户姓名", "周华健", "用户姓名：周华健"),
            (Some("用户姓名".to_string()), Some("周华健".to_string()))
        );
        assert_eq!(
            derive_memory_title_and_summary("", "", "用户姓名：周华健"),
            (Some("用户姓名".to_string()), Some("周华健".to_string()))
        );
    }

    #[tokio::test]
    async fn memory_manager_tool_writes_visible_fragments() {
        let harness = TestHarness::new();
        let context = harness.context("u1", "sess-1", Some("agent-demo"));

        let add = execute_memory_manager_tool(
            &context,
            &json!({
                "action": "add",
                "memory_id": "pref-reply-language",
                "content": "默认使用中文回答。"
            }),
        )
        .await
        .expect("add memory");
        assert_eq!(add.get("saved").and_then(Value::as_bool), Some(true));

        let fragment_store = MemoryFragmentStore::new(harness.storage.clone());
        let fragments = fragment_store.list_fragments(
            "u1",
            Some("agent-demo"),
            MemoryFragmentListOptions::default(),
        );
        assert_eq!(fragments.len(), 1);
        assert_eq!(fragments[0].memory_id, "pref-reply-language");
        assert_eq!(fragments[0].source_type, "memory-manager");
        assert_eq!(fragments[0].category, "tool-note");
        assert_eq!(fragments[0].title_l0, "默认使用中文回答。");

        let listed = execute_memory_manager_tool(&context, &json!({ "action": "list" }))
            .await
            .expect("list memory");
        assert_eq!(listed.get("count").and_then(Value::as_u64), Some(1));

        let recalled = execute_memory_manager_tool(
            &context,
            &json!({
                "action": "recall",
                "query": "中文回答"
            }),
        )
        .await
        .expect("recall memory");
        assert_eq!(recalled.get("count").and_then(Value::as_u64), Some(1));
        let recall_item = recalled
            .get("items")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .cloned()
            .expect("recall item");
        assert!(recall_item.get("score").is_none());
        assert!(recall_item.get("lexical_score").is_none());
        assert!(recall_item.get("reason").is_none());
        assert!(recall_item.get("why").and_then(Value::as_str).is_some());

        let updated = execute_memory_manager_tool(
            &context,
            &json!({
                "action": "update",
                "memory_id": "pref-reply-language",
                "content": "默认使用简体中文回答。"
            }),
        )
        .await
        .expect("update memory");
        assert_eq!(updated.get("updated").and_then(Value::as_bool), Some(true));

        let refreshed = fragment_store
            .get_fragment("u1", Some("agent-demo"), "pref-reply-language")
            .expect("get updated fragment");
        assert_eq!(refreshed.content_l2, "默认使用简体中文回答。");

        let deleted = execute_memory_manager_tool(
            &context,
            &json!({
                "action": "delete",
                "memory_id": "pref-reply-language"
            }),
        )
        .await
        .expect("delete memory");
        assert_eq!(deleted.get("deleted").and_then(Value::as_i64), Some(1));
        assert!(fragment_store
            .get_fragment("u1", Some("agent-demo"), "pref-reply-language")
            .is_none());
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
        assert_eq!(add.get("saved").and_then(Value::as_bool), Some(true));
        assert_eq!(
            add.get("agent_id").and_then(Value::as_str),
            Some("__default__")
        );

        let fragment_store = MemoryFragmentStore::new(harness.storage.clone());
        let fragments = fragment_store.list_fragments(
            "u1",
            Some("__default__"),
            MemoryFragmentListOptions::default(),
        );
        assert_eq!(fragments.len(), 1);
        assert_eq!(fragments[0].memory_id, "pref-tone-default");
        assert_eq!(fragments[0].agent_id, "__default__");

        let listed = execute_memory_manager_tool(&context, &json!({ "action": "list" }))
            .await
            .expect("list default-scope memory");
        assert_eq!(listed.get("count").and_then(Value::as_u64), Some(1));
        assert_eq!(
            listed.get("agent_id").and_then(Value::as_str),
            Some("__default__")
        );
    }

    #[tokio::test]
    async fn memory_manager_tool_supports_structured_fields() {
        let harness = TestHarness::new();
        let context = harness.context("u1", "sess-3", Some("agent-demo"));

        let add = execute_memory_manager_tool(
            &context,
            &json!({
                "action": "add",
                "memory_id": "profile-name",
                "title": "用户姓名",
                "summary": "周华健",
                "content": "用户姓名：周华健",
                "category": "profile",
                "tags": ["identity", "name"]
            }),
        )
        .await
        .expect("add structured memory");
        assert_eq!(add.get("saved").and_then(Value::as_bool), Some(true));

        let fragment_store = MemoryFragmentStore::new(harness.storage.clone());
        let fragment = fragment_store
            .get_fragment("u1", Some("agent-demo"), "profile-name")
            .expect("get structured fragment");
        assert_eq!(fragment.title_l0, "用户姓名");
        assert_eq!(fragment.summary_l1, "周华健");
        assert_eq!(fragment.content_l2, "用户姓名：周华健");
        assert_eq!(fragment.category, "profile");
        assert_eq!(
            fragment.tags,
            vec!["identity".to_string(), "name".to_string()]
        );
        assert_eq!(fragment.fact_key, "profile::用户姓名");
    }
}
