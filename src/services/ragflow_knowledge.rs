use crate::config::{Config, KnowledgeBaseConfig, KnowledgeBaseType};
use crate::i18n;
use crate::vector_knowledge::{VectorChunkPreview, VectorDocumentMeta, VectorDocumentSummary};
use anyhow::{anyhow, Result};
use reqwest::multipart::{Form, Part};
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_TOP_K: usize = 5;
const DEFAULT_PAGE_SIZE: usize = 200;
const DEFAULT_CHUNK_METHOD: &str = "naive";
const SUPPORTED_CHUNK_METHODS: &[&str] = &[
    "naive",
    "book",
    "email",
    "laws",
    "manual",
    "one",
    "paper",
    "picture",
    "presentation",
    "qa",
    "resume",
    "table",
    "tag",
];

const LAYOUT_CHUNK_METHODS: &[&str] = &[
    "naive",
    "book",
    "laws",
    "manual",
    "one",
    "paper",
    "presentation",
];
const AUTO_FIELD_CHUNK_METHODS: &[&str] = &[
    "naive",
    "book",
    "email",
    "laws",
    "manual",
    "one",
    "paper",
    "picture",
    "presentation",
];
const TEXT_CONTROL_CHUNK_METHODS: &[&str] = &["naive"];
const HTML_EXCEL_CHUNK_METHODS: &[&str] = &["naive"];
const SUPPORTED_LAYOUT_RECOGNIZERS: &[&str] = &[
    "DeepDOC",
    "Plain Text",
    "Docling",
    "OpenDataLoader",
    "TCADP Parser",
];

#[derive(Debug, Clone)]
pub struct RagflowUpload {
    pub filename: String,
    pub input_path: std::path::PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct RagflowSearchHit {
    pub doc_id: String,
    pub doc_name: String,
    pub chunk_id: String,
    pub chunk_index: usize,
    pub start: usize,
    pub end: usize,
    pub content: String,
    pub score: Option<f64>,
}

#[derive(Debug, Clone)]
struct RagflowClient {
    client: Client,
    base_url: String,
    api_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RagflowEnvelope {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    data: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct RagflowDataset {
    id: String,
}

#[derive(Debug, Deserialize)]
struct RagflowDocumentListData {
    #[serde(default)]
    docs: Vec<RagflowDocument>,
}

#[derive(Debug, Deserialize)]
struct RagflowDocument {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    location: Option<String>,
    #[serde(default)]
    run: Option<Value>,
    #[serde(default)]
    status: Option<Value>,
    #[serde(default)]
    chunk_count: Option<usize>,
    #[serde(default)]
    token_count: Option<usize>,
    #[serde(default)]
    update_time: Option<Value>,
    #[serde(default)]
    create_time: Option<Value>,
    #[serde(default, alias = "chunk_method")]
    parser_id: Option<String>,
    #[serde(default)]
    parser_config: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct RagflowChunkListData {
    #[serde(default)]
    chunks: Vec<RagflowChunk>,
    #[serde(default)]
    doc: Option<RagflowDocument>,
}

#[derive(Debug, Deserialize)]
struct RagflowChunk {
    id: String,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    content_with_weight: Option<String>,
    #[serde(default)]
    available: Option<bool>,
    #[serde(default)]
    available_int: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct RagflowRetrievalData {
    #[serde(default)]
    chunks: Vec<RagflowRetrievalChunk>,
}

#[derive(Debug, Deserialize)]
struct RagflowRetrievalChunk {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    document_id: Option<String>,
    #[serde(default)]
    document_keyword: Option<String>,
    #[serde(default)]
    docnm_kwd: Option<String>,
    #[serde(default)]
    similarity: Option<f64>,
    #[serde(default)]
    score: Option<f64>,
}

impl RagflowClient {
    fn from_config(config: &Config) -> Result<Self> {
        let base_url = config.ragflow.base_url.trim().trim_end_matches('/');
        if base_url.is_empty() {
            return Err(anyhow!("RAGFlow base URL is not configured"));
        }
        let timeout_s = config.ragflow.timeout_s.clamp(1, 600);
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_s))
            .build()?;
        let api_key = config
            .ragflow
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        Ok(Self {
            client,
            base_url: base_url.to_string(),
            api_key,
        })
    }

    fn url(&self, path: &str) -> String {
        let path = path.trim_start_matches('/');
        format!("{}/{}", self.base_url, path)
    }

    fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        let mut builder = self.client.request(method, self.url(path));
        if let Some(api_key) = self.api_key.as_deref() {
            builder = builder.bearer_auth(api_key);
        }
        builder
    }

    async fn send_json<T>(&self, builder: reqwest::RequestBuilder) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let response = builder.send().await?;
        let status = response.status();
        let text = response.text().await?;
        if !status.is_success() {
            return Err(anyhow!("RAGFlow request failed ({status}): {text}"));
        }
        let envelope: RagflowEnvelope = serde_json::from_str(&text)
            .map_err(|err| anyhow!("RAGFlow response parse failed: {err}; body: {text}"))?;
        if envelope.code != 0 {
            return Err(anyhow!(
                "RAGFlow error {}: {}",
                envelope.code,
                envelope
                    .message
                    .unwrap_or_else(|| "unknown error".to_string())
            ));
        }
        let data = envelope
            .data
            .ok_or_else(|| anyhow!("RAGFlow response missing data"))?;
        serde_json::from_value(data).map_err(|err| anyhow!("RAGFlow response decode failed: {err}"))
    }

