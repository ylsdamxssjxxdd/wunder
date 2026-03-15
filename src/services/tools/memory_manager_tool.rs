use super::context::ToolContext;
use crate::i18n;
use crate::memory::{build_agent_memory_owner, normalize_agent_memory_scope, MemoryStore};
use crate::services::memory_fragments::MemoryFragmentStore;
use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

const MAX_MEMORY_RECALL_LIMIT: usize = 12;

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
    content: Option<String>,
    #[serde(default)]
    summary: Option<String>,
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

fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
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

    let owner_key = build_agent_memory_owner(context.user_id, context.agent_id);
    let agent_scope = normalize_agent_memory_scope(context.agent_id);
    let memory_store = MemoryStore::new(context.storage.clone());

    let response = match action.as_str() {
        "list" => {
            let limit = normalize_memory_list_limit(payload.limit);
            let records = memory_store.list_records(
                &owner_key,
                Some(limit),
                normalize_memory_order_desc(payload.order.as_deref()),
            );
            let items = records
                .into_iter()
                .map(|record| {
                    json!({
                        "memory_id": record.session_id,
                        "content": record.summary,
                        "created_time_ts": record.created_time,
                        "updated_time_ts": record.updated_time,
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
            let mut memory_id = normalize_memory_record_id(&payload);
            if memory_id.is_empty() {
                memory_id = format!("mem_{}", Uuid::new_v4().simple());
            }
            let saved =
                memory_store.upsert_record(&owner_key, &memory_id, &content, Some(now_ts()), None);
            json!({
                "action": action,
                "agent_id": agent_scope,
                "memory_id": memory_id,
                "saved": saved,
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
            let updated =
                memory_store.update_record(&owner_key, &memory_id, &content, Some(now_ts()));
            json!({
                "action": action,
                "agent_id": agent_scope,
                "memory_id": memory_id,
                "updated": updated,
                "note": i18n::t("tool.memory_manager.note_new_sessions_only"),
            })
        }
        "delete" => {
            let memory_id = normalize_memory_record_id(&payload);
            if memory_id.is_empty() {
                return Err(anyhow!(i18n::t("error.content_required")));
            }
            let deleted = memory_store.delete_record(&owner_key, &memory_id);
            json!({
                "action": action,
                "agent_id": agent_scope,
                "memory_id": memory_id,
                "deleted": deleted,
                "note": i18n::t("tool.memory_manager.note_new_sessions_only"),
            })
        }
        "clear" => {
            let deleted = memory_store.clear_records(&owner_key);
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
            let fragment_store = MemoryFragmentStore::new(context.storage.clone());
            let hits = fragment_store.recall_for_prompt(
                context.user_id,
                context.agent_id,
                Some(context.session_id),
                None,
                (!query.is_empty()).then_some(query.as_str()),
                Some(recall_limit),
            );
            let items = hits
                .into_iter()
                .map(|hit| {
                    let fragment = hit.fragment;
                    json!({
                        "memory_id": fragment.memory_id,
                        "title": fragment.title_l0,
                        "summary": fragment.summary_l1,
                        "content": fragment.content_l2,
                        "category": fragment.category,
                        "source_type": fragment.source_type,
                        "tags": fragment.tags,
                        "entities": fragment.entities,
                        "pinned": fragment.pinned,
                        "confirmed_by_user": fragment.confirmed_by_user,
                        "status": fragment.status,
                        "updated_at": fragment.updated_at,
                        "score": hit.final_score,
                        "reason": hit.reason_json,
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(normalize_memory_recall_limit(Some(99)), 12);
    }
}
