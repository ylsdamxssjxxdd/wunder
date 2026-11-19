//! 字面替换的容错匹配阶梯。
//!
//! 源问题：`old_string` 只要有一个字符对不上（行尾多一个空格、整块缩进被
//! 格式化过、行内多/少一个空格）就判 `NoMatch`，模型只能回读后重写整段，
//! 一次编辑多花一轮，且重写整段又容易带进新偏差。
//!
//! 这里在精确匹配失败后，按「行尾空白 → 块内缩进 → 行内空白折叠」逐级放宽，
//! **首个有命中的档位**胜出并单独完成这一次替换，档位随回执返回，模型知道
//! 这次是「一字不差」还是「靠空白容错」。
//!
//! 边界（宁可失败也不猜）：
//! - 只容忍空白类差异；字符本身不同（数值、标识符、引号内容、大小写）不猜，
//!   仍交给 `edit_recovery` 的最近片段诊断。
//! - 每个档位要求整块逐行一致，不跨行混搭，也不会跨行折叠空白。
//! - 命中数有上限，候选锚点有上限，避免病态输入把编辑拖成全文扫描。

/// 匹配档位，越靠前越严格（`Ord` 按声明顺序，即严格度递增）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum MatchTier {
    /// 逐字节一致（换行风格已在调用方归一）。
    Exact,
    /// 只差行尾空白（含 CRLF 的 `\r`）。
    TrailingWhitespace,
    /// 整块缩进不同，块内相对缩进一致。
    Indentation,
    /// 行内空白数量/位置不同（按空白折叠后一致）。
    Whitespace,
    /// 只剩「去掉全部空白后一致」——最后一档，连块内相对缩进也不计较了。
    LooseWhitespace,
}

impl MatchTier {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            MatchTier::Exact => "exact",
            MatchTier::TrailingWhitespace => "trailing_whitespace",
            MatchTier::Indentation => "indentation",
            MatchTier::Whitespace => "whitespace",
            MatchTier::LooseWhitespace => "loose_whitespace",
        }
    }

    pub(crate) fn is_exact(self) -> bool {
        matches!(self, MatchTier::Exact)
    }

    /// 面向模型的说明：这次为什么会「看起来对但没逐字对上」。
    pub(crate) fn note(self) -> &'static str {
        match self {
            MatchTier::Exact => "old_string 与文件逐字一致。",
            MatchTier::TrailingWhitespace => {
                "old_string 与实际内容只差行尾空白（空格/制表符/CR），已按行尾空白归一匹配；写入的是 new_string 原文，请留意行尾是否带上了不可见空白。"
            }
            MatchTier::Indentation => {
                "old_string 与实际内容的块内缩进不同（相对缩进一致），已按缩进归一匹配；new_string 原样写入，请确认缩进与所选上下文对齐。"
            }
            MatchTier::Whitespace => {
                "old_string 与实际内容只差行内空白数量/位置，已按空白折叠匹配；new_string 原样写入，请确认行内空格符合预期。"
            }
            MatchTier::LooseWhitespace => {
                "old_string 与实际内容只差空白（含缩进），已按「去掉全部空白后一致」这一最后一档匹配；new_string 原样写入，第一行不再继承文件缩进，请按回执的 changed_lines 确认排版。"
            }
        }
    }
}

/// 单次匹配最多记录的命中数：只防病态输入，正常编辑远达不到。
const MAX_MATCHES: usize = 100_000;
/// 容错档位最多尝试的候选锚点数；超过就放弃容错（大概率是重复片段）。
const MAX_CANDIDATES: usize = 4_096;

/// 一次匹配的结果。
#[derive(Debug, Clone)]
pub(crate) struct Found {
    pub(crate) tier: MatchTier,
    /// 命中区间的字节范围（`start..end`），按位置升序、互不重叠。
    pub(crate) spans: Vec<(usize, usize)>,
}

impl Found {
    pub(crate) fn occurrences(&self) -> usize {
        self.spans.len()
    }

    /// 命中起点，供行号换算与 diff 采样使用。
    pub(crate) fn starts(&self) -> Vec<usize> {
        self.spans.iter().map(|(start, _)| *start).collect()
    }
}

