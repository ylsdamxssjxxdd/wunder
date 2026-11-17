//! 编辑失败的「哪里不一样」：把 `no match` 换成本地可自愈的诊断。
//!
//! 源问题：`old_string` 匹配失败只回一句未匹配，模型只能靠 grep 重新找位置，
//! 或者整段重写。这里给出最相似的 3 个片段（带行区间）、相似度，以及第一候选
//! 的逐行差异（空白/引号/字符位），并顺势提示可以直接 `replace_range`。
//!
//! 代价上限：只对「首行像」的行做锚点，锚点最多 [`MAX_ANCHOR_CANDIDATES`] 个，
//! 每个候选只做一次字符二元组 Dice 相似度，整体近似 O(文件行数 × old_string 长度)。

use super::write_receipt::{char_bigrams, dice_similarity, split_lines};

/// 参与精确打分的锚点上限：先按首行相似度粗筛，再逐窗口算分。
const MAX_ANCHOR_CANDIDATES: usize = 48;
/// 首行相似度低于此值的行不作为锚点。
const ANCHOR_FLOOR: f32 = 0.45;
/// 片段预览最多显示的行数。
const SNIPPET_LINES: usize = 8;
/// 单个片段最多报告的差异条数。
const MAX_DIFFERENCES: usize = 4;
/// 差异描述里单侧上下文的字符数。
const DIFF_CONTEXT_CHARS: usize = 18;

/// 一个近似片段。
#[derive(Debug, Clone)]
pub(crate) struct Nearest {
    /// 1-based 起始行。
    pub line: usize,
    /// 1-based 结束行（含）。
    pub end_line: usize,
    /// 0-100 的相似度。
    pub similarity: u8,
    /// 带行号的片段预览。
    pub snippet: String,
    /// 逐行差异说明，例如「第 237 行：缩进/空白不同」。
    pub differences: Vec<String>,
}

/// 找出最相似的 `limit` 个片段，按相似度从高到低。
pub(crate) fn nearest_matches(content: &str, needle: &str, limit: usize) -> Vec<Nearest> {
    if limit == 0 {
        return Vec::new();
    }
    let needle_lines: Vec<&str> = split_lines(needle)
        .into_iter()
        .collect::<Vec<_>>();
    let needle_lines = trim_trailing_empty(needle_lines);
    if needle_lines.is_empty() {
        return Vec::new();
    }
    let file_lines = split_lines(content);
    if file_lines.is_empty() {
        return Vec::new();
    }
    let span = needle_lines.len();
    let needle_joined = join_normalized(&needle_lines);
    let needle_bigrams = char_bigrams(&needle_joined);

    let mut anchors: Vec<(usize, f32)> = Vec::new();
    let first_bigram = char_bigrams(&collapse(&needle_lines[0]));
    for (index, raw) in file_lines.iter().enumerate() {
        let line = collapse(raw);
        if line.is_empty() {
            continue;
        }
        let score = dice_similarity(&first_bigram, &char_bigrams(&line));
        if score >= ANCHOR_FLOOR {
            anchors.push((index, score));
        }
    }
    anchors.sort_by(|left, right| right.1.total_cmp(&left.1));
    anchors.truncate(MAX_ANCHOR_CANDIDATES);

    let mut scored: Vec<(usize, f32)> = Vec::new();
    for (start, _) in anchors {
        let end = (start + span).min(file_lines.len());
        let window = &file_lines[start..end];
        let joined = join_normalized(window);
        let score = dice_similarity(&needle_bigrams, &char_bigrams(&joined));
        scored.push((start, score));
    }
    scored.sort_by(|left, right| right.1.total_cmp(&left.1));

    let mut picked: Vec<(usize, f32)> = Vec::new();
    for (start, score) in scored {
        if picked
            .iter()
            .any(|(taken, _)| start.abs_diff(*taken) < span.max(1) / 2 + 1)
        {
            continue;
        }
        picked.push((start, score));
        if picked.len() >= limit {
            break;
        }
    }

    picked
        .into_iter()
        .map(|(start, score)| {
            let end = (start + span).min(file_lines.len());
            let window = &file_lines[start..end];
            Nearest {
                line: start + 1,
                end_line: end,
                similarity: (score * 100.0).round().clamp(0.0, 100.0) as u8,
                snippet: render_snippet(&file_lines, start, end),
                differences: describe_window(&needle_lines, window, start),
            }
        })
        .collect()
}

fn trim_trailing_empty(mut lines: Vec<&str>) -> Vec<&str> {
    while lines.last().map(|line| line.trim().is_empty()) == Some(true) {
        lines.pop();
    }
    lines
}

