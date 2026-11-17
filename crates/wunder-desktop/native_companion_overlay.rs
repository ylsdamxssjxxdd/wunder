//! Persisted state for the native floating companion (desktop pet).
//!
//! This mirrors the web layer's companion settings — the master switch, the
//! selected companion, the global scale, the message-hint switch and the
//! per-entry drag positions — but lives in its own file next to the desktop
//! settings so dragging a pet never rewrites runtime configuration. Position
//! keys match the web ones: `desktop:<companionId>`. Visibility has a single
//! source of truth (`enabled`), so a pet hidden from its context menu can
//! always be brought back from the settings switch.

use super::{now_ts, NativeDesktop};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Settings slider range. The desktop pet may shrink to 0.1 (a minimal
/// footprint) while the upper bound stays at the web value of 1.6.
pub const COMPANION_SCALE_MIN: f32 = 0.1;
pub const COMPANION_SCALE_MAX: f32 = 1.6;

/// Half size keeps the pet a compact desktop companion out of the box.
pub const DEFAULT_COMPANION_SCALE: f32 = 0.5;

pub fn clamp_companion_scale(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(COMPANION_SCALE_MIN, COMPANION_SCALE_MAX)
    } else {
        DEFAULT_COMPANION_SCALE
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct CompanionPlacement {
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
}

fn default_message_hints() -> bool {
    true
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompanionOverlayState {
    /// Web `settings.messageHintsEnabled`; the desktop pet only bubbles hints
    /// while this switch is on.
    #[serde(default = "default_message_hints")]
    pub message_hints: bool,
    /// Web `settings.enabled`: the desktop pet master switch. Off hides the pet
    /// no matter which companion is selected.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Web `settings.selectedId`: the single companion the floating pet follows.
    /// Empty means nothing is selected, so no pet is projected.
    #[serde(default)]
    pub selected_id: String,
    /// Web `settings.scale`: the global pet scale.
    #[serde(default = "default_scale")]
    pub scale: f32,
    #[serde(default)]
    pub positions: HashMap<String, CompanionPlacement>,
    #[serde(default)]
    pub updated_at: f64,
}

fn default_scale() -> f32 {
    DEFAULT_COMPANION_SCALE
}

impl Default for CompanionOverlayState {
    fn default() -> Self {
        Self {
            message_hints: true,
            enabled: true,
            selected_id: String::new(),
            scale: DEFAULT_COMPANION_SCALE,
            positions: HashMap::new(),
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

    /// Persist the desktop pet's global settings, the desktop counterpart of the
    /// web `companionStore.settings` block: master switch, selected companion,
    /// global scale and message-hint switch.
    pub fn save_companion_settings(
        &self,
        enabled: bool,
        selected_id: &str,
        scale: f32,
        message_hints: bool,
    ) -> Result<CompanionOverlayState> {
        let scale = clamp_companion_scale(scale);
        let selected_id = selected_id.trim().to_string();
        self.mutate_overlay(|state| {
            state.enabled = enabled;
            state.selected_id = selected_id.clone();
            state.scale = scale;
            state.message_hints = message_hints;
        })?;
        Ok(self.companion_overlay_state())
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
    fn overlay_round_trips_positions() {
        let path = temp_path("wunder-companion-overlay-roundtrip");
        let mut state = CompanionOverlayState::default();
        state.positions.insert(
            "desktop:pet-a".to_string(),
            CompanionPlacement { x: 420, y: 96 },
        );
        write_overlay(&path, &state).expect("write overlay");
        let loaded = read_overlay(&path);
        assert_eq!(loaded, state, "overlay must round-trip");
        assert!(loaded.message_hints);
        assert!(loaded.enabled);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn unreadable_overlay_falls_back_to_defaults() {
        let path = temp_path("wunder-companion-overlay-invalid");
        fs::create_dir_all(path.parent().unwrap()).expect("create dir");
        fs::write(&path, "{").expect("write invalid overlay");
        let loaded = read_overlay(&path);
        assert!(loaded.positions.is_empty());
        assert!(loaded.message_hints, "hints default to on");
        assert!(loaded.enabled, "the pet shows by default");
        assert_eq!(loaded.scale, DEFAULT_COMPANION_SCALE, "half size default");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn legacy_override_files_still_load() {
        let path = temp_path("wunder-companion-overlay-legacy");
        fs::create_dir_all(path.parent().unwrap()).expect("create dir");
        fs::write(
            &path,
            r#"{"message_hints":true,"enabled":true,"selected_id":"pet-a","scale":1.0,
                "positions":{},"overrides":{"desktop:pet-a":{"show":false}},"updated_at":1.0}"#,
        )
        .expect("write legacy overlay");
        let loaded = read_overlay(&path);
        assert_eq!(loaded.selected_id, "pet-a");
        assert!(loaded.positions.is_empty(), "overrides are dropped");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn scale_is_clamped_to_the_settings_range() {
        assert_eq!(clamp_companion_scale(0.05), COMPANION_SCALE_MIN);
        assert_eq!(clamp_companion_scale(0.15), 0.15);
        assert_eq!(clamp_companion_scale(2.5), COMPANION_SCALE_MAX);
        assert_eq!(clamp_companion_scale(f32::NAN), DEFAULT_COMPANION_SCALE);
        assert_eq!(clamp_companion_scale(1.35), 1.35);
    }
}
