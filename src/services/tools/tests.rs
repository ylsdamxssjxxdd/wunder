use super::*;
use crate::a2a_store::A2aStore;
use crate::config::LlmModelConfig;
use crate::lsp::LspManager;
use crate::storage::{
    AgentThreadRecord, ChatSessionRecord, SqliteStorage, StorageBackend, UserAgentRecord,
};
use crate::workspace::WorkspaceManager;
#[cfg(windows)]
use encoding_rs::GBK;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::tempdir;

fn sample_chat_session_record(agent_id: &str) -> ChatSessionRecord {
    ChatSessionRecord {
        session_id: "sess_test".to_string(),
        user_id: "alice".to_string(),
        title: "test".to_string(),
        status: "active".to_string(),
        created_at: 1.0,
        updated_at: 1.0,
        last_message_at: 1.0,
        agent_id: Some(agent_id.to_string()),
        tool_overrides: Vec::new(),
        parent_session_id: None,
        parent_message_id: None,
        spawn_label: None,
        spawned_by: None,
    }
}

fn sample_agent_record() -> UserAgentRecord {
    UserAgentRecord {
        agent_id: "agent_policy_worker".to_string(),
        user_id: "alice".to_string(),
        hive_id: "hive_policy".to_string(),
        name: "worker_a".to_string(),
        description: String::new(),
        system_prompt: "use policy knowledge".to_string(),
        preview_skill: false,
        model_name: None,
        ability_items: Vec::new(),
        tool_names: vec!["skill_creator".to_string()],
        declared_tool_names: vec!["read_file".to_string()],
        declared_skill_names: vec!["sample_skill".to_string()],
        visible_unit_ids: Vec::new(),
        preset_questions: Vec::new(),
        access_level: "A".to_string(),
        approval_mode: "auto_edit".to_string(),
        is_shared: false,
        status: "active".to_string(),
        icon: None,
        sandbox_container_id: 1,
        created_at: 1.0,
        updated_at: 1.0,
        preset_binding: None,
        silent: false,
        prefer_mother: false,
    }
}

fn sample_llm_model_config(model: &str) -> LlmModelConfig {
    LlmModelConfig {
        enable: Some(true),
        provider: Some("openai".to_string()),
        api_mode: None,
        base_url: Some("http://127.0.0.1:18080/v1".to_string()),
        api_key: Some("test-key".to_string()),
        model: Some(model.to_string()),
        temperature: Some(0.0),
        timeout_s: Some(15),
        max_rounds: Some(4),
        max_context: Some(16_384),
        max_output: Some(256),
        thinking_token_budget: None,
        support_vision: Some(false),
        support_hearing: Some(false),
        stream: Some(false),
        stream_include_usage: Some(false),
        history_compaction_ratio: None,
        tool_call_mode: Some("tool_call".to_string()),
        reasoning_effort: None,
        model_type: Some("llm".to_string()),
        stop: None,
        mock_if_unconfigured: None,
        ..Default::default()
    }
}

fn sample_parent_agent_record() -> UserAgentRecord {
    UserAgentRecord {
        agent_id: "agent_parent".to_string(),
        user_id: "alice".to_string(),
        hive_id: "hive_policy".to_string(),
        name: "parent_agent".to_string(),
        description: String::new(),
        system_prompt: "coordinate workers".to_string(),
        preview_skill: false,
        model_name: None,
        ability_items: Vec::new(),
        tool_names: vec!["agent_swarm".to_string()],
        declared_tool_names: vec!["agent_swarm".to_string()],
        declared_skill_names: Vec::new(),
        visible_unit_ids: Vec::new(),
        preset_questions: Vec::new(),
        access_level: "A".to_string(),
        approval_mode: "auto_edit".to_string(),
        is_shared: false,
        status: "active".to_string(),
        icon: None,
        sandbox_container_id: 1,
        created_at: 1.0,
        updated_at: 1.0,
        preset_binding: None,
        silent: false,
        prefer_mother: false,
    }
}

#[test]
fn parse_list_files_pagination_defaults_to_500() {
    let pagination =
        file_tool::parse_list_files_pagination(&json!({})).expect("default pagination");
    assert_eq!(pagination.start, 0);
    assert_eq!(pagination.limit, DEFAULT_LIST_PAGE_LIMIT);
}

#[test]
fn parse_list_files_pagination_accepts_cursor_and_clamps_limit() {
    let pagination = file_tool::parse_list_files_pagination(&json!({
        "cursor": "12",
        "limit": 9999
    }))
    .expect("pagination should parse");
    assert_eq!(pagination.start, 12);
    assert_eq!(pagination.limit, MAX_LIST_ITEMS);
}

#[test]
fn parse_list_files_pagination_rejects_invalid_cursor() {
    let err = file_tool::parse_list_files_pagination(&json!({
        "cursor": "not-a-number"
    }))
    .expect_err("cursor should be validated");
    assert!(err.to_string().contains("cursor"));
}

#[tokio::test]
async fn write_file_uses_orchestration_run_root_for_round_short_paths() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("state.sqlite3");
    let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
    let workspace_root = dir.path().join("workspace");
    let workspace = Arc::new(WorkspaceManager::new(
        workspace_root.to_string_lossy().as_ref(),
        storage.clone(),
        0,
        &HashMap::new(),
    ));
    let run_root = workspace_root
        .join("workspace-test")
        .join("orchestration")
        .join("orch_demo");
    std::fs::create_dir_all(&run_root).expect("create run root");
    crate::services::orchestration_context::persist_session_context(
        storage.as_ref(),
        "alice",
        "sess_mother",
        &crate::services::orchestration_context::OrchestrationSessionContext {
            mode: crate::services::orchestration_context::ORCHESTRATION_MODE.to_string(),
            run_id: "orch_demo".to_string(),
            group_id: "hive_demo".to_string(),
            role: "mother".to_string(),
            round_index: 2,
            mother_agent_id: "agent_mother".to_string(),
        },
    )
    .expect("persist orchestration context");

    let config = Config::default();
    let a2a_store = A2aStore::default();
    let skills = SkillRegistry::default();
    let http = reqwest::Client::new();
    let lsp_manager = LspManager::new(workspace.clone());
    let context = ToolContext {
        user_id: "alice",
        session_id: "sess_mother",
        workspace_id: "workspace-test",
        agent_id: Some("agent_mother"),
        user_round: Some(2),
        model_round: Some(1),
        is_admin: false,
        storage: storage.clone(),
        orchestrator: None,
        monitor: None,
        beeroom_realtime: None,
        workspace: workspace.clone(),
        lsp_manager,
        config: &config,
        a2a_store: &a2a_store,
        skills: &skills,
        gateway: None,
        user_world: None,
        cron_wake_signal: None,
        user_tool_manager: None,
        user_tool_bindings: None,
        user_tool_store: None,
        request_config_overrides: None,
        allow_roots: None,
        read_roots: None,
        command_sessions: None,
        event_emitter: None,
        http: &http,
    };

    let result = file_tool::write_file(
        &context,
        &json!({
            "path": "round_02/worker/report.txt",
            "content": "artifact"
        }),
    )
    .await
    .expect("write file");

    assert_eq!(result["ok"], true);
    assert!(run_root.join("round_02/worker/report.txt").is_file());
    assert!(!workspace_root
        .join("workspace-test")
        .join("round_02/worker/report.txt")
        .exists());
}

#[test]
fn session_spawn_args_accept_message_alias() {
    let payload: SessionSpawnArgs = serde_json::from_value(json!({
        "message": "hello child"
    }))
    .expect("message alias should deserialize");
    assert_eq!(payload.task, "hello child");
}

#[test]
fn session_spawn_args_prefers_task_when_task_and_message_are_both_present() {
    let payload: SessionSpawnArgs = serde_json::from_value(json!({
        "task": "explicit task",
        "message": "legacy alias"
    }))
    .expect("task and message should deserialize together");
    assert_eq!(payload.task, "explicit task");
}

#[test]
fn session_spawn_args_accept_thread_strategy_aliases() {
    let camel: SessionSpawnArgs = serde_json::from_value(json!({
        "task": "hello child",
        "threadStrategy": "main_thread",
    }))
    .expect("camel thread strategy should deserialize");
    assert_eq!(camel.thread_strategy.as_deref(), Some("main_thread"));

    let snake: SessionSpawnArgs = serde_json::from_value(json!({
        "task": "hello child",
        "thread_strategy": "fresh_main_thread",
        "reuse_main_thread": true,
    }))
    .expect("snake thread strategy should deserialize");
    assert_eq!(snake.thread_strategy.as_deref(), Some("fresh_main_thread"));
    assert_eq!(snake.reuse_main_thread, Some(true));
}

#[test]
fn list_files_inner_supports_cursor_pagination() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("list-files-pagination.db");
    let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
    let workspace_root = dir.path().join("workspaces");
    let workspace = WorkspaceManager::new(
        workspace_root.to_string_lossy().as_ref(),
        storage,
        0,
        &HashMap::new(),
    );

    let user_root = workspace_root.join("admin");
    std::fs::create_dir_all(&user_root).expect("create user root");
    for idx in 0..5usize {
        std::fs::write(user_root.join(format!("f{idx}.txt")), "demo").expect("write file");
    }

    let page1 = file_tool::list_files_inner(&workspace, "admin", ".", &[], 1, 0, 2).expect("page1");
    assert_eq!(
        page1
            .pointer("/data/items")
            .and_then(Value::as_array)
            .map(|v| v.len()),
        Some(2)
    );
    assert_eq!(
        page1.pointer("/data/next_cursor").and_then(Value::as_str),
        Some("2")
    );
    assert_eq!(
        page1.pointer("/data/has_more").and_then(Value::as_bool),
        Some(true)
    );

    let page2 = file_tool::list_files_inner(&workspace, "admin", ".", &[], 1, 2, 2).expect("page2");
    assert_eq!(
        page2
            .pointer("/data/items")
            .and_then(Value::as_array)
            .map(|v| v.len()),
        Some(2)
    );
    assert_eq!(
        page2.pointer("/data/next_cursor").and_then(Value::as_str),
        Some("4")
    );
    assert_eq!(
        page2.pointer("/data/has_more").and_then(Value::as_bool),
        Some(true)
    );

    let page3 = file_tool::list_files_inner(&workspace, "admin", ".", &[], 1, 4, 2).expect("page3");
    assert_eq!(
        page3
            .pointer("/data/items")
            .and_then(Value::as_array)
            .map(|v| v.len()),
        Some(1)
    );
    assert_eq!(page3.pointer("/data/next_cursor"), Some(&Value::Null));
    assert_eq!(
        page3.pointer("/data/has_more").and_then(Value::as_bool),
        Some(false)
    );
}

