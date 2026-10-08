use super::*;

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(super) struct UserKnowledgeBasePayload {
    pub(super) name: String,
    #[serde(default)]
    pub(super) description: String,
    #[serde(default)]
    pub(super) root: String,
    #[serde(default = "default_true")]
    pub(super) enabled: bool,
    #[serde(default)]
    pub(super) shared: bool,
    #[serde(default)]
    pub(super) base_type: Option<String>,
    #[serde(default)]
    pub(super) embedding_model: Option<String>,
    #[serde(default)]
    pub(super) ragflow_dataset_id: Option<String>,
    #[serde(default)]
    pub(super) ragflow_dataset_managed: Option<bool>,
    #[serde(default)]
    pub(super) chunk_method: Option<String>,
    #[serde(default)]
    pub(super) chunk_delimiter: Option<String>,
    #[serde(default)]
    pub(super) layout_recognize: Option<String>,
    #[serde(default)]
    pub(super) auto_keywords: Option<usize>,
    #[serde(default)]
    pub(super) auto_questions: Option<usize>,
    #[serde(default)]
    pub(super) html4excel: Option<bool>,
    #[serde(default)]
    pub(super) chunk_size: Option<usize>,
    #[serde(default)]
    pub(super) chunk_overlap: Option<usize>,
    #[serde(default)]
    pub(super) top_k: Option<usize>,
    #[serde(default)]
    pub(super) score_threshold: Option<f32>,
}

impl UserKnowledgeBasePayload {
    pub(super) fn from_with_root(base: &UserKnowledgeBase, root: String) -> Self {
        let normalized_type = normalize_knowledge_base_type(base.base_type.as_deref());
        let base_type = match normalized_type {
            KnowledgeBaseType::Vector => Some("vector".to_string()),
            KnowledgeBaseType::Ragflow => Some("ragflow".to_string()),
            KnowledgeBaseType::Literal => Some("literal".to_string()),
        };
        Self {
            name: base.name.clone(),
            description: base.description.clone(),
            root,
            enabled: base.enabled,
            shared: base.shared,
            base_type,
            embedding_model: base.embedding_model.clone(),
            ragflow_dataset_id: base.ragflow_dataset_id.clone(),
            ragflow_dataset_managed: base
                .ragflow_dataset_managed
                .or_else(|| (normalized_type == KnowledgeBaseType::Ragflow).then_some(true)),
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
}

impl From<UserKnowledgeBasePayload> for UserKnowledgeBase {
    fn from(payload: UserKnowledgeBasePayload) -> Self {
        let base_type = normalize_knowledge_base_type(payload.base_type.as_deref());
        let base_type_value = match base_type {
            KnowledgeBaseType::Vector => Some("vector".to_string()),
            KnowledgeBaseType::Ragflow => Some("ragflow".to_string()),
            KnowledgeBaseType::Literal => None,
        };
        Self {
            name: payload.name,
            description: payload.description,
            enabled: payload.enabled,
            shared: payload.shared,
            base_type: base_type_value,
            embedding_model: if base_type == KnowledgeBaseType::Vector {
                payload
                    .embedding_model
                    .as_deref()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
            } else {
                None
            },
            ragflow_dataset_id: if base_type == KnowledgeBaseType::Ragflow {
                ragflow_knowledge::normalize_dataset_id(payload.ragflow_dataset_id.as_deref())
                    .or_else(|| {
                        ragflow_knowledge::normalize_synthetic_root_dataset_id(&payload.root)
                    })
            } else {
                None
            },
            ragflow_dataset_managed: if base_type == KnowledgeBaseType::Ragflow {
                payload.ragflow_dataset_managed
            } else {
                None
            },
            chunk_method: if base_type == KnowledgeBaseType::Ragflow {
                ragflow_knowledge::normalize_chunk_method(payload.chunk_method.as_deref())
            } else {
                None
            },
            chunk_delimiter: if base_type == KnowledgeBaseType::Ragflow {
                ragflow_knowledge::normalize_chunk_delimiter(payload.chunk_delimiter.as_deref())
            } else {
                None
            },
            layout_recognize: if base_type == KnowledgeBaseType::Ragflow {
                ragflow_knowledge::normalize_layout_recognize(payload.layout_recognize.as_deref())
            } else {
                None
            },
            auto_keywords: if base_type == KnowledgeBaseType::Ragflow {
                payload.auto_keywords.map(|value| value.min(32))
            } else {
                None
            },
            auto_questions: if base_type == KnowledgeBaseType::Ragflow {
                payload.auto_questions.map(|value| value.min(10))
            } else {
                None
            },
            html4excel: if base_type == KnowledgeBaseType::Ragflow {
                payload.html4excel
            } else {
                None
            },
            chunk_size: payload.chunk_size,
            chunk_overlap: payload.chunk_overlap,
            top_k: payload.top_k,
            score_threshold: payload.score_threshold,
        }
    }
}
