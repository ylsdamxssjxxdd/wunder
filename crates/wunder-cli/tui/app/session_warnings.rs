//! Degraded signals the reader would otherwise lose when the transcript scrolls past them.
//!
//! The bar counts workarounds the app actually performed — an approval the user refused,
//! stream events that passed the retention window, tool arguments repaired before the call
//! went out. Nothing here manufactures a warning, and the panel never claims a list that
//! is empty.

use super::*;

/// Entries kept per thread view. Repetitions fold into a count instead of growing the list.
const MAX_SESSION_WARNINGS: usize = 24;
const WARNING_TEXT_MAX_CHARS: usize = 160;
/// Shrink order for the notice: hint sentence, then the key, then only the count.
const NOTICE_WITH_HINT_MIN_WIDTH: usize = 40;
const NOTICE_WITH_KEY_MIN_WIDTH: usize = 24;
/// Rows the panel shows before it says how many it left out.
const PANEL_MAX_ROWS: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SessionWarning {
    text: String,
    repeats: usize,
}

impl TuiApp {
    /// Record one workaround.
    pub(super) fn note_session_warning(&mut self, text: &str) {
        fold_session_warning(&mut self.session_warnings, text);
    }

    /// `(count part, hint part)` for the composer notice, already shrunk to `width`.
    pub fn warning_notice(&self, is_zh: bool, width: u16) -> Option<(String, String)> {
        warning_notice_text(
            session_warning_total(self.session_warnings.as_slice()),
            is_zh,
            width,
        )
    }

    pub(super) fn warning_panel_open(&self) -> bool {
        self.warning_panel_open && !self.session_warnings.is_empty()
    }

    pub(super) fn toggle_warning_panel(&mut self) {
        if self.session_warnings.is_empty() {
            self.warning_panel_open = false;
            return;
        }
        self.warning_panel_open = !self.warning_panel_open;
    }

    pub(super) fn close_warning_panel(&mut self) {
        self.warning_panel_open = false;
    }

    pub fn warning_panel_lines(&self, is_zh: bool) -> Option<Vec<String>> {
        if !self.warning_panel_open() {
            return None;
        }
        Some(warning_panel_lines_of(
            self.session_warnings.as_slice(),
            is_zh,
        ))
    }
}

/// Identical consecutive signals fold into a count, so a model that retries the same
/// repaired call fifty times still leaves one line to read.
fn fold_session_warning(warnings: &mut Vec<SessionWarning>, text: &str) {
    let cleaned = text.trim();
    if cleaned.is_empty() {
        return;
    }
    let (capped, _) = truncate_by_chars(cleaned, WARNING_TEXT_MAX_CHARS);
    if let Some(head) = warnings.first_mut() {
        if head.text == capped {
            head.repeats = head.repeats.saturating_add(1);
            return;
        }
    }
    warnings.insert(
        0,
        SessionWarning {
            text: capped,
            repeats: 1,
        },
    );
    while warnings.len() > MAX_SESSION_WARNINGS {
        warnings.pop();
    }
}

fn session_warning_total(warnings: &[SessionWarning]) -> usize {
    warnings.iter().map(|warning| warning.repeats).sum()
}

fn warning_notice_text(total: usize, is_zh: bool, width: u16) -> Option<(String, String)> {
    if total == 0 {
        return None;
    }
    let budget = usize::from(width);
    if budget >= NOTICE_WITH_HINT_MIN_WIDTH {
        let hint = if is_zh {
            " 条提醒 · f2 查看"
        } else {
            " warnings · f2 to view"
        };
        return Some((format!("⚠ {total}"), hint.to_string()));
    }
    if budget >= NOTICE_WITH_KEY_MIN_WIDTH {
        return Some((format!("⚠ {total}"), " · f2".to_string()));
    }
    Some((format!("⚠{total}"), String::new()))
}

fn warning_panel_lines_of(warnings: &[SessionWarning], is_zh: bool) -> Vec<String> {
    let total: usize = warnings.iter().map(|warning| warning.repeats).sum();
    let mut lines = vec![if is_zh {
        format!("⚠ {total} 条提醒")
    } else {
        format!("⚠ {total} warnings")
    }];
    let visible = warnings.len().min(PANEL_MAX_ROWS);
    lines.extend(
        warnings
            .iter()
            .take(visible)
            .enumerate()
            .map(|(index, warning)| {
                let marker = if warning.repeats > 1 {
                    format!(" ×{}", warning.repeats)
                } else {
                    String::new()
                };
                format!("{}. {}{marker}", index + 1, warning.text)
            }),
    );
    let hidden = warnings.len().saturating_sub(visible);
    if hidden > 0 {
        lines.push(if is_zh {
            format!("… 另有 {hidden} 条未显示")
        } else {
            format!("… +{hidden} not shown")
        });
    }
    lines.push(String::new());
    lines.push(
        if is_zh {
            "f2 或 esc 关闭"
        } else {
            "f2 or esc to close"
        }
        .to_string(),
    );
    lines
}

