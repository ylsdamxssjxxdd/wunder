//! `glob` tool: locate files by glob pattern.
//!
//! Returns files (not directories) whose path matches a glob pattern, ordered by
//! modification time (most recently modified first). The result is capped; when
//! the cap is exceeded the paths are sampled across top-level directories so
//! every subtree stays represented and the total match count is reported.
//!
//! Defaults keep the result usable on real repositories: dependency and build
//! cache directories (`node_modules`, `target`, `.cargo`, …) plus everything
//! ignored by `.gitignore` are pruned. Pass `include_ignored=true` to walk the
//! raw tree, and `exclude` to subtract extra glob patterns.

use super::{
    build_model_tool_success_with_hint, collect_read_roots, resolve_tool_path,
    tool_error::{build_failed_tool_result, ToolErrorMeta},
    ToolContext,
};
use crate::core::blocking;
use crate::i18n;
use crate::workspace::WorkspaceManager;
use anyhow::Result;
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use super::fs_walk::{self, ExcludeFilter};

/// Canonical tool name.
pub(crate) const TOOL_GLOB: &str = "文件名匹配";

/// Hard cap on returned paths, matching the dsh glob tool.
const GLOB_MAX_RESULTS: usize = 100;

pub(crate) async fn glob_files(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let args = super::recover_tool_args_value(args);
    if let Some(result) = super::execute_in_sandbox(context, TOOL_GLOB, &args).await {
        return Ok(result);
    }
    let pattern = args
        .get("pattern")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(pattern) = pattern else {
        return Ok(build_failed_tool_result(
            "缺少 pattern",
            json!({ "pattern_required": true }),
            ToolErrorMeta::new(
                "TOOL_GLOB_PATTERN_REQUIRED",
                Some("请提供非空字符串 pattern，例如 \"*.rs\" 或 \"src/**/*.rs\"。".to_string()),
                false,
                None,
            ),
            false,
        ));
    };
    let matcher = match GlobMatcher::new(pattern) {
        Ok(matcher) => matcher,
        Err(message) => {
            return Ok(build_failed_tool_result(
                message,
                json!({ "pattern": pattern }),
                ToolErrorMeta::new(
                    "TOOL_GLOB_INVALID_PATTERN",
                    Some("请检查 glob 语法，例如 \"*.rs\"、\"src/**/*.rs\"。".to_string()),
                    false,
                    None,
                ),
                false,
            ));
        }
    };
    let exclude_patterns = match fs_walk::parse_exclude_patterns(&args) {
        Ok(patterns) => patterns,
        Err(message) => {
            return Ok(build_failed_tool_result(
                message,
                json!({ "exclude": args.get("exclude").cloned().unwrap_or(Value::Null) }),
                ToolErrorMeta::new(
                    "TOOL_GLOB_INVALID_EXCLUDE",
                    Some(
                        "exclude 接受 glob 字符串或字符串数组，例如 \"*.min.js\" 或 [\"dist\", \"vendor\"]。"
                            .to_string(),
                    ),
                    false,
                    None,
                ),
                false,
            ));
        }
    };
    let include_ignored = fs_walk::parse_include_ignored(&args);
    let pattern = pattern.to_string();
    let raw_path = args
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(".")
        .to_string();
    let workspace = context.workspace.clone();
    let user_id = context.workspace_id.to_string();
    let extra_roots = collect_read_roots(context);
    blocking::run_fs("tools.file.glob", move || {
        glob_files_inner(
            workspace.as_ref(),
            &user_id,
            &extra_roots,
            &pattern,
            &raw_path,
            &matcher,
            &exclude_patterns,
            include_ignored,
        )
    })
    .await
}

struct GlobHit {
    rel: String,
    top: String,
    mtime: Option<SystemTime>,
}

