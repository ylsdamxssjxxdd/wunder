use super::*;
use crate::config::{normalize_knowledge_base_type, KnowledgeBaseConfig, KnowledgeBaseType};
use crate::knowledge;
use crate::llm;
use crate::ragflow_knowledge;
use crate::storage::UserAccountRecord;
use crate::user_tools::UserKnowledgeBase;
use crate::vector_knowledge;
use std::io::ErrorKind;
mod types;
mod upload;

use types::UserKnowledgeBasePayload;

use upload::{
    build_markdown_output_path, cleanup_non_markdown_upload, convert_upload_to_markdown,
    list_markdown_files, persist_knowledge_upload, resolve_knowledge_path,
    save_knowledge_upload_field, UploadedKnowledgeFile,
};

const MAX_KNOWLEDGE_UPLOAD_BYTES: usize = 20 * 1024 * 1024;
const MAX_KNOWLEDGE_CONTENT_BYTES: usize = 10 * 1024 * 1024;

pub(super) async fn user_knowledge_get(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UserIdQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let payload = state.user_tool_store.load_user_tools(&user_id);
    let bases = build_user_knowledge_payload(&state, &user_id, &payload.knowledge_bases, false);
    let config = state.config_store.get().await;
    let mut embedding_models = config
        .llm
        .models
        .iter()
        .filter(|(_, model)| llm::is_embedding_model(model))
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    embedding_models.sort();
    let tts_models = crate::multimodal_models::list_tts_model_names(&config);
    let image_models = crate::multimodal_models::list_image_model_names(&config);
    Ok(Json(json!({
        "data": {
            "knowledge": { "bases": bases },
            "embedding_models": embedding_models,
            "tts_models": tts_models,
            "image_models": image_models
        }
    })))
}

pub(super) async fn user_knowledge_update(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<UserKnowledgeUpdate>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, payload.user_id.as_deref()).await?;
    let user = resolved.user;
    let user_id = user.user_id.clone();
    let current = state.user_tool_store.load_user_tools(&user_id);
    let bases = payload
        .knowledge
        .bases
        .into_iter()
        .map(UserKnowledgeBase::from)
        .collect::<Vec<_>>();
    let config = state.config_store.get().await;
    let bases = prepare_user_ragflow_bases(&config, &user, bases)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let removed_vector_bases = collect_removed_vector_bases(&current.knowledge_bases, &bases);
    let removed_ragflow_dataset_ids =
        collect_removed_ragflow_dataset_ids(&current.knowledge_bases, &bases);
    let changed_ragflow_parser_configs =
        collect_changed_ragflow_parser_configs(&current.knowledge_bases, &bases);
    sync_changed_ragflow_parser_configs(&config, changed_ragflow_parser_configs)
        .await
        .map_err(vector_error_response)?;
    let updated = state
        .user_tool_store
        .update_knowledge_bases(&user_id, bases)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    cleanup_removed_user_vector_docs(state.storage.clone(), &user_id, removed_vector_bases).await;
    cleanup_removed_ragflow_datasets(&config, removed_ragflow_dataset_ids).await;
    let bases = build_user_knowledge_payload(&state, &user_id, &updated.knowledge_bases, true);
    Ok(Json(json!({ "data": { "knowledge": { "bases": bases } } })))
}

pub(super) async fn user_knowledge_files(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UserKnowledgeFilesQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let payload = state.user_tool_store.load_user_tools(&user_id);
    let base = resolve_user_knowledge_base(&payload, &query.base)?;
    let base_type = normalize_knowledge_base_type(base.base_type.as_deref());
    if base_type == KnowledgeBaseType::Vector {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.vector_knowledge_not_file_based"),
        ));
    }
    let root = state
        .user_tool_store
        .resolve_knowledge_base_root_with_type(&user_id, &base.name, base_type, false)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let files = list_markdown_files(&root);
    Ok(Json(
        json!({ "data": { "base": query.base, "files": files } }),
    ))
}

pub(super) async fn user_knowledge_file(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UserKnowledgeFileQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let payload = state.user_tool_store.load_user_tools(&user_id);
    let base = resolve_user_knowledge_base(&payload, &query.base)?;
    let base_type = normalize_knowledge_base_type(base.base_type.as_deref());
    if base_type == KnowledgeBaseType::Vector {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.vector_knowledge_not_file_based"),
        ));
    }
    let root = state
        .user_tool_store
        .resolve_knowledge_base_root_with_type(&user_id, &base.name, base_type, false)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let target = resolve_knowledge_path(&root, &query.path)?;
    if target
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase()
        != "md"
    {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.markdown_only"),
        ));
    }
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
        "data": { "base": query.base, "path": query.path, "content": content }
    })))
}

pub(super) async fn user_knowledge_file_update(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<UserKnowledgeFileUpdate>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, payload.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let user_payload = state.user_tool_store.load_user_tools(&user_id);
    let base = resolve_user_knowledge_base(&user_payload, &payload.base)?;
    let base_type = normalize_knowledge_base_type(base.base_type.as_deref());
    if base_type == KnowledgeBaseType::Vector {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.vector_knowledge_not_file_based"),
        ));
    }
    let root = state
        .user_tool_store
        .resolve_knowledge_base_root_with_type(&user_id, &base.name, base_type, true)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let target = resolve_knowledge_path(&root, &payload.path)?;
    if target
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase()
        != "md"
    {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.markdown_only"),
        ));
    }
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await.ok();
    }
    tokio::fs::write(&target, payload.content)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    refresh_user_knowledge_cache(&payload.base, &root).await;
    Ok(Json(json!({
        "data": { "ok": true, "message": i18n::t("message.saved_and_reindexed") }
    })))
}

pub(super) async fn user_knowledge_file_delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UserKnowledgeFileQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let user_payload = state.user_tool_store.load_user_tools(&user_id);
    let base = resolve_user_knowledge_base(&user_payload, &query.base)?;
    let base_type = normalize_knowledge_base_type(base.base_type.as_deref());
    if base_type == KnowledgeBaseType::Vector {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.vector_knowledge_not_file_based"),
        ));
    }
    let root = state
        .user_tool_store
        .resolve_knowledge_base_root_with_type(&user_id, &base.name, base_type, false)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let target = resolve_knowledge_path(&root, &query.path)?;
    if target
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase()
        != "md"
    {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.markdown_only"),
        ));
    }
    if target.exists() {
        tokio::fs::remove_file(&target)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        refresh_user_knowledge_cache(&query.base, &root).await;
    }
    Ok(Json(json!({
        "data": { "ok": true, "message": i18n::t("message.deleted") }
    })))
}

