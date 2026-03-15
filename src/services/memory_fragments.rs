use crate::i18n;
use crate::services::memory::{
    build_agent_memory_owner, normalize_agent_memory_scope, MemoryStore,
};
use crate::storage::{MemoryFragmentRecord, MemoryHitRecord, StorageBackend};
use anyhow::Result;
use chrono::Utc;
use serde::Serialize;
use serde_json::{json, Value};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::sync::Arc;
use tracing::warn;
use uuid::Uuid;

const DEFAULT_CATEGORY: &str = "session_summary";
const DEFAULT_SOURCE_TYPE: &str = "manual";
const LEGACY_SOURCE_TYPE: &str = "legacy_summary";
const DEFAULT_TIER: &str = "working";
const STATUS_ACTIVE: &str = "active";
const STATUS_INVALIDATED: &str = "invalidated";
const DEFAULT_IMPORTANCE: f64 = 0.6;
const DEFAULT_CONFIDENCE: f64 = 0.7;
const DEFAULT_RECALL_LIMIT: usize = 6;
const MAX_LIST_LIMIT: usize = 200;
const MAX_RECALL_LIMIT: usize = 12;

#[derive(Debug, Clone, Default)]
pub struct MemoryFragmentListOptions<'a> {
    pub query: Option<&'a str>,
    pub category: Option<&'a str>,
    pub status: Option<&'a str>,
    pub pinned: Option<bool>,
    pub include_invalidated: bool,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Default)]
pub struct MemoryFragmentInput {
    pub memory_id: Option<String>,
    pub source_session_id: Option<String>,
    pub source_round_id: Option<String>,
    pub source_type: Option<String>,
    pub category: Option<String>,
    pub title_l0: Option<String>,
    pub summary_l1: Option<String>,
    pub content_l2: Option<String>,
    pub fact_key: Option<String>,
    pub tags: Option<Vec<String>>,
    pub entities: Option<Vec<String>>,
    pub importance: Option<f64>,
    pub confidence: Option<f64>,
    pub tier: Option<String>,
    pub status: Option<String>,
    pub pinned: Option<bool>,
    pub confirmed_by_user: Option<bool>,
    pub invalidated: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryRecallHit {
    pub fragment: MemoryFragmentRecord,
    pub reason_json: Value,
    pub lexical_score: f64,
    pub semantic_score: f64,
    pub freshness_score: f64,
    pub importance_score: f64,
    pub final_score: f64,
}

pub struct MemoryFragmentStore {
    storage: Arc<dyn StorageBackend>,
    legacy_store: MemoryStore,
}

impl MemoryFragmentStore {
    pub fn new(storage: Arc<dyn StorageBackend>) -> Self {
        let legacy_store = MemoryStore::new(storage.clone());
        Self {
            storage,
            legacy_store,
        }
    }

    pub fn list_fragments(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        options: MemoryFragmentListOptions<'_>,
    ) -> Vec<MemoryFragmentRecord> {
        let scope = normalize_agent_memory_scope(agent_id);
        self.ensure_legacy_migrated(user_id, &scope);
        let query = options.query.unwrap_or("").trim().to_lowercase();
        let category = normalize_slug(options.category).unwrap_or_default();
        let status = normalize_slug(options.status).unwrap_or_default();
        let mut items = self
            .storage
            .list_memory_fragments(user_id, &scope)
            .unwrap_or_default()
            .into_iter()
            .filter(|item| {
                if !options.include_invalidated
                    && item.invalidated_at.unwrap_or(0.0) > 0.0
                    && status.is_empty()
                {
                    return false;
                }
                if !category.is_empty() && item.category != category {
                    return false;
                }
                if !status.is_empty() && item.status != status {
                    return false;
                }
                if let Some(pinned) = options.pinned {
                    if item.pinned != pinned {
                        return false;
                    }
                }
                if query.is_empty() {
                    return true;
                }
                search_blob(item).contains(&query)
            })
            .collect::<Vec<_>>();
        items.sort_by(compare_fragment_records);
        if let Some(limit) = options.limit {
            items.truncate(limit.clamp(1, MAX_LIST_LIMIT));
        }
        items
    }