/// Tool arguments the runtime had to repair before the call went out. The card already shows
/// the note; the bar keeps it findable after the card scrolls away.
pub(super) fn repaired_arguments_note(tool: &str, payload: &Value, is_zh: bool) -> Option<String> {
    let result = payload.get("result").unwrap_or(payload);
    let repair = result
        .get("meta")
        .and_then(|value| value.get("repair"))
        .or_else(|| payload.get("repair"))?;
    let strategy = repair
        .get("strategy")
        .and_then(Value::as_str)
        .map(str::trim)?;
    if strategy.is_empty() {
        return None;
    }
    Some(if is_zh {
        format!("工具参数在调用前被修复：{tool}（{strategy}）")
    } else {
        format!("tool arguments repaired before the call: {tool} ({strategy})")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joined(total: usize, is_zh: bool, width: u16) -> String {
        let (count, hint) = warning_notice_text(total, is_zh, width).expect("notice");
        format!("{count}{hint}")
    }

    fn warning(text: &str, repeats: usize) -> SessionWarning {
        SessionWarning {
            text: text.to_string(),
            repeats,
        }
    }

    #[test]
    fn the_notice_shrinks_hint_then_key_then_count() {
        assert_eq!(warning_notice_text(0, false, 80), None, "no signal, no bar");
        assert_eq!(joined(1, false, 60), "⚠ 1 warnings · f2 to view");
        assert_eq!(joined(7, false, 40), "⚠ 7 warnings · f2 to view");
        assert_eq!(joined(7, false, 32), "⚠ 7 · f2");
        assert_eq!(joined(7, false, 24), "⚠ 7 · f2");
        assert_eq!(joined(7, false, 12), "⚠7");

        assert_eq!(joined(1, true, 60), "⚠ 1 条提醒 · f2 查看");
        assert_eq!(joined(1, true, 12), "⚠1");
        for width in [1u16, 4, 12, 24, 40, 80] {
            let notice = warning_notice_text(3, false, width).expect("notice");
            let text = format!("{}{}", notice.0, notice.1);
            assert!(
                UnicodeWidthStr::width(text.as_str()) <= usize::from(width).max(3),
                "notice {text:?} at {width} columns"
            );
        }
    }

    #[test]
    fn repeated_signals_fold_and_the_list_stays_bounded() {
        let mut warnings: Vec<SessionWarning> = Vec::new();
        for _ in 0..5 {
            fold_session_warning(&mut warnings, "arguments repaired before the call");
        }
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].repeats, 5);
        assert_eq!(session_warning_total(warnings.as_slice()), 5);

        fold_session_warning(&mut warnings, "  ");
        assert_eq!(warnings.len(), 1, "an empty signal is not a warning");

        for index in 0..(MAX_SESSION_WARNINGS + 8) {
            fold_session_warning(&mut warnings, format!("retention notice {index}").as_str());
        }
        assert_eq!(warnings.len(), MAX_SESSION_WARNINGS);
        assert_eq!(
            warnings[0].text,
            format!("retention notice {}", MAX_SESSION_WARNINGS + 7)
        );

        let long = "x".repeat(WARNING_TEXT_MAX_CHARS + 200);
        fold_session_warning(&mut warnings, long.as_str());
        assert!(
            warnings[0].text.chars().count() <= WARNING_TEXT_MAX_CHARS + 1,
            "the stored text stays bounded"
        );
    }

    #[test]
    fn the_panel_names_the_count_and_how_much_it_left_out() {
        let lines =
            warning_panel_lines_of(&[warning("approval denied: write src/demo.rs", 3)], false);
        assert_eq!(lines[0], "⚠ 3 warnings");
        assert_eq!(lines[1], "1. approval denied: write src/demo.rs ×3");
        assert_eq!(lines.last().map(String::as_str), Some("f2 or esc to close"));

        let many = (0..MAX_SESSION_WARNINGS)
            .map(|index| warning(format!("notice {index}").as_str(), 1))
            .collect::<Vec<_>>();
        let lines = warning_panel_lines_of(many.as_slice(), true);
        assert_eq!(lines.len(), 1 + PANEL_MAX_ROWS + 1 + 2);
        assert_eq!(lines[0], "⚠ 24 条提醒");
        assert!(
            lines[1 + PANEL_MAX_ROWS].contains("另有 12 条未显示"),
            "{lines:?}"
        );

        let zh = warning_panel_lines_of(&[warning("参数被修复", 1)], true);
        assert_eq!(zh.last().map(String::as_str), Some("f2 或 esc 关闭"));
    }
}
