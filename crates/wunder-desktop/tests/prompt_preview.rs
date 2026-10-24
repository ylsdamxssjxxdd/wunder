//! Read-only prompt previews share web semantics and use isolated storage.
use wunder_desktop::{args::DesktopArgs, NativeDesktop};
use wunder_server::stable_core::thread_store::*;

fn settings_payload() -> wunder_desktop::AgentSettingsEdit {
    wunder_desktop::AgentSettingsEdit {
        name: "Fixture agent".into(),
        description: "Fixture greeting".into(),
        system_prompt: String::new(),
        model_name: String::new(),
        icon_name: String::new(),
        icon_color: String::new(),
        icon: None,
        tool_names: Vec::new(),
        preset_questions: Vec::new(),
        approval_mode: "full_auto".into(),
        preview_skill: false,
        silent: false,
        prefer_mother: false,
    }
}

#[test]
fn prompt_preview_uses_agent_settings_and_preserves_the_frozen_thread_prompt() {
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("runtime");
    let config = home.join("config");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(
        config.join("wunder.yaml"),
        r#"{"llm":{"default":"","models":{}}}"#,
    )
    .unwrap();
    std::fs::write(
        config.join("desktop.settings.json"),
        r#"{"lan_mesh":{"enabled":false}}"#,
    )
    .unwrap();
    // This integration-test binary owns a single test. WUNDER_HOME isolates
    // the engine storage as well as desktop settings; temp_root alone does not.
    std::env::set_var("WUNDER_HOME", &home);
    let mut args = DesktopArgs::native_defaults();
    args.temp_root = Some(home.canonicalize().unwrap());
    args.workspace = Some(directory.path().join("workspace"));
    let runtime = NativeDesktop::start_with_args(args).unwrap();
    let mut settings = settings_payload();
    settings.system_prompt = "Fixture initial instructions".into();
    runtime
        .update_agent_settings(wunder_desktop::DEFAULT_AGENT_ID, settings)
        .unwrap();
    let session = runtime.create_session(None).unwrap();
    let initial = runtime
        .preview_thread_system_prompt(Some(&session.id), None)
        .unwrap();
    assert!(initial.contains("Fixture initial instructions"));
    assert!(!initial.contains("<<WUNDER_HISTORY_MEMORY>>"));
    let state = runtime.state();
    assert!(state
        .storage
        .list_thread_turns(runtime.user_id(), &session.id, None, 10)
        .unwrap()
        .is_empty());

    state
        .storage
        .accept_thread_turn(
            runtime.user_id(),
            &session.id,
            &serde_json::json!({"content":"Fixture input"}),
        )
        .unwrap();
    let workspace_id = state
        .workspace
        .scoped_user_id_by_container(runtime.user_id(), 1);
    let root = state.workspace.ensure_user_root(&workspace_id).unwrap();
    let workdir = state.workspace.display_path(&workspace_id, &root);
    let frozen = format!("Fixture frozen instructions\n{workdir}");
    let storage_runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let thread = ThreadId::new(session.id.clone()).unwrap();
    let rollout = RolloutId::new("fixture-prompt-rollout").unwrap();
    storage_runtime
        .block_on(state.thread_store.create_thread(CreateThreadRequest {
            metadata: ThreadMetadata {
                thread_id: thread.clone(),
                current_rollout_id: rollout.clone(),
                status: ThreadStatus::Active,
                created_at_ms: 1,
                updated_at_ms: 1,
                title: None,
                workspace_id: Some(workspace_id),
                pinned: false,
            },
            session_meta: SessionMeta {
                thread_id: thread.clone(),
                rollout_id: rollout,
                created_at_ms: 1,
                source: "fixture".into(),
                initial_system_prompt: frozen.clone(),
                initial_memory: None,
                parent_thread_id: None,
                history_base: None,
                capabilities: serde_json::Value::Null,
            },
        }))
        .unwrap();

    let mut settings = settings_payload();
    settings.system_prompt = "Fixture revised instructions".into();
    runtime
        .update_agent_settings(wunder_desktop::DEFAULT_AGENT_ID, settings)
        .unwrap();
    for _ in 0..2 {
        assert_eq!(
            runtime
                .preview_thread_system_prompt(Some(&session.id), None)
                .unwrap(),
            frozen
        );
    }
    assert_eq!(
        storage_runtime
            .block_on(state.thread_store.session_meta(&thread))
            .unwrap()
            .initial_system_prompt,
        frozen
    );
    assert!(runtime
        .preview_system_prompt()
        .unwrap()
        .contains("Fixture revised instructions"));
}