    async fn send_empty(&self, builder: reqwest::RequestBuilder) -> Result<()> {
        let response = builder.send().await?;
        let status = response.status();
        let text = response.text().await?;
        if !status.is_success() {
            return Err(anyhow!("RAGFlow request failed ({status}): {text}"));
        }
        let envelope: RagflowEnvelope = serde_json::from_str(&text)
            .map_err(|err| anyhow!("RAGFlow response parse failed: {err}; body: {text}"))?;
        if envelope.code != 0 {
            return Err(anyhow!(
                "RAGFlow error {}: {}",
                envelope.code,
                envelope
                    .message
                    .unwrap_or_else(|| "unknown error".to_string())
            ));
        }
        Ok(())
    }

    async fn send_binary(&self, builder: reqwest::RequestBuilder) -> Result<Vec<u8>> {
        let response = builder.send().await?;
        let status = response.status();
        let bytes = response.bytes().await?;
        if !status.is_success() {
            let text = String::from_utf8_lossy(&bytes);
            return Err(anyhow!("RAGFlow request failed ({status}): {text}"));
        }
        Ok(bytes.to_vec())
    }
}

pub fn ensure_ragflow_base_type(base: &KnowledgeBaseConfig) -> Result<()> {
    if base.base_type() != KnowledgeBaseType::Ragflow {
        return Err(anyhow!("RAGFlow knowledge base is required"));
    }
    Ok(())
}

pub fn ensure_ragflow_base_config(base: &KnowledgeBaseConfig) -> Result<&str> {
    ensure_ragflow_base_type(base)?;
    dataset_id(base)
}

pub fn dataset_id(base: &KnowledgeBaseConfig) -> Result<&str> {
    base.ragflow_dataset_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("RAGFlow dataset id is missing"))
}

pub fn resolve_top_k(base: &KnowledgeBaseConfig) -> usize {
    base.top_k.unwrap_or(DEFAULT_TOP_K).max(1)
}

pub fn normalize_chunk_method(raw: Option<&str>) -> Option<String> {
    let value = raw?.trim().to_ascii_lowercase();
    if value.is_empty() {
        return None;
    }
    let normalized = match value.as_str() {
        "general" => DEFAULT_CHUNK_METHOD,
        "q&a" | "qna" => "qa",
        other => other,
    };
    SUPPORTED_CHUNK_METHODS
        .contains(&normalized)
        .then(|| normalized.to_string())
}

