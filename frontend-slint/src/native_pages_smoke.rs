//! Isolated persistence and permission regression for native page operations.
use std::path::Path;
use wunder_desktop::{ModelEdit, NativeCronJobEdit, NativeDesktop};
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
        .update_agent(
            "missing-agent",
            "test-agent",
            "",
            "",
            "",
            "spark",
            "#94a3b8"
        )
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
    // Runtime tool paths: a configured python file resolves as custom, an
    // invalid git path is reported invalid, and clearing restores auto mode.
    let tool_dir = output.join("tool-bin");
    std::fs::create_dir_all(&tool_dir)?;
    let python_stub = tool_dir.join("python.exe");
    std::fs::write(&python_stub, b"stub interpreter")?;
    let saved = runtime.save_runtime(
        root.to_str().ok_or("invalid isolated path")?,
        "en-US",
        python_stub.to_str().ok_or("invalid python path")?,
        "missing-git.exe",
        "",
    )?;
    assert_eq!(saved.python_path, python_stub.to_str().ok_or("path")?);
    assert_eq!(saved.git_path, "missing-git.exe");
    let python_status = saved
        .tool_status
        .iter()
        .find(|entry| entry.tool == "python")
        .ok_or("python status missing")?;
    assert_eq!(python_status.source, "custom");
    assert_eq!(python_status.effective, python_stub.to_str().ok_or("path")?);
    let git_status = saved
        .tool_status
        .iter()
        .find(|entry| entry.tool == "git")
        .ok_or("git status missing")?;
    assert_eq!(git_status.source, "invalid");
    let rg_status = saved
        .tool_status
        .iter()
        .find(|entry| entry.tool == "rg")
        .ok_or("rg status missing")?;
    assert!(matches!(rg_status.source.as_str(), "system" | "embedded"));
    let updated = runtime.save_runtime(
        root.to_str().ok_or("invalid isolated path")?,
        "en-US",
        "",
        "",
        "",
    )?;
    assert_eq!(updated.language, "en-US");
    assert!(updated.python_path.is_empty());
    assert!(updated
        .tool_status
        .iter()
        .all(|entry| entry.tool == "rg" || entry.source != "invalid"));
    let page = runtime.workspace_directory("", "", 0)?;
    assert_eq!(page.path, "");
    let settings_file = output.join("runtime/config/desktop.settings.json");
    let persisted: serde_json::Value = serde_json::from_slice(&std::fs::read(settings_file)?)?;
    assert_eq!(
        persisted["llm"]["models"]["test-embedding"]["api_key"],
        "test-secret"
    );
    // Tool path fields must survive the settings round-trip for restarts.
    assert!(persisted["python_path"].is_string());
    assert!(persisted["git_path"].is_string());
    assert!(persisted["rg_path"].is_string());
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
    runtime.save_runtime(root.to_str().ok_or("invalid path")?, "zh-CN", "", "", "")?;
    std::fs::write(
        output.join("pages-check.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "agent_id": created.id, "model_key": "test-embedding", "workspace": "workspace-next", "permissions": "passed"
        }))?,
    )?;
    check_world(runtime)?;
    check_cron(runtime)?;
    check_agent_cards(runtime, output)?;
    Ok(())
}

/// Worker-card contract: export reflects the stored record, delete removes
/// the record AND its file projection (the bidirectional sync must not
/// resurrect it), and import restores the fields verbatim.
fn check_agent_cards(
    runtime: &NativeDesktop,
    output: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use wunder_desktop::{AgentSettingsEdit, WORKER_CARD_SCHEMA_VERSION};
    let tool = runtime
        .list_tools()?
        .into_iter()
        .find(|item| item.category == "内置工具")
        .map(|item| item.name)
        .ok_or("builtin tool unavailable")?;
    let created = runtime.create_agent("card-agent")?;
    runtime.update_agent_settings(
        &created.id,
        AgentSettingsEdit {
            name: "card-agent".into(),
            description: "冒烟工蜂卡".into(),
            system_prompt: "你是冒烟测试专家。".into(),
            model_name: "test-model".into(),
            icon_name: "robot".into(),
            icon_color: "#3b82f6".into(),
            tool_names: vec![tool.clone()],
            preset_questions: vec!["总结当前进展".into()],
            sandbox_container_id: 1,
            approval_mode: "suggest".into(),
            preview_skill: true,
            silent: false,
            prefer_mother: false,
        },
    )?;
    let document = runtime.export_agent_document(&created.id)?;
    assert_eq!(document["kind"], "WorkerCard");
    assert_eq!(document["schema_version"], WORKER_CARD_SCHEMA_VERSION);
    assert_eq!(document["extra_prompt"], "你是冒烟测试专家。");
    assert_eq!(document["runtime"]["preview_skill"], true);

    let directory = output.join("agent-cards");
    let path = runtime.export_agent_to_file(&created.id, &directory)?;
    runtime.delete_agent(&created.id)?;
    assert!(runtime
        .list_agents()?
        .iter()
        .all(|agent| agent.id != created.id));

    let outcomes = runtime.import_agent_from_file(&path, false)?;
    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].created);
    let imported = &outcomes[0].agent;
    assert_eq!(imported.name, "card-agent");
    assert_eq!(imported.system_prompt, "你是冒烟测试专家。");
    assert!(imported.tool_names.contains(&tool));
    assert_eq!(imported.preset_questions, vec!["总结当前进展".to_string()]);
    assert!(imported.preview_skill);
    runtime.delete_agent(&imported.id)?;
    Ok(())
}

