use super::{
    build_model_tool_success, build_model_tool_success_with_hint,
    collect_orchestration_aware_allow_roots, collect_read_roots,
    command_options::parse_dry_run,
    execute_in_sandbox, read_file_guard, read_image_tool, read_indentation,
    recover_tool_args_value, resolve_tool_path, roots_allow_any_path,
    tool_error::{build_failed_tool_result, ToolErrorMeta},
    touch_lsp_file, ToolContext, DEFAULT_LIST_DEPTH, DEFAULT_LIST_PAGE_LIMIT,
    DEFAULT_START_LINE_WINDOW, MAX_LIST_ITEMS, MAX_RANGE_SPAN, MAX_READ_BUDGET_FILES,
    MAX_READ_BYTES, MAX_READ_LINES, MAX_READ_OUTPUT_BUDGET_BYTES, MAX_READ_TIME_BUDGET_MS,
    MIN_READ_OUTPUT_BUDGET_BYTES,
};
use crate::core::atomic_write::atomic_write_text;
use crate::i18n;
use crate::path_utils::{is_within_root, normalize_path_for_compare, normalize_target_path};
use crate::workspace::WorkspaceManager;
use anyhow::{anyhow, Result};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use walkdir::WalkDir;
pub(crate) async fn list_files(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    if let Some(result) = execute_in_sandbox(context, "列出文件", args).await {
        return Ok(result);
    }
    let raw_path = args
        .get("path")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(".");
    let max_depth = args
        .get("max_depth")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_LIST_DEPTH as u64) as usize;
    let pagination = match parse_list_files_pagination(args) {
        Ok(value) => value,
        Err(err) => {
            return Ok(build_failed_tool_result(
                err.to_string(),
                json!({}),
                ToolErrorMeta::new(
                    "TOOL_LIST_INVALID_ARGS",
                    Some(
                        "Use cursor/offset as non-negative integers and limit within 1..500."
                            .to_string(),
                    ),
                    false,
                    None,
                ),
                false,
            ));
        }
    };
    let workspace = context.workspace.clone();
    let user_id = context.workspace_id.to_string();
    let extra_roots = collect_read_roots(context);
    let path = raw_path.to_string();
    tokio::task::spawn_blocking(move || {
        list_files_inner(
            workspace.as_ref(),
            &user_id,
            &path,
            &extra_roots,
            max_depth,
            pagination.start,
            pagination.limit,
        )
    })
    .await
    .map_err(|err| anyhow!(err.to_string()))?
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ListFilesPagination {
    pub(crate) start: usize,
    pub(crate) limit: usize,
}

pub(crate) fn parse_list_files_pagination(args: &Value) -> Result<ListFilesPagination> {
    let start = if let Some(cursor) = args.get("cursor") {
        parse_list_cursor_value(cursor)?
    } else if let Some(offset) = args.get("offset") {
        parse_list_offset_value(offset)?
    } else {
        0
    };
    let limit = if let Some(limit) = args.get("limit") {
        parse_list_limit_value(limit)?
    } else {
        DEFAULT_LIST_PAGE_LIMIT
    };
    Ok(ListFilesPagination {
        start,
        limit: limit.clamp(1, MAX_LIST_ITEMS),
    })
}

fn parse_list_cursor_value(value: &Value) -> Result<usize> {
    match value {
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return Ok(0);
            }
            trimmed
                .parse::<usize>()
                .map_err(|_| anyhow!("cursor must be a non-negative integer string"))
        }
        Value::Number(number) => number
            .as_u64()
            .map(|raw| raw as usize)
            .ok_or_else(|| anyhow!("cursor must be a non-negative integer string")),
        Value::Null => Ok(0),
        _ => Err(anyhow!("cursor must be a non-negative integer string")),
    }
}

fn parse_list_offset_value(value: &Value) -> Result<usize> {
    match value {
        Value::Number(number) => number
            .as_u64()
            .map(|raw| raw as usize)
            .ok_or_else(|| anyhow!("offset must be a non-negative integer")),
        Value::String(text) => text
            .trim()
            .parse::<usize>()
            .map_err(|_| anyhow!("offset must be a non-negative integer")),
        Value::Null => Ok(0),
        _ => Err(anyhow!("offset must be a non-negative integer")),
    }
}

fn parse_list_limit_value(value: &Value) -> Result<usize> {
    let parsed = match value {
        Value::Number(number) => number
            .as_u64()
            .map(|raw| raw as usize)
            .ok_or_else(|| anyhow!("limit must be a positive integer"))?,
        Value::String(text) => text
            .trim()
            .parse::<usize>()
            .map_err(|_| anyhow!("limit must be a positive integer"))?,
        Value::Null => DEFAULT_LIST_PAGE_LIMIT,
        _ => return Err(anyhow!("limit must be a positive integer")),
    };
    Ok(parsed.max(1).min(MAX_LIST_ITEMS))
}

