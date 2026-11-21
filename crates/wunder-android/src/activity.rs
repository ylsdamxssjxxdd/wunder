//! 模型浮层的「后台任务 / 子智能体」投影。
//!
//! 桌面版把这两块放在输入区的两个浮层里（`host_hud.slint` 的任务面板、
//! `subagent_dock.slint` 的子智能体面板），手机没有悬停，改放在点开模型选项后
//! 呼出的浮层里、上下文占用下面。数据与桌面版同源：
//! - 后台任务取 `desktop.running_command_tasks`（运行时命令会话的轻量投影）；
//! - 子智能体取本线程已加载回合里的 `subagent_run` 卡片，状态与耗时都由卡片
//!   自己报告，手机侧不另建一张任务表。
//!
//! 两个列表都按屏宽裁剪：手机浮层放不下更多条目，余下的由「还有 N 条未显示」
//! 收口，计数仍按真实总数投影。

use crate::project::{STATUS_DONE, STATUS_FAILED, STATUS_RUNNING};
use crate::{CommandTask, SubagentItem};
use wunder_desktop::{NativeChatTurn, NativeCommandTask, NativeSubagentCard};

/// 浮层里最多显示的条目数。
pub const MAX_TASKS: usize = 2;
pub const MAX_SUBAGENTS: usize = 2;

/// 一次投影的结果。整体比较后只在内容变化时写回 `Shared`，避免每秒刷新都让
/// Slint 重建模型。
#[derive(Clone, PartialEq, Default)]
pub struct ActivityView {
    pub tasks: Vec<CommandTask>,
    /// 仍在运行的命令总数（可能大于 `tasks.len()`）。
    pub task_count: i32,
    pub subagents: Vec<SubagentItem>,
    /// 本线程所有子智能体（含已结束），与桌面版浮层的计数口径一致。
    pub subagent_count: i32,
}

/// 把命令任务与回合里的子智能体卡片摊平成浮层行。
pub fn project(
    tasks: &[NativeCommandTask],
    turns: &[NativeChatTurn],
    now_s: f64,
) -> ActivityView {
    let cards: Vec<&NativeSubagentCard> = turns
        .iter()
        .flat_map(|turn| turn.subagents.iter())
        .collect();
    project_cards(tasks, &cards, now_s)
}

/// 同上，但卡片直接来自线程日志（浮层打开时引擎按秒回读一次，与桌面版浮层
/// 的口径一致：正在跑的、已结束的都算，跨回合不漏）。
pub fn project_cards(
    tasks: &[NativeCommandTask],
    cards: &[&NativeSubagentCard],
    now_s: f64,
) -> ActivityView {
    let task_count = tasks.len() as i32;
    let rows: Vec<CommandTask> = tasks
        .iter()
        .take(MAX_TASKS)
        .map(|task| CommandTask {
            id: task.id.as_str().into(),
            command: command_line(&task.command).into(),
            // 手机不展开输出尾部：完整命令、工作目录与输出只给桌面版的悬停面板。
            full_command: "".into(),
            cwd: "".into(),
            elapsed: elapsed(task.elapsed_ms).into(),
            expanded: false,
            output: "".into(),
            detail_state: 0,
        })
        .collect();

    let subagent_count = cards.len() as i32;
    let mut cards = cards.to_vec();
    // 还在跑的排在最前，其余按开始时间倒序：浮层里最该先看到的是正在进行的工作。
    cards.sort_by(|a, b| match (a.is_running(), b.is_running()) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => b
            .started_at
            .partial_cmp(&a.started_at)
            .unwrap_or(std::cmp::Ordering::Equal),
    });

    ActivityView {
        tasks: rows,
        task_count,
        subagents: cards
            .iter()
            .take(MAX_SUBAGENTS)
            .map(|card| project_subagent(card, now_s))
            .collect(),
        subagent_count,
    }
}

