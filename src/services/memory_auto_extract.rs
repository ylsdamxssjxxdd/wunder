use crate::services::memory_fragments::{
    MemoryFragmentInput, MemoryFragmentListOptions, MemoryFragmentStore,
};
use crate::storage::{MemoryFragmentRecord, MemoryJobRecord, StorageBackend};
use anyhow::Result;
use chrono::Utc;
use serde::Serialize;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;
use uuid::Uuid;

const ACTIVE_STATUS: &str = "active";
const AUTO_SOURCE_TYPE: &str = "auto-turn";
const JOB_TYPE_AUTO_EXTRACT_TURN: &str = "auto_extract_turn";
const JOB_STATUS_QUEUED: &str = "queued";
const JOB_STATUS_RUNNING: &str = "running";
const JOB_STATUS_COMPLETED: &str = "completed";
const JOB_STATUS_SKIPPED: &str = "skipped";
const JOB_STATUS_FAILED: &str = "failed";
const MAX_AUTO_CANDIDATES: usize = 4;
const MAX_SEGMENT_CHARS: usize = 240;
const MAX_TEXT_CHARS: usize = 1_000;

const ZH_REPLY: &str = "回复";
const ZH_ANSWER: &str = "回答";
const ZH_OUTPUT: &str = "输出";
const ZH_EXPLAIN: &str = "说明";
const ZH_COMMUNICATE: &str = "交流";
const ZH_ZH: &str = "中文";
const ZH_ZH_ALT: &str = "汉语";
const ZH_EN: &str = "英文";
const ZH_EN_ALT: &str = "英语";
const ZH_BRIEF: &str = "简洁";
const ZH_SHORT: &str = "简短";
const ZH_COMPACT: &str = "精简";
const ZH_DETAILED: &str = "详细";
const ZH_SPECIFIC: &str = "具体";
const ZH_EXPAND: &str = "展开";
const ZH_NO_TABLE: &str = "不要表格";
const ZH_NO_TABLE_ALT1: &str = "别用表格";
const ZH_NO_TABLE_ALT2: &str = "避免表格";
const ZH_NO_TABLE_ALT3: &str = "不用表格";
const ZH_NAME_PREFIX: &str = "我叫";
const ZH_NAME_PREFIX_ALT: &str = "我的名字是";
const ZH_IDENTITY_PREFIX: &str = "我是";
const ZH_ROLE_PREFIX: &str = "我的角色是";
const ZH_TITLE_PREFIX: &str = "我的职位是";
const ZH_RESPONSIBLE_PREFIX: &str = "我负责";
const ZH_RESPONSIBLE_PREFIX_ALT: &str = "我在负责";
const ZH_PLAN_PREFIX1: &str = "我正在";
const ZH_PLAN_PREFIX2: &str = "我计划";
const ZH_PLAN_PREFIX3: &str = "接下来我要";
const ZH_PLAN_PREFIX4: &str = "接下来我会";
const ZH_PLAN_PREFIX5: &str = "本周要";
const ZH_PLAN_PREFIX6: &str = "今天要";
const ZH_PLAN_PREFIX7: &str = "当前在做";
const ZH_PREFERENCE_PREFIX1: &str = "我喜欢";
const ZH_PREFERENCE_PREFIX2: &str = "我偏好";
const ZH_PREFERENCE_PREFIX3: &str = "我习惯";
const ZH_PREFERENCE_PREFIX4: &str = "我常用";
const ZH_PREFERENCE_PREFIX5: &str = "我不喜欢";
const ZH_PREFERENCE_PREFIX6: &str = "我讨厌";
const ZH_PREFERENCE_PREFIX7: &str = "我不想";
const ZH_PREFERENCE_PREFIX8: &str = "请优先";
const ZH_PREFERENCE_PREFIX9: &str = "默认使用";
const ZH_REMEMBER: &str = "记住";
const ZH_REMEMBER_ALT: &str = "请记住";
const ZH_REMEMBER_NOTE: &str = "记一下";
const ZH_AFTER_THIS: &str = "以后";
const ZH_LATER: &str = "之后";
const ZH_NEXT: &str = "后面";
const ZH_DEFAULT: &str = "默认";
const ZH_PREFER: &str = "优先";
const ZH_USE: &str = "请用";
const ZH_DONT_USE: &str = "不要用";
const ZH_DONT_USE_ALT: &str = "别用";
const ZH_ALWAYS: &str = "始终";
const ZH_TOTAL_ALWAYS: &str = "总是";
const ZH_QM: &str = "？";
const ZH_HOW: &str = "如何";
const ZH_HOW_ALT: &str = "怎么";

