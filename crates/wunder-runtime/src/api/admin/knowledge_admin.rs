use crate::api::admin::error_response;
use crate::config::{
    normalize_knowledge_base_type, Config, KnowledgeBaseConfig, KnowledgeBaseType,
};
use crate::core::repo_assets;
use crate::i18n;
use crate::knowledge;
use crate::llm;
use crate::path_utils::{
    is_within_root, normalize_existing_path, normalize_path_for_compare, normalize_target_path,
};
use crate::services::ragflow_knowledge;
use crate::state::AppState;
use crate::storage::StorageBackend;
use crate::vector_knowledge;
use anyhow::anyhow;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::{routing::get, routing::post, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_stream::StreamExt;
use tracing::info;

mod file_admin;

pub(super) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .merge(file_admin::router())
        .route(
            "/wunder/admin/knowledge",
            get(admin_knowledge_get).post(admin_knowledge_update),
        )
        .route("/wunder/admin/knowledge/docs", get(admin_knowledge_docs))
        .route(
            "/wunder/admin/knowledge/doc",
            get(admin_knowledge_doc).delete(admin_knowledge_doc_delete),
        )
        .route(
            "/wunder/admin/knowledge/chunks",
            get(admin_knowledge_chunks),
        )
        .route(
            "/wunder/admin/knowledge/chunk/update",
            post(admin_knowledge_chunk_update),
        )
        .route(
            "/wunder/admin/knowledge/chunk/embed",
            post(admin_knowledge_chunk_embed),
        )
        .route(
            "/wunder/admin/knowledge/chunk/delete",
            post(admin_knowledge_chunk_delete),
        )
        .route("/wunder/admin/knowledge/test", post(admin_knowledge_test))
        .route(
            "/wunder/admin/knowledge/test/stream",
            post(admin_knowledge_test_stream),
        )
        .route(
            "/wunder/admin/knowledge/reindex",
            post(admin_knowledge_reindex),
        )
}

async fn admin_knowledge_get(State(state): State<Arc<AppState>>) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    Ok(Json(
        json!({ "knowledge": { "bases": config.knowledge.bases } }),
    ))
}

async fn admin_knowledge_update(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<KnowledgeUpdateRequest>,
) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let normalized = normalize_admin_knowledge_bases(&config, payload.knowledge.bases).await?;
    let removed_vector_bases = collect_removed_vector_bases(&config.knowledge.bases, &normalized);
    let removed_ragflow_dataset_ids =
        collect_removed_ragflow_dataset_ids(&config.knowledge.bases, &normalized);
    let changed_ragflow_parser_configs =
        collect_changed_ragflow_parser_configs(&config.knowledge.bases, &normalized);
    sync_changed_ragflow_parser_configs(&config, changed_ragflow_parser_configs)
        .await
        .map_err(vector_error_response)?;
    let removed_literal_bases = collect_removed_literal_bases(&config.knowledge.bases, &normalized);
    let updated = state
        .config_store
        .update(|config| {
            config.knowledge.bases = normalized.clone();
        })
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    cleanup_removed_vector_roots(state.storage.clone(), removed_vector_bases).await;
    cleanup_removed_ragflow_datasets(&config, removed_ragflow_dataset_ids).await;
    cleanup_removed_literal_roots(removed_literal_bases, &normalized).await;
    Ok(Json(
        json!({ "knowledge": { "bases": updated.knowledge.bases } }),
    ))
}

fn collect_removed_vector_bases(
    current: &[KnowledgeBaseConfig],
    next: &[KnowledgeBaseConfig],
) -> Vec<String> {
    let mut next_vector = HashSet::new();
    for base in next {
        if base.is_vector() {
            next_vector.insert(base.name.clone());
        }
    }
    current
        .iter()
        .filter(|base| base.is_vector())
        .filter(|base| !next_vector.contains(&base.name))
        .map(|base| base.name.clone())
        .collect()
}