pub fn resolve_chunk_method(base: &KnowledgeBaseConfig) -> String {
    normalize_chunk_method(base.chunk_method.as_deref())
        .unwrap_or_else(|| DEFAULT_CHUNK_METHOD.to_string())
}

fn method_supports(method: &str, methods: &[&str]) -> bool {
    methods.contains(&method)
}

pub fn resolve_chunk_token_num(base: &KnowledgeBaseConfig) -> Option<usize> {
    if !method_supports(&resolve_chunk_method(base), TEXT_CONTROL_CHUNK_METHODS) {
        return None;
    }
    base.chunk_size
        .filter(|value| *value > 0)
        .map(|value| value.min(2048))
}

pub fn normalize_chunk_delimiter(raw: Option<&str>) -> Option<String> {
    let value = raw?;
    if value.is_empty() {
        return None;
    }
    Some(
        value
            .replace("\\r\\n", "\n")
            .replace("\\n", "\n")
            .replace("\\t", "\t"),
    )
}

pub fn normalize_layout_recognize(raw: Option<&str>) -> Option<String> {
    let value = raw?.trim();
    if value.is_empty() {
        return None;
    }
    SUPPORTED_LAYOUT_RECOGNIZERS
        .iter()
        .find(|candidate| candidate.eq_ignore_ascii_case(value))
        .map(|candidate| (*candidate).to_string())
}

pub fn build_dataset_parser_config(base: &KnowledgeBaseConfig) -> Option<Value> {
    let method = resolve_chunk_method(base);
    let mut parser_config = serde_json::Map::new();

    if method_supports(&method, TEXT_CONTROL_CHUNK_METHODS) {
        if let Some(chunk_size) = resolve_chunk_token_num(base) {
            parser_config.insert("chunk_token_num".to_string(), json!(chunk_size));
        }
        if let Some(delimiter) = normalize_chunk_delimiter(base.chunk_delimiter.as_deref()) {
            parser_config.insert("delimiter".to_string(), json!(delimiter));
        }
    }

    if method_supports(&method, LAYOUT_CHUNK_METHODS) {
        if let Some(layout) = normalize_layout_recognize(base.layout_recognize.as_deref()) {
            parser_config.insert("layout_recognize".to_string(), json!(layout));
        }
    }

    if method_supports(&method, AUTO_FIELD_CHUNK_METHODS) {
        if let Some(auto_keywords) = base.auto_keywords.filter(|value| *value > 0) {
            parser_config.insert("auto_keywords".to_string(), json!(auto_keywords.min(32)));
        }
        if let Some(auto_questions) = base.auto_questions.filter(|value| *value > 0) {
            parser_config.insert("auto_questions".to_string(), json!(auto_questions.min(10)));
        }
    }

    if method_supports(&method, HTML_EXCEL_CHUNK_METHODS) {
        if let Some(html4excel) = base.html4excel {
            parser_config.insert("html4excel".to_string(), json!(html4excel));
        }
    }

    (!parser_config.is_empty()).then_some(Value::Object(parser_config))
}

fn build_parser_update_body(base: &KnowledgeBaseConfig) -> Value {
    let mut body = json!({ "chunk_method": resolve_chunk_method(base) });
    body["parser_config"] = build_dataset_parser_config(base).unwrap_or_else(|| json!({}));
    body
}

fn document_parser_config_matches(base: &KnowledgeBaseConfig, doc: &RagflowDocument) -> bool {
    let expected_method = resolve_chunk_method(base);
    let current_method = doc
        .parser_id
        .as_deref()
        .and_then(|value| normalize_chunk_method(Some(value)))
        .unwrap_or_else(|| DEFAULT_CHUNK_METHOD.to_string());
    if expected_method != current_method {
        return false;
    }
    build_dataset_parser_config(base).unwrap_or_else(|| json!({}))
        == doc.parser_config.clone().unwrap_or_else(|| json!({}))
}