const TITLE_REPLY_ZH: &str = "默认使用中文回复";
const TITLE_REPLY_EN: &str = "默认使用英文回复";
const TITLE_STYLE_BRIEF: &str = "回复尽量简洁";
const TITLE_STYLE_DETAILED: &str = "回复尽量详细";
const TITLE_NO_TABLE: &str = "避免使用表格";
const TITLE_USER_NAME: &str = "用户称呼：";
const TITLE_USER_IDENTITY: &str = "用户身份：";
const TITLE_USER_BACKGROUND: &str = "用户背景：";
const TITLE_CURRENT_PLAN: &str = "当前计划：";
const TITLE_USER_PREFERENCE: &str = "用户偏好：";
const TITLE_MEMORY_NOTE: &str = "记忆备注：";

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct MemoryAutoExtractOutcome {
    pub created: usize,
    pub updated: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone)]
struct ExtractionCandidate {
    category: String,
    fact_key: String,
    title: String,
    summary: String,
    content: String,
    tags: Vec<String>,
    tier: String,
    importance: f64,
    confidence: f64,
}

pub struct MemoryAutoExtractService {
    storage: Arc<dyn StorageBackend>,
    fragment_store: MemoryFragmentStore,
}

impl MemoryAutoExtractService {
    pub fn new(storage: Arc<dyn StorageBackend>) -> Self {
        let fragment_store = MemoryFragmentStore::new(storage.clone());
        Self {
            storage,
            fragment_store,
        }
    }

    pub fn capture_turn(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        session_id: &str,
        round_id: Option<&str>,
        question: &str,
        answer: &str,
    ) -> Result<MemoryAutoExtractOutcome> {
        let now = now_ts();
        let agent_scope = agent_id.unwrap_or("__default__").trim().to_string();
        let mut job = MemoryJobRecord {
            job_id: format!("mjob_{}", Uuid::new_v4().simple()),
            user_id: user_id.trim().to_string(),
            agent_id: agent_scope,
            session_id: session_id.trim().to_string(),
            job_type: JOB_TYPE_AUTO_EXTRACT_TURN.to_string(),
            status: JOB_STATUS_QUEUED.to_string(),
            request_payload: json!({
                "question": truncate_chars(question, MAX_TEXT_CHARS),
                "answer": truncate_chars(answer, MAX_TEXT_CHARS),
                "round_id": round_id.unwrap_or("").trim(),
            }),
            result_summary: String::new(),
            error_message: String::new(),
            queued_at: now,
            started_at: 0.0,
            finished_at: 0.0,
            updated_at: now,
        };
        self.storage.upsert_memory_job(&job)?;

        job.status = JOB_STATUS_RUNNING.to_string();
        job.started_at = now_ts();
        job.updated_at = job.started_at;
        self.storage.upsert_memory_job(&job)?;

        let run_result = self.capture_turn_inner(user_id, agent_id, session_id, round_id, question);
        match run_result {
            Ok(outcome) => {
                job.status = if outcome.created + outcome.updated > 0 {
                    JOB_STATUS_COMPLETED.to_string()
                } else {
                    JOB_STATUS_SKIPPED.to_string()
                };
                job.result_summary = format!(
                    "created={}, updated={}, skipped={}",
                    outcome.created, outcome.updated, outcome.skipped
                );
                job.error_message.clear();
                job.finished_at = now_ts();
                job.updated_at = job.finished_at;
                self.storage.upsert_memory_job(&job)?;
                Ok(outcome)
            }
            Err(err) => {
                job.status = JOB_STATUS_FAILED.to_string();
                job.result_summary.clear();
                job.error_message = truncate_chars(&err.to_string(), 300);
                job.finished_at = now_ts();
                job.updated_at = job.finished_at;
                let _ = self.storage.upsert_memory_job(&job);
                Err(err)
            }
        }
    }
    fn capture_turn_inner(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        session_id: &str,
        round_id: Option<&str>,
        question: &str,
    ) -> Result<MemoryAutoExtractOutcome> {
        let candidates = extract_candidates(question);
        if candidates.is_empty() {
            return Ok(MemoryAutoExtractOutcome::default());
        }

        let existing = self.fragment_store.list_fragments(
            user_id,
            agent_id,
            MemoryFragmentListOptions {
                include_invalidated: true,
                limit: Some(200),
                ..Default::default()
            },
        );
        let mut existing_by_fact_key = HashMap::<String, MemoryFragmentRecord>::new();
        for item in existing {
            if item.fact_key.trim().is_empty() {
                continue;
            }
            existing_by_fact_key
                .entry(item.fact_key.clone())
                .or_insert(item);
        }

        let mut outcome = MemoryAutoExtractOutcome::default();
        for candidate in candidates {
            match self.apply_candidate(
                user_id,
                agent_id,
                session_id,
                round_id,
                &candidate,
                existing_by_fact_key.get(&candidate.fact_key),
            )? {
                CandidateApplyAction::Created(record) => {
                    existing_by_fact_key.insert(record.fact_key.clone(), record);
                    outcome.created += 1;
                }
                CandidateApplyAction::Updated(record) => {
                    existing_by_fact_key.insert(record.fact_key.clone(), record);
                    outcome.updated += 1;
                }
                CandidateApplyAction::Skipped => {
                    outcome.skipped += 1;
                }
            }
        }
        Ok(outcome)
    }

