#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod args;
mod bridge;
mod runtime;

use anyhow::{anyhow, Context, Result};
use args::DesktopArgs;
use bridge::{DesktopBridge, DesktopRuntimeInfo};
use clap::Parser;
use serde::Serialize;
use std::process::Command;
use std::sync::Arc;
use tauri::{WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::sync::Mutex;
use tracing_subscriber::EnvFilter;
use url::Url;

#[derive(Clone)]
struct DesktopAppState {
    runtime: DesktopRuntimeInfo,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DesktopUpdateSnapshot {
    phase: String,
    current_version: String,
    latest_version: String,
    downloaded: bool,
    progress: f64,
    message: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DesktopInstallResult {
    ok: bool,
    state: DesktopUpdateSnapshot,
}

struct PendingDesktopUpdate {
    update: Update,
    bytes: Vec<u8>,
}

struct DesktopUpdateState {
    snapshot: DesktopUpdateSnapshot,
    pending: Option<PendingDesktopUpdate>,
    checking: bool,
}

const TAURI_UPDATER_ENDPOINTS_ENV: &str = "WUNDER_TAURI_UPDATE_ENDPOINTS";
const TAURI_UPDATER_PUBKEY_ENV: &str = "WUNDER_TAURI_UPDATE_PUBKEY";

impl DesktopUpdateSnapshot {
    fn idle() -> Self {
        Self {
            phase: "idle".to_string(),
            current_version: String::new(),
            latest_version: String::new(),
            downloaded: false,
            progress: 0.0,
            message: String::new(),
        }
    }
}

impl DesktopUpdateState {
    fn new() -> Self {
        Self {
            snapshot: DesktopUpdateSnapshot::idle(),
            pending: None,
            checking: false,
        }
    }
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
  api.getUpdateState = () => call('desktop_get_update_state');
  api.checkForUpdates = () => call('desktop_check_for_updates');
  api.installUpdate = () => call('desktop_install_update');
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

fn normalize_update_message(error: impl std::fmt::Display) -> String {
    let message = error.to_string();
    if message.contains("updater target not configured")
        || message.contains("Unable to find update")
        || message.contains("No release")
    {
        "update source is not configured".to_string()
    } else {
        message
    }
}

fn parse_update_endpoints() -> Result<Vec<Url>, String> {
    let raw = std::env::var(TAURI_UPDATER_ENDPOINTS_ENV).unwrap_or_default();
    let endpoints: Result<Vec<_>, _> = raw
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            Url::parse(value)
                .map_err(|error| format!("invalid updater endpoint `{value}`: {error}"))
        })
        .collect();

    let endpoints = endpoints?;
    if endpoints.is_empty() {
        return Err("update source is not configured".to_string());
    }
    Ok(endpoints)
}

fn build_updater(app: &tauri::AppHandle) -> Result<tauri_plugin_updater::Updater, String> {
    let endpoints = parse_update_endpoints()?;
    let pubkey = std::env::var(TAURI_UPDATER_PUBKEY_ENV).unwrap_or_default();
    if pubkey.trim().is_empty() {
        return Err("update source is not configured".to_string());
    }

    let builder = app
        .updater_builder()
        .endpoints(endpoints)
        .map_err(normalize_update_message)?
        .pubkey(pubkey);

    builder.build().map_err(normalize_update_message)
}

fn with_current_version(
    snapshot: &DesktopUpdateSnapshot,
    app: &tauri::AppHandle,
) -> DesktopUpdateSnapshot {
    let mut cloned = snapshot.clone();
    cloned.current_version = app.package_info().version.to_string();
    cloned
}

#[tauri::command]
async fn desktop_get_update_state(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<Mutex<DesktopUpdateState>>>,
) -> Result<DesktopUpdateSnapshot, String> {
    let guard = state.lock().await;
    Ok(with_current_version(&guard.snapshot, &app))
}

#[tauri::command]
async fn desktop_check_for_updates(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<Mutex<DesktopUpdateState>>>,
) -> Result<DesktopUpdateSnapshot, String> {
    if cfg!(debug_assertions) {
        let mut guard = state.lock().await;
        guard.snapshot.phase = "unsupported".to_string();
        guard.snapshot.latest_version.clear();
        guard.snapshot.downloaded = false;
        guard.snapshot.progress = 0.0;
        guard.snapshot.message = "auto update is only available in packaged app".to_string();
        guard.pending = None;
        guard.checking = false;
        return Ok(with_current_version(&guard.snapshot, &app));
    }

    {
        let mut guard = state.lock().await;
        if guard.checking {
            return Ok(with_current_version(&guard.snapshot, &app));
        }
        guard.checking = true;
        guard.snapshot.phase = "checking".to_string();
        guard.snapshot.latest_version.clear();
        guard.snapshot.downloaded = false;
        guard.snapshot.progress = 0.0;
        guard.snapshot.message.clear();
        guard.pending = None;
    }

    let updater = match build_updater(&app) {
        Ok(updater) => updater,
        Err(error) => {
            let mut guard = state.lock().await;
            guard.snapshot.phase = "error".to_string();
            guard.snapshot.message = error;
            guard.checking = false;
            return Ok(with_current_version(&guard.snapshot, &app));
        }
    };

    let update = match updater.check().await {
        Ok(Some(update)) => update,
        Ok(None) => {
            let mut guard = state.lock().await;
            guard.snapshot.phase = "not-available".to_string();
            guard.snapshot.latest_version.clear();
            guard.snapshot.downloaded = false;
            guard.snapshot.progress = 0.0;
            guard.snapshot.message.clear();
            guard.checking = false;
            return Ok(with_current_version(&guard.snapshot, &app));
        }
        Err(error) => {
            let mut guard = state.lock().await;
            guard.snapshot.phase = "error".to_string();
            guard.snapshot.message = normalize_update_message(error);
            guard.checking = false;
            return Ok(with_current_version(&guard.snapshot, &app));
        }
    };

    let latest_version = update.version.to_string();
    {
        let mut guard = state.lock().await;
        guard.snapshot.phase = "available".to_string();
        guard.snapshot.latest_version = latest_version.clone();
        guard.snapshot.downloaded = false;
        guard.snapshot.progress = 0.0;
        guard.snapshot.message.clear();
    }

    let progress_state = Arc::clone(state.inner());
    let mut downloaded_bytes: u64 = 0;
    let bytes = match update
        .download(
            move |chunk_length, content_length| {
                downloaded_bytes = downloaded_bytes.saturating_add(chunk_length as u64);
                let progress = content_length
                    .filter(|total| *total > 0)
                    .map(|total| (downloaded_bytes as f64 / total as f64) * 100.0)
                    .unwrap_or(0.0);
                let progress_state = Arc::clone(&progress_state);
                tauri::async_runtime::spawn(async move {
                    let mut guard = progress_state.lock().await;
                    guard.snapshot.phase = "downloading".to_string();
                    guard.snapshot.progress = progress.clamp(0.0, 100.0);
                });
            },
            || {},
        )
        .await
    {
        Ok(bytes) => bytes,
        Err(error) => {
            let mut guard = state.lock().await;
            guard.snapshot.phase = "error".to_string();
            guard.snapshot.message = normalize_update_message(error);
            guard.snapshot.downloaded = false;
            guard.snapshot.progress = 0.0;
            guard.checking = false;
            return Ok(with_current_version(&guard.snapshot, &app));
        }
    };

    let mut guard = state.lock().await;
    guard.snapshot.phase = "downloaded".to_string();
    guard.snapshot.latest_version = latest_version;
    guard.snapshot.downloaded = true;
    guard.snapshot.progress = 100.0;
    guard.snapshot.message.clear();
    guard.pending = Some(PendingDesktopUpdate { update, bytes });
    guard.checking = false;
    Ok(with_current_version(&guard.snapshot, &app))
}

#[tauri::command]
async fn desktop_install_update(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<Mutex<DesktopUpdateState>>>,
) -> Result<DesktopInstallResult, String> {
    let pending = {
        let mut guard = state.lock().await;
        if guard.checking {
            let snapshot = with_current_version(&guard.snapshot, &app);
            return Ok(DesktopInstallResult {
                ok: false,
                state: snapshot,
            });
        }
        guard.pending.take()
    };

    let Some(pending) = pending else {
        let guard = state.lock().await;
        let snapshot = with_current_version(&guard.snapshot, &app);
        return Ok(DesktopInstallResult {
            ok: false,
            state: snapshot,
        });
    };

    if let Err(error) = pending.update.install(pending.bytes) {
        let mut guard = state.lock().await;
        guard.snapshot.phase = "error".to_string();
        guard.snapshot.message = normalize_update_message(error);
        guard.snapshot.downloaded = false;
        guard.snapshot.progress = 0.0;
        let snapshot = with_current_version(&guard.snapshot, &app);
        return Ok(DesktopInstallResult {
            ok: false,
            state: snapshot,
        });
    }

    let mut guard = state.lock().await;
    guard.snapshot.phase = "installing".to_string();
    guard.snapshot.downloaded = false;
    guard.snapshot.progress = 100.0;
    guard.snapshot.message.clear();
    let snapshot = with_current_version(&guard.snapshot, &app);
    Ok(DesktopInstallResult {
        ok: true,
        state: snapshot,
    })
}

#[tauri::command]
#[cfg(debug_assertions)]
fn desktop_toggle_devtools(window: tauri::WebviewWindow) {
    if window.is_devtools_open() {
        window.close_devtools();
    } else {
        window.open_devtools();
    }
}

#[tauri::command]
#[cfg(not(debug_assertions))]
fn desktop_toggle_devtools(_window: tauri::WebviewWindow) {}

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
        .manage(Arc::new(Mutex::new(DesktopUpdateState::new())))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            desktop_runtime_info,
            desktop_get_update_state,
            desktop_check_for_updates,
            desktop_install_update,
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
