use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::tui::app::{CommandCenterRow, CommandCenterView};
use crate::tui::theme;

pub(crate) fn draw(frame: &mut Frame, area: Rect, view: CommandCenterView, is_zh: bool) {
    frame.render_widget(Clear, area);
    if area.width < 32 || area.height < 8 {
        let text = if is_zh {
            "终端过窄，请扩大窗口以打开任务中心"
        } else {
            "Terminal too small for the thread center"
        };
        frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), area);
        return;
    }
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(4),
            Constraint::Length(2),
        ])
        .split(area);
    draw_header(frame, sections[0], &view, is_zh);
    if view.help {
        draw_help(frame, sections[1], is_zh);
    } else if sections[1].width >= 90 {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(58),
                Constraint::Length(1),
                Constraint::Percentage(42),
            ])
            .split(sections[1]);
        draw_rows(frame, columns[0], &view, is_zh);
        frame.render_widget(
            Paragraph::new("│").style(theme::secondary_text()),
            columns[1],
        );
        draw_details(frame, columns[2], &view, is_zh);
    } else {
        draw_rows(frame, sections[1], &view, is_zh);
    }
    draw_footer(frame, sections[2], &view, is_zh);
}

fn draw_header(frame: &mut Frame, area: Rect, view: &CommandCenterView, is_zh: bool) {
    let title = if is_zh { "任务中心" } else { "Threads" };
    let tabs = view
        .filter_counts
        .iter()
        .map(|(name, count)| {
            if name == &view.filter {
                format!("[{name} {count}]")
            } else {
                format!(" {name} {count} ")
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    let lines = vec![
        Line::from(vec![
            Span::styled(
                title,
                Style::default().add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    "  {}: {}",
                    if is_zh { "筛选" } else { "Filter" },
                    view.filter
                ),
                theme::secondary_text(),
            ),
            Span::styled(
                format!("  {}: {}", if is_zh { "分组" } else { "Group" }, view.group),
                theme::secondary_text(),
            ),
        ]),
        Line::from(Span::styled(tabs, theme::secondary_text())),
        Line::from(Span::styled(
            truncate(&view.tunnel.line(is_zh), area.width as usize),
            tunnel_tone(view.tunnel.phase, view.tunnel.dropped_frames),
        )),
        Line::from(Span::styled(
            "─".repeat(area.width as usize),
            theme::secondary_text(),
        )),
    ];
    frame.render_widget(Paragraph::new(lines), area);
}

/// Congestion is the one condition that must not read as a healthy link.
fn tunnel_tone(phase: crate::interlink_tunnel::TunnelPhase, dropped_frames: u64) -> Style {
    if dropped_frames > 0 || matches!(phase, crate::interlink_tunnel::TunnelPhase::Reconnecting) {
        return theme::warning_text();
    }
    theme::secondary_text()
}

fn draw_rows(frame: &mut Frame, area: Rect, view: &CommandCenterView, is_zh: bool) {
    let header = if view.searching {
        format!(
            "{} › {}",
            if is_zh { "搜索" } else { "Search" },
            view.search
        )
    } else if view.rows.is_empty() {
        if is_zh {
            "没有匹配的线程".to_string()
        } else {
            "No matching threads".to_string()
        }
    } else {
        if is_zh {
            "任务                          工作区           状态       更新时间".to_string()
        } else {
            "Tasks                         Workspace        Status     Updated".to_string()
        }
    };
    let mut lines = vec![Line::from(Span::styled(header, theme::secondary_text()))];
    let capacity = usize::from(area.height.saturating_sub(1));
    let selected = view
        .rows
        .iter()
        .position(|row| view.selected_session_id.as_deref() == Some(row.session_id.as_str()))
        .unwrap_or(0);
    let start = selected.saturating_sub(capacity.saturating_sub(1));
    for row in view.rows.iter().skip(start).take(capacity) {
        lines.push(row_line(
            row,
            view.selected_session_id.as_deref() == Some(row.session_id.as_str()),
            area.width,
            is_zh,
        ));
    }
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::NONE)),
        area,
    );
}