pub(crate) fn list_files_inner(
    workspace: &WorkspaceManager,
    user_id: &str,
    path: &str,
    extra_roots: &[PathBuf],
    max_depth: usize,
    page_start: usize,
    page_limit: usize,
) -> Result<Value> {
    let root = resolve_tool_path(workspace, user_id, path, extra_roots)?;
    if !root.exists() {
        return Ok(build_failed_tool_result(
            i18n::t("tool.list.path_not_found"),
            json!({ "path": path }),
            ToolErrorMeta::new(
                "TOOL_LIST_PATH_NOT_FOUND",
                Some(
                    "Use a directory path that exists under the current workspace or allowed roots."
                        .to_string(),
                ),
                false,
                None,
            ),
            false,
        ));
    }
    let mut items = Vec::new();
    let mut seen_entries: usize = 0;
    let mut has_more = false;
    let unrestricted_paths = roots_allow_any_path(extra_roots);
    let _ = unrestricted_paths;
    for entry in WalkDir::new(&root)
        .min_depth(1)
        .max_depth(max_depth.saturating_add(1))
        .into_iter()
        .filter_map(|item| item.ok())
    {
        let rel = entry.path().strip_prefix(&root).unwrap_or(entry.path());
        let mut display = rel.to_string_lossy().replace('\\', "/");
        if entry.file_type().is_dir() {
            display.push('/');
        }
        if seen_entries < page_start {
            seen_entries += 1;
            continue;
        }
        if items.len() >= page_limit {
            has_more = true;
            break;
        }
        items.push(display);
        seen_entries += 1;
    }
    let returned = items.len();
    let next_offset = page_start.saturating_add(returned);
    let next_cursor = has_more.then(|| next_offset.to_string());
    let resolved_path = root.to_string_lossy().to_string();
    Ok(build_model_tool_success_with_hint(
        "list_files",
        "completed",
        format!("Listed {returned} entries from {path}."),
        json!({
            "path": path,
            "resolved_path": resolved_path,
            "items": items,
            "offset": page_start,
            "limit": page_limit,
            "returned": returned,
            "has_more": has_more,
            "next_offset": has_more.then_some(next_offset),
            "next_cursor": next_cursor,
            "max_depth": max_depth,
        }),
        has_more.then(|| {
            "More entries are available. Reuse next_cursor to continue listing the same directory."
                .to_string()
        }),
    ))
}