fn collect_removed_ragflow_dataset_ids(
    current: &[KnowledgeBaseConfig],
    next: &[KnowledgeBaseConfig],
) -> Vec<String> {
    let mut next_dataset_ids = HashSet::new();
    for base in next {
        if !base.is_ragflow() {
            continue;
        }
        if let Some(dataset_id) =
            ragflow_knowledge::normalize_dataset_id(base.ragflow_dataset_id.as_deref())
        {
            next_dataset_ids.insert(dataset_id);
        }
    }
    let mut removed = Vec::new();
    for base in current {
        if !base.is_ragflow() {
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
    current: &[KnowledgeBaseConfig],
    next: &[KnowledgeBaseConfig],
) -> Vec<KnowledgeBaseConfig> {
    let mut current_by_dataset = HashMap::new();
    for base in current {
        if !base.is_ragflow() {
            continue;
        }
        if let Some(dataset_id) =
            ragflow_knowledge::normalize_dataset_id(base.ragflow_dataset_id.as_deref())
        {
            current_by_dataset.insert(dataset_id, base.clone());
        }
    }
    let mut changed = Vec::new();
    let mut seen = HashSet::new();
    for base in next {
        if !base.is_ragflow() {
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
        if ragflow_parser_config_changed(current_base, base) {
            changed.push(base.clone());
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

fn collect_removed_literal_bases(
    current: &[KnowledgeBaseConfig],
    next: &[KnowledgeBaseConfig],
) -> Vec<(String, String)> {
    let mut next_names = HashSet::new();
    for base in next {
        next_names.insert(base.name.clone());
    }
    current
        .iter()
        .filter(|base| !base.is_vector() && !base.is_ragflow())
        .filter(|base| !next_names.contains(&base.name))
        .map(|base| (base.name.clone(), base.root.clone()))
        .collect()
}

async fn cleanup_removed_ragflow_datasets(config: &Config, dataset_ids: Vec<String>) {
    if dataset_ids.is_empty() {
        return;
    }
    if let Err(err) = ragflow_knowledge::delete_datasets(config, &dataset_ids).await {
        info!("Failed to remove RAGFlow datasets: {err}");
    }
}

async fn cleanup_removed_vector_roots(storage: Arc<dyn StorageBackend>, bases: Vec<String>) {
    for name in bases {
        let owner_key = vector_knowledge::resolve_owner_key(None);
        let _ = storage.delete_vector_documents_by_base(&owner_key, &name);
        let root = match vector_knowledge::resolve_vector_root(None, &name, false) {
            Ok(path) => path,
            Err(err) => {
                info!("Failed to resolve vector knowledge root for {name}: {err}");
                continue;
            }
        };
        if let Err(err) = tokio::fs::remove_dir_all(&root).await {
            if err.kind() != ErrorKind::NotFound {
                info!(
                    "Failed to remove vector knowledge root {}: {}",
                    root.to_string_lossy(),
                    err
                );
            }
        }
    }
}

async fn cleanup_removed_literal_roots(bases: Vec<(String, String)>, next: &[KnowledgeBaseConfig]) {
    if bases.is_empty() {
        return;
    }

    let current_dir = match std::env::current_dir() {
        Ok(dir) => dir,
        Err(_) => PathBuf::from("."),
    };
    let knowledge_root = repo_assets::builtin_knowledge_root(&current_dir);
    let knowledge_root_compare =
        normalize_path_for_compare(&normalize_existing_path(&knowledge_root));

    // If a base is renamed but keeps the same root, never delete the folder.
    let mut next_root_keys = HashSet::new();
    for base in next {
        if base.is_vector() || base.is_ragflow() {
            continue;
        }
        let root = base.root.trim();
        if root.is_empty() {
            continue;
        }
        let root_path = PathBuf::from(root);
        let root_abs = if root_path.is_absolute() {
            root_path
        } else {
            current_dir.join(root_path)
        };
        let key = normalize_path_for_compare(&normalize_existing_path(&root_abs));
        next_root_keys.insert(key);
    }

    for (name, root_text) in bases {
        let trimmed = root_text.trim();
        if trimmed.is_empty() {
            continue;
        }
        let root_path = PathBuf::from(trimmed);
        let root_abs = if root_path.is_absolute() {
            root_path
        } else {
            current_dir.join(root_path)
        };
        let root_key = normalize_path_for_compare(&normalize_existing_path(&root_abs));
        if next_root_keys.contains(&root_key) {
            continue;
        }

        if root_key == knowledge_root_compare {
            info!(
                "Skip removing literal knowledge root for {name}: root points to {}",
                knowledge_root.to_string_lossy()
            );
            continue;
        }
        if !is_within_root(&knowledge_root, &root_abs) {
            info!(
                "Skip removing literal knowledge root for {name}: root is outside {} ({})",
                knowledge_root.to_string_lossy(),
                root_abs.to_string_lossy()
            );
            continue;
        }
        if !root_abs.exists() {
            continue;
        }
        if !root_abs.is_dir() {
            continue;
        }
        if let Err(err) = tokio::fs::remove_dir_all(&root_abs).await {
            if err.kind() != ErrorKind::NotFound {
                info!(
                    "Failed to remove literal knowledge root for {name} ({}): {err}",
                    root_abs.to_string_lossy()
                );
            }
        }
    }
}

async fn admin_knowledge_docs(
    State(state): State<Arc<AppState>>,
    Query(query): Query<KnowledgeDocsQuery>,
) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let base = resolve_knowledge_base(&config, &query.base)?;
    if base.is_ragflow() {
        let docs = ragflow_knowledge::list_documents(&config, &base)
            .await
            .map_err(vector_error_response)?;
        return Ok(Json(json!({ "base": query.base, "docs": docs })));
    }
    ensure_vector_base(&base)?;
    let root = resolve_vector_root_for_admin(&base, false)?;
    let docs =
        vector_knowledge::list_vector_documents(state.storage.as_ref(), None, &base.name, &root)
            .await
            .map_err(vector_error_response)?;
    Ok(Json(json!({ "base": query.base, "docs": docs })))
}

async fn admin_knowledge_doc(
    State(state): State<Arc<AppState>>,
    Query(query): Query<KnowledgeDocQuery>,
) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let base = resolve_knowledge_base(&config, &query.base)?;
    if base.is_ragflow() {
        let meta = ragflow_knowledge::read_document_meta(&config, &base, &query.doc_id)
            .await
            .map_err(vector_error_response)?;
        let content = ragflow_knowledge::download_document_content(&config, &base, &query.doc_id)
            .await
            .unwrap_or_default();
        return Ok(Json(
            json!({ "base": query.base, "doc": meta, "content": content }),
        ));
    }
    ensure_vector_base(&base)?;
    let root = resolve_vector_root_for_admin(&base, false)?;
    let meta = vector_knowledge::read_vector_document_meta(
        state.storage.as_ref(),
        None,
        &base.name,
        &root,
        &query.doc_id,
    )
    .await
    .map_err(vector_error_response)?;
    let content = vector_knowledge::read_vector_document_content(
        state.storage.as_ref(),
        None,
        &base.name,
        &root,
        &query.doc_id,
    )
    .await
    .map_err(vector_error_response)?;
    Ok(Json(
        json!({ "base": query.base, "doc": meta, "content": content }),
    ))
}

async fn admin_knowledge_doc_delete(
    State(state): State<Arc<AppState>>,
    Query(query): Query<KnowledgeDocQuery>,
) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let base = resolve_knowledge_base(&config, &query.base)?;
    if base.is_ragflow() {
        let meta = ragflow_knowledge::read_document_meta(&config, &base, &query.doc_id)
            .await
            .map_err(vector_error_response)?;
        ragflow_knowledge::delete_document(&config, &base, &query.doc_id)
            .await
            .map_err(vector_error_response)?;
        return Ok(Json(json!({
            "ok": true,
            "deleted": 0,
            "doc_id": meta.doc_id,
            "doc_name": meta.name
        })));
    }
    ensure_vector_base(&base)?;
    let root = resolve_vector_root_for_admin(&base, false)?;
    let root_for_lock = root.clone();
    let doc_id = query.doc_id.clone();
    let base_name = base.name.clone();
    let storage = state.storage.clone();
    let (meta, deleted) = vector_knowledge::with_document_lock(&root_for_lock, &doc_id, || {
        let storage = storage.clone();
        let base_name = base_name.clone();
        let root = root.clone();
        let doc_id = doc_id.clone();
        async move {
            let meta = vector_knowledge::read_vector_document_meta(
                storage.as_ref(),
                None,
                &base_name,
                &root,
                &doc_id,
            )
            .await
            .map_err(vector_error_response)?;
            let client = vector_knowledge::resolve_weaviate_client(&config)
                .map_err(vector_error_response)?;
            let owner_key = vector_knowledge::resolve_owner_key(None);
            let deleted = client
                .delete_doc_chunks_all(&owner_key, &base_name, &meta.embedding_model, &meta.doc_id)
                .await
                .map_err(vector_error_response)?;
            vector_knowledge::delete_vector_document_files(
                storage.as_ref(),
                None,
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
        "ok": true,
        "deleted": deleted,
        "doc_id": meta.doc_id,
        "doc_name": meta.name
    })))
}

async fn admin_knowledge_chunks(
    State(state): State<Arc<AppState>>,
    Query(query): Query<KnowledgeChunksQuery>,
) -> Result<Json<Value>, Response> {
    let config = state.config_store.get().await;
    let base = resolve_knowledge_base(&config, &query.base)?;
    if base.is_ragflow() {
        let chunks = ragflow_knowledge::list_chunks(&config, &base, &query.doc_id)
            .await
            .map_err(vector_error_response)?;
        return Ok(Json(json!({
            "base": query.base,
            "doc_id": query.doc_id,
            "chunks": chunks
        })));
    }
    ensure_vector_base(&base)?;
    let root = resolve_vector_root_for_admin(&base, false)?;
    let meta = vector_knowledge::read_vector_document_meta(
        state.storage.as_ref(),
        None,
        &base.name,
        &root,
        &query.doc_id,
    )
    .await
    .map_err(vector_error_response)?;
    let content = vector_knowledge::read_vector_document_content(
        state.storage.as_ref(),
        None,
        &base.name,
        &root,
        &query.doc_id,
    )
    .await
    .map_err(vector_error_response)?;
    let chunks = vector_knowledge::build_chunk_previews(&content, &meta).await;
    Ok(Json(json!({
        "base": query.base,
        "doc_id": query.doc_id,
        "chunks": chunks
    })))
}

type AdminKnowledgeTestEvent = (String, Value);

fn send_admin_knowledge_test_event(
    sender: &UnboundedSender<AdminKnowledgeTestEvent>,
    event_type: &str,
    payload: Value,
) {
    let _ = sender.send((event_type.to_string(), payload));
}

fn build_admin_vector_knowledge_test_hits(
    hits: Vec<vector_knowledge::VectorSearchHit>,
) -> Vec<Value> {
    hits.into_iter()
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
        .collect()
}

fn build_admin_ragflow_knowledge_test_hits(
    hits: Vec<ragflow_knowledge::RagflowSearchHit>,
) -> Vec<Value> {
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

fn build_admin_literal_knowledge_test_hits(docs: Vec<knowledge::KnowledgeDocument>) -> Vec<Value> {
    docs.into_iter()
        .map(|doc| {
            let document = if doc.document.trim().is_empty() {
                doc.name.clone()
            } else {
                doc.document.clone()
            };
            json!({
                "doc_id": doc.code,
                "document": document,
                "chunk_index": Value::Null,
                "content": doc.content,
                "score": doc.score,
                "section_path": doc.section_path,
                "reason": doc.reason
            })
        })
        .collect()
}

async fn admin_knowledge_test_stream(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<KnowledgeTestRequest>,
) -> Result<Response, Response> {
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
    let top_k = payload.top_k;
    let config = state.config_store.get().await;
    let base = resolve_knowledge_base(&config, base_name)?;
    let query = query.to_string();

    let (event_tx, event_rx) = unbounded_channel::<AdminKnowledgeTestEvent>();
    tokio::spawn(async move {
        let outcome = async {
            if base.is_vector() {
                let embedding_name = base.embedding_model.as_deref().unwrap_or("").trim();
                let embed_config =
                    vector_knowledge::resolve_embedding_model(&config, embedding_name)?;
                let timeout_s = embed_config.timeout_s.unwrap_or(120);
                let effective_top_k = top_k
                    .filter(|value| *value > 0)
                    .unwrap_or_else(|| vector_knowledge::resolve_top_k(&base));
                send_admin_knowledge_test_event(
                    &event_tx,
                    "request",
                    json!({
                        "knowledge_base": base.name,
                        "base_type": "vector",
                        "query": query,
                        "embedding_model": embedding_name,
                        "top_k": effective_top_k,
                    }),
                );
                let vectors = llm::embed_texts(&embed_config, &[query.clone()], timeout_s).await?;
                let vector = vectors
                    .first()
                    .ok_or_else(|| anyhow!(i18n::t("error.llm_request_failed")))?;
                let client = vector_knowledge::resolve_weaviate_client(&config)?;
                let owner_key = vector_knowledge::resolve_owner_key(None);
                let mut hits = client
                    .query_chunks(
                        &owner_key,
                        &base.name,
                        embedding_name,
                        vector,
                        effective_top_k,
                    )
                    .await?;
                if let Some(threshold) = base.score_threshold {
                    hits.retain(|hit| hit.score.unwrap_or(0.0) >= f64::from(threshold));
                }
                if hits.len() > effective_top_k {
                    hits.truncate(effective_top_k);
                }
                send_admin_knowledge_test_event(
                    &event_tx,
                    "complete",
                    json!({
                        "base": base.name,
                        "query": query,
                        "embedding_model": embedding_name,
                        "top_k": effective_top_k,
                        "hits": build_admin_vector_knowledge_test_hits(hits),
                    }),
                );
                return Ok::<(), anyhow::Error>(());
            }
            if base.is_ragflow() {
                let effective_top_k = top_k
                    .filter(|value| *value > 0)
                    .unwrap_or_else(|| ragflow_knowledge::resolve_top_k(&base));
                send_admin_knowledge_test_event(
                    &event_tx,
                    "request",
                    json!({
                        "knowledge_base": base.name,
                        "base_type": "ragflow",
                        "query": query,
                        "embedding_model": "ragflow",
                        "top_k": effective_top_k,
                    }),
                );
                let hits =
                    ragflow_knowledge::retrieve(&config, &base, &query, effective_top_k).await?;
                send_admin_knowledge_test_event(
                    &event_tx,
                    "complete",
                    json!({
                        "base": base.name,
                        "query": query,
                        "embedding_model": "ragflow",
                        "top_k": effective_top_k,
                        "hits": build_admin_ragflow_knowledge_test_hits(hits),
                    }),
                );
                return Ok::<(), anyhow::Error>(());
            }

            let _ = knowledge::resolve_knowledge_root(&base, false)?;
            let llm_config = knowledge::resolve_llm_config(&config, None);
            let request_sender = event_tx.clone();
            let request_logger = move |request: Value| {
                send_admin_knowledge_test_event(&request_sender, "request", request);
            };
            let delta_sender = event_tx.clone();
            let (reply, reasoning, docs) = knowledge::query_knowledge_raw_with_documents_streaming(
                &query,
                &base,
                llm_config.as_ref(),
                top_k,
                Some(&request_logger),
                move |content_delta: String, reasoning_delta: String| {
                    let delta_sender = delta_sender.clone();
                    async move {
                        if !reasoning_delta.is_empty() {
                            send_admin_knowledge_test_event(
                                &delta_sender,
                                "reasoning",
                                json!({ "delta": reasoning_delta }),
                            );
                        }
                        if !content_delta.is_empty() {
                            send_admin_knowledge_test_event(
                                &delta_sender,
                                "output",
                                json!({ "delta": content_delta }),
                            );
                        }
                        Ok::<(), anyhow::Error>(())
                    }
                },
            )
            .await?;
            send_admin_knowledge_test_event(
                &event_tx,
                "complete",
                json!({
                    "base": base.name,
                    "query": query,
                    "text": reply,
                    "reasoning": reasoning,
                    "hits": build_admin_literal_knowledge_test_hits(docs),
                }),
            );
            Ok::<(), anyhow::Error>(())
        }
        .await;

        if let Err(err) = outcome {
            send_admin_knowledge_test_event(
                &event_tx,
                "error",
                json!({ "message": err.to_string() }),
            );
        }
    });

    let stream = UnboundedReceiverStream::new(event_rx).map(|(event_type, payload)| {
        Ok::<Event, Infallible>(Event::default().event(event_type).data(payload.to_string()))
    });
    let sse = Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)));
    Ok(sse.into_response())
}

async fn admin_knowledge_test(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<KnowledgeTestRequest>,
) -> Result<Json<Value>, Response> {
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
    let config = state.config_store.get().await;
    let base = resolve_knowledge_base(&config, base_name)?;
    if base.is_vector() {
        let embedding_name = base.embedding_model.as_deref().unwrap_or("").trim();
        let embed_config = vector_knowledge::resolve_embedding_model(&config, embedding_name)
            .map_err(vector_error_response)?;
        let timeout_s = embed_config.timeout_s.unwrap_or(120);
        let vectors = llm::embed_texts(&embed_config, &[query.to_string()], timeout_s)
            .await
            .map_err(vector_error_response)?;
        let vector = vectors.first().ok_or_else(|| {
            error_response(StatusCode::BAD_REQUEST, i18n::t("error.llm_request_failed"))
        })?;
        let top_k = payload
            .top_k
            .filter(|value| *value > 0)
            .unwrap_or_else(|| vector_knowledge::resolve_top_k(&base));
        let client =
            vector_knowledge::resolve_weaviate_client(&config).map_err(vector_error_response)?;
        let owner_key = vector_knowledge::resolve_owner_key(None);
        let mut hits = client
            .query_chunks(&owner_key, &base.name, embedding_name, vector, top_k)
            .await
            .map_err(vector_error_response)?;
        if let Some(threshold) = base.score_threshold {
            hits.retain(|hit| hit.score.unwrap_or(0.0) >= f64::from(threshold));
        }
        if hits.len() > top_k {
            hits.truncate(top_k);
        }
        let request = json!({
            "knowledge_base": base.name,
            "base_type": "vector",
            "query": query,
            "embedding_model": embedding_name,
            "top_k": top_k,
        });
        Ok(Json(json!({
            "base": base.name,
            "query": query,
            "embedding_model": embedding_name,
            "top_k": top_k,
            "request": request,
            "hits": build_admin_vector_knowledge_test_hits(hits)
        })))
    } else if base.is_ragflow() {
        let top_k = payload
            .top_k
            .filter(|value| *value > 0)
            .unwrap_or_else(|| ragflow_knowledge::resolve_top_k(&base));
        let hits = ragflow_knowledge::retrieve(&config, &base, query, top_k)
            .await
            .map_err(vector_error_response)?;
        let request = json!({
            "knowledge_base": base.name,
            "base_type": "ragflow",
            "query": query,
            "embedding_model": "ragflow",
            "top_k": top_k,
        });
        Ok(Json(json!({
            "base": base.name,
            "query": query,
            "embedding_model": "ragflow",
            "top_k": top_k,
            "request": request,
            "hits": build_admin_ragflow_knowledge_test_hits(hits)
        })))
    } else {
        let _ = resolve_knowledge_root(&base, false)?;
        let llm_config = knowledge::resolve_llm_config(&config, None);
        let request_log = StdMutex::new(None::<Value>);
        let request_logger = |request: Value| {
            let mut guard = request_log.lock().expect("knowledge test request log lock");
            *guard = Some(request);
        };
        let (reply, reasoning, docs) = knowledge::query_knowledge_raw_with_documents(
            query,
            &base,
            llm_config.as_ref(),
            payload.top_k,
            Some(&request_logger),
        )
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        let request = request_log
            .lock()
            .expect("knowledge test request log lock")
            .clone();
        Ok(Json(json!({
            "base": base.name,
            "query": query,
            "request": request,
            "text": reply,
            "reasoning": reasoning,
            "hits": build_admin_literal_knowledge_test_hits(docs)
        })))
    }
}

async fn admin_knowledge_chunk_update(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<KnowledgeChunkUpdateRequest>,
) -> Result<Json<Value>, Response> {
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
    let content_text = payload.content.trim();
    if content_text.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.content_required"),
        ));
    }
    let config = state.config_store.get().await;
    let base = resolve_knowledge_base(&config, base_name)?;
    if base.is_ragflow() {
        ragflow_knowledge::update_chunk(
            &config,
            &base,
            doc_id,
            payload.chunk_index,
            &payload.content,
        )
        .await
        .map_err(vector_error_response)?;
        let meta = ragflow_knowledge::read_document_meta(&config, &base, doc_id)
            .await
            .map_err(vector_error_response)?;
        return Ok(Json(json!({ "ok": true, "doc": meta })));
    }
    ensure_vector_base(&base)?;
    let root = resolve_vector_root_for_admin(&base, false)?;
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
                None,
                &base_name,
                &root,
                &doc_id,
            )
            .await
            .map_err(vector_error_response)?;
            let content = vector_knowledge::read_vector_document_content(
                storage.as_ref(),
                None,
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
            chunk.content = Some(content_text.to_string());
            chunk.status = Some("pending".to_string());
            vector_knowledge::refresh_document_meta(&mut meta);
            vector_knowledge::write_vector_document(
                storage.as_ref(),
                None,
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
    Ok(Json(json!({ "ok": true, "doc": meta })))
}

async fn admin_knowledge_chunk_embed(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<KnowledgeChunkActionRequest>,
) -> Result<Json<Value>, Response> {
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
    let config = state.config_store.get().await;
    let base = resolve_knowledge_base(&config, base_name)?;
    if base.is_ragflow() {
        ragflow_knowledge::set_chunk_available(&config, &base, doc_id, payload.chunk_index, true)
            .await
            .map_err(vector_error_response)?;
        let meta = ragflow_knowledge::read_document_meta(&config, &base, doc_id)
            .await
            .map_err(vector_error_response)?;
        return Ok(Json(json!({ "ok": true, "doc": meta })));
    }
    ensure_vector_base(&base)?;
    let root = resolve_vector_root_for_admin(&base, false)?;
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
                None,
                &base_name,
                &root,
                &doc_id,
            )
            .await
            .map_err(vector_error_response)?;
            let content = vector_knowledge::read_vector_document_content(
                storage.as_ref(),
                None,
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
            let embedding_name = base
                .embedding_model
                .as_deref()
                .unwrap_or("")
                .trim()
                .to_string();
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
            let owner_key = vector_knowledge::resolve_owner_key(None);
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
                None,
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
    Ok(Json(json!({ "ok": true, "doc": meta })))
}

async fn admin_knowledge_chunk_delete(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<KnowledgeChunkActionRequest>,
) -> Result<Json<Value>, Response> {
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
    let config = state.config_store.get().await;
    let base = resolve_knowledge_base(&config, base_name)?;
    if base.is_ragflow() {
        ragflow_knowledge::delete_chunk(&config, &base, doc_id, payload.chunk_index)
            .await
            .map_err(vector_error_response)?;
        let meta = ragflow_knowledge::read_document_meta(&config, &base, doc_id)
            .await
            .map_err(vector_error_response)?;
        return Ok(Json(json!({ "ok": true, "doc": meta })));
    }
    ensure_vector_base(&base)?;
    let root = resolve_vector_root_for_admin(&base, false)?;
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
                None,
                &base_name,
                &root,
                &doc_id,
            )
            .await
            .map_err(vector_error_response)?;
            let content = vector_knowledge::read_vector_document_content(
                storage.as_ref(),
                None,
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
            let client = vector_knowledge::resolve_weaviate_client(&config)
                .map_err(vector_error_response)?;
            let _ = client
                .delete_chunk(&vector_knowledge::build_chunk_id(&meta.doc_id, chunk.index))
                .await;
            chunk.status = Some("deleted".to_string());
            vector_knowledge::refresh_document_meta(&mut meta);
            vector_knowledge::write_vector_document(
                storage.as_ref(),
                None,
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
    Ok(Json(json!({ "ok": true, "doc": meta })))
}

async fn admin_knowledge_reindex(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<KnowledgeReindexRequest>,
) -> Result<Json<Value>, Response> {
    let base_name = payload.base.trim();
    if base_name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.knowledge_base_name_required"),
        ));
    }
    let config = state.config_store.get().await;
    let base = resolve_knowledge_base(&config, base_name)?;
    if base.is_ragflow() {
        let mut targets = Vec::new();
        if let Some(doc_id) = payload.doc_id.as_deref() {
            let cleaned = doc_id.trim();
            if !cleaned.is_empty() {
                targets.push(cleaned.to_string());
            }
        }
        if targets.is_empty() {
            let docs = ragflow_knowledge::list_documents(&config, &base)
                .await
                .map_err(vector_error_response)?;
            targets = docs.into_iter().map(|doc| doc.doc_id).collect();
        }
        ragflow_knowledge::reparse_documents(&config, &base, &targets)
            .await
            .map_err(vector_error_response)?;
        return Ok(Json(json!({
            "ok": true,
            "reindexed": targets,
            "failed": []
        })));
    }
    ensure_vector_base(&base)?;
    let root = resolve_vector_root_for_admin(&base, true)?;
    let base_name = base.name.clone();
    let storage = state.storage.clone();
    let mut targets = Vec::new();
    if let Some(doc_id) = payload.doc_id.as_deref() {
        let cleaned = doc_id.trim();
        if !cleaned.is_empty() {
            targets.push(cleaned.to_string());
        }
    }
    if targets.is_empty() {
        let docs =
            vector_knowledge::list_vector_documents(storage.as_ref(), None, &base_name, &root)
                .await
                .map_err(vector_error_response)?;
        targets = docs.into_iter().map(|doc| doc.doc_id).collect();
    }
    let mut reindexed = Vec::new();
    let mut failed = Vec::new();
    for doc_id in targets {
        let meta = match vector_knowledge::read_vector_document_meta(
            storage.as_ref(),
            None,
            &base_name,
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
            None,
            &base_name,
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
            &base,
            None,
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
        "ok": failed.is_empty(),
        "reindexed": reindexed,
        "failed": failed
    })))
}

