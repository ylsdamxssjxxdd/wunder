use super::{build_model_tool_success_with_hint, context::ToolContext};
use crate::config::{
    is_debug_log_level, normalize_knowledge_base_type, Config, KnowledgeBaseConfig,
    KnowledgeBaseType,
};
use crate::i18n;
use crate::knowledge;
use crate::llm::embed_texts;
use crate::ragflow_knowledge;
use crate::user_tools::UserToolAlias;
use crate::vector_knowledge;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::HashSet;

pub(crate) async fn execute_user_knowledge_tool(
    context: &ToolContext<'_>,
    alias: &UserToolAlias,
    args: &Value,
) -> Result<Value> {
    let Some(query) = resolve_query_text(args) else {
        return Err(anyhow!(i18n::t("error.knowledge_query_required")));
    };
    let store = context
        .user_tool_store
        .ok_or_else(|| anyhow!(i18n::t("error.knowledge_base_not_found")))?;
    let payload = store.load_user_tools(&alias.owner_id);
    let base_info = payload
        .knowledge_bases
        .iter()
        .find(|base| base.name == alias.target)
        .cloned()
        .ok_or_else(|| anyhow!(i18n::t("error.knowledge_base_not_found")))?;
    let base_type = normalize_knowledge_base_type(base_info.base_type.as_deref());
    let root = store
        .resolve_knowledge_base_root_with_type(&alias.owner_id, &base_info.name, base_type, false)
        .map_err(|err| anyhow!(err.to_string()))?;
    let base = KnowledgeBaseConfig {
        name: base_info.name.clone(),
        description: base_info.description.clone(),
        root: if base_type == KnowledgeBaseType::Ragflow {
            ragflow_knowledge::synthetic_root(base_info.ragflow_dataset_id.as_deref().unwrap_or(""))
        } else {
            root.to_string_lossy().to_string()
        },
        enabled: base_info.enabled,
        shared: Some(base_info.shared),
        base_type: base_info.base_type.clone(),
        embedding_model: base_info.embedding_model.clone(),
        ragflow_dataset_id: base_info.ragflow_dataset_id.clone(),
        ragflow_dataset_managed: base_info.ragflow_dataset_managed,
        chunk_method: base_info.chunk_method.clone(),
        chunk_delimiter: base_info.chunk_delimiter.clone(),
        layout_recognize: base_info.layout_recognize.clone(),
        auto_keywords: base_info.auto_keywords,
        auto_questions: base_info.auto_questions,
        html4excel: base_info.html4excel,
        chunk_size: base_info.chunk_size,
        chunk_overlap: base_info.chunk_overlap,
        top_k: base_info.top_k,
        score_threshold: base_info.score_threshold,
    };
    if base_type == KnowledgeBaseType::Ragflow {
        return execute_ragflow_knowledge(context, &base, args).await;
    }
    if base_type == KnowledgeBaseType::Vector {
        return execute_vector_knowledge(context, &base, Some(&alias.owner_id), args).await;
    }
    execute_plain_knowledge(context, &base, &query, args).await
}

pub(crate) async fn execute_knowledge_tool(
    context: &ToolContext<'_>,
    base: &KnowledgeBaseConfig,
    args: &Value,
) -> Result<Value> {
    let Some(query) = resolve_query_text(args) else {
        return Err(anyhow!(i18n::t("error.knowledge_query_required")));
    };
    if base.is_vector() {
        return execute_vector_knowledge(context, base, None, args).await;
    }
    if base.is_ragflow() {
        return execute_ragflow_knowledge(context, base, args).await;
    }
    let _ =
        knowledge::resolve_knowledge_root(base, false).map_err(|err| anyhow!(err.to_string()))?;
    execute_plain_knowledge(context, base, &query, args).await
}

