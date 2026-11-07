//! Small linear lexer for terminal code blocks; no grammar bundles or caches.
use slint::StyledText;

pub fn highlight(source: &str, language: &str) -> StyledText {
    let markup = markup(source, language);
    StyledText::from_markdown(&markup).unwrap_or_else(|_| StyledText::from_plain_text(source))
}

fn markup(source: &str, language: &str) -> String {
    let lang = language.to_ascii_lowercase();
    let supported = matches!(
        lang.as_str(),
        "rust"
            | "rs"
            | "python"
            | "py"
            | "js"
            | "javascript"
            | "ts"
            | "typescript"
            | "json"
            | "sh"
            | "bash"
            | "shell"
            | "sql"
            | "c"
            | "cpp"
            | "java"
            | "go"
    );
    let hash_comment = matches!(lang.as_str(), "python" | "py" | "sh" | "bash" | "shell");
    let sql = lang == "sql";
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len() * 2);
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        let mut color = None;
        if supported
            && (hash_comment && bytes[i] == b'#'
                || sql && source[i..].starts_with("--")
                || !hash_comment && !sql && source[i..].starts_with("//"))
        {
            i = source[i..].find('\n').map_or(bytes.len(), |n| i + n);
            color = Some("#6a737d");
        } else if supported && source[i..].starts_with("/*") {
            i = source[i + 2..]
                .find("*/")
                .map_or(bytes.len(), |n| i + n + 4);
            color = Some("#6a737d");
        } else if supported && matches!(bytes[i], b'"' | b'\'' | b'`') {
            let quote = bytes[i];
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i = (i + 2).min(bytes.len());
                } else if bytes[i] == quote {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
            color = Some("#22863a");
        } else if supported && bytes[i].is_ascii_digit() {
            i += 1;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'.' | b'_'))
            {
                i += 1;
            }
            color = Some("#b05a00");
        } else if supported && (bytes[i].is_ascii_alphabetic() || bytes[i] == b'_') {
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = source[start..i].to_ascii_lowercase();
            if matches!(
                word.as_str(),
                "let"
                    | "mut"
                    | "fn"
                    | "pub"
                    | "use"
                    | "impl"
                    | "struct"
                    | "enum"
                    | "match"
                    | "self"
                    | "const"
                    | "var"
                    | "function"
                    | "async"
                    | "await"
                    | "return"
                    | "if"
                    | "else"
                    | "for"
                    | "while"
                    | "loop"
                    | "in"
                    | "break"
                    | "continue"
                    | "true"
                    | "false"
                    | "null"
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
                    | "int"
                    | "void"
                    | "string"
                    | "package"
                    | "func"
            ) {
                color = Some("#8250df");
            }
        } else {
            i += source[i..].chars().next().unwrap().len_utf8();
        }
        while !source.is_char_boundary(i) {
            i += 1;
        }
        if let Some(color) = color {
            out.push_str(&format!("<font color=\"{color}\">"));
        }
        // Encode syntax and whitespace so code can never be interpreted as
        // Markdown/HTML and leading indentation/newlines remain intact.
        for ch in source[start..i].chars() {
            if ch.is_ascii() {
                out.push_str(&format!("&#{};", ch as u32));
            } else {
                out.push(ch);
            }
        }
        if color.is_some() {
            out.push_str("</font>");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escaped_code_is_literal_and_highlight_markup_parses() {
        let source = "    <tag> **text** & `code`\n\n";
        assert_eq!(
            highlight(source, "unknown"),
            StyledText::from_plain_text(source)
        );
        for lang in ["rust", "python", "json", "typescript", "sql", "bash"] {
            let output = markup("let value = 42; // 注释\nprint(\"示例\");", lang);
            assert!(output.contains("<font"));
            assert!(StyledText::from_markdown(&output).is_ok());
        }
    }
}
