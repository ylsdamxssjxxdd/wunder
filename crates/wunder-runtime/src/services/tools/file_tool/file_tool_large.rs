use super::{merge_read_ranges, truncate_utf8_output, ReadFileSpec};
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub(super) struct LargeReadOutput {
    pub rendered: String,
    pub summary: Value,
    pub omitted_bytes: usize,
}

pub(super) fn read_large_slice(
    path: &Path,
    raw_path: &str,
    spec: &ReadFileSpec,
    include_header: bool,
) -> Result<LargeReadOutput> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut line = String::new();
    let mut total_lines = 0usize;
    let mut match_count = 0usize;
    let mut ranges = Vec::new();
    let mut selected: BTreeMap<usize, String> = BTreeMap::new();
    let mut recent: VecDeque<(usize, String)> = VecDeque::new();
    let matcher = spec
        .pattern
        .as_deref()
        .map(|pattern| {
            let expression = if spec.pattern_literal {
                regex::escape(pattern)
            } else {
                pattern.to_string()
            };
            regex::RegexBuilder::new(&expression)
                .case_insensitive(!spec.pattern_case_sensitive)
                .build()
        })
        .transpose()?;
    let mut plain_lines = Vec::new();
    loop {
        line.clear();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            break;
        }
        total_lines += 1;
        let text = line.trim_end_matches(['\r', '\n']);
        if let Some(matcher) = matcher.as_ref() {
            if matcher.is_match(text) {
                match_count += 1;
                if ranges.len() < 16 {
                    let start = total_lines.saturating_sub(spec.context_lines).max(1);
                    let end = total_lines + spec.context_lines;
                    ranges.push((start, end));
                    for (number, previous) in &recent {
                        if *number >= start {
                            selected.insert(*number, previous.clone());
                        }
                    }
                }
            }
            if ranges
                .iter()
                .any(|(left, right)| total_lines >= *left && total_lines <= *right)
            {
                selected.insert(total_lines, text.to_string());
            }
            recent.push_back((total_lines, text.to_string()));
            while recent.len() > spec.context_lines + 1 {
                recent.pop_front();
            }
        } else {
            if spec
                .ranges
                .iter()
                .any(|(start, end)| total_lines >= *start && total_lines <= *end)
            {
                plain_lines.push((total_lines, text.to_string()));
            }
        }
    }
    if matcher.is_some() {
        ranges = merge_read_ranges(ranges);
    } else {
        ranges = plain_lines
            .iter()
            .map(|(number, _)| (*number, *number))
            .collect();
        selected = plain_lines.into_iter().collect();
    }
    let mut parts = Vec::new();
    let show_headers = ranges.len() > 1;
    let mut cursor = 0usize;
    for (start, end) in &ranges {
        if show_headers {
            parts.push(format!("[lines {start}-{end}]"));
        }
        for number in *start..=*end {
            if let Some(text) = selected.get(&number) {
                parts.push(text.clone());
            }
        }
        cursor = cursor.max(*end);
    }
    if selected.is_empty() {
        parts.push(String::new());
    }
    let joined = parts.join("\n");
    let (content, omitted_bytes) =
        truncate_utf8_output(&joined, super::DEFAULT_READ_OUTPUT_BUDGET_BYTES);
    let matches_truncated = match_count > ranges.len() || omitted_bytes > 0;
    let read_lines = selected.len();
    let complete =
        omitted_bytes == 0 && !matches_truncated && (matcher.is_some() || cursor >= total_lines);
    let content_start_line = ranges.first().map(|(start, _)| *start);
    let summary = json!({
        "path": raw_path,
        "mode": "slice",
        "size_bytes": path.metadata()?.len(),
        "streaming": true,
        "truncated_by_size": false,
        "loaded_bytes": path.metadata()?.len(),
        "loaded_lines": total_lines,
        "read_lines": read_lines,
        "total_lines": total_lines,
        "effective_ranges": ranges,
        "content_start_line": content_start_line,
        "requested_ranges": spec.requested_ranges,
        "used_default_range": spec.used_default_range,
        "match_count": match_count,
        "matches_truncated": matches_truncated,
        "pattern": spec.pattern,
        "complete": complete,
        "request_satisfied": !matches_truncated,
        "hit_eof": true,
        "range_reaches_eof": cursor >= total_lines,
    });
    Ok(LargeReadOutput {
        rendered: if include_header {
            format!(">>> {raw_path}\n{}", content)
        } else {
            content
        },
        summary,
        omitted_bytes,
    })
}

