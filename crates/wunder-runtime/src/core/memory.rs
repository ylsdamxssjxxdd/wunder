// 进程外的 cgroup 内存快照：管理端系统状态与内存诊断共用，避免两套读取口径。
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Default, Clone, Serialize)]
pub struct CgroupMemory {
    pub available: bool,
    pub version: u8,
    pub current_bytes: u64,
    pub limit_bytes: Option<u64>,
    pub anon_bytes: u64,
    pub file_bytes: u64,
    pub file_mapped_bytes: u64,
    pub active_file_bytes: u64,
    pub inactive_file_bytes: u64,
    pub shmem_bytes: u64,
    pub kernel_bytes: u64,
    pub kernel_stack_bytes: u64,
    pub slab_bytes: u64,
    pub slab_reclaimable_bytes: u64,
    pub sock_bytes: u64,
    pub pgfault: u64,
    pub pgmajfault: u64,
}

pub fn cgroup_memory() -> CgroupMemory {
    if Path::new("/sys/fs/cgroup/memory.current").exists() {
        return cgroup_v2();
    }
    if Path::new("/sys/fs/cgroup/memory/memory.usage_in_bytes").exists() {
        return cgroup_v1();
    }
    CgroupMemory::default()
}

fn read_trim(path: &str) -> Option<String> {
    fs::read_to_string(Path::new(path))
        .ok()
        .map(|value| value.trim().to_string())
}

fn parse_space_kv(text: &str) -> BTreeMap<String, u64> {
    let mut output = BTreeMap::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let Some(key) = parts.next() else {
            continue;
        };
        let Some(value) = parts.next() else {
            continue;
        };
        if let Ok(number) = value.parse::<u64>() {
            output.insert(key.to_string(), number);
        }
    }
    output
}

fn cgroup_v2() -> CgroupMemory {
    let current = read_trim("/sys/fs/cgroup/memory.current")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let max = read_trim("/sys/fs/cgroup/memory.max").unwrap_or_default();
    let stat = parse_space_kv(&read_trim("/sys/fs/cgroup/memory.stat").unwrap_or_default());
    let pick = |key: &str| stat.get(key).copied().unwrap_or(0);
    CgroupMemory {
        available: true,
        version: 2,
        current_bytes: current,
        limit_bytes: if max == "max" {
            None
        } else {
            max.parse::<u64>().ok()
        },
        anon_bytes: pick("anon"),
        file_bytes: pick("file"),
        file_mapped_bytes: pick("file_mapped"),
        active_file_bytes: pick("active_file"),
        inactive_file_bytes: pick("inactive_file"),
        shmem_bytes: pick("shmem"),
        kernel_bytes: pick("kernel"),
        kernel_stack_bytes: pick("kernel_stack"),
        slab_bytes: pick("slab"),
        slab_reclaimable_bytes: pick("slab_reclaimable"),
        sock_bytes: pick("sock"),
        pgfault: pick("pgfault"),
        pgmajfault: pick("pgmajfault"),
    }
}

fn cgroup_v1() -> CgroupMemory {
    let root = "/sys/fs/cgroup/memory";
    let current = read_trim(&format!("{root}/memory.usage_in_bytes"))
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let limit = read_trim(&format!("{root}/memory.limit_in_bytes"))
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value < u64::MAX / 2);
    let stat = parse_space_kv(&read_trim(&format!("{root}/memory.stat")).unwrap_or_default());
    let pick = |key: &str| stat.get(key).copied().unwrap_or(0);
    CgroupMemory {
        available: true,
        version: 1,
        current_bytes: current,
        limit_bytes: limit,
        anon_bytes: pick("total_rss"),
        file_bytes: pick("total_cache"),
        file_mapped_bytes: pick("total_mapped_file"),
        active_file_bytes: pick("active_file"),
        inactive_file_bytes: pick("inactive_file"),
        shmem_bytes: pick("total_shmem"),
        kernel_bytes: pick("kernel"),
        kernel_stack_bytes: pick("kernel_stack"),
        slab_bytes: pick("slab"),
        slab_reclaimable_bytes: pick("slab_reclaimable"),
        sock_bytes: pick("sock"),
        pgfault: 0,
        pgmajfault: 0,
    }
}
