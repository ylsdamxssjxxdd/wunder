#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod args;
mod bridge;
mod runtime;

use anyhow::{anyhow, Context, Result};
use args::DesktopArgs;
use bridge::{DesktopBridge, DesktopRuntimeInfo};
use clap::Parser;
use std::process::Command;
use tauri::{WebviewUrl, WebviewWindowBuilder};
use tracing_subscriber::EnvFilter;

#[derive(Clone)]
struct DesktopAppState {
    runtime: DesktopRuntimeInfo,
}

const DESKTOP_WINDOW_BRIDGE_SCRIPT: &str = r#"
(function () {
  const invoke =
    (window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke)
    || (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke);
  if (typeof invoke !== 'function') return;

  const call = (cmd, args) => invoke(cmd, args || {});
  const api = window.wunderDesktop || {};
  api.toggleDevTools = () => call('desktop_toggle_devtools');
  api.minimizeWindow = () => call('desktop_window_minimize');
  api.toggleMaximizeWindow = () => call('desktop_window_toggle_maximize');
  api.closeWindow = () => call('desktop_window_close');
  api.isWindowMaximized = () => call('desktop_window_is_maximized');
  api.startWindowDrag = () => call('desktop_window_start_dragging');
  window.wunderDesktop = api;
})();
"#;

#[tauri::command]
fn desktop_runtime_info(state: tauri::State<'_, DesktopAppState>) -> DesktopRuntimeInfo {
    state.runtime.clone()
}

#[tauri::command]
fn desktop_toggle_devtools(window: tauri::WebviewWindow) {
    if window.is_devtools_open() {
        window.close_devtools();
    } else {
        window.open_devtools();
    }
}

#[tauri::command]
fn desktop_window_minimize(window: tauri::WebviewWindow) -> Result<(), String> {
    window.minimize().map_err(|err| err.to_string())
}

#[tauri::command]
fn desktop_window_toggle_maximize(window: tauri::WebviewWindow) -> Result<(), String> {
    if window.is_maximized().map_err(|err| err.to_string())? {
        window.unmaximize().map_err(|err| err.to_string())
    } else {
        window.maximize().map_err(|err| err.to_string())
    }
}

#[tauri::command]
fn desktop_window_close(window: tauri::WebviewWindow) -> Result<(), String> {
    window.close().map_err(|err| err.to_string())
}

#[tauri::command]
fn desktop_window_is_maximized(window: tauri::WebviewWindow) -> Result<bool, String> {
    window.is_maximized().map_err(|err| err.to_string())
}

#[tauri::command]
fn desktop_window_start_dragging(window: tauri::WebviewWindow) -> Result<(), String> {
    window.start_dragging().map_err(|err| err.to_string())
}

fn main() -> Result<()> {
    init_tracing();
    let args = DesktopArgs::parse();

    if args.bridge_only {
        return run_bridge_only(args);
    }

    run_gui(args)
}

fn run_bridge_only(args: DesktopArgs) -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("create tokio runtime failed")?;

    rt.block_on(async move {
        let mut bridge = DesktopBridge::launch(&args).await?;
        bridge.print_banner(args.print_token);
        if args.open {
            open_external_browser(&bridge.info().web_base)?;
        }
        wunder_server::shutdown::shutdown_signal().await;
        bridge.shutdown().await;
        Ok(())
    })
}

fn run_gui(args: DesktopArgs) -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("create tokio runtime failed")?;
    let mut bridge = rt.block_on(DesktopBridge::launch(&args))?;

    let runtime_info = bridge.info().clone();
    if args.print_token {
        println!("desktop_token={}", runtime_info.desktop_token);
    }

    let web_url = runtime_info.web_base.clone();
    let run_result = tauri::Builder::default()
        .manage(DesktopAppState {
            runtime: runtime_info,
        })
        .invoke_handler(tauri::generate_handler![
            desktop_runtime_info,
            desktop_toggle_devtools,
            desktop_window_minimize,
            desktop_window_toggle_maximize,
            desktop_window_close,
            desktop_window_is_maximized,
            desktop_window_start_dragging
        ])
        .setup(move |app| {
            let external = url::Url::parse(&web_url)
                .with_context(|| format!("invalid desktop web url: {web_url}"))?;
            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(external))
                .title("Wunder Desktop")
                .decorations(false)
                .inner_size(1360.0, 860.0)
                .min_inner_size(1024.0, 700.0)
                .resizable(true)
                .initialization_script(DESKTOP_WINDOW_BRIDGE_SCRIPT)
                .center()
                .build()
                .map_err(|err| anyhow!("create desktop window failed: {err}"))?;
            Ok(())
        })
        .run(tauri::generate_context!("wunder-desktop/tauri.conf.json"));

    rt.block_on(bridge.shutdown());
    run_result.map_err(|err| anyhow!("tauri runtime exited with error: {err}"))
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

fn open_external_browser(web_base: &str) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", "", web_base])
            .spawn()
            .with_context(|| format!("open browser failed: {web_base}"))?;
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(web_base)
            .spawn()
            .with_context(|| format!("open browser failed: {web_base}"))?;
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(web_base)
            .spawn()
            .with_context(|| format!("open browser failed: {web_base}"))?;
    }
    Ok(())
}
