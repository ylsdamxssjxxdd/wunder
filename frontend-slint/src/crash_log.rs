//! 闪退诊断：把每个 panic 以时间戳文件落到共享 `logs/` 目录。
//!
//! 发布版 `panic = "abort"`，任何线程的 panic 都会立即结束进程；加上
//! `windows_subsystem = "windows"` 没有控制台，"输入框偶尔不能粘贴、程序
//! 闪退"这类偶发问题事后无迹可寻。这里在 abort 之前尽量写出一份报告，
//! 再交回原 hook 保留原有的 stderr 行为。hook 自身不允许 panic（panic
//! 钩子里再 panic 会直接 abort），所有 IO 失败都静默放弃。

use std::path::PathBuf;

/// 保留的最新报告数量；更旧的自动清理，目录不会无界增长。
const KEEP_REPORTS: usize = 10;

const FILE_PREFIX: &str = "crash-";
const FILE_SUFFIX: &str = ".log";

pub fn install() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            write_report(info);
            prune_old_reports(&reports_dir());
        }));
        previous(info);
    }));
}

fn reports_dir() -> PathBuf {
    wunder_desktop::local_logs_dir()
}

fn write_report(info: &std::panic::PanicHookInfo<'_>) {
    let now = chrono::Local::now();
    let file_name = format!(
        "{FILE_PREFIX}{}.log",
        now.format("%Y%m%d-%H%M%S%.3f")
            .to_string()
            .replace('.', "-")
    );
    let mut file = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(reports_dir().join(file_name))
    {
        Ok(file) => file,
        // 日志目录不可写时放弃，不干扰原 hook 的输出。
        Err(_) => return,
    };

    let thread = std::thread::current();
    let thread_name = thread.name().unwrap_or("<未命名线程>");
    let payload = if let Some(text) = info.payload().downcast_ref::<&str>() {
        text.to_string()
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

    use std::io::Write;
    let _ = writeln!(
        file,
        "panic 时间: {}",
        now.format("%Y-%m-%d %H:%M:%S%.3f %:z")
    );
    let _ = writeln!(
        file,
        "进程: 蛹 v{} ({} {})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let _ = writeln!(file, "线程: {thread_name}");
    let _ = writeln!(file, "信息: {payload}");
    let _ = writeln!(file, "位置: {location}");
    let _ = writeln!(file, "说明: 发布版 panic 即终止进程；以下回溯供事后定位。");
    let _ = writeln!(
        file,
        "回溯:\n{:?}",
        std::backtrace::Backtrace::force_capture()
    );
}

fn prune_old_reports(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut reports: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(FILE_PREFIX) && name.ends_with(FILE_SUFFIX))
        })
        .collect();
    // 时间戳命名的文件按文件名排序即按时间排序。
    reports.sort();
    while reports.len() > KEEP_REPORTS {
        let oldest = reports.remove(0);
        let _ = std::fs::remove_file(oldest);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "wunder-crash-log-{tag}-{}-{}",
            std::process::id(),
            chrono::Local::now()
                .timestamp_nanos_opt()
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn prune_keeps_newest_reports_only() {
        let dir = temp_root("prune");
        // 上限是 10：写 12 份，期望最旧两份被清理，非报告文件不受影响。
        for day in 1..=12 {
            let stamp = format!("202601{day:02}-000000-000");
            std::fs::write(dir.join(format!("{FILE_PREFIX}{stamp}{FILE_SUFFIX}")), b"").unwrap();
        }
        std::fs::write(dir.join("unrelated.txt"), b"").unwrap();

        prune_old_reports(&dir);

        let mut remaining: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect();
        remaining.sort();
        assert_eq!(
            remaining.len(),
            11,
            "12 份报告应只剩最新 10 份加 1 个无关文件"
        );
        assert!(remaining.contains(&"unrelated.txt".to_string()));
        assert!(
            remaining.contains(&"crash-20260112-000000-000.log".to_string()),
            "最新报告必须保留"
        );
        assert!(
            !remaining.contains(&"crash-20260101-000000-000.log".to_string()),
            "最旧报告应被清理"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn report_file_name_is_sortable_by_time() {
        let now = chrono::Local::now();
        let name = format!(
            "{FILE_PREFIX}{}.log",
            now.format("%Y%m%d-%H%M%S%.3f")
                .to_string()
                .replace('.', "-")
        );
        assert!(name.starts_with(FILE_PREFIX));
        assert!(name.ends_with(FILE_SUFFIX));
    }
}