#[derive(Clone, Debug)]
pub(crate) struct ReadFileSpec {
    pub(crate) path: String,
    pub(crate) requested_ranges: Vec<(usize, usize)>,
    pub(crate) ranges: Vec<(usize, usize)>,
    pub(crate) used_default_range: bool,
    pub(crate) mode: ReadFileMode,
    pub(crate) indentation: read_indentation::IndentationReadOptions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReadFileMode {
    Slice,
    Indentation,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ReadBudget {
    pub(crate) time_budget_ms: Option<u64>,
    pub(crate) output_budget_bytes: Option<usize>,
    pub(crate) max_files: Option<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReadFailureKind {
    PathInvalid,
    NotFound,
    Binary,
}

#[derive(Clone, Debug)]
struct ReadFailure {
    kind: ReadFailureKind,
}

#[derive(Clone, Debug)]
pub(crate) struct ReadSpecParseError {
    pub(crate) code: &'static str,
    pub(crate) message: String,
    pub(crate) hint: Option<String>,
    pub(crate) data: Value,
}

impl ReadBudget {
    fn to_json(self) -> Value {
        json!({
            "time_budget_ms": self.time_budget_ms,
            "output_budget_bytes": self.output_budget_bytes,
            "max_files": self.max_files,
        })
    }
}

impl ReadSpecParseError {
    fn invalid_args(message: String) -> Self {
        Self {
            code: "TOOL_READ_INVALID_ARGS",
            message,
            hint: Some("请检查 files/path/line_ranges/mode/budget 参数格式。".to_string()),
            data: json!({}),
        }
    }

    fn reversed_range(start: usize, end: usize) -> Self {
        let params = HashMap::from([
            ("start".to_string(), start.to_string()),
            ("end".to_string(), end.to_string()),
        ]);
        Self {
            code: "TOOL_READ_INVALID_RANGE",
            message: i18n::t_with_params("tool.read.invalid_reversed_range", &params),
            hint: Some(i18n::t("tool.read.invalid_reversed_range_hint")),
            data: json!({
                "kind": "reversed_line_range",
                "start_line": start,
                "end_line": end,
            }),
        }
    }

    fn too_many_files(count: usize, max: usize) -> Self {
        let params = HashMap::from([
            ("count".to_string(), count.to_string()),
            ("max".to_string(), max.to_string()),
        ]);
        Self {
            code: "TOOL_READ_TOO_MANY_FILES",
            message: i18n::t_with_params("tool.read.too_many_files", &params),
            hint: Some(i18n::t("tool.read.too_many_files_hint")),
            data: json!({
                "kind": "too_many_files",
                "count": count,
                "max_files": max,
            }),
        }
    }
}

pub(crate) fn parse_read_file_specs(
    args: &Value,
) -> std::result::Result<Vec<ReadFileSpec>, ReadSpecParseError> {
    let mut specs = Vec::new();

    if let Some(files) = args.get("files").and_then(Value::as_array) {
        if files.len() > MAX_READ_BUDGET_FILES {
            return Err(ReadSpecParseError::too_many_files(
                files.len(),
                MAX_READ_BUDGET_FILES,
            ));
        }
        for file in files {
            let Some(obj) = file.as_object() else {
                continue;
            };
            if let Some(spec) = parse_read_file_spec_object(obj)? {
                specs.push(spec);
            }
        }
    }

    if specs.is_empty() {
        if let Some(obj) = args.as_object() {
            if let Some(spec) = parse_read_file_spec_object(obj)? {
                specs.push(spec);
            }
        } else if let Some(path) = args
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            specs.push(ReadFileSpec {
                path: path.to_string(),
                requested_ranges: vec![(1, MAX_READ_LINES)],
                ranges: vec![(1, MAX_READ_LINES)],
                used_default_range: true,
                mode: ReadFileMode::Slice,
                indentation: read_indentation::IndentationReadOptions::default(),
            });
        }
    }

    if specs.is_empty() {
        return Err(ReadSpecParseError::invalid_args(i18n::t(
            "tool.read.no_path",
        )));
    }
    Ok(coalesce_read_specs(specs))
}

fn parse_read_file_spec_object(
    obj: &serde_json::Map<String, Value>,
) -> std::result::Result<Option<ReadFileSpec>, ReadSpecParseError> {
    let path = normalize_read_path_hint(
        obj.get("path")
            .or_else(|| obj.get("file_path"))
            .or_else(|| obj.get("file"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string(),
    );
    if path.is_empty() {
        return Ok(None);
    }

    let mut requested_ranges = Vec::new();
    let mut ranges = Vec::new();
    if let Some(Value::Array(items)) = obj.get("line_ranges") {
        for item in items {
            let Some(pair) = item.as_array() else {
                continue;
            };
            if pair.len() < 2 {
                continue;
            }
            let Some(start) = pair.first().and_then(parse_line_number) else {
                continue;
            };
            let Some(end) = pair.get(1).and_then(parse_line_number) else {
                continue;
            };
            validate_line_range_order(start, end)?;
            requested_ranges.push((start, end));
            ranges.push(normalize_range(start, end));
        }
    }

    if let Some(start) = obj.get("start_line").and_then(parse_line_number) {
        let end = obj
            .get("end_line")
            .and_then(parse_line_number)
            .unwrap_or_else(|| start.saturating_add(DEFAULT_START_LINE_WINDOW.saturating_sub(1)));
        validate_line_range_order(start, end)?;
        requested_ranges.push((start, end));
        ranges.push(normalize_range(start, end));
    }
    if let Some(offset) = obj.get("offset").and_then(parse_line_number) {
        let limit = obj
            .get("limit")
            .and_then(parse_line_number)
            .unwrap_or(MAX_READ_LINES)
            .max(1);
        let end = offset.saturating_add(limit.saturating_sub(1));
        requested_ranges.push((offset, end));
        ranges.push(normalize_range(offset, end));
    }

    let used_default_range = ranges.is_empty();
    if ranges.is_empty() {
        requested_ranges.push((1, MAX_READ_LINES));
        ranges.push((1, MAX_READ_LINES));
    }
    ranges = merge_read_ranges(ranges);
    let mode = parse_read_mode(obj);
    let mut indentation = parse_indentation_options(obj);
    if indentation.anchor_line.is_none() {
        indentation.anchor_line = ranges.first().map(|(start, _)| *start);
    }
    Ok(Some(ReadFileSpec {
        path,
        requested_ranges,
        ranges,
        used_default_range,
        mode,
        indentation,
    }))
}

fn parse_read_mode(obj: &serde_json::Map<String, Value>) -> ReadFileMode {
    let raw = obj
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("slice")
        .trim()
        .to_ascii_lowercase();
    match raw.as_str() {
        "indentation" | "indent" | "block" => ReadFileMode::Indentation,
        _ => ReadFileMode::Slice,
    }
}

fn parse_indentation_options(
    obj: &serde_json::Map<String, Value>,
) -> read_indentation::IndentationReadOptions {
    let mut options = read_indentation::IndentationReadOptions::default();
    let Some(indentation) = obj.get("indentation").and_then(Value::as_object) else {
        return options;
    };
    options.anchor_line = indentation.get("anchor_line").and_then(parse_line_number);
    options.max_levels = indentation
        .get("max_levels")
        .and_then(parse_line_number)
        .unwrap_or(0);
    options.include_siblings = indentation
        .get("include_siblings")
        .and_then(Value::as_bool)
        .unwrap_or(options.include_siblings);
    options.include_header = indentation
        .get("include_header")
        .and_then(Value::as_bool)
        .unwrap_or(options.include_header);
    options.max_lines = indentation.get("max_lines").and_then(parse_line_number);
    options
}

pub(crate) fn parse_read_budget(args: &Value) -> ReadBudget {
    let Some(obj) = args.as_object() else {
        return ReadBudget::default();
    };
    let budget_obj = obj.get("budget").and_then(Value::as_object);
    let time_budget_ms = budget_obj
        .and_then(|value| value.get("time_budget_ms"))
        .or_else(|| obj.get("time_budget_ms"))
        .and_then(parse_optional_positive_u64)
        .map(|value| value.clamp(1, MAX_READ_TIME_BUDGET_MS));
    let output_budget_bytes = budget_obj
        .and_then(|value| value.get("output_budget_bytes"))
        .or_else(|| obj.get("output_budget_bytes"))
        .and_then(parse_optional_positive_usize)
        .map(|value| value.clamp(MIN_READ_OUTPUT_BUDGET_BYTES, MAX_READ_OUTPUT_BUDGET_BYTES));
    let max_files = budget_obj
        .and_then(|value| value.get("max_files"))
        .or_else(|| obj.get("max_files"))
        .and_then(parse_optional_positive_usize)
        .map(|value| value.clamp(1, MAX_READ_BUDGET_FILES));
    ReadBudget {
        time_budget_ms,
        output_budget_bytes,
        max_files,
    }
}

fn parse_optional_positive_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.trim().parse::<u64>().ok(),
        _ => None,
    }
    .filter(|value| *value > 0)
}

fn parse_optional_positive_usize(value: &Value) -> Option<usize> {
    parse_optional_positive_u64(value).map(|value| value as usize)
}

fn normalize_read_path_hint(path: String) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    trimmed.to_string()
}

pub(crate) fn normalize_read_path_for_workspace(raw_path: &str, workspace_id: &str) -> String {
    let _ = workspace_id;
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let normalized = trimmed.replace('\\', "/");

    for prefix in ["/workspaces/", "workspaces/"] {
        if let Some(value) = normalized.strip_prefix(prefix) {
            let candidate = value.trim_matches('/').trim();
            if candidate.is_empty() {
                return String::new();
            }
            return format!("/workspaces/{candidate}");
        }
    }

    for prefix in ["/workspace/", "workspace/"] {
        if let Some(value) = normalized.strip_prefix(prefix) {
            return value.trim_matches('/').trim().to_string();
        }
    }

    trimmed.to_string()
}

fn parse_line_number(value: &Value) -> Option<usize> {
    if let Some(num) = value.as_u64() {
        return Some(num as usize);
    }
    if let Some(num) = value.as_i64() {
        if num >= 0 {
            return Some(num as usize);
        }
    }
    if let Some(num) = value.as_f64() {
        if num >= 0.0 {
            return Some(num as usize);
        }
    }
    if let Some(text) = value.as_str() {
        if let Ok(num) = text.trim().parse::<usize>() {
            return Some(num);
        }
    }
    None
}

fn normalize_range(start: usize, end: usize) -> (usize, usize) {
    let start = start.max(1);
    let end = end.max(start);
    if end - start + 1 > MAX_RANGE_SPAN {
        return (start, start + MAX_RANGE_SPAN - 1);
    }
    (start, end)
}

fn validate_line_range_order(
    start: usize,
    end: usize,
) -> std::result::Result<(), ReadSpecParseError> {
    if end < start {
        return Err(ReadSpecParseError::reversed_range(start, end));
    }
    Ok(())
}

fn merge_read_ranges(mut ranges: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    if ranges.len() <= 1 {
        return ranges;
    }
    ranges.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(ranges.len());
    for (start, end) in ranges {
        if let Some((_, last_end)) = merged.last_mut() {
            if start <= last_end.saturating_add(1) {
                *last_end = (*last_end).max(end);
                continue;
            }
        }
        merged.push((start, end));
    }
    merged
}

fn can_merge_read_specs(left: &ReadFileSpec, right: &ReadFileSpec) -> bool {
    left.path == right.path
        && left.mode == right.mode
        && match left.mode {
            ReadFileMode::Slice => true,
            ReadFileMode::Indentation => left.indentation == right.indentation,
        }
}

fn coalesce_read_specs(specs: Vec<ReadFileSpec>) -> Vec<ReadFileSpec> {
    let mut merged = Vec::with_capacity(specs.len());
    for mut spec in specs {
        spec.ranges = merge_read_ranges(spec.ranges);
        if let Some(last) = merged.last_mut() {
            if can_merge_read_specs(last, &spec) {
                last.requested_ranges.extend(spec.requested_ranges);
                last.ranges.extend(spec.ranges);
                last.ranges = merge_read_ranges(std::mem::take(&mut last.ranges));
                last.used_default_range &= spec.used_default_range;
                continue;
            }
        }
        merged.push(spec);
    }
    merged
}

fn summarize_read_ranges(ranges: &[(usize, usize)], total_lines: usize) -> (usize, bool) {
    if total_lines == 0 {
        return (0, true);
    }
    if ranges.is_empty() {
        return (0, false);
    }
    let mut intervals = Vec::with_capacity(ranges.len());
    for (start, end) in ranges {
        let start = (*start).max(1);
        if start > total_lines {
            continue;
        }
        let end = (*end).min(total_lines).max(start);
        intervals.push((start, end));
    }
    if intervals.is_empty() {
        return (0, false);
    }
    intervals.sort_by_key(|(start, _)| *start);
    let mut read_lines = 0usize;
    let mut current = intervals[0];
    for (start, end) in intervals.into_iter().skip(1) {
        if start <= current.1 + 1 {
            current.1 = current.1.max(end);
        } else {
            read_lines += current.1 - current.0 + 1;
            current = (start, end);
        }
    }
    read_lines += current.1 - current.0 + 1;
    let complete = read_lines == total_lines;
    (read_lines, complete)
}

pub(crate) fn summarize_slice_eof(ranges: &[(usize, usize)], total_lines: usize) -> (bool, bool) {
    if total_lines == 0 || ranges.is_empty() {
        return (false, false);
    }
    let mut hit_eof = false;
    let mut range_reaches_eof = false;
    for (start, end) in ranges {
        if *start > total_lines {
            hit_eof = true;
            continue;
        }
        if *end >= total_lines {
            hit_eof = true;
            range_reaches_eof = true;
        }
    }
    (hit_eof, range_reaches_eof)
}

fn slice_request_satisfied(ranges: &[(usize, usize)], total_lines: usize) -> bool {
    if ranges.is_empty() {
        return total_lines == 0;
    }
    ranges
        .iter()
        .all(|(start, end)| *start <= total_lines && *end <= total_lines)
}

fn summary_requires_read_continuation(summary: &Value) -> bool {
    let Some(obj) = summary.as_object() else {
        return false;
    };
    if obj
        .get("truncated_by_size")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return true;
    }
    let used_default_range = obj
        .get("used_default_range")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let read_lines = obj.get("read_lines").and_then(Value::as_u64).unwrap_or(0);
    let total_lines = obj.get("total_lines").and_then(Value::as_u64).unwrap_or(0);
    used_default_range && read_lines > 0 && total_lines > read_lines
}

