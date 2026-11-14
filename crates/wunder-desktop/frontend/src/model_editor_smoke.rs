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
    // Catalogue size before this probe adds/removes its own rows; the engine
    // may seed extra models (e.g. the virtual replay entry), so the count is
    // relative rather than a fixed one.
    let mut baseline_models = 0;
    // 运行前是否已有默认模型：全新运行时默认位是空的，新增的第一条真实配置
    // 接管它是预期行为。
    let mut had_default = false;
    timer.start(TimerMode::Repeated, Duration::from_millis(40), move || {
        let Some(app) = weak.upgrade() else { return };
        let saved = app.get_models().iter().find(|model| model.key == "extra-model");
        let checked = (|| -> Result<bool, Box<dyn std::error::Error>> {
            if started.elapsed() > Duration::from_secs(60) {
                return Err(format!("model smoke timed out in phase {phase}: {}", app.get_model_save_error()).into());
            }
            if app.get_chat_loading() || app.get_saving() || app.get_settings_loading() || app.get_agents_loading() || app.get_creating_session() || app.get_session_loading() {
                return Ok(false);
            }
            match phase {
                0 => {
                    app.set_settings_open(true);
                    app.set_settings_active_panel(1);
                    baseline_models = app.get_composer_models().row_count();
                    // 预置的「虚拟模型」占着默认位不算真实默认：第一条真实配置
                    // 接管它是预期行为。
                    had_default = app
                        .get_models()
                        .iter()
                        .any(|model| model.is_default && model.provider != "virtual_replay");
                    // 「添加模型」立即写出一条完整配置；接下来只需改名并保存。
                    app.invoke_create_model();
                    // 线程模型钉选需要一条真实会话。
                    app.invoke_new_thread();
                    phase = 1;
                }
                1 => {
                    let created = app
                        .get_models()
                        .iter()
                        .find(|model| model.key.starts_with("新模型"))
                        .map(|model| model.key.to_string());
                    let Some(created) = created else { return Ok(false) };
                    assert_state(
                        app.get_settings_model_key() == created,
                        "the added model was not selected in the editor",
                    )?;
                    app.set_model_key_draft("extra-model".into());
                    app.set_model_name_draft("sample-model".into());
                    app.set_model_temperature_draft("0.6".into());
                    app.invoke_save_model();
                    assert_state(app.get_saving(), "save did not begin")?;
                    phase = 2;
                }
                2 => {
                    if !saved.as_ref().is_some_and(|model| model.temperature == "0.6") { return Ok(false) }
                    if app.get_settings_model_key() != "extra-model" { return Ok(false) }
                    assert_state(
                        saved.as_ref().is_some_and(|model| model.is_default) == !had_default,
                        "a saved model took over an existing default seat",
                    )?;
                    assert_state(app.get_composer_models().iter().any(|model| model.key == "extra-model"), "new model missing from composer")?;
                    app.invoke_select_thread_model("extra-model".into());
                    // 未保存的改动不再自动落库：切到另一行会被拦下并提示保存。
                    app.set_model_temperature_draft("0.8".into());
                    let other = app
                        .get_models()
                        .iter()
                        .find(|model| model.key != "extra-model")
                        .map(|model| model.key.to_string());
                    if let Some(other) = other {
                        crate::entity_state::restore_model(&app, &other);
                        assert_state(
                            app.get_settings_model_key() == "extra-model",
                            "switching rows silently dropped the unsaved edit",
                        )?;
                        assert_state(
                            !app.get_model_save_error().is_empty(),
                            "blocked switch did not explain what to do",
                        )?;
                    }
                    app.invoke_save_model();
                    assert_state(app.get_saving(), "save did not begin")?;
                    phase = 3;
                }
                3 => {
                    if !saved.as_ref().is_some_and(|model| model.temperature == "0.8") { return Ok(false) }
                    assert_state(app.get_model_temperature_draft() == "0.8", "save acknowledgement overwrote newer input")?;
                    let session = api.get_session(app.get_active_session_id().as_str())?.0;
                    assert_state(session.session_model_name.as_deref() == Some("extra-model"), "nondefault model was not selectable for the thread")?;
                    assert_state(app.get_model_save_error().is_empty(), "successful save retained an error")?;
                    app.set_model_max_output_draft("invalid".into());
                    app.invoke_save_model();
                    phase = 4;
                }
                4 => {
                    if app.get_model_save_error().is_empty() { return Ok(false) }
                    assert_state(app.get_model_max_output_draft() == "invalid", "failed save discarded input")?;
                    app.set_model_max_output_draft("4096".into());
                    app.invoke_save_model();
                    phase = 5;
                }
                5 => {
                    if !saved.as_ref().is_some_and(|model| model.max_output == "4096") { return Ok(false) }
                    assert_state(app.get_model_save_error().is_empty(), "successful retry retained an error")?;
                    // 再添加一条：它就是后文的「默认位被删后该由谁接管」的被删方。
                    app.invoke_create_model();
                    phase = 6;
                }
                6 => {
                    let created = app
                        .get_models()
                        .iter()
                        .find(|model| model.key.starts_with("新模型"))
                        .map(|model| model.key.to_string());
                    let Some(created) = created else { return Ok(false) };
                    assert_state(
                        app.get_settings_model_key() == created,
                        "the second added model was not selected",
                    )?;
                    app.set_model_key_draft("test-model".into());
                    app.set_model_name_draft("sample-other".into());
                    app.invoke_save_model();
                    assert_state(app.get_saving(), "second model save did not begin")?;
                    phase = 7;
                }
                7 => {
                    let other = app.get_models().iter().find(|model| model.key == "test-model");
                    if other.is_none() { return Ok(false) }
                    app.invoke_set_default_model("test-model".into());
                    assert_state(app.get_saving(), "set-default did not begin")?;
                    phase = 8;
                }
                8 => {
                    let other = app.get_models().iter().find(|model| model.key == "test-model");
                    let Some(other) = other else { return Ok(false) };
                    if !other.is_default { return Ok(false) }
                    assert_state(!saved.as_ref().is_some_and(|model| model.is_default), "old default survived the switch")?;
                    crate::entity_state::restore_model(&app, "test-model");
                    app.invoke_delete_model("test-model".into());
                    phase = 9;
                }
                9 => {
                    if app.get_models().iter().any(|model| model.key == "test-model") { return Ok(false) }
                    assert_state(saved.as_ref().is_some_and(|model| model.is_default), "default was not promoted")?;
                    assert_state(app.get_composer_models().iter().any(|model| model.key == "extra-model"), "composer lost the surviving model")?;
                    assert_state(!app.get_composer_models().iter().any(|model| model.key == "test-model"), "composer retained the deleted model")?;
                    // 改名只能在选中该行时提交。
                    crate::entity_state::restore_model(&app, "extra-model");
                    app.set_model_key_draft("renamed-extra".into());
                    app.invoke_commit_model_key();
                    phase = 10;
                }
                10 => {
                    let renamed = app.get_models().iter().find(|model| model.key == "renamed-extra");
                    if renamed.is_none() { return Ok(false) }
                    assert_state(!app.get_models().iter().any(|model| model.key == "extra-model"), "old key survived the rename")?;
                    assert_state(app.get_settings_model_key() == "renamed-extra", "editor kept the old key")?;
                    assert_state(
                        app.get_selected_model_key() == "renamed-extra",
                        &format!(
                            "composer kept the old key (selected {:?}, composer {:?})",
                            app.get_selected_model_key(),
                            app.get_composer_models()
                                .iter()
                                .map(|model| model.key.to_string())
                                .collect::<Vec<_>>()
                        ),
                    )?;
                    assert_state(renamed.as_ref().is_some_and(|model| model.is_default), "rename dropped the default badge")?;
                    let session = api.get_session(app.get_active_session_id().as_str())?.0;
                    assert_state(session.session_model_name.as_deref() == Some("renamed-extra"), "thread pin lost on rename")?;
                    // A second model makes the picker's row click meaningful:
                    // the pinned row and this one are different keys.
                    app.invoke_create_model();
                    phase = 11;
                }
                11 => {
                    let created = app
                        .get_models()
                        .iter()
                        .find(|model| model.key.starts_with("新模型"))
                        .map(|model| model.key.to_string());
                    let Some(created) = created else { return Ok(false) };
                    assert_state(
                        app.get_settings_model_key() == created,
                        "the third added model was not selected",
                    )?;
                    app.set_model_key_draft("threadless-model".into());
                    app.set_model_name_draft("sample-alt".into());
                    app.invoke_save_model();
                    assert_state(app.get_saving(), "third model save did not begin")?;
                    phase = 12;
                }
                12 => {
                    let alt = app.get_models().iter().find(|model| model.key == "threadless-model");
                    let Some(alt) = alt else { return Ok(false) };
                    assert_state(!alt.is_default, "an added model unexpectedly became the default")?;
                    // 新增后仍停在设置页：留一张编辑器截图（保存/默认/删除三个
                    // 常驻按钮与当前配置）。
                    crate::smoke::snapshot(&app, &directory.join("model-editor.png"))?;
                    // Close the settings overlay on its own tick so the next
                    // pointer press cannot land on the panel that still owns
                    // the window this frame.
                    app.set_settings_open(false);
                    phase = 13;
                }
                13 => {
                    let x = app.get_composer_model_trigger_x() + app.get_composer_model_trigger_width() / 2.0;
                    let y = app.get_composer_model_trigger_y() + app.get_composer_model_trigger_height() / 2.0;
                    crate::smoke::click(&app, x, y)?;
                    phase = 14;
                }
                14 => {
                    assert_state(app.get_composer_model_open(), "composer model trigger did not open the picker")?;
                    let menu_x = app.get_composer_model_menu_x();
                    let menu_y = app.get_composer_model_menu_y();
                    assert_state(
                        app.get_composer_model_menu_width() > 0.0
                            && app.get_composer_model_menu_height() > 0.0
                            && menu_x >= 0.0
                            && menu_y >= 0.0,
                        "model popover geometry is not usable for pointer input",
                    )?;
                    // Row geometry mirrors composer.slint: 8px popover padding
                    // + 20px context row + 6px spacing + 1px divider + 6px
                    // spacing above row 0, then 40px per row.
                    let index = app.get_composer_models().iter()
                        .position(|model| model.key == "threadless-model")
                        .ok_or("second model missing from the composer catalogue")?;
                    crate::smoke::click(&app, menu_x + 150.0, menu_y + 61.0 + 40.0 * index as f32)?;
                    phase = 15;
                }
                15 => {
                    assert_state(!app.get_composer_model_open(), "model picker stayed open after selecting a row")?;
                    let session = api.get_session(app.get_active_session_id().as_str())?.0;
                    if session.session_model_name.as_deref() != Some("threadless-model") { return Ok(false) }
                    assert_state(app.get_selected_model_key() == "threadless-model", "row click did not reach the composer label")?;
                    // Without an active thread the picker must still switch:
                    // the local agent follows the global default, so the pick
                    // lands there instead of dying silently.
                    app.set_active_session_id("".into());
                    app.set_selected_model_key("".into());
                    phase = 16;
                }
                16 => {
                    let x = app.get_composer_model_trigger_x() + app.get_composer_model_trigger_width() / 2.0;
                    let y = app.get_composer_model_trigger_y() + app.get_composer_model_trigger_height() / 2.0;
                    crate::smoke::click(&app, x, y)?;
                    phase = 17;
                }
                17 => {
                    assert_state(app.get_composer_model_open(), "threadless model trigger did not open the picker")?;
                    let menu_x = app.get_composer_model_menu_x();
                    let menu_y = app.get_composer_model_menu_y();
                    let index = app.get_composer_models().iter()
                        .position(|model| model.key == "threadless-model")
                        .ok_or("second model missing from the composer catalogue")?;
                    crate::smoke::click(&app, menu_x + 150.0, menu_y + 61.0 + 40.0 * index as f32)?;
                    phase = 18;
                }
                18 => {
                    let alt = app.get_models().iter().find(|model| model.key == "threadless-model");
                    if !alt.as_ref().is_some_and(|model| model.is_default) { return Ok(false) }
                    assert_state(app.get_selected_model_key() == "", "threadless switch left a thread pin in the label")?;
                    if app.get_selected_agent_model() != "threadless-model" { return Ok(false) }
                    crate::smoke::snapshot(&app, &directory.join("model-composer.png"))?;
                    return Ok(true);
                }
                _ => unreachable!(),
            }
            Ok(false)
        })();
        if matches!(checked, Ok(false)) { return; }
        let report = match &checked {
            Ok(_) => "PASS: model add-persists/catalogue/unsaved-guard/validation/default-promotion/rename/picker-click/threadless-default\n".to_owned(),
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
