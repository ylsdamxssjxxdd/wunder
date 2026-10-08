//! `glob` tool: locate files by glob pattern.
//!
//! dsh-aligned behaviour: returns files (not directories) whose path matches a
//! glob pattern, including hidden and ignored entries, ordered by modification
//! time (most recently modified first). The result is capped; when the cap is
//! exceeded the paths are sampled across top-level directories so every subtree
//! stays represented and the total match count is reported.

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
use std::path::PathBuf;
use std::time::SystemTime;
use walkdir::WalkDir;

/// Canonical tool name.
pub(crate) const TOOL_GLOB: &str = "glob";

/// Hard cap on returned paths, matching the dsh glob tool.
const GLOB_MAX_RESULTS: usize = 100;

/// Version-control directories pruned from the walk.
const GLOB_VCS_EXCLUDES: &[&str] = &[".git", ".svn", ".hg", ".bzr", ".jj", ".sl"];

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
    let mut hits: Vec<GlobHit> = Vec::new();
    let walker = WalkDir::new(&root).into_iter().filter_entry(|entry| {
        if entry.depth() == 0 || !entry.file_type().is_dir() {
            return true;
        }
        match entry.file_name().to_str() {
            Some(name) => !GLOB_VCS_EXCLUDES.contains(&name),
            None => true,
        }
    });
    for entry in walker.filter_map(|item| item.ok()) {
        if !entry.file_type().is_file() {
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
    Ok(build_model_tool_success_with_hint(
        "glob",
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
        }),
        truncated.then(|| {
            format!(
                "Matched {total} files; showing {returned} sampled across top-level entries. Narrow the pattern or path to see the rest."
            )
        }),
    ))
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
