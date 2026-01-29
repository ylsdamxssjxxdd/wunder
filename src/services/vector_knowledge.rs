use crate::config::{
    Config, KnowledgeBaseConfig, KnowledgeBaseType, LlmModelConfig, WeaviateConfig,
};
use crate::i18n;
use crate::llm::{embed_texts, is_embedding_model};
use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use uuid::Uuid;

const VECTOR_ROOT_DIR: &str = "vector_knowledge";
const VECTOR_SHARED_DIR: &str = "shared";
const VECTOR_USERS_DIR: &str = "users";
const VECTOR_DOCS_DIR: &str = "documents";
const VECTOR_DOC_EXT: &str = ".md";
const VECTOR_META_EXT: &str = ".json";

const DEFAULT_CHUNK_SIZE: usize = 800;
const DEFAULT_CHUNK_OVERLAP: usize = 100;
const DEFAULT_TOP_K: usize = 5;

const WEAVIATE_CLASS: &str = "KnowledgeChunk";

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
    pub preview: String,
    pub content: String,
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

pub fn ensure_vector_base_config(base: &KnowledgeBaseConfig) -> Result<()> {
    if base.base_type() != KnowledgeBaseType::Vector {
        return Ok(());
    }
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
    let root = PathBuf::from(VECTOR_ROOT_DIR);
    let owner_root = match owner_id {
        Some(user_id) => root.join(VECTOR_USERS_DIR).join(safe_user_id(user_id)),
        None => root.join(VECTOR_SHARED_DIR),
    };
    let target = owner_root.join(cleaned);
    if create {
        std::fs::create_dir_all(&target)?;
    }
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
        })
        .collect()
}

pub fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

pub async fn write_vector_document(
    root: &Path,
    meta: &VectorDocumentMeta,
    content: &str,
) -> Result<()> {
    let docs_dir = resolve_vector_documents_dir(root, true)?;
    let content_path = docs_dir.join(format!("{}{}", meta.doc_id, VECTOR_DOC_EXT));
    let meta_path = docs_dir.join(format!("{}{}", meta.doc_id, VECTOR_META_EXT));
    tokio::fs::write(content_path, content).await?;
    let encoded = serde_json::to_vec_pretty(meta)?;
    tokio::fs::write(meta_path, encoded).await?;
    Ok(())
}

pub async fn read_vector_document_content(root: &Path, doc_id: &str) -> Result<String> {
    let docs_dir = resolve_vector_documents_dir(root, false)?;
    let content_path = docs_dir.join(format!("{doc_id}{VECTOR_DOC_EXT}"));
    let content = tokio::fs::read_to_string(content_path).await?;
    Ok(content)
}

pub async fn read_vector_document_meta(root: &Path, doc_id: &str) -> Result<VectorDocumentMeta> {
    let docs_dir = resolve_vector_documents_dir(root, false)?;
    let meta_path = docs_dir.join(format!("{doc_id}{VECTOR_META_EXT}"));
    let raw = tokio::fs::read_to_string(meta_path).await?;
    let meta = serde_json::from_str::<VectorDocumentMeta>(&raw)?;
    Ok(meta)
}

