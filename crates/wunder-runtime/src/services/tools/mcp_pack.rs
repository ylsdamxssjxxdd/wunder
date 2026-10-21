use crate::config::{Config, McpServerConfig};
use crate::mcp;
use crate::schemas::ToolSpec;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::HashSet;

pub const MCP_PACK_TOOL_NAME: &str = "__mcp_pack__";

const MAX_LIST_DESCRIPTION_CHARS: usize = 180;
const DEFAULT_LIST_LIMIT: usize = 32;
const MAX_LIST_LIMIT: usize = 64;

pub fn runtime_name(server_name: &str) -> String {
    format!("{}@{}", server_name.trim(), MCP_PACK_TOOL_NAME)
}

pub fn schema() -> Value {
    fn branch(
        action: &str,
        mut properties: serde_json::Map<String, Value>,
        required: &[&str],
    ) -> Value {
        properties.insert(
            "action".to_string(),
            json!({
                "const": action,
                "description": "MCP package operation."
            }),
        );
        json!({
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false
        })
    }

    let mut list = serde_json::Map::new();
    list.insert(
        "cursor".to_string(),
        json!({
            "type": "string",
            "description": "Opaque continuation cursor for action=list."
        }),
    );
    list.insert(
        "limit".to_string(),
        json!({
            "type": "integer",
            "minimum": 1,
            "maximum": MAX_LIST_LIMIT,
            "description": "Maximum compact list items for action=list (default 32)."
        }),
    );

    let mut get = serde_json::Map::new();
    get.insert(
        "tool".to_string(),
        json!({
            "type": "string",
            "minLength": 1,
            "description": "Remote MCP tool name."
        }),
    );

    let mut call = get.clone();
    call.insert(
        "arguments".to_string(),
        json!({
            "type": "object",
            "description": "Arguments passed to the remote MCP tool."
        }),
    );

    json!({
        "oneOf": [
            branch("list", list, &["action"]),
            branch("get", get, &["action", "tool"]),
            branch("call", call, &["action", "tool"])
        ]
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
    validate_action_args(args)?;
    let action = args
        .get("action")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    match action {
        "list" => list_tools(config, server, args).await,
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

/// Validate the model-facing package action before dispatching to MCP.
///
/// The catalog exposes three closed `oneOf` branches. Runtime validation is
/// intentionally repeated here because calls can arrive through compatibility
/// APIs without passing through provider-side JSON-schema validation.
fn validate_action_args(args: &Value) -> Result<()> {
    let object = args
        .as_object()
        .ok_or_else(|| anyhow!("MCP package arguments must be a JSON object"))?;
    let action = object
        .get("action")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("MCP package action is required"))?;
    let allowed: &[&str] = match action {
        "list" => &["action", "cursor", "limit"],
        "get" => &["action", "tool"],
        "call" => &["action", "tool", "arguments"],
        other => return Err(anyhow!("unsupported MCP package action: {other}")),
    };
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(anyhow!(
            "MCP package field '{key}' is not valid for action '{action}'"
        ));
    }
    match action {
        "list" => {
            if let Some(cursor) = object.get("cursor") {
                if !cursor.is_string() {
                    return Err(anyhow!("MCP package list cursor must be a string"));
                }
            }
            if let Some(limit) = object.get("limit") {
                let valid = limit
                    .as_u64()
                    .is_some_and(|value| (1..=MAX_LIST_LIMIT as u64).contains(&value));
                if !valid {
                    return Err(anyhow!(
                        "MCP package list limit must be an integer from 1 to {MAX_LIST_LIMIT}"
                    ));
                }
            }
        }
        "get" | "call" => {
            let tool = object
                .get("tool")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty());
            if tool.is_none() {
                return Err(anyhow!(
                    "MCP package tool is required for action '{action}'"
                ));
            }
            if action == "call" {
                if let Some(arguments) = object.get("arguments") {
                    if !arguments.is_object() {
                        return Err(anyhow!("MCP package call arguments must be a JSON object"));
                    }
                }
            }
        }
        _ => unreachable!("action validated above"),
    }
    Ok(())
}

async fn list_tools(config: &Config, server: &McpServerConfig, args: &Value) -> Result<Value> {
    let mut tools = load_allowed_specs(config, server).await?;
    tools.sort_by(|left, right| left.name.cmp(&right.name));
    let cursor = args
        .get("cursor")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(DEFAULT_LIST_LIMIT)
        .clamp(1, MAX_LIST_LIMIT);
    let total_tools = tools.len();
    let start = cursor
        .and_then(|cursor| {
            tools
                .iter()
                .position(|tool| tool.name == cursor)
                .map(|index| index + 1)
        })
        .unwrap_or_else(|| if cursor.is_some() { total_tools } else { 0 });
    if cursor.is_some() && start == total_tools && total_tools > 0 {
        return Err(anyhow!(
            "MCP package list cursor is invalid or expired; restart action=list"
        ));
    }
    let has_more = start.saturating_add(limit) < total_tools;
    let page = tools
        .into_iter()
        .skip(start)
        .take(limit)
        .collect::<Vec<_>>();
    let next_cursor = if has_more {
        page.last().map(|tool| tool.name.clone())
    } else {
        None
    };
    let items = page
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
            "count": count,
            "next_cursor": next_cursor
        }),
    ))
}

