use super::*;

#[derive(Clone, Debug)]
pub(super) struct PreflightDiagnostic {
    rule: &'static str,
    severity: &'static str,
    message: String,
    hint: Option<String>,
}

impl PreflightDiagnostic {
    fn reject(rule: &'static str, message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            rule,
            severity: "error",
            message: message.into(),
            hint: Some(hint.into()),
        }
    }

    fn rewrite(rule: &'static str, message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            rule,
            severity: "warn",
            message: message.into(),
            hint: Some(hint.into()),
        }
    }

    pub(super) fn to_value(&self) -> Value {
        json!({
            "rule": self.rule,
            "severity": self.severity,
            "message": self.message,
            "hint": self.hint,
        })
    }
}

#[derive(Clone, Debug)]
pub(super) enum PreflightDecision {
    Pass,
    Rewrite {
        code: &'static str,
        args: Value,
        diagnostics: Vec<PreflightDiagnostic>,
    },
    Reject {
        code: &'static str,
        message: String,
        diagnostics: Vec<PreflightDiagnostic>,
    },
}

impl Orchestrator {
    pub(super) fn run_tool_preflight(&self, tool_name: &str, args: &Value) -> PreflightDecision {
        if is_execute_command_tool_name(tool_name) {
            if let Some(content) = extract_text_field(args, "content") {
                if let Some(line) = detect_bad_heredoc_line(content) {
                    return PreflightDecision::Reject {
                        code: "PRECHECK_SHELL_BAD_HEREDOC",
                        message: "Command preflight blocked: detected invalid heredoc syntax. Use <<EOF ... EOF.".to_string(),
                        diagnostics: vec![PreflightDiagnostic::reject(
                            "shell.heredoc.invalid_redirect",
                            format!("Detected suspicious heredoc-like redirection: {line}"),
                            "Replace `< 'EOF'` with `<<'EOF'`, or write script to a file and execute it.",
                        )],
                    };
                }
                if let Some(line) = detect_large_printf_script(content) {
                    return PreflightDecision::Reject {
                        code: "PRECHECK_SHELL_PRINTF_INLINE_SCRIPT",
                        message:
                            "Command preflight blocked: oversized inline printf script is brittle."
                                .to_string(),
                        diagnostics: vec![PreflightDiagnostic::reject(
                            "shell.printf.oversized_inline_script",
                            format!("Detected oversized inline printf script: {line}"),
                            "Use `write_file` to create script file, then run `python script.py`.",
                        )],
                    };
                }
            }
            return PreflightDecision::Pass;
        }

        if is_programmatic_tool_name(tool_name) {
            let mut effective_args = args.clone();
            let mut rewrite_diagnostics = Vec::new();
            if let Some((rewritten_args, diagnostics)) =
                maybe_rewrite_python_content_args(&effective_args, "content")
            {
                effective_args = rewritten_args;
                rewrite_diagnostics = diagnostics;
            }

            if let Some(content) = extract_text_field_raw(&effective_args, "content") {
                if let Some((line_no, indent)) = detect_non_standard_python_indentation(content) {
                    return PreflightDecision::Reject {
                        code: "PRECHECK_PYTHON_INDENTATION",
                        message:
                            "Script preflight blocked: non-standard Python indentation detected."
                                .to_string(),
                        diagnostics: vec![PreflightDiagnostic::reject(
                            "python.indent.non_standard",
                            format!("Line {line_no} uses indentation width {indent}."),
                            "Use consistent 4-space indentation or only tabs (not mixed).",
                        )],
                    };
                }
                if has_unbalanced_python_brackets(content) {
                    return PreflightDecision::Reject {
                        code: "PRECHECK_PYTHON_BRACKET",
                        message: "Script preflight blocked: unmatched Python bracket detected."
                            .to_string(),
                        diagnostics: vec![PreflightDiagnostic::reject(
                            "python.bracket.unbalanced",
                            "Detected unmatched bracket/brace in script.".to_string(),
                            "Fix bracket pairing before execution.",
                        )],
                    };
                }
            }
            if !rewrite_diagnostics.is_empty() {
                return PreflightDecision::Rewrite {
                    code: "PRECHECK_PYTHON_INDENTATION_NORMALIZED",
                    args: effective_args,
                    diagnostics: rewrite_diagnostics,
                };
            }
            return PreflightDecision::Pass;
        }

        if is_db_query_tool_name(tool_name) {
            if let Some(sql) = extract_text_field(args, "sql") {
                let normalized = normalize_sql_punctuation(sql);
                if normalized != sql {
                    let mut rewritten = args.clone();
                    if let Some(obj) = rewritten.as_object_mut() {
                        obj.insert("sql".to_string(), Value::String(normalized));
                        return PreflightDecision::Rewrite {
                            code: "PRECHECK_SQL_PUNCTUATION_NORMALIZED",
                            args: rewritten,
                            diagnostics: vec![PreflightDiagnostic::rewrite(
                                "sql.punctuation.fullwidth",
                                "Detected fullwidth SQL punctuation; auto-normalized to ASCII."
                                    .to_string(),
                                "Review SQL punctuation when using Chinese input method.",
                            )],
                        };
                    }
                }
            }
            return PreflightDecision::Pass;
        }

        PreflightDecision::Pass
    }
}