pub(super) fn excerpt_long_line(text: &str, budget: usize) -> (String, usize) {
    if text.len() <= budget {
        return (text.to_string(), 0);
    }
    let marker = "...(truncated line excerpt)...";
    let head_budget = budget.saturating_sub(marker.len() + 2) / 2;
    let tail_budget = budget
        .saturating_sub(marker.len() + 2)
        .saturating_sub(head_budget);
    let mut head_end = head_budget.min(text.len());
    while head_end > 0 && !text.is_char_boundary(head_end) {
        head_end -= 1;
    }
    let mut tail_start = text.len().saturating_sub(tail_budget);
    while tail_start < text.len() && !text.is_char_boundary(tail_start) {
        tail_start += 1;
    }
    let omitted = text
        .len()
        .saturating_sub(head_end + (text.len() - tail_start));
    (
        format!("{}\n{}\n{}", &text[..head_end], marker, &text[tail_start..]),
        omitted,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::tools::file_tool::parse_read_file_specs;

    #[test]
    fn large_query_scans_the_whole_file_with_bounded_matches() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("large.log");
        let mut text = String::new();
        for index in 0..120_000 {
            if index % 4 == 0 {
                text.push_str(&format!("ERROR fixture {index}\n"));
            } else {
                text.push_str(&format!("INFO fixture {index}\n"));
            }
        }
        assert!(text.len() > super::super::MAX_READ_BYTES);
        std::fs::write(&path, text).unwrap();
        let spec =
            parse_read_file_specs(&json!({"path": "large.log", "query": "ERROR", "context": 0}))
                .unwrap()
                .remove(0);
        let result = read_large_slice(&path, "large.log", &spec, true).unwrap();
        assert_eq!(result.summary["total_lines"], 120_000);
        assert_eq!(result.summary["match_count"], 30_000);
        assert_eq!(result.summary["matches_truncated"], true);
        assert_eq!(result.summary["truncated_by_size"], false);
        assert_eq!(result.summary["read_lines"], 16);
        assert!(result.rendered.contains("ERROR fixture 0"));
    }

    #[test]
    fn long_line_excerpt_keeps_both_ends_without_splitting_utf8() {
        let text = format!("{}MIDDLE{}", "前".repeat(5000), "尾".repeat(5000));
        let (excerpt, omitted) = excerpt_long_line(&text, 512);
        assert!(omitted > 0);
        assert!(excerpt.starts_with('前'));
        assert!(excerpt.ends_with('尾'));
        assert!(!excerpt.contains('\u{fffd}'));
    }
}

#[cfg(test)]
mod plain_read_tests {
    use super::*;
    use crate::services::tools::file_tool::parse_read_file_specs;

    #[test]
    fn large_plain_read_returns_requested_window_and_full_line_count() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("large.log");
        let text = "INFO fixture line\n".repeat(120_000);
        assert!(text.len() > super::super::MAX_READ_BYTES);
        std::fs::write(&path, text).unwrap();
        let spec = parse_read_file_specs(&json!({"path": "large.log"}))
            .unwrap()
            .remove(0);
        let result = read_large_slice(&path, "large.log", &spec, true).unwrap();
        assert_eq!(result.summary["total_lines"], 120_000);
        assert_eq!(result.summary["read_lines"], 500);
        assert_eq!(result.summary["truncated_by_size"], false);
        assert!(result.rendered.contains("INFO fixture line"));
    }
}
