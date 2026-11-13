use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub(crate) const BRAND_MARK: &str = ">_";

/// The brand row is the only coloured text in the banner: a calm blue, so the
/// opening line reads as the product name instead of as another grey log row.
/// `NO_COLOR` keeps the weight and drops the hue.
const BRAND_BLUE: Color = Color::Rgb(99, 168, 248);

fn colours_enabled() -> bool {
    std::env::var_os("NO_COLOR").is_none()
}

fn brand_style() -> Style {
    let style = Style::default().add_modifier(Modifier::BOLD);
    if colours_enabled() {
        style.fg(BRAND_BLUE)
    } else {
        style
    }
}

fn version_style() -> Style {
    let style = Style::default().add_modifier(Modifier::DIM);
    if colours_enabled() {
        style.fg(BRAND_BLUE)
    } else {
        style
    }
}

pub(crate) fn brand_name(is_zh: bool) -> &'static str {
    if is_zh {
        "wunder 命令行"
    } else {
        "wunder CLI"
    }
}

/// The heading rows of a fresh thread. They carry their own left margin and are
/// never re-centred: the welcome text reads like ordinary CLI output, and only
/// the relief below it sits on the terminal's axis.
pub(crate) fn header_lines(version: &str, workdir: &str, is_zh: bool) -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            Span::styled(
                format!("  {BRAND_MARK} {}", brand_name(is_zh)),
                brand_style(),
            ),
            Span::styled(format!(" (v{version})"), version_style()),
        ]),
        Line::from(Span::styled(
            format!("     {workdir}"),
            Style::default().add_modifier(Modifier::DIM),
        )),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(line: &Line<'static>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn the_brand_row_starts_at_the_left_margin() {
        let lines = header_lines("0.4.0", "/tmp/work", true);
        assert!(text_of(&lines[0]).starts_with("  >_ wunder 命令行"));
        assert!(text_of(&lines[1]).starts_with("     /tmp/work"));
    }

    #[test]
    fn only_the_brand_row_carries_the_blue() {
        let lines = header_lines("0.4.0", "/tmp/work", true);
        let brand = &lines[0].spans;
        assert_eq!(brand[0].style.fg, Some(BRAND_BLUE));
        assert!(brand[0].style.add_modifier.contains(Modifier::BOLD));
        // The version stays quiet: same hue, less weight.
        assert_eq!(brand[1].style.fg, Some(BRAND_BLUE));
        assert!(brand[1].style.add_modifier.contains(Modifier::DIM));
        for span in &lines[1].spans {
            assert_eq!(span.style.fg, None);
        }
    }
}
