use serde::Serialize;
use serde_json::{Map, Value};
use std::borrow::Cow;
use url::Url;

const MAX_ANALYSIS_BYTES: usize = 32 * 1024;
const MAX_IMAGE_LINK_SCANS: usize = 32;
const SUSPICIOUS_HOST_FRAGMENTS: [&str; 2] = ["acelerate", "accelarate"];

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct OutputQualityMarkers {
    pub version: u8,
    pub truncated: bool,
    pub malformed_table_blocks: usize,
    pub suspicious_image_urls: usize,
}

impl OutputQualityMarkers {
    fn has_issues(&self) -> bool {
        self.malformed_table_blocks > 0 || self.suspicious_image_urls > 0
    }
}

pub fn annotate_chat_payload<'a>(payload: &'a Value) -> Cow<'a, Value> {
    if !should_analyze_payload(payload) {
        return Cow::Borrowed(payload);
    }
    let Some(content) = payload.get("content").and_then(Value::as_str) else {
        return Cow::Borrowed(payload);
    };
    let markers = analyze_output_quality(content);
    if !markers.has_issues() {
        return Cow::Borrowed(payload);
    }
    let mut annotated = payload.clone();
    attach_markers(&mut annotated, markers);
    Cow::Owned(annotated)
}

pub fn analyze_output_quality(content: &str) -> OutputQualityMarkers {
    let (sample, truncated) = truncate_utf8(content, MAX_ANALYSIS_BYTES);
    if !sample.contains('|') && !sample.contains("![") {
        return OutputQualityMarkers {
            version: 1,
            truncated,
            ..OutputQualityMarkers::default()
        };
    }
    OutputQualityMarkers {
        version: 1,
        truncated,
        malformed_table_blocks: count_malformed_table_blocks(sample),
        suspicious_image_urls: count_suspicious_image_urls(sample),
    }
}

fn should_analyze_payload(payload: &Value) -> bool {
    if payload.get("role").and_then(Value::as_str) != Some("assistant") {
        return false;
    }
    if payload
        .get("analysis")
        .and_then(Value::as_object)
        .and_then(|analysis| analysis.get("output_quality"))
        .is_some()
    {
        return false;
    }
    payload
        .get("meta")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("type"))
        .and_then(Value::as_str)
        != Some("tool_call")
}

fn attach_markers(payload: &mut Value, markers: OutputQualityMarkers) {
    let Some(root) = payload.as_object_mut() else {
        return;
    };
    let analysis = ensure_object_entry(root, "analysis");
    analysis.insert(
        "output_quality".to_string(),
        serde_json::to_value(markers).unwrap_or(Value::Null),
    );
}

fn ensure_object_entry<'a>(
    map: &'a mut Map<String, Value>,
    key: &str,
) -> &'a mut Map<String, Value> {
    let entry = map
        .entry(key.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    entry
        .as_object_mut()
        .expect("analysis entry must be object")
}

fn truncate_utf8(input: &str, max_bytes: usize) -> (&str, bool) {
    if input.len() <= max_bytes {
        return (input, false);
    }
    let mut end = max_bytes;
    while !input.is_char_boundary(end) {
        end -= 1;
    }
    (&input[..end], true)
}

fn count_malformed_table_blocks(markdown: &str) -> usize {
    let mut malformed = 0usize;
    let mut previous_line: Option<&str> = None;
    let mut in_fence = false;
    for raw_line in markdown.lines() {
        let line = raw_line.trim();
        if is_fence_toggle(line) {
            in_fence = !in_fence;
            previous_line = None;
            continue;
        }
        if in_fence {
            continue;
        }
        if line.is_empty() {
            previous_line = None;
            continue;
        }
        let is_malformed = previous_line
            .and_then(|header_line| table_cell_count(header_line).zip(delimiter_cell_count(line)))
            .is_some_and(|(header_cells, delimiter_cells)| {
                header_cells >= 2 && header_cells != delimiter_cells
            });
        if is_malformed {
            malformed = malformed.saturating_add(1);
        }
        previous_line = Some(line);
    }
    malformed
}

fn is_fence_toggle(line: &str) -> bool {
    line.starts_with("```") || line.starts_with("~~~")
}

fn table_cell_count(line: &str) -> Option<usize> {
    let cells = split_table_cells(line)?;
    if cells.iter().all(|cell| is_delimiter_cell(cell)) {
        return None;
    }
    Some(cells.len())
}

fn delimiter_cell_count(line: &str) -> Option<usize> {
    let cells = split_table_cells(line)?;
    if cells.iter().all(|cell| is_delimiter_cell(cell)) {
        return Some(cells.len());
    }
    None
}

