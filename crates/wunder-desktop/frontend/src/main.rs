#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();
mod agent_editor;
mod audio_recording;
pub(crate) mod avatar_assets;
mod avatar_ui;
mod backup_ui;
mod channel_ui;
mod chat_process;
mod chat_split;
mod clipboard_image;
mod cloud_threads;
mod code_highlight;
mod command_tasks;
mod companion_pet;
mod companion_sprite;
mod completion_notice;
mod crash_log;
mod cron_ui;
mod demo;
mod demo_entities;
mod entity_state;
mod error_card;
mod expert_ui;
mod feedback;
mod file_dialog;
mod file_icons;
mod help_browser;
mod host_hud;
mod hotkey;
mod image_attachment;
mod md;
mod md_probe;
mod message_blocks;
mod model_editor;
mod model_editor_smoke;
mod model_setup_smoke;
mod native_chat;

mod command_probe;
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
mod pointer_chase;
mod runtime_phase;
mod runtime_settings;
mod screen_capture;
mod screenshot;
mod settings_search;
mod shutdown;
mod single_instance;
mod smoke;
mod subagent_detail;
mod subagent_dock;
mod thread_background;
mod thread_trajectory;
mod timeline;
mod timeline_text;
mod tools_ui;
mod touch_keyboard;
mod tray;
mod turn_stats;
mod ui_language;
mod update_ui;
mod workspace_files;
mod workspace_ui;

