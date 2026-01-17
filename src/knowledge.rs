// 知识库模块：解析 Markdown、缓存章节，并支持 LLM + 词面回退检索。
use crate::config::{Config, KnowledgeBaseConfig, LlmModelConfig};
use crate::i18n;
use crate::llm::{build_llm_client, is_llm_configured, ChatMessage};
use anyhow::Result;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use tokio::sync::Mutex;
use tracing::error;
use walkdir::WalkDir;

const DEFAULT_MAX_DOCUMENTS: usize = 5;
const DEFAULT_CANDIDATE_LIMIT: usize = 80;

#[derive(Debug, Clone)]
pub struct KnowledgeSection {
    pub document: String,
    pub section_path: Vec<String>,
    pub content: String,
    pub code: String,
}

impl KnowledgeSection {
    pub fn identifier(&self) -> String {
        let mut parts = Vec::new();
        parts.push(self.document.clone());
        parts.extend(self.section_path.clone());
        let labels = full_text_labels();
        let mut cleaned: Vec<String> = Vec::new();
        for part in parts {
            let name = part.trim();
            if name.is_empty() {
                continue;
            }
            let mut normalized = name.to_string();
            if labels.contains(name) {
                normalized = i18n::t("knowledge.section.full_text");
            }
            if cleaned
                .last()
                .map(|last| last == &normalized)
                .unwrap_or(false)
            {
                continue;
            }
            cleaned.push(normalized);
        }
        if cleaned.len() >= 2 {
            let first = cleaned[0].clone();
            if cleaned[1].starts_with(&first) {
                cleaned.remove(0);
            }
        }
        if cleaned.is_empty() {
            self.document.clone()
        } else {
            cleaned.join(" - ")
        }
    }

    pub fn preview(&self) -> String {
        let plain = strip_markdown(&self.content);
        let trimmed = plain.trim();
        if trimmed.is_empty() {
            return String::new();
        }
        let mut output = String::new();
        for (idx, ch) in trimmed.chars().enumerate() {
            if idx >= 80 {
                break;
            }
            output.push(ch);
        }
        if trimmed.chars().count() > 80 {
            output.push_str("...");
        }
        output
    }
}

#[derive(Debug, Clone)]
pub struct KnowledgeDocument {
    pub code: String,
    pub name: String,
    pub content: String,
    pub document: String,
    pub section_path: Vec<String>,
    pub score: Option<f64>,
    pub reason: Option<String>,
}

impl KnowledgeDocument {
    pub fn to_value(&self) -> Value {
        let labels = full_text_labels();
        let normalized_path = self
            .section_path
            .iter()
            .map(|part| {
                if labels.contains(part) {
                    i18n::t("knowledge.section.full_text")
                } else {
                    part.clone()
                }
            })
            .collect::<Vec<_>>();
        let mut payload = json!({
            "code": self.code,
            "name": self.name,
            "content": self.content,
            "document": self.document,
            "section_path": normalized_path,
        });
        if let Some(score) = self.score {
            payload["score"] = json!(score);
        }
        if let Some(reason) = &self.reason {
            payload["reason"] = json!(reason);
        }
        payload
    }
}

#[derive(Debug, Clone)]
struct KnowledgeCache {
    sections: Vec<KnowledgeSection>,
}

#[derive(Default)]
struct KnowledgeStore {
    cache: Mutex<HashMap<String, KnowledgeCache>>,
    locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

static STORE: OnceLock<KnowledgeStore> = OnceLock::new();

fn store() -> &'static KnowledgeStore {
    STORE.get_or_init(KnowledgeStore::default)
}

pub fn resolve_llm_config(config: &Config, model_name: Option<&str>) -> Option<LlmModelConfig> {
    let name = model_name
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| config.llm.default.as_str());
    if let Some(model) = config.llm.models.get(name) {
        return Some(model.clone());
    }
    config.llm.models.values().next().cloned()
}

pub fn resolve_knowledge_root(base: &KnowledgeBaseConfig, create: bool) -> Result<PathBuf> {
    let root = PathBuf::from(&base.root);
    if root.as_os_str().is_empty() {
        return Err(anyhow::anyhow!(i18n::t("error.knowledge_root_not_found")));
    }
    if !root.exists() {
        if create {
            std::fs::create_dir_all(&root).map_err(|err| {
                anyhow::anyhow!(i18n::t_with_params(
                    "error.knowledge_root_create_failed",
                    &HashMap::from([
                        ("root".to_string(), base.root.clone()),
                        ("detail".to_string(), err.to_string())
                    ]),
                ))
            })?;
        } else {
            return Err(anyhow::anyhow!(i18n::t("error.knowledge_root_not_found")));
        }
    }
    if !root.is_dir() {
        return Err(anyhow::anyhow!(i18n::t("error.knowledge_root_not_dir")));
    }
    Ok(crate::path_utils::normalize_existing_path(&root))
}