fn extract_text_field<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.as_object()
        .and_then(|obj| obj.get(key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn extract_text_field_raw<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.as_object()
        .and_then(|obj| obj.get(key))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
}

fn is_execute_command_tool_name(name: &str) -> bool {
    let cleaned = name.trim();
    cleaned == resolve_tool_name("execute_command")
        || cleaned.eq_ignore_ascii_case("execute_command")
}

fn is_programmatic_tool_name(name: &str) -> bool {
    let cleaned = name.trim();
    cleaned == "ptc"
        || cleaned.eq_ignore_ascii_case("programmatic_tool_call")
        || cleaned == resolve_tool_name("programmatic_tool_call")
}

fn is_db_query_tool_name(name: &str) -> bool {
    let cleaned = name.trim().to_ascii_lowercase();
    cleaned == "db_query" || cleaned.ends_with("@db_query")
}

fn maybe_rewrite_python_content_args(
    args: &Value,
    content_key: &str,
) -> Option<(Value, Vec<PreflightDiagnostic>)> {
    let content = extract_text_field_raw(args, content_key)?;
    let (rewritten_content, diagnostics) = normalize_python_indentation(content)?;
    let mut rewritten_args = args.clone();
    let object = rewritten_args.as_object_mut()?;
    object.insert(
        content_key.to_string(),
        Value::String(rewritten_content.clone()),
    );
    (rewritten_content != content).then_some((rewritten_args, diagnostics))
}

fn normalize_python_indentation(script: &str) -> Option<(String, Vec<PreflightDiagnostic>)> {
    let mut rewritten = script.to_string();
    let mut diagnostics = Vec::new();

    if let Some((dedented, indent_width)) = dedent_common_leading_indent(&rewritten) {
        rewritten = dedented;
        diagnostics.push(PreflightDiagnostic::rewrite(
            "python.indent.global_offset",
            format!("Detected global leading indentation ({indent_width} spaces); auto-dedented."),
            "Keep module-level Python code at column 0 and indent nested blocks with 4 spaces.",
        ));
    }

    if let Some((tabs_normalized, tab_lines)) = normalize_leading_tabs(&rewritten) {
        rewritten = tabs_normalized;
        diagnostics.push(PreflightDiagnostic::rewrite(
            "python.indent.tabs_to_spaces",
            format!(
                "Detected tabs in leading indentation on {tab_lines} line(s); auto-normalized."
            ),
            "Prefer spaces-only indentation for generated Python scripts.",
        ));
    }

    if let Some((expanded, line_count)) = expand_two_space_indentation(&rewritten) {
        rewritten = expanded;
        diagnostics.push(PreflightDiagnostic::rewrite(
            "python.indent.two_to_four_spaces",
            format!(
                "Detected 2-space indentation style on {line_count} line(s); expanded to 4 spaces."
            ),
            "Use 4-space indentation consistently to avoid Python syntax errors.",
        ));
    }

    if diagnostics.is_empty() || detect_non_standard_python_indentation(&rewritten).is_some() {
        return None;
    }

    Some((rewritten, diagnostics))
}

fn dedent_common_leading_indent(script: &str) -> Option<(String, usize)> {
    let (mut lines, trailing_newline) = split_lines(script);
    let mut first_indent = None;
    for line in &lines {
        if line.trim().is_empty() {
            continue;
        }
        let indent = leading_whitespace(line);
        if !indent.is_empty() {
            first_indent = Some(indent.to_string());
        }
        break;
    }
    let indent = first_indent?;
    let indent_width = indent.chars().count();
    if indent_width == 0 {
        return None;
    }

    let mut changed = 0usize;
    for line in &mut lines {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with(&indent) {
            *line = line[indent.len()..].to_string();
            changed = changed.saturating_add(1);
        }
    }
    if changed < 2 {
        return None;
    }

    let rewritten = join_lines(&lines, trailing_newline);
    (rewritten != script).then_some((rewritten, indent_width))
}

fn normalize_leading_tabs(script: &str) -> Option<(String, usize)> {
    let (mut lines, trailing_newline) = split_lines(script);
    let mut touched_lines = 0usize;
    for line in &mut lines {
        if line.is_empty() {
            continue;
        }
        let indent = leading_whitespace(line);
        if !indent.contains('\t') {
            continue;
        }
        let normalized_indent = indent.replace('\t', "    ");
        if normalized_indent == indent {
            continue;
        }
        touched_lines = touched_lines.saturating_add(1);
        *line = format!("{}{rest}", normalized_indent, rest = &line[indent.len()..]);
    }
    if touched_lines == 0 {
        return None;
    }
    let rewritten = join_lines(&lines, trailing_newline);
    (rewritten != script).then_some((rewritten, touched_lines))
}

fn expand_two_space_indentation(script: &str) -> Option<(String, usize)> {
    let (mut lines, trailing_newline) = split_lines(script);
    let mut has_non_multiple_of_four = false;
    let mut expandable_lines = 0usize;

    for line in &lines {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = leading_whitespace(line);
        if indent.is_empty() {
            continue;
        }
        if indent.contains('\t') {
            return None;
        }
        let spaces = indent.chars().filter(|ch| *ch == ' ').count();
        if spaces % 4 == 0 {
            continue;
        }
        if spaces % 2 != 0 {
            return None;
        }
        has_non_multiple_of_four = true;
    }
    if !has_non_multiple_of_four {
        return None;
    }

    for line in &mut lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let indent = leading_whitespace(line);
        if indent.is_empty() || indent.contains('\t') {
            continue;
        }
        let spaces = indent.chars().filter(|ch| *ch == ' ').count();
        if spaces == 0 || spaces % 2 != 0 {
            continue;
        }
        let expanded_spaces = spaces.saturating_mul(2);
        if expanded_spaces == spaces {
            continue;
        }
        expandable_lines = expandable_lines.saturating_add(1);
        let rest = &line[indent.len()..];
        *line = format!("{}{rest}", " ".repeat(expanded_spaces));
    }

    if expandable_lines == 0 {
        return None;
    }

    let rewritten = join_lines(&lines, trailing_newline);
    (rewritten != script).then_some((rewritten, expandable_lines))
}

