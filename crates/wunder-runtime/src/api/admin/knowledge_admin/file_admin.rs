use super::{
    resolve_knowledge_base, resolve_knowledge_path, resolve_knowledge_root,
    resolve_vector_root_for_admin, vector_error_response,
};
use crate::api::admin::error_response;
use crate::attachment::{convert_to_markdown, get_supported_extensions, sanitize_filename_stem};
use crate::config::KnowledgeBaseConfig;
use crate::i18n;
use crate::knowledge;
use crate::services::ragflow_knowledge;
use crate::state::AppState;
use crate::vector_knowledge;
use axum::extract::{Form, Multipart, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::{routing::get, routing::post, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;
use walkdir::WalkDir;

const MAX_KNOWLEDGE_UPLOAD_BYTES: usize = 20 * 1024 * 1024;
const MAX_KNOWLEDGE_CONTENT_BYTES: usize = 10 * 1024 * 1024;

pub(super) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/admin/knowledge/files", get(admin_knowledge_files))
        .route(
            "/wunder/admin/knowledge/file",
            get(admin_knowledge_file)
                .put(admin_knowledge_file_update)
                .delete(admin_knowledge_file_delete),
        )
        .route(
            "/wunder/admin/knowledge/upload",
            post(admin_knowledge_upload),
        )
        .route(
            "/wunder/admin/knowledge/refresh",
            post(admin_knowledge_refresh),
        )
}

async fn admin_knowledge_files(
    State(state): State<Arc<AppState>>,
    Query(query): Query<KnowledgeFilesQuery>,
) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let base = resolve_knowledge_base(&config, &query.base)?;
    ensure_literal_file_base(&base)?;
    let root = resolve_knowledge_root(&base, false)?;
    let files = list_markdown_files(&root);
    Ok(Json(json!({ "base": query.base, "files": files })))
}

async fn admin_knowledge_file(
    State(state): State<Arc<AppState>>,
    Query(query): Query<KnowledgeFileQuery>,
) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let base = resolve_knowledge_base(&config, &query.base)?;
    ensure_literal_file_base(&base)?;
    let root = resolve_knowledge_root(&base, false)?;
    let target = resolve_knowledge_path(&root, &query.path)?;
    ensure_markdown_path(&target)?;
    if !target.exists() || !target.is_file() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("error.file_not_found"),
        ));
    }
    let content = tokio::fs::read_to_string(&target)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "base": query.base,
        "path": query.path,
        "content": content
    })))
}

async fn admin_knowledge_file_update(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<KnowledgeFileUpdate>,
) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let base = resolve_knowledge_base(&config, &payload.base)?;
    ensure_literal_file_base(&base)?;
    let root = resolve_knowledge_root(&base, true)?;
    let target = resolve_knowledge_path(&root, &payload.path)?;
    ensure_markdown_path(&target)?;
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await.ok();
    }
    tokio::fs::write(&target, payload.content)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    refresh_literal_knowledge_cache(&base, &root).await;
    Ok(Json(
        json!({ "ok": true, "message": i18n::t("message.saved_and_reindexed") }),
    ))
}

async fn admin_knowledge_file_delete(
    State(state): State<Arc<AppState>>,
    Query(query): Query<KnowledgeFileQuery>,
) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let base = resolve_knowledge_base(&config, &query.base)?;
    ensure_literal_file_base(&base)?;
    let root = resolve_knowledge_root(&base, true)?;
    let target = resolve_knowledge_path(&root, &query.path)?;
    ensure_markdown_path(&target)?;
    if target.exists() && target.is_file() {
        tokio::fs::remove_file(&target)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        refresh_literal_knowledge_cache(&base, &root).await;
    }
    Ok(Json(
        json!({ "ok": true, "message": i18n::t("message.deleted") }),
    ))
}

