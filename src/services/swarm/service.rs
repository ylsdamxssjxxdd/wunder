use super::errors::SwarmError;
use super::scope::SwarmHiveResolver;
use super::types::SwarmHiveScope;
use crate::storage::{StorageBackend, UserAgentRecord};
use std::sync::Arc;

pub struct SwarmService {
    resolver: SwarmHiveResolver,
    storage: Arc<dyn StorageBackend>,
}

impl SwarmService {
    pub fn new(storage: Arc<dyn StorageBackend>) -> Self {
        Self {
            resolver: SwarmHiveResolver::new(storage.clone()),
            storage,
        }
    }

    pub fn resolve_scope(
        &self,
        user_id: &str,
        current_agent_id: Option<&str>,
        requested_hive_id: Option<&str>,
    ) -> Result<SwarmHiveScope, SwarmError> {
        self.resolver
            .resolve(user_id, current_agent_id, requested_hive_id)
    }

    pub fn list_agents_in_scope(
        &self,
        user_id: &str,
        _hive_id: &str,
    ) -> Result<Vec<UserAgentRecord>, SwarmError> {
        self.storage
            .list_user_agents(user_id)
            .map_err(|err| SwarmError::unresolved(err.to_string()))
    }

    pub fn ensure_agent_in_scope(
        &self,
        user_id: &str,
        _hive_id: &str,
        agent_id: &str,
    ) -> Result<UserAgentRecord, SwarmError> {
        let agent = self
            .storage
            .get_user_agent(user_id, agent_id)
            .map_err(|err| SwarmError::unresolved(err.to_string()))?
            .ok_or_else(|| SwarmError::denied("agent not found"))?;
        Ok(agent)
    }
}
