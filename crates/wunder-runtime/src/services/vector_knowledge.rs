use crate::config::{Config, KnowledgeBaseConfig, KnowledgeBaseType, LlmModelConfig};
use crate::i18n;
use crate::llm::{embed_texts, is_embedding_model};
use crate::path_utils::normalize_existing_path;
use crate::storage::{
    StorageBackend, VectorChunkEmbeddingRecord, VectorDocumentRecord, VectorDocumentSummaryRecord,
};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tracing::{info, warn};
use uuid::Uuid;

const VECTOR_ROOT_DIR: &str = "config/data/vector_knowledge";
const VECTOR_ROOT_DIR_ENV: &str = "WUNDER_VECTOR_KNOWLEDGE_ROOT";
const VECTOR_SHARED_DIR: &str = "shared";
const VECTOR_USERS_DIR: &str = "users";
const VECTOR_DOCS_DIR: &str = "documents";
const VECTOR_DOC_EXT: &str = ".md";
const VECTOR_LITERAL_FALLBACK_CANDIDATE_LIMIT: usize = 200;
const VECTOR_META_EXT: &str = ".json";

const DEFAULT_CHUNK_SIZE: usize = 800;
const DEFAULT_CHUNK_OVERLAP: usize = 100;
const DEFAULT_TOP_K: usize = 5;
const VECTOR_SEARCH_CANDIDATE_LIMIT: i64 = 2048;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorDocumentMeta {
    pub doc_id: String,
    pub name: String,
    pub embedding_model: String,
    pub chunk_size: usize,
    pub chunk_overlap: usize,
    pub chunk_count: usize,
    pub status: String,
    pub created_at: f64,
    pub updated_at: f64,
    pub chunks: Vec<VectorChunkMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorChunkMeta {
    pub index: usize,
    pub start: usize,
    pub end: usize,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VectorDocumentSummary {
    pub doc_id: String,
    pub name: String,
    pub status: String,
    pub chunk_count: usize,
    pub embedding_model: String,
    pub updated_at: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct VectorChunkPreview {
    pub index: usize,
    pub start: usize,
    pub end: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk_id: Option<String>,
    pub preview: String,
    pub content: String,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct VectorChunk {
    pub index: usize,
    pub start: usize,
    pub end: usize,
    pub content: String,
    pub chunk_id: String,
}

#[derive(Debug, Clone)]
pub struct VectorSearchHit {
    pub doc_id: String,
    pub doc_name: String,
    pub chunk_index: usize,
    pub start: usize,
    pub end: usize,
    pub content: String,
    pub embedding_model: String,
    pub score: Option<f64>,
}

pub fn ensure_vector_base_type(base: &KnowledgeBaseConfig) -> Result<()> {
    if base.base_type() != KnowledgeBaseType::Vector {
        return Err(anyhow!(i18n::t("error.vector_knowledge_required")));
    }
    Ok(())
}

pub fn ensure_vector_base_config(base: &KnowledgeBaseConfig) -> Result<()> {
    ensure_vector_base_type(base)?;
    if base
        .embedding_model
        .as_deref()
        .map(|value| value.trim().is_empty())
        .unwrap_or(true)
    {
        return Err(anyhow!(i18n::t("error.embedding_model_required")));
    }
    Ok(())
}

pub fn resolve_embedding_model(config: &Config, name: &str) -> Result<LlmModelConfig> {
    let model_name = name.trim();
    if model_name.is_empty() {
        return Err(anyhow!(i18n::t("error.embedding_model_required")));
    }
    let Some(model) = config.llm.models.get(model_name) else {
        return Err(anyhow!(i18n::t("error.embedding_model_not_found")));
    };
    if !is_embedding_model(model) {
        return Err(anyhow!(i18n::t("error.embedding_model_not_found")));
    }
    Ok(model.clone())
}

pub fn resolve_vector_root(
    owner_id: Option<&str>,
    base_name: &str,
    create: bool,
) -> Result<PathBuf> {
    let cleaned = base_name.trim();
    if cleaned.is_empty() {
        return Err(anyhow!(i18n::t("error.knowledge_base_name_required")));
    }
    if cleaned.contains('/') || cleaned.contains('\\') || cleaned.contains("..") {
        return Err(anyhow!(i18n::t("error.knowledge_name_invalid_path")));
    }
    let root = resolve_vector_root_dir();
    let owner_root = match owner_id {
        Some(user_id) => root.join(VECTOR_USERS_DIR).join(safe_user_id(user_id)),
        None => root.join(VECTOR_SHARED_DIR),
    };
    let target = owner_root.join(cleaned);
    let _ = create;
    Ok(target)
}

pub fn resolve_owner_key(owner_id: Option<&str>) -> String {
    match owner_id {
        Some(value) => {
            let cleaned = safe_user_id(value);
            if cleaned.trim().is_empty() {
                "shared".to_string()
            } else {
                cleaned
            }
        }
        None => "shared".to_string(),
    }
}

pub fn resolve_vector_documents_dir(root: &Path, create: bool) -> Result<PathBuf> {
    let docs = root.join(VECTOR_DOCS_DIR);
    if create {
        std::fs::create_dir_all(&docs)?;
    }
    Ok(docs)
}

pub fn build_doc_id(owner_id: Option<&str>, base_name: &str, filename: &str) -> String {
    let owner = resolve_owner_key(owner_id);
    let key = format!("{owner}::{base_name}::{filename}");
    Uuid::new_v5(&Uuid::NAMESPACE_OID, key.as_bytes()).to_string()
}

pub fn build_chunk_id(doc_id: &str, index: usize) -> String {
    let key = format!("{doc_id}::{index}");
    Uuid::new_v5(&Uuid::NAMESPACE_OID, key.as_bytes()).to_string()
}

pub fn resolve_chunk_size(base: &KnowledgeBaseConfig) -> usize {
    let value = base.chunk_size.unwrap_or(DEFAULT_CHUNK_SIZE);
    value.max(100)
}

pub fn resolve_chunk_overlap(base: &KnowledgeBaseConfig) -> usize {
    let value = base.chunk_overlap.unwrap_or(DEFAULT_CHUNK_OVERLAP);
    value.min(resolve_chunk_size(base).saturating_sub(1))
}

pub fn resolve_top_k(base: &KnowledgeBaseConfig) -> usize {
    base.top_k.unwrap_or(DEFAULT_TOP_K).max(1)
}

pub fn split_text_into_chunks(
    text: &str,
    chunk_size: usize,
    chunk_overlap: usize,
    doc_id: &str,
) -> Vec<VectorChunk> {
    if text.trim().is_empty() {
        return Vec::new();
    }
    let chars: Vec<char> = text.chars().collect();
    let total = chars.len();
    if total == 0 {
        return Vec::new();
    }
    let size = chunk_size.max(1);
    let overlap = chunk_overlap.min(size.saturating_sub(1));
    let mut chunks = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while start < total {
        let end = (start + size).min(total);
        let content: String = chars[start..end].iter().collect();
        if !content.trim().is_empty() {
            chunks.push(VectorChunk {
                index,
                start,
                end,
                content,
                chunk_id: build_chunk_id(doc_id, index),
            });
            index += 1;
        }
        if end >= total {
            break;
        }
        start = end.saturating_sub(overlap);
    }
    chunks
}

pub fn build_chunk_meta(chunks: &[VectorChunk]) -> Vec<VectorChunkMeta> {
    chunks
        .iter()
        .map(|chunk| VectorChunkMeta {
            index: chunk.index,
            start: chunk.start,
            end: chunk.end,
            status: Some("pending".to_string()),
            content: None,
        })
        .collect()
}

pub fn resolve_chunk_status(meta: &VectorDocumentMeta, chunk: &VectorChunkMeta) -> String {
    if let Some(status) = chunk.status.as_deref().map(|value| value.trim()) {
        if !status.is_empty() {
            return status.to_string();
        }
    }
    if meta.status.eq_ignore_ascii_case("ready") {
        "embedded".to_string()
    } else {
        "pending".to_string()
    }
}

pub fn resolve_chunk_content(content_chars: &[char], chunk: &VectorChunkMeta) -> String {
    if let Some(custom) = chunk
        .content
        .as_deref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        return custom.to_string();
    }
    let start = chunk.start.min(content_chars.len());
    let end = chunk.end.min(content_chars.len());
    content_chars[start..end].iter().collect()
}

fn is_chunk_deleted(chunk: &VectorChunkMeta) -> bool {
    matches!(chunk.status.as_deref(), Some("deleted"))
}

fn build_chunk_from_meta(
    content_chars: &[char],
    doc_id: &str,
    chunk: &VectorChunkMeta,
) -> VectorChunk {
    VectorChunk {
        index: chunk.index,
        start: chunk.start,
        end: chunk.end,
        content: resolve_chunk_content(content_chars, chunk),
        chunk_id: build_chunk_id(doc_id, chunk.index),
    }
}

pub fn refresh_document_meta(meta: &mut VectorDocumentMeta) {
    let mut active = 0;
    let mut all_embedded = true;
    for chunk in &meta.chunks {
        let status = resolve_chunk_status(meta, chunk);
        if status == "deleted" {
            continue;
        }
        active += 1;
        if status != "embedded" {
            all_embedded = false;
        }
    }
    meta.chunk_count = active;
    meta.status = if active > 0 && all_embedded {
        "ready".to_string()
    } else {
        "pending".to_string()
    };
    meta.updated_at = now_ts();
}

pub fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

fn build_vector_record(
    owner_id: &str,
    base_name: &str,
    meta: &VectorDocumentMeta,
    content: &str,
) -> Result<VectorDocumentRecord> {
    let chunks_json = serde_json::to_string(&meta.chunks)?;
    Ok(VectorDocumentRecord {
        doc_id: meta.doc_id.clone(),
        owner_id: owner_id.to_string(),
        base_name: base_name.to_string(),
        doc_name: meta.name.clone(),
        embedding_model: meta.embedding_model.clone(),
        chunk_size: meta.chunk_size as i64,
        chunk_overlap: meta.chunk_overlap as i64,
        chunk_count: meta.chunk_count as i64,
        status: meta.status.clone(),
        created_at: meta.created_at,
        updated_at: meta.updated_at,
        content: content.to_string(),
        chunks_json,
    })
}

fn build_meta_from_record(record: &VectorDocumentRecord) -> Result<VectorDocumentMeta> {
    let chunks =
        serde_json::from_str::<Vec<VectorChunkMeta>>(&record.chunks_json).unwrap_or_default();
    Ok(VectorDocumentMeta {
        doc_id: record.doc_id.clone(),
        name: record.doc_name.clone(),
        embedding_model: record.embedding_model.clone(),
        chunk_size: record.chunk_size as usize,
        chunk_overlap: record.chunk_overlap as usize,
        chunk_count: record.chunk_count as usize,
        status: record.status.clone(),
        created_at: record.created_at,
        updated_at: record.updated_at,
        chunks,
    })
}

fn build_summary_from_record(record: &VectorDocumentSummaryRecord) -> VectorDocumentSummary {
    VectorDocumentSummary {
        doc_id: record.doc_id.clone(),
        name: record.doc_name.clone(),
        status: record.status.clone(),
        chunk_count: record.chunk_count as usize,
        embedding_model: record.embedding_model.clone(),
        updated_at: record.updated_at,
    }
}

fn vector_to_json(vector: &[f32]) -> Result<String> {
    Ok(serde_json::to_string(vector)?)
}

fn parse_vector_json(raw: &str) -> Option<Vec<f32>> {
    let vector = serde_json::from_str::<Vec<f32>>(raw).ok()?;
    if vector.is_empty() || vector.iter().any(|value| !value.is_finite()) {
        return None;
    }
    Some(vector)
}

fn cosine_similarity(left: &[f32], right: &[f32]) -> Option<f64> {
    if left.is_empty() || left.len() != right.len() {
        return None;
    }
    let mut dot = 0.0f64;
    let mut left_norm = 0.0f64;
    let mut right_norm = 0.0f64;
    for (a, b) in left.iter().zip(right.iter()) {
        let a = f64::from(*a);
        let b = f64::from(*b);
        if !a.is_finite() || !b.is_finite() {
            return None;
        }
        dot += a * b;
        left_norm += a * a;
        right_norm += b * b;
    }
    if left_norm <= f64::EPSILON || right_norm <= f64::EPSILON {
        return None;
    }
    Some(dot / (left_norm.sqrt() * right_norm.sqrt()))
}

#[allow(clippy::too_many_arguments)]
fn build_vector_chunk_embedding_records(
    owner_id: &str,
    base_name: &str,
    doc_id: &str,
    doc_name: &str,
    embedding_model: &str,
    chunks: &[VectorChunk],
    vectors: &[Vec<f32>],
    updated_at: f64,
) -> Result<Vec<VectorChunkEmbeddingRecord>> {
    if chunks.len() != vectors.len() {
        return Err(anyhow!("embedding count mismatch"));
    }
    let mut records = Vec::with_capacity(chunks.len());
    for (chunk, vector) in chunks.iter().zip(vectors.iter()) {
        records.push(VectorChunkEmbeddingRecord {
            chunk_id: chunk.chunk_id.clone(),
            owner_id: owner_id.to_string(),
            base_name: base_name.to_string(),
            doc_id: doc_id.to_string(),
            doc_name: doc_name.to_string(),
            chunk_index: chunk.index as i64,
            start: chunk.start as i64,
            end: chunk.end as i64,
            content: chunk.content.clone(),
            embedding_model: embedding_model.to_string(),
            vector_json: vector_to_json(vector)?,
            dimensions: vector.len() as i64,
            updated_at,
        });
    }
    Ok(records)
}

#[allow(clippy::too_many_arguments)]
pub fn upsert_vector_chunk_embeddings(
    storage: &dyn StorageBackend,
    owner_id: Option<&str>,
    base_name: &str,
    doc_id: &str,
    doc_name: &str,
    embedding_model: &str,
    chunks: &[VectorChunk],
    vectors: &[Vec<f32>],
) -> Result<usize> {
    let owner_key = resolve_owner_key(owner_id);
    let records = build_vector_chunk_embedding_records(
        &owner_key,
        base_name,
        doc_id,
        doc_name,
        embedding_model,
        chunks,
        vectors,
        now_ts(),
    )?;
    let count = records.len();
    storage.upsert_vector_chunk_embeddings(&records)?;
    Ok(count)
}

pub fn delete_vector_chunk_embedding(storage: &dyn StorageBackend, chunk_id: &str) -> Result<bool> {
    storage.delete_vector_chunk_embedding(chunk_id)
}

pub fn delete_vector_document_embeddings(
    storage: &dyn StorageBackend,
    owner_id: Option<&str>,
    base_name: &str,
    doc_id: &str,
) -> Result<i64> {
    let owner_key = resolve_owner_key(owner_id);
    storage.delete_vector_chunk_embeddings_by_doc(&owner_key, base_name, doc_id)
}

pub fn delete_vector_base_embeddings(
    storage: &dyn StorageBackend,
    owner_id: Option<&str>,
    base_name: &str,
) -> Result<i64> {
    let owner_key = resolve_owner_key(owner_id);
    storage.delete_vector_chunk_embeddings_by_base(&owner_key, base_name)
}

static MIGRATED_BASES: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

async fn ensure_vector_documents_migrated(
    storage: &dyn StorageBackend,
    owner_id: Option<&str>,
    base_name: &str,
    root: &Path,
) -> Result<()> {
    let owner_key = resolve_owner_key(owner_id);
    let key = format!("{owner_key}::{base_name}");
    let store = MIGRATED_BASES.get_or_init(|| Mutex::new(HashSet::new()));
    {
        let mut guard = store.lock().await;
        if guard.contains(&key) {
            return Ok(());
        }
        guard.insert(key.clone());
    }
    let result = migrate_vector_documents_from_fs(storage, &owner_key, base_name, root).await;
    if let Err(err) = result {
        warn!("Vector knowledge migration failed for {owner_key}/{base_name}: {err}");
        let mut guard = store.lock().await;
        guard.remove(&key);
    }
    Ok(())
}

async fn migrate_vector_documents_from_fs(
    storage: &dyn StorageBackend,
    owner_id: &str,
    base_name: &str,
    root: &Path,
) -> Result<usize> {
    let docs_dir = resolve_vector_documents_dir(root, false)?;
    if !docs_dir.exists() {
        return Ok(0);
    }
    let mut entries = tokio::fs::read_dir(&docs_dir).await?;
    let mut migrated = 0usize;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let raw = tokio::fs::read_to_string(&path).await.unwrap_or_default();
        if raw.trim().is_empty() {
            continue;
        }
        let meta = match serde_json::from_str::<VectorDocumentMeta>(&raw) {
            Ok(meta) => meta,
            Err(err) => {
                warn!(
                    "Skip malformed vector knowledge meta {}: {err}",
                    path.to_string_lossy()
                );
                continue;
            }
        };
        let content_path = docs_dir.join(format!("{}{}", meta.doc_id, VECTOR_DOC_EXT));
        let content = match tokio::fs::read_to_string(&content_path).await {
            Ok(content) => content,
            Err(err) => {
                warn!(
                    "Skip vector knowledge content {}: {err}",
                    content_path.to_string_lossy()
                );
                continue;
            }
        };
        let record = match build_vector_record(owner_id, base_name, &meta, &content) {
            Ok(record) => record,
            Err(err) => {
                warn!("Skip vector knowledge record {}: {err}", meta.doc_id);
                continue;
            }
        };
        if storage.upsert_vector_document(&record).is_ok() {
            migrated += 1;
            let _ = tokio::fs::remove_file(&content_path).await;
            let _ = tokio::fs::remove_file(&path).await;
        }
    }
    if migrated > 0 {
        if tokio::fs::remove_dir(&docs_dir).await.is_ok() {
            let _ = tokio::fs::remove_dir(root).await;
        }
        info!(
            "Vector knowledge migrated {} docs for {owner_id}/{base_name}",
            migrated
        );
    }
    Ok(migrated)
}

async fn read_vector_document_content_from_fs(root: &Path, doc_id: &str) -> Result<String> {
    let docs_dir = resolve_vector_documents_dir(root, false)?;
    let content_path = docs_dir.join(format!("{doc_id}{VECTOR_DOC_EXT}"));
    let content = tokio::fs::read_to_string(content_path).await?;
    Ok(content)
}

async fn read_vector_document_meta_from_fs(
    root: &Path,
    doc_id: &str,
) -> Result<VectorDocumentMeta> {
    let docs_dir = resolve_vector_documents_dir(root, false)?;
    let meta_path = docs_dir.join(format!("{doc_id}{VECTOR_META_EXT}"));
    let raw = tokio::fs::read_to_string(meta_path).await?;
    let meta = serde_json::from_str::<VectorDocumentMeta>(&raw)?;
    Ok(meta)
}

async fn delete_vector_document_files_from_fs(root: &Path, doc_id: &str) -> Result<()> {
    let docs_dir = resolve_vector_documents_dir(root, false)?;
    let content_path = docs_dir.join(format!("{doc_id}{VECTOR_DOC_EXT}"));
    let meta_path = docs_dir.join(format!("{doc_id}{VECTOR_META_EXT}"));
    if content_path.exists() {
        let _ = tokio::fs::remove_file(content_path).await;
    }
    if meta_path.exists() {
        let _ = tokio::fs::remove_file(meta_path).await;
    }
    Ok(())
}

pub async fn write_vector_document(
    storage: &dyn StorageBackend,
    owner_id: Option<&str>,
    base_name: &str,
    meta: &VectorDocumentMeta,
    content: &str,
) -> Result<()> {
    let owner_key = resolve_owner_key(owner_id);
    let record = build_vector_record(&owner_key, base_name, meta, content)?;
    storage.upsert_vector_document(&record)?;
    Ok(())
}

pub async fn read_vector_document_content(
    storage: &dyn StorageBackend,
    owner_id: Option<&str>,
    base_name: &str,
    root: &Path,
    doc_id: &str,
) -> Result<String> {
    ensure_vector_documents_migrated(storage, owner_id, base_name, root).await?;
    let owner_key = resolve_owner_key(owner_id);
    if let Some(record) = storage.get_vector_document(&owner_key, base_name, doc_id)? {
        return Ok(record.content);
    }
    let content = read_vector_document_content_from_fs(root, doc_id).await?;
    if let Ok(meta) = read_vector_document_meta_from_fs(root, doc_id).await {
        let record = build_vector_record(&owner_key, base_name, &meta, &content)?;
        let _ = storage.upsert_vector_document(&record);
        let _ = delete_vector_document_files_from_fs(root, doc_id).await;
    }
    Ok(content)
}

pub async fn read_vector_document_meta(
    storage: &dyn StorageBackend,
    owner_id: Option<&str>,
    base_name: &str,
    root: &Path,
    doc_id: &str,
) -> Result<VectorDocumentMeta> {
    ensure_vector_documents_migrated(storage, owner_id, base_name, root).await?;
    let owner_key = resolve_owner_key(owner_id);
    if let Some(record) = storage.get_vector_document(&owner_key, base_name, doc_id)? {
        return build_meta_from_record(&record);
    }
    let meta = read_vector_document_meta_from_fs(root, doc_id).await?;
    if let Ok(content) = read_vector_document_content_from_fs(root, doc_id).await {
        if let Ok(record) = build_vector_record(&owner_key, base_name, &meta, &content) {
            let _ = storage.upsert_vector_document(&record);
            let _ = delete_vector_document_files_from_fs(root, doc_id).await;
        }
    }
    Ok(meta)
}

pub async fn list_vector_documents(
    storage: &dyn StorageBackend,
    owner_id: Option<&str>,
    base_name: &str,
    root: &Path,
) -> Result<Vec<VectorDocumentSummary>> {
    ensure_vector_documents_migrated(storage, owner_id, base_name, root).await?;
    let owner_key = resolve_owner_key(owner_id);
    let records = storage.list_vector_document_summaries(&owner_key, base_name)?;
    Ok(records
        .into_iter()
        .map(|record| build_summary_from_record(&record))
        .collect())
}

pub async fn delete_vector_document_files(
    storage: &dyn StorageBackend,
    owner_id: Option<&str>,
    base_name: &str,
    root: &Path,
    doc_id: &str,
) -> Result<()> {
    ensure_vector_documents_migrated(storage, owner_id, base_name, root).await?;
    let owner_key = resolve_owner_key(owner_id);
    let _ = storage.delete_vector_chunk_embeddings_by_doc(&owner_key, base_name, doc_id)?;
    let _ = storage.delete_vector_document(&owner_key, base_name, doc_id)?;
    let _ = delete_vector_document_files_from_fs(root, doc_id).await;
    Ok(())
}

pub async fn build_chunk_previews(
    content: &str,
    meta: &VectorDocumentMeta,
) -> Vec<VectorChunkPreview> {
    let chars: Vec<char> = content.chars().collect();
    meta.chunks
        .iter()
        .filter_map(|chunk| {
            let status = resolve_chunk_status(meta, chunk);
            if status == "deleted" {
                return None;
            }
            let slice = resolve_chunk_content(&chars, chunk);
            let preview = build_preview(&slice, 120);
            Some(VectorChunkPreview {
                index: chunk.index,
                start: chunk.start,
                end: chunk.end,
                chunk_id: None,
                preview,
                content: slice,
                status,
            })
        })
        .collect()
}

pub async fn query_chunks_by_text(
    storage: &dyn StorageBackend,
    owner_id: Option<&str>,
    base: &KnowledgeBaseConfig,
    root: &Path,
    query: &str,
    limit: usize,
) -> Result<Vec<VectorSearchHit>> {
    ensure_vector_base_type(base)?;
    ensure_vector_documents_migrated(storage, owner_id, &base.name, root).await?;
    let cleaned_query = query.trim();
    if cleaned_query.is_empty() {
        return Ok(Vec::new());
    }
    let owner_key = resolve_owner_key(owner_id);
    let summaries = storage.list_vector_document_summaries(&owner_key, &base.name)?;
    let tokens = extract_text_query_tokens(cleaned_query);
    let normalized_query = cleaned_query.to_lowercase();
    let limit = limit.max(1);
    let mut scored = Vec::new();
    for summary in summaries {
        let Some(record) = storage.get_vector_document(&owner_key, &base.name, &summary.doc_id)?
        else {
            continue;
        };
        let chunks =
            serde_json::from_str::<Vec<VectorChunkMeta>>(&record.chunks_json).unwrap_or_default();
        let meta = VectorDocumentMeta {
            doc_id: record.doc_id.clone(),
            name: record.doc_name.clone(),
            embedding_model: record.embedding_model.clone(),
            chunk_size: usize::try_from(record.chunk_size).unwrap_or_default(),
            chunk_overlap: usize::try_from(record.chunk_overlap).unwrap_or_default(),
            chunk_count: usize::try_from(record.chunk_count).unwrap_or_default(),
            status: record.status.clone(),
            created_at: record.created_at,
            updated_at: record.updated_at,
            chunks,
        };
        let content_chars: Vec<char> = record.content.chars().collect();
        for chunk in &meta.chunks {
            let status = resolve_chunk_status(&meta, chunk);
            if status == "deleted" {
                continue;
            }
            let chunk_content = resolve_chunk_content(&content_chars, chunk);
            if chunk_content.trim().is_empty() {
                continue;
            }
            let score =
                score_text_chunk(&record.doc_name, &chunk_content, &normalized_query, &tokens);
            if score <= 0 {
                continue;
            }
            scored.push((
                score,
                VectorSearchHit {
                    doc_id: record.doc_id.clone(),
                    doc_name: record.doc_name.clone(),
                    chunk_index: chunk.index,
                    start: chunk.start,
                    end: chunk.end,
                    content: chunk_content,
                    embedding_model: record.embedding_model.clone(),
                    score: Some(score as f64),
                },
            ));
            if scored.len() >= VECTOR_LITERAL_FALLBACK_CANDIDATE_LIMIT {
                break;
            }
        }
        if scored.len() >= VECTOR_LITERAL_FALLBACK_CANDIDATE_LIMIT {
            break;
        }
    }
    scored.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.doc_name.cmp(&right.1.doc_name))
            .then_with(|| left.1.chunk_index.cmp(&right.1.chunk_index))
    });
    scored.truncate(limit);
    Ok(scored.into_iter().map(|(_, hit)| hit).collect())
}

