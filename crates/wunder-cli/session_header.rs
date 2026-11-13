use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

pub(crate) const BRAND_MARK: &str = ">_";

pub(crate) fn brand_name(is_zh: bool) -> &'static str {
    if is_zh {
        "wunder 命令行"
    } else {
        "wunder CLI"
    }
}

pub(crate) fn header_lines(version: &str, workdir: &str, is_zh: bool) -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            Span::styled(
                format!("  {BRAND_MARK} {}", brand_name(is_zh)),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" (v{version})"),
                Style::default().add_modifier(Modifier::DIM),
            ),
        ]),
        Line::from(Span::styled(
            format!("     {workdir}"),
            Style::default().add_modifier(Modifier::DIM),
        )),
    ]
}
