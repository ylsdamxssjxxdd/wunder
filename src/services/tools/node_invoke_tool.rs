use super::{build_model_tool_success, context::ToolContext};
use crate::gateway::GatewayNodeInvokeRequest;
use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeInvokeAction {
    List,
    Invoke,
}

#[derive(Debug, Deserialize)]
struct NodeInvokeArgs {
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    node_id: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    args: Option<Value>,
    #[serde(default)]
    timeout_s: Option<f64>,
    #[serde(default)]
    metadata: Option<Value>,
}

pub(crate) async fn execute_node_invoke_tool(
    context: &ToolContext<'_>,
    args: &Value,
) -> Result<Value> {
    let payload: NodeInvokeArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    match resolve_node_invoke_action(&payload)? {
        NodeInvokeAction::List => execute_node_list(context).await,
        NodeInvokeAction::Invoke => execute_node_invoke_action(context, payload).await,
    }
}

fn resolve_node_invoke_action(payload: &NodeInvokeArgs) -> Result<NodeInvokeAction> {
    if let Some(action) = payload.action.as_deref() {
        let action = action.trim();
        if action.is_empty() {
            return Err(anyhow!("节点调用 action 不能为空"));
        }
        let normalized = action.to_ascii_lowercase();
        return match normalized.as_str() {
            "list" | "ls" | "列表" | "列出" => Ok(NodeInvokeAction::List),
            "invoke" | "call" | "调用" => Ok(NodeInvokeAction::Invoke),
            _ => Err(anyhow!("未知节点调用 action: {action}")),
        };
    }
    let has_node_id = payload
        .node_id
        .as_deref()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    let has_command = payload
        .command
        .as_deref()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    if has_node_id && has_command {
        Ok(NodeInvokeAction::Invoke)
    } else {
        Err(anyhow!(
            "节点调用缺少 action，支持 list/invoke；兼容模式下需提供 node_id 与 command"
        ))
    }
}

async fn execute_node_list(context: &ToolContext<'_>) -> Result<Value> {
    let gateway = context
        .gateway
        .clone()
        .ok_or_else(|| anyhow!("gateway not available"))?;
    let snapshot = gateway.snapshot().await;
    let mut nodes = Vec::new();
    for item in snapshot.items {
        if !item.role.eq_ignore_ascii_case("node") {
            continue;
        }
        let Some(node_id) = normalize_optional_string(item.node_id) else {
            continue;
        };
        nodes.push(json!({
            "node_id": node_id,
            "connection_id": item.connection_id,
            "scopes": item.scopes,
            "caps": item.caps,
            "commands": item.commands,
            "connected_at": item.connected_at,
            "last_seen_at": item.last_seen_at,
            "client": item.client
        }));
    }
    nodes.sort_by(|left, right| {
        let left_node = left.get("node_id").and_then(Value::as_str).unwrap_or("");
        let right_node = right.get("node_id").and_then(Value::as_str).unwrap_or("");
        left_node.cmp(right_node).then_with(|| {
            let left_connection = left
                .get("connection_id")
                .and_then(Value::as_str)
                .unwrap_or("");
            let right_connection = right
                .get("connection_id")
                .and_then(Value::as_str)
                .unwrap_or("");
            left_connection.cmp(right_connection)
        })
    });
    Ok(build_model_tool_success(
        "list",
        "completed",
        format!("Listed {} gateway nodes.", nodes.len()),
        json!({
            "state_version": snapshot.state_version,
            "count": nodes.len(),
            "nodes": nodes
        }),
    ))
}

async fn execute_node_invoke_action(
    context: &ToolContext<'_>,
    payload: NodeInvokeArgs,
) -> Result<Value> {
    let gateway = context
        .gateway
        .clone()
        .ok_or_else(|| anyhow!("gateway not available"))?;
    let node_id = normalize_optional_string(payload.node_id)
        .ok_or_else(|| anyhow!("节点调用 invoke 需要 node_id"))?;
    let command = normalize_optional_string(payload.command)
        .ok_or_else(|| anyhow!("节点调用 invoke 需要 command"))?;
    let timeout_s = payload.timeout_s.unwrap_or(30.0);
    let result = gateway
        .invoke_node(GatewayNodeInvokeRequest {
            node_id: node_id.clone(),
            command: command.clone(),
            args: payload.args,
            timeout_s,
            metadata: payload.metadata,
        })
        .await?;
    if result.ok {
        Ok(build_model_tool_success(
            "invoke",
            "completed",
            format!("Invoked command {command} on node {node_id}."),
            json!({
                "node_id": node_id,
                "command": command,
                "result": result.payload
            }),
        ))
    } else {
        let message = result
            .error
            .as_ref()
            .and_then(|value| value.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("node invoke failed");
        Err(anyhow!(message.to_string()))
    }
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value.and_then(|raw| {
        let trimmed = raw.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{resolve_node_invoke_action, NodeInvokeAction, NodeInvokeArgs};

    #[test]
    fn node_invoke_action_uses_compatible_implicit_invoke() {
        let payload = NodeInvokeArgs {
            action: None,
            node_id: Some("node_a".to_string()),
            command: Some("ping".to_string()),
            args: None,
            timeout_s: None,
            metadata: None,
        };
        assert_eq!(
            resolve_node_invoke_action(&payload).expect("implicit invoke"),
            NodeInvokeAction::Invoke
        );
    }

    #[test]
    fn node_invoke_action_rejects_missing_action_and_target() {
        let payload = NodeInvokeArgs {
            action: None,
            node_id: None,
            command: None,
            args: None,
            timeout_s: None,
            metadata: None,
        };
        assert!(resolve_node_invoke_action(&payload).is_err());
    }
}
