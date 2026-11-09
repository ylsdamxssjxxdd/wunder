use super::{
    build_model_tool_success, build_model_tool_success_with_hint, collect_allow_roots,
    collect_read_roots,
    command_options::parse_dry_run,
    execute_in_sandbox, read_file_guard, read_image_tool, read_indentation, read_outline,
    recover_tool_args_value, resolve_tool_path, roots_allow_any_path,
    tool_error::{build_failed_tool_result, ToolErrorMeta},
    touch_lsp_file, ToolContext, DEFAULT_LIST_DEPTH, DEFAULT_LIST_PAGE_LIMIT,
    DEFAULT_READ_OUTPUT_BUDGET_BYTES, DEFAULT_START_LINE_WINDOW, MAX_LIST_ITEMS, MAX_RANGE_SPAN,
    MAX_READ_BUDGET_FILES, MAX_READ_BYTES, MAX_READ_LINES, MAX_READ_OUTPUT_BUDGET_BYTES,
    MAX_READ_TIME_BUDGET_MS, MIN_READ_OUTPUT_BUDGET_BYTES,
};
use crate::core::atomic_write::atomic_write_text;
use crate::core::blocking;
use crate::i18n;
use crate::path_utils::{is_within_root, normalize_path_for_compare, normalize_target_path};
use crate::services::tool_result_store;
use crate::workspace::WorkspaceManager;
use anyhow::{anyhow, Result};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
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
    let top_level_only = args
        .get("top_level_only")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let folders_first = args
        .get("folders_first")
        .and_then(Value::as_bool)
        .unwrap_or(true);
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
    blocking::run_fs("tools.file.list", move || {
        list_files_inner(
            workspace.as_ref(),
            &user_id,
            &path,
            &extra_roots,
            max_depth,
            top_level_only,
            folders_first,
            pagination.start,
            pagination.limit,
        )
    })
    .await
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
    top_level_only: bool,
    folders_first: bool,
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
    let walk_depth = if top_level_only {
        1
    } else {
        max_depth.saturating_add(1)
    };
    // Directory-first ordering needs to see beyond a single page, but the walk
    // must stay bounded on very large trees.
    let scan_cap = page_start
        .saturating_add(page_limit)
        .saturating_add(MAX_LIST_ITEMS)
        .min(MAX_LIST_ITEMS.saturating_mul(4))
        // Deep pagination must always be able to advance: never let the
        // look-ahead cap fall below the end of the requested page.
        .max(page_start.saturating_add(page_limit).saturating_add(1));
    let mut entries: Vec<(String, bool)> = Vec::new();
    let mut scan_limited = false;
    let unrestricted_paths = roots_allow_any_path(extra_roots);
    let _ = unrestricted_paths;
    for entry in WalkDir::new(&root)
        .min_depth(1)
        .max_depth(walk_depth)
        .into_iter()
        .filter_map(|item| item.ok())
    {
        if entries.len() >= scan_cap {
            scan_limited = true;
            break;
        }
        let rel = entry.path().strip_prefix(&root).unwrap_or(entry.path());
        let is_dir = entry.file_type().is_dir();
        let mut display = rel.to_string_lossy().replace('\\', "/");
        if is_dir {
            display.push('/');
        }
        entries.push((display, is_dir));
    }
    let dir_count = entries.iter().filter(|(_, is_dir)| *is_dir).count();
    let file_count = entries.len().saturating_sub(dir_count);
    if folders_first {
        // Stable: directories first, original walk order preserved in each group.
        entries.sort_by_key(|(_, is_dir)| !*is_dir);
    }
    let total_collected = entries.len();
    let items: Vec<String> = entries
        .iter()
        .skip(page_start)
        .take(page_limit)
        .map(|(display, _)| display.clone())
        .collect();
    let returned = items.len();
    let next_offset = page_start.saturating_add(returned);
    let has_more = next_offset < total_collected || scan_limited;
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
            "effective_depth": walk_depth.saturating_sub(1),
            "top_level_only": top_level_only,
            "folders_first": folders_first,
            "dir_count": dir_count,
            "file_count": file_count,
            "scan_limited": scan_limited,
        }),
        has_more.then(|| {
            if top_level_only {
                "More entries are available. Reuse next_cursor to continue listing the same directory."
                    .to_string()
            } else {
                "More entries are available. Directories are listed first; pass top_level_only=true to get only direct children in one call, or reuse next_cursor to continue."
                    .to_string()
            }
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
    pub(crate) pattern: Option<String>,
    pub(crate) pattern_literal: bool,
    pub(crate) pattern_case_sensitive: bool,
    pub(crate) context_lines: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReadFileMode {
    Slice,
    Indentation,
    Outline,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ReadBudget {
    pub(crate) time_budget_ms: Option<u64>,
    pub(crate) output_budget_bytes: Option<usize>,
    pub(crate) max_files: Option<usize>,
}

impl Default for ReadBudget {
    fn default() -> Self {
        Self {
            time_budget_ms: None,
            output_budget_bytes: Some(DEFAULT_READ_OUTPUT_BUDGET_BYTES),
            max_files: None,
        }
    }
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
                pattern: None,
                pattern_literal: false,
                pattern_case_sensitive: false,
                context_lines: 3,
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
            .clamp(1, MAX_READ_LINES);
        let end = offset.saturating_add(limit.saturating_sub(1));
        requested_ranges.push((offset, end));
        ranges.push(normalize_range(offset, end));
    }

    let mode = parse_read_mode(obj);
    // `query` is the handle-read spelling of a content probe. Accepting it here
    // as a literal pattern keeps one mental model for both read paths instead of
    // failing with "query requires a tool-result: path".
    let pattern_source = obj
        .get("pattern")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| (value, false))
        .or_else(|| {
            obj.get("query")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| (value, true))
        });
    let pattern = pattern_source.map(|(value, _)| value.to_string());
    let pattern_forced_literal = pattern_source.is_some_and(|(_, forced)| forced);
    let pattern_literal = pattern_forced_literal
        || obj
            .get("query_mode")
            .and_then(Value::as_str)
            .is_some_and(|mode| mode.eq_ignore_ascii_case("literal"));
    if let Some(pattern) = pattern.as_deref() {
        if pattern.len() > 512 {
            return Err(ReadSpecParseError::invalid_args(
                "pattern 最多 512 个 UTF-8 字节。".to_string(),
            ));
        }
        if !ranges.is_empty() || mode != ReadFileMode::Slice {
            return Err(ReadSpecParseError::invalid_args(
                "pattern 仅用于 slice 模式，不能与行范围或 outline/indentation 模式混用。"
                    .to_string(),
            ));
        }
        let expression = if pattern_literal {
            regex::escape(pattern)
        } else {
            pattern.to_string()
        };
        regex::RegexBuilder::new(&expression)
            .case_insensitive(
                !obj.get("case_sensitive")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            )
            .build()
            .map_err(|error| ReadSpecParseError::invalid_args(format!("pattern 无效：{error}")))?;
    }
    // `limit` is a first-class line window. It used to be ignored unless
    // `offset` was also supplied, which silently upgraded an explicit
    // "give me N lines" request to the 500-line default and tripped the output
    // budget even though the caller asked for fewer lines than that.
    if ranges.is_empty() && pattern.is_none() {
        if let Some(limit) = obj.get("limit").and_then(parse_line_number) {
            let limit = limit.clamp(1, MAX_READ_LINES);
            requested_ranges.push((1, limit));
            ranges.push((1, limit));
        }
    }
    let used_default_range =
        ranges.is_empty() && pattern.is_none() && mode != ReadFileMode::Outline;
    if ranges.is_empty() && pattern.is_none() && mode != ReadFileMode::Outline {
        requested_ranges.push((1, MAX_READ_LINES));
        ranges.push((1, MAX_READ_LINES));
    }
    ranges = merge_read_ranges(ranges);
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
        pattern,
        pattern_literal,
        pattern_case_sensitive: obj
            .get("case_sensitive")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        context_lines: obj
            .get("context")
            .and_then(parse_line_number)
            .unwrap_or(3)
            .min(MAX_READ_LINES),
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
        "outline" | "symbols" | "structure" => ReadFileMode::Outline,
        _ => ReadFileMode::Slice,
    }
}