pub async fn query_chunks_by_vector(
    storage: &dyn StorageBackend,
    owner_id: Option<&str>,
    base: &KnowledgeBaseConfig,
    root: &Path,
    embedding_model: &str,
    vector: &[f32],
    top_k: usize,
) -> Result<Vec<VectorSearchHit>> {
    ensure_vector_base_type(base)?;
    ensure_vector_documents_migrated(storage, owner_id, &base.name, root).await?;
    if vector.is_empty() {
        return Ok(Vec::new());
    }
    let owner_key = resolve_owner_key(owner_id);
    let candidate_limit = VECTOR_SEARCH_CANDIDATE_LIMIT.max(top_k.max(1) as i64);
    let records = storage.list_vector_chunk_embeddings(
        &owner_key,
        &base.name,
        embedding_model,
        candidate_limit,
    )?;
    let mut scored = Vec::new();
    for record in records {
        let Some(candidate) = parse_vector_json(&record.vector_json) else {
            continue;
        };
        let Some(score) = cosine_similarity(vector, &candidate) else {
            continue;
        };
        scored.push((
            score,
            VectorSearchHit {
                doc_id: record.doc_id,
                doc_name: record.doc_name,
                chunk_index: usize::try_from(record.chunk_index).unwrap_or_default(),
                start: usize::try_from(record.start).unwrap_or_default(),
                end: usize::try_from(record.end).unwrap_or_default(),
                content: record.content,
                embedding_model: record.embedding_model,
                score: Some(score),
            },
        ));
    }
    scored.sort_by(|left, right| {
        right
            .0
            .total_cmp(&left.0)
            .then_with(|| left.1.doc_name.cmp(&right.1.doc_name))
            .then_with(|| left.1.chunk_index.cmp(&right.1.chunk_index))
    });
    scored.truncate(top_k.max(1));
    Ok(scored.into_iter().map(|(_, hit)| hit).collect())
}

