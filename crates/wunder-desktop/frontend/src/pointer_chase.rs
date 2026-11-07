//! Pointer chase for drag gestures (Windows).
//!
//! 32 位 Win7 上，按住之后指针事件流会在几个像素内断掉：窗口在抓取被系统
//! 改写时会补一条合成的松开/取消，Slint 随即清掉按下态与抓取，之后 move 不再
//! 送到被拖的控件上。表现是线程行只闪一下就停、侧栏宽度拖一点就冻住，而同一
//! 时刻拖动窗口边缘的原生尺寸手势完全丝滑——说明鼠标与系统捕获都正常，断的是
//! 我们这条事件流。
//!
//! 拖拽因此不能只依赖事件流。这里在 Slint 侧声明的 `PointerChase` 全局上提供
//! 两件事：按帧读真实光标位置回灌（`pointer-x/pointer-y`），以及在主键真的松开
//! 时给出收尾信号（`finish-serial`）。`primary-held` 回调则用于判断事件流里那条
//! 松开到底是不是合成出来的。
use crate::{MainWindow, PointerChase};
use slint::{ComponentHandle, Timer, TimerMode};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

/// 与渲染帧同量级的追踪节奏：比事件流慢不了多少，又不至于把 32 位 Win7 的
/// UI 线程压住（每帧只有一次 GetCursorPos + GetAsyncKeyState）。
const CHASE_INTERVAL: Duration = Duration::from_millis(16);

thread_local! {
    static CHASE: Rc<Timer> = Rc::new(Timer::default());
    /// 上一次回灌的位置，避免同一位置重复写属性触发整列行重算。
    static LAST: Cell<Option<(f32, f32)>> = const { Cell::new(None) };
    /// 光标是否已在窗口外：只在从内到外的那一跳上报，避免每帧重复触发。
    static OUTSIDE: Cell<bool> = const { Cell::new(false) };
}

/// 装上追踪通道：声明平台能力，并把 Slint 侧的两个回调接到系统调用上。
pub fn install(app: &MainWindow) {
    let chase = app.global::<PointerChase>();
    chase.set_enabled(cfg!(windows));
    chase.on_primary_held(|| crate::os_drag::pointer_button_held());

    let weak = app.as_weak();
    chase.on_begin(move || {
        let Some(app) = weak.upgrade() else { return };
        start(&app);
    });
}

fn start(app: &MainWindow) {
    if !cfg!(windows) {
        return;
    }
    OUTSIDE.with(|slot| slot.set(false));
    let weak = app.as_weak();
    CHASE.with(|timer| {
        // 手势可能连着下一次按下：无条件重启，让节奏从这次按下重新计。
        timer.start(TimerMode::Repeated, CHASE_INTERVAL, move || {
            let Some(app) = weak.upgrade() else { return };
            tick(&app);
        });
    });
}

fn tick(app: &MainWindow) {
    let chase = app.global::<PointerChase>();
    if !chase.get_holding() {
        stop();
        return;
    }
    let Some((x, y, held)) = cursor_in_window(app) else {
        // 窗口暂时不可命中（最小化、正在销毁）：停掉，别空转。
        stop();
        return;
    };
    if LAST.with(|last| last.replace(Some((x, y))) != Some((x, y))) {
        // `length` 在 Rust 侧就是 f32（逻辑像素）。
        chase.set_pointer_x(x);
        chase.set_pointer_y(y);
    }
    // 拖拽把手势带出窗口（原生文件管理器的习惯用法）：只在从内到外的那一跳
    // 上报，由工作目录面板决定要不要转成 OS 拖出——其它手势（线程行排序、
    // 侧栏宽度）的转换请求会在面板里因为拖拽状态不匹配而落空。
    let (width, height) = window_logical_size(app);
    let outside = x < 0.0 || y < 0.0 || x > width || y > height;
    if outside && !OUTSIDE.with(Cell::get) {
        OUTSIDE.with(|slot| slot.set(true));
        crate::workspace_files::drag_left_window(app);
    } else if !outside {
        OUTSIDE.with(|slot| slot.set(false));
    }
    if !held {
        // 事件流没送来那条松开（或被我们判成合成松开而忽略），这里收尾。
        chase.set_finish_serial(chase.get_finish_serial().wrapping_add(1));
        chase.set_holding(false);
        stop();
    }
}

/// The window client area in logical pixels, the same space the chase feeds.
fn window_logical_size(app: &MainWindow) -> (f32, f32) {
    let scale = f64::from(app.window().scale_factor()).max(0.001);
    let size = app.window().size();
    (
        (f64::from(size.width) / scale) as f32,
        (f64::from(size.height) / scale) as f32,
    )
}

fn stop() {
    CHASE.with(|timer| timer.stop());
    LAST.with(|last| last.set(None));
}

/// 主键是否还按着、光标在窗口客户区里的逻辑坐标。
#[cfg(windows)]
fn cursor_in_window(app: &MainWindow) -> Option<(f32, f32, bool)> {
    imp::cursor_in_window(app)
}

#[cfg(not(windows))]
fn cursor_in_window(_app: &MainWindow) -> Option<(f32, f32, bool)> {
    None
}

#[cfg(windows)]
mod imp {
    use super::MainWindow;
    use slint::ComponentHandle;

    #[link(name = "user32")]
    extern "system" {
        fn GetCursorPos(point: *mut Point) -> i32;
        fn ScreenToClient(window: isize, point: *mut Point) -> i32;
        fn GetAsyncKeyState(key: i32) -> i16;
    }

    #[repr(C)]
    #[derive(Default)]
    struct Point {
        x: i32,
        y: i32,
    }

    const VK_LBUTTON: i32 = 0x01;

    pub(super) fn cursor_in_window(app: &MainWindow) -> Option<(f32, f32, bool)> {
        let hwnd = hwnd_of(app)?;
        let mut point = Point::default();
        if unsafe { GetCursorPos(&mut point) } == 0 {
            return None;
        }
        if unsafe { ScreenToClient(hwnd, &mut point) } == 0 {
            return None;
        }
        let scale = f64::from(app.window().scale_factor()).max(0.001) as f32;
        let held = (unsafe { GetAsyncKeyState(VK_LBUTTON) } as u16) & 0x8000 != 0;
        Some((point.x as f32 / scale, point.y as f32 / scale, held))
    }

    fn hwnd_of(app: &MainWindow) -> Option<isize> {
        use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let handle = app.window().window_handle();
        let RawWindowHandle::Win32(handle) = handle.window_handle().ok()?.as_raw() else {
            return None;
        };
        Some(handle.hwnd.get() as isize)
    }
}