fn parse_indentation_options(
    obj: &serde_json::Map<String, Value>,
) -> read_indentation::IndentationReadOptions {
    let mut options = read_indentation::IndentationReadOptions {
        max_lines: Some(MAX_READ_LINES),
        ..Default::default()
    };
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
    options.max_lines = indentation
        .get("max_lines")
        .and_then(parse_line_number)
        .map(|value| value.clamp(1, MAX_READ_LINES))
        .or(Some(MAX_READ_LINES));
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
        .map(|value| value.clamp(MIN_READ_OUTPUT_BUDGET_BYTES, MAX_READ_OUTPUT_BUDGET_BYTES))
        .or(Some(DEFAULT_READ_OUTPUT_BUDGET_BYTES));
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

    // `/workspace/<...>`（单数、带前导斜杠）是逻辑公共根 `/workspaces/<...>` 的常见别名，
    // 统一映射到规范根，避免落到不存在的目录导致 TOOL_READ_NOT_FOUND。
    // 注意：裸相对路径 `workspace/...` 必须原样保留——智能体云端目录下真实存在名为
    // `workspace` 的子目录，剥离它会把 `workspace/heart.py` 破坏成 `heart.py` 这样的错误位置。
    if let Some(value) = normalized.strip_prefix("/workspace/") {
        let candidate = value.trim_matches('/').trim();
        if candidate.is_empty() {
            return String::new();
        }
        return format!("/workspaces/{candidate}");
    }

    trimmed.to_string()
}

#[cfg(test)]
mod normalize_read_path_tests {
    use super::normalize_read_path_for_workspace;

    #[test]
    fn keeps_bare_relative_workspace_subdir() {
        // 智能体云端目录下 `workspace/` 是真实子目录，不能被当作根别名剥离成 `heart.py`
        assert_eq!(
            normalize_read_path_for_workspace("workspace/heart.py", "sjxx"),
            "workspace/heart.py"
        );
    }

    #[test]
    fn maps_singular_absolute_root_to_canonical() {
        assert_eq!(
            normalize_read_path_for_workspace("/workspace/sjxx/workspace/heart.py", "sjxx"),
            "/workspaces/sjxx/workspace/heart.py"
        );
    }

