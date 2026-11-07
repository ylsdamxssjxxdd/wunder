//! Inline AST → the markup dialect Slint's `StyledText` accepts.
//!
//! `StyledText::from_markdown` understands emphasis, strong, strikethrough,
//! inline code, links, `<u>` and `<font color>`. It rejects everything else,
//! and — more importantly — it re-parses whatever string it is handed, so
//! literal text has to be neutralised before it can be mistaken for structure.
//!
//! Every ASCII character of literal text is emitted as a numeric entity. That
//! one decision solves three problems at once:
//!
//! * `*`, `_`, `[`, `<` in prose can no longer open markup;
//! * a line starting with four spaces can no longer become a code block;
//! * a space at the edge of a bold run no longer breaks the delimiter (the
//!   character before `**` is then `;`, not a space, so the run is still
//!   right-flanking).
//!
//! Newlines stay real: a soft break has to reach the parser as a break, not as
//! `&#10;` sitting inside a paragraph.

use crate::md::ast::Inline;
use std::fmt::Write as _;

/// Slint markup for one inline run.
pub fn markup(nodes: &[Inline]) -> String {
    let mut out = String::with_capacity(nodes.len() * 8);
    write_nodes(nodes, &mut out, false);
    out
}

/// Markup for a run that is bold as a whole (a heading). Already-strong spans
/// are flattened: the enclosing `**…**` covers them, and nesting the same
/// delimiter inside itself is the one construct CommonMark resolves by luck.
pub fn markup_bold(nodes: &[Inline]) -> String {
    let mut out = String::with_capacity(nodes.len() * 8);
    out.push_str("**");
    write_nodes(nodes, &mut out, true);
    out.push_str("**");
    out
}

/// Plain-text projection of the same run: copy targets, search, image labels.
pub fn plain(nodes: &[Inline]) -> String {
    let mut out = String::with_capacity(nodes.len() * 4);
    write_plain(nodes, &mut out);
    out
}

fn write_nodes(nodes: &[Inline], out: &mut String, bold: bool) {
    for node in nodes {
        match node {
            Inline::Text(text) => encode(text, out),
            Inline::Code(text) => write_code_span(text, out),
            // Both break kinds are a line break here: the renderer already
            // decided that a single newline is a new line, not a wrap hint.
            Inline::SoftBreak | Inline::HardBreak => out.push('\n'),
            Inline::Strong(children) => {
                if bold {
                    write_nodes(children, out, true);
                } else {
                    out.push_str("**");
                    write_nodes(children, out, false);
                    out.push_str("**");
                }
            }
            Inline::Emphasis(children) => {
                out.push('*');
                write_nodes(children, out, bold);
                out.push('*');
            }
            Inline::Strike(children) => {
                out.push_str("~~");
                write_nodes(children, out, bold);
                out.push_str("~~");
            }
            Inline::Link { url, children } => {
                out.push('[');
                write_nodes(children, out, bold);
                out.push_str("](<");
                write_destination(url, out);
                out.push_str(">)");
            }
        }
    }
}

fn write_plain(nodes: &[Inline], out: &mut String) {
    for node in nodes {
        match node {
            Inline::Text(text) | Inline::Code(text) => out.push_str(text),
            Inline::SoftBreak | Inline::HardBreak => out.push('\n'),
            Inline::Strong(children) | Inline::Emphasis(children) | Inline::Strike(children) => {
                write_plain(children, out)
            }
            Inline::Link { url, children } => {
                let start = out.len();
                write_plain(children, out);
                // A bare URL already reads as itself; repeating it would only
                // add noise to the copy buffer.
                if out.len() == start || out[start..].trim() != url.trim() {
                    if out.len() > start {
                        out.push_str(" (");
                        out.push_str(url);
                        out.push(')');
                    } else {
                        out.push_str(url);
                    }
                }
            }
        }
    }
}

/// Encode literal text for the `StyledText` parser. Shared with the code
/// highlighter, which needs the same guarantees inside a fence.
pub(crate) fn encode(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            // A real newline is structural for StyledText: it ends a paragraph
            // and starts the next line.
            '\n' => out.push('\n'),
            '\r' => {}
            '\t' => out.push_str("&#9;"),
            ch if ch.is_ascii() => {
                out.push_str("&#");
                let _ = write!(out, "{}", ch as u32);
                out.push(';');
            }
            ch => out.push(ch),
        }
    }
}

