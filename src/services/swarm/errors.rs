use anyhow::anyhow;

#[derive(Debug, Clone)]
pub struct SwarmError {
    pub code: &'static str,
    pub message: String,
}

impl SwarmError {
    pub fn unresolved(message: impl Into<String>) -> Self {
        Self {
            code: "SWARM_HIVE_UNRESOLVED",
            message: message.into(),
        }
    }

    pub fn denied(message: impl Into<String>) -> Self {
        Self {
            code: "SWARM_HIVE_DENIED",
            message: message.into(),
        }
    }

    pub fn policy_blocked(message: impl Into<String>) -> Self {
        Self {
            code: "SWARM_POLICY_BLOCKED",
            message: message.into(),
        }
    }

    pub fn to_anyhow(&self) -> anyhow::Error {
        anyhow!("{}: {}", self.code, self.message)
    }
}
