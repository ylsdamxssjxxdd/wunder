use super::PostgresStorage;
use crate::storage::{
    normalize_hive_id, normalize_sandbox_container_id, HiveRecord, StorageBackend, TeamRunRecord,
    TeamTaskRecord, UserAgentAccessRecord, UserAgentRecord, UserToolAccessRecord, DEFAULT_HIVE_ID,
};
use anyhow::Result;

pub(super) trait PostgresAgentDirectoryStorage {
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

impl PostgresAgentDirectoryStorage for PostgresStorage {
    fn get_user_tool_access_impl(&self, user_id: &str) -> Result<Option<UserToolAccessRecord>> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT allowed_tools, updated_at FROM user_tool_access WHERE user_id = $1",
            &[&cleaned],
        )?;
        let Some(row) = row else {
            return Ok(None);
        };
        let allowed: Option<String> = row.get(0);
        let updated_at: f64 = row.get(1);
        let allowed_tools = allowed
            .map(|value| Self::parse_string_list(Some(value)))
            .filter(|items| !items.is_empty());
        Ok(Some(UserToolAccessRecord {
            user_id: cleaned.to_string(),
            allowed_tools,
            updated_at,
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
        let mut conn = self.conn()?;
        let normalized_allowed_tools = allowed_tools.filter(|items| !items.is_empty());
        if normalized_allowed_tools.is_some() {
            let payload = normalized_allowed_tools
                .map(|value| Self::string_list_to_json(value))
                .unwrap_or_else(|| "[]".to_string());
            let now = Self::now_ts();
            conn.execute(
                "INSERT INTO user_tool_access (user_id, allowed_tools, updated_at) VALUES ($1, $2, $3) \
                 ON CONFLICT(user_id) DO UPDATE SET allowed_tools = EXCLUDED.allowed_tools, updated_at = EXCLUDED.updated_at",
                &[&cleaned, &payload, &now],
            )?;
        } else {
            conn.execute(
                "DELETE FROM user_tool_access WHERE user_id = $1",
                &[&cleaned],
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
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT allowed_agent_ids, blocked_agent_ids, updated_at FROM user_agent_access WHERE user_id = $1",
            &[&cleaned],
        )?;
        let Some(row) = row else {
            return Ok(None);
        };
        let allowed: Option<String> = row.get(0);
        let blocked: Option<String> = row.get(1);
        let updated_at: f64 = row.get(2);
        Ok(Some(UserAgentAccessRecord {
            user_id: cleaned.to_string(),
            allowed_agent_ids: allowed.map(|value| Self::parse_string_list(Some(value))),
            blocked_agent_ids: Self::parse_string_list(blocked),
            updated_at,
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
        let mut conn = self.conn()?;
        if allowed_agent_ids.is_some() || blocked_agent_ids.is_some() {
            let allowed_payload = allowed_agent_ids
                .map(|value| Self::string_list_to_json(value))
                .unwrap_or_else(|| "[]".to_string());
            let blocked_payload = blocked_agent_ids
                .map(|value| Self::string_list_to_json(value))
                .unwrap_or_else(|| "[]".to_string());
            let now = Self::now_ts();
            conn.execute(
                "INSERT INTO user_agent_access (user_id, allowed_agent_ids, blocked_agent_ids, updated_at) VALUES ($1, $2, $3, $4) \
                 ON CONFLICT(user_id) DO UPDATE SET allowed_agent_ids = EXCLUDED.allowed_agent_ids, blocked_agent_ids = EXCLUDED.blocked_agent_ids, updated_at = EXCLUDED.updated_at",
                &[&cleaned, &allowed_payload, &blocked_payload, &now],
            )?;
        } else {
            conn.execute(
                "DELETE FROM user_agent_access WHERE user_id = $1",
                &[&cleaned],
            )?;
        }
        Ok(())
    }

    fn upsert_user_agent_impl(&self, record: &UserAgentRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
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
        let is_shared = if record.is_shared { 1 } else { 0 };
        let silent = if record.silent { 1 } else { 0 };
        let prefer_mother = if record.prefer_mother { 1 } else { 0 };
        let preview_skill = if record.preview_skill { 1 } else { 0 };
        let sandbox_container_id = normalize_sandbox_container_id(record.sandbox_container_id);
        let hive_id = normalize_hive_id(&record.hive_id);
        conn.execute(
            "INSERT INTO user_agents (agent_id, user_id, hive_id, name, description, system_prompt, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids)              VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25)              ON CONFLICT(agent_id) DO UPDATE SET user_id = EXCLUDED.user_id, hive_id = EXCLUDED.hive_id, name = EXCLUDED.name, description = EXCLUDED.description,              system_prompt = EXCLUDED.system_prompt, model_name = EXCLUDED.model_name, tool_names = EXCLUDED.tool_names, declared_tool_names = EXCLUDED.declared_tool_names, declared_skill_names = EXCLUDED.declared_skill_names, ability_items = EXCLUDED.ability_items, access_level = EXCLUDED.access_level, approval_mode = EXCLUDED.approval_mode,              is_shared = EXCLUDED.is_shared, status = EXCLUDED.status, icon = EXCLUDED.icon, sandbox_container_id = EXCLUDED.sandbox_container_id, updated_at = EXCLUDED.updated_at, preset_questions = EXCLUDED.preset_questions, preset_binding = EXCLUDED.preset_binding, silent = EXCLUDED.silent, prefer_mother = EXCLUDED.prefer_mother, preview_skill = EXCLUDED.preview_skill, visible_unit_ids = EXCLUDED.visible_unit_ids",
            &[
                &record.agent_id,
                &record.user_id,
                &hive_id,
                &record.name,
                &record.description,
                &record.system_prompt,
                &record.model_name,
                &tool_names,
                &declared_tool_names,
                &declared_skill_names,
                &ability_items,
                &record.access_level,
                &record.approval_mode,
                &is_shared,
                &record.status,
                &record.icon,
                &sandbox_container_id,
                &record.created_at,
                &record.updated_at,
                &preset_questions,
                &preset_binding,
                &silent,
                &prefer_mother,
                &preview_skill,
                &visible_unit_ids,
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
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT agent_id, user_id, hive_id, name, description, system_prompt, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids FROM user_agents WHERE user_id = $1 AND agent_id = $2",
            &[&cleaned_user, &cleaned_agent],
        )?;
        Ok(row.map(|row| Self::read_user_agent_row(&row)))
    }

    fn get_user_agent_by_id_impl(&self, agent_id: &str) -> Result<Option<UserAgentRecord>> {
        self.ensure_initialized()?;
        let cleaned_agent = agent_id.trim();
        if cleaned_agent.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT agent_id, user_id, hive_id, name, description, system_prompt, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids FROM user_agents WHERE agent_id = $1",
            &[&cleaned_agent],
        )?;
        Ok(row.map(|row| Self::read_user_agent_row(&row)))
    }

    fn list_user_agents_impl(&self, user_id: &str) -> Result<Vec<UserAgentRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(Vec::new());
        }
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT agent_id, user_id, hive_id, name, description, system_prompt, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids FROM user_agents WHERE user_id = $1 ORDER BY updated_at DESC",
            &[&cleaned_user],
        )?;
        let mut output = Vec::new();
        for row in rows {
            output.push(Self::read_user_agent_row(&row));
        }
        Ok(output)
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
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT agent_id, user_id, hive_id, name, description, system_prompt, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids FROM user_agents WHERE user_id = $1 AND hive_id = $2 ORDER BY updated_at DESC",
            &[&cleaned_user, &normalized_hive_id],
        )?;
        let mut output = Vec::new();
        for row in rows {
            output.push(Self::read_user_agent_row(&row));
        }
        Ok(output)
    }

    fn list_shared_user_agents_impl(&self, user_id: &str) -> Result<Vec<UserAgentRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(Vec::new());
        }
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT agent_id, user_id, hive_id, name, description, system_prompt, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids FROM user_agents WHERE is_shared = 1 AND user_id <> $1 ORDER BY updated_at DESC",
            &[&cleaned_user],
        )?;
        let mut output = Vec::new();
        for row in rows {
            output.push(Self::read_user_agent_row(&row));
        }
        Ok(output)
    }

    fn delete_user_agent_impl(&self, user_id: &str, agent_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_agent = agent_id.trim();
        if cleaned_user.is_empty() || cleaned_agent.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM user_agents WHERE user_id = $1 AND agent_id = $2",
            &[&cleaned_user, &cleaned_agent],
        )?;
        Ok(affected as i64)
    }

    fn upsert_hive_impl(&self, record: &HiveRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let hive_id = normalize_hive_id(&record.hive_id);
        let is_default = if record.is_default { 1 } else { 0 };
        conn.execute(
            "INSERT INTO hives (hive_id, user_id, name, description, is_default, status, created_time, updated_time)              VALUES ($1,$2,$3,$4,$5,$6,$7,$8)              ON CONFLICT(hive_id) DO UPDATE SET user_id = EXCLUDED.user_id, name = EXCLUDED.name, description = EXCLUDED.description,              is_default = EXCLUDED.is_default, status = EXCLUDED.status, updated_time = EXCLUDED.updated_time",
            &[
                &hive_id,
                &record.user_id,
                &record.name,
                &record.description,
                &is_default,
                &record.status,
                &record.created_time,
                &record.updated_time,
            ],
        )?;
        Ok(())
    }

    fn get_hive_impl(&self, user_id: &str, hive_id: &str) -> Result<Option<HiveRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(None);
        }
        let normalized_hive_id = normalize_hive_id(hive_id);
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT hive_id, user_id, name, description, is_default, status, created_time, updated_time FROM hives WHERE user_id = $1 AND hive_id = $2",
            &[&cleaned_user, &normalized_hive_id],
        )?;
        Ok(row.map(|row| HiveRecord {
            hive_id: normalize_hive_id(&row.get::<_, String>(0)),
            user_id: row.get(1),
            name: row.get(2),
            description: row.get(3),
            is_default: row.get::<_, i32>(4) != 0,
            status: row.get(5),
            created_time: row.get(6),
            updated_time: row.get(7),
        }))
    }

    fn list_hives_impl(&self, user_id: &str, include_archived: bool) -> Result<Vec<HiveRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(Vec::new());
        }
        let mut conn = self.conn()?;
        let sql = if include_archived {
            "SELECT hive_id, user_id, name, description, is_default, status, created_time, updated_time FROM hives WHERE user_id = $1 ORDER BY is_default DESC, updated_time DESC"
        } else {
            "SELECT hive_id, user_id, name, description, is_default, status, created_time, updated_time FROM hives WHERE user_id = $1 AND status <> 'archived' ORDER BY is_default DESC, updated_time DESC"
        };
        let rows = conn.query(sql, &[&cleaned_user])?;
        let mut output = Vec::new();
        for row in rows {
            output.push(HiveRecord {
                hive_id: normalize_hive_id(&row.get::<_, String>(0)),
                user_id: row.get(1),
                name: row.get(2),
                description: row.get(3),
                is_default: row.get::<_, i32>(4) != 0,
                status: row.get(5),
                created_time: row.get(6),
                updated_time: row.get(7),
            });
        }
        Ok(output)
    }

    fn delete_hive_impl(&self, user_id: &str, hive_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let normalized_hive_id = normalize_hive_id(hive_id);
        if cleaned_user.is_empty() || normalized_hive_id == DEFAULT_HIVE_ID {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM hives WHERE user_id = $1 AND hive_id = $2 AND is_default = 0",
            &[&cleaned_user, &normalized_hive_id],
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
        let normalized_hive_id = normalize_hive_id(hive_id);
        let now = Self::now_ts();
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "UPDATE user_agents SET hive_id = $1, updated_at = $2 WHERE user_id = $3 AND agent_id = ANY($4)",
            &[&normalized_hive_id, &now, &cleaned_user, &cleaned_ids],
        )?;
        Ok(affected as i64)
    }

    fn upsert_team_run_impl(&self, record: &TeamRunRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let hive_id = normalize_hive_id(&record.hive_id);
        conn.execute(
            "INSERT INTO team_runs (team_run_id, user_id, hive_id, parent_session_id, parent_agent_id, mother_agent_id, strategy, status, task_total, task_success, task_failed, context_tokens_total, context_tokens_peak, model_round_total, started_time, finished_time, elapsed_s, summary, error, updated_time)              VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20)              ON CONFLICT(team_run_id) DO UPDATE SET user_id = EXCLUDED.user_id, hive_id = EXCLUDED.hive_id, parent_session_id = EXCLUDED.parent_session_id, parent_agent_id = EXCLUDED.parent_agent_id, mother_agent_id = EXCLUDED.mother_agent_id,              strategy = EXCLUDED.strategy, status = EXCLUDED.status, task_total = EXCLUDED.task_total, task_success = EXCLUDED.task_success, task_failed = EXCLUDED.task_failed,              context_tokens_total = EXCLUDED.context_tokens_total, context_tokens_peak = EXCLUDED.context_tokens_peak, model_round_total = EXCLUDED.model_round_total,              started_time = EXCLUDED.started_time, finished_time = EXCLUDED.finished_time, elapsed_s = EXCLUDED.elapsed_s, summary = EXCLUDED.summary, error = EXCLUDED.error, updated_time = EXCLUDED.updated_time",
            &[
                &record.team_run_id,
                &record.user_id,
                &hive_id,
                &record.parent_session_id,
                &record.parent_agent_id,
                &record.mother_agent_id,
                &record.strategy,
                &record.status,
                &record.task_total,
                &record.task_success,
                &record.task_failed,
                &record.context_tokens_total,
                &record.context_tokens_peak,
                &record.model_round_total,
                &record.started_time,
                &record.finished_time,
                &record.elapsed_s,
                &record.summary,
                &record.error,
                &record.updated_time,
            ],
        )?;
        Ok(())
    }

    fn delete_team_runs_by_hive_impl(&self, user_id: &str, hive_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(0);
        }
        let normalized_hive_id = normalize_hive_id(hive_id);
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM team_runs WHERE user_id = $1 AND hive_id = $2",
            &[&cleaned_user, &normalized_hive_id],
        )?;
        Ok(affected as i64)
    }

    fn get_team_run_impl(&self, team_run_id: &str) -> Result<Option<TeamRunRecord>> {
        self.ensure_initialized()?;
        let cleaned = team_run_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT team_run_id, user_id, hive_id, parent_session_id, parent_agent_id, mother_agent_id, strategy, status, task_total, task_success, task_failed, context_tokens_total, context_tokens_peak, model_round_total, started_time, finished_time, elapsed_s, summary, error, updated_time FROM team_runs WHERE team_run_id = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| TeamRunRecord {
            team_run_id: row.get(0),
            user_id: row.get(1),
            hive_id: normalize_hive_id(&row.get::<_, String>(2)),
            parent_session_id: row.get(3),
            parent_agent_id: row.get(4),
            mother_agent_id: row.get(5),
            strategy: row.get(6),
            status: row.get(7),
            task_total: row.get(8),
            task_success: row.get(9),
            task_failed: row.get(10),
            context_tokens_total: row.get(11),
            context_tokens_peak: row.get(12),
            model_round_total: row.get(13),
            started_time: row.get(14),
            finished_time: row.get(15),
            elapsed_s: row.get(16),
            summary: row.get(17),
            error: row.get(18),
            updated_time: row.get(19),
        }))
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
        let hive_filter = hive_id.map(normalize_hive_id).unwrap_or_default();
        let parent_filter = parent_session_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .unwrap_or_default();
        let safe_limit = limit.max(1);
        let safe_offset = offset.max(0);
        let mut conn = self.conn()?;
        let total: i64 = conn
            .query_one(
                "SELECT COUNT(1) FROM team_runs WHERE user_id = $1 AND ($2 = '' OR hive_id = $2) AND ($3 = '' OR parent_session_id = $3)",
                &[&cleaned_user, &hive_filter, &parent_filter],
            )?
            .get(0);
        let rows = conn.query(
            "SELECT team_run_id, user_id, hive_id, parent_session_id, parent_agent_id, mother_agent_id, strategy, status, task_total, task_success, task_failed, context_tokens_total, context_tokens_peak, model_round_total, started_time, finished_time, elapsed_s, summary, error, updated_time              FROM team_runs WHERE user_id = $1 AND ($2 = '' OR hive_id = $2) AND ($3 = '' OR parent_session_id = $3)              ORDER BY updated_time DESC LIMIT $4 OFFSET $5",
            &[&cleaned_user, &hive_filter, &parent_filter, &safe_limit, &safe_offset],
        )?;
        let mut output = Vec::new();
        for row in rows {
            output.push(TeamRunRecord {
                team_run_id: row.get(0),
                user_id: row.get(1),
                hive_id: normalize_hive_id(&row.get::<_, String>(2)),
                parent_session_id: row.get(3),
                parent_agent_id: row.get(4),
                mother_agent_id: row.get(5),
                strategy: row.get(6),
                status: row.get(7),
                task_total: row.get(8),
                task_success: row.get(9),
                task_failed: row.get(10),
                context_tokens_total: row.get(11),
                context_tokens_peak: row.get(12),
                model_round_total: row.get(13),
                started_time: row.get(14),
                finished_time: row.get(15),
                elapsed_s: row.get(16),
                summary: row.get(17),
                error: row.get(18),
                updated_time: row.get(19),
            });
        }
        Ok((output, total))
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

        let safe_limit = limit.max(1);
        let safe_offset = offset.max(0);
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT team_run_id, user_id, hive_id, parent_session_id, parent_agent_id, mother_agent_id, strategy, status, task_total, task_success, task_failed, context_tokens_total, context_tokens_peak, model_round_total, started_time, finished_time, elapsed_s, summary, error, updated_time              FROM team_runs WHERE status = ANY($1::text[]) ORDER BY updated_time ASC LIMIT $2 OFFSET $3",
            &[&cleaned_statuses, &safe_limit, &safe_offset],
        )?;
        let mut output = Vec::with_capacity(rows.len());
        for row in rows {
            output.push(TeamRunRecord {
                team_run_id: row.get(0),
                user_id: row.get(1),
                hive_id: normalize_hive_id(&row.get::<_, String>(2)),
                parent_session_id: row.get(3),
                parent_agent_id: row.get(4),
                mother_agent_id: row.get(5),
                strategy: row.get(6),
                status: row.get(7),
                task_total: row.get(8),
                task_success: row.get(9),
                task_failed: row.get(10),
                context_tokens_total: row.get(11),
                context_tokens_peak: row.get(12),
                model_round_total: row.get(13),
                started_time: row.get(14),
                finished_time: row.get(15),
                elapsed_s: row.get(16),
                summary: row.get(17),
                error: row.get(18),
                updated_time: row.get(19),
            });
        }
        Ok(output)
    }

    fn upsert_team_task_impl(&self, record: &TeamTaskRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let hive_id = normalize_hive_id(&record.hive_id);
        conn.execute(
            "INSERT INTO team_tasks (task_id, team_run_id, user_id, hive_id, agent_id, target_session_id, spawned_session_id, session_run_id, status, retry_count, priority, started_time, finished_time, elapsed_s, result_summary, error, updated_time)              VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)              ON CONFLICT(task_id) DO UPDATE SET team_run_id = EXCLUDED.team_run_id, user_id = EXCLUDED.user_id, hive_id = EXCLUDED.hive_id, agent_id = EXCLUDED.agent_id,              target_session_id = EXCLUDED.target_session_id, spawned_session_id = EXCLUDED.spawned_session_id, session_run_id = EXCLUDED.session_run_id, status = EXCLUDED.status, retry_count = EXCLUDED.retry_count,              priority = EXCLUDED.priority, started_time = EXCLUDED.started_time, finished_time = EXCLUDED.finished_time, elapsed_s = EXCLUDED.elapsed_s,              result_summary = EXCLUDED.result_summary, error = EXCLUDED.error, updated_time = EXCLUDED.updated_time",
            &[
                &record.task_id,
                &record.team_run_id,
                &record.user_id,
                &hive_id,
                &record.agent_id,
                &record.target_session_id,
                &record.spawned_session_id,
                &record.session_run_id,
                &record.status,
                &record.retry_count,
                &record.priority,
                &record.started_time,
                &record.finished_time,
                &record.elapsed_s,
                &record.result_summary,
                &record.error,
                &record.updated_time,
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
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT task_id, team_run_id, user_id, hive_id, agent_id, target_session_id, spawned_session_id, session_run_id, status, retry_count, priority, started_time, finished_time, elapsed_s, result_summary, error, updated_time              FROM team_tasks WHERE team_run_id = $1 ORDER BY updated_time DESC",
            &[&cleaned_run_id],
        )?;
        let mut output = Vec::new();
        for row in rows {
            output.push(TeamTaskRecord {
                task_id: row.get(0),
                team_run_id: row.get(1),
                user_id: row.get(2),
                hive_id: normalize_hive_id(&row.get::<_, String>(3)),
                agent_id: row.get(4),
                target_session_id: row.get(5),
                spawned_session_id: row.get(6),
                session_run_id: row.get(7),
                status: row.get(8),
                retry_count: row.get(9),
                priority: row.get(10),
                started_time: row.get(11),
                finished_time: row.get(12),
                elapsed_s: row.get(13),
                result_summary: row.get(14),
                error: row.get(15),
                updated_time: row.get(16),
            });
        }
        Ok(output)
    }

    fn get_team_task_impl(&self, task_id: &str) -> Result<Option<TeamTaskRecord>> {
        self.ensure_initialized()?;
        let cleaned = task_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT task_id, team_run_id, user_id, hive_id, agent_id, target_session_id, spawned_session_id, session_run_id, status, retry_count, priority, started_time, finished_time, elapsed_s, result_summary, error, updated_time FROM team_tasks WHERE task_id = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| TeamTaskRecord {
            task_id: row.get(0),
            team_run_id: row.get(1),
            user_id: row.get(2),
            hive_id: normalize_hive_id(&row.get::<_, String>(3)),
            agent_id: row.get(4),
            target_session_id: row.get(5),
            spawned_session_id: row.get(6),
            session_run_id: row.get(7),
            status: row.get(8),
            retry_count: row.get(9),
            priority: row.get(10),
            started_time: row.get(11),
            finished_time: row.get(12),
            elapsed_s: row.get(13),
            result_summary: row.get(14),
            error: row.get(15),
            updated_time: row.get(16),
        }))
    }
}
