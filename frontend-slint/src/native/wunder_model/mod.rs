mod glb;
mod raster;

use crate::MainWindow;
use glb::Scene;
use crate::native::presenter::{GestureCallback, ModelPresenter, PresenterFrame};
use raster::{render, View, REFERENCE_VIEW};
use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer};
use slint::{Timer, TimerMode};
use std::cell::RefCell;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

static SCENE: OnceLock<Result<Arc<Scene>, String>> = OnceLock::new();
static PRESENTER: OnceLock<Arc<std::sync::Mutex<Option<Arc<std::sync::Mutex<ModelPresenter>>>>>> = OnceLock::new();
thread_local! { static MODEL_TIMER: RefCell<Option<Timer>> = const { RefCell::new(None) }; }
thread_local! { static DETAIL_TIMER: RefCell<Option<Timer>> = const { RefCell::new(None) }; }
thread_local! { static NATIVE_PUMP_TIMER: RefCell<Option<Timer>> = const { RefCell::new(None) }; }

#[derive(Default)]
struct RenderCoordinator {
    running: bool,
    pending: bool,
    pending_preview: bool,
}

#[derive(Clone, Copy)]
struct RenderSize {
    width: usize,
    height: usize,
}

fn clamp_size(width: i32, height: i32) -> RenderSize {
    RenderSize {
        width: (width.max(160) as usize).min(900),
        height: (height.max(140) as usize).min(700),
    }
}