static STARTUP_BOOT: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// Claim the single-instance slot. A relaunch after an in-place update
/// (`WUNDER_UPDATE_RELAUNCH`) retries briefly: the previous process may still
/// be shutting down while the replacement binary starts, and that handover
/// must not be mistaken for a duplicate launch.
fn claim_for_launch() -> std::io::Result<single_instance::Launch> {
    let relaunch = std::env::var(wunder_desktop::UPDATE_RELAUNCH_ENV)
        .ok()
        .as_deref()
        == Some("1");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        match single_instance::claim()? {
            single_instance::Launch::Activated
                if relaunch && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            launch => return Ok(launch),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 必须最先安装：发布版 panic=abort，任何后续初始化的 panic 都要求
    // 先有崩溃日志落点，闪退才能事后定位。
    crash_log::install();
    // 多 MB 的请求体与图片像素缓冲释放后 glibc 默认留在 arena 里不还给系统；
    // 先收窄 arena 并把大块固定走 mmap，峰值过后 RSS 才能落回来。
    wunder_server::runtime_tuning::tune_allocator_for_large_buffers();
    let mut arguments = std::env::args_os().skip(1);
    let first_argument = arguments.next();
    let mut instance = if matches!(
        first_argument.as_deref().and_then(std::ffi::OsStr::to_str),
        None | Some("--native" | "--demo")
    ) {
        match claim_for_launch()? {
            single_instance::Launch::Primary(instance) => Some(instance),
            single_instance::Launch::Activated => return Ok(()),
        }
    } else {
        None
    };
    wunder_desktop::install_local_home_env();
    // 数据与恢复的「恢复」只解包到暂存目录；真正替换 config 与数据库必须在
    // 任何 SQLite 连接打开之前完成，所以放在这里、窗口创建之前。
    wunder_desktop::native_backup::apply_pending_restore();
    rotate_startup_log();
    let startup_boot = *STARTUP_BOOT.get_or_init(std::time::Instant::now);
    // 引擎内部的阶段耗时默认只走 tracing，桌面版没有 subscriber；注册后同一批
    // 里程碑也写进启动日志，卡在哪个阶段可以直接读出来。
    wunder_server::startup_probe::install(engine_startup_point);
    startup_log("process_begin", startup_boot);
    install_slint_platform()?;
    startup_log("slint_platform_ready", startup_boot);
    // Wayland/X11 任务栏按 app_id 匹配 .desktop 文件；不设置时只能拿到默认
    // 程序图标，即使窗口自带的 icon 属性正常。
    #[cfg(target_os = "linux")]
    if let Err(error) = slint::set_xdg_app_id("wunder") {
        startup_log(&format!("xdg_app_id_failed: {error}"), startup_boot);
    }
    register_bundled_font()?;
    startup_log("font_ready", startup_boot);

    let app = MainWindow::new()?;
    startup_log("main_window_new", startup_boot);
    // 界面语言：窗口把 `runtime-language` 派生成 `ui-zh` 后回传，这里落到
    // `I18n.zh`。先装回调，任何设置投影（启动加载 / 设置页保存）都会即时生效。
    ui_language::install(&app);
    // 操作结果只有 `status` 一个落点，而没有任何视图渲染它：这里把每次写入
    // 交给 Rust 分类后再决定是否弹提示，成功和失败都不会再悄无声息。
    feedback::install(&app);
    // 语言投影探针（`WUNDER_LANG_PROBE=<dir> --demo`）：强制一次英文/中文投影并
    // 截图退出，用于人工复核两套文案，不进入发布路径。
    if ui_language::probe::run(&app) {
        app.show()?;
        app.run()?;
        return Ok(());
    }
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
    app.global::<TextFieldActivation>().on_activated(move || {
        if let Some(app) = weak.upgrade() {
            touch_keyboard::on_text_field_activated(app.window());
        }
    });
    install_window_chrome(&app);
    // 拖拽手势的指针追踪通道（见 pointer_chase）：事件流在 32 位 Win7 上会
    // 中途断掉，行/宽度拖拽要有第二条位置来源。
    pointer_chase::install(&app);
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
    // Transcript context menu: the selection lives in the text layout, so the
    // two actions have to run through the same core entry points the keyboard
    // shortcuts use (Ctrl+C / Ctrl+A).
    let menu_weak = app.as_weak();
    app.on_copy_selection(move || {
        if let Some(app) = menu_weak.upgrade() {
            i_slint_core::text_selection::copy_context(&app.window());
        }
    });
    let select_weak = app.as_weak();
    app.on_select_all_text(move || {
        let _ = select_weak;
        i_slint_core::text_selection::select_all();
    });
    if first_argument.as_deref() == Some(std::ffi::OsStr::new("--command-fold-probe")) {
        // One expanded command entry, so the capped detail panel can be looked
        // at instead of reasoned about.
        let directory = arguments
            .next()
            .ok_or("missing command-fold-probe output directory")?;
        return command_probe::run(&app, std::path::PathBuf::from(directory));
    }
    if first_argument.as_deref() == Some(std::ffi::OsStr::new("--timeline-probe")) {
        // §十二.2: measure the worst-case timeline on the real renderer without
        // depending on the rest of the smoke check.
        let directory = arguments
            .next()
            .ok_or("missing timeline-probe output directory")?;
        return smoke::run_timeline_probe(&app, std::path::PathBuf::from(directory));
    }
    if first_argument.as_deref() == Some(std::ffi::OsStr::new("--scroll-probe")) {
        let directory = arguments
            .next()
            .ok_or("missing scroll-probe output directory")?;
        return smoke::run_scroll_probe(&app, std::path::PathBuf::from(directory));
    }
    if first_argument.as_deref() == Some(std::ffi::OsStr::new("--scroll-restore-probe")) {
        // The transcript is one viewport for whichever thread is on screen;
        // this proves a switch lands each thread on its own position.
        let directory = arguments
            .next()
            .ok_or("missing scroll-restore-probe output directory")?;
        return smoke::run_scroll_restore_probe(&app, std::path::PathBuf::from(directory));
    }
    if first_argument.as_deref() == Some(std::ffi::OsStr::new("--list-probe")) {
        // ListView-specific scroll probe: virtualization shifts work into the
        // frame that reveals each row.
        let directory = arguments
            .next()
            .ok_or("missing list-probe output directory")?;
        return smoke::run_list_probe(&app, std::path::PathBuf::from(directory));
    }
    if first_argument.as_deref() == Some(std::ffi::OsStr::new("--markdown-probe")) {
        // Render one document that covers every block kind, for a layout check
        // that does not depend on the rest of the shell.
        let directory = arguments
            .next()
            .ok_or("missing markdown-probe output directory")?;
        return md_probe::run(&app, std::path::PathBuf::from(directory));
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
        Some(
            "--native-smoke"
                | "--native-restore"
                | "--native-reasoning-smoke"
                | "--native-model-smoke"
                | "--native-setup-smoke"
                | "--native-split-smoke"
                | "--native-queue-smoke"
        )
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
        let models =
            first_argument.as_deref() == Some(std::ffi::OsStr::new("--native-model-smoke"));
        let setup = first_argument.as_deref() == Some(std::ffi::OsStr::new("--native-setup-smoke"));
        let splitting =
            first_argument.as_deref() == Some(std::ffi::OsStr::new("--native-split-smoke"));
        let queueing =
            first_argument.as_deref() == Some(std::ffi::OsStr::new("--native-queue-smoke"));
        if !restoring {
            args.workspace = Some(directory.join("workspace"));
        }
        let runtime = std::sync::Arc::new(wunder_desktop::NativeDesktop::start_with_args(args)?);
        if splitting {
            for _ in 0..3 {
                runtime.create_session(None)?;
            }
        }
        // The restore phase deliberately runs a read-only guard: the streaming
        // phase's guard writes probe fixtures, and a check that mutates the
        // store cannot prove what survived the restart.
        if !restoring && !reasoning && !models && !setup && !splitting && !queueing {
            native_smoke::check_runtime(&runtime)?;
        }
        native_runtime::install_ready_without_pets(&app, runtime.clone());
        app.show()?;
        startup_log("window_show_return", startup_boot);
        return if setup {
            model_setup_smoke::run(&app, directory)
        } else if models {
            model_editor_smoke::run(&app, directory, runtime)
        } else if reasoning {
            native_smoke::run_reasoning(&app, directory)
        } else if splitting {
            native_smoke::run_split(&app, directory)
        } else if queueing {
            native_smoke::run_queue(&app, directory)
        } else if restoring {
            native_restore::run(&app, directory, runtime)
        } else {
            native_smoke::run(&app, directory)
        };
    }
    match first_argument.as_deref().and_then(std::ffi::OsStr::to_str) {
        Some("--demo") => demo::install(&app),
        None | Some("--native") => {
            native_runtime::install(&app);
            // 设置 → 数据与恢复：只读写本机文件，装在做真实运行时的形态上。
            backup_ui::install(&app);
        }
        Some(_) => return Err("旧 bridge 参数已移除；请直接启动桌面程序".into()),
    }
    tray::install(&app);
    // 截图快捷键：F1 先隐藏窗口再截图，F2 保留窗口截图。装在窗口层，
    // 因此不依赖托盘是否可用。
    screenshot::install_shortcuts(&app);
    // 输入区底部的本机硬件状态网格。采样在独立线程，这里只装定时投影，
    // 因此放在窗口显示之前即可，首帧显示占位符、约一秒后落成真实读数。
    host_hud::install(&app);
    install_startup_stage_notes(&app);
    app.show()?;
    startup_log("window_show_return", startup_boot);
    fit_and_center_main_window(&app);
    if let Some(instance) = instance.as_mut() {
        #[cfg(windows)]
        instance.register_window(&app)?;
        let weak = app.as_weak();
        let pending = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        instance.listen(move || {
            use std::sync::atomic::Ordering;
            if pending.swap(true, Ordering::AcqRel) {
                return;
            }
            let completed = pending.clone();
            if weak
                .upgrade_in_event_loop(move |app| {
                    completed.store(false, Ordering::Release);
                    tray::show_main(&app);
                })
                .is_err()
            {
                pending.store(false, Ordering::Release);
            }
        })?;
    }
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
            winit::event::WindowEvent::Focused(true)
            | winit::event::WindowEvent::Occluded(false) => {
                let weak = weak.clone();
                slint::Timer::single_shot(std::time::Duration::ZERO, move || {
                    if let Some(app) = weak.upgrade() {
                        native_chat::resume_rendering(&app);
                    }
                });
                return EventResult::Propagate;
            }
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
            // Escape closes the topmost transient layer: composer popovers
            // (plus menu, model picker and their cascades) first, then the
            // settings sheet. Filter level, not a Slint key handler, because
            // key dispatch starts from the focus item and menu presses move
            // no focus — Escape would be dropped whenever nothing holds the
            // caret.
            winit::event::WindowEvent::KeyboardInput { event: key, .. }
                if !key.repeat
                    && key.state == winit::event::ElementState::Pressed
                    && matches!(
                        key.logical_key,
                        winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape)
                    ) =>
            {
                let Some(app) = weak.upgrade() else {
                    return EventResult::Propagate;
                };
                if app.get_composer_popover_open() {
                    app.invoke_dismiss_composer_popovers();
                    return EventResult::PreventDefault;
                }
                if app.get_settings_open() {
                    app.set_settings_open(false);
                    return EventResult::PreventDefault;
                }
                return EventResult::Propagate;
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
            app.get_composer_preset_menu_x(),
            app.get_composer_preset_menu_y(),
            app.get_composer_preset_menu_width(),
            app.get_composer_preset_menu_height(),
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

/// 启动里程碑默认落盘。桌面版没有控制台，也没有安装 tracing subscriber：
/// 不默认记录时，「老机器上打开后停在正在加载运行时」这类反馈就只剩一句描述、
/// 没有证据。默认开启，可用 `WUNDER_STARTUP_TIMING=0` 关掉（与运行时侧一致）。
fn startup_timing_enabled() -> bool {
    match std::env::var("WUNDER_STARTUP_TIMING")
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
    {
        Some(value) if matches!(value.as_str(), "0" | "false" | "off" | "no") => false,
        _ => true,
    }
}

/// 引擎侧的启动里程碑（存储初始化、技能加载、服务装配等）走同一条落点。
fn engine_startup_point(name: &str, total_ms: f64) {
    if let Some(boot) = STARTUP_BOOT.get() {
        startup_log(&format!("stage={} stage_ms={:.1}", name, total_ms), *boot);
    }
    if let Some(note) = startup_stage_note(name) {
        publish_startup_stage(note);
    }
}

/// 引擎里程碑对应的“接下来在做什么”，供启动横幅显示。老机器上引擎初始化要
/// 1 秒以上，横幅一直停在同一句话会被读成卡死；有了进度，等待至少是可见的。
fn startup_stage_note(stage: &str) -> Option<&'static str> {
    match stage {
        // 运行时初始化的粗粒度段：老机器上这几步各要几百毫秒到 1 秒。
        "resolve_paths_and_user" => Some("正在准备运行环境…"),
        "ensure_runtime_dirs" => Some("正在检查内置资源…"),
        "load_desktop_settings" => Some("正在读取桌面设置…"),
        "normalize_settings_and_save" => Some("正在整理桌面设置…"),
        "prepare_config_paths" | "apply_environment" => Some("正在应用运行环境…"),
        "config_store_update" => Some("正在读取配置文件…"),
        "lsp_sync_with_config" => Some("正在启动代码服务…"),
        "ensure_desktop_identity" => Some("正在确认本机身份…"),
        "desktop_runtime_recovery" => Some("正在恢复上次运行…"),
        "migrate_desktop_agent_approval_modes" | "migrate_local_workspaces" => {
            Some("正在升级本地数据…")
        }
        // AppState 内部的细粒度点。
        "new_begin" => Some("正在打开本地数据库…"),
        "storage_open_done" => Some("正在打开本地数据库…"),
        "thread_store_done" | "init_storage_done" => Some("正在读取会话索引…"),
        // 以下几步在慢盘上各占几十到几百毫秒，横幅逐段推进而不是停在
        // 同一句话，用户能看出是在走流程而不是卡住。
        "workspace_lsp_done" => Some("正在准备本地工作区…"),
        "monitor_done" => Some("正在接入运行监控…"),
        "load_skills_done" => Some("正在装配本地服务…"),
        "base_services_done" => Some("正在准备模型网关…"),
        "gateway_done" => Some("正在准备智能体…"),
        "orchestrator_done" => Some("正在准备任务调度…"),
        "thread_runtime_done" => Some("正在接入消息渠道…"),
        "channel_hub_done" => Some("正在恢复定时任务…"),
        "new_done" => Some("正在清理上次运行…"),
        "desktop:purge_thread_leases"
        | "desktop:settle_restarted_threads"
        | "desktop:settle_orphaned_tasks" => Some("正在收尾上次运行…"),
        _ => None,
    }
}

/// 当前启动阶段：(版本号, 提示文案)。引擎在后台线程改，UI 线程按帧读取。
static STARTUP_STAGE: std::sync::Mutex<(u64, &'static str)> = std::sync::Mutex::new((0, ""));
static STARTUP_STAGE_VERSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn publish_startup_stage(note: &'static str) {
    if let Ok(mut stage) = STARTUP_STAGE.lock() {
        *stage = (stage.0.wrapping_add(1), note);
    }
    STARTUP_STAGE_VERSION.fetch_add(1, std::sync::atomic::Ordering::Release);
}

/// 启动日志只保留最近两次运行：老机器反复启动时不会长成无界文件。
fn rotate_startup_log() {
    const LIMIT: u64 = 512 * 1024;
    let path = wunder_desktop::local_logs_dir().join("startup.log");
    let too_large = std::fs::metadata(&path)
        .map(|meta| meta.len() > LIMIT)
        .unwrap_or(false);
    if too_large {
        let _ = std::fs::rename(&path, path.with_extension("log.1"));
    }
}

/// 启动日志句柄。里程碑每次 open/close 一次的话，老机器上杀软会逐次扫描，
/// 记录本身反而拖慢启动；这里只开一次，之后每条只做一次追加写。
fn startup_log_file() -> Option<&'static std::sync::Mutex<std::fs::File>> {
    static FILE: std::sync::OnceLock<Option<std::sync::Mutex<std::fs::File>>> =
        std::sync::OnceLock::new();
    FILE.get_or_init(|| {
        let path = wunder_desktop::local_logs_dir().join("startup.log");
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok()
            .map(std::sync::Mutex::new)
    })
    .as_ref()
}

fn startup_log(point: &str, boot: std::time::Instant) {
    if !startup_timing_enabled() {
        return;
    }
    let line = format!(
        "[startup][desktop-ui] point={point} total_ms={:.1}",
        boot.elapsed().as_secs_f64() * 1000.0
    );
    eprintln!("{line}");
    // 桌面版是 GUI 进程：没有控制台，stderr 会随进程一起消失。启动里程碑同时
    // 落到 <home>/logs/startup.log，报障时只需提供这一个文件。
    use std::io::Write;
    let stamp = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f");
    if let Some(file) = startup_log_file() {
        if let Ok(mut file) = file.lock() {
            let _ = writeln!(file, "{stamp} {line}");
        }
    }
}

/// 把引擎阶段提示投影到启动横幅：只在工作台就绪前每 100ms 看一次版本号，
/// 就绪即停，不会覆盖后续文案。同一次 tick 里推进横幅底部的扫描条——引擎
/// 某个阶段在老机器上停超过一秒时，光有一句话会被读成卡死。
fn install_startup_stage_notes(app: &MainWindow) {
    use slint::ComponentHandle;
    use std::rc::Rc;
    use std::sync::atomic::Ordering;

    let weak = app.as_weak();
    let timer = Rc::new(slint::Timer::default());
    let timer_handle = Rc::clone(&timer);
    let applied = Rc::new(std::sync::atomic::AtomicU64::new(0));
    // 扫描条状态：位置 0..1，方向 +1/-1。往返而不是回卷，避免每次跳回左端。
    let scan = Rc::new(std::cell::Cell::<(f32, f32)>::new((0.0, 1.0)));
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(100),
        move || {
            let Some(app) = weak.upgrade() else {
                timer_handle.stop();
                return;
            };
            if app.get_profile_ready() && app.get_threads_ready() {
                timer_handle.stop();
                return;
            }
            let (mut pos, dir) = scan.get();
            pos += dir * 0.05;
            let dir = if pos >= 1.0 {
                pos = 1.0;
                -1.0
            } else if pos <= 0.0 {
                pos = 0.0;
                1.0
            } else {
                dir
            };
            scan.set((pos, dir));
            app.set_startup_progress(pos);

            let version = STARTUP_STAGE_VERSION.load(Ordering::Acquire);
            if version == applied.load(Ordering::Acquire) {
                return;
            }
            applied.store(version, Ordering::Release);
            if let Ok(stage) = STARTUP_STAGE.lock() {
                if !stage.1.is_empty() {
                    app.set_startup_note(stage.1.into());
                }
            }
        },
    );
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

/// Workbench floor and the size it opens at, in logical pixels.
///
/// `MIN_WINDOW_*` must stay equal to `Theme.min-window-width/height`
/// (design_tokens.slint): Slint's `min-width` is only advisory to the
/// platform, winit enforces the real floor through `with_min_inner_size`, and
/// the fit below clamps against the same pair. `OPEN_WINDOW_MAX_*` is the
/// ceiling of the automatic open size: the first frame should read as a small
/// workbench, so a large monitor never gets a canvas-sized window. Both are
/// ceilings, not requests — the user can still drag the window wider.
const MIN_WINDOW_W: f64 = 360.0;
const MIN_WINDOW_H: f64 = 360.0;
const OPEN_WINDOW_MAX_W: f64 = 960.0;
const OPEN_WINDOW_MAX_H: f64 = 780.0;

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
                            let min_w = (MIN_WINDOW_W * scale).round() as i32;
                            let min_h = (MIN_WINDOW_H * scale).round() as i32;
                            let max_w = (OPEN_WINDOW_MAX_W * scale).round() as i32;
                            let max_h = (OPEN_WINDOW_MAX_H * scale).round() as i32;
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

/// Register the bundled faces before the first text layout.
///
/// MiSans carries the UI copy: it is the face the phone shell draws in, it is
/// licenced for embedding, and its natural line height (1.33em) sits within 1%
/// of the Microsoft YaHei the layout tokens were tuned on — so the transcript
/// leading keeps working without retuning.
///
/// DejaVu Sans and Noto Emoji are appended behind it as pure glyph fallbacks.
/// MiSans ships no glyph for `⚠ ⌘ ☑ ☐ ☁ ⊞ ⊟ ✦ ◉ ❖` (nor did the YaHei it
/// replaces), and no emoji at all; the software renderer can only rasterise
/// outlines, so a colour emoji font would leave the emoji blank. MiSans still
/// wins every character the chain covers, Latin included, because the generic
/// family list is ordered.
pub(crate) fn register_bundled_font() -> Result<(), Box<dyn std::error::Error>> {
    use slint::fontique_011::fontique;

    let mut collection = slint::fontique_011::shared_collection();
    startup_point("font_collection_ready");
    let text = bundled_font::misans();
    let symbols = bundled_font::dejavu();
    let emoji = bundled_font::noto_emoji();
    startup_point("font_decompressed");
    let registered =
        collection.register_fonts(fontique::Blob::new(std::sync::Arc::new(text)), None);
    // Missing fallbacks are not fatal: the shell then keeps the pre-existing
    // gaps instead of failing to start over a decorative glyph.
    let symbol_families =
        collection.register_fonts(fontique::Blob::new(std::sync::Arc::new(symbols)), None);
    let emoji_families =
        collection.register_fonts(fontique::Blob::new(std::sync::Arc::new(emoji)), None);
    startup_point("font_registered");
    let families: Vec<_> = registered.iter().map(|(family, _)| *family).collect();
    if families.is_empty() {
        return Err("内嵌 MiSans 注册失败".into());
    }
    let mut chain = families.clone();
    chain.extend(symbol_families.iter().map(|(family, _)| *family));
    chain.extend(emoji_families.iter().map(|(family, _)| *family));
    for generic in [
        fontique::GenericFamily::SansSerif,
        fontique::GenericFamily::SystemUi,
        fontique::GenericFamily::UiSansSerif,
    ] {
        collection.set_generic_families(generic, chain.iter().copied());
    }
    // Inline code is shaped through the generic Monospace family (Slint's
    // `Style::Code`), not through a family name. Left unmapped, parley resolves
    // every character on its own through the script fallbacks: a hex id came
    // out as alternating UI-face letters and symbol-face digits, each fragment
    // its own glyph run — and the renderer draws one inline-code capsule per
    // run, so the id read as a row of little boxes. This shell already carries
    // a complete UI chain that covers Latin, Han and symbols, so pointing the
    // generic at it keeps a whole span in one family. (No monospace face is
    // installed on these targets — the collection is built from the bundled
    // blobs alone — so code reads as UI text on a capsule, which is what
    // `Theme.font-mono` ends up resolving to anyway.)
    collection.set_generic_families(
        fontique::GenericFamily::Monospace,
        chain.iter().copied(),
    );
    // Emoji are drawn, not substituted: naming the face for the emoji generic
    // family is what stops a codepoint from falling through to a bitmap-only
    // system font the software renderer cannot rasterise.
    if !emoji_families.is_empty() {
        collection.set_generic_families(
            fontique::GenericFamily::Emoji,
            emoji_families.iter().map(|(family, _)| *family),
        );
    }
    // Text that names MiSans explicitly does not walk the generic list when a
    // glyph is missing: parley asks the collection for the fallback families of
    // the character's script instead. Symbols and emoji are Common script, so
    // that is the list the two fallback faces have to join — otherwise a ⚠ or a
    // 🎉 inside a message still draws as a blank cell even though the faces are
    // registered.
    let mut symbol_chain: Vec<_> = symbol_families.iter().map(|(family, _)| *family).collect();
    symbol_chain.extend(emoji_families.iter().map(|(family, _)| *family));
    collection.set_fallbacks(
        fontique::FallbackKey::new(fontique::Script::from_str_unchecked("Zyyy"), None),
        symbol_chain.iter().copied(),
    );
    collection.append_fallbacks(
        fontique::FallbackKey::new(fontique::Script::from_str_unchecked("Hani"), None),
        families.iter().copied(),
    );
    Ok(())
}

mod bundled_font {
    static MISANS_LZ4: &[u8] =
        include_bytes!(concat!(env!("OUT_DIR"), "/MiSans-Regular.ttf.lz4"));
    static DEJAVU_LZ4: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/DejaVuSans.ttf.lz4"));
    static NOTO_EMOJI_LZ4: &[u8] =
        include_bytes!(concat!(env!("OUT_DIR"), "/NotoEmoji-Regular.ttf.lz4"));

    /// UI copy. Decompressed once, on the first call, and kept for the process
    /// lifetime because fontique borrows the bytes.
    pub fn misans() -> &'static [u8] {
        static DECOMPRESSED: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
        DECOMPRESSED
            .get_or_init(|| {
                lz4_flex::decompress_size_prepended(MISANS_LZ4).expect("内嵌 MiSans 解压失败")
            })
            .as_slice()
    }

    /// Symbol fallback face, same lifecycle as [`misans`].
    pub fn dejavu() -> &'static [u8] {
        static DECOMPRESSED: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
        DECOMPRESSED
            .get_or_init(|| {
                lz4_flex::decompress_size_prepended(DEJAVU_LZ4).expect("内嵌 DejaVu Sans 解压失败")
            })
            .as_slice()
    }

    /// Monochrome emoji face, same lifecycle as [`misans`].
    pub fn noto_emoji() -> &'static [u8] {
        static DECOMPRESSED: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
        DECOMPRESSED
            .get_or_init(|| {
                lz4_flex::decompress_size_prepended(NOTO_EMOJI_LZ4)
                    .expect("内嵌 Noto Emoji 解压失败")
            })
            .as_slice()
    }
}

