//! Renaming a model config keeps the entry, its default role and the threads
//! that pinned it instead of dropping them on the floor.

use std::sync::{Mutex, OnceLock};
use wunder_desktop::{args::DesktopArgs, ModelEdit, NativeDesktop};

/// One runtime per home at a time: the second initializer in the same home
/// races the default-user seed.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Config and the database always live under `WUNDER_HOME`; point it at a
/// throw-away directory so a rename test never edits the developer's real
/// model list. Shared by both tests in this binary.
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

fn create<'a>(key: &'a str) -> ModelEdit<'a> {
    ModelEdit {
        key,
        provider: "openai",
        model: "upstream-model",
        base_url: "http://127.0.0.1:9/v1",
        model_type: "llm",
        timeout_s: "5",
        ..ModelEdit::default()
    }
}

fn rename<'a>(key: &'a str, original_key: &'a str) -> ModelEdit<'a> {
    ModelEdit {
        original_key,
        ..create(key)
    }
}

#[test]
fn renaming_keeps_default_and_thread_pins() {
    let _serial = serial();
    let runtime = start_isolated().expect("isolated runtime");
    runtime
        .save_model(create("alpha-model"))
        .expect("create model");
    runtime
        .set_default_model("alpha-model")
        .expect("make default");
    let session = runtime.create_session(None).expect("create session");
    runtime
        .set_session_model(&session.id, "alpha-model")
        .expect("pin model");

    let settings = runtime
        .save_model(rename("renamed-model", "alpha-model"))
        .expect("rename");

    assert!(
        settings
            .models
            .iter()
            .all(|model| model.key != "alpha-model"),
        "old key is gone"
    );
    assert!(
        settings
            .models
            .iter()
            .find(|model| model.key == "renamed-model")
            .is_some_and(|model| model.is_default),
        "renamed model inherits the default role"
    );
    assert_eq!(
        runtime
            .get_session_info(&session.id)
            .expect("reload session")
            .session_model_name
            .as_deref(),
        Some("renamed-model"),
        "thread pin follows the rename"
    );
}

#[test]
fn rename_rejects_an_existing_key() {
    let _serial = serial();
    let runtime = start_isolated().expect("isolated runtime");
    runtime
        .save_model(create("alpha-model"))
        .expect("create model");
    runtime
        .save_model(create("second-model"))
        .expect("create second model");

    assert!(
        runtime
            .save_model(rename("second-model", "alpha-model"))
            .is_err(),
        "renaming onto an occupied key is rejected"
    );
    let settings = runtime.get_desktop_settings().expect("reload settings");
    assert!(
        settings
            .models
            .iter()
            .any(|model| model.key == "alpha-model"),
        "failed rename leaves the original entry in place"
    );
}
