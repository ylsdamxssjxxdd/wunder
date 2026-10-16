use super::*;
use crate::tool_display::tool_result_ledger_summary;
use crate::tui::theme;
use ratatui::layout::{Constraint, Direction as LayoutDirection, Layout};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

fn tr(zh: bool, chinese: &'static str, english: &'static str) -> &'static str {
    if zh {
        chinese
    } else {
        english
    }
}
fn kind_label(kind: &str, zh: bool) -> &str {
    if !zh {
        return kind;
    }
    match kind {
        "user_message" => "用户输入",
        "assistant_message" => "模型回复",
        "system_message" => "系统上下文",
        "context" => "上下文",
        "tool_call" => "工具调用",
        "tool_result" => "工具结果",
        "model_call" => "模型调用",
        "subagent_run" => "子智能体",
        "compaction" => "压缩",
        "approval" => "审批",
        "terminal" => "轮次结束",
        "plan" => "计划",
        "search_hit" => "搜索命中",
        _ => kind,
    }
}
fn status_label(status: &str, zh: bool) -> &str {
    if !zh {
        return status;
    }
    match status {
        "completed" => "完成",
        "running" => "运行中",
        "failed" => "失败",
        "cancelled" => "取消",
        "interrupted" => "中断",
        "queued" => "等待",
        "waiting_input" => "待输入",
        _ => status,
    }
}
fn style(row: &TrajectoryViewRow) -> Style {
    match row.status.as_str() {
        "failed" => theme::danger_text(),
        "running" | "waiting_input" => theme::warning_text(),
        _ => match lane(&row.kind) {
            0 => theme::brand_text(),
            1 => theme::accent_text(),
            _ => theme::success_text(),
        },
    }
}
fn mark(status: &str) -> &'static str {
    match status {
        "failed" => "×",
        "running" | "waiting_input" => "◌",
        "cancelled" | "interrupted" => "!",
        _ => "●",
    }
}
fn one(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut cells = 0;
    for c in text.chars() {
        let c = if c.is_control() { ' ' } else { c };
        let count = UnicodeWidthChar::width(c).unwrap_or(0);
        if cells + count > width {
            if width > cells {
                out.push('…');
            }
            break;
        }
        out.push(c);
        cells += count;
    }
    out
}
fn pad(text: &str, width: usize) -> String {
    let mut s = one(text, width);
    s.push_str(&" ".repeat(width.saturating_sub(s.width())));
    s
}
fn usage(row: &TrajectoryViewRow) -> String {
    let Some(stats) = row.stats.as_ref() else {
        return "—".into();
    };
    let keys = [
        "total_tokens",
        "totalTokens",
        "output_tokens",
        "outputTokens",
        "completion_tokens",
    ];
    for source in [Some(stats), stats.get("usage"), stats.get("round_usage")]
        .into_iter()
        .flatten()
    {
        for key in keys {
            if let Some(value) = source.get(key).filter(|v| v.is_number() || v.is_string()) {
                return value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string());
            }
        }
    }
    "—".into()
}