pub(super) fn resolve_knowledge_base(
    config: &Config,
    base_name: &str,
) -> Result<KnowledgeBaseConfig, Response> {
    let name = base_name.trim();
    if name.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.knowledge_base_name_required"),
        ));
    }
    config
        .knowledge
        .bases
        .iter()
        .find(|item| item.name == name)
        .cloned()
        .ok_or_else(|| {
            error_response(
                StatusCode::NOT_FOUND,
                i18n::t("error.knowledge_base_not_found"),
            )
        })
}

pub(super) fn resolve_knowledge_root(
    base: &KnowledgeBaseConfig,
    create: bool,
) -> Result<PathBuf, Response> {
    knowledge::resolve_knowledge_root(base, create)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))
}

pub(super) fn resolve_knowledge_path(
    root: &Path,
    relative_path: &str,
) -> Result<PathBuf, Response> {
    let rel = Path::new(relative_path);
    if rel.is_absolute() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.absolute_path_forbidden"),
        ));
    }
    let target = root.join(rel);
    let resolved = normalize_target_path(&target);
    let normalized_root = normalize_existing_path(root);
    // Windows 有时会生成 \\?\ 前缀，这里做统一化比较避免误报路径越界。
    let root_compare = normalize_path_for_compare(&normalized_root);
    let target_compare = normalize_path_for_compare(&resolved);
    if resolved != root && !target_compare.starts_with(&root_compare) {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.path_out_of_bounds"),
        ));
    }
    Ok(resolved)
}

