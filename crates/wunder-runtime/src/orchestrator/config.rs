use super::*;

impl Orchestrator {
    pub(crate) async fn resolve_frozen_session_tool_overrides(
        &self,
        session: &crate::storage::ChatSessionRecord,
        agent: Option<&crate::storage::UserAgentRecord>,
    ) -> Vec<String> {
        let frozen = self
            .workspace
            .load_session_frozen_tool_overrides_async(&session.user_id, &session.session_id)
            .await;
        let overrides = crate::services::agent_execution::resolve_session_tool_overrides(
            session,
            frozen.as_deref(),
            agent,
        );
        if frozen.is_none() {
            self.workspace.save_session_frozen_tool_overrides(
                &session.user_id,
                &session.session_id,
                &overrides,
            );
        }
        overrides
    }

    pub(super) async fn resolve_config(&self, overrides: Option<&Value>) -> Config {
        let base = self.config_store.get().await;
        let Some(overrides) = overrides else {
            return base;
        };
        let mut base_value = serde_json::to_value(&base).unwrap_or(Value::Null);
        merge_json(&mut base_value, overrides);
        serde_json::from_value::<Config>(base_value).unwrap_or(base)
    }
}

fn merge_json(base: &mut Value, override_value: &Value) {
    match (base, override_value) {
        (Value::Object(base_map), Value::Object(override_map)) => {
            for (key, value) in override_map {
                match base_map.get_mut(key) {
                    Some(existing) => merge_json(existing, value),
                    None => {
                        base_map.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (base_slot, override_value) => {
            if !override_value.is_null() {
                *base_slot = override_value.clone();
            }
        }
    }
}
