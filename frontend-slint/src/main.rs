#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();
mod activity_ui;
mod agent_editor;
mod audio_recording;
pub(crate) mod avatar_assets;
mod avatar_ui;
mod channel_ui;
mod chat_process;
mod cloud_threads;
mod code_highlight;
mod companion_pet;
mod companion_sprite;
mod cron_ui;
mod demo;
mod demo_entities;
mod entity_state;
mod expert_ui;
mod file_dialog;
mod file_icons;
mod help_browser;
mod hotkey;
mod message_blocks;
mod native_chat;
mod native_interlink;
mod native_markdown;
mod native_pages;
mod native_restore;
mod native_runtime;
mod native_smoke;
mod native_window;
mod navigation_ui;
mod os_drag;
mod pet_settings;
mod pet_window;
mod runtime_settings;
mod screen_capture;
mod screenshot;
mod settings_search;
mod shutdown;
mod smoke;
mod subagent_detail;
mod thread_trajectory;
mod timeline;
mod timeline_text;
mod tool_icons;
mod tools_ui;
mod touch_keyboard;
mod tray;
mod turn_stats;
mod workspace_files;
mod workspace_ui;

static STARTUP_BOOT: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

fn main() -> Result<(), Box<dyn std::error::Error>> {
    wunder_desktop::install_local_home_env();
    let startup_boot = *STARTUP_BOOT.get_or_init(std::time::Instant::now);
    startup_log("process_begin", startup_boot);
    install_slint_platform()?;
    startup_log("slint_platform_ready", startup_boot);
    register_bundled_font()?;
    startup_log("font_ready", startup_boot);

    let mut arguments = std::env::args_os().skip(1);
    let first_argument = arguments.next();
    let app = MainWindow::new()?;
    startup_log("main_window_new", startup_boot);
    install_composer_popover_dismissal(&app);
    install_first_frame_probe(startup_boot);
    let close_weak = app.as_weak();
    app.window().on_close_requested(move || {
        if let Some(app) = close_weak.upgrade() {
            if crate::tray::hide(&app) {
                return slint::CloseRequestResponse::KeepWindowShown;
            }
        }
        crate::shutdown::request_exit();
        slint::CloseRequestResponse::KeepWindowShown
    });
    let weak = app.as_weak();
    app.on_composer_focus_gained(move || {
        if let Some(app) = weak.upgrade() {
            touch_keyboard::on_text_field_activated(app.window());
        }
    });
    install_window_chrome(&app);
    if first_argument.as_deref() == Some(std::ffi::OsStr::new("--smoke-check")) {
        demo::install(&app);
        let directory = arguments
            .next()
            .ok_or("missing smoke-check output directory")?;
        return smoke::run(&app, std::path::PathBuf::from(directory));
    }
    // 帮助窗口按需弹出：设置页「常规」的按钮触发 open-help。
    let help_weak = app.as_weak();
    app.on_open_help(move || {
        if let Some(app) = help_weak.upgrade() {
            help_browser::open(&app);
        }
    });
    if first_argument.as_deref() == Some(std::ffi::OsStr::new("--timeline-probe")) {
        // §十二.2: measure the worst-case timeline on the real renderer without
        // depending on the rest of the smoke check.
        let directory = arguments
            .next()
            .ok_or("missing timeline-probe output directory")?;
        return smoke::run_timeline_probe(&app, std::path::PathBuf::from(directory));
    }
    if first_argument.as_deref() == Some(std::ffi::OsStr::new("--hide-probe")) {
        // The hide verdict can only be measured on a window the screen really
        // shows, so the probe starts the same session production starts.
        native_runtime::install(&app);
        tray::install(&app);
        app.show()?;
        startup_log("window_show_return", startup_boot);
        fit_and_center_main_window(&app);
        return screenshot::run_hide_probe(&app);
    }
    if matches!(
        first_argument.as_deref().and_then(std::ffi::OsStr::to_str),
        Some("--native-smoke" | "--native-restore" | "--native-reasoning-smoke")
    ) {
        let directory = std::path::PathBuf::from(
            arguments
                .next()
                .ok_or("missing isolated native directory")?,
        );
        if !directory.join("runtime/config/wunder.yaml").is_file() {
            return Err("native smoke requires an explicitly prepared isolated runtime".into());
        }
        let mut args = wunder_desktop::args::DesktopArgs::native_defaults();
        args.temp_root = Some(directory.join("runtime").canonicalize()?);
        let restoring = first_argument.as_deref() == Some(std::ffi::OsStr::new("--native-restore"));
        let reasoning =
            first_argument.as_deref() == Some(std::ffi::OsStr::new("--native-reasoning-smoke"));
        if !restoring {
            args.workspace = Some(directory.join("workspace"));
        }
        let runtime = std::sync::Arc::new(wunder_desktop::NativeDesktop::start_with_args(args)?);
        // The restore phase deliberately runs a read-only guard: the streaming
        // phase's guard writes probe fixtures, and a check that mutates the
        // store cannot prove what survived the restart.
        if !restoring && !reasoning {
            native_smoke::check_runtime(&runtime)?;
        }
        native_runtime::install_ready_without_pets(&app, runtime.clone());
        app.show()?;
        startup_log("window_show_return", startup_boot);
        return if reasoning {
            native_smoke::run_reasoning(&app, directory)
        } else if restoring {
            native_restore::run(&app, directory, runtime)
        } else {
            native_smoke::run(&app, directory)
        };
    }
    match first_argument.as_deref().and_then(std::ffi::OsStr::to_str) {
        Some("--demo") => demo::install(&app),
        None | Some("--native") => native_runtime::install(&app),
        Some(_) => return Err("旧 bridge 参数已移除；请直接启动桌面程序".into()),
    }
    tray::install(&app);
    app.show()?;
    startup_log("window_show_return", startup_boot);
    fit_and_center_main_window(&app);
    app.run()?;
    Ok(())
}

