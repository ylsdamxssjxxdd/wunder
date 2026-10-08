use super::PostgresStorage;
use crate::storage::{
    normalize_sandbox_container_id, PresetBoundAgentRecord, StorageLifecycle,
    UserAgentAccessRecord, UserAgentRecord, UserToolAccessRecord,
};
use anyhow::Result;
use tokio_postgres::types::ToSql;

/// Canonical `user_agents` projection order; `read_user_agent_row` indexes into it.
const AGENT_COLUMNS: &str = "agent_id, user_id, name, description, system_prompt, model_name, \
     tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, \
     approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, \
     preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids";

/// Preset ids live inside the `preset_binding` JSON payload; extracting them in
/// SQL keeps binding filters and counts off a full table scan in Rust.
const BINDING_PRESET_ID: &str = "(ua.preset_binding::jsonb ->> 'preset_id')";
const BINDING_PRESET_ID_PLAIN: &str = "(preset_binding::jsonb ->> 'preset_id')";
const BINDING_USABLE: &str =
    "ua.preset_binding IS NOT NULL AND left(btrim(ua.preset_binding), 1) = '{'";

/// `IN (...)` chunk size for batched user lookups.
const USER_ID_CHUNK: usize = 400;

fn aliased_agent_columns() -> String {
    AGENT_COLUMNS
        .split(',')
        .map(|column| format!("ua.{}", column.trim()))
        .collect::<Vec<_>>()
        .join(", ")
}

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
        conn.execute(
            "INSERT INTO user_agents (agent_id, user_id, name, description, system_prompt, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids)              VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24)              ON CONFLICT(agent_id) DO UPDATE SET user_id = EXCLUDED.user_id, name = EXCLUDED.name, description = EXCLUDED.description,              system_prompt = EXCLUDED.system_prompt, model_name = EXCLUDED.model_name, tool_names = EXCLUDED.tool_names, declared_tool_names = EXCLUDED.declared_tool_names, declared_skill_names = EXCLUDED.declared_skill_names, ability_items = EXCLUDED.ability_items, access_level = EXCLUDED.access_level, approval_mode = EXCLUDED.approval_mode,              is_shared = EXCLUDED.is_shared, status = EXCLUDED.status, icon = EXCLUDED.icon, sandbox_container_id = EXCLUDED.sandbox_container_id, updated_at = EXCLUDED.updated_at, preset_questions = EXCLUDED.preset_questions, preset_binding = EXCLUDED.preset_binding, silent = EXCLUDED.silent, prefer_mother = EXCLUDED.prefer_mother, preview_skill = EXCLUDED.preview_skill, visible_unit_ids = EXCLUDED.visible_unit_ids",
            &[
                &record.agent_id,
                &record.user_id,
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
            "SELECT agent_id, user_id, name, description, system_prompt, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids FROM user_agents WHERE user_id = $1 AND agent_id = $2",
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
            "SELECT agent_id, user_id, name, description, system_prompt, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids FROM user_agents WHERE agent_id = $1",
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
            "SELECT agent_id, user_id, name, description, system_prompt, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids FROM user_agents WHERE user_id = $1 ORDER BY updated_at DESC",
            &[&cleaned_user],
        )?;
        let mut output = Vec::new();
        for row in rows {
            output.push(Self::read_user_agent_row(&row));
        }
        Ok(output)
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
        let mut conn = self.conn()?;
        let mut output = Vec::new();
        for chunk in cleaned.chunks(USER_ID_CHUNK) {
            let sql = format!(
                "SELECT {AGENT_COLUMNS} FROM user_agents WHERE user_id = ANY($1) ORDER BY updated_at DESC"
            );
            let rows = conn.query(&sql, &[&chunk])?;
            for row in rows {
                output.push(Self::read_user_agent_row(&row));
            }
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
        let preset_param = cleaned_preset.to_string();
        let keyword_param = keyword
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| format!("%{value}%"));
        let units_param = unit_ids
            .filter(|ids| !ids.is_empty())
            .map(|ids| ids.to_vec());
        let limit_param = limit;
        let offset_param = offset.max(0);

        let mut conditions = vec![
            BINDING_USABLE.to_string(),
            format!("{BINDING_PRESET_ID} = $1"),
        ];
        let mut params: Vec<&(dyn ToSql + Sync)> = vec![&preset_param];
        if let Some(pattern) = keyword_param.as_ref() {
            params.push(pattern);
            params.push(pattern);
            conditions.push(format!(
                "(u.username LIKE ${} OR ua.name LIKE ${})",
                params.len() - 1,
                params.len()
            ));
        }
        if let Some(units) = units_param.as_ref() {
            params.push(units);
            conditions.push(format!("u.unit_id = ANY(${})", params.len()));
        }
        let where_sql = conditions.join(" AND ");
        let mut conn = self.conn()?;
        let count_sql = format!(
            "SELECT COUNT(*) FROM user_agents ua JOIN user_accounts u ON u.user_id = ua.user_id WHERE {where_sql}"
        );
        let total: i64 = conn.query_one(&count_sql, &params)?.get(0);

        let mut sql = format!(
            "SELECT {}, u.username FROM user_agents ua JOIN user_accounts u ON u.user_id = ua.user_id WHERE {where_sql} ORDER BY u.username ASC",
            aliased_agent_columns()
        );
        if limit_param > 0 {
            params.push(&limit_param);
            params.push(&offset_param);
            sql.push_str(&format!(
                " LIMIT ${} OFFSET ${}",
                params.len() - 1,
                params.len()
            ));
        }
        let rows = conn.query(&sql, &params)?;
        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            let record = Self::read_user_agent_row(&row);
            let username: Option<String> = row.get(24);
            items.push(PresetBoundAgentRecord {
                user_id: record.user_id.clone(),
                username: username.unwrap_or_default(),
                record,
            });
        }
        Ok((items, total))
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
        let mut conn = self.conn()?;
        let sql = format!(
            "SELECT {BINDING_PRESET_ID_PLAIN} AS preset_id, COUNT(DISTINCT user_id) FROM user_agents \
             WHERE preset_binding IS NOT NULL AND left(btrim(preset_binding), 1) = '{{' \
             AND {BINDING_PRESET_ID_PLAIN} = ANY($1) GROUP BY {BINDING_PRESET_ID_PLAIN}"
        );
        let rows = conn.query(&sql, &[&cleaned])?;
        let mut output = Vec::with_capacity(rows.len());
        for row in rows {
            let preset_id: Option<String> = row.get(0);
            output.push((preset_id.unwrap_or_default(), row.get(1)));
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
            "SELECT agent_id, user_id, name, description, system_prompt, model_name, tool_names, declared_tool_names, declared_skill_names, ability_items, access_level, approval_mode, is_shared, status, icon, sandbox_container_id, created_at, updated_at, preset_questions, preset_binding, silent, prefer_mother, preview_skill, visible_unit_ids FROM user_agents WHERE is_shared = 1 AND user_id <> $1 ORDER BY updated_at DESC",
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
}