fn glob_files_inner(
    workspace: &WorkspaceManager,
    user_id: &str,
    extra_roots: &[PathBuf],
    pattern: &str,
    raw_path: &str,
    matcher: &GlobMatcher,
    exclude_patterns: &[String],
    include_ignored: bool,
) -> Result<Value> {
    let root = resolve_tool_path(workspace, user_id, raw_path, extra_roots)?;
    if !root.exists() {
        return Ok(build_failed_tool_result(
            i18n::t("tool.list.path_not_found"),
            json!({ "path": raw_path }),
            ToolErrorMeta::new(
                "TOOL_GLOB_PATH_NOT_FOUND",
                Some(
                    "Use a directory that exists under the workspace or allowed roots.".to_string(),
                ),
                false,
                None,
            ),
            false,
        ));
    }
    let exclude = match ExcludeFilter::new(exclude_patterns) {
        Ok(filter) => filter.map(Arc::new),
        Err(message) => {
            return Ok(build_failed_tool_result(
                message,
                json!({ "exclude": exclude_patterns }),
                ToolErrorMeta::new(
                    "TOOL_GLOB_INVALID_EXCLUDE",
                    Some(
                        "exclude 接受 glob 字符串或字符串数组，例如 \"*.min.js\" 或 [\"dist\", \"vendor\"]。"
                            .to_string(),
                    ),
                    false,
                    None,
                ),
                false,
            ));
        }
    };
    let mut builder = fs_walk::walk_builder(&root, include_ignored);
    {
        let exclude = exclude.clone();
        let root = root.clone();
        builder.filter_entry(move |entry| entry_allowed(entry, &root, exclude.as_deref(), include_ignored));
    }
    let mut hits: Vec<GlobHit> = Vec::new();
    for entry in builder.build().filter_map(|item| item.ok()) {
        if entry.depth() == 0 || !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let rel = entry.path().strip_prefix(&root).unwrap_or(entry.path());
        let rel_path = rel.to_string_lossy().replace('\\', "/");
        if rel_path.is_empty() {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().to_string();
        if !matcher.is_match(&rel_path, &file_name) {
            continue;
        }
        let top = rel_path.split('/').next().unwrap_or_default().to_string();
        let mtime = entry.metadata().ok().and_then(|meta| meta.modified().ok());
        hits.push(GlobHit {
            rel: rel_path,
            top,
            mtime,
        });
    }
    hits.sort_by(|left, right| {
        right
            .mtime
            .cmp(&left.mtime)
            .then_with(|| left.rel.cmp(&right.rel))
    });
    let total = hits.len();
    let truncated = total > GLOB_MAX_RESULTS;
    let paths: Vec<String> = if truncated {
        sample_across_top_level(&hits, GLOB_MAX_RESULTS)
    } else {
        hits.iter().map(|hit| hit.rel.clone()).collect()
    };
    let returned = paths.len();
    let resolved_root = root.to_string_lossy().to_string();
    let summary = if total == 0 {
        format!("No files found matching {pattern}.")
    } else if truncated {
        format!("Found {total} files matching {pattern}; returning {returned} sampled paths.")
    } else {
        format!("Found {total} files matching {pattern}.")
    };
    // 0 命中时最可能的原因就是目录被默认忽略或列进了 .gitignore，直接把逃生口写出来。
    let hint = if truncated {
        Some(format!(
            "Matched {total} files; showing {returned} sampled across top-level entries. Narrow the pattern or path to see the rest."
        ))
    } else if total == 0 && !include_ignored {
        Some(
            "No match under the default ignore rules (.gitignore plus dependency/build cache directories). Pass include_ignored=true if the target lives in node_modules, target, .cargo or is gitignored."
                .to_string(),
        )
    } else {
        None
    };
    Ok(build_model_tool_success_with_hint(
        TOOL_GLOB,
        "completed",
        summary,
        json!({
            "pattern": pattern,
            "path": raw_path,
            "resolved_path": resolved_root,
            "paths": paths,
            "count": returned,
            "total_matched": total,
            "truncated": truncated,
            "exclude": exclude_patterns,
            "include_ignored": include_ignored,
        }),
        hint,
    ))
}

/// 目录剪枝与 `exclude` 过滤：目录被排除时整棵子树都不进入遍历。
fn entry_allowed(
    entry: &ignore::DirEntry,
    root: &Path,
    exclude: Option<&ExcludeFilter>,
    include_ignored: bool,
) -> bool {
    if entry.depth() == 0 {
        return true;
    }
    let name = entry.file_name().to_string_lossy().to_string();
    if let Some(exclude) = exclude {
        let rel = entry.path().strip_prefix(root).unwrap_or(entry.path());
        let rel_path = rel.to_string_lossy().replace('\\', "/");
        if exclude.is_match(&rel_path, &name) {
            return false;
        }
    }
    if !entry.file_type().is_some_and(|kind| kind.is_dir()) {
        return true;
    }
    !fs_walk::prunes_dir(&name, include_ignored)
}

/// Round-robin sampling across top-level directories, preserving the incoming
/// modification-time order so the most recent files are kept first.
fn sample_across_top_level(hits: &[GlobHit], cap: usize) -> Vec<String> {
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<&GlobHit>> = HashMap::new();
    for hit in hits {
        if !groups.contains_key(&hit.top) {
            order.push(hit.top.clone());
        }
        groups.entry(hit.top.clone()).or_default().push(hit);
    }
    let mut cursor = vec![0usize; order.len()];
    let mut output = Vec::with_capacity(cap);
    'outer: loop {
        let mut progressed = false;
        for (index, key) in order.iter().enumerate() {
            let group = &groups[key];
            if cursor[index] < group.len() {
                output.push(group[cursor[index]].rel.clone());
                cursor[index] += 1;
                progressed = true;
                if output.len() >= cap {
                    break 'outer;
                }
            }
        }
        if !progressed {
            break;
        }
    }
    output
}

/// Glob matcher: patterns without a separator match the file name at any depth,
/// patterns with a separator match the path relative to the search root.
struct GlobMatcher {
    has_separator: bool,
    set: GlobSet,
}

impl GlobMatcher {
    fn new(pattern: &str) -> std::result::Result<Self, String> {
        let has_separator = pattern.contains('/') || pattern.contains('\\');
        let normalized = pattern.replace('\\', "/");
        let glob = Glob::new(&normalized).map_err(|err| format!("无效的 glob 模式：{err}"))?;
        let mut builder = GlobSetBuilder::new();
        builder.add(glob);
        let set = builder
            .build()
            .map_err(|err| format!("无效的 glob 模式：{err}"))?;
        Ok(Self { has_separator, set })
    }

    fn is_match(&self, rel_path: &str, file_name: &str) -> bool {
        if self.has_separator {
            self.set.is_match(rel_path)
        } else {
            self.set.is_match(file_name)
        }
    }
}