fn ledger_summary(row: &TrajectoryViewRow, zh: bool) -> String {
    let raw = row.summary.trim();
    if !matches!(row.kind.as_str(), "tool_result" | "subagent_run") {
        return row.summary.clone();
    }
    let start = raw.find(['{', '[']).unwrap_or(raw.len());
    if start == raw.len() {
        return row.summary.clone();
    }
    let (prefix, json_text) = raw.split_at(start);
    let Ok(payload) = serde_json::from_str::<serde_json::Value>(json_text) else {
        return row.summary.clone();
    };
    let Some(display) = tool_result_ledger_summary(&payload, zh) else {
        return row.summary.clone();
    };
    let prefix = prefix.trim_end();
    if prefix.is_empty() {
        display
    } else {
        format!("{prefix} {display}")
    }
}
pub(super) fn tabs(view: &TrajectoryView) -> Vec<usize> {
    if view.row().is_some_and(|r| is_tool(&r.kind)) {
        vec![0, 3, 4, 5, 6, 2]
    } else {
        vec![0, 1, 2, 6]
    }
}
fn tab_label(tab: usize, zh: bool) -> &'static str {
    match tab {
        0 => tr(zh, "概览", "Overview"),
        1 => tr(zh, "正文", "Preview"),
        2 => tr(zh, "原始", "Raw"),
        3 => tr(zh, "参数", "Params"),
        4 => tr(zh, "结果", "Result"),
        5 => tr(zh, "契约", "Schema"),
        _ => tr(zh, "耗时", "Timing"),
    }
}
fn detail_lines(view: &TrajectoryView) -> Option<&[String]> {
    let available = tabs(view);
    let tab = available[view.tab % available.len()];
    let current = view.row().is_some_and(|row| {
        view.detail_key
            .as_ref()
            .is_some_and(|key| key.0 == row.item_id && key.1 == row.revision)
    });
    if let Some(lines) = view
        .detail
        .sections
        .get(tab)
        .filter(|lines| current && !lines.is_empty())
    {
        return Some(lines);
    }
    None
}
pub(super) fn detail_text(view: &TrajectoryView, zh: bool) -> String {
    detail_lines(view)
        .map(|lines| lines.join("\n"))
        .unwrap_or_else(|| {
            view.row()
                .map(|row| {
                    format!(
                        "{} · {}\n{}",
                        kind_label(&row.kind, zh),
                        status_label(&row.status, zh),
                        row.summary
                    )
                })
                .unwrap_or_default()
        })
}

