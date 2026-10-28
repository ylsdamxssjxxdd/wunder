//! Presentation projection for the chat timeline: localized tool labels, the
//! target chip, the bounded patch cards handed to `patch_diff_card.slint`, and
//! the mapping from native workflow sections onto the detail model.
use crate::{PatchCard, PatchLine, ProcessEntry, TimelineRow, WorkflowLine, WorkflowSection};
use slint::{ModelRc, VecModel};
use wunder_desktop::native::{NativePatchFile, NativeWorkflowEntry, NativeWorkflowSection};

/// The canonical name of the shell tool. Aliases and streamed command frames
/// all resolve to it, so one invocation can never read as two different tools.
pub const COMMAND_TOOL: &str = "执行命令";

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
        "glob" | "glob_files" | "文件名匹配" => "文件名匹配",
        "search_content" | "search_files" => "搜索内容",
        "web_search" | "web_search_tool" | "web search" | "websearch" | "网页搜索" => {
            "网页搜索"
        }
        "web_fetch" | "web_fetch_tool" | "web fetch" | "webfetch" | "网页抓取" => "网页抓取",
        "execute_command" | "run_command" | "shell" | "运行命令" => COMMAND_TOOL,
        "command_session" => "命令会话",
        "question_panel" | "ask_panel" => "问询面板",
        "update_plan" | "plan_update" => "计划面板",
        "read_image" | "view_image" => "读图",
        "skill_call" | "skill_get" => "技能调用",
        "schedule_task" => "定时任务",
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

/// The tool batch bar label of §7.3 D. A batch that only holds thinking has no
/// tool count to show, and reads as the settled verb instead.
pub fn tool_calls(count: i32) -> String {
    if count <= 0 {
        return "已处理".into();
    }
    format!("执行工具 {count} 次")
}

/// The work-process bar label of §7.3 A: one line over everything the turn did
/// before its newest answer block, counted in steps (rounds' notes, thinking
/// segments and tool calls; batch bars are containers and do not count).
pub fn process_label(steps: i32) -> String {
    format!("工作过程 · {steps} 步")
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
    if matches!(
        title.trim(),
        "execute_command" | "运行命令" | "执行命令" | "command_session"
    ) {
        return (String::new(), patch);
    }
    let mut lines = detail.lines();
    let _action = lines.next();
    (short(lines.next().unwrap_or_default(), 64), patch)
}

