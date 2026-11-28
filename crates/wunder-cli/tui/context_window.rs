//! Context window composition projection.
//!
//! Pure functions over the engine snapshot: no terminal types, no IO. The TUI
//! panel and the non-interactive text block both render from the same output,
//! so the three shells cannot disagree on wording, order or numbers.

use wunder_server::context_window::{ContextItem, ContextWindowSnapshot};

/// Fixed composition slots, in the desktop order: system, tools, images,
/// results, chat. The engine snapshot only carries non-empty parts; the legend
/// below is always padded to these five, missing parts read 0 / 0%.
pub(crate) const SLOT_KINDS: [(&str, i32); 5] = [
    ("system", 1),
    ("tools", 2),
    ("files", 3),
    ("results", 4),
    ("chat", 5),
];

/// What the projection can honestly show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ContextState {
    /// The thread has never sent a request: window capacity only, no composition.
    Empty,
    /// Compacted and no provider measurement has arrived since. Identity and
    /// the window stay visible; every token reading is suppressed.
    UnobservedAfterCompaction,
    /// A recorded request drives every reading below.
    Observed,
}

#[derive(Clone, Debug)]
pub(crate) struct PartRow {
    pub slot: i32,
    pub kind: String,
    pub label: String,
    pub tokens_text: String,
    pub pct_text: String,
}

#[derive(Clone, Debug)]
pub(crate) struct ItemRow {
    pub slot: i32,
    pub tag: String,
    pub name: String,
    pub count: i64,
    pub order: i64,
    pub label: String,
    pub tokens_text: String,
}

#[derive(Clone, Debug)]
pub(crate) struct ContextWindowView {
    pub state: ContextState,
    pub model: String,
    pub provider: String,
    pub window: i64,
    pub used: i64,
    pub measured: bool,
    pub turns: i64,
    pub parts: Vec<PartRow>,
    pub items: Vec<ItemRow>,
}

/// Whole-snapshot projection.
///
/// `capacity` comes from the model/vendor capacity lookup; it fills in when the
/// recorded snapshot did not carry a window. `unobserved_after_compaction` is the
/// shell-side flag: true between a compaction and the next observed usage.
pub(crate) fn project(
    snapshot: Option<ContextWindowSnapshot>,
    capacity: i64,
    unobserved_after_compaction: bool,
    is_zh: bool,
) -> ContextWindowView {
    let Some(recorded) = snapshot else {
        return ContextWindowView {
            state: ContextState::Empty,
            model: String::new(),
            provider: String::new(),
            window: capacity.max(0),
            used: 0,
            measured: false,
            turns: 0,
            parts: Vec::new(),
            items: Vec::new(),
        };
    };

    let window = if recorded.window > 0 {
        recorded.window
    } else {
        capacity.max(0)
    };
    let empty = recorded.parts.is_empty();
    let state = if empty {
        ContextState::Empty
    } else if unobserved_after_compaction {
        ContextState::UnobservedAfterCompaction
    } else {
        ContextState::Observed
    };

    match state {
        ContextState::Empty => ContextWindowView {
            state,
            model: recorded.model,
            provider: recorded.provider,
            window,
            used: 0,
            measured: false,
            turns: 0,
            parts: Vec::new(),
            items: Vec::new(),
        },
        ContextState::UnobservedAfterCompaction => ContextWindowView {
            state,
            model: recorded.model,
            provider: recorded.provider,
            window,
            used: 0,
            measured: false,
            turns: 0,
            parts: Vec::new(),
            items: Vec::new(),
        },
        ContextState::Observed => {
            let parts = SLOT_KINDS
                .iter()
                .map(|(kind, slot)| {
                    let tokens = recorded
                        .parts
                        .iter()
                        .find(|part| part.kind == *kind)
                        .map(|part| part.tokens)
                        .unwrap_or(0);
                    PartRow {
                        slot: *slot,
                        kind: (*kind).to_string(),
                        label: part_label(kind, is_zh).to_string(),
                        tokens_text: fmt_k(tokens),
                        pct_text: pct(tokens, recorded.tokens),
                    }
                })
                .collect();
            let mut flattened: Vec<(i32, ContextItem)> = Vec::new();
            for part in &recorded.parts {
                let slot = SLOT_KINDS
                    .iter()
                    .find(|(kind, _)| *kind == part.kind)
                    .map(|(_, slot)| *slot);
                if let Some(slot) = slot {
                    for item in &part.items {
                        flattened.push((slot, item.clone()));
                    }
                }
            }
            let items = order_items(&flattened, is_zh);
            ContextWindowView {
                state,
                model: recorded.model,
                provider: recorded.provider,
                window,
                used: recorded.tokens,
                measured: recorded.measured,
                turns: recorded.turns,
                parts,
                items,
            }
        }
    }
}