#[test]
fn list_files_inner_reads_public_workspace_directory_outside_current_workspace_root() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("list-files-public.db");
    let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
    let workspace_root = dir.path().join("workspaces");
    let workspace = WorkspaceManager::new(
        workspace_root.to_string_lossy().as_ref(),
        storage,
        0,
        &HashMap::new(),
    );

    let target_dir = workspace_root
        .join("admin")
        .join("skills")
        .join("my-test-skill");
    std::fs::create_dir_all(target_dir.join("assets")).expect("mkdir");
    std::fs::write(target_dir.join("SKILL.md"), "# demo").expect("write skill");
    std::fs::write(target_dir.join("assets").join("example.txt"), "hello").expect("write asset");

    let value = file_tool::list_files_inner(
        &workspace,
        "admin__c__1",
        "/workspaces/admin/skills/my-test-skill",
        &[PathBuf::from("/")],
        2,
        0,
        20,
    )
    .expect("list files result");

    assert_eq!(value.get("ok").and_then(Value::as_bool), Some(true));
    let items = value
        .pointer("/data/items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(items.iter().any(|item| item.as_str() == Some("SKILL.md")));
    assert!(items.iter().any(|item| item.as_str() == Some("assets/")));
}

#[test]
fn parse_read_file_specs_accepts_shorthand_path_payload() {
    let specs = file_tool::parse_read_file_specs(&json!({
        "path": "Cargo.toml",
        "start_line": 2,
        "end_line": 5,
    }))
    .expect("shorthand path should parse");

    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].path, "Cargo.toml");
    assert_eq!(specs[0].ranges, vec![(2, 5)]);
}

#[test]
fn parse_read_file_specs_accepts_file_path_alias() {
    let specs = file_tool::parse_read_file_specs(&json!({
        "file_path": "README.md",
    }))
    .expect("file_path alias should parse");

    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].path, "README.md");
    assert_eq!(specs[0].ranges, vec![(1, MAX_READ_LINES)]);
}

#[test]
fn parse_read_file_specs_accepts_offset_and_limit_aliases() {
    let specs = file_tool::parse_read_file_specs(&json!({
        "file_path": "README.md",
        "offset": 15,
        "limit": 20,
    }))
    .expect("offset/limit alias should parse");

    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].path, "README.md");
    assert_eq!(specs[0].ranges, vec![(15, 34)]);
}

#[test]
fn parse_read_file_specs_treats_start_line_without_end_line_as_window() {
    let specs = file_tool::parse_read_file_specs(&json!({
        "path": "README.md",
        "start_line": 18,
    }))
    .expect("start_line window payload should parse");

    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].path, "README.md");
    assert_eq!(
        specs[0].ranges,
        vec![(18, 18 + DEFAULT_START_LINE_WINDOW - 1)]
    );
}

#[test]
fn parse_read_file_specs_clamps_explicit_range_to_max_span() {
    let specs = file_tool::parse_read_file_specs(&json!({
        "path": "README.md",
        "start_line": 10,
        "end_line": 10000,
    }))
    .expect("explicit range should parse");

    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].ranges, vec![(10, 10 + MAX_RANGE_SPAN - 1)]);
}

#[test]
fn parse_read_file_specs_rejects_descending_ranges() {
    let err = file_tool::parse_read_file_specs(&json!({
        "path": "README.md",
        "start_line": 80,
        "end_line": 12,
    }))
    .expect_err("descending ranges should fail");

    assert_eq!(err.code, "TOOL_READ_INVALID_RANGE");
    assert!(err.message.contains("80"));
    assert!(err.message.contains("12"));
}

#[test]
fn parse_read_file_specs_rejects_more_than_max_budget_files() {
    let files = (0..=MAX_READ_BUDGET_FILES)
        .map(|idx| {
            json!({
                "path": format!("docs/{idx}.md"),
            })
        })
        .collect::<Vec<_>>();
    let err = file_tool::parse_read_file_specs(&json!({
        "files": files,
    }))
    .expect_err("oversized files payload should fail");

    assert_eq!(err.code, "TOOL_READ_TOO_MANY_FILES");
    assert_eq!(err.data.get("count").and_then(Value::as_u64), Some(21));
    assert_eq!(
        err.data.get("max_files").and_then(Value::as_u64),
        Some(MAX_READ_BUDGET_FILES as u64)
    );
}

#[test]
fn parse_read_file_specs_normalizes_zero_start_line_to_first_line() {
    let specs = file_tool::parse_read_file_specs(&json!({
        "path": "README.md",
        "start_line": 0,
        "end_line": 12,
    }))
    .expect("zero-based start should parse");

    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].requested_ranges, vec![(0, 12)]);
    assert_eq!(specs[0].ranges, vec![(1, 12)]);
}

#[test]
fn parse_read_file_specs_coalesces_adjacent_slice_specs_for_same_file() {
    let specs = file_tool::parse_read_file_specs(&json!({
        "files": [
            {
                "path": "README.md",
                "start_line": 0,
                "end_line": 100
            },
            {
                "path": "README.md",
                "start_line": 100,
                "end_line": 200
            },
            {
                "path": "README.md",
                "start_line": 200,
                "end_line": 300
            }
        ]
    }))
    .expect("adjacent slice specs should parse");

    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].ranges, vec![(1, 300)]);
}

#[test]
fn normalize_read_path_for_workspace_strips_matching_workspace_id() {
    let normalized = file_tool::normalize_read_path_for_workspace(
        "/workspaces/admin/agents/demo.worker-card.json",
        "admin",
    );
    assert_eq!(normalized, "agents/demo.worker-card.json");
}

#[test]
fn normalize_read_path_for_workspace_keeps_mismatched_workspace_id() {
    let normalized =
        file_tool::normalize_read_path_for_workspace("/workspaces/another_owner/demo.txt", "admin");
    assert_eq!(normalized, "/workspaces/another_owner/demo.txt");
}

#[test]
fn normalize_read_path_for_workspace_accepts_legacy_workspace_prefix() {
    let normalized =
        file_tool::normalize_read_path_for_workspace("/workspaces/Cargo.toml", "admin");
    assert_eq!(normalized, "Cargo.toml");
}

#[test]
fn parse_read_file_specs_parses_indentation_mode() {
    let specs = file_tool::parse_read_file_specs(&json!({
        "path": "src/main.rs",
        "mode": "indentation",
        "indentation": {
            "anchor_line": 12,
            "max_levels": 2,
            "include_siblings": true,
            "include_header": false,
            "max_lines": 40
        }
    }))
    .expect("indentation mode should parse");

    assert_eq!(specs.len(), 1);
    assert!(matches!(
        specs[0].mode,
        file_tool::ReadFileMode::Indentation
    ));
    assert_eq!(specs[0].indentation.anchor_line, Some(12));
    assert_eq!(specs[0].indentation.max_levels, 2);
    assert!(specs[0].indentation.include_siblings);
    assert!(!specs[0].indentation.include_header);
    assert_eq!(specs[0].indentation.max_lines, Some(40));
}

#[test]
fn parse_read_budget_reads_nested_and_top_level_fields() {
    let budget = file_tool::parse_read_budget(&json!({
        "time_budget_ms": 9000,
        "budget": {
            "output_budget_bytes": 4096,
            "max_files": 3
        }
    }));
    assert_eq!(budget.time_budget_ms, Some(9000));
    assert_eq!(budget.output_budget_bytes, Some(4096));
    assert_eq!(budget.max_files, Some(3));
}

#[test]
fn read_files_inner_returns_failed_result_when_all_files_missing() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("read-files-missing.db");
    let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
    let workspace_root = dir.path().join("workspaces");
    let workspace = WorkspaceManager::new(
        workspace_root.to_string_lossy().as_ref(),
        storage,
        0,
        &HashMap::new(),
    );

    let value = file_tool::read_files_inner(
        &workspace,
        "admin",
        &[],
        vec![file_tool::ReadFileSpec {
            path: "missing.txt".to_string(),
            requested_ranges: vec![(1, 20)],
            ranges: vec![(1, 20)],
            used_default_range: false,
            mode: file_tool::ReadFileMode::Slice,
            indentation: read_indentation::IndentationReadOptions::default(),
        }],
        file_tool::ReadBudget::default(),
        false,
        1,
        false,
    )
    .expect("read files result");

    assert_eq!(value.get("ok").and_then(Value::as_bool), Some(false));
    assert_eq!(
        value.pointer("/error_meta/code").and_then(Value::as_str),
        Some("TOOL_READ_NOT_FOUND")
    );
    assert_eq!(
        value.pointer("/data/path").and_then(Value::as_str),
        Some("missing.txt")
    );
    assert_eq!(
        value.pointer("/data/reason").and_then(Value::as_str),
        Some("not_found")
    );
    assert!(value.pointer("/data/content").is_none());
}