async fn execute_plain_knowledge(
    context: &ToolContext<'_>,
    base: &KnowledgeBaseConfig,
    query: &str,
    args: &Value,
) -> Result<Value> {
    let llm_config = knowledge::resolve_llm_config(context.config, None);
    let docs = if let Some(emitter) = context.event_emitter.as_ref() {
        let include_payload = is_debug_log_level(&context.config.observability.log_level);
        let log_request = |mut payload: Value| {
            if !include_payload {
                if let Value::Object(ref mut map) = payload {
                    map.remove("payload");
                }
            }
            emitter.emit("knowledge_request", payload);
        };
        knowledge::query_knowledge_documents(
            query,
            base,
            llm_config.as_ref(),
            extract_limit(args),
            Some(&log_request),
        )
        .await
    } else {
        knowledge::query_knowledge_documents(
            query,
            base,
            llm_config.as_ref(),
            extract_limit(args),
            None,
        )
        .await
    };
    let documents = docs
        .into_iter()
        .map(|doc| doc.to_value())
        .collect::<Vec<_>>();
    Ok(build_knowledge_tool_success(
        &base.name,
        Some(query),
        &[],
        None,
        false,
        documents,
        None,
    ))
}

async fn execute_ragflow_knowledge(
    context: &ToolContext<'_>,
    base: &KnowledgeBaseConfig,
    args: &Value,
) -> Result<Value> {
    let keywords = extract_keywords(args);
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let queries = if !keywords.is_empty() {
        keywords
    } else if !query.is_empty() {
        vec![query.clone()]
    } else {
        return Err(anyhow!(i18n::t("error.knowledge_query_required")));
    };
    ragflow_knowledge::ensure_ragflow_base_config(base)?;
    let top_k = extract_limit(args).unwrap_or_else(|| ragflow_knowledge::resolve_top_k(base));
    let mut grouped_results = Vec::new();
    let mut flat_documents = Vec::new();
    let multi_query = queries.len() > 1;
    let mut seen_chunks = HashSet::new();
    for keyword in &queries {
        let hits = ragflow_knowledge::retrieve(context.config, base, keyword, top_k).await?;
        let documents = hits
            .into_iter()
            .filter_map(|hit| {
                if multi_query {
                    let key = format!("{}::{}", hit.doc_id, hit.chunk_id);
                    if !seen_chunks.insert(key) {
                        return None;
                    }
                }
                let mut doc = json!({
                    "doc_id": hit.doc_id,
                    "document": hit.doc_name,
                    "name": hit.doc_name,
                    "chunk_id": hit.chunk_id,
                    "chunk_index": hit.chunk_index,
                    "start": hit.start,
                    "end": hit.end,
                    "content": hit.content,
                    "embedding_model": "ragflow",
                    "score": hit.score
                });
                if multi_query {
                    doc["keyword"] = json!(keyword);
                }
                Some(doc)
            })
            .collect::<Vec<_>>();
        if multi_query {
            flat_documents.extend(documents.clone());
        }
        grouped_results.push(json!({
            "keyword": keyword,
            "documents": documents
        }));
    }
    if let Some(emitter) = context.event_emitter.as_ref() {
        let mut payload = json!({
            "knowledge_base": base.name,
            "vector": true,
            "engine": "ragflow",
            "embedding_model": "ragflow",
            "limit": top_k,
            "score_threshold": base.score_threshold,
        });
        if queries.len() == 1 {
            payload["query"] = json!(queries[0].clone());
        } else {
            payload["keywords"] = json!(queries.clone());
        }
        emitter.emit("knowledge_request", payload);
    }
    let documents = if queries.len() == 1 {
        grouped_results
            .first()
            .and_then(|value| value.get("documents"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    } else {
        flat_documents
    };
    Ok(build_knowledge_tool_success(
        &base.name,
        if query.is_empty() {
            None
        } else {
            Some(query.as_str())
        },
        &queries,
        Some("ragflow"),
        true,
        documents,
        (queries.len() > 1).then_some(grouped_results),
    ))
}

async fn execute_vector_knowledge(
    context: &ToolContext<'_>,
    base: &KnowledgeBaseConfig,
    owner_id: Option<&str>,
    args: &Value,
) -> Result<Value> {
    let keywords = extract_keywords(args);
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let queries = if !keywords.is_empty() {
        keywords
    } else if !query.is_empty() {
        vec![query.clone()]
    } else {
        return Err(anyhow!(i18n::t("error.knowledge_query_required")));
    };
    vector_knowledge::ensure_vector_base_type(base)?;
    let embedding_name = base
        .embedding_model
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_string();
    let owner_key = vector_knowledge::resolve_owner_key(owner_id);
    let top_k = extract_limit(args).unwrap_or_else(|| vector_knowledge::resolve_top_k(base));
    let root = std::path::PathBuf::from(&base.root);
    let aggregated = if let Ok(embed_config) =
        vector_knowledge::resolve_embedding_model(context.config, &embedding_name)
    {
        let timeout_s = embed_config.timeout_s.unwrap_or(120);
        match embed_texts(&embed_config, &queries, timeout_s).await {
            Ok(vectors) if vectors.len() == queries.len() => {
                let query_results = futures::future::join_all(vectors.into_iter().enumerate().map(
                    |(index, vector)| {
                        let storage = context.storage.clone();
                        let base = base.clone();
                        let root = root.clone();
                        let embedding_name = embedding_name.clone();
                        let owner_id = owner_id.map(str::to_string);
                        let keyword = queries.get(index).cloned().unwrap_or_default();
                        let score_threshold = base.score_threshold;
                        async move {
                            let mut hits = vector_knowledge::query_chunks_by_vector(
                                storage.as_ref(),
                                owner_id.as_deref(),
                                &base,
                                &root,
                                &embedding_name,
                                &vector,
                                top_k,
                            )
                            .await?;
                            if let Some(threshold) = score_threshold {
                                hits.retain(|hit| hit.score.unwrap_or(0.0) >= f64::from(threshold));
                            }
                            if hits.len() > top_k {
                                hits.truncate(top_k);
                            }
                            Ok::<_, anyhow::Error>((index, keyword, hits))
                        }
                    },
                ))
                .await;
                let mut aggregated = Vec::new();
                let mut failed = false;
                for result in query_results {
                    match result {
                        Ok(item) => aggregated.push(item),
                        Err(_) => {
                            failed = true;
                            break;
                        }
                    }
                }
                if !failed {
                    aggregated.sort_by_key(|(index, _, _)| *index);
                    (aggregated, false)
                } else {
                    let fallback = build_vector_literal_fallback_results(
                        context, base, owner_id, &queries, top_k, &root,
                    )
                    .await?;
                    (fallback, true)
                }
            }
            _ => {
                let fallback = build_vector_literal_fallback_results(
                    context, base, owner_id, &queries, top_k, &root,
                )
                .await?;
                (fallback, true)
            }
        }
    } else {
        let fallback =
            build_vector_literal_fallback_results(context, base, owner_id, &queries, top_k, &root)
                .await?;
        (fallback, true)
    };
    let (aggregated, fallback_mode) = aggregated;
    if let Some(emitter) = context.event_emitter.as_ref() {
        let mut payload = json!({
            "knowledge_base": base.name,
            "vector": true,
            "embedding_model": embedding_name.clone(),
            "owner_id": owner_key,
            "limit": top_k,
            "score_threshold": base.score_threshold,
            "fallback_mode": fallback_mode
        });
        if queries.len() == 1 {
            payload["query"] = json!(queries[0].clone());
        } else {
            payload["keywords"] = json!(queries.clone());
        }
        emitter.emit("knowledge_request", payload);
    }
    let mut grouped_results = Vec::new();
    let mut flat_documents = Vec::new();
    let multi_query = queries.len() > 1;
    let mut seen_chunks = HashSet::new();
    for (_, keyword, hits) in aggregated {
        let documents = hits
            .into_iter()
            .filter_map(|hit| {
                if multi_query {
                    let key = format!("{}::{}", hit.doc_id, hit.chunk_index);
                    if !seen_chunks.insert(key) {
                        return None;
                    }
                }
                let mut doc = json!({
                    "doc_id": hit.doc_id,
                    "document": hit.doc_name,
                    "name": hit.doc_name,
                    "chunk_index": hit.chunk_index,
                    "start": hit.start,
                    "end": hit.end,
                    "content": hit.content,
                    "embedding_model": hit.embedding_model,
                    "score": hit.score
                });
                if multi_query {
                    doc["keyword"] = json!(keyword);
                }
                Some(doc)
            })
            .collect::<Vec<_>>();
        if multi_query {
            flat_documents.extend(documents.clone());
        }
        grouped_results.push(json!({
            "keyword": keyword,
            "documents": documents
        }));
    }
    let documents = if queries.len() == 1 {
        grouped_results
            .first()
            .and_then(|value| value.get("documents"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    } else {
        flat_documents
    };
    Ok(build_knowledge_tool_success(
        &base.name,
        if query.is_empty() {
            None
        } else {
            Some(query.as_str())
        },
        &queries,
        Some(embedding_name.as_str()),
        true,
        documents,
        (queries.len() > 1).then_some(grouped_results),
    ))
}

async fn build_vector_literal_fallback_results(
    context: &ToolContext<'_>,
    base: &KnowledgeBaseConfig,
    owner_id: Option<&str>,
    queries: &[String],
    top_k: usize,
    root: &std::path::Path,
) -> Result<Vec<(usize, String, Vec<vector_knowledge::VectorSearchHit>)>> {
    let mut aggregated = Vec::with_capacity(queries.len());
    for (index, keyword) in queries.iter().enumerate() {
        let hits = vector_knowledge::query_chunks_by_text(
            context.storage.as_ref(),
            owner_id,
            base,
            root,
            keyword,
            top_k,
        )
        .await?;
        aggregated.push((index, keyword.clone(), hits));
    }
    Ok(aggregated)
}

pub(crate) fn find_knowledge_base(config: &Config, name: &str) -> Option<KnowledgeBaseConfig> {
    config
        .knowledge
        .bases
        .iter()
        .find(|base| base.enabled && base.name == name && !base.root.trim().is_empty())
        .cloned()
}

fn compact_knowledge_document_for_model(item: &Value) -> Value {
    json!({
        "document": item.get("document").cloned().unwrap_or(Value::Null),
        "name": item.get("name").cloned().unwrap_or(Value::Null),
        "section_path": item.get("section_path").cloned().unwrap_or(Value::Null),
        "content": item.get("content").cloned().unwrap_or(Value::Null),
        "score": item.get("score").cloned().unwrap_or(Value::Null),
        "reason": item.get("reason").cloned().unwrap_or(Value::Null),
    })
}

fn compact_vector_knowledge_document_for_model(item: &Value) -> Value {
    json!({
        "doc_id": item.get("doc_id").cloned().unwrap_or(Value::Null),
        "document": item.get("document").cloned().unwrap_or(Value::Null),
        "chunk_index": item.get("chunk_index").cloned().unwrap_or(Value::Null),
        "start": item.get("start").cloned().unwrap_or(Value::Null),
        "end": item.get("end").cloned().unwrap_or(Value::Null),
        "content": item.get("content").cloned().unwrap_or(Value::Null),
        "score": item.get("score").cloned().unwrap_or(Value::Null),
        "keyword": item.get("keyword").cloned().unwrap_or(Value::Null),
    })
}

fn build_knowledge_tool_success(
    base_name: &str,
    query: Option<&str>,
    queries: &[String],
    embedding_model: Option<&str>,
    vector: bool,
    documents: Vec<Value>,
    grouped_queries: Option<Vec<Value>>,
) -> Value {
    let compact_documents = documents
        .iter()
        .map(|item| {
            if vector {
                compact_vector_knowledge_document_for_model(item)
            } else {
                compact_knowledge_document_for_model(item)
            }
        })
        .collect::<Vec<_>>();
    let count = compact_documents.len();
    let mut data = json!({
        "knowledge_base": base_name,
        "vector": vector,
        "count": count,
        "documents": compact_documents,
    });
    if let Some(map) = data.as_object_mut() {
        if let Some(query) = query.map(str::trim).filter(|value| !value.is_empty()) {
            map.insert("query".to_string(), Value::String(query.to_string()));
        }
        if queries.len() > 1 {
            map.insert("keywords".to_string(), json!(queries));
        }
        if let Some(embedding_model) = embedding_model
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            map.insert(
                "embedding_model".to_string(),
                Value::String(embedding_model.to_string()),
            );
        }
        if let Some(grouped_queries) = grouped_queries {
            let compact_queries = grouped_queries
                .into_iter()
                .map(|entry| {
                    json!({
                        "keyword": entry.get("keyword").cloned().unwrap_or(Value::Null),
                        "documents": entry
                            .get("documents")
                            .and_then(Value::as_array)
                            .map(|items| {
                                items.iter()
                                    .map(compact_vector_knowledge_document_for_model)
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default(),
                    })
                })
                .collect::<Vec<_>>();
            map.insert("queries".to_string(), json!(compact_queries));
        }
    }
    build_model_tool_success_with_hint(
        "knowledge",
        "completed",
        format!("Retrieved {count} knowledge snippets from {base_name}."),
        data,
        (count == 0).then(|| {
            "No matching knowledge snippets were found. Refine the query or try narrower keywords."
                .to_string()
        }),
    )
}

fn extract_keywords(args: &Value) -> Vec<String> {
    let Some(Value::Array(items)) = args.get("keywords") else {
        return Vec::new();
    };
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for item in items {
        let Some(text) = item.as_str() else {
            continue;
        };
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            output.push(trimmed.to_string());
        }
    }
    output
}

fn resolve_query_text(args: &Value) -> Option<String> {
    if let Some(text) = args.get("query").and_then(Value::as_str) {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    let keywords = extract_keywords(args);
    if keywords.is_empty() {
        None
    } else {
        Some(keywords.join(" "))
    }
}

fn extract_limit(args: &Value) -> Option<usize> {
    let value = args.get("limit")?;
    if let Some(num) = value.as_u64() {
        return Some(num as usize);
    }
    if let Some(num) = value.as_i64() {
        if num > 0 {
            return Some(num as usize);
        }
    }
    if let Some(num) = value.as_f64() {
        if num > 0.0 {
            return Some(num as usize);
        }
    }
    if let Some(text) = value.as_str() {
        if let Ok(num) = text.trim().parse::<usize>() {
            if num > 0 {
                return Some(num);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{
        compact_knowledge_document_for_model, compact_vector_knowledge_document_for_model,
    };
    use serde_json::json;

    #[test]
    fn compact_knowledge_document_for_model_keeps_only_model_relevant_fields() {
        let value = compact_knowledge_document_for_model(&json!({
            "code": "sec-1",
            "document": "design.md",
            "name": "Overview",
            "section_path": ["Overview"],
            "content": "Important details",
            "score": 0.92,
            "reason": "semantic_match",
        }));

        assert_eq!(
            value,
            json!({
                "document": "design.md",
                "name": "Overview",
                "section_path": ["Overview"],
                "content": "Important details",
                "score": 0.92,
                "reason": "semantic_match",
            })
        );
    }

    #[test]
    fn compact_vector_knowledge_document_for_model_drops_embedding_noise() {
        let value = compact_vector_knowledge_document_for_model(&json!({
            "doc_id": "doc-1",
            "document": "guide.md",
            "name": "guide.md",
            "chunk_index": 3,
            "start": 120,
            "end": 240,
            "content": "Chunk content",
            "embedding_model": "bge-large",
            "score": 0.81,
            "keyword": "timeout",
        }));

        assert_eq!(
            value,
            json!({
                "doc_id": "doc-1",
                "document": "guide.md",
                "chunk_index": 3,
                "start": 120,
                "end": 240,
                "content": "Chunk content",
                "score": 0.81,
                "keyword": "timeout",
            })
        );
    }
}