pub fn install(app: &MainWindow) {
    let state = Arc::new(std::sync::Mutex::new(REFERENCE_VIEW));
    let coordinator = Arc::new(std::sync::Mutex::new(RenderCoordinator::default()));
    let presenter: Arc<std::sync::Mutex<Option<Arc<std::sync::Mutex<ModelPresenter>>>>> =
        Arc::new(std::sync::Mutex::new(None));
    let _ = PRESENTER.set(presenter.clone());
    let size = Arc::new(std::sync::Mutex::new(RenderSize {
        width: 640,
        height: 480,
    }));
    let weak = app.as_weak();
    let render_state = state.clone();
    let gesture_state = state.clone();
    let gesture_coordinator = coordinator.clone();
    let gesture_size = size.clone();
    let geometry_presenter = presenter.clone();
    let geometry_weak = app.as_weak();
    app.on_wunder_model_geometry_changed(move |x, y, width, height| {
        let Some(app) = geometry_weak.upgrade() else { return; };
        let scale = app.window().scale_factor();
        let width_px = (width.max(1) as f32 * scale).round().max(1.0) as u32;
        let height_px = (height.max(1) as f32 * scale).round().max(1.0) as u32;
        let gesture_weak = app.as_weak();
        let callback: GestureCallback = Arc::new(move |kind, dx, dy| {
            let kind = kind.to_string();
            let gesture_weak = gesture_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = gesture_weak.upgrade() {
                    app.invoke_wunder_model_gesture(kind.into(), dx, dy);
                }
            });
        });
        let presenter = {
            let Ok(mut slot) = geometry_presenter.lock() else { return; };
            if slot.is_none() {
                *slot = ModelPresenter::new(&app.window(), callback)
                    .map(|p| Arc::new(std::sync::Mutex::new(p)));
            }
            slot.clone()
        };
        if let Some(presenter) = presenter {
            if let Ok(presenter) = presenter.lock() {
                let ready = presenter.set_geometry(
                    (x as f32 * scale).round() as i32,
                    (y as f32 * scale).round() as i32,
                    width_px,
                    height_px,
                );
                // Keep the child hidden until its first frame has been
                // submitted. This avoids a blank native surface covering the
                // Slint fallback/loading layer during the initial layout pass.
                let active = ready
                    && app.get_wunder_model_native_active()
                    && app.get_settings_open()
                    && app.get_settings_active_panel() == 11;
                presenter.set_visible(active);
                NATIVE_PUMP_TIMER.with(|slot| {
                    let mut timer = slot.borrow_mut();
                    let timer = timer.get_or_insert_with(Timer::default);
                    if active {
                        let pump_presenter = geometry_presenter.clone();
                        timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
                            if let Ok(slot) = pump_presenter.lock() {
                                if let Some(presenter) = slot.as_ref() {
                                    if let Ok(presenter) = presenter.lock() {
                                        presenter.pump_events();
                                    }
                                }
                            }
                        });
                    } else {
                        timer.stop();
                    }
                });
            }
        }
    });
    app.on_wunder_model_gesture(move |kind, dx, dy| {
        if kind == "settle" {
            let detail_weak = weak.clone();
            let detail_state = gesture_state.clone();
            let detail_coordinator = gesture_coordinator.clone();
            let detail_size = gesture_size.clone();
            DETAIL_TIMER.with(|slot| {
                let mut timer = slot.borrow_mut();
                let timer = timer.get_or_insert_with(Timer::default);
                timer.start(
                    TimerMode::SingleShot,
                    Duration::from_millis(120),
                    move || {
                        request_render(
                            &detail_weak,
                            detail_state.clone(),
                            detail_coordinator.clone(),
                            detail_size.clone(),
                            false,
                        );
                    },
                );
            });
            return;
        }
        let Ok(mut view) = render_state.lock() else {
            return;
        };
        match kind.as_str() {
            "rotate" => {
                view.yaw = (view.yaw + dx * 0.22).rem_euclid(360.0);
                view.pitch = (view.pitch + dy * 0.16).clamp(-75.0, 75.0);
            }
            "pan" => {
                view.pan_x += dx * 0.008;
                view.pan_y -= dy * 0.008;
            }
            "zoom" => view.zoom = (view.zoom * (-dy * 0.0012).exp()).clamp(0.4, 5.0),
            "reset" => {
                *view = REFERENCE_VIEW
            }
            _ => {}
        }
        drop(view);
        request_render(
            &weak,
            gesture_state.clone(),
            gesture_coordinator.clone(),
            gesture_size.clone(),
            true,
        );
    });
    let weak = app.as_weak();
    let reset_state = state.clone();
    let reset_coordinator = coordinator.clone();
    let reset_size = size.clone();
    app.on_wunder_model_reset(move || {
        if let Ok(mut v) = reset_state.lock() {
            *v = REFERENCE_VIEW;
        }
        request_render(
            &weak,
            reset_state.clone(),
            reset_coordinator.clone(),
            reset_size.clone(),
            false,
        );
    });
    let weak = app.as_weak();
    let retry_state = state.clone();
    let retry_coordinator = coordinator.clone();
    let retry_size = size.clone();
    app.on_wunder_model_retry(move || {
        request_render(
            &weak,
            retry_state.clone(),
            retry_coordinator.clone(),
            retry_size.clone(),
            false,
        );
    });
    let rotating = Arc::new(std::sync::Mutex::new(true));
    let toggle_rotating = rotating.clone();
    let toggle_weak = app.as_weak();
    app.on_wunder_model_toggle_rotate(move || {
        let Ok(mut value) = toggle_rotating.lock() else {
            return;
        };
        *value = !*value;
        if let Some(app) = toggle_weak.upgrade() {
            app.set_wunder_model_rotating(*value);
        }
    });
    let timer_state = state.clone();
    let timer_rotating = rotating.clone();
    let timer_coordinator = coordinator.clone();
    let timer_size = size.clone();
    let timer_weak = app.as_weak();
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, Duration::from_millis(90), move || {
        let Some(app) = timer_weak.upgrade() else {
            return;
        };
        if !app.get_settings_open() || app.get_settings_active_panel() != 11 {
            return;
        }
        if !timer_rotating.lock().map(|value| *value).unwrap_or(false) {
            return;
        }
        if let Ok(mut view) = timer_state.lock() {
            view.yaw = (view.yaw + 1.0).rem_euclid(360.0);
        }
        request_render(
            &timer_weak,
            timer_state.clone(),
            timer_coordinator.clone(),
            timer_size.clone(),
            true,
        );
    });
    MODEL_TIMER.with(|slot| *slot.borrow_mut() = Some(timer));
    let visibility_coordinator = coordinator.clone();
    let visibility_size = size.clone();
    let visibility_state = state.clone();
    let visibility_weak = app.as_weak();
    let visibility_presenter = presenter.clone();
    app.on_wunder_model_visibility(move |open, panel| {
        let visible = open && panel == 11;
        MODEL_TIMER.with(|slot| {
            if let Some(timer) = slot.borrow_mut().as_mut() {
                if visible {
                    timer.start(TimerMode::Repeated, Duration::from_millis(90), {
                        let timer_state = visibility_state.clone();
                        let timer_coordinator = visibility_coordinator.clone();
                        let timer_size = visibility_size.clone();
                        let timer_weak = visibility_weak.clone();
                        move || {
                            let Some(app) = timer_weak.upgrade() else {
                                return;
                            };
                            if !app.get_settings_open() || app.get_settings_active_panel() != 11 {
                                return;
                            }
                            if let Ok(mut view) = timer_state.lock() {
                                view.yaw = (view.yaw + 1.0).rem_euclid(360.0);
                            }
                            request_render(
                                &timer_weak,
                                timer_state.clone(),
                                timer_coordinator.clone(),
                                timer_size.clone(),
                                true,
                            );
                        }
                    });
                } else {
                    timer.stop();
                }
            }
        });
        if !visible {
            if let Some(app) = visibility_weak.upgrade() {
                app.set_wunder_model_native_active(false);
            }
            if let Ok(mut queue) = visibility_coordinator.lock() {
                queue.pending = false;
                queue.pending_preview = false;
            }
        } else {
            request_render(
                &visibility_weak,
                visibility_state.clone(),
                visibility_coordinator.clone(),
                visibility_size.clone(),
                false,
            );
        }
        if let Ok(slot) = visibility_presenter.lock() {
            if let Some(presenter) = slot.as_ref() {
                if let Ok(presenter) = presenter.lock() {
                    let native_active = visibility_weak
                        .upgrade()
                        .map(|app| app.get_wunder_model_native_active())
                        .unwrap_or(false);
                    presenter.set_visible(visible && native_active);
                }
            }
        }
        let has_native_presenter = visibility_presenter
            .lock()
            .map(|slot| slot.is_some())
            .unwrap_or(false);
        NATIVE_PUMP_TIMER.with(|slot| {
            let mut timer = slot.borrow_mut();
            let timer = timer.get_or_insert_with(Timer::default);
            if visible && has_native_presenter {
                let pump_presenter = visibility_presenter.clone();
                timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
                    if let Ok(slot) = pump_presenter.lock() {
                        if let Some(presenter) = slot.as_ref() {
                            if let Ok(presenter) = presenter.lock() {
                                presenter.pump_events();
                            }
                        }
                    }
                });
            } else {
                timer.stop();
            }
        });
    });
    let size_state = size.clone();
    let size_weak = app.as_weak();
    let size_coordinator = coordinator.clone();
    let size_view = state.clone();
    app.on_wunder_model_size_changed(move |width, height| {
        let scale = size_weak.upgrade().map(|app| app.window().scale_factor()).unwrap_or(1.0);
        if let Ok(mut target) = size_state.lock() {
            *target = clamp_size((width as f32 * scale).round() as i32, (height as f32 * scale).round() as i32);
        }
        request_render(
            &size_weak,
            size_view.clone(),
            size_coordinator.clone(),
            size_state.clone(),
            false,
        );
    });
    app.set_wunder_model_loading(true);
    app.set_wunder_model_status("".into());
    app.set_wunder_model_native_active(false);

}

