//! Shared egui front end for the local Pupa and the browser workbench.
//!
//! The UI owns only presentation state. Native execution is isolated behind a
//! bounded worker in [`backend::NativeBackend`]; the WASM shell uses the same
//! command/event boundary and never links the local desktop runtime.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod app;
pub mod backend;
#[cfg(all(feature = "fixtures", not(target_arch = "wasm32")))]
mod capture;
pub mod icons;
pub mod markdown;
pub mod model;
pub mod settings;
pub mod theme;

/// Stable presentation data for screenshot and visual regression review.
pub mod fixtures;

pub use app::EguiApp;
pub use backend::{BackendCommand, BackendEvent, BackendPort};
pub use model::{
    process_panel_visible, AppState, MessageView, ProcessPageView, SessionView, TurnView,
    WorkflowView, WorkspaceFileView,
};

#[cfg(not(target_arch = "wasm32"))]
pub fn run_native() -> eframe::Result {
    run_native_with_mode(false, None, None)
}

/// Run the native shell with deterministic fixture data for visual review.
///
/// This mode is deliberately local to the UI process and does not initialize
/// `NativeDesktop`, SQLite, or a bridge service.
#[cfg(not(target_arch = "wasm32"))]
pub fn run_native_demo() -> eframe::Result {
    run_native_with_mode(true, None, None)
}

/// Open a deterministic settings view for comparison with the web client.
#[cfg(not(target_arch = "wasm32"))]
pub fn run_native_demo_settings(category: String) -> eframe::Result {
    run_native_with_mode(true, Some(category), None)
}

/// Open the fixed process timeline, optionally with its tool detail expanded.
#[cfg(not(target_arch = "wasm32"))]
pub fn run_native_demo_workflow(tool_detail: bool) -> eframe::Result {
    run_native_with_mode(true, None, Some(tool_detail))
}

#[cfg(not(target_arch = "wasm32"))]
fn run_native_with_mode(
    demo: bool,
    settings_category: Option<String>,
    workflow: Option<bool>,
) -> eframe::Result {
    let title = if demo {
        "蛹 · wunder · 演示"
    } else {
        "蛹 · wunder"
    };
    let inner_size = capture_viewport().unwrap_or(if demo {
        [1440.0, 900.0]
    } else {
        [1280.0, 820.0]
    });
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size(inner_size)
            .with_min_inner_size([760.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        title,
        options,
        Box::new(move |cc| {
            // set_fonts becomes active at the next egui pass. Register named
            // icon families in the creation callback, before any UI paints.
            theme::apply(&cc.egui_ctx);
            if demo {
                cc.egui_ctx.set_pixels_per_point(1.0);
            }
            let mut app = if demo {
                EguiApp::demo()
            } else {
                EguiApp::native(cc.egui_ctx.clone())
            };
            if let Some(category) = &settings_category {
                app.state.settings_open = true;
                app.state.settings_category = category.clone();
            }
            if let Some(tool_detail) = workflow {
                cc.egui_ctx.data_mut(|data| {
                    data.insert_temp(egui::Id::new(("timeline-process", "fixture-turn")), true);
                    if tool_detail {
                        data.insert_temp(
                            egui::Id::new((
                                "timeline-process-item",
                                "fixture-turn",
                                "fixture-tool-1",
                            )),
                            true,
                        );
                    }
                });
            }
            cc.egui_ctx.request_repaint();
            Ok(Box::new(app))
        }),
    )
}

/// Optional fixed viewport for visual review, from `WUNDER_EGUI_VIEWPORT`
/// as `1440x900`. Only the demo modes read it, so a normal launch keeps the
/// default window size; screenshot fixtures use it to pin one logical size.
#[cfg(not(target_arch = "wasm32"))]
fn capture_viewport() -> Option<[f32; 2]> {
    let raw = std::env::var("WUNDER_EGUI_VIEWPORT").ok()?;
    let (width, height) = raw.split_once(['x', 'X'])?;
    let width: f32 = width.trim().parse().ok()?;
    let height: f32 = height.trim().parse().ok()?;
    if !(320.0..=7680.0).contains(&width) || !(240.0..=4320.0).contains(&height) {
        return None;
    }
    Some([width, height])
}

#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub async fn run_web(canvas: web_sys::HtmlCanvasElement) -> Result<(), String> {    let options = eframe::WebOptions::default();
    eframe::WebRunner::new()
        .start(
            canvas,
            options,
            Box::new(|cc| {
                theme::apply(&cc.egui_ctx);
                Ok(Box::new(EguiApp::web(cc.egui_ctx.clone())))
            }),
        )
        .await
        .map_err(|_| "浏览器画布启动失败".to_string())
}