async fn normalize_admin_knowledge_bases(
    config: &Config,
    bases: Vec<KnowledgeBaseConfig>,
) -> Result<Vec<KnowledgeBaseConfig>, Response> {
    let mut output = Vec::new();
    for mut base in bases {
        base.name = base.name.trim().to_string();
        base.description = base.description.trim().to_string();
        if base.name.is_empty() {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("error.knowledge_base_name_required"),
            ));
        }
        let base_type = normalize_knowledge_base_type(base.base_type.as_deref());
        if base_type == KnowledgeBaseType::Vector {
            let embedding_model = base
                .embedding_model
                .as_deref()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    error_response(
                        StatusCode::BAD_REQUEST,
                        i18n::t("error.embedding_model_required"),
                    )
                })?;
            vector_knowledge::resolve_embedding_model(config, &embedding_model)
                .map_err(vector_error_response)?;
            let root = resolve_vector_root_for_admin(&base, true)?;
            base.root = root.to_string_lossy().to_string();
            base.base_type = Some("vector".to_string());
            base.embedding_model = Some(embedding_model);
            base.ragflow_dataset_id = None;
            base.ragflow_dataset_managed = None;
            base.chunk_method = None;
            base.chunk_delimiter = None;
            base.layout_recognize = None;
            base.auto_keywords = None;
            base.auto_questions = None;
            base.html4excel = None;
        } else if base_type == KnowledgeBaseType::Ragflow {
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
            base.ragflow_dataset_id =
                ragflow_knowledge::normalize_dataset_id(base.ragflow_dataset_id.as_deref())
                    .or_else(|| ragflow_knowledge::normalize_synthetic_root_dataset_id(&base.root));
            if base.ragflow_dataset_id.is_none() {
                let remote_name = build_admin_ragflow_dataset_name(&base.name);
                let dataset_id =
                    ragflow_knowledge::create_dataset_with_name(config, &base, &remote_name)
                        .await
                        .map_err(vector_error_response)?;
                base.ragflow_dataset_id = Some(dataset_id);
                base.ragflow_dataset_managed = Some(true);
            } else if base.ragflow_dataset_managed.is_none() {
                base.ragflow_dataset_managed = Some(false);
            }
            base.root =
                ragflow_knowledge::synthetic_root(base.ragflow_dataset_id.as_deref().unwrap_or(""));
        } else {
            if base.root.trim().is_empty() {
                base.root = repo_assets::default_literal_knowledge_root(&base.name);
            } else {
                base.root = base.root.trim().to_string();
            }
            base.base_type = None;
            base.embedding_model = None;
            base.ragflow_dataset_id = None;
            base.ragflow_dataset_managed = None;
            base.chunk_method = None;
            base.chunk_delimiter = None;
            base.layout_recognize = None;
            base.auto_keywords = None;
            base.auto_questions = None;
            base.html4excel = None;
        }
        output.push(base);
    }
    Ok(output)
}