fn request_render(
    weak: &slint::Weak<MainWindow>,
    state: Arc<std::sync::Mutex<View>>,
    coordinator: Arc<std::sync::Mutex<RenderCoordinator>>,
    size: Arc<std::sync::Mutex<RenderSize>>,
    preview: bool,
) {
    let Some(app) = weak.upgrade() else { return };
    if !app.get_settings_open() || app.get_settings_active_panel() != 11 {
        return;
    }
    let should_start = coordinator
        .lock()
        .map(|mut queue| {
            if queue.running {
                queue.pending = true;
                queue.pending_preview = preview;
                false
            } else {
                queue.running = true;
                queue.pending_preview = false;
                true
            }
        })
        .unwrap_or(false);
    if !should_start {
        return;
    }
    let weak = weak.clone();
    std::thread::spawn(move || {
        let scene = SCENE.get_or_init(|| {
            glb::parse(include_bytes!("../../../assets/models/wunder.glb")).map(Arc::new)
        });
        let full_dimensions = size.lock().map(|s| *s).unwrap_or(RenderSize {
            width: 640,
            height: 480,
        });
        // Camera motion never lowers the canvas resolution. The scheduler
        // still coalesces input to one worker, while every presented frame
        // uses the current bounded viewport size.
        let dimensions = full_dimensions;
        let result = scene.as_ref().ok().map(|scene| {
            let view = state.lock().map(|v| *v).unwrap_or(REFERENCE_VIEW);
            render(scene.as_ref(), view, dimensions.width, dimensions.height)
        });
        let coordinator_next = coordinator.clone();
        let state_next = state.clone();
        let _ = weak.upgrade_in_event_loop(move |app| {
            let visible = app.get_settings_open() && app.get_settings_active_panel() == 11;
            if visible {
                if let Some(rgba) = result {
                    let mut frame = PresenterFrame {
                        width: dimensions.width as u32,
                        height: dimensions.height as u32,
                        rgba,
                    };
                    let mut presented = false;
                    if let Some(slot) = PRESENTER.get() {
                        if let Ok(slot) = slot.lock() {
                            if let Some(presenter) = slot.as_ref() {
                                if let Ok(presenter) = presenter.lock() {
                                    presented = presenter.present(&mut frame);
                                    presenter.set_visible(presented);
                                }
                            }
                        }
                    }
                    if !presented {
                        // Native presentation is optional. Keep the complete
                        // frame for Slint whenever the platform has no child
                        // surface or the native upload fails.
                        app.set_wunder_model_frame(Image::from_rgba8(
                            SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                                &frame.rgba,
                                frame.width,
                                frame.height,
                            ),
                        ));
                    }
                    app.set_wunder_model_native_active(presented);
                    app.set_wunder_model_loading(false);
                    app.set_wunder_model_failed(false);
                    app.set_wunder_model_status("心舰模型 · 拖动旋转 · 滚轮缩放".into());
                } else {
                    app.set_wunder_model_loading(false);
                    app.set_wunder_model_failed(true);
                }
            }
            let visible = app.get_settings_open() && app.get_settings_active_panel() == 11;
            let next_preview = coordinator_next
                .lock()
                .map(|mut queue| {
                    queue.running = false;
                    if visible && queue.pending {
                        queue.pending = false;
                        let next_preview = queue.pending_preview;
                        queue.pending_preview = false;
                        Some(next_preview)
                    } else {
                        queue.pending = false;
                        queue.pending_preview = false;
                        None
                    }
                })
                .unwrap_or(None);
            if let Some(next_preview) = next_preview {
                request_render(
                    &app.as_weak(),
                    state_next,
                    coordinator_next,
                    size,
                    next_preview,
                );
            }
        });
    });
}
