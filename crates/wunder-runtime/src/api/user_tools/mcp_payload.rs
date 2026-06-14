use super::*;

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(super) struct UserMcpServerPayload {
    name: String,
    endpoint: String,
    #[serde(default)]
    allow_tools: Vec<String>,
    #[serde(default)]
    packaged: bool,
    #[serde(default)]
    shared_tools: Vec<String>,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    transport: Option<String>,
    #[serde(default)]
    description: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    auth: Option<Value>,
    #[serde(default)]
    tool_specs: Vec<Value>,
}

impl From<UserMcpServerPayload> for UserMcpServer {
    fn from(payload: UserMcpServerPayload) -> Self {
        Self {
            name: payload.name,
            endpoint: payload.endpoint,
            allow_tools: payload.allow_tools,
            packaged: payload.packaged,
            shared_tools: payload.shared_tools,
            enabled: payload.enabled,
            transport: payload.transport.unwrap_or_default(),
            description: payload.description,
            display_name: payload.display_name,
            headers: payload.headers,
            auth: payload.auth,
            tool_specs: payload.tool_specs,
        }
    }
}

impl From<&UserMcpServer> for UserMcpServerPayload {
    fn from(server: &UserMcpServer) -> Self {
        Self {
            name: server.name.clone(),
            endpoint: server.endpoint.clone(),
            allow_tools: server.allow_tools.clone(),
            packaged: server.packaged,
            shared_tools: server.shared_tools.clone(),
            enabled: server.enabled,
            transport: if server.transport.trim().is_empty() {
                None
            } else {
                Some(server.transport.clone())
            },
            description: server.description.clone(),
            display_name: server.display_name.clone(),
            headers: server.headers.clone(),
            auth: server.auth.clone(),
            tool_specs: server.tool_specs.clone(),
        }
    }
}