pub(super) async fn user_knowledge_upload(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<Value>, Response> {
    let mut raw_user_id = String::new();
    let mut base = String::new();
    let mut upload: Option<UploadedKnowledgeFile> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    {
        let field_name = field.name().unwrap_or("");
        if field_name == "user_id" {
            raw_user_id = field.text().await.unwrap_or_default();
            continue;
        }
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
    let resolved = resolve_user(
        &state,
        &headers,
        if raw_user_id.trim().is_empty() {
            None
        } else {
            Some(raw_user_id.trim())
        },
    )
    .await?;
    let user_id = resolved.user.user_id;
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
    let user_payload = state.user_tool_store.load_user_tools(&user_id);
    let base_config = resolve_user_knowledge_base(&user_payload, &base)?;
    let base_type = normalize_knowledge_base_type(base_config.base_type.as_deref());
    if base_type == KnowledgeBaseType::Ragflow {
        let config = state.config_store.get().await;
        let knowledge_config = build_user_ragflow_knowledge_config(&base_config);
        let temp_dir = upload.temp_dir.clone();
        let result = ragflow_knowledge::upload_document(
            &config,
            &knowledge_config,
            ragflow_knowledge::RagflowUpload {
                filename: upload.filename.clone(),
                input_path: upload.input_path.clone(),
            },
        )
        .await;
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
        let doc = result.map_err(vector_error_response)?;
        return Ok(Json(json!({
            "data": {
                "ok": true,
                "message": i18n::t("message.upload_converted"),
                "doc_id": doc.doc_id,
                "doc_name": doc.name,
                "chunk_count": doc.chunk_count,
                "embedding_model": doc.embedding_model,
                "converter": "ragflow",
                "warnings": []
            }
        })));
    }
    if base_type == KnowledgeBaseType::Vector {
        ensure_user_vector_base(&base_config)?;
        let root = state
            .user_tool_store
            .resolve_knowledge_base_root_with_type(&user_id, &base_config.name, base_type, true)
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        let storage = state.storage.clone();
        let temp_dir = upload.temp_dir.clone();
        let result = convert_upload_to_markdown(&upload).await;
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
        let (content, converter, warnings) = result?;
        let doc_name = upload.stem.clone();
        let existing = vector_knowledge::list_vector_documents(
            storage.as_ref(),
            Some(&user_id),
            &base_config.name,
            &root,
        )
        .await
        .map_err(vector_error_response)?;
        let mut doc_id: Option<String> = None;
        let mut previous_meta = None;
        if let Some(doc) = existing.iter().find(|doc| doc.name == doc_name) {
            doc_id = Some(doc.doc_id.clone());
            previous_meta = vector_knowledge::read_vector_document_meta(
                storage.as_ref(),
                Some(&user_id),
                &base_config.name,
                &root,
                &doc.doc_id,
            )
            .await
            .ok();
        }
        let knowledge_config = build_user_knowledge_config(&base_config, &root);
        let config = state.config_store.get().await;
        let meta = if vector_knowledge::resolve_embedding_model(
            &config,
            base_config.embedding_model.as_deref().unwrap_or(""),
        )
        .is_ok()
        {
            vector_knowledge::index_document(
                &config,
                &knowledge_config,
                Some(&user_id),
                storage.as_ref(),
                &root,
                &doc_name,
                doc_id.as_deref(),
                &content,
                previous_meta.as_ref(),
            )
            .await
            .map_err(vector_error_response)?
        } else {
            vector_knowledge::prepare_document(
                &knowledge_config,
                Some(&user_id),
                storage.as_ref(),
                &root,
                &doc_name,
                doc_id.as_deref(),
                &content,
                previous_meta.as_ref(),
            )
            .await
            .map_err(vector_error_response)?
        };
        return Ok(Json(json!({
            "data": {
                "ok": true,
                "message": i18n::t("message.upload_converted"),
                "doc_id": meta.doc_id,
                "doc_name": meta.name,
                "chunk_count": meta.chunk_count,
                "embedding_model": meta.embedding_model,
                "converter": converter,
                "warnings": warnings
            }
        })));
    }
    let root = state
        .user_tool_store
        .resolve_knowledge_base_root_with_type(&user_id, &base_config.name, base_type, true)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let output_name = build_markdown_output_path(&upload.filename, &upload.stem);
    let target = resolve_knowledge_path(&root, &output_name)?;
    let temp_dir = upload.temp_dir.clone();
    let result = persist_knowledge_upload(&upload, &target).await;
    let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    let (converter, warnings) = result?;
    cleanup_non_markdown_upload(&root, &upload.filename, &output_name).await;
    refresh_user_knowledge_cache(&base, &root).await;
    Ok(Json(json!({
        "data": {
            "ok": true,
            "message": i18n::t("message.upload_converted"),
            "path": output_name,
            "converter": converter,
            "warnings": warnings
        }
    })))
}

pub(super) async fn user_knowledge_docs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UserKnowledgeDocsQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let payload = state.user_tool_store.load_user_tools(&user_id);
    let base = resolve_user_knowledge_base(&payload, &query.base)?;
    let base_type = normalize_knowledge_base_type(base.base_type.as_deref());
    if base_type == KnowledgeBaseType::Ragflow {
        let config = state.config_store.get().await;
        let knowledge_config = build_user_ragflow_knowledge_config(&base);
        let docs = ragflow_knowledge::list_documents(&config, &knowledge_config)
            .await
            .map_err(vector_error_response)?;
        return Ok(Json(
            json!({ "data": { "base": query.base, "docs": docs } }),
        ));
    }
    if base_type != KnowledgeBaseType::Vector {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.vector_knowledge_required"),
        ));
    }
    let root = state
        .user_tool_store
        .resolve_knowledge_base_root_with_type(&user_id, &base.name, base_type, false)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let docs = vector_knowledge::list_vector_documents(
        state.storage.as_ref(),
        Some(&user_id),
        &base.name,
        &root,
    )
    .await
    .map_err(vector_error_response)?;
    Ok(Json(
        json!({ "data": { "base": query.base, "docs": docs } }),
    ))
}