#[test]
fn read_files_inner_returns_compact_binary_failure() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("read-files-binary.db");
    let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
    let workspace_root = dir.path().join("workspaces");
    let workspace = WorkspaceManager::new(
        workspace_root.to_string_lossy().as_ref(),
        storage,
        0,
        &HashMap::new(),
    );

    let user_root = workspace_root.join("admin");
    std::fs::create_dir_all(&user_root).expect("create user root");
    let file_path = user_root.join("heart.png");
    std::fs::write(&file_path, b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR").expect("write png");

    let value = file_tool::read_files_inner(
        &workspace,
        "admin",
        &[],
        vec![file_tool::ReadFileSpec {
            path: "heart.png".to_string(),
            requested_ranges: vec![(1, 20)],
            ranges: vec![(1, 20)],
            used_default_range: false,
            mode: file_tool::ReadFileMode::Slice,
            indentation: read_indentation::IndentationReadOptions::default(),
        }],
        file_tool::ReadBudget::default(),
        false,
        1,
        false,
    )
    .expect("read files result");

    assert_eq!(value.get("ok").and_then(Value::as_bool), Some(false));
    assert_eq!(
        value.pointer("/error_meta/code").and_then(Value::as_str),
        Some("TOOL_READ_BINARY_FILE")
    );
    assert_eq!(
        value.pointer("/data/path").and_then(Value::as_str),
        Some("heart.png")
    );
    assert_eq!(
        value.pointer("/data/kind").and_then(Value::as_str),
        Some("image")
    );
    assert_eq!(
        value.pointer("/data/mime_type").and_then(Value::as_str),
        Some("image/png")
    );
    assert_eq!(
        value
            .pointer("/data/suggested_tool")
            .and_then(Value::as_str),
        Some(read_image_tool::TOOL_READ_IMAGE)
    );
    assert!(value.pointer("/data/content").is_none());
}

#[test]
fn read_files_inner_returns_truncated_excerpt_for_large_text_file() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("read-files-large.db");
    let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
    let workspace_root = dir.path().join("workspaces");
    let workspace = WorkspaceManager::new(
        workspace_root.to_string_lossy().as_ref(),
        storage,
        0,
        &HashMap::new(),
    );

    let user_root = workspace_root.join("admin");
    std::fs::create_dir_all(&user_root).expect("create user root");
    let file_path = user_root.join("large.md");
    let mut content = String::new();
    for idx in 1..=60_000usize {
        content.push_str(&format!("line {idx:05} {}\n", "x".repeat(24)));
    }
    std::fs::write(&file_path, content).expect("write large file");

    let value = file_tool::read_files_inner(
        &workspace,
        "admin",
        &[],
        vec![file_tool::ReadFileSpec {
            path: "large.md".to_string(),
            requested_ranges: vec![(1, 5)],
            ranges: vec![(1, 5)],
            used_default_range: false,
            mode: file_tool::ReadFileMode::Slice,
            indentation: read_indentation::IndentationReadOptions::default(),
        }],
        file_tool::ReadBudget::default(),
        false,
        1,
        false,
    )
    .expect("read files result");

    assert_ne!(value.get("ok").and_then(Value::as_bool), Some(false));
    assert_eq!(
        value
            .pointer("/data/files/0/truncated_by_size")
            .and_then(Value::as_bool),
        Some(true)
    );
    let body = value
        .pointer("/data/content")
        .and_then(Value::as_str)
        .expect("content should exist");
    assert!(body.contains("line 00001"));
    assert!(body.contains(">>> large.md"));
    assert_eq!(
        value
            .pointer("/data/patch_usage_hint")
            .and_then(Value::as_str),
        Some(i18n::t("tool.read.patch_usage_hint").as_str())
    );
}

#[test]
fn read_files_inner_prefers_workspace_file_for_relative_path_with_extra_root() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("read-files-relative.db");
    let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
    let workspace_root = dir.path().join("workspaces");
    let workspace = WorkspaceManager::new(
        workspace_root.to_string_lossy().as_ref(),
        storage,
        0,
        &HashMap::new(),
    );

    let user_root = workspace_root.join("admin");
    std::fs::create_dir_all(&user_root).expect("create user root");
    let workspace_file = user_root.join("note.txt");
    std::fs::write(&workspace_file, "workspace only\n").expect("write workspace file");
    let extra_root = dir.path().join("extra");
    std::fs::create_dir_all(&extra_root).expect("create extra root");
    std::fs::write(extra_root.join("note.txt"), "extra root\n").expect("write extra file");

    let value = file_tool::read_files_inner(
        &workspace,
        "admin",
        &[extra_root.clone()],
        vec![file_tool::ReadFileSpec {
            path: "note.txt".to_string(),
            requested_ranges: vec![(1, 20)],
            ranges: vec![(1, 20)],
            used_default_range: false,
            mode: file_tool::ReadFileMode::Slice,
            indentation: read_indentation::IndentationReadOptions::default(),
        }],
        file_tool::ReadBudget::default(),
        false,
        1,
        false,
    )
    .expect("read files result");

    assert_eq!(value.get("ok").and_then(Value::as_bool), Some(true));
    let body = value
        .pointer("/data/content")
        .and_then(Value::as_str)
        .expect("content should exist");
    assert!(body.contains("workspace only"));
    assert!(!body.contains("extra root"));
}

#[test]
fn read_files_inner_marks_default_full_window_as_continuable() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("read-files-default-window.db");
    let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
    let workspace_root = dir.path().join("workspaces");
    let workspace = WorkspaceManager::new(
        workspace_root.to_string_lossy().as_ref(),
        storage,
        0,
        &HashMap::new(),
    );

    let user_root = workspace_root.join("admin");
    std::fs::create_dir_all(&user_root).expect("create user root");
    let file_path = user_root.join("treaty.md");
    let mut content = String::new();
    for idx in 1..=2_500usize {
        content.push_str(&format!("line {idx:05}\n"));
    }
    std::fs::write(&file_path, content).expect("write treaty file");

    let value = file_tool::read_files_inner(
        &workspace,
        "admin",
        &[],
        vec![file_tool::ReadFileSpec {
            path: "treaty.md".to_string(),
            requested_ranges: vec![(1, MAX_READ_LINES)],
            ranges: vec![(1, MAX_READ_LINES)],
            used_default_range: true,
            mode: file_tool::ReadFileMode::Slice,
            indentation: read_indentation::IndentationReadOptions::default(),
        }],
        file_tool::ReadBudget::default(),
        false,
        1,
        false,
    )
    .expect("read files result");

    assert_eq!(
        value
            .pointer("/data/continuation_required")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        value
            .pointer("/data/files/0/request_satisfied")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        value
            .pointer("/data/files/0/used_default_range")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        value
            .pointer("/data/patch_usage_hint")
            .and_then(Value::as_str),
        Some(i18n::t("tool.read.patch_usage_hint").as_str())
    );
}

#[test]
fn compact_command_result_for_model_flattens_output_guard_fields() {
    let value = command_tool::compact_command_result_for_model(&json!({
        "command": "rg hello src",
        "command_index": 1,
        "command_session_id": "cmdsess_1",
        "returncode": 0,
        "stdout": "hello",
        "stderr": "",
        "output_meta": {
            "truncated": true,
            "total_bytes": 8192,
            "omitted_bytes": 2048
        },
        "raw_bytes": 12345
    }));

    assert_eq!(
        value,
        json!({
            "command": "rg hello src",
            "command_index": 1,
            "command_session_id": "cmdsess_1",
            "returncode": 0,
            "stdout": "hello",
            "stderr": "",
            "truncated": true,
            "total_bytes": 8192,
            "omitted_bytes": 2048
        })
    );
}

#[test]
fn summarize_slice_eof_marks_eof_ranges() {
    let (hit_eof, range_reaches_eof) = file_tool::summarize_slice_eof(&[(100, 200)], 178);
    assert!(hit_eof);
    assert!(range_reaches_eof);

    let (hit_eof, range_reaches_eof) = file_tool::summarize_slice_eof(&[(200, 300)], 178);
    assert!(hit_eof);
    assert!(!range_reaches_eof);

    let (hit_eof, range_reaches_eof) = file_tool::summarize_slice_eof(&[(1, 50)], 178);
    assert!(!hit_eof);
    assert!(!range_reaches_eof);
}

#[test]
fn truncate_utf8_output_respects_char_boundary() {
    let text = "a中b";
    let (truncated, omitted) = file_tool::truncate_utf8_output(text, 2);
    assert!(omitted > 0);
    assert!(truncated.contains("truncated read output"));
}

#[test]
fn extract_direct_patch_from_command_accepts_raw_patch_payload() {
    let command = r#"
*** Begin Patch
*** Update File: src/main.rs
@@
-fn old() {}
+fn new() {}
*** End Patch
"#;
    let extracted = command_tool::extract_direct_patch_from_command(command);
    assert!(extracted.is_some());
    let patch = extracted.expect("patch should be extracted");
    assert!(patch.starts_with("*** Begin Patch"));
    assert!(patch.ends_with("*** End Patch"));
}

#[test]
fn extract_direct_patch_from_command_rejects_wrapped_shell_text() {
    let command = r#"cat <<'PATCH'
*** Begin Patch
*** Update File: src/main.rs
@@
-fn old() {}
+fn new() {}
*** End Patch
PATCH"#;
    assert!(command_tool::extract_direct_patch_from_command(command).is_none());
}

#[test]
fn builtin_tool_specs_excludes_replace_text() {
    let specs = builtin_tool_specs_with_language("zh-CN");
    assert!(specs.iter().all(|spec| spec.name != "替换文本"));
    assert!(specs.iter().any(|spec| spec.name == "应用补丁"));
}

