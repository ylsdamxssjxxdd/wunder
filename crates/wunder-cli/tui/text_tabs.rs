//! Eight-column tab-stop projection, shared by the display side and the hit side.
//!
//! The transcript cache and `LogEntry` text keep **logical** text: tabs stay tabs, so a
//! copy round-trips the characters the model actually wrote. What a terminal renders is
//! **projected** text: every tab advances to the next eight-column stop, measured from the
//! display column the line has already consumed. A pointer answers in that projected
//! column space, so layout, wrapping, hit testing, word bounds, URL lookup and the
//! highlight all have to agree on one conversion — this module. Neither side may keep a
//! private copy of the arithmetic; a divergence here is exactly how a click lands on the
//! wrong character.
//!
//! A tab or a wide character occupies a *range* of display columns. A display column that
//! falls inside such a range maps back to the character that owns it, which is what a
//! double click on tab padding reads as "the tab", not as the character after it.

use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;

/// How far apart the stops are: the default VT100 tab pitch every terminal uses.
pub(crate) const TAB_STOP: usize = 8;

/// Columns a tab consumes when it starts at display column `column`.
pub(crate) fn tab_advance(column: usize) -> usize {
    TAB_STOP - column % TAB_STOP
}

/// The one width function: columns a character consumes starting at `column`. Tabs
/// advance to the next stop; everything else uses its display width (never zero).
pub(crate) fn char_advance(ch: char, column: usize) -> usize {
    if ch == '\t' {
        tab_advance(column)
    } else {
        ch.width().unwrap_or(0).max(1)
    }
}

/// Projected display width of a piece of logical text, tabs included.
pub(crate) fn projected_width(text: &str) -> usize {
    let mut column = 0usize;
    for ch in text.chars() {
        column += char_advance(ch, column);
    }
    column
}

/// Projected display width of a whole line, indents and spans included.
pub(crate) fn line_display_width(line: &Line<'_>) -> usize {
    let mut column = 0usize;
    for span in &line.spans {
        for ch in span.content.chars() {
            column += char_advance(ch, column);
        }
    }
    column
}

/// Project one logical line into display text: every tab becomes the spaces that carry
/// it to its eight-column stop, styles, indents and alignment untouched. A line without
/// tabs comes back equal to itself, so callers never need to test for tabs first.
pub(crate) fn expand_line(line: &Line<'_>) -> Line<'static> {
    let mut column = 0usize;
    let spans = line
        .spans
        .iter()
        .map(|span| {
            let mut text = String::with_capacity(span.content.len());
            for ch in span.content.chars() {
                if ch == '\t' {
                    let advance = tab_advance(column);
                    text.push_str(&" ".repeat(advance));
                    column += advance;
                } else {
                    text.push(ch);
                    column += ch.width().unwrap_or(0).max(1);
                }
            }
            Span::styled(text, span.style)
        })
        .collect::<Vec<_>>();
    let mut expanded = Line::from(spans).style(line.style);
    expanded.alignment = line.alignment;
    expanded
}

/// Display column where the logical character at `logical_index` (a char index, not a
/// byte index) starts. Indices past the text answer with the line's total width.
pub(crate) fn display_column_at(text: &str, logical_index: usize) -> usize {
    let mut column = 0usize;
    for (index, ch) in text.chars().enumerate() {
        if index == logical_index {
            return column;
        }
        column += char_advance(ch, column);
    }
    column
}

/// Logical char index (char count, not bytes) owning a display column. A column inside
/// the padding a tab or a wide character covers answers with that character; a column
/// past the end answers with the index just past the last character.
pub(crate) fn logical_index_at(text: &str, display_column: usize) -> usize {
    let mut column = 0usize;
    for (index, ch) in text.chars().enumerate() {
        let advance = char_advance(ch, column);
        if display_column < column + advance {
            return index;
        }
        column += advance;
    }
    text.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_advance_to_eight_sixteen_and_twenty_four() {
        let expanded = expand_line(&Line::from("a\tb\tc\td"));
        let text = expanded
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(text, "a       b       c       d");
        assert_eq!(projected_width("a\tb\tc\td"), 25);
        assert_eq!(display_column_at("a\tb\tc\td", 2), 8);
        assert_eq!(display_column_at("a\tb\tc\td", 4), 16);
        assert_eq!(display_column_at("a\tb\tc\td", 6), 24);
    }

    #[test]
    fn a_leading_tab_starts_the_line_at_the_first_stop() {
        assert_eq!(display_column_at("\tvalue", 1), 8);
        assert_eq!(projected_width("\tvalue"), 13);
        let expanded = expand_line(&Line::from("\tvalue"));
        assert_eq!(expanded.spans[0].content.as_ref(), "        value");
    }

    #[test]
    fn a_tab_after_a_wide_character_counts_display_columns() {
        // 中 occupies columns 0..2, so the tab runs 2..8 and the next word starts at 8.
        assert_eq!(display_column_at("中\tgo", 1), 2);
        assert_eq!(display_column_at("中\tgo", 2), 8);
        assert_eq!(projected_width("中\tgo"), 10);
    }

    #[test]
    fn display_columns_map_back_to_the_character_that_owns_them() {
        // gate\tvalue: gate 0..4, the tab covers 4..8, value sits at 8..13.
        assert_eq!(logical_index_at("gate\tvalue", 3), 3);
        assert_eq!(
            logical_index_at("gate\tvalue", 4),
            4,
            "the tab's first column"
        );
        assert_eq!(
            logical_index_at("gate\tvalue", 6),
            4,
            "tab padding is the tab"
        );
        assert_eq!(logical_index_at("gate\tvalue", 7), 4);
        assert_eq!(logical_index_at("gate\tvalue", 8), 5);
        assert_eq!(logical_index_at("gate\tvalue", 12), 9);
        assert_eq!(logical_index_at("gate\tvalue", 30), 10, "past the end");
    }

    #[test]
    fn the_round_trip_lands_on_a_stop_for_every_tab() {
        for text in ["\tvalue", "a\tb\tc", "中\tx\ty", "gate\tvalue\tend"] {
            let count = text.chars().count();
            for index in 0..=count {
                let column = display_column_at(text, index);
                if index == count {
                    assert_eq!(column, projected_width(text));
                    continue;
                }
                assert_eq!(
                    logical_index_at(text, column),
                    index,
                    "column {column} of {text:?} must come back to {index}"
                );
            }
        }
    }

    #[test]
    fn expanding_keeps_styles_and_leaves_tab_free_lines_alone() {
        let line = Line::from(vec![
            Span::raw("head\t"),
            Span::styled(
                "x\ty",
                ratatui::style::Style::default().fg(ratatui::style::Color::Green),
            ),
        ]);
        let expanded = expand_line(&line);
        assert_eq!(expanded.spans.len(), 2);
        assert_eq!(
            expanded.spans[1].style.fg,
            Some(ratatui::style::Color::Green)
        );
        let text = expanded
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(text, "head    x       y");
        let plain = expand_line(&Line::from("no tabs here"));
        assert_eq!(plain.spans[0].content.as_ref(), "no tabs here");
    }
}
