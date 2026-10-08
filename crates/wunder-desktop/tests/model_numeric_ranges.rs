//! Numeric range contract for the model settings form (§9.4).
//!
//! The panel labels every numeric field with its accepted range; the façade is
//! what actually enforces it. These cases pin the bounds so a panel that starts
//! accepting an out-of-range value fails here instead of at run time.

use wunder_desktop::ModelEdit;

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
    let directory = tempfile::tempdir()?;
    let config = directory.path().join("runtime/config");
    std::fs::create_dir_all(&config)?;
    std::fs::write(config.join("wunder.yaml"), "{}\n")?;
    std::fs::write(
        config.join("desktop.settings.json"),
        serde_json::to_vec(&serde_json::json!({
            "workspace_root": "", "desktop_token": "", "updated_at": 0,
            "lan_mesh": {"enabled": false},
            "llm": {"default": "test-model", "models": {}}
        }))?,
    )?;
    let mut args = wunder_desktop::args::DesktopArgs::native_defaults();
    args.temp_root = Some(directory.path().join("runtime").canonicalize()?);
    args.workspace = Some(directory.path().join("workspace"));
    std::mem::forget(directory);
    let runtime = wunder_desktop::NativeDesktop::start_with_args(args)?;
    runtime.save_model(edit)
}

#[test]
fn temperature_outside_zero_to_two_is_refused() {
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
