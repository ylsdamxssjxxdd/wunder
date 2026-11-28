use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use std::sync::OnceLock;

use super::app::LogKind;

pub(crate) const STATUS_DOT: &str = "●";
pub(crate) const STATUS_PREFIX: &str = "● ";

/// `NO_COLOR` is the de-facto switch for "this terminal cannot carry meaning in
/// hue". Read once so no frame pays for the lookup.
fn colorless() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| {
        std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty() && value != "0")
    })
}

/// One choke point: drop every hue while keeping the weight a style carries, so
/// glyphs and words remain the only signal (方案 §6 可访问性、§11.3 无色变体).
fn degrade(style: Style) -> Style {
    degrade_forced(style, colorless())
}

fn degrade_forced(style: Style, colorless: bool) -> Style {
    if !colorless {
        return style;
    }
    Style::default()
        .fg(Color::Reset)
        .bg(Color::Reset)
        .add_modifier(style.add_modifier)
}

pub(crate) fn secondary_text() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

/// The interactive accent on a dark terminal. A saturated ANSI cyan shouts next
/// to dim body text and reads as an error highlight once three cards carry it;
/// Codex paints its controls with ChatGPT Blue 200 (#63A8F8), so the same tone
/// keeps this surface quiet. Terminals that never advertise 256+ colors keep the
/// ANSI cyan, which every palette defines.
const ACCENT_RGB: (u8, u8, u8) = (99, 168, 248);

fn rich_color_supported() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| {
        let colorterm = std::env::var("COLORTERM").unwrap_or_default();
        let term = std::env::var("TERM").unwrap_or_default();
        let rich = colorterm.contains("truecolor")
            || colorterm.contains("24bit")
            || term.contains("truecolor")
            || term.contains("direct")
            || term.contains("256color");
        rich
    })
}

pub(crate) fn accent_color() -> Color {
    if rich_color_supported() {
        let (r, g, b) = ACCENT_RGB;
        Color::Rgb(r, g, b)
    } else {
        Color::Cyan
    }
}

pub(crate) fn accent_text() -> Style {
    degrade(Style::default().fg(accent_color()).add_modifier(Modifier::BOLD))
}

/// The five context-window part tints, matched to the desktop card: system,
/// tools, images, results, chat. Terminals without truecolor keep the nearest
/// ANSI slot; `NO_COLOR` strips the hue through [`degrade`].
const SLOT_INK_RGB: [(u8, u8, u8); 5] = [
    (139, 92, 246),
    (14, 165, 233),
    (16, 185, 129),
    (234, 88, 12),
    (79, 70, 229),
];
const SLOT_INK_ANSI: [Color; 5] = [
    Color::Magenta,
    Color::Cyan,
    Color::Green,
    Color::Yellow,
    Color::Blue,
];

pub(crate) fn slot_ink(slot: i32) -> Color {
    let index = ((slot.clamp(1, 5) - 1) as usize).min(4);
    if rich_color_supported() {
        let (r, g, b) = SLOT_INK_RGB[index];
        Color::Rgb(r, g, b)
    } else {
        SLOT_INK_ANSI[index]
    }
}

pub(crate) fn slot_ink_text(slot: i32, bold: bool) -> Style {
    let mut style = Style::default().fg(slot_ink(slot));
    if bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    degrade(style)
}

/// A tool card's own name — the two-character verb in front of the target.
/// Blue is the product's tool tint (the same blue that marks a running card's
/// bullet), so a reader finds "读取 / 编辑 / 执行" by hue alone; the state still
/// lives in the leading bullet, and the target next to it stays plain, which is
/// what kept the whole title from reading as one loud alert.
pub(crate) fn tool_title() -> Style {
    degrade(Style::default().fg(accent_color()).add_modifier(Modifier::BOLD))
}

/// What the tool acted on: a path, a command, a query. Same weight as the name, no
/// hue — the name carries the identity, the target carries the data.
pub(crate) fn tool_subject() -> Style {
    degrade(Style::default().add_modifier(Modifier::BOLD))
}

/// A running card's bullet, in the accent instead of a saturated cyan.
pub(crate) fn tool_pending_marker() -> Style {
    degrade(Style::default().fg(accent_color()).add_modifier(Modifier::BOLD))
}

