//! Bounded structured patch projection for the timeline diff card (§7.4).
//!
//! The runtime already returns `files[]` with `diff_blocks[]`, `old_line`,
//! `new_line` and `added_lines`/`deleted_lines`; the native layer turns that
//! into a flat, pre-bounded card so the UI never parses a diff itself.
use crate::native::{NativePatchFile, NativePatchLine};
use serde_json::Value;

/// Files rendered per tool result.
const MAX_FILES: usize = 8;
/// Diff lines retained per file, matching the section projection bound.
const MAX_LINES: usize = 120;
/// A pending patch preview is echoed back from the submitted arguments.
const MAX_PREVIEW_BYTES: usize = 32 * 1024;

fn action(value: &Value) -> &'static str {
    match value["action"].as_str() {
        Some("add") => "add",
        Some("delete") => "delete",
        Some("move") => "move",
        _ => "update",
    }
}

fn line(value: &Value) -> NativePatchLine {
    let kind = match value["kind"].as_str() {
        Some("add") => "add",
        Some("delete") => "delete",
        Some("error") => "error",
        _ => "context",
    };
    let number = if kind == "delete" {
        value.get("old_line")
    } else {
        value
            .get("new_line")
            .filter(|value| !value.is_null())
            .or_else(|| value.get("old_line"))
    };
    NativePatchLine {
        text: value["text"]
            .as_str()
            .unwrap_or_default()
            .chars()
            .take(2048)
            .collect(),
        kind: kind.into(),
        number: number
            .and_then(Value::as_u64)
            .map(|value| value.to_string())
            .unwrap_or_default(),
    }
}

/// `data` is the unwrapped tool result payload (see `sections::project`).
pub(super) fn project(tool: &str, payload: &Value, args: &Value) -> Vec<NativePatchFile> {
    if !is_patch_tool(tool) {
        return Vec::new();
    }
    let result = payload.get("result").unwrap_or(payload);
    let mut data = result;
    for _ in 0..4 {
        if let Some(nested) = data.get("data").filter(|value| value.is_object()) {
            data = nested;
        } else {
            break;
        }
    }
    if let Some(files) = data["files"].as_array() {
        let cards: Vec<NativePatchFile> = files
            .iter()
            .take(MAX_FILES)
            .map(|file| {
                let mut lines = Vec::new();
                if let Some(blocks) = file["diff_blocks"].as_array() {
                    for block in blocks {
                        if let Some(header) =
                            block["header"].as_str().filter(|value| !value.is_empty())
                        {
                            if lines.len() < MAX_LINES {
                                lines.push(NativePatchLine {
                                    text: header.chars().take(2048).collect(),
                                    kind: "header".into(),
                                    number: String::new(),
                                });
                            }
                        }
                        if let Some(block_lines) = block["lines"].as_array() {
                            for value in block_lines {
                                if lines.len() >= MAX_LINES {
                                    break;
                                }
                                lines.push(line(value));
                            }
                        }
                    }
                }
                NativePatchFile {
                    path: file["path"]
                        .as_str()
                        .or_else(|| file["to_path"].as_str())
                        .unwrap_or_default()
                        .into(),
                    action: action(file).into(),
                    added: file["added_lines"].as_u64().unwrap_or(0).to_string(),
                    deleted: file["deleted_lines"].as_u64().unwrap_or(0).to_string(),
                    lines: lines.into(),
                }
            })
            .collect();
        if !cards.is_empty() {
            return cards;
        }
    }
    // Before the result arrives the submitted patch is the only structured
    // source; it has no line numbers, so the card carries a preview action.
    let patch: String = args["patch"]
        .as_str()
        .or_else(|| args["input"].as_str())
        .unwrap_or_default()
        .chars()
        .take(MAX_PREVIEW_BYTES)
        .collect();
    preview(&patch)
}

fn preview(patch: &str) -> Vec<NativePatchFile> {
    let mut cards: Vec<NativePatchFile> = Vec::new();
    let mut card: Option<NativePatchFile> = None;
    for value in patch.lines() {
        // The submitted patch format separates files with "*** " headers.
        if let Some(title) = value.strip_prefix("*** ") {
            if let Some(finished) = card.take() {
                if !finished.lines.is_empty() {
                    cards.push(finished);
                }
            }
            if cards.len() >= MAX_FILES {
                break;
            }
            card = Some(NativePatchFile {
                path: title.trim().into(),
                action: "preview".into(),
                added: "0".into(),
                deleted: "0".into(),
                lines: Vec::new().into(),
            });
            continue;
        }
        let Some(current) = card.as_mut() else {
            continue;
        };
        if current.lines.len() >= MAX_LINES {
            continue;
        }
        let (kind, text) = if let Some(text) = value.strip_prefix('+') {
            ("add", text)
        } else if let Some(text) = value.strip_prefix('-') {
            ("delete", text)
        } else if value.starts_with("@@") {
            ("header", value)
        } else {
            ("context", value)
        };
        current.lines.push(NativePatchLine {
            text: text.chars().take(2048).collect(),
            kind: kind.into(),
            number: String::new(),
        });
    }
    if let Some(finished) = card.take() {
        if !finished.lines.is_empty() {
            cards.push(finished);
        }
    }
    cards
}

/// Only the tools that actually produce file diffs get a card.
pub(super) fn is_patch_tool(tool: &str) -> bool {
    matches!(tool, "apply_patch" | "应用补丁" | "write_file" | "写入文件")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn file_blocks_keep_line_numbers_kinds_and_counts() {
        let payload = json!({"result":{"data":{"files":[{
            "path": "sample.txt", "action": "update", "added_lines": 2, "deleted_lines": 1,
            "diff_blocks": [{"header": "@@ -2 +2 @@", "lines": [
                {"kind": "delete", "old_line": 2, "text": "old"},
                {"kind": "add", "new_line": 2, "text": "new"},
            ]}]
        }]}}});
        let cards = project("apply_patch", &payload, &Value::Null);
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].path, "sample.txt");
        assert_eq!(cards[0].action, "update");
        assert_eq!(
            (cards[0].added.as_str(), cards[0].deleted.as_str()),
            ("2", "1")
        );
        assert_eq!(cards[0].lines[0].kind, "header");
        assert_eq!(cards[0].lines[1].number, "2");
        assert_eq!(cards[0].lines[2].kind, "add");
    }

    #[test]
    fn pending_preview_splits_files_and_never_claims_line_numbers() {
        let args =
            json!({"patch": "*** sample.txt\n@@ -1 +1 @@\n-old\n+new\n*** other.txt\n+added"});
        let cards = project("apply_patch", &json!({}), &args);
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].path, "sample.txt");
        assert_eq!(cards[0].action, "preview");
        assert!(cards[0].lines.iter().all(|line| line.number.is_empty()));
        assert_eq!(cards[1].lines.len(), 1);
    }

    #[test]
    fn unrelated_tools_never_produce_a_card() {
        assert!(project(
            "read_file",
            &json!({"files": [{"path": "x"}]}),
            &Value::Null
        )
        .is_empty());
    }
}