pub fn synthetic_root(dataset_id: &str) -> String {
    let cleaned = dataset_id.trim();
    if cleaned.is_empty() {
        "ragflow:pending".to_string()
    } else {
        format!("ragflow:{cleaned}")
    }
}

pub fn normalize_dataset_id(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .strip_prefix("ragflow:")
                .unwrap_or(value)
                .trim()
                .to_string()
        })
        .filter(|value| !value.is_empty())
}

pub fn normalize_synthetic_root_dataset_id(root: &str) -> Option<String> {
    let raw = root.trim();
    let value = raw.strip_prefix("ragflow:")?;
    normalize_dataset_id(Some(value))
}

pub async fn create_dataset(config: &Config, base: &KnowledgeBaseConfig) -> Result<String> {
    let client = RagflowClient::from_config(config)?;
    let name = base.name.trim();
    if name.is_empty() {
        return Err(anyhow!(i18n::t("error.knowledge_base_name_required")));
    }
    let mut body = json!({
        "name": name,
        "chunk_method": resolve_chunk_method(base),
    });
    if !base.description.trim().is_empty() {
        body["description"] = json!(base.description.trim());
    }
    if let Some(parser_config) = build_dataset_parser_config(base) {
        body["parser_config"] = parser_config;
    }
    let dataset: RagflowDataset = client
        .send_json(client.request(Method::POST, "/api/v1/datasets").json(&body))
        .await?;
    Ok(dataset.id)
}

pub async fn update_dataset_parser_config(
    config: &Config,
    base: &KnowledgeBaseConfig,
) -> Result<()> {
    let dataset_id = ensure_ragflow_base_config(base)?;
    let client = RagflowClient::from_config(config)?;
    let body = build_parser_update_body(base);
    client
        .send_empty(
            client
                .request(Method::PUT, &format!("/api/v1/datasets/{dataset_id}"))
                .json(&body),
        )
        .await
}

pub async fn sync_parser_config_and_reparse_documents(
    config: &Config,
    base: &KnowledgeBaseConfig,
) -> Result<usize> {
    update_dataset_parser_config(config, base).await?;

    let dataset_id = ensure_ragflow_base_config(base)?.to_string();
    let docs = list_raw_documents(config, &dataset_id).await?;
    let doc_ids = docs
        .iter()
        .filter(|doc| !document_parser_config_matches(base, doc))
        .map(|doc| doc.id.clone())
        .collect::<Vec<_>>();
    if doc_ids.is_empty() {
        return Ok(0);
    }

    let client = RagflowClient::from_config(config)?;
    let body = build_parser_update_body(base);
    for doc_id in &doc_ids {
        client
            .send_empty(
                client
                    .request(
                        Method::PATCH,
                        &format!("/api/v1/datasets/{dataset_id}/documents/{doc_id}"),
                    )
                    .json(&body),
            )
            .await?;
    }
    parse_documents(config, &dataset_id, &doc_ids).await?;
    Ok(doc_ids.len())
}

pub async fn delete_datasets(config: &Config, dataset_ids: &[String]) -> Result<()> {
    let ids = dataset_ids
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(());
    }
    let client = RagflowClient::from_config(config)?;
    client
        .send_empty(
            client
                .request(Method::DELETE, "/api/v1/datasets")
                .json(&json!({ "ids": ids })),
        )
        .await
}

pub async fn list_documents(
    config: &Config,
    base: &KnowledgeBaseConfig,
) -> Result<Vec<VectorDocumentSummary>> {
    let dataset_id = ensure_ragflow_base_config(base)?;
    Ok(list_raw_documents(config, dataset_id)
        .await?
        .into_iter()
        .map(map_document_summary)
        .collect())
}

