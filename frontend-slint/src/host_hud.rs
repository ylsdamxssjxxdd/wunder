//! 输入区底部「本机硬件状态」网格的数据接线。
//!
//! 采样（CPU / 内存）在 `wunder_desktop::native_host_hud` 的后台线程里跑，这里只
//! 做三件事：
//! 1. 每秒把最近一次快照投影成 Slint 属性，且只在数值真的变化时写，避免无谓的
//!    绑定重算；
//! 2. 把字节量格式化成悬停提示里放得下的紧凑文本（渲染路径不做换算）；
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
    cpu_text: String,
    memory_text: String,
    memory_used: String,
    memory_total: String,
    cores: i32,
}

impl Projection {
    fn from(sample: HostHud) -> Self {
        Self {
            cpu_ratio: (sample.cpu_percent / 100.0).clamp(0.0, 1.0),
            memory_ratio: (sample.memory_percent / 100.0).clamp(0.0, 1.0),
            cpu_text: format!("{:.0}%", sample.cpu_percent.clamp(0.0, 100.0)),
            memory_text: format!("{:.0}%", sample.memory_percent.clamp(0.0, 100.0)),
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
    // 文本只喂悬停提示，但同样只在变化时写：字符串比较比一次属性写入便宜。
    if projection.cpu_text != last.cpu_text {
        app.set_host_cpu_text(projection.cpu_text.as_str().into());
    }
    if projection.memory_text != last.memory_text {
        app.set_host_memory_text(projection.memory_text.as_str().into());
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
    fn ratios_are_clamped_and_rounded_for_the_hint() {
        let sample = HostHud {
            cpu_percent: 143.4,
            memory_percent: 61.6,
            memory_used: 1024 * 1024 * 1024 * 9,
            memory_total: 1024 * 1024 * 1024 * 16,
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
        assert_eq!(projection.cores, 32);
    }
}
