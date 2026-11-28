//! `/context` overlay: the 脑图 — a treemap of what the last request carried,
//! with the rows behind it.
//!
//! The projection lives in [`crate::tui::context_window`], the tile geometry in
//! [`crate::tui::context_map`]; this module owns the panel's selection state,
//! keyboard routing, and how the map is painted (colour blocks in the terminal,
//! a shaded texture when `NO_COLOR` forbids hue).

use crate::terminal_geometry;
use crate::tui::context_map::{self, CellRect};
use crate::tui::context_window::{self, ContextState, ContextWindowView, ItemRow};
use crate::tui::theme;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub(crate) struct ContextPanelState {
    pub view: context_window::ContextWindowView,
    /// 0 = all parts; 1..5 filters one slot.
    pub filter: i32,
    pub selected: usize,
    pub detail: Option<DetailState>,
}

pub(crate) struct DetailState {
    pub slot: i32,
    pub label: String,
    /// `None` means the process released the text after restart/eviction.
    pub text: Option<String>,
}

impl super::TuiApp {
    pub(crate) fn context_panel_open(&self) -> bool {
        self.context_panel.is_some()
    }

    /// Load this thread's last recorded request and project it for the panel.
    /// The snapshot is memory/persisted metadata; detail text stays a separate
    /// per-row lookup so opening the card never reads the whole transcript.
    pub(crate) async fn open_context_panel(&mut self) -> Result<()> {
        let user = self.runtime.user_id.clone();
        let session = self.session_id.clone();
        let workspace = self.runtime.state.workspace.clone();
        let state = self.runtime.state.clone();
        let snapshot = workspace
            .load_session_context_window_async(&user, &session)
            .await;
        let capacity =
            wunder_server::api::chat::native_chat_context_capacity(&state, &user, &session)
                .await
                .ok()
                .flatten()
                .map(i64::from)
                .unwrap_or(0);
        let unobserved = self.context_unobserved_after_compaction;
        let view = context_window::project(snapshot, capacity, unobserved, self.is_zh_language());
        self.context_panel = Some(ContextPanelState {
            view,
            filter: 0,
            selected: 0,
            detail: None,
        });
        Ok(())
    }

    pub(crate) fn close_context_panel(&mut self) {
        self.context_panel = None;
    }

    pub(crate) async fn handle_context_panel_key(&mut self, key: KeyEvent) -> Result<bool> {
        let Some(panel) = self.context_panel.as_mut() else {
            return Ok(false);
        };
        match key.code {
            KeyCode::Esc => {
                if panel.detail.take().is_some() {
                    return Ok(true);
                }
                self.close_context_panel();
            }
            KeyCode::Tab => {
                panel.filter = (panel.filter + 1) % 6;
                panel.selected = 0;
                panel.detail = None;
            }
            KeyCode::Up | KeyCode::Char('k') => move_selection(panel, -1),
            KeyCode::Down | KeyCode::Char('j') => move_selection(panel, 1),
            KeyCode::Enter => self.open_context_detail(),
            _ => {}
        }
        Ok(true)
    }

    fn open_context_detail(&mut self) {
        let Some(mut panel) = self.context_panel.take() else {
            return;
        };
        let Some(row) = visible_items(&panel)
            .get(panel.selected)
            .map(|r| (*r).clone())
        else {
            self.context_panel = Some(panel);
            return;
        };
        let text = self.runtime.state.workspace.context_text(
            &self.runtime.user_id,
            &self.session_id,
            row.slot,
            &row.tag,
            &row.name,
            row.count,
        );
        panel.detail = Some(DetailState {
            slot: row.slot,
            label: row.label.clone(),
            text,
        });
        self.context_panel = Some(panel);
    }
}

fn visible_items<'a>(panel: &'a ContextPanelState) -> Vec<&'a ItemRow> {
    panel
        .view
        .items
        .iter()
        .filter(|item| panel.filter == 0 || item.slot == panel.filter)
        .collect()
}

fn move_selection(panel: &mut ContextPanelState, step: isize) {
    let visible = visible_items(panel);
    if visible.is_empty() {
        panel.selected = 0;
        return;
    }
    let count = visible.len() as isize;
    let next = (panel.selected as isize + step).rem_euclid(count) as usize;
    panel.selected = next;
}

