use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
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
            "终端过窄，请扩大窗口以打开 Agent command center"
        } else {
            "Terminal too small for Agent command center"
        };
        frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), area);
        return;
    }
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
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
    let title = if is_zh {
        "Agent command center"
    } else {
        "Agent command center"
    };
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
            "─".repeat(area.width as usize),
            theme::secondary_text(),
        )),
    ];
    frame.render_widget(Paragraph::new(lines), area);
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
            "线程                                      状态       更新时间".to_string()
        } else {
            "Tasks                                       Status     Updated".to_string()
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
    let title_budget = usize::from(width).saturating_sub(30);
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
    let left = format!("{prefix} {} {title}", row.marker);
    let status = truncate(row.status.as_str(), 10);
    let stamp = truncate(row.updated_at.as_str(), 14);
    let remaining = usize::from(width).saturating_sub(
        UnicodeWidthStr::width(left.as_str())
            + UnicodeWidthStr::width(status.as_str())
            + UnicodeWidthStr::width(stamp.as_str())
            + 2,
    );
    let content = truncate(
        &format!("{left}{}{} {stamp}", " ".repeat(remaining), status),
        usize::from(width),
    );
    if selected {
        Line::from(Span::styled(content, theme::popup_selected()))
    } else {
        Line::from(content)
    }
}

fn draw_details(frame: &mut Frame, area: Rect, view: &CommandCenterView, is_zh: bool) {
    let selected = view
        .selected_session_id
        .as_deref()
        .and_then(|id| view.rows.iter().find(|row| row.session_id == id));
    let lines = if let Some(row) = selected {
        vec![
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
            Line::from(vec![
                Span::styled("● ", theme::accent_text()),
                Span::raw(row.status.clone()),
            ]),
            Line::default(),
            Line::from(Span::styled(
                if is_zh { "智能体" } else { "Agent" },
                theme::secondary_text(),
            )),
            Line::from(row.agent.clone().unwrap_or_else(|| "-".to_string())),
            Line::from(Span::styled(
                if is_zh { "父线程" } else { "Parent" },
                theme::secondary_text(),
            )),
            Line::from(row.parent.clone().unwrap_or_else(|| "-".to_string())),
            Line::from(Span::styled(
                if is_zh { "派生标签" } else { "Spawn label" },
                theme::secondary_text(),
            )),
            Line::from(row.spawn_label.clone().unwrap_or_else(|| "-".to_string())),
            Line::from(Span::styled(
                if is_zh { "子线程" } else { "Child threads" },
                theme::secondary_text(),
            )),
            Line::from(row.child_threads.to_string()),
            Line::from(Span::styled(
                if is_zh { "会话" } else { "Session" },
                theme::secondary_text(),
            )),
            Line::from(row.session_id.clone()),
            Line::from(Span::styled(
                if is_zh {
                    "未读事件"
                } else {
                    "Unread events"
                },
                theme::secondary_text(),
            )),
            Line::from(row.unread_events.to_string()),
            Line::from(format!(
                "{}: {}",
                if is_zh { "待审批" } else { "Approvals" },
                row.pending_approvals
            )),
            Line::from(format!(
                "{}: {}",
                if is_zh {
                    "待呈现事件"
                } else {
                    "Pending events"
                },
                row.pending_events
            )),
            Line::from(format!(
                "{}: {}",
                if is_zh { "事件流" } else { "Stream" },
                if row.stream_active { "●" } else { "○" }
            )),
            Line::from(if row.needs_replay {
                if is_zh {
                    "↻ 正在补齐事件"
                } else {
                    "↻ Replay pending"
                }
            } else {
                ""
            }),
        ]
    } else {
        vec![Line::from(if is_zh {
            "没有可查看的线程"
        } else {
            "No task selected"
        })]
    };
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
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
        "Agent command center\n\n↑↓ / PageUp PageDown：选择线程\nEnter：进入选中线程\nTab / Shift+Tab：切换状态筛选\ng：循环父子谱系、状态、智能体分组\n/：搜索标题、智能体或会话\nr：刷新线程目录\n← 或 Esc：返回会话\n\n按 Esc 或 ? 返回"
    } else {
        "Agent command center\n\n↑↓ / PageUp PageDown: select a thread\nEnter: switch to selected thread\nTab / Shift+Tab: change status filter\ng: cycle parent/status/agent grouping\n/: search title, agent, or session\nr: refresh the thread directory\n← or Esc: return to conversation\n\nPress Esc or ? to return"
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
            agent: None,
            parent: Some("parent".into()),
            spawn_label: None,
            unread_events: 42,
            needs_replay: true,
            pending_approvals: 2,
            pending_events: 4,
            stream_active: true,
            child_threads: 2,
        };
        for width in [32, 40, 60, 90] {
            assert!(row_line(&row, true, width, true).width() <= usize::from(width));
        }
    }
}
