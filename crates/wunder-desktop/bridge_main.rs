#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod args;
mod bridge;
mod runtime;

use anyhow::{Context, Result};
use args::DesktopArgs;
use bridge::DesktopBridge;
use clap::Parser;
use std::process::Command;
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    wunder_server::rustls_provider::install_process_default_provider();
    init_tracing();
    let args = DesktopArgs::parse();
    run_bridge(args)
}

fn run_bridge(args: DesktopArgs) -> Result<()> {
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

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        if startup_timing_enabled() || cfg!(debug_assertions) {
            EnvFilter::new("info")
        } else {
            EnvFilter::new("warn")
        }
    });
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

fn startup_timing_enabled() -> bool {
    match std::env::var("WUNDER_STARTUP_TIMING")
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
    {
        Some(value) if matches!(value.as_str(), "1" | "true" | "on" | "yes") => true,
        Some(value) if matches!(value.as_str(), "0" | "false" | "off" | "no") => false,
        Some(_) => true,
        None => true,
    }
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