async fn admin_knowledge_upload(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<Value>, Response> {
    let mut base = String::new();
    let mut upload: Option<UploadedKnowledgeFile> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    {
        let field_name = field.name().unwrap_or("");
        if field_name == "base" {
            base = field.text().await.unwrap_or_default();
            continue;
        }
        let is_upload_field = matches!(field_name, "file" | "files") || field.file_name().is_some();
        if !is_upload_field {
            continue;
        }
        if let Some(previous) = upload.take() {
            let _ = tokio::fs::remove_dir_all(&previous.temp_dir).await;
        }
        upload = Some(save_knowledge_upload_field(field).await?);
    }
    if base.trim().is_empty() {
        if let Some(previous) = upload {
            let _ = tokio::fs::remove_dir_all(&previous.temp_dir).await;
        }
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.knowledge_base_name_required"),
        ));
    }
    let upload = match upload {
        Some(value) => value,
        None => {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("error.file_not_found"),
            ))
        }
    };
    let config = state.config_store.get().await;
    let base_config = resolve_knowledge_base(&config, &base)?;
    if base_config.is_ragflow() {
        let temp_dir = upload.temp_dir.clone();
        let result = ragflow_knowledge::upload_document(
            &config,
            &base_config,
            ragflow_knowledge::RagflowUpload {
                filename: upload.filename.clone(),
                input_path: upload.input_path.clone(),
            },
        )
        .await;
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
        let doc = result.map_err(vector_error_response)?;
        return Ok(Json(json!({
            "ok": true,
            "message": i18n::t("message.upload_converted"),
            "doc_id": doc.doc_id,
            "doc_name": doc.name,
            "chunk_count": doc.chunk_count,
            "embedding_model": doc.embedding_model,
            "converter": "ragflow",
            "warnings": []
        })));
    }
    if base_config.is_vector() {
        let root = resolve_vector_root_for_admin(&base_config, true)?;
        let storage = state.storage.clone();
        let temp_dir = upload.temp_dir.clone();
        let result = convert_upload_to_markdown(&upload).await;
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
        let (content, converter, warnings) = result?;
        let doc_name = upload.stem.clone();
        let existing = vector_knowledge::list_vector_documents(
            storage.as_ref(),
            None,
            &base_config.name,
            &root,
        )
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        let mut doc_id: Option<String> = None;
        let mut previous_meta = None;
        if let Some(doc) = existing.iter().find(|doc| doc.name == doc_name) {
            doc_id = Some(doc.doc_id.clone());
            previous_meta = vector_knowledge::read_vector_document_meta(
                storage.as_ref(),
                None,
                &base_config.name,
                &root,
                &doc.doc_id,
            )
            .await
            .ok();
        }
        let meta = vector_knowledge::index_document(
            &config,
            &base_config,
            None,
            storage.as_ref(),
            &root,
            &doc_name,
            doc_id.as_deref(),
            &content,
            previous_meta.as_ref(),
        )
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        return Ok(Json(json!({
            "ok": true,
            "message": i18n::t("message.upload_converted"),
            "doc_id": meta.doc_id,
            "doc_name": meta.name,
            "chunk_count": meta.chunk_count,
            "embedding_model": meta.embedding_model,
            "converter": converter,
            "warnings": warnings
        })));
    }
    let root = resolve_knowledge_root(&base_config, true)?;
    let output_name = build_markdown_output_path(&upload.filename, &upload.stem);
    let target = resolve_knowledge_path(&root, &output_name)?;
    let temp_dir = upload.temp_dir.clone();
    let result = persist_knowledge_upload(&upload, &target).await;
    let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    let (converter, warnings) = result?;
    cleanup_non_markdown_upload(&root, &upload.filename, &output_name).await;
    refresh_literal_knowledge_cache(&base_config, &root).await;
    Ok(Json(json!({
        "ok": true,
        "message": i18n::t("message.upload_converted"),
        "path": output_name,
        "converter": converter,
        "warnings": warnings
    })))
}

