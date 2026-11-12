//! Presentation projection for the chat timeline: localized tool labels, the
//! target chip, the bounded patch cards handed to `patch_diff_card.slint`, and
//! the mapping from native workflow sections onto the detail model.
use crate::{PatchCard, PatchLine, ProcessEntry, TimelineRow, WorkflowLine, WorkflowSection};
use slint::{Model, ModelRc, VecModel};
use wunder_desktop::native::{
    NativePatchFile, NativePatchLine, NativeWorkflowEntry, NativeWorkflowSection,
};

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

/// The work-process bar's running activity line. Every built-in tool is named
/// as a verb phrase already (执行命令 / 读取文件 / 搜索内容), so the bar shows the
/// name alone: prefixing 执行工具： made the command tool read 执行工具：执行命令.
pub fn tool_running(tool: &str) -> String {
    tool_label(tool)
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
            return (target_text(&section.title), true);
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
    (target_text(lines.next().unwrap_or_default()), patch)
}

/// The call target as the chip acts on it. A path is never cut: a character
/// budget slices it mid-segment, which for the chip's file-name label reads as
/// a stray fragment ("…/tools/pa") and for opening the resource is plain
/// broken. Only a non-path value (a query, a one-line description) is bounded,
/// and that cut is marked so it cannot pass for the whole text.
fn target_text(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.contains(['/', '\\']) {
        return trimmed.to_string();
    }
    const TEXT_LIMIT: usize = 240;
    if trimmed.chars().count() <= TEXT_LIMIT {
        return trimmed.to_string();
    }
    let mut kept: String = trimmed.chars().take(TEXT_LIMIT).collect();
    kept.push('…');
    kept
}

