use crate::args::GlobalArgs;

pub fn resolve_cli_language(global: &GlobalArgs) -> String {
    let mut candidates = Vec::new();
    if let Some(language) = global
        .language
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        candidates.push(language.to_string());
    }
    wunder_server::i18n::resolve_language(candidates.iter().map(String::as_str))
}

pub fn is_zh_language(language: &str) -> bool {
    language.trim().to_ascii_lowercase().starts_with("zh")
}

pub fn tr(language: &str, zh: &str, en: &str) -> String {
    if is_zh_language(language) {
        zh.to_string()
    } else {
        en.to_string()
    }
}

/// Every copy key the progress and tool-result renderers can ask for. Both the
/// headless renderer (`render.rs`) and the TUI (`tui/app/helpers.rs`) read their
/// wording from this table, so a sentence is spelled once for both languages and a
/// display language can never pick up the other language's fixed string.
pub const UI_TEXT_KEYS: &[&str] = &[
    "patch_apply_failed",
    "changed_files_one",
    "changed_files_count",
    "patch_edited",
    "tool_failed",
    "tool_completed",
    "tool_only_summary",
    "gap_follow_full",
    "gap_follow_count",
    "gap_follow_back",
    "gap_follow_min",
    "gap_copied",
    "gap_copied_short",
    "gap_copied_min",
    "search_query",
    "search_count",
];

/// The Chinese and English form of one copy key. Templates may carry `{name}`
/// placeholders, which `ui_text` fills; both forms must carry the same set.
pub fn ui_pair(key: &str) -> Option<(&'static str, &'static str)> {
    match key {
        "patch_apply_failed" => Some(("✘ 补丁应用失败", "✘ Failed to apply patch")),
        "changed_files_one" => Some(("1 个文件", "1 file")),
        "changed_files_count" => Some(("{count} 个文件", "{count} files")),
        "patch_edited" => Some((
            "• 已修改 {files}{metrics}",
            "• Edited {files}{metrics}",
        )),
        "tool_failed" => Some(("✘ {tool} 失败", "✘ {tool} failed")),
        "tool_completed" => Some(("• 已完成 {tool}", "• Completed {tool}")),
        "tool_only_summary" => Some((
            "已完成本轮任务，结果见上方工具输出。",
            "Done. Review the tool results above.",
        )),
        // The composer gap: one row between the transcript and the draft carrying the
        // return-to-latest control and the copy feedback. Every rung is one display row,
        // so the ladder sheds words before the band ever asks for a second row.
        "gap_follow_full" => Some((
            "↓ {count} 条新内容 (End)",
            "↓ {count} new (End)",
        )),
        "gap_follow_count" => Some(("↓ {count} 条新内容", "↓ {count} new")),
        "gap_follow_back" => Some(("↓ 回到最新 (End)", "↓ back to latest (End)")),
        "gap_follow_min" => Some(("↓ 回最新", "↓ latest")),
        "gap_copied" => Some(("已复制 {count} 个字符", "Copied {count} characters")),
        "gap_copied_short" => Some(("已复制 {count} 字", "Copied {count} chars")),
        "gap_copied_min" => Some(("已复制", "Copied")),
        // Transcript search, in the same gap row: the query the reader is typing and where
        // they stand in the hits. The second rung is what survives a narrow frame, so the
        // counts alone still read as a search state.
        "search_query" => Some((
            "⌕ 查找 {query} {index}/{total}",
            "⌕ {query} {index}/{total}",
        )),
        "search_count" => Some(("⌕ 命中 {index}/{total}", "⌕ {index}/{total}")),
        _ => None,
    }
}

/// Localized copy for a renderer key with `{name}` placeholders substituted.
pub fn ui_text(is_zh: bool, key: &str, args: &[(&str, &str)]) -> String {
    let (zh, en) = ui_pair(key).unwrap_or(("", ""));
    let mut text = if is_zh { zh } else { en }.to_string();
    for (name, value) in args {
        text = text.replace(&format!("{{{name}}}"), value);
    }
    text
}

#[cfg(test)]
mod ui_text_table_tests {
    use super::*;

    fn has_han(text: &str) -> bool {
        text.chars()
            .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))
    }

    fn placeholders(text: &str) -> Vec<String> {
        let mut found = Vec::new();
        let mut rest = text;
        while let Some(start) = rest.find('{') {
            let tail = &rest[start + 1..];
            match tail.find('}') {
                Some(end) => {
                    found.push(tail[..end].to_string());
                    rest = &tail[end + 1..];
                }
                None => break,
            }
        }
        found.sort();
        found
    }

    #[test]
    fn every_copy_key_carries_one_form_per_language() {
        for key in UI_TEXT_KEYS {
            let (zh, en) = ui_pair(key).unwrap_or_else(|| panic!("unknown copy key: {key}"));
            assert!(has_han(zh), "{key} has no Chinese wording: {zh}");
            assert!(!has_han(en), "{key} carries Chinese into the English form: {en}");
            assert!(
                en.chars().any(|ch| ch.is_ascii_alphabetic()),
                "{key} English form is not wording: {en}"
            );
            assert_eq!(
                placeholders(zh),
                placeholders(en),
                "{key} placeholder mismatch between languages"
            );
        }
    }

    #[test]
    fn every_copy_key_is_reachable_from_the_table() {
        for key in UI_TEXT_KEYS {
            assert!(ui_pair(key).is_some(), "{key} missing from the table");
        }
        assert!(ui_pair("not_a_copy_key").is_none());
    }

    #[test]
    fn placeholders_are_substituted_in_both_languages() {
        let zh = ui_text(true, "tool_completed", &[("tool", "sample_tool")]);
        let en = ui_text(false, "tool_completed", &[("tool", "sample_tool")]);
        assert!(zh.contains("sample_tool"), "{zh}");
        assert!(en.contains("sample_tool"), "{en}");
        assert_ne!(zh, en, "the two languages must not collapse to one string");

        let zh_count = ui_text(true, "changed_files_count", &[("count", "3")]);
        let en_count = ui_text(false, "changed_files_count", &[("count", "3")]);
        assert!(zh_count.contains('3'), "{zh_count}");
        assert!(en_count.contains('3'), "{en_count}");
    }
}
