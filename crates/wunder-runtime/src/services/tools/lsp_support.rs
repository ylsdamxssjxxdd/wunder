use super::context::ToolContext;
use crate::config::Config;
use crate::lsp::LspDiagnostic;
use crate::path_utils::{is_within_root, normalize_path_for_compare, normalize_target_path};
use serde_json::{json, Value};
use std::path::Path;
use tracing::warn;

const MAX_LSP_DIAGNOSTICS: usize = 20;

fn normalize_lsp_extension(value: &str) -> String {
    value.trim().trim_start_matches('.').to_lowercase()
}

fn lsp_file_extension(path: &Path) -> String {
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .trim()
        .to_string();
    normalize_lsp_extension(&ext)
}

fn lsp_matches_file(config: &Config, path: &Path) -> bool {
    let extension = lsp_file_extension(path);
    config
        .lsp
        .servers
        .iter()
        .filter(|server| server.enabled)
        .any(|server| {
            if server.extensions.is_empty() {
                return true;
            }
            server
                .extensions
                .iter()
                .any(|ext| normalize_lsp_extension(ext) == extension)
        })
}

fn format_lsp_diagnostics(diagnostics: &[LspDiagnostic]) -> Option<Value> {
    if diagnostics.is_empty() {
        return None;
    }
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    for diag in diagnostics {
        if diag.is_error() {
            errors.push(diag.pretty());
        } else {
            warnings.push(diag.pretty());
        }
    }
    let total = errors.len() + warnings.len();
    let mut items: Vec<String> = errors.iter().chain(warnings.iter()).cloned().collect();
    let truncated = items.len() > MAX_LSP_DIAGNOSTICS;
    if truncated {
        items.truncate(MAX_LSP_DIAGNOSTICS);
    }
    Some(json!({
        "total": total,
        "errors": errors.len(),
        "warnings": warnings.len(),
        "truncated": truncated,
        "items": items,
    }))
}

fn lsp_diagnostics_summary(context: &ToolContext<'_>, path: &Path) -> Option<Value> {
    let diagnostics_map = context
        .lsp_manager
        .diagnostics_for_user(context.workspace_id);
    if diagnostics_map.is_empty() {
        return None;
    }
    let target = normalize_target_path(path);
    let target_compare = normalize_path_for_compare(&target);
    for (candidate, diagnostics) in diagnostics_map {
        if normalize_path_for_compare(&candidate) == target_compare {
            return format_lsp_diagnostics(&diagnostics);
        }
    }
    None
}

pub(crate) async fn touch_lsp_file(
    context: &ToolContext<'_>,
    path: &Path,
    wait_for_diagnostics: bool,
) -> Value {
    if !context.config.lsp.enabled {
        return Value::Null;
    }
    let workspace_root = context.workspace.workspace_root(context.workspace_id);
    if !is_within_root(&workspace_root, path) {
        return json!({
            "enabled": true,
            "matched": false,
            "touched": false,
            "diagnostics": Option::<Value>::None,
            "error": "文件不在工作区范围内"
        });
    }
    let matched = lsp_matches_file(context.config, path);
    if !matched {
        return json!({
            "enabled": true,
            "matched": false,
            "touched": false,
            "diagnostics": Option::<Value>::None,
            "error": "未匹配到可用的 LSP 服务"
        });
    }
    let mut diagnostics = None;
    let mut error = None;
    let touched = match context
        .lsp_manager
        .touch_file(
            context.config,
            context.workspace_id,
            path,
            wait_for_diagnostics,
        )
        .await
    {
        Ok(()) => true,
        Err(err) => {
            warn!("LSP touch failed: {err}");
            error = Some(err.to_string());
            false
        }
    };
    if touched && wait_for_diagnostics {
        diagnostics = lsp_diagnostics_summary(context, path);
    }
    json!({
        "enabled": true,
        "matched": matched,
        "touched": touched,
        "diagnostics": diagnostics,
        "error": error
    })
}