/// Close the composer menus when the pointer is pressed elsewhere, while still
/// allowing that same press to reach the real Slint control underneath. A
/// full-window transparent TouchArea would consume the first click and make
/// sidebars, threads and the editor feel one click behind, so this filter only
/// performs a few geometry checks on press events.
fn install_composer_popover_dismissal(app: &MainWindow) {
    use slint::winit_030::{winit, EventResult, WinitWindowAccessor};
    use std::cell::Cell;
    use std::rc::Rc;

    let weak = app.as_weak();
    let last_physical_pointer = Rc::new(Cell::new(None::<(f64, f64)>));
    let pointer_for_filter = Rc::clone(&last_physical_pointer);
    app.window().on_winit_window_event(move |window, event| {
        // The touch-keyboard gate reads this before Slint dispatches the
        // press that may move focus into a text field.
        touch_keyboard::note_winit_event(event);
        let touch_started = matches!(
            event,
            winit::event::WindowEvent::Touch(touch)
                if touch.phase == winit::event::TouchPhase::Started
        );
        let scale = f64::from(window.scale_factor()).max(0.001);
        match event {
            winit::event::WindowEvent::CursorMoved { position, .. } => {
                pointer_for_filter.set(Some((position.x, position.y)));
                return EventResult::Propagate;
            }
            winit::event::WindowEvent::MouseInput {
                state: winit::event::ElementState::Pressed,
                button: winit::event::MouseButton::Left,
                ..
            } => {}
            winit::event::WindowEvent::Touch(touch)
                if touch.phase == winit::event::TouchPhase::Started =>
            {
                pointer_for_filter.set(Some((touch.location.x, touch.location.y)));
            }
            _ => return EventResult::Propagate,
        }

        let Some((physical_x, physical_y)) = pointer_for_filter.get() else {
            return EventResult::Propagate;
        };
        let point = (physical_x / scale as f64) as f32;
        let point_y = (physical_y / scale as f64) as f32;
        let Some(app) = weak.upgrade() else {
            return EventResult::Propagate;
        };
        // A focused editor does not emit another focus-change callback. Handle
        // its direct finger press here as well, so dismissing TabTip and
        // tapping the same input area raises it again. This runs before Slint
        // consumes the touch and never intercepts the edit gesture.
        let in_editor = point >= app.get_composer_editor_x()
            && point < app.get_composer_editor_x() + app.get_composer_editor_width()
            && point_y >= app.get_composer_editor_y()
            && point_y < app.get_composer_editor_y() + app.get_composer_editor_height();
        if touch_started && in_editor && app.get_composer_editor_has_focus() {
            touch_keyboard::on_text_field_activated(window);
        }
        if !app.get_composer_popover_open() {
            return EventResult::Propagate;
        }

        let inside = |x: f32, y: f32, width: f32, height: f32| {
            width > 0.0
                && height > 0.0
                && point >= x
                && point < x + width
                && point_y >= y
                && point_y < y + height
        };
        let in_plus_trigger = inside(
            app.get_composer_plus_trigger_x(),
            app.get_composer_plus_trigger_y(),
            app.get_composer_plus_trigger_width(),
            app.get_composer_plus_trigger_height(),
        );
        let in_model_trigger = inside(
            app.get_composer_model_trigger_x(),
            app.get_composer_model_trigger_y(),
            app.get_composer_model_trigger_width(),
            app.get_composer_model_trigger_height(),
        );
        let in_model_menu = inside(
            app.get_composer_model_menu_x(),
            app.get_composer_model_menu_y(),
            app.get_composer_model_menu_width(),
            app.get_composer_model_menu_height(),
        );
        let in_plus_menu = inside(
            app.get_composer_plus_menu_x(),
            app.get_composer_plus_menu_y(),
            app.get_composer_plus_menu_width(),
            app.get_composer_plus_menu_height(),
        );
        let in_active_flyout = inside(
            app.get_composer_command_menu_x(),
            app.get_composer_command_menu_y(),
            app.get_composer_command_menu_width(),
            app.get_composer_command_menu_height(),
        ) || inside(
            app.get_composer_preset_menu_x(),
            app.get_composer_preset_menu_y(),
            app.get_composer_preset_menu_width(),
            app.get_composer_preset_menu_height(),
        ) || inside(
            app.get_composer_permission_menu_x(),
            app.get_composer_permission_menu_y(),
            app.get_composer_permission_menu_width(),
            app.get_composer_permission_menu_height(),
        ) || inside(
            app.get_composer_screenshot_menu_x(),
            app.get_composer_screenshot_menu_y(),
            app.get_composer_screenshot_menu_width(),
            app.get_composer_screenshot_menu_height(),
        );

        // Keep clicks on the currently open popover and its trigger intact.
        // The other trigger is deliberately treated as outside: closing the
        // old popover first lets the same click open the newly selected one.
        let keep_open = if app.get_composer_model_open() {
            in_model_menu || in_model_trigger
        } else {
            in_plus_menu || in_active_flyout || in_plus_trigger
        };
        if !keep_open {
            app.invoke_dismiss_composer_popovers();
        }
        EventResult::Propagate
    });
}