fn split_lines(script: &str) -> (Vec<String>, bool) {
    let trailing_newline = script.ends_with('\n');
    let mut lines = script
        .split('\n')
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if trailing_newline && lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    (lines, trailing_newline)
}

fn join_lines(lines: &[String], trailing_newline: bool) -> String {
    let mut joined = lines.join("\n");
    if trailing_newline {
        joined.push('\n');
    }
    joined
}

fn leading_whitespace(line: &str) -> &str {
    let len = line
        .chars()
        .take_while(|ch| *ch == ' ' || *ch == '\t')
        .count();
    &line[..len]
}

fn detect_bad_heredoc_line(content: &str) -> Option<String> {
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let contains_marker = line.contains("EOF")
            || line.contains("PYEOF")
            || line.contains("ENDOFSCRIPT")
            || line.contains("SCRIPTEND");
        if !contains_marker || line.contains("<<") {
            continue;
        }
        let suspicious = line.contains("< '")
            || line.contains("<\"")
            || line.contains("< '")
            || line.contains("<'")
            || line.contains("< \"");
        if suspicious {
            return Some(line.chars().take(200).collect());
        }
    }
    None
}

fn detect_large_printf_script(content: &str) -> Option<String> {
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if !line.starts_with("printf '%s\\n'") {
            continue;
        }
        if line.chars().count() > 480 {
            return Some(line.chars().take(200).collect());
        }
    }
    None
}