pub async fn refresh_knowledge_cache(base: &KnowledgeBaseConfig) -> Vec<KnowledgeSection> {
    store().refresh(base).await
}

pub async fn query_knowledge_documents(
    query: &str,
    base: &KnowledgeBaseConfig,
    llm_config: Option<&LlmModelConfig>,
    limit: Option<usize>,
    request_logger: Option<&(dyn Fn(Value) + Send + Sync)>,
) -> Vec<KnowledgeDocument> {
    let normalized_query = query.trim();
    if normalized_query.is_empty() {
        return Vec::new();
    }
    let sections = store().get_sections(base, false).await;
    if sections.is_empty() {
        return Vec::new();
    }
    let max_docs = resolve_positive_int(limit, DEFAULT_MAX_DOCUMENTS);
    let candidates =
        select_candidate_sections(&sections, normalized_query, DEFAULT_CANDIDATE_LIMIT);
    let prompt = build_system_prompt(max_docs);
    let question = build_question(&base.name, normalized_query, &candidates);
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: json!(prompt),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        },
        ChatMessage {
            role: "user".to_string(),
            content: json!(question.clone()),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        },
    ];
    if let Some(logger) = request_logger {
        let payload = build_llm_payload(&messages, llm_config);
        let base_url = llm_config
            .and_then(|config| config.base_url.clone())
            .unwrap_or_default();
        logger(json!({
            "knowledge_base": base.name.clone(),
            "query": normalized_query.to_string(),
            "limit": max_docs,
            "candidate_count": candidates.len(),
            "payload": payload,
            "base_url": base_url,
        }));
    }

    let reply = match llm_config {
        Some(config) if is_llm_configured(config) => {
            let client = build_llm_client(config, reqwest::Client::new());
            match client.complete(&messages).await {
                Ok(response) => response.content,
                Err(_) => {
                    return fallback_documents(&candidates, max_docs);
                }
            }
        }
        _ => {
            return fallback_documents(&candidates, max_docs);
        }
    };

    let structured = extract_structured_documents(&reply);
    let documents = materialize_documents(&structured, &sections, max_docs);
    if !documents.is_empty() {
        return documents;
    }
    fallback_documents(&candidates, max_docs)
}

impl KnowledgeStore {
    async fn get_sections(
        &self,
        base: &KnowledgeBaseConfig,
        refresh: bool,
    ) -> Vec<KnowledgeSection> {
        let base_name = base.name.trim();
        let root_path = base.root.trim();
        if base_name.is_empty() || root_path.is_empty() {
            return Vec::new();
        }
        let key = format!("{base_name}::{root_path}");
        if !refresh {
            if let Some(cached) = self.cache.lock().await.get(&key) {
                return cached.sections.clone();
            }
        }
        let lock = {
            let mut locks = self.locks.lock().await;
            locks
                .entry(key.clone())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _guard = lock.lock().await;
        if !refresh {
            if let Some(cached) = self.cache.lock().await.get(&key) {
                return cached.sections.clone();
            }
        }
        let sections = load_sections(root_path).await;
        let cache_entry = KnowledgeCache {
            sections: sections.clone(),
        };
        self.cache.lock().await.insert(key, cache_entry);
        sections
    }

    async fn refresh(&self, base: &KnowledgeBaseConfig) -> Vec<KnowledgeSection> {
        self.get_sections(base, true).await
    }
}

async fn load_sections(root: &str) -> Vec<KnowledgeSection> {
    let root_path = PathBuf::from(root);
    if !root_path.exists() || !root_path.is_dir() {
        return Vec::new();
    }
    tokio::task::spawn_blocking(move || load_knowledge_sections(&root_path))
        .await
        .unwrap_or_default()
}

fn load_knowledge_sections(root: &Path) -> Vec<KnowledgeSection> {
    let mut sections = Vec::new();
    let mut files = Vec::new();
    for entry in WalkDir::new(root).into_iter().filter_map(|item| item.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()).unwrap_or("") != "md" {
            continue;
        }
        files.push(path.to_path_buf());
    }
    files.sort();
    for path in files {
        sections.extend(parse_markdown_sections(&path));
    }
    for (idx, section) in sections.iter_mut().enumerate() {
        section.code = format!("K{:04}", idx + 1);
    }
    sections
}

