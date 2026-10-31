//! 本机硬件状态采样：CPU 占用与内存占用。
//!
//! 设计约束（蛹的运行环境是 32 位 Win7 与 ubuntu18.04，且要求低开销）：
//! - 采样在独立线程里按固定节拍进行，UI 线程只取最近一次快照，渲染路径上
//!   没有任何系统调用；两条路径只共享一把保护小结构体的互斥锁，且只在
//!   每秒一次的快速拷贝里持有。
//! - 每个平台只保留最小的原生探针：Windows 走 kernel32 的 `GetSystemTimes` 与
//!   `GlobalMemoryStatusEx`，Linux 读 `/proc/stat` 与 `/proc/meminfo`。不引入
//!   跨平台采集依赖，避免体积与启动成本，也保证 Win7 32 位这条老链路上的依赖
//!   图不变。
//! - 两个指标都是「累计计数器的增量比」，不做平滑也不重采，因此单次采样成本
//!   恒定且极小：Windows 约 2 个 P/Invoke，Linux 两次小文件读取。
//! - 窗口不可见时由调用方 `set_active(false)`，采样周期退避到 [`IDLE_INTERVAL`]，
//!   避免后台驻留时每秒唤醒。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 窗口不可见（托盘驻留、最小化）时的采样周期。
const IDLE_INTERVAL: Duration = Duration::from_secs(5);

/// 一次采样的结果。字段全部是「已经算好的数值」，前端只负责格式化。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HostHud {
    /// CPU 占用百分比（0..=100，全核平均）。
    pub cpu_percent: f32,
    /// 物理内存占用百分比（0..=100）。
    pub memory_percent: f32,
    /// 已用物理内存（字节）。
    pub memory_used: u64,
    /// 物理内存总量（字节）。
    pub memory_total: u64,
    /// 逻辑核心数，0 表示未知。
    pub cores: u32,
    /// CPU/内存是否可用；平台不支持时为 false，前端据此显示占位符。
    pub available: bool,
}

/// 采样器句柄。内部持有一个脱离线程，`Drop` 时通知线程退出。
pub struct HostHudSampler {
    latest: Arc<Mutex<HostHud>>,
    active: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
}

impl HostHudSampler {
    /// 启动采样线程。`interval` 是窗口可见时的节拍，通常 1s。
    pub fn start(interval: Duration) -> Self {
        let interval = interval.max(Duration::from_millis(200));
        let latest = Arc::new(Mutex::new(HostHud::default()));
        let active = Arc::new(AtomicBool::new(true));
        let running = Arc::new(AtomicBool::new(true));
        let (latest_thread, active_thread, running_thread) =
            (latest.clone(), active.clone(), running.clone());
        let _ = std::thread::Builder::new()
            .name("host-hud".into())
            .spawn(move || {
                let mut probe = platform::Probe::new();
                // 先采一次基线：CPU 是差分量，没有基线就无法在第一个节拍给出
                // 有效占用（否则会报出一个从开机累计至今的均值）。
                let _ = probe.sample();
                loop {
                    let wait = if active_thread.load(Ordering::Relaxed) {
                        interval
                    } else {
                        IDLE_INTERVAL
                    };
                    std::thread::sleep(wait);
                    if !running_thread.load(Ordering::Relaxed) {
                        return;
                    }
                    let sample = probe.sample();
                    if let Ok(mut slot) = latest_thread.lock() {
                        *slot = sample;
                    }
                }
            });
        Self {
            latest,
            active,
            running,
        }
    }

    /// 最近一次采样。拷贝量固定（约 40 字节），不做分配。
    pub fn snapshot(&self) -> HostHud {
        self.latest.lock().map(|value| *value).unwrap_or_default()
    }

    /// 窗口可见性变化时调用；不可见只降低采样频率，不停止（恢复时即刻有值）。
    pub fn set_active(&self, active: bool) {
        self.active.store(active, Ordering::Relaxed);
    }
}

impl Drop for HostHudSampler {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }
}

fn cores() -> u32 {
    std::thread::available_parallelism()
        .map(|count| count.get() as u32)
        .unwrap_or(1)
}