pub(crate) fn draw(frame: &mut ratatui::Frame, app: &super::TuiApp) {
    let Some(panel) = app.context_panel.as_ref() else {
        return;
    };
    let is_zh = app.is_zh_language();
    let area = panel_rect(frame.area());
    let inner_w = area.width.saturating_sub(2);
    let inner_h = area.height.saturating_sub(2);

    let title = context_window::title(is_zh);
    let filter_label = filter_label(panel, is_zh);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme::secondary_text())
        .title(Span::styled(
            format!(" {title} · {filter_label} "),
            theme::accent_text(),
        ));

    let lines = match &panel.detail {
        Some(detail) => detail_lines(detail, is_zh, inner_w as usize),
        None => body_lines(panel, is_zh, inner_w, inner_h),
    };
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// The card is the width of a comfortable reading column, not the whole screen:
/// a treemap 200 cells wide is a strip of colour, not a map.
fn panel_rect(area: Rect) -> Rect {
    let width = area.width.saturating_sub(4).clamp(16, 100).min(area.width);
    let height = area.height.saturating_sub(2).clamp(8, 34).min(area.height);
    Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    }
}

fn body_lines(
    panel: &ContextPanelState,
    is_zh: bool,
    width: u16,
    height: u16,
) -> Vec<Line<'static>> {
    let view = &panel.view;
    let mut lines: Vec<Line<'static>> = Vec::new();

    match view.state {
        ContextState::Empty | ContextState::UnobservedAfterCompaction => {
            if view.window > 0 {
                lines.push(capacity_line(
                    0,
                    view.window,
                    view.measured,
                    width as usize,
                    is_zh,
                ));
            }
            lines.push(Line::default());
            let notice = if view.state == ContextState::Empty {
                context_window::no_request_notice(is_zh)
            } else {
                context_window::unobserved_notice(is_zh)
            };
            lines.push(Line::from(Span::styled(
                notice.to_string(),
                theme::secondary_text(),
            )));
        }
        ContextState::Observed => {
            lines.push(capacity_line(
                view.used,
                view.window,
                view.measured,
                width as usize,
                is_zh,
            ));
            if let Some(meta) = meta_line(view, is_zh) {
                lines.push(meta);
            }
            lines.push(Line::default());

            // The map is the headline but the legend is the index: a short
            // terminal drops the map and keeps the part names and shares, which
            // is what a reader needs to place a number.
            let map_rows = map_height(height);
            if map_rows > 0 {
                let map = map_lines(panel, is_zh, width, map_rows);
                if !map.is_empty() {
                    lines.extend(map);
                    lines.push(Line::default());
                }
            }
            lines.push(legend_line(view, panel.filter, is_zh));
            lines.push(Line::default());

            // Whatever is left belongs to the rows; the hint and one spacer are
            // reserved before the list is cut, so the hint never scrolls away.
            let list_rows = (height as usize).saturating_sub(lines.len() + 2);
            lines.extend(item_lines(panel, is_zh, width as usize, list_rows));
        }
    }

    lines.push(Line::default());
    lines.push(hint_line(is_zh));
    lines
}

/// The map gets a share of the card that grows with the card, and disappears
/// rather than squeezing the rows when the terminal is short.
fn map_height(height: u16) -> u16 {
    match height {
        0..=12 => 0,
        13..=15 => 5,
        16..=19 => 7,
        20..=26 => 9,
        _ => 12,
    }
}

fn capacity_line(
    used: i64,
    window: i64,
    measured: bool,
    width: usize,
    is_zh: bool,
) -> Line<'static> {
    if window <= 0 {
        let text = if is_zh {
            format!(
                "已用 {} · {}",
                context_window::fmt_k(used),
                context_window::measurement_mark(measured, is_zh)
            )
        } else {
            format!(
                "{} used · {}",
                context_window::fmt_k(used),
                context_window::measurement_mark(measured, is_zh)
            )
        };
        return Line::from(Span::styled(text, theme::secondary_text()));
    }

    if used <= 0 {
        return Line::from(Span::styled(
            context_window::window_line(window, is_zh),
            theme::secondary_text(),
        ));
    }

    let share = ((used as f64 / window as f64) * 100.0)
        .round()
        .clamp(0.0, 100.0) as i64;
    let numbers = format!(
        "{} / {} · {share}% · {}",
        context_window::fmt_k(used),
        context_window::fmt_k(window),
        context_window::measurement_mark(measured, is_zh)
    );
    let numbers_width = UnicodeWidthStr::width(numbers.as_str());
    let bar_width = width
        .saturating_sub(numbers_width)
        .saturating_sub(2)
        .min(48);
    if bar_width < 6 {
        return Line::from(vec![Span::styled(numbers, theme::secondary_text())]);
    }

    let bar = context_window::share_bar(used, window, bar_width);
    let filled = bar.chars().take_while(|ch| *ch == '█').count();
    Line::from(vec![
        Span::styled("█".repeat(filled), theme::accent_text()),
        Span::styled("░".repeat(bar_width - filled), theme::secondary_text()),
        Span::styled(" ", Style::default()),
        Span::styled(numbers, theme::secondary_text()),
    ])
}