#[cfg(test)]
mod bundled_font_tests {
    //! The bundled faces are the only source of glyphs on a machine with no CJK
    //! font installed, so a silently failed registration would ship a shell of
    //! empty boxes. These assertions run against the real embedded blobs.

    use super::register_bundled_font;

    #[test]
    fn bundled_faces_register_in_priority_order() {
        register_bundled_font().expect("register bundled fonts");

        let mut collection = slint::fontique_011::shared_collection();
        let text = collection.family_id("MiSans").expect("MiSans family");
        let symbols = collection
            .family_id("DejaVu Sans")
            .expect("DejaVu Sans family");
        let emoji = collection
            .family_id("Noto Emoji")
            .expect("Noto Emoji family");
        let chain = [text, symbols, emoji];

        // The two fallbacks exist for glyphs MiSans has none of: a warning sign
        // DejaVu carries, and an emoji only the monochrome face draws. If either
        // one stopped resolving, users would see blank cells mid-sentence.
        for (label, ch) in [("⚠", '⚠'), ("🎉", '🎉'), ("🚀", '🚀')] {
            assert!(
                chain_covers(&mut collection, &chain, ch),
                "{label} has no glyph in the bundled face chain"
            );
        }

        // The path the shaper actually takes for text that names MiSans: the
        // family stack is just MiSans, and a missing glyph is resolved through
        // the fallback families of the character's script — Common, for both
        // symbols and emoji. One test covers both paths because the shared font
        // collection may only be initialised once per process.
        for (label, ch) in [("⚠", '⚠'), ("⌘", '⌘'), ("☑", '☑'), ("🎉", '🎉'), ("🚀", '🚀')] {
            assert!(
                script_fallback_covers(&mut collection, text, ch),
                "{label} is not reachable through the Common-script fallback"
            );
        }
    }