async fn admin_knowledge_refresh(
    State(state): State<Arc<AppState>>,
    Form(payload): Form<KnowledgeRefreshForm>,
) -> Result<Json<Value>, Response> {
    let base = payload.base.trim();
    if base.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.knowledge_base_name_required"),
        ));
    }
    let config = state.config_store.get().await;
    let base_config = resolve_knowledge_base(&config, base)?;
    ensure_literal_file_base(&base_config)?;
    let root = resolve_knowledge_root(&base_config, true)?;
    refresh_literal_knowledge_cache(&base_config, &root).await;
    Ok(Json(
        json!({ "ok": true, "message": i18n::t("message.index_refreshed") }),
    ))
}

fn ensure_literal_file_base(base: &KnowledgeBaseConfig) -> Result<(), Response> {
    if base.is_vector() || base.is_ragflow() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.vector_knowledge_not_file_based"),
        ));
    }
    Ok(())
}

fn ensure_markdown_path(path: &Path) -> Result<(), Response> {
    if path.extension().and_then(|ext| ext.to_str()).unwrap_or("") != "md" {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.markdown_only"),
        ));
    }
    Ok(())
}

async fn refresh_literal_knowledge_cache(base: &KnowledgeBaseConfig, root: &Path) {
    knowledge::refresh_knowledge_cache(&KnowledgeBaseConfig {
        name: base.name.clone(),
        description: base.description.clone(),
        root: root.to_string_lossy().to_string(),
        enabled: base.enabled,
        shared: base.shared,
        base_type: base.base_type.clone(),
        embedding_model: base.embedding_model.clone(),
        ragflow_dataset_id: base.ragflow_dataset_id.clone(),
        ragflow_dataset_managed: base.ragflow_dataset_managed,
        chunk_method: base.chunk_method.clone(),
        chunk_delimiter: base.chunk_delimiter.clone(),
        layout_recognize: base.layout_recognize.clone(),
        auto_keywords: base.auto_keywords,
        auto_questions: base.auto_questions,
        html4excel: base.html4excel,
        chunk_size: base.chunk_size,
        chunk_overlap: base.chunk_overlap,
        top_k: base.top_k,
        score_threshold: base.score_threshold,
    })
    .await;
}

struct UploadedKnowledgeFile {
    filename: String,
    extension: String,
    stem: String,
    temp_dir: PathBuf,
    input_path: PathBuf,
}

async fn save_knowledge_upload_field(
    field: axum::extract::multipart::Field<'_>,
) -> Result<UploadedKnowledgeFile, Response> {
    let filename = field.file_name().unwrap_or("upload").to_string();
    let extension = Path::new(&filename)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase();
    if extension.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.file_extension_missing"),
        ));
    }
    let extension = format!(".{extension}");
    let supported = get_supported_extensions();
    if !supported
        .iter()
        .any(|item| item.eq_ignore_ascii_case(&extension))
    {
        let message = i18n::t_with_params(
            "error.unsupported_file_type",
            &HashMap::from([("extension".to_string(), extension.clone())]),
        );
        return Err(error_response(StatusCode::BAD_REQUEST, message));
    }
    let stem_raw = Path::new(&filename)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("document");
    let stem = sanitize_filename_stem(stem_raw);
    let stem = if stem.trim().is_empty() {
        "document".to_string()
    } else {
        stem
    };
    let temp_dir = create_knowledge_temp_dir().await?;
    let input_path = temp_dir.join(format!("{stem}{extension}"));
    save_knowledge_upload_content(field, &input_path).await?;
    Ok(UploadedKnowledgeFile {
        filename,
        extension,
        stem,
        temp_dir,
        input_path,
    })
}

fn build_markdown_output_path(filename: &str, stem: &str) -> String {
    let raw_path = Path::new(filename);
    let output_name = format!("{stem}.md");
    let output = match raw_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        Some(parent) => parent.join(output_name),
        None => PathBuf::from(output_name),
    };
    output.to_string_lossy().replace('\\', "/")
}