#[test]
fn builtin_aliases_excludes_replace_text() {
    let aliases = builtin_aliases();
    assert!(!aliases.contains_key("replace_text"));
    assert_eq!(
        aliases.get("apply_patch").map(String::as_str),
        Some("应用补丁")
    );
    assert_eq!(
        aliases
            .get(read_image_tool::TOOL_VIEW_IMAGE_ALIAS)
            .map(String::as_str),
        Some(read_image_tool::TOOL_READ_IMAGE)
    );
    assert_eq!(
        aliases
            .get(sleep_tool::TOOL_SLEEP_ALIAS)
            .map(String::as_str),
        Some(sleep_tool::TOOL_SLEEP_WAIT)
    );
}

#[test]
fn load_agent_record_accepts_default_agent_alias() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("tools-default-agent.db");
    let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());

    let record = load_agent_record(&storage, "alice", Some("__default__"), false)
        .expect("load default agent")
        .expect("default agent record");

    assert_eq!(record.agent_id, "__default__");
    assert_eq!(record.user_id, "alice");
}

#[test]
fn resolve_session_tool_overrides_prefers_declared_agent_defaults() {
    let session = sample_chat_session_record("agent_policy_worker");
    let agent = sample_agent_record();

    let overrides = resolve_session_tool_overrides(&session, None, Some(&agent));

    assert_eq!(
        overrides,
        vec!["read_file".to_string(), "sample_skill".to_string()]
    );
}

#[test]
fn resolve_child_session_tool_names_uses_target_agent_defaults_for_swarm_children() {
    let parent_tool_names = vec!["skill_creator".to_string()];
    let agent = sample_agent_record();

    let inherited = resolve_child_session_tool_names(
        ChildSessionToolMode::InheritParentSession,
        &parent_tool_names,
        Some(&agent),
    );
    let swarm_defaults = resolve_child_session_tool_names(
        ChildSessionToolMode::UseTargetAgentDefaults,
        &parent_tool_names,
        Some(&agent),
    );

    assert_eq!(inherited, vec!["skill_creator".to_string()]);
    assert_eq!(
        swarm_defaults,
        vec!["read_file".to_string(), "sample_skill".to_string()]
    );
}

#[tokio::test]
async fn prepare_swarm_child_session_creates_fresh_main_thread_even_when_worker_has_existing_main_session(
) {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("swarm-fresh-main-thread.db");
    let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
    storage.ensure_initialized().expect("init storage");
    let storage_backend: Arc<dyn StorageBackend> = storage.clone();

    let parent_agent = sample_parent_agent_record();
    let worker_agent = sample_agent_record();
    storage_backend
        .upsert_user_agent(&parent_agent)
        .expect("upsert parent agent");
    storage_backend
        .upsert_user_agent(&worker_agent)
        .expect("upsert worker agent");

    let mut parent_session = sample_chat_session_record(&parent_agent.agent_id);
    parent_session.session_id = "sess_parent".to_string();
    parent_session.tool_overrides = vec!["agent_swarm".to_string()];
    storage_backend
        .upsert_chat_session(&parent_session)
        .expect("upsert parent session");

    let mut old_worker_session = sample_chat_session_record(&worker_agent.agent_id);
    old_worker_session.session_id = "sess_worker_existing".to_string();
    storage_backend
        .upsert_chat_session(&old_worker_session)
        .expect("upsert existing worker session");
    storage_backend
        .upsert_agent_thread(&AgentThreadRecord {
            thread_id: "thread_existing_worker".to_string(),
            user_id: "alice".to_string(),
            agent_id: worker_agent.agent_id.clone(),
            session_id: old_worker_session.session_id.clone(),
            status: "idle".to_string(),
            created_at: 1.0,
            updated_at: 1.0,
        })
        .expect("bind existing worker main thread");

    let workspace_root = dir.path().join("workspace");
    let workspace = Arc::new(WorkspaceManager::new(
        workspace_root.to_string_lossy().as_ref(),
        storage_backend.clone(),
        0,
        &HashMap::new(),
    ));
    let lsp_manager = LspManager::new(workspace.clone());
    let config = Config::default();
    let a2a_store = A2aStore::default();
    let skills = SkillRegistry::default();
    let http = reqwest::Client::new();
    let context = ToolContext {
        user_id: "alice",
        session_id: "sess_parent",
        workspace_id: "workspace-test",
        agent_id: Some("agent_parent"),
        user_round: Some(1),
        model_round: Some(1),
        is_admin: false,
        storage: storage_backend.clone(),
        orchestrator: None,
        monitor: None,
        beeroom_realtime: None,
        workspace,
        lsp_manager,
        config: &config,
        a2a_store: &a2a_store,
        skills: &skills,
        gateway: None,
        user_world: None,
        cron_wake_signal: None,
        user_tool_manager: None,
        user_tool_bindings: None,
        user_tool_store: None,
        request_config_overrides: None,
        allow_roots: None,
        read_roots: None,
        command_sessions: None,
        event_emitter: None,
        http: &http,
    };

    let prepared = prepare_swarm_child_session(
        &context,
        "clean worker context",
        Some("worker_a".to_string()),
        &worker_agent.agent_id,
    )
    .expect("prepare fresh swarm child session");

    assert_ne!(prepared.child_session_id, old_worker_session.session_id);
    assert_eq!(
        storage_backend
            .get_agent_thread("alice", &worker_agent.agent_id)
            .expect("get worker thread")
            .expect("worker thread")
            .session_id,
        prepared.child_session_id
    );
    assert_eq!(
        storage_backend
            .get_chat_session("alice", &prepared.child_session_id)
            .expect("load child session")
            .expect("child session")
            .parent_session_id
            .as_deref(),
        Some("sess_parent")
    );
    assert_eq!(
        storage_backend
            .get_chat_session("alice", &prepared.child_session_id)
            .expect("reload child session")
            .expect("child session")
            .spawned_by
            .as_deref(),
        Some("agent_swarm")
    );
}

#[tokio::test]
async fn prepare_child_session_does_not_rebind_parent_agent_main_thread_for_subagent_children() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("subagent-keep-parent-main-thread.db");
    let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
    storage.ensure_initialized().expect("init storage");
    let storage_backend: Arc<dyn StorageBackend> = storage.clone();

    let parent_agent = sample_parent_agent_record();
    storage_backend
        .upsert_user_agent(&parent_agent)
        .expect("upsert parent agent");

    let mut parent_session = sample_chat_session_record(&parent_agent.agent_id);
    parent_session.session_id = "sess_parent".to_string();
    storage_backend
        .upsert_chat_session(&parent_session)
        .expect("upsert parent session");
    storage_backend
        .upsert_agent_thread(&AgentThreadRecord {
            thread_id: "thread_parent_main".to_string(),
            user_id: "alice".to_string(),
            agent_id: parent_agent.agent_id.clone(),
            session_id: parent_session.session_id.clone(),
            status: "idle".to_string(),
            created_at: 1.0,
            updated_at: 1.0,
        })
        .expect("bind parent main thread");

    let workspace_root = dir.path().join("workspace");
    let workspace = Arc::new(WorkspaceManager::new(
        workspace_root.to_string_lossy().as_ref(),
        storage_backend.clone(),
        0,
        &HashMap::new(),
    ));
    let lsp_manager = LspManager::new(workspace.clone());
    let config = Config::default();
    let a2a_store = A2aStore::default();
    let skills = SkillRegistry::default();
    let http = reqwest::Client::new();
    let context = ToolContext {
        user_id: "alice",
        session_id: "sess_parent",
        workspace_id: "workspace-test",
        agent_id: Some(parent_agent.agent_id.as_str()),
        user_round: Some(1),
        model_round: Some(1),
        is_admin: false,
        storage: storage_backend.clone(),
        orchestrator: None,
        monitor: None,
        beeroom_realtime: None,
        workspace,
        lsp_manager,
        config: &config,
        a2a_store: &a2a_store,
        skills: &skills,
        gateway: None,
        user_world: None,
        cron_wake_signal: None,
        user_tool_manager: None,
        user_tool_bindings: None,
        user_tool_store: None,
        request_config_overrides: None,
        allow_roots: None,
        read_roots: None,
        command_sessions: None,
        event_emitter: None,
        http: &http,
    };

    let prepared = prepare_child_session(
        &context,
        "sess_parent",
        "delegate subagent task",
        Some("temporary child".to_string()),
        None,
        None,
        ChildSessionToolMode::InheritParentSession,
    )
    .expect("prepare child session");

    assert_ne!(prepared.child_session_id, "sess_parent");
    assert_eq!(
        storage_backend
            .get_agent_thread("alice", &parent_agent.agent_id)
            .expect("get parent thread")
            .expect("parent thread")
            .session_id,
        "sess_parent"
    );
    assert_eq!(
        storage_backend
            .get_chat_session("alice", &prepared.child_session_id)
            .expect("load child session")
            .expect("child session")
            .parent_session_id
            .as_deref(),
        Some("sess_parent")
    );
    assert_eq!(
        storage_backend
            .get_chat_session("alice", &prepared.child_session_id)
            .expect("reload child session")
            .expect("child session")
            .spawned_by
            .as_deref(),
        Some("model")
    );
}

