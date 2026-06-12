use super::*;

#[test]
fn user_knowledge_payload_preserves_ragflow_dataset_id() {
    let payload = UserKnowledgeBasePayload {
        name: "base".to_string(),
        root: "config/knowledge/base".to_string(),
        base_type: Some("ragflow".to_string()),
        embedding_model: Some("ignored".to_string()),
        ragflow_dataset_id: Some("dataset_id".to_string()),
        ragflow_dataset_managed: Some(false),
        chunk_method: Some("Q&A".to_string()),
        chunk_delimiter: Some("\\n##".to_string()),
        layout_recognize: Some("plain text".to_string()),
        auto_keywords: Some(99),
        auto_questions: Some(42),
        html4excel: Some(true),
        ..Default::default()
    };

    let base = crate::services::user_tools::UserKnowledgeBase::from(payload);

    assert_eq!(base.base_type, Some("ragflow".to_string()));
    assert_eq!(base.embedding_model, None);
    assert_eq!(base.ragflow_dataset_id, Some("dataset_id".to_string()));
    assert_eq!(base.ragflow_dataset_managed, Some(false));
    assert_eq!(base.chunk_method, Some("qa".to_string()));
    assert_eq!(base.chunk_delimiter, Some("\n##".to_string()));
    assert_eq!(base.layout_recognize, Some("Plain Text".to_string()));
    assert_eq!(base.auto_keywords, Some(32));
    assert_eq!(base.auto_questions, Some(10));
    assert_eq!(base.html4excel, Some(true));

    let roundtrip =
        UserKnowledgeBasePayload::from_with_root(&base, "ragflow:dataset_id".to_string());
    assert_eq!(roundtrip.base_type, Some("ragflow".to_string()));
    assert_eq!(roundtrip.ragflow_dataset_id, Some("dataset_id".to_string()));
    assert_eq!(roundtrip.ragflow_dataset_managed, Some(false));
    assert_eq!(roundtrip.chunk_method, Some("qa".to_string()));
    assert_eq!(roundtrip.chunk_delimiter, Some("\n##".to_string()));
    assert_eq!(roundtrip.layout_recognize, Some("Plain Text".to_string()));
    assert_eq!(roundtrip.auto_keywords, Some(32));
    assert_eq!(roundtrip.auto_questions, Some(10));
    assert_eq!(roundtrip.html4excel, Some(true));
}

#[test]
fn user_knowledge_payload_does_not_use_plain_root_as_ragflow_dataset_id() {
    let payload = UserKnowledgeBasePayload {
        name: "base".to_string(),
        root: "config/knowledge/base".to_string(),
        base_type: Some("ragflow".to_string()),
        ..Default::default()
    };

    let base = crate::services::user_tools::UserKnowledgeBase::from(payload);

    assert_eq!(base.base_type, Some("ragflow".to_string()));
    assert_eq!(base.ragflow_dataset_id, None);
}

#[test]
fn user_ragflow_dataset_name_uses_username_prefix() {
    let user = UserAccountRecord {
        user_id: "user-id".to_string(),
        username: "alice".to_string(),
        email: None,
        password_hash: String::new(),
        roles: Vec::new(),
        status: "active".to_string(),
        access_level: "user".to_string(),
        unit_id: None,
        token_balance: 0,
        token_granted_total: 0,
        token_used_total: 0,
        last_token_grant_date: None,
        experience_total: 0,
        is_demo: false,
        created_at: 0.0,
        updated_at: 0.0,
        last_login_at: None,
    };

    assert_eq!(
        build_user_ragflow_dataset_name(&user, " product docs "),
        "[alice] product docs"
    );
}

#[test]
fn removed_ragflow_dataset_ids_detects_deleted_bases() {
    let current = vec![
        UserKnowledgeBase {
            name: "keep".to_string(),
            base_type: Some("ragflow".to_string()),
            ragflow_dataset_id: Some("dataset_keep".to_string()),
            ragflow_dataset_managed: Some(true),
            ..Default::default()
        },
        UserKnowledgeBase {
            name: "remove".to_string(),
            base_type: Some("ragflow".to_string()),
            ragflow_dataset_id: Some("dataset_remove".to_string()),
            ragflow_dataset_managed: Some(true),
            ..Default::default()
        },
        UserKnowledgeBase {
            name: "external".to_string(),
            base_type: Some("ragflow".to_string()),
            ragflow_dataset_id: Some("dataset_external".to_string()),
            ragflow_dataset_managed: Some(false),
            ..Default::default()
        },
    ];
    let next = vec![UserKnowledgeBase {
        name: "keep".to_string(),
        base_type: Some("ragflow".to_string()),
        ragflow_dataset_id: Some("dataset_keep".to_string()),
        ..Default::default()
    }];

    assert_eq!(
        collect_removed_ragflow_dataset_ids(&current, &next),
        vec!["dataset_remove".to_string()]
    );
}