async fn persist_knowledge_upload(
    upload: &UploadedKnowledgeFile,
    target: &Path,
) -> Result<(String, Vec<String>), Response> {
    let output_path = upload.temp_dir.join(format!("{}.md", upload.stem));
    let conversion = convert_to_markdown(&upload.input_path, &output_path, &upload.extension)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let content = read_converted_markdown(&output_path).await?;
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await.ok();
    }
    tokio::fs::write(target, content)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok((conversion.converter, conversion.warnings))
}

async fn convert_upload_to_markdown(
    upload: &UploadedKnowledgeFile,
) -> Result<(String, String, Vec<String>), Response> {
    let output_path = upload.temp_dir.join(format!("{}.md", upload.stem));
    let conversion = convert_to_markdown(&upload.input_path, &output_path, &upload.extension)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let content = read_converted_markdown(&output_path).await?;
    Ok((content, conversion.converter, conversion.warnings))
}

async fn read_converted_markdown(output_path: &Path) -> Result<String, Response> {
    let metadata = tokio::fs::metadata(output_path)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if metadata.len() > MAX_KNOWLEDGE_CONTENT_BYTES as u64 {
        return Err(error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            i18n::t("tool.read.too_large"),
        ));
    }
    let content = tokio::fs::read_to_string(output_path)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if content.len() > MAX_KNOWLEDGE_CONTENT_BYTES {
        return Err(error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            i18n::t("tool.read.too_large"),
        ));
    }
    if content.trim().is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.empty_parse_result"),
        ));
    }
    Ok(content)
}

async fn create_knowledge_temp_dir() -> Result<PathBuf, Response> {
    let mut root = std::env::temp_dir();
    root.push("wunder_uploads");
    root.push(Uuid::new_v4().simple().to_string());
    tokio::fs::create_dir_all(&root)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(root)
}

async fn save_knowledge_upload_content(
    mut field: axum::extract::multipart::Field<'_>,
    target: &Path,
) -> Result<(), Response> {
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
    let mut file = tokio::fs::File::create(target)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let mut total = 0usize;
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    {
        total = total.saturating_add(chunk.len());
        if total > MAX_KNOWLEDGE_UPLOAD_BYTES {
            let _ = tokio::fs::remove_file(target).await;
            return Err(error_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                i18n::t("workspace.error.upload_too_large"),
            ));
        }
        file.write_all(&chunk)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
    Ok(())
}

async fn cleanup_non_markdown_upload(root: &Path, filename: &str, output_name: &str) {
    if filename == output_name {
        return;
    }
    let raw_path = match resolve_knowledge_path(root, filename) {
        Ok(path) => path,
        Err(_) => return,
    };
    let is_markdown = raw_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("md"))
        .unwrap_or(false);
    if is_markdown {
        return;
    }
    if raw_path.exists() && raw_path.is_file() {
        let _ = tokio::fs::remove_file(raw_path).await;
    }
}

fn list_markdown_files(root: &Path) -> Vec<String> {
    if !root.exists() || !root.is_dir() {
        return Vec::new();
    }
    let mut files = Vec::new();
    for entry in WalkDir::new(root).into_iter().filter_map(|item| item.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        let rel = path.strip_prefix(root).unwrap_or(path);
        files.push(rel.to_string_lossy().replace('\\', "/"));
    }
    files.sort();
    files
}

#[derive(Debug, Deserialize)]
struct KnowledgeFilesQuery {
    base: String,
}

#[derive(Debug, Deserialize)]
struct KnowledgeFileQuery {
    base: String,
    path: String,
}

#[derive(Debug, Deserialize)]
struct KnowledgeFileUpdate {
    base: String,
    path: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct KnowledgeRefreshForm {
    base: String,
}
