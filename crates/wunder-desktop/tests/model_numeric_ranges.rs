//! Numeric range contract for the model settings form (§9.4).
//!
//! The panel labels every numeric field with its accepted range; the façade is
//! what actually enforces it. These cases pin the bounds so a panel that starts
//! accepting an out-of-range value fails here instead of at run time.

use std::sync::{Mutex, OnceLock};
use wunder_desktop::ModelEdit;

/// One runtime per home at a time: the second initializer in the same home
/// races the default-user seed.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Config, settings and the database all live under `WUNDER_HOME`; point it at
/// a throw-away directory so a model save never edits the developer's real
/// model list.
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

fn edit<'a>(temperature: &'a str, timeout_s: &'a str, image_steps: &'a str) -> ModelEdit<'a> {
    ModelEdit {
        key: "test-model",
        provider: "openai",
        model: "test-model",
        base_url: "",
        api_key: "",
        model_type: "llm",
        temperature,
        timeout_s,
        image_steps,
        ..ModelEdit::default()
    }
}

fn save(edit: ModelEdit<'_>) -> anyhow::Result<wunder_desktop::DesktopSettings> {
    let home = isolated_home();
    let config = home.join("config");
    std::fs::create_dir_all(&config)?;
    std::fs::write(config.join("wunder.yaml"), "{}\n")?;
    let settings = serde_json::json!({
        "workspace_root": "", "desktop_token": "", "updated_at": 0,
        "lan_mesh": {"enabled": false}
    });
    std::fs::write(
        config.join("desktop.settings.json"),
        serde_json::to_vec(&settings)?,
    )?;
    let mut args = wunder_desktop::args::DesktopArgs::native_defaults();
    args.temp_root = Some(home.join("temp"));
    args.workspace = Some(home.join("workspace"));
    let runtime = wunder_desktop::NativeDesktop::start_with_args(args)?;
    runtime.save_model(edit)
}

#[test]
fn temperature_outside_zero_to_two_is_refused() {
    let _serial = serial();
    let refused = save(edit("3.5", "", ""));
    assert!(
        refused.is_err(),
        "temperature 3.5 must be refused, got {:?}",
        refused.map(|settings| settings.models.len())
    );
    // The boundary values stay accepted.
    save(edit("2", "", "")).expect("temperature 2 is the inclusive upper bound");
    save(edit("0", "", "")).expect("temperature 0 is the inclusive lower bound");
}

#[test]
fn a_non_numeric_numeric_field_is_refused() {
    let _serial = serial();
    assert!(
        save(edit("", "soon", "")).is_err(),
        "timeout must be numeric"
    );
    assert!(
        save(edit("", "", "0")).is_err(),
        "sampling steps must be at least 1"
    );
}

#[test]
fn an_empty_numeric_field_keeps_the_model_default() {
    let _serial = serial();
    let settings = save(edit("", "", "")).expect("blank fields mean 'use the model default'");
    let saved = settings
        .models
        .iter()
        .find(|model| model.key == "test-model")
        .expect("the saved model is listed");
    assert_eq!(saved.temperature, "");
    assert_eq!(saved.timeout_s, "");
    assert_eq!(
        saved.image_steps, "",
        "a parameter that does not apply to this type stays unset"
    );
}
