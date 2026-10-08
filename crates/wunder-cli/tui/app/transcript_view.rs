//! The Ctrl+T transcript view: the same history with every card expanded and scrollable.
//!
//! Cards fold at render time, so the view reuses the ordinary entry renderer with the
//! expanded flag on and reads the windows the cards already hold. Nothing is re-parsed and
//! nothing is rebuilt per frame: lines are cached and invalidated with the transcript.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::text::{Line, Span};

use super::TuiApp;

/// Newest entries the view renders at most.
const VIEW_MAX_ENTRIES: usize = 120;
/// Line ceiling for one rebuilt view, independent of how tall the entries are.
const VIEW_MAX_LINES: usize = 2_000;

#[derive(Default)]
pub(super) struct TranscriptView {
    open: bool,
    scroll: usize,
    lines: Vec<Line<'static>>,
    /// `false` means the next frame rebuilds `lines`.
    cached: bool,
    width: u16,
    height: u16,
    /// Opening lands on the newest line, which is only known once lines are built.
    stick_bottom: bool,
}

impl TranscriptView {
    pub(super) fn invalidate(&mut self) {
        self.cached = false;
    }

    fn height(&self) -> usize {
        usize::from(self.height.max(1))
    }

    fn clamp_scroll(&mut self) {
        let last = self.lines.len().saturating_sub(1);
        if self.scroll > last {
            self.scroll = self.lines.len().saturating_sub(self.height());
        }
    }
}

/// What the transcript view does with a key, when it does anything at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ViewAction {
    Toggle,
    Close,
    Scroll(isize),
    Top,
    Bottom,
}

/// Ctrl+T opens and closes; Esc and `q` close; everything else only moves the window.
/// The control-character arm is there because legacy consoles deliver Ctrl+T as 0x14.
fn view_action(key: KeyEvent) -> Option<ViewAction> {
    if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('t')) {
        return Some(ViewAction::Toggle);
    }
    match key.code {
        KeyCode::Char('\u{0014}') => Some(ViewAction::Toggle),
        KeyCode::Esc | KeyCode::Char('q') => Some(ViewAction::Close),
        KeyCode::Up | KeyCode::Char('\u{0010}') => Some(ViewAction::Scroll(-1)),
        KeyCode::Down | KeyCode::Char('\u{000e}') => Some(ViewAction::Scroll(1)),
        KeyCode::PageUp => Some(ViewAction::Scroll(-10)),
        KeyCode::PageDown => Some(ViewAction::Scroll(10)),
        KeyCode::Home => Some(ViewAction::Top),
        KeyCode::End => Some(ViewAction::Bottom),
        _ => None,
    }
}

impl TuiApp {
    pub fn transcript_view_open(&self) -> bool {
        self.transcript_view.open
    }

    /// Handle a key while the view is up. Returns `true` when the view consumed it, so the
    /// composer never sees a key that belonged to the transcript.
    pub fn handle_transcript_view_key(&mut self, key: KeyEvent) -> bool {
        if !self.transcript_view.open {
            // Only the opener may claim a key while the view is down. Esc, `q`, Home and
            // the arrows all belong to the composer and the transcript scroll otherwise.
            if view_action(key) == Some(ViewAction::Toggle) {
                self.open_transcript_view();
                return true;
            }
            return false;
        }
        let Some(action) = view_action(key) else {
            // The view is a modal reader: a key it does not use still does not reach the
            // composer, so half-typed text cannot be hidden behind it.
            return true;
        };
        match action {
            ViewAction::Toggle | ViewAction::Close => self.close_transcript_view(),
            ViewAction::Scroll(delta) => self.scroll_transcript_view(delta),
            ViewAction::Top => self.transcript_view.scroll = 0,
            ViewAction::Bottom => {
                self.transcript_view.scroll = usize::MAX;
                self.transcript_view.clamp_scroll();
            }
        }
        true
    }

    /// Move the view window, clamped to the document. Also the mouse-wheel entry point.
    pub fn scroll_transcript_view(&mut self, delta: isize) {
        let next = self.transcript_view.scroll as isize + delta;
        self.transcript_view.scroll = next.max(0) as usize;
        self.transcript_view.clamp_scroll();
    }

    /// Enter on a selected card opens or closes its fold. Returns whether it acted, so the
    /// caller can fall back to the key's other meaning.
    pub fn toggle_selected_card(&mut self) -> bool {
        let Some(index) = self.transcript_selected else {
            return false;
        };
        let width = self.transcript_viewport_width.max(1);
        let Some(entry) = self.logs.get(index) else {
            return false;
        };
        let kind = entry.kind;
        let collapsible = if entry.special.is_some() {
            entry
                .special
                .as_ref()
                .is_some_and(|special| special.is_collapsible(width))
        } else {
            kind == super::LogKind::Reasoning
                && !self.entry_is_streaming(kind, index)
                && self.reasoning_full_rows(index, width) > super::REASONING_FOLD_LINES
        };
        if !collapsible {
            return false;
        }
        if !self.expanded_cards.insert(index) {
            self.expanded_cards.remove(&index);
        }
        self.invalidate_transcript_metrics();
        true
    }

