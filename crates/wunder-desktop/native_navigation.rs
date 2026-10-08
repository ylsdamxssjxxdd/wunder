//! Small per-user navigation preferences, independent of session execution.
use super::NativeDesktop;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NativeNavigationOrder {
    pub agents: Vec<String>,
    pub threads: Vec<String>,
    /// Model config display order; the model list itself is an unordered map.
    #[serde(default)]
    pub models: Vec<String>,
}
impl NativeDesktop {
    pub fn navigation_order(&self) -> Result<NativeNavigationOrder> {
        self.state()
            .user_store
            .get_meta(&format!("desktop_navigation:v1:{}", self.user_id()))?
            .map(|raw| serde_json::from_str(&raw).map_err(Into::into))
            .unwrap_or_else(|| Ok(Default::default()))
    }
    pub fn save_navigation_order(&self, order: &NativeNavigationOrder) -> Result<()> {
        for ids in [&order.agents, &order.threads] {
            if ids.len() > 100 || ids.iter().any(|id| id.len() > 256 || id.is_empty()) {
                bail!("Invalid navigation order");
            }
            let unique: std::collections::HashSet<_> = ids.iter().collect();
            if unique.len() != ids.len() {
                bail!("Duplicate navigation entry");
            }
        }
        if order.models.len() > 200
            || order
                .models
                .iter()
                .any(|key| key.is_empty() || key.len() > 96)
        {
            bail!("Invalid navigation order");
        }
        let unique: std::collections::HashSet<_> = order.models.iter().collect();
        if unique.len() != order.models.len() {
            bail!("Duplicate navigation entry");
        }
        self.state().user_store.set_meta(
            &format!("desktop_navigation:v1:{}", self.user_id()),
            &serde_json::to_string(order)?,
        )?;
        Ok(())
    }
}