fn compact_read_file_summary_for_model(summary: &Value) -> Value {
    json!({
        "path": summary.get("path").cloned().unwrap_or(Value::Null),
        "mode": summary.get("mode").cloned().unwrap_or(Value::Null),
        "requested_ranges": summary.get("requested_ranges").cloned().unwrap_or(Value::Null),
        "effective_ranges": summary.get("effective_ranges").cloned().unwrap_or(Value::Null),
        "used_default_range": summary.get("used_default_range").cloned().unwrap_or(Value::Null),
        "exists": summary.get("exists").cloned().unwrap_or(Value::Null),
        "binary": summary.get("binary").cloned().unwrap_or(Value::Null),
        "mime_type": summary.get("mime_type").cloned().unwrap_or(Value::Null),
        "size_bytes": summary.get("size_bytes").cloned().unwrap_or(Value::Null),
        "read_lines": summary.get("read_lines").cloned().unwrap_or(Value::Null),
        "total_lines": summary.get("total_lines").cloned().unwrap_or(Value::Null),
        "request_satisfied": summary.get("request_satisfied").cloned().unwrap_or(Value::Null),
        "complete": summary.get("complete").cloned().unwrap_or(Value::Null),
        "truncated_by_size": summary.get("truncated_by_size").cloned().unwrap_or(Value::Null),
        "error": summary.get("error").cloned().unwrap_or(Value::Null),
    })
}