fn detect_non_standard_python_indentation(script: &str) -> Option<(usize, usize)> {
    let mut saw_tabs = false;
    let mut saw_spaces = false;
    for (idx, raw_line) in script.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent: String = raw_line
            .chars()
            .take_while(|ch| *ch == ' ' || *ch == '\t')
            .collect();
        if indent.is_empty() {
            continue;
        }
        if indent.contains('\t') {
            saw_tabs = true;
        }
        let space_count = indent.chars().filter(|ch| *ch == ' ').count();
        if space_count > 0 {
            saw_spaces = true;
        }
        if space_count > 0 && !indent.contains('\t') && space_count % 4 != 0 {
            return Some((line_no, space_count));
        }
        if indent.contains('\t') && space_count > 0 {
            return Some((line_no, space_count));
        }
    }
    if saw_tabs && saw_spaces {
        return Some((1, 0));
    }
    None
}

fn has_unbalanced_python_brackets(script: &str) -> bool {
    let mut stack: Vec<char> = Vec::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    for ch in script.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if (in_single || in_double) && ch == '\\' {
            escaped = true;
            continue;
        }
        if in_single {
            if ch == '\'' {
                in_single = false;
            }
            continue;
        }
        if in_double {
            if ch == '"' {
                in_double = false;
            }
            continue;
        }
        match ch {
            '\'' => in_single = true,
            '"' => in_double = true,
            '(' | '[' | '{' => stack.push(ch),
            ')' | ']' | '}' => {
                let expected = match ch {
                    ')' => '(',
                    ']' => '[',
                    '}' => '{',
                    _ => unreachable!(),
                };
                if stack.pop() != Some(expected) {
                    return true;
                }
            }
            _ => {}
        }
    }
    in_single || in_double || !stack.is_empty()
}

fn normalize_sql_punctuation(sql: &str) -> String {
    sql.replace('\u{FF0C}', ",")
        .replace('\u{FF1B}', ";")
        .replace('\u{FF08}', "(")
        .replace('\u{FF09}', ")")
}

#[cfg(test)]
mod tests {
    use super::{
        dedent_common_leading_indent, detect_bad_heredoc_line, detect_large_printf_script,
        detect_non_standard_python_indentation, expand_two_space_indentation,
        has_unbalanced_python_brackets, normalize_leading_tabs, normalize_sql_punctuation,
    };

    #[test]
    fn detects_invalid_heredoc_redirect_pattern() {
        let line = "python3 < 'EOF'\nprint('x')\nEOF";
        assert!(detect_bad_heredoc_line(line).is_some());
    }

    #[test]
    fn detects_non_standard_python_indent() {
        let script = "if True:\n   x = 1\n";
        assert!(detect_non_standard_python_indentation(script).is_some());
    }

    #[test]
    fn detects_unbalanced_python_brackets() {
        let script = "x = [1, 2, 3";
        assert!(has_unbalanced_python_brackets(script));
    }

    #[test]
    fn detects_oversized_inline_printf_script() {
        let body = "x".repeat(500);
        let line = format!("printf '%s\\n' '{body}' | python -");
        assert!(detect_large_printf_script(&line).is_some());
    }

    #[test]
    fn normalizes_fullwidth_sql_punctuation() {
        let sql = "SELECT a\u{FF0C}b FROM t WHERE id\u{FF08}1\u{FF09}\u{FF1B}";
        assert_eq!(
            normalize_sql_punctuation(sql),
            "SELECT a,b FROM t WHERE id(1);"
        );
    }

    #[test]
    fn dedents_global_python_indent_block() {
        let script = "    import os\n    if True:\n        print('ok')\n";
        let (rewritten, _) = dedent_common_leading_indent(script).expect("should dedent");
        assert_eq!(rewritten, "import os\nif True:\n    print('ok')\n");
    }

    #[test]
    fn normalizes_leading_tabs() {
        let script = "if True:\n\tprint('ok')\n";
        let (rewritten, touched) = normalize_leading_tabs(script).expect("should normalize tabs");
        assert_eq!(touched, 1);
        assert_eq!(rewritten, "if True:\n    print('ok')\n");
    }

    #[test]
    fn expands_two_space_indent_style() {
        let script = "if True:\n  x = 1\n  if x:\n    print(x)\n";
        let (rewritten, touched) =
            expand_two_space_indentation(script).expect("should expand two-space indent");
        assert!(touched >= 3);
        assert_eq!(
            rewritten,
            "if True:\n    x = 1\n    if x:\n        print(x)\n"
        );
    }
}