#[tokio::test]
async fn swarm_worker_subagent_child_does_not_steal_worker_main_thread() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir
        .path()
        .join("swarm-worker-subagent-keeps-worker-main-thread.db");
    let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
    storage.ensure_initialized().expect("init storage");
    let storage_backend: Arc<dyn StorageBackend> = storage.clone();

    let parent_agent = sample_parent_agent_record();
    let worker_agent = sample_agent_record();
    storage_backend
        .upsert_user_agent(&parent_agent)
        .expect("upsert parent agent");
    storage_backend
        .upsert_user_agent(&worker_agent)
        .expect("upsert worker agent");

    let mut mother_session = sample_chat_session_record(&parent_agent.agent_id);
    mother_session.session_id = "sess_mother".to_string();
    mother_session.tool_overrides = vec!["agent_swarm".to_string()];
    storage_backend
        .upsert_chat_session(&mother_session)
        .expect("upsert mother session");

    let workspace_root = dir.path().join("workspace");
    let workspace = Arc::new(WorkspaceManager::new(
        workspace_root.to_string_lossy().as_ref(),
        storage_backend.clone(),
        0,
        &HashMap::new(),
    ));
    let lsp_manager = LspManager::new(workspace.clone());
    let config = Config::default();
    let a2a_store = A2aStore::default();
    let skills = SkillRegistry::default();
    let http = reqwest::Client::new();
    let mother_context = ToolContext {
        user_id: "alice",
        session_id: "sess_mother",
        workspace_id: "workspace-test",
        agent_id: Some(parent_agent.agent_id.as_str()),
        user_round: Some(1),
        model_round: Some(1),
        is_admin: false,
        storage: storage_backend.clone(),
        orchestrator: None,
        monitor: None,
        beeroom_realtime: None,
        workspace: workspace.clone(),
        lsp_manager: lsp_manager.clone(),
        config: &config,
        a2a_store: &a2a_store,
        skills: &skills,
        gateway: None,
        user_world: None,
        cron_wake_signal: None,
        user_tool_manager: None,
        user_tool_bindings: None,
        user_tool_store: None,
        request_config_overrides: None,
        allow_roots: None,
        read_roots: None,
        command_sessions: None,
        event_emitter: None,
        http: &http,
    };

    let worker_prepared = prepare_swarm_child_session(
        &mother_context,
        "worker task",
        Some("worker".to_string()),
        &worker_agent.agent_id,
    )
    .expect("prepare worker session");

    assert_eq!(
        storage_backend
            .get_agent_thread("alice", &worker_agent.agent_id)
            .expect("get worker thread after swarm dispatch")
            .expect("worker thread after swarm dispatch")
            .session_id,
        worker_prepared.child_session_id
    );

    let worker_context = ToolContext {
        user_id: "alice",
        session_id: worker_prepared.child_session_id.as_str(),
        workspace_id: "workspace-test",
        agent_id: Some(worker_agent.agent_id.as_str()),
        user_round: Some(1),
        model_round: Some(1),
        is_admin: false,
        storage: storage_backend.clone(),
        orchestrator: None,
        monitor: None,
        beeroom_realtime: None,
        workspace,
        lsp_manager,
        config: &config,
        a2a_store: &a2a_store,
        skills: &skills,
        gateway: None,
        user_world: None,
        cron_wake_signal: None,
        user_tool_manager: None,
        user_tool_bindings: None,
        user_tool_store: None,
        request_config_overrides: None,
        allow_roots: None,
        read_roots: None,
        command_sessions: None,
        event_emitter: None,
        http: &http,
    };

    let subagent_prepared = prepare_child_session(
        &worker_context,
        &worker_prepared.child_session_id,
        "subagent task",
        Some("temporary worker child".to_string()),
        None,
        None,
        ChildSessionToolMode::InheritParentSession,
    )
    .expect("prepare subagent child session");

    assert_ne!(
        subagent_prepared.child_session_id,
        worker_prepared.child_session_id
    );
    assert_eq!(
        storage_backend
            .get_agent_thread("alice", &worker_agent.agent_id)
            .expect("get worker thread after subagent spawn")
            .expect("worker thread after subagent spawn")
            .session_id,
        worker_prepared.child_session_id
    );
    assert_eq!(
        storage_backend
            .get_chat_session("alice", &subagent_prepared.child_session_id)
            .expect("load subagent child session")
            .expect("subagent child session")
            .parent_session_id
            .as_deref(),
        Some(worker_prepared.child_session_id.as_str())
    );
}

#[test]
fn prepare_child_session_inherits_effective_model_from_parent_agent() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("subagent-inherit-model.db");
    let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
    storage.ensure_initialized().expect("init storage");
    let storage_backend: Arc<dyn StorageBackend> = storage.clone();

    let mut parent_agent = sample_parent_agent_record();
    parent_agent.model_name = Some("model-parent".to_string());
    storage_backend
        .upsert_user_agent(&parent_agent)
        .expect("upsert parent agent");

    let mut parent_session = sample_chat_session_record(&parent_agent.agent_id);
    parent_session.session_id = "sess_parent".to_string();
    storage_backend
        .upsert_chat_session(&parent_session)
        .expect("upsert parent session");

    let workspace_root = dir.path().join("workspace");
    let workspace = Arc::new(WorkspaceManager::new(
        workspace_root.to_string_lossy().as_ref(),
        storage_backend.clone(),
        0,
        &HashMap::new(),
    ));
    let lsp_manager = LspManager::new(workspace.clone());
    let mut config = Config::default();
    config.llm.default = "model-default".to_string();
    config.llm.models.insert(
        "model-default".to_string(),
        sample_llm_model_config("provider-default"),
    );
    config.llm.models.insert(
        "model-parent".to_string(),
        sample_llm_model_config("provider-parent"),
    );
    let a2a_store = A2aStore::default();
    let skills = SkillRegistry::default();
    let http = reqwest::Client::new();
    let context = ToolContext {
        user_id: "alice",
        session_id: "sess_parent",
        workspace_id: "workspace-test",
        agent_id: Some(parent_agent.agent_id.as_str()),
        user_round: Some(1),
        model_round: Some(1),
        is_admin: false,
        storage: storage_backend.clone(),
        orchestrator: None,
        monitor: None,
        beeroom_realtime: None,
        workspace,
        lsp_manager,
        config: &config,
        a2a_store: &a2a_store,
        skills: &skills,
        gateway: None,
        user_world: None,
        cron_wake_signal: None,
        user_tool_manager: None,
        user_tool_bindings: None,
        user_tool_store: None,
        request_config_overrides: None,
        allow_roots: None,
        read_roots: None,
        command_sessions: None,
        event_emitter: None,
        http: &http,
    };

    let prepared = prepare_child_session(
        &context,
        "sess_parent",
        "solve it",
        Some("worker".to_string()),
        None,
        None,
        ChildSessionToolMode::InheritParentSession,
    )
    .expect("prepare child session");

    assert_eq!(prepared.model_name.as_deref(), Some("model-parent"));
    assert_eq!(prepared.request.model_name.as_deref(), Some("model-parent"));
}

#[tokio::test]
async fn prepare_swarm_child_session_uses_target_agent_model_for_initial_run() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("swarm-target-model.db");
    let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
    storage.ensure_initialized().expect("init storage");
    let storage_backend: Arc<dyn StorageBackend> = storage.clone();

    let parent_agent = sample_parent_agent_record();
    let mut worker_agent = sample_agent_record();
    worker_agent.model_name = Some("model-worker".to_string());
    storage_backend
        .upsert_user_agent(&parent_agent)
        .expect("upsert parent agent");
    storage_backend
        .upsert_user_agent(&worker_agent)
        .expect("upsert worker agent");

    let mut parent_session = sample_chat_session_record(&parent_agent.agent_id);
    parent_session.session_id = "sess_parent".to_string();
    parent_session.tool_overrides = vec!["agent_swarm".to_string()];
    storage_backend
        .upsert_chat_session(&parent_session)
        .expect("upsert parent session");

    let workspace_root = dir.path().join("workspace");
    let workspace = Arc::new(WorkspaceManager::new(
        workspace_root.to_string_lossy().as_ref(),
        storage_backend.clone(),
        0,
        &HashMap::new(),
    ));
    let lsp_manager = LspManager::new(workspace.clone());
    let mut config = Config::default();
    config.llm.default = "model-default".to_string();
    config.llm.models.insert(
        "model-default".to_string(),
        sample_llm_model_config("provider-default"),
    );
    config.llm.models.insert(
        "model-worker".to_string(),
        sample_llm_model_config("provider-worker"),
    );
    let a2a_store = A2aStore::default();
    let skills = SkillRegistry::default();
    let http = reqwest::Client::new();
    let context = ToolContext {
        user_id: "alice",
        session_id: "sess_parent",
        workspace_id: "workspace-test",
        agent_id: Some(parent_agent.agent_id.as_str()),
        user_round: Some(1),
        model_round: Some(1),
        is_admin: false,
        storage: storage_backend.clone(),
        orchestrator: None,
        monitor: None,
        beeroom_realtime: None,
        workspace,
        lsp_manager,
        config: &config,
        a2a_store: &a2a_store,
        skills: &skills,
        gateway: None,
        user_world: None,
        cron_wake_signal: None,
        user_tool_manager: None,
        user_tool_bindings: None,
        user_tool_store: None,
        request_config_overrides: None,
        allow_roots: None,
        read_roots: None,
        command_sessions: None,
        event_emitter: None,
        http: &http,
    };

    let prepared = prepare_swarm_child_session(
        &context,
        "review this",
        Some("worker".to_string()),
        &worker_agent.agent_id,
    )
    .expect("prepare swarm child session");

    assert_eq!(prepared.model_name.as_deref(), Some("model-worker"));
    assert_eq!(prepared.request.model_name.as_deref(), Some("model-worker"));
}