pub(crate) async fn read_files(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let args = recover_tool_args_value(args);
    if let Some(result) = execute_in_sandbox(context, "读取文件", &args).await {
        return Ok(result);
    }
    let dry_run = parse_dry_run(&args);
    let read_budget = parse_read_budget(&args);
    let mut specs = match parse_read_file_specs(&args) {
        Ok(specs) => specs,
        Err(err) => {
            return Ok(build_failed_tool_result(
                err.message,
                err.data,
                ToolErrorMeta::new(err.code, err.hint, false, None),
                false,
            ));
        }
    };
    let user_id = context.workspace_id.to_string();
    for spec in &mut specs {
        spec.path = normalize_read_path_for_workspace(&spec.path, &user_id);
    }
    let requested_files = specs.len();
    specs = coalesce_read_specs(specs);
    let mut budget_file_limit_hit = false;
    if let Some(max_files) = read_budget.max_files {
        if specs.len() > max_files {
            specs.truncate(max_files);
            budget_file_limit_hit = true;
        }
    }

    let specs_for_lsp = specs.clone();
    let workspace = context.workspace.clone();
    let extra_roots = collect_read_roots(context);
    let budget_for_task = read_budget;
    let result = tokio::task::spawn_blocking(move || {
        read_files_inner(
            workspace.as_ref(),
            &user_id,
            &extra_roots,
            specs,
            budget_for_task,
            dry_run,
            requested_files,
            budget_file_limit_hit,
        )
    })
    .await
    .map_err(|err| anyhow!(err.to_string()))?;
    if result.is_ok() && context.config.lsp.enabled && !dry_run {
        for spec in specs_for_lsp {
            if let Ok(target) = context
                .workspace
                .resolve_path(context.workspace_id, &spec.path)
            {
                let _ = touch_lsp_file(context, &target, false).await;
            }
        }
    }
    result
}

