use crate::config::{Config, LlmModelConfig};
use crate::i18n;
use crate::services::llm::{embed_texts, is_embedding_configured, is_embedding_model};
use crate::services::memory::{
    build_agent_memory_owner, normalize_agent_memory_scope, MemoryStore,
};
use crate::storage::{
    MemoryFragmentEmbeddingRecord, MemoryFragmentRecord, MemoryHitRecord, StorageBackend,
};
use anyhow::Result;
use chrono::{Local, TimeZone, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;
use tracing::warn;
use uuid::Uuid;

const DEFAULT_CATEGORY: &str = "session_summary";
const DEFAULT_SOURCE_TYPE: &str = "manual";
const LEGACY_SOURCE_TYPE: &str = "legacy_summary";
const TIER_CORE: &str = "core";
const TIER_WORKING: &str = "working";
const TIER_PERIPHERAL: &str = "peripheral";
const DEFAULT_TIER: &str = TIER_WORKING;
const STATUS_ACTIVE: &str = "active";
const STATUS_SUPERSEDED: &str = "superseded";
const STATUS_INVALIDATED: &str = "invalidated";
const DEFAULT_IMPORTANCE: f64 = 0.6;
const DEFAULT_CONFIDENCE: f64 = 0.7;
const DEFAULT_RECALL_LIMIT: usize = 6;
const MAX_LIST_LIMIT: usize = 200;
const MAX_RECALL_LIMIT: usize = 30;
const MEMORY_SEMANTIC_RECALL_ENABLED: bool = false;
const MAX_REASON_TERMS: usize = 8;
const MAX_REASON_FIELDS: usize = 6;
const SEMANTIC_RECALL_CANDIDATE_LIMIT: usize = 24;
const SEMANTIC_RECALL_KEEP_LIMIT: usize = 8;
const SEMANTIC_RECALL_MIN_SCORE: f64 = 0.42;
const SEMANTIC_RECALL_TIMEOUT_S: u64 = 20;
const LIFECYCLE_CORE_ACCESS_THRESHOLD: i64 = 10;
const LIFECYCLE_CORE_HIT_THRESHOLD: i64 = 4;
const LIFECYCLE_WORKING_ACCESS_THRESHOLD: i64 = 3;
const LIFECYCLE_WORKING_HIT_THRESHOLD: i64 = 1;
const LIFECYCLE_CORE_SCORE_THRESHOLD: f64 = 0.72;
const LIFECYCLE_WORKING_SCORE_THRESHOLD: f64 = 0.38;
const LIFECYCLE_PERIPHERAL_SCORE_THRESHOLD: f64 = 0.18;
const LIFECYCLE_PERIPHERAL_AGE_DAYS: f64 = 60.0;
const SENTENCE_STOP_CHARS: [char; 9] = ['。', '.', '，', ',', '！', '!', '？', '?', '\n'];
const ELLIPSIS: char = '…';

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

#[derive(Debug, Clone, Default)]
pub struct PromptMemoryRecall {
    pub hits: Vec<MemoryRecallHit>,
    pub total_available: usize,
}

#[derive(Debug, Clone)]
enum MemoryHitEventKey {
    Round {
        session_id: String,
        round_id: String,
    },
    Query {
        session_id: String,
        query_text: String,
    },
}

impl MemoryHitEventKey {
    fn build(session_id: Option<&str>, round_id: Option<&str>, query_text: &str) -> Option<Self> {
        let cleaned_session = session_id
            .map(str::trim)
            .filter(|value| !value.is_empty())?;
        if let Some(cleaned_round) = round_id.map(str::trim).filter(|value| !value.is_empty()) {
            return Some(Self::Round {
                session_id: cleaned_session.to_string(),
                round_id: cleaned_round.to_string(),
            });
        }
        let cleaned_query = query_text.trim();
        if cleaned_query.is_empty() {
            return None;
        }
        Some(Self::Query {
            session_id: cleaned_session.to_string(),
            query_text: cleaned_query.to_string(),
        })
    }

    fn session_id(&self) -> &str {
        match self {
            Self::Round { session_id, .. } | Self::Query { session_id, .. } => session_id,
        }
    }

    fn round_id(&self) -> Option<&str> {
        match self {
            Self::Round { round_id, .. } => Some(round_id.as_str()),
            Self::Query { .. } => None,
        }
    }

    fn query_text(&self) -> Option<&str> {
        match self {
            Self::Round { .. } => None,
            Self::Query { query_text, .. } => Some(query_text.as_str()),
        }
    }
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
        let now = now_ts();
        let raw_items = self
            .storage
            .list_memory_fragments(user_id, &scope)
            .unwrap_or_default();
        let mut items = raw_items
            .into_iter()
            .filter_map(|mut item| {
                if refresh_fragment_lifecycle_record(&mut item, now) {
                    let _ = self.storage.upsert_memory_fragment(&item);
                }
                if !options.include_invalidated
                    && item.invalidated_at.unwrap_or(0.0) > 0.0
                    && status.is_empty()
                {
                    return None;
                }
                if !category.is_empty() && item.category != category {
                    return None;
                }
                if !status.is_empty() && item.status != status {
                    return None;
                }
                if let Some(pinned) = options.pinned {
                    if item.pinned != pinned {
                        return None;
                    }
                }
                if query.is_empty() {
                    return Some(item);
                }
                search_blob(&item).contains(&query).then_some(item)
            })
            .collect::<Vec<_>>();
        self.sync_fragment_hit_counts(user_id, &scope, &mut items);
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
        let mut item = self
            .storage
            .get_memory_fragment(user_id, &scope, memory_id.trim())
            .unwrap_or(None)?;
        if refresh_fragment_lifecycle_record(&mut item, now_ts()) {
            let _ = self.storage.upsert_memory_fragment(&item);
        }
        self.sync_fragment_hit_count(user_id, &scope, &mut item);
        Some(item)
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
        let existing = input
            .memory_id
            .as_deref()
            .and_then(|memory_id| self.get_fragment(user_id, Some(&scope), memory_id.trim()));
        let is_create = existing.is_none();
        let requested_tier = input
            .tier
            .as_deref()
            .map(|value| normalize_fragment_tier(Some(value)));
        let requested_status = input
            .status
            .as_deref()
            .map(|value| normalize_fragment_status(Some(value)));
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
        record.tier = requested_tier.unwrap_or_else(|| normalize_fragment_tier(Some(&record.tier)));
        record.status = if invalidated {
            STATUS_INVALIDATED.to_string()
        } else {
            requested_status.unwrap_or_else(|| normalize_fragment_status(Some(&record.status)))
        };
        record.pinned = input.pinned.unwrap_or(record.pinned);
        record.confirmed_by_user = input.confirmed_by_user.unwrap_or(record.confirmed_by_user);
        record.invalidated_at = if invalidated || record.status == STATUS_INVALIDATED {
            Some(now)
        } else {
            None
        };
        if is_create {
            let supersede_targets = self.find_supersede_targets(user_id, &record.agent_id, &record);
            if let Some(target) = supersede_targets.first() {
                record.supersedes_memory_id = Some(target.memory_id.clone());
                record.superseded_by_memory_id = None;
            }
            refresh_fragment_lifecycle_record(&mut record, now);
            record.updated_at = now;
            self.storage.upsert_memory_fragment(&record)?;
            self.mark_fragment_superseded(&record, supersede_targets, now)?;
            return Ok(record);
        }
        refresh_fragment_lifecycle_record(&mut record, now);
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
        let now = now_ts();
        refresh_fragment_lifecycle_record(&mut record, now);
        record.updated_at = now;
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
        let now = now_ts();
        refresh_fragment_lifecycle_record(&mut record, now);
        record.updated_at = now;
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
        let now = now_ts();
        record.status = if invalidated {
            STATUS_INVALIDATED.to_string()
        } else {
            STATUS_ACTIVE.to_string()
        };
        record.invalidated_at = if invalidated { Some(now) } else { None };
        refresh_fragment_lifecycle_record(&mut record, now);
        record.updated_at = now;
        self.storage.upsert_memory_fragment(&record)?;
        Ok(Some(record))
    }

    pub fn delete_fragment(&self, user_id: &str, agent_id: Option<&str>, memory_id: &str) -> bool {
        let scope = normalize_agent_memory_scope(agent_id);
        let cleaned_memory_id = memory_id.trim();
        if cleaned_memory_id.is_empty() {
            return false;
        }
        self.repair_fragment_links_after_delete(user_id, &scope, cleaned_memory_id);
        self.storage
            .delete_memory_fragment(user_id, &scope, memory_id.trim())
            .unwrap_or(0)
            > 0
    }

    fn find_supersede_targets(
        &self,
        user_id: &str,
        agent_scope: &str,
        incoming: &MemoryFragmentRecord,
    ) -> Vec<MemoryFragmentRecord> {
        let incoming_fact_key = incoming.fact_key.trim().to_lowercase();
        if incoming_fact_key.is_empty() || incoming.status == STATUS_INVALIDATED {
            return Vec::new();
        }
        let incoming_signature = fragment_material_signature(incoming);
        let mut candidates = self
            .storage
            .list_memory_fragments(user_id, agent_scope)
            .unwrap_or_default()
            .into_iter()
            .filter(|item| item.memory_id != incoming.memory_id)
            .filter(|item| item.fact_key.trim().to_lowercase() == incoming_fact_key)
            .filter(|item| {
                item.status == STATUS_ACTIVE && item.invalidated_at.unwrap_or(0.0) <= 0.0
            })
            .filter(|item| !is_superseded_fragment(item))
            .filter(|item| fragment_material_signature(item) != incoming_signature)
            .collect::<Vec<_>>();
        candidates.sort_by(compare_fragment_records);
        candidates
    }

    fn mark_fragment_superseded(
        &self,
        current: &MemoryFragmentRecord,
        targets: Vec<MemoryFragmentRecord>,
        now: f64,
    ) -> Result<()> {
        for mut target in targets {
            target.status = STATUS_SUPERSEDED.to_string();
            target.superseded_by_memory_id = Some(current.memory_id.clone());
            target.invalidated_at = None;
            target.updated_at = now;
            self.storage.upsert_memory_fragment(&target)?;
        }
        Ok(())
    }

    fn repair_fragment_links_after_delete(
        &self,
        user_id: &str,
        agent_scope: &str,
        memory_id: &str,
    ) {
        let now = now_ts();
        let fragments = self
            .storage
            .list_memory_fragments(user_id, agent_scope)
            .unwrap_or_default();
        for mut fragment in fragments {
            let mut changed = false;
            if fragment.supersedes_memory_id.as_deref() == Some(memory_id) {
                fragment.supersedes_memory_id = None;
                changed = true;
            }
            if fragment.superseded_by_memory_id.as_deref() == Some(memory_id) {
                fragment.superseded_by_memory_id = None;
                changed = true;
            }
            if !changed {
                continue;
            }
            refresh_fragment_lifecycle_record(&mut fragment, now);
            fragment.updated_at = now;
            let _ = self.storage.upsert_memory_fragment(&fragment);
        }
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

    fn sync_fragment_hit_count(
        &self,
        user_id: &str,
        agent_scope: &str,
        fragment: &mut MemoryFragmentRecord,
    ) {
        let hit_counts = self
            .storage
            .list_memory_hit_counts(user_id, agent_scope)
            .unwrap_or_default();
        let previous_hit_count = fragment.hit_count;
        sync_fragment_hit_count_from_map(fragment, &hit_counts);
        if fragment.hit_count != previous_hit_count {
            let _ = self.storage.upsert_memory_fragment(fragment);
        }
    }

    fn sync_fragment_hit_counts(
        &self,
        user_id: &str,
        agent_scope: &str,
        fragments: &mut [MemoryFragmentRecord],
    ) {
        let hit_counts = self
            .storage
            .list_memory_hit_counts(user_id, agent_scope)
            .unwrap_or_default();
        for fragment in fragments {
            let previous_hit_count = fragment.hit_count;
            sync_fragment_hit_count_from_map(fragment, &hit_counts);
            if fragment.hit_count != previous_hit_count {
                let _ = self.storage.upsert_memory_fragment(fragment);
            }
        }
    }

    pub async fn recall_for_prompt(
        &self,
        config: Option<&Config>,
        user_id: &str,
        agent_id: Option<&str>,
        session_id: Option<&str>,
        round_id: Option<&str>,
        query_text: Option<&str>,
        limit: Option<usize>,
    ) -> Vec<MemoryRecallHit> {
        self.recall_for_prompt_inventory(
            config, user_id, agent_id, session_id, round_id, query_text, limit,
        )
        .await
        .hits
    }

    pub async fn recall_for_prompt_inventory(
        &self,
        config: Option<&Config>,
        user_id: &str,
        agent_id: Option<&str>,
        session_id: Option<&str>,
        round_id: Option<&str>,
        query_text: Option<&str>,
        limit: Option<usize>,
    ) -> PromptMemoryRecall {
        let scope = normalize_agent_memory_scope(agent_id);
        self.ensure_legacy_migrated(user_id, &scope);
        let now = now_ts();
        let query = query_text.unwrap_or("").trim().to_lowercase();
        let tokens = tokenize(&query);
        let fragments = self.load_active_fragments_for_prompt(user_id, &scope, now);
        let total_available = fragments.len();
        let mut hits = fragments
            .iter()
            .cloned()
            .filter_map(|item| build_recall_hit(item, &query, &tokens, now))
            .collect::<Vec<_>>();
        hits.sort_by(compare_recall_hits);
        hits = dedupe_recall_hits(hits);
        if MEMORY_SEMANTIC_RECALL_ENABLED && !query.is_empty() {
            if let Some(config) = config {
                hits = self
                    .apply_semantic_recall(config, &query, &fragments, hits, now)
                    .await;
            }
        }
        hits.sort_by(compare_recall_hits);
        hits = dedupe_recall_hits(hits);
        hits.truncate(
            limit
                .unwrap_or(DEFAULT_RECALL_LIMIT)
                .clamp(1, MAX_RECALL_LIMIT),
        );
        if let Some(recall_event) = MemoryHitEventKey::build(session_id, round_id, &query) {
            for hit in &mut hits {
                let already_recorded = self
                    .storage
                    .has_memory_hit_event(
                        user_id,
                        &scope,
                        &hit.fragment.memory_id,
                        recall_event.session_id(),
                        recall_event.round_id(),
                        recall_event.query_text(),
                    )
                    .unwrap_or(false);
                if already_recorded {
                    continue;
                }
                let mut fragment = hit.fragment.clone();
                fragment.access_count = fragment.access_count.saturating_add(1);
                fragment.hit_count = fragment.hit_count.saturating_add(1);
                fragment.last_accessed_at = now;
                refresh_fragment_lifecycle_record(&mut fragment, now);
                let _ = self.storage.upsert_memory_fragment(&fragment);
                hit.fragment = fragment.clone();
                let _ = self.storage.insert_memory_hit(&MemoryHitRecord {
                    hit_id: format!("mhit_{}", Uuid::new_v4().simple()),
                    memory_id: fragment.memory_id.clone(),
                    user_id: user_id.to_string(),
                    agent_id: scope.clone(),
                    session_id: recall_event.session_id().to_string(),
                    round_id: recall_event.round_id().unwrap_or("").to_string(),
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
        PromptMemoryRecall {
            hits,
            total_available,
        }
    }

    async fn apply_semantic_recall(
        &self,
        config: &Config,
        query: &str,
        fragments: &[MemoryFragmentRecord],
        lexical_hits: Vec<MemoryRecallHit>,
        now: f64,
    ) -> Vec<MemoryRecallHit> {
        let Some((embedding_name, embed_config)) = resolve_memory_embedding_model(config) else {
            return lexical_hits;
        };
        let candidates = select_semantic_candidates(fragments, &lexical_hits);
        if candidates.is_empty() {
            return lexical_hits;
        }

        let query_vector = match embed_texts(
            &embed_config,
            &[query.to_string()],
            SEMANTIC_RECALL_TIMEOUT_S,
        )
        .await
        {
            Ok(vectors) if vectors.len() == 1 => vectors.into_iter().next().unwrap_or_default(),
            Ok(_) => return lexical_hits,
            Err(err) => {
                warn!("memory semantic recall embedding failed: {err}");
                return lexical_hits;
            }
        };
        let candidate_vectors = self
            .load_or_create_memory_embeddings(&embedding_name, &embed_config, &candidates)
            .await;
        if candidate_vectors.is_empty() {
            return lexical_hits;
        }
        let mut hit_map = lexical_hits
            .into_iter()
            .map(|hit| (hit.fragment.memory_id.clone(), hit))
            .collect::<HashMap<_, _>>();

        for (fragment, vector) in candidate_vectors {
            let semantic_score = cosine_similarity(&query_vector, &vector);
            let has_existing = hit_map.contains_key(&fragment.memory_id);
            if semantic_score < SEMANTIC_RECALL_MIN_SCORE && !has_existing {
                continue;
            }
            if let Some(existing) = hit_map.get_mut(&fragment.memory_id) {
                merge_semantic_score(existing, semantic_score, &embedding_name);
                continue;
            }
            let hit = build_semantic_only_hit(fragment, semantic_score, &embedding_name, now);
            hit_map.insert(hit.fragment.memory_id.clone(), hit);
        }

        let mut hits = hit_map.into_values().collect::<Vec<_>>();
        hits.sort_by(compare_recall_hits);
        hits
    }

    async fn load_or_create_memory_embeddings(
        &self,
        embedding_name: &str,
        embed_config: &LlmModelConfig,
        fragments: &[MemoryFragmentRecord],
    ) -> Vec<(MemoryFragmentRecord, Vec<f32>)> {
        let mut cached = Vec::new();
        let mut missing_fragments = Vec::new();
        let mut missing_inputs = Vec::new();

        for fragment in fragments {
            let embedding_text = build_semantic_recall_text(fragment);
            let content_hash = hash_text(&embedding_text);
            let cached_vector = self
                .storage
                .get_memory_fragment_embedding(
                    &fragment.user_id,
                    &fragment.agent_id,
                    &fragment.memory_id,
                    embedding_name,
                    &content_hash,
                )
                .ok()
                .flatten()
                .map(|record| record.vector)
                .filter(|vector| !vector.is_empty());
            if let Some(vector) = cached_vector {
                cached.push((fragment.clone(), vector));
                continue;
            }
            missing_fragments.push((fragment.clone(), content_hash));
            missing_inputs.push(embedding_text);
        }

        if missing_inputs.is_empty() {
            return cached;
        }
        let fresh_vectors =
            match embed_texts(embed_config, &missing_inputs, SEMANTIC_RECALL_TIMEOUT_S).await {
                Ok(vectors) if vectors.len() == missing_fragments.len() => vectors,
                Ok(_) => return cached,
                Err(err) => {
                    warn!("memory fragment embedding cache fill failed: {err}");
                    return cached;
                }
            };

        for ((fragment, content_hash), vector) in
            missing_fragments.into_iter().zip(fresh_vectors.into_iter())
        {
            if vector.is_empty() {
                continue;
            }
            let record = MemoryFragmentEmbeddingRecord {
                memory_id: fragment.memory_id.clone(),
                user_id: fragment.user_id.clone(),
                agent_id: fragment.agent_id.clone(),
                embedding_model: embedding_name.to_string(),
                content_hash: content_hash.clone(),
                dimensions: vector.len() as i64,
                vector: vector.clone(),
                updated_at: now_ts(),
            };
            if let Err(err) = self.storage.upsert_memory_fragment_embedding(&record) {
                warn!("memory fragment embedding cache upsert failed: {err}");
            }
            cached.push((fragment, vector));
        }
        cached
    }

    pub fn build_prompt_block(&self, hits: &[MemoryRecallHit], total_available: usize) -> String {
        if hits.is_empty() && total_available == 0 {
            return String::new();
        }
        let injected_count = hits.len();
        let mut meta_lines = Vec::with_capacity(2);
        let meta = i18n::t_with_params(
            "memory.prompt_meta.summary",
            &HashMap::from([
                ("total".to_string(), total_available.to_string()),
                ("injected".to_string(), injected_count.to_string()),
                ("limit".to_string(), MAX_RECALL_LIMIT.to_string()),
            ]),
        );
        if !meta.trim().is_empty() {
            meta_lines.push(meta);
        }
        if total_available > injected_count || injected_count == 0 {
            meta_lines.push(i18n::t("memory.prompt_meta.more_hint"));
        }
        let lines = hits
            .iter()
            .map(|hit| {
                let fragment = &hit.fragment;
                let timestamp = format_prompt_memory_timestamp(fragment.updated_at);
                let content = build_prompt_memory_text(fragment);
                if fragment.pinned {
                    format!("- [{timestamp}] 【置顶】{content}")
                } else {
                    format!("- [{timestamp}] {content}")
                }
            })
            .collect::<Vec<_>>();
        meta_lines.extend(lines);
        let lines = meta_lines;
        format!("{}\n{}", i18n::t("memory.block_prefix"), lines.join("\n"))
    }

    fn load_active_fragments_for_prompt(
        &self,
        user_id: &str,
        agent_scope: &str,
        now: f64,
    ) -> Vec<MemoryFragmentRecord> {
        let mut items = self
            .storage
            .list_memory_fragments(user_id, agent_scope)
            .unwrap_or_default()
            .into_iter()
            .map(|mut item| {
                if refresh_fragment_lifecycle_record(&mut item, now) {
                    let _ = self.storage.upsert_memory_fragment(&item);
                }
                item
            })
            .filter(|item| {
                item.status == STATUS_ACTIVE && item.invalidated_at.unwrap_or(0.0) <= 0.0
            })
            .filter(|item| !is_superseded_fragment(item))
            .collect::<Vec<_>>();
        self.sync_fragment_hit_counts(user_id, agent_scope, &mut items);
        items
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
    let title_text = fragment.title_l0.to_lowercase();
    let summary_text = fragment.summary_l1.to_lowercase();
    let content_text = fragment.content_l2.to_lowercase();
    let fact_key_text = fragment.fact_key.to_lowercase();
    let category_text = fragment.category.to_lowercase();
    let tag_text = fragment.tags.join(" ").to_lowercase();
    let entity_text = fragment.entities.join(" ").to_lowercase();

    let title_terms = matched_terms_for_field(tokens, &title_text);
    let summary_terms = matched_terms_for_field(tokens, &summary_text);
    let content_terms = matched_terms_for_field(tokens, &content_text);
    let fact_key_terms = matched_terms_for_field(tokens, &fact_key_text);
    let tag_terms = matched_terms_for_field(tokens, &tag_text);
    let entity_terms = matched_terms_for_field(tokens, &entity_text);

    let phrase_fields = [
        (!query.is_empty() && title_text.contains(query)).then_some("title"),
        (!query.is_empty() && summary_text.contains(query)).then_some("summary"),
        (!query.is_empty() && content_text.contains(query)).then_some("content"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();

    let category_match = !query.is_empty()
        && (category_text == query
            || tokens
                .iter()
                .any(|token| !token.is_empty() && category_text.contains(token.as_str())));
    let fact_key_match =
        !query.is_empty() && (!fact_key_terms.is_empty() || fact_key_text.contains(query));

    let matched_terms = merge_matched_terms(&[
        &title_terms,
        &summary_terms,
        &content_terms,
        &fact_key_terms,
        &tag_terms,
        &entity_terms,
    ]);
    let matched_fields = collect_matched_fields(
        &title_terms,
        &summary_terms,
        &content_terms,
        &fact_key_terms,
        &tag_terms,
        &entity_terms,
        category_match,
        &phrase_fields,
    );
    let has_query_match =
        !matched_terms.is_empty() || !matched_fields.is_empty() || category_match || fact_key_match;
    if !query.is_empty() && !has_query_match && !fragment.pinned {
        return None;
    }

    let lexical_score = if query.is_empty() {
        clamp01(if fragment.pinned { 0.42 } else { 0.08 })
    } else {
        let token_total = tokens.len().max(1) as f64;
        let overall_coverage = matched_terms.len() as f64 / token_total;
        let title_coverage = title_terms.len() as f64 / token_total;
        let summary_coverage = summary_terms.len() as f64 / token_total;
        let content_coverage = content_terms.len() as f64 / token_total;
        let tag_coverage = tag_terms.len() as f64 / token_total;
        let entity_coverage = entity_terms.len() as f64 / token_total;
        let fact_key_coverage = fact_key_terms.len() as f64 / token_total;
        let phrase_bonus = if phrase_fields.is_empty() {
            0.0
        } else if phrase_fields.contains(&"title") {
            0.24
        } else if phrase_fields.contains(&"summary") {
            0.18
        } else {
            0.12
        };
        clamp01(
            overall_coverage * 0.16
                + title_coverage * 0.28
                + summary_coverage * 0.18
                + content_coverage * 0.08
                + tag_coverage * 0.14
                + entity_coverage * 0.12
                + fact_key_coverage * 0.08
                + if category_match { 0.08 } else { 0.0 }
                + phrase_bonus
                + if fragment.pinned { 0.04 } else { 0.0 },
        )
    };
    let freshness_score = compute_freshness_score(&fragment, now);
    let importance_score = compute_importance_score(&fragment);
    let final_score = combine_recall_scores(
        query.is_empty(),
        lexical_score,
        0.0,
        freshness_score,
        importance_score,
    );
    let reason_json = json!({
        "match_kind": if query.is_empty() { "recent" } else { "keyword" },
        "matched_terms": matched_terms.into_iter().take(MAX_REASON_TERMS).collect::<Vec<_>>(),
        "matched_fields": matched_fields.into_iter().take(MAX_REASON_FIELDS).collect::<Vec<_>>(),
        "phrase_match": !phrase_fields.is_empty(),
        "phrase_fields": phrase_fields,
        "category_match": category_match,
        "fact_key_match": fact_key_match,
        "pinned": fragment.pinned,
        "tier": fragment.tier.clone(),
        "access_count": fragment.access_count,
        "hit_count": fragment.hit_count,
    });
    Some(MemoryRecallHit {
        fragment,
        reason_json,
        lexical_score,
        semantic_score: 0.0,
        freshness_score,
        importance_score,
        final_score,
    })
}

fn build_prompt_memory_text(fragment: &MemoryFragmentRecord) -> String {
    let title = fragment.title_l0.trim();
    let summary = fragment.summary_l1.trim();
    let content = fragment.content_l2.trim();

    let combined_title_summary = if !title.is_empty()
        && !summary.is_empty()
        && title != summary
        && !summary.contains(title)
        && !title.contains(summary)
    {
        format!("{title}：{summary}")
    } else {
        String::new()
    };

    let primary = if !content.is_empty() && content.chars().count() <= 120 {
        content.to_string()
    } else if !combined_title_summary.is_empty() {
        combined_title_summary
    } else if !summary.is_empty() {
        summary.to_string()
    } else if !content.is_empty() {
        content.to_string()
    } else if !title.is_empty() {
        title.to_string()
    } else {
        "未命名记忆".to_string()
    };

    truncate_chars(&primary, 180)
}

fn format_prompt_memory_timestamp(value: f64) -> String {
    let seconds = value.floor() as i64;
    if seconds <= 0 {
        return "unknown time".to_string();
    }
    Local
        .timestamp_opt(seconds, 0)
        .single()
        .map(|datetime| datetime.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "unknown time".to_string())
}

fn sync_fragment_hit_count_from_map(
    fragment: &mut MemoryFragmentRecord,
    hit_counts: &HashMap<String, i64>,
) {
    fragment.hit_count = hit_counts
        .get(&fragment.memory_id)
        .copied()
        .unwrap_or(0)
        .max(0);
}

fn compare_fragment_records(left: &MemoryFragmentRecord, right: &MemoryFragmentRecord) -> Ordering {
    right
        .hit_count
        .cmp(&left.hit_count)
        .then_with(|| right.pinned.cmp(&left.pinned))
        .then_with(|| fragment_status_rank(&right.status).cmp(&fragment_status_rank(&left.status)))
        .then_with(|| fragment_tier_rank(&right.tier).cmp(&fragment_tier_rank(&left.tier)))
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

fn dedupe_recall_hits(hits: Vec<MemoryRecallHit>) -> Vec<MemoryRecallHit> {
    let mut seen = HashSet::new();
    let mut output = Vec::with_capacity(hits.len());
    for hit in hits {
        let key = recall_dedupe_key(&hit.fragment);
        if seen.insert(key) {
            output.push(hit);
        }
    }
    output
}

fn select_semantic_candidates(
    fragments: &[MemoryFragmentRecord],
    lexical_hits: &[MemoryRecallHit],
) -> Vec<MemoryFragmentRecord> {
    let lexical_ids = lexical_hits
        .iter()
        .take(SEMANTIC_RECALL_KEEP_LIMIT)
        .map(|hit| hit.fragment.memory_id.clone())
        .collect::<HashSet<_>>();
    let mut ranked = fragments.to_vec();
    ranked.sort_by(compare_fragment_records);
    let mut seen = HashSet::new();
    let mut selected = Vec::new();

    for fragment in ranked {
        let should_include = lexical_ids.contains(&fragment.memory_id)
            || fragment.pinned
            || selected.len() < SEMANTIC_RECALL_CANDIDATE_LIMIT;
        if !should_include || !seen.insert(fragment.memory_id.clone()) {
            continue;
        }
        selected.push(fragment);
        if selected.len() >= SEMANTIC_RECALL_CANDIDATE_LIMIT + lexical_ids.len() {
            break;
        }
    }
    selected
}

fn build_semantic_recall_text(fragment: &MemoryFragmentRecord) -> String {
    truncate_chars(
        &format!(
            "{}\n{}\n{}\n{}\n{}\n{}",
            fragment.title_l0,
            fragment.summary_l1,
            truncate_chars(&fragment.content_l2, 360),
            fragment.fact_key,
            fragment.tags.join(" "),
            fragment.entities.join(" "),
        ),
        700,
    )
}

fn hash_text(text: &str) -> String {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn build_semantic_only_hit(
    fragment: MemoryFragmentRecord,
    semantic_score: f64,
    embedding_name: &str,
    now: f64,
) -> MemoryRecallHit {
    let freshness_score = compute_freshness_score(&fragment, now);
    let importance_score = compute_importance_score(&fragment);
    let final_score = combine_recall_scores(
        false,
        0.0,
        semantic_score,
        freshness_score,
        importance_score,
    );
    MemoryRecallHit {
        reason_json: json!({
            "match_kind": "semantic",
            "matched_terms": [],
            "matched_fields": [],
            "phrase_match": false,
            "phrase_fields": [],
            "category_match": false,
            "fact_key_match": false,
            "pinned": fragment.pinned,
            "tier": fragment.tier.clone(),
            "access_count": fragment.access_count,
            "hit_count": fragment.hit_count,
            "semantic_match": true,
            "semantic_model": embedding_name,
        }),
        fragment,
        lexical_score: 0.0,
        semantic_score,
        freshness_score,
        importance_score,
        final_score,
    }
}

fn merge_semantic_score(hit: &mut MemoryRecallHit, semantic_score: f64, embedding_name: &str) {
    if semantic_score <= hit.semantic_score {
        return;
    }
    hit.semantic_score = semantic_score;
    hit.final_score = combine_recall_scores(
        false,
        hit.lexical_score,
        hit.semantic_score,
        hit.freshness_score,
        hit.importance_score,
    );
    if let Some(map) = hit.reason_json.as_object_mut() {
        map.insert("semantic_match".to_string(), Value::Bool(true));
        map.insert(
            "semantic_model".to_string(),
            Value::String(embedding_name.to_string()),
        );
        map.insert(
            "match_kind".to_string(),
            Value::String(if hit.lexical_score > 0.0 {
                "hybrid".to_string()
            } else {
                "semantic".to_string()
            }),
        );
    }
}

fn resolve_memory_embedding_model(config: &Config) -> Option<(String, LlmModelConfig)> {
    let mut models = config
        .llm
        .models
        .iter()
        .filter(|(_, model)| is_embedding_model(model) && is_embedding_configured(model))
        .map(|(name, model)| (name.clone(), model.clone()))
        .collect::<Vec<_>>();
    models.sort_by(|left, right| left.0.cmp(&right.0));
    models.into_iter().next()
}

fn compute_freshness_score(fragment: &MemoryFragmentRecord, now: f64) -> f64 {
    let updated = if fragment.updated_at > 0.0 {
        fragment.updated_at
    } else {
        fragment.created_at
    };
    let updated_score = recency_score(updated, now, 14.0);
    let accessed_score = if fragment.last_accessed_at > 0.0 {
        recency_score(fragment.last_accessed_at, now, 7.0)
    } else {
        0.0
    };
    clamp01(updated_score * 0.74 + accessed_score * 0.26)
}

fn compute_importance_score(fragment: &MemoryFragmentRecord) -> f64 {
    let tier_score = tier_recall_weight(&fragment.tier);
    let access_score = clamp01(
        ((fragment.access_count.max(0) as f64).ln_1p() / 2.6)
            + ((fragment.hit_count.max(0) as f64).ln_1p() / 3.2),
    );
    clamp01(
        fragment.importance * 0.36
            + fragment.confidence * 0.24
            + tier_score * 0.18
            + access_score * 0.08
            + if fragment.pinned { 0.1 } else { 0.0 },
    )
}

fn combine_recall_scores(
    query_empty: bool,
    lexical_score: f64,
    semantic_score: f64,
    freshness_score: f64,
    importance_score: f64,
) -> f64 {
    if query_empty {
        return clamp01(lexical_score * 0.12 + freshness_score * 0.34 + importance_score * 0.54);
    }
    let lexical_only =
        clamp01(lexical_score * 0.54 + freshness_score * 0.14 + importance_score * 0.32);
    if semantic_score > 0.0 {
        return clamp01(
            lexical_score * 0.38
                + semantic_score * 0.24
                + freshness_score * 0.12
                + importance_score * 0.26,
        )
        .max(lexical_only);
    }
    lexical_only
}

fn cosine_similarity(left: &[f32], right: &[f32]) -> f64 {
    if left.is_empty() || right.is_empty() || left.len() != right.len() {
        return 0.0;
    }
    let mut dot = 0.0f64;
    let mut left_norm = 0.0f64;
    let mut right_norm = 0.0f64;
    for (l, r) in left.iter().zip(right.iter()) {
        let l = *l as f64;
        let r = *r as f64;
        dot += l * r;
        left_norm += l * l;
        right_norm += r * r;
    }
    if left_norm <= f64::EPSILON || right_norm <= f64::EPSILON {
        return 0.0;
    }
    clamp01(dot / left_norm.sqrt() / right_norm.sqrt())
}

fn recall_dedupe_key(fragment: &MemoryFragmentRecord) -> String {
    let fact_key = fragment.fact_key.trim().to_lowercase();
    if !fact_key.is_empty() {
        return format!("fact:{fact_key}");
    }
    let fingerprint = tokenize(&format!("{} {}", fragment.title_l0, fragment.summary_l1))
        .into_iter()
        .take(8)
        .collect::<Vec<_>>()
        .join("|");
    if fingerprint.is_empty() {
        format!("id:{}", fragment.memory_id)
    } else {
        format!("text:{fingerprint}")
    }
}

fn is_superseded_fragment(fragment: &MemoryFragmentRecord) -> bool {
    fragment
        .superseded_by_memory_id
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
}

fn fragment_status_rank(status: &str) -> i32 {
    match normalize_fragment_status(Some(status)).as_str() {
        STATUS_ACTIVE => 3,
        STATUS_SUPERSEDED => 2,
        STATUS_INVALIDATED => 1,
        _ => 0,
    }
}

fn fragment_tier_rank(tier: &str) -> i32 {
    match normalize_fragment_tier(Some(tier)).as_str() {
        TIER_CORE => 3,
        TIER_WORKING => 2,
        TIER_PERIPHERAL => 1,
        _ => 0,
    }
}

fn fragment_material_signature(fragment: &MemoryFragmentRecord) -> String {
    let mut tags = fragment.tags.clone();
    let mut entities = fragment.entities.clone();
    tags.sort();
    entities.sort();
    format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        fragment.category.trim().to_lowercase(),
        fragment.title_l0.trim(),
        fragment.summary_l1.trim(),
        fragment.content_l2.trim(),
        tags.join("|"),
        entities.join("|"),
    )
}

fn normalize_fragment_tier(value: Option<&str>) -> String {
    match normalize_slug(value).as_deref() {
        Some(TIER_CORE) => TIER_CORE.to_string(),
        Some(TIER_PERIPHERAL) => TIER_PERIPHERAL.to_string(),
        Some(TIER_WORKING) => TIER_WORKING.to_string(),
        _ => DEFAULT_TIER.to_string(),
    }
}

fn normalize_fragment_status(value: Option<&str>) -> String {
    match normalize_slug(value).as_deref() {
        Some(STATUS_INVALIDATED) => STATUS_INVALIDATED.to_string(),
        Some(STATUS_SUPERSEDED) => STATUS_SUPERSEDED.to_string(),
        Some(STATUS_ACTIVE) => STATUS_ACTIVE.to_string(),
        _ => STATUS_ACTIVE.to_string(),
    }
}

fn last_active_timestamp(fragment: &MemoryFragmentRecord) -> f64 {
    if fragment.last_accessed_at > 0.0 {
        return fragment.last_accessed_at;
    }
    if fragment.updated_at > 0.0 {
        return fragment.updated_at;
    }
    if fragment.valid_from > 0.0 {
        return fragment.valid_from;
    }
    fragment.created_at
}

fn fragment_age_days(fragment: &MemoryFragmentRecord, now: f64) -> f64 {
    let anchor = last_active_timestamp(fragment);
    if anchor <= 0.0 || now <= anchor {
        return 0.0;
    }
    (now - anchor) / 86_400.0
}

fn compute_frequency_score(fragment: &MemoryFragmentRecord) -> f64 {
    clamp01(
        ((fragment.access_count.max(0) as f64).ln_1p() / 2.8)
            + ((fragment.hit_count.max(0) as f64).ln_1p() / 2.4),
    )
}

fn compute_intrinsic_score(fragment: &MemoryFragmentRecord) -> f64 {
    clamp01(fragment.importance * 0.58 + fragment.confidence * 0.42)
}

fn compute_decay_score(fragment: &MemoryFragmentRecord, now: f64) -> f64 {
    let age_days = fragment_age_days(fragment, now);
    let half_life_days = match normalize_fragment_tier(Some(&fragment.tier)).as_str() {
        TIER_CORE => 60.0,
        TIER_PERIPHERAL => 14.0,
        _ => 30.0,
    } + fragment.importance * 28.0
        + ((fragment.access_count.max(0) + fragment.hit_count.max(0)) as f64).ln_1p() * 8.0;
    clamp01(1.0 / (1.0 + age_days / half_life_days.max(1.0)))
}

fn compute_lifecycle_score(fragment: &MemoryFragmentRecord, now: f64) -> f64 {
    clamp01(
        compute_decay_score(fragment, now) * 0.4
            + compute_frequency_score(fragment) * 0.3
            + compute_intrinsic_score(fragment) * 0.3,
    )
}

fn evaluate_fragment_tier(fragment: &MemoryFragmentRecord, now: f64) -> String {
    if fragment.pinned {
        return TIER_CORE.to_string();
    }
    let lifecycle_score = compute_lifecycle_score(fragment, now);
    let age_days = fragment_age_days(fragment, now);
    if fragment.importance >= 0.82
        && (fragment.access_count >= LIFECYCLE_CORE_ACCESS_THRESHOLD
            || fragment.hit_count >= LIFECYCLE_CORE_HIT_THRESHOLD
            || lifecycle_score >= LIFECYCLE_CORE_SCORE_THRESHOLD)
    {
        return TIER_CORE.to_string();
    }
    if lifecycle_score < LIFECYCLE_PERIPHERAL_SCORE_THRESHOLD
        || (age_days >= LIFECYCLE_PERIPHERAL_AGE_DAYS
            && fragment.access_count < LIFECYCLE_WORKING_ACCESS_THRESHOLD
            && fragment.hit_count < LIFECYCLE_WORKING_HIT_THRESHOLD)
    {
        return TIER_PERIPHERAL.to_string();
    }
    if fragment.access_count >= LIFECYCLE_WORKING_ACCESS_THRESHOLD
        || fragment.hit_count >= LIFECYCLE_WORKING_HIT_THRESHOLD
        || lifecycle_score >= LIFECYCLE_WORKING_SCORE_THRESHOLD
        || age_days <= 30.0
    {
        return TIER_WORKING.to_string();
    }
    TIER_PERIPHERAL.to_string()
}

fn refresh_fragment_lifecycle_record(fragment: &mut MemoryFragmentRecord, now: f64) -> bool {
    let original_status = fragment.status.clone();
    let original_tier = fragment.tier.clone();
    let original_invalidated_at = fragment.invalidated_at;

    if fragment.invalidated_at.unwrap_or(0.0) > 0.0
        || normalize_fragment_status(Some(&fragment.status)) == STATUS_INVALIDATED
    {
        fragment.status = STATUS_INVALIDATED.to_string();
        fragment.invalidated_at = Some(fragment.invalidated_at.unwrap_or(now));
    } else if is_superseded_fragment(fragment) {
        fragment.status = STATUS_SUPERSEDED.to_string();
        fragment.invalidated_at = None;
    } else {
        fragment.status = STATUS_ACTIVE.to_string();
        fragment.invalidated_at = None;
        fragment.tier = evaluate_fragment_tier(fragment, now);
    }

    original_status != fragment.status
        || original_tier != fragment.tier
        || original_invalidated_at != fragment.invalidated_at
}

fn matched_terms_for_field(tokens: &[String], field_text: &str) -> Vec<String> {
    tokens
        .iter()
        .filter(|token| !token.is_empty() && field_text.contains(token.as_str()))
        .cloned()
        .collect::<Vec<_>>()
}

fn merge_matched_terms(groups: &[&Vec<String>]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut merged = Vec::new();
    for group in groups {
        for term in *group {
            if seen.insert(term.clone()) {
                merged.push(term.clone());
            }
        }
    }
    merged
}

fn collect_matched_fields(
    title_terms: &[String],
    summary_terms: &[String],
    content_terms: &[String],
    fact_key_terms: &[String],
    tag_terms: &[String],
    entity_terms: &[String],
    category_match: bool,
    phrase_fields: &[&str],
) -> Vec<String> {
    let mut fields = Vec::new();
    let mut seen = HashSet::new();
    for (name, has_match) in [
        ("title", !title_terms.is_empty()),
        ("summary", !summary_terms.is_empty()),
        ("content", !content_terms.is_empty()),
        ("fact_key", !fact_key_terms.is_empty()),
        ("tags", !tag_terms.is_empty()),
        ("entities", !entity_terms.is_empty()),
        ("category", category_match),
    ] {
        if has_match && seen.insert(name.to_string()) {
            fields.push(name.to_string());
        }
    }
    for field in phrase_fields {
        let field_name = (*field).to_string();
        if seen.insert(field_name.clone()) {
            fields.push(field_name);
        }
    }
    fields
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

fn recency_score(timestamp: f64, now: f64, half_life_days: f64) -> f64 {
    if timestamp <= 0.0 || now <= timestamp {
        return 1.0;
    }
    clamp01(1.0 / (1.0 + (now - timestamp) / 86_400.0 / half_life_days.max(1.0)))
}

fn tier_recall_weight(tier: &str) -> f64 {
    match tier.trim().to_lowercase().as_str() {
        "core" => 1.0,
        "working" => 0.76,
        "peripheral" => 0.52,
        _ => 0.6,
    }
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
        if SENTENCE_STOP_CHARS.contains(&ch) || title.chars().count() >= 40 {
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
            output.push(ELLIPSIS);
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
    use crate::storage::{
        MemoryFragmentEmbeddingRecord, MemoryHitRecord, SqliteStorage, StorageBackend,
    };
    use rusqlite::Connection;
    use std::sync::Arc;
    use tempfile::tempdir;

    fn sample_fragment() -> MemoryFragmentRecord {
        MemoryFragmentRecord {
            memory_id: "m1".to_string(),
            user_id: "u1".to_string(),
            agent_id: "a1".to_string(),
            source_session_id: String::new(),
            source_round_id: String::new(),
            source_type: DEFAULT_SOURCE_TYPE.to_string(),
            category: "preference".to_string(),
            title_l0: "Rust 开发偏好".to_string(),
            summary_l1: "用户偏好使用 Rust 与 Axum 完成服务端开发。".to_string(),
            content_l2: "项目长期偏好 Rust、Axum、SQLite。".to_string(),
            fact_key: "preference::backend_stack".to_string(),
            tags: vec!["rust".to_string(), "axum".to_string()],
            entities: vec!["SQLite".to_string(), "wunder".to_string()],
            importance: 0.8,
            confidence: 0.85,
            tier: "core".to_string(),
            status: STATUS_ACTIVE.to_string(),
            pinned: false,
            confirmed_by_user: false,
            access_count: 2,
            hit_count: 1,
            last_accessed_at: 0.0,
            valid_from: 0.0,
            invalidated_at: None,
            supersedes_memory_id: None,
            superseded_by_memory_id: None,
            embedding_model: None,
            vector_ref: None,
            created_at: 0.0,
            updated_at: 0.0,
        }
    }

    #[test]
    fn tokenize_supports_mixed_text() {
        let items = tokenize("Rust 开发 Vue3 memory");
        assert!(items.contains(&"rust".to_string()));
        assert!(items.contains(&"开发".to_string()));
        assert!(items.contains(&"vue3".to_string()));
    }

    #[test]
    fn search_blob_contains_tags() {
        let mut fragment = sample_fragment();
        fragment.tags = vec!["rust".to_string()];
        assert!(search_blob(&fragment).contains("rust"));
    }

    #[test]
    fn build_recall_hit_records_matched_fields() {
        let fragment = sample_fragment();
        let hit = build_recall_hit(fragment, "rust axum", &tokenize("rust axum"), now_ts())
            .expect("recall hit");
        let matched_fields = hit
            .reason_json
            .get("matched_fields")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        assert!(matched_fields
            .iter()
            .any(|value| value.as_str() == Some("title")));
        assert!(matched_fields
            .iter()
            .any(|value| value.as_str() == Some("tags")));
    }

    #[test]
    fn build_prompt_block_is_compact_and_timestamped() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("memory-prompt-block.db");
        let storage: Arc<dyn StorageBackend> =
            Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        storage.ensure_initialized().expect("init storage");
        let store = MemoryFragmentStore::new(storage);

        let mut fragment = sample_fragment();
        fragment.updated_at = 1_700_000_000.0;
        let block = store.build_prompt_block(
            &[MemoryRecallHit {
                fragment,
                reason_json: serde_json::json!({
                    "matched_terms": ["rust"]
                }),
                lexical_score: 0.8,
                semantic_score: 0.0,
                freshness_score: 0.9,
                importance_score: 0.8,
                final_score: 0.85,
            }],
            7,
        );

        assert!(block.contains("[长期记忆]"));
        assert!(block.contains("7"));
        assert!(block.contains("1"));
        let expected_timestamp = Local
            .timestamp_opt(1_700_000_000, 0)
            .single()
            .expect("local timestamp")
            .format("%Y-%m-%d %H:%M")
            .to_string();
        assert!(block.contains(&format!("[{expected_timestamp}]")));
        assert!(block.contains("项目长期偏好 Rust、Axum、SQLite。"));
        assert!(!block.contains("preference"));
        assert!(!block.contains("matched"));
    }

    #[test]
    fn build_prompt_block_keeps_inventory_hint_without_hits() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("memory-prompt-empty-hit.db");
        let storage: Arc<dyn StorageBackend> =
            Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        storage.ensure_initialized().expect("init storage");
        let store = MemoryFragmentStore::new(storage);

        let block = store.build_prompt_block(&[], 42);

        assert!(block.contains("42"));
        assert!(!block.contains("- ["));
    }

    #[test]
    fn dedupe_recall_hits_prefers_top_scored_fact_key() {
        let mut newer = sample_fragment();
        newer.memory_id = "m-newer".to_string();
        newer.updated_at = 200.0;
        let mut older = newer.clone();
        older.memory_id = "m-older".to_string();
        older.updated_at = 100.0;
        let deduped = dedupe_recall_hits(vec![
            build_recall_hit(newer, "rust", &tokenize("rust"), 300.0).expect("newer hit"),
            build_recall_hit(older, "rust", &tokenize("rust"), 300.0).expect("older hit"),
        ]);
        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].fragment.memory_id, "m-newer");
    }

    #[test]
    fn list_fragments_prefers_higher_deduped_hit_count() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("memory-hit-count-order.db");
        let storage: Arc<dyn StorageBackend> =
            Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        storage.ensure_initialized().expect("init storage");
        let store = MemoryFragmentStore::new(storage.clone());

        let mut top = sample_fragment();
        top.memory_id = "m-top".to_string();
        top.title_l0 = "Top hit".to_string();
        top.hit_count = 0;
        top.updated_at = 10.0;
        storage
            .upsert_memory_fragment(&top)
            .expect("save top fragment");

        let mut low = sample_fragment();
        low.memory_id = "m-low".to_string();
        low.title_l0 = "Low hit".to_string();
        low.hit_count = 99;
        low.updated_at = 100.0;
        storage
            .upsert_memory_fragment(&low)
            .expect("save low fragment");

        for (hit_id, memory_id, session_id, round_id, query_text) in [
            ("hit-1", "m-top", "s1", "r1", "rust"),
            ("hit-2", "m-top", "s1", "r1", "rust"),
            ("hit-3", "m-top", "s2", "r2", "rust"),
            ("hit-4", "m-low", "s3", "", "axum"),
        ] {
            storage
                .insert_memory_hit(&MemoryHitRecord {
                    hit_id: hit_id.to_string(),
                    memory_id: memory_id.to_string(),
                    user_id: "u1".to_string(),
                    agent_id: "a1".to_string(),
                    session_id: session_id.to_string(),
                    round_id: round_id.to_string(),
                    query_text: query_text.to_string(),
                    reason_json: serde_json::json!({}),
                    lexical_score: 0.8,
                    semantic_score: 0.0,
                    freshness_score: 0.5,
                    importance_score: 0.5,
                    final_score: 0.8,
                    created_at: now_ts(),
                })
                .expect("insert hit");
        }

        let listed = store.list_fragments("u1", Some("a1"), MemoryFragmentListOptions::default());
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].memory_id, "m-top");
        assert_eq!(listed[0].hit_count, 2);
        assert_eq!(listed[1].memory_id, "m-low");
        assert_eq!(listed[1].hit_count, 1);

        let persisted_top = store
            .get_fragment("u1", Some("a1"), "m-top")
            .expect("top fragment exists");
        let persisted_low = store
            .get_fragment("u1", Some("a1"), "m-low")
            .expect("low fragment exists");
        assert_eq!(persisted_top.hit_count, 2);
        assert_eq!(persisted_low.hit_count, 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn recall_for_prompt_skips_preview_only_hits_and_dedupes_same_round() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("memory-hit-dedupe.db");
        let storage: Arc<dyn StorageBackend> =
            Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        storage.ensure_initialized().expect("init storage");
        let store = MemoryFragmentStore::new(storage);

        let mut fragment = sample_fragment();
        fragment.memory_id = "m-dedupe".to_string();
        fragment.hit_count = 0;
        fragment.access_count = 0;
        store
            .storage
            .upsert_memory_fragment(&fragment)
            .expect("save fragment");

        let preview_inventory = store
            .recall_for_prompt_inventory(
                None,
                "u1",
                Some("a1"),
                Some("preview-session"),
                None,
                None,
                Some(5),
            )
            .await;
        assert_eq!(preview_inventory.hits.len(), 1);
        let after_preview = store
            .get_fragment("u1", Some("a1"), "m-dedupe")
            .expect("fragment after preview");
        assert_eq!(after_preview.hit_count, 0);

        let first_recall = store
            .recall_for_prompt_inventory(
                None,
                "u1",
                Some("a1"),
                Some("chat-session"),
                Some("round-1"),
                Some("rust"),
                Some(5),
            )
            .await;
        assert_eq!(first_recall.hits.len(), 1);
        let after_first = store
            .get_fragment("u1", Some("a1"), "m-dedupe")
            .expect("fragment after first recall");
        assert_eq!(after_first.hit_count, 1);

        let second_recall = store
            .recall_for_prompt_inventory(
                None,
                "u1",
                Some("a1"),
                Some("chat-session"),
                Some("round-1"),
                Some("rust"),
                Some(5),
            )
            .await;
        assert_eq!(second_recall.hits.len(), 1);
        let after_second = store
            .get_fragment("u1", Some("a1"), "m-dedupe")
            .expect("fragment after second recall");
        assert_eq!(after_second.hit_count, 1);

        let hit_rows = store.list_hits("u1", Some("a1"), Some("chat-session"), 20);
        assert_eq!(hit_rows.len(), 1);
        assert_eq!(hit_rows[0].memory_id, "m-dedupe");
    }

    #[test]
    fn is_superseded_fragment_detects_active_successor() {
        let mut fragment = sample_fragment();
        fragment.superseded_by_memory_id = Some("m2".to_string());
        assert!(is_superseded_fragment(&fragment));
    }

    #[test]
    fn combine_recall_scores_prefers_hybrid_when_semantic_available() {
        let lexical_only = combine_recall_scores(false, 0.8, 0.0, 0.4, 0.6);
        let hybrid = combine_recall_scores(false, 0.8, 0.7, 0.4, 0.6);
        assert!(hybrid >= lexical_only);
    }

    #[test]
    fn cosine_similarity_returns_unit_for_same_vector() {
        let score = cosine_similarity(&[1.0, 2.0, 3.0], &[1.0, 2.0, 3.0]);
        assert!((score - 1.0).abs() < 1e-6);
    }

    #[test]
    fn sqlite_memory_fragment_embedding_roundtrip() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("memory-embedding.db");
        let storage: Arc<dyn StorageBackend> =
            Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        storage.ensure_initialized().expect("init storage");

        let record = MemoryFragmentEmbeddingRecord {
            memory_id: "m1".to_string(),
            user_id: "u1".to_string(),
            agent_id: "a1".to_string(),
            embedding_model: "embed-demo".to_string(),
            content_hash: "hash-1".to_string(),
            vector: vec![0.1, 0.2, 0.3],
            dimensions: 3,
            updated_at: 123.0,
        };
        storage
            .upsert_memory_fragment_embedding(&record)
            .expect("upsert embedding");

        let loaded = storage
            .get_memory_fragment_embedding("u1", "a1", "m1", "embed-demo", "hash-1")
            .expect("load embedding")
            .expect("embedding exists");
        assert_eq!(loaded, record);
    }

    #[test]
    fn sqlite_legacy_memory_fragments_table_is_migrated_on_save() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("memory-legacy.db");
        let conn = Connection::open(&db_path).expect("open legacy db");
        conn.execute_batch(
            r#"
            CREATE TABLE memory_fragments (
              memory_id TEXT PRIMARY KEY,
              user_id TEXT NOT NULL,
              agent_id TEXT NOT NULL,
              source_session_id TEXT NOT NULL,
              source_type TEXT NOT NULL,
              category TEXT NOT NULL,
              title_l0 TEXT NOT NULL,
              summary_l1 TEXT NOT NULL,
              content_l2 TEXT NOT NULL,
              fact_key TEXT NOT NULL,
              status TEXT NOT NULL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL
            );
            CREATE INDEX idx_memory_fragments_fact_key
              ON memory_fragments (user_id, agent_id, fact_key);
            CREATE INDEX idx_memory_fragments_status
              ON memory_fragments (user_id, agent_id, status, updated_at DESC);
            "#,
        )
        .expect("create legacy table");
        drop(conn);

        let storage: Arc<dyn StorageBackend> =
            Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let store = MemoryFragmentStore::new(storage);
        let record = store
            .save_fragment(
                "u1",
                Some("__default__"),
                MemoryFragmentInput {
                    memory_id: Some("legacy-upgraded".to_string()),
                    source_session_id: Some("sess-1".to_string()),
                    source_type: Some("memory_manager".to_string()),
                    category: Some("tool-note".to_string()),
                    summary_l1: Some("记住用户姓名：周华健".to_string()),
                    content_l2: Some("用户姓名：周华健".to_string()),
                    fact_key: Some("tool-note::legacy-upgraded".to_string()),
                    ..Default::default()
                },
            )
            .expect("save fragment after migration");

        assert_eq!(record.memory_id, "legacy-upgraded");
        assert_eq!(record.source_round_id, "");
        assert!(record.tags.is_empty());
        assert!(record.entities.is_empty());

        let loaded = store
            .get_fragment("u1", Some("__default__"), "legacy-upgraded")
            .expect("load upgraded fragment");
        assert_eq!(loaded.content_l2, "用户姓名：周华健");
        assert_eq!(loaded.category, "tool-note");
    }

    #[test]
    fn delete_fragment_cleans_embedding_cache() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("memory-embedding-clean.db");
        let storage: Arc<dyn StorageBackend> =
            Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        storage.ensure_initialized().expect("init storage");
        let store = MemoryFragmentStore::new(storage.clone());

        let saved = store
            .save_fragment(
                "u1",
                Some("a1"),
                MemoryFragmentInput {
                    title_l0: Some("记忆标题".to_string()),
                    summary_l1: Some("记忆摘要".to_string()),
                    content_l2: Some("记忆正文".to_string()),
                    fact_key: Some("fact::demo".to_string()),
                    ..Default::default()
                },
            )
            .expect("save fragment");
        storage
            .upsert_memory_fragment_embedding(&MemoryFragmentEmbeddingRecord {
                memory_id: saved.memory_id.clone(),
                user_id: "u1".to_string(),
                agent_id: "a1".to_string(),
                embedding_model: "embed-demo".to_string(),
                content_hash: "hash-1".to_string(),
                vector: vec![0.1, 0.2],
                dimensions: 2,
                updated_at: 10.0,
            })
            .expect("cache embedding");

        assert!(store.delete_fragment("u1", Some("a1"), &saved.memory_id));
        let cached = storage
            .get_memory_fragment_embedding("u1", "a1", &saved.memory_id, "embed-demo", "hash-1")
            .expect("load embedding after delete");
        assert_eq!(cached, None);
    }

    #[test]
    fn save_fragment_supersedes_previous_fact_version() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("memory-supersede.db");
        let storage: Arc<dyn StorageBackend> =
            Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        storage.ensure_initialized().expect("init storage");
        let store = MemoryFragmentStore::new(storage);

        let old_record = store
            .save_fragment(
                "u1",
                Some("a1"),
                MemoryFragmentInput {
                    title_l0: Some("Response format".to_string()),
                    summary_l1: Some("Prefer markdown tables.".to_string()),
                    content_l2: Some("Please use markdown tables in answers.".to_string()),
                    fact_key: Some("preference::response_format".to_string()),
                    importance: Some(0.7),
                    confidence: Some(0.8),
                    ..Default::default()
                },
            )
            .expect("save old fragment");

        let new_record = store
            .save_fragment(
                "u1",
                Some("a1"),
                MemoryFragmentInput {
                    title_l0: Some("Response format".to_string()),
                    summary_l1: Some("Prefer bullet lists instead of tables.".to_string()),
                    content_l2: Some(
                        "Avoid markdown tables. Use concise bullet lists.".to_string(),
                    ),
                    fact_key: Some("preference::response_format".to_string()),
                    importance: Some(0.72),
                    confidence: Some(0.86),
                    ..Default::default()
                },
            )
            .expect("save new fragment");

        assert_eq!(
            new_record.supersedes_memory_id.as_deref(),
            Some(old_record.memory_id.as_str())
        );

        let old_loaded = store
            .get_fragment("u1", Some("a1"), &old_record.memory_id)
            .expect("load old fragment");
        assert_eq!(old_loaded.status, STATUS_SUPERSEDED.to_string());
        assert_eq!(
            old_loaded.superseded_by_memory_id.as_deref(),
            Some(new_record.memory_id.as_str())
        );
    }

    #[tokio::test]
    async fn recall_for_prompt_ignores_superseded_fragments() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("memory-recall-superseded.db");
        let storage: Arc<dyn StorageBackend> =
            Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        storage.ensure_initialized().expect("init storage");
        let store = MemoryFragmentStore::new(storage);

        let old_record = store
            .save_fragment(
                "u1",
                Some("a1"),
                MemoryFragmentInput {
                    title_l0: Some("Response format".to_string()),
                    summary_l1: Some("Prefer markdown tables.".to_string()),
                    content_l2: Some("Please use markdown tables in answers.".to_string()),
                    fact_key: Some("preference::response_format".to_string()),
                    ..Default::default()
                },
            )
            .expect("save old fragment");

        let new_record = store
            .save_fragment(
                "u1",
                Some("a1"),
                MemoryFragmentInput {
                    title_l0: Some("Response format".to_string()),
                    summary_l1: Some("Prefer bullet lists instead of tables.".to_string()),
                    content_l2: Some(
                        "Avoid markdown tables. Use concise bullet lists.".to_string(),
                    ),
                    fact_key: Some("preference::response_format".to_string()),
                    ..Default::default()
                },
            )
            .expect("save new fragment");

        let hits = store
            .recall_for_prompt(
                None,
                "u1",
                Some("a1"),
                Some("s1"),
                Some("r1"),
                Some("bullet lists"),
                Some(6),
            )
            .await;

        assert!(hits
            .iter()
            .any(|hit| hit.fragment.memory_id == new_record.memory_id));
        assert!(hits
            .iter()
            .all(|hit| hit.fragment.memory_id != old_record.memory_id));
    }

    #[test]
    fn refresh_fragment_lifecycle_updates_tier_by_usage_and_decay() {
        let now = now_ts();

        let mut pinned = sample_fragment();
        pinned.tier = TIER_PERIPHERAL.to_string();
        pinned.pinned = true;
        assert!(refresh_fragment_lifecycle_record(&mut pinned, now));
        assert_eq!(pinned.tier, TIER_CORE.to_string());

        let mut stale = sample_fragment();
        stale.tier = TIER_WORKING.to_string();
        stale.pinned = false;
        stale.confirmed_by_user = false;
        stale.access_count = 0;
        stale.hit_count = 0;
        stale.importance = 0.2;
        stale.confidence = 0.35;
        stale.last_accessed_at = now - 95.0 * 86_400.0;
        stale.updated_at = stale.last_accessed_at;
        stale.created_at = stale.last_accessed_at;
        assert!(refresh_fragment_lifecycle_record(&mut stale, now));
        assert_eq!(stale.tier, TIER_PERIPHERAL.to_string());
    }

    #[test]
    fn delete_fragment_restores_linked_predecessor() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("memory-delete-restore.db");
        let storage: Arc<dyn StorageBackend> =
            Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        storage.ensure_initialized().expect("init storage");
        let store = MemoryFragmentStore::new(storage);

        let old_record = store
            .save_fragment(
                "u1",
                Some("a1"),
                MemoryFragmentInput {
                    title_l0: Some("Reply language".to_string()),
                    summary_l1: Some("Reply in English.".to_string()),
                    content_l2: Some("Reply in English by default.".to_string()),
                    fact_key: Some("preference::reply_language".to_string()),
                    ..Default::default()
                },
            )
            .expect("save old fragment");

        let current = store
            .save_fragment(
                "u1",
                Some("a1"),
                MemoryFragmentInput {
                    title_l0: Some("Reply language".to_string()),
                    summary_l1: Some("Reply in Chinese.".to_string()),
                    content_l2: Some("Reply in Chinese by default.".to_string()),
                    fact_key: Some("preference::reply_language".to_string()),
                    ..Default::default()
                },
            )
            .expect("save current fragment");

        assert!(store.delete_fragment("u1", Some("a1"), &current.memory_id));
        let restored = store
            .get_fragment("u1", Some("a1"), &old_record.memory_id)
            .expect("load restored fragment");
        assert_eq!(restored.status, STATUS_ACTIVE.to_string());
        assert_eq!(restored.superseded_by_memory_id, None);
    }
}
