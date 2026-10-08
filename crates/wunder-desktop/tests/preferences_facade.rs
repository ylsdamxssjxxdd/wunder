//! Preferences, prompt-pack, diagnostics and work-state reset contract tests
//! against an isolated SQLite runtime (desktop N5 gates).

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
                           "support_vision": false, "api_key": "contract-secret"}}}
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
fn prompt_pack_lifecycle_roundtrip() {
    let runtime = start_isolated().expect("isolated runtime");
    let (active, packs, segments) = runtime.list_prompt_packs().expect("list");
    assert!(!active.is_empty());
    assert!(segments.len() >= 6, "system segments exposed");
    let builtins: Vec<_> = packs.iter().filter(|pack| pack.builtin).collect();
    assert!(builtins.len() >= 2);
    assert!(builtins.iter().all(|pack| pack.readonly));

    // A custom pack starts empty, becomes editable, and can be activated.
    runtime.create_prompt_pack("contract-pack").expect("create");
    assert!(
        runtime.create_prompt_pack("contract-pack").is_err(),
        "duplicate"
    );
    assert!(
        runtime.create_prompt_pack("default-zh").is_err(),
        "builtin name"
    );
    assert!(
        runtime.create_prompt_pack("非法/名称").is_err(),
        "invalid id"
    );

    // Reading an absent segment falls back to the system pack content.
    let fallback = runtime
        .read_prompt_segment("contract-pack", "role")
        .expect("read fallback");
    assert!(!fallback.readonly);
    assert!(!fallback.exists);
    assert!(
        !fallback.content.is_empty(),
        "system fallback provides content"
    );

    let edited = "合同测试角色提示词。";
    runtime
        .write_prompt_segment("contract-pack", "role", edited)
        .expect("write");
    let stored = runtime
        .read_prompt_segment("contract-pack", "role")
        .expect("read");
    assert!(stored.exists);
    assert_eq!(stored.content, edited);

    runtime
        .set_active_prompt_pack("contract-pack")
        .expect("activate");
    let (active, _, _) = runtime.list_prompt_packs().expect("list");
    assert_eq!(active, "contract-pack");
    assert!(runtime.set_active_prompt_pack("missing-pack").is_err());

    // Built-in packs reject writes and deletion at the façade boundary.
    assert!(runtime
        .write_prompt_segment("default-zh", "role", "x")
        .is_err());
    assert!(runtime.delete_prompt_pack("default-zh").is_err());
    runtime
        .write_prompt_segment("contract-pack", "role", "")
        .expect("clear");

    runtime.delete_prompt_pack("contract-pack").expect("delete");
    let (active, packs, _) = runtime.list_prompt_packs().expect("list");
    assert!(packs.iter().all(|pack| pack.id != "contract-pack"));
    let language_default = wunder_server::user_prompt_templates::resolve_default_user_pack_id();
    assert_eq!(
        active, language_default,
        "deleting the active pack falls back to the system language default"
    );
}

#[test]
fn preferences_validate_and_persist() {
    let runtime = start_isolated().expect("isolated runtime");
    assert!(
        runtime.save_preferences("dark", "enter", 14).is_err(),
        "unknown theme"
    );
    assert!(
        runtime.save_preferences("light", "space", 14).is_err(),
        "unknown send key"
    );
    assert!(
        runtime.save_preferences("light", "enter", 9).is_err(),
        "font size out of range"
    );
    let settings = runtime
        .save_preferences("hula-green", "none", 18)
        .expect("save");
    assert_eq!(settings.theme, "hula-green");
    assert_eq!(settings.send_key, "none");
    assert_eq!(settings.font_size, 18);
    let reloaded = runtime.get_desktop_settings().expect("reload");
    assert_eq!(reloaded.send_key, "none");
    assert_eq!(reloaded.font_size, 18);
}

#[test]
fn session_reasoning_effort_persists_through_the_native_facade() {
    let runtime = start_isolated().expect("isolated runtime");
    let session = runtime.create_session(None).expect("create session");

    assert_eq!(session.reasoning_effort, "default");
    assert_eq!(
        runtime
            .save_session_reasoning_effort(&session.id, "high")
            .expect("save reasoning effort"),
        "high"
    );
    assert_eq!(
        runtime
            .get_session_info(&session.id)
            .expect("reload session")
            .reasoning_effort,
        "high"
    );
    assert!(runtime
        .save_session_reasoning_effort("missing-session", "low")
        .is_err());
}

#[test]
fn diagnostics_export_is_secret_free() {
    let runtime = start_isolated().expect("isolated runtime");
    let directory = tempfile::tempdir().expect("tempdir");
    let path = runtime
        .export_diagnostics(directory.path())
        .expect("export");
    let text = std::fs::read_to_string(&path).expect("read bundle");
    assert!(text.contains("wunder-desktop-diagnostics"));
    let lowered = text.to_lowercase();
    assert!(
        !lowered.contains("contract-secret"),
        "api key leaked into bundle"
    );
    assert!(!lowered.contains("api_key"), "key field name leaked");
    assert!(lowered.contains("counts"));
}

#[test]
fn reset_work_state_preserves_assets_and_reports_summary() {
    let runtime = start_isolated().expect("isolated runtime");
    let summary = runtime.reset_work_state().expect("reset");
    assert_eq!(summary.cancelled_sessions, 0);
    assert!(
        runtime
            .list_agents()
            .expect("list")
            .iter()
            .any(|agent| agent.id == "__default__"),
        "the built-in agent must survive the reset"
    );
}
