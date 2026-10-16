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
    /// Bridge session key. Always namespaced by user: a caller-supplied
    /// `browser_session_id` can only address the caller's own namespace, never
    /// another tenant's live browser inside the shared bridge process.
    pub fn session_key(&self) -> Result<String> {
        let user_id = self.user_id.trim();
        if user_id.is_empty() {
            return Err(anyhow!("Missing browser scope field 'user_id'"));
        }
        if let Some(session_id) = self
            .browser_session_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Ok(format!("{user_id}:{session_id}"));
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

#[cfg(test)]
mod tests {
    use super::BrowserSessionScope;

    fn scope(user: &str, session: &str, agent: Option<&str>, id: Option<&str>) -> BrowserSessionScope {
        BrowserSessionScope {
            user_id: user.to_string(),
            session_id: session.to_string(),
            agent_id: agent.map(str::to_string),
            profile: None,
            browser_session_id: id.map(str::to_string),
        }
    }

    #[test]
    fn agent_scope_wins_over_thread_scope() {
        assert_eq!(
            scope("u1", "s1", Some("a1"), None).session_key().unwrap(),
            "u1:a1"
        );
        assert_eq!(
            scope("u1", "s1", None, None).session_key().unwrap(),
            "u1:s1"
        );
    }

    #[test]
    fn explicit_id_stays_inside_the_caller_namespace() {
        // A supplied id cannot reach another user's live browser.
        assert_eq!(
            scope("u1", "s1", None, Some("web-fetch:x")).session_key().unwrap(),
            "u1:web-fetch:x"
        );
        assert_ne!(
            scope("u1", "s1", None, Some("u2:s2")).session_key().unwrap(),
            "u2:s2"
        );
    }

    #[test]
    fn missing_identity_is_rejected() {
        assert!(scope("", "s1", None, Some("any")).session_key().is_err());
        assert!(scope("u1", "", None, None).session_key().is_err());
        assert!(scope("u1", "  ", Some(" "), None).session_key().is_err());
    }
}