    fn apply_candidate(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        session_id: &str,
        round_id: Option<&str>,
        candidate: &ExtractionCandidate,
        existing: Option<&MemoryFragmentRecord>,
    ) -> Result<CandidateApplyAction> {
        if let Some(existing) = existing {
            if is_same_candidate(existing, candidate) {
                return Ok(CandidateApplyAction::Skipped);
            }
            // Never auto-overwrite fragments the user already curated by hand.
            if should_protect_existing(existing) {
                return Ok(CandidateApplyAction::Skipped);
            }
            let record = self.fragment_store.save_fragment(
                user_id,
                agent_id,
                MemoryFragmentInput {
                    memory_id: Some(existing.memory_id.clone()),
                    source_session_id: Some(session_id.trim().to_string()),
                    source_round_id: round_id
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string),
                    source_type: Some(AUTO_SOURCE_TYPE.to_string()),
                    category: Some(candidate.category.clone()),
                    title_l0: Some(candidate.title.clone()),
                    summary_l1: Some(candidate.summary.clone()),
                    content_l2: Some(candidate.content.clone()),
                    fact_key: Some(candidate.fact_key.clone()),
                    tags: Some(candidate.tags.clone()),
                    entities: Some(Vec::new()),
                    importance: Some(candidate.importance),
                    confidence: Some(candidate.confidence),
                    tier: Some(candidate.tier.clone()),
                    status: Some(ACTIVE_STATUS.to_string()),
                    pinned: Some(existing.pinned),
                    confirmed_by_user: Some(existing.confirmed_by_user),
                    invalidated: Some(false),
                },
            )?;
            return Ok(CandidateApplyAction::Updated(record));
        }

        let record = self.fragment_store.save_fragment(
            user_id,
            agent_id,
            MemoryFragmentInput {
                memory_id: None,
                source_session_id: Some(session_id.trim().to_string()),
                source_round_id: round_id
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string),
                source_type: Some(AUTO_SOURCE_TYPE.to_string()),
                category: Some(candidate.category.clone()),
                title_l0: Some(candidate.title.clone()),
                summary_l1: Some(candidate.summary.clone()),
                content_l2: Some(candidate.content.clone()),
                fact_key: Some(candidate.fact_key.clone()),
                tags: Some(candidate.tags.clone()),
                entities: Some(Vec::new()),
                importance: Some(candidate.importance),
                confidence: Some(candidate.confidence),
                tier: Some(candidate.tier.clone()),
                status: Some(ACTIVE_STATUS.to_string()),
                pinned: Some(false),
                confirmed_by_user: Some(false),
                invalidated: Some(false),
            },
        )?;
        Ok(CandidateApplyAction::Created(record))
    }
}

#[derive(Debug)]
enum CandidateApplyAction {
    Created(MemoryFragmentRecord),
    Updated(MemoryFragmentRecord),
    Skipped,
}

