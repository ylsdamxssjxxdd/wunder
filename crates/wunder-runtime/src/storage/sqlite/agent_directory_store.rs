use super::SqliteStorage;
use crate::storage::{
    normalize_hive_id, normalize_sandbox_container_id, HiveRecord, StorageBackend, TeamRunRecord,
    TeamTaskRecord, UserAgentAccessRecord, UserAgentRecord, UserToolAccessRecord, DEFAULT_HIVE_ID,
};
use anyhow::Result;
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, OptionalExtension};

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
    fn list_user_agents_by_hive_impl(
        &self,
        user_id: &str,
        hive_id: &str,
    ) -> Result<Vec<UserAgentRecord>>;
    fn list_shared_user_agents_impl(&self, user_id: &str) -> Result<Vec<UserAgentRecord>>;
    fn delete_user_agent_impl(&self, user_id: &str, agent_id: &str) -> Result<i64>;
    fn upsert_hive_impl(&self, record: &HiveRecord) -> Result<()>;
    fn get_hive_impl(&self, user_id: &str, hive_id: &str) -> Result<Option<HiveRecord>>;
    fn list_hives_impl(&self, user_id: &str, include_archived: bool) -> Result<Vec<HiveRecord>>;
    fn delete_hive_impl(&self, user_id: &str, hive_id: &str) -> Result<i64>;
    fn move_agents_to_hive_impl(
        &self,
        user_id: &str,
        hive_id: &str,
        agent_ids: &[String],
    ) -> Result<i64>;
    fn upsert_team_run_impl(&self, record: &TeamRunRecord) -> Result<()>;
    fn delete_team_runs_by_hive_impl(&self, user_id: &str, hive_id: &str) -> Result<i64>;
    fn get_team_run_impl(&self, team_run_id: &str) -> Result<Option<TeamRunRecord>>;
    fn list_team_runs_impl(
        &self,
        user_id: &str,
        hive_id: Option<&str>,
        parent_session_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<TeamRunRecord>, i64)>;
    fn list_team_runs_by_status_impl(
        &self,
        statuses: &[&str],
        offset: i64,
        limit: i64,
    ) -> Result<Vec<TeamRunRecord>>;
    fn upsert_team_task_impl(&self, record: &TeamTaskRecord) -> Result<()>;
    fn list_team_tasks_impl(&self, team_run_id: &str) -> Result<Vec<TeamTaskRecord>>;
    fn get_team_task_impl(&self, task_id: &str) -> Result<Option<TeamTaskRecord>>;
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
        let hive_id = normalize_hive_id(&record.hive_id);
        let preview_skill = if record.preview_skill { 1 } else { 0 };
        conn.execute(
            "INSERT INTO user_agents (agent_id, user_id, hive_id, name, description, system_prompt, preview_skill, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, visible_unit_ids) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(agent_id) DO UPDATE SET user_id = excluded.user_id, hive_id = excluded.hive_id, name = excluded.name, description = excluded.description, \
             system_prompt = excluded.system_prompt, preview_skill = excluded.preview_skill, model_name = excluded.model_name, tool_names = excluded.tool_names, declared_tool_names = excluded.declared_tool_names, declared_skill_names = excluded.declared_skill_names, ability_items = excluded.ability_items, access_level = excluded.access_level, approval_mode = excluded.approval_mode, \
             is_shared = excluded.is_shared, status = excluded.status, icon = excluded.icon, sandbox_container_id = excluded.sandbox_container_id, updated_at = excluded.updated_at, preset_questions = excluded.preset_questions, preset_binding = excluded.preset_binding, silent = excluded.silent, prefer_mother = excluded.prefer_mother, visible_unit_ids = excluded.visible_unit_ids",
            params![
                record.agent_id,
                record.user_id,
                hive_id,
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
                "SELECT agent_id, user_id, hive_id, name, description, system_prompt, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids FROM user_agents WHERE user_id = ? AND agent_id = ?",
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
                "SELECT agent_id, user_id, hive_id, name, description, system_prompt, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids FROM user_agents WHERE agent_id = ?",
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
            "SELECT agent_id, user_id, hive_id, name, description, system_prompt, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids FROM user_agents WHERE user_id = ? ORDER BY updated_at DESC",
        )?;
        let rows = stmt
            .query_map(params![cleaned_user], Self::read_user_agent_row)?
            .collect::<std::result::Result<Vec<UserAgentRecord>, _>>()?;
        Ok(rows)
    }

    fn list_user_agents_by_hive_impl(
        &self,
        user_id: &str,
        hive_id: &str,
    ) -> Result<Vec<UserAgentRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(Vec::new());
        }
        let normalized_hive_id = normalize_hive_id(hive_id);
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT agent_id, user_id, hive_id, name, description, system_prompt, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids FROM user_agents WHERE user_id = ? AND hive_id = ? ORDER BY updated_at DESC",
        )?;
        let rows = stmt
            .query_map(
                params![cleaned_user, normalized_hive_id],
                Self::read_user_agent_row,
            )?
            .collect::<std::result::Result<Vec<UserAgentRecord>, _>>()?;
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
            "SELECT agent_id, user_id, hive_id, name, description, system_prompt, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids FROM user_agents WHERE is_shared = 1 AND user_id <> ? ORDER BY updated_at DESC",
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

    fn upsert_hive_impl(&self, record: &HiveRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let hive_id = normalize_hive_id(&record.hive_id);
        conn.execute(
            "INSERT INTO hives (hive_id, user_id, name, description, is_default, status, created_time, updated_time)              VALUES (?, ?, ?, ?, ?, ?, ?, ?)              ON CONFLICT(hive_id) DO UPDATE SET user_id = excluded.user_id, name = excluded.name, description = excluded.description,              is_default = excluded.is_default, status = excluded.status, updated_time = excluded.updated_time",
            params![
                hive_id,
                record.user_id,
                record.name,
                record.description,
                if record.is_default { 1 } else { 0 },
                record.status,
                record.created_time,
                record.updated_time,
            ],
        )?;
        Ok(())
    }

    fn get_hive_impl(&self, user_id: &str, hive_id: &str) -> Result<Option<HiveRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let normalized_hive_id = normalize_hive_id(hive_id);
        if cleaned_user.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT hive_id, user_id, name, description, is_default, status, created_time, updated_time                  FROM hives WHERE user_id = ? AND hive_id = ?",
                params![cleaned_user, normalized_hive_id],
                |row| {
                    let is_default: Option<i64> = row.get(4)?;
                    Ok(HiveRecord {
                        hive_id: normalize_hive_id(&row.get::<_, String>(0)?),
                        user_id: row.get(1)?,
                        name: row.get(2)?,
                        description: row.get(3)?,
                        is_default: is_default.unwrap_or(0) != 0,
                        status: row.get(5)?,
                        created_time: row.get(6)?,
                        updated_time: row.get(7)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn list_hives_impl(&self, user_id: &str, include_archived: bool) -> Result<Vec<HiveRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let mut sql = String::from(
            "SELECT hive_id, user_id, name, description, is_default, status, created_time, updated_time              FROM hives WHERE user_id = ?",
        );
        if !include_archived {
            sql.push_str(" AND status <> 'archived'");
        }
        sql.push_str(" ORDER BY is_default DESC, updated_time DESC");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params![cleaned_user], |row| {
                let is_default: Option<i64> = row.get(4)?;
                Ok(HiveRecord {
                    hive_id: normalize_hive_id(&row.get::<_, String>(0)?),
                    user_id: row.get(1)?,
                    name: row.get(2)?,
                    description: row.get(3)?,
                    is_default: is_default.unwrap_or(0) != 0,
                    status: row.get(5)?,
                    created_time: row.get(6)?,
                    updated_time: row.get(7)?,
                })
            })?
            .collect::<std::result::Result<Vec<HiveRecord>, _>>()?;
        Ok(rows)
    }

    fn delete_hive_impl(&self, user_id: &str, hive_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let normalized_hive_id = normalize_hive_id(hive_id);
        if cleaned_user.is_empty() || normalized_hive_id == DEFAULT_HIVE_ID {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM hives WHERE user_id = ? AND hive_id = ? AND is_default = 0",
            params![cleaned_user, normalized_hive_id],
        )?;
        Ok(affected as i64)
    }

    fn move_agents_to_hive_impl(
        &self,
        user_id: &str,
        hive_id: &str,
        agent_ids: &[String],
    ) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() || agent_ids.is_empty() {
            return Ok(0);
        }
        let normalized_hive_id = normalize_hive_id(hive_id);
        let conn = self.open()?;
        let mut cleaned_ids = Vec::new();
        for agent_id in agent_ids {
            let cleaned = agent_id.trim();
            if !cleaned.is_empty() {
                cleaned_ids.push(cleaned.to_string());
            }
        }
        if cleaned_ids.is_empty() {
            return Ok(0);
        }
        let placeholders = std::iter::repeat_n("?", cleaned_ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "UPDATE user_agents SET hive_id = ?, updated_at = ? WHERE user_id = ? AND agent_id IN ({placeholders})"
        );
        let now = Self::now_ts();
        let mut values: Vec<SqlValue> = Vec::with_capacity(cleaned_ids.len() + 3);
        values.push(SqlValue::from(normalized_hive_id));
        values.push(SqlValue::from(now));
        values.push(SqlValue::from(cleaned_user.to_string()));
        for agent_id in cleaned_ids {
            values.push(SqlValue::from(agent_id));
        }
        let affected = conn.execute(&sql, params_from_iter(values))?;
        Ok(affected as i64)
    }

    fn upsert_team_run_impl(&self, record: &TeamRunRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO team_runs (team_run_id, user_id, hive_id, parent_session_id, parent_agent_id, mother_agent_id, strategy, status, task_total, task_success, task_failed, context_tokens_total, context_tokens_peak, model_round_total, started_time, finished_time, elapsed_s, summary, error, updated_time)              VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)              ON CONFLICT(team_run_id) DO UPDATE SET user_id = excluded.user_id, hive_id = excluded.hive_id, parent_session_id = excluded.parent_session_id, parent_agent_id = excluded.parent_agent_id, mother_agent_id = excluded.mother_agent_id,              strategy = excluded.strategy, status = excluded.status, task_total = excluded.task_total, task_success = excluded.task_success, task_failed = excluded.task_failed,              context_tokens_total = excluded.context_tokens_total, context_tokens_peak = excluded.context_tokens_peak, model_round_total = excluded.model_round_total,              started_time = excluded.started_time, finished_time = excluded.finished_time, elapsed_s = excluded.elapsed_s, summary = excluded.summary, error = excluded.error, updated_time = excluded.updated_time",
            params![
                record.team_run_id,
                record.user_id,
                normalize_hive_id(&record.hive_id),
                record.parent_session_id,
                record.parent_agent_id,
                record.mother_agent_id,
                record.strategy,
                record.status,
                record.task_total,
                record.task_success,
                record.task_failed,
                record.context_tokens_total,
                record.context_tokens_peak,
                record.model_round_total,
                record.started_time,
                record.finished_time,
                record.elapsed_s,
                record.summary,
                record.error,
                record.updated_time,
            ],
        )?;
        Ok(())
    }

    fn delete_team_runs_by_hive_impl(&self, user_id: &str, hive_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let normalized_hive_id = normalize_hive_id(hive_id);
        if cleaned_user.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM team_runs WHERE user_id = ? AND hive_id = ?",
            params![cleaned_user, normalized_hive_id],
        )?;
        Ok(affected as i64)
    }

    fn get_team_run_impl(&self, team_run_id: &str) -> Result<Option<TeamRunRecord>> {
        self.ensure_initialized()?;
        let cleaned = team_run_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT team_run_id, user_id, hive_id, parent_session_id, parent_agent_id, mother_agent_id, strategy, status, task_total, task_success, task_failed, context_tokens_total, context_tokens_peak, model_round_total, started_time, finished_time, elapsed_s, summary, error, updated_time FROM team_runs WHERE team_run_id = ?",
                params![cleaned],
                |row| {
                    Ok(TeamRunRecord {
                        team_run_id: row.get(0)?,
                        user_id: row.get(1)?,
                        hive_id: normalize_hive_id(&row.get::<_, String>(2)?),
                        parent_session_id: row.get(3)?,
                        parent_agent_id: row.get(4)?,
                        mother_agent_id: row.get(5)?,
                        strategy: row.get(6)?,
                        status: row.get(7)?,
                        task_total: row.get(8)?,
                        task_success: row.get(9)?,
                        task_failed: row.get(10)?,
                        context_tokens_total: row.get(11)?,
                        context_tokens_peak: row.get(12)?,
                        model_round_total: row.get(13)?,
                        started_time: row.get(14)?,
                        finished_time: row.get(15)?,
                        elapsed_s: row.get(16)?,
                        summary: row.get(17)?,
                        error: row.get(18)?,
                        updated_time: row.get(19)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn list_team_runs_impl(
        &self,
        user_id: &str,
        hive_id: Option<&str>,
        parent_session_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<TeamRunRecord>, i64)> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok((Vec::new(), 0));
        }
        let conn = self.open()?;
        let mut filters = vec!["user_id = ?".to_string()];
        let mut values: Vec<SqlValue> = vec![SqlValue::from(cleaned_user.to_string())];
        if let Some(hive_id) = hive_id {
            filters.push("hive_id = ?".to_string());
            values.push(SqlValue::from(normalize_hive_id(hive_id)));
        }
        if let Some(parent_session_id) = parent_session_id.map(str::trim).filter(|v| !v.is_empty())
        {
            filters.push("parent_session_id = ?".to_string());
            values.push(SqlValue::from(parent_session_id.to_string()));
        }
        let where_clause = filters.join(" AND ");
        let count_sql = format!("SELECT COUNT(1) FROM team_runs WHERE {where_clause}");
        let total = conn.query_row(&count_sql, params_from_iter(values.clone()), |row| {
            row.get::<_, i64>(0)
        })?;

        let mut query_values = values;
        query_values.push(SqlValue::from(limit.max(1)));
        query_values.push(SqlValue::from(offset.max(0)));
        let query_sql = format!(
            "SELECT team_run_id, user_id, hive_id, parent_session_id, parent_agent_id, mother_agent_id, strategy, status, task_total, task_success, task_failed, context_tokens_total, context_tokens_peak, model_round_total, started_time, finished_time, elapsed_s, summary, error, updated_time              FROM team_runs WHERE {where_clause} ORDER BY updated_time DESC LIMIT ? OFFSET ?"
        );
        let mut stmt = conn.prepare(&query_sql)?;
        let rows = stmt
            .query_map(params_from_iter(query_values), |row| {
                Ok(TeamRunRecord {
                    team_run_id: row.get(0)?,
                    user_id: row.get(1)?,
                    hive_id: normalize_hive_id(&row.get::<_, String>(2)?),
                    parent_session_id: row.get(3)?,
                    parent_agent_id: row.get(4)?,
                    mother_agent_id: row.get(5)?,
                    strategy: row.get(6)?,
                    status: row.get(7)?,
                    task_total: row.get(8)?,
                    task_success: row.get(9)?,
                    task_failed: row.get(10)?,
                    context_tokens_total: row.get(11)?,
                    context_tokens_peak: row.get(12)?,
                    model_round_total: row.get(13)?,
                    started_time: row.get(14)?,
                    finished_time: row.get(15)?,
                    elapsed_s: row.get(16)?,
                    summary: row.get(17)?,
                    error: row.get(18)?,
                    updated_time: row.get(19)?,
                })
            })?
            .collect::<std::result::Result<Vec<TeamRunRecord>, _>>()?;
        Ok((rows, total))
    }

    fn list_team_runs_by_status_impl(
        &self,
        statuses: &[&str],
        offset: i64,
        limit: i64,
    ) -> Result<Vec<TeamRunRecord>> {
        self.ensure_initialized()?;
        let mut cleaned_statuses = statuses
            .iter()
            .map(|status| status.trim().to_string())
            .filter(|status| !status.is_empty())
            .collect::<Vec<_>>();
        cleaned_statuses.sort();
        cleaned_statuses.dedup();
        if cleaned_statuses.is_empty() {
            return Ok(Vec::new());
        }

        let conn = self.open()?;
        let placeholders = vec!["?"; cleaned_statuses.len()].join(",");
        let query_sql = format!(
            "SELECT team_run_id, user_id, hive_id, parent_session_id, parent_agent_id, mother_agent_id, strategy, status, task_total, task_success, task_failed, context_tokens_total, context_tokens_peak, model_round_total, started_time, finished_time, elapsed_s, summary, error, updated_time              FROM team_runs WHERE status IN ({placeholders}) ORDER BY updated_time ASC LIMIT ? OFFSET ?"
        );
        let mut values = cleaned_statuses
            .into_iter()
            .map(SqlValue::from)
            .collect::<Vec<_>>();
        values.push(SqlValue::from(limit.max(1)));
        values.push(SqlValue::from(offset.max(0)));

        let mut stmt = conn.prepare(&query_sql)?;
        let rows = stmt
            .query_map(params_from_iter(values), |row| {
                Ok(TeamRunRecord {
                    team_run_id: row.get(0)?,
                    user_id: row.get(1)?,
                    hive_id: normalize_hive_id(&row.get::<_, String>(2)?),
                    parent_session_id: row.get(3)?,
                    parent_agent_id: row.get(4)?,
                    mother_agent_id: row.get(5)?,
                    strategy: row.get(6)?,
                    status: row.get(7)?,
                    task_total: row.get(8)?,
                    task_success: row.get(9)?,
                    task_failed: row.get(10)?,
                    context_tokens_total: row.get(11)?,
                    context_tokens_peak: row.get(12)?,
                    model_round_total: row.get(13)?,
                    started_time: row.get(14)?,
                    finished_time: row.get(15)?,
                    elapsed_s: row.get(16)?,
                    summary: row.get(17)?,
                    error: row.get(18)?,
                    updated_time: row.get(19)?,
                })
            })?
            .collect::<std::result::Result<Vec<TeamRunRecord>, _>>()?;
        Ok(rows)
    }

    fn upsert_team_task_impl(&self, record: &TeamTaskRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO team_tasks (task_id, team_run_id, user_id, hive_id, agent_id, target_session_id, spawned_session_id, session_run_id, status, retry_count, priority, started_time, finished_time, elapsed_s, result_summary, error, updated_time)              VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)              ON CONFLICT(task_id) DO UPDATE SET team_run_id = excluded.team_run_id, user_id = excluded.user_id, hive_id = excluded.hive_id, agent_id = excluded.agent_id,              target_session_id = excluded.target_session_id, spawned_session_id = excluded.spawned_session_id, session_run_id = excluded.session_run_id, status = excluded.status, retry_count = excluded.retry_count,              priority = excluded.priority, started_time = excluded.started_time, finished_time = excluded.finished_time, elapsed_s = excluded.elapsed_s,              result_summary = excluded.result_summary, error = excluded.error, updated_time = excluded.updated_time",
            params![
                record.task_id,
                record.team_run_id,
                record.user_id,
                normalize_hive_id(&record.hive_id),
                record.agent_id,
                record.target_session_id,
                record.spawned_session_id,
                record.session_run_id,
                record.status,
                record.retry_count,
                record.priority,
                record.started_time,
                record.finished_time,
                record.elapsed_s,
                record.result_summary,
                record.error,
                record.updated_time,
            ],
        )?;
        Ok(())
    }

    fn list_team_tasks_impl(&self, team_run_id: &str) -> Result<Vec<TeamTaskRecord>> {
        self.ensure_initialized()?;
        let cleaned_run_id = team_run_id.trim();
        if cleaned_run_id.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT task_id, team_run_id, user_id, hive_id, agent_id, target_session_id, spawned_session_id, session_run_id, status, retry_count, priority, started_time, finished_time, elapsed_s, result_summary, error, updated_time              FROM team_tasks WHERE team_run_id = ? ORDER BY updated_time DESC",
        )?;
        let rows = stmt
            .query_map(params![cleaned_run_id], |row| {
                Ok(TeamTaskRecord {
                    task_id: row.get(0)?,
                    team_run_id: row.get(1)?,
                    user_id: row.get(2)?,
                    hive_id: normalize_hive_id(&row.get::<_, String>(3)?),
                    agent_id: row.get(4)?,
                    target_session_id: row.get(5)?,
                    spawned_session_id: row.get(6)?,
                    session_run_id: row.get(7)?,
                    status: row.get(8)?,
                    retry_count: row.get(9)?,
                    priority: row.get(10)?,
                    started_time: row.get(11)?,
                    finished_time: row.get(12)?,
                    elapsed_s: row.get(13)?,
                    result_summary: row.get(14)?,
                    error: row.get(15)?,
                    updated_time: row.get(16)?,
                })
            })?
            .collect::<std::result::Result<Vec<TeamTaskRecord>, _>>()?;
        Ok(rows)
    }

    fn get_team_task_impl(&self, task_id: &str) -> Result<Option<TeamTaskRecord>> {
        self.ensure_initialized()?;
        let cleaned = task_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT task_id, team_run_id, user_id, hive_id, agent_id, target_session_id, spawned_session_id, session_run_id, status, retry_count, priority, started_time, finished_time, elapsed_s, result_summary, error, updated_time FROM team_tasks WHERE task_id = ?",
                params![cleaned],
                |row| {
                    Ok(TeamTaskRecord {
                        task_id: row.get(0)?,
                        team_run_id: row.get(1)?,
                        user_id: row.get(2)?,
                        hive_id: normalize_hive_id(&row.get::<_, String>(3)?),
                        agent_id: row.get(4)?,
                        target_session_id: row.get(5)?,
                        spawned_session_id: row.get(6)?,
                        session_run_id: row.get(7)?,
                        status: row.get(8)?,
                        retry_count: row.get(9)?,
                        priority: row.get(10)?,
                        started_time: row.get(11)?,
                        finished_time: row.get(12)?,
                        elapsed_s: row.get(13)?,
                        result_summary: row.get(14)?,
                        error: row.get(15)?,
                        updated_time: row.get(16)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }
}

#[cfg(test)]
mod tests {
    use super::SqliteStorage;
    use crate::storage::{
        HiveRecord, StorageBackend, TeamRunRecord, TeamTaskRecord, UserAgentRecord, DEFAULT_HIVE_ID,
    };
    use tempfile::tempdir;

    fn build_storage() -> (SqliteStorage, tempfile::TempDir) {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("agent-directory-store.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize sqlite");
        (storage, dir)
    }

    fn hive(hive_id: &str, user_id: &str, name: &str, is_default: bool) -> HiveRecord {
        HiveRecord {
            hive_id: hive_id.to_string(),
            user_id: user_id.to_string(),
            name: name.to_string(),
            description: String::new(),
            is_default,
            status: "active".to_string(),
            created_time: 1.0,
            updated_time: 1.0,
        }
    }

    fn agent(agent_id: &str, user_id: &str, hive_id: &str, updated_at: f64) -> UserAgentRecord {
        UserAgentRecord {
            agent_id: agent_id.to_string(),
            user_id: user_id.to_string(),
            hive_id: hive_id.to_string(),
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

    fn team_run(
        team_run_id: &str,
        user_id: &str,
        hive_id: &str,
        status: &str,
        updated_time: f64,
    ) -> TeamRunRecord {
        TeamRunRecord {
            team_run_id: team_run_id.to_string(),
            user_id: user_id.to_string(),
            hive_id: hive_id.to_string(),
            parent_session_id: "session-a".to_string(),
            parent_agent_id: Some("agent-a".to_string()),
            mother_agent_id: Some("agent-a".to_string()),
            strategy: "standard".to_string(),
            status: status.to_string(),
            task_total: 1,
            task_success: 0,
            task_failed: 0,
            context_tokens_total: 0,
            context_tokens_peak: 0,
            model_round_total: 0,
            started_time: Some(updated_time),
            finished_time: None,
            elapsed_s: None,
            summary: None,
            error: None,
            updated_time,
        }
    }

    fn team_task(task_id: &str, team_run_id: &str, user_id: &str, hive_id: &str) -> TeamTaskRecord {
        TeamTaskRecord {
            task_id: task_id.to_string(),
            team_run_id: team_run_id.to_string(),
            user_id: user_id.to_string(),
            hive_id: hive_id.to_string(),
            agent_id: "agent-a".to_string(),
            target_session_id: Some("session-a".to_string()),
            spawned_session_id: Some("session-b".to_string()),
            session_run_id: Some("run-a".to_string()),
            status: "pending".to_string(),
            retry_count: 1,
            priority: 2,
            started_time: None,
            finished_time: None,
            elapsed_s: None,
            result_summary: Some("summary".to_string()),
            error: None,
            updated_time: 4.0,
        }
    }

    #[test]
    fn agent_directory_hive_and_team_roundtrip() {
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
            .upsert_hive(&hive(DEFAULT_HIVE_ID, "user-a", "Default", true))
            .expect("upsert default hive");
        storage
            .upsert_hive(&hive("hive-a", "user-a", "Hive A", false))
            .expect("upsert hive");
        storage
            .upsert_user_agent(&agent("agent-a", "user-a", DEFAULT_HIVE_ID, 1.0))
            .expect("upsert default agent");
        storage
            .upsert_user_agent(&agent("agent-b", "user-a", "hive-a", 2.0))
            .expect("upsert hive agent");
        let mut shared = agent("agent-c", "user-b", "hive-b", 3.0);
        shared.is_shared = true;
        storage
            .upsert_user_agent(&shared)
            .expect("upsert shared agent");

        assert_eq!(
            storage
                .list_user_agents_by_hive("user-a", "hive-a")
                .expect("list hive agents")
                .iter()
                .map(|record| record.agent_id.as_str())
                .collect::<Vec<_>>(),
            vec!["agent-b"]
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
                .move_agents_to_hive("user-a", "hive-a", &["agent-a".to_string()])
                .expect("move agent"),
            1
        );
        assert_eq!(
            storage
                .get_user_agent("user-a", "agent-a")
                .expect("get moved agent")
                .map(|record| record.hive_id),
            Some("hive-a".to_string())
        );
        assert_eq!(
            storage
                .delete_hive("user-a", DEFAULT_HIVE_ID)
                .expect("protect default hive"),
            0
        );
        assert_eq!(
            storage
                .list_hives("user-a", false)
                .expect("list hives")
                .len(),
            2
        );

        storage
            .upsert_team_run(&team_run("team-run-a", "user-a", "hive-a", "running", 5.0))
            .expect("upsert team run");
        storage
            .upsert_team_task(&team_task("team-task-a", "team-run-a", "user-a", "hive-a"))
            .expect("upsert team task");
        let (team_runs, team_run_total) = storage
            .list_team_runs("user-a", Some("hive-a"), Some("session-a"), 0, 8)
            .expect("list team runs");
        assert_eq!(team_run_total, 1);
        assert_eq!(
            team_runs
                .iter()
                .map(|record| record.team_run_id.as_str())
                .collect::<Vec<_>>(),
            vec!["team-run-a"]
        );
        assert_eq!(
            storage
                .list_team_runs_by_status(&["running"], 0, 8)
                .expect("list team runs by status")
                .iter()
                .map(|record| record.team_run_id.as_str())
                .collect::<Vec<_>>(),
            vec!["team-run-a"]
        );
        assert_eq!(
            storage
                .get_team_task("team-task-a")
                .expect("get team task")
                .map(|record| record.retry_count),
            Some(1)
        );
        assert_eq!(
            storage
                .list_team_tasks("team-run-a")
                .expect("list team tasks")
                .len(),
            1
        );
        assert_eq!(
            storage
                .delete_team_runs_by_hive("user-a", "hive-a")
                .expect("delete team runs"),
            1
        );
    }
}