fn parse_markdown_sections(path: &Path) -> Vec<KnowledgeSection> {
    let Some(text) = read_markdown_text(path) else {
        return Vec::new();
    };
    let text = text.replace('\u{feff}', "");
    let document_content = text.trim().to_string();
    let mut sections = Vec::new();
    let mut current_h1: Option<String> = None;
    let mut buffer: Vec<String> = Vec::new();

    let flush = |sections: &mut Vec<KnowledgeSection>,
                 buffer: &mut Vec<String>,
                 current_h1: &Option<String>| {
        if buffer.is_empty() || current_h1.is_none() {
            buffer.clear();
            return;
        }
        let content = buffer.join("\n").trim().to_string();
        buffer.clear();
        if content.is_empty() {
            return;
        }
        let section_path = vec![current_h1.clone().unwrap_or_default()];
        sections.push(KnowledgeSection {
            document: path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("")
                .to_string(),
            section_path,
            content,
            code: String::new(),
        });
    };

    let heading_re = heading_regex();
    for line in text.lines() {
        if let Some(regex) = heading_re {
            if let Some(caps) = regex.captures(line.trim()) {
                flush(&mut sections, &mut buffer, &current_h1);
                current_h1 = Some(caps[1].trim().to_string());
                buffer.clear();
                continue;
            }
        }
        buffer.push(line.to_string());
    }
    flush(&mut sections, &mut buffer, &current_h1);
    if sections.is_empty() && !document_content.is_empty() {
        sections.push(KnowledgeSection {
            document: path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("")
                .to_string(),
            section_path: vec![i18n::t("knowledge.section.full_text")],
            content: document_content.clone(),
            code: String::new(),
        });
    }
    sections
}

fn read_markdown_text(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    if let Ok(text) = String::from_utf8(bytes.clone()) {
        return Some(text);
    }
    let (cow, _, _) = encoding_rs::GBK.decode(&bytes);
    Some(cow.into_owned())
}

fn build_system_prompt(limit: usize) -> String {
    let language = i18n::get_language();
    let template = if language.starts_with("en") {
        KNOWLEDGE_PROMPT_EN
    } else {
        KNOWLEDGE_PROMPT_ZH
    };
    template.replace("{limit}", &limit.to_string())
}

fn build_llm_payload(messages: &[ChatMessage], llm_config: Option<&LlmModelConfig>) -> Value {
    let mut payload = json!({
        "model": llm_config.and_then(|config| config.model.clone()).unwrap_or_default(),
        "messages": messages,
        "stream": false,
    });
    if let Some(config) = llm_config {
        if let Some(temp) = config.temperature {
            payload["temperature"] = json!(temp);
        }
        if let Some(max_output) = config.max_output {
            if max_output > 0 {
                payload["max_tokens"] = json!(max_output);
            }
        }
    }
    payload
}

fn build_question(base_name: &str, query: &str, candidates: &[KnowledgeSection]) -> String {
    let labels = if i18n::get_language().starts_with("en") {
        QUESTION_LABELS_EN
    } else {
        QUESTION_LABELS_ZH
    };
    let mut lines = Vec::new();
    for section in candidates {
        let preview = section.preview();
        if preview.is_empty() {
            lines.push(format!("- [{}] {}", section.code, section.identifier()));
        } else {
            lines.push(format!(
                "- [{}] {} | {}",
                section.code,
                section.identifier(),
                preview
            ));
        }
    }
    let listing = if lines.is_empty() {
        labels.empty.to_string()
    } else {
        lines.join("\n")
    };
    format!(
        "{base_label}\n{base_name}\n\n{query_label}\n{query}\n\n{candidate_label}\n{listing}\n\n{footer}",
        base_label = labels.base_name,
        query_label = labels.user_query,
        candidate_label = labels.candidates,
        footer = labels.footer
    )
}

fn extract_structured_documents(reply: &str) -> Vec<Value> {
    let Some(regex) = knowledge_block_regex() else {
        return Vec::new();
    };
    let Some(caps) = regex.captures(reply) else {
        return Vec::new();
    };
    let block = caps.get(1).map(|m| m.as_str().trim()).unwrap_or("");
    if block.is_empty() {
        return Vec::new();
    }
    let parsed = match serde_json::from_str::<Value>(block) {
        Ok(value) => value,
        Err(_) => return Vec::new(),
    };
    parsed
        .get("documents")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default()
}

