use super::SqliteStorage;
use crate::storage::{
    normalize_sandbox_container_id, PresetBoundAgentRecord, StorageLifecycle,
    UserAgentAccessRecord, UserAgentRecord, UserToolAccessRecord,
};
use anyhow::Result;
use rusqlite::{params, params_from_iter, types::Value as SqlValue, OptionalExtension};

/// Canonical `user_agents` projection order; `read_user_agent_row` indexes into it.
const AGENT_COLUMNS: &str = "agent_id, user_id, name, description, system_prompt, model_name, \
     tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, \
     approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, \
     preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids";

/// Preset ids live inside the `preset_binding` JSON payload; the JSON1 extract
/// keeps binding filters and counts in SQL instead of scanning every row.
const BINDING_PRESET_ID: &str = "json_extract(ua.preset_binding, '$.preset_id')";
const BINDING_PRESET_ID_PLAIN: &str = "json_extract(preset_binding, '$.preset_id')";
const BINDING_USABLE: &str = "ua.preset_binding IS NOT NULL AND json_valid(ua.preset_binding)";

/// `IN (...)` chunk size for batched user lookups (well below SQLite's variable cap).
const USER_ID_CHUNK: usize = 400;

fn aliased_agent_columns() -> String {
    AGENT_COLUMNS
        .split(',')
        .map(|column| format!("ua.{}", column.trim()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn placeholders(count: usize) -> String {
    std::iter::repeat_n("?", count)
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) trait SqliteAgentDirectoryStorage {
    fn get_user_tool_access_impl(&self, user_id: &str) -> Result<Option<UserToolAccessRecord>>;
    fn set_user_tool_access_impl(
        &self,
        user_id: &str,
        allowed_tools: Option<&Vec<String>>,
    ) -> Result<()>;
    fn get_user_agent_access_impl(&self, user_id: &str) -> Result<Option<UserAgentAccessRecord>>;
    fn set_user_agent_access_impl(
        &self,
        user_id: &str,
        allowed_agent_ids: Option<&Vec<String>>,
        blocked_agent_ids: Option<&Vec<String>>,
    ) -> Result<()>;
    fn upsert_user_agent_impl(&self, record: &UserAgentRecord) -> Result<()>;
    fn get_user_agent_impl(&self, user_id: &str, agent_id: &str)
        -> Result<Option<UserAgentRecord>>;
    fn get_user_agent_by_id_impl(&self, agent_id: &str) -> Result<Option<UserAgentRecord>>;
    fn list_user_agents_impl(&self, user_id: &str) -> Result<Vec<UserAgentRecord>>;
    fn list_user_agents_for_users_impl(&self, user_ids: &[String]) -> Result<Vec<UserAgentRecord>>;
    fn list_preset_bound_agents_impl(
        &self,
        preset_id: &str,
        keyword: Option<&str>,
        unit_ids: Option<&[String]>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<PresetBoundAgentRecord>, i64)>;
    fn count_preset_bound_users_impl(&self, preset_ids: &[String]) -> Result<Vec<(String, i64)>>;
    fn list_shared_user_agents_impl(&self, user_id: &str) -> Result<Vec<UserAgentRecord>>;
    fn delete_user_agent_impl(&self, user_id: &str, agent_id: &str) -> Result<i64>;
}

impl SqliteAgentDirectoryStorage for SqliteStorage {
    fn get_user_tool_access_impl(&self, user_id: &str) -> Result<Option<UserToolAccessRecord>> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row: Option<(Option<String>, f64)> = conn
            .query_row(
                "SELECT allowed_tools, updated_at FROM user_tool_access WHERE user_id = ?",
                params![cleaned],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some(raw) = row else {
            return Ok(None);
        };
        let allowed_tools = raw
            .0
            .map(|value| Self::parse_string_list(Some(value)))
            .filter(|items| !items.is_empty());
        Ok(Some(UserToolAccessRecord {
            user_id: cleaned.to_string(),
            allowed_tools,
            updated_at: raw.1,
        }))
    }

    fn set_user_tool_access_impl(
        &self,
        user_id: &str,
        allowed_tools: Option<&Vec<String>>,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        let conn = self.open()?;
        let normalized_allowed_tools = allowed_tools.filter(|items| !items.is_empty());
        if normalized_allowed_tools.is_some() {
            let payload = normalized_allowed_tools
                .map(|value| Self::string_list_to_json(value))
                .unwrap_or_else(|| "[]".to_string());
            let now = Self::now_ts();
            conn.execute(
                "INSERT INTO user_tool_access (user_id, allowed_tools, updated_at) VALUES (?, ?, ?) \
                 ON CONFLICT(user_id) DO UPDATE SET allowed_tools = excluded.allowed_tools, updated_at = excluded.updated_at",
                params![cleaned, payload, now],
            )?;
        } else {
            conn.execute(
                "DELETE FROM user_tool_access WHERE user_id = ?",
                params![cleaned],
            )?;
        }
        Ok(())
    }

    fn get_user_agent_access_impl(&self, user_id: &str) -> Result<Option<UserAgentAccessRecord>> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row: Option<(Option<String>, Option<String>, f64)> = conn
            .query_row(
                "SELECT allowed_agent_ids, blocked_agent_ids, updated_at FROM user_agent_access WHERE user_id = ?",
                params![cleaned],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some(raw) = row else {
            return Ok(None);
        };
        Ok(Some(UserAgentAccessRecord {
            user_id: cleaned.to_string(),
            allowed_agent_ids: raw.0.map(|value| Self::parse_string_list(Some(value))),
            blocked_agent_ids: Self::parse_string_list(raw.1),
            updated_at: raw.2,
        }))
    }

    fn set_user_agent_access_impl(
        &self,
        user_id: &str,
        allowed_agent_ids: Option<&Vec<String>>,
        blocked_agent_ids: Option<&Vec<String>>,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        let conn = self.open()?;
        if allowed_agent_ids.is_some() || blocked_agent_ids.is_some() {
            let allowed_payload = allowed_agent_ids
                .map(|value| Self::string_list_to_json(value))
                .unwrap_or_else(|| "[]".to_string());
            let blocked_payload = blocked_agent_ids
                .map(|value| Self::string_list_to_json(value))
                .unwrap_or_else(|| "[]".to_string());
            let now = Self::now_ts();
            conn.execute(
                "INSERT INTO user_agent_access (user_id, allowed_agent_ids, blocked_agent_ids, updated_at) VALUES (?, ?, ?, ?) \
                 ON CONFLICT(user_id) DO UPDATE SET allowed_agent_ids = excluded.allowed_agent_ids, blocked_agent_ids = excluded.blocked_agent_ids, updated_at = excluded.updated_at",
                params![cleaned, allowed_payload, blocked_payload, now],
            )?;
        } else {
            conn.execute(
                "DELETE FROM user_agent_access WHERE user_id = ?",
                params![cleaned],
            )?;
        }
        Ok(())
    }

    fn upsert_user_agent_impl(&self, record: &UserAgentRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let tool_names = if record.tool_names.is_empty() {
            None
        } else {
            Some(Self::string_list_to_json(&record.tool_names))
        };
        let declared_tool_names = if record.declared_tool_names.is_empty() {
            None
        } else {
            Some(Self::string_list_to_json(&record.declared_tool_names))
        };
        let declared_skill_names = if record.declared_skill_names.is_empty() {
            None
        } else {
            Some(Self::string_list_to_json(&record.declared_skill_names))
        };
        let visible_unit_ids = if record.visible_unit_ids.is_empty() {
            None
        } else {
            Some(Self::string_list_to_json(&record.visible_unit_ids))
        };
        let ability_items = if record.ability_items.is_empty() {
            None
        } else {
            serde_json::to_string(&record.ability_items).ok()
        };
        let preset_questions = if record.preset_questions.is_empty() {
            None
        } else {
            Some(Self::string_list_to_json(&record.preset_questions))
        };
        let preset_binding = record
            .preset_binding
            .as_ref()
            .and_then(|value| serde_json::to_string(value).ok());
        let preview_skill = if record.preview_skill { 1 } else { 0 };
        conn.execute(
            "INSERT INTO user_agents (agent_id, user_id, name, description, system_prompt, preview_skill, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, visible_unit_ids) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(agent_id) DO UPDATE SET user_id = excluded.user_id, name = excluded.name, description = excluded.description, \
             system_prompt = excluded.system_prompt, preview_skill = excluded.preview_skill, model_name = excluded.model_name, tool_names = excluded.tool_names, declared_tool_names = excluded.declared_tool_names, declared_skill_names = excluded.declared_skill_names, ability_items = excluded.ability_items, access_level = excluded.access_level, approval_mode = excluded.approval_mode, \
             is_shared = excluded.is_shared, status = excluded.status, icon = excluded.icon, sandbox_container_id = excluded.sandbox_container_id, updated_at = excluded.updated_at, preset_questions = excluded.preset_questions, preset_binding = excluded.preset_binding, silent = excluded.silent, prefer_mother = excluded.prefer_mother, visible_unit_ids = excluded.visible_unit_ids",
            params![
                record.agent_id,
                record.user_id,
                record.name,
                record.description,
                record.system_prompt,
                preview_skill,
                record.model_name,
                tool_names,
                declared_tool_names,
                declared_skill_names,
                ability_items,
                record.access_level,
                record.approval_mode,
                if record.is_shared { 1 } else { 0 },
                record.status,
                record.icon,
                normalize_sandbox_container_id(record.sandbox_container_id),
                record.created_at,
                record.updated_at,
                preset_questions,
                preset_binding,
                if record.silent { 1 } else { 0 },
                if record.prefer_mother { 1 } else { 0 },
                visible_unit_ids
            ],
        )?;
        Ok(())
    }

    fn get_user_agent_impl(
        &self,
        user_id: &str,
        agent_id: &str,
    ) -> Result<Option<UserAgentRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_agent = agent_id.trim();
        if cleaned_user.is_empty() || cleaned_agent.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT agent_id, user_id, name, description, system_prompt, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids FROM user_agents WHERE user_id = ? AND agent_id = ?",
                params![cleaned_user, cleaned_agent],
                Self::read_user_agent_row,
            )
            .optional()?;
        Ok(row)
    }

    fn get_user_agent_by_id_impl(&self, agent_id: &str) -> Result<Option<UserAgentRecord>> {
        self.ensure_initialized()?;
        let cleaned_agent = agent_id.trim();
        if cleaned_agent.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT agent_id, user_id, name, description, system_prompt, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids FROM user_agents WHERE agent_id = ?",
                params![cleaned_agent],
                Self::read_user_agent_row,
            )
            .optional()?;
        Ok(row)
    }

    fn list_user_agents_impl(&self, user_id: &str) -> Result<Vec<UserAgentRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT agent_id, user_id, name, description, system_prompt, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids FROM user_agents WHERE user_id = ? ORDER BY updated_at DESC",
        )?;
        let rows = stmt
            .query_map(params![cleaned_user], Self::read_user_agent_row)?
            .collect::<std::result::Result<Vec<UserAgentRecord>, _>>()?;
        Ok(rows)
    }

    fn list_user_agents_for_users_impl(&self, user_ids: &[String]) -> Result<Vec<UserAgentRecord>> {
        self.ensure_initialized()?;
        let mut cleaned = user_ids
            .iter()
            .map(|user_id| user_id.trim().to_string())
            .filter(|user_id| !user_id.is_empty())
            .collect::<Vec<_>>();
        cleaned.sort();
        cleaned.dedup();
        if cleaned.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let mut output = Vec::new();
        for chunk in cleaned.chunks(USER_ID_CHUNK) {
            let sql = format!(
                "SELECT {AGENT_COLUMNS} FROM user_agents WHERE user_id IN ({}) ORDER BY updated_at DESC",
                placeholders(chunk.len())
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt
                .query_map(params_from_iter(chunk.iter()), Self::read_user_agent_row)?
                .collect::<std::result::Result<Vec<UserAgentRecord>, _>>()?;
            output.extend(rows);
        }
        Ok(output)
    }

    fn list_preset_bound_agents_impl(
        &self,
        preset_id: &str,
        keyword: Option<&str>,
        unit_ids: Option<&[String]>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<PresetBoundAgentRecord>, i64)> {
        self.ensure_initialized()?;
        let cleaned_preset = preset_id.trim();
        if cleaned_preset.is_empty() {
            return Ok((Vec::new(), 0));
        }
        let mut conditions = vec![
            BINDING_USABLE.to_string(),
            format!("{BINDING_PRESET_ID} = ?"),
        ];
        let mut params_list = vec![SqlValue::from(cleaned_preset.to_string())];
        if let Some(keyword) = keyword.map(str::trim).filter(|value| !value.is_empty()) {
            let pattern = format!("%{keyword}%");
            conditions.push("(u.username LIKE ? OR ua.name LIKE ?)".to_string());
            params_list.push(SqlValue::from(pattern.clone()));
            params_list.push(SqlValue::from(pattern));
        }
        if let Some(unit_ids) = unit_ids.filter(|ids| !ids.is_empty()) {
            conditions.push(format!("u.unit_id IN ({})", placeholders(unit_ids.len())));
            for unit_id in unit_ids {
                params_list.push(SqlValue::from(unit_id.clone()));
            }
        }
        let where_sql = conditions.join(" AND ");
        let conn = self.open()?;
        let count_sql = format!(
            "SELECT COUNT(*) FROM user_agents ua JOIN user_accounts u ON u.user_id = ua.user_id WHERE {where_sql}"
        );
        let total: i64 =
            conn.query_row(&count_sql, params_from_iter(params_list.iter()), |row| {
                row.get(0)
            })?;

        let mut sql = format!(
            "SELECT {}, u.username FROM user_agents ua JOIN user_accounts u ON u.user_id = ua.user_id WHERE {where_sql} ORDER BY u.username ASC",
            aliased_agent_columns()
        );
        let mut page_params = params_list;
        if limit > 0 {
            sql.push_str(" LIMIT ? OFFSET ?");
            page_params.push(SqlValue::from(limit));
            page_params.push(SqlValue::from(offset.max(0)));
        }
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(page_params.iter()), |row| {
                let record = Self::read_user_agent_row(row)?;
                let username: Option<String> = row.get(24)?;
                Ok(PresetBoundAgentRecord {
                    user_id: record.user_id.clone(),
                    username: username.unwrap_or_default(),
                    record,
                })
            })?
            .collect::<std::result::Result<Vec<PresetBoundAgentRecord>, _>>()?;
        Ok((rows, total))
    }

    fn count_preset_bound_users_impl(&self, preset_ids: &[String]) -> Result<Vec<(String, i64)>> {
        self.ensure_initialized()?;
        let mut cleaned = preset_ids
            .iter()
            .map(|preset_id| preset_id.trim().to_string())
            .filter(|preset_id| !preset_id.is_empty())
            .collect::<Vec<_>>();
        cleaned.sort();
        cleaned.dedup();
        if cleaned.is_empty() {
            return Ok(Vec::new());
        }
        let sql = format!(
            "SELECT {BINDING_PRESET_ID_PLAIN} AS preset_id, COUNT(DISTINCT user_id) FROM user_agents \
             WHERE preset_binding IS NOT NULL AND json_valid(preset_binding) \
             AND {BINDING_PRESET_ID_PLAIN} IN ({}) GROUP BY {BINDING_PRESET_ID_PLAIN}",
            placeholders(cleaned.len())
        );
        let conn = self.open()?;
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(cleaned.iter()), |row| {
                let preset_id: Option<String> = row.get(0)?;
                let count: i64 = row.get(1)?;
                Ok((preset_id.unwrap_or_default(), count))
            })?
            .collect::<std::result::Result<Vec<(String, i64)>, _>>()?;
        Ok(rows)
    }

    fn list_shared_user_agents_impl(&self, user_id: &str) -> Result<Vec<UserAgentRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT agent_id, user_id, name, description, system_prompt, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids FROM user_agents WHERE is_shared = 1 AND user_id <> ? ORDER BY updated_at DESC",
        )?;
        let rows = stmt
            .query_map(params![cleaned_user], Self::read_user_agent_row)?
            .collect::<std::result::Result<Vec<UserAgentRecord>, _>>()?;
        Ok(rows)
    }

    fn delete_user_agent_impl(&self, user_id: &str, agent_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_agent = agent_id.trim();
        if cleaned_user.is_empty() || cleaned_agent.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM user_agents WHERE user_id = ? AND agent_id = ?",
            params![cleaned_user, cleaned_agent],
        )?;
        Ok(affected as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::SqliteStorage;
    use crate::storage::*;
    use tempfile::tempdir;

    fn build_storage() -> (SqliteStorage, tempfile::TempDir) {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("agent-directory-store.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize sqlite");
        (storage, dir)
    }

    fn agent(agent_id: &str, user_id: &str, updated_at: f64) -> UserAgentRecord {
        UserAgentRecord {
            agent_id: agent_id.to_string(),
            user_id: user_id.to_string(),
            name: format!("Agent {agent_id}"),
            description: String::new(),
            system_prompt: "system".to_string(),
            preview_skill: false,
            model_name: Some("model".to_string()),
            ability_items: Vec::new(),
            tool_names: vec!["tool-a".to_string()],
            declared_tool_names: vec!["tool-a".to_string()],
            declared_skill_names: vec!["skill-a".to_string()],
            visible_unit_ids: vec!["unit-a".to_string()],
            preset_questions: vec!["question".to_string()],
            access_level: "private".to_string(),
            approval_mode: "full_auto".to_string(),
            is_shared: false,
            status: "active".to_string(),
            icon: None,
            sandbox_container_id: 1,
            created_at: updated_at,
            updated_at,
            preset_binding: None,
            silent: false,
            prefer_mother: false,
        }
    }

    #[test]
    fn agent_directory_roundtrip() {
        let (storage, _dir) = build_storage();
        let tools = vec!["tool-a".to_string(), "tool-b".to_string()];
        let allowed_agents = vec!["agent-a".to_string()];
        let blocked_agents = vec!["agent-c".to_string()];

        storage
            .set_user_tool_access("user-a", Some(&tools))
            .expect("set tool access");
        assert_eq!(
            storage
                .get_user_tool_access("user-a")
                .expect("get tool access")
                .and_then(|record| record.allowed_tools),
            Some(tools)
        );
        storage
            .set_user_tool_access("user-a", None)
            .expect("clear tool access");
        assert!(storage
            .get_user_tool_access("user-a")
            .expect("cleared tool access")
            .is_none());

        storage
            .set_user_agent_access("user-a", Some(&allowed_agents), Some(&blocked_agents))
            .expect("set agent access");
        let access = storage
            .get_user_agent_access("user-a")
            .expect("get agent access")
            .expect("agent access");
        assert_eq!(access.allowed_agent_ids, Some(allowed_agents));
        assert_eq!(access.blocked_agent_ids, blocked_agents);

        storage
            .upsert_user_agent(&agent("agent-a", "user-a", 1.0))
            .expect("upsert agent");
        let mut shared = agent("agent-c", "user-b", 3.0);
        shared.is_shared = true;
        storage
            .upsert_user_agent(&shared)
            .expect("upsert shared agent");

        assert_eq!(
            storage
                .list_user_agents("user-a")
                .expect("list agents")
                .iter()
                .map(|record| record.agent_id.as_str())
                .collect::<Vec<_>>(),
            vec!["agent-a"]
        );
        assert_eq!(
            storage
                .list_shared_user_agents("user-a")
                .expect("list shared agents")
                .iter()
                .map(|record| record.agent_id.as_str())
                .collect::<Vec<_>>(),
            vec!["agent-c"]
        );
        assert_eq!(
            storage
                .get_user_agent("user-a", "agent-a")
                .expect("get agent")
                .map(|record| record.name),
            Some("Agent agent-a".to_string())
        );
        assert_eq!(
            storage
                .delete_user_agent("user-a", "agent-a")
                .expect("delete agent"),
            1
        );
    }
}
