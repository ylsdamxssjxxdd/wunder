//! 输入区「后台任务」格的接线：本线程里仍在执行的命令工具。
//!
//! 数据源是运行时的命令会话表（每条命令工具调用都会登记一条），因此这里不维护
//! 第二份任务表，只做每秒一次的投影：
//! - 命令行的单行化与截断、时长格式化都在这里做完，渲染只排版；
//! - 展开态与已读到的输出尾部留在 Rust，模型重建不会把用户展开的那一行收起来；
//! - 终止走该条命令自己的取消令牌，只影响这一条，不动线程本身。

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use slint::{ComponentHandle, Model, ModelRc, Timer, TimerMode, VecModel};
use wunder_desktop::{NativeCommandTask, NativeDesktop};

use crate::{CommandTask, MainWindow};

/// 与硬件网格同频；命令会话表是内存结构，空表时遍历代价接近零。
const TICK: Duration = Duration::from_millis(1000);
/// 面板最多列这么多条，其余折成「还有 N 条未显示」，保证面板高度有界。
const MAX_ROWS: usize = 6;
/// 展开详情的输出预算：尾部行数与字符数双限，长输出不会撑爆模型。
const DETAIL_LINES: usize = 10;
const DETAIL_CHARS: usize = 1200;

thread_local! {
    static TIMER: Timer = Timer::default();
    static BRIDGE: RefCell<Option<Rc<RefCell<Bridge>>>> = const { RefCell::new(None) };
}

struct Bridge {
    app: slint::Weak<MainWindow>,
    desktop: Arc<NativeDesktop>,
    /// 稳定的行模型：只做逐行替换，面板不会因为每秒刷新而重建。
    model: Rc<VecModel<CommandTask>>,
    /// 上一次投影的行，用于挑出真正变化的行。
    rows: Vec<CommandTask>,
    /// 用户展开的任务及其已渲染的输出尾部。
    expanded: HashSet<String>,
    details: HashMap<String, String>,
    /// 上次投影所属线程；切线程时展开态与缓存一并丢弃。
    session: String,
}

pub fn install(app: &MainWindow, desktop: Arc<NativeDesktop>) {
    let model = Rc::new(VecModel::<CommandTask>::default());
    app.set_running_tasks(ModelRc::from(model.clone()));
    let bridge = Rc::new(RefCell::new(Bridge {
        app: app.as_weak(),
        desktop,
        model,
        rows: Vec::new(),
        expanded: HashSet::new(),
        details: HashMap::new(),
        session: String::new(),
    }));
    BRIDGE.with(|slot| *slot.borrow_mut() = Some(bridge.clone()));

    let weak = Rc::downgrade(&bridge);
    app.on_toggle_task(move |id| {
        let Some(bridge) = weak.upgrade() else { return };
        let mut bridge = bridge.borrow_mut();
        let id = id.to_string();
        if bridge.expanded.contains(&id) {
            bridge.expanded.remove(&id);
        } else {
            bridge.expanded.insert(id.clone());
            // 展开时才读输出尾部：折叠状态下的每秒轮询不搬运这几 KB。
            if !bridge.details.contains_key(&id) {
                // 先对齐线程，避免刚启动、第一次投影还没跑时读到空会话。
                sync_session(&mut bridge);
                let detail = bridge
                    .desktop
                    .command_task_detail(&bridge.session, &id)
                    .map(|detail| tail(&detail_text(&detail)))
                    .unwrap_or_default();
                bridge.details.insert(id, detail);
            }
        }
        tick(&mut bridge);
    });

    let weak = Rc::downgrade(&bridge);
    app.on_terminate_task(move |id| {
        let Some(bridge) = weak.upgrade() else { return };
        let mut bridge = bridge.borrow_mut();
        sync_session(&mut bridge);
        if bridge
            .desktop
            .terminate_command_task(&bridge.session, id.as_str())
        {
            // 取消是异步生效的：这一帧先按「即将消失」刷新，下一帧由运行时收尾。
            bridge.expanded.remove(id.as_str());
            bridge.details.remove(id.as_str());
            tick(&mut bridge);
        }
    });

    let weak = Rc::downgrade(&bridge);
    TIMER.with(|timer| {
        timer.start(TimerMode::Repeated, TICK, move || {
            let Some(bridge) = weak.upgrade() else { return };
            tick(&mut bridge.borrow_mut());
        });
    });
}