    fn script_fallback_covers(
        collection: &mut slint::fontique_011::fontique::Collection,
        family: slint::fontique_011::fontique::FamilyId,
        target: char,
    ) -> bool {
        use slint::fontique_011::fontique;

        let mut cache = fontique::SourceCache::default();
        let mut query = collection.query(&mut cache);
        query.set_families(core::iter::once(family));
        query.set_fallbacks(fontique::FallbackKey::new(
            fontique::Script::from_str_unchecked("Zyyy"),
            None,
        ));
        let mut covered = false;
        query.matches_with(|font| {
            if font
                .charmap()
                .is_some_and(|charmap| charmap.map(target).is_some())
            {
                covered = true;
                fontique::QueryStatus::Stop
            } else {
                fontique::QueryStatus::Continue
            }
        });
        covered
    }

    fn chain_covers(
        collection: &mut slint::fontique_011::fontique::Collection,
        families: &[slint::fontique_011::fontique::FamilyId],
        target: char,
    ) -> bool {
        use slint::fontique_011::fontique;

        let mut cache = fontique::SourceCache::default();
        let mut query = collection.query(&mut cache);
        query.set_families(families.iter().copied());
        let mut covered = false;
        query.matches_with(|font| {
            if font
                .charmap()
                .is_some_and(|charmap| charmap.map(target).is_some())
            {
                covered = true;
                fontique::QueryStatus::Stop
            } else {
                fontique::QueryStatus::Continue
            }
        });
        covered
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    //! Headless UI tests serialize window construction behind one lock, and
    //! every test thread carries its own MinimalSoftwareWindow platform.
    use std::sync::{Mutex, MutexGuard};

    static LOCK: Mutex<()> = Mutex::new(());

    struct TestPlatform;
    impl slint::platform::Platform for TestPlatform {
        fn create_window_adapter(
            &self,
        ) -> Result<std::rc::Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> {
            Ok(
                slint::platform::software_renderer::MinimalSoftwareWindow::new(
                    slint::platform::software_renderer::RepaintBufferType::NewBuffer,
                ),
            )
        }
    }

    pub(crate) fn lock() -> MutexGuard<'static, ()> {
        // Slint's global context is thread-local, so installing the platform
        // once per process only serves the first test's thread. Every later
        // thread would fall back to the lazy default backend (winit), whose
        // event-loop proxy claims a process-global slot and locks every
        // further thread out with "The Slint platform was initialized in
        // another thread". Install per thread instead; on a thread that
        // already holds a context `set_platform` reports AlreadySet, which is
        // exactly the case we want to keep working.
        let _ = slint::platform::set_platform(Box::new(TestPlatform));
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
                if crate::completion_notice::take_notice_window() {
                    // The completion toast is a tool window that never activates
                    // on show: no taskbar button, no focus taken from whatever
                    // the user is doing. It also has no minimum size, unlike the
                    // workbench and the other auxiliary windows.
                    let attributes = attributes
                        .with_title("wunder 桌面版")
                        .with_decorations(false)
                        .with_active(false);
                    #[cfg(windows)]
                    {
                        use winit::platform::windows::WindowAttributesExtWindows as _;
                        return attributes.with_skip_taskbar(true).with_no_activate(true);
                    }
                    #[cfg(target_os = "linux")]
                    {
                        // GNOME also counts skip-taskbar windows as application
                        // windows and may hand a dock click to the toast instead
                        // of the workbench; a Utility window is filtered out.
                        use winit::platform::x11::WindowAttributesExtX11 as _;
                        return attributes
                            .with_x11_window_type(vec![winit::platform::x11::WindowType::Utility]);
                    }
                    #[cfg(not(any(windows, target_os = "linux")))]
                    return attributes;
                }
                if crate::tray::take_notice_window() {
                    // The tray notice must not become the window a taskbar or
                    // dock click activates; keep it a Utility top-level on X11.
                    #[cfg(target_os = "linux")]
                    {
                        use winit::platform::x11::WindowAttributesExtX11 as _;
                        return attributes
                            .with_title("wunder 桌面版")
                            .with_decorations(false)
                            .with_active(false)
                            .with_x11_window_type(vec![winit::platform::x11::WindowType::Utility]);
                    }
                }
                if crate::pet_window::take_overlay_window() {
                    // A pet overlay: frameless, never activated, and on X11 a
                    // Utility window so it stays out of the taskbar and pager.
                    let attributes = attributes
                        .with_title("wunder 桌面版")
                        .with_decorations(false)
                        .with_active(false)
                        .with_resizable(false);
                    #[cfg(target_os = "linux")]
                    {
                        use winit::platform::x11::WindowAttributesExtX11 as _;
                        return attributes
                            .with_x11_window_type(vec![winit::platform::x11::WindowType::Utility]);
                    }
                    #[cfg(not(target_os = "linux"))]
                    return attributes;
                }
                let frameless =
                    frameless_first_window.swap(false, std::sync::atomic::Ordering::AcqRel);
                let attributes = attributes
                    .with_min_inner_size(winit::dpi::LogicalSize::new(MIN_WINDOW_W, MIN_WINDOW_H))
                    .with_title("wunder 桌面版")
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
