//! 输入区底部「本机硬件状态」读数行的数据接线。
//!
//! 采样（CPU / 内存 / 网络 / 磁盘）在 `wunder_desktop::native_host_hud` 的后台线程里跑，这里只
//! 做三件事：
//! 1. 每秒把最近一次快照投影成 Slint 属性，且只在数值真的变化时写，避免无谓的
//!    绑定重算；
//! 2. 把字节量与速率格式化成详情面板里放得下的紧凑文本（渲染路径不做换算）；
//! 3. 主窗口隐藏到托盘时让采样器退避到 5s 节拍，并跳过这一帧的投影。
//!
//! 渲染线程上没有任何系统调用，单次投影的成本是「一次小结构体加锁拷贝 + 几次
//! 字符串比较」。

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use slint::{ComponentHandle, Timer, TimerMode};
use wunder_desktop::native_host_hud::{HostHud, HostHudSampler};

use crate::MainWindow;

/// 可见状态下的投影节拍。与采样器同频，避免同一份数据被重复取两次。
const TICK: Duration = Duration::from_millis(1000);

thread_local! {
    static TIMER: Timer = Timer::default();
    static BRIDGE: RefCell<Option<Rc<RefCell<Bridge>>>> = const { RefCell::new(None) };
}

struct Bridge {
    app: slint::Weak<MainWindow>,
    sampler: HostHudSampler,
    last: Projection,
}

/// 上一次投影到 UI 的值。逐字段比较，只写变化的属性。
#[derive(Default, PartialEq)]
struct Projection {
    cpu_ratio: f32,
    memory_ratio: f32,
    net_ratio: f32,
    disk_ratio: f32,
    cpu_text: String,
    memory_text: String,
    net_text: String,
    disk_text: String,
    link: String,
    net_rx: String,
    net_tx: String,
    disk_read: String,
    disk_write: String,
    memory_used: String,
    memory_total: String,
    cores: i32,
}

impl Projection {
    fn from(sample: HostHud) -> Self {
        // 网络与磁盘拿不到读数时用空串表达「当前不可用」，由 Slint 侧渲染成破折号，
        // 液位保持空——不能用 0% 假装「很闲」。
        let (net_ratio, net_text, link, net_rx, net_tx) = if sample.net_ok {
            (
                (sample.net_percent / 100.0).clamp(0.0, 1.0),
                format!("{:.0}%", sample.net_percent.clamp(0.0, 100.0)),
                link_speed(sample.net_link_bps),
                rate(sample.net_rx_bps),
                rate(sample.net_tx_bps),
            )
        } else {
            (
                0.0,
                String::new(),
                String::new(),
                String::new(),
                String::new(),
            )
        };
        let (disk_ratio, disk_text, disk_read, disk_write) = if sample.disk_ok {
            (
                (sample.disk_percent / 100.0).clamp(0.0, 1.0),
                format!("{:.0}%", sample.disk_percent.clamp(0.0, 100.0)),
                rate(sample.disk_read_bps),
                rate(sample.disk_write_bps),
            )
        } else {
            (0.0, String::new(), String::new(), String::new())
        };
        Self {
            cpu_ratio: (sample.cpu_percent / 100.0).clamp(0.0, 1.0),
            memory_ratio: (sample.memory_percent / 100.0).clamp(0.0, 1.0),
            net_ratio,
            disk_ratio,
            cpu_text: format!("{:.0}%", sample.cpu_percent.clamp(0.0, 100.0)),
            memory_text: format!("{:.0}%", sample.memory_percent.clamp(0.0, 100.0)),
            net_text,
            disk_text,
            link,
            net_rx,
            net_tx,
            disk_read,
            disk_write,
            memory_used: bytes(sample.memory_used),
            memory_total: bytes(sample.memory_total),
            cores: sample.cores as i32,
        }
    }
}