pub async fn read_document_meta(
    config: &Config,
    base: &KnowledgeBaseConfig,
    doc_id: &str,
) -> Result<VectorDocumentMeta> {
    let doc_id = doc_id.trim();
    if doc_id.is_empty() {
        return Err(anyhow!(i18n::t("error.knowledge_document_not_found")));
    }
    let dataset_id = ensure_ragflow_base_config(base)?;
    let client = RagflowClient::from_config(config)?;
    let path = format!("/api/v1/datasets/{dataset_id}/documents?page=1&page_size=1&id={doc_id}");
    let data: RagflowDocumentListData =
        client.send_json(client.request(Method::GET, &path)).await?;
    let doc = data
        .docs
        .into_iter()
        .find(|item| item.id == doc_id)
        .ok_or_else(|| anyhow!(i18n::t("error.knowledge_document_not_found")))?;
    Ok(map_document_meta(doc))
}

pub async fn download_document_content(
    config: &Config,
    base: &KnowledgeBaseConfig,
    doc_id: &str,
) -> Result<String> {
    let dataset_id = ensure_ragflow_base_config(base)?;
    let client = RagflowClient::from_config(config)?;
    let path = format!("/api/v1/datasets/{dataset_id}/documents/{}", doc_id.trim());
    let bytes = client
        .send_binary(client.request(Method::GET, &path))
        .await?;
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

pub async fn upload_document(
    config: &Config,
    base: &KnowledgeBaseConfig,
    upload: RagflowUpload,
) -> Result<VectorDocumentSummary> {
    let dataset_id = ensure_ragflow_base_config(base)?.to_string();
    let client = RagflowClient::from_config(config)?;
    let bytes = tokio::fs::read(&upload.input_path).await?;
    let file_name = sanitize_upload_filename(&upload.filename, &upload.input_path);
    let part = Part::bytes(bytes).file_name(file_name);
    let form = Form::new().part("file", part);
    let docs: Vec<RagflowDocument> = client
        .send_json(
            client
                .request(
                    Method::POST,
                    &format!("/api/v1/datasets/{dataset_id}/documents"),
                )
                .multipart(form),
        )
        .await?;
    let doc = docs
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("RAGFlow upload response missing document"))?;
    parse_documents(config, &dataset_id, std::slice::from_ref(&doc.id)).await?;
    Ok(map_document_summary(doc))
}

pub async fn delete_document(
    config: &Config,
    base: &KnowledgeBaseConfig,
    doc_id: &str,
) -> Result<()> {
    let dataset_id = ensure_ragflow_base_config(base)?;
    let client = RagflowClient::from_config(config)?;
    let body = json!({ "ids": [doc_id.trim()] });
    client
        .send_empty(
            client
                .request(
                    Method::DELETE,
                    &format!("/api/v1/datasets/{dataset_id}/documents"),
                )
                .json(&body),
        )
        .await
}

pub async fn list_chunks(
    config: &Config,
    base: &KnowledgeBaseConfig,
    doc_id: &str,
) -> Result<Vec<VectorChunkPreview>> {
    let chunks = list_raw_chunks(config, base, doc_id).await?;
    Ok(chunks
        .into_iter()
        .enumerate()
        .map(|(index, chunk)| map_chunk_preview(index, chunk))
        .collect())
}

pub async fn update_chunk(
    config: &Config,
    base: &KnowledgeBaseConfig,
    doc_id: &str,
    chunk_index: usize,
    content: &str,
) -> Result<()> {
    let chunk = resolve_chunk_by_index(config, base, doc_id, chunk_index).await?;
    let dataset_id = ensure_ragflow_base_config(base)?;
    let client = RagflowClient::from_config(config)?;
    let body = json!({ "content": content });
    client
        .send_empty(
            client
                .request(
                    Method::PATCH,
                    &format!(
                        "/api/v1/datasets/{dataset_id}/documents/{}/chunks/{}",
                        doc_id.trim(),
                        chunk.id
                    ),
                )
                .json(&body),
        )
        .await
}

