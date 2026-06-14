#[cfg(feature = "mcp")]
mod enabled {
    pub use super::super::mcp_impl::*;
}

#[cfg(feature = "mcp")]
pub use enabled::*;

#[cfg(not(feature = "mcp"))]
mod disabled {
    use crate::config::{Config, McpServerConfig};
    use crate::schemas::ToolSpec;
    use crate::state::AppState;
    use anyhow::{anyhow, Result};
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use axum::routing::any;
    use axum::Router;
    use serde_json::{json, Value};
    use std::sync::Arc;

    pub fn router(_state: Arc<AppState>) -> Router<Arc<AppState>> {
        Router::new().route("/wunder/mcp", any(mcp_disabled_handler))
    }

    async fn mcp_disabled_handler() -> impl IntoResponse {
        (
            StatusCode::NOT_IMPLEMENTED,
            axum::Json(json!({
                "error": "mcp feature is disabled; rebuild with --features mcp"
            })),
        )
    }

    pub async fn fetch_tools(_config: &Config, server: &McpServerConfig) -> Result<Vec<ToolSpec>> {
        if !server.tool_specs.is_empty() {
            return Ok(build_tool_specs_from_config(server));
        }
        Err(mcp_disabled_error())
    }

    pub fn build_tool_specs_from_config(server: &McpServerConfig) -> Vec<ToolSpec> {
        server
            .tool_specs
            .iter()
            .map(|spec| ToolSpec {
                name: spec.name.clone(),
                title: spec.title.clone(),
                description: spec.description.clone(),
                input_schema: serde_json::to_value(&spec.input_schema).unwrap_or(Value::Null),
            })
            .collect()
    }

    pub async fn call_tool(
        _config: &Config,
        _server_name: &str,
        _tool_name: &str,
        _args: &Value,
    ) -> Result<Value> {
        Err(mcp_disabled_error())
    }

    pub async fn call_tool_with_server(
        _config: &Config,
        _server: &McpServerConfig,
        _tool_name: &str,
        _args: &Value,
    ) -> Result<Value> {
        Err(mcp_disabled_error())
    }

    pub(crate) fn normalize_transport(transport: Option<&str>) -> String {
        let value = transport.unwrap_or("streamable-http").trim();
        if value.is_empty() {
            return "streamable-http".to_string();
        }
        if value.eq_ignore_ascii_case("http") {
            return "streamable-http".to_string();
        }
        let lowered = value.to_ascii_lowercase();
        match lowered.as_str() {
            "streamable-http" | "streamable_http" | "streamablehttp" => {
                "streamable-http".to_string()
            }
            _ => lowered,
        }
    }

    fn mcp_disabled_error() -> anyhow::Error {
        anyhow!("mcp feature is disabled; rebuild with --features mcp")
    }
}

#[cfg(not(feature = "mcp"))]
pub use disabled::*;

#[cfg(all(test, not(feature = "mcp")))]
mod tests {
    use super::*;
    use crate::config::{McpServerConfig, McpToolSpec};

    #[test]
    fn disabled_stub_keeps_static_tool_specs_available() {
        let server = McpServerConfig {
            tool_specs: vec![McpToolSpec {
                name: "tool".to_string(),
                title: Some("Tool".to_string()),
                description: "desc".to_string(),
                input_schema: serde_yaml::Value::Mapping(serde_yaml::Mapping::new()),
            }],
            ..Default::default()
        };
        let specs = build_tool_specs_from_config(&server);
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].name, "tool");
    }

    #[test]
    fn disabled_stub_normalizes_transport_aliases() {
        assert_eq!(normalize_transport(None), "streamable-http");
        assert_eq!(normalize_transport(Some("http")), "streamable-http");
        assert_eq!(normalize_transport(Some("sse")), "sse");
    }
}