    #[test]
    fn keeps_plural_public_root() {
        assert_eq!(
            normalize_read_path_for_workspace("workspaces/sjxx/workspace/heart.py", "sjxx"),
            "/workspaces/sjxx/workspace/heart.py"
        );
        assert_eq!(
            normalize_read_path_for_workspace("/workspaces/sjxx/workspace/heart.py", "sjxx"),
            "/workspaces/sjxx/workspace/heart.py"
        );
    }
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
            ReadFileMode::Slice => {
                left.pattern == right.pattern
                    && left.pattern_literal == right.pattern_literal
                    && left.pattern_case_sensitive == right.pattern_case_sensitive
                    && left.context_lines == right.context_lines
            }
            ReadFileMode::Indentation => left.indentation == right.indentation,
            ReadFileMode::Outline => true,
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

fn pattern_read_ranges(
    lines: &[&str],
    spec: &ReadFileSpec,
) -> Result<(Vec<(usize, usize)>, usize, bool)> {
    let Some(pattern) = spec.pattern.as_deref() else {
        return Ok((spec.ranges.clone(), 0, false));
    };
    let expression = if spec.pattern_literal {
        regex::escape(pattern)
    } else {
        pattern.to_string()
    };
    let matcher = regex::RegexBuilder::new(&expression)
        .case_insensitive(!spec.pattern_case_sensitive)
        .build()?;
    const MAX_PATTERN_MATCHES: usize = 16;
    let mut match_count = 0usize;
    let mut ranges = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if !matcher.is_match(line) {
            continue;
        }
        match_count += 1;
        if ranges.len() < MAX_PATTERN_MATCHES {
            let line = index + 1;
            ranges.push((
                line.saturating_sub(spec.context_lines).max(1),
                line.saturating_add(spec.context_lines).min(lines.len()),
            ));
        }
    }
    ranges = merge_read_ranges(ranges);
    let uncapped = ranges.clone();
    let mut remaining = MAX_READ_LINES;
    let mut capped = Vec::new();
    for (start, end) in ranges {
        if remaining == 0 {
            break;
        }
        let span = end.saturating_sub(start).saturating_add(1);
        let kept_end = if span > remaining {
            start.saturating_add(remaining).saturating_sub(1)
        } else {
            end
        };
        capped.push((start, kept_end));
        remaining = remaining.saturating_sub(kept_end.saturating_sub(start).saturating_add(1));
    }
    let matches_truncated = match_count > MAX_PATTERN_MATCHES || capped != uncapped;
    Ok((capped, match_count, matches_truncated))
}

fn cap_ranges_by_bytes(
    lines: &[&str],
    ranges: &[(usize, usize)],
    budget: Option<usize>,
) -> (Vec<(usize, usize)>, usize) {
    let Some(budget) = budget else {
        return (ranges.to_vec(), 0);
    };
    let total = ranges
        .iter()
        .filter_map(|(start, end)| {
            let last = (*end).min(lines.len());
            (*start <= last).then_some((*start, last))
        })
        .flat_map(|(start, end)| start..=end)
        .map(|line| lines[line - 1].len().saturating_add(1))
        .sum::<usize>();
    let mut capped = Vec::new();
    let mut used = 0usize;
    'ranges: for (start, end) in ranges {
        let last = (*end).min(lines.len());
        if *start > last {
            continue;
        }
        let mut kept_end = None;
        for line_number in *start..=last {
            let cost = lines[line_number - 1].len().saturating_add(1);
            if used.saturating_add(cost) > budget {
                break;
            }
            used = used.saturating_add(cost);
            kept_end = Some(line_number);
        }
        if let Some(kept_end) = kept_end {
            capped.push((*start, kept_end));
            if kept_end < last {
                break 'ranges;
            }
        } else {
            break 'ranges;
        }
    }
    (capped, total.saturating_sub(used))
}

fn cap_selected_by_bytes(
    selected: Vec<(usize, String)>,
    budget: Option<usize>,
    with_line_numbers: bool,
) -> (Vec<(usize, String)>, usize) {
    let Some(budget) = budget else {
        return (selected, 0);
    };
    let cost = |(line, text): &(usize, String)| {
        text.len()
            .saturating_add(1)
            .saturating_add(if with_line_numbers {
                line.to_string().len().saturating_add(2)
            } else {
                0
            })
    };
    let total = selected.iter().map(cost).sum::<usize>();
    let mut used = 0usize;
    let mut kept = Vec::new();
    for item in selected {
        let item_cost = cost(&item);
        if used.saturating_add(item_cost) > budget {
            break;
        }
        used = used.saturating_add(item_cost);
        kept.push(item);
    }
    (kept, total.saturating_sub(used))
}

/// Pick exactly one reason why a read result is not the whole file. The order
/// reflects what actually removed content; `line_window` is last because it is
/// only an informational default-window boundary, not a cut.
fn classify_read_truncation(
    timeout_hit: bool,
    budget_file_limit_hit: bool,
    output_budget_hit: bool,
    summaries: &[Value],
) -> Option<&'static str> {
    if timeout_hit {
        return Some("time_budget");
    }
    if budget_file_limit_hit {
        return Some("file_limit");
    }
    if summaries
        .iter()
        .any(|summary| summary.get("truncated_by_size").and_then(Value::as_bool) == Some(true))
    {
        return Some("source_size");
    }
    if output_budget_hit {
        return Some("output_budget");
    }
    if summaries
        .iter()
        .any(|summary| summary.get("matches_truncated").and_then(Value::as_bool) == Some(true))
    {
        return Some("pattern_matches");
    }
    if summaries
        .iter()
        .any(|summary| summary.get("outline_truncated").and_then(Value::as_bool) == Some(true))
    {
        return Some("outline_budget");
    }
    if summaries.iter().any(summary_stopped_at_default_window) {
        return Some("line_window");
    }
    None
}

fn summary_stopped_at_default_window(summary: &Value) -> bool {
    summary
        .get("used_default_range")
        .and_then(Value::as_bool)
        == Some(true)
        && summary_has_more_lines(summary)
}

fn summary_has_more_lines(summary: &Value) -> bool {
    let read_lines = summary
        .get("read_lines")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let total_lines = summary
        .get("total_lines")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    read_lines > 0 && total_lines > read_lines
}

fn read_next_line(summaries: &[Value]) -> Option<u64> {
    let summary = summaries.last()?;
    if let Some(end) = summary
        .get("effective_ranges")
        .and_then(Value::as_array)
        .and_then(|ranges| ranges.last())
        .and_then(Value::as_array)
        .and_then(|pair| pair.get(1))
        .and_then(Value::as_u64)
    {
        return Some(end.saturating_add(1));
    }
    let read_lines = summary
        .get("read_lines")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    (read_lines > 0).then_some(read_lines.saturating_add(1))
}

/// Bounded per-thread window history. The hard "no fixed-step paging" rule is
/// only worth emitting when the model is actually doing it; emitting it on a
/// single ordinary read was the false positive that made callers doubt the
/// result. At most 256 stamps are kept, so this cannot grow with session count.
const READ_WINDOW_HISTORY_CAPACITY: usize = 256;
static READ_WINDOW_HISTORY: OnceLock<Mutex<VecDeque<(String, u64, u64)>>> = OnceLock::new();

