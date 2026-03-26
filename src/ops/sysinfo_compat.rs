use std::path::Path;
#[cfg(target_vendor = "win7")]
use sysinfo::{DiskExt, LoadAvg, ProcessRefreshKind, System, SystemExt};
#[cfg(not(target_vendor = "win7"))]
use sysinfo::{Disks, LoadAvg, ProcessRefreshKind, System};

pub type MonitorSystem = System;

#[cfg(target_vendor = "win7")]
pub type MonitorDisks = System;
#[cfg(not(target_vendor = "win7"))]
pub type MonitorDisks = Disks;

pub fn new_system() -> MonitorSystem {
    let mut system = System::new_all();
    system.refresh_all();
    system
}

pub fn new_disks() -> MonitorDisks {
    #[cfg(target_vendor = "win7")]
    {
        let mut system = System::new_all();
        system.refresh_disks_list();
        system.refresh_disks();
        system
    }
    #[cfg(not(target_vendor = "win7"))]
    {
        let mut disks = Disks::new_with_refreshed_list();
        disks.refresh();
        disks
    }
}

pub fn refresh_system_snapshot(system: &mut MonitorSystem, pid: Option<sysinfo::Pid>) {
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

pub fn load_average(system: &MonitorSystem) -> LoadAvg {
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

fn normalize_mount_path(path: &Path) -> String {
    let mut normalized = target_path_for_disk_match(path);
    let separator = std::path::MAIN_SEPARATOR;
    if normalized.len() > 1 && normalized.ends_with(separator) {
        normalized.pop();
    }
    normalized
}

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

pub fn disk_space(disks: &mut MonitorDisks, target_path: &Path) -> (u64, u64) {
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
