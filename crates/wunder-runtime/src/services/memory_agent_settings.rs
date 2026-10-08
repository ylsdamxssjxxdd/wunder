use crate::services::memory::normalize_agent_memory_scope;
use crate::storage::StorageBackend;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const MEMORY_AGENT_SETTINGS_PREFIX: &str = "memory:agent-settings:";

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct AgentMemorySettings {
    #[serde(default)]
    pub auto_extract_enabled: bool,
    #[serde(default)]
    pub updated_at: f64,
}

pub struct AgentMemorySettingsService {
    storage: Arc<dyn StorageBackend>,
}

impl AgentMemorySettingsService {
    pub fn new(storage: Arc<dyn StorageBackend>) -> Self {
        Self { storage }
    }

    pub fn get_settings(&self, user_id: &str, agent_id: Option<&str>) -> AgentMemorySettings {
        let key = build_memory_agent_settings_key(user_id, agent_id);
        self.storage
            .get_meta(&key)
            .ok()
            .flatten()
            .and_then(|raw| serde_json::from_str::<AgentMemorySettings>(&raw).ok())
            .unwrap_or_default()
    }

    pub fn auto_extract_enabled(&self, user_id: &str, agent_id: Option<&str>) -> bool {
        self.get_settings(user_id, agent_id).auto_extract_enabled
    }

    pub fn set_auto_extract_enabled(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        enabled: bool,
    ) -> Result<AgentMemorySettings> {
        let settings = AgentMemorySettings {
            auto_extract_enabled: enabled,
            updated_at: now_ts(),
        };
        let key = build_memory_agent_settings_key(user_id, agent_id);
        self.storage.set_meta(
            &key,
            &serde_json::to_string(&settings).unwrap_or_else(|_| "{}".to_string()),
        )?;
        Ok(settings)
    }
}

fn build_memory_agent_settings_key(user_id: &str, agent_id: Option<&str>) -> String {
    let scope = normalize_agent_memory_scope(agent_id);
    format!("{MEMORY_AGENT_SETTINGS_PREFIX}{}:{scope}", user_id.trim())
}

fn now_ts() -> f64 {
    (chrono::Utc::now().timestamp_millis() as f64) / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{SqliteStorage, StorageBackend};
    use tempfile::tempdir;

    #[test]
    fn memory_agent_settings_default_to_disabled() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("memory-agent-settings.db");
        let storage: Arc<dyn StorageBackend> =
            Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        storage.ensure_initialized().expect("init storage");
        let service = AgentMemorySettingsService::new(storage);

        let settings = service.get_settings("u1", Some("agent-demo"));
        assert_eq!(settings, AgentMemorySettings::default());
        assert!(!service.auto_extract_enabled("u1", Some("agent-demo")));
    }

    #[test]
    fn memory_agent_settings_roundtrip_by_agent_scope() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("memory-agent-settings-roundtrip.db");
        let storage: Arc<dyn StorageBackend> =
            Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        storage.ensure_initialized().expect("init storage");
        let service = AgentMemorySettingsService::new(storage);

        let saved = service
            .set_auto_extract_enabled("u1", Some("agent-demo"), true)
            .expect("save settings");
        assert!(saved.auto_extract_enabled);
        assert!(saved.updated_at > 0.0);

        assert!(service.auto_extract_enabled("u1", Some("agent-demo")));
        assert!(!service.auto_extract_enabled("u1", Some("other-agent")));
        assert!(!service.auto_extract_enabled("other-user", Some("agent-demo")));
    }
}
