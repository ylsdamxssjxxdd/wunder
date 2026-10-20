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
        if frozen.as_deref() != Some(overrides.as_slice()) {
            self.workspace.save_session_frozen_tool_overrides(
                &session.user_id,
                &session.session_id,
                &overrides,
            );
        }
        overrides
    }

    /// Resolve the named tool surface once for a thread. The binding is kept
    /// outside the frozen system-prompt item: existing thread prompts must
    /// remain byte-for-byte stable while the runtime still has a durable
    /// execution boundary for the selected profile.
    pub(super) async fn resolve_frozen_session_tool_profile(
        &self,
        user_id: &str,
        session_id: &str,
        config: &Config,
        overrides: Option<&Value>,
    ) -> (
        crate::tools::ToolProfile,
        Option<crate::tools::ToolProfileBinding>,
    ) {
        if let Some(binding) = self
            .workspace
            .load_session_frozen_tool_profile_async(user_id, session_id)
            .await
        {
            // `Dynamic` classifies catalog entries but is not a selectable
            // named profile. Ignore a malformed legacy metadata value rather
            // than treating it as an unrestricted profile.
            if binding.profile != crate::tools::ToolProfile::Dynamic {
                return (binding.profile, Some(binding));
            }
        }
        (resolve_request_tool_profile(config, overrides), None)
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

/// A request may choose a named profile without exposing arbitrary config
/// mutation. `tool_profile` is the explicit spelling; `profile` remains a
/// compact compatibility spelling. The nested form mirrors `tools.profile` in
/// persisted configuration.
pub(super) fn resolve_request_tool_profile(
    config: &Config,
    overrides: Option<&Value>,
) -> crate::tools::ToolProfile {
    let requested = overrides.and_then(|value| {
        let fields = value.as_object()?;
        ["tool_profile", "profile"]
            .into_iter()
            .filter_map(|key| fields.get(key).and_then(Value::as_str))
            .find_map(crate::tools::ToolProfile::parse)
            .or_else(|| {
                fields
                    .get("tools")
                    .and_then(Value::as_object)
                    .and_then(|tools| tools.get("tool_profile").or_else(|| tools.get("profile")))
                    .and_then(Value::as_str)
                    .and_then(crate::tools::ToolProfile::parse)
            })
    });
    requested.unwrap_or_else(|| crate::tools::resolve_tool_profile(Some(&config.tools.profile)))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_tool_profile_uses_named_override_before_config_baseline() {
        let mut config = Config::default();
        config.tools.profile = "desktop".to_string();

        assert_eq!(
            resolve_request_tool_profile(&config, Some(&serde_json::json!({"profile":"research"}))),
            crate::tools::ToolProfile::Research
        );
        assert_eq!(
            resolve_request_tool_profile(
                &config,
                Some(&serde_json::json!({"tool_profile":"development", "profile":"research"}))
            ),
            crate::tools::ToolProfile::Development
        );
        assert_eq!(
            resolve_request_tool_profile(
                &config,
                Some(&serde_json::json!({"tools":{"profile":"media"}}))
            ),
            crate::tools::ToolProfile::Media
        );
        assert_eq!(
            resolve_request_tool_profile(
                &config,
                Some(&serde_json::json!({"tool_profile":"unknown"}))
            ),
            crate::tools::ToolProfile::Desktop
        );
    }
}