fn extract_text_query_tokens(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for ch in query.chars() {
        if ch.is_ascii_alphanumeric() {
            current.push(ch.to_ascii_lowercase());
            continue;
        }
        if current.len() >= 2 {
            tokens.push(current.clone());
        }
        current.clear();
        if ('\u{4e00}'..='\u{9fff}').contains(&ch) {
            tokens.push(ch.to_string());
        }
    }
    if current.len() >= 2 {
        tokens.push(current);
    }
    let mut seen = HashSet::new();
    tokens
        .into_iter()
        .filter(|token| seen.insert(token.clone()))
        .take(32)
        .collect()
}

fn score_text_chunk(
    doc_name: &str,
    content: &str,
    normalized_query: &str,
    tokens: &[String],
) -> i32 {
    let text = format!("{doc_name}\n{content}").to_lowercase();
    let mut score = 0;
    if !normalized_query.is_empty() && text.contains(normalized_query) {
        score += 16;
    }
    for token in tokens {
        if token.is_empty() {
            continue;
        }
        let mut start = 0usize;
        let mut count = 0;
        while let Some(offset) = text[start..].find(token) {
            count += 1;
            start += offset + token.len();
            if count >= 4 {
                break;
            }
        }
        score += count;
    }
    score
}

fn build_preview(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for (idx, ch) in trimmed.chars().enumerate() {
        if idx >= limit {
            break;
        }
        out.push(ch);
    }
    if trimmed.chars().count() > limit {
        out.push_str("...");
    }
    out
}

