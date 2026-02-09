use anyhow::Result;
use serde_json::{json, Value};

pub struct SwarmBridge;

impl SwarmBridge {
    pub async fn send_to_agent(agent_id: &str, message: &str) -> Result<Value> {
        Ok(json!({
            "agent_id": agent_id,
            "message": message,
            "status": "queued"
        }))
    }
}