fn row_line(row: &CommandCenterRow, selected: bool, width: u16, _is_zh: bool) -> Line<'static> {
    let prefix = if selected { "›" } else { " " };
    let child = row.parent.as_ref().map(|_| "↳ ").unwrap_or("");
    let workspace = row.workspace.clone().unwrap_or_else(|| "-".to_string());
    // The row is title · workspace · status · updated; the workspace column is
    // what makes a list of tasks from several folders readable.
    let fixed = 12 + 16 + 14 + 4;
    let title_budget = usize::from(width).saturating_sub(fixed);
    let unread = if row.unread_events > 0 {
        format!(" [{}]", row.unread_events.min(999))
    } else {
        String::new()
    };
    let replay = if row.needs_replay { " ↻" } else { "" };
    let title = truncate(
        &format!("{child}{}{}{}", row.title, unread, replay),
        title_budget.max(12),
    );
    let left = format!(
        "{prefix} {} {}",
        row.marker,
        pad_cell(title.as_str(), title_budget.max(12))
    );
    let workspace_name = truncate(&workspace, 14);
    let workspace_cell = pad_cell(workspace_name.as_str(), 16);
    let status = truncate(row.status.as_str(), 10);
    let stamp = truncate(row.updated_at.as_str(), 14);
    let base = if selected {
        theme::popup_selected()
    } else {
        Style::default()
    };

    // The workspace carries the same colour the desktop gives that folder, in
    // one leading dot. A narrow terminal sheds the right-hand columns first, so
    // the decorated form is only used when it actually fits.
    if let Some(color) = row.workspace_color.as_deref().and_then(workspace_color) {
        let decorated = Line::from(vec![
            Span::styled(left.clone(), base),
            Span::styled("● ", base.fg(color)),
            Span::styled(pad_cell(workspace_name.as_str(), 14), base),
            Span::styled(format!("{status} {stamp}"), base),
        ]);
        if decorated.width() <= usize::from(width) {
            return decorated;
        }
    }

    let content = truncate(
        &format!("{left}{workspace_cell}{status} {stamp}"),
        usize::from(width),
    );
    Line::from(Span::styled(content, base))
}

/// Workspace colours travel as names (the desktop writes `blue`, `green`, …).
/// An unknown name simply means "no dot" rather than a guessed colour.
fn workspace_color(name: &str) -> Option<Color> {
    match name.trim().to_ascii_lowercase().as_str() {
        "blue" => Some(Color::Blue),
        "cyan" => Some(Color::Cyan),
        "green" => Some(Color::Green),
        "yellow" => Some(Color::Yellow),
        "orange" => Some(Color::LightRed),
        "red" => Some(Color::Red),
        "purple" | "violet" => Some(Color::Magenta),
        "gray" | "grey" => Some(Color::DarkGray),
        _ => None,
    }
}

/// Pad a cell to a fixed column width so the workspace/status columns line up.
fn pad_cell(value: &str, width: usize) -> String {
    let used = UnicodeWidthStr::width(value);
    if used >= width {
        return value.to_string();
    }
    format!("{value}{}", " ".repeat(width - used))
}