    pub fn get_fragment(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        memory_id: &str,
    ) -> Option<MemoryFragmentRecord> {
        let scope = normalize_agent_memory_scope(agent_id);
        self.ensure_legacy_migrated(user_id, &scope);
        self.storage
            .get_memory_fragment(user_id, &scope, memory_id.trim())
            .unwrap_or(None)
    }

    pub fn save_fragment(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        input: MemoryFragmentInput,
    ) -> Result<MemoryFragmentRecord> {
        let scope = normalize_agent_memory_scope(agent_id);
        self.ensure_legacy_migrated(user_id, &scope);
        let now = now_ts();
        let existing = input.memory_id.as_deref().and_then(|memory_id| {
            self.storage
                .get_memory_fragment(user_id, &scope, memory_id.trim())
                .ok()
                .flatten()
        });
        let memory_id = input
            .memory_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("memf_{}", Uuid::new_v4().simple()));
        let title = normalize_text(input.title_l0.as_deref(), 120)
            .or_else(|| existing.as_ref().map(|item| item.title_l0.clone()))
            .unwrap_or_default();
        let summary = normalize_text(input.summary_l1.as_deref(), 320)
            .or_else(|| existing.as_ref().map(|item| item.summary_l1.clone()))
            .unwrap_or_default();
        let content = normalize_text(input.content_l2.as_deref(), 2_000)
            .or_else(|| existing.as_ref().map(|item| item.content_l2.clone()))
            .unwrap_or_else(|| summary.clone());
        let title_l0 = if title.is_empty() {
            build_title(&summary, &content)
        } else {
            title
        };
        let summary_l1 = if summary.is_empty() {
            truncate_chars(&content, 320)
        } else {
            summary
        };
        let fact_key = normalize_text(input.fact_key.as_deref(), 80)
            .or_else(|| existing.as_ref().map(|item| item.fact_key.clone()))
            .unwrap_or_else(|| title_l0.clone());
        let invalidated = input.invalidated.unwrap_or(false);
        let mut record = existing.unwrap_or(MemoryFragmentRecord {
            memory_id,
            user_id: user_id.trim().to_string(),
            agent_id: scope.clone(),
            source_session_id: String::new(),
            source_round_id: String::new(),
            source_type: DEFAULT_SOURCE_TYPE.to_string(),
            category: DEFAULT_CATEGORY.to_string(),
            title_l0: String::new(),
            summary_l1: String::new(),
            content_l2: String::new(),
            fact_key: String::new(),
            tags: Vec::new(),
            entities: Vec::new(),
            importance: DEFAULT_IMPORTANCE,
            confidence: DEFAULT_CONFIDENCE,
            tier: DEFAULT_TIER.to_string(),
            status: STATUS_ACTIVE.to_string(),
            pinned: false,
            confirmed_by_user: false,
            access_count: 0,
            hit_count: 0,
            last_accessed_at: 0.0,
            valid_from: now,
            invalidated_at: None,
            supersedes_memory_id: None,
            superseded_by_memory_id: None,
            embedding_model: None,
            vector_ref: None,
            created_at: now,
            updated_at: now,
        });
        record.agent_id = scope;
        record.title_l0 = title_l0;
        record.summary_l1 = summary_l1;
        record.content_l2 = content;
        record.fact_key = fact_key;
        record.source_session_id =
            clean_string(input.source_session_id).unwrap_or(record.source_session_id);
        record.source_round_id =
            clean_string(input.source_round_id).unwrap_or(record.source_round_id);
        record.source_type = normalize_slug(input.source_type.as_deref())
            .unwrap_or_else(|| record.source_type.clone());
        record.category =
            normalize_slug(input.category.as_deref()).unwrap_or_else(|| record.category.clone());
        record.tags = normalize_string_list(input.tags.unwrap_or_else(|| record.tags.clone()));
        record.entities =
            normalize_string_list(input.entities.unwrap_or_else(|| record.entities.clone()));
        record.importance = clamp01(input.importance.unwrap_or(record.importance));
        record.confidence = clamp01(input.confidence.unwrap_or(record.confidence));
        record.tier = normalize_slug(input.tier.as_deref()).unwrap_or_else(|| record.tier.clone());
        record.status = if invalidated {
            STATUS_INVALIDATED.to_string()
        } else {
            normalize_slug(input.status.as_deref()).unwrap_or_else(|| record.status.clone())
        };
        record.pinned = input.pinned.unwrap_or(record.pinned);
        record.confirmed_by_user = input.confirmed_by_user.unwrap_or(record.confirmed_by_user);
        record.invalidated_at = if invalidated || record.status == STATUS_INVALIDATED {
            Some(now)
        } else {
            None
        };
        record.updated_at = now;
        self.storage.upsert_memory_fragment(&record)?;
        Ok(record)
    }