fn extract_candidates(question: &str) -> Vec<ExtractionCandidate> {
    let normalized = normalize_sentence(question);
    if normalized.is_empty() {
        return Vec::new();
    }

    let mut items = Vec::new();
    let mut seen = HashSet::new();
    for segment in split_segments(&normalized) {
        for candidate in extract_segment_candidates(&segment) {
            if items.len() >= MAX_AUTO_CANDIDATES {
                return items;
            }
            if !seen.insert(candidate.fact_key.clone()) {
                continue;
            }
            items.push(candidate);
        }
    }
    items
}

fn extract_segment_candidates(segment: &str) -> Vec<ExtractionCandidate> {
    let mut items = extract_response_preferences(segment);
    if let Some(candidate) = extract_profile(segment) {
        items.push(candidate);
    }
    if let Some(candidate) = extract_plan(segment) {
        items.push(candidate);
    }
    if let Some(candidate) = extract_preference(segment) {
        items.push(candidate);
    }
    if items.is_empty() {
        if let Some(candidate) = extract_explicit_memory_note(segment) {
            items.push(candidate);
        }
    }
    items
}

fn extract_response_preferences(segment: &str) -> Vec<ExtractionCandidate> {
    let has_reply_context = contains_any(
        segment,
        &[
            ZH_REPLY,
            ZH_ANSWER,
            ZH_OUTPUT,
            ZH_EXPLAIN,
            ZH_COMMUNICATE,
            "reply",
            "respond",
            "response",
            "answer",
        ],
    );
    let has_directive = has_memory_directive(segment);
    if !has_reply_context && !has_directive {
        return Vec::new();
    }
    if looks_like_question(segment) && !has_directive {
        return Vec::new();
    }

    let mut items = Vec::new();
    if contains_any(
        segment,
        &[
            ZH_ZH,
            ZH_ZH_ALT,
            "in chinese",
            "use chinese",
            "reply in chinese",
        ],
    ) {
        items.push(build_candidate(
            "response-preference",
            "constraint::reply_language",
            TITLE_REPLY_ZH,
            segment,
            vec!["language", "reply", "zh"],
            "core",
            0.92,
            0.96,
        ));
    } else if contains_any(
        segment,
        &[ZH_EN, ZH_EN_ALT, "english", "in english", "use english"],
    ) {
        items.push(build_candidate(
            "response-preference",
            "constraint::reply_language",
            TITLE_REPLY_EN,
            segment,
            vec!["language", "reply", "en"],
            "core",
            0.92,
            0.96,
        ));
    }

    if contains_any(
        segment,
        &[ZH_BRIEF, ZH_SHORT, ZH_COMPACT, "brief", "concise", "short"],
    ) {
        items.push(build_candidate(
            "response-preference",
            "constraint::response_style",
            TITLE_STYLE_BRIEF,
            segment,
            vec!["style", "brief"],
            "core",
            0.84,
            0.9,
        ));
    } else if contains_any(
        segment,
        &[
            ZH_DETAILED,
            ZH_SPECIFIC,
            ZH_EXPAND,
            "detail",
            "detailed",
            "specific",
        ],
    ) {
        items.push(build_candidate(
            "response-preference",
            "constraint::response_style",
            TITLE_STYLE_DETAILED,
            segment,
            vec!["style", "detailed"],
            "core",
            0.84,
            0.9,
        ));
    }
    if contains_any(
        segment,
        &[
            ZH_NO_TABLE,
            ZH_NO_TABLE_ALT1,
            ZH_NO_TABLE_ALT2,
            ZH_NO_TABLE_ALT3,
            "不要用表格",
            "no table",
            "without table",
            "do not use tables",
            "don't use tables",
            "avoid tables",
        ],
    ) {
        items.push(build_candidate(
            "response-preference",
            "constraint::response_format",
            TITLE_NO_TABLE,
            segment,
            vec!["format", "table"],
            "core",
            0.82,
            0.88,
        ));
    }
    items
}