/// 在 `content` 里定位 `needle`：先精确，再按档位逐级放宽。
pub(crate) fn find(content: &str, needle: &str) -> Option<Found> {
    if needle.is_empty() {
        return None;
    }
    let exact: Vec<(usize, usize)> = content
        .match_indices(needle)
        .take(MAX_MATCHES)
        .map(|(start, _)| (start, start + needle.len()))
        .collect();
    if !exact.is_empty() {
        return Some(Found {
            tier: MatchTier::Exact,
            spans: exact,
        });
    }

    // 末尾换行单独处理：整块可能以换行结尾，匹配区间也要把这个换行吃进去。
    let body = needle.strip_suffix('\n').unwrap_or(needle);
    if body.is_empty() {
        return None;
    }
    let tail_newline = needle.len() > body.len();
    let old_lines: Vec<&str> = body.split('\n').collect();
    let lines = scan_lines(content);
    if old_lines.len() > lines.len() {
        return None;
    }
    let span = old_lines.len();
    let last_start = lines.len() - span;

    // 候选锚点：只挑首行「去掉全部空白后」一致的位置。这是所有档位的公共
    // 必要条件（最松的档位会折掉行内空白，最严的档位本来就逐字一致），
    // 因此是安全的超集过滤，不会漏掉真命中。
    let first_key = strip_whitespace(old_lines[0]);
    let mut candidates = Vec::new();
    for start in 0..=last_start {
        if strip_whitespace(line_text(content, lines[start])) == first_key {
            candidates.push(start);
            if candidates.len() > MAX_CANDIDATES {
                return None;
            }
        }
    }
    if candidates.is_empty() {
        return None;
    }

    for tier in [
        MatchTier::TrailingWhitespace,
        MatchTier::Indentation,
        MatchTier::Whitespace,
        MatchTier::LooseWhitespace,
    ] {
        let expected = normalize_block(&old_lines, tier);
        let mut spans: Vec<(usize, usize)> = Vec::new();
        for start in candidates.iter().copied() {
            let window: Vec<&str> = (start..start + span)
                .map(|index| line_text(content, lines[index]))
                .collect();
            if normalize_block(&window, tier) != expected {
                continue;
            }
            let start_byte = lines[start].0;
            let mut end_byte = lines[start + span - 1].1;
            if tail_newline && content.as_bytes().get(end_byte) == Some(&b'\n') {
                end_byte += 1;
            }
            // 防止 span > 1 时相邻候选互相重叠，replace 会因此切错区间。
            if spans.last().is_some_and(|(_, previous_end)| start_byte < *previous_end) {
                continue;
            }
            spans.push((start_byte, end_byte));
            if spans.len() >= MAX_MATCHES {
                break;
            }
        }
        if !spans.is_empty() {
            return Some(Found { tier, spans });
        }
    }
    None
}

/// 按命中区间把内容换成 `replacement`；`replace_all=false` 只换第一处。
pub(crate) fn replace(content: &str, found: &Found, replacement: &str, replace_all: bool) -> String {
    let count = if replace_all {
        found.spans.len()
    } else {
        found.spans.len().min(1)
    };
    let extra: usize = found.spans[..count]
        .iter()
        .map(|(start, end)| end - start)
        .sum();
    let mut out = String::with_capacity(content.len() + replacement.len().saturating_mul(count) - extra.min(content.len()));
    let mut cursor = 0usize;
    for (start, end) in found.spans.iter().take(count) {
        let start = *start;
        out.push_str(&content[cursor..start]);
        out.push_str(replacement);
        cursor = *end;
    }
    out.push_str(&content[cursor..]);
    out
}

/// 每行的字节区间：`start` 到 `end`（不含 `\n`；CRLF 的 `\r` 留在行内，由
/// 各档位归一化处理）。
fn scan_lines(content: &str) -> Vec<(usize, usize)> {
    let bytes = content.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0usize;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' {
            lines.push((start, index));
            start = index + 1;
        }
    }
    if start < bytes.len() {
        lines.push((start, bytes.len()));
    }
    lines
}

fn line_text<'a>(content: &'a str, span: (usize, usize)) -> &'a str {
    &content[span.0..span.1]
}

/// 去掉所有空白字符，只用于候选锚点的快速预筛。
fn strip_whitespace(line: &str) -> String {
    line.chars().filter(|ch| !ch.is_whitespace()).collect()
}

/// 把整块按档位归一成可逐行比较的形态。
fn normalize_block(lines: &[&str], tier: MatchTier) -> Vec<String> {
    match tier {
        MatchTier::Exact => lines.iter().map(|line| (*line).to_string()).collect(),
        MatchTier::TrailingWhitespace => lines
            .iter()
            .map(|line| line.trim_end().to_string())
            .collect(),
        MatchTier::Indentation => strip_common_indent(lines),
        MatchTier::Whitespace => lines.iter().map(|line| collapse_inner(line)).collect(),
        MatchTier::LooseWhitespace => lines.iter().map(|line| strip_whitespace(line)).collect(),
    }
}

/// 折掉行内空白，但**保留前导缩进**：缩进漂移由上一档专门负责，这里不越权，
/// 否则「相对缩进被压平」的片段也会被误判命中。
fn collapse_inner(line: &str) -> String {
    let trimmed_end = line.trim_end();
    let indent_len = trimmed_end.len() - trimmed_end.trim_start().len();
    let (indent, rest) = trimmed_end.split_at(indent_len);
    let mut out = String::with_capacity(trimmed_end.len());
    out.push_str(indent);
    out.push_str(&rest.split_whitespace().collect::<Vec<_>>().join(" "));
    out
}

