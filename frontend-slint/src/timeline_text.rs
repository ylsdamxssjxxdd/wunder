//! Presentation projection for the chat timeline: localized tool labels, the
//! target chip, the bounded patch cards handed to `patch_diff_card.slint`, and
//! the mapping from native workflow sections onto the detail model.
use crate::{PatchCard, PatchLine};
use slint::{ModelRc, VecModel};
use wunder_desktop::native::{NativePatchFile, NativeWorkflowSection};

/// Tool display names (§7.3 C). Built-in runtime tools are already named in
/// Chinese; this maps the English aliases so the timeline reads the same
/// whichever name the event carried.
pub fn tool_label(tool: &str) -> String {
    let mapped = match tool.trim() {
        "read_file" | "read files" => "读取文件",
        "write_file" => "写入文件",
        "edit_file" | "edit_file2" | "text_editor" => "编辑文件",
        "apply_patch" => "应用补丁",
        "list_files" | "list_file" => "列出文件",
        "search_content" | "search_files" => "搜索内容",
        "execute_command" | "run_command" | "shell" => "运行命令",
        "command_session" => "命令会话",
        "context_compaction" | "compact" => "上下文压缩",
        "" => "工具",
        other => other,
    };
    mapped.chars().take(64).collect()
}

pub fn thought_running() -> &'static str {
    "正在思考…"
}

/// The settled thinking label; the same verb the previous bubble used.
pub fn thought_done() -> &'static str {
    "已思考"
}

pub fn thought_failed() -> &'static str {
    "思考中断"
}

/// The tool batch bar label of §7.3 D.
pub fn tool_calls(count: i32) -> String {
    format!("执行工具 {count} 次")
}

/// The small target chip of a tool entry: the path or command the call acted
/// on. `tool_result_display` keeps "action · status" on line one and the
/// target on line two, so a single-line result simply has no target.
pub fn target_of(title: &str, detail: &str, sections: &[NativeWorkflowSection]) -> (String, bool) {
    let patch = is_patch_tool(title);
    if patch {
        if let Some(section) = sections.iter().find(|section| section.kind == "patch") {
            return (short(&section.title, 64), true);
        }
    }
    let mut lines = detail.lines();
    let _action = lines.next();
    (short(lines.next().unwrap_or_default(), 64), patch)
}

fn short(value: &str, limit: usize) -> String {
    value.trim().chars().take(limit).collect()
}

pub fn is_patch_tool(tool: &str) -> bool {
    matches!(
        tool,
        "apply_patch" | "应用补丁" | "edit_file" | "edit_file2" | "文本编辑"
    )
}

/// Flat text of a tool result, kept for the copy affordance of the detail view.
pub fn copy_text(items: &[NativeWorkflowSection], fallback: &str) -> slint::SharedString {
    if items.is_empty() {
        return fallback.into();
    }
    let mut output = String::new();
    for section in items {
        output.push_str(&section.title);
        if !section.meta.is_empty() {
            output.push_str(" · ");
            output.push_str(&section.meta);
        }
        output.push('\n');
        for line in &section.lines {
            output.push_str(match line.kind.as_str() {
                "add" => "+",
                "delete" => "−",
                _ => "",
            });
            output.push_str(&line.text);
            output.push('\n');
        }
        output.push('\n');
    }
    output.into()
}

/// The patch cards of one tool entry (§7.4). Bounds are re-applied here so a
/// caller can never hand the card more than it renders.
pub fn patch_cards(files: &[NativePatchFile]) -> ModelRc<PatchCard> {
    ModelRc::new(VecModel::from(
        files
            .iter()
            .take(8)
            .map(|file| PatchCard {
                path: file.path.as_str().into(),
                action: action_label(&file.action).into(),
                added: file.added.as_str().into(),
                deleted: file.deleted.as_str().into(),
                lines: ModelRc::new(VecModel::from(
                    file.lines
                        .iter()
                        .take(200)
                        .map(|line| PatchLine {
                            number: line.number.as_str().into(),
                            text: line.text.as_str().into(),
                            kind: line.kind.as_str().into(),
                        })
                        .collect::<Vec<_>>(),
                )),
            })
            .collect::<Vec<_>>(),
    ))
}

/// File action verb for the card header.
pub fn action_label(action: &str) -> &'static str {
    match action {
        "add" => "新增",
        "delete" => "删除",
        "move" => "移动",
        "preview" => "待应用",
        _ => "更新",
    }
}

/// One-line entry summary from the already bounded `tool_result_display` text.
pub fn summary(detail: &str) -> String {
    detail
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .chars()
        .take(160)
        .collect()
}
