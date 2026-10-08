use std::path::Path;

#[cfg(all(feature = "host-metrics", target_vendor = "win7"))]
use sysinfo::{CpuExt, DiskExt, LoadAvg, ProcessExt, ProcessRefreshKind, System, SystemExt};
#[cfg(all(feature = "host-metrics", not(target_vendor = "win7")))]
use sysinfo::{Disks, LoadAvg, ProcessRefreshKind, System};

#[derive(Debug, Clone, Copy, Default)]
pub struct HostMetrics {
    pub cpu_percent: f32,
    pub memory_total: u64,
    pub memory_used: u64,
    pub memory_available: u64,
    pub process_rss: u64,
    pub process_cpu_percent: f32,
    pub load_avg_1: f64,
    pub load_avg_5: f64,
    pub load_avg_15: f64,
    pub disk_total: u64,
    pub disk_used: u64,
    pub disk_free: u64,
    pub disk_percent: f32,
}

#[cfg(feature = "host-metrics")]
pub type MonitorSystem = System;

#[cfg(not(feature = "host-metrics"))]
pub struct MonitorSystem;

#[cfg(all(feature = "host-metrics", target_vendor = "win7"))]
pub type MonitorDisks = System;

#[cfg(all(feature = "host-metrics", not(target_vendor = "win7")))]
pub type MonitorDisks = Disks;

#[cfg(not(feature = "host-metrics"))]
pub struct MonitorDisks;

pub fn new_system() -> MonitorSystem {
    #[cfg(feature = "host-metrics")]
    {
        // Delay expensive host discovery until the first actual snapshot refresh.
        return System::new();
    }

    #[cfg(not(feature = "host-metrics"))]
    {
        MonitorSystem
    }
}

pub fn new_disks() -> MonitorDisks {
    #[cfg(all(feature = "host-metrics", target_vendor = "win7"))]
    {
        // Win7 bridge startup must stay lightweight; disk enumeration is deferred.
        return System::new();
    }

    #[cfg(all(feature = "host-metrics", not(target_vendor = "win7")))]
    {
        return Disks::new();
    }

    #[cfg(not(feature = "host-metrics"))]
    {
        MonitorDisks
    }
}

#[cfg(feature = "host-metrics")]
pub fn collect_host_metrics(
    system: &mut MonitorSystem,
    disks: &mut MonitorDisks,
    workspace_root: &Path,
) -> HostMetrics {
    let pid = sysinfo::get_current_pid().ok();
    refresh_system_snapshot(system, pid);

    let cpu_percent = system.global_cpu_info().cpu_usage();
    let memory_total = system.total_memory();
    let memory_used = system.used_memory();
    let memory_available = memory_total.saturating_sub(memory_used);
    let mut process_rss = 0;
    let mut process_cpu_percent = 0.0;
    if let Some(pid) = pid {
        if let Some(process) = system.process(pid) {
            process_rss = process.memory();
            process_cpu_percent = process.cpu_usage();
        }
    }
    let load_avg = load_average(system);

    let (disk_total, disk_free) = disk_space(disks, workspace_root);
    let disk_used = disk_total.saturating_sub(disk_free);
    let disk_percent = if disk_total > 0 {
        (disk_used as f64 / disk_total as f64 * 100.0) as f32
    } else {
        0.0
    };

    HostMetrics {
        cpu_percent,
        memory_total,
        memory_used,
        memory_available,
        process_rss,
        process_cpu_percent,
        load_avg_1: load_avg.one,
        load_avg_5: load_avg.five,
        load_avg_15: load_avg.fifteen,
        disk_total,
        disk_used,
        disk_free,
        disk_percent,
    }
}

#[cfg(not(feature = "host-metrics"))]
pub fn collect_host_metrics(
    _system: &mut MonitorSystem,
    _disks: &mut MonitorDisks,
    _workspace_root: &Path,
) -> HostMetrics {
    HostMetrics::default()
}

#[cfg(feature = "host-metrics")]
fn refresh_system_snapshot(system: &mut MonitorSystem, pid: Option<sysinfo::Pid>) {
    #[cfg(target_vendor = "win7")]
    {
        system.refresh_cpu();
        system.refresh_memory();
        let refresh_kind = ProcessRefreshKind::new().with_cpu();
        if let Some(pid) = pid {
            system.refresh_process_specifics(pid, refresh_kind);
        } else {
            system.refresh_processes_specifics(refresh_kind);
        }
    }

    #[cfg(not(target_vendor = "win7"))]
    {
        system.refresh_cpu_usage();
        system.refresh_memory();
        let refresh_kind = ProcessRefreshKind::new().with_cpu().with_memory();
        if let Some(pid) = pid {
            system.refresh_process_specifics(pid, refresh_kind);
        } else {
            system.refresh_processes_specifics(refresh_kind);
        }
    }
}