pub(super) async fn user_knowledge_doc(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UserKnowledgeDocQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let payload = state.user_tool_store.load_user_tools(&user_id);
    let base = resolve_user_knowledge_base(&payload, &query.base)?;
    let base_type = normalize_knowledge_base_type(base.base_type.as_deref());
    if base_type == KnowledgeBaseType::Ragflow {
        let config = state.config_store.get().await;
        let knowledge_config = build_user_ragflow_knowledge_config(&base);
        let meta = ragflow_knowledge::read_document_meta(&config, &knowledge_config, &query.doc_id)
            .await
            .map_err(vector_error_response)?;
        let content =
            ragflow_knowledge::download_document_content(&config, &knowledge_config, &query.doc_id)
                .await
                .unwrap_or_default();
        return Ok(Json(json!({
            "data": { "base": query.base, "doc": meta, "content": content }
        })));
    }
    if base_type != KnowledgeBaseType::Vector {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.vector_knowledge_required"),
        ));
    }
    let root = state
        .user_tool_store
        .resolve_knowledge_base_root_with_type(&user_id, &base.name, base_type, false)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let meta = vector_knowledge::read_vector_document_meta(
        state.storage.as_ref(),
        Some(&user_id),
        &base.name,
        &root,
        &query.doc_id,
    )
    .await
    .map_err(vector_error_response)?;
    let content = vector_knowledge::read_vector_document_content(
        state.storage.as_ref(),
        Some(&user_id),
        &base.name,
        &root,
        &query.doc_id,
    )
    .await
    .map_err(vector_error_response)?;
    Ok(Json(json!({
        "data": { "base": query.base, "doc": meta, "content": content }
    })))
}

pub(super) async fn user_knowledge_doc_delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UserKnowledgeDocQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let payload = state.user_tool_store.load_user_tools(&user_id);
    let base = resolve_user_knowledge_base(&payload, &query.base)?;
    let base_type = normalize_knowledge_base_type(base.base_type.as_deref());
    if base_type == KnowledgeBaseType::Ragflow {
        let config = state.config_store.get().await;
        let knowledge_config = build_user_ragflow_knowledge_config(&base);
        let meta = ragflow_knowledge::read_document_meta(&config, &knowledge_config, &query.doc_id)
            .await
            .map_err(vector_error_response)?;
        ragflow_knowledge::delete_document(&config, &knowledge_config, &query.doc_id)
            .await
            .map_err(vector_error_response)?;
        return Ok(Json(json!({
            "data": {
                "ok": true,
                "deleted": 0,
                "doc_id": meta.doc_id,
                "doc_name": meta.name
            }
        })));
    }
    if base_type != KnowledgeBaseType::Vector {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.vector_knowledge_required"),
        ));
    }
    let root = state
        .user_tool_store
        .resolve_knowledge_base_root_with_type(&user_id, &base.name, base_type, false)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let config = state.config_store.get().await;
    let base_name = base.name.clone();
    let storage = state.storage.clone();
    let root_for_lock = root.clone();
    let doc_id = query.doc_id.clone();
    let (meta, deleted) = vector_knowledge::with_document_lock(&root_for_lock, &doc_id, || {
        let storage = storage.clone();
        let base_name = base_name.clone();
        let root = root.clone();
        let doc_id = doc_id.clone();
        async move {
            let meta = vector_knowledge::read_vector_document_meta(
                storage.as_ref(),
                Some(&user_id),
                &base_name,
                &root,
                &doc_id,
            )
            .await
            .map_err(vector_error_response)?;
            let deleted = if !meta.embedding_model.trim().is_empty() {
                if let Ok(client) = vector_knowledge::resolve_weaviate_client(&config) {
                    let owner_key = vector_knowledge::resolve_owner_key(Some(&user_id));
                    client
                        .delete_doc_chunks_all(
                            &owner_key,
                            &base_name,
                            &meta.embedding_model,
                            &meta.doc_id,
                        )
                        .await
                        .unwrap_or(0)
                } else {
                    0
                }
            } else {
                0
            };
            vector_knowledge::delete_vector_document_files(
                storage.as_ref(),
                Some(&user_id),
                &base_name,
                &root,
                &meta.doc_id,
            )
            .await
            .map_err(vector_error_response)?;
            Ok((meta, deleted))
        }
    })
    .await?;
    Ok(Json(json!({
        "data": {
            "ok": true,
            "deleted": deleted,
            "doc_id": meta.doc_id,
            "doc_name": meta.name
        }
    })))
}

pub(super) async fn user_knowledge_chunks(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UserKnowledgeChunksQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, query.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let payload = state.user_tool_store.load_user_tools(&user_id);
    let base = resolve_user_knowledge_base(&payload, &query.base)?;
    let base_type = normalize_knowledge_base_type(base.base_type.as_deref());
    if base_type == KnowledgeBaseType::Ragflow {
        let config = state.config_store.get().await;
        let knowledge_config = build_user_ragflow_knowledge_config(&base);
        let chunks = ragflow_knowledge::list_chunks(&config, &knowledge_config, &query.doc_id)
            .await
            .map_err(vector_error_response)?;
        return Ok(Json(json!({
            "data": {
                "base": query.base,
                "doc_id": query.doc_id,
                "chunks": chunks
            }
        })));
    }
    if base_type != KnowledgeBaseType::Vector {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.vector_knowledge_required"),
        ));
    }
    let root = state
        .user_tool_store
        .resolve_knowledge_base_root_with_type(&user_id, &base.name, base_type, false)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let meta = vector_knowledge::read_vector_document_meta(
        state.storage.as_ref(),
        Some(&user_id),
        &base.name,
        &root,
        &query.doc_id,
    )
    .await
    .map_err(vector_error_response)?;
    let content = vector_knowledge::read_vector_document_content(
        state.storage.as_ref(),
        Some(&user_id),
        &base.name,
        &root,
        &query.doc_id,
    )
    .await
    .map_err(vector_error_response)?;
    let chunks = vector_knowledge::build_chunk_previews(&content, &meta).await;
    Ok(Json(json!({
        "data": {
            "base": query.base,
            "doc_id": query.doc_id,
            "chunks": chunks
        }
    })))
}

