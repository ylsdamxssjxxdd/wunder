//! 全新安装只预置「虚拟模型」，而它占着会话默认位：用户配好的第一个真实模型
//! 必须直接接管这个默认位，不必再手动点一次「设为默认」；之后的模型不再抢。

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

/// Never touch the developer's real model list: config and database live under
/// a throw-away `WUNDER_HOME`.
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

#[test]
fn the_first_saved_model_takes_over_the_virtual_default() {
    let _serial = serial();
    let runtime = start_isolated().expect("isolated runtime");

    let seeded = runtime.get_desktop_settings().expect("seeded settings");
    let default = seeded
        .models
        .iter()
        .find(|model| model.is_default)
        .expect("a fresh install seeds a default model");
    assert_eq!(
        default.provider, "virtual_replay",
        "a fresh install starts on the seeded virtual model"
    );

    let settings = runtime.save_model(create("first-model")).expect("save");
    assert!(
        settings
            .models
            .iter()
            .find(|model| model.key == "first-model")
            .is_some_and(|model| model.is_default),
        "the first real model must take the default seat"
    );

    // A later model never steals a seat a real model already holds.
    let settings = runtime.save_model(create("second-model")).expect("save");
    assert!(
        settings
            .models
            .iter()
            .find(|model| model.key == "first-model")
            .is_some_and(|model| model.is_default),
        "the second model must not move the default seat"
    );
}
