//! Persisted state for the native floating companions (desktop pets).
//!
//! This mirrors the web layer's three localStorage keys — per-pet positions,
//! per-agent show/scale overrides and the global message-hint switch — but
//! lives in its own file next to the desktop settings so dragging a pet never
//! rewrites runtime configuration. Keys are the web ones: positions are
//! `agentId:scope:companionId`, overrides are keyed by agent id.

use super::{now_ts, NativeDesktop};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Web `resolveScaleValue`: the menu and the store clamp to 0.5..=1.6.
pub const COMPANION_SCALE_MIN: f32 = 0.5;
pub const COMPANION_SCALE_MAX: f32 = 1.6;

pub fn clamp_companion_scale(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(COMPANION_SCALE_MIN, COMPANION_SCALE_MAX)
    } else {
        1.0
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct CompanionPlacement {
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct CompanionAgentOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f32>,
}

fn default_message_hints() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompanionOverlayState {
    /// Web `settings.messageHintsEnabled`; a pet only bubbles hints while this
    /// and its own `messageHints` flag are both on.
    #[serde(default = "default_message_hints")]
    pub message_hints: bool,
    #[serde(default)]
    pub positions: HashMap<String, CompanionPlacement>,
    #[serde(default)]
    pub overrides: HashMap<String, CompanionAgentOverride>,
    #[serde(default)]
    pub updated_at: f64,
}

impl Default for CompanionOverlayState {
    fn default() -> Self {
        Self {
            message_hints: true,
            positions: HashMap::new(),
            overrides: HashMap::new(),
            updated_at: 0.0,
        }
    }
}

fn overlay_path(settings_path: &std::path::Path) -> PathBuf {
    settings_path
        .parent()
        .map(|parent| parent.join("companion.overlay.json"))
        .unwrap_or_else(|| PathBuf::from("companion.overlay.json"))
}

fn read_overlay(path: &std::path::Path) -> CompanionOverlayState {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(_) => return CompanionOverlayState::default(),
    };
    serde_json::from_str::<CompanionOverlayState>(&text).unwrap_or_default()
}

fn write_overlay(path: &std::path::Path, state: &CompanionOverlayState) -> Result<()> {
    let text = serde_json::to_string_pretty(state).context("serialize companion overlay failed")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("create companion overlay dir failed: {}", parent.display())
        })?;
    }
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, text)
        .with_context(|| format!("write companion overlay temp failed: {}", temp.display()))?;
    if path.exists() {
        // Keep one previous copy so a broken rename never loses all placements.
        let _ = fs::copy(path, path.with_extension("json.bak"));
    }
    if fs::rename(&temp, path).is_err() {
        fs::copy(&temp, path).with_context(|| {
            format!(
                "replace companion overlay failed: {} -> {}",
                temp.display(),
                path.display()
            )
        })?;
        let _ = fs::remove_file(&temp);
    }
    Ok(())
}

impl NativeDesktop {
    pub fn companion_overlay_state(&self) -> CompanionOverlayState {
        read_overlay(&overlay_path(&self.desktop.settings_path))
    }

    /// Remember where a pet was dropped. Positions are floored at zero, the way
    /// the web loads them, and only written once per drag. The refreshed state
    /// lets the caller keep one in-memory copy instead of re-reading the file.
    pub fn save_companion_position(
        &self,
        key: &str,
        x: i32,
        y: i32,
    ) -> Result<CompanionOverlayState> {
        self.mutate_overlay(|state| {
            state.positions.insert(
                key.to_string(),
                CompanionPlacement {
                    x: x.max(0),
                    y: y.max(0),
                },
            );
        })?;
        Ok(self.companion_overlay_state())
    }

    /// Persist the local per-agent show/scale override, never the agent record.
    pub fn save_companion_override(
        &self,
        agent_id: &str,
        show: bool,
        scale: f32,
    ) -> Result<CompanionOverlayState> {
        let scale = clamp_companion_scale(scale);
        self.mutate_overlay(|state| {
            state.overrides.insert(
                agent_id.to_string(),
                CompanionAgentOverride {
                    show: Some(show),
                    scale: Some(scale),
                },
            );
        })?;
        Ok(self.companion_overlay_state())
    }

    /// Forget the local menu override for one agent. Saving the agent settings
    /// panel is an explicit intent, so the panel becomes authoritative again.
    pub fn clear_companion_override(&self, agent_id: &str) -> Result<CompanionOverlayState> {
        self.mutate_overlay(|state| {
            state.overrides.remove(agent_id);
        })?;
        Ok(self.companion_overlay_state())
    }

    fn mutate_overlay(&self, apply: impl FnOnce(&mut CompanionOverlayState)) -> Result<()> {
        let path = overlay_path(&self.desktop.settings_path);
        let _guard = self
            .settings_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("配置锁不可用"))?;
        let mut state = read_overlay(&path);
        apply(&mut state);
        state.updated_at = now_ts();
        write_overlay(&path, &state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(name)
            .join(uuid::Uuid::new_v4().simple().to_string())
            .join("config/desktop.settings.json")
    }

    #[test]
    fn overlay_round_trips_positions_and_overrides() {
        let path = temp_path("wunder-companion-overlay-roundtrip");
        let mut state = CompanionOverlayState::default();
        state.positions.insert(
            "agent-a:global:pet-a".to_string(),
            CompanionPlacement { x: 420, y: 96 },
        );
        state.overrides.insert(
            "agent-a".to_string(),
            CompanionAgentOverride {
                show: Some(false),
                scale: Some(1.2),
            },
        );
        write_overlay(&path, &state).expect("write overlay");
        let loaded = read_overlay(&path);
        assert_eq!(loaded, state, "overlay must round-trip");
        assert!(loaded.message_hints);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn unreadable_overlay_falls_back_to_defaults() {
        let path = temp_path("wunder-companion-overlay-invalid");
        fs::create_dir_all(path.parent().unwrap()).expect("create dir");
        fs::write(&path, "{").expect("write invalid overlay");
        let loaded = read_overlay(&path);
        assert!(loaded.positions.is_empty() && loaded.overrides.is_empty());
        assert!(loaded.message_hints, "hints default to on");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn scale_is_clamped_to_the_web_menu_range() {
        assert_eq!(clamp_companion_scale(0.2), COMPANION_SCALE_MIN);
        assert_eq!(clamp_companion_scale(2.5), COMPANION_SCALE_MAX);
        assert_eq!(clamp_companion_scale(f32::NAN), 1.0);
        assert_eq!(clamp_companion_scale(1.35), 1.35);
    }
}