fn project_subagent(card: &NativeSubagentCard, now_s: f64) -> SubagentItem {
    let status_kind = if card.failed || matches!(card.status.as_str(), "failed" | "error") {
        STATUS_FAILED
    } else if card.is_running() {
        STATUS_RUNNING
    } else {
        STATUS_DONE
    };
    let message = if card.latest_message.trim().is_empty() {
        card.error_message.as_str()
    } else {
        card.latest_message.as_str()
    };
    // 摘要只留第一行：浮层一行放不下一段多行汇报。
    let mut summary: String = message
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .chars()
        .take(120)
        .collect();
    if summary.is_empty() && status_kind == STATUS_RUNNING {
        summary = "正在执行…".into();
    }
    let title = if card.title.trim().is_empty() {
        "子智能体"
    } else {
        card.title.trim()
    };
    SubagentItem {
        id: card.session_id.as_str().into(),
        title: title.into(),
        summary: summary.into(),
        status_kind,
        stoppable: card.can_terminate && status_kind == STATUS_RUNNING,
        duration: duration_text(card, now_s).into(),
        credits: format_compact_count(card.credits).into(),
        context: context_text(card).into(),
    }
}

pub fn now_epoch_seconds() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs_f64())
        .unwrap_or(0.0)
}

/// 运行时长：在跑的按开始时间实时推算；已结束优先用运行记录自己的 elapsed_s。
fn duration_seconds(card: &NativeSubagentCard, now_s: f64) -> f64 {
    if card.is_running() && card.started_at > 0.0 {
        return (now_s - card.started_at).max(0.0);
    }
    if card.elapsed_s > 0.0 {
        return card.elapsed_s;
    }
    if card.started_at > 0.0 && card.finished_at >= card.started_at {
        return card.finished_at - card.started_at;
    }
    0.0
}

/// 与云端 `formatDurationSeconds` 同形：`45s` / `1m 5s` / `2h 3m 4s`。
fn format_elapsed(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let secs = total % 60;
    if hours > 0 {
        format!("{}h {}m {}s", hours, minutes, secs)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, secs)
    } else {
        format!("{}s", secs)
    }
}

fn duration_text(card: &NativeSubagentCard, now_s: f64) -> String {
    let seconds = duration_seconds(card, now_s);
    if seconds < 0.5 {
        "--".into()
    } else {
        format_elapsed(seconds)
    }
}

/// 与云端 `formatCompactCount` 同形：`7` / `1.2k` / `3m`。
fn format_compact_count(value: i64) -> String {
    let count = value.max(0);
    if count < 1_000 {
        return count.to_string();
    }
    let (divisor, unit) = if count >= 1_000_000 {
        (1_000_000.0, "m")
    } else {
        (1_000.0, "k")
    };
    let compact = count as f64 / divisor;
    let digits = if compact >= 100.0 { 0 } else { 1 };
    let mut text = format!("{:.*}", digits, compact);
    if text.ends_with(".0") {
        text.truncate(text.len() - 2);
    }
    format!("{}{}", text, unit)
}

fn context_text(card: &NativeSubagentCard) -> String {
    if card.context_tokens <= 0 && card.max_context <= 0 {
        return "--".into();
    }
    if card.max_context > 0 {
        let percent =
            (card.context_tokens.max(0) as f64 / card.max_context as f64 * 100.0).round();
        return format!("{}%", percent as i64);
    }
    format_compact_count(card.context_tokens)
}

/// 命令行本身可能带换行（heredoc、多行脚本），浮层里压成一行再截断。
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

/// 已运行时长。`12s` / `3m20s` / `1h05m`（与桌面版任务面板同一形）。
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

#[cfg(test)]
mod tests {
    use super::*;

    fn message(text: &str) -> wunder_desktop::NativeMessage {
        wunder_desktop::NativeMessage {
            turn_id: "root".into(),
            text: text.into(),
            mine: false,
            created_at: 0.0,
            state: String::new(),
            stats_status: String::new(),
            user_round: 1,
            stats: Default::default(),
            attachments: Vec::new(),
        }
    }