fn draw_details(frame: &mut Frame, area: Rect, view: &CommandCenterView, is_zh: bool) {
    let selected = view
        .selected_session_id
        .as_deref()
        .and_then(|id| view.rows.iter().find(|row| row.session_id == id));
    let Some(row) = selected else {
        frame.render_widget(
            Paragraph::new(if is_zh {
                "没有可查看的线程"
            } else {
                "No task selected"
            })
            .wrap(Wrap { trim: false }),
            area,
        );
        return;
    };
    let mut lines = vec![
        Line::from(Span::styled(
            if is_zh {
                "线程详情"
            } else {
                "Task details"
            },
            Style::default().add_modifier(ratatui::style::Modifier::BOLD),
        )),
        Line::default(),
        Line::from(row.title.clone()),
    ];
    let mut headline = vec![
        Span::styled("● ", theme::accent_text()),
        Span::raw(row.status.clone()),
    ];
    if let Some(reason) = row.pending_reason.as_deref() {
        headline.push(Span::styled(
            format!(" · {reason}"),
            theme::secondary_text(),
        ));
    }
    lines.push(Line::from(headline));
    // Round, tools and context come from the catalog row, not from this UI's
    // own counters, so a background thread reports the same numbers as the list.
    lines.push(Line::from(Span::styled(
        format!(
            "{} {} · {} {} · {} {}",
            if is_zh { "轮次" } else { "rounds" },
            row.user_round,
            if is_zh { "工具" } else { "tools" },
            row.tool_calls,
            if is_zh { "上下文" } else { "context" },
            format_tokens(row.context_tokens),
        ),
        theme::secondary_text(),
    )));
    if let Some(activity) = row.last_activity.as_deref() {
        lines.push(Line::from(vec![
            Span::styled(
                if is_zh {
                    "最近活动: "
                } else {
                    "last activity: "
                },
                theme::secondary_text(),
            ),
            Span::raw(activity.to_string()),
        ]));
    }
    lines.push(Line::default());
    for (label, value) in [
        (
            if is_zh { "工作区" } else { "Workspace" },
            row.workspace.clone().unwrap_or_else(|| "-".to_string()),
        ),
        (
            if is_zh {
                "工作区 ID"
            } else {
                "Workspace id"
            },
            row.workspace_id.clone().unwrap_or_else(|| "-".to_string()),
        ),
        (
            if is_zh { "父线程" } else { "Parent" },
            row.parent.clone().unwrap_or_else(|| "-".to_string()),
        ),
        (
            if is_zh { "派生来源" } else { "Spawned by" },
            row.spawn_label.clone().unwrap_or_else(|| "-".to_string()),
        ),
        (
            if is_zh { "子线程" } else { "Child threads" },
            row.child_threads.to_string(),
        ),
    ] {
        lines.push(Line::from(vec![
            Span::styled(format!("{label}: "), theme::secondary_text()),
            Span::raw(value),
        ]));
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        format!(
            "{} {} · {} {} · {} {}",
            if is_zh { "游标" } else { "cursor" },
            row.change_seq,
            if is_zh { "未折叠" } else { "unfolded" },
            row.durable_unread,
            if is_zh { "未读事件" } else { "unread" },
            row.unread_events,
        ),
        theme::secondary_text(),
    )));
    lines.push(Line::from(Span::styled(
        format!(
            "{} {} · {} {} · {} {}",
            if is_zh { "待审批" } else { "approvals" },
            row.pending_approvals,
            if is_zh { "待呈现" } else { "queued" },
            row.pending_events,
            if is_zh { "事件流" } else { "stream" },
            if row.stream_active { "●" } else { "○" },
        ),
        theme::secondary_text(),
    )));
    if row.needs_replay {
        lines.push(Line::from(Span::styled(
            if is_zh {
                "↻ 正在补齐事件"
            } else {
                "↻ Replay pending"
            },
            theme::secondary_text(),
        )));
    }
    lines.push(Line::from(Span::styled(
        row.session_id.clone(),
        theme::secondary_text(),
    )));
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

/// Compact token count for the detail block; the terminal is narrow.
fn format_tokens(tokens: i64) -> String {
    if tokens >= 1_000_000 {
        format!("{}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

fn draw_footer(frame: &mut Frame, area: Rect, view: &CommandCenterView, is_zh: bool) {
    let text = if view.searching {
        if is_zh {
            "输入搜索词  Enter 完成  Esc 清除"
        } else {
            "Type to search  Enter done  Esc clear"
        }
    } else if is_zh {
        "↑↓ 选择  Enter 切换  Tab 筛选  g 分组  / 搜索  r 刷新  ? 帮助  ← 返回"
    } else {
        "↑↓ select  Enter switch  Tab filter  g group  / search  r refresh  ? help  ← back"
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(text, theme::secondary_text()))),
        area,
    );
}

fn draw_help(frame: &mut Frame, area: Rect, is_zh: bool) {
    let text = if is_zh {
        "任务中心\n\n↑↓ / PageUp PageDown：选择任务\nEnter：进入选中任务\nTab / Shift+Tab：切换状态筛选\ng：循环父子谱系、状态、工作区分组\n/：搜索标题、工作区或会话\nr：刷新任务目录\n← 或 Esc：返回会话\n\n按 Esc 或 ? 返回"
    } else {
        "Threads\n\n↑↓ / PageUp PageDown: select a task\nEnter: switch to the selected task\nTab / Shift+Tab: change the status filter\ng: cycle parent/status/workspace grouping\n/: search title, workspace, or session\nr: refresh the task directory\n← or Esc: return to the conversation\n\nPress Esc or ? to return"
    };
    frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), area);
}