/// 跟随当前线程。切线程即丢弃展开态与输出缓存，避免把 A 线程的任务详情
/// 挂到 B 线程的同名 id 上（id 是命令会话的全局唯一值，但仍按线程隔离更稳）。
fn sync_session(bridge: &mut Bridge) {
    let Some(app) = bridge.app.upgrade() else {
        return;
    };
    let session = app.get_active_session_id().to_string();
    if session != bridge.session {
        bridge.session = session;
        bridge.expanded.clear();
        bridge.details.clear();
    }
}

fn tick(bridge: &mut Bridge) {
    let Some(app) = bridge.app.upgrade() else {
        return;
    };
    // 窗口隐藏到托盘时没人看这块面板，采样与投影一起停。
    if !app.window().is_visible() {
        return;
    }
    sync_session(bridge);
    let session = bridge.session.clone();
    let tasks = if session.is_empty() || app.get_cloud_chat_active() {
        Vec::new()
    } else {
        bridge.desktop.running_command_tasks(&session)
    };
    let count = tasks.len() as i32;
    if app.get_running_task_count() != count {
        app.set_running_task_count(count);
    }

    // 已结束的任务不再占着展开态与输出缓存。
    let live: HashSet<&str> = tasks.iter().map(|task| task.id.as_str()).collect();
    bridge.expanded.retain(|id| live.contains(id.as_str()));
    bridge.details.retain(|id, _| live.contains(id.as_str()));

    let rows = bridge.rows_for(&tasks);
    publish(bridge, rows);
}

impl Bridge {
    /// 把运行时任务投影成面板行。超出 `MAX_ROWS` 的部分由任务计数体现。
    fn rows_for(&self, tasks: &[NativeCommandTask]) -> Vec<CommandTask> {
        tasks
            .iter()
            .take(MAX_ROWS)
            .map(|task| {
                let expanded = self.expanded.contains(&task.id);
                let output = self.details.get(&task.id).cloned().unwrap_or_default();
                CommandTask {
                    id: task.id.as_str().into(),
                    command: command_line(&task.command).into(),
                    cwd: task.cwd.as_str().into(),
                    elapsed: elapsed(task.elapsed_ms).into(),
                    expanded,
                    detail_state: if !expanded {
                        0
                    } else if output.is_empty() {
                        2
                    } else {
                        1
                    },
                    output: output.into(),
                }
            })
            .collect()
    }
}

/// 逐行比较后写模型：形状变了才整表替换，否则只替换真正变化的行，
/// 这样每秒刷新不会重建展开行、也不会把指针下的行换成新实例。
fn publish(bridge: &mut Bridge, rows: Vec<CommandTask>) {
    let reshaped = rows.len() != bridge.rows.len()
        || rows
            .iter()
            .zip(bridge.rows.iter())
            .any(|(next, previous)| next.id != previous.id);
    if reshaped {
        bridge.model.set_vec(rows.clone());
    } else {
        for (index, row) in rows.iter().enumerate() {
            if bridge.rows.get(index) != Some(row) {
                bridge.model.set_row_data(index, row.clone());
            }
        }
    }
    bridge.rows = rows;
}

/// 命令行本身可能带换行（heredoc、多行脚本），面板里压成一行再截断。
fn command_line(command: &str) -> String {
    let mut line = String::with_capacity(command.len().min(160));
    for (index, part) in command
        .split(['\n', '\r'])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .enumerate()
    {
        if index > 0 {
            line.push(' ');
        }
        line.push_str(part);
        if line.chars().count() > 160 {
            break;
        }
    }
    if line.chars().count() > 160 {
        let head: String = line.chars().take(159).collect();
        return format!("{head}…");
    }
    line
}