fn meta_line(view: &ContextWindowView, is_zh: bool) -> Option<Line<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let model = context_window::model_row(&view.model, &view.provider);
    if !model.is_empty() {
        spans.push(Span::styled(model, theme::secondary_text()));
    }
    if view.turns > 0 {
        if !spans.is_empty() {
            spans.push(Span::styled(" · ", theme::secondary_text()));
        }
        spans.push(Span::styled(
            context_window::turns_line(view.turns, is_zh),
            theme::secondary_text(),
        ));
    }
    (!spans.is_empty()).then(|| Line::from(spans))
}

fn legend_line(view: &ContextWindowView, filter: i32, is_zh: bool) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    for part in &view.parts {
        if part.tokens == 0 && filter != part.slot {
            continue;
        }
        let lit = filter == 0 || filter == part.slot;
        spans.push(Span::styled("• ", theme::slot_ink_text(part.slot, lit)));
        spans.push(Span::styled(
            format!("{} {}", part.label, part.pct_text),
            if lit {
                Style::default()
            } else {
                theme::secondary_text()
            },
        ));
        spans.push(Span::styled("  ", Style::default()));
    }
    if spans.is_empty() {
        spans.push(Span::styled(
            if is_zh {
                "没有内容"
            } else {
                "Nothing here"
            }
            .to_string(),
            theme::secondary_text(),
        ));
    }
    Line::from(spans)
}

/// The treemap itself: the parts, and inside a part the rows that make it up.
/// Each tile is a filled block; labels sit inside it in a contrasting colour,
/// and the gaps between tiles are the panel showing through, which is what makes
/// the blocks read as blocks.
fn map_lines(
    panel: &ContextPanelState,
    is_zh: bool,
    width: u16,
    height: u16,
) -> Vec<Line<'static>> {
    let aspect = terminal_geometry::terminal_cell_aspect()
        .unwrap_or(terminal_geometry::FALLBACK_CELL_ASPECT);
    let tiles = context_map::layout(
        &panel.view.parts,
        &panel.view.items,
        width,
        height,
        aspect,
        is_zh,
    );
    if tiles.is_empty() {
        return Vec::new();
    }
    let textured = !theme::color_permitted();
    let mut grid: Vec<Vec<(char, Style)>> =
        vec![vec![(' ', Style::default()); width as usize]; height as usize];

    for tile in &tiles {
        let lit = panel.filter == 0 || panel.filter == tile.slot;
        let (bg, ink) = if lit {
            theme::slot_fill(tile.slot)
        } else {
            theme::slot_fill_muted(tile.slot)
        };
        let rect = tile_body(tile.rect);
        fill(&mut grid, rect, bg, ink, textured, '▒');
        if tile.header {
            if lit {
                paint_header(&mut grid, tile, rect, bg, ink);
            }
            for child in &tile.items {
                let (child_bg, child_ink) = theme::slot_fill_child(tile.slot, lit);
                let body = tile_body(child.rect);
                fill(&mut grid, body, child_bg, child_ink, textured, '░');
                if lit {
                    paint_child(&mut grid, child, body, child_bg, child_ink);
                }
            }
        } else if lit {
            paint_part(&mut grid, tile, rect, bg, ink);
        }
    }

    grid.iter().map(|row| row_line(row, width)).collect()
}

/// One cell of the gap between tiles: the tile keeps its area, the seam is the
/// panel's own background.
fn tile_body(rect: CellRect) -> CellRect {
    CellRect {
        x: rect.x,
        y: rect.y,
        w: rect.w.saturating_sub(1).max(1),
        h: rect.h.saturating_sub(1).max(1),
    }
}

fn fill(
    grid: &mut [Vec<(char, Style)>],
    rect: CellRect,
    bg: Color,
    ink: Color,
    textured: bool,
    texture: char,
) {
    let symbol = if textured { texture } else { ' ' };
    let style = Style::default()
        .bg(bg)
        .fg(if textured { ink } else { Color::Reset });
    for y in rect.y..rect.y.saturating_add(rect.h) {
        for x in rect.x..rect.x.saturating_add(rect.w) {
            if let Some(cell) = grid
                .get_mut(y as usize)
                .and_then(|row| row.get_mut(x as usize))
            {
                *cell = (symbol, style);
            }
        }
    }
}