pub(super) async fn user_knowledge_chunk_embed(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<UserKnowledgeChunkActionRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, payload.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let base_name = payload.base.trim();
    if base_name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.knowledge_base_name_required"),
        ));
    }
    let doc_id = payload.doc_id.trim();
    if doc_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.knowledge_document_not_found"),
        ));
    }
    let user_payload = state.user_tool_store.load_user_tools(&user_id);
    let base = resolve_user_knowledge_base(&user_payload, base_name)?;
    let config = state.config_store.get().await;
    if normalize_knowledge_base_type(base.base_type.as_deref()) == KnowledgeBaseType::Ragflow {
        let knowledge_config = build_user_ragflow_knowledge_config(&base);
        ragflow_knowledge::set_chunk_available(
            &config,
            &knowledge_config,
            doc_id,
            payload.chunk_index,
            true,
        )
        .await
        .map_err(vector_error_response)?;
        let meta = ragflow_knowledge::read_document_meta(&config, &knowledge_config, doc_id)
            .await
            .map_err(vector_error_response)?;
        return Ok(Json(json!({ "data": { "ok": true, "doc": meta } })));
    }
    ensure_user_vector_base(&base)?;
    let root = state
        .user_tool_store
        .resolve_knowledge_base_root_with_type(
            &user_id,
            &base.name,
            KnowledgeBaseType::Vector,
            false,
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let root_for_lock = root.clone();
    let doc_id = doc_id.to_string();
    let base_name = base.name.clone();
    let embedding_name = base
        .embedding_model
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_string();
    let storage = state.storage.clone();
    let meta = vector_knowledge::with_document_lock(&root_for_lock, &doc_id, || {
        let storage = storage.clone();
        let base_name = base_name.clone();
        let root = root.clone();
        let doc_id = doc_id.clone();
        let embedding_name = embedding_name.clone();
        async move {
            let mut meta = vector_knowledge::read_vector_document_meta(
                storage.as_ref(),
                Some(&user_id),
                &base_name,
                &root,
                &doc_id,
            )
            .await
            .map_err(vector_error_response)?;
            let content = vector_knowledge::read_vector_document_content(
                storage.as_ref(),
                Some(&user_id),
                &base_name,
                &root,
                &doc_id,
            )
            .await
            .map_err(vector_error_response)?;
            let chunk = meta
                .chunks
                .iter_mut()
                .find(|chunk| chunk.index == payload.chunk_index)
                .ok_or_else(|| {
                    error_response(
                        StatusCode::NOT_FOUND,
                        i18n::t("error.knowledge_chunk_not_found"),
                    )
                })?;
            if chunk.status.as_deref() == Some("deleted") {
                return Err(error_response(
                    StatusCode::BAD_REQUEST,
                    i18n::t("error.knowledge_chunk_deleted"),
                ));
            }
            let embed_config = vector_knowledge::resolve_embedding_model(&config, &embedding_name)
                .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
            meta.embedding_model = embedding_name.clone();
            let content_chars: Vec<char> = content.chars().collect();
            let chunk_content = vector_knowledge::resolve_chunk_content(&content_chars, chunk);
            if chunk_content.trim().is_empty() {
                return Err(error_response(
                    StatusCode::BAD_REQUEST,
                    i18n::t("error.content_required"),
                ));
            }
            let vector_chunk = vector_knowledge::VectorChunk {
                index: chunk.index,
                start: chunk.start,
                end: chunk.end,
                content: chunk_content,
                chunk_id: vector_knowledge::build_chunk_id(&meta.doc_id, chunk.index),
            };
            let timeout_s = embed_config.timeout_s.unwrap_or(120);
            let vectors = vector_knowledge::embed_chunks(
                &embed_config,
                std::slice::from_ref(&vector_chunk),
                timeout_s,
            )
            .await
            .map_err(vector_error_response)?;
            let client = vector_knowledge::resolve_weaviate_client(&config)
                .map_err(vector_error_response)?;
            let owner_key = vector_knowledge::resolve_owner_key(Some(&user_id));
            let _ = client
                .upsert_chunks(
                    &owner_key,
                    &base_name,
                    &meta.doc_id,
                    &meta.name,
                    &embedding_name,
                    &[vector_chunk],
                    &vectors,
                )
                .await
                .map_err(vector_error_response)?;
            chunk.status = Some("embedded".to_string());
            vector_knowledge::refresh_document_meta(&mut meta);
            vector_knowledge::write_vector_document(
                storage.as_ref(),
                Some(&user_id),
                &base_name,
                &meta,
                &content,
            )
            .await
            .map_err(vector_error_response)?;
            Ok(meta)
        }
    })
    .await?;
    Ok(Json(json!({ "data": { "ok": true, "doc": meta } })))
}

pub(super) async fn user_knowledge_chunk_delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<UserKnowledgeChunkActionRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, payload.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let base_name = payload.base.trim();
    if base_name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.knowledge_base_name_required"),
        ));
    }
    let doc_id = payload.doc_id.trim();
    if doc_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.knowledge_document_not_found"),
        ));
    }
    let user_payload = state.user_tool_store.load_user_tools(&user_id);
    let base = resolve_user_knowledge_base(&user_payload, base_name)?;
    let config = state.config_store.get().await;
    if normalize_knowledge_base_type(base.base_type.as_deref()) == KnowledgeBaseType::Ragflow {
        let knowledge_config = build_user_ragflow_knowledge_config(&base);
        ragflow_knowledge::delete_chunk(&config, &knowledge_config, doc_id, payload.chunk_index)
            .await
            .map_err(vector_error_response)?;
        let meta = ragflow_knowledge::read_document_meta(&config, &knowledge_config, doc_id)
            .await
            .map_err(vector_error_response)?;
        return Ok(Json(json!({ "data": { "ok": true, "doc": meta } })));
    }
    ensure_user_vector_base(&base)?;
    let root = state
        .user_tool_store
        .resolve_knowledge_base_root_with_type(
            &user_id,
            &base.name,
            KnowledgeBaseType::Vector,
            false,
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let root_for_lock = root.clone();
    let doc_id = doc_id.to_string();
    let base_name = base.name.clone();
    let storage = state.storage.clone();
    let meta = vector_knowledge::with_document_lock(&root_for_lock, &doc_id, || {
        let storage = storage.clone();
        let base_name = base_name.clone();
        let root = root.clone();
        let doc_id = doc_id.clone();
        async move {
            let mut meta = vector_knowledge::read_vector_document_meta(
                storage.as_ref(),
                Some(&user_id),
                &base_name,
                &root,
                &doc_id,
            )
            .await
            .map_err(vector_error_response)?;
            let content = vector_knowledge::read_vector_document_content(
                storage.as_ref(),
                Some(&user_id),
                &base_name,
                &root,
                &doc_id,
            )
            .await
            .map_err(vector_error_response)?;
            let chunk = meta
                .chunks
                .iter_mut()
                .find(|chunk| chunk.index == payload.chunk_index)
                .ok_or_else(|| {
                    error_response(
                        StatusCode::NOT_FOUND,
                        i18n::t("error.knowledge_chunk_not_found"),
                    )
                })?;
            if chunk.status.as_deref() == Some("deleted") {
                return Ok(meta);
            }
            if !meta.embedding_model.trim().is_empty() {
                if let Ok(client) = vector_knowledge::resolve_weaviate_client(&config) {
                    let _ = client
                        .delete_chunk(&vector_knowledge::build_chunk_id(&meta.doc_id, chunk.index))
                        .await;
                }
            }
            chunk.status = Some("deleted".to_string());
            vector_knowledge::refresh_document_meta(&mut meta);
            vector_knowledge::write_vector_document(
                storage.as_ref(),
                Some(&user_id),
                &base_name,
                &meta,
                &content,
            )
            .await
            .map_err(vector_error_response)?;
            Ok(meta)
        }
    })
    .await?;
    Ok(Json(json!({ "data": { "ok": true, "doc": meta } })))
}