fn write_code_span(text: &str, out: &mut String) {
    if text.is_empty() {
        return;
    }
    // The fence has to be longer than any backtick run inside the content.
    let (mut longest, mut run) = (0usize, 0usize);
    for ch in text.chars() {
        if ch == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    let fence = "`".repeat(longest + 1);
    // One space of padding on each side is always stripped by CommonMark when
    // both ends have it, so this both preserves real edge spaces and keeps a
    // leading backtick away from the fence.
    out.push_str(&fence);
    out.push(' ');
    out.push_str(text);
    out.push(' ');
    out.push_str(&fence);
}

fn write_destination(url: &str, out: &mut String) {
    for ch in url.chars() {
        match ch {
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            '\\' => out.push_str("%5C"),
            '\n' | '\r' => out.push_str("%0A"),
            ch => out.push(ch),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::md::ast::Inline;
    use crate::md::parse;

    /// Round-trip through a real CommonMark parse: markup written by this module
    /// must read back as exactly one paragraph of the same text.
    fn roundtrip(nodes: &[Inline]) -> String {
        let source = markup(nodes);
        let blocks = parse::parse(&source);
        assert_eq!(
            blocks.len(),
            1,
            "markup opened a block boundary: {source:?} -> {blocks:?}"
        );
        match &blocks[0] {
            crate::md::ast::Block::Paragraph(inner) => plain(inner),
            other => panic!("expected one paragraph, got {other:?} from {source:?}"),
        }
    }

    fn text(value: &str) -> Vec<Inline> {
        vec![Inline::Text(value.to_string())]
    }

    #[test]
    fn literal_markdown_characters_survive_a_round_trip() {
        for source in [
            "*not italic*",
            "_not italic_",
            "# not a heading",
            "- not a list",
            "1. not a list",
            "> not a quote",
            "a | b",
            "``` not a fence",
            "<b>not html</b>",
            "4 spaces:    indented",
            "    leading four",
            "~~not struck~~",
            "back\\slash",
        ] {
            let nodes = text(source);
            assert_eq!(roundtrip(&nodes), source);
        }
    }

    #[test]
    fn emphasis_at_a_space_boundary_still_opens_and_closes() {
        let nodes = vec![Inline::Strong(vec![Inline::Text(" padded ".into())])];
        let source = markup(&nodes);
        assert_eq!(roundtrip(&nodes), " padded ");
        assert!(source.contains("**"), "{source}");
        // Bold must actually have been applied, not silently dropped.
        let blocks = parse::parse(&source);
        let crate::md::ast::Block::Paragraph(inner) = &blocks[0] else {
            panic!()
        };
        assert!(matches!(inner[0], Inline::Strong(_)), "{inner:?}");
    }

    #[test]
    fn inline_code_keeps_backticks_and_spaces() {
        let nodes = vec![Inline::Code("a `b` c".into())];
        assert_eq!(roundtrip(&nodes), "a `b` c");
        let nodes = vec![Inline::Code(" leading".into())];
        assert_eq!(roundtrip(&nodes), " leading");
    }

    #[test]
    fn links_keep_their_target_and_label() {
        let nodes = vec![Inline::Link {
            url: "https://example.com/a_(b)?q=1".into(),
            children: vec![Inline::Text("示例 [链接]".into())],
        }];
        assert_eq!(roundtrip(&nodes), "示例 [链接] (https://example.com/a_(b)?q=1)");
        let source = markup(&nodes);
        let blocks = parse::parse(&source);
        let crate::md::ast::Block::Paragraph(inner) = &blocks[0] else {
            panic!()
        };
        match &inner[0] {
            Inline::Link { url, .. } => {
                assert_eq!(url, "https://example.com/a_(b)?q=1");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn nested_emphasis_nests_instead_of_leaking_markers() {
        let nodes = vec![Inline::Strong(vec![
            Inline::Text("粗".into()),
            Inline::Emphasis(vec![Inline::Text("斜".into())]),
        ])];
        assert_eq!(roundtrip(&nodes), "粗斜");
    }

    #[test]
    fn a_bold_run_flattens_inner_strong_spans() {
        let nodes = vec![
            Inline::Text("前 ".into()),
            Inline::Strong(vec![Inline::Text("中".into())]),
            Inline::Text(" 后".into()),
        ];
        let source = markup_bold(&nodes);
        assert_eq!(source.matches("**").count(), 2, "{source}");
        assert_eq!(roundtrip(&nodes), "前 中 后");
    }
}