fn materialize_documents(
    structured_docs: &[Value],
    sections: &[KnowledgeSection],
    limit: usize,
) -> Vec<KnowledgeDocument> {
    if structured_docs.is_empty() {
        return Vec::new();
    }
    let mut by_identifier: HashMap<String, KnowledgeSection> = HashMap::new();
    let mut by_code: HashMap<String, KnowledgeSection> = HashMap::new();
    for section in sections {
        by_identifier.insert(section.identifier(), section.clone());
        if !section.code.trim().is_empty() {
            by_code.insert(section.code.clone(), section.clone());
        }
    }
    let mut resolved = Vec::new();
    for item in structured_docs {
        if resolved.len() >= limit {
            break;
        }
        let code = item
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_uppercase();
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let section = if !code.is_empty() {
            by_code.get(&code).cloned()
        } else {
            None
        }
        .or_else(|| resolve_section(&name, &by_identifier));
        let Some(section) = section else {
            continue;
        };
        resolved.push(KnowledgeDocument {
            code: section.code.clone(),
            name: section.identifier(),
            content: section.content.clone(),
            document: section.document.clone(),
            section_path: section.section_path.clone(),
            score: item.get("score").and_then(Value::as_f64),
            reason: item
                .get("reason")
                .and_then(Value::as_str)
                .map(|text| text.trim().to_string())
                .filter(|text| !text.is_empty()),
        });
    }
    resolved
}

fn fallback_documents(candidates: &[KnowledgeSection], limit: usize) -> Vec<KnowledgeDocument> {
    let reason = i18n::t("knowledge.fallback_reason");
    candidates
        .iter()
        .take(limit)
        .map(|section| KnowledgeDocument {
            code: section.code.clone(),
            name: section.identifier(),
            content: section.content.clone(),
            document: section.document.clone(),
            section_path: section.section_path.clone(),
            score: None,
            reason: Some(reason.clone()),
        })
        .collect()
}

fn resolve_section(
    name: &str,
    candidates: &HashMap<String, KnowledgeSection>,
) -> Option<KnowledgeSection> {
    if name.is_empty() {
        return None;
    }
    if let Some(section) = candidates.get(name) {
        return Some(section.clone());
    }
    let matches = candidates
        .iter()
        .filter(|(key, _)| key.ends_with(name))
        .map(|(_, section)| section.clone())
        .collect::<Vec<_>>();
    if matches.len() == 1 {
        return matches.into_iter().next();
    }
    None
}

fn select_candidate_sections(
    sections: &[KnowledgeSection],
    query: &str,
    limit: usize,
) -> Vec<KnowledgeSection> {
    if sections.is_empty() {
        return Vec::new();
    }
    let normalized_query = query.to_lowercase();
    let tokens = extract_tokens(query);
    if tokens.is_empty() {
        return sections.iter().take(limit).cloned().collect();
    }
    let mut scored: Vec<(i32, KnowledgeSection)> = Vec::new();
    for section in sections {
        let score = score_section(section, &normalized_query, &tokens);
        if score > 0 {
            scored.push((score, section.clone()));
        }
    }
    if scored.is_empty() {
        return sections.iter().take(limit).cloned().collect();
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.code.cmp(&b.1.code)));
    scored
        .into_iter()
        .take(limit)
        .map(|(_, section)| section)
        .collect()
}

fn extract_tokens(query: &str) -> Vec<String> {
    let lowered = query.to_lowercase();
    let mut tokens = Vec::new();
    if let Some(regex) = ascii_token_regex() {
        for mat in regex.find_iter(&lowered) {
            let text = mat.as_str().trim();
            if text.len() >= 2 {
                tokens.push(text.to_string());
            }
        }
    }
    for ch in query.chars() {
        if is_chinese(ch) {
            tokens.push(ch.to_string());
        }
    }
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for token in tokens {
        if deduped.len() >= 24 {
            break;
        }
        if seen.insert(token.clone()) {
            deduped.push(token);
        }
    }
    deduped
}

fn score_section(section: &KnowledgeSection, normalized_query: &str, tokens: &[String]) -> i32 {
    let text = format!("{}\n{}", section.identifier(), section.content).to_lowercase();
    let mut score = 0;
    if !normalized_query.is_empty() && text.contains(normalized_query) {
        score += 4;
    }
    for token in tokens {
        if !token.is_empty() && text.contains(&token.to_lowercase()) {
            score += 1;
        }
    }
    score
}

fn resolve_positive_int(value: Option<usize>, default: usize) -> usize {
    match value {
        Some(val) if val > 0 => val,
        _ => default.max(1),
    }
}