/// A part with children names itself on its own first row, tokens pushed to the
/// right edge: the block below belongs to the rows, not to the name.
fn paint_header(
    grid: &mut [Vec<(char, Style)>],
    tile: &context_map::Tile,
    body: CellRect,
    bg: Color,
    ink: Color,
) {
    let room = body.w.saturating_sub(2) as usize;
    if room < 3 {
        return;
    }
    let style = Style::default().bg(bg).fg(ink).add_modifier(Modifier::BOLD);
    let left = clipped(&format!("{} {}", tile.label, tile.pct_text), room);
    put_text(grid, body.x + 1, body.y, &left, style);
    let tokens = tile.tokens_text.as_str();
    let left_width = UnicodeWidthStr::width(left.as_str());
    if UnicodeWidthStr::width(tokens) + left_width + 2 <= room {
        let x = body.x + body.w - 1 - UnicodeWidthStr::width(tokens) as u16;
        put_text(grid, x, body.y, tokens, style);
    }
}

/// A part without children keeps its name centred, with the token count under it
/// when the block is tall enough.
fn paint_part(
    grid: &mut [Vec<(char, Style)>],
    tile: &context_map::Tile,
    body: CellRect,
    bg: Color,
    ink: Color,
) {
    let room = body.w as usize;
    // Narrow tiles stay pure colour: a bare "4%" floating in a 3-cell sliver
    // reads as noise, and the legend already names every part.
    if room < 6 || body.h == 0 {
        return;
    }
    let label = tile.label.as_str();
    let pct = tile.pct_text.as_str();
    let joined = format!("{label} {pct}");
    let first = if UnicodeWidthStr::width(joined.as_str()) <= room {
        joined
    } else if UnicodeWidthStr::width(label) <= room {
        // The name beats the number: a bare "4%" in a sliver says nothing.
        label.to_string()
    } else if UnicodeWidthStr::width(pct) <= room {
        pct.to_string()
    } else {
        return;
    };
    let mut lines = vec![first];
    let tokens = tile.tokens_text.as_str();
    if body.h >= 3 && UnicodeWidthStr::width(tokens) <= room {
        lines.push(tokens.to_string());
    }
    paint_centered(grid, body, bg, ink, &lines);
}

fn paint_child(
    grid: &mut [Vec<(char, Style)>],
    child: &context_map::ItemTile,
    body: CellRect,
    bg: Color,
    ink: Color,
) {
    if body.w < 5 || body.h == 0 {
        return;
    }
    let room = body.w as usize;
    let name = if UnicodeWidthStr::width(child.label.as_str()) <= room {
        child.label.clone()
    } else if UnicodeWidthStr::width(child.short.as_str()) <= room {
        child.short.clone()
    } else {
        clipped(&child.label, room)
    };
    let mut lines = vec![name];
    let tokens = child.tokens_text.as_str();
    if body.h >= 2 && UnicodeWidthStr::width(tokens) <= room {
        lines.push(tokens.to_string());
    }
    lines.retain(|line| !line.is_empty());
    paint_centered(grid, body, bg, ink, &lines);
}

fn paint_centered(
    grid: &mut [Vec<(char, Style)>],
    body: CellRect,
    bg: Color,
    ink: Color,
    lines: &[String],
) {
    if body.w == 0 || body.h == 0 || lines.is_empty() {
        return;
    }
    let style = Style::default().bg(bg).fg(ink).add_modifier(Modifier::BOLD);
    let texts: Vec<&String> = lines.iter().take(2).collect();
    let spare = (body.h as usize).saturating_sub(texts.len());
    let top = body.y + (spare / 2) as u16;
    for (offset, text) in texts.iter().enumerate() {
        let width = UnicodeWidthStr::width(text.as_str());
        let x = body.x + body.w.saturating_sub(width as u16) / 2;
        put_text(grid, x, top + offset as u16, text, style);
    }
}

/// Fit a label into `room` cells, using an ellipsis rather than a hard cut.
fn clipped(text: &str, room: usize) -> String {
    if room == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(text) <= room {
        return text.to_string();
    }
    if room == 1 {
        return "…".to_string();
    }
    let mut out = String::new();
    let mut used = 0usize;
    for ch in text.chars() {
        let width = UnicodeWidthChar::width(ch).unwrap_or(1).max(1);
        if used + width + 1 > room {
            break;
        }
        out.push(ch);
        used += width;
    }
    out.push('…');
    out
}