fn extract_profile(segment: &str) -> Option<ExtractionCandidate> {
    if let Some(value) = strip_prefixes(
        segment,
        &[
            ZH_NAME_PREFIX,
            ZH_NAME_PREFIX_ALT,
            "call me ",
            "my name is ",
        ],
    ) {
        let cleaned = clean_statement(&value);
        if cleaned.is_empty() {
            return None;
        }
        return Some(build_candidate(
            "profile",
            "profile::name",
            &format!("{TITLE_USER_NAME}{}", truncate_chars(&cleaned, 40)),
            segment,
            vec!["identity", "name"],
            "core",
            0.82,
            0.86,
        ));
    }

    if let Some(value) = strip_prefixes(
        segment,
        &[
            ZH_IDENTITY_PREFIX,
            ZH_ROLE_PREFIX,
            ZH_TITLE_PREFIX,
            "i am ",
            "i'm ",
            "my role is ",
            "my title is ",
        ],
    ) {
        let cleaned = clean_statement(&value);
        if cleaned.is_empty() || looks_like_question(&cleaned) {
            return None;
        }
        return Some(build_candidate(
            "profile",
            "profile::identity",
            &format!("{TITLE_USER_IDENTITY}{}", truncate_chars(&cleaned, 48)),
            segment,
            vec!["identity", "role"],
            "core",
            0.8,
            0.84,
        ));
    }

    if let Some(value) = strip_prefixes(
        segment,
        &[
            ZH_RESPONSIBLE_PREFIX,
            ZH_RESPONSIBLE_PREFIX_ALT,
            "i work on ",
            "i am responsible for ",
            "i handle ",
        ],
    ) {
        let cleaned = clean_statement(&value);
        if cleaned.is_empty() || looks_like_question(&cleaned) {
            return None;
        }
        return Some(build_candidate(
            "profile",
            &format!("profile::{}", stable_hash(&format!("profile:{cleaned}"))),
            &format!("{TITLE_USER_BACKGROUND}{}", truncate_chars(&cleaned, 48)),
            segment,
            vec!["identity", "background"],
            "core",
            0.74,
            0.8,
        ));
    }

    None
}

fn extract_plan(segment: &str) -> Option<ExtractionCandidate> {
    let value = strip_prefixes(
        segment,
        &[
            ZH_PLAN_PREFIX1,
            ZH_PLAN_PREFIX2,
            ZH_PLAN_PREFIX3,
            ZH_PLAN_PREFIX4,
            ZH_PLAN_PREFIX5,
            ZH_PLAN_PREFIX6,
            ZH_PLAN_PREFIX7,
            "i am working on ",
            "i'm working on ",
            "i plan to ",
            "today i need to ",
            "this week i need to ",
            "next i will ",
            "i am currently ",
        ],
    )?;
    let cleaned = clean_statement(&value);
    if cleaned.is_empty() || looks_like_question(&cleaned) {
        return None;
    }
    Some(build_candidate(
        "plan",
        &format!("plan::{}", stable_hash(&format!("plan:{cleaned}"))),
        &format!("{TITLE_CURRENT_PLAN}{}", truncate_chars(&cleaned, 48)),
        segment,
        vec!["plan", "current"],
        "working",
        0.72,
        0.78,
    ))
}

fn extract_preference(segment: &str) -> Option<ExtractionCandidate> {
    let value = strip_prefixes(
        segment,
        &[
            ZH_PREFERENCE_PREFIX1,
            ZH_PREFERENCE_PREFIX2,
            ZH_PREFERENCE_PREFIX3,
            ZH_PREFERENCE_PREFIX4,
            ZH_PREFERENCE_PREFIX5,
            ZH_PREFERENCE_PREFIX6,
            ZH_PREFERENCE_PREFIX7,
            ZH_PREFERENCE_PREFIX8,
            ZH_PREFERENCE_PREFIX9,
            "i like ",
            "i prefer ",
            "i usually ",
            "i often use ",
            "please prioritize ",
            "default to ",
            "avoid ",
            "do not use ",
            "don't use ",
        ],
    )?;
    let cleaned = clean_statement(&value);
    if cleaned.is_empty() || looks_like_question(&cleaned) {
        return None;
    }
    Some(build_candidate(
        "preference",
        &format!("preference::{}", stable_hash(&format!("pref:{segment}"))),
        &format!("{TITLE_USER_PREFERENCE}{}", truncate_chars(&cleaned, 48)),
        segment,
        vec!["preference"],
        "core",
        0.76,
        0.82,
    ))
}