pub(super) async fn user_knowledge_chunk_update(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<UserKnowledgeChunkUpdateRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, payload.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let base_name = payload.base.trim();
    if base_name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.knowledge_base_name_required"),
        ));
    }
    let doc_id = payload.doc_id.trim();
    if doc_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.knowledge_document_not_found"),
        ));
    }
    if payload.content.trim().is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let user_payload = state.user_tool_store.load_user_tools(&user_id);
    let base = resolve_user_knowledge_base(&user_payload, base_name)?;
    if normalize_knowledge_base_type(base.base_type.as_deref()) == KnowledgeBaseType::Ragflow {
        let config = state.config_store.get().await;
        let knowledge_config = build_user_ragflow_knowledge_config(&base);
        ragflow_knowledge::update_chunk(
            &config,
            &knowledge_config,
            doc_id,
            payload.chunk_index,
            &payload.content,
        )
        .await
        .map_err(vector_error_response)?;
        let meta = ragflow_knowledge::read_document_meta(&config, &knowledge_config, doc_id)
            .await
            .map_err(vector_error_response)?;
        return Ok(Json(json!({ "data": { "ok": true, "doc": meta } })));
    }
    ensure_user_vector_base(&base)?;
    let root = state
        .user_tool_store
        .resolve_knowledge_base_root_with_type(
            &user_id,
            &base.name,
            KnowledgeBaseType::Vector,
            false,
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let root_for_lock = root.clone();
    let doc_id = doc_id.to_string();
    let base_name = base.name.clone();
    let storage = state.storage.clone();
    let updated_content = payload.content.clone();
    let meta = vector_knowledge::with_document_lock(&root_for_lock, &doc_id, || {
        let storage = storage.clone();
        let base_name = base_name.clone();
        let root = root.clone();
        let doc_id = doc_id.clone();
        let updated_content = updated_content.clone();
        async move {
            let mut meta = vector_knowledge::read_vector_document_meta(
                storage.as_ref(),
                Some(&user_id),
                &base_name,
                &root,
                &doc_id,
            )
            .await
            .map_err(vector_error_response)?;
            let full_content = vector_knowledge::read_vector_document_content(
                storage.as_ref(),
                Some(&user_id),
                &base_name,
                &root,
                &doc_id,
            )
            .await
            .map_err(vector_error_response)?;
            let chunk = meta
                .chunks
                .iter_mut()
                .find(|chunk| chunk.index == payload.chunk_index)
                .ok_or_else(|| {
                    error_response(
                        StatusCode::NOT_FOUND,
                        i18n::t("error.knowledge_chunk_not_found"),
                    )
                })?;
            if chunk.status.as_deref() == Some("deleted") {
                return Err(error_response(
                    StatusCode::BAD_REQUEST,
                    i18n::t("error.knowledge_chunk_deleted"),
                ));
            }
            chunk.content = Some(updated_content);
            chunk.status = Some("pending".to_string());
            vector_knowledge::refresh_document_meta(&mut meta);
            vector_knowledge::write_vector_document(
                storage.as_ref(),
                Some(&user_id),
                &base_name,
                &meta,
                &full_content,
            )
            .await
            .map_err(vector_error_response)?;
            Ok(meta)
        }
    })
    .await?;
    Ok(Json(json!({ "data": { "ok": true, "doc": meta } })))
}