#[test]
fn agent_swarm_batch_send_args_accept_team_run_id_aliases() {
    let camel: AgentSwarmBatchSendArgs = serde_json::from_value(json!({
        "tasks": [{ "agent_id": "worker_a", "message": "hello" }],
        "teamRunId": "team_demo_camel",
    }))
    .expect("parse camel args");
    assert_eq!(camel.team_run_id.as_deref(), Some("team_demo_camel"));

    let snake: AgentSwarmBatchSendArgs = serde_json::from_value(json!({
        "tasks": [{ "agent_id": "worker_a", "message": "hello" }],
        "team_run_id": "team_demo_snake",
    }))
    .expect("parse snake args");
    assert_eq!(snake.team_run_id.as_deref(), Some("team_demo_snake"));
}

#[test]
fn swarm_batch_helpers_ignore_artifact_session_id_and_infer_role_name() {
    assert_eq!(
        resolve_swarm_batch_session_key(Some("orchestration/artifact/round_01/".to_string()))
            .expect("resolve artifact path session key"),
        None
    );
    assert_eq!(
        resolve_swarm_batch_session_key(Some(
            "/workspaces/admin__c__1/orchestration/artifact".to_string()
        ))
        .expect("resolve public path session key"),
        None
    );
    assert_eq!(
        resolve_swarm_batch_session_key(Some("sess_worker".to_string()))
            .expect("resolve normal session key")
            .as_deref(),
        Some("sess_worker")
    );
    assert_eq!(
        infer_swarm_agent_name_from_task_message(
            "Task 1\nrole: worker_a\nComplete the assigned task."
        )
        .as_deref(),
        Some("worker_a")
    );
}

#[test]
fn agent_swarm_send_args_accept_thread_strategy_aliases() {
    let camel: AgentSwarmSendArgs = serde_json::from_value(json!({
        "agent_name": "worker_a",
        "message": "hello",
        "threadStrategy": "main_thread",
    }))
    .expect("parse camel send args");
    assert_eq!(camel.thread_strategy.as_deref(), Some("main_thread"));

    let snake: AgentSwarmSendArgs = serde_json::from_value(json!({
        "agent_name": "worker_a",
        "message": "hello",
        "thread_strategy": "fresh_main_thread",
        "reuse_main_thread": true,
    }))
    .expect("parse snake send args");
    assert_eq!(snake.thread_strategy.as_deref(), Some("fresh_main_thread"));
    assert_eq!(snake.reuse_main_thread, Some(true));
}

#[test]
fn agent_swarm_batch_send_args_accept_thread_strategy_aliases() {
    let camel: AgentSwarmBatchSendArgs = serde_json::from_value(json!({
        "tasks": [{ "agent_id": "worker_a", "message": "hello" }],
        "threadStrategy": "main_thread",
    }))
    .expect("parse camel batch args");
    assert_eq!(camel.thread_strategy.as_deref(), Some("main_thread"));

    let snake: AgentSwarmBatchSendArgs = serde_json::from_value(json!({
        "tasks": [{
            "agent_id": "worker_a",
            "message": "hello",
            "thread_strategy": "fresh_main_thread",
            "reuse_main_thread": true
        }],
        "reuse_main_thread": true,
    }))
    .expect("parse snake batch args");
    assert_eq!(snake.reuse_main_thread, Some(true));
    assert_eq!(
        snake.tasks[0].thread_strategy.as_deref(),
        Some("fresh_main_thread")
    );
    assert_eq!(snake.tasks[0].reuse_main_thread, Some(true));
}

#[test]
fn agent_swarm_send_args_accept_canonical_session_id() {
    let payload: AgentSwarmSendArgs = serde_json::from_value(json!({
        "session_id": "sess_worker_demo",
        "message": "hello",
    }))
    .expect("parse canonical send args");
    assert_eq!(payload.session_key.as_deref(), Some("sess_worker_demo"));
    assert_eq!(payload.message, "hello");
}

#[test]
fn agent_swarm_wait_args_accept_canonical_run_ids() {
    let payload: AgentSwarmWaitArgs = serde_json::from_value(json!({
        "run_ids": ["run_demo_1"],
        "wait_seconds": 3,
    }))
    .expect("parse canonical wait args");
    assert_eq!(payload.run_ids, Some(vec!["run_demo_1".to_string()]));
    assert_eq!(payload.wait_seconds, Some(3.0));
}

#[test]
fn tool_result_field_reads_nested_data_before_top_level() {
    let result = json!({
        "status": "top-level-status",
        "data": {
            "status": "accepted",
            "run_id": "run_worker_a",
            "agent_id": "worker_a",
            "agent_name": "Worker A",
            "session_id": "sess_worker_a",
            "created_session": true,
            "thread_strategy": "main_thread",
            "error": "nested error"
        }
    });

    assert_eq!(
        tool_result_field(&result, "status").and_then(Value::as_str),
        Some("accepted")
    );
    assert_eq!(
        tool_result_field(&result, "run_id").and_then(Value::as_str),
        Some("run_worker_a")
    );
    assert_eq!(
        tool_result_field(&result, "agent_id").and_then(Value::as_str),
        Some("worker_a")
    );
    assert_eq!(
        tool_result_field(&result, "session_id").and_then(Value::as_str),
        Some("sess_worker_a")
    );
    assert_eq!(
        tool_result_field(&result, "created_session").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        tool_result_field_or_null(&result, "thread_strategy").as_str(),
        Some("main_thread")
    );
    assert_eq!(tool_result_field_or_null(&result, "missing"), Value::Null);
    assert_eq!(
        tool_result_field(&result, "error").and_then(Value::as_str),
        Some("nested error")
    );
}

#[test]
fn skipped_swarm_task_result_exposes_skipped_reason_in_data() {
    let result = skipped_swarm_task_result(
        "batch_send",
        "task_a",
        "sess_a",
        "agent_a",
        "Worker A",
        "already_dispatched_this_round",
    );
    assert_eq!(
        tool_result_field(&result, "state").and_then(Value::as_str),
        Some("skipped")
    );
    assert_eq!(
        tool_result_field(&result, "task_id").and_then(Value::as_str),
        Some("task_a")
    );
    assert_eq!(
        tool_result_field(&result, "skip_reason").and_then(Value::as_str),
        Some("already_dispatched_this_round")
    );
    assert_eq!(tool_result_field(&result, "run_id"), Some(&Value::Null));
}

#[test]
fn batch_send_all_skipped_response_keeps_team_run_null() {
    let result = build_model_tool_success(
        "batch_send",
        "skipped",
        "All swarm tasks were already dispatched in this round.",
        json!({
            "items": [
                tool_result_data(&skipped_swarm_task_result(
                    "batch_send",
                    "task_a",
                    "sess_a",
                    "agent_a",
                    "Worker A",
                    "already_dispatched_this_round"
                )).clone()
            ],
            "task_total": 0,
            "task_success": 0,
            "task_failed": 0,
            "skip_reason": "already_dispatched_this_round",
            "team_run_id": Value::Null,
        }),
    );
    assert_eq!(
        tool_result_field(&result, "state").and_then(Value::as_str),
        Some("skipped")
    );
    assert_eq!(
        tool_result_field(&result, "team_run_id"),
        Some(&Value::Null)
    );
    assert_eq!(
        tool_result_field(&result, "skip_reason").and_then(Value::as_str),
        Some("already_dispatched_this_round")
    );
}

#[test]
fn parse_swarm_worker_thread_strategy_supports_main_thread_option() {
    assert_eq!(
        parse_swarm_worker_thread_strategy(None, None).expect("default strategy"),
        SwarmWorkerThreadStrategy::MainThread
    );
    assert_eq!(
        parse_swarm_worker_thread_strategy(Some("main_thread"), None)
            .expect("main_thread strategy"),
        SwarmWorkerThreadStrategy::MainThread
    );
    assert_eq!(
        parse_swarm_worker_thread_strategy(Some("fresh_main_thread"), None)
            .expect("fresh_main_thread strategy"),
        SwarmWorkerThreadStrategy::FreshMainThread
    );
    assert_eq!(
        parse_swarm_worker_thread_strategy(None, Some(true)).expect("reuseMainThread strategy"),
        SwarmWorkerThreadStrategy::MainThread
    );
}

#[test]
fn parse_swarm_worker_thread_strategy_rejects_unknown_value() {
    let err = parse_swarm_worker_thread_strategy(Some("reuse_previous"), None)
        .expect_err("unknown strategy should fail");
    assert!(err.to_string().contains("fresh_main_thread"));
    assert!(err.to_string().contains("main_thread"));
}

#[tokio::test]
async fn swarm_main_thread_strategy_reuses_existing_worker_main_thread() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("swarm-main-thread-reuse.db");
    let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
    storage.ensure_initialized().expect("init storage");
    let storage_backend: Arc<dyn StorageBackend> = storage.clone();

    let worker_agent = sample_agent_record();
    storage_backend
        .upsert_user_agent(&worker_agent)
        .expect("upsert worker agent");

    let mut worker_session = sample_chat_session_record(&worker_agent.agent_id);
    worker_session.session_id = "sess_worker_main".to_string();
    storage_backend
        .upsert_chat_session(&worker_session)
        .expect("upsert worker session");
    storage_backend
        .upsert_agent_thread(&AgentThreadRecord {
            thread_id: "thread_worker_main".to_string(),
            user_id: "alice".to_string(),
            agent_id: worker_agent.agent_id.clone(),
            session_id: worker_session.session_id.clone(),
            status: "idle".to_string(),
            created_at: 1.0,
            updated_at: 1.0,
        })
        .expect("bind worker main thread");

    let (resolved, created) =
        crate::services::swarm::beeroom::resolve_or_create_agent_main_session(
            storage_backend.as_ref(),
            "alice",
            &worker_agent,
        )
        .expect("resolve main session");

    assert!(!created);
    assert_eq!(resolved.session_id, worker_session.session_id);
    assert_eq!(
        storage_backend
            .get_agent_thread("alice", &worker_agent.agent_id)
            .expect("get agent thread")
            .expect("agent thread")
            .session_id,
        worker_session.session_id
    );
}