/// Head slots (system/tools/images) keep part order; talk slots (results/chat)
/// interleave by `order`. When no item carries an order, results follow turns,
/// the desktop fallback.
fn order_items(items: &[(i32, ContextItem)], is_zh: bool) -> Vec<ItemRow> {
    let talk_slots = [4, 5];
    let any_order = items.iter().any(|(_, item)| item.order > 0);
    let mut head: Vec<ItemRow> = Vec::new();
    let mut talk: Vec<ItemRow> = Vec::new();
    for (slot, item) in items {
        let row = ItemRow {
            slot: *slot,
            tag: item.tag.clone(),
            name: item.name.clone(),
            count: item.count,
            order: item.order,
            label: item_label(&item.tag, &item.name, item.count, is_zh),
            tokens_text: fmt_k(item.tokens),
        };
        if talk_slots.contains(slot) {
            talk.push(row);
        } else {
            head.push(row);
        }
    }
    if any_order {
        talk.sort_by_key(|row| row.order);
    } else {
        talk.sort_by_key(|row| match row.tag.as_str() {
            "result" => 1,
            _ => 0,
        });
    }
    head.extend(talk);
    head
}

fn pct(part: i64, total: i64) -> String {
    if total <= 0 {
        return "0%".to_string();
    }
    let share = (part.max(0) as f64 * 100.0 / total as f64).round();
    format!("{share:.0}%")
}

/// 12 345 → 12.3K; 1 234 000 → 1.2M; smaller values stay raw.
pub(crate) fn fmt_k(value: i64) -> String {
    let abs = value.unsigned_abs();
    if abs >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    } else if abs >= 1_000 {
        format!("{:.1}K", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

pub(crate) fn part_label(kind: &str, is_zh: bool) -> &'static str {
    match kind {
        "system" => {
            if is_zh {
                "系统"
            } else {
                "System"
            }
        }
        "tools" => {
            if is_zh {
                "工具"
            } else {
                "Tools"
            }
        }
        "files" => {
            if is_zh {
                "图片"
            } else {
                "Images"
            }
        }
        "results" => {
            if is_zh {
                "结果"
            } else {
                "Results"
            }
        }
        "chat" => {
            if is_zh {
                "会话"
            } else {
                "Chat"
            }
        }
        _ => {
            if is_zh {
                "内容"
            } else {
                "Content"
            }
        }
    }
}

/// Row wording, aligned with the desktop `ContextText.item(tag, name, count)`
/// mapping. `count` is the turn ordinal for `turn`, the result count for
/// `result`, the image count for `image`; `turn`'s name is always empty.
pub(crate) fn item_label(tag: &str, name: &str, count: i64, is_zh: bool) -> String {
    match tag {
        "tool" if !name.is_empty() => format!(
            "{name} · {}",
            if is_zh {
                "工具定义"
            } else {
                "Tool definition"
            }
        ),
        "result" if !name.is_empty() => {
            format!("{name} · {}", if is_zh { "结果" } else { "Result" })
        }
        "turn" => {
            if is_zh {
                format!("第 {count} 轮")
            } else {
                format!("Turn {count}")
            }
        }
        "image" => {
            if is_zh {
                format!("图片 ×{count}")
            } else {
                format!("Images ×{count}")
            }
        }
        _ if !name.is_empty() => name.to_string(),
        "prompt" => (if is_zh { "提示词" } else { "Prompt" }).to_string(),
        "reminder" => (if is_zh { "提醒" } else { "Reminders" }).to_string(),
        _ => (if is_zh { "内容" } else { "Content" }).to_string(),
    }
}

pub(crate) fn title(is_zh: bool) -> &'static str {
    if is_zh {
        "脑图"
    } else {
        "Mind map"
    }
}