pub async fn set_chunk_available(
    config: &Config,
    base: &KnowledgeBaseConfig,
    doc_id: &str,
    chunk_index: usize,
    available: bool,
) -> Result<()> {
    let chunk = resolve_chunk_by_index(config, base, doc_id, chunk_index).await?;
    let dataset_id = ensure_ragflow_base_config(base)?;
    let client = RagflowClient::from_config(config)?;
    let body = json!({ "chunk_ids": [chunk.id], "available": available });
    client
        .send_empty(
            client
                .request(
                    Method::PATCH,
                    &format!(
                        "/api/v1/datasets/{dataset_id}/documents/{}/chunks",
                        doc_id.trim()
                    ),
                )
                .json(&body),
        )
        .await
}

pub async fn delete_chunk(
    config: &Config,
    base: &KnowledgeBaseConfig,
    doc_id: &str,
    chunk_index: usize,
) -> Result<()> {
    let chunk = resolve_chunk_by_index(config, base, doc_id, chunk_index).await?;
    let dataset_id = ensure_ragflow_base_config(base)?;
    let client = RagflowClient::from_config(config)?;
    let body = json!({ "chunk_ids": [chunk.id] });
    client
        .send_empty(
            client
                .request(
                    Method::DELETE,
                    &format!(
                        "/api/v1/datasets/{dataset_id}/documents/{}/chunks",
                        doc_id.trim()
                    ),
                )
                .json(&body),
        )
        .await
}

pub async fn reparse_documents(
    config: &Config,
    base: &KnowledgeBaseConfig,
    doc_ids: &[String],
) -> Result<()> {
    let dataset_id = ensure_ragflow_base_config(base)?;
    parse_documents(config, dataset_id, doc_ids).await
}

pub async fn retrieve(
    config: &Config,
    base: &KnowledgeBaseConfig,
    question: &str,
    top_k: usize,
) -> Result<Vec<RagflowSearchHit>> {
    let question = question.trim();
    if question.is_empty() {
        return Err(anyhow!(i18n::t("error.knowledge_query_required")));
    }
    let dataset_id = ensure_ragflow_base_config(base)?;
    let client = RagflowClient::from_config(config)?;
    let top_k = top_k.max(1);
    let body = json!({
        "question": question,
        "dataset_ids": [dataset_id],
        "page": 1,
        "page_size": top_k,
        "top_k": top_k,
        "similarity_threshold": base.score_threshold.unwrap_or(0.2),
    });
    let data: RagflowRetrievalData = client
        .send_json(
            client
                .request(Method::POST, "/api/v1/retrieval")
                .json(&body),
        )
        .await?;
    Ok(data
        .chunks
        .into_iter()
        .enumerate()
        .map(|(index, chunk)| map_retrieval_hit(index, chunk))
        .collect())
}

async fn parse_documents(config: &Config, dataset_id: &str, doc_ids: &[String]) -> Result<()> {
    let ids = doc_ids
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(());
    }
    let client = RagflowClient::from_config(config)?;
    let body = json!({ "document_ids": ids });
    client
        .send_empty(
            client
                .request(
                    Method::POST,
                    &format!("/api/v1/datasets/{dataset_id}/chunks"),
                )
                .json(&body),
        )
        .await
}

async fn list_raw_documents(config: &Config, dataset_id: &str) -> Result<Vec<RagflowDocument>> {
    let client = RagflowClient::from_config(config)?;
    let path = format!(
        "/api/v1/datasets/{}/documents?page=1&page_size={}&orderby=update_time&desc=true",
        dataset_id, DEFAULT_PAGE_SIZE
    );
    let data: RagflowDocumentListData =
        client.send_json(client.request(Method::GET, &path)).await?;
    Ok(data.docs)
}

