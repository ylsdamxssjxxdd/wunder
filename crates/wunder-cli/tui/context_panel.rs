//! `/context` overlay: browse the recorded context window composition.
//!
//! The projection lives in [`crate::tui::context_window`]; this module only owns
//! the panel's selection state, keyboard routing and terminal rendering.

use crate::tui::context_window::{self, ContextState, ItemRow};
use crate::tui::theme;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

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
    let area = centered_rect(frame.area());
    let width = area.width.saturating_sub(4).max(4) as usize;

    let title = context_window::title(is_zh);
    let filter_label = filter_label(panel, is_zh);
    let block = Block::default().borders(Borders::ALL).title(Span::styled(
        format!(" {title} · {filter_label} "),
        theme::accent_text(),
    ));

    let lines: Vec<Line<'static>> = if let Some(detail) = &panel.detail {
        detail_lines(detail, is_zh, width)
    } else {
        list_lines(panel, is_zh, width)
    };
    frame.render_widget(Paragraph::new(lines).block(block), area);
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

fn list_lines(panel: &ContextPanelState, is_zh: bool, width: usize) -> Vec<Line<'static>> {
    let view = &panel.view;
    let mut lines: Vec<Line<'static>> = Vec::new();

    match view.state {
        ContextState::Empty => {
            if view.window > 0 {
                lines.push(plain(context_window::window_line(view.window, is_zh)));
            }
            lines.push(plain(context_window::no_request_notice(is_zh)));
        }
        ContextState::UnobservedAfterCompaction => {
            if view.window > 0 {
                lines.push(plain(context_window::window_line(view.window, is_zh)));
            }
            lines.push(plain(context_window::unobserved_notice(is_zh)));
        }
        ContextState::Observed => {
            lines.push(plain(context_window::summary_line(
                view.used,
                view.window,
                view.measured,
                is_zh,
            )));
            let model_row = context_window::model_row(&view.model, &view.provider);
            if !model_row.is_empty() {
                lines.push(plain(model_row));
            }
            if view.turns > 0 {
                lines.push(plain(context_window::turns_line(view.turns, is_zh)));
            }
            lines.push(Line::default());

            for part in &view.parts {
                let left = context_window::part_label(&part.kind, is_zh);
                let right = format!("{}  {}", part.tokens_text, part.pct_text);
                lines.push(Line::from(vec![
                    Span::styled("● ", theme::slot_ink_text(part.slot, false)),
                    Span::styled(left.to_string(), theme::slot_ink_text(part.slot, true)),
                    plain_span(format!("  {right}")),
                ]));
            }
            lines.push(Line::default());

            let visible = visible_items(panel);
            if visible.is_empty() {
                lines.push(plain(if is_zh {
                    "这里没有内容"
                } else {
                    "Nothing here"
                }));
            }
            for (index, row) in visible.into_iter().enumerate() {
                let selected = index == panel.selected;
                let right = row.tokens_text.clone();
                let mut spans = vec![Span::styled(
                    row.label.clone(),
                    if selected {
                        theme::picker_selected()
                    } else {
                        Style::default()
                    },
                )];
                let pad = width.saturating_sub(row.label.len() + right.len() + 2);
                spans.push(Span::styled(
                    format!("{:>pad$}{right}", "", pad = pad),
                    theme::secondary_text(),
                ));
                lines.push(Line::from(spans));
            }
        }
    }

    lines.push(Line::default());
    let hint = if is_zh {
        "↑↓ 选择 · Tab 部位 · Enter 详情 · Esc 关闭"
    } else {
        "↑↓ select · Tab part · Enter detail · Esc close"
    };
    lines.push(plain(hint));
    lines
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
                lines.push(plain(wrapped.into_owned()));
            }
        }
        None => lines.push(plain(context_window::released_text(is_zh))),
    }

    lines.push(Line::default());
    let hint = if is_zh { "Esc 返回" } else { "Esc back" };
    lines.push(plain(hint));
    lines
}

fn plain(text: impl Into<String>) -> Line<'static> {
    Line::from(plain_span(text))
}

fn plain_span(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), Style::default())
}

fn centered_rect(area: Rect) -> Rect {
    let width = area.width.min(80).max(20);
    let height = area.height.saturating_sub(2).min(24).max(6);
    Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    }
}