    pub fn set_confirmed(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        memory_id: &str,
        confirmed: bool,
    ) -> Result<Option<MemoryFragmentRecord>> {
        let Some(mut record) = self.get_fragment(user_id, agent_id, memory_id) else {
            return Ok(None);
        };
        record.confirmed_by_user = confirmed;
        record.updated_at = now_ts();
        self.storage.upsert_memory_fragment(&record)?;
        Ok(Some(record))
    }

    pub fn set_pinned(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        memory_id: &str,
        pinned: bool,
    ) -> Result<Option<MemoryFragmentRecord>> {
        let Some(mut record) = self.get_fragment(user_id, agent_id, memory_id) else {
            return Ok(None);
        };
        record.pinned = pinned;
        record.updated_at = now_ts();
        self.storage.upsert_memory_fragment(&record)?;
        Ok(Some(record))
    }

    pub fn set_invalidated(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        memory_id: &str,
        invalidated: bool,
    ) -> Result<Option<MemoryFragmentRecord>> {
        let Some(mut record) = self.get_fragment(user_id, agent_id, memory_id) else {
            return Ok(None);
        };
        record.status = if invalidated {
            STATUS_INVALIDATED.to_string()
        } else {
            STATUS_ACTIVE.to_string()
        };
        record.invalidated_at = if invalidated { Some(now_ts()) } else { None };
        record.updated_at = now_ts();
        self.storage.upsert_memory_fragment(&record)?;
        Ok(Some(record))
    }

    pub fn delete_fragment(&self, user_id: &str, agent_id: Option<&str>, memory_id: &str) -> bool {
        let scope = normalize_agent_memory_scope(agent_id);
        self.storage
            .delete_memory_fragment(user_id, &scope, memory_id.trim())
            .unwrap_or(0)
            > 0
    }

    pub fn list_hits(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        session_id: Option<&str>,
        limit: i64,
    ) -> Vec<MemoryHitRecord> {
        let scope = normalize_agent_memory_scope(agent_id);
        self.storage
            .list_memory_hits(
                user_id,
                &scope,
                session_id.map(str::trim).filter(|item| !item.is_empty()),
                limit.max(1),
            )
            .unwrap_or_default()
    }

    pub fn recall_for_prompt(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        session_id: Option<&str>,
        round_id: Option<&str>,
        query_text: Option<&str>,
        limit: Option<usize>,
    ) -> Vec<MemoryRecallHit> {
        let scope = normalize_agent_memory_scope(agent_id);
        self.ensure_legacy_migrated(user_id, &scope);
        let now = now_ts();
        let query = query_text.unwrap_or("").trim().to_lowercase();
        let tokens = tokenize(&query);
        let mut hits = self
            .storage
            .list_memory_fragments(user_id, &scope)
            .unwrap_or_default()
            .into_iter()
            .filter(|item| {
                item.status == STATUS_ACTIVE && item.invalidated_at.unwrap_or(0.0) <= 0.0
            })
            .filter_map(|item| build_recall_hit(item, &query, &tokens, now))
            .collect::<Vec<_>>();
        hits.sort_by(compare_recall_hits);
        hits.truncate(
            limit
                .unwrap_or(DEFAULT_RECALL_LIMIT)
                .clamp(1, MAX_RECALL_LIMIT),
        );
        if let Some(session_id) = session_id.map(str::trim).filter(|item| !item.is_empty()) {
            for hit in &mut hits {
                let mut fragment = hit.fragment.clone();
                fragment.access_count = fragment.access_count.saturating_add(1);
                fragment.hit_count = fragment.hit_count.saturating_add(1);
                fragment.last_accessed_at = now;
                let _ = self.storage.upsert_memory_fragment(&fragment);
                hit.fragment = fragment.clone();
                let _ = self.storage.insert_memory_hit(&MemoryHitRecord {
                    hit_id: format!("mhit_{}", Uuid::new_v4().simple()),
                    memory_id: fragment.memory_id.clone(),
                    user_id: user_id.to_string(),
                    agent_id: scope.clone(),
                    session_id: session_id.to_string(),
                    round_id: round_id.unwrap_or("").trim().to_string(),
                    query_text: query.clone(),
                    reason_json: hit.reason_json.clone(),
                    lexical_score: hit.lexical_score,
                    semantic_score: hit.semantic_score,
                    freshness_score: hit.freshness_score,
                    importance_score: hit.importance_score,
                    final_score: hit.final_score,
                    created_at: now,
                });
            }
        }
        hits
    }