async fn list_raw_chunks(
    config: &Config,
    base: &KnowledgeBaseConfig,
    doc_id: &str,
) -> Result<Vec<RagflowChunk>> {
    let doc_id = doc_id.trim();
    if doc_id.is_empty() {
        return Err(anyhow!(i18n::t("error.knowledge_document_not_found")));
    }
    let dataset_id = ensure_ragflow_base_config(base)?;
    let client = RagflowClient::from_config(config)?;
    let path = format!(
        "/api/v1/datasets/{dataset_id}/documents/{doc_id}/chunks?page=1&page_size={DEFAULT_PAGE_SIZE}"
    );
    let data: RagflowChunkListData = client.send_json(client.request(Method::GET, &path)).await?;
    let _ = data.doc;
    Ok(data.chunks)
}

async fn resolve_chunk_by_index(
    config: &Config,
    base: &KnowledgeBaseConfig,
    doc_id: &str,
    chunk_index: usize,
) -> Result<RagflowChunk> {
    list_raw_chunks(config, base, doc_id)
        .await?
        .into_iter()
        .enumerate()
        .find_map(|(index, chunk)| (index == chunk_index).then_some(chunk))
        .ok_or_else(|| anyhow!(i18n::t("error.knowledge_chunk_not_found")))
}

fn map_document_summary(doc: RagflowDocument) -> VectorDocumentSummary {
    let updated_at = parse_time(doc.update_time.as_ref())
        .or_else(|| parse_time(doc.create_time.as_ref()))
        .unwrap_or_else(now_ts);
    VectorDocumentSummary {
        doc_id: doc.id,
        name: doc
            .name
            .or(doc.location)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "document".to_string()),
        status: map_document_status(doc.run.as_ref().or(doc.status.as_ref())),
        chunk_count: doc.chunk_count.or(doc.token_count).unwrap_or(0),
        embedding_model: "ragflow".to_string(),
        updated_at,
    }
}

fn map_document_meta(doc: RagflowDocument) -> VectorDocumentMeta {
    let summary = map_document_summary(doc);
    VectorDocumentMeta {
        doc_id: summary.doc_id,
        name: summary.name,
        embedding_model: "ragflow".to_string(),
        chunk_size: 0,
        chunk_overlap: 0,
        chunk_count: summary.chunk_count,
        status: summary.status,
        created_at: summary.updated_at,
        updated_at: summary.updated_at,
        chunks: Vec::new(),
    }
}

fn map_chunk_preview(index: usize, chunk: RagflowChunk) -> VectorChunkPreview {
    let content = chunk_content(&chunk);
    let status = match (chunk.available, chunk.available_int) {
        (Some(false), _) | (_, Some(0)) => "deleted",
        _ => "embedded",
    }
    .to_string();
    VectorChunkPreview {
        index,
        start: 0,
        end: content.chars().count(),
        chunk_id: Some(chunk.id),
        preview: preview_text(&content),
        content,
        status,
    }
}

fn map_retrieval_hit(index: usize, chunk: RagflowRetrievalChunk) -> RagflowSearchHit {
    let content = chunk.content.unwrap_or_default();
    RagflowSearchHit {
        doc_id: chunk.document_id.unwrap_or_default(),
        doc_name: chunk
            .document_keyword
            .or(chunk.docnm_kwd)
            .unwrap_or_else(|| "document".to_string()),
        chunk_id: chunk.id.unwrap_or_default(),
        chunk_index: index,
        start: 0,
        end: content.chars().count(),
        content,
        score: chunk.similarity.or(chunk.score),
    }
}

fn chunk_content(chunk: &RagflowChunk) -> String {
    chunk
        .content
        .clone()
        .or_else(|| chunk.content_with_weight.clone())
        .unwrap_or_default()
}

fn preview_text(content: &str) -> String {
    let trimmed = content.trim();
    let mut output = trimmed.chars().take(160).collect::<String>();
    if trimmed.chars().count() > 160 {
        output.push_str("...");
    }
    output
}

fn map_document_status(value: Option<&Value>) -> String {
    let raw = value
        .and_then(|value| match value {
            Value::String(text) => Some(text.trim().to_ascii_uppercase()),
            Value::Number(num) => Some(num.to_string()),
            _ => None,
        })
        .unwrap_or_default();
    match raw.as_str() {
        "3" | "DONE" => "ready",
        "1" | "RUNNING" => "indexing",
        "4" | "FAIL" => "failed",
        "2" | "CANCEL" => "failed",
        _ => "pending",
    }
    .to_string()
}