fn put_text(grid: &mut [Vec<(char, Style)>], x: u16, y: u16, text: &str, style: Style) {
    let Some(row) = grid.get_mut(y as usize) else {
        return;
    };
    let mut cursor = x as usize;
    for ch in text.chars() {
        let width = UnicodeWidthChar::width(ch).unwrap_or(1).max(1);
        if cursor + width > row.len() {
            break;
        }
        row[cursor] = (ch, style);
        // A wide glyph covers the next cell; the terminal paints it there, so
        // the cell itself needs no own symbol.
        cursor += width;
    }
}

/// Assemble one grid row into spans, merging neighbours that share a style.
/// Wide glyphs advance the cursor by their display width, so a 中字 keeps the
/// rest of the row aligned.
fn row_line(row: &[(char, Style)], width: u16) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut buffer = String::new();
    let mut current: Option<Style> = None;
    let mut used = 0usize;
    let mut index = 0;
    while index < row.len() {
        let (ch, style) = row[index];
        if current != Some(style) {
            if !buffer.is_empty() {
                spans.push(Span::styled(
                    std::mem::take(&mut buffer),
                    current.unwrap_or_default(),
                ));
            }
            current = Some(style);
        }
        buffer.push(ch);
        let advance = UnicodeWidthChar::width(ch).unwrap_or(1).max(1);
        used += advance;
        index += advance;
    }
    if !buffer.is_empty() {
        spans.push(Span::styled(buffer, current.unwrap_or_default()));
    }
    if used < width as usize {
        spans.push(Span::styled(
            " ".repeat(width as usize - used),
            Style::default(),
        ));
    }
    Line::from(spans)
}

fn item_lines(
    panel: &ContextPanelState,
    is_zh: bool,
    width: usize,
    rows: usize,
) -> Vec<Line<'static>> {
    let visible = visible_items(panel);
    if visible.is_empty() {
        return vec![Line::from(Span::styled(
            if is_zh {
                "这里没有内容"
            } else {
                "Nothing here"
            }
            .to_string(),
            theme::secondary_text(),
        ))];
    }
    let rows = rows.max(1);
    // Keep the cursor inside the window without any stored scroll offset: the
    // selection is the only thing the user moves, so it scrolls the list.
    let offset = panel.selected.saturating_sub(rows.saturating_sub(1));
    let mut lines = Vec::new();
    for (index, row) in visible.iter().enumerate().skip(offset).take(rows) {
        let selected = index == panel.selected;
        let base = if selected {
            theme::picker_selected()
        } else {
            Style::default()
        };
        let tokens = row.tokens_text.clone();
        let label = row.label.clone();
        let label_width = UnicodeWidthStr::width(label.as_str());
        let tokens_width = UnicodeWidthStr::width(tokens.as_str());
        let pad = width.saturating_sub(2 + label_width + tokens_width + 1);
        lines.push(Line::from(vec![
            Span::styled(
                "● ",
                if selected {
                    base
                } else {
                    theme::slot_ink_text(row.slot, false)
                },
            ),
            Span::styled(label, base),
            Span::styled(
                format!("{:>pad$}{tokens}", "", pad = pad),
                if selected {
                    base
                } else {
                    theme::secondary_text()
                },
            ),
        ]));
    }
    lines
}

fn hint_line(is_zh: bool) -> Line<'static> {
    let hint = if is_zh {
        "↑↓ 选择 · Tab 部位 · Enter 详情 · Esc 关闭"
    } else {
        "↑↓ select · Tab part · Enter detail · Esc close"
    };
    Line::from(Span::styled(hint.to_string(), theme::secondary_text()))
}

fn detail_lines(detail: &DetailState, is_zh: bool, width: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(vec![Span::styled(
        detail.label.clone(),
        theme::slot_ink_text(detail.slot, true),
    )]));
    lines.push(Line::default());

    match &detail.text {
        Some(text) => {
            for wrapped in textwrap::wrap(text, width.max(4)) {
                lines.push(Line::from(wrapped.into_owned()));
            }
        }
        None => lines.push(Line::from(Span::styled(
            context_window::released_text(is_zh).to_string(),
            theme::secondary_text(),
        ))),
    }

    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        if is_zh { "Esc 返回" } else { "Esc back" }.to_string(),
        theme::secondary_text(),
    )));
    lines
}

fn filter_label(panel: &ContextPanelState, is_zh: bool) -> String {
    if panel.filter == 0 {
        return if is_zh {
            "全部".to_string()
        } else {
            "All".to_string()
        };
    }
    let kind = context_window::SLOT_KINDS
        .iter()
        .find(|(_, slot)| *slot == panel.filter)
        .map(|(kind, _)| *kind)
        .unwrap_or("system");
    context_window::part_label(kind, is_zh).to_string()
}