pub async fn list_vector_documents(root: &Path) -> Result<Vec<VectorDocumentSummary>> {
    let docs_dir = resolve_vector_documents_dir(root, false)?;
    if !docs_dir.exists() {
        return Ok(Vec::new());
    }
    let mut entries = tokio::fs::read_dir(docs_dir).await?;
    let mut output = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let raw = tokio::fs::read_to_string(&path).await.unwrap_or_default();
        if raw.trim().is_empty() {
            continue;
        }
        if let Ok(meta) = serde_json::from_str::<VectorDocumentMeta>(&raw) {
            output.push(meta.to_summary());
        }
    }
    output.sort_by(|a, b| {
        b.updated_at
            .partial_cmp(&a.updated_at)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(output)
}

pub async fn delete_vector_document_files(root: &Path, doc_id: &str) -> Result<()> {
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

pub async fn build_chunk_previews(
    content: &str,
    meta: &VectorDocumentMeta,
) -> Vec<VectorChunkPreview> {
    let chars: Vec<char> = content.chars().collect();
    meta.chunks
        .iter()
        .map(|chunk| {
            let start = chunk.start.min(chars.len());
            let end = chunk.end.min(chars.len());
            let slice: String = chars[start..end].iter().collect();
            let preview = build_preview(&slice, 120);
            VectorChunkPreview {
                index: chunk.index,
                start: chunk.start,
                end: chunk.end,
                preview,
                content: slice,
            }
        })
        .collect()
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

#[derive(Clone)]
pub struct WeaviateClient {
    http: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
    batch_size: usize,
}

impl WeaviateClient {
    pub fn from_config(config: &WeaviateConfig) -> Option<Self> {
        let url = config.url.trim().trim_end_matches('/').to_string();
        if url.is_empty() {
            return None;
        }
        let timeout = std::time::Duration::from_secs(config.timeout_s.max(10));
        let http = reqwest::Client::builder().timeout(timeout).build().ok()?;
        let batch_size = if config.batch_size == 0 {
            64
        } else {
            config.batch_size
        };
        Some(Self {
            http,
            base_url: url,
            api_key: config.api_key.clone(),
            batch_size,
        })
    }

    pub async fn ensure_schema(&self) -> Result<()> {
        static READY: OnceLock<Mutex<bool>> = OnceLock::new();
        let lock = READY.get_or_init(|| Mutex::new(false));
        let mut guard = lock.lock().await;
        if *guard {
            return Ok(());
        }
        let payload = json!({
            "class": WEAVIATE_CLASS,
            "description": "Vector knowledge chunks",
            "vectorizer": "none",
            "vectorIndexConfig": {
                "distance": "cosine"
            },
            "properties": [
                {"name": "owner_id", "dataType": ["text"]},
                {"name": "base_name", "dataType": ["text"]},
                {"name": "doc_id", "dataType": ["text"]},
                {"name": "doc_name", "dataType": ["text"]},
                {"name": "chunk_index", "dataType": ["int"]},
                {"name": "start", "dataType": ["int"]},
                {"name": "end", "dataType": ["int"]},
                {"name": "content", "dataType": ["text"]},
                {"name": "embedding_model", "dataType": ["text"]},
                {"name": "created_at", "dataType": ["date"]},
            ],
        });
        let response = self
            .http
            .post(format!("{}/v1/schema", self.base_url))
            .headers(self.build_headers())
            .json(&payload)
            .send()
            .await?;
        if response.status().is_success() {
            *guard = true;
            return Ok(());
        }
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if status.as_u16() == 422 || status.as_u16() == 409 {
            *guard = true;
            return Ok(());
        }
        Err(anyhow!("weaviate schema create failed: {status} {body}"))
    }

    pub async fn upsert_chunks(
        &self,
        owner_id: &str,
        base_name: &str,
        doc_id: &str,
        doc_name: &str,
        embedding_model: &str,
        chunks: &[VectorChunk],
        vectors: &[Vec<f32>],
    ) -> Result<()> {
        if chunks.len() != vectors.len() {
            return Err(anyhow!("embedding count mismatch"));
        }
        self.ensure_schema().await?;
        let created_at = Utc::now().to_rfc3339();
        let mut start = 0;
        while start < chunks.len() {
            let end = (start + self.batch_size).min(chunks.len());
            let mut objects = Vec::new();
            for (chunk, vector) in chunks[start..end].iter().zip(&vectors[start..end]) {
                let properties = json!({
                    "owner_id": owner_id,
                    "base_name": base_name,
                    "doc_id": doc_id,
                    "doc_name": doc_name,
                    "chunk_index": chunk.index as i64,
                    "start": chunk.start as i64,
                    "end": chunk.end as i64,
                    "content": chunk.content,
                    "embedding_model": embedding_model,
                    "created_at": created_at,
                });
                objects.push(json!({
                    "class": WEAVIATE_CLASS,
                    "id": chunk.chunk_id,
                    "properties": properties,
                    "vector": vector,
                }));
            }
            let payload = json!({ "objects": objects });
            let response = self
                .http
                .post(format!("{}/v1/batch/objects", self.base_url))
                .headers(self.build_headers())
                .json(&payload)
                .send()
                .await?;
            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(anyhow!("weaviate batch insert failed: {status} {body}"));
            }
            start = end;
        }
        Ok(())
    }

    pub async fn query_chunks(
        &self,
        owner_id: &str,
        base_name: &str,
        embedding_model: &str,
        vector: &[f32],
        top_k: usize,
    ) -> Result<Vec<VectorSearchHit>> {
        self.ensure_schema().await?;
        let vector_list = vector
            .iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let query = format!(
            "{{{{ Get {{{class}(nearVector: {{vector: [{vector_list}]}}, limit: {top_k}, where: {{operator: And, operands: [{{path: [\"owner_id\"], operator: Equal, valueString: \"{owner}\"}}, {{path: [\"base_name\"], operator: Equal, valueString: \"{base}\"}}, {{path: [\"embedding_model\"], operator: Equal, valueString: \"{model}\"}}]}}) {{ doc_id doc_name chunk_index start end content embedding_model _additional {{ distance }} }} }} }}}}",
            class = WEAVIATE_CLASS,
            owner = escape_graphql_string(owner_id),
            base = escape_graphql_string(base_name),
            model = escape_graphql_string(embedding_model),
        );
        let payload = json!({ "query": query });
        let response = self
            .http
            .post(format!("{}/v1/graphql", self.base_url))
            .headers(self.build_headers())
            .json(&payload)
            .send()
            .await?;
        let status = response.status();
        let body: Value = response.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            return Err(anyhow!("weaviate query failed: {status} {body}"));
        }
        let items = body
            .get("data")
            .and_then(|value| value.get("Get"))
            .and_then(|value| value.get(WEAVIATE_CLASS))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut hits = Vec::new();
        for item in items {
            let doc_id = item
                .get("doc_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let doc_name = item
                .get("doc_name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let chunk_index = item.get("chunk_index").and_then(Value::as_i64).unwrap_or(0) as usize;
            let start = item.get("start").and_then(Value::as_i64).unwrap_or(0) as usize;
            let end = item.get("end").and_then(Value::as_i64).unwrap_or(0) as usize;
            let content = item
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let embedding_model = item
                .get("embedding_model")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let distance = item
                .get("_additional")
                .and_then(|value| value.get("distance"))
                .and_then(Value::as_f64);
            let score = distance.map(|value| (1.0 - value).max(0.0));
            hits.push(VectorSearchHit {
                doc_id,
                doc_name,
                chunk_index,
                start,
                end,
                content,
                embedding_model,
                score,
            });
        }
        Ok(hits)
    }

    pub async fn delete_doc_chunks(
        &self,
        owner_id: &str,
        base_name: &str,
        embedding_model: &str,
        doc_id: &str,
        limit: usize,
    ) -> Result<usize> {
        let ids = self
            .list_chunk_ids(owner_id, base_name, embedding_model, doc_id, limit)
            .await?;
        let mut deleted = 0;
        for id in ids {
            let response = self
                .http
                .delete(format!("{}/v1/objects/{}", self.base_url, id))
                .headers(self.build_headers())
                .send()
                .await?;
            if response.status().is_success() {
                deleted += 1;
            }
        }
        Ok(deleted)
    }

    pub async fn delete_doc_chunks_all(
        &self,
        owner_id: &str,
        base_name: &str,
        embedding_model: &str,
        doc_id: &str,
    ) -> Result<usize> {
        let mut total = 0;
        let limit = self.batch_size.max(64).min(2048);
        loop {
            let ids = self
                .list_chunk_ids(owner_id, base_name, embedding_model, doc_id, limit)
                .await?;
            if ids.is_empty() {
                break;
            }
            for id in ids {
                let response = self
                    .http
                    .delete(format!("{}/v1/objects/{}", self.base_url, id))
                    .headers(self.build_headers())
                    .send()
                    .await?;
                if response.status().is_success() {
                    total += 1;
                }
            }
        }
        Ok(total)
    }

    async fn list_chunk_ids(
        &self,
        owner_id: &str,
        base_name: &str,
        embedding_model: &str,
        doc_id: &str,
        limit: usize,
    ) -> Result<Vec<String>> {
        self.ensure_schema().await?;
        let query = format!(
            "{{{{ Get {{{class}(limit: {limit}, where: {{operator: And, operands: [{{path: [\"owner_id\"], operator: Equal, valueString: \"{owner}\"}}, {{path: [\"base_name\"], operator: Equal, valueString: \"{base}\"}}, {{path: [\"doc_id\"], operator: Equal, valueString: \"{doc}\"}}, {{path: [\"embedding_model\"], operator: Equal, valueString: \"{model}\"}}]}}) {{ _additional {{ id }} }} }} }}}}",
            class = WEAVIATE_CLASS,
            owner = escape_graphql_string(owner_id),
            base = escape_graphql_string(base_name),
            doc = escape_graphql_string(doc_id),
            model = escape_graphql_string(embedding_model),
        );
        let payload = json!({ "query": query });
        let response = self
            .http
            .post(format!("{}/v1/graphql", self.base_url))
            .headers(self.build_headers())
            .json(&payload)
            .send()
            .await?;
        let status = response.status();
        let body: Value = response.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            return Err(anyhow!("weaviate list ids failed: {status} {body}"));
        }
        let items = body
            .get("data")
            .and_then(|value| value.get("Get"))
            .and_then(|value| value.get(WEAVIATE_CLASS))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut ids = Vec::new();
        for item in items {
            if let Some(id) = item
                .get("_additional")
                .and_then(|value| value.get("id"))
                .and_then(Value::as_str)
            {
                ids.push(id.to_string());
            }
        }
        Ok(ids)
    }

    fn build_headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(api_key) = &self.api_key {
            if !api_key.trim().is_empty() {
                if let Ok(value) = format!("Bearer {api_key}").parse() {
                    headers.insert(reqwest::header::AUTHORIZATION, value);
                }
            }
        }
        headers
    }
}

fn escape_graphql_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\"', "\\\"")
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

