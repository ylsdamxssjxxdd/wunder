use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BrowserSessionScope {
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub browser_session_id: Option<String>,
}

impl BrowserSessionScope {
    pub fn session_key(&self) -> Result<String> {
        if let Some(session_id) = self
            .browser_session_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Ok(session_id.to_string());
        }
        let user_id = self.user_id.trim();
        if user_id.is_empty() {
            return Err(anyhow!("Missing browser scope field 'user_id'"));
        }
        if let Some(agent_id) = self
            .agent_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Ok(format!("{user_id}:{agent_id}"));
        }
        let session_id = self.session_id.trim();
        if session_id.is_empty() {
            return Err(anyhow!(
                "Missing browser scope field 'session_id' when 'agent_id' is absent"
            ));
        }
        Ok(format!("{user_id}:{session_id}"))
    }

    pub fn profile_name(&self) -> Option<String> {
        self.profile
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    }
}
