use super::*;

impl Orchestrator {
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