fn split_table_cells(line: &str) -> Option<Vec<&str>> {
    let trimmed = line.trim();
    if !trimmed.contains('|') {
        return None;
    }
    let parts: Vec<&str> = trimmed.split('|').collect();
    let mut start = usize::from(trimmed.starts_with('|'));
    let mut end = parts.len();
    if trimmed.ends_with('|') && end > start {
        end -= 1;
    }
    while start < end && parts[start].trim().is_empty() && start + 1 < end {
        start += 1;
    }
    while end > start && parts[end - 1].trim().is_empty() && end - 1 > start {
        end -= 1;
    }
    let cells: Vec<&str> = parts[start..end].iter().map(|cell| cell.trim()).collect();
    if cells.len() < 2 {
        return None;
    }
    Some(cells)
}

fn is_delimiter_cell(cell: &str) -> bool {
    let trimmed = cell.trim();
    if trimmed.len() < 3 {
        return false;
    }
    let without_prefix = trimmed.strip_prefix(':').unwrap_or(trimmed);
    let stripped = without_prefix.strip_suffix(':').unwrap_or(without_prefix);
    stripped.len() >= 3 && stripped.bytes().all(|byte| byte == b'-')
}

fn count_suspicious_image_urls(markdown: &str) -> usize {
    let mut suspicious = 0usize;
    let mut remaining = markdown;
    let mut scanned = 0usize;
    while scanned < MAX_IMAGE_LINK_SCANS {
        let Some(start) = remaining.find("![") else {
            break;
        };
        let after_start = &remaining[start + 2..];
        let Some(caption_end) = after_start.find("](") else {
            break;
        };
        let after_caption = &after_start[caption_end + 2..];
        let Some(url_end) = after_caption.find(')') else {
            break;
        };
        if is_suspicious_image_url(&after_caption[..url_end]) {
            suspicious = suspicious.saturating_add(1);
        }
        remaining = &after_caption[url_end + 1..];
        scanned += 1;
    }
    suspicious
}

fn is_suspicious_image_url(raw: &str) -> bool {
    let mut candidate = raw.split_whitespace().next().unwrap_or("").trim();
    if candidate.is_empty() {
        return false;
    }
    candidate = candidate.trim_matches(|ch| ch == '<' || ch == '>');
    if candidate.starts_with("data:")
        || candidate.starts_with("blob:")
        || candidate.starts_with('/')
        || candidate.starts_with("./")
        || candidate.starts_with("../")
    {
        return false;
    }
    if candidate.starts_with("http://") || candidate.starts_with("https://") {
        return match Url::parse(candidate) {
            Ok(url) => url
                .host_str()
                .map(contains_suspicious_host_fragment)
                .unwrap_or(true),
            Err(_) => true,
        };
    }
    candidate.contains("://") && Url::parse(candidate).is_err()
}

fn contains_suspicious_host_fragment(host: &str) -> bool {
    let lowered = host.to_ascii_lowercase();
    SUSPICIOUS_HOST_FRAGMENTS
        .iter()
        .any(|fragment| lowered.contains(fragment))
}

#[cfg(test)]
mod tests {
    use super::{analyze_output_quality, annotate_chat_payload, OutputQualityMarkers};
    use serde_json::json;

    #[test]
    fn flags_malformed_table_block() {
        let report = analyze_output_quality(
            "| 武器 | 使用者 | 效果 |\n|--------|------|\n| 战斧 | 美国 | 纵深打击 |",
        );
        assert_eq!(
            report,
            OutputQualityMarkers {
                version: 1,
                truncated: false,
                malformed_table_blocks: 1,
                suspicious_image_urls: 0,
            }
        );
    }

    #[test]
    fn ignores_valid_markdown_table() {
        let report = analyze_output_quality(
            "| 文件名 | 内容说明 |\n|--------|---------|\n| report.md | 汇总 |",
        );
        assert_eq!(
            report,
            OutputQualityMarkers {
                version: 1,
                truncated: false,
                malformed_table_blocks: 0,
                suspicious_image_urls: 0,
            }
        );
    }

    #[test]
    fn flags_suspicious_image_host_typo() {
        let report = analyze_output_quality(
            "![统计信息图](https://dashscope-a717.oss-acelerate.aliyuncs.com/demo.png)",
        );
        assert_eq!(
            report,
            OutputQualityMarkers {
                version: 1,
                truncated: false,
                malformed_table_blocks: 0,
                suspicious_image_urls: 1,
            }
        );
    }

    #[test]
    fn skips_tool_call_payloads() {
        let payload = json!({
            "role": "assistant",
            "content": "| a | b | c |\n|---|---|\n",
            "meta": { "type": "tool_call" }
        });
        assert_eq!(annotate_chat_payload(&payload).into_owned(), payload);
    }

    #[test]
    fn annotates_assistant_payload_with_issues() {
        let payload = json!({
            "role": "assistant",
            "content": "| a | b | c |\n|---|---|\n",
        });
        let annotated = annotate_chat_payload(&payload).into_owned();
        assert_eq!(
            annotated
                .get("analysis")
                .and_then(|value| value.get("output_quality")),
            Some(&json!({
                "version": 1,
                "truncated": false,
                "malformed_table_blocks": 1,
                "suspicious_image_urls": 0,
            }))
        );
    }
}
