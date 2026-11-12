//! First-run model setup smoke check: a fresh runtime holds only the seeded
//! 虚拟模型, so the chat surface must offer the setup banner; one real click
//! opens the model panel with a fresh draft, and saving it clears the banner.

use crate::MainWindow;
use slint::{ComponentHandle, Model, Timer, TimerMode};
use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

pub fn run(app: &MainWindow, directory: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let result = Rc::new(RefCell::new(None));
    let output = result.clone();
    let weak = app.as_weak();
    let timer = Timer::default();
    let started = Instant::now();
    let mut phase = 0;
    timer.start(TimerMode::Repeated, Duration::from_millis(40), move || {
        let Some(app) = weak.upgrade() else { return };
        let checked = (|| -> Result<bool, Box<dyn std::error::Error>> {
            if started.elapsed() > Duration::from_secs(60) {
                return Err(format!("model setup smoke timed out in phase {phase}").into());
            }
            if app.get_chat_loading()
                || app.get_saving()
                || app.get_settings_loading()
                || app.get_agents_loading()
                || app.get_session_loading()
            {
                return Ok(false);
            }
            match phase {
                0 => {
                    assert_state(
                        app.get_needs_model_setup(),
                        "a fresh runtime must offer the model setup banner",
                    )?;
                    assert_state(
                        app.get_composer_models().row_count() == 1,
                        "a fresh catalogue must hold only the seeded virtual model",
                    )?;
                    crate::smoke::snapshot(&app, &directory.join("setup-banner.png"))?;
                    let x =
                        app.get_model_setup_banner_x() + app.get_model_setup_banner_width() / 2.0;
                    let y =
                        app.get_model_setup_banner_y() + app.get_model_setup_banner_height() / 2.0;
                    assert_state(
                        x > 0.0 && y > 0.0,
                        "setup banner geometry is not usable for pointer input",
                    )?;
                    crate::smoke::click(&app, x, y)?;
                    phase = 1;
                }
                1 => {
                    assert_state(
                        app.get_settings_open(),
                        "banner click did not open settings",
                    )?;
                    assert_state(
                        app.get_settings_active_panel() == 1,
                        "banner click did not land on the model panel",
                    )?;
                    assert_state(
                        app.get_model_creating(),
                        "banner click did not create a model draft",
                    )?;
                    assert_state(
                        app.get_settings_model_key() == "新模型",
                        "the auto-created draft is not a fresh model",
                    )?;
                    app.set_model_key_draft("first-model".into());
                    app.set_model_name_draft("sample-model".into());
                    app.invoke_save_model();
                    phase = 2;
                }
                2 => {
                    let saved = app
                        .get_models()
                        .iter()
                        .any(|model| model.key == "first-model");
                    if app.get_model_creating() || !saved {
                        return Ok(false);
                    }
                    assert_state(
                        !app.get_needs_model_setup(),
                        "the setup banner stayed on after a real model was saved",
                    )?;
                    assert_state(
                        app.get_models()
                            .iter()
                            .any(|model| model.key == "first-model" && model.is_default),
                        "the first real model did not take over the default seat",
                    )?;
                    app.set_settings_open(false);
                    phase = 3;
                }
                3 => {
                    crate::smoke::snapshot(&app, &directory.join("setup-done.png"))?;
                    return Ok(true);
                }
                _ => unreachable!(),
            }
            Ok(false)
        })();
        if matches!(checked, Ok(false)) {
            return;
        }
        let report = match &checked {
            Ok(_) => "PASS: model-setup banner/click-opens-draft/save-clears-banner\n".to_owned(),
            Err(error) => format!("FAIL: {error}\n"),
        };
        *output.borrow_mut() = Some(
            std::fs::write(directory.join("smoke.txt"), report)
                .map_err(|error| error.to_string())
                .and(checked.map(|_| ()).map_err(|error| error.to_string())),
        );
        let _ = slint::quit_event_loop();
    });
    app.run()?;
    timer.stop();
    result
        .borrow_mut()
        .take()
        .ok_or("model setup smoke did not run")??;
    Ok(())
}

fn assert_state(condition: bool, message: &str) -> Result<(), Box<dyn std::error::Error>> {
    crate::native_smoke::ensure(condition, message)
}