impl VectorDocumentMeta {
    pub fn to_summary(&self) -> VectorDocumentSummary {
        VectorDocumentSummary {
            doc_id: self.doc_id.clone(),
            name: self.name.clone(),
            status: self.status.clone(),
            chunk_count: self.chunk_count,
            embedding_model: self.embedding_model.clone(),
            updated_at: self.updated_at,
        }
    }
}

fn resolve_vector_root_dir() -> PathBuf {
    std::env::var(VECTOR_ROOT_DIR_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(VECTOR_ROOT_DIR))
}

fn safe_user_id(user_id: &str) -> String {
    let cleaned = user_id.trim();
    if cleaned.is_empty() {
        return "anonymous".to_string();
    }
    let mut output = String::with_capacity(cleaned.len());
    for ch in cleaned.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            output.push(ch);
        } else {
            output.push('_');
        }
    }
    output
}

static DOCUMENT_LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();

fn document_lock_key(root: &Path, doc_id: &str) -> String {
    let normalized_root = normalize_existing_path(root);
    format!("{}::{}", normalized_root.to_string_lossy(), doc_id)
}

pub async fn with_document_lock<T, F, Fut>(root: &Path, doc_id: &str, op: F) -> T
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = T>,
{
    let key = document_lock_key(root, doc_id);
    let lock = {
        let store = DOCUMENT_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
        let mut guard = store.lock().await;
        guard
            .entry(key)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    };
    let _guard = lock.lock().await;
    op().await
}