pub(crate) fn read_files_inner(
    workspace: &WorkspaceManager,
    user_id: &str,
    extra_roots: &[PathBuf],
    specs: Vec<ReadFileSpec>,
    budget: ReadBudget,
    dry_run: bool,
    requested_files: usize,
    budget_file_limit_hit: bool,
) -> Result<Value> {
    let started_at = Instant::now();
    let mut outputs = Vec::new();
    let mut summaries = Vec::new();
    let mut failures = Vec::new();
    let mut successful_reads = 0usize;
    let mut timeout_hit = false;
    let mut output_budget_hit = false;
    let mut output_budget_omitted_bytes = 0usize;
    for spec in specs {
        if let Some(limit_ms) = budget.time_budget_ms {
            if started_at.elapsed() >= Duration::from_millis(limit_ms) {
                timeout_hit = true;
                break;
            }
        }
        let raw_path = spec.path.as_str();
        let requested_ranges = spec.requested_ranges.clone();
        let effective_ranges = spec.ranges.clone();
        let range_args_normalized = requested_ranges != effective_ranges;
        let mut summary = json!({
            "path": raw_path,
            "requested_ranges": requested_ranges,
            "effective_ranges": effective_ranges,
            "range_args_normalized": range_args_normalized,
            "used_default_range": spec.used_default_range,
            "read_lines": 0,
            "total_lines": 0,
            "complete": false,
            "dry_run": dry_run
        });
        let target = match resolve_tool_path(workspace, user_id, raw_path, extra_roots) {
            Ok(path) => Some(path),
            Err(err) => {
                let message = err.to_string();
                outputs.push(format!(">>> {}\n{}", raw_path, message));
                failures.push(ReadFailure {
                    kind: ReadFailureKind::PathInvalid,
                });
                if let Value::Object(ref mut map) = summary {
                    map.insert("error".to_string(), Value::String(message));
                }
                None
            }
        };
        let Some(target) = target else {
            summaries.push(summary);
            continue;
        };
        if let Value::Object(ref mut map) = summary {
            map.insert(
                "resolved_path".to_string(),
                Value::String(target.to_string_lossy().to_string()),
            );
        }
        if !target.exists() {
            let message = i18n::t("tool.read.not_found");
            outputs.push(format!(">>> {}\n{}", raw_path, message));
            failures.push(ReadFailure {
                kind: ReadFailureKind::NotFound,
            });
            if let Value::Object(ref mut map) = summary {
                map.insert("exists".to_string(), Value::Bool(false));
                map.insert("error".to_string(), Value::String(message));
            }
            summaries.push(summary);
            continue;
        }
        let size = target.metadata().map(|meta| meta.len()).unwrap_or(0);
        if dry_run {
            if let Value::Object(ref mut map) = summary {
                map.insert("exists".to_string(), Value::Bool(true));
                map.insert("size_bytes".to_string(), Value::from(size));
                map.insert(
                    "mode".to_string(),
                    Value::String(match spec.mode {
                        ReadFileMode::Slice => "slice".to_string(),
                        ReadFileMode::Indentation => "indentation".to_string(),
                    }),
                );
            }
            outputs.push(format!(
                ">>> {}\n[dry_run] exists=true size={} bytes mode={}",
                raw_path,
                size,
                match spec.mode {
                    ReadFileMode::Slice => "slice",
                    ReadFileMode::Indentation => "indentation",
                }
            ));
            successful_reads += 1;
            summaries.push(summary);
            continue;
        }
        let source_truncated_by_size = size > MAX_READ_BYTES as u64;
        let guarded = read_file_guard::read_text_file_with_limit(&target, MAX_READ_BYTES)?;
        let content = match guarded {
            read_file_guard::ReadFileGuardResult::Text(content) => content,
            read_file_guard::ReadFileGuardResult::Omitted(notice) => {
                let read_file_guard::BinaryFileNotice {
                    message,
                    kind,
                    mime_type,
                } = notice;
                if let Value::Object(ref mut map) = summary {
                    map.insert("binary".to_string(), Value::Bool(true));
                    map.insert("kind".to_string(), Value::String(kind.to_string()));
                    if let Some(mime_type) = mime_type {
                        map.insert("mime_type".to_string(), Value::String(mime_type));
                    }
                    map.insert("size_bytes".to_string(), Value::from(size));
                }
                outputs.push(format!(">>> {}\n{}", raw_path, message));
                failures.push(ReadFailure {
                    kind: ReadFailureKind::Binary,
                });
                summaries.push(summary);
                continue;
            }
        };
        successful_reads += 1;
        let lines: Vec<&str> = content.lines().collect();
        let loaded_lines = lines.len();
        if let Value::Object(ref mut map) = summary {
            map.insert("size_bytes".to_string(), Value::from(size));
            map.insert(
                "truncated_by_size".to_string(),
                Value::Bool(source_truncated_by_size),
            );
            if source_truncated_by_size {
                map.insert("loaded_lines".to_string(), Value::from(loaded_lines as u64));
                map.insert(
                    "loaded_bytes".to_string(),
                    Value::from(content.len() as u64),
                );
            }
        }
        match spec.mode {
            ReadFileMode::Slice => {
                let (read_lines, mut complete) = summarize_read_ranges(&spec.ranges, loaded_lines);
                let request_satisfied = slice_request_satisfied(&spec.ranges, loaded_lines);
                let (hit_eof, range_reaches_eof) = if source_truncated_by_size {
                    complete = false;
                    (false, false)
                } else {
                    summarize_slice_eof(&spec.ranges, loaded_lines)
                };
                if let Value::Object(ref mut map) = summary {
                    map.insert("mode".to_string(), Value::String("slice".to_string()));
                    map.insert("read_lines".to_string(), Value::from(read_lines as u64));
                    map.insert("total_lines".to_string(), Value::from(loaded_lines as u64));
                    map.insert(
                        "request_satisfied".to_string(),
                        Value::Bool(request_satisfied),
                    );
                    map.insert("complete".to_string(), Value::Bool(complete));
                    map.insert("hit_eof".to_string(), Value::Bool(hit_eof));
                    map.insert(
                        "range_reaches_eof".to_string(),
                        Value::Bool(range_reaches_eof),
                    );
                }
                let mut file_output = Vec::new();
                if source_truncated_by_size {
                    file_output.push(i18n::t("tool.read.truncated_prefix"));
                }
                let show_range_headers = spec.ranges.len() > 1;
                for (start, end) in spec.ranges {
                    if lines.is_empty() {
                        file_output.push(i18n::t("tool.read.empty_file"));
                        continue;
                    }
                    if start > lines.len() {
                        if source_truncated_by_size {
                            let params = HashMap::from([
                                ("start".to_string(), start.to_string()),
                                ("end".to_string(), end.to_string()),
                                ("loaded".to_string(), lines.len().to_string()),
                            ]);
                            file_output.push(i18n::t_with_params(
                                "tool.read.range_out_of_truncated_excerpt",
                                &params,
                            ));
                        } else {
                            let params = HashMap::from([
                                ("start".to_string(), start.to_string()),
                                ("end".to_string(), end.to_string()),
                                ("total".to_string(), lines.len().to_string()),
                            ]);
                            file_output
                                .push(i18n::t_with_params("tool.read.range_out_of_file", &params));
                        }
                        continue;
                    }
                    let last = end.min(lines.len());
                    let mut slice_lines = Vec::new();
                    if show_range_headers {
                        slice_lines.push(format!("[lines {start}-{last}]"));
                    }
                    for (idx, line) in lines.iter().enumerate().take(last).skip(start - 1) {
                        slice_lines.push(format!("{}: {}", idx + 1, line));
                    }
                    file_output.push(slice_lines.join("\n"));
                    if source_truncated_by_size && end > lines.len() {
                        let params = HashMap::from([
                            ("start".to_string(), start.to_string()),
                            ("end".to_string(), end.to_string()),
                            ("loaded".to_string(), lines.len().to_string()),
                        ]);
                        file_output.push(i18n::t_with_params(
                            "tool.read.range_out_of_truncated_excerpt",
                            &params,
                        ));
                    }
                }
                let joined = file_output.join("\n---\n");
                outputs.push(format!(">>> {}\n{}", raw_path, joined));
            }
            ReadFileMode::Indentation => {
                let selected = read_indentation::read_block(&content, &spec.indentation);
                let read_lines = selected.len();
                let complete = !source_truncated_by_size && loaded_lines == read_lines;
                if let Value::Object(ref mut map) = summary {
                    map.insert("mode".to_string(), Value::String("indentation".to_string()));
                    map.insert("read_lines".to_string(), Value::from(read_lines as u64));
                    map.insert("total_lines".to_string(), Value::from(loaded_lines as u64));
                    map.insert("complete".to_string(), Value::Bool(complete));
                }
                let mut parts = Vec::new();
                if source_truncated_by_size {
                    parts.push(i18n::t("tool.read.truncated_prefix"));
                }
                if selected.is_empty() {
                    parts.push(i18n::t("tool.read.empty_file"));
                } else {
                    let formatted = selected
                        .into_iter()
                        .map(|(line, text)| format!("{line}: {text}"))
                        .collect::<Vec<_>>()
                        .join("\n");
                    parts.push(formatted);
                }
                outputs.push(format!(">>> {}\n{}", raw_path, parts.join("\n")));
            }
        }
        summaries.push(summary);
    }
    let mut result = if outputs.is_empty() {
        i18n::t("tool.read.empty_result")
    } else {
        outputs.join("\n\n")
    };
    let bytes_before_budget = result.len();
    if let Some(output_budget_bytes) = budget.output_budget_bytes {
        let (truncated, omitted) = truncate_utf8_output(result.as_str(), output_budget_bytes);
        if omitted > 0 {
            output_budget_hit = true;
            output_budget_omitted_bytes = omitted;
        }
        result = truncated;
    }
    let continuation_required =
        output_budget_hit || summaries.iter().any(summary_requires_read_continuation);
    let processed_files = summaries.len();
    let mut data = json!({
        "content": result,
        "files": summaries
            .iter()
            .map(compact_read_file_summary_for_model)
            .collect::<Vec<_>>(),
        "patch_usage_hint": i18n::t("tool.read.patch_usage_hint"),
        "dry_run": dry_run,
        "requested_files": requested_files,
        "processed_files": processed_files,
        "budget_file_limit_hit": budget_file_limit_hit,
        "timeout_hit": timeout_hit,
        "output_budget_hit": output_budget_hit,
        "output_budget_omitted_bytes": output_budget_omitted_bytes,
        "content_bytes_before_budget": bytes_before_budget,
        "budget": budget.to_json(),
    });
    if continuation_required {
        data["continuation_required"] = Value::Bool(true);
        data["continuation_hint"] = Value::String(i18n::t("tool.read.continuation_hint"));
    }
    if successful_reads == 0 && !failures.is_empty() {
        let (code, hint) = classify_read_failure(&failures);
        let failure_files = summaries
            .iter()
            .zip(failures.iter())
            .map(|(summary, failure)| compact_read_failure_for_model(summary, failure))
            .collect::<Vec<_>>();
        let error = failure_files
            .first()
            .map(summarize_read_failure_for_model)
            .unwrap_or_else(|| i18n::t("tool.read.empty_result"));
        return Ok(build_failed_tool_result(
            error,
            build_read_failure_data(&failure_files),
            ToolErrorMeta::new(code, Some(hint), false, None),
            false,
        ));
    }
    Ok(build_model_tool_success_with_hint(
        "read_file",
        if dry_run { "dry_run" } else { "completed" },
        if dry_run {
            format!("Validated {processed_files} file read targets without reading content.")
        } else {
            format!("Read {processed_files} files.")
        },
        data,
        continuation_required.then(|| i18n::t("tool.read.continuation_hint")),
    ))
}