    pub fn build_prompt_block(&self, hits: &[MemoryRecallHit]) -> String {
        if hits.is_empty() {
            return String::new();
        }
        let lines = hits
            .iter()
            .map(|hit| {
                let fragment = &hit.fragment;
                let mut flags = vec![fragment.category.clone()];
                if fragment.pinned {
                    flags.push("pinned".to_string());
                }
                if fragment.confirmed_by_user {
                    flags.push("confirmed".to_string());
                }
                let reason = hit
                    .reason_json
                    .get("matched_terms")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .take(4)
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .filter(|value| !value.is_empty());
                let mut line = format!(
                    "- [{}] {}: {}",
                    flags.join("|"),
                    truncate_chars(&fragment.title_l0, 72),
                    truncate_chars(&fragment.summary_l1, 180)
                );
                if let Some(reason) = reason {
                    line.push_str(&format!(" (matched: {reason})"));
                }
                line
            })
            .collect::<Vec<_>>();
        format!("{}\n{}", i18n::t("memory.block_prefix"), lines.join("\n"))
    }

    fn ensure_legacy_migrated(&self, user_id: &str, agent_scope: &str) {
        let owner_key = build_agent_memory_owner(user_id, Some(agent_scope));
        for record in self.legacy_store.list_records(&owner_key, Some(0), false) {
            let summary = MemoryStore::normalize_summary(&record.summary);
            if summary.is_empty() {
                continue;
            }
            let memory_id = format!("legacy::{}", record.session_id.trim());
            let existing = self
                .storage
                .get_memory_fragment(user_id, agent_scope, &memory_id)
                .unwrap_or(None);
            if let Some(current) = existing.as_ref() {
                if current.source_type != LEGACY_SOURCE_TYPE {
                    continue;
                }
            }
            let now = now_ts();
            let item = MemoryFragmentRecord {
                memory_id,
                user_id: user_id.to_string(),
                agent_id: agent_scope.to_string(),
                source_session_id: record.session_id.trim().to_string(),
                source_round_id: String::new(),
                source_type: LEGACY_SOURCE_TYPE.to_string(),
                category: existing
                    .as_ref()
                    .map(|value| value.category.clone())
                    .unwrap_or_else(|| DEFAULT_CATEGORY.to_string()),
                title_l0: build_title(&summary, &summary),
                summary_l1: truncate_chars(&summary, 320),
                content_l2: truncate_chars(&summary, 2_000),
                fact_key: format!("legacy:{}", record.session_id.trim()),
                tags: existing
                    .as_ref()
                    .map(|value| value.tags.clone())
                    .unwrap_or_else(|| tokenize(&summary).into_iter().take(8).collect()),
                entities: existing
                    .as_ref()
                    .map(|value| value.entities.clone())
                    .unwrap_or_default(),
                importance: existing
                    .as_ref()
                    .map(|value| value.importance)
                    .unwrap_or(DEFAULT_IMPORTANCE),
                confidence: existing
                    .as_ref()
                    .map(|value| value.confidence)
                    .unwrap_or(DEFAULT_CONFIDENCE),
                tier: existing
                    .as_ref()
                    .map(|value| value.tier.clone())
                    .unwrap_or_else(|| DEFAULT_TIER.to_string()),
                status: existing
                    .as_ref()
                    .map(|value| value.status.clone())
                    .unwrap_or_else(|| STATUS_ACTIVE.to_string()),
                pinned: existing.as_ref().map(|value| value.pinned).unwrap_or(false),
                confirmed_by_user: existing
                    .as_ref()
                    .map(|value| value.confirmed_by_user)
                    .unwrap_or(false),
                access_count: existing
                    .as_ref()
                    .map(|value| value.access_count)
                    .unwrap_or(0),
                hit_count: existing.as_ref().map(|value| value.hit_count).unwrap_or(0),
                last_accessed_at: existing
                    .as_ref()
                    .map(|value| value.last_accessed_at)
                    .unwrap_or(0.0),
                valid_from: if record.created_time > 0.0 {
                    record.created_time
                } else {
                    now
                },
                invalidated_at: existing.as_ref().and_then(|value| value.invalidated_at),
                supersedes_memory_id: existing
                    .as_ref()
                    .and_then(|value| value.supersedes_memory_id.clone()),
                superseded_by_memory_id: existing
                    .as_ref()
                    .and_then(|value| value.superseded_by_memory_id.clone()),
                embedding_model: existing
                    .as_ref()
                    .and_then(|value| value.embedding_model.clone()),
                vector_ref: existing.as_ref().and_then(|value| value.vector_ref.clone()),
                created_at: existing
                    .as_ref()
                    .map(|value| value.created_at)
                    .unwrap_or(now),
                updated_at: if record.updated_time > 0.0 {
                    record.updated_time
                } else {
                    now
                },
            };
            if let Err(err) = self.storage.upsert_memory_fragment(&item) {
                warn!("memory legacy migration failed for {user_id}/{agent_scope}: {err}");
            }
        }
    }
}