fn join_normalized(lines: &[&str]) -> String {
    lines
        .iter()
        .map(|line| collapse(line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 空白折叠：只用于相似度与「差在空白」的判定，不用于写回。
fn collapse(line: &str) -> String {
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn render_snippet(lines: &[&str], start: usize, end: usize) -> String {
    let shown_end = end.min(start + SNIPPET_LINES);
    let mut out = String::new();
    for index in start..shown_end {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!("{:>5}| {}", index + 1, lines[index]));
    }
    if end > shown_end {
        out.push_str(&format!("\n      | …(还有 {} 行)", end - shown_end));
    }
    out
}

/// 逐行差异：按起点对齐比较，先报空白/引号这类最常见的走样。
fn describe_window(needle: &[&str], window: &[&str], start: usize) -> Vec<String> {
    let mut notes = Vec::new();
    for (offset, expected) in needle.iter().enumerate() {
        if notes.len() >= MAX_DIFFERENCES {
            break;
        }
        let Some(actual) = window.get(offset) else {
            notes.push(format!(
                "第 {} 行起少了 {} 行（文件到第 {} 行就结束了）",
                start + offset + 1,
                needle.len() - offset,
                start + window.len()
            ));
            break;
        };
        if expected == actual {
            continue;
        }
        notes.push(format!(
            "第 {} 行：{}",
            start + offset + 1,
            describe_line_diff(expected, actual)
        ));
    }
    if notes.is_empty() && needle.len() < window.len() {
        notes.push(format!(
            "文件里该片段多出 {} 行（到第 {} 行）",
            window.len() - needle.len(),
            start + window.len()
        ));
    }
    notes
}

/// 单行差异的最小解释。
fn describe_line_diff(expected: &str, actual: &str) -> String {
    if expected.trim_end() == actual.trim_end() {
        return "行尾空白不同".to_string();
    }
    if expected.trim() == actual.trim() {
        return "首尾空白不同".to_string();
    }
    if collapse(expected) == collapse(actual) {
        return "缩进/空白不同".to_string();
    }
    if strip_whitespace(expected) == strip_whitespace(actual) {
        return "空白不同（空格数量/位置不一致，去掉空白后完全相同）".to_string();
    }
    if normalize_quotes(expected) == normalize_quotes(actual) {
        return "引号不同（直引号与弯引号/单双引号）".to_string();
    }
    if normalize_quotes(&collapse(expected)) == normalize_quotes(&collapse(actual)) {
        return "引号与空白都不同".to_string();
    }
    let column = first_difference_column(expected, actual);
    format!(
        "第 {} 列不同：old «{}» vs 文件 «{}»",
        column + 1,
        context_around(expected, column),
        context_around(actual, column)
    )
}

fn strip_whitespace(text: &str) -> String {
    text.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn normalize_quotes(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            '“' | '”' | '„' => '"',
            '‘' | '’' | '‚' => '\'',
            _ => ch,
        })
        .collect()
}

fn first_difference_column(expected: &str, actual: &str) -> usize {
    expected
        .chars()
        .zip(actual.chars())
        .position(|(left, right)| left != right)
        .unwrap_or_else(|| expected.chars().count().min(actual.chars().count()))
}

fn context_around(line: &str, column: usize) -> String {
    let chars: Vec<char> = line.chars().collect();
    let start = column.saturating_sub(DIFF_CONTEXT_CHARS);
    let end = (column + DIFF_CONTEXT_CHARS).min(chars.len());
    let mut out: String = chars[start..end].iter().collect();
    if start > 0 {
        out.insert(0, '…');
    }
    if end < chars.len() {
        out.push('…');
    }
    out
}

/// 失败时给模型看的一行结论 + 可直接照做的恢复动作。
pub(crate) fn summary_line(nearest: &[Nearest]) -> Option<String> {
    let first = nearest.first()?;
    let mut line = format!(
        "最相近的是第 {}-{} 行（相似度 {}%）",
        first.line, first.end_line, first.similarity
    );
    if let Some(detail) = first.differences.first() {
        line.push('；');
        line.push_str(detail);
    }
    Some(line)
}

/// 结构化失败载荷里的 `nearest` 数组（每个片段含预览与差异）。
pub(crate) fn to_json(nearest: &[Nearest]) -> serde_json::Value {
    serde_json::Value::Array(
        nearest
            .iter()
            .map(|item| {
                serde_json::json!({
                    "line": item.line,
                    "end_line": item.end_line,
                    "similarity": item.similarity,
                    "snippet": item.snippet,
                    "differences": item.differences,
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "def draw_bike(t):\n    wheel = (40, 80)\n    pouch = [(385, 114), (390, 118)]\n    return wheel\n\n\ndef render(t):\n    return draw_bike(t)\n";

    #[test]
    fn blank_and_quote_drift_are_explained() {
        let needle = "def draw_bike(t):\n    wheel = (40, 80)\n    pouch  = [(385,114), (390, 118)]\n    return wheel";
        let nearest = nearest_matches(SOURCE, needle, 3);
        assert!(!nearest.is_empty());
        let first = &nearest[0];
        assert_eq!(first.line, 1);
        assert!(first.similarity >= 60, "{:?}", first);
        assert!(first.differences.iter().any(|note| note.contains("第 3 行")));
        assert!(first.snippet.contains("pouch = [(385, 114)"));
    }

    #[test]
    fn unrelated_needle_still_points_at_the_closest_block() {
        let nearest = nearest_matches(SOURCE, "def draw_bike(t):\n    wheel = (41, 80)", 3);
        assert_eq!(nearest[0].line, 1);
        assert!(nearest[0].differences.iter().any(|note| note.contains("第 2 行")));
    }

    #[test]
    fn missing_tail_lines_are_reported() {
        let nearest = nearest_matches("def a():\n    pass\n", "def a():\n    pass\n\n\n", 1);
        assert!(!nearest.is_empty());
        assert!(nearest[0].similarity >= 90);
    }

    #[test]
    fn summary_line_mentions_the_range() {
        let nearest = nearest_matches(SOURCE, "def draw_bike(t):\n    wheel = (40, 80)\n    pouch = [(385, 114), (390, 118)]\n    return wheels", 1);
        let summary = summary_line(&nearest).expect("summary");
        assert!(summary.contains("第 1-4 行"), "{summary}");
    }
}