    /// Rendered rows of a thinking block with nothing folded away.
    fn reasoning_full_rows(&mut self, index: usize, width: u16) -> usize {
        let Some(entry) = self.logs.get_mut(index) else {
            return 0;
        };
        super::ensure_markdown_cache(entry, width);
        entry
            .markdown_cache
            .as_ref()
            .map(|cache| cache.lines.len())
            .unwrap_or(0)
    }

    fn open_transcript_view(&mut self) {
        self.transcript_view.open = true;
        // Opening and closing flips every card's fold state, so the main transcript's
        // cached lines and metrics are stale the moment the view changes either way.
        self.invalidate_transcript_metrics();
        self.transcript_view.stick_bottom = true;
        self.transcript_view.scroll = 0;
    }

    fn close_transcript_view(&mut self) {
        self.transcript_view.open = false;
        self.transcript_view.scroll = 0;
        self.transcript_view.stick_bottom = false;
        self.invalidate_transcript_metrics();
    }

    /// Visible lines for this frame, rebuilding the cache only when the transcript moved.
    pub fn transcript_view_frame(&mut self, width: u16, height: u16) -> Vec<Line<'static>> {
        self.transcript_view.height = height;
        self.ensure_transcript_view_lines(width);
        if self.transcript_view.stick_bottom {
            self.transcript_view.stick_bottom = false;
            self.transcript_view.scroll = usize::MAX;
        }
        self.transcript_view.clamp_scroll();
        let start = self.transcript_view.scroll;
        let end = start
            .saturating_add(self.transcript_view.height())
            .min(self.transcript_view.lines.len());
        self.transcript_view.lines[start..end].to_vec()
    }

    /// `transcript 132/480` for the view's own status line.
    pub fn transcript_view_status(&mut self, width: u16) -> String {
        let total = self.ensure_transcript_view_lines(width);
        let shown = self
            .transcript_view
            .scroll
            .saturating_add(self.transcript_view.height())
            .min(total);
        let is_zh = self.is_zh_language();
        if is_zh {
            format!("转录 {shown}/{total}")
        } else {
            format!("transcript {shown}/{total}")
        }
    }

    fn ensure_transcript_view_lines(&mut self, width: u16) -> usize {
        if self.transcript_view.cached && self.transcript_view.width == width {
            return self.transcript_view.lines.len();
        }
        // Bounded both ways: newest entries only and a hard line ceiling. Opening the view
        // must not re-render a whole session every time the transcript moves.
        let mut chunks: Vec<Vec<Line<'static>>> = Vec::new();
        let mut total = 0usize;
        let mut next = self.logs.len();
        while next > 0 {
            if chunks.len() == VIEW_MAX_ENTRIES || total >= VIEW_MAX_LINES {
                break;
            }
            next -= 1;
            let lines = self.render_entry_lines(next, false, width);
            total = total.saturating_add(lines.len());
            chunks.push(lines);
        }
        let mut lines = Vec::with_capacity(total + usize::from(next > 0));
        if next > 0 {
            lines.push(view_ceiling_note(next, self.is_zh_language()));
        }
        for chunk in chunks.into_iter().rev() {
            lines.extend(chunk);
        }
        self.transcript_view.lines = lines;
        self.transcript_view.cached = true;
        self.transcript_view.width = width;
        self.transcript_view.lines.len()
    }
}

fn view_ceiling_note(omitted: usize, is_zh: bool) -> Line<'static> {
    let text = if is_zh {
        format!("… 较早的 {omitted} 条消息未显示")
    } else {
        format!("… {omitted} older entries not shown")
    };
    Line::from(Span::styled(text, crate::tui::theme::secondary_text()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_transcript_keys_are_claimed() {
        assert_eq!(
            view_action(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL)),
            Some(ViewAction::Toggle)
        );
        assert_eq!(
            view_action(KeyEvent::new(KeyCode::Char('\u{0014}'), KeyModifiers::NONE)),
            Some(ViewAction::Toggle),
            "legacy consoles deliver Ctrl+T as a control character"
        );
        assert_eq!(
            view_action(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            Some(ViewAction::Close)
        );
        assert_eq!(
            view_action(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE)),
            Some(ViewAction::Scroll(10))
        );
        assert_eq!(
            view_action(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
            None
        );
    }

    #[test]
    fn scroll_clamps_to_the_document_not_below_zero() {
        let mut view = TranscriptView::default();
        view.height = 4;
        view.lines = (0..20).map(|n| Line::from(format!("l{n}"))).collect();
        view.scroll = 500;
        view.clamp_scroll();
        assert_eq!(view.scroll, 16, "bottom is total minus the visible height");
        view.scroll = 0;
        view.clamp_scroll();
        assert_eq!(view.scroll, 0);
    }

    #[test]
    fn a_short_document_never_scrolls_past_its_start() {
        let mut view = TranscriptView::default();
        view.height = 10;
        view.lines = (0..3).map(|n| Line::from(format!("l{n}"))).collect();
        view.scroll = usize::MAX;
        view.clamp_scroll();
        assert_eq!(view.scroll, 0);
    }
}
