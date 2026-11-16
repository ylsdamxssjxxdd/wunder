//! 启动阶段耗时探针。
//!
//! 引擎内部的阶段耗时（尤其 `AppState::new_with_options` 里的存储初始化、
//! 技能加载、服务装配）一直只走 `tracing`。桌面版是 GUI 进程，没有安装
//! subscriber，这些点全部丢失，于是“老机器上启动停在加载运行时”这类反馈
//! 没有任何证据可查。
//!
//! 这里提供一个零依赖的 sink：由进程外壳注册一个函数指针，把同一批里程碑
//! 写进自己的启动日志；没注册时只保留 tracing 输出，不产生额外开销。

use std::sync::OnceLock;

/// 里程碑回调：阶段名与“从启动开始”的累计毫秒数。
type Sink = fn(&str, f64);

static SINK: OnceLock<Sink> = OnceLock::new();

/// 注册里程碑回调；重复注册忽略，保证一个进程只有一个落点。
pub fn install(sink: Sink) {
    let _ = SINK.set(sink);
}

/// 记录一个启动阶段耗时。保持原有 tracing 文案不变，服务端与命令行继续沿用。
pub fn point(name: &str, total_ms: f64) {
    tracing::info!("[startup][app-state] point={name} total_ms={total_ms:.1}");
    if let Some(sink) = SINK.get() {
        sink(name, total_ms);
    }
}