/// Custom-chrome gestures for the frameless workbench window. The close
/// button shares the window-X path: hide to the tray when one is available.
fn install_window_chrome(app: &MainWindow) {
    let weak = app.as_weak();
    app.on_window_drag_requested(move || {
        if let Some(app) = weak.upgrade() {
            native_window::begin_system_move(app.window());
        }
    });
    let weak = app.as_weak();
    app.on_window_minimize_requested(move || {
        if let Some(app) = weak.upgrade() {
            native_window::minimize(app.window());
        }
    });
    let weak = app.as_weak();
    app.on_window_maximize_requested(move || {
        if let Some(app) = weak.upgrade() {
            native_window::toggle_maximize(app.window());
        }
    });
    let weak = app.as_weak();
    app.on_window_close_requested(move || {
        if let Some(app) = weak.upgrade() {
            if crate::tray::hide(&app) {
                return;
            }
        }
        crate::shutdown::request_exit();
    });
}

fn startup_timing_enabled() -> bool {
    matches!(
        std::env::var("WUNDER_STARTUP_TIMING").ok().as_deref(),
        Some("1" | "true" | "yes")
    )
}

fn startup_log(point: &str, boot: std::time::Instant) {
    if startup_timing_enabled() {
        eprintln!(
            "[startup][desktop-ui] point={point} total_ms={:.1}",
            boot.elapsed().as_secs_f64() * 1000.0
        );
    }
}