pub fn tool_target(entry: &NativeWorkflowEntry) -> (String, bool) {
    let patch = is_patch_tool(&entry.title);
    if patch {
        return target_of(&entry.title, "", &entry.sections);
    }
    if matches!(
        entry.title.as_str(),
        "read_file"
            | "读取文件"
            | "write_file"
            | "写入文件"
            | "list_files"
            | "列出文件"
            | "list_file"
            | "read_image"
            | "view_image"
            | "读图"
    ) {
        // A bounded preview at its limit may contain only a path prefix.
        // Keep it as summary text rather than opening an incomplete resource.
        return (
            if entry.preview.chars().count() < 640 {
                entry.preview.clone()
            } else {
                String::new()
            },
            false,
        );
    }
    (String::new(), false)
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
    if !fallback.is_empty() {
        return fallback.into();
    }
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

/// Preserve the native adapter's typed, bounded sections.
pub fn sections(items: &[NativeWorkflowSection]) -> ModelRc<WorkflowSection> {
    ModelRc::new(VecModel::from(
        items
            .iter()
            .take(12)
            .map(|section| WorkflowSection {
                title: section.title.as_str().into(),
                meta: section.meta.as_str().into(),
                kind: section.kind.as_str().into(),
                lines: ModelRc::new(VecModel::from(
                    section
                        .lines
                        .iter()
                        .take(201)
                        .map(|line| WorkflowLine {
                            ratio: line.ratio,
                            text: line.text.as_str().into(),
                            kind: line.kind.as_str().into(),
                            number: line.number.as_str().into(),
                        })
                        .collect::<Vec<_>>(),
                )),
            })
            .collect::<Vec<_>>(),
    ))
}

pub fn process_entry(row: &TimelineRow) -> ProcessEntry {
    let body = row.kind == crate::timeline::KIND_BODY;
    ProcessEntry {
        kind: row.kind,
        id: row.id.clone(),
        text: row.text.clone(),
        blocks: row.blocks.clone(),
        summary: if body {
            summary(&row.text).into()
        } else {
            row.summary.clone()
        },
        tool_name: if body {
            "模型输出".into()
        } else {
            row.tool_name.clone()
        },
        tool_icon: row.tool_icon.clone(),
        target: row.target.clone(),
        detail: if body {
            row.text.clone()
        } else {
            row.detail.clone()
        },
        sections: row.sections.clone(),
        patch: row.patch.clone(),
        tokens: row.tokens.clone(),
        duration: row.duration.clone(),
        status_kind: row.status_kind,
        open: row.open,
        payload: row.payload,
    }
}

pub fn sync_process_child(model: &VecModel<TimelineRow>, row: &TimelineRow) {
    use slint::Model;
    if row.process_idx == crate::timeline::NO_PROCESS {
        return;
    }
    let Some(bar) = model.iter().find(|bar| {
        bar.kind == crate::timeline::KIND_PROCESS && bar.process_idx == row.process_idx
    }) else {
        return;
    };
    if let Some(children) = bar
        .process_entries
        .as_any()
        .downcast_ref::<VecModel<ProcessEntry>>()
    {
        if let Some(index) = children.iter().position(|entry| entry.id == row.id) {
            if bar.process_paged {
                return;
            }
            children.set_row_data(index, process_entry(row));
        }
    }
}

/// Merge the current presentation thought into a lazy durable page by item
/// identity. All other entries, disclosure choices and model handles stay put.
pub fn merge_reasoning_tail(bar: &TimelineRow, mut entry: ProcessEntry) {
    use slint::Model;
    if bar.process_newer {
        return;
    }
    let Some(children) = bar
        .process_entries
        .as_any()
        .downcast_ref::<VecModel<ProcessEntry>>()
    else {
        return;
    };
    if let Some(index) = children.iter().position(|old| old.id == entry.id) {
        let old = children.row_data(index).unwrap();
        if old.status_kind != crate::timeline::STATUS_RUNNING
            && entry.status_kind == crate::timeline::STATUS_RUNNING
        {
            return;
        }
        entry.open = old.open;
        entry.payload = old.payload;
        if entry.status_kind != crate::timeline::STATUS_RUNNING
            && old.detail.len() > entry.detail.len()
        {
            entry.detail = old.detail;
            entry.summary = crate::message_blocks::reasoning_preview(&entry.detail);
        }
        if children.row_data(index).as_ref() != Some(&entry) {
            children.set_row_data(index, entry);
        }
    } else if let Some(index) = (0..children.row_count())
        .filter_map(|index| {
            let old = children.row_data(index)?;
            if old.kind != crate::timeline::KIND_REASON
                || old.status_kind != crate::timeline::STATUS_RUNNING
                || (stable_reasoning_identity(old.id.as_str())
                    && stable_reasoning_identity(entry.id.as_str()))
            {
                return None;
            }
            let old_detail = old.detail.as_str();
            let new_detail = entry.detail.as_str();
            // The live reducer and the durable pager use different row ids for
            // the same reasoning segment. Match their bounded text when the
            // ids differ, otherwise every refresh appends another “已思考”.
            let overlap = if old_detail == new_detail
                && entry.status_kind != crate::timeline::STATUS_RUNNING
            {
                old_detail.len()
            } else {
                return None;
            };
            Some((overlap, index))
        })
        .max_by_key(|(overlap, _)| *overlap)
        .map(|(_, index)| index)
    {
        let old = children.row_data(index).unwrap();
        entry.open = old.open;
        entry.payload = old.payload;
        children.set_row_data(index, entry);
    } else {
        // A new live segment settles any previous running thought. Do this
        // only after trying the text match above, so a durable/live identity
        // mismatch cannot manufacture a second completed thought row.
        for index in 0..children.row_count() {
            let Some(mut old) = children.row_data(index) else {
                continue;
            };
            if old.kind == crate::timeline::KIND_REASON
                && old.status_kind == crate::timeline::STATUS_RUNNING
            {
                old.status_kind = crate::timeline::STATUS_DONE;
                old.tool_name = thought_done().into();
                children.set_row_data(index, old);
            }
        }
        // Process pages retain at most 100 items, each with at most two rows.
        // Reserve one bounded live tail rather than grow a second transcript.
        if children.row_count() >= 201 {
            children.remove(200);
        }
        entry.payload = -1 - bar.process_idx * 1024 - children.row_count() as i32;
        children.push(entry);
    }
}

pub fn sync_reasoning_tails(rows: &[TimelineRow]) {
    for reason in rows
        .iter()
        .filter(|row| row.kind == crate::timeline::KIND_REASON)
    {
        if let Some(bar) = rows.iter().find(|row| {
            row.kind == crate::timeline::KIND_PROCESS && row.process_idx == reason.process_idx
        }) {
            merge_reasoning_tail(bar, process_entry(reason));
        }
    }
}

fn stable_reasoning_identity(id: &str) -> bool {
    id.rsplit_once(":text-").is_some_and(|(_, round)| {
        !round.is_empty() && round.chars().all(|value| value.is_ascii_digit())
    })
}

/// History replacement builds one child model per process; live deltas patch
/// the existing child VecModel instead of rebuilding the transcript.
pub fn process_models(rows: &mut [TimelineRow]) {
    use slint::Model;
    let mut children = std::collections::HashMap::<i32, Vec<ProcessEntry>>::new();
    for row in rows.iter().filter(|row| {
        row.process_idx != crate::timeline::NO_PROCESS
            && matches!(
                row.kind,
                crate::timeline::KIND_TOOL
                    | crate::timeline::KIND_REASON
                    | crate::timeline::KIND_BODY
            )
    }) {
        children
            .entry(row.process_idx)
            .or_default()
            .push(process_entry(row));
    }
    for row in rows
        .iter_mut()
        .filter(|row| row.kind == crate::timeline::KIND_PROCESS)
    {
        if row.process_paged {
            continue;
        }
        let entries = children.remove(&row.process_idx).unwrap_or_default();
        if let Some(model) = row
            .process_entries
            .as_any()
            .downcast_ref::<VecModel<ProcessEntry>>()
        {
            if model.row_count() == entries.len()
                && model
                    .iter()
                    .zip(&entries)
                    .all(|(old, new)| old.id == new.id)
            {
                for (index, entry) in entries.into_iter().enumerate() {
                    if model.row_data(index).as_ref() != Some(&entry) {
                        model.set_row_data(index, entry);
                    }
                }
            } else {
                model.set_vec(entries);
            }
        } else {
            row.process_entries = ModelRc::new(VecModel::from(entries));
        }
    }
}

/// Keep user disclosure choices through durable refreshes in both the main
/// chat and child-thread dialog. State stays bounded by the loaded history.
pub fn restore_folds(rows: &mut [TimelineRow], previous: &VecModel<TimelineRow>) {
    use slint::Model;
    let old: std::collections::HashMap<_, _> = previous
        .iter()
        .filter(|row| {
            matches!(
                row.kind,
                crate::timeline::KIND_PROCESS
                    | crate::timeline::KIND_TOOL
                    | crate::timeline::KIND_REASON
            )
        })
        .map(|row| (row.id.clone(), row))
        .collect();
    for row in rows.iter_mut() {
        if let Some(previous) = old.get(&row.id) {
            row.open = previous.open;
            if row.kind == crate::timeline::KIND_PROCESS {
                if previous.status_kind == crate::timeline::STATUS_RUNNING
                    && row.status_kind != crate::timeline::STATUS_RUNNING
                    && (previous.process_paged || previous.process_loading)
                    && !previous.process_newer
                {
                    row.process_entries = ModelRc::new(VecModel::<crate::ProcessEntry>::default());
                    row.process_paged = false;
                    row.process_local = false;
                    row.process_loading = false;
                    row.process_error = false;
                    continue;
                }
                row.process_entries = previous.process_entries.clone();
                row.process_paged = previous.process_paged;
                row.process_older = previous.process_older;
                row.process_newer = previous.process_newer;
                row.process_loading = previous.process_loading;
                row.process_error = previous.process_error;
                row.process_revision = previous.process_revision;
                if row.process_paged {
                    for index in 0..row.process_entries.row_count() {
                        if let Some(mut entry) = row.process_entries.row_data(index) {
                            let payload = -1 - row.process_idx * 1024 - index as i32;
                            if entry.payload != payload {
                                entry.payload = payload;
                                row.process_entries.set_row_data(index, entry);
                            }
                        }
                    }
                }
                if previous.process_root.is_empty() && !row.process_root.is_empty() {
                    row.process_loading = false;
                }
            }
        }
    }
    let folds: std::collections::HashMap<_, _> = rows
        .iter()
        .filter(|row| row.kind == crate::timeline::KIND_PROCESS)
        .map(|row| (row.process_idx, row.open))
        .collect();
    for row in rows.iter_mut() {
        if let Some(open) = folds.get(&row.process_idx) {
            row.process_open = *open;
        }
    }
    process_models(rows);
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

/// The web timeline's collapsed row describes the call, not the result status.
/// The native workflow adapter carries the bounded call preview separately so
/// live and durable rows stay identical even when result frames arrive later.
pub fn tool_summary(entry: &NativeWorkflowEntry) -> String {
    let preview = entry.preview.trim();
    if !preview.is_empty() {
        return short(preview, 160);
    }
    String::new()
}

/// The web timeline keeps invocation usage at the far edge of the row. Native
/// projection already receives the bounded, formatted values from the shared
/// desktop workflow adapter, so this helper only joins the populated parts.
pub fn tool_metrics(entry: &NativeWorkflowEntry) -> String {
    [entry.tokens.as_str(), entry.duration.as_str()]
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::Model;

    #[test]
    fn command_summary_uses_only_invocation_and_never_result_text() {
        let mut entry = NativeWorkflowEntry {
            id: "fixture-command".into(),
            title: "execute_command".into(),
            preview: "list files".into(),
            detail: "执行 · 运行中\nlist files\noutput".into(),
            state: "running".into(),
            tokens: String::new(),
            duration: String::new(),
            sections: Vec::new(),
            patch_files: Vec::new(),
        };
        assert_eq!(tool_summary(&entry), "list files");
        entry.preview.clear();
        assert_eq!(tool_summary(&entry), "");
        assert_eq!(tool_target(&entry).0, "");
    }

    #[test]
    fn command_aliases_share_one_display_name() {
        for alias in [
            "execute_command",
            "run_command",
            "shell",
            "运行命令",
            "执行命令",
        ] {
            assert_eq!(tool_label(alias), COMMAND_TOOL, "{alias}");
        }
    }

    #[test]
    fn durable_thought_reuses_a_live_tail_with_a_different_identity() {
        let children = ModelRc::new(VecModel::from(vec![ProcessEntry {
            id: "reason-live-tail".into(),
            kind: crate::timeline::KIND_REASON,
            detail: "Fixture thought".into(),
            status_kind: crate::timeline::STATUS_RUNNING,
            ..Default::default()
        }]));
        let bar = TimelineRow {
            kind: crate::timeline::KIND_PROCESS,
            process_idx: 3,
            process_entries: children.clone(),
            ..Default::default()
        };
        merge_reasoning_tail(
            &bar,
            ProcessEntry {
                id: "reason-fixture:text-1".into(),
                kind: crate::timeline::KIND_REASON,
                detail: "Fixture thought".into(),
                status_kind: crate::timeline::STATUS_DONE,
                ..Default::default()
            },
        );
        assert_eq!(children.row_count(), 1);
        assert_eq!(children.row_data(0).unwrap().id, "reason-fixture:text-1");
    }

    #[test]
    fn canonical_thoughts_do_not_merge_by_equal_or_prefix_text() {
        for detail in ["Fixture repeated", "Fixture repeated completed"] {
            let children = ModelRc::new(VecModel::from(vec![ProcessEntry {
                id: "reason-fixture:text-1".into(),
                kind: crate::timeline::KIND_REASON,
                detail: "Fixture repeated completed".into(),
                status_kind: crate::timeline::STATUS_DONE,
                ..Default::default()
            }]));
            let bar = TimelineRow {
                kind: crate::timeline::KIND_PROCESS,
                process_entries: children.clone(),
                ..Default::default()
            };
            merge_reasoning_tail(
                &bar,
                ProcessEntry {
                    id: "reason-fixture:text-2".into(),
                    kind: crate::timeline::KIND_REASON,
                    detail: detail.into(),
                    status_kind: crate::timeline::STATUS_RUNNING,
                    ..Default::default()
                },
            );
            assert_eq!(children.row_count(), 2);
            assert_eq!(
                children.row_data(0).unwrap().detail,
                "Fixture repeated completed"
            );
            assert_eq!(
                children.row_data(0).unwrap().status_kind,
                crate::timeline::STATUS_DONE
            );
            merge_reasoning_tail(
                &bar,
                ProcessEntry {
                    id: "reason-fixture:text-2".into(),
                    kind: crate::timeline::KIND_REASON,
                    detail: "Fixture repeated completed continued".into(),
                    status_kind: crate::timeline::STATUS_RUNNING,
                    ..Default::default()
                },
            );
            assert_eq!(children.row_count(), 2);
        }
    }
}
