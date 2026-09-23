//! Small, secret-free profile projection for the native desktop surface.
use super::NativeDesktop;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

const PROFILE_PREFS_PREFIX: &str = "user_preferences:v1:";
const DEFAULT_AVATAR_ICON: &str = "initial";
const DEFAULT_AVATAR_COLOR: &str = "#3b82f6";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NativeProfile {
    pub user_id: String,
    pub username: String,
    pub email: String,
    pub unit: String,
    pub sessions: i64,
    pub sessions_last_7d: i64,
    pub tool_calls: i64,
    pub consumed_tokens: i64,
    pub agents: i64,
    pub last_active_at: String,
    pub avatar_icon: String,
    pub avatar_color: String,
}

impl NativeDesktop {
    pub fn get_profile(&self) -> Result<NativeProfile> {
        let user = self
            .state()
            .user_store
            .get_user_by_id(self.user_id())?
            .ok_or_else(|| anyhow!("用户不存在"))?;
        let (sessions, total) = self.state().storage.list_chat_sessions_by_status(
            self.user_id(),
            None,
            None,
            Some("active"),
            0,
            0,
        )?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_secs_f64())
            .unwrap_or_default();
        let week_ago = now - 7.0 * 86_400.0;
        let sessions_last_7d = sessions
            .iter()
            .filter(|item| {
                item.last_message_at
                    .max(item.updated_at)
                    .max(item.created_at)
                    >= week_ago
            })
            .count() as i64;
        let last_active_at = sessions
            .iter()
            .map(|item| item.last_message_at.max(item.updated_at))
            .fold(0.0, f64::max);
        let usage = self.state().workspace.get_user_usage_stats();
        let tool_calls = usage
            .get(self.user_id())
            .and_then(|row| row.get("tool_records"))
            .copied()
            .unwrap_or(0)
            .max(0);
        let agents = self.list_agents()?.len() as i64;
        let prefs = self
            .state()
            .user_store
            .get_meta(&format!("{PROFILE_PREFS_PREFIX}{}", self.user_id()))?
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
            .unwrap_or_default();
        let unit = user
            .unit_id
            .as_deref()
            .and_then(|id| self.state().user_store.get_org_unit(id).ok().flatten())
            .map(|record| record.name)
            .unwrap_or_default();
        Ok(NativeProfile {
            user_id: user.user_id,
            username: user.username,
            email: user.email.unwrap_or_default(),
            unit,
            sessions: total.max(0),
            sessions_last_7d,
            tool_calls,
            consumed_tokens: self
                .state()
                .monitor
                .sum_consumed_tokens_by_user(self.user_id())
                .max(0),
            agents,
            last_active_at: if last_active_at > 0.0 {
                chrono::DateTime::from_timestamp(last_active_at as i64, 0)
                    .map(|value| value.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_default()
            } else {
                String::new()
            },
            avatar_icon: normalize_avatar_icon(
                prefs.get("avatar_icon").and_then(|value| value.as_str()),
            ),
            avatar_color: normalize_avatar_color(
                prefs.get("avatar_color").and_then(|value| value.as_str()),
            ),
        })
    }

    pub fn save_profile_avatar(&self, icon: &str, color: &str) -> Result<NativeProfile> {
        let key = format!("{PROFILE_PREFS_PREFIX}{}", self.user_id());
        let icon = normalize_avatar_icon(Some(icon));
        let color = normalize_avatar_color(Some(color));
        let mut value = self
            .state()
            .user_store
            .get_meta(&key)?
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        let object = value
            .as_object_mut()
            .ok_or_else(|| anyhow!("个人偏好格式无效"))?;
        object.insert("avatar_icon".into(), serde_json::Value::String(icon));
        object.insert("avatar_color".into(), serde_json::Value::String(color));
        self.state()
            .user_store
            .set_meta(&key, &serde_json::to_string(&value)?)?;
        self.get_profile()
    }
}

fn normalize_avatar_icon(raw: Option<&str>) -> String {
    let value = raw.unwrap_or_default().trim().to_ascii_lowercase();
    if value == DEFAULT_AVATAR_ICON {
        return value;
    }
    let Some(number) = value.strip_prefix("qq-avatar-") else {
        return DEFAULT_AVATAR_ICON.to_string();
    };
    if number.len() != 4 || !number.chars().all(|character| character.is_ascii_digit()) {
        DEFAULT_AVATAR_ICON.to_string()
    } else {
        format!("qq-avatar-{number}")
    }
}

fn normalize_avatar_color(raw: Option<&str>) -> String {
    let value = raw.unwrap_or_default().trim().to_ascii_lowercase();
    if value.len() == 7
        && value.starts_with('#')
        && value
            .chars()
            .skip(1)
            .all(|character| character.is_ascii_hexdigit())
    {
        value
    } else {
        DEFAULT_AVATAR_COLOR.to_string()
    }
}