fn percent(used: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        (used as f64 / total as f64 * 100.0) as f32
    }
}

#[cfg(windows)]
mod platform {
    use super::{cores, percent, HostHud};
    use std::mem::zeroed;
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows_sys::Win32::System::Threading::GetSystemTimes;

    pub struct Probe {
        last_idle: u64,
        last_kernel: u64,
        last_user: u64,
        cpu_percent: f32,
        cores: u32,
    }

    impl Probe {
        pub fn new() -> Self {
            let (last_idle, last_kernel, last_user) = cpu_times().unwrap_or((0, 0, 0));
            Self {
                last_idle,
                last_kernel,
                last_user,
                cpu_percent: 0.0,
                cores: cores(),
            }
        }

        pub fn sample(&mut self) -> HostHud {
            let (memory_used, memory_total) = self.memory();
            self.update_cpu();
            HostHud {
                cpu_percent: self.cpu_percent,
                memory_percent: percent(memory_used, memory_total),
                memory_used,
                memory_total,
                cores: self.cores,
                available: true,
            }
        }

        fn update_cpu(&mut self) {
            let Some((idle, kernel, user)) = cpu_times() else {
                return;
            };
            // kernel 时间包含 idle，所以总时间是 kernel + user。
            let idle_delta = idle.saturating_sub(self.last_idle);
            let total =
                kernel.saturating_sub(self.last_kernel) + user.saturating_sub(self.last_user);
            if total > 0 {
                let busy = total.saturating_sub(idle_delta);
                self.cpu_percent = (busy as f64 / total as f64 * 100.0) as f32;
            }
            self.last_idle = idle;
            self.last_kernel = kernel;
            self.last_user = user;
        }

        fn memory(&self) -> (u64, u64) {
            let mut status: MEMORYSTATUSEX = unsafe { zeroed() };
            status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
            let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
            if ok == 0 {
                return (0, 0);
            }
            (
                status.ullTotalPhys.saturating_sub(status.ullAvailPhys),
                status.ullTotalPhys,
            )
        }
    }