#[cfg(feature = "host-metrics")]
fn load_average(system: &MonitorSystem) -> LoadAvg {
    #[cfg(target_vendor = "win7")]
    {
        system.load_average()
    }

    #[cfg(not(target_vendor = "win7"))]
    {
        let _ = system;
        System::load_average()
    }
}

#[cfg(feature = "host-metrics")]
fn target_path_for_disk_match(path: &Path) -> String {
    #[cfg(windows)]
    {
        // std::fs::canonicalize may return verbatim paths (`\\?\C:\...` / `\\?\UNC\...`),
        // while sysinfo mount points are usually regular drive/UNC paths.
        // Strip the verbatim prefix so mount matching works reliably.
        let mut normalized = path
            .to_string_lossy()
            .replace('/', "\\")
            .to_ascii_lowercase();
        if let Some(stripped) = normalized.strip_prefix(r"\\?\unc\") {
            normalized = format!(r"\\{stripped}");
        } else if let Some(stripped) = normalized.strip_prefix(r"\\?\") {
            normalized = stripped.to_string();
        }
        normalized
    }

    #[cfg(not(windows))]
    {
        path.to_string_lossy().into_owned()
    }
}

#[cfg(feature = "host-metrics")]
fn normalize_mount_path(path: &Path) -> String {
    let mut normalized = target_path_for_disk_match(path);
    let separator = std::path::MAIN_SEPARATOR;
    if normalized.len() > 1 && normalized.ends_with(separator) {
        normalized.pop();
    }
    normalized
}

#[cfg(feature = "host-metrics")]
fn mount_match_score(target_path: &Path, mount_path: &Path) -> Option<usize> {
    if target_path.starts_with(mount_path) {
        return Some(mount_path.components().count());
    }
    let target = normalize_mount_path(target_path);
    let mount = normalize_mount_path(mount_path);
    if target == mount {
        return Some(mount_path.components().count());
    }
    let prefixed = format!("{mount}{}", std::path::MAIN_SEPARATOR);
    if target.starts_with(&prefixed) {
        return Some(mount_path.components().count());
    }
    None
}

#[cfg(feature = "host-metrics")]
fn disk_space(disks: &mut MonitorDisks, target_path: &Path) -> (u64, u64) {
    let resolved_target = std::fs::canonicalize(target_path)
        .or_else(|_| {
            if target_path.is_absolute() {
                Err(std::io::Error::other("absolute path resolve failed"))
            } else {
                std::env::current_dir().map(|cwd| cwd.join(target_path))
            }
        })
        .unwrap_or_else(|_| target_path.to_path_buf());

    #[cfg(target_vendor = "win7")]
    {
        // Keep a dedicated System instance for disk scanning to avoid sharing mutable state
        // with the CPU/memory sampler in MonitorState.
        disks.refresh_disks_list();
        disks.refresh_disks();
        let mut best_match: Option<(usize, u64, u64)> = None;
        for disk in disks.disks() {
            let mount_point = disk.mount_point();
            let Some(score) = mount_match_score(&resolved_target, mount_point) else {
                continue;
            };
            let candidate = (score, disk.total_space(), disk.available_space());
            if best_match
                .as_ref()
                .map(|(best_score, _, _)| score > *best_score)
                .unwrap_or(true)
            {
                best_match = Some(candidate);
            }
        }
        if let Some((_, total, free)) = best_match {
            (total, free)
        } else {
            (0, 0)
        }
    }

    #[cfg(not(target_vendor = "win7"))]
    {
        if disks.list().is_empty() {
            disks.refresh_list();
        }
        disks.refresh();
        let mut best_match: Option<(usize, u64, u64)> = None;
        for disk in disks.list() {
            let mount_point = disk.mount_point();
            let Some(score) = mount_match_score(&resolved_target, mount_point) else {
                continue;
            };
            let candidate = (score, disk.total_space(), disk.available_space());
            if best_match
                .as_ref()
                .map(|(best_score, _, _)| score > *best_score)
                .unwrap_or(true)
            {
                best_match = Some(candidate);
            }
        }
        if let Some((_, total, free)) = best_match {
            (total, free)
        } else {
            (0, 0)
        }
    }
}
