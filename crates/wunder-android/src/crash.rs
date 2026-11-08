//! Rust panic reports on Android.
//!
//! The release shell is a `cdylib` with no console: a panic on an engine or
//! stream worker otherwise leaves only a logcat line that is gone after the
//! process restarts. This hook writes one bounded report under
//! `WUNDER_HOME/logs` before the default hook runs, and keeps the newest
//! reports so the directory never grows without limit. The hook itself must
//! not panic: every IO failure is ignored.

use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

const FILE_PREFIX: &str = "crash-rust-";
const FILE_SUFFIX: &str = ".log";
const KEEP_REPORTS: usize = 10;
/// One report read by 设置 → 数据与恢复; enough for a stack trace.
pub const REPORT_READ_LIMIT: usize = 64 * 1024;

/// One crash report surfaced to the settings page.
pub struct CrashReport {
    pub name: String,
    pub text: String,
}

pub fn install() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            write_report(info);
            prune(&logs_dir());
        }));
        previous(info);
    }));
}

pub fn logs_dir() -> PathBuf {
    crate::backup::home_dir().join("logs")
}

/// Newest Java or Rust crash report, bounded to `REPORT_READ_LIMIT` bytes.
pub fn latest_report() -> Option<CrashReport> {
    let entries = std::fs::read_dir(logs_dir()).ok()?;
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with("crash-") || !name.ends_with(FILE_SUFFIX) {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        if newest
            .as_ref()
            .map(|(time, _)| modified > *time)
            .unwrap_or(true)
        {
            newest = Some((modified, path));
        }
    }
    let (_, path) = newest?;
    let name = path.file_name()?.to_string_lossy().into_owned();
    let file = std::fs::File::open(&path).ok()?;
    let mut bytes = Vec::with_capacity(REPORT_READ_LIMIT.min(8 * 1024));
    file.take(REPORT_READ_LIMIT as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(CrashReport {
        name,
        text: String::from_utf8_lossy(&bytes).into_owned(),
    })
}

fn write_report(info: &std::panic::PanicHookInfo<'_>) {
    let directory = logs_dir();
    if std::fs::create_dir_all(&directory).is_err() {
        return;
    }
    let now = chrono::Local::now();
    let file_name = format!(
        "{FILE_PREFIX}{}{FILE_SUFFIX}",
        now.format("%Y%m%d-%H%M%S%.3f")
            .to_string()
            .replace('.', "-")
    );
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join(file_name))
    else {
        return;
    };
    let thread = std::thread::current();
    let thread_name = thread.name().unwrap_or("<未命名线程>");
    let payload = if let Some(text) = info.payload().downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = info.payload().downcast_ref::<String>() {
        text.clone()
    } else {
        "<非文本 panic 载荷>".to_string()
    };
    let location = info
        .location()
        .map(|location| {
            format!(
                "{}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            )
        })
        .unwrap_or_else(|| "<未知位置>".to_string());
    let _ = writeln!(
        file,
        "panic 时间: {}",
        now.format("%Y-%m-%d %H:%M:%S%.3f %:z")
    );
    let _ = writeln!(
        file,
        "进程: wunder 安卓版 v{} ({} {})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let _ = writeln!(file, "线程: {thread_name}");
    let _ = writeln!(file, "信息: {payload}");
    let _ = writeln!(file, "位置: {location}");
    let _ = writeln!(
        file,
        "回溯:\n{:?}",
        std::backtrace::Backtrace::force_capture()
    );
}

fn prune(directory: &Path) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut reports: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("crash-") && name.ends_with(FILE_SUFFIX))
        })
        .collect();
    // Timestamped names sort chronologically; Java reports share the prefix.
    reports.sort();
    while reports.len() > KEEP_REPORTS {
        let oldest = reports.remove(0);
        let _ = std::fs::remove_file(oldest);
    }
}
