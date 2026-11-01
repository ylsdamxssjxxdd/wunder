//! Preferences, prompt-pack, diagnostics and work-state reset contract tests
//! against an isolated SQLite runtime (desktop N5 gates).

use std::sync::{Mutex, OnceLock};
use wunder_desktop::{args::DesktopArgs, NativeDesktop};

/// One runtime per home at a time: the second initializer in the same home
/// races the default-user seed.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Config, settings and the database all live under `WUNDER_HOME`; point it at
/// a throw-away directory so these tests never touch the developer's real
/// home. The engine config file is the source of truth for the model
/// catalogue; JSON is valid YAML so the fixture keeps a single document.
fn isolated_home() -> &'static std::path::Path {
    static HOME: OnceLock<std::path::PathBuf> = OnceLock::new();
    HOME.get_or_init(|| {
        let directory = tempfile::tempdir().expect("temp home");
        let path = directory.path().to_path_buf();
        std::env::set_var("WUNDER_HOME", &path);
        std::mem::forget(directory);
        path
    })
}

fn start_isolated() -> Result<NativeDesktop, Box<dyn std::error::Error>> {
    let home = isolated_home();
    let config = home.join("config");
    std::fs::create_dir_all(&config)?;
    let engine_config = serde_json::json!({
        "llm": {"default": "test-model", "models": {
            "test-model": {"provider": "openai", "model": "test-model", "model_type": "llm",
                           "base_url": "http://127.0.0.1:9/v1", "timeout_s": 5,
                           "support_vision": false, "api_key": "contract-secret"}}}
    });
    std::fs::write(config.join("wunder.yaml"), serde_json::to_vec(&engine_config)?)?;
    let settings = serde_json::json!({
        "workspace_root": "", "desktop_token": "", "updated_at": 0,
        "lan_mesh": {"enabled": false}
    });
    std::fs::write(
        config.join("desktop.settings.json"),
        serde_json::to_vec(&settings)?,
    )?;
    let mut args = DesktopArgs::native_defaults();
    args.temp_root = Some(home.join("temp"));
    args.workspace = Some(home.join("workspace"));
    Ok(NativeDesktop::start_with_args(args)?)
}

#[test]
fn prompt_pack_lifecycle_roundtrip() {
    let _serial = serial();
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
    let _serial = serial();
    let runtime = start_isolated().expect("isolated runtime");
    assert!(
        runtime.save_preferences("dark", "enter", 14, "cloud").is_err(),
        "unknown theme"
    );
    assert!(
        runtime.save_preferences("light", "space", 14, "cloud").is_err(),
        "unknown send key"
    );
    assert!(
        runtime.save_preferences("light", "enter", 9, "cloud").is_err(),
        "font size out of range"
    );
    let settings = runtime
        .save_preferences("hula-green", "none", 18, "github")
        .expect("save");
    assert_eq!(settings.theme, "hula-green");
    assert_eq!(settings.send_key, "none");
    assert_eq!(settings.font_size, 18);
    assert_eq!(settings.update_source, "github");
    // The 蛹 theme card writes these four ids; a rejected save leaves the
    // accent applied live but lost on the next start.
    for theme in ["terracotta", "forest", "honey"] {
        let saved = runtime
            .save_preferences(theme, "none", 18, "cloud")
            .unwrap_or_else(|error| panic!("save {theme}: {error}"));
        assert_eq!(saved.theme, theme);
        assert_eq!(runtime.get_desktop_settings().expect("reload").theme, theme);
    }
    let reloaded = runtime.get_desktop_settings().expect("reload");
    assert_eq!(reloaded.send_key, "none");
    assert_eq!(reloaded.font_size, 18);
    assert_eq!(reloaded.update_source, "cloud", "last save wins");
    // Unknown values normalize to the cloud default instead of poisoning the
    // settings file.
    runtime
        .save_preferences("honey", "enter", 14, "  ")
        .expect("blank source save");
    assert_eq!(
        runtime.get_desktop_settings().expect("reload").update_source,
        "cloud"
    );
}

#[test]
fn session_reasoning_effort_persists_through_the_native_facade() {
    let _serial = serial();
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
fn session_model_override_persists_through_the_native_facade() {
    let _serial = serial();
    let runtime = start_isolated().expect("isolated runtime");
    let session = runtime.create_session(None).expect("create session");

    assert_eq!(session.session_model_name, None, "no override at birth");
    assert!(
        runtime
            .set_session_model(&session.id, "missing-model")
            .is_err(),
        "unknown model keys are rejected at the façade"
    );
    runtime
        .set_session_model(&session.id, "test-model")
        .expect("pin model");
    let reloaded = runtime
        .get_session_info(&session.id)
        .expect("reload session");
    assert_eq!(
        reloaded.session_model_name.as_deref(),
        Some("test-model"),
        "override round-trips"
    );
    runtime
        .set_session_model(&session.id, "")
        .expect("clear override");
    assert_eq!(
        runtime
            .get_session_info(&session.id)
            .expect("reload session")
            .session_model_name,
        None,
        "clearing falls back to the agent chain"
    );
    assert!(runtime
        .set_session_model("missing-session", "test-model")
        .is_err());
}

#[test]
fn diagnostics_export_is_secret_free() {
    let _serial = serial();
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
    let _serial = serial();
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
