use crate::MainWindow;
use slint::{ComponentHandle, Model, Timer, TimerMode};
use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

pub fn run(
    app: &MainWindow,
    directory: PathBuf,
    api: Arc<wunder_desktop::NativeDesktop>,
) -> Result<(), Box<dyn std::error::Error>> {
    let result = Rc::new(RefCell::new(None));
    let output = result.clone();
    let weak = app.as_weak();
    let timer = Timer::default();
    let started = Instant::now();
    let mut phase = 0;
    timer.start(TimerMode::Repeated, Duration::from_millis(40), move || {
        let Some(app) = weak.upgrade() else { return };
        let checked = (|| -> Result<bool, Box<dyn std::error::Error>> {
            if started.elapsed() > Duration::from_secs(45) {
                return Err(format!("model smoke timed out in phase {phase}: {}", app.get_model_save_error()).into());
            }
            if app.get_chat_loading() || app.get_saving() || app.get_settings_loading() || app.get_agents_loading() || app.get_creating_session() || app.get_session_loading() {
                return Ok(false);
            }
            let saved = app.get_models().iter().find(|model| model.key == "extra-model");
            match phase {
                0 => {
                    app.set_settings_open(true);
                    app.set_settings_active_panel(1);
                    app.invoke_create_model();
                    app.set_model_key_draft("extra-model".into());
                    app.set_model_name_draft("sample-model".into());
                    // 新建草稿不再自动保存，这里显式提交（表单上的「保存」）。
                    app.invoke_save_model();
                    app.invoke_new_thread();
                    phase = 1;
                }
                1 => {
                    let Some(model) = saved.filter(|_| !app.get_model_creating()) else { return Ok(false) };
                    assert_state(!model.is_default, "new model changed the default")?;
                    assert_state(app.get_composer_models().iter().any(|model| model.key == "extra-model"), "new model missing from composer")?;
                    app.invoke_select_thread_model("extra-model".into());
                    app.set_model_temperature_draft("0.6".into());
                    app.invoke_save_model();
                    assert_state(app.get_saving(), "save did not begin")?;
                    app.set_model_temperature_draft("0.8".into());
                    app.invoke_refresh_settings();
                    phase = 2;
                }
                2 => {
                    if !saved.is_some_and(|model| model.temperature == "0.8") { return Ok(false) }
                    let session = api.get_session(app.get_active_session_id().as_str())?.0;
                    assert_state(session.session_model_name.as_deref() == Some("extra-model"), "nondefault model was not selectable for the thread")?;
                    assert_state(app.get_model_temperature_draft() == "0.8", "save acknowledgement overwrote newer input")?;
                    app.set_model_temperature_draft("0.9".into());
                    crate::entity_state::restore_model(&app, "test-model");
                    phase = 3;
                }
                3 => {
                    if app.get_settings_model_key() != "test-model" || !saved.is_some_and(|model| model.temperature == "0.9") { return Ok(false) }
                    crate::entity_state::restore_model(&app, "extra-model");
                    app.set_model_max_output_draft("invalid".into());
                    phase = 4;
                }
                4 => {
                    if app.get_model_save_error().is_empty() { return Ok(false) }
                    assert_state(app.get_model_max_output_draft() == "invalid", "failed save discarded input")?;
                    app.set_model_max_output_draft("4096".into());
                    phase = 5;
                }
                5 => {
                    if !saved.is_some_and(|model| model.max_output == "4096") { return Ok(false) }
                    assert_state(app.get_model_save_error().is_empty(), "successful retry retained an error")?;
                    crate::native_pages::select_model_key(&app, "test-model");
                    app.invoke_delete_model("test-model".into());
                    phase = 6;
                }
                6 => {
                    if app.get_models().iter().any(|model| model.key == "test-model") { return Ok(false) }
                    assert_state(saved.is_some_and(|model| model.is_default), "default was not promoted")?;
                    assert_state(app.get_selected_model_key() == "extra-model", "composer retained the deleted model")?;
                    assert_state(app.get_composer_models().row_count() == 1, "composer catalogue did not reconcile deletion")?;
                    crate::smoke::snapshot(&app, &directory.join("model-settings.png"))?;
                    return Ok(true);
                }
                _ => unreachable!(),
            }
            Ok(false)
        })();
        if matches!(checked, Ok(false)) { return; }
        let report = match &checked {
            Ok(_) => "PASS: model autosave/catalogue/inflight-edit/switch-flush/validation/default-fallback\n".to_owned(),
            Err(error) => format!("FAIL: {error}\n"),
        };
        *output.borrow_mut() = Some(std::fs::write(directory.join("smoke.txt"), report)
            .map_err(|error| error.to_string())
            .and(checked.map(|_| ()).map_err(|error| error.to_string())));
        let _ = slint::quit_event_loop();
    });
    app.run()?;
    timer.stop();
    result
        .borrow_mut()
        .take()
        .ok_or("model smoke did not run")??;
    Ok(())
}

fn assert_state(condition: bool, message: &str) -> Result<(), Box<dyn std::error::Error>> {
    crate::native_smoke::ensure(condition, message)
}
