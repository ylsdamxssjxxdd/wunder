//! Worker-card export/import contract tests. Export must round-trip through
//! import field-for-field and stay secret-free; import must refuse malformed
//! documents and honour overwrite semantics.

use std::path::Path;
use wunder_desktop::{args::DesktopArgs, NativeDesktop};

fn start_isolated() -> Result<NativeDesktop, Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let config = directory.path().join("runtime/config");
    std::fs::create_dir_all(&config)?;
    std::fs::write(config.join("wunder.yaml"), "{}\n")?;
    let settings = serde_json::json!({
        "workspace_root": "", "desktop_token": "", "updated_at": 0,
        "lan_mesh": {"enabled": false},
        "llm": {"default": "test-model", "models": {
            "test-model": {"provider": "openai", "model": "test-model", "model_type": "llm",
                           "base_url": "http://127.0.0.1:9/v1", "timeout_s": 5,
                           "support_vision": false}}}
    });
    std::fs::write(
        config.join("desktop.settings.json"),
        serde_json::to_vec(&settings)?,
    )?;
    let mut args = DesktopArgs::native_defaults();
    args.temp_root = Some(directory.path().join("runtime").canonicalize()?);
    args.workspace = Some(directory.path().join("workspace"));
    std::mem::forget(directory);
    Ok(NativeDesktop::start_with_args(args)?)
}

#[test]
fn worker_card_export_import_roundtrip() {
    let runtime = start_isolated().expect("isolated runtime");
    // Use a real tool name from the resolved catalog; fixture names that do
    // not exist would be filtered out by the allowed-tools rule.
    let catalog_tool = runtime
        .list_tools()
        .expect("tools")
        .into_iter()
        .find(|tool| tool.category == "内置工具")
        .map(|tool| tool.name)
        .expect("builtin tool available");
    let created = runtime.create_agent("roundtrip-agent").expect("create");
    let updated = runtime
        .update_agent_settings(
            &created.id,
            wunder_desktop::AgentSettingsEdit {
                name: "roundtrip-agent".into(),
                description: "往返测试智能体".into(),
                system_prompt: "你是往返测试助手。".into(),
                model_name: "test-model".into(),
                icon_name: "robot".into(),
                icon_color: "#3b82f6".into(),
                tool_names: vec![catalog_tool.clone()],
                preset_questions: vec!["帮我总结今天的进展".into()],
                sandbox_container_id: 1,
                approval_mode: "suggest".into(),
                preview_skill: true,
                silent: false,
                prefer_mother: false,
            },
        )
        .expect("update settings");

    let document = runtime.export_agent_document(&updated.id).expect("export");
    assert_eq!(document["kind"], "WorkerCard");
    assert_eq!(document["schema_version"], "wunder/worker-card@2");
    assert_eq!(document["metadata"]["name"], "roundtrip-agent");
    assert_eq!(document["metadata"]["description"], "往返测试智能体");
    assert_eq!(document["extra_prompt"], "你是往返测试助手。");
    assert_eq!(document["runtime"]["model_name"], "test-model");
    assert_eq!(document["interaction"]["preset_questions"][0], "帮我总结今天的进展");
    // The document carries no secrets: the model config key is not an API key
    // and no token/authorization field may appear anywhere.
    let serialized = serde_json::to_string(&document).expect("serialize");
    assert!(!serialized.to_lowercase().contains("api_key"));
    assert!(!serialized.to_lowercase().contains("token"));

    // Export to file, delete the agent, then import from the file.
    let directory = tempfile::tempdir().expect("tempdir");
    let path = runtime
        .export_agent_to_file(&updated.id, directory.path())
        .expect("export file");
    assert!(path.is_file());
    runtime.delete_agent(&updated.id).expect("delete");
    assert!(runtime
        .list_agents()
        .expect("list")
        .iter()
        .all(|agent| agent.id != updated.id));

    let outcomes = runtime.import_agent_from_file(&path, false).expect("import");
    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].created);
    let imported = &outcomes[0].agent;
    assert_eq!(imported.name, "roundtrip-agent");
    assert_eq!(imported.description, "往返测试智能体");
    assert_eq!(imported.system_prompt, "你是往返测试助手。");
    assert_eq!(imported.model, "test-model");
    assert_eq!(imported.icon_name, "robot");
    assert_eq!(imported.icon_color, "#3b82f6");
    assert!(imported.tool_names.contains(&catalog_tool));
    assert_eq!(imported.preset_questions, vec!["帮我总结今天的进展".to_string()]);
    assert_eq!(imported.approval_mode, "suggest");
    assert!(imported.preview_skill);

    // Import again without overwrite creates a second agent; with overwrite
    // it updates the newest same-named agent in place instead of adding one.
    let duplicate = runtime
        .import_agent_document(&document, false)
        .expect("import duplicate");
    assert!(duplicate.created);
    let overwritten = runtime
        .import_agent_document(&document, true)
        .expect("import overwrite");
    assert!(!overwritten.created);
    assert_eq!(overwritten.agent.name, "roundtrip-agent");
    assert_eq!(
        runtime
            .list_agents()
            .expect("list")
            .iter()
            .filter(|agent| agent.name == "roundtrip-agent")
            .count(),
        2
    );
}

#[test]
fn worker_card_import_rejects_malformed_documents() {
    let runtime = start_isolated().expect("isolated runtime");
    let missing_name = serde_json::json!({
        "schema_version": "wunder/worker-card@2",
        "kind": "WorkerCard",
        "metadata": { "agent_id": "x", "name": "" }
    });
    assert!(runtime.import_agent_document(&missing_name, false).is_err());
    let bad_version = serde_json::json!({
        "schema_version": "wunder/worker-card@99",
        "kind": "WorkerCard",
        "metadata": { "name": "x" }
    });
    assert!(runtime.import_agent_document(&bad_version, false).is_err());
    let not_object = serde_json::json!("worker-card");
    assert!(runtime.import_agent_document(&not_object, false).is_err());
    let empty_file = std::path::Path::new("definitely-missing-card.json");
    assert!(runtime.import_agent_from_file(empty_file, false).is_err());
}

#[test]
fn worker_card_import_reports_missing_dependencies() {
    let runtime = start_isolated().expect("isolated runtime");
    let document = serde_json::json!({
        "schema_version": "wunder/worker-card@2",
        "kind": "WorkerCard",
        "metadata": { "agent_id": "", "name": "依赖缺失卡", "description": "" },
        "abilities": {
            "tool_names": ["绝不存在的工具"],
            "skills": ["绝不存在的技能"]
        },
        "interaction": { "preset_questions": [] },
        "runtime": { "model_name": "", "approval_mode": "suggest" }
    });
    let outcome = runtime.import_agent_document(&document, false).expect("import");
    assert!(outcome.created);
    assert_eq!(outcome.missing_tools, vec!["绝不存在的工具".to_string()]);
    assert_eq!(outcome.missing_skills, vec!["绝不存在的技能".to_string()]);
    runtime.delete_agent(&outcome.agent.id).expect("cleanup");
}

#[test]
fn worker_card_export_writes_files_importable_by_listing() {
    let runtime = start_isolated().expect("isolated runtime");
    let created = runtime.create_agent("listing-card").expect("create");
    let directory = tempfile::tempdir().expect("tempdir");
    let path = runtime
        .export_agent_to_file(&created.id, directory.path())
        .expect("export");
    let listed = runtime
        .list_agent_card_files(directory.path())
        .expect("list files");
    assert!(listed.iter().any(|(file, label)| file == &path.display().to_string()
        && label.contains("listing-card")));
}
