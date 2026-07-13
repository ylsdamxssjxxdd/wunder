#[cfg(target_os = "linux")]
use std::fs;
use std::thread;

pub const SERVER_WORKER_THREADS_ENV: &str = "WUNDER_SERVER_WORKER_THREADS";
pub const SERVER_MAX_BLOCKING_THREADS_ENV: &str = "WUNDER_SERVER_MAX_BLOCKING_THREADS";
pub const POSTGRES_RUNTIME_THREADS_ENV: &str = "WUNDER_POSTGRES_RUNTIME_THREADS";
pub const SESSION_RUN_WORKER_THREADS_ENV: &str = "WUNDER_SESSION_RUN_WORKER_THREADS";
pub const SESSION_RUN_MAX_BLOCKING_THREADS_ENV: &str = "WUNDER_SESSION_RUN_MAX_BLOCKING_THREADS";

const MAX_SERVER_WORKER_THREADS: usize = 128;
const MAX_SERVER_BLOCKING_THREADS: usize = 512;
const MAX_POSTGRES_RUNTIME_THREADS: usize = 32;
const MAX_SESSION_RUN_WORKER_THREADS: usize = 64;
const MAX_SESSION_RUN_BLOCKING_THREADS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerRuntimeThreads {
    pub worker_threads: usize,
    pub max_blocking_threads: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionRunRuntimeThreads {
    pub worker_threads: usize,
    pub max_blocking_threads: usize,
}

pub fn available_parallelism() -> usize {
    let host_parallelism = thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(2)
        .max(1);
    cgroup_cpu_parallelism().map_or(host_parallelism, |quota| host_parallelism.min(quota))
}

#[cfg(target_os = "linux")]
fn cgroup_cpu_parallelism() -> Option<usize> {
    fs::read_to_string("/sys/fs/cgroup/cpu.max")
        .ok()
        .as_deref()
        .and_then(parse_cgroup_v2_cpu_max)
        .or_else(|| {
            let quota = fs::read_to_string("/sys/fs/cgroup/cpu/cpu.cfs_quota_us").ok()?;
            let period = fs::read_to_string("/sys/fs/cgroup/cpu/cpu.cfs_period_us").ok()?;
            parse_cpu_quota(quota.trim(), period.trim())
        })
}

#[cfg(not(target_os = "linux"))]
fn cgroup_cpu_parallelism() -> Option<usize> {
    None
}

fn parse_cgroup_v2_cpu_max(raw: &str) -> Option<usize> {
    let mut parts = raw.split_whitespace();
    let quota = parts.next()?;
    let period = parts.next()?;
    parse_cpu_quota(quota, period)
}

fn parse_cpu_quota(quota: &str, period: &str) -> Option<usize> {
    if quota == "max" {
        return None;
    }
    let quota = quota.parse::<u64>().ok()?;
    let period = period.parse::<u64>().ok()?;
    if quota == 0 || period == 0 {
        return None;
    }
    Some(quota.div_ceil(period).max(1) as usize)
}

pub fn server_runtime_threads() -> ServerRuntimeThreads {
    let parallelism = available_parallelism();
    let worker_threads = env_thread_count(
        SERVER_WORKER_THREADS_ENV,
        parallelism.min(8),
        MAX_SERVER_WORKER_THREADS,
    );
    let max_blocking_threads = env_thread_count(
        SERVER_MAX_BLOCKING_THREADS_ENV,
        worker_threads.saturating_mul(4).clamp(16, 64),
        MAX_SERVER_BLOCKING_THREADS,
    );
    ServerRuntimeThreads {
        worker_threads,
        max_blocking_threads,
    }
}

pub fn postgres_runtime_threads() -> usize {
    env_thread_count(
        POSTGRES_RUNTIME_THREADS_ENV,
        available_parallelism().min(2),
        MAX_POSTGRES_RUNTIME_THREADS,
    )
}

pub fn session_run_runtime_threads() -> SessionRunRuntimeThreads {
    let worker_threads = env_thread_count(
        SESSION_RUN_WORKER_THREADS_ENV,
        available_parallelism().min(4),
        MAX_SESSION_RUN_WORKER_THREADS,
    );
    let max_blocking_threads = env_thread_count(
        SESSION_RUN_MAX_BLOCKING_THREADS_ENV,
        worker_threads.saturating_mul(8).clamp(16, 32),
        MAX_SESSION_RUN_BLOCKING_THREADS,
    );
    SessionRunRuntimeThreads {
        worker_threads,
        max_blocking_threads,
    }
}

fn env_thread_count(name: &str, default_value: usize, max_value: usize) -> usize {
    let raw = std::env::var(name).ok();
    normalize_thread_count(raw.as_deref(), default_value, max_value)
}

fn normalize_thread_count(raw: Option<&str>, default_value: usize, max_value: usize) -> usize {
    raw.and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default_value)
        .clamp(1, max_value.max(1))
}

#[cfg(test)]
mod tests {
    use super::{normalize_thread_count, parse_cgroup_v2_cpu_max, parse_cpu_quota};

    #[test]
    fn cpu_quota_rounds_fractional_cpu_up_and_ignores_unlimited_values() {
        assert_eq!(parse_cgroup_v2_cpu_max("150000 100000"), Some(2));
        assert_eq!(parse_cgroup_v2_cpu_max("max 100000"), None);
        assert_eq!(parse_cpu_quota("-1", "100000"), None);
        assert_eq!(parse_cpu_quota("0", "100000"), None);
    }

    #[test]
    fn thread_count_uses_default_for_missing_or_invalid_values() {
        assert_eq!(normalize_thread_count(None, 8, 128), 8);
        assert_eq!(normalize_thread_count(Some(""), 8, 128), 8);
        assert_eq!(normalize_thread_count(Some("0"), 8, 128), 8);
        assert_eq!(normalize_thread_count(Some("invalid"), 8, 128), 8);
    }

    #[test]
    fn thread_count_accepts_positive_override_and_caps_it() {
        assert_eq!(normalize_thread_count(Some(" 12 "), 8, 128), 12);
        assert_eq!(normalize_thread_count(Some("1024"), 8, 128), 128);
    }
}