pub fn install(app: &MainWindow) {
    let bridge = Rc::new(RefCell::new(Bridge {
        app: app.as_weak(),
        sampler: HostHudSampler::start(TICK),
        last: Projection::default(),
    }));
    BRIDGE.with(|slot| *slot.borrow_mut() = Some(bridge.clone()));
    let weak = Rc::downgrade(&bridge);
    TIMER.with(|timer| {
        timer.start(TimerMode::Repeated, TICK, move || {
            // 弱引用：窗口关掉后定时器不会把 façade 留在内存里。
            let Some(bridge) = weak.upgrade() else {
                return;
            };
            tick(&mut bridge.borrow_mut());
        });
    });
}

fn tick(bridge: &mut Bridge) {
    let Some(app) = bridge.app.upgrade() else {
        return;
    };
    // 托盘驻留或最小化时用户看不到这块网格：跳过投影，采样器同时退避。
    let visible = app.window().is_visible();
    bridge.sampler.set_active(visible);
    if !visible {
        return;
    }
    let projection = Projection::from(bridge.sampler.snapshot());
    if projection == bridge.last {
        return;
    }
    let last = &bridge.last;
    if projection.cpu_ratio != last.cpu_ratio {
        app.set_host_cpu(projection.cpu_ratio);
    }
    if projection.memory_ratio != last.memory_ratio {
        app.set_host_memory(projection.memory_ratio);
    }
    if projection.net_ratio != last.net_ratio {
        app.set_host_net(projection.net_ratio);
    }
    if projection.disk_ratio != last.disk_ratio {
        app.set_host_disk(projection.disk_ratio);
    }
    // 文本只喂悬停提示，但同样只在变化时写：字符串比较比一次属性写入便宜。
    if projection.cpu_text != last.cpu_text {
        app.set_host_cpu_text(projection.cpu_text.as_str().into());
    }
    if projection.memory_text != last.memory_text {
        app.set_host_memory_text(projection.memory_text.as_str().into());
    }
    if projection.net_text != last.net_text {
        app.set_host_net_text(projection.net_text.as_str().into());
    }
    if projection.disk_text != last.disk_text {
        app.set_host_disk_text(projection.disk_text.as_str().into());
    }
    if projection.link != last.link {
        app.set_host_link(projection.link.as_str().into());
    }
    if projection.net_rx != last.net_rx {
        app.set_host_net_rx(projection.net_rx.as_str().into());
    }
    if projection.net_tx != last.net_tx {
        app.set_host_net_tx(projection.net_tx.as_str().into());
    }
    if projection.disk_read != last.disk_read {
        app.set_host_disk_read(projection.disk_read.as_str().into());
    }
    if projection.disk_write != last.disk_write {
        app.set_host_disk_write(projection.disk_write.as_str().into());
    }
    if projection.memory_used != last.memory_used {
        app.set_host_memory_used(projection.memory_used.as_str().into());
    }
    if projection.memory_total != last.memory_total {
        app.set_host_memory_total(projection.memory_total.as_str().into());
    }
    if projection.cores != last.cores {
        app.set_host_cores(projection.cores);
    }
    bridge.last = projection;
}

/// 链路速率（bit/s）→ "1.0G" / "100M"。只出现在网络提示里，格子里没有数字。
fn link_speed(bits_per_second: u64) -> String {
    if bits_per_second == 0 {
        return String::new();
    }
    let gbit = bits_per_second as f64 / 1_000_000_000.0;
    if gbit >= 1.0 {
        return format!("{gbit:.1}G");
    }
    format!("{:.0}M", bits_per_second as f64 / 1_000_000.0)
}

/// 速率（字节/秒）→ "1.2M" / "48K"。面板里统一拼成 "↓1.2M/s"，这里只给数量。
fn rate(bytes_per_second: u64) -> String {
    if bytes_per_second == 0 {
        return "0B".to_string();
    }
    let mut amount = bytes_per_second as f64;
    for unit in ["B", "K", "M", "G"] {
        if amount < 1024.0 {
            return format!("{}{unit}", decimal(amount));
        }
        amount /= 1024.0;
    }
    format!("{}T", decimal(amount))
}