fn build_read_failure_data(failures: &[Value]) -> Value {
    match failures {
        [] => json!({}),
        [single] => single.clone(),
        many => json!({
            "failed_count": many.len(),
            "failures": many,
        }),
    }
}

fn compact_read_failure_for_model(summary: &Value, failure: &ReadFailure) -> Value {
    let mut map = Map::new();
    if let Some(path) = summary
        .get("path")
        .cloned()
        .filter(|value| !value.is_null())
    {
        map.insert("path".to_string(), path);
    }
    match failure.kind {
        ReadFailureKind::PathInvalid => {
            map.insert(
                "reason".to_string(),
                Value::String("path_invalid".to_string()),
            );
        }
        ReadFailureKind::NotFound => {
            map.insert("reason".to_string(), Value::String("not_found".to_string()));
        }
        ReadFailureKind::Binary => {
            map.insert("reason".to_string(), Value::String("binary".to_string()));
            if let Some(kind) = summary
                .get("kind")
                .cloned()
                .filter(|value| !value.is_null())
            {
                map.insert("kind".to_string(), kind);
            }
            if let Some(mime_type) = summary
                .get("mime_type")
                .cloned()
                .filter(|value| !value.is_null())
            {
                map.insert("mime_type".to_string(), mime_type);
            }
            if let Some(size_bytes) = summary
                .get("size_bytes")
                .cloned()
                .filter(|value| !value.is_null())
            {
                map.insert("size_bytes".to_string(), size_bytes);
            }
            if summary
                .get("kind")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind == "image")
            {
                map.insert(
                    "suggested_tool".to_string(),
                    Value::String(read_image_tool::TOOL_READ_IMAGE.to_string()),
                );
            }
        }
    }
    Value::Object(map)
}

fn summarize_read_failure_for_model(failure: &Value) -> String {
    let path = failure
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("目标文件");
    match failure
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "path_invalid" => format!("{path} 路径无效或无法解析。"),
        "not_found" => format!("{path} 不存在。"),
        "binary" => {
            if failure
                .get("kind")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind == "image")
            {
                format!(
                    "{path} 是图片，请改用{}。",
                    read_image_tool::TOOL_READ_IMAGE
                )
            } else {
                format!("{path} 是二进制文件，读取文件仅支持纯文本。")
            }
        }
        _ => format!("{path} 无法读取。"),
    }
}

