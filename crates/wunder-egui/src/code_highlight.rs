//! Linear lexer for fenced code blocks.
//!
//! Ported from the Slint shell's `code_highlight` so both desktop front ends
//! colour the same tokens. The lexer is intentionally linear and stateless:
//! no grammar bundles, no caches, no extra dependency.
//!
//! Unlike the Slint version it reports byte ranges instead of styled markup,
//! so the caller owns the presentation: the egui front end pours the ranges
//! into an `egui` layout job, and any other shell could map them to its own
//! rich text without parsing colour markup back out of the string.

use std::ops::Range;

/// Token classes the renderer knows how to colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    /// Reserved words: control flow, declarations and literals.
    Keyword,
    /// Quoted strings, including unterminated ones that reach the end of input.
    String,
    /// Line and block comments.
    Comment,
    /// Numeric literals, keeping suffixes and separators together (`0xff`, `1_000`).
    Number,
    /// Called identifiers (`foo` in `foo(`) and the name after `fn`/`def`.
    Function,
    /// Built-in type names such as `int`, `u32` or `string`.
    Type,
    /// Whitespace, punctuation and anything else the lexer leaves alone.
    Plain,
}

/// Comment markers accepted by one language family.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CommentStyle {
    /// `//` lines and `/* … */` blocks: Rust, C-family, JSON.
    Slash,
    /// `#` lines: Python and the POSIX shells.
    Hash,
    /// `--` lines and `/* … */` blocks: SQL.
    Sql,
}

impl CommentStyle {
    fn line_comment(self, rest: &str) -> bool {
        match self {
            Self::Slash => rest.starts_with("//"),
            Self::Hash => rest.starts_with('#'),
            Self::Sql => rest.starts_with("--"),
        }
    }

    fn block_comment(self) -> bool {
        !matches!(self, Self::Hash)
    }
}

/// The languages the Slint shell supports, with the aliases it accepts.
fn comment_style(language: &str) -> Option<CommentStyle> {
    let language = language.trim().to_ascii_lowercase();
    match language.as_str() {
        "rust" | "rs" | "js" | "javascript" | "ts" | "typescript" | "json" | "c" | "cpp"
        | "java" | "go" => Some(CommentStyle::Slash),
        "python" | "py" | "sh" | "bash" | "shell" => Some(CommentStyle::Hash),
        "sql" => Some(CommentStyle::Sql),
        _ => None,
    }
}

fn is_keyword(word: &str) -> bool {
    matches!(
        word,
        "let"
            | "mut"
            | "fn"
            | "pub"
            | "use"
            | "impl"
            | "trait"
            | "struct"
            | "enum"
            | "match"
            | "self"
            | "const"
            | "static"
            | "var"
            | "function"
            | "async"
            | "await"
            | "return"
            | "if"
            | "else"
            | "elif"
            | "for"
            | "while"
            | "loop"
            | "in"
            | "break"
            | "continue"
            | "true"
            | "false"
            | "null"
            | "nil"
            | "none"
            | "def"
            | "class"
            | "import"
            | "from"
            | "as"
            | "try"
            | "except"
            | "raise"
            | "with"
            | "yield"
            | "select"
            | "where"
            | "join"
            | "on"
            | "order"
            | "by"
            | "group"
            | "insert"
            | "into"
            | "values"
            | "update"
            | "set"
            | "delete"
            | "and"
            | "or"
            | "not"
            | "export"
            | "default"
            | "new"
            | "package"
            | "func"
            | "go"
            | "defer"
            | "chan"
            | "range"
            | "switch"
            | "case"
            | "interface"
            | "type"
    )
}

/// Type names are split from keywords only so the renderer can give them a
/// different colour from control flow; the list stays small on purpose.
fn is_type(word: &str) -> bool {
    matches!(
        word,
        "int"
            | "uint"
            | "void"
            | "string"
            | "str"
            | "bool"
            | "float"
            | "double"
            | "char"
            | "byte"
            | "usize"
            | "isize"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "f32"
            | "f64"
            | "any"
            | "number"
            | "boolean"
            | "object"
    )
}

/// Words that name the function declared right after them. C-family code is
/// covered by the trailing `(` instead; Python needs this for `def foo:`.
fn declares_function(word: &str) -> bool {
    matches!(word, "fn" | "def" | "func" | "function")
}

