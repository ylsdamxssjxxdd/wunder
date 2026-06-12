use super::{build_model_tool_success_with_hint, context::ToolContext};
use crate::config::Config;
use crate::lsp::LspDiagnostic;
use crate::path_utils::{is_within_root, normalize_path_for_compare, normalize_target_path};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::path::Path;
use tracing::warn;
use url::Url;

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

fn resolve_lsp_timeout_s(config: &Config) -> u64 {
    if config.lsp.timeout_s == 0 {
        30
    } else {
        config.lsp.timeout_s
    }
}

fn parse_lsp_position(args: &Value) -> Result<(u32, u32)> {
    let line = args
        .get("line")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("缺少 line"))?;
    let character = args
        .get("character")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("缺少 character"))?;
    if line == 0 || character == 0 {
        return Err(anyhow!("line/character 必须 >= 1"));
    }
    Ok(((line - 1) as u32, (character - 1) as u32))
}

fn lsp_path_to_uri(path: &Path) -> Result<String> {
    Url::from_file_path(path)
        .map(|url| url.to_string())
        .map_err(|_| anyhow!("LSP 文件路径无效"))
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

pub(crate) async fn lsp_query(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    if !context.config.lsp.enabled {
        return Err(anyhow!("LSP 未启用"));
    }
    let operation = args
        .get("operation")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("缺少 operation"))?
        .trim()
        .to_string();
    if operation.is_empty() {
        return Err(anyhow!("operation 不能为空"));
    }
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("缺少 path"))?
        .trim()
        .to_string();
    if path.is_empty() {
        return Err(anyhow!("path 不能为空"));
    }
    let target = context
        .workspace
        .resolve_path(context.workspace_id, &path)?;
    if !target.exists() {
        return Err(anyhow!("LSP 文件不存在: {path}"));
    }
    context
        .lsp_manager
        .touch_file(context.config, context.workspace_id, &target, false)
        .await?;
    let uri = lsp_path_to_uri(&target)?;
    let timeout_s = resolve_lsp_timeout_s(context.config);
    let operation_key = normalize_lsp_operation_key(&operation);
    let needs_position = matches!(
        operation_key.as_str(),
        "definition" | "references" | "hover" | "implementation" | "callhierarchy"
    );
    let position_value = if needs_position {
        let (line, character) = parse_lsp_position(args)?;
        Some(json!({ "line": line, "character": character }))
    } else {
        None
    };
    let query = if operation_key == "workspacesymbol" {
        Some(
            args.get("query")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string(),
        )
    } else {
        None
    };
    if operation_key == "workspacesymbol" && query.as_deref().unwrap_or("").is_empty() {
        return Err(anyhow!("workspaceSymbol 缺少 query"));
    }
    let call_direction = args
        .get("call_hierarchy_direction")
        .and_then(Value::as_str)
        .unwrap_or("incoming")
        .trim()
        .to_lowercase();
    let text_document = json!({ "uri": uri });
    let position_value = position_value.clone();
    let query_value = query.clone();
    let operation_key = operation_key.clone();
    let call_direction = call_direction.clone();
    let results = context
        .lsp_manager
        .run_on_clients(
            context.config,
            context.workspace_id,
            &target,
            move |client| {
                let text_document = text_document.clone();
                let position = position_value.clone();
                let query = query_value.clone();
                let operation = operation_key.clone();
                let direction = call_direction.clone();
                async move {
                    let server_id = client.server_id().to_string();
                    let server_name = client.server_name().to_string();
                    let result = match operation.as_str() {
                        "definition" => {
                            let position =
                                position.ok_or_else(|| anyhow!("缺少 line/character"))?;
                            client
                                .request(
                                    "textDocument/definition",
                                    json!({ "textDocument": text_document, "position": position }),
                                    timeout_s,
                                )
                                .await?
                        }
                        "references" => {
                            let position =
                                position.ok_or_else(|| anyhow!("缺少 line/character"))?;
                            client
                                .request(
                                    "textDocument/references",
                                    json!({
                                        "textDocument": text_document,
                                        "position": position,
                                        "context": { "includeDeclaration": true }
                                    }),
                                    timeout_s,
                                )
                                .await?
                        }
                        "hover" => {
                            let position =
                                position.ok_or_else(|| anyhow!("缺少 line/character"))?;
                            client
                                .request(
                                    "textDocument/hover",
                                    json!({ "textDocument": text_document, "position": position }),
                                    timeout_s,
                                )
                                .await?
                        }
                        "documentsymbol" => {
                            client
                                .request(
                                    "textDocument/documentSymbol",
                                    json!({ "textDocument": text_document }),
                                    timeout_s,
                                )
                                .await?
                        }
                        "workspacesymbol" => {
                            let query = query.unwrap_or_default();
                            client
                                .request("workspace/symbol", json!({ "query": query }), timeout_s)
                                .await?
                        }
                        "implementation" => {
                            let position =
                                position.ok_or_else(|| anyhow!("缺少 line/character"))?;
                            client
                                .request(
                                    "textDocument/implementation",
                                    json!({ "textDocument": text_document, "position": position }),
                                    timeout_s,
                                )
                                .await?
                        }
                        "callhierarchy" => {
                            let position =
                                position.ok_or_else(|| anyhow!("缺少 line/character"))?;
                            let items = client
                                .request(
                                    "textDocument/prepareCallHierarchy",
                                    json!({ "textDocument": text_document, "position": position }),
                                    timeout_s,
                                )
                                .await?;
                            let calls = if let Some(item) =
                                items.as_array().and_then(|items| items.first()).cloned()
                            {
                                let method = if direction == "outgoing" {
                                    "callHierarchy/outgoingCalls"
                                } else {
                                    "callHierarchy/incomingCalls"
                                };
                                client
                                    .request(method, json!({ "item": item }), timeout_s)
                                    .await?
                            } else {
                                Value::Null
                            };
                            json!({
                                "items": items,
                                "direction": direction,
                                "calls": calls
                            })
                        }
                        _ => {
                            return Err(anyhow!("未知 LSP operation: {operation}"));
                        }
                    };
                    Ok(json!({
                        "server_id": server_id,
                        "server_name": server_name,
                        "result": result
                    }))
                }
            },
        )
        .await?;
    Ok(build_model_tool_success_with_hint(
        "lsp_query",
        "completed",
        format!("Ran LSP {operation} on {path} across {} servers.", results.len()),
        json!({
            "operation": operation,
            "path": path,
            "results": results,
            "server_count": results.len(),
        }),
        results.is_empty().then(|| {
            "No LSP servers returned a result for this file. Check server availability or file type support."
                .to_string()
        }),
    ))
}

fn normalize_lsp_operation_key(raw: &str) -> String {
    raw.trim().to_ascii_lowercase().replace(['_', '-'], "")
}

#[cfg(test)]
mod tests {
    use super::normalize_lsp_operation_key;

    #[test]
    fn normalize_lsp_operation_key_accepts_snake_case_and_legacy_camel_case() {
        assert_eq!(
            normalize_lsp_operation_key("document_symbol"),
            "documentsymbol"
        );
        assert_eq!(
            normalize_lsp_operation_key("documentSymbol"),
            "documentsymbol"
        );
        assert_eq!(
            normalize_lsp_operation_key("workspace-symbol"),
            "workspacesymbol"
        );
        assert_eq!(
            normalize_lsp_operation_key("call_hierarchy"),
            "callhierarchy"
        );
    }
}