#[tokio::test]
async fn swarm_main_thread_strategy_creates_worker_main_thread_when_missing() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("swarm-main-thread-create.db");
    let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
    storage.ensure_initialized().expect("init storage");
    let storage_backend: Arc<dyn StorageBackend> = storage.clone();

    let worker_agent = sample_agent_record();
    storage_backend
        .upsert_user_agent(&worker_agent)
        .expect("upsert worker agent");

    let (resolved, created) =
        crate::services::swarm::beeroom::resolve_or_create_agent_main_session(
            storage_backend.as_ref(),
            "alice",
            &worker_agent,
        )
        .expect("create main session");

    assert!(created);
    assert_eq!(
        resolved.agent_id.as_deref(),
        Some(worker_agent.agent_id.as_str())
    );
    assert!(resolved.parent_session_id.is_none());
    assert!(resolved.spawned_by.is_none());
    assert_eq!(
        storage_backend
            .get_agent_thread("alice", &worker_agent.agent_id)
            .expect("get agent thread")
            .expect("agent thread")
            .session_id,
        resolved.session_id
    );
    assert_eq!(
        storage_backend
            .get_chat_session("alice", &resolved.session_id)
            .expect("load created session")
            .expect("created session")
            .session_id,
        resolved.session_id
    );
}

#[tokio::test]
async fn agent_swarm_batch_send_missing_message_returns_actionable_failure_and_skips_team_run() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("swarm-batch-send-validation.db");
    let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
    storage.ensure_initialized().expect("init storage");
    let storage_backend: Arc<dyn StorageBackend> = storage.clone();

    let parent_agent = sample_parent_agent_record();
    let worker_agent = sample_agent_record();
    storage_backend
        .upsert_user_agent(&parent_agent)
        .expect("upsert parent agent");
    storage_backend
        .upsert_user_agent(&worker_agent)
        .expect("upsert worker agent");

    let mut parent_session = sample_chat_session_record(&parent_agent.agent_id);
    parent_session.session_id = "sess_parent".to_string();
    parent_session.tool_overrides = vec!["agent_swarm".to_string()];
    storage_backend
        .upsert_chat_session(&parent_session)
        .expect("upsert parent session");

    let workspace_root = dir.path().join("workspace");
    let workspace = Arc::new(WorkspaceManager::new(
        workspace_root.to_string_lossy().as_ref(),
        storage_backend.clone(),
        0,
        &HashMap::new(),
    ));
    let lsp_manager = LspManager::new(workspace.clone());
    let config = Config::default();
    let a2a_store = A2aStore::default();
    let skills = SkillRegistry::default();
    let http = reqwest::Client::new();
    let context = ToolContext {
        user_id: "alice",
        session_id: "sess_parent",
        workspace_id: "workspace-test",
        agent_id: Some("agent_parent"),
        user_round: Some(1),
        model_round: Some(1),
        is_admin: false,
        storage: storage_backend.clone(),
        orchestrator: None,
        monitor: None,
        beeroom_realtime: None,
        workspace,
        lsp_manager,
        config: &config,
        a2a_store: &a2a_store,
        skills: &skills,
        gateway: None,
        user_world: None,
        cron_wake_signal: None,
        user_tool_manager: None,
        user_tool_bindings: None,
        user_tool_store: None,
        request_config_overrides: None,
        allow_roots: None,
        read_roots: None,
        command_sessions: None,
        event_emitter: None,
        http: &http,
    };

    let result = agent_swarm_batch_send(
        &context,
        &json!({
            "action": "batch_send",
            "tasks": [
                { "agent_name": "worker_a" }
            ]
        }),
    )
    .await
    .expect("batch send result");

    assert_eq!(result.get("ok").and_then(Value::as_bool), Some(false));
    assert_eq!(
        result.pointer("/error_meta/code").and_then(Value::as_str),
        Some("TOOL_ARGS_MISSING_FIELD")
    );
    assert_eq!(
        result.pointer("/data/task_index").and_then(Value::as_u64),
        Some(0)
    );
    assert_eq!(
        result
            .pointer("/data/example/action")
            .and_then(Value::as_str),
        Some("batch_send")
    );
    let (runs, total) = storage_backend
        .list_team_runs("alice", Some("hive_policy"), Some("sess_parent"), 0, 20)
        .expect("list team runs");
    assert_eq!(total, 0);
    assert!(runs.is_empty());
}

#[tokio::test]
async fn agent_swarm_batch_send_ignores_artifact_path_session_id_and_infers_agent_name() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("swarm-batch-send-artifact-session.db");
    let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
    storage.ensure_initialized().expect("init storage");
    let storage_backend: Arc<dyn StorageBackend> = storage.clone();

    let parent_agent = sample_parent_agent_record();
    let mut worker_agent = sample_agent_record();
    worker_agent.name = "worker_a".to_string();
    storage_backend
        .upsert_user_agent(&parent_agent)
        .expect("upsert parent agent");
    storage_backend
        .upsert_user_agent(&worker_agent)
        .expect("upsert worker agent");

    let mut parent_session = sample_chat_session_record(&parent_agent.agent_id);
    parent_session.session_id = "sess_parent".to_string();
    parent_session.tool_overrides = vec!["agent_swarm".to_string()];
    storage_backend
        .upsert_chat_session(&parent_session)
        .expect("upsert parent session");

    let workspace_root = dir.path().join("workspace");
    let workspace = Arc::new(WorkspaceManager::new(
        workspace_root.to_string_lossy().as_ref(),
        storage_backend.clone(),
        0,
        &HashMap::new(),
    ));
    let lsp_manager = LspManager::new(workspace.clone());
    let config = Config::default();
    let a2a_store = A2aStore::default();
    let skills = SkillRegistry::default();
    let http = reqwest::Client::new();
    let context = ToolContext {
        user_id: "alice",
        session_id: "sess_parent",
        workspace_id: "workspace-test",
        agent_id: Some("agent_parent"),
        user_round: Some(1),
        model_round: Some(1),
        is_admin: false,
        storage: storage_backend.clone(),
        orchestrator: None,
        monitor: None,
        beeroom_realtime: None,
        workspace,
        lsp_manager,
        config: &config,
        a2a_store: &a2a_store,
        skills: &skills,
        gateway: None,
        user_world: None,
        cron_wake_signal: None,
        user_tool_manager: None,
        user_tool_bindings: None,
        user_tool_store: None,
        request_config_overrides: None,
        allow_roots: None,
        read_roots: None,
        command_sessions: None,
        event_emitter: None,
        http: &http,
    };

    let result = agent_swarm_batch_send(
        &context,
        &json!({
            "action": "batch_send",
            "tasks": [
                {
                    "session_id": "orchestration/artifact/round_01/",
                    "message": "Task 1\nrole: worker_a\nComplete the assigned task."
                }
            ]
        }),
    )
    .await
    .expect("batch send result");

    assert_eq!(result.get("ok").and_then(Value::as_bool), Some(true));
    assert_eq!(
        result.pointer("/data/counts/total").and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        result
            .pointer("/data/items/0/agent_name")
            .and_then(Value::as_str),
        Some("worker_a")
    );
    assert_ne!(
        result
            .pointer("/data/items/0/session_id")
            .and_then(Value::as_str),
        Some("orchestration/artifact/round_01/")
    );
}