pub fn tool_target(entry: &NativeWorkflowEntry) -> (String, bool) {
    let patch = is_patch_tool(&entry.title);
    if patch {
        // 补丁类工具优先用差异卡片的文件标题；写入文件在结果到达前还没有
        // 差异卡片，继续走下面的调用预览，避免折叠行的文件芯片中途消失。
        let (target, _) = target_of(&entry.title, "", &entry.sections);
        if !target.is_empty() {
            return (target, true);
        }
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

/// The last segment of a path-like value; the whole value when it carries no
/// separator. Used both for the chip label and for spotting a summary that
/// names the same file in another form.
fn file_name(value: &str) -> &str {
    let trimmed = value.trim().trim_end_matches(['/', '\\']);
    trimmed
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(trimmed)
        .trim()
}

/// The folded row prints only the file name of the call target. A full path
/// reads as noise beside the tool name and pushes the summary off a narrow
/// shell, so the chip keeps the directory out of sight while `target` keeps the
/// whole path for opening the resource and for the tooltip. A target without a
/// separator (a query, a URL) is left as it is.
pub fn target_label(target: &str) -> String {
    file_name(target).chars().take(64).collect()
}

/// The chip's text width (12px font, 7px padding each side) is 166px on a wide
/// row and 94px on a narrow one. Measured on the real UI font, a 12px ASCII
/// glyph averages ~6.2px and a CJK glyph is 12px, so this budget keeps the
/// whole name on screen instead of letting the last characters fall behind the
/// ellipsis. The name is whole or elided by the chip; it is never cut here.
const CHIP_TEXT_WIDTH: f32 = 166.0;
const CHIP_TEXT_WIDTH_NARROW: f32 = 94.0;
const CHIP_ELLIPSIS_WIDTH: f32 = 12.0;

fn glyph_width(ch: char) -> f32 {
    // CJK, Hangul, kana, fullwidth forms and astral ideographs are one em wide;
    // every other glyph of a file name (ASCII, Latin accents) is about half.
    let wide = matches!(
        ch as u32,
        0x1100..=0x115F
            | 0x2E80..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE4F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x20000..=0x2FFFD
    );
    if wide {
        12.0
    } else {
        6.2
    }
}

/// Shorten a chip label to what the chip can actually print. The chip keeps
/// the head of the name (the part that identifies the file) and marks the cut,
/// so a long name shows more characters than the renderer's own tail elide,
/// which only reserves room for the ellipsis and drops the rest.
pub fn fit_chip_label(label: &str, narrow: bool) -> String {
    let budget = if narrow {
        CHIP_TEXT_WIDTH_NARROW
    } else {
        CHIP_TEXT_WIDTH
    } - CHIP_ELLIPSIS_WIDTH;
    let total: f32 = label.chars().map(glyph_width).sum();
    if total <= budget {
        return label.to_string();
    }
    let mut used = 0.0;
    let mut kept = String::new();
    for ch in label.chars() {
        let width = glyph_width(ch);
        if used + width > budget {
            break;
        }
        used += width;
        kept.push(ch);
    }
    kept.push('…');
    kept
}

pub fn is_patch_tool(tool: &str) -> bool {
    matches!(
        tool,
        "apply_patch"
            | "应用补丁"
            | "write_file"
            | "写入文件"
            | "edit_file"
            | "edit_file2"
            | "文本编辑"
            | "text_edit"
            | "str_replace_editor"
            | "str_replace_based_edit_tool"
    )
}

/// Compact wall-clock label ("HH:MM") for a unix-seconds stamp; empty when the
/// stamp is unusable. The user bubble shows it beside the copy button.
pub fn clock_hhmm(secs: f64) -> String {
    if !secs.is_finite() || secs <= 0.0 {
        return String::new();
    }
    match chrono::DateTime::from_timestamp(secs as i64, 0) {
        Some(stamp) => stamp
            .with_timezone(&chrono::Local)
            .format("%H:%M")
            .to_string(),
        None => String::new(),
    }
}

/// The same label for "now", used by the provisional live user row until the
/// durable record replaces it.
pub fn clock_hhmm_now() -> String {
    clock_hhmm(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_secs_f64())
            .unwrap_or(0.0),
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

/// Collapsed-row change size of an edit / patch call: the added and deleted
/// line counts the cards already carry, summed over the cards' files. The row
/// only formats them; an empty string means "that side has no change", so the
/// header never shows a meaningless "+0 −0".
pub fn diff_stats(files: &[NativePatchFile]) -> (String, String) {
    let mut added = 0u64;
    let mut deleted = 0u64;
    for file in files {
        added = added.saturating_add(file.added.trim().parse::<u64>().unwrap_or(0));
        deleted = deleted.saturating_add(file.deleted.trim().parse::<u64>().unwrap_or(0));
    }
    format_diff(added, deleted)
}

/// The same numbers once the patch cards are built; the live stream updates
/// only the row's card model, so the row derives its counts from it.
fn card_diff(cards: &ModelRc<PatchCard>) -> (String, String) {
    let mut added = 0u64;
    let mut deleted = 0u64;
    for card in cards.iter() {
        added = added.saturating_add(card.added.trim().parse::<u64>().unwrap_or(0));
        deleted = deleted.saturating_add(card.deleted.trim().parse::<u64>().unwrap_or(0));
    }
    format_diff(added, deleted)
}

/// A failed or stopped call did not produce this change, so its row keeps no
/// change size — the pending preview must not read as a real diff.
fn change_size(row: &TimelineRow) -> (String, String) {
    if matches!(
        row.status_kind,
        crate::timeline::STATUS_FAILED | crate::timeline::STATUS_STOPPED
    ) {
        return (String::new(), String::new());
    }
    card_diff(&row.patch)
}

/// A side without change stays empty so the row shows only the number that
/// applies instead of a meaningless "+0 −0".
fn format_diff(added: u64, deleted: u64) -> (String, String) {
    (
        if added > 0 {
            added.to_string()
        } else {
            String::new()
        },
        if deleted > 0 {
            deleted.to_string()
        } else {
            String::new()
        },
    )
}

pub fn process_entry(row: &TimelineRow) -> ProcessEntry {
    process_entry_for(row, false)
}

/// Project one timeline row for the folded work-process row. `narrow` matches
/// the chip width the shell gives the row (a phone hands part of it to the
/// change size), so the label is shortened to what the chip can print.
pub fn process_entry_for(row: &TimelineRow, narrow: bool) -> ProcessEntry {
    let body = row.kind == crate::timeline::KIND_BODY;
    let (diff_add, diff_del) = change_size(row);
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
        target_label: fit_chip_label(&target_label(&row.target), narrow).into(),
        detail: if body {
            row.text.clone()
        } else {
            row.detail.clone()
        },
        sections: row.sections.clone(),
        patch: row.patch.clone(),
        diff_add: diff_add.into(),
        diff_del: diff_del.into(),
        lines_read: row.read_lines.clone(),
        round: row.model_round,
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
    let Some(children) = bar
        .process_entries
        .as_any()
        .downcast_ref::<VecModel<ProcessEntry>>()
    else {
        return;
    };
    // A thought that is still streaming is not part of the loaded page, so the
    // "a newer page exists" guard must not drop its updates: doing so froze the
    // active thought as soon as the fold outgrew its first page. The guard still
    // applies to a thought the page has not seen yet — inserting a live row
    // above an unloaded page would duplicate or misorder the durable history.
    let exists = children.iter().any(|old| old.id == entry.id);
    if bar.process_newer && !exists {
        return;
    }
    if let Some(index) = children.iter().position(|old| old.id == entry.id) {
        let old = children.row_data(index).unwrap();
        if old.status_kind != crate::timeline::STATUS_RUNNING
            && entry.status_kind == crate::timeline::STATUS_RUNNING
        {
            return;
        }
        entry.open = old.open;
        entry.payload = old.payload;
        // The live thought row and the durable page row can carry different
        // halves of the collapsing metadata; a merge must not blank the two
        // right-hand columns that one side already knows.
        if entry.tokens.is_empty() && entry.duration.is_empty() {
            entry.tokens = old.tokens.clone();
            entry.duration = old.duration.clone();
        }
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
        if entry.tokens.is_empty() && entry.duration.is_empty() {
            entry.tokens = old.tokens.clone();
            entry.duration = old.duration.clone();
        }
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
        // A live-only bar keeps one bounded tail. A paged bar has already
        // merged its pages into the same model, and trimming at a fixed index
        // would cut a loaded page in half.
        if !bar.process_paged && children.row_count() >= 201 {
            children.remove(200);
        }
        entry.payload = -1 - bar.process_idx * crate::chat_process::PROCESS_SLOT - children.row_count() as i32;
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
                            let payload = -1 - row.process_idx * crate::chat_process::PROCESS_SLOT - index as i32;
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
                content_width: patch_content_width(&file.lines),
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

/// Natural width of one diff card's line area. Touch shells pan this area
/// horizontally, so it covers the longest line instead of letting the row
/// elide it on the right.
fn patch_content_width(lines: &[NativePatchLine]) -> f32 {
    const GUTTER: f32 = 100.0;
    let longest = lines
        .iter()
        .take(200)
        .map(|line| {
            line.text
                .chars()
                .map(|ch| {
                    if ch == '\t' {
                        60.0
                    } else if is_wide_char(ch) {
                        12.5
                    } else {
                        7.5
                    }
                })
                .sum::<f32>()
        })
        .fold(0.0f32, f32::max);
    (GUTTER + longest).clamp(260.0, 3000.0)
}

/// CJK and emoji take roughly a full monospace cell; the rest about 0.6.
fn is_wide_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x1100..=0x115F
            | 0x2E80..=0x303E
            | 0x3041..=0x33FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xA000..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE6F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x1F300..=0x1FAFF
    )
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

/// The collapsed entry row places the target chip right next to the tool
/// name. Read/write-style calls preview exactly that target, so echoing the
/// preview as the summary would print the same file name twice on one row;
/// the row keeps the chip (it opens the resource) and leaves the outcome slot
/// empty. The folded batch bar has no chip and keeps reading `tool_summary`.
pub fn tool_row_summary(entry: &NativeWorkflowEntry, target: &str) -> String {
    let summary = tool_summary(entry);
    if is_same_target(target, &summary) {
        return String::new();
    }
    summary
}

/// Either side may be the bounded one: the chip clips long paths to 64 chars
/// while the preview keeps the whole path (or the preview is clipped first).
/// A prefix of the same path is the same resource and must not print twice.
/// The chip itself prints only the file name, so a summary that resolves to the
/// same file name in a differently folded form is the same resource as well.
fn is_same_target(target: &str, summary: &str) -> bool {
    let target = target.trim();
    let summary = summary.trim();
    if target.is_empty() || summary.is_empty() {
        return false;
    }
    let (long, short) = if target.len() >= summary.len() {
        (target, summary)
    } else {
        (summary, target)
    };
    if long.starts_with(short) {
        return true;
    }
    let (long_name, short_name) = (file_name(long), file_name(short));
    !long_name.is_empty() && long_name == short_name
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::Model;

    #[test]
    fn running_activity_names_the_tool_without_a_prefix() {
        // The bar used to print 执行工具：执行命令; the tool name is its own verb
        // phrase, so the prefix only repeated it.
        assert_eq!(tool_running("execute_command"), "执行命令");
        assert_eq!(tool_running("执行命令"), "执行命令");
        assert_eq!(tool_running("读取文件"), "读取文件");
        assert!(!tool_running(COMMAND_TOOL).contains("执行工具"));
        // An unmapped name still reads as itself rather than an empty chip.
        assert_eq!(tool_running("fixture_tool"), "fixture_tool");
    }

    #[test]
    fn folded_row_states_the_change_size_beside_the_file_chip() {
        let files = vec![
            NativePatchFile {
                added: "3".into(),
                deleted: "1".into(),
                ..Default::default()
            },
            NativePatchFile {
                added: "2".into(),
                deleted: "0".into(),
                ..Default::default()
            },
        ];
        assert_eq!(diff_stats(&files).0, "5");
        assert_eq!(diff_stats(&files).1, "1");
        // A tool with no file change keeps both sides empty, so the row never
        // prints "+0 −0".
        assert_eq!(diff_stats(&[]), (String::new(), String::new()));
        let row = TimelineRow {
            id: "fixture-edit".into(),
            kind: crate::timeline::KIND_TOOL,
            patch: patch_cards(&files),
            ..Default::default()
        };
        let entry = process_entry(&row);
        assert_eq!(entry.diff_add.as_str(), "5");
        assert_eq!(entry.diff_del.as_str(), "1");
        // A failed call keeps its pending preview card but never reads as a
        // real change.
        let failed = TimelineRow {
            status_kind: crate::timeline::STATUS_FAILED,
            ..row
        };
        let entry = process_entry(&failed);
        assert!(entry.diff_add.is_empty() && entry.diff_del.is_empty());
    }

    #[test]
    fn a_deep_path_target_keeps_the_file_name_whole() {
        // A 64-character budget used to slice this path mid-segment, so the
        // chip printed the fragment "pa" instead of "panel_tools.rs".
        let path = "/home/fixture/wunder/crates/wunder-runtime/src/services/tools/panel_tools.rs";
        assert!(path.chars().count() > 64);
        let section = NativeWorkflowSection {
            title: path.into(),
            kind: "patch".into(),
            ..Default::default()
        };
        let (target, patch) = target_of("文本编辑", "", &[section]);
        assert!(patch);
        assert_eq!(target, path);
        assert_eq!(target_label(&target), "panel_tools.rs");
        // A non-path target (a query) is still bounded, and says so.
        let long = "查".repeat(300);
        let (target, _) = target_of("搜索内容", &format!("搜索 · 完成\n{long}"), &[]);
        assert!(target.ends_with('…'), "{}", target.len());
        assert_eq!(target.chars().count(), 241);
    }

    #[test]
    fn chip_label_fits_the_printed_width_instead_of_the_renderer_eliding_it() {
        // A short name is printed whole.
        assert_eq!(fit_chip_label("main.rs", false), "main.rs");
        // A 40-character name fits the 166px chip (40 * 6.2px ≈ 248px would
        // not): it is cut to the head plus an ellipsis, so the tail is not
        // silently dropped by the renderer.
        let long = "desktop_timeline_target_label_fixture_test.rs";
        let fitted = fit_chip_label(long, false);
        assert!(fitted.ends_with('…'), "{fitted}");
        assert!(fitted.chars().count() > 20, "{fitted}");
        assert!(long.starts_with(fitted.trim_end_matches('…')));
        // A CJK name is measured one em per glyph, so far fewer fit.
        let cjk = "工作过程工具行目标芯片的长文件名验证.txt";
        let fitted = fit_chip_label(cjk, false);
        assert!(fitted.ends_with('…'), "{fitted}");
        assert!(fitted.chars().count() <= 14, "{fitted}");
        // A phone row hands part of the chip to the change size.
        assert!(fit_chip_label(long, true).chars().count() < fit_chip_label(long, false).chars().count());
    }

    #[test]
    fn folded_row_chip_prints_only_the_file_name() {
        assert_eq!(target_label("/home/fixture/project/src/main.rs"), "main.rs");
        assert_eq!(target_label("fixture\\project\\main.rs"), "main.rs");
        assert_eq!(target_label("main.rs"), "main.rs");
        // A directory target (a list call) keeps its last segment.
        assert_eq!(target_label("fixture/project/"), "project");
        assert_eq!(target_label("   "), "");
        // A target without a separator is not a path and stays as it is.
        assert_eq!(target_label("fixture query"), "fixture query");
        let row = TimelineRow {
            id: "fixture-read".into(),
            kind: crate::timeline::KIND_TOOL,
            target: "/home/fixture/project/src/main.rs".into(),
            ..Default::default()
        };
        let entry = process_entry(&row);
        // The chip prints the name; opening the chip and the tooltip keep the
        // whole path.
        assert_eq!(entry.target_label.as_str(), "main.rs");
        assert_eq!(entry.target.as_str(), "/home/fixture/project/src/main.rs");
    }

    #[test]
    fn write_file_row_keeps_the_call_preview_until_the_patch_section_exists() {
        let mut entry = NativeWorkflowEntry {
            id: "fixture-write".into(),
            title: "write_file".into(),
            preview: "fixture/sample.txt".into(),
            detail: String::new(),
            state: "running".into(),
            tokens: String::new(),
            duration: String::new(),
            read_lines: String::new(),
            sections: Vec::new(),
            patch_files: Vec::new(),
            round: 0,
        };
        assert_eq!(tool_target(&entry), ("fixture/sample.txt".into(), false));
        assert!(is_patch_tool(&entry.title));
        entry.sections = vec![NativeWorkflowSection {
            title: "fixture/sample.txt".into(),
            kind: "patch".into(),
            ..Default::default()
        }];
        assert_eq!(tool_target(&entry), ("fixture/sample.txt".into(), true));
    }

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
            read_lines: String::new(),
            sections: Vec::new(),
            patch_files: Vec::new(),
            round: 0,
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
    fn row_summary_drops_the_target_the_chip_already_shows() {
        let mut entry = NativeWorkflowEntry {
            id: "fixture-read".into(),
            title: "read_file".into(),
            preview: "fixture/sample.txt".into(),
            detail: String::new(),
            state: "completed".into(),
            tokens: String::new(),
            duration: String::new(),
            read_lines: String::new(),
            sections: Vec::new(),
            patch_files: Vec::new(),
            round: 0,
        };
        let (target, _) = tool_target(&entry);
        assert_eq!(target, "fixture/sample.txt");
        assert_eq!(tool_row_summary(&entry, &target), "");
        // A truncated chip still counts as the same target.
        entry.preview = "a".repeat(200);
        let (target, _) = tool_target(&entry);
        assert_eq!(tool_row_summary(&entry, &target), "");
        // A long path reaches the row whole: clipping it here turned the chip's
        // file name into a fragment ("…/tools/pa") and left open-resource with
        // a path that cannot be opened. The chip shortens only what it prints.
        let long = format!("fixture/{}.rs", "a".repeat(90));
        entry.title = "文本编辑".into();
        entry.preview = long.clone();
        entry.sections = vec![NativeWorkflowSection {
            title: long.clone(),
            kind: "patch".into(),
            ..Default::default()
        }];
        let (target, _) = tool_target(&entry);
        assert_eq!(target, long);
        assert_eq!(tool_row_summary(&entry, &target), "");
        // An unrelated summary is kept.
        entry.title = "read_file".into();
        entry.sections.clear();
        entry.preview = "fixture query".into();
        assert_eq!(tool_row_summary(&entry, "other.txt"), "fixture query");
    }

    #[test]
    fn row_summary_treats_a_folded_path_as_the_same_file() {
        // The chip prints only the file name, so a summary that folds the same
        // path another way must not print the file name a second time.
        let mut entry = NativeWorkflowEntry {
            id: "fixture-edit".into(),
            title: "文本编辑".into(),
            preview: ".../src/lib.rs".into(),
            detail: String::new(),
            state: "completed".into(),
            tokens: String::new(),
            duration: String::new(),
            read_lines: String::new(),
            sections: Vec::new(),
            patch_files: Vec::new(),
            round: 0,
        };
        assert_eq!(tool_row_summary(&entry, "crates/engine/src/lib.rs"), "");
        // A different file keeps its summary.
        entry.preview = "src/other.rs".into();
        assert_eq!(tool_row_summary(&entry, "src/lib.rs"), "src/other.rs");
    }

    #[test]
    fn read_row_carries_the_returned_line_count() {
        let row = TimelineRow {
            id: "fixture-read".into(),
            kind: crate::timeline::KIND_TOOL,
            read_lines: "142 行".into(),
            ..Default::default()
        };
        assert_eq!(process_entry(&row).lines_read.as_str(), "142 行");
        assert!(process_entry(&TimelineRow::default()).lines_read.is_empty());
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