pub async fn embed_chunks(
    config: &LlmModelConfig,
    chunks: &[VectorChunk],
    timeout_s: u64,
) -> Result<Vec<Vec<f32>>> {
    let inputs = chunks
        .iter()
        .map(|chunk| chunk.content.clone())
        .collect::<Vec<_>>();
    embed_texts(config, &inputs, timeout_s).await
}

#[allow(clippy::too_many_arguments)]
pub async fn prepare_document(
    base: &KnowledgeBaseConfig,
    owner_id: Option<&str>,
    storage: &dyn StorageBackend,
    root: &Path,
    doc_name: &str,
    doc_id: Option<&str>,
    content: &str,
    previous_meta: Option<&VectorDocumentMeta>,
) -> Result<VectorDocumentMeta> {
    ensure_vector_base_type(base)?;
    ensure_vector_documents_migrated(storage, owner_id, &base.name, root).await?;
    let chunk_size = resolve_chunk_size(base);
    let chunk_overlap = resolve_chunk_overlap(base);
    let owner_key = resolve_owner_key(owner_id);
    let doc_id = doc_id
        .map(|value| value.to_string())
        .unwrap_or_else(|| build_doc_id(Some(owner_key.as_str()), &base.name, doc_name));
    let chunks = split_text_into_chunks(content, chunk_size, chunk_overlap, &doc_id);
    if chunks.is_empty() {
        return Err(anyhow!(i18n::t("error.empty_parse_result")));
    }
    let chunk_meta = build_chunk_meta(&chunks);
    let created_at = previous_meta
        .map(|meta| meta.created_at)
        .unwrap_or_else(now_ts);
    let updated_at = now_ts();
    let mut meta = VectorDocumentMeta {
        doc_id,
        name: doc_name.to_string(),
        embedding_model: base
            .embedding_model
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_string(),
        chunk_size,
        chunk_overlap,
        chunk_count: chunk_meta.len(),
        status: "pending".to_string(),
        created_at,
        updated_at,
        chunks: chunk_meta,
    };
    refresh_document_meta(&mut meta);
    let doc_id = meta.doc_id.clone();
    with_document_lock(root, &doc_id, || async {
        write_vector_document(storage, owner_id, &base.name, &meta, content).await
    })
    .await?;
    Ok(meta)
}