#[tokio::test]
async fn agent_swarm_batch_send_ignores_other_hive_agents_during_prefetch() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("swarm-batch-send-cross-hive-prefetch.db");
    let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
    storage.ensure_initialized().expect("init storage");
    let storage_backend: Arc<dyn StorageBackend> = storage.clone();

    let parent_agent = sample_parent_agent_record();
    let worker_agent = sample_agent_record();
    let mut other_hive_agent = sample_agent_record();
    other_hive_agent.agent_id = "agent_other_hive".to_string();
    other_hive_agent.hive_id = "hive_other".to_string();
    other_hive_agent.name = "other hive worker".to_string();
    storage_backend
        .upsert_user_agent(&parent_agent)
        .expect("upsert parent agent");
    storage_backend
        .upsert_user_agent(&worker_agent)
        .expect("upsert worker agent");
    storage_backend
        .upsert_user_agent(&other_hive_agent)
        .expect("upsert other hive agent");

    let mut parent_session = sample_chat_session_record(&parent_agent.agent_id);
    parent_session.session_id = "sess_parent".to_string();
    parent_session.tool_overrides = vec!["agent_swarm".to_string()];
    storage_backend
        .upsert_chat_session(&parent_session)
        .expect("upsert parent session");

    let mut worker_session = sample_chat_session_record(&worker_agent.agent_id);
    worker_session.session_id = "sess_worker".to_string();
    storage_backend
        .upsert_chat_session(&worker_session)
        .expect("upsert worker session");

    let workspace_root = dir.path().join("workspace");
    let workspace = Arc::new(WorkspaceManager::new(
        workspace_root.to_string_lossy().as_ref(),
        storage_backend.clone(),
        0,
        &HashMap::new(),
    ));
    let lsp_manager = LspManager::new(workspace.clone());
    let config = Config::default();
    let a2a_store = A2aStore::default();
    let skills = SkillRegistry::default();
    let http = reqwest::Client::new();
    let context = ToolContext {
        user_id: "alice",
        session_id: "sess_parent",
        workspace_id: "workspace-test",
        agent_id: Some("agent_parent"),
        user_round: Some(1),
        model_round: Some(1),
        is_admin: false,
        storage: storage_backend.clone(),
        orchestrator: None,
        monitor: None,
        beeroom_realtime: None,
        workspace,
        lsp_manager,
        config: &config,
        a2a_store: &a2a_store,
        skills: &skills,
        gateway: None,
        user_world: None,
        cron_wake_signal: None,
        user_tool_manager: None,
        user_tool_bindings: None,
        user_tool_store: None,
        request_config_overrides: None,
        allow_roots: None,
        read_roots: None,
        command_sessions: None,
        event_emitter: None,
        http: &http,
    };

    let err = agent_swarm_batch_send(
        &context,
        &json!({
            "action": "batch_send",
            "tasks": [{
                "session_id": "sess_worker",
                "agent_name": "wrong worker name",
                "message": "review this",
                "wait_seconds": 0
            }]
        }),
    )
    .await
    .expect_err("cross-hive prefetch should not fail before target validation");

    assert!(
        err.to_string()
            .contains("agent_swarm send agent_name does not match target session"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn agent_swarm_send_missing_target_returns_actionable_failure() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("swarm-send-validation.db");
    let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
    storage.ensure_initialized().expect("init storage");
    let storage_backend: Arc<dyn StorageBackend> = storage.clone();

    let parent_agent = sample_parent_agent_record();
    storage_backend
        .upsert_user_agent(&parent_agent)
        .expect("upsert parent agent");

    let mut parent_session = sample_chat_session_record(&parent_agent.agent_id);
    parent_session.session_id = "sess_parent".to_string();
    parent_session.tool_overrides = vec!["agent_swarm".to_string()];
    storage_backend
        .upsert_chat_session(&parent_session)
        .expect("upsert parent session");

    let workspace_root = dir.path().join("workspace");
    let workspace = Arc::new(WorkspaceManager::new(
        workspace_root.to_string_lossy().as_ref(),
        storage_backend.clone(),
        0,
        &HashMap::new(),
    ));
    let lsp_manager = LspManager::new(workspace.clone());
    let config = Config::default();
    let a2a_store = A2aStore::default();
    let skills = SkillRegistry::default();
    let http = reqwest::Client::new();
    let context = ToolContext {
        user_id: "alice",
        session_id: "sess_parent",
        workspace_id: "workspace-test",
        agent_id: Some("agent_parent"),
        user_round: Some(1),
        model_round: Some(1),
        is_admin: false,
        storage: storage_backend,
        orchestrator: None,
        monitor: None,
        beeroom_realtime: None,
        workspace,
        lsp_manager,
        config: &config,
        a2a_store: &a2a_store,
        skills: &skills,
        gateway: None,
        user_world: None,
        cron_wake_signal: None,
        user_tool_manager: None,
        user_tool_bindings: None,
        user_tool_store: None,
        request_config_overrides: None,
        allow_roots: None,
        read_roots: None,
        command_sessions: None,
        event_emitter: None,
        http: &http,
    };

    let result = agent_swarm_send(
        &context,
        &json!({
            "action": "send",
            "message": "Summarize the requested material."
        }),
    )
    .await
    .expect("send result");

    assert_eq!(result.get("ok").and_then(Value::as_bool), Some(false));
    assert_eq!(
        result.pointer("/error_meta/code").and_then(Value::as_str),
        Some("TOOL_ARGS_MISSING_FIELD")
    );
    assert_eq!(
        result
            .pointer("/data/example/agent_name")
            .and_then(Value::as_str),
        Some("worker_a")
    );
}

#[test]
fn resolve_swarm_wait_mode_defaults_to_infinite_when_config_is_zero() {
    assert!(matches!(
        resolve_swarm_wait_mode(None, 0),
        SwarmWaitMode::Infinite
    ));
    assert!(matches!(
        resolve_swarm_wait_mode(Some(0.0), 0),
        SwarmWaitMode::Immediate
    ));
    assert!(matches!(
        resolve_swarm_wait_mode(Some(12.0), 0),
        SwarmWaitMode::Finite(timeout) if (timeout - 12.0).abs() < f64::EPSILON
    ));
    assert!(matches!(
        resolve_swarm_wait_mode(None, 30),
        SwarmWaitMode::Finite(timeout) if (timeout - 30.0).abs() < f64::EPSILON
    ));
}

#[test]
fn background_child_runs_enable_parent_auto_wake() {
    assert!(should_auto_wake_parent_after_child_run(false, 0.0));
    assert!(!should_auto_wake_parent_after_child_run(false, 5.0));
    assert!(!should_auto_wake_parent_after_child_run(true, 0.0));
    assert!(should_auto_wake_parent_follow_up(false, false, 0.0));
    assert!(!should_auto_wake_parent_follow_up(true, false, 0.0));
}

#[test]
fn enrich_agent_swarm_spawn_response_preserves_spawn_contract() {
    let response = enrich_agent_swarm_spawn_response(json!({
        "ok": true,
        "action": "send",
        "state": "accepted",
        "summary": "Worker task was queued and is still running.",
        "data": {
            "run_id": "run_swarm_demo",
            "session_id": "sess_worker_demo",
            "team_run_id": "team_demo",
            "task_id": "task_demo"
        }
    }));

    assert_eq!(
        response.get("action").and_then(Value::as_str),
        Some("spawn")
    );
    assert_eq!(
        response.pointer("/data/session_id").and_then(Value::as_str),
        Some("sess_worker_demo")
    );
    assert_eq!(
        response
            .pointer("/data/child_session_id")
            .and_then(Value::as_str),
        Some("sess_worker_demo")
    );
    assert_eq!(
        response.pointer("/data/run_id").and_then(Value::as_str),
        Some("run_swarm_demo")
    );
    assert_eq!(
        response.pointer("/data/spawned").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        response.get("state").and_then(Value::as_str),
        Some("accepted")
    );
}

#[test]
fn sync_announce_auto_wake_updates_run_metadata() {
    let mut announce = AnnounceConfig {
        parent_session_id: "sess_parent".to_string(),
        label: None,
        dispatch_id: None,
        strategy: None,
        completion_mode: None,
        remaining_action: None,
        parent_turn_ref: None,
        parent_user_round: None,
        parent_model_round: None,
        emit_parent_events: true,
        auto_wake: false,
        persist_history_message: false,
    };
    let mut run_metadata = json!({});

    sync_announce_auto_wake(&mut announce, Some(&mut run_metadata), true);

    assert!(announce.auto_wake);
    assert_eq!(
        run_metadata.get("auto_wake").and_then(Value::as_bool),
        Some(true)
    );
}

#[test]
fn build_parent_follow_up_announce_keeps_turn_context_for_auto_wake() {
    let announce = build_parent_follow_up_announce(
        Some("sess_parent".to_string()),
        "sess_child",
        Some("worker".to_string()),
        true,
        false,
        true,
        Some("subagent_turn:3:2".to_string()),
        Some(3),
        Some(2),
    )
    .expect("announce");

    assert_eq!(announce.parent_session_id, "sess_parent");
    assert_eq!(
        announce.parent_turn_ref.as_deref(),
        Some("subagent_turn:3:2")
    );
    assert_eq!(announce.parent_user_round, Some(3));
    assert_eq!(announce.parent_model_round, Some(2));
    assert!(announce.auto_wake);
    assert!(!announce.persist_history_message);
}

#[test]
fn build_parent_follow_up_announce_rejects_same_session() {
    assert!(build_parent_follow_up_announce(
        Some("sess_same".to_string()),
        "sess_same",
        None,
        true,
        false,
        true,
        None,
        None,
        None,
    )
    .is_none());
}

#[test]
fn filter_tool_names_by_model_capability_blocks_read_image_when_vision_disabled() {
    let names = HashSet::from([
        read_image_tool::TOOL_READ_IMAGE.to_string(),
        read_image_tool::TOOL_READ_IMAGE_ALIAS.to_string(),
        "read_file".to_string(),
    ]);
    let filtered = filter_tool_names_by_model_capability(names, false);
    assert!(!filtered.contains(read_image_tool::TOOL_READ_IMAGE));
    assert!(!filtered.contains(read_image_tool::TOOL_READ_IMAGE_ALIAS));
    assert!(filtered.contains("read_file"));
}

#[test]
fn normalize_ptc_script_name_accepts_simple_filename() {
    let script =
        command_tool::normalize_ptc_script_name("demo").expect("filename should be normalized");
    assert_eq!(script, PathBuf::from("demo.py"));
}

#[test]
fn normalize_ptc_script_name_rejects_path_segments() {
    let error = command_tool::normalize_ptc_script_name("nested/demo.py")
        .expect_err("path must be rejected");
    assert_eq!(error, "tool.ptc.filename_invalid");
}

#[test]
fn normalize_ptc_script_name_rejects_non_python_extension() {
    let error = command_tool::normalize_ptc_script_name("demo.txt")
        .expect_err("non-python ext should fail");
    assert_eq!(error, "tool.ptc.ext_invalid");
}

#[cfg(windows)]
#[test]
fn decode_command_output_prefers_gbk_when_utf8_lossy_contains_replacements() {
    let expected = "\u{65e0}\u{6cd5}\u{5c06} pip \u{8bc6}\u{522b}\u{4e3a} cmdlet";
    let (encoded, _, _) = GBK.encode(expected);
    let decoded = command_tool::decode_command_output(encoded.as_ref());
    assert!(decoded.contains("\u{65e0}\u{6cd5}\u{5c06}"));
    assert!(decoded.contains("cmdlet"));
}

#[cfg(windows)]
#[test]
fn decode_command_output_handles_utf16_le_streams() {
    let expected = "PowerShell output";
    let utf16_bytes = expected
        .encode_utf16()
        .flat_map(|unit| unit.to_le_bytes())
        .collect::<Vec<_>>();
    let decoded = command_tool::decode_command_output(&utf16_bytes);
    assert_eq!(decoded, expected);
}
