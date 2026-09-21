use crate::args::DesktopArgs;
use crate::bridge::{DesktopBridge, DesktopRuntimeInfo};
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct DesktopStartupState {
    args: DesktopArgs,
    bridge: Mutex<Option<DesktopBridge>>,
}

impl DesktopStartupState {
    pub fn new(args: DesktopArgs) -> Self {
        Self {
            args,
            bridge: Mutex::new(None),
        }
    }

    pub async fn runtime_info(&self) -> Result<DesktopRuntimeInfo, String> {
        self.bridge
            .lock()
            .await
            .as_ref()
            .map(|bridge| bridge.info().clone())
            .ok_or_else(|| "desktop runtime is starting".to_string())
    }

    pub async fn shutdown(&self) {
        if let Some(mut bridge) = self.bridge.lock().await.take() {
            bridge.shutdown().await;
        }
    }
}

#[tauri::command]
pub async fn desktop_startup_ready(
    state: tauri::State<'_, Arc<DesktopStartupState>>,
) -> Result<String, String> {
    // Serialize retry/duplicate notifications without creating parallel runtimes.
    let mut guard = state.bridge.lock().await;
    if let Some(bridge) = guard.as_ref() {
        return Ok(bridge.info().web_base.clone());
    }
    let args = state.args.clone();
    // Runtime initialization starts after paint and yields during async IO while
    // the lightweight shell remains responsive.
    let bridge = tauri::async_runtime::spawn(async move { DesktopBridge::launch(&args).await })
        .await
        .map_err(|error| {
            tracing::error!(%error, "desktop startup task failed");
            "desktop runtime failed to start".to_string()
        })?
        .map_err(|error| {
            tracing::error!(%error, "desktop runtime initialization failed");
            "desktop runtime failed to start".to_string()
        })?;
    let web_base = bridge.info().web_base.clone();
    if state.args.print_token {
        println!("desktop_token={}", bridge.info().desktop_token);
    }
    *guard = Some(bridge);
    Ok(web_base)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[tokio::test]
    async fn unpainted_shell_has_no_runtime_and_can_shutdown_repeatedly() {
        let state = DesktopStartupState::new(DesktopArgs::parse_from(["desktop"]));
        assert_eq!(
            state.runtime_info().await.err(),
            Some("desktop runtime is starting".to_string())
        );
        state.shutdown().await;
        state.shutdown().await;
        assert!(state.bridge.lock().await.is_none());
    }
}