/// Cron contract check: create/update validation, manual run record flow,
/// and cleanup. A separate disabled job is kept for the restore check.
fn check_cron(runtime: &NativeDesktop) -> Result<(), Box<dyn std::error::Error>> {
    let at_text = (chrono::Local::now() + chrono::Duration::minutes(5))
        .format("%Y-%m-%d %H:%M")
        .to_string();
    let base = NativeCronJobEdit {
        job_id: String::new(),
        name: "test-cron-job".into(),
        schedule_kind: "at".into(),
        schedule_at: at_text,
        every_s: 0,
        cron_expr: String::new(),
        timezone: String::new(),
        message: "冒烟测试消息".into(),
        agent_id: String::new(),
        delete_after_run: false,
        enabled: true,
    };
    let created = runtime.save_cron_job(&base)?;
    assert!(!created.id.is_empty());
    assert!(created.enabled);
    assert!(runtime
        .list_cron_jobs()?
        .iter()
        .any(|job| job.id == created.id));

    // Validation rejects: past schedule time, empty message, bad interval.
    let past = NativeCronJobEdit {
        schedule_at: "2020-01-01 00:00".into(),
        ..base.clone()
    };
    assert!(runtime.save_cron_job(&past).is_err());
    let empty_message = NativeCronJobEdit {
        message: "  ".into(),
        ..base.clone()
    };
    assert!(runtime.save_cron_job(&empty_message).is_err());
    let bad_interval = NativeCronJobEdit {
        schedule_kind: "every".into(),
        every_s: 0,
        ..base.clone()
    };
    assert!(runtime.save_cron_job(&bad_interval).is_err());

    // Cron expression jobs normalize a 5-field expression.
    let cron_expr = NativeCronJobEdit {
        schedule_kind: "cron".into(),
        schedule_at: String::new(),
        cron_expr: "0 9 * * *".into(),
        name: "test-cron-expr".into(),
        ..base.clone()
    };
    let expr_job = runtime.save_cron_job(&cron_expr)?;
    assert_eq!(expr_job.schedule_kind, "cron");

    // Update renames and disables; unknown ids are rejected.
    let updated = runtime.save_cron_job(&NativeCronJobEdit {
        job_id: created.id.clone(),
        name: "test-cron-updated".into(),
        enabled: false,
        ..base.clone()
    })?;
    assert_eq!(updated.name, "test-cron-updated");
    assert!(!updated.enabled);
    let missing = NativeCronJobEdit {
        job_id: "missing-cron-job".into(),
        ..base.clone()
    };
    assert!(runtime.save_cron_job(&missing).is_err());

    assert!(runtime.list_cron_job_runs(&created.id)?.is_empty());
    let queued = runtime.run_cron_job_now(&created.id)?;
    assert!(queued == "queued" || queued == "running");
    let mut runs = Vec::new();
    for _ in 0..30 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        runs = runtime.list_cron_job_runs(&created.id)?;
        if !runs.is_empty() {
            break;
        }
    }
    assert!(!runs.is_empty(), "manual run did not produce a run record");
    assert!(!runs[0].run_id.is_empty());
    runtime.delete_cron_job(&created.id)?;
    runtime.delete_cron_job(&expr_job.id)?;
    assert!(runtime.delete_cron_job(&created.id).is_err());

    // Disabled job kept on disk for the restart restore check.
    runtime.save_cron_job(&NativeCronJobEdit {
        name: "test-cron-restore".into(),
        enabled: false,
        ..base.clone()
    })?;
    Ok(())
}