pub(crate) fn startup_point(point: &str) {
    if let Some(boot) = STARTUP_BOOT.get() {
        startup_log(point, *boot);
    }
}

fn install_first_frame_probe(boot: std::time::Instant) {
    if !startup_timing_enabled() {
        return;
    }
    // Event-loop entry and native presentation are separate milestones. The
    // vendored software backend signals only a successful, non-empty present.
    slint::Timer::single_shot(std::time::Duration::from_millis(0), move || {
        eprintln!(
            "[startup][desktop-ui] point=first_event_loop total_ms={:.1}",
            boot.elapsed().as_secs_f64() * 1000.0
        );
    });
    i_slint_backend_winit::first_frame::on_first_frame_presented(move || {
        startup_log("first_frame_presented", boot);
    });
}

/// Fit the workbench to a compact size and center it on the active monitor.
///
/// Slint creates the window before the monitor work area is known, and some
/// window managers ignore geometry requests while it is still withdrawn. Retry
/// briefly after the first map, then leave the window alone so a later resize
/// or move by the user is never snapped back.
fn fit_and_center_main_window(app: &MainWindow) {
    use slint::winit_030::{winit, WinitWindowAccessor};
    use std::cell::Cell;
    use std::rc::Rc;

    let weak = app.as_weak();
    let timer = Rc::new(slint::Timer::default());
    let timer_handle = Rc::clone(&timer);
    let attempts = Rc::new(Cell::new(0u32));
    let attempts_handle = Rc::clone(&attempts);
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(50),
        move || {
            attempts_handle.set(attempts_handle.get().saturating_add(1));
            let applied = weak
                .upgrade()
                .map(|app| {
                    let area = native_window::work_area(app.window());
                    app.window()
                        .with_winit_window(|native| {
                            if !native.is_visible().unwrap_or(false) {
                                return false;
                            }
                            let Some(monitor) = native
                                .current_monitor()
                                .or_else(|| native.primary_monitor())
                            else {
                                return false;
                            };
                            let scale = monitor.scale_factor();
                            let monitor_position = monitor.position();
                            let monitor_size = monitor.size();
                            let (area_x, area_y, area_w, area_h) = area.unwrap_or((
                                monitor_position.x,
                                monitor_position.y,
                                monitor_size.width as i32,
                                monitor_size.height as i32,
                            ));
                            if area_w <= 0 || area_h <= 0 {
                                return false;
                            }
                            let min_w = (1024.0 * scale).round() as i32;
                            let min_h = (700.0 * scale).round() as i32;
                            let max_w = (1280.0 * scale).round() as i32;
                            let max_h = (820.0 * scale).round() as i32;
                            let target_w = ((area_w as f64 * 0.8).round() as i32)
                                .min(max_w)
                                .max(min_w)
                                .min(area_w);
                            let target_h = ((area_h as f64 * 0.8).round() as i32)
                                .min(max_h)
                                .max(min_h)
                                .min(area_h);
                            let x = area_x + (area_w - target_w) / 2;
                            let y = area_y + (area_h - target_h) / 2;
                            let _ = native.request_inner_size(winit::dpi::PhysicalSize::new(
                                target_w as u32,
                                target_h as u32,
                            ));
                            native.set_outer_position(winit::dpi::PhysicalPosition::new(x, y));
                            true
                        })
                        .unwrap_or(false)
                })
                .unwrap_or(true);
            if applied || attempts_handle.get() >= 100 {
                timer_handle.stop();
            }
        },
    );
}