#[allow(clippy::too_many_arguments)]
pub async fn index_document(
    config: &Config,
    base: &KnowledgeBaseConfig,
    owner_id: Option<&str>,
    storage: &dyn StorageBackend,
    root: &Path,
    doc_name: &str,
    doc_id: Option<&str>,
    content: &str,
    previous_meta: Option<&VectorDocumentMeta>,
) -> Result<VectorDocumentMeta> {
    ensure_vector_base_config(base)?;
    ensure_vector_documents_migrated(storage, owner_id, &base.name, root).await?;
    let embedding_name = base
        .embedding_model
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_string();
    let embed_config = resolve_embedding_model(config, &embedding_name)?;
    let chunk_size = resolve_chunk_size(base);
    let chunk_overlap = resolve_chunk_overlap(base);
    let owner_key = resolve_owner_key(owner_id);
    let doc_id = doc_id
        .map(|value| value.to_string())
        .unwrap_or_else(|| build_doc_id(Some(owner_key.as_str()), &base.name, doc_name));
    let mut chunk_meta = previous_meta
        .filter(|meta| meta.chunk_size == chunk_size && meta.chunk_overlap == chunk_overlap)
        .map(|meta| meta.chunks.clone())
        .unwrap_or_else(|| {
            let chunks = split_text_into_chunks(content, chunk_size, chunk_overlap, &doc_id);
            build_chunk_meta(&chunks)
        });
    if chunk_meta.is_empty() {
        return Err(anyhow!(i18n::t("error.empty_parse_result")));
    }
    let content_chars: Vec<char> = content.chars().collect();
    let mut chunks = Vec::new();
    for chunk in &chunk_meta {
        if is_chunk_deleted(chunk) {
            continue;
        }
        let vector_chunk = build_chunk_from_meta(&content_chars, &doc_id, chunk);
        if vector_chunk.content.trim().is_empty() {
            continue;
        }
        chunks.push(vector_chunk);
    }
    if chunks.is_empty() {
        return Err(anyhow!(i18n::t("error.empty_parse_result")));
    }
    let created_at = previous_meta
        .map(|meta| meta.created_at)
        .unwrap_or_else(now_ts);
    let base_name = base.name.clone();
    let doc_name = doc_name.to_string();
    let lock_doc_id = doc_id.clone();
    let embedding_name = embedding_name.clone();
    let owner_key = owner_key.clone();
    let result = with_document_lock(root, &lock_doc_id, move || async move {
        let timeout_s = embed_config.timeout_s.unwrap_or(120);
        let vectors = embed_chunks(&embed_config, &chunks, timeout_s).await?;
        storage.delete_vector_chunk_embeddings_by_doc(&owner_key, &base_name, &doc_id)?;
        let records = build_vector_chunk_embedding_records(
            &owner_key,
            &base_name,
            &doc_id,
            &doc_name,
            &embedding_name,
            &chunks,
            &vectors,
            now_ts(),
        )?;
        storage.upsert_vector_chunk_embeddings(&records)?;
        for chunk in &mut chunk_meta {
            if is_chunk_deleted(chunk) {
                continue;
            }
            chunk.status = Some("embedded".to_string());
        }
        let updated_at = now_ts();
        let mut meta = VectorDocumentMeta {
            doc_id,
            name: doc_name,
            embedding_model: embedding_name,
            chunk_size,
            chunk_overlap,
            chunk_count: chunk_meta.len(),
            status: "ready".to_string(),
            created_at,
            updated_at,
            chunks: chunk_meta,
        };
        refresh_document_meta(&mut meta);
        write_vector_document(storage, owner_id, &base_name, &meta, content).await?;
        Ok::<VectorDocumentMeta, anyhow::Error>(meta)
    })
    .await?;
    Ok(result)
}