pub(crate) fn draw(frame: &mut Frame, area: Rect, app: &mut TuiApp, zh: bool) {
    let Some(v) = app.trajectory_view.as_mut() else {
        return;
    };
    frame.render_widget(Clear, area);
    let bands = Layout::default()
        .direction(LayoutDirection::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(if area.height >= 18 { 5 } else { 3 }),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(area);
    let state = if v.pending.is_some() {
        tr(zh, "读取中…", "Loading…")
    } else if v.live && !v.newer {
        tr(zh, "跟随", "Live")
    } else {
        tr(zh, "暂停跟随", "Paused")
    };
    let scope = if v.root.is_some() {
        tr(zh, "当前轮次", "Turn")
    } else {
        tr(zh, "全部轮次", "All turns")
    };
    let title = format!(
        " {} · {} · {} · {} ",
        tr(zh, "线程轨迹", "Trajectory"),
        one(&v.session, 24),
        scope,
        state
    );
    let toolbar = format!(
        "[d] {}   [t] {} {}   [c] {} {}   [+/-] {}×   [e] {}   [?] {}",
        if v.duration {
            tr(zh, "耗时", "Duration")
        } else {
            tr(zh, "顺序", "Sequence")
        },
        tr(zh, "轮次", "Turns"),
        if v.fold_turns { "▸" } else { "▾" },
        tr(zh, "调用", "Calls"),
        if v.fold_calls { "▸" } else { "▾" },
        1_u32 << v.zoom,
        tr(zh, "导出", "Export"),
        tr(zh, "帮助", "Help")
    );
    let mut info = format!(
        "{} {} · {} {} · {} {}{}",
        tr(zh, "用户轮次", "User turns"),
        v.latest_round,
        tr(zh, "窗口", "Window"),
        v.active_rows().len(),
        v.projection,
        if v.hits.is_some() {
            tr(zh, "搜索", "Search")
        } else {
            ""
        },
        if v.more_hits && v.hits.is_some() {
            " 20+"
        } else {
            ""
        }
    );
    if !v.search.is_empty() && v.hits.is_some() {
        info.push_str(&format!(" /{}", v.search));
    }
    if !v.tiles.is_empty() {
        info.push_str(&format!(
            " · {} {}",
            tr(zh, "事件", "Events"),
            v.tiles.iter().map(|t| t.count).sum::<i64>()
        ));
    }
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(toolbar, theme::accent_text())),
            Line::from(Span::styled(info, theme::secondary_text())),
        ])
        .block(Block::default().borders(Borders::TOP).title(title)),
        bands[0],
    );
    draw_timeline(frame, bands[1], v, zh);
    let wide = bands[2].width >= 100;
    v.ledger_area = Rect::default();
    v.detail_area = Rect::default();
    if wide {
        let parts = Layout::default()
            .direction(LayoutDirection::Horizontal)
            .constraints([Constraint::Percentage(57), Constraint::Percentage(43)])
            .split(bands[2]);
        draw_ledger(frame, parts[0], v, zh);
        draw_detail(frame, parts[1], v, zh);
    } else if v.detail_focus {
        draw_detail(frame, bands[2], v, zh);
    } else {
        draw_ledger(frame, bands[2], v, zh);
    }
    let message = v
        .error
        .as_deref()
        .or(v.diagnostic.as_deref())
        .unwrap_or(&v.notice);
    let footer = if v.searching {
        format!(
            "/{}  {}",
            v.search,
            tr(
                zh,
                "Enter 搜索正文 · Esc 清除",
                "Enter search bodies · Esc clear"
            )
        )
    } else {
        tr(zh,"↑↓/PgUp/Dn 选择  Enter 轮次  i 详情  Tab 详情页签  / 搜索  y 复制  r 刷新  Esc 返回","↑↓/PgUp/Dn select  Enter turn  i details  Tab tabs  / search  y copy  r refresh  Esc back").into()
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                one(message, area.width as usize),
                if v.error.is_some() {
                    theme::danger_text()
                } else {
                    theme::secondary_text()
                },
            )),
            Line::from(Span::styled(footer, theme::secondary_text())),
        ]),
        bands[3],
    );
    if v.help {
        frame.render_widget(Clear, area);
        frame.render_widget(Paragraph::new(tr(zh,
            "轨迹日志\n\n↑↓ / j k：选择记录；PageUp / PageDown：翻页（边缘自动读取）\nEnter：进入当前用户轮次；Esc：依次返回列表、全局与会话\ni / →：切换详情焦点；窄终端显示独立详情页\nTab / Shift+Tab：切换详情页签；详情焦点内 ↑↓ / PgUp PgDn 滚动，h / l 平移长行\nd：顺序 / 真实耗时；+ / -：围绕选中条目缩放；0：复位\nt：全部轮次折叠；空格：当前轮次折叠；c：同模型轮次工具折叠\n/：全文搜索（Enter 提交，最多 20 个命中；缩小词组可继续定位）\nf：切换实时跟随；手动浏览暂停跟随；r：刷新或重试\ny：复制当前详情页签；e：导出完整 JSONL 到启动目录\n鼠标：台账点击选择，时间线点击定位已载入记录，滚轮浏览\n\nEsc / ? 返回",
            "Trajectory log\n\n↑↓ / j k: select; PageUp / PageDown: page (load at either edge)\nEnter: inspect user turn; Esc: return through details, overview, conversation\ni / →: focus details; narrow terminals show a separate detail page\nTab / Shift+Tab: detail tabs; focused ↑↓ / PgUp PgDn: scroll details; h / l: pan long lines\nd: sequence / elapsed time; + / -: zoom around selection; 0: reset\nt: fold all turns; Space: fold selected turn; c: fold tool groups\n/: full-text search (Enter submits, up to 20 hits; refine for more)\nf: toggle live follow; browsing pauses follow; r: refresh or retry\ny: copy current detail tab; e: export full JSONL to launch directory\nMouse: click ledger or loaded timeline record; wheel to browse\n\nEsc / ? returns")), area);
    }
}
fn draw_timeline(frame: &mut Frame, area: Rect, v: &mut TrajectoryView, zh: bool) {
    let range = v.domain();
    v.timeline_range = range;
    v.timeline_area = area;
    let width = area.width.saturating_sub(8) as usize;
    if width == 0 {
        return;
    }
    let mut lanes = vec![vec![("·", theme::secondary_text()); width]; 3];
    if v.root.is_none() && v.hits.is_none() {
        let size = (128_i64 << (v.tile_level.min(7) * 3)) as f64;
        for tile in &v.tiles {
            let (start, end) = if v.duration {
                (tile.start, tile.end)
            } else {
                (tile.bucket as f64 * size, (tile.bucket as f64 + 1.) * size)
            };
            if end < range.0 || start > range.1 {
                continue;
            }
            let counts = [tile.input_count, tile.model_count, tile.tool_count];
            for (lane, count) in counts.iter().enumerate() {
                if *count == 0 && counts.iter().any(|n| *n > 0) {
                    continue;
                }
                for x in coordinate(start, range, width)..=coordinate(end, range, width) {
                    lanes[lane][x] = (
                        "░",
                        if tile.failed > 0 {
                            theme::danger_text()
                        } else {
                            theme::secondary_text()
                        },
                    );
                }
            }
        }
    }
    for (i, row) in v.active_rows().iter().enumerate() {
        let (start, end) = row_span(row, v.duration);
        if end < range.0 || start > range.1 {
            continue;
        }
        let lane = lane(&row.kind);
        for x in coordinate(start, range, width)..=coordinate(end, range, width) {
            lanes[lane][x] = (
                if i == v.selected {
                    "◆"
                } else {
                    mark(&row.status)
                },
                style(row),
            );
        }
    }
    // Selection must remain visible when several spans occupy the same terminal cell.
    if let Some(row) = v.row() {
        let (start, end) = row_span(row, v.duration);
        if end >= range.0 && start <= range.1 {
            for x in coordinate(start, range, width)..=coordinate(end, range, width) {
                lanes[lane(&row.kind)][x] = ("◆", style(row));
            }
        }
    }
    let labels = if zh {
        ["输入", "模型", "工具"]
    } else {
        ["Input", "Model", "Tools"]
    };
    let mut lines = vec![];
    for (i, lane) in lanes.into_iter().enumerate() {
        let mut spans = vec![Span::styled(
            format!("{} ", pad(labels[i], 7)),
            theme::secondary_text(),
        )];
        spans.extend(
            lane.into_iter()
                .map(|(symbol, style)| Span::styled(symbol, style)),
        );
        lines.push(Line::from(spans));
    }
    if area.height >= 5 {
        let labels = if v.duration {
            format!("0 → {:.3}s  Δ {:.3}s", range.1 - range.0, range.1 - range.0)
        } else {
            format!("#{} → #{}", range.0 as i64, range.1 as i64)
        };
        lines.push(Line::from(Span::styled(
            format!("        {labels}"),
            theme::secondary_text(),
        )));
    }
    frame.render_widget(Paragraph::new(lines), area);
}
fn draw_ledger(frame: &mut Frame, area: Rect, v: &mut TrajectoryView, zh: bool) {
    v.ledger_area = area;
    let ids = v.visible();
    let capacity = area.height.saturating_sub(2) as usize;
    let pos = ids.iter().position(|i| *i == v.selected).unwrap_or(0);
    v.ledger_offset = v.ledger_offset.min(pos);
    if pos >= v.ledger_offset + capacity {
        v.ledger_offset = pos.saturating_sub(capacity.saturating_sub(1));
    }
    v.ledger_offset = v.ledger_offset.min(ids.len().saturating_sub(capacity));
    let mut turn_sizes = HashMap::<&str, (usize, usize)>::new();
    let mut call_sizes = HashMap::<(&str, i64), usize>::new();
    for row in v.active_rows() {
        let counts = turn_sizes.entry(&row.root_turn_id).or_default();
        counts.0 += 1;
        counts.1 += usize::from(is_tool(&row.kind));
        if is_tool(&row.kind) {
            *call_sizes
                .entry((&row.turn_id, row.model_round))
                .or_default() += 1;
        }
    }
    let metrics = area.width >= 72;
    let header = tr(
        zh,
        "  用户/模型  类型        状态    摘要",
        "  user/model kind        status  summary",
    );
    let mut lines = vec![Line::from(Span::styled(
        if metrics {
            format!(
                "{} {}",
                tr(
                    zh,
                    "  用户/模型  类型        状态     耗时    tokens",
                    "  user/model kind        status   time    tokens"
                ),
                tr(zh, "摘要", "summary")
            )
        } else {
            header.into()
        },
        theme::secondary_text(),
    ))];
    for index in ids.iter().skip(v.ledger_offset).take(capacity) {
        let row = &v.active_rows()[*index];
        let selected = *index == v.selected;
        let folded = row.overview_only || v.fold_turns || v.folded.contains(&row.root_turn_id);
        let mut prefix = format!(
            "{}{:>3}/{:<3} {} {} ",
            if selected { "›" } else { " " },
            row.user_round,
            row.model_round,
            pad(
                row.tool_name
                    .as_deref()
                    .unwrap_or(kind_label(&row.kind, zh)),
                10
            ),
            pad(status_label(&row.status, zh), 6)
        );
        if metrics {
            let elapsed = row
                .completed_at
                .map(|end| format!("{:.2}s", (end - row.started_at).max(0.)))
                .unwrap_or_else(|| "—".into());
            prefix.push_str(&format!("{} {} ", pad(&elapsed, 7), pad(&usage(row), 7)));
        }
        let summary = if folded && v.hits.is_none() {
            let (steps, tools) = turn_sizes
                .get(row.root_turn_id.as_str())
                .copied()
                .unwrap_or_default();
            format!(
                "▸ [{steps} {} / {tools} {}] {}",
                tr(zh, "条", "items"),
                tr(zh, "工具", "tools"),
                ledger_summary(row, zh)
            )
        } else if v.fold_calls && is_tool(&row.kind) && v.hits.is_none() {
            let count = call_sizes
                .get(&(row.turn_id.as_str(), row.model_round))
                .copied()
                .unwrap_or(1);
            format!("▸ [{count} {}] {}", tr(zh, "次调用", "calls"), ledger_summary(row, zh))
        } else {
            ledger_summary(row, zh)
        };
        let remaining = (area.width as usize).saturating_sub(prefix.width());
        lines.push(Line::from(vec![
            Span::styled(
                prefix,
                if selected {
                    theme::modal_selected()
                } else {
                    style(row)
                },
            ),
            Span::styled(
                one(&summary, remaining),
                if selected {
                    theme::modal_selected()
                } else {
                    Style::default()
                },
            ),
        ]));
    }
    if ids.is_empty() {
        lines.push(Line::from(tr(
            zh,
            "暂无记录或搜索未命中",
            "No records or search matches",
        )));
    }
    let title = format!(
        " {} {}{} ",
        tr(zh, "台账", "Ledger"),
        if v.older { "↑" } else { "" },
        if v.newer { "↓" } else { "" }
    );
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::TOP).title(title)),
        area,
    );
}
fn draw_detail(frame: &mut Frame, area: Rect, v: &mut TrajectoryView, zh: bool) {
    v.detail_area = area;
    let available = tabs(v);
    let mut labels = Vec::new();
    for (i, tab) in available.iter().enumerate() {
        labels.push(Span::styled(
            format!(" {} ", tab_label(*tab, zh)),
            if i == v.tab % available.len() {
                theme::modal_selected()
            } else {
                theme::secondary_text()
            },
        ));
    }
    let fallback = [tr(
        zh,
        "未记录此项；系统提示词内容保持隐藏。",
        "Not recorded; system prompt contents remain hidden.",
    )
    .to_owned()];
    let content = detail_lines(v).unwrap_or(&fallback);
    let selected_tab = available[v.tab % available.len()];
    let rendered_markdown = if matches!(selected_tab, 1 | 4) {
        Some(crate::tui::markdown_render::render_markdown_text_with_width(
            content.join("\n").as_str(),
            Some(area.width.saturating_sub(1) as usize),
        ))
    } else {
        None
    };
    let markdown_lines = rendered_markdown.as_ref().map(|text| {
        text.lines
            .iter()
            .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect::<String>())
            .collect::<Vec<_>>()
    });
    let display_lines = markdown_lines.as_deref().unwrap_or(content);
    let height = display_lines.len();
    let capacity = area.height.saturating_sub(3) as usize;
    let scroll = v.detail_scroll.min(height.saturating_sub(capacity));
    let mut lines = vec![Line::from(labels)];
    lines.extend(display_lines.iter().skip(scroll).take(capacity).map(|line| {
        let mut offset = 0;
        let tail = line
            .char_indices()
            .find_map(|(i, c)| {
                if offset >= v.detail_x {
                    Some(&line[i..])
                } else {
                    offset += UnicodeWidthChar::width(c).unwrap_or(0);
                    None
                }
            })
            .unwrap_or("");
        Line::from(one(tail, area.width.saturating_sub(1) as usize))
    }));
    let title = format!(
        " {}{} {}/{} ",
        if v.detail_focus { "› " } else { "" },
        tr(zh, "详情", "Details"),
        scroll + 1,
        height
    );
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::TOP).title(title)),
        area,
    );
    v.detail_height = height;
}