fn record_read_windows(session_id: &str, specs: &[ReadFileSpec]) -> bool {
    let [spec] = specs else {
        return false;
    };
    if spec.used_default_range || spec.mode != ReadFileMode::Slice {
        return false;
    }
    let Some(&(start, end)) = spec.requested_ranges.first() else {
        return false;
    };
    let key = format!("{session_id}\n{}", spec.path);
    let history = READ_WINDOW_HISTORY.get_or_init(|| Mutex::new(VecDeque::new()));
    let Ok(mut history) = history.lock() else {
        return false;
    };
    history.push_back((key.clone(), start as u64, end as u64));
    while history.len() > READ_WINDOW_HISTORY_CAPACITY {
        history.pop_front();
    }
    let recent: Vec<&(String, u64, u64)> = history
        .iter()
        .rev()
        .filter(|(candidate, _, _)| candidate == &key)
        .take(3)
        .collect();
    if recent.len() < 3 {
        return false;
    }
    // newest first: [w3, w2, w1]
    let (w1, w2, w3) = (recent[2], recent[1], recent[0]);
    let first_step = w2.1.saturating_sub(w1.1);
    let second_step = w3.1.saturating_sub(w2.1);
    let width1 = w1.2.saturating_sub(w1.1);
    let width2 = w2.2.saturating_sub(w2.1);
    let width3 = w3.2.saturating_sub(w3.1);
    first_step > 0
        && first_step == second_step
        && width1 == width2
        && width2 == width3
}