    fn turn_with(cards: Vec<NativeSubagentCard>) -> NativeChatTurn {
        NativeChatTurn {
            root_id: "root".into(),
            user: message("问"),
            assistant: message("答"),
            rounds: Vec::new(),
            subagents: cards,
            process_steps: 0,
            plan: None,
            question_panel: None,
            failure: None,
        }
    }

    fn command_task(id: &str, command: &str, elapsed_ms: i64) -> NativeCommandTask {
        NativeCommandTask {
            id: id.into(),
            command: command.into(),
            cwd: "/workspace".into(),
            elapsed_ms,
            running: true,
            exit_code: None,
            status: "running".into(),
            tty: false,
        }
    }

    #[test]
    fn tasks_are_single_lined_and_bounded() {
        let tasks = vec![
            command_task("a", "cargo test\n  --all", 12_400),
            command_task("b", "ls", 900),
            command_task("c", "sleep 60", 1_000),
        ];
        let view = project(&tasks, &[], 0.0);
        assert_eq!(view.task_count, 3);
        assert_eq!(view.tasks.len(), MAX_TASKS);
        assert_eq!(view.tasks[0].command.as_str(), "cargo test --all");
        assert_eq!(view.tasks[0].elapsed.as_str(), "12s");
        assert_eq!(view.tasks[1].elapsed.as_str(), "0s");
    }

    #[test]
    fn running_subagents_lead_and_are_stoppable() {
        let now = now_epoch_seconds();
        let running = NativeSubagentCard {
            session_id: "child-running".into(),
            title: "整理资料".into(),
            latest_message: "第一行\n第二行".into(),
            can_terminate: true,
            started_at: now - 65.0,
            ..Default::default()
        };
        let done = NativeSubagentCard {
            session_id: "child-done".into(),
            title: "旧任务".into(),
            terminal: true,
            status: "success".into(),
            elapsed_s: 12.0,
            credits: 1_200,
            context_tokens: 2_048,
            max_context: 8_192,
            started_at: now - 900.0,
            ..Default::default()
        };
        let view = project(&[], &[turn_with(vec![done, running])], now);
        assert_eq!(view.subagent_count, 2);
        assert_eq!(view.subagents[0].id.as_str(), "child-running");
        assert_eq!(view.subagents[0].summary.as_str(), "第一行");
        assert_eq!(view.subagents[0].status_kind, STATUS_RUNNING);
        assert!(view.subagents[0].stoppable);
        assert!(view.subagents[0].duration.as_str().starts_with("1m 5"));
        assert_eq!(view.subagents[1].id.as_str(), "child-done");
        assert_eq!(view.subagents[1].status_kind, STATUS_DONE);
        assert!(!view.subagents[1].stoppable);
        assert_eq!(view.subagents[1].duration.as_str(), "12s");
        assert_eq!(view.subagents[1].credits.as_str(), "1.2k");
        assert_eq!(view.subagents[1].context.as_str(), "25%");
    }

    #[test]
    fn failed_subagent_keeps_its_error_line() {
        let card = NativeSubagentCard {
            session_id: "child".into(),
            title: "子任务".into(),
            failed: true,
            terminal: true,
            status: "failed".into(),
            error_message: "工具调用失败".into(),
            ..Default::default()
        };
        let view = project(&[], &[turn_with(vec![card])], 0.0);
        assert_eq!(view.subagents[0].status_kind, STATUS_FAILED);
        assert_eq!(view.subagents[0].summary.as_str(), "工具调用失败");
        assert_eq!(view.subagents[0].duration.as_str(), "--");
    }

    #[test]
    fn empty_projection_reports_nothing() {
        let view = project(&[], &[], 0.0);
        assert_eq!(view.task_count, 0);
        assert_eq!(view.subagent_count, 0);
        assert_eq!(view.subagents.len(), 0);
        assert_eq!(view.tasks.len(), 0);
    }
}