/// Split `source` into contiguous, non-overlapping byte ranges.
///
/// The returned ranges always tile the whole input — joining `source[range]`
/// for every entry reproduces `source` — and every boundary sits on a UTF-8
/// character boundary. An empty input, or a language outside the supported
/// set, yields an empty vector so the caller can fall back to one colour.
pub fn spans(source: &str, language: &str) -> Vec<(Range<usize>, TokenKind)> {
    let Some(style) = comment_style(language) else {
        return Vec::new();
    };
    let bytes = source.as_bytes();
    let mut spans: Vec<(Range<usize>, TokenKind)> = Vec::new();
    // Start of the run of characters not yet claimed by a token.
    let mut plain = 0usize;
    let mut index = 0usize;
    let mut expect_function = false;

    while index < bytes.len() {
        let start = index;
        let rest = &source[start..];
        let kind = if style.line_comment(rest) {
            // A line comment runs to the newline but keeps it out of the
            // span; CRLF therefore leaves `\r` inside the comment.
            index = rest.find('\n').map_or(bytes.len(), |offset| start + offset);
            Some(TokenKind::Comment)
        } else if style.block_comment() && rest.starts_with("/*") {
            index = source[start + 2..]
                .find("*/")
                .map_or(bytes.len(), |offset| start + 2 + offset + 2);
            Some(TokenKind::Comment)
        } else if matches!(bytes[start], b'"' | b'\'' | b'`') {
            index = end_of_string(source, start);
            Some(TokenKind::String)
        } else if bytes[start].is_ascii_digit() {
            index = end_of_number(bytes, start);
            Some(TokenKind::Number)
        } else if is_identifier_start(bytes[start]) {
            index = end_of_identifier(bytes, start);
            let word = source[start..index].to_ascii_lowercase();
            if is_keyword(&word) {
                expect_function = declares_function(&word);
                Some(TokenKind::Keyword)
            } else if is_type(&word) {
                expect_function = false;
                Some(TokenKind::Type)
            } else {
                let declared = expect_function;
                let called = bytes.get(index) == Some(&b'(');
                expect_function = false;
                if declared || called {
                    Some(TokenKind::Function)
                } else {
                    Some(TokenKind::Plain)
                }
            }
        } else {
            // Whitespace keeps a pending `fn`/`def` alive; punctuation ends it.
            if !bytes[start].is_ascii_whitespace() {
                expect_function = false;
            }
            index += char_len_at(source, start);
            None
        };

        // Plain runs stay pending so adjacent punctuation and identifiers
        // share one span instead of fragmenting the layout job.
        if let Some(kind) = kind {
            if kind != TokenKind::Plain {
                if plain < start {
                    spans.push((plain..start, TokenKind::Plain));
                }
                spans.push((start..index, kind));
                plain = index;
            }
        }
    }

    if plain < bytes.len() {
        spans.push((plain..bytes.len(), TokenKind::Plain));
    }
    spans
}

fn is_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

/// Length of the character starting at `index`, which must be a boundary.
fn char_len_at(source: &str, index: usize) -> usize {
    source
        .get(index..)
        .and_then(|rest| rest.chars().next())
        .map_or(1, char::len_utf8)
}

/// Scan a quoted string, honouring backslash escapes. An unterminated string
/// simply ends at the end of input, and the index always lands on a boundary.
fn end_of_string(source: &str, start: usize) -> usize {
    let bytes = source.as_bytes();
    let quote = bytes[start];
    let mut index = start + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => {
                index += 1;
                if index < bytes.len() {
                    index += char_len_at(source, index);
                }
            }
            byte if byte == quote => {
                index += 1;
                break;
            }
            _ => index += char_len_at(source, index),
        }
    }
    index
}

fn end_of_number(bytes: &[u8], start: usize) -> usize {
    let mut index = start + 1;
    while index < bytes.len()
        && (bytes[index].is_ascii_alphanumeric() || matches!(bytes[index], b'.' | b'_'))
    {
        index += 1;
    }
    index
}