fn compact_read_file_summary_for_model(summary: &Value) -> Value {
    json!({
        "path": summary.get("path").cloned().unwrap_or(Value::Null),
        "mode": summary.get("mode").cloned().unwrap_or(Value::Null),
        "requested_ranges": summary.get("requested_ranges").cloned().unwrap_or(Value::Null),
        "effective_ranges": summary.get("effective_ranges").cloned().unwrap_or(Value::Null),
        "used_default_range": summary.get("used_default_range").cloned().unwrap_or(Value::Null),
        "pattern": summary.get("pattern").cloned().unwrap_or(Value::Null),
        "match_count": summary.get("match_count").cloned().unwrap_or(Value::Null),
        "matches_truncated": summary.get("matches_truncated").cloned().unwrap_or(Value::Null),
        "outline_entries": summary.get("outline_entries").cloned().unwrap_or(Value::Null),
        "outline_truncated": summary.get("outline_truncated").cloned().unwrap_or(Value::Null),
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

const DEFAULT_RESULT_READ_BYTES: usize = 8 * 1024;
const MAX_RESULT_READ_BYTES: usize = 16 * 1024;
const MAX_RESULT_QUERY_BYTES: usize = 512;

#[derive(Debug, PartialEq, Eq)]
struct SavedResultReadArgs {
    handle: String,
    offset: u64,
    limit: usize,
    query: Option<String>,
}

fn saved_result_path(args: &Value) -> Option<&str> {
    args.as_str()
        .or_else(|| {
            args.get("path")
                .or_else(|| args.get("file_path"))
                .or_else(|| args.get("file"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|path| path.starts_with("tool-result:"))
}

fn parse_saved_result_read_args(args: &Value) -> Result<Option<SavedResultReadArgs>> {
    let handle = saved_result_path(args);
    let has_nested_handle = args
        .get("files")
        .and_then(Value::as_array)
        .is_some_and(|files| files.iter().any(|file| saved_result_path(file).is_some()));
    let Some(handle) = handle else {
        if has_nested_handle {
            return Err(anyhow!(
                "Saved tool results must use a single top-level path; files cannot contain a tool-result: handle."
            ));
        }
        // `query` is also accepted on regular files as a literal content search
        // (see `parse_read_spec`); only a missing handle is worth returning here.
        return Ok(None);
    };
    if handle == "tool-result:" {
        return Err(anyhow!("The saved tool result handle is empty."));
    }
    for field in [
        "files",
        "start_line",
        "end_line",
        "line_ranges",
        "mode",
        "indentation",
        "pattern",
        "query_mode",
        "case_sensitive",
        "context",
        "output_budget_bytes",
        "dry_run",
    ] {
        if args.get(field).is_some() {
            return Err(anyhow!(
                "{field} cannot be combined with a tool-result: path; use byte offset/limit instead."
            ));
        }
    }
    let offset = args
        .get("offset")
        .map(|value| {
            value
                .as_u64()
                .ok_or_else(|| anyhow!("offset must be a non-negative integer byte offset."))
        })
        .transpose()?
        .unwrap_or(0);
    let limit = args
        .get("limit")
        .map(|value| {
            value
                .as_u64()
                .filter(|value| (1..=MAX_RESULT_READ_BYTES as u64).contains(value))
                .map(|value| value as usize)
                .ok_or_else(|| {
                    anyhow!("limit must be an integer within 1..{MAX_RESULT_READ_BYTES} bytes.")
                })
        })
        .transpose()?
        .unwrap_or(DEFAULT_RESULT_READ_BYTES);
    let query = args
        .get("query")
        .map(|value| {
            value
                .as_str()
                .filter(|value| !value.is_empty() && value.len() <= MAX_RESULT_QUERY_BYTES)
                .map(ToOwned::to_owned)
                .ok_or_else(|| {
                    anyhow!("query must be a non-empty literal string of at most {MAX_RESULT_QUERY_BYTES} UTF-8 bytes.")
                })
        })
        .transpose()?;
    Ok(Some(SavedResultReadArgs {
        handle: handle.to_string(),
        offset,
        limit,
        query,
    }))
}

fn saved_result_read_failure(code: &str, error: impl Into<String>) -> Value {
    build_failed_tool_result(
        error,
        json!({ "result_read": true }),
        ToolErrorMeta::new(
            code,
            Some("Read the original result_handle with path, a zero-based byte offset, limit (1..16384), and optional literal query. Handles belong to the current task thread.".to_string()),
            false,
            None,
        ),
        false,
    )
}

async fn read_saved_result(context: &ToolContext<'_>, args: SavedResultReadArgs) -> Result<Value> {
    let root = tool_result_store::store_root(context.config);
    let user_id = context.user_id.to_string();
    let session_id = context.session_id.to_string();
    let workspace_id = context.workspace_id.to_string();
    let result = blocking::run_fs("tools.result.read", move || {
        let mut data = tool_result_store::read(
            &root,
            &user_id,
            &session_id,
            &workspace_id,
            &args.handle,
            args.offset,
            args.limit,
            args.query.as_deref(),
        )?;
        let object = data
            .as_object_mut()
            .ok_or_else(|| anyhow!("Saved tool result reader returned an invalid response."))?;
        object.insert("result_read".to_string(), Value::Bool(true));
        Ok(data)
    })
    .await;
    Ok(match result {
        Ok(data) => {
            build_model_tool_success("read_file", "completed", "Read saved tool result.", data)
        }
        Err(error) => saved_result_read_failure("TOOL_RESULT_READ_FAILED", error.to_string()),
    })
}

pub(crate) async fn read_files(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let args = recover_tool_args_value(args);
    match parse_saved_result_read_args(&args) {
        Ok(Some(saved_args)) => return read_saved_result(context, saved_args).await,
        Ok(None) => {}
        Err(error) => {
            return Ok(saved_result_read_failure(
                "TOOL_RESULT_READ_INVALID_ARGS",
                error.to_string(),
            ));
        }
    }
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
    let repeated_paging = record_read_windows(context.session_id, &specs);
    let workspace = context.workspace.clone();
    let extra_roots = collect_read_roots(context);
    let budget_for_task = read_budget;
    let result = blocking::run_fs("tools.file.read", move || {
        read_files_inner(
            workspace.as_ref(),
            &user_id,
            &extra_roots,
            specs,
            budget_for_task,
            dry_run,
            requested_files,
            budget_file_limit_hit,
            repeated_paging,
        )
    })
    .await;
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
    repeated_paging: bool,
) -> Result<Value> {
    let started_at = Instant::now();
    let mut outputs = Vec::new();
    let mut summaries = Vec::new();
    let mut failures = Vec::new();
    let mut successful_reads = 0usize;
    let mut timeout_hit = false;
    let mut output_budget_hit = false;
    let mut output_budget_omitted_bytes = 0usize;
    let mut remaining_output_budget = budget.output_budget_bytes;
    for spec in specs {
        if let Some(limit_ms) = budget.time_budget_ms {
            if started_at.elapsed() >= Duration::from_millis(limit_ms) {
                timeout_hit = true;
                break;
            }
        }
        let raw_path = spec.path.as_str();
        let file_output_budget = remaining_output_budget
            .map(|remaining| remaining.saturating_sub(raw_path.len().saturating_add(16)));
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
                        ReadFileMode::Outline => "outline".to_string(),
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
                    ReadFileMode::Outline => "outline",
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
        let rendered_output = match spec.mode {
            ReadFileMode::Slice => {
                let (effective_ranges, match_count, mut matches_truncated) =
                    pattern_read_ranges(&lines, &spec)?;
                let slice_budget = file_output_budget.map(|budget| {
                    budget
                        .saturating_sub(effective_ranges.len().saturating_mul(24))
                        .saturating_sub(if source_truncated_by_size { 256 } else { 0 })
                });
                let (effective_ranges, omitted_bytes) =
                    cap_ranges_by_bytes(&lines, &effective_ranges, slice_budget);
                if omitted_bytes > 0 {
                    output_budget_hit = true;
                    output_budget_omitted_bytes =
                        output_budget_omitted_bytes.saturating_add(omitted_bytes);
                    matches_truncated = true;
                }
                let (read_lines, mut complete) =
                    summarize_read_ranges(&effective_ranges, loaded_lines);
                let request_satisfied = omitted_bytes == 0
                    && if spec.pattern.is_some() {
                        !matches_truncated && !source_truncated_by_size
                    } else {
                        slice_request_satisfied(&effective_ranges, loaded_lines)
                    };
                let (hit_eof, range_reaches_eof) = if source_truncated_by_size || omitted_bytes > 0
                {
                    complete = false;
                    (false, false)
                } else {
                    summarize_slice_eof(&effective_ranges, loaded_lines)
                };
                if let Value::Object(ref mut map) = summary {
                    map.insert("mode".to_string(), Value::String("slice".to_string()));
                    map.insert("effective_ranges".to_string(), json!(effective_ranges));
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
                    if let Some(pattern) = spec.pattern.as_deref() {
                        map.insert("pattern".to_string(), json!(pattern));
                        map.insert("match_count".to_string(), json!(match_count));
                        map.insert("matches_truncated".to_string(), json!(matches_truncated));
                    }
                }
                let mut file_output = Vec::new();
                if source_truncated_by_size {
                    file_output.push(i18n::t("tool.read.truncated_prefix"));
                }
                let show_range_headers = effective_ranges.len() > 1;
                for (start, end) in effective_ranges {
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
                    for line in lines.iter().take(last).skip(start - 1) {
                        slice_lines.push((*line).to_string());
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
                // A single line larger than the whole output budget cannot be
                // expressed as a line window, so dropping it returned no text at
                // all and left no continuation path (minified bundles, one-line
                // JSON, compressed logs). Emit a bounded excerpt instead.
                if file_output.is_empty() && !lines.is_empty() {
                    let anchor = spec.ranges.first().map(|(start, _)| *start).unwrap_or(1);
                    let long_line = lines
                        .get(anchor.saturating_sub(1))
                        .copied()
                        .unwrap_or(lines[0]);
                    let budget = file_output_budget
                        .unwrap_or(DEFAULT_READ_OUTPUT_BUDGET_BYTES)
                        .saturating_sub(96)
                        .max(256);
                    let (excerpt, omitted) = truncate_utf8_output(long_line, budget);
                    if omitted > 0 {
                        output_budget_hit = true;
                        output_budget_omitted_bytes =
                            output_budget_omitted_bytes.saturating_add(omitted);
                        if let Value::Object(ref mut map) = summary {
                            map.insert("long_line_excerpt".to_string(), Value::Bool(true));
                        }
                    }
                    file_output.push(excerpt);
                }
                let joined = file_output.join("\n---\n");
                format!(">>> {}\n{}", raw_path, joined)
            }
            ReadFileMode::Indentation => {
                let selected = read_indentation::read_block(&content, &spec.indentation);
                let (selected, omitted_bytes) =
                    cap_selected_by_bytes(selected, file_output_budget, false);
                if omitted_bytes > 0 {
                    output_budget_hit = true;
                    output_budget_omitted_bytes =
                        output_budget_omitted_bytes.saturating_add(omitted_bytes);
                }
                let read_lines = selected.len();
                let complete =
                    omitted_bytes == 0 && !source_truncated_by_size && loaded_lines == read_lines;
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
                        .map(|(_, text)| text)
                        .collect::<Vec<_>>()
                        .join("\n");
                    parts.push(formatted);
                }
                format!(">>> {}\n{}", raw_path, parts.join("\n"))
            }
            ReadFileMode::Outline => {
                let (selected, outline_entries) =
                    read_outline::read_outline(&content, MAX_READ_LINES);
                let (selected, omitted_bytes) =
                    cap_selected_by_bytes(selected, file_output_budget, true);
                if omitted_bytes > 0 {
                    output_budget_hit = true;
                    output_budget_omitted_bytes =
                        output_budget_omitted_bytes.saturating_add(omitted_bytes);
                }
                let read_lines = selected.len();
                let outline_truncated =
                    omitted_bytes > 0 || outline_entries > read_lines || source_truncated_by_size;
                if let Value::Object(ref mut map) = summary {
                    map.insert("mode".to_string(), Value::String("outline".to_string()));
                    map.insert("read_lines".to_string(), Value::from(read_lines as u64));
                    map.insert("total_lines".to_string(), Value::from(loaded_lines as u64));
                    map.insert("outline_entries".to_string(), json!(outline_entries));
                    map.insert("outline_truncated".to_string(), json!(outline_truncated));
                    map.insert("complete".to_string(), Value::Bool(!outline_truncated));
                }
                let mut parts = Vec::new();
                if source_truncated_by_size {
                    parts.push(i18n::t("tool.read.truncated_prefix"));
                }
                if selected.is_empty() {
                    parts.push(i18n::t("tool.read.empty_file"));
                } else {
                    parts.push(
                        selected
                            .into_iter()
                            .map(|(line, text)| format!("{line}: {text}"))
                            .collect::<Vec<_>>()
                            .join("\n"),
                    );
                }
                format!(">>> {}\n{}", raw_path, parts.join("\n"))
            }
        };
        if let Some(remaining) = remaining_output_budget.as_mut() {
            *remaining = remaining.saturating_sub(rendered_output.len());
        }
        outputs.push(rendered_output);
        summaries.push(summary);
    }
    let mut result = if outputs.is_empty() {
        i18n::t("tool.read.empty_result")
    } else {
        outputs.join("\n\n")
    };
    let bytes_before_budget = result.len().saturating_add(output_budget_omitted_bytes);
    if let Some(output_budget_bytes) = budget.output_budget_bytes {
        let (truncated, omitted) = truncate_utf8_output(result.as_str(), output_budget_bytes);
        if omitted > 0 {
            output_budget_hit = true;
            output_budget_omitted_bytes = output_budget_omitted_bytes.saturating_add(omitted);
        }
        result = truncated;
    }
    // Exactly one reason explains why the payload is not the whole file.
    // Exposing `output_budget_hit` next to a per-file `truncated_by_size: false`
    // made two unrelated signals look contradictory to the model.
    let truncation_reason = classify_read_truncation(
        timeout_hit,
        budget_file_limit_hit,
        output_budget_hit,
        &summaries,
    );
    // A default window that simply reached its line limit is informative, not a
    // demand to keep reading. Only content that was actually cut short requires
    // a continuation, and only repeated fixed-step paging earns the hard rule.
    let continuation_required = truncation_reason.is_some_and(|reason| reason != "line_window");
    let has_more = summaries.iter().any(summary_has_more_lines);
    let next_line = read_next_line(&summaries);
    let continuation_hint = if continuation_required {
        Some(if repeated_paging {
            i18n::t("tool.read.continuation_hint_repeated")
        } else {
            i18n::t("tool.read.continuation_hint")
        })
    } else {
        None
    };
    let processed_files = summaries.len();
    let read_lines = summaries
        .iter()
        .filter_map(|summary| summary.get("read_lines").and_then(Value::as_u64))
        .sum::<u64>();
    let total_lines = summaries
        .iter()
        .filter_map(|summary| summary.get("total_lines").and_then(Value::as_u64))
        .sum::<u64>();
    let returned_bytes = result.len();
    let mut data = json!({
        "content": result,
        "read_lines": read_lines,
        "total_lines": total_lines,
        "returned_bytes": returned_bytes,
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
        "truncation_reason": truncation_reason,
        "has_more": has_more,
        "next_line": next_line,
    });
    if repeated_paging {
        data["repeated_paging"] = Value::Bool(true);
    }
    if let Some(hint) = &continuation_hint {
        data["continuation_required"] = Value::Bool(true);
        data["continuation_hint"] = Value::String(hint.clone());
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
            format!("Read {read_lines} of {total_lines} lines from {processed_files} files.")
        },
        data,
        continuation_hint,
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

/// Validate the small, model-facing part of `write_file` before sandbox or
/// filesystem work.  Keeping this deterministic makes malformed calls cheap
/// to recover from and prevents the error payload from echoing file content.
pub(crate) fn validate_write_file_args(args: &Value) -> Option<Value> {
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(path) = path else {
        return Some(build_failed_tool_result(
            "缺少 path",
            json!({"path_required": true}),
            ToolErrorMeta::new(
                "TOOL_WRITE_PATH_REQUIRED",
                Some("请提供非空字符串 path。".to_string()),
                false,
                None,
            ),
            false,
        ));
    };

    if !args.get("content").is_some_and(Value::is_string) {
        return Some(build_failed_tool_result(
            "缺少 content",
            json!({
                "path": path,
                "content_required": true,
            }),
            ToolErrorMeta::new(
                "TOOL_WRITE_CONTENT_REQUIRED",
                Some("请提供字符串 content；空字符串可用于明确清空文件。".to_string()),
                false,
                None,
            ),
            false,
        ));
    }

    None
}

pub(crate) async fn write_file(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let args = recover_tool_args_value(args);
    // Validate required input before sandbox dispatch or filesystem resolution.
    // Missing content must never be interpreted as an intentional empty write.
    if let Some(failure) = validate_write_file_args(&args) {
        return Ok(failure);
    }
    if let Some(result) = execute_in_sandbox(context, "写入文件", &args).await {
        if !parse_dry_run(&args) {
            context.workspace.mark_tree_dirty(context.workspace_id);
        }
        return Ok(result);
    }
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .expect("write_file arguments were validated")
        .trim();
    let dry_run = parse_dry_run(&args);
    let content = args
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let path = path.to_string();
    let content = content.to_string();
    let bytes = content.len();
    let workspace = context.workspace.clone();
    let user_id = context.workspace_id.to_string();
    let path_for_write = path.clone();
    let allow_roots = collect_allow_roots(context);
    let write_outcome = blocking::run_fs("tools.file.write", move || {
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
        if !dry_run {
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
        }
        Ok::<WriteFileOutcome, anyhow::Error>(WriteFileOutcome {
            target,
            existed,
            previous_bytes,
        })
    })
    .await;
    let write_outcome = match write_outcome {
        Ok(outcome) => outcome,
        Err(err) => {
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
    let data = build_write_data(
        &path,
        bytes,
        dry_run,
        write_outcome.existed,
        write_outcome.previous_bytes,
        lsp_info,
    );
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
        data,
    ))
}

fn build_write_data(
    path: &str,
    bytes: usize,
    dry_run: bool,
    existed: bool,
    previous_bytes: u64,
    lsp: Value,
) -> Value {
    json!({
        "path": path,
        "bytes": bytes,
        "dry_run": dry_run,
        "existed": existed,
        "previous_bytes": previous_bytes,
        "lsp": lsp
    })
}

#[cfg(test)]
mod saved_result_read_tests {
    use super::*;

    #[test]
    fn result_read_defaults_and_byte_offsets_are_distinct_from_file_lines() {
        let args = parse_saved_result_read_args(&json!({
            "path": "tool-result:sample"
        }))
        .expect("valid default read")
        .expect("saved result");
        assert_eq!(args.offset, 0);
        assert_eq!(args.limit, DEFAULT_RESULT_READ_BYTES);
        assert!(args.query.is_none());

        let args = parse_saved_result_read_args(&json!({
            "path": "tool-result:sample",
            "offset": 4096,
            "limit": 16384,
            "query": "sample value"
        }))
        .expect("valid paged search")
        .expect("saved result");
        assert_eq!(args.offset, 4096);
        assert_eq!(args.limit, MAX_RESULT_READ_BYTES);
        assert_eq!(args.query.as_deref(), Some("sample value"));
        assert!(parse_saved_result_read_args(&json!({
            "path": "sample.txt", "start_line": 1, "end_line": 5
        }))
        .expect("ordinary file read")
        .is_none());
    }

    #[test]
    fn result_read_rejects_invalid_pagination_instead_of_reading_a_file() {
        for invalid in [
            json!({ "offset": -1 }),
            json!({ "offset": 1.5 }),
            json!({ "offset": "0" }),
            json!({ "limit": 0 }),
            json!({ "limit": 16385 }),
            json!({ "limit": null }),
            json!({ "query": "" }),
            json!({ "query": 7 }),
            json!({ "query": "x".repeat(MAX_RESULT_QUERY_BYTES + 1) }),
        ] {
            let mut args = json!({ "path": "tool-result:sample" });
            args.as_object_mut()
                .expect("object")
                .extend(invalid.as_object().expect("object").clone());
            assert!(parse_saved_result_read_args(&args).is_err(), "{args}");
        }
        assert!(parse_saved_result_read_args(&json!({ "path": "tool-result:" })).is_err());
    }

    #[test]
    fn result_read_rejects_file_options_and_nested_handles() {
        for field in [
            "files",
            "start_line",
            "end_line",
            "line_ranges",
            "mode",
            "indentation",
            "pattern",
            "query_mode",
            "case_sensitive",
            "context",
            "output_budget_bytes",
            "dry_run",
        ] {
            let mut args = json!({ "path": "tool-result:sample" });
            args[field] = Value::Null;
            assert!(parse_saved_result_read_args(&args).is_err(), "{field}");
        }
        assert!(parse_saved_result_read_args(&json!({
            "files": [{ "path": "tool-result:sample" }]
        }))
        .is_err());
        // `query` is a handle-read spelling of a content probe. On a regular file
        // it must behave as a literal pattern instead of erroring out, so the two
        // read paths share one mental model.
        assert!(parse_saved_result_read_args(&json!({
            "path": "sample.txt", "query": "sample"
        }))
        .expect("query on a regular file is not a handle read")
        .is_none());
        let query_specs = parse_read_file_specs(&json!({
            "path": "sample.txt", "query": "sample"
        }))
        .expect("query resolves to a literal pattern");
        assert_eq!(query_specs[0].pattern.as_deref(), Some("sample"));
        assert!(query_specs[0].pattern_literal);
    }
}

#[cfg(test)]
mod truncation_contract_tests {
    use super::*;

    #[test]
    fn limit_without_offset_is_an_explicit_line_window() {
        let specs = parse_read_file_specs(&json!({ "path": "sample.txt", "limit": 200 }))
            .expect("valid read spec");
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].requested_ranges, vec![(1, 200)]);
        assert!(!specs[0].used_default_range);
    }

    #[test]
    fn limit_is_clamped_and_does_not_collide_with_pattern() {
        let specs = parse_read_file_specs(&json!({ "path": "sample.txt", "limit": 99_999 }))
            .expect("valid read spec");
        assert_eq!(specs[0].requested_ranges, vec![(1, MAX_READ_LINES)]);
        assert!(parse_read_file_specs(&json!({
            "path": "sample.txt", "pattern": "needle", "limit": 20
        }))
        .is_ok());
    }

    #[test]
    fn only_one_truncation_reason_is_reported() {
        let summaries = vec![json!({
            "used_default_range": true,
            "read_lines": 488,
            "total_lines": 676,
            "truncated_by_size": false
        })];
        assert_eq!(
            classify_read_truncation(false, false, true, &summaries),
            Some("output_budget")
        );
        assert_eq!(
            classify_read_truncation(false, false, false, &summaries),
            Some("line_window")
        );
        assert_eq!(classify_read_truncation(false, false, false, &[]), None);
        assert_eq!(
            classify_read_truncation(
                false,
                false,
                false,
                &[json!({
                    "used_default_range": true,
                    "read_lines": 676,
                    "total_lines": 676,
                    "truncated_by_size": false
                })]
            ),
            None
        );
        assert!(summary_has_more_lines(&summaries[0]));
        assert_eq!(
            read_next_line(&[json!({
                "effective_ranges": [[1, 488]],
                "read_lines": 488
            })]),
            Some(489)
        );
    }

    #[test]
    fn fixed_step_paging_is_detected_but_a_single_read_is_not() {
        let session = "truncation-contract-fixed-step";
        let mut hits = Vec::new();
        for (start, end) in [(1, 100), (101, 200), (201, 300)] {
            let specs = parse_read_file_specs(&json!({
                "path": "sample.txt", "start_line": start, "end_line": end
            }))
            .expect("valid read spec");
            hits.push(record_read_windows(session, &specs));
        }
        assert_eq!(hits, vec![false, false, true]);
        let narrow = parse_read_file_specs(&json!({
            "path": "sample.txt", "start_line": 205, "end_line": 210
        }))
        .expect("valid read spec");
        assert!(!record_read_windows(session, &narrow));
    }
}

#[cfg(test)]
mod read_behavior_tests {
    use super::*;

    #[test]
    fn default_read_is_bounded_to_500_lines_and_16_kib() {
        let specs = parse_read_file_specs(&json!({"path": "sample.txt"})).expect("read spec");
        assert_eq!(specs[0].ranges, vec![(1, 500)]);
        assert!(specs[0].used_default_range);
        let budget = parse_read_budget(&json!({}));
        assert_eq!(
            budget.output_budget_bytes,
            Some(DEFAULT_READ_OUTPUT_BUDGET_BYTES)
        );
        let specs = parse_read_file_specs(&json!({"path": "sample.txt", "limit": 900}))
            .expect("limited read spec");
        assert_eq!(specs[0].ranges, vec![(1, 500)]);
        let indentation = parse_read_file_specs(&json!({
            "path": "sample.py", "mode": "indentation", "start_line": 8
        }))
        .expect("indentation read spec");
        assert_eq!(indentation[0].indentation.max_lines, Some(500));
    }

    #[test]
    fn pattern_read_merges_context_and_outline_skips_default_range() {
        let specs = parse_read_file_specs(&json!({
            "path": "sample.py",
            "pattern": "needle",
            "query_mode": "literal",
            "context": 1
        }))
        .expect("pattern read spec");
        let lines = vec![
            "alpha",
            "needle beta",
            "gamma",
            "filler",
            "needle delta",
            "omega",
        ];
        let (ranges, matches, truncated) = pattern_read_ranges(&lines, &specs[0]).expect("ranges");
        assert_eq!(ranges, vec![(1, 6)]);
        assert_eq!(matches, 2);
        assert!(!truncated);

        let outline = parse_read_file_specs(&json!({"path": "sample.py", "mode": "outline"}))
            .expect("outline spec");
        assert_eq!(outline[0].mode, ReadFileMode::Outline);
        assert!(outline[0].ranges.is_empty());
        assert!(!outline[0].used_default_range);
    }

    #[test]
    fn pattern_rejects_ambiguous_ranges_and_invalid_regex() {
        assert!(parse_read_file_specs(&json!({
            "path": "sample.txt", "pattern": "x", "start_line": 2
        }))
        .is_err());
        assert!(parse_read_file_specs(&json!({"path": "sample.txt", "pattern": "("})).is_err());
    }
}

#[cfg(test)]
mod write_result_tests {
    use super::*;

    #[test]
    fn write_result_contains_status_without_content_or_diff_echo() {
        let data = build_write_data("sample.txt", 12, false, true, 8, Value::Null);
        assert_eq!(
            data,
            json!({
                "path": "sample.txt",
                "bytes": 12,
                "dry_run": false,
                "existed": true,
                "previous_bytes": 8,
                "lsp": null
            })
        );
        assert!(data.get("content").is_none());
        assert!(data.get("files").is_none());
        assert!(data.get("diff_blocks").is_none());
    }
}
