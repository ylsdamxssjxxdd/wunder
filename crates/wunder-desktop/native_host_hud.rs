//! 本机硬件状态采样：CPU、内存、网络、磁盘四项占用。
//!
//! 四个指标都归一成 0..=100 的「占用百分比」，前端因此能用同一套刻度表示，
//! 不需要区分「速率」和「比例」两种量。另外附上网络双向速率与磁盘读写速率，
//! 供输入区的详情面板显示具体吞吐：
//! - CPU：忙碌增量 / 总增量；
//! - 内存：已用 / 物理总量；
//! - 网络：最忙那块网卡的收发速率 / 它自己的链路速率；
//! - 磁盘：整盘累计忙碌时间 / 实际间隔。
//!
//! 设计约束（蛹的运行环境是 32 位 Win7 与 ubuntu18.04，且要求低开销）：
//! - 采样在独立线程里按固定节拍进行，UI 线程只取最近一次快照，渲染路径上
//!   没有任何系统调用；两条路径只共享一把保护小结构体的互斥锁。
//! - 每个平台只保留最小的原生探针：Windows 走 kernel32 / iphlpapi / PDH，
//!   Linux 走 `/proc` 与 `/sys`。不引入跨平台采集依赖，避免体积与启动成本。
//! - 窗口不可见时由调用方 `set_active(false)`，采样周期退避到 [`IDLE_INTERVAL`]，
//!   避免后台驻留时每秒唤醒。
//!
//! 网络为什么按「最忙的那块网卡」而不是「全部网卡吞吐 / 全部链路带宽」：开发机上
//! 常年挂着 Hyper-V / WSL / VMware 的虚拟网卡，它们自称 10Gbps 却几乎不跑流量，
//! 按总和算真实上下行会被稀释到 0.1% 以下，图标永远是空的。取单块网卡自身的占用
//! 率，读数才跟着真正在用的那条链路走。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 窗口不可见（托盘驻留、最小化）时的采样周期。
const IDLE_INTERVAL: Duration = Duration::from_secs(5);
/// 差分间隔上界。机器休眠、调试断点后间隔会非常大，直接用会让占用率失真；
/// 超过该值只重设基线，不产出新值。
const MAX_GAP_SECONDS: f64 = 30.0;

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
    /// 网络占用百分比（0..=100）：最忙那块网卡的收发速率占其链路速率的比例。
    pub net_percent: f32,
    /// 上面那块网卡的链路速率（bit/s），0 表示未知。
    pub net_link_bps: u64,
    /// 上面那块网卡的接收速率（字节/秒）。
    pub net_rx_bps: u64,
    /// 上面那块网卡的发送速率（字节/秒）。
    pub net_tx_bps: u64,
    /// 磁盘占用百分比（0..=100）：整盘忙碌时间占比。
    pub disk_percent: f32,
    /// 整盘读取速率（字节/秒）。
    pub disk_read_bps: u64,
    /// 整盘写入速率（字节/秒）。
    pub disk_write_bps: u64,
    /// 逻辑核心数，0 表示未知。
    pub cores: u32,
    /// CPU/内存是否可用；平台不支持时为 false。
    pub available: bool,
    /// 本次是否测到了网络占用（没有可用链路时为 false）。
    pub net_ok: bool,
    /// 本次是否测到了磁盘占用（部分系统关闭了磁盘性能计数器）。
    pub disk_ok: bool,
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
                // 探针初始化有一次性成本（Windows 上 PDH 首次查询要加载性能计数器
                // 库，约 300~400ms），放在后台线程里，不碰首帧。
                let mut probe = platform::Probe::new();
                let mut last = std::time::Instant::now();
                // 先采一次基线：四项都是差分量，没有基线就无法在第一个节拍给出
                // 有效占用（否则会报出一个从开机累计至今的均值）。
                let _ = probe.sample(0.0);
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
                    let dt = last.elapsed().as_secs_f64();
                    last = std::time::Instant::now();
                    // 间隔过大（休眠/断点）时只重设基线，避免出现假读数。
                    let sample = if dt > MAX_GAP_SECONDS {
                        probe.sample(0.0)
                    } else {
                        probe.sample(dt)
                    };
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

    /// 最近一次采样。拷贝量固定（几十字节），不做分配。
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

