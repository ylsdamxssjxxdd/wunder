use super::config::Config;
use anyhow::{Context, Result};
use chrono::{SecondsFormat, Utc};
use std::backtrace::Backtrace;
use std::env;
use std::fmt as stdfmt;
use std::fs;
use std::io;
use std::io::IsTerminal;
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};
use tracing::field::{Field, Visit};
use tracing::{error, info, warn, Event, Level, Subscriber};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt::format::{FormatEvent, FormatFields, Writer};
use tracing_subscriber::fmt::FmtContext;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

const DEFAULT_LOG_LEVEL: &str = "info";
const DEFAULT_SERVER_LOG_DIR: &str = "./config/data/logs/server";
const LOG_FILE_BASENAME: &str = "server.jsonl";

static FILE_LOG_GUARD: OnceLock<WorkerGuard> = OnceLock::new();
static PANIC_HOOK_INSTALLED: OnceLock<()> = OnceLock::new();

#[derive(Clone, Debug)]
struct ConsoleEventFormatter {
    ansi: bool,
}

impl ConsoleEventFormatter {
    fn new(ansi: bool) -> Self {
        Self { ansi }
    }

    fn write_level(&self, writer: &mut Writer<'_>, level: &Level) -> stdfmt::Result {
        if self.ansi {
            match *level {
                Level::TRACE => write!(writer, "[\x1b[35mTRACE\x1b[0m]"),
                Level::DEBUG => write!(writer, "[\x1b[34mDEBUG\x1b[0m]"),
                Level::INFO => write!(writer, "[\x1b[32mINFO\x1b[0m]"),
                Level::WARN => write!(writer, "[\x1b[33mWARN\x1b[0m]"),
                Level::ERROR => write!(writer, "[\x1b[31mERROR\x1b[0m]"),
            }
        } else {
            write!(writer, "[{level}]")
        }
    }
}

#[derive(Default)]
struct ConsoleFieldVisitor {
    message: Option<String>,
    fields: Vec<(String, String)>,
}

impl ConsoleFieldVisitor {
    fn record_pair(&mut self, field: &Field, value: String) {
        if field.name() == "message" {
            self.message = Some(value);
            return;
        }
        self.fields.push((field.name().to_string(), value));
    }
}

impl Visit for ConsoleFieldVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
            return;
        }
        let formatted = if value.chars().any(char::is_whitespace) {
            format!("{value:?}")
        } else {
            value.to_string()
        };
        self.record_pair(field, formatted);
    }

    fn record_debug(&mut self, field: &Field, value: &dyn stdfmt::Debug) {
        self.record_pair(field, format!("{value:?}"));
    }
}

impl<S, N> FormatEvent<S, N> for ConsoleEventFormatter
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> stdfmt::Result {
        let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true);
        write!(writer, "{timestamp} ")?;
        self.write_level(&mut writer, event.metadata().level())?;

        let mut visitor = ConsoleFieldVisitor::default();
        event.record(&mut visitor);

        if let Some(message) = visitor.message {
            write!(writer, " {message}")?;
        }
        for (key, value) in visitor.fields {
            write!(writer, " {key}={value}")?;
        }

        writeln!(writer)
    }
}

pub fn init_server_tracing(
    config: &Config,
    server_mode: &str,
    config_path: &Path,
) -> Result<Option<PathBuf>> {
    let log_dir = resolve_server_log_dir(config);
    let persist_server_logs = should_persist_server_logs(server_mode);
    if persist_server_logs {
        fs::create_dir_all(&log_dir)
            .with_context(|| format!("create server log dir failed: {}", log_dir.display()))?;
    }

    let retention_days = resolve_server_log_retention_days(config);
    let cleanup_result = if persist_server_logs {
        Some(cleanup_expired_log_files(&log_dir, retention_days))
    } else {
        None
    };
    let env_filter = build_env_filter(config);

    let console_ansi = resolve_console_ansi_enabled();
    let console_layer = fmt::layer()
        .event_format(ConsoleEventFormatter::new(console_ansi))
        .with_ansi(console_ansi)
        .with_writer(io::stdout);

    let (file_layer, file_guard) = if persist_server_logs {
        let file_appender = tracing_appender::rolling::daily(&log_dir, LOG_FILE_BASENAME);
        let (file_writer, file_guard) = tracing_appender::non_blocking(file_appender);
        let file_layer = fmt::layer()
            .json()
            .with_ansi(false)
            .with_target(true)
            .with_thread_names(false)
            .with_thread_ids(false)
            .with_file(false)
            .with_line_number(false)
            .with_current_span(false)
            .with_span_list(false)
            .with_writer(file_writer);
        (Some(file_layer), Some(file_guard))
    } else {
        (None, None)
    };

    tracing_subscriber::registry()
        .with(env_filter)
        .with(console_layer)
        .with(file_layer)
        .init();

    if let Some(file_guard) = file_guard {
        let _ = FILE_LOG_GUARD.set(file_guard);
    }
    install_panic_hook();

    if let Some(cleanup_result) = cleanup_result {
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
    }

    info!(
        pid = std::process::id(),
        server_mode = %server_mode,
        config_path = %config_path.display(),
        persist_server_logs,
        log_dir = %log_dir.display(),
        retention_days,
        console_ansi,
        "server tracing initialized"
    );
    Ok(persist_server_logs.then_some(log_dir))
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

fn build_env_filter(config: &Config) -> EnvFilter {
    let mut env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(resolve_log_level(config)));
    for directive in default_noise_filter_directives() {
        if is_target_overridden_by_rust_log(directive) {
            continue;
        }
        if let Ok(parsed) = directive.parse() {
            env_filter = env_filter.add_directive(parsed);
        }
    }
    env_filter
}

fn default_noise_filter_directives() -> &'static [&'static str] {
    &["tower_http::trace=warn", "hyper=warn", "h2=warn"]
}

fn is_target_overridden_by_rust_log(directive: &str) -> bool {
    let Some((target, _)) = directive.split_once('=') else {
        return false;
    };
    let Some(raw) = env::var("RUST_LOG").ok() else {
        return false;
    };
    raw.split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .any(|item| item.starts_with(&format!("{target}=")) || item == target)
}

fn should_persist_server_logs(server_mode: &str) -> bool {
    !server_mode.trim().eq_ignore_ascii_case("sandbox")
}

fn resolve_console_ansi_enabled() -> bool {
    if let Some(mode) = env::var("WUNDER_LOG_COLOR")
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
    {
        if mode == "always" {
            return true;
        }
        if mode == "never" {
            return false;
        }
    }
    if env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if let Some(value) = env::var_os("CLICOLOR_FORCE").or_else(|| env::var_os("FORCE_COLOR")) {
        if value.to_string_lossy() != "0" {
            return true;
        }
    }
    if io::stdout().is_terminal() {
        return true;
    }
    is_likely_container_runtime()
}

fn is_likely_container_runtime() -> bool {
    Path::new("/.dockerenv").exists() || env::var_os("KUBERNETES_SERVICE_HOST").is_some()
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
    use super::{resolve_server_log_dir, should_delete_log_file, should_persist_server_logs};
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

    #[test]
    fn should_persist_server_logs_disables_sandbox_mode() {
        assert!(should_persist_server_logs("api"));
        assert!(!should_persist_server_logs("sandbox"));
        assert!(!should_persist_server_logs("SANDBOX"));
    }
}