pub(crate) fn no_request_notice(is_zh: bool) -> &'static str {
    if is_zh {
        "本线程还没有发出请求；第一次发送后，这里显示它实际占用的构成。"
    } else {
        "This thread hasn't sent a request yet. After the first send this shows what it was made of."
    }
}

pub(crate) fn unobserved_notice(is_zh: bool) -> &'static str {
    if is_zh {
        "压缩后尚未收到新的实测用量；下一次请求后显示构成。"
    } else {
        "No observed usage since compaction; the composition appears after the next request."
    }
}

pub(crate) fn released_text(is_zh: bool) -> &'static str {
    if is_zh {
        "正文只保留在当前进程内，重启或释放后不可查看。"
    } else {
        "Text lives only in this process; it is gone after a restart or release."
    }
}

/// Plain-text rendering for non-TTY / pipe / `--no-tui` use. No ANSI escapes,
/// no decoration beyond the readable lines.
pub(crate) fn plain_text(view: &ContextWindowView, is_zh: bool) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push(title(is_zh).to_string());

    let model_row = model_row(&view.model, &view.provider);
    if !model_row.is_empty() {
        lines.push(model_row);
    }

    match view.state {
        ContextState::Empty => {
            if view.window > 0 {
                lines.push(window_line(view.window, is_zh));
            }
            lines.push(no_request_notice(is_zh).to_string());
        }
        ContextState::UnobservedAfterCompaction => {
            if view.window > 0 {
                lines.push(window_line(view.window, is_zh));
            }
            lines.push(unobserved_notice(is_zh).to_string());
        }
        ContextState::Observed => {
            lines.push(summary_line(view.used, view.window, view.measured, is_zh));
            if view.turns > 0 {
                lines.push(turns_line(view.turns, is_zh));
            }
            lines.push(String::new());
            for part in &view.parts {
                lines.push(format!(
                    "  {}  {}  {}",
                    part.label, part.tokens_text, part.pct_text
                ));
            }
            lines.push(String::new());
            for item in &view.items {
                lines.push(format!("  {}  {}", item.label, item.tokens_text));
            }
        }
    }
    lines.join("\n")
}

pub(crate) fn model_row(model: &str, provider: &str) -> String {
    match (model.is_empty(), provider.is_empty()) {
        (false, false) => format!("{provider} › {model}"),
        (false, true) => model.to_string(),
        (true, false) => provider.to_string(),
        (true, true) => String::new(),
    }
}

pub(crate) fn summary_line(used: i64, window: i64, measured: bool, is_zh: bool) -> String {
    let mark = if measured {
        if is_zh {
            "已计量"
        } else {
            "Measured"
        }
    } else {
        if is_zh {
            "估算"
        } else {
            "Estimated"
        }
    };
    if window > 0 {
        let pct = pct(used, window);
        if is_zh {
            format!("已用 {} / {window} · {pct} · {mark}", fmt_k(used))
        } else {
            format!("{} / {window} used · {pct} · {mark}", fmt_k(used))
        }
    } else if is_zh {
        format!("已用 {} · {mark}", fmt_k(used))
    } else {
        format!("{} used · {mark}", fmt_k(used))
    }
}

pub(crate) fn window_line(window: i64, is_zh: bool) -> String {
    if is_zh {
        format!("窗口 {}", fmt_k(window))
    } else {
        format!("Window {}", fmt_k(window))
    }
}