pub fn ensure_unique_doc_name(name: &str, existing: &[VectorDocumentSummary]) -> Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(anyhow!(i18n::t("error.knowledge_base_name_required")));
    }
    let seen: HashSet<String> = existing.iter().map(|doc| doc.name.clone()).collect();
    if !seen.contains(trimmed) {
        return Ok(trimmed.to_string());
    }
    let mut index = 1;
    loop {
        let candidate = format!("{trimmed} ({index})");
        if !seen.contains(&candidate) {
            return Ok(candidate);
        }
        index += 1;
        if index > 1000 {
            return Err(anyhow!(i18n::t("error.knowledge_name_invalid_path")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_similarity_ranks_matching_vectors() {
        let same = cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]).expect("same dimensions");
        let orthogonal = cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]).expect("same dimensions");

        assert!(same > orthogonal);
        assert_eq!(cosine_similarity(&[1.0], &[1.0, 0.0]), None);
    }

    #[test]
    fn vector_chunk_embedding_records_preserve_chunk_identity() {
        let chunks = vec![VectorChunk {
            index: 2,
            start: 10,
            end: 20,
            content: "content".to_string(),
            chunk_id: "chunk-a".to_string(),
        }];
        let records = build_vector_chunk_embedding_records(
            "owner-a",
            "base-a",
            "doc-a",
            "Doc A",
            "model-a",
            &chunks,
            &[vec![0.5, 0.25]],
            1.0,
        )
        .expect("build records");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].chunk_id, "chunk-a");
        assert_eq!(records[0].doc_id, "doc-a");
        assert_eq!(records[0].chunk_index, 2);
        assert_eq!(
            parse_vector_json(&records[0].vector_json),
            Some(vec![0.5, 0.25])
        );
    }
}