fn build_admin_ragflow_dataset_name(base_name: &str) -> String {
    let name = ragflow_knowledge::normalize_dataset_name_part(base_name, "knowledge");
    format!("[Wunder Admin] {name}")
}

pub(super) fn resolve_vector_root_for_admin(
    base: &KnowledgeBaseConfig,
    create: bool,
) -> Result<PathBuf, Response> {
    vector_knowledge::resolve_vector_root(None, &base.name, create)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))
}

fn ensure_vector_base(base: &KnowledgeBaseConfig) -> Result<(), Response> {
    if !base.is_vector() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.vector_knowledge_required"),
        ));
    }
    vector_knowledge::ensure_vector_base_config(base)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(())
}

pub(super) fn vector_error_response(err: anyhow::Error) -> Response {
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
struct KnowledgeUpdateRequest {
    knowledge: KnowledgePayload,
}

#[derive(Debug, Deserialize)]
struct KnowledgePayload {
    bases: Vec<KnowledgeBaseConfig>,
}

#[derive(Debug, Deserialize)]
struct KnowledgeDocsQuery {
    base: String,
}

#[derive(Debug, Deserialize)]
struct KnowledgeDocQuery {
    base: String,
    doc_id: String,
}

#[derive(Debug, Deserialize)]
struct KnowledgeChunksQuery {
    base: String,
    doc_id: String,
}

#[derive(Debug, Deserialize)]
struct KnowledgeTestRequest {
    base: String,
    query: String,
    #[serde(default)]
    top_k: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct KnowledgeChunkUpdateRequest {
    base: String,
    doc_id: String,
    chunk_index: usize,
    content: String,
}

#[derive(Debug, Deserialize)]
struct KnowledgeChunkActionRequest {
    base: String,
    doc_id: String,
    chunk_index: usize,
}

#[derive(Debug, Deserialize)]
struct KnowledgeReindexRequest {
    base: String,
    #[serde(default)]
    doc_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{build_admin_ragflow_dataset_name, collect_removed_ragflow_dataset_ids};
    use crate::config::KnowledgeBaseConfig;

    #[test]
    fn admin_ragflow_dataset_name_uses_management_prefix() {
        assert_eq!(
            build_admin_ragflow_dataset_name("  shared   docs  "),
            "[Wunder Admin] shared docs"
        );
    }

    #[test]
    fn removed_ragflow_dataset_ids_skip_unmanaged_datasets() {
        let current = vec![
            KnowledgeBaseConfig {
                name: "managed".to_string(),
                base_type: Some("ragflow".to_string()),
                ragflow_dataset_id: Some("dataset_managed".to_string()),
                ragflow_dataset_managed: Some(true),
                ..Default::default()
            },
            KnowledgeBaseConfig {
                name: "external".to_string(),
                base_type: Some("ragflow".to_string()),
                ragflow_dataset_id: Some("dataset_external".to_string()),
                ragflow_dataset_managed: Some(false),
                ..Default::default()
            },
        ];

        assert_eq!(
            collect_removed_ragflow_dataset_ids(&current, &[]),
            vec!["dataset_managed".to_string()]
        );
    }
}