fn strip_markdown(content: &str) -> String {
    let cleaned = match markdown_clean_regex() {
        Some(regex) => regex.replace_all(content, "").to_string(),
        None => content.to_string(),
    };
    let collapsed = match whitespace_regex() {
        Some(regex) => regex.replace_all(&cleaned, " ").to_string(),
        None => cleaned,
    };
    collapsed.trim().to_string()
}

fn is_chinese(ch: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&ch)
}

fn heading_regex() -> Option<&'static Regex> {
    static REGEX: OnceLock<Option<Regex>> = OnceLock::new();
    REGEX
        .get_or_init(|| compile_regex(r"^#\\s+(.+?)\\s*$", "heading"))
        .as_ref()
}

fn markdown_clean_regex() -> Option<&'static Regex> {
    static REGEX: OnceLock<Option<Regex>> = OnceLock::new();
    REGEX
        .get_or_init(|| compile_regex(r"[#>`*_`]+", "markdown_clean"))
        .as_ref()
}

fn whitespace_regex() -> Option<&'static Regex> {
    static REGEX: OnceLock<Option<Regex>> = OnceLock::new();
    REGEX
        .get_or_init(|| compile_regex(r"\\s+", "whitespace"))
        .as_ref()
}

fn ascii_token_regex() -> Option<&'static Regex> {
    static REGEX: OnceLock<Option<Regex>> = OnceLock::new();
    REGEX
        .get_or_init(|| compile_regex(r"[a-z0-9]+", "ascii_token"))
        .as_ref()
}

fn knowledge_block_regex() -> Option<&'static Regex> {
    static REGEX: OnceLock<Option<Regex>> = OnceLock::new();
    REGEX
        .get_or_init(|| compile_regex(r"(?s)<knowledge>(.*?)</knowledge>", "knowledge_block"))
        .as_ref()
}

fn compile_regex(pattern: &str, label: &str) -> Option<Regex> {
    match Regex::new(pattern) {
        Ok(regex) => Some(regex),
        Err(err) => {
            error!("invalid knowledge regex {label}: {err}");
            None
        }
    }
}

fn full_text_labels() -> &'static HashSet<String> {
    static LABELS: OnceLock<HashSet<String>> = OnceLock::new();
    LABELS.get_or_init(|| {
        i18n::get_known_prefixes("knowledge.section.full_text")
            .into_iter()
            .collect()
    })
}

struct QuestionLabels {
    base_name: &'static str,
    user_query: &'static str,
    candidates: &'static str,
    empty: &'static str,
    footer: &'static str,
}

const QUESTION_LABELS_ZH: QuestionLabels = QuestionLabels {
    base_name: "【知识库名称】",
    user_query: "【用户提问】",
    candidates: "【候选知识点列表】",
    empty: "- （暂无知识点）",
    footer: "请按要求返回 JSON 结果。",
};

const QUESTION_LABELS_EN: QuestionLabels = QuestionLabels {
    base_name: "[Knowledge Base]",
    user_query: "[User Question]",
    candidates: "[Candidate Knowledge Items]",
    empty: "- (No items available)",
    footer: "Return the JSON result as required.",
};

const KNOWLEDGE_PROMPT_ZH: &str = "你是一名字面知识库检索助手，需要根据用户提问在给定的知识点列表中挑选最相关的内容。\n\
请严格按照以下要求输出：\n\
1. 每个知识点都对应唯一编号（如 K0001），请优先依赖编号定位章节。\n\
2. 最多返回{limit}个知识点，可根据相关度筛选；即便相关度较低，也尽量返回2-3条最接近的知识点，避免空列表。\n\
3. 输出必须使用 <knowledge></knowledge> 包裹 JSON，字段为 documents(List)。\n\
4. documents 中的每个对象需包含 code、name、score(0~1) 与 reason(简述命中原因)。\n\
5. 未命中时也要输出空数组，切勿输出 JSON 之外的多余文字。";

const KNOWLEDGE_PROMPT_EN: &str = "You are a knowledge-base retrieval assistant. Select the most relevant items from the list based on the user query.\n\
Follow these requirements strictly:\n\
1. Each knowledge item has a unique code (e.g., K0001). Prefer matching by code.\n\
2. Return at most {limit} items. Even if relevance is low, try to return 2-3 closest items to avoid an empty list.\n\
3. Output must be JSON wrapped by <knowledge></knowledge>, with field documents (List).\n\
4. Each document item must include code, name, score(0~1), and reason (brief).\n\
5. If nothing matches, return an empty array only, and do not output extra text outside JSON.";