/// Register the compressed Microsoft YaHei before the first text layout.
/// The bundled TTC keeps the UI independent of fonts installed on the target
/// machine while matching the system default CJK face the preview path uses.
pub(crate) fn register_bundled_font() -> Result<(), Box<dyn std::error::Error>> {
    use slint::fontique_011::fontique;

    let mut collection = slint::fontique_011::shared_collection();
    startup_point("font_collection_ready");
    let bytes = bundled_font::yahei_ttc();
    startup_point("font_decompressed");
    let registered =
        collection.register_fonts(fontique::Blob::new(std::sync::Arc::new(bytes)), None);
    startup_point("font_registered");
    let families: Vec<_> = registered.iter().map(|(family, _)| *family).collect();
    if families.is_empty() {
        return Err("内嵌 Microsoft YaHei 注册失败".into());
    }
    for generic in [
        fontique::GenericFamily::SansSerif,
        fontique::GenericFamily::SystemUi,
        fontique::GenericFamily::UiSansSerif,
    ] {
        collection.set_generic_families(generic, families.iter().copied());
    }
    collection.append_fallbacks(
        fontique::FallbackKey::new(fontique::Script::from_str_unchecked("Hani"), None),
        families.iter().copied(),
    );
    Ok(())
}

mod bundled_font {
    static YAHEI_LZ4: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/msyh.ttc.lz4"));

    pub fn yahei_ttc() -> &'static [u8] {
        static DECOMPRESSED: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
        DECOMPRESSED
            .get_or_init(|| {
                lz4_flex::decompress_size_prepended(YAHEI_LZ4)
                    .expect("内嵌 Microsoft YaHei 解压失败")
            })
            .as_slice()
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    //! Headless UI tests share one MinimalSoftwareWindow platform and one
    //! global lock: Slint installs the platform once per process and window
    //! construction must not race across test threads.
    use std::sync::{Mutex, MutexGuard, Once};

    static LOCK: Mutex<()> = Mutex::new(());
    static PLATFORM: Once = Once::new();

    pub(crate) fn lock() -> MutexGuard<'static, ()> {
        PLATFORM.call_once(|| {
            struct TestPlatform;
            impl slint::platform::Platform for TestPlatform {
                fn create_window_adapter(
                    &self,
                ) -> Result<std::rc::Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError>
                {
                    Ok(
                        slint::platform::software_renderer::MinimalSoftwareWindow::new(
                            slint::platform::software_renderer::RepaintBufferType::NewBuffer,
                        ),
                    )
                }
            }
            let _ = slint::platform::set_platform(Box::new(TestPlatform));
        });
        LOCK.lock().unwrap_or_else(|poison| poison.into_inner())
    }
}

fn install_slint_platform() -> Result<(), Box<dyn std::error::Error>> {
    // The workbench draws its own WindowChrome; auxiliary top-levels (tray
    // notice, screenshot selector, help, terminal) keep the system frame.
    let frameless_first_window = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let backend = i_slint_backend_winit::Backend::builder()
        .with_renderer_name("software")
        .with_window_attributes_hook({
            let frameless_first_window = std::sync::Arc::clone(&frameless_first_window);
            move |attributes| {
                let frameless =
                    frameless_first_window.swap(false, std::sync::atomic::Ordering::AcqRel);
                let attributes = attributes
                    .with_min_inner_size(winit::dpi::LogicalSize::new(1024.0, 700.0))
                    .with_title("蛹")
                    .with_decorations(!frameless)
                    // A frameless window owns its caption buttons in client
                    // content; a latent system button can surface after the
                    // first minimize/maximize transition on Win7 otherwise.
                    .with_enabled_buttons(if frameless {
                        winit::window::WindowButtons::empty()
                    } else {
                        winit::window::WindowButtons::all()
                    });
                attributes
            }
        })
        .build()?;
    slint::platform::set_platform(Box::new(backend))
        .map_err(|error| format!("初始化 Slint Win7 软件渲染平台失败：{error:?}"))?;
    Ok(())
}