/// 一块网卡的一次观测：累计收发字节与链路速率（bit/s，0 表示未知）。
pub(crate) struct LinkSample {
    pub id: String,
    pub received: u64,
    pub transmitted: u64,
    pub speed_bps: u64,
}

#[derive(Default)]
struct LinkState {
    received: u64,
    transmitted: u64,
}

/// 一块网卡在两次采样之间的读数：占用率 + 链路速率 + 双向实时速率。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LinkReading {
    /// 收发合计占链路带宽的百分比（0..=100）。
    pub usage: f32,
    /// 该网卡的链路速率（bit/s）。
    pub speed_bps: u64,
    /// 接收速率（字节/秒）。
    pub rx_bps: u64,
    /// 发送速率（字节/秒）。
    pub tx_bps: u64,
}

/// 逐网卡的差值状态。取「占用率最大的那块」，返回它自己的完整读数。
#[derive(Default)]
pub(crate) struct LinkMeter {
    links: HashMap<String, LinkState>,
}

impl LinkMeter {
    pub(crate) fn update(&mut self, dt: f64, rows: &[LinkSample]) -> Option<LinkReading> {
        if rows.is_empty() {
            self.links.clear();
            return None;
        }
        let mut next = HashMap::with_capacity(rows.len());
        let mut busiest: Option<LinkReading> = None;
        for row in rows {
            let mut reading = LinkReading {
                usage: 0.0,
                speed_bps: row.speed_bps,
                rx_bps: 0,
                tx_bps: 0,
            };
            if let Some(previous) = self.links.get(&row.id) {
                if dt > 0.0 {
                    let rx = row.received.saturating_sub(previous.received) as f64 / dt;
                    let tx = row.transmitted.saturating_sub(previous.transmitted) as f64 / dt;
                    reading.rx_bps = rx as u64;
                    reading.tx_bps = tx as u64;
                    if row.speed_bps > 0 {
                        let capacity = row.speed_bps as f64 / 8.0;
                        reading.usage = ((rx + tx) / capacity * 100.0).clamp(0.0, 100.0) as f32;
                    }
                }
            }
            // 只有报得出链路速率的接口才能当「占用率」的基准；虚拟网卡空闲时
            // 天然是 0%，不会盖掉真正在用的那条。
            if row.speed_bps > 0
                && busiest
                    .map(|seen| reading.usage > seen.usage)
                    .unwrap_or(true)
            {
                busiest = Some(reading);
            }
            next.insert(
                row.id.clone(),
                LinkState {
                    received: row.received,
                    transmitted: row.transmitted,
                },
            );
        }
        self.links = next;
        busiest
    }
}

