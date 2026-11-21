//! Resume-action detection for context compaction.
//!
//! The compaction prompt asks the summarizer for one explicit
//! `resume_action: final|continue|retry|ask_user` line. Models do not always
//! emit it, so the runtime has to infer the action from the summary text. A
//! wrong guess here is expensive: `final` stops the turn and forbids tools, so
//! an unfinished task gets reported as delivered (see
//! `docs/功能迭代.md` / `docs/经验教训.md`). The inference below therefore
//! follows two rules:
//!
//! 1. Never read a completion signal out of a heading or a negated sentence.
//!    `## What remains to be done` is a *pending* heading, not a "done" one.
//! 2. When the evidence is ambiguous, continue. Only an explicit, empty
//!    remaining-work section may end the turn.

use super::strip_compaction_prefix_text;
use super::CompactionResumeAction;

/// Words that read as "the task is finished" when they are not negated.
const COMPLETION_WORDS: &[&str] = &["done", "complete", "completed", "finished"];
/// Tokens that flip a nearby completion word into "not done yet".
const COMPLETION_NEGATIONS: &[&str] = &[
    "not",
    "no",
    "never",
    "none",
    "yet",
    "still",
    "remain",
    "remains",
    "remaining",
    "unfinished",
    "incomplete",
    "pending",
    "todo",
    "todos",
    "待",
];
/// Explicit "nothing left to do" signals.
const NO_REMAINING_SIGNALS: &[&str] = &[
    "no remaining",
    "no work remains",
    "nothing remains",
    "nothing left to do",
    "nothing left",
    "no explicit todo",
    "no pending",
    "no further",
    "no outstanding",
    "all done",
    "task is complete",
    "task complete",
];
const NO_REMAINING_SIGNALS_CJK: &[&str] = &[
    "无明确待办",
    "没有明确待办",
    "无需继续",
    "没有剩余待办",
    "无剩余待办",
    "无剩余工作",
    "没有剩余工作",
    "已全部完成",
];
/// Signals that concrete work is still pending.
const PENDING_SIGNALS: &[&str] = &[
    "to be done",
    "remains to",
    "remaining",
    "remains",
    "still to",
    "still need",
    "not yet",
    "next step",
    "next steps",
    "next action",
    "in progress",
    "follow-up",
    "follow up",
    "unfinished",
];
const PENDING_WORDS: &[&str] = &["pending", "todo", "todos", "continue", "continues"];
const PENDING_SIGNALS_CJK: &[&str] = &[
    "剩余",
    "待办",
    "待执行",
    "待完成",
    "待验证",
    "待复核",
    "尚未",
    "还未",
    "还没",
    "未完成",
    "未落盘",
    "未生成",
    "未渲染",
    "未交付",
    "未验证",
    "下一步",
    "还要",
    "仍需",
    "尚需",
    "继续",
];
const FAILURE_SIGNALS: &[&str] = &["failed", "failure", "error", "retry", "repair", "blocked"];
const FAILURE_SIGNALS_CJK: &[&str] = &["失败", "错误", "修复", "重试", "报错", "阻塞"];
/// CJK completion markers; the char in front decides whether they are negated.
const COMPLETION_SIGNALS_CJK: &[&str] = &[
    "已完成",
    "已经完成",
    "已生成",
    "已经生成",
    "已绘制",
    "已渲染",
    "已交付",
    "已出片",
];
const COMPLETION_NEGATION_CHARS: &[char] = &['未', '没', '尚', '还', '非', '不', '无'];

/// Headings that introduce the remaining-work section.
const REMAINING_HEADINGS: &[&str] = &[
    "what remains",
    "what's left",
    "what is left",
    "remaining work",
    "remaining tasks",
    "remaining todo",
    "outstanding work",
    "next steps",
    "todo",
    "剩余待办",
    "剩余工作",
    "剩余任务",
    "待办",
    "下一步",
];

/// A section that only says "nothing" still counts as empty.
const EMPTY_SECTION_PLACEHOLDERS: &[&str] = &[
    "none",
    "nothing",
    "n/a",
    "na",
    "empty",
    "no",
    "无",
    "无明确待办",
    "没有待办",
    "无剩余工作",
    "没有剩余工作",
    "无需继续",
    "none.",
    "nothing.",
];

/// Words immediately before a completion word that void it.
fn preceded_by_negation(words: &[&str]) -> bool {
    words
        .iter()
        .any(|word| COMPLETION_NEGATIONS.iter().any(|negation| word == negation))
}