/// 去掉整块的公共缩进（按非空行的最小缩进），再去掉行尾空白。
fn strip_common_indent(lines: &[&str]) -> Vec<String> {
    let indent = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|line| strip_leading_whitespace(line, indent).trim_end().to_string())
        .collect()
}

/// 最多剥掉 `limit` 字节的前导空白，不会切在多字节字符中间。
fn strip_leading_whitespace(line: &str, limit: usize) -> &str {
    let mut cut = 0usize;
    for (offset, ch) in line.char_indices() {
        if cut >= limit || !ch.is_whitespace() {
            break;
        }
        cut = offset + ch.len_utf8();
    }
    &line[cut..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match_wins_before_any_tolerance() {
        let content = "let a = 1;\nlet b = 2;\n";
        let found = find(content, "let b = 2;\n").expect("exact match");
        assert_eq!(found.tier, MatchTier::Exact);
        assert_eq!(found.occurrences(), 1);
        assert_eq!(&content[found.spans[0].0..found.spans[0].1], "let b = 2;\n");
    }

    #[test]
    fn trailing_whitespace_drift_is_tolerated() {
        let content = "fn main() {   \n    run();\t\n}\n";
        let found = find(content, "fn main() {\n    run();\n}").expect("trailing whitespace");
        assert_eq!(found.tier, MatchTier::TrailingWhitespace);
        let replaced = replace(content, &found, "fn main() {\n    stop();\n}", false);
        assert_eq!(replaced, "fn main() {\n    stop();\n}\n");
    }

    #[test]
    fn block_reindentation_is_tolerated() {
        let content = "impl A {\n        fn f() {\n            go();\n        }\n}\n";
        let found = find(content, "fn f() {\n    go();\n}").expect("reindented block");
        assert_eq!(found.tier, MatchTier::Indentation);
        // 命中区间覆盖整行；new_string 原样写入，不猜测缩进。
        let replaced = replace(content, &found, "fn f() {\n    stop();\n}", false);
        assert_eq!(replaced, "impl A {\nfn f() {\n    stop();\n}\n}\n");
    }

    #[test]
    fn flattened_relative_indentation_falls_to_the_loosest_tier() {
        let content = "fn f() {\n  if x {\n    go();\n  }\n}\n";
        // 块内相对缩进被压平：严格档位都不命中，只有最后一档兜住。
        let found = find(content, "fn f() {\n  if x {\n  go();\n  }\n}").expect("loose match");
        assert_eq!(found.tier, MatchTier::LooseWhitespace);
        assert_eq!(found.occurrences(), 1);
    }

    #[test]
    fn inline_whitespace_is_collapsed_last() {
        let content = "let  value = compute( a , b );\n";
        let found = find(content, "let value = compute( a , b );").expect("collapsed");
        assert_eq!(found.tier, MatchTier::Whitespace);
        let replaced = replace(content, &found, "let value = 0;", false);
        assert_eq!(replaced, "let value = 0;\n");
    }

    #[test]
    fn content_drift_is_never_guessed() {
        let content = "height: 12.5px;\ncolor: #f5f5f5;\n";
        assert!(find(content, "height: 12.50px;").is_none());
        assert!(find(content, "color: #F5F5F5;").is_none());
    }

    #[test]
    fn replace_all_rewrites_every_hit() {
        let content = "a();\n\na();\n";
        let found = find(content, "a();").expect("two hits");
        assert_eq!(found.occurrences(), 2);
        assert_eq!(replace(content, &found, "b();", false), "b();\n\na();\n");
        assert_eq!(replace(content, &found, "b();", true), "b();\n\nb();\n");
    }

    #[test]
    fn crlf_block_matches_without_trailing_cr_noise() {
        let content = "fn main() {\r\n    run();\r\n}\r\n";
        let found = find(content, "fn main() {\n    run();\n}\n").expect("crlf");
        assert_eq!(found.tier, MatchTier::TrailingWhitespace);
        assert_eq!(replace(content, &found, "fn main() {\r\n    stop();\r\n}\r\n", false), "fn main() {\r\n    stop();\r\n}\r\n");
    }

    #[test]
    fn spans_are_non_overlapping() {
        let content = "}\n}\n}\n";
        let found = find(content, "}\n}").expect("overlapping windows");
        // 相邻两行同形时会命中多个窗口，但抽样区间不得互相重叠。
        for pair in found.spans.windows(2) {
            assert!(pair[0].1 <= pair[1].0);
        }
    }
}