/// 已运行时长。"12s" / "3m20s" / "1h05m"。
fn elapsed(ms: i64) -> String {
    let seconds = (ms / 1000).max(0);
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m{:02}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h{:02}m", seconds / 3600, (seconds % 3600) / 60)
    }
}

/// 输出细节拼成一段可读文本：交互式会话优先 pty，否则 stdout 后接 stderr。
fn detail_text(detail: &wunder_desktop::NativeCommandTaskDetail) -> String {
    let mut text = if !detail.pty_tail.trim().is_empty() {
        detail.pty_tail.clone()
    } else {
        let mut text = detail.stdout_tail.clone();
        if !detail.stderr_tail.trim().is_empty() {
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(&detail.stderr_tail);
        }
        text
    };
    if let Some(error) = detail
        .error
        .as_deref()
        .filter(|error| !error.trim().is_empty())
    {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(error);
    }
    text
}

/// 只保留尾部若干行与固定字符预算，并去掉 ANSI 控制序列——面板里是纯文本，
/// 保留转义码只会显示成乱码。
fn tail(text: &str) -> String {
    let cleaned = strip_ansi(text);
    let mut lines: Vec<&str> = cleaned.lines().collect();
    if lines.len() > DETAIL_LINES {
        lines = lines.split_off(lines.len() - DETAIL_LINES);
    }
    let mut out = String::with_capacity(DETAIL_CHARS.min(cleaned.len()));
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        out.push_str(line);
        if out.chars().count() > DETAIL_CHARS {
            break;
        }
    }
    let trimmed = out.trim_end_matches(['\r', ' ', '\t']).to_string();
    if trimmed.chars().count() > DETAIL_CHARS {
        let head: String = trimmed
            .chars()
            .skip(trimmed.chars().count() - DETAIL_CHARS)
            .collect();
        return format!("…{head}");
    }
    trimmed
}

/// 去掉 CSI / OSC 转义与其余不可打印控制符，保留换行与制表。
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\u{1b}' => match chars.next() {
                // CSI: ESC [ ... 终止于 0x40..=0x7e
                Some('[') => {
                    for next in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&next) {
                            break;
                        }
                    }
                }
                // OSC: ESC ] ... 终止于 BEL 或 ESC \
                Some(']') => {
                    while let Some(next) = chars.next() {
                        if next == '\u{7}' {
                            break;
                        }
                        if next == '\u{1b}' {
                            let _ = chars.next();
                            break;
                        }
                    }
                }
                _ => {}
            },
            '\r' => {}
            '\n' | '\t' => out.push(ch),
            ch if ch.is_control() => {}
            ch => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elapsed_reads_as_a_duration() {
        assert_eq!(elapsed(0), "0s");
        assert_eq!(elapsed(38_000), "38s");
        assert_eq!(elapsed(134_000), "2m14s");
        assert_eq!(elapsed(3_900_000), "1h05m");
    }

    #[test]
    fn command_lines_collapse_to_one_line() {
        assert_eq!(
            command_line("  npm run build\n  --workspace=x  "),
            "npm run build --workspace=x"
        );
        let long = command_line(&"a".repeat(400));
        assert_eq!(long.chars().count(), 160);
        assert!(long.ends_with('…'));
    }

    #[test]
    fn output_tail_keeps_the_last_lines_and_drops_escapes() {
        let text = (1..=20)
            .map(|index| format!("\u{1b}[32mline {index}\u{1b}[0m"))
            .collect::<Vec<_>>()
            .join("\r\n");
        let tail = tail(&text);
        assert!(tail.starts_with("line 11"));
        assert!(tail.ends_with("line 20"));
        assert!(!tail.contains('\u{1b}'));
    }
}