pub(super) async fn user_knowledge_test(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<UserKnowledgeTestRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, payload.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let base_name = payload.base.trim();
    if base_name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.knowledge_base_name_required"),
        ));
    }
    let query = payload.query.trim();
    if query.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.knowledge_query_required"),
        ));
    }
    let user_payload = state.user_tool_store.load_user_tools(&user_id);
    let base = resolve_user_knowledge_base(&user_payload, base_name)?;
    let config = state.config_store.get().await;
    let base_type = normalize_knowledge_base_type(base.base_type.as_deref());
    if base_type == KnowledgeBaseType::Ragflow {
        let knowledge_config = build_user_ragflow_knowledge_config(&base);
        let top_k = payload
            .top_k
            .filter(|value| *value > 0)
            .unwrap_or_else(|| ragflow_knowledge::resolve_top_k(&knowledge_config));
        let hits = ragflow_knowledge::retrieve(&config, &knowledge_config, query, top_k)
            .await
            .map_err(vector_error_response)?;
        return Ok(Json(json!({
            "data": {
                "base": base.name,
                "query": query,
                "embedding_model": "ragflow",
                "top_k": top_k,
                "hits": build_ragflow_knowledge_test_hits(hits),
                "fallback_mode": false
            }
        })));
    }
    let root = state
        .user_tool_store
        .resolve_knowledge_base_root_with_type(&user_id, &base.name, base_type, false)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let knowledge_config = build_user_knowledge_config(&base, &root);
    if base_type == KnowledgeBaseType::Vector {
        let embedding_name = base
            .embedding_model
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_string();
        let top_k = payload
            .top_k
            .filter(|value| *value > 0)
            .unwrap_or_else(|| vector_knowledge::resolve_top_k(&knowledge_config));
        let vector_hits = if let Ok(embed_config) =
            vector_knowledge::resolve_embedding_model(&config, &embedding_name)
        {
            let timeout_s = embed_config.timeout_s.unwrap_or(120);
            if let Ok(vectors) =
                llm::embed_texts(&embed_config, &[query.to_string()], timeout_s).await
            {
                if let Some(vector) = vectors.first() {
                    if let Ok(client) = vector_knowledge::resolve_weaviate_client(&config) {
                        let owner_key = vector_knowledge::resolve_owner_key(Some(&user_id));
                        if let Ok(mut hits) = client
                            .query_chunks(&owner_key, &base.name, &embedding_name, vector, top_k)
                            .await
                        {
                            if let Some(threshold) = base.score_threshold {
                                hits.retain(|hit| hit.score.unwrap_or(0.0) >= f64::from(threshold));
                            }
                            if hits.len() > top_k {
                                hits.truncate(top_k);
                            }
                            Some(hits)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        let (hits, fallback_mode) = if let Some(hits) = vector_hits {
            (hits, false)
        } else {
            (
                vector_knowledge::query_chunks_by_text(
                    state.storage.as_ref(),
                    Some(&user_id),
                    &knowledge_config,
                    &root,
                    query,
                    top_k,
                )
                .await
                .map_err(vector_error_response)?,
                true,
            )
        };
        let items = hits
            .into_iter()
            .map(|hit| {
                json!({
                    "doc_id": hit.doc_id,
                    "document": hit.doc_name,
                    "chunk_index": hit.chunk_index,
                    "start": hit.start,
                    "end": hit.end,
                    "content": hit.content,
                    "embedding_model": hit.embedding_model,
                    "score": hit.score
                })
            })
            .collect::<Vec<_>>();
        Ok(Json(json!({
            "data": {
                "base": base.name,
                "query": query,
                "embedding_model": embedding_name,
                "top_k": top_k,
                "hits": items,
                "fallback_mode": fallback_mode
            }
        })))
    } else {
        let llm_config = knowledge::resolve_llm_config(&config, None);
        let (reply, reasoning, _) = knowledge::query_knowledge_raw_with_documents(
            query,
            &knowledge_config,
            llm_config.as_ref(),
            payload.top_k,
            None,
        )
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        Ok(Json(json!({
            "data": {
                "base": base.name,
                "query": query,
                "text": reply,
                "reasoning": reasoning
            }
        })))
    }
}

pub(super) async fn user_knowledge_reindex(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<UserKnowledgeReindexRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, payload.user_id.as_deref()).await?;
    let user_id = resolved.user.user_id;
    let base_name = payload.base.trim();
    if base_name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.knowledge_base_name_required"),
        ));
    }
    let user_payload = state.user_tool_store.load_user_tools(&user_id);
    let base = resolve_user_knowledge_base(&user_payload, base_name)?;
    let base_type = normalize_knowledge_base_type(base.base_type.as_deref());
    if base_type == KnowledgeBaseType::Ragflow {
        let config = state.config_store.get().await;
        let knowledge_config = build_user_ragflow_knowledge_config(&base);
        let mut targets = Vec::new();
        if let Some(doc_id) = payload.doc_id.as_deref() {
            let cleaned = doc_id.trim();
            if !cleaned.is_empty() {
                targets.push(cleaned.to_string());
            }
        }
        if targets.is_empty() {
            let docs = ragflow_knowledge::list_documents(&config, &knowledge_config)
                .await
                .map_err(vector_error_response)?;
            targets = docs.into_iter().map(|doc| doc.doc_id).collect();
        }
        ragflow_knowledge::reparse_documents(&config, &knowledge_config, &targets)
            .await
            .map_err(vector_error_response)?;
        return Ok(Json(json!({
            "data": {
                "ok": true,
                "reindexed": targets,
                "failed": []
            }
        })));
    }
    if base_type != KnowledgeBaseType::Vector {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.vector_knowledge_required"),
        ));
    }
    let root = state
        .user_tool_store
        .resolve_knowledge_base_root_with_type(&user_id, &base.name, base_type, true)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let storage = state.storage.clone();
    let mut targets = Vec::new();
    if let Some(doc_id) = payload.doc_id.as_deref() {
        let cleaned = doc_id.trim();
        if !cleaned.is_empty() {
            targets.push(cleaned.to_string());
        }
    }
    if targets.is_empty() {
        let docs = vector_knowledge::list_vector_documents(
            storage.as_ref(),
            Some(&user_id),
            &base.name,
            &root,
        )
        .await
        .map_err(vector_error_response)?;
        targets = docs.into_iter().map(|doc| doc.doc_id).collect();
    }
    let knowledge_config = build_user_knowledge_config(&base, &root);
    let config = state.config_store.get().await;
    let mut reindexed = Vec::new();
    let mut failed = Vec::new();
    for doc_id in targets {
        let meta = match vector_knowledge::read_vector_document_meta(
            storage.as_ref(),
            Some(&user_id),
            &base.name,
            &root,
            &doc_id,
        )
        .await
        {
            Ok(meta) => meta,
            Err(err) => {
                failed.push(json!({ "doc_id": doc_id, "error": err.to_string() }));
                continue;
            }
        };
        let content = match vector_knowledge::read_vector_document_content(
            storage.as_ref(),
            Some(&user_id),
            &base.name,
            &root,
            &doc_id,
        )
        .await
        {
            Ok(content) => content,
            Err(err) => {
                failed.push(json!({ "doc_id": doc_id, "error": err.to_string() }));
                continue;
            }
        };
        match vector_knowledge::index_document(
            &config,
            &knowledge_config,
            Some(&user_id),
            storage.as_ref(),
            &root,
            &meta.name,
            Some(&meta.doc_id),
            &content,
            Some(&meta),
        )
        .await
        {
            Ok(updated) => reindexed.push(updated.doc_id),
            Err(err) => failed.push(json!({ "doc_id": doc_id, "error": err.to_string() })),
        }
    }
    Ok(Json(json!({
        "data": {
            "ok": failed.is_empty(),
            "reindexed": reindexed,
            "failed": failed
        }
    })))
}

fn build_user_knowledge_payload(
    state: &Arc<AppState>,
    user_id: &str,
    bases: &[UserKnowledgeBase],
    create: bool,
) -> Vec<UserKnowledgeBasePayload> {
    bases
        .iter()
        .map(|base| {
            let mut root = String::new();
            if !base.name.trim().is_empty() {
                let base_type = normalize_knowledge_base_type(base.base_type.as_deref());
                if base_type == KnowledgeBaseType::Ragflow {
                    root = ragflow_knowledge::synthetic_root(
                        base.ragflow_dataset_id.as_deref().unwrap_or(""),
                    );
                } else if let Ok(path) = state
                    .user_tool_store
                    .resolve_knowledge_base_root_with_type(user_id, &base.name, base_type, create)
                {
                    root = path.to_string_lossy().to_string();
                }
            }
            UserKnowledgeBasePayload::from_with_root(base, root)
        })
        .collect()
}

