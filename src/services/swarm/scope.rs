use super::errors::SwarmError;
use super::types::SwarmHiveScope;
use crate::storage::StorageBackend;
use crate::storage::{normalize_hive_id, DEFAULT_HIVE_ID};
use std::sync::Arc;

pub struct SwarmHiveResolver {
    storage: Arc<dyn StorageBackend>,
}

impl SwarmHiveResolver {
    pub fn new(storage: Arc<dyn StorageBackend>) -> Self {
        Self { storage }
    }

    pub fn resolve(
        &self,
        user_id: &str,
        current_agent_id: Option<&str>,
        requested_hive_id: Option<&str>,
    ) -> Result<SwarmHiveScope, SwarmError> {
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Err(SwarmError::unresolved("user_id is empty"));
        }

        if let Some(hive_id) = requested_hive_id {
            let normalized_hive_id = normalize_hive_id(hive_id);
            let hive = self
                .storage
                .get_hive(cleaned_user, &normalized_hive_id)
                .map_err(|err| SwarmError::unresolved(err.to_string()))?;
            if hive.is_none() {
                return Err(SwarmError::unresolved("hive not found"));
            }
            return Ok(SwarmHiveScope {
                user_id: cleaned_user.to_string(),
                hive_id: normalized_hive_id,
                current_agent_id: current_agent_id.map(ToString::to_string),
            });
        }

        if let Some(agent_id) = current_agent_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let record = self
                .storage
                .get_user_agent(cleaned_user, agent_id)
                .map_err(|err| SwarmError::unresolved(err.to_string()))?;
            if let Some(record) = record {
                return Ok(SwarmHiveScope {
                    user_id: cleaned_user.to_string(),
                    hive_id: normalize_hive_id(&record.hive_id),
                    current_agent_id: Some(agent_id.to_string()),
                });
            }
        }

        if let Some(record) = self
            .storage
            .list_hives(cleaned_user, false)
            .map_err(|err| SwarmError::unresolved(err.to_string()))?
            .into_iter()
            .next()
        {
            return Ok(SwarmHiveScope {
                user_id: cleaned_user.to_string(),
                hive_id: normalize_hive_id(&record.hive_id),
                current_agent_id: current_agent_id.map(ToString::to_string),
            });
        }

        Ok(SwarmHiveScope {
            user_id: cleaned_user.to_string(),
            hive_id: DEFAULT_HIVE_ID.to_string(),
            current_agent_id: current_agent_id.map(ToString::to_string),
        })
    }
}
