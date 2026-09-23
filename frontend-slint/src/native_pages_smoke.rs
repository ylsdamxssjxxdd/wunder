//! Isolated persistence and permission regression for native page operations.
use std::path::Path;
use wunder_desktop::{ModelEdit, NativeDesktop};

pub fn check_runtime(
    runtime: &NativeDesktop,
    output: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let settings = runtime.get_desktop_settings()?;
    let default = settings
        .models
        .iter()
        .find(|m| m.is_default && m.model_type == "llm")
        .ok_or("default model missing")?;
    let agents = runtime.list_agents()?;
    assert!(agents.iter().any(|a| a.id == "__default__"));
    assert!(!runtime.list_tools()?.is_empty());
    assert!(runtime
        .update_agent("missing-agent", "test-agent", "", "", "", "spark", "#94a3b8")
        .is_err());
    assert!(runtime.create_agent("\n").is_err());
    let created = runtime.create_agent("test-agent")?;
    let updated = runtime.update_agent(
        &created.id,
        "test-agent-updated",
        "test-description",
        "test-prompt",
        &default.key,
        "robot",
        "#3b82f6",
    )?;
    let reloaded = runtime
        .list_agents()?
        .into_iter()
        .find(|a| a.id == created.id)
        .ok_or("created agent missing")?;
    assert_eq!(
        (
            reloaded.name,
            reloaded.description,
            reloaded.system_prompt,
            reloaded.model
        ),
        (
            updated.name,
            updated.description,
            updated.system_prompt,
            default.key.clone()
        )
    );
    let session = runtime.create_session_for_agent(Some(&created.id))?;
    assert_eq!(
        runtime.get_session(&session.id)?.0.agent_id.as_deref(),
        Some(created.id.as_str())
    );
    runtime.save_model(ModelEdit {
        key: "test-embedding",
        provider: "openai",
        model: "test-model",
        base_url: &default.base_url,
        api_key: "test-secret",
        model_type: "embedding",
    })?;
    runtime.save_model(ModelEdit {
        key: "test-embedding",
        provider: "openai",
        model: "test-model-updated",
        base_url: &default.base_url,
        api_key: "",
        model_type: "embedding",
    })?;
    let models = runtime.set_default_model("test-embedding")?;
    assert!(models
        .models
        .iter()
        .any(|m| m.key == "test-embedding" && m.is_default && m.model == "test-model-updated"));
    assert!(runtime.set_default_model("missing-model").is_err());
    assert!(runtime.workspace_directory("missing-agent", "", 0).is_err());
    assert!(runtime.workspace_directory("", "../", 0).is_err());
    assert!(runtime
        .workspace_preview("", "../config/desktop.settings.json")
        .is_err());
    let root = output.join("workspace-next");
    let updated = runtime.save_runtime(root.to_str().ok_or("invalid isolated path")?, "en-US")?;
    assert_eq!(updated.language, "en-US");
    let page = runtime.workspace_directory("", "", 0)?;
    assert_eq!(page.path, "");
    let settings_file = output.join("runtime/config/desktop.settings.json");
    let persisted: serde_json::Value = serde_json::from_slice(&std::fs::read(settings_file)?)?;
    assert_eq!(
        persisted["llm"]["models"]["test-embedding"]["api_key"],
        "test-secret"
    );
    let container = persisted["container_roots"]["1"]
        .as_str()
        .ok_or("container root missing")?;
    std::fs::write(
        Path::new(container).join("test.txt"),
        "测试文本\n".repeat(9000),
    )?;
    let page = runtime.workspace_directory("", "", 0)?;
    assert!(page.entries.iter().any(|entry| entry.name == "test.txt"));
    let preview = runtime.workspace_preview("", "test.txt")?;
    assert!(preview.starts_with("测试文本\n"));
    assert!(preview.ends_with("（仅预览前 32 KiB）"));
    assert!(preview.len() < 33_000);
    runtime.save_runtime(root.to_str().ok_or("invalid path")?, "zh-CN")?;
    std::fs::write(
        output.join("pages-check.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "agent_id": created.id, "model_key": "test-embedding", "workspace": "workspace-next", "permissions": "passed"
        }))?,
    )?;
    Ok(())
}

pub fn check_restored(
    runtime: &NativeDesktop,
    output: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let settings = runtime.get_desktop_settings()?;
    assert_eq!(settings.language, "zh-CN");
    assert!(settings
        .models
        .iter()
        .any(|m| m.key == "test-ui-model" && m.is_default));
    assert!(runtime
        .list_agents()?
        .iter()
        .any(|a| a.name == "test-ui-updated" && a.system_prompt == "test-prompt"));
    assert!(runtime
        .workspace_preview("", "test.txt")?
        .starts_with("测试文本"));
    std::fs::write(
        output.join("restore.txt"),
        "PASS: settings/agents/workspace restored after process restart\n",
    )?;
    Ok(())
}
