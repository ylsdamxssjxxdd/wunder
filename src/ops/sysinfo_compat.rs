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

pub fn disk_space(disks: &mut MonitorDisks) -> (u64, u64) {
    #[cfg(target_vendor = "win7")]
    {
        // Keep a dedicated System instance for disk scanning to avoid sharing mutable state
        // with the CPU/memory sampler in MonitorState.
        disks.refresh_disks_list();
        disks.refresh_disks();
        let mut total = 0_u64;
        let mut free = 0_u64;
        for disk in disks.disks() {
            total = total.saturating_add(disk.total_space());
            free = free.saturating_add(disk.available_space());
        }
        (total, free)
    }

    #[cfg(not(target_vendor = "win7"))]
    {
        if disks.list().is_empty() {
            disks.refresh_list();
        }
        disks.refresh();
        let mut total = 0_u64;
        let mut free = 0_u64;
        for disk in disks.list() {
            total = total.saturating_add(disk.total_space());
            free = free.saturating_add(disk.available_space());
        }
        (total, free)
    }
}