fn truncate(value: &str, width: usize) -> String {
    if UnicodeWidthStr::width(value) <= width {
        return value.to_string();
    }
    let mut output = String::new();
    let mut used = 0;
    for ch in value.chars() {
        let size = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + size + 1 > width {
            break;
        }
        output.push(ch);
        used += size;
    }
    output.push('…');
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrow_rows_never_exceed_terminal_columns() {
        let row = CommandCenterRow {
            session_id: "thread-a".into(),
            title: "测试线程标题很长且带有宽字符".repeat(8),
            status: "Needs you".into(),
            marker: '!',
            updated_at: "00:00".into(),
            workspace: Some("很长的中文工作区名字".into()),
            workspace_color: Some("blue".into()),
            workspace_id: Some("ws_fixture".into()),
            parent: Some("parent".into()),
            spawn_label: None,
            unread_events: 42,
            needs_replay: true,
            pending_approvals: 2,
            pending_events: 4,
            stream_active: true,
            child_threads: 2,
            user_round: 4,
            tool_calls: 3,
            context_tokens: 12_800,
            last_activity: Some("读取了配置文件".into()),
            change_seq: 97,
            durable_unread: 3,
            pending_reason: Some("等待授权".into()),
        };
        for width in [32, 40, 60, 90] {
            assert!(row_line(&row, true, width, true).width() <= usize::from(width));
        }
    }

    #[test]
    fn the_workspace_column_lines_up_and_falls_back_to_a_dash() {
        let mut row = CommandCenterRow {
            session_id: "thread-b".into(),
            title: "task".into(),
            status: "Ready".into(),
            marker: '○',
            updated_at: "12:00".into(),
            workspace: Some("wunder".into()),
            workspace_color: None,
            workspace_id: None,
            parent: None,
            spawn_label: None,
            unread_events: 0,
            needs_replay: false,
            pending_approvals: 0,
            pending_events: 0,
            stream_active: false,
            child_threads: 0,
            user_round: 0,
            tool_calls: 0,
            context_tokens: 0,
            last_activity: None,
            change_seq: 0,
            durable_unread: 0,
            pending_reason: None,
        };
        let named = row_line(&row, false, 80, false).to_string();
        assert!(named.contains("wunder"), "{named}");
        row.workspace = None;
        let unnamed = row_line(&row, false, 80, false).to_string();
        assert!(
            unnamed.contains(" - "),
            "an unknown workspace shows a dash: {unnamed}"
        );
    }

    #[test]
    fn token_counts_stay_short_enough_for_the_detail_column() {
        assert_eq!(format_tokens(0), "0");
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(12_800), "12.8k");
        assert_eq!(format_tokens(2_400_000), "2.4M");
    }

    #[test]
    fn the_tunnel_header_line_stays_inside_the_header_width() {
        let busy = crate::interlink_tunnel::TunnelSummary {
            phase: crate::interlink_tunnel::TunnelPhase::Reconnecting,
            remote_pending_approvals: 3,
            dropped_frames: 12,
        };
        for width in [12, 20, 40] {
            let line = truncate(&busy.line(true), usize::from(width));
            assert!(line.width() <= usize::from(width), "{line} overflows {width}");
        }
        // Congestion must change the tone, not only the text.
        assert_eq!(
            tunnel_tone(crate::interlink_tunnel::TunnelPhase::Connected, 0),
            theme::secondary_text()
        );
        assert_ne!(
            tunnel_tone(crate::interlink_tunnel::TunnelPhase::Connected, 1),
            theme::secondary_text()
        );
    }
}
