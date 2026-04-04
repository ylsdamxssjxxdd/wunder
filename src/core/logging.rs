use super::config::Config;
use anyhow::{Context, Result};
use std::backtrace::Backtrace;
use std::fs;
use std::io;
use std::io::IsTerminal;
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};
use tracing::{error, info, warn};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

const DEFAULT_LOG_LEVEL: &str = "info";
const DEFAULT_SERVER_LOG_DIR: &str = "./config/data/logs/server";
const LOG_FILE_BASENAME: &str = "server.jsonl";

static FILE_LOG_GUARD: OnceLock<WorkerGuard> = OnceLock::new();
static PANIC_HOOK_INSTALLED: OnceLock<()> = OnceLock::new();

pub fn init_server_tracing(
    config: &Config,
    server_mode: &str,
    config_path: &Path,
) -> Result<PathBuf> {
    let log_dir = resolve_server_log_dir(config);
    fs::create_dir_all(&log_dir)
        .with_context(|| format!("create server log dir failed: {}", log_dir.display()))?;

    let retention_days = resolve_server_log_retention_days(config);
    let cleanup_result = cleanup_expired_log_files(&log_dir, retention_days);
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(resolve_log_level(config)));

    let stdout_layer = fmt::layer()
        .compact()
        .with_ansi(io::stdout().is_terminal())
        .with_target(false)
        .with_thread_names(false)
        .with_file(false)
        .with_line_number(false);

    let file_appender = tracing_appender::rolling::daily(&log_dir, LOG_FILE_BASENAME);
    let (file_writer, file_guard) = tracing_appender::non_blocking(file_appender);
    let file_layer = fmt::layer()
        .json()
        .with_ansi(false)
        .with_target(true)
        .with_thread_names(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .with_current_span(true)
        .with_span_list(true)
        .with_writer(file_writer);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(stdout_layer)
        .with(file_layer)
        .init();

    let _ = FILE_LOG_GUARD.set(file_guard);
    install_panic_hook();

    match cleanup_result {
        Ok(removed) if removed > 0 => {
            info!(removed, retention_days, "expired server log files removed");
        }
        Ok(_) => {}
        Err(err) => {
            warn!(
                log_dir = %log_dir.display(),
                retention_days,
                "failed to cleanup expired server log files: {err}"
            );
        }
    }

    info!(
        pid = std::process::id(),
        server_mode = %server_mode,
        config_path = %config_path.display(),
        log_dir = %log_dir.display(),
        retention_days,
        "server tracing initialized"
    );
    Ok(log_dir)
}

pub fn resolve_server_log_dir(config: &Config) -> PathBuf {
    let configured = config.observability.server_log_dir.trim();
    if configured.is_empty() {
        PathBuf::from(DEFAULT_SERVER_LOG_DIR)
    } else {
        PathBuf::from(configured)
    }
}

fn resolve_server_log_retention_days(config: &Config) -> u64 {
    config.observability.server_log_retention_days
}

fn resolve_log_level(config: &Config) -> String {
    let cleaned = config.observability.log_level.trim().to_ascii_lowercase();
    if cleaned.is_empty() {
        DEFAULT_LOG_LEVEL.to_string()
    } else {
        cleaned
    }
}

fn cleanup_expired_log_files(log_dir: &Path, retention_days: u64) -> io::Result<usize> {
    if retention_days == 0 {
        return Ok(0);
    }

    let retention_window = retention_days
        .checked_mul(24 * 60 * 60)
        .map(Duration::from_secs)
        .unwrap_or(Duration::MAX);
    let cutoff = SystemTime::now()
        .checked_sub(retention_window)
        .unwrap_or(SystemTime::UNIX_EPOCH);

    let mut removed = 0usize;
    for entry in fs::read_dir(log_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        if !is_server_log_file(&path) {
            continue;
        }
        let modified = entry.metadata()?.modified()?;
        if should_delete_log_file(modified, cutoff) {
            fs::remove_file(&path)?;
            removed += 1;
        }
    }
    Ok(removed)
}

fn is_server_log_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    name.starts_with("server") && name.contains(".jsonl")
}

fn should_delete_log_file(modified: SystemTime, cutoff: SystemTime) -> bool {
    modified < cutoff
}

fn install_panic_hook() {
    if PANIC_HOOK_INSTALLED.set(()).is_err() {
        return;
    }

    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        log_panic(panic_info);
        previous_hook(panic_info);
    }));
}

fn log_panic(panic_info: &PanicHookInfo<'_>) {
    let location = panic_info
        .location()
        .map(|value| format!("{}:{}", value.file(), value.line()))
        .unwrap_or_else(|| "unknown".to_string());
    let payload = if let Some(message) = panic_info.payload().downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = panic_info.payload().downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic payload".to_string()
    };
    let thread = std::thread::current();
    let thread_name = thread.name().unwrap_or("unnamed");
    error!(
        thread_name = thread_name,
        location = %location,
        panic_payload = %payload,
        backtrace = %Backtrace::force_capture(),
        "process panic captured"
    );
}

#[cfg(test)]
mod tests {
    use super::{resolve_server_log_dir, should_delete_log_file};
    use crate::config::Config;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    #[test]
    fn resolve_server_log_dir_defaults_to_config_data() {
        let config = Config::default();
        assert_eq!(
            resolve_server_log_dir(&config),
            PathBuf::from("./config/data/logs/server")
        );
    }

    #[test]
    fn resolve_server_log_dir_uses_custom_value() {
        let mut config = Config::default();
        config.observability.server_log_dir = "./custom/logs".to_string();
        assert_eq!(
            resolve_server_log_dir(&config),
            PathBuf::from("./custom/logs")
        );
    }

    #[test]
    fn should_delete_log_file_only_when_older_than_cutoff() {
        let cutoff = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        assert!(should_delete_log_file(
            SystemTime::UNIX_EPOCH + Duration::from_secs(99),
            cutoff
        ));
        assert!(!should_delete_log_file(
            SystemTime::UNIX_EPOCH + Duration::from_secs(100),
            cutoff
        ));
        assert!(!should_delete_log_file(
            SystemTime::UNIX_EPOCH + Duration::from_secs(101),
            cutoff
        ));
    }
}