    fn cpu_times() -> Option<(u64, u64, u64)> {
        let mut idle: FILETIME = unsafe { zeroed() };
        let mut kernel: FILETIME = unsafe { zeroed() };
        let mut user: FILETIME = unsafe { zeroed() };
        let ok = unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) };
        if ok == 0 {
            return None;
        }
        Some((filetime(idle), filetime(kernel), filetime(user)))
    }

    fn filetime(value: FILETIME) -> u64 {
        ((value.dwHighDateTime as u64) << 32) | value.dwLowDateTime as u64
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::{cores, percent, HostHud};
    use std::fs::File;
    use std::io::Read;

    pub struct Probe {
        cores: u32,
        cpu_busy: Option<u64>,
        cpu_total: Option<u64>,
        cpu_percent: f32,
    }

    impl Probe {
        pub fn new() -> Self {
            Self {
                cores: cores(),
                cpu_busy: None,
                cpu_total: None,
                cpu_percent: 0.0,
            }
        }

        pub fn sample(&mut self) -> HostHud {
            // 两个文件各一次小读取，返回的都是 Copy 的累计量，读取结束后不再借用
            // 自身，随后再更新差分。
            let cpu = read("/proc/stat").map(cpu_totals);
            let memory = read("/proc/meminfo").map(memory_totals).unwrap_or((0, 0));
            self.update_cpu(cpu);
            HostHud {
                cpu_percent: self.cpu_percent,
                memory_percent: percent(memory.0, memory.1),
                memory_used: memory.0,
                memory_total: memory.1,
                cores: self.cores,
                available: true,
            }
        }

        fn update_cpu(&mut self, cpu: Option<(u64, u64)>) {
            let Some((busy, total)) = cpu else {
                return;
            };
            // CPU 占用必须是「忙碌增量 / 总增量」，不能各自差分后再相除。
            if let (Some(last_busy), Some(last_total)) = (self.cpu_busy, self.cpu_total) {
                let busy_delta = busy.saturating_sub(last_busy);
                let total_delta = total.saturating_sub(last_total);
                if total_delta > 0 {
                    self.cpu_percent = (busy_delta as f64 / total_delta as f64 * 100.0) as f32;
                }
            }
            self.cpu_busy = Some(busy);
            self.cpu_total = Some(total);
        }
    }

    /// 读一个小 `/proc` 文件。两者都在几 KB 以内。
    fn read(path: &str) -> Option<String> {
        let mut file = File::open(path).ok()?;
        let mut buffer = String::with_capacity(4096);
        file.read_to_string(&mut buffer).ok()?;
        Some(buffer)
    }

    /// `cpu` 行：user nice system idle iowait irq softirq steal。
    /// 返回（累计忙碌时长, 累计总时长），单位是内核的 USER_HZ。
    pub(super) fn cpu_totals(data: String) -> (u64, u64) {
        let line = data.lines().next().unwrap_or("");
        let mut fields = line.split_whitespace();
        let _ = fields.next();
        let mut values = [0u64; 8];
        for slot in values.iter_mut() {
            *slot = fields
                .next()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);
        }
        let idle = values[3].saturating_add(values[4]);
        let total: u64 = values.iter().sum();
        (total.saturating_sub(idle), total)
    }

    fn memory_totals(data: String) -> (u64, u64) {
        let mut total = 0u64;
        let mut available = 0u64;
        let mut fallback = 0u64;
        for line in data.lines() {
            let Some((key, rest)) = line.split_once(':') else {
                continue;
            };
            let value = rest
                .split_whitespace()
                .next()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);
            match key {
                "MemTotal" => total = value,
                "MemAvailable" => available = value,
                // 3.14 之前的内核没有 MemAvailable，用空闲+缓存近似。
                "MemFree" => fallback = fallback.saturating_add(value),
                "Buffers" | "Cached" => fallback = fallback.saturating_add(value),
                _ => {}
            }
        }
        let available = if available > 0 { available } else { fallback };
        // /proc/meminfo 的单位是 kB。
        (
            total.saturating_sub(available).saturating_mul(1024),
            total.saturating_mul(1024),
        )
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod platform {
    use super::HostHud;

    pub struct Probe;

    impl Probe {
        pub fn new() -> Self {
            Self
        }

        pub fn sample(&mut self) -> HostHud {
            HostHud::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_guards_against_a_zero_total() {
        assert_eq!(percent(0, 0), 0.0);
        assert_eq!(percent(1, 4), 25.0);
    }

    #[test]
    fn sampler_publishes_a_snapshot() {
        let sampler = HostHudSampler::start(Duration::from_millis(200));
        // 只验证「会发布」，不验证时延：首个快照依赖线程调度。
        let mut snapshot = sampler.snapshot();
        for _ in 0..40 {
            if snapshot.available {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
            snapshot = sampler.snapshot();
        }
        assert!(snapshot.available);
        assert!(snapshot.memory_total > 0);
        assert!(snapshot.cpu_percent >= 0.0 && snapshot.cpu_percent <= 100.0);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parses_proc_stat() {
        use super::platform::cpu_totals;
        let (busy, total) = cpu_totals("cpu  10 0 10 80 0 0 0 0 0 0\n".to_string());
        assert_eq!(busy, 20);
        assert_eq!(total, 100);
    }
}

#[cfg(all(test, windows))]
mod windows_probe {
    use super::platform::Probe;
    use std::time::{Duration, Instant};

    /// 单次采样必须便宜：它每秒跑一次，成本要在微秒级而不是毫秒级。
    #[test]
    fn probe_stays_cheap() {
        let mut probe = Probe::new();
        let first = probe.sample();
        assert!(first.available);
        std::thread::sleep(Duration::from_millis(250));
        let started = Instant::now();
        let second = probe.sample();
        let single = started.elapsed();
        println!("single={single:?} sample={second:?}");
        assert!(single < Duration::from_millis(20));
        assert!(second.cpu_percent >= 0.0 && second.cpu_percent <= 100.0);
    }
}
