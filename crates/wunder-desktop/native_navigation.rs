//! Small per-user navigation preferences, independent of session execution.
use super::NativeDesktop;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NativeNavigationOrder {
    pub agents: Vec<String>,
    pub threads: Vec<String>,
    /// Sidebar workspace row order. The cloud sentinel uses the stable
    /// `cloud` key alongside local workspace ids.
    #[serde(default)]
    pub workspaces: Vec<String>,
    /// Model config display order; the model list itself is an unordered map.
    #[serde(default)]
    pub models: Vec<String>,
}
impl NativeDesktop {
    /// Per-user sidebar layout, separate from execution and model preferences.
    pub fn sidebar_split_ratio(&self) -> Result<f32> {
        Ok(self
            .state()
            .user_store
            .get_meta(&format!("desktop_sidebar_split:v1:{}", self.user_id()))?
            .and_then(|raw| raw.parse::<f32>().ok())
            .filter(|ratio| ratio.is_finite() && (0.3..=0.8).contains(ratio))
            .unwrap_or(0.6))
    }

    pub fn save_sidebar_split_ratio(&self, ratio: f32) -> Result<()> {
        if !ratio.is_finite() || !(0.3..=0.8).contains(&ratio) {
            bail!("Invalid sidebar split ratio");
        }
        self.state().user_store.set_meta(
            &format!("desktop_sidebar_split:v1:{}", self.user_id()),
            &ratio.to_string(),
        )?;
        Ok(())
    }

    pub fn navigation_order(&self) -> Result<NativeNavigationOrder> {
        self.state()
            .user_store
            .get_meta(&format!("desktop_navigation:v1:{}", self.user_id()))?
            .map(|raw| serde_json::from_str(&raw).map_err(Into::into))
            .unwrap_or_else(|| Ok(Default::default()))
    }
    pub fn save_thread_order(&self, ids: &[String]) -> Result<()> {
        let mut order = self.navigation_order()?;
        order.threads = ids.to_vec();
        self.save_navigation_order(&order)
    }
    pub fn save_navigation_order(&self, order: &NativeNavigationOrder) -> Result<()> {
        for ids in [&order.agents, &order.threads, &order.workspaces] {
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
    pub fn save_workspace_order(&self, ids: &[String]) -> Result<()> {
        let mut order = self.navigation_order()?;
        order.workspaces = ids.to_vec();
        self.save_navigation_order(&order)
    }
}
