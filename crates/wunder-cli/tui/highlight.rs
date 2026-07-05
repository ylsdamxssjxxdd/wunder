#[cfg(not(target_vendor = "win7"))]
use std::sync::OnceLock;

#[cfg(not(target_vendor = "win7"))]
use ratatui::style::Color;
#[cfg(not(target_vendor = "win7"))]
use ratatui::style::Modifier;
#[cfg(not(target_vendor = "win7"))]
use ratatui::style::Style;
use ratatui::text::Line;
#[cfg(not(target_vendor = "win7"))]
use ratatui::text::Span;
#[cfg(not(target_vendor = "win7"))]
use syntect::easy::HighlightLines;
#[cfg(not(target_vendor = "win7"))]
use syntect::highlighting::FontStyle;
#[cfg(not(target_vendor = "win7"))]
use syntect::highlighting::Style as SyntectStyle;
#[cfg(not(target_vendor = "win7"))]
use syntect::highlighting::Theme;
#[cfg(not(target_vendor = "win7"))]
use syntect::highlighting::ThemeSet;
#[cfg(not(target_vendor = "win7"))]
use syntect::parsing::SyntaxSet;

#[cfg(not(target_vendor = "win7"))]
const MAX_HIGHLIGHT_CHARS: usize = 40_000;
#[cfg(not(target_vendor = "win7"))]
const MAX_HIGHLIGHT_LINES: usize = 600;

#[cfg(target_vendor = "win7")]
pub(crate) fn highlight_code_to_lines(_code: &str, _lang: &str) -> Option<Vec<Line<'static>>> {
    None
}

#[cfg(not(target_vendor = "win7"))]
pub(crate) fn highlight_code_to_lines(code: &str, lang: &str) -> Option<Vec<Line<'static>>> {
    if code.is_empty()
        || code.len() > MAX_HIGHLIGHT_CHARS
        || code.lines().count() > MAX_HIGHLIGHT_LINES
    {
        return None;
    }

    let token = normalize_language_token(lang)?;
    let syntax_set = syntax_set();
    let syntax = syntax_set
        .find_syntax_by_token(token)
        .or_else(|| syntax_set.find_syntax_by_extension(token))?;
    let mut highlighter = HighlightLines::new(syntax, theme());
    let mut lines = Vec::new();

    for raw_line in code.split_inclusive('\n') {
        let newline_trimmed = raw_line.trim_end_matches(['\r', '\n']);
        if newline_trimmed.is_empty() {
            let _ = highlighter.highlight_line(raw_line, syntax_set).ok()?;
            lines.push(Line::default());
            continue;
        }

        let ranges = highlighter.highlight_line(raw_line, syntax_set).ok()?;
        let spans = ranges
            .into_iter()
            .filter_map(|(style, text)| {
                let trimmed = text.trim_end_matches(['\r', '\n']);
                if trimmed.is_empty() {
                    None
                } else {
                    Some(Span::styled(trimmed.to_string(), syntect_to_ratatui(style)))
                }
            })
            .collect::<Vec<_>>();
        if spans.is_empty() {
            lines.push(Line::default());
        } else {
            lines.push(Line::from(spans));
        }
    }

    if lines.is_empty() && !code.is_empty() {
        lines.push(Line::from(code.to_string()));
    }

    Some(lines)
}

#[cfg(not(target_vendor = "win7"))]
fn normalize_language_token(lang: &str) -> Option<&str> {
    let token = lang
        .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == ';')
        .find(|value| !value.trim().is_empty())?;
    Some(token.trim())
}

#[cfg(not(target_vendor = "win7"))]
fn syntax_set() -> &'static SyntaxSet {
    static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

#[cfg(not(target_vendor = "win7"))]
fn theme() -> &'static Theme {
    static THEMES: OnceLock<ThemeSet> = OnceLock::new();
    let theme_set = THEMES.get_or_init(ThemeSet::load_defaults);
    theme_set
        .themes
        .get("base16-ocean.dark")
        .or_else(|| theme_set.themes.values().next())
        .expect("syntect default themes should provide at least one theme")
}

#[cfg(not(target_vendor = "win7"))]
fn syntect_to_ratatui(style: SyntectStyle) -> Style {
    let mut output = Style::default().fg(Color::Rgb(
        style.foreground.r,
        style.foreground.g,
        style.foreground.b,
    ));
    if style.font_style.contains(FontStyle::BOLD) {
        output = output.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        output = output.add_modifier(Modifier::ITALIC);
    }
    if style.font_style.contains(FontStyle::UNDERLINE) {
        output = output.add_modifier(Modifier::UNDERLINED);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_highlighting_returns_lines() {
        let lines = highlight_code_to_lines("fn main() {\n    println!(\"hi\");\n}\n", "rust")
            .expect("expected syntax-highlighted rust lines");
        assert!(lines.len() >= 3);
        assert!(lines
            .iter()
            .any(|line| { line.spans.iter().any(|span| span.style.fg.is_some()) }));
    }
}
