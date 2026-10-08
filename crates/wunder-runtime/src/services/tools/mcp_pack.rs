use crate::config::{Config, McpServerConfig};
use crate::mcp;
use crate::schemas::ToolSpec;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::HashSet;

pub const MCP_PACK_TOOL_NAME: &str = "__mcp_pack__";

const MAX_LIST_DESCRIPTION_CHARS: usize = 180;

pub fn runtime_name(server_name: &str) -> String {
    format!("{}@{}", server_name.trim(), MCP_PACK_TOOL_NAME)
}

pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["list", "get", "call"],
                "description": "list returns compact available tools; get returns one tool schema; call invokes one remote MCP tool."
            },
            "tool": {
                "type": "string",
                "description": "Remote MCP tool name. Required for get and call."
            },
            "arguments": {
                "type": "object",
                "description": "Arguments passed to the remote MCP tool when action is call."
            }
        },
        "required": ["action"],
        "additionalProperties": false
    })
}

pub fn spec_for_server(server: &McpServerConfig) -> Option<ToolSpec> {
    let server_name = server.name.trim();
    if server_name.is_empty() {
        return None;
    }
    let display_name = server
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(server_name);
    let description = server
        .description
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            format!(
                "MCP service package for {display_name}. Use action=list to inspect available tools, action=get with a tool name to fetch its schema, and action=call to invoke it. Service description: {value}"
            )
        })
        .unwrap_or_else(|| {
            format!(
                "MCP service package for {display_name}. Use action=list to inspect available tools, action=get with a tool name to fetch its schema, and action=call to invoke it."
            )
        });
    Some(ToolSpec {
        name: runtime_name(server_name),
        title: Some(format!("{display_name} MCP package")),
        description,
        input_schema: schema(),
    })
}

pub async fn execute(config: &Config, server: &McpServerConfig, args: &Value) -> Result<Value> {
    let action = args
        .get("action")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    match action {
        "list" => list_tools(config, server).await,
        "get" => {
            let tool_name = required_tool_name(args)?;
            get_tool(config, server, &tool_name).await
        }
        "call" => {
            let tool_name = required_tool_name(args)?;
            let arguments = args.get("arguments").unwrap_or(&Value::Null);
            call_tool(config, server, &tool_name, arguments).await
        }
        "" => Err(anyhow!("MCP package action is required")),
        other => Err(anyhow!("unsupported MCP package action: {other}")),
    }
}

async fn list_tools(config: &Config, server: &McpServerConfig) -> Result<Value> {
    let tools = load_allowed_specs(config, server).await?;
    let items = tools
        .into_iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "title": tool.title,
                "description": truncate_description(&tool.description)
            })
        })
        .collect::<Vec<_>>();
    let count = items.len();
    Ok(package_success(
        "list",
        format!("Listed {count} tools from MCP package {}.", server.name),
        json!({
            "server": server.name,
            "tools": items,
            "count": count
        }),
    ))
}

async fn get_tool(config: &Config, server: &McpServerConfig, tool_name: &str) -> Result<Value> {
    let tool = find_tool(config, server, tool_name).await?;
    Ok(package_success(
        "get",
        format!("Fetched schema for MCP tool {tool_name}."),
        json!({
            "server": server.name,
            "tool": tool
        }),
    ))
}

async fn call_tool(
    config: &Config,
    server: &McpServerConfig,
    tool_name: &str,
    arguments: &Value,
) -> Result<Value> {
    let _ = find_tool(config, server, tool_name).await?;
    let result = mcp::call_tool_with_server(config, server, tool_name, arguments).await?;
    Ok(package_success(
        "call",
        format!("Called MCP tool {tool_name}."),
        json!({
            "server": server.name,
            "tool": tool_name,
            "result": result
        }),
    ))
}

fn package_success(action: &str, summary: impl Into<String>, data: Value) -> Value {
    json!({
        "ok": true,
        "action": "mcp_package",
        "package_action": action,
        "state": "completed",
        "summary": summary.into(),
        "data": data
    })
}

async fn find_tool(config: &Config, server: &McpServerConfig, tool_name: &str) -> Result<ToolSpec> {
    let target = tool_name.trim();
    if target.is_empty() {
        return Err(anyhow!("MCP package tool name is required"));
    }
    load_allowed_specs(config, server)
        .await?
        .into_iter()
        .find(|tool| tool.name == target)
        .ok_or_else(|| anyhow!("MCP tool not found or not allowed: {target}"))
}

async fn load_allowed_specs(config: &Config, server: &McpServerConfig) -> Result<Vec<ToolSpec>> {
    if !server.enabled {
        return Err(anyhow!("MCP server disabled: {}", server.name));
    }
    let specs = if server.tool_specs.is_empty() {
        mcp::fetch_tools(config, server).await?
    } else {
        mcp::build_tool_specs_from_config(server)
    };
    let allow = allowed_set(server);
    Ok(specs
        .into_iter()
        .filter(|tool| is_tool_allowed(&allow, &tool.name))
        .collect())
}

fn allowed_set(server: &McpServerConfig) -> HashSet<String> {
    server
        .allow_tools
        .iter()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect()
}

fn is_tool_allowed(allow: &HashSet<String>, name: &str) -> bool {
    allow.is_empty() || allow.contains(name)
}

fn required_tool_name(args: &Value) -> Result<String> {
    args.get("tool")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("MCP package tool is required for this action"))
}

fn truncate_description(value: &str) -> String {
    let trimmed = value.trim();
    let mut output = String::new();
    for ch in trimmed.chars().take(MAX_LIST_DESCRIPTION_CHARS) {
        output.push(ch);
    }
    if trimmed.chars().count() > MAX_LIST_DESCRIPTION_CHARS {
        output.push_str("...");
    }
    output
}