fn classify_read_failure(failures: &[ReadFailure]) -> (&'static str, String) {
    let all_are = |kind| failures.iter().all(|failure| failure.kind == kind);
    if all_are(ReadFailureKind::NotFound) {
        return (
            "TOOL_READ_NOT_FOUND",
            "请先调用列出文件确认真实路径；若目标是技能正文，优先使用技能调用，不要猜测 SKILL.md 路径。".to_string(),
        );
    }
    if all_are(ReadFailureKind::PathInvalid) {
        return (
            "TOOL_READ_PATH_INVALID",
            "请使用当前工作目录相对路径、绝对路径，或直接传入 /workspaces/... 公共路径；若仍失败，请先 list_files 确认真实位置。".to_string(),
        );
    }
    if all_are(ReadFailureKind::Binary) {
        return (
            "TOOL_READ_BINARY_FILE",
            "该工具只适合纯文本文件；图片请改用读图工具，Office/PDF/压缩包请改用对应工具。"
                .to_string(),
        );
    }
    (
        "TOOL_READ_NO_USABLE_TEXT",
        "请先列出文件或搜索内容定位更精确的文本文件，再读取所需片段。".to_string(),
    )
}

pub(crate) fn truncate_utf8_output(text: &str, budget_bytes: usize) -> (String, usize) {
    if text.len() <= budget_bytes {
        return (text.to_string(), 0);
    }
    let mut cut = budget_bytes.min(text.len());
    while cut > 0 && !text.is_char_boundary(cut) {
        cut = cut.saturating_sub(1);
    }
    if cut == 0 {
        return ("".to_string(), text.len());
    }
    let omitted = text.len().saturating_sub(cut);
    (
        format!(
            "{}\n...(truncated read output, omitted {} bytes)...",
            &text[..cut],
            omitted
        ),
        omitted,
    )
}

struct WriteFileOutcome {
    target: PathBuf,
    existed: bool,
    previous_bytes: u64,
}

pub(crate) async fn write_file(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let args = recover_tool_args_value(args);
    if let Some(result) = execute_in_sandbox(context, "写入文件", &args).await {
        if !parse_dry_run(&args) {
            context.workspace.mark_tree_dirty(context.workspace_id);
        }
        return Ok(result);
    }
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if path.is_empty() {
        return Ok(build_failed_tool_result(
            "缺少 path",
            json!({}),
            ToolErrorMeta::new(
                "TOOL_WRITE_PATH_REQUIRED",
                Some("请提供写入目标路径。".to_string()),
                false,
                None,
            ),
            false,
        ));
    }
    let dry_run = parse_dry_run(&args);
    let content = args.get("content").and_then(Value::as_str).unwrap_or("");
    let path = path.to_string();
    let content = content.to_string();
    let bytes = content.len();
    let workspace = context.workspace.clone();
    let user_id = context.workspace_id.to_string();
    let path_for_write = path.clone();
    let allow_roots = collect_orchestration_aware_allow_roots(context);
    let write_outcome = tokio::task::spawn_blocking(move || {
        let target =
            resolve_tool_path(workspace.as_ref(), &user_id, &path_for_write, &allow_roots)?;
        if target.exists() && target.is_dir() {
            return Err(anyhow!("target path is a directory"));
        }
        let existed = target.exists();
        let previous_bytes = if existed {
            target.metadata().map(|meta| meta.len()).unwrap_or(0)
        } else {
            0
        };
        if dry_run {
            return Ok::<WriteFileOutcome, anyhow::Error>(WriteFileOutcome {
                target,
                existed,
                previous_bytes,
            });
        }
        let workspace_root = workspace.workspace_root(&user_id);
        let default_workspace_target = workspace.resolve_path(&user_id, &path_for_write)?;
        if is_within_root(&workspace_root, &target)
            && normalize_path_for_compare(&normalize_target_path(&target))
                == normalize_path_for_compare(&normalize_target_path(&default_workspace_target))
        {
            workspace.write_file(&user_id, &path_for_write, &content, true)?;
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            atomic_write_text(&target, &content)?;
        }
        Ok::<WriteFileOutcome, anyhow::Error>(WriteFileOutcome {
            target,
            existed,
            previous_bytes,
        })
    })
    .await
    .map_err(|err| anyhow!(err.to_string()));
    let write_outcome = match write_outcome {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(err)) | Err(err) => {
            return Ok(build_failed_tool_result(
                format!("写入文件失败：{err}"),
                json!({
                    "path": path,
                    "dry_run": dry_run,
                }),
                ToolErrorMeta::new(
                    "TOOL_WRITE_FAILED",
                    Some("请确认路径权限与目录状态后重试。".to_string()),
                    true,
                    Some(200),
                ),
                false,
            ));
        }
    };
    let lsp_info = if dry_run {
        Value::Null
    } else {
        touch_lsp_file(context, &write_outcome.target, true).await
    };
    Ok(build_model_tool_success(
        "write_file",
        if dry_run { "dry_run" } else { "completed" },
        if dry_run {
            format!("Validated write target for {path} without writing content.")
        } else if write_outcome.existed {
            format!("Updated file {path}.")
        } else {
            format!("Created file {path}.")
        },
        json!({
            "path": path,
            "bytes": bytes,
            "dry_run": dry_run,
            "existed": write_outcome.existed,
            "previous_bytes": write_outcome.previous_bytes,
            "lsp": lsp_info
        }),
    ))
}