fn collect_removed_vector_bases(
    current: &[UserKnowledgeBase],
    next: &[UserKnowledgeBase],
) -> Vec<String> {
    let mut next_vector = HashSet::new();
    for base in next {
        if normalize_knowledge_base_type(base.base_type.as_deref()) == KnowledgeBaseType::Vector {
            next_vector.insert(base.name.clone());
        }
    }
    current
        .iter()
        .filter(|base| {
            normalize_knowledge_base_type(base.base_type.as_deref()) == KnowledgeBaseType::Vector
        })
        .filter(|base| !next_vector.contains(&base.name))
        .map(|base| base.name.clone())
        .collect()
}

fn collect_removed_ragflow_dataset_ids(
    current: &[UserKnowledgeBase],
    next: &[UserKnowledgeBase],
) -> Vec<String> {
    let mut next_dataset_ids = HashSet::new();
    for base in next {
        if normalize_knowledge_base_type(base.base_type.as_deref()) == KnowledgeBaseType::Ragflow {
            if let Some(dataset_id) =
                ragflow_knowledge::normalize_dataset_id(base.ragflow_dataset_id.as_deref())
            {
                next_dataset_ids.insert(dataset_id);
            }
        }
    }
    let mut removed = Vec::new();
    for base in current {
        if normalize_knowledge_base_type(base.base_type.as_deref()) != KnowledgeBaseType::Ragflow {
            continue;
        }
        if base.ragflow_dataset_managed == Some(false) {
            continue;
        }
        let Some(dataset_id) =
            ragflow_knowledge::normalize_dataset_id(base.ragflow_dataset_id.as_deref())
        else {
            continue;
        };
        if !next_dataset_ids.contains(&dataset_id) {
            removed.push(dataset_id);
        }
    }
    removed.sort();
    removed.dedup();
    removed
}

fn collect_changed_ragflow_parser_configs(
    current: &[UserKnowledgeBase],
    next: &[UserKnowledgeBase],
) -> Vec<KnowledgeBaseConfig> {
    let mut current_by_dataset = HashMap::new();
    for base in current {
        if normalize_knowledge_base_type(base.base_type.as_deref()) != KnowledgeBaseType::Ragflow {
            continue;
        }
        if let Some(dataset_id) =
            ragflow_knowledge::normalize_dataset_id(base.ragflow_dataset_id.as_deref())
        {
            current_by_dataset.insert(dataset_id, build_user_ragflow_knowledge_config(base));
        }
    }
    let mut changed = Vec::new();
    let mut seen = HashSet::new();
    for base in next {
        if normalize_knowledge_base_type(base.base_type.as_deref()) != KnowledgeBaseType::Ragflow {
            continue;
        }
        let Some(dataset_id) =
            ragflow_knowledge::normalize_dataset_id(base.ragflow_dataset_id.as_deref())
        else {
            continue;
        };
        if !seen.insert(dataset_id.clone()) {
            continue;
        }
        let Some(current_base) = current_by_dataset.get(&dataset_id) else {
            continue;
        };
        let next_base = build_user_ragflow_knowledge_config(base);
        if ragflow_parser_config_changed(current_base, &next_base) {
            changed.push(next_base);
        }
    }
    changed
}

fn ragflow_parser_config_changed(
    current: &KnowledgeBaseConfig,
    next: &KnowledgeBaseConfig,
) -> bool {
    ragflow_knowledge::resolve_chunk_method(current)
        != ragflow_knowledge::resolve_chunk_method(next)
        || ragflow_knowledge::build_dataset_parser_config(current)
            != ragflow_knowledge::build_dataset_parser_config(next)
}

async fn sync_changed_ragflow_parser_configs(
    config: &Config,
    bases: Vec<KnowledgeBaseConfig>,
) -> anyhow::Result<()> {
    for base in bases {
        ragflow_knowledge::sync_parser_config_and_reparse_documents(config, &base).await?;
    }
    Ok(())
}

async fn cleanup_removed_user_vector_docs(
    storage: Arc<dyn StorageBackend>,
    user_id: &str,
    bases: Vec<String>,
) {
    let owner_key = vector_knowledge::resolve_owner_key(Some(user_id));
    for name in bases {
        let _ = storage.delete_vector_documents_by_base(&owner_key, &name);
        if let Ok(root) = vector_knowledge::resolve_vector_root(Some(user_id), &name, false) {
            let _ = tokio::fs::remove_dir_all(&root).await;
        }
    }
}

async fn cleanup_removed_ragflow_datasets(config: &Config, dataset_ids: Vec<String>) {
    if dataset_ids.is_empty() {
        return;
    }
    if let Err(err) = ragflow_knowledge::delete_datasets(config, &dataset_ids).await {
        tracing::info!("failed to remove RAGFlow datasets: {err}");
    }
}

async fn prepare_user_ragflow_bases(
    config: &Config,
    user: &UserAccountRecord,
    bases: Vec<UserKnowledgeBase>,
) -> anyhow::Result<Vec<UserKnowledgeBase>> {
    let mut output = Vec::with_capacity(bases.len());
    for mut base in bases {
        if normalize_knowledge_base_type(base.base_type.as_deref()) == KnowledgeBaseType::Ragflow {
            base.base_type = Some("ragflow".to_string());
            base.embedding_model = None;
            base.chunk_method =
                ragflow_knowledge::normalize_chunk_method(base.chunk_method.as_deref());
            base.chunk_delimiter =
                ragflow_knowledge::normalize_chunk_delimiter(base.chunk_delimiter.as_deref());
            base.layout_recognize =
                ragflow_knowledge::normalize_layout_recognize(base.layout_recognize.as_deref());
            base.auto_keywords = base.auto_keywords.map(|value| value.min(32));
            base.auto_questions = base.auto_questions.map(|value| value.min(10));
            if base
                .ragflow_dataset_id
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
            {
                let knowledge_base = KnowledgeBaseConfig {
                    name: base.name.clone(),
                    description: base.description.clone(),
                    root: String::new(),
                    enabled: base.enabled,
                    shared: Some(base.shared),
                    base_type: Some("ragflow".to_string()),
                    embedding_model: None,
                    ragflow_dataset_id: None,
                    ragflow_dataset_managed: Some(true),
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
                };
                let mut knowledge_base = knowledge_base;
                knowledge_base.name = build_user_ragflow_dataset_name(user, &base.name);
                base.ragflow_dataset_id =
                    Some(ragflow_knowledge::create_dataset(config, &knowledge_base).await?);
                base.ragflow_dataset_managed = Some(true);
            } else if base.ragflow_dataset_managed.is_none() {
                base.ragflow_dataset_managed = Some(false);
            }
        }
        output.push(base);
    }
    Ok(output)
}