fn extract_explicit_memory_note(segment: &str) -> Option<ExtractionCandidate> {
    if !has_memory_directive(segment) || looks_like_question(segment) {
        return None;
    }
    let cleaned = clean_statement(
        &strip_prefixes(
            segment,
            &[
                ZH_REMEMBER,
                ZH_REMEMBER_ALT,
                ZH_REMEMBER_NOTE,
                ZH_AFTER_THIS,
                ZH_LATER,
                ZH_NEXT,
                "remember ",
                "please remember ",
                "note this ",
                "from now on ",
                "going forward ",
                "later ",
            ],
        )
        .unwrap_or_else(|| segment.to_string()),
    );
    if cleaned.chars().count() < 6 {
        return None;
    }
    Some(build_candidate(
        "working-note",
        &format!("note::{}", stable_hash(&format!("note:{cleaned}"))),
        &format!("{TITLE_MEMORY_NOTE}{}", truncate_chars(&cleaned, 48)),
        segment,
        vec!["memory-note"],
        "working",
        0.66,
        0.76,
    ))
}

#[allow(clippy::too_many_arguments)]
fn build_candidate(
    category: &str,
    fact_key: &str,
    title: &str,
    raw_segment: &str,
    tags: Vec<&str>,
    tier: &str,
    importance: f64,
    confidence: f64,
) -> ExtractionCandidate {
    let summary = truncate_chars(title, 120);
    ExtractionCandidate {
        category: category.to_string(),
        fact_key: fact_key.to_string(),
        title: truncate_chars(title, 60),
        summary,
        content: truncate_chars(raw_segment, 280),
        tags: tags.into_iter().map(str::to_string).collect(),
        tier: tier.to_string(),
        importance: importance.clamp(0.0, 1.0),
        confidence: confidence.clamp(0.0, 1.0),
    }
}

fn split_segments(text: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        let is_split = matches!(
            ch,
            '\n' | '\r' | '。' | '！' | '？' | '!' | '?' | ';' | '；' | ',' | '，'
        );
        if is_split {
            let cleaned = clean_statement(&current);
            if is_viable_segment(&cleaned) {
                segments.push(cleaned);
            }
            current.clear();
            continue;
        }
        current.push(ch);
    }
    let cleaned = clean_statement(&current);
    if is_viable_segment(&cleaned) {
        segments.push(cleaned);
    }
    segments
}

fn is_viable_segment(segment: &str) -> bool {
    let chars = segment.chars().count();
    (4..=MAX_SEGMENT_CHARS).contains(&chars)
}

fn normalize_sentence(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

fn clean_statement(text: &str) -> String {
    text.trim_matches(|ch: char| {
        matches!(
            ch,
            ' ' | '"'
                | '\''
                | '“'
                | '”'
                | '‘'
                | '’'
                | ':'
                | '：'
                | '。'
                | '！'
                | '？'
                | '!'
                | '?'
                | '，'
                | ','
                | ';'
                | '；'
                | '.'
        )
    })
    .trim()
    .to_string()
}

fn strip_prefixes(text: &str, prefixes: &[&str]) -> Option<String> {
    let trimmed = text.trim();
    let lower = trimmed.to_lowercase();
    for prefix in prefixes {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return Some(rest.to_string());
        }
        let prefix_lower = prefix.to_lowercase();
        if lower.starts_with(&prefix_lower) {
            return Some(trimmed.chars().skip(prefix.chars().count()).collect());
        }
    }
    None
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    let lower = text.to_lowercase();
    needles
        .iter()
        .any(|item| lower.contains(&item.to_lowercase()))
}

fn has_memory_directive(text: &str) -> bool {
    contains_any(
        text,
        &[
            ZH_REMEMBER,
            ZH_REMEMBER_NOTE,
            ZH_AFTER_THIS,
            ZH_LATER,
            ZH_DEFAULT,
            ZH_PREFER,
            ZH_USE,
            ZH_DONT_USE,
            ZH_DONT_USE_ALT,
            ZH_ALWAYS,
            ZH_TOTAL_ALWAYS,
            "remember",
            "default",
            "prefer",
            "from now on",
            "going forward",
            "always",
            "avoid",
            "please use",
        ],
    )
}

fn looks_like_question(text: &str) -> bool {
    contains_any(
        text,
        &["?", ZH_QM, ZH_HOW, ZH_HOW_ALT, "how ", "what ", "why "],
    )
}