/// Split lowercase ASCII words with their byte offsets.
fn word_spans(text: &str) -> Vec<(usize, &str)> {
    let mut spans = Vec::new();
    let mut start: Option<usize> = None;
    for (index, ch) in text.char_indices() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            if start.is_none() {
                start = Some(index);
            }
        } else if let Some(begin) = start.take() {
            spans.push((begin, &text[begin..index]));
        }
    }
    if let Some(begin) = start {
        spans.push((begin, &text[begin..]));
    }
    spans
}

fn contains_word(text_lower: &str, needle: &str) -> bool {
    word_spans(text_lower)
        .iter()
        .any(|(_, word)| *word == needle)
}

fn contains_any(text_lower: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text_lower.contains(needle))
}

fn contains_any_exact(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

pub(crate) fn summary_has_failure_signal(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    contains_any(&lower, FAILURE_SIGNALS) || contains_any_exact(text, FAILURE_SIGNALS_CJK)
}

pub(crate) fn summary_has_no_remaining_work_signal(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    contains_any(&lower, NO_REMAINING_SIGNALS) || contains_any_exact(text, NO_REMAINING_SIGNALS_CJK)
}

/// Drops heading lines: `## What remains to be done` is a label for the section
/// below it, not a statement that work is pending.
fn without_heading_lines(text: &str) -> String {
    text.lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn summary_has_pending_work_signal(text: &str) -> bool {
    let body = without_heading_lines(text);
    let lower = body.to_ascii_lowercase();
    if contains_any(&lower, PENDING_SIGNALS)
        || PENDING_WORDS.iter().any(|word| contains_word(&lower, word))
    {
        return true;
    }
    contains_any_exact(&body, PENDING_SIGNALS_CJK)
}

/// True only when the summary claims completion *without* a nearby negation.
/// `## What remains to be done`, `not done`, `still pending` and
/// `尚未完成` must not count.
pub(crate) fn summary_has_completed_signal(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let spans = word_spans(&lower);
    for (index, (begin, word)) in spans.iter().enumerate() {
        if !COMPLETION_WORDS.contains(word) {
            continue;
        }
        let start = index.saturating_sub(5);
        let preceding = spans[start..index]
            .iter()
            .map(|(_, word)| *word)
            .collect::<Vec<_>>();
        if preceded_by_negation(&preceding) {
            continue;
        }
        // "to be done", "remains to be done": the bigram is the giveaway even
        // when one of the two words is outside the negation list.
        if preceding.ends_with(&["to", "be"]) {
            continue;
        }
        // Ignore the word when it sits inside a heading line.
        let line_start = lower[..*begin].rfind('\n').map_or(0, |pos| pos + 1);
        if lower[line_start..*begin].trim_start().starts_with('#') {
            continue;
        }
        return true;
    }
    for signal in COMPLETION_SIGNALS_CJK {
        let mut search_from = 0;
        while let Some(offset) = text[search_from..].find(signal) {
            let position = search_from + offset;
            let negated = text[..position]
                .chars()
                .next_back()
                .is_some_and(|previous| COMPLETION_NEGATION_CHARS.contains(&previous));
            if !negated {
                return true;
            }
            search_from = position + signal.len();
        }
    }
    false
}

/// Body of the remaining-work section, if the summary has one.
fn remaining_work_section(text: &str) -> Option<Vec<String>> {
    let mut collecting = false;
    let mut lines: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        let is_heading = trimmed.starts_with('#');
        if is_heading {
            if collecting {
                break;
            }
            let title = trimmed.trim_start_matches('#').trim();
            let title_lower = title.to_ascii_lowercase();
            let matches_heading = REMAINING_HEADINGS
                .iter()
                .any(|heading| title_lower.contains(heading) || title.contains(heading));
            if matches_heading {
                collecting = true;
            }
            continue;
        }
        if !collecting {
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.to_ascii_lowercase().starts_with("resume_action") {
            continue;
        }
        lines.push(trimmed.to_string());
    }
    if collecting {
        Some(lines)
    } else {
        None
    }
}

fn is_empty_section_line(line: &str) -> bool {
    let stripped = line
        .trim()
        .trim_start_matches(|ch: char| matches!(ch, '-' | '*' | '+' | '•'))
        .trim()
        .trim_matches(|ch: char| matches!(ch, '.' | '。' | ':' | '：' | ';' | '；' | ',' | '，'))
        .trim();
    if stripped.is_empty() {
        return true;
    }
    let lower = stripped.to_ascii_lowercase();
    EMPTY_SECTION_PLACEHOLDERS
        .iter()
        .any(|placeholder| lower == *placeholder || stripped == *placeholder)
}

/// `Some(false)` when the summary carries a remaining-work section that lists
/// real steps; `Some(true)` when that section exists but is empty; `None` when
/// there is no such section at all.
pub(crate) fn summary_remaining_work_is_empty(text: &str) -> Option<bool> {
    let lines = remaining_work_section(text)?;
    Some(lines.iter().all(|line| is_empty_section_line(line)))
}

/// A summary that still lists concrete remaining steps must never finalize the
/// turn, whatever the model claimed in `resume_action`.
fn downgrade_final_with_pending_work(
    action: CompactionResumeAction,
    summary_text: &str,
) -> CompactionResumeAction {
    if action == CompactionResumeAction::Final
        && summary_remaining_work_is_empty(summary_text) == Some(false)
    {
        return CompactionResumeAction::Continue;
    }
    action
}

pub(crate) fn infer_compaction_resume_action_from_sections(
    summary_text: &str,
) -> CompactionResumeAction {
    let body = summary_text.trim();
    if body.is_empty() {
        return CompactionResumeAction::Unknown;
    }
    let action = if summary_has_failure_signal(body) {
        CompactionResumeAction::Retry
    } else if let Some(remaining_is_empty) = summary_remaining_work_is_empty(body) {
        // An explicit remaining-work section is the strongest evidence either
        // way: listed steps keep the turn running, an empty one concludes it.
        if remaining_is_empty {
            CompactionResumeAction::Final
        } else {
            CompactionResumeAction::Continue
        }
    } else if summary_has_no_remaining_work_signal(body) {
        CompactionResumeAction::Final
    } else if summary_has_pending_work_signal(body) {
        CompactionResumeAction::Continue
    } else if summary_has_completed_signal(body) {
        CompactionResumeAction::Final
    } else {
        // No evidence that the task is finished: keep the turn alive instead of
        // turning an unfinished task into a "final" answer.
        CompactionResumeAction::Continue
    };
    downgrade_final_with_pending_work(action, body)
}

pub(crate) fn parse_compaction_resume_action_value(value: &str) -> CompactionResumeAction {
    let raw = value
        .trim()
        .trim_matches(['`', '"', '\'', '*', ' ', '-', ':', '：']);
    let direct = raw.trim();
    match direct {
        "最终回复" | "结束" | "完成" => return CompactionResumeAction::Final,
        "继续" => return CompactionResumeAction::Continue,
        "重试" | "修复" => return CompactionResumeAction::Retry,
        "询问用户" | "澄清" => return CompactionResumeAction::AskUser,
        _ => {}
    }
    // "final|continue|retry|ask_user" is the prompt template echoed back as-is.
    if direct.contains('|') {
        return CompactionResumeAction::Unknown;
    }
    let cleaned = raw.to_ascii_lowercase();
    let first = cleaned
        .split(|ch: char| ch.is_whitespace() || matches!(ch, ',' | ';' | '，' | '；'))
        .next()
        .unwrap_or_default()
        .trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_');
    match first {
        "final" | "finish" | "done" | "complete" | "completed" | "answer" => {
            CompactionResumeAction::Final
        }
        "continue" | "next" | "work" | "run" => CompactionResumeAction::Continue,
        "retry" | "repair" | "fix" => CompactionResumeAction::Retry,
        "ask_user" | "ask-user" | "ask" | "clarify" | "clarification" => {
            CompactionResumeAction::AskUser
        }
        _ => CompactionResumeAction::Unknown,
    }
}

pub(crate) fn detect_compaction_resume_action(summary_text: &str) -> CompactionResumeAction {
    let body = strip_compaction_prefix_text(summary_text);
    let normalized = body.replace("\r\n", "\n");
    for line in normalized.lines() {
        let trimmed = line.trim().trim_start_matches(['-', '*', ' ']).trim();
        let lower = trimmed.to_ascii_lowercase();
        if let Some(value) = lower
            .strip_prefix("resume_action:")
            .or_else(|| lower.strip_prefix("next_action:"))
            .or_else(|| lower.strip_prefix("resume action:"))
            .or_else(|| lower.strip_prefix("next action:"))
        {
            return downgrade_final_with_pending_work(
                parse_compaction_resume_action_value(value),
                &normalized,
            );
        }
        if let Some(value) = trimmed
            .strip_prefix("下一步动作：")
            .or_else(|| trimmed.strip_prefix("下一步动作:"))
            .or_else(|| trimmed.strip_prefix("续跑动作："))
            .or_else(|| trimmed.strip_prefix("续跑动作:"))
        {
            return downgrade_final_with_pending_work(
                parse_compaction_resume_action_value(value),
                &normalized,
            );
        }
    }

    infer_compaction_resume_action_from_sections(&normalized)
}