fn parse_time(value: Option<&Value>) -> Option<f64> {
    let raw = match value? {
        Value::Number(num) => num.as_f64()?,
        Value::String(text) => text.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    if raw > 1_000_000_000_000.0 {
        Some(raw / 1000.0)
    } else {
        Some(raw)
    }
}

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

fn sanitize_upload_filename(filename: &str, path: &Path) -> String {
    let candidate = filename.trim();
    if !candidate.is_empty() {
        return candidate.to_string();
    }
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("upload")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        build_dataset_parser_config, normalize_chunk_method, normalize_dataset_id,
        normalize_synthetic_root_dataset_id, resolve_chunk_method, resolve_chunk_token_num,
    };
    use crate::config::KnowledgeBaseConfig;

    #[test]
    fn normalizes_dataset_ids() {
        assert_eq!(
            normalize_dataset_id(Some(" ragflow:dataset_id ")),
            Some("dataset_id".to_string())
        );
        assert_eq!(
            normalize_dataset_id(Some(" dataset_id ")),
            Some("dataset_id".to_string())
        );
        assert_eq!(normalize_dataset_id(Some("   ")), None);
    }

    #[test]
    fn synthetic_root_dataset_id_requires_prefix() {
        assert_eq!(
            normalize_synthetic_root_dataset_id(" ragflow:dataset_id "),
            Some("dataset_id".to_string())
        );
        assert_eq!(
            normalize_synthetic_root_dataset_id("config/knowledge/base"),
            None
        );
        assert_eq!(normalize_synthetic_root_dataset_id(""), None);
    }

    #[test]
    fn normalizes_ragflow_chunk_method() {
        assert_eq!(
            normalize_chunk_method(Some(" General ")),
            Some("naive".to_string())
        );
        assert_eq!(normalize_chunk_method(Some("Q&A")), Some("qa".to_string()));
        assert_eq!(
            normalize_chunk_method(Some("table")),
            Some("table".to_string())
        );
        assert_eq!(
            normalize_chunk_method(Some("resume")),
            Some("resume".to_string())
        );
        assert_eq!(normalize_chunk_method(Some("unknown")), None);

        let config = KnowledgeBaseConfig {
            chunk_method: Some("paper".to_string()),
            chunk_size: Some(3000),
            ..Default::default()
        };
        assert_eq!(resolve_chunk_method(&config), "paper");
        assert_eq!(resolve_chunk_token_num(&config), None);
    }

    #[test]
    fn builds_parser_config_by_ragflow_chunk_method() {
        let naive = KnowledgeBaseConfig {
            chunk_method: Some("naive".to_string()),
            chunk_size: Some(3000),
            chunk_delimiter: Some("\\n##".to_string()),
            layout_recognize: Some("plain text".to_string()),
            auto_keywords: Some(99),
            auto_questions: Some(42),
            html4excel: Some(true),
            ..Default::default()
        };
        let parser_config = build_dataset_parser_config(&naive).unwrap();
        assert_eq!(parser_config["chunk_token_num"], 2048);
        assert_eq!(parser_config["delimiter"], "\n##");
        assert_eq!(parser_config["layout_recognize"], "Plain Text");
        assert_eq!(parser_config["auto_keywords"], 32);
        assert_eq!(parser_config["auto_questions"], 10);
        assert_eq!(parser_config["html4excel"], true);

        let qa = KnowledgeBaseConfig {
            chunk_method: Some("qa".to_string()),
            chunk_size: Some(512),
            chunk_delimiter: Some("\\n".to_string()),
            auto_keywords: Some(3),
            ..Default::default()
        };
        assert_eq!(build_dataset_parser_config(&qa), None);
    }
}
