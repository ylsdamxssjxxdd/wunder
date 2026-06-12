use super::{a2a_tool, build_model_tool_success, knowledge_tool, mcp_pack, ToolContext};
use crate::i18n;
use crate::mcp;
use crate::skills::execute_skill;
use crate::user_tools::{UserToolAlias, UserToolKind};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::HashMap;

pub(crate) async fn execute_user_tool(
    context: &ToolContext<'_>,
    alias: &UserToolAlias,
    args: &Value,
) -> Result<Value> {
    match alias.kind {
        UserToolKind::Mcp => execute_user_mcp_tool(context, alias, args).await,
        UserToolKind::Skill => execute_user_skill(context, alias, args).await,
        UserToolKind::Knowledge => {
            knowledge_tool::execute_user_knowledge_tool(context, alias, args).await
        }
    }
}

async fn execute_user_skill(
    context: &ToolContext<'_>,
    alias: &UserToolAlias,
    args: &Value,
) -> Result<Value> {
    let manager = context
        .user_tool_manager
        .as_ref()
        .ok_or_else(|| anyhow!(i18n::t("tool.invoke.user_skill_not_loaded")))?;
    let bindings = context
        .user_tool_bindings
        .ok_or_else(|| anyhow!(i18n::t("tool.invoke.user_skill_not_loaded")))?;
    let registry = manager
        .get_user_skill_registry(context.config, bindings, &alias.owner_id)
        .ok_or_else(|| anyhow!(i18n::t("tool.invoke.user_skill_not_loaded")))?;
    let spec = registry
        .get(&alias.target)
        .ok_or_else(|| anyhow!(i18n::t("tool.invoke.user_skill_not_found")))?;
    let result = execute_skill(&spec, args, 60).await.map_err(|err| {
        anyhow!(i18n::t_with_params(
            "tool.invoke.user_skill_failed",
            &HashMap::from([("detail".to_string(), err.to_string())]),
        ))
    })?;
    context.workspace.mark_tree_dirty(context.workspace_id);
    Ok(result)
}

async fn execute_user_mcp_tool(
    context: &ToolContext<'_>,
    alias: &UserToolAlias,
    args: &Value,
) -> Result<Value> {
    let target = alias.target.trim();
    let Some((server_name, tool_name)) = split_mcp_target(target) else {
        return Err(anyhow!(i18n::t("tool.invoke.mcp_name_invalid")));
    };
    let bindings = context
        .user_tool_bindings
        .ok_or_else(|| anyhow!(i18n::t("tool.invoke.mcp_server_unavailable")))?;
    let server_map = bindings.mcp_servers.get(&alias.owner_id);
    let server_config = server_map.and_then(|map| map.get(server_name));
    let Some(server_config) = server_config else {
        return Err(anyhow!(i18n::t("tool.invoke.mcp_server_unavailable")));
    };
    if tool_name == mcp_pack::MCP_PACK_TOOL_NAME {
        let result = mcp_pack::execute(context.config, server_config, args)
            .await
            .map_err(|err| {
                anyhow!(i18n::t_with_params(
                    "tool.invoke.mcp_call_failed",
                    &HashMap::from([("detail".to_string(), err.to_string())]),
                ))
            })?;
        return Ok(result);
    }
    let result = mcp::call_tool_with_server(context.config, server_config, tool_name, args)
        .await
        .map_err(|err| {
            anyhow!(i18n::t_with_params(
                "tool.invoke.mcp_call_failed",
                &HashMap::from([("detail".to_string(), err.to_string())]),
            ))
        })?;
    if result
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err(anyhow!(i18n::t("tool.invoke.mcp_result_error")));
    }
    Ok(build_model_tool_success(
        "mcp_call",
        "completed",
        format!("Called MCP tool {tool_name}@{server_name}."),
        json!({
            "server": server_name,
            "tool": tool_name,
            "result": result
        }),
    ))
}

pub(crate) fn is_mcp_tool_name(name: &str) -> bool {
    name.contains('@') && !a2a_tool::is_a2a_service_tool(name)
}

fn split_mcp_tool_name(name: &str) -> Result<(String, String)> {
    let (server, tool) = name
        .split_once('@')
        .ok_or_else(|| anyhow!("MCP tool name format is invalid"))?;
    if server.trim().is_empty() || tool.trim().is_empty() {
        return Err(anyhow!("MCP tool name format is invalid"));
    }
    Ok((server.trim().to_string(), tool.trim().to_string()))
}

pub(crate) async fn execute_mcp_tool(
    context: &ToolContext<'_>,
    name: &str,
    args: &Value,
) -> Result<Value> {
    let (server_name, tool_name) = split_mcp_tool_name(name)?;
    if tool_name == mcp_pack::MCP_PACK_TOOL_NAME {
        let server = context
            .config
            .mcp
            .servers
            .iter()
            .find(|item| item.name == server_name)
            .ok_or_else(|| anyhow!("MCP server not found: {server_name}"))?;
        if !server.packaged {
            return Err(anyhow!("MCP server is not packaged: {server_name}"));
        }
        return mcp_pack::execute(context.config, server, args).await;
    }
    mcp::call_tool(context.config, &server_name, &tool_name, args).await
}

fn split_mcp_target(target: &str) -> Option<(&str, &str)> {
    let mut parts = target.splitn(2, '@');
    let server = parts.next()?.trim();
    let tool = parts.next()?.trim();
    if server.is_empty() || tool.is_empty() {
        None
    } else {
        Some((server, tool))
    }
}