/// 字节量 → "9.9G" / "683G"。用于内存提示里的绝对用量。
fn bytes(value: u64) -> String {
    let mut amount = value as f64;
    for unit in ["B", "K", "M", "G"] {
        if amount < 1024.0 {
            return format!("{}{unit}", decimal(amount));
        }
        amount /= 1024.0;
    }
    format!("{}T", decimal(amount))
}

/// 小于 10 保留一位小数（1.2G），否则取整（683G）——提示里只要一个量级。
fn decimal(value: f64) -> String {
    if value < 10.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.0}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_switch_units_at_the_threshold() {
        assert_eq!(bytes(0), "0.0B");
        assert_eq!(bytes(1024), "1.0K");
        assert_eq!(bytes(1024 * 1024 * 1024 * 16), "16G");
    }

    #[test]
    fn link_speed_collapses_to_one_reading() {
        assert_eq!(link_speed(0), "");
        assert_eq!(link_speed(100_000_000), "100M");
        assert_eq!(link_speed(1_000_000_000), "1.0G");
        assert_eq!(link_speed(2_500_000_000), "2.5G");
        assert_eq!(link_speed(137_600_000), "138M");
    }

    #[test]
    fn rate_reads_at_a_glance() {
        assert_eq!(rate(0), "0B");
        assert_eq!(rate(900), "900B");
        assert_eq!(rate(1536), "1.5K");
        assert_eq!(rate(12 * 1024 * 1024), "12M");
        assert_eq!(rate(3 * 1024 * 1024 * 1024), "3.0G");
    }

    #[test]
    fn unavailable_usage_projects_to_empty_readings() {
        let sample = HostHud {
            net_ok: false,
            disk_ok: false,
            ..HostHud::default()
        };
        let projection = Projection::from(sample);
        assert_eq!(projection.net_ratio, 0.0);
        assert_eq!(projection.disk_ratio, 0.0);
        assert!(projection.net_text.is_empty() && projection.disk_text.is_empty());
        assert!(projection.link.is_empty());
        assert!(projection.net_rx.is_empty() && projection.net_tx.is_empty());
        assert!(projection.disk_read.is_empty() && projection.disk_write.is_empty());
    }

    #[test]
    fn ratios_are_clamped_and_rounded_for_the_hint() {
        let sample = HostHud {
            cpu_percent: 143.4,
            memory_percent: 61.6,
            memory_used: 1024 * 1024 * 1024 * 9,
            memory_total: 1024 * 1024 * 1024 * 16,
            net_percent: 12.4,
            net_link_bps: 1_000_000_000,
            net_rx_bps: 1_500_000,
            net_tx_bps: 48 * 1024,
            net_ok: true,
            disk_percent: 88.0,
            disk_read_bps: 12 * 1024 * 1024,
            disk_write_bps: 0,
            disk_ok: true,
            cores: 32,
            available: true,
        };
        let projection = Projection::from(sample);
        // 条长必须落在 0..=1，否则会画出父级之外。
        assert_eq!(projection.cpu_ratio, 1.0);
        assert_eq!(projection.cpu_text, "100%");
        assert_eq!(projection.memory_ratio, 0.616);
        assert_eq!(projection.memory_text, "62%");
        assert_eq!(projection.memory_used, "9.0G");
        assert_eq!(projection.memory_total, "16G");
        assert_eq!(projection.net_ratio, 0.124);
        assert_eq!(projection.net_text, "12%");
        assert_eq!(projection.link, "1.0G");
        assert_eq!(projection.disk_ratio, 0.88);
        assert_eq!(projection.disk_text, "88%");
        assert_eq!(projection.net_rx, "1.4M");
        assert_eq!(projection.net_tx, "48K");
        assert_eq!(projection.disk_read, "12M");
        assert_eq!(projection.disk_write, "0B");
        assert_eq!(projection.cores, 32);
    }
}
