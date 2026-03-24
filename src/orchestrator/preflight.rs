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
                        message: "Command preflight blocked: oversized inline printf script is brittle.".to_string(),
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
            if let Some(content) = extract_text_field(args, "content") {
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
                        message:
                            "Script preflight blocked: unmatched Python bracket detected."
                                .to_string(),
                        diagnostics: vec![PreflightDiagnostic::reject(
                            "python.bracket.unbalanced",
                            "Detected unmatched bracket/brace in script.".to_string(),
                            "Fix bracket pairing before execution.",
                        )],
                    };
                }
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

fn is_execute_command_tool_name(name: &str) -> bool {
    let cleaned = name.trim();
    cleaned == resolve_tool_name("execute_command") || cleaned.eq_ignore_ascii_case("execute_command")
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
    sql.replace('，', ",")
        .replace('；', ";")
        .replace('（', "(")
        .replace('）', ")")
}

#[cfg(test)]
mod tests {
    use super::{
        detect_bad_heredoc_line, detect_non_standard_python_indentation, has_unbalanced_python_brackets,
        normalize_sql_punctuation,
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
    fn normalizes_fullwidth_sql_punctuation() {
        let sql = "SELECT a，b FROM t WHERE id（1）";
        assert_eq!(
            normalize_sql_punctuation(sql),
            "SELECT a,b FROM t WHERE id(1)"
        );
    }
}