fn end_of_identifier(bytes: &[u8], start: usize) -> usize {
    let mut index = start + 1;
    while index < bytes.len() && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_') {
        index += 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joined(source: &str, spans: &[(Range<usize>, TokenKind)]) -> String {
        spans
            .iter()
            .filter_map(|(range, _)| source.get(range.clone()))
            .collect()
    }

    fn kind_for(
        source: &str,
        spans: &[(Range<usize>, TokenKind)],
        text: &str,
    ) -> Option<TokenKind> {
        spans
            .iter()
            .find(|(range, _)| source.get(range.clone()) == Some(text))
            .map(|(_, kind)| *kind)
    }

    fn assert_tiled(source: &str, spans: &[(Range<usize>, TokenKind)]) {
        assert_eq!(joined(source, spans), source, "区间必须铺满原文");
        let mut cursor = 0;
        for (range, _) in spans {
            assert_eq!(range.start, cursor, "区间必须连续且有序：{range:?}");
            assert!(
                source.is_char_boundary(range.start) && source.is_char_boundary(range.end),
                "区间切在字符中间：{range:?}"
            );
            cursor = range.end;
        }
        assert_eq!(cursor, source.len(), "末尾区间必须覆盖到文末");
    }

    #[test]
    fn rust_tokens_are_split_into_their_kinds() {
        let source = "pub fn main() {\n    let count = 42; // 注释\n    println!(\"hi\\n\");\n}\n";
        let found = spans(source, "rust");
        assert_tiled(source, &found);

        assert_eq!(kind_for(source, &found, "pub"), Some(TokenKind::Keyword));
        assert_eq!(kind_for(source, &found, "let"), Some(TokenKind::Keyword));
        assert_eq!(kind_for(source, &found, "main"), Some(TokenKind::Function));
        assert_eq!(kind_for(source, &found, "42"), Some(TokenKind::Number));
        assert_eq!(
            kind_for(source, &found, "\"hi\\n\""),
            Some(TokenKind::String)
        );
        assert_eq!(
            kind_for(source, &found, "// 注释"),
            Some(TokenKind::Comment)
        );
        assert_eq!(kind_for(source, &found, " "), Some(TokenKind::Plain));
    }

    #[test]
    fn python_and_sql_use_their_own_comment_markers() {
        let python = "def greet(name):\n    return \"hi\"  # 问候\n";
        let found = spans(python, "py");
        assert_tiled(python, &found);
        assert_eq!(kind_for(python, &found, "greet"), Some(TokenKind::Function));
        assert_eq!(kind_for(python, &found, "# 问候"), Some(TokenKind::Comment));

        let sql = "SELECT * FROM t -- 备注\n/* 块 */ WHERE id = 1";
        let found = spans(sql, "sql");
        assert_tiled(sql, &found);
        assert_eq!(kind_for(sql, &found, "-- 备注"), Some(TokenKind::Comment));
        assert_eq!(kind_for(sql, &found, "/* 块 */"), Some(TokenKind::Comment));
        assert_eq!(kind_for(sql, &found, "SELECT"), Some(TokenKind::Keyword));
    }

    #[test]
    fn unclosed_string_and_block_comment_stop_at_the_end() {
        let string = "let s = \"没有闭合的字符串";
        let found = spans(string, "rust");
        assert_tiled(string, &found);
        assert_eq!(
            kind_for(string, &found, "\"没有闭合的字符串"),
            Some(TokenKind::String)
        );

        let comment = "fn a() {}\n/* 没有闭合的块注释";
        let found = spans(comment, "cpp");
        assert_tiled(comment, &found);
        assert_eq!(
            kind_for(comment, &found, "/* 没有闭合的块注释"),
            Some(TokenKind::Comment)
        );

        // An escape at the very end must not read past the buffer.
        let trailing = "let s = \"abc\\";
        let found = spans(trailing, "rust");
        assert_tiled(trailing, &found);
    }

    #[test]
    fn unknown_language_and_empty_input_have_no_spans() {
        assert!(spans("fn main() {}", "brainfuck").is_empty());
        assert!(spans("", "rust").is_empty());
        // Whitespace-only input keeps one plain run instead of vanishing.
        let blank = "\n\t \r\n";
        let found = spans(blank, "rust");
        assert_eq!(found.len(), 1);
        assert_eq!(found.first().map(|(_, kind)| *kind), Some(TokenKind::Plain));
        assert_tiled(blank, &found);
    }

    #[test]
    fn cjk_text_splits_on_character_boundaries() {
        let source = "let 名称 = \"中文示例\"; // 中文注释\nlet 下一个 = 1;";
        let found = spans(source, "rust");
        assert_tiled(source, &found);
        assert_eq!(
            kind_for(source, &found, "\"中文示例\""),
            Some(TokenKind::String)
        );
        assert_eq!(
            kind_for(source, &found, "// 中文注释"),
            Some(TokenKind::Comment)
        );
        // The identifier after the CJK comment is still lexed normally.
        assert_eq!(kind_for(source, &found, "let"), Some(TokenKind::Keyword));
    }

    #[test]
    fn crlf_line_comments_stop_before_the_newline() {
        let source = "let a = 1; // 注释\r\nlet b = 2;";
        let found = spans(source, "rust");
        assert_tiled(source, &found);
        assert_eq!(
            kind_for(source, &found, "// 注释\r"),
            Some(TokenKind::Comment)
        );
        assert_eq!(
            found
                .iter()
                .filter(|(range, kind)| {
                    *kind == TokenKind::Keyword && source.get(range.clone()) == Some("let")
                })
                .count(),
            2,
            "换行后必须继续分词"
        );
    }

    #[test]
    fn keywords_cover_the_supported_language_aliases() {
        for language in [
            "rust",
            "rs",
            "python",
            "py",
            "js",
            "javascript",
            "ts",
            "typescript",
            "json",
            "sh",
            "bash",
            "shell",
            "sql",
            "c",
            "cpp",
            "java",
            "go",
        ] {
            let source = "let value = 1;";
            let found = spans(source, language);
            assert!(!found.is_empty(), "{language} 应被识别");
            assert_tiled(source, &found);
        }
    }
}