/// The row that offers or closes a fold. Codex tints it only while the activity
/// is focused; unfocused it stays dim, so an idle transcript carries no control
/// that competes with the answer above it.
pub(crate) fn tool_fold_hint() -> Style {
    secondary_text()
}

/// Inline code and fenced blocks. A saturated cyan turned every snippet into a
/// block of neon once a tool result carried one; this is the same blue family as
/// the accent, stepped back far enough to stay a reading surface.
pub(crate) fn code_text() -> Style {
    if rich_color_supported() {
        degrade(Style::default().fg(Color::Rgb(150, 176, 204)))
    } else {
        degrade(Style::default().add_modifier(Modifier::DIM))
    }
}

pub(crate) fn success_text() -> Style {
    degrade(Style::default().fg(Color::Green))
}

pub(crate) fn link_text() -> Style {
    degrade(
        Style::default()
            .fg(accent_color())
            .add_modifier(Modifier::BOLD)
            .add_modifier(Modifier::UNDERLINED),
    )
}

pub(crate) fn brand_text() -> Style {
    degrade(Style::default().fg(Color::Magenta))
}

pub(crate) fn danger_text() -> Style {
    degrade(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
}

/// Amber for the count a warning notice leads with; the sentence next to it stays dim so the
/// number is what a reader's eye lands on.
pub(crate) fn warning_text() -> Style {
    degrade(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )
}

pub(crate) fn diff_hunk_prefix() -> Style {
    degrade(
        Style::default()
            .bg(Color::Rgb(22, 30, 50))
            .add_modifier(Modifier::DIM),
    )
}

pub(crate) fn diff_hunk_text() -> Style {
    degrade(
        Style::default()
            .fg(accent_color())
            .bg(Color::Rgb(22, 30, 50))
            .add_modifier(Modifier::BOLD),
    )
}

pub(crate) fn diff_added_prefix() -> Style {
    degrade(
        Style::default()
            .bg(Color::Rgb(18, 48, 31))
            .add_modifier(Modifier::DIM),
    )
}

pub(crate) fn diff_added_text() -> Style {
    degrade(
        Style::default()
            .fg(Color::Rgb(157, 230, 188))
            .bg(Color::Rgb(18, 48, 31)),
    )
}

pub(crate) fn diff_added_marker() -> Style {
    degrade(
        Style::default()
            .fg(Color::Rgb(157, 230, 188))
            .bg(Color::Rgb(18, 48, 31))
            .add_modifier(Modifier::BOLD),
    )
}

pub(crate) fn diff_deleted_prefix() -> Style {
    degrade(
        Style::default()
            .bg(Color::Rgb(58, 24, 24))
            .add_modifier(Modifier::DIM),
    )
}

pub(crate) fn diff_deleted_text() -> Style {
    degrade(
        Style::default()
            .fg(Color::Rgb(255, 182, 182))
            .bg(Color::Rgb(58, 24, 24)),
    )
}

pub(crate) fn diff_deleted_marker() -> Style {
    degrade(
        Style::default()
            .fg(Color::Rgb(255, 182, 182))
            .bg(Color::Rgb(58, 24, 24))
            .add_modifier(Modifier::BOLD),
    )
}

pub(crate) fn block_title(active: bool) -> Style {
    if active {
        accent_text()
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    }
}

pub(crate) fn popup_item() -> Style {
    secondary_text()
}

pub(crate) fn popup_selected() -> Style {
    degrade(
        Style::default()
            .fg(accent_color())
            .bg(Color::Rgb(26, 42, 54))
            .add_modifier(Modifier::BOLD),
    )
}

pub(crate) fn modal_selected() -> Style {
    degrade(
        Style::default()
            .fg(accent_color())
            .bg(Color::Rgb(26, 42, 54))
            .add_modifier(Modifier::BOLD),
    )
}

/// 居中选择面板的表面底色：与 composer 同族的浅色叠层（codex 的 Panel surface）。
pub(crate) fn panel_surface() -> Style {
    composer_surface()
}

/// 居中选择列表的选中行：整行填充，必须明显盖过面板底色（codex 的 selection fill）。
pub(crate) fn picker_selected() -> Style {
    degrade(
        Style::default()
            .fg(accent_color())
            .bg(Color::Rgb(40, 74, 106))
            .add_modifier(Modifier::BOLD),
    )
}

/// The fill Codex's selection surface paints. `None` when the terminal disables
/// colour, so the mark falls back to reversed video instead.
pub(crate) fn selection_fill() -> Option<Color> {
    (!colorless()).then_some(Color::Rgb(99, 168, 248))
}

/// Codex's selection surface: a light-blue fill with dark text. Terminals that
/// explicitly disable color keep a reversed, non-dim mark instead.
pub(crate) fn selection_mark() -> Style {
    let Some(fill) = selection_fill() else {
        return Style::default()
            .add_modifier(Modifier::REVERSED | Modifier::BOLD)
            .remove_modifier(Modifier::DIM);
    };
    Style::default()
        .fg(Color::Rgb(0, 0, 46))
        .bg(fill)
        .add_modifier(Modifier::BOLD)
        .remove_modifier(Modifier::DIM | Modifier::REVERSED)
}

/// Whether a cell carries the selection mark: the fill in a colour terminal,
/// reversed video where colour is off. The painter and its tests name one rule.
#[cfg(test)]
pub(crate) fn carries_selection_mark(bg: Color, modifier: Modifier) -> bool {
    modifier.contains(Modifier::REVERSED) || selection_fill().is_some_and(|fill| bg == fill)
}

pub(crate) fn transcript_selection(base: Style) -> Style {
    degrade(
        Style::default()
            .fg(base.fg.unwrap_or(Color::White))
            .bg(Color::Rgb(24, 36, 48))
            .add_modifier(base.add_modifier | Modifier::BOLD),
    )
}

/// The composer is the only editable region on screen, so it gets the same surface
/// tone the selection states use instead of floating on the terminal background.
pub(crate) fn composer_surface() -> Style {
    degrade(Style::default().bg(Color::Rgb(40, 40, 40)))
}

/// The prompt glyph keeps its weight in both focus states; only its colour yields.
pub(crate) fn composer_prompt(focused: bool) -> Style {
    if focused {
        accent_text()
    } else {
        Style::default()
            .add_modifier(Modifier::DIM)
            .add_modifier(Modifier::BOLD)
    }
}

pub(crate) fn log_style(kind: LogKind) -> Style {
    match kind {
        LogKind::Info => secondary_text(),
        LogKind::User => Style::default(),
        LogKind::Assistant => Style::default(),
        LogKind::Reasoning => secondary_text(),
        LogKind::Tool => secondary_text(),
        LogKind::Plan => secondary_text(),
        LogKind::Approval | LogKind::Inquiry => accent_text(),
        LogKind::Error => danger_text(),
    }
}

/// The bullet in front of the model's answer. Thinking already sinks to a dim italic
/// margin note, so the answer's dot is the one bright dot in a turn; it is painted
/// explicitly white instead of inheriting the terminal's default foreground, which a
/// themed terminal can set to grey — the same tone the thinking dot already uses.
pub(crate) fn answer_marker() -> Style {
    let white = if rich_color_supported() {
        Color::Rgb(255, 255, 255)
    } else {
        Color::White
    };
    degrade(Style::default().fg(white).add_modifier(Modifier::BOLD))
}

/// The bullet a transcript entry leads with. An answer carries its own bright dot;
/// every other entry keeps the tone its body text already uses.
pub(crate) fn log_prefix_style(kind: LogKind) -> Style {
    match kind {
        LogKind::Assistant => answer_marker(),
        _ => log_style(kind),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The degradation itself is tested through the forced flag: reading `NO_COLOR`
    /// from a snapshot test would mutate process state that other tests in this
    /// binary are already reading in parallel.
    #[test]
    fn degradation_keeps_the_weight_that_carries_the_meaning() {
        // The hue pair is what a colored terminal gets; the reset pair is what a
        // colorless one gets. `secondary_text` has no hue at all, so it is unchanged
        // and stays the dim baseline everywhere else builds on.
        let colored = diff_added_marker();
        assert!(colored.fg.is_some() && colored.bg.is_some());

        let degraded = degrade_forced(colored, true);
        assert_eq!(degraded.fg, Some(Color::Reset));
        assert_eq!(degraded.bg, Some(Color::Reset));
        assert!(
            degraded.add_modifier.contains(Modifier::BOLD),
            "the marker's weight is what survives without color"
        );

        let dimmed = degrade_forced(diff_added_prefix(), true);
        assert_eq!(dimmed.fg, Some(Color::Reset));
        assert!(dimmed.add_modifier.contains(Modifier::DIM));
    }
}
