use super::errors::SwarmError;
use super::types::SwarmHiveScope;
use crate::storage::DEFAULT_HIVE_ID;

pub struct SwarmHiveResolver;

impl SwarmHiveResolver {
    pub fn new(_storage: std::sync::Arc<dyn crate::storage::StorageBackend>) -> Self {
        Self
    }

    pub fn resolve(
        &self,
        user_id: &str,
        current_agent_id: Option<&str>,
        _requested_hive_id: Option<&str>,
    ) -> Result<SwarmHiveScope, SwarmError> {
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Err(SwarmError::unresolved("user_id is empty"));
        }

        Ok(SwarmHiveScope {
            user_id: cleaned_user.to_string(),
            hive_id: DEFAULT_HIVE_ID.to_string(),
            current_agent_id: current_agent_id
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string),
        })
    }
}