pub(crate) fn turns_line(turns: i64, is_zh: bool) -> String {
    if is_zh {
        format!("承载 {turns} 轮对话")
    } else {
        format!("{turns} turns carried")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wunder_server::context_window::{ContextItem, ContextPart};

    fn part(kind: &str, tokens: i64, items: Vec<ContextItem>) -> ContextPart {
        ContextPart {
            kind: kind.into(),
            tokens,
            items,
        }
    }

    fn item(tag: &str, name: &str, count: i64, tokens: i64, order: i64) -> ContextItem {
        ContextItem {
            tag: tag.into(),
            name: name.into(),
            count,
            tokens,
            order,
            ..Default::default()
        }
    }

    fn snapshot(parts: Vec<ContextPart>) -> ContextWindowSnapshot {
        ContextWindowSnapshot {
            model: "gpt-test".to_string(),
            provider: "vendor".to_string(),
            window: 128_000,
            tokens: 8_300,
            measured: true,
            turns: 2,
            parts,
        }
    }

    #[test]
    fn fmt_k_cores() {
        assert_eq!(fmt_k(0), "0");
        assert_eq!(fmt_k(999), "999");
        assert_eq!(fmt_k(12_300), "12.3K");
        assert_eq!(fmt_k(1_234_000), "1.2M");
    }

    #[test]
    fn empty_thread_shows_window_only_without_fake_parts() {
        let view = project(None, 128_000, false, true);
        assert_eq!(view.state, ContextState::Empty);
        assert_eq!(view.window, 128_000);
        assert!(view.parts.is_empty());
        assert!(view.items.is_empty());
        assert!(plain_text(&view, true).contains("本线程还没有发出请求"));
    }

    #[test]
    fn observed_thread_pads_the_five_part_legend() {
        let recorded = snapshot(vec![
            part("system", 3_100, vec![]),
            part("tools", 4_400, vec![]),
            part("chat", 748, vec![]),
        ]);
        let view = project(Some(recorded), 0, false, true);
        assert_eq!(view.state, ContextState::Observed);
        assert_eq!(view.parts.len(), 5);
        let labels: Vec<String> = view.parts.iter().map(|part| part.label.clone()).collect();
        assert_eq!(labels, ["系统", "工具", "图片", "结果", "会话"]);
        let missing = view.parts.iter().find(|part| part.kind == "files").unwrap();
        assert_eq!(missing.tokens_text, "0");
        assert_eq!(missing.pct_text, "0%");
    }

    #[test]
    fn unobserved_after_compaction_hides_every_number() {
        let recorded = snapshot(vec![part("system", 3_100, vec![])]);
        let view = project(Some(recorded), 128_000, true, true);
        assert_eq!(view.state, ContextState::UnobservedAfterCompaction);
        assert_eq!(view.used, 0);
        assert_eq!(view.turns, 0);
        assert!(view.parts.is_empty());
        assert!(view.items.is_empty());
        let text = plain_text(&view, true);
        assert!(text.contains("压缩后尚未收到"));
        assert!(!text.contains("3.1K"));
    }

    #[test]
    fn talk_rows_interleave_by_order_with_results_after_turns_on_zero_order() {
        let recorded = snapshot(vec![
            part(
                "chat",
                260,
                vec![item("turn", "", 1, 120, 1), item("turn", "", 2, 140, 3)],
            ),
            part("results", 60, vec![item("result", "read_file", 1, 60, 2)]),
        ]);
        let view = project(Some(recorded), 0, false, true);
        let orders: Vec<i64> = view.items.iter().map(|item| item.order).collect();
        assert_eq!(orders, [1, 2, 3]);

        let fallback = snapshot(vec![
            part("chat", 120, vec![item("turn", "", 1, 120, 0)]),
            part("results", 60, vec![item("result", "read_file", 1, 60, 0)]),
        ]);
        let view = project(Some(fallback), 0, false, true);
        let tags: Vec<&str> = view.items.iter().map(|item| item.tag.as_str()).collect();
        assert_eq!(tags, ["turn", "result"]);
    }

    #[test]
    fn item_labels_follow_the_desktop_mapping() {
        assert_eq!(
            item_label("tool", "read_file", 1, true),
            "read_file · 工具定义"
        );
        assert_eq!(
            item_label("result", "read_file", 3, true),
            "read_file · 结果"
        );
        assert_eq!(item_label("turn", "", 2, true), "第 2 轮");
        assert_eq!(item_label("image", "", 4, true), "图片 ×4");
        assert_eq!(item_label("prompt", "", 0, true), "提示词");
        assert_eq!(item_label("reminder", "", 0, true), "提醒");
        assert_eq!(item_label("turn", "", 2, false), "Turn 2");
        assert_eq!(item_label("image", "", 4, false), "Images ×4");
    }
}