fn stable_hash(text: &str) -> String {
    let mut hasher = DefaultHasher::new();
    text.trim().to_lowercase().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn is_same_candidate(existing: &MemoryFragmentRecord, candidate: &ExtractionCandidate) -> bool {
    existing.summary_l1.trim() == candidate.summary.trim()
        && existing.content_l2.trim() == candidate.content.trim()
        && existing.category.trim() == candidate.category.trim()
}

fn should_protect_existing(record: &MemoryFragmentRecord) -> bool {
    record.confirmed_by_user
        || record.pinned
        || is_invalidated(record)
        || record.source_type.trim() != AUTO_SOURCE_TYPE
}

fn is_invalidated(record: &MemoryFragmentRecord) -> bool {
    record.invalidated_at.unwrap_or(0.0) > 0.0 || record.status.trim() == "invalidated"
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
#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::memory_fragments::{MemoryFragmentInput, MemoryFragmentStore};
    use crate::storage::{SqliteStorage, StorageBackend};
    use std::sync::Arc;
    use tempfile::tempdir;

    #[test]
    fn extract_candidates_detects_response_preference() {
        let items = extract_candidates("以后请用中文回复，回答尽量简洁，不要用表格。");
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].fact_key, "constraint::reply_language");
        assert_eq!(items[1].fact_key, "constraint::response_style");
        assert_eq!(items[2].fact_key, "constraint::response_format");
    }

    #[test]
    fn extract_candidates_collects_multiple_preferences_from_one_segment() {
        let items = extract_candidates("以后请用中文回复并尽量简洁且不要表格");
        let fact_keys = items
            .into_iter()
            .map(|item| item.fact_key)
            .collect::<Vec<_>>();
        assert_eq!(
            fact_keys,
            vec![
                "constraint::reply_language".to_string(),
                "constraint::response_style".to_string(),
                "constraint::response_format".to_string(),
            ]
        );
    }

    #[test]
    fn capture_turn_creates_and_updates_auto_memory() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("memory-auto.db");
        let storage: Arc<dyn StorageBackend> =
            Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        storage.ensure_initialized().expect("init storage");
        let service = MemoryAutoExtractService::new(storage.clone());

        let first = service
            .capture_turn(
                "u1",
                Some("agent-demo"),
                "s1",
                Some("1"),
                "以后请用中文回复，回答尽量简洁。",
                "好的。",
            )
            .expect("first capture");
        assert_eq!(
            first,
            MemoryAutoExtractOutcome {
                created: 2,
                updated: 0,
                skipped: 0,
            }
        );

        let second = service
            .capture_turn(
                "u1",
                Some("agent-demo"),
                "s1",
                Some("2"),
                "以后请用中文回复，回答尽量详细。",
                "收到。",
            )
            .expect("second capture");
        assert_eq!(
            second,
            MemoryAutoExtractOutcome {
                created: 0,
                updated: 1,
                skipped: 1,
            }
        );

        let items = storage
            .list_memory_fragments("u1", "agent-demo")
            .expect("list memory fragments");
        assert!(items
            .iter()
            .any(|item| item.fact_key == "constraint::reply_language"));
        assert!(items.iter().any(|item| {
            item.fact_key == "constraint::response_style" && item.summary_l1.contains("详细")
        }));
    }

    #[test]
    fn capture_turn_keeps_manual_memory_intact() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("memory-manual.db");
        let storage: Arc<dyn StorageBackend> =
            Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        storage.ensure_initialized().expect("init storage");
        let service = MemoryAutoExtractService::new(storage.clone());
        let fragment_store = MemoryFragmentStore::new(storage.clone());
        let manual = fragment_store
            .save_fragment(
                "u1",
                Some("agent-demo"),
                MemoryFragmentInput {
                    source_type: Some("manual".to_string()),
                    category: Some("response-preference".to_string()),
                    title_l0: Some(TITLE_REPLY_ZH.to_string()),
                    summary_l1: Some(TITLE_REPLY_ZH.to_string()),
                    content_l2: Some(TITLE_REPLY_ZH.to_string()),
                    fact_key: Some("constraint::reply_language".to_string()),
                    ..Default::default()
                },
            )
            .expect("save manual fragment");

        let outcome = service
            .capture_turn(
                "u1",
                Some("agent-demo"),
                "s1",
                Some("3"),
                "Please reply in English from now on.",
                "Sure.",
            )
            .expect("capture turn");

        assert_eq!(
            outcome,
            MemoryAutoExtractOutcome {
                created: 0,
                updated: 0,
                skipped: 1,
            }
        );

        let stored = storage
            .get_memory_fragment("u1", "agent-demo", &manual.memory_id)
            .expect("get fragment")
            .expect("fragment exists");
        assert_eq!(stored.source_type, "manual");
        assert_eq!(stored.summary_l1, TITLE_REPLY_ZH.to_string());
    }
}