/// User-world contract check: group CRUD, announcement rules, message
/// ordering, event replay and the bounded realtime feed.
fn check_world(runtime: &NativeDesktop) -> Result<(), Box<dyn std::error::Error>> {
    use wunder_server::storage::UserAccountRecord;

    // Groups require the owner plus at least one member; provision a peer.
    let peer = UserAccountRecord {
        user_id: "smoke-peer".to_string(),
        username: "smoke-peer".to_string(),
        email: None,
        password_hash: "smoke-hash".to_string(),
        roles: vec!["user".to_string()],
        status: "active".to_string(),
        access_level: "A".to_string(),
        unit_id: None,
        quota_balance: 0,
        quota_granted_total: 0,
        quota_used_total: 0,
        last_quota_grant_date: None,
        experience_total: 0,
        is_demo: false,
        created_at: 1.0,
        updated_at: 1.0,
        last_login_at: None,
    };
    runtime.state().storage.upsert_user_account(&peer)?;
    let contacts = runtime.list_world_contacts("", 0)?.0;
    assert!(contacts.iter().any(|contact| contact.user_id == "smoke-peer"));

    assert!(runtime.create_world_group("空成员群组", &[]).is_err());
    let conversation =
        runtime.create_world_group("test-world-group", std::slice::from_ref(&peer.user_id))?;
    let groups = runtime.list_world_groups(0)?.0;
    let group = groups
        .iter()
        .find(|g| g.name == "test-world-group")
        .ok_or("created group missing")?;
    assert_eq!(group.conversation_id, conversation);
    let detail = runtime.get_world_group_detail(&group.group_id)?;
    assert_eq!(detail.announcement, "");
    assert_eq!(detail.owner_user_id, runtime.user_id());
    assert!(detail.members.iter().any(|m| m.user_id == runtime.user_id()));

    runtime.update_world_group_announcement(&group.group_id, "  阶段说明  ")?;
    assert_eq!(
        runtime.get_world_group_detail(&group.group_id)?.announcement,
        "阶段说明"
    );
    let long = "长".repeat(4_001);
    assert!(runtime
        .update_world_group_announcement(&group.group_id, &long)
        .is_err());
    runtime.update_world_group_announcement(&group.group_id, "")?;
    assert_eq!(
        runtime.get_world_group_detail(&group.group_id)?.announcement,
        ""
    );
    assert!(runtime
        .update_world_group_announcement("missing-group", "无权限写入")
        .is_err());

    assert!(runtime
        .list_world_messages(&conversation, None)?
        .is_empty());
    let sent = runtime.send_world_message(&conversation, "第一条消息")?;
    runtime.send_world_message(&conversation, "第二条消息")?;
    let messages = runtime.list_world_messages(&conversation, None)?;
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].content, "第一条消息");
    assert_eq!(messages[1].content, "第二条消息");
    assert!(messages.iter().all(|m| m.mine));
    assert_eq!(messages[0].id, sent.id);
    assert!(!runtime.has_older_world_messages(&conversation, messages[0].id)?);
    runtime.mark_world_read(&conversation, None)?;

    let events = runtime.list_world_events(&conversation, 0)?;
    assert!(events
        .iter()
        .any(|event| event.event_type == "uw.message"
            && event
                .message
                .as_ref()
                .is_some_and(|message| message.content == "第二条消息")));
    let newest_event = events.last().map(|event| event.event_id).unwrap_or(0);
    assert!(runtime
        .list_world_events(&conversation, newest_event)?
        .is_empty());

    let direct = runtime.create_world_direct_conversation(&peer.user_id)?;
    runtime.send_world_message(&direct, "单聊消息")?;
    let direct_messages = runtime.list_world_messages(&direct, None)?;
    assert_eq!(direct_messages.len(), 1);
    assert!(direct_messages[0].mine);

    // The rail badge counts only messages from others: a peer message raises
    // the total by one and marking the conversation read restores it.
    let baseline = runtime.total_world_unread()?;
    runtime.state().storage.send_user_world_message(
        &conversation,
        "smoke-peer",
        "来自成员的消息",
        "text",
        None,
        1_800_000_000.0,
    )?;
    assert_eq!(runtime.total_world_unread()?, baseline + 1);
    runtime.mark_world_read(&conversation, None)?;
    assert_eq!(runtime.total_world_unread()?, baseline);

    // The realtime feed must deliver the next send without polling storage.
    let feed = runtime.start_world_event_feed()?;
    std::thread::sleep(std::time::Duration::from_millis(200));
    runtime.send_world_message(&conversation, "实时消息")?;
    let mut received = None;
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        for event in feed.drain(16) {
            if let Some(message) = event.message {
                if message.content == "实时消息" {
                    received = Some(message);
                }
            }
        }
        if received.is_some() {
            break;
        }
    }
    assert!(received.is_some(), "realtime feed did not deliver the message");
    assert!(!feed.take_overflowed());
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
    let groups = runtime.list_world_groups(0)?.0;
    let group = groups
        .iter()
        .find(|g| g.name == "test-world-group")
        .ok_or("world group missing after restart")?;
    assert!(!runtime
        .list_world_messages(&group.conversation_id, None)?
        .is_empty());
    assert!(runtime
        .list_cron_jobs()?
        .iter()
        .any(|job| job.name == "test-cron-restore" && !job.enabled));
    std::fs::write(
        output.join("restore.txt"),
        "PASS: settings/agents/workspace/world/cron restored after process restart\n",
    )?;
    Ok(())
}