async fn get_tool(config: &Config, server: &McpServerConfig, tool_name: &str) -> Result<Value> {
    let tool = find_tool(config, server, tool_name).await?;
    let reference = crate::tools::schema_reference(
        &format!("{}@{}", server.name, tool.name),
        &crate::tools::tool_schema_document(&tool),
    );
    Ok(package_success(
        "get",
        format!("Fetched schema for MCP tool {tool_name}."),
        json!({
            "server": server.name,
            "tool": tool,
            "schema_ref": reference.schema_ref,
            "catalog_version": reference.catalog_version,
            "schema_hash": reference.schema_hash
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::McpToolSpec;

    fn server(tool_count: usize) -> McpServerConfig {
        McpServerConfig {
            name: "sample".to_string(),
            enabled: true,
            tool_specs: (0..tool_count)
                .map(|index| McpToolSpec {
                    name: format!("tool-{index:03}"),
                    description: "A sufficiently compact remote tool.".to_string(),
                    input_schema: serde_yaml::from_str(
                        "type: object\nproperties: {}\nadditionalProperties: false",
                    )
                    .unwrap(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn list_is_bounded_and_cursor_continues_in_stable_order() {
        let config = Config::default();
        let server = server(3);
        let first = execute(&config, &server, &json!({"action":"list","limit":2}))
            .await
            .unwrap();
        let items = first["data"]["tools"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        let cursor = first["data"]["next_cursor"].as_str().unwrap();
        assert_eq!(cursor, "tool-001");
        let second = execute(
            &config,
            &server,
            &json!({"action":"list","cursor":cursor,"limit":2}),
        )
        .await
        .unwrap();
        assert_eq!(second["data"]["tools"][0]["name"], "tool-002");
        assert!(second["data"]["next_cursor"].is_null());
    }

    #[tokio::test]
    async fn get_returns_stable_schema_reference() {
        let config = Config::default();
        let output = execute(
            &config,
            &server(1),
            &json!({"action":"get","tool":"tool-000"}),
        )
        .await
        .unwrap();
        assert!(output["data"]["schema_ref"]
            .as_str()
            .is_some_and(|value| value.starts_with("tool-schema:")));
        assert_eq!(
            output["data"]["catalog_version"],
            crate::tools::TOOL_CATALOG_VERSION
        );
    }

    #[test]
    fn schema_closes_each_action_to_its_own_fields() {
        let document = schema();
        let branches = document["oneOf"].as_array().expect("oneOf branches");
        assert_eq!(branches.len(), 3);
        for branch in branches {
            assert_eq!(branch["additionalProperties"], json!(false));
            assert_eq!(branch["type"], json!("object"));
        }
        let list = branches
            .iter()
            .find(|branch| branch["properties"]["action"]["const"] == json!("list"))
            .expect("list branch");
        assert!(list["properties"]["cursor"].is_object());
        assert!(list["properties"]["limit"].is_object());
        assert!(list["properties"]["tool"].is_null());
        assert!(list["properties"]["arguments"].is_null());

        let get = branches
            .iter()
            .find(|branch| branch["properties"]["action"]["const"] == json!("get"))
            .expect("get branch");
        assert!(get["properties"]["tool"].is_object());
        assert!(get["properties"]["cursor"].is_null());
        assert!(get["properties"]["arguments"].is_null());

        let call = branches
            .iter()
            .find(|branch| branch["properties"]["action"]["const"] == json!("call"))
            .expect("call branch");
        assert!(call["properties"]["tool"].is_object());
        assert!(call["properties"]["arguments"].is_object());
        assert!(call["properties"]["cursor"].is_null());
    }

    #[tokio::test]
    async fn runtime_rejects_cross_action_and_unknown_fields() {
        let config = Config::default();
        let server = server(1);
        let cases = [
            json!({"action":"list", "tool":"tool-000"}),
            json!({"action":"get", "tool":"tool-000", "limit":1}),
            json!({"action":"call", "tool":"tool-000", "cursor":"x"}),
            json!({"action":"list", "limit":0}),
            json!({"action":"call", "tool":"tool-000", "arguments":null}),
            json!({"action":"call", "tool":"tool-000", "unknown":true}),
        ];
        for args in cases {
            let error = execute(&config, &server, &args)
                .await
                .expect_err("invalid package action should fail");
            assert!(!error.to_string().is_empty());
        }
    }
}
