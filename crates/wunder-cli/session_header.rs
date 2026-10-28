use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

pub(crate) const BRAND_MARK: &str = ">_";

pub(crate) fn brand_name(is_zh: bool) -> &'static str {
    if is_zh {
        "wunder 舵机"
    } else {
        "wunder CLI"
    }
}

pub(crate) fn header_lines(version: &str, workdir: &str, is_zh: bool) -> Vec<Line<'static>> {
    let hint = if is_zh {
        "/help 查看命令 · Ctrl+T 查看完整记录"
    } else {
        "/help for commands · ctrl+t for transcript"
    };
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
        Line::default(),
        Line::from(Span::styled(
            format!("  {hint}"),
            Style::default().add_modifier(Modifier::DIM),
        )),
    ]
}