fn build_recall_hit(
    fragment: MemoryFragmentRecord,
    query: &str,
    tokens: &[String],
    now: f64,
) -> Option<MemoryRecallHit> {
    let matched_terms = tokens
        .iter()
        .filter(|token| search_blob(&fragment).contains(token.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !query.is_empty()
        && matched_terms.is_empty()
        && !fragment.pinned
        && !fragment.confirmed_by_user
    {
        return None;
    }
    let lexical_score = if query.is_empty() {
        (if fragment.pinned { 0.4 } else { 0.08 })
            + if fragment.confirmed_by_user {
                0.16
            } else {
                0.0
            }
    } else {
        let coverage = matched_terms.len() as f64 / tokens.len().max(1) as f64;
        let exact = if search_blob(&fragment).contains(query) {
            0.25
        } else {
            0.0
        };
        clamp01(coverage * 0.72 + exact + if fragment.pinned { 0.08 } else { 0.0 })
    };
    let freshness_score = {
        let updated = if fragment.updated_at > 0.0 {
            fragment.updated_at
        } else {
            fragment.created_at
        };
        if updated <= 0.0 || now <= updated {
            1.0
        } else {
            clamp01(1.0 / (1.0 + (now - updated) / 86_400.0 / 14.0))
        }
    };
    let importance_score = clamp01(
        fragment.importance * 0.58
            + fragment.confidence * 0.42
            + if fragment.pinned { 0.1 } else { 0.0 }
            + if fragment.confirmed_by_user {
                0.08
            } else {
                0.0
            },
    );
    let final_score = clamp01(
        lexical_score * if query.is_empty() { 0.18 } else { 0.56 }
            + freshness_score * 0.18
            + importance_score * 0.26,
    );
    Some(MemoryRecallHit {
        fragment,
        reason_json: json!({
            "match_kind": if query.is_empty() { "recent" } else { "keyword" },
            "matched_terms": matched_terms,
        }),
        lexical_score,
        semantic_score: 0.0,
        freshness_score,
        importance_score,
        final_score,
    })
}

fn compare_fragment_records(left: &MemoryFragmentRecord, right: &MemoryFragmentRecord) -> Ordering {
    right
        .pinned
        .cmp(&left.pinned)
        .then_with(|| right.confirmed_by_user.cmp(&left.confirmed_by_user))
        .then_with(|| {
            right
                .updated_at
                .partial_cmp(&left.updated_at)
                .unwrap_or(Ordering::Equal)
        })
}

fn compare_recall_hits(left: &MemoryRecallHit, right: &MemoryRecallHit) -> Ordering {
    right
        .final_score
        .partial_cmp(&left.final_score)
        .unwrap_or(Ordering::Equal)
        .then_with(|| right.fragment.pinned.cmp(&left.fragment.pinned))
        .then_with(|| {
            right
                .fragment
                .updated_at
                .partial_cmp(&left.fragment.updated_at)
                .unwrap_or(Ordering::Equal)
        })
}

fn search_blob(fragment: &MemoryFragmentRecord) -> String {
    format!(
        "{} {} {} {} {} {}",
        fragment.title_l0,
        fragment.summary_l1,
        fragment.content_l2,
        fragment.fact_key,
        fragment.tags.join(" "),
        fragment.entities.join(" "),
    )
    .to_lowercase()
}

fn tokenize(text: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    for raw in text
        .split(|ch: char| !(ch.is_alphanumeric() || ch == '-' || ch == '_'))
        .filter(|item| !item.trim().is_empty())
    {
        let value = raw.trim().to_lowercase();
        if seen.insert(value.clone()) {
            items.push(value);
        }
    }
    items
}

fn normalize_string_list(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    for value in values {
        let cleaned = value.trim().to_string();
        if cleaned.is_empty() || !seen.insert(cleaned.clone()) {
            continue;
        }
        items.push(cleaned);
    }
    items
}

fn normalize_slug(value: Option<&str>) -> Option<String> {
    let raw = value?.trim().to_lowercase();
    let mut out = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else if matches!(ch, '-' | '_' | ' ') {
            out.push('-');
        }
    }
    let normalized = out.trim_matches('-').to_string();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn normalize_text(value: Option<&str>, limit: usize) -> Option<String> {
    let cleaned = value?.trim();
    if cleaned.is_empty() {
        return None;
    }
    Some(truncate_chars(cleaned, limit))
}

fn clean_string(value: Option<String>) -> Option<String> {
    value
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
}

fn build_title(summary: &str, content: &str) -> String {
    let seed = if !summary.trim().is_empty() {
        summary
    } else {
        content
    };
    let mut title = String::new();
    for ch in seed.trim().chars() {
        title.push(ch);
        if matches!(ch, '。' | '.' | '！' | '!' | '？' | '?' | '\n') || title.chars().count() >= 40
        {
            break;
        }
    }
    truncate_chars(&title, 60)
}

fn truncate_chars(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= limit {
        return trimmed.to_string();
    }
    let mut output = String::new();
    for (index, ch) in trimmed.chars().enumerate() {
        if index >= limit {
            output.push('…');
            break;
        }
        output.push(ch);
    }
    output
}

fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}

fn clamp01(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_supports_mixed_text() {
        let items = tokenize("Rust 开发 Vue3 memory");
        assert!(items.contains(&"rust".to_string()));
        assert!(items.contains(&"开发".to_string()));
        assert!(items.contains(&"vue3".to_string()));
    }

    #[test]
    fn search_blob_contains_tags() {
        let fragment = MemoryFragmentRecord {
            memory_id: "m1".to_string(),
            user_id: "u1".to_string(),
            agent_id: "a1".to_string(),
            source_session_id: String::new(),
            source_round_id: String::new(),
            source_type: DEFAULT_SOURCE_TYPE.to_string(),
            category: DEFAULT_CATEGORY.to_string(),
            title_l0: "title".to_string(),
            summary_l1: "summary".to_string(),
            content_l2: "content".to_string(),
            fact_key: "fact".to_string(),
            tags: vec!["rust".to_string()],
            entities: vec![],
            importance: 0.6,
            confidence: 0.7,
            tier: DEFAULT_TIER.to_string(),
            status: STATUS_ACTIVE.to_string(),
            pinned: false,
            confirmed_by_user: false,
            access_count: 0,
            hit_count: 0,
            last_accessed_at: 0.0,
            valid_from: 0.0,
            invalidated_at: None,
            supersedes_memory_id: None,
            superseded_by_memory_id: None,
            embedding_model: None,
            vector_ref: None,
            created_at: 0.0,
            updated_at: 0.0,
        };
        assert!(search_blob(&fragment).contains("rust"));
    }
}
