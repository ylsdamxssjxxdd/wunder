#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwarmAction {
    List,
    Status,
    Send,
    History,
    Spawn,
}

impl SwarmAction {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "list" | "agents_list" | "agent_list" | "swarm_list" => Some(Self::List),
            "status" | "agent_status" | "agents_status" | "swarm_status" => Some(Self::Status),
            "send" | "agent_send" | "agents_send" | "swarm_send" => Some(Self::Send),
            "history" | "agent_history" | "agents_history" | "swarm_history" => Some(Self::History),
            "spawn" | "agent_spawn" | "agents_spawn" | "swarm_spawn" => Some(Self::Spawn),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SwarmHiveScope {
    pub user_id: String,
    pub hive_id: String,
    pub current_agent_id: Option<String>,
}