fn build_user_ragflow_dataset_name(user: &UserAccountRecord, base_name: &str) -> String {
    let prefix = ragflow_knowledge::normalize_dataset_name_part(
        if user.username.trim().is_empty() {
            &user.user_id
        } else {
            &user.username
        },
        "user",
    );
    let name = ragflow_knowledge::normalize_dataset_name_part(base_name, "knowledge");
    format!("[{prefix}] {name}")
}

async fn refresh_user_knowledge_cache(base: &str, root: &Path) {
    let config = KnowledgeBaseConfig {
        name: base.to_string(),
        description: String::new(),
        root: root.to_string_lossy().to_string(),
        enabled: true,
        shared: None,
        base_type: None,
        embedding_model: None,
        ragflow_dataset_id: None,
        ragflow_dataset_managed: None,
        chunk_method: None,
        chunk_delimiter: None,
        layout_recognize: None,
        auto_keywords: None,
        auto_questions: None,
        html4excel: None,
        chunk_size: None,
        chunk_overlap: None,
        top_k: None,
        score_threshold: None,
    };
    let _ = knowledge::refresh_knowledge_cache(&config).await;
}

fn resolve_user_knowledge_base(
    payload: &UserToolsPayload,
    base_name: &str,
) -> Result<UserKnowledgeBase, Response> {
    let name = base_name.trim();
    if name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.knowledge_base_name_required"),
        ));
    }
    payload
        .knowledge_bases
        .iter()
        .find(|base| base.name == name)
        .cloned()
        .ok_or_else(|| {
            error_response(
                StatusCode::NOT_FOUND,
                i18n::t("error.knowledge_base_not_found"),
            )
        })
}

fn build_user_knowledge_config(base: &UserKnowledgeBase, root: &Path) -> KnowledgeBaseConfig {
    KnowledgeBaseConfig {
        name: base.name.clone(),
        description: base.description.clone(),
        root: root.to_string_lossy().to_string(),
        enabled: base.enabled,
        shared: Some(base.shared),
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
    }
}

fn build_user_ragflow_knowledge_config(base: &UserKnowledgeBase) -> KnowledgeBaseConfig {
    let dataset_id = base.ragflow_dataset_id.as_deref().unwrap_or("");
    KnowledgeBaseConfig {
        name: base.name.clone(),
        description: base.description.clone(),
        root: ragflow_knowledge::synthetic_root(dataset_id),
        enabled: base.enabled,
        shared: Some(base.shared),
        base_type: Some("ragflow".to_string()),
        embedding_model: None,
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
    }
}

fn build_ragflow_knowledge_test_hits(hits: Vec<ragflow_knowledge::RagflowSearchHit>) -> Vec<Value> {
    hits.into_iter()
        .map(|hit| {
            json!({
                "doc_id": hit.doc_id,
                "document": hit.doc_name,
                "chunk_id": hit.chunk_id,
                "chunk_index": hit.chunk_index,
                "start": hit.start,
                "end": hit.end,
                "content": hit.content,
                "embedding_model": "ragflow",
                "score": hit.score
            })
        })
        .collect()
}

fn ensure_user_vector_base(base: &UserKnowledgeBase) -> Result<(), Response> {
    if normalize_knowledge_base_type(base.base_type.as_deref()) != KnowledgeBaseType::Vector {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.vector_knowledge_required"),
        ));
    }
    Ok(())
}

fn vector_error_response(err: anyhow::Error) -> Response {
    if let Some(io_error) = err.downcast_ref::<std::io::Error>() {
        if io_error.kind() == ErrorKind::NotFound {
            return error_response(
                StatusCode::NOT_FOUND,
                i18n::t("error.knowledge_document_not_found"),
            );
        }
    }
    error_response(StatusCode::BAD_REQUEST, err.to_string())
}

#[derive(Debug, Deserialize)]
pub(super) struct UserKnowledgeUpdate {
    #[serde(default)]
    user_id: Option<String>,
    knowledge: UserKnowledgePayload,
}

#[derive(Debug, Deserialize)]
pub(super) struct UserKnowledgePayload {
    #[serde(default)]
    bases: Vec<UserKnowledgeBasePayload>,
}

#[derive(Debug, Deserialize)]
pub(super) struct UserKnowledgeFilesQuery {
    #[serde(default)]
    user_id: Option<String>,
    base: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct UserKnowledgeDocsQuery {
    #[serde(default)]
    user_id: Option<String>,
    base: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct UserKnowledgeFileQuery {
    #[serde(default)]
    user_id: Option<String>,
    base: String,
    path: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct UserKnowledgeFileUpdate {
    #[serde(default)]
    user_id: Option<String>,
    base: String,
    path: String,
    content: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct UserKnowledgeDocQuery {
    #[serde(default)]
    user_id: Option<String>,
    base: String,
    doc_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct UserKnowledgeChunksQuery {
    #[serde(default)]
    user_id: Option<String>,
    base: String,
    doc_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct UserKnowledgeChunkActionRequest {
    #[serde(default)]
    user_id: Option<String>,
    base: String,
    doc_id: String,
    chunk_index: usize,
}

#[derive(Debug, Deserialize)]
pub(super) struct UserKnowledgeChunkUpdateRequest {
    #[serde(default)]
    user_id: Option<String>,
    base: String,
    doc_id: String,
    chunk_index: usize,
    content: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct UserKnowledgeTestRequest {
    #[serde(default)]
    user_id: Option<String>,
    base: String,
    query: String,
    #[serde(default)]
    top_k: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(super) struct UserKnowledgeReindexRequest {
    #[serde(default)]
    user_id: Option<String>,
    base: String,
    #[serde(default)]
    doc_id: Option<String>,
}

#[cfg(test)]
mod tests;