pub fn resolve_weaviate_client(config: &Config) -> Result<WeaviateClient> {
    WeaviateClient::from_config(&config.vector_store.weaviate)
        .ok_or_else(|| anyhow!(i18n::t("error.vector_store_not_configured")))
}

pub async fn index_document(
    config: &Config,
    base: &KnowledgeBaseConfig,
    owner_id: Option<&str>,
    root: &Path,
    doc_name: &str,
    doc_id: Option<&str>,
    content: &str,
    previous_meta: Option<&VectorDocumentMeta>,
) -> Result<VectorDocumentMeta> {
    ensure_vector_base_config(base)?;
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
    let chunks = split_text_into_chunks(content, chunk_size, chunk_overlap, &doc_id);
    if chunks.is_empty() {
        return Err(anyhow!(i18n::t("error.empty_parse_result")));
    }
    let chunk_meta = build_chunk_meta(&chunks);
    let timeout_s = embed_config.timeout_s.unwrap_or(120);
    let vectors = embed_chunks(&embed_config, &chunks, timeout_s).await?;
    let client = resolve_weaviate_client(config)?;
    let _ = client
        .delete_doc_chunks_all(&owner_key, &base.name, &embedding_name, &doc_id)
        .await;
    client
        .upsert_chunks(
            &owner_key,
            &base.name,
            &doc_id,
            doc_name,
            &embedding_name,
            &chunks,
            &vectors,
        )
        .await?;
    let created_at = previous_meta
        .map(|meta| meta.created_at)
        .unwrap_or_else(now_ts);
    let updated_at = now_ts();
    let meta = VectorDocumentMeta {
        doc_id,
        name: doc_name.to_string(),
        embedding_model: embedding_name,
        chunk_size,
        chunk_overlap,
        chunk_count: chunk_meta.len(),
        status: "ready".to_string(),
        created_at,
        updated_at,
        chunks: chunk_meta,
    };
    write_vector_document(root, &meta, content).await?;
    Ok(meta)
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