#[cfg(windows)]
mod platform {
    use super::{cores, percent, HostHud, LinkMeter, LinkSample};
    use std::mem::zeroed;
    use std::ptr;
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        FreeMibTable, GetIfTable2, IF_TYPE_SOFTWARE_LOOPBACK, MIB_IF_ROW2, MIB_IF_TABLE2,
    };
    use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows_sys::Win32::System::Performance::{
        PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterValue,
        PdhOpenQueryW, PDH_FMT_COUNTERVALUE, PDH_FMT_DOUBLE,
    };
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows_sys::Win32::System::Threading::GetSystemTimes;

    /// `_Total` 实例的磁盘忙碌时间占比。PDH 自己按两次采集的间隔折算，这里不再
    /// 差分。用英文计数器名，避免本地化系统上路径随语言漂移。
    const DISK_BUSY_PATH: &str = r"\PhysicalDisk(_Total)\% Disk Time";
    /// 整盘吞吐。`* Bytes/sec` 是 PDH 自己算好的速率，直接读即可，不用再差分。
    const DISK_READ_PATH: &str = r"\PhysicalDisk(_Total)\Disk Read Bytes/sec";
    const DISK_WRITE_PATH: &str = r"\PhysicalDisk(_Total)\Disk Write Bytes/sec";

    pub struct Probe {
        last_idle: u64,
        last_kernel: u64,
        last_user: u64,
        cpu_percent: f32,
        cores: u32,
        links: LinkMeter,
        disk: Option<DiskProbe>,
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
                links: LinkMeter::default(),
                disk: DiskProbe::new(),
            }
        }

        /// `dt` 为与上次采样的实际间隔（秒）；0 表示只重设基线。
        pub fn sample(&mut self, dt: f64) -> HostHud {
            let (memory_used, memory_total) = self.memory();
            self.update_cpu(dt);
            let mut out = HostHud {
                cpu_percent: self.cpu_percent,
                memory_percent: percent(memory_used, memory_total),
                memory_used,
                memory_total,
                cores: self.cores,
                available: true,
                ..HostHud::default()
            };
            if let Some(rows) = net_samples() {
                if let Some(reading) = self.links.update(dt, &rows) {
                    out.net_percent = reading.usage;
                    out.net_link_bps = reading.speed_bps;
                    out.net_rx_bps = reading.rx_bps;
                    out.net_tx_bps = reading.tx_bps;
                    out.net_ok = true;
                }
            }
            if let Some(disk) = self.disk.as_mut() {
                if let Some((busy, read, write)) = disk.sample(dt) {
                    out.disk_percent = busy;
                    out.disk_read_bps = read;
                    out.disk_write_bps = write;
                    out.disk_ok = true;
                }
            }
            out
        }

        fn update_cpu(&mut self, dt: f64) {
            let Some((idle, kernel, user)) = cpu_times() else {
                return;
            };
            if dt > 0.0 {
                // kernel 时间包含 idle，所以总时间是 kernel + user。
                let idle_delta = idle.saturating_sub(self.last_idle);
                let total =
                    kernel.saturating_sub(self.last_kernel) + user.saturating_sub(self.last_user);
                if total > 0 {
                    let busy = total.saturating_sub(idle_delta);
                    self.cpu_percent = (busy as f64 / total as f64 * 100.0) as f32;
                }
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

    /// 处于 Up 状态的非回环接口。虚拟网卡也留在里面——占用率按各自链路速率算，
    /// 空闲的虚拟网卡自然得到 0%，不会污染真正在用的那条。
    fn net_samples() -> Option<Vec<LinkSample>> {
        let mut table: *mut MIB_IF_TABLE2 = ptr::null_mut();
        let status = unsafe { GetIfTable2(&mut table) };
        if status != 0 || table.is_null() {
            return None;
        }
        let mut rows = Vec::new();
        unsafe {
            let count = (*table).NumEntries as usize;
            let entries: *const MIB_IF_ROW2 = ptr::addr_of!((*table).Table) as *const MIB_IF_ROW2;
            for index in 0..count {
                let row = &*entries.add(index);
                if row.Type == IF_TYPE_SOFTWARE_LOOPBACK || row.OperStatus != IfOperStatusUp {
                    continue;
                }
                rows.push(LinkSample {
                    id: row.InterfaceIndex.to_string(),
                    received: row.InOctets,
                    transmitted: row.OutOctets,
                    speed_bps: row.TransmitLinkSpeed.max(row.ReceiveLinkSpeed),
                });
            }
            FreeMibTable(table as *const _);
        }
        Some(rows)
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

    struct DiskProbe {
        query: isize,
        busy: isize,
        read: isize,
        write: isize,
    }

    impl DiskProbe {
        fn new() -> Option<Self> {
            unsafe {
                let mut query = 0isize;
                if PdhOpenQueryW(ptr::null(), 0, &mut query) != 0 {
                    return None;
                }
                // 忙碌占比是主指标，缺它整块磁盘读数就没有意义；吞吐两个计数器缺
                // 一个只影响详情里的速率，按「读不到就报 0」处理，不让整块失效。
                let busy = add_counter(query, DISK_BUSY_PATH)?;
                let read = add_counter(query, DISK_READ_PATH).unwrap_or(0);
                let write = add_counter(query, DISK_WRITE_PATH).unwrap_or(0);
                if PdhCollectQueryData(query) != 0 {
                    PdhCloseQuery(query);
                    return None;
                }
                Some(Self {
                    query,
                    busy,
                    read,
                    write,
                })
            }
        }

        /// 第一次调用只建立基线（PDH 需要一个间隔才能算出百分比），返回 None。
        fn sample(&mut self, dt: f64) -> Option<(f32, u64, u64)> {
            unsafe {
                if PdhCollectQueryData(self.query) != 0 {
                    return None;
                }
                let busy = read_counter(self.busy)?;
                if dt <= 0.0 {
                    return None;
                }
                let read = read_counter(self.read).unwrap_or(0.0).max(0.0) as u64;
                let write = read_counter(self.write).unwrap_or(0.0).max(0.0) as u64;
                // 多盘合计可能超过 100%，对读数意义不大，夹住即可。
                Some(((busy as f32).clamp(0.0, 100.0), read, write))
            }
        }
    }

    /// 把英文计数器名挂到查询上。失败时自己不留残句柄，由调用方决定是否降级。
    unsafe fn add_counter(query: isize, path: &str) -> Option<isize> {
        let mut counter = 0isize;
        let wide_path = wide(path);
        let status = unsafe { PdhAddEnglishCounterW(query, wide_path.as_ptr(), 0, &mut counter) };
        if status == 0 {
            Some(counter)
        } else {
            None
        }
    }

    impl Drop for DiskProbe {
        fn drop(&mut self) {
            unsafe {
                PdhCloseQuery(self.query);
            }
        }
    }

    unsafe fn read_counter(counter: isize) -> Option<f64> {
        let mut value: PDH_FMT_COUNTERVALUE = unsafe { zeroed() };
        let status = unsafe {
            PdhGetFormattedCounterValue(counter, PDH_FMT_DOUBLE, ptr::null_mut(), &mut value)
        };
        if status != 0 {
            return None;
        }
        Some(unsafe { value.Anonymous.doubleValue })
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::{cores, percent, HostHud, LinkMeter, LinkSample};
    use std::fs;
    use std::fs::File;
    use std::io::Read;

    /// `/sys/class/net/<if>/speed` 的单位是 Mbit/s。
    const MBIT: u64 = 1_000_000;

    pub struct Probe {
        cores: u32,
        cpu_busy: Option<u64>,
        cpu_total: Option<u64>,
        cpu_percent: f32,
        links: LinkMeter,
        /// 累计（io_ticks 毫秒, 读字节, 写字节）。
        disk_prev: Option<(u64, u64, u64)>,
        disk_percent: f32,
        disk_read_bps: u64,
        disk_write_bps: u64,
    }

    impl Probe {
        pub fn new() -> Self {
            Self {
                cores: cores(),
                cpu_busy: None,
                cpu_total: None,
                cpu_percent: 0.0,
                links: LinkMeter::default(),
                disk_prev: None,
                disk_percent: 0.0,
                disk_read_bps: 0,
                disk_write_bps: 0,
            }
        }

        pub fn sample(&mut self, dt: f64) -> HostHud {
            // 每个文件一次小读取，返回的都是 Copy 的累计量，读取结束后不再借用
            // 自身，随后再更新差分。
            let cpu = read("/proc/stat").map(cpu_totals);
            let memory = read("/proc/meminfo").map(memory_totals).unwrap_or((0, 0));
            self.update_cpu(cpu, dt);

            let mut out = HostHud {
                cpu_percent: self.cpu_percent,
                memory_percent: percent(memory.0, memory.1),
                memory_used: memory.0,
                memory_total: memory.1,
                cores: self.cores,
                available: true,
                ..HostHud::default()
            };

            if let Some(rows) = read("/proc/net/dev").map(net_samples) {
                if let Some(reading) = self.links.update(dt, &rows) {
                    out.net_percent = reading.usage;
                    out.net_link_bps = reading.speed_bps;
                    out.net_rx_bps = reading.rx_bps;
                    out.net_tx_bps = reading.tx_bps;
                    out.net_ok = true;
                }
            }

            if let Some(totals) = read("/proc/diskstats").map(disk_totals) {
                if let Some(previous) = self.disk_prev {
                    if dt > 0.0 {
                        // io_ticks 的单位是毫秒。
                        self.disk_percent =
                            (totals.0.saturating_sub(previous.0) as f64 / (dt * 1000.0) * 100.0)
                                .clamp(0.0, 100.0) as f32;
                        self.disk_read_bps =
                            (totals.1.saturating_sub(previous.1) as f64 / dt) as u64;
                        self.disk_write_bps =
                            (totals.2.saturating_sub(previous.2) as f64 / dt) as u64;
                    }
                }
                self.disk_prev = Some(totals);
                out.disk_percent = self.disk_percent;
                out.disk_read_bps = self.disk_read_bps;
                out.disk_write_bps = self.disk_write_bps;
                out.disk_ok = true;
            }

            out
        }

        fn update_cpu(&mut self, cpu: Option<(u64, u64)>, dt: f64) {
            let Some((busy, total)) = cpu else {
                return;
            };
            // CPU 占用必须是「忙碌增量 / 总增量」，不能各自差分后再相除。
            if dt > 0.0 {
                if let (Some(last_busy), Some(last_total)) = (self.cpu_busy, self.cpu_total) {
                    let busy_delta = busy.saturating_sub(last_busy);
                    let total_delta = total.saturating_sub(last_total);
                    if total_delta > 0 {
                        self.cpu_percent = (busy_delta as f64 / total_delta as f64 * 100.0) as f32;
                    }
                }
            }
            self.cpu_busy = Some(busy);
            self.cpu_total = Some(total);
        }
    }

    /// 读一个小 `/proc` 文件。都在几 KB 以内。
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

    /// `/proc/net/dev`：跳过回环；链路速率另从 sysfs 取（读不到按未知处理，
    /// 该网卡仍参与差值统计，只是不贡献占用率）。
    pub(super) fn net_samples(data: String) -> Vec<LinkSample> {
        let mut rows = Vec::new();
        for line in data.lines().skip(2) {
            let Some((name, rest)) = line.split_once(':') else {
                continue;
            };
            let name = name.trim();
            if name.is_empty() || name == "lo" {
                continue;
            }
            let mut fields = rest.split_whitespace();
            let received = fields
                .next()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);
            // 收字节之后依次是收包数、错误、丢弃等 7 列，第 8 列才是发包字节数。
            let transmitted = fields
                .nth(7)
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);
            rows.push(LinkSample {
                id: name.to_string(),
                received,
                transmitted,
                speed_bps: link_speed_bps(name),
            });
        }
        rows
    }

    /// `/sys/class/net/*/speed` 给出 Mbit/s，虚拟/未连接接口会返回 -1 或读失败。
    fn link_speed_bps(name: &str) -> u64 {
        fs::read_to_string(format!("/sys/class/net/{name}/speed"))
            .ok()
            .and_then(|text| text.trim().parse::<i64>().ok())
            .filter(|value| *value > 0)
            .map(|mbit| mbit as u64 * MBIT)
            .unwrap_or(0)
    }

    /// 第 3 列是设备名，其后第 10 个字段（`io_ticks`，毫秒）是该盘的累计忙碌时间。
    /// 同时累加读/写扇区（第 3、7 个字段）换算成字节，供详情面板显示吞吐。
    pub(super) fn disk_totals(data: String) -> (u64, u64, u64) {
        const SECTOR: u64 = 512;
        let mut busy = 0u64;
        let mut read = 0u64;
        let mut write = 0u64;
        for line in data.lines() {
            let mut fields = line.split_whitespace();
            let _major = fields.next();
            let _minor = fields.next();
            let Some(name) = fields.next() else {
                continue;
            };
            if !is_whole_disk(name) {
                continue;
            }
            let mut values = [0u64; 11];
            for slot in values.iter_mut() {
                *slot = fields
                    .next()
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(0);
            }
            // values: 读完成、读合并、读扇区… 写完成、写合并、写扇区… 之后第 10 项
            // （0 基的 9）才是 io_ticks。
            read = read.saturating_add(values[2].saturating_mul(SECTOR));
            write = write.saturating_add(values[6].saturating_mul(SECTOR));
            busy = busy.saturating_add(values[9]);
        }
        (busy, read, write)
    }

    /// 只累计整盘，避免分区与整盘各算一次。名字规则按内核命名习惯：
    /// `sda`/`vda`/`xvda` 以字母结尾的是整盘（`sda1` 是分区）；
    /// `nvme0n1`/`mmcblk0` 不带 `p` 的是整盘（`nvme0n1p1` 是分区）。
    pub(super) fn is_whole_disk(name: &str) -> bool {
        const VIRTUAL: [&str; 9] = [
            "loop", "ram", "zram", "dm-", "md", "sr", "fd", "nbd", "drbd",
        ];
        if VIRTUAL.iter().any(|prefix| name.starts_with(prefix)) {
            return false;
        }
        if name.starts_with("nvme") || name.starts_with("mmcblk") {
            return !name.contains('p');
        }
        name.chars()
            .last()
            .map(|last| last.is_ascii_alphabetic())
            .unwrap_or(false)
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

        pub fn sample(&mut self, _dt: f64) -> HostHud {
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
    fn link_meter_needs_a_baseline_then_reports_the_busiest_link() {
        let mut meter = LinkMeter::default();
        // 1Gbps = 125MB/s。首帧只登记基线。
        let first = vec![
            LinkSample {
                id: "wired".into(),
                received: 1_000,
                transmitted: 1_000,
                speed_bps: 1_000_000_000,
            },
            LinkSample {
                id: "virtual".into(),
                received: 0,
                transmitted: 0,
                speed_bps: 10_000_000_000,
            },
        ];
        let first = meter.update(1.0, &first).expect("有链路速率就应该有读数");
        assert_eq!(
            first,
            LinkReading {
                usage: 0.0,
                speed_bps: 1_000_000_000,
                rx_bps: 0,
                tx_bps: 0,
            },
            "首个节拍没有基线，速率与占用都应为 0"
        );

        // 有线网卡 1 秒内收 25,000,000 字节 = 125,000,000 B/s 的 20%；虚拟网卡仍闲着。
        let second = vec![
            LinkSample {
                id: "wired".into(),
                received: 1_000 + 25_000_000,
                transmitted: 2_000,
                speed_bps: 1_000_000_000,
            },
            LinkSample {
                id: "virtual".into(),
                received: 0,
                transmitted: 0,
                speed_bps: 10_000_000_000,
            },
        ];
        let reading = meter.update(1.0, &second).expect("仍然有读数");
        assert_eq!(reading.speed_bps, 1_000_000_000);
        assert!(
            (reading.usage - 20.0).abs() < 0.01,
            "usage={}",
            reading.usage
        );
        // 收发分向也要单独给出：详情面板里是「↓ rx ↑ tx」两个数。
        assert_eq!(reading.rx_bps, 25_000_000);
        assert_eq!(reading.tx_bps, 1_000);
    }

    #[test]
    fn link_meter_reports_nothing_without_a_known_speed() {
        let mut meter = LinkMeter::default();
        let rows = vec![LinkSample {
            id: "unknown".into(),
            received: 10,
            transmitted: 10,
            speed_bps: 0,
        }];
        assert_eq!(meter.update(0.0, &rows), None);
    }

    #[test]
    fn sampler_publishes_a_snapshot() {
        let sampler = HostHudSampler::start(Duration::from_millis(200));
        // 只验证「会发布」，不验证时延：首个快照依赖线程调度与 PDH 初始化。
        let mut snapshot = sampler.snapshot();
        for _ in 0..60 {
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
    fn parses_proc_samples() {
        use super::platform::{cpu_totals, disk_totals, is_whole_disk, net_samples};
        let (busy, total) = cpu_totals("cpu  10 0 10 80 0 0 0 0 0 0\n".to_string());
        assert_eq!(busy, 20);
        assert_eq!(total, 100);

        assert!(is_whole_disk("sda"));
        assert!(!is_whole_disk("sda1"));
        assert!(is_whole_disk("nvme0n1"));
        assert!(!is_whole_disk("nvme0n1p1"));
        assert!(!is_whole_disk("loop0"));

        let rows = net_samples(
            "Inter-|   Receive\n face |bytes\n  eth0: 100 0 0 0 0 0 0 0 40 0 0 0 0 0 0 0\n"
                .to_string(),
        );
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].received, rows[0].transmitted), (100, 40));

        // 两个整盘：io_ticks 各 30ms / 20ms，读扇区 200 + 400，写扇区 20 + 40；
        // 分区不重复计入。
        let (busy, read, write) = disk_totals(
            "   8       0 sda 10 0 200 30 5 0 20 20 0 30 0\n   8       1 sda1 1 0 20 5 1 0 40 3 0 5 0\n   8      16 sdb 10 0 400 20 5 0 40 20 0 20 0\n"
                .to_string(),
        );
        assert_eq!(busy, 50);
        assert_eq!(read, (200 + 400) * 512);
        assert_eq!(write, (20 + 40) * 512);
    }
}

#[cfg(all(test, windows))]
mod windows_probe {
    use super::platform::Probe;
    use std::time::{Duration, Instant};

    /// 单次采样必须便宜：它每秒跑一次，接口枚举是主要开销。
    #[test]
    fn probe_stays_cheap() {
        let mut probe = Probe::new();
        let first = probe.sample(0.0);
        assert!(first.available);
        std::thread::sleep(Duration::from_millis(250));
        let started = Instant::now();
        let second = probe.sample(0.25);
        let single = started.elapsed();
        println!("single={single:?} sample={second:?}");
        assert!(single < Duration::from_millis(20));
        assert!(second.cpu_percent >= 0.0 && second.cpu_percent <= 100.0);
    }
}
