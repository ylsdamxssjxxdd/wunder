use super::SqliteStorage;
use crate::storage::{
    ExternalLinkRecord, OrgUnitRecord, StorageLifecycle, UserAccountRecord,
    UserExperienceUpdateResult, UserSessionScopeRecord, UserTokenRecord,
};
use anyhow::Result;
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, OptionalExtension};

pub(super) trait SqliteUserAccountStorage {
    fn upsert_user_account_impl(&self, record: &UserAccountRecord) -> Result<()>;
    fn upsert_user_accounts_impl(&self, records: &[UserAccountRecord]) -> Result<()>;
    fn get_user_account_impl(&self, user_id: &str) -> Result<Option<UserAccountRecord>>;
    fn get_user_account_by_username_impl(
        &self,
        username: &str,
    ) -> Result<Option<UserAccountRecord>>;
    fn get_user_account_by_email_impl(&self, email: &str) -> Result<Option<UserAccountRecord>>;
    fn list_user_accounts_impl(
        &self,
        keyword: Option<&str>,
        unit_ids: Option<&[String]>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<UserAccountRecord>, i64)>;
    fn add_user_experience_impl(
        &self,
        user_id: &str,
        delta: i64,
        updated_at: f64,
    ) -> Result<UserExperienceUpdateResult>;
    fn delete_user_account_impl(&self, user_id: &str) -> Result<i64>;
    fn list_org_units_impl(&self) -> Result<Vec<OrgUnitRecord>>;
    fn get_org_unit_impl(&self, unit_id: &str) -> Result<Option<OrgUnitRecord>>;
    fn upsert_org_unit_impl(&self, record: &OrgUnitRecord) -> Result<()>;
    fn delete_org_unit_impl(&self, unit_id: &str) -> Result<i64>;
    fn upsert_external_link_impl(&self, record: &ExternalLinkRecord) -> Result<()>;
    fn get_external_link_impl(&self, link_id: &str) -> Result<Option<ExternalLinkRecord>>;
    fn list_external_links_impl(&self, include_disabled: bool) -> Result<Vec<ExternalLinkRecord>>;
    fn delete_external_link_impl(&self, link_id: &str) -> Result<i64>;
    fn create_user_token_impl(&self, record: &UserTokenRecord) -> Result<()>;
    fn get_user_token_impl(&self, token: &str) -> Result<Option<UserTokenRecord>>;
    fn touch_user_token_impl(&self, token: &str, last_used_at: f64) -> Result<()>;
    fn delete_user_token_impl(&self, token: &str) -> Result<i64>;
    fn upsert_user_session_scope_impl(&self, record: &UserSessionScopeRecord) -> Result<()>;
    fn get_user_session_scope_impl(
        &self,
        user_id: &str,
        session_scope: &str,
    ) -> Result<Option<UserSessionScopeRecord>>;
}

impl SqliteUserAccountStorage for SqliteStorage {
    fn upsert_user_account_impl(&self, record: &UserAccountRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let roles = Self::string_list_to_json(&record.roles);
        conn.execute(
            "INSERT INTO user_accounts (user_id, username, email, password_hash, roles, status, access_level, unit_id, \
             token_balance, token_granted_total, token_used_total, last_token_grant_date, experience_total, is_demo, created_at, updated_at, last_login_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(user_id) DO UPDATE SET username = excluded.username, email = excluded.email, password_hash = excluded.password_hash, \
             roles = excluded.roles, status = excluded.status, access_level = excluded.access_level, unit_id = excluded.unit_id, \
             token_balance = excluded.token_balance, token_granted_total = excluded.token_granted_total, token_used_total = excluded.token_used_total, \
             last_token_grant_date = excluded.last_token_grant_date, \
             experience_total = excluded.experience_total, \
             is_demo = excluded.is_demo, created_at = excluded.created_at, updated_at = excluded.updated_at, last_login_at = excluded.last_login_at",
            params![
                record.user_id,
                record.username,
                record.email,
                record.password_hash,
                roles,
                record.status,
                record.access_level,
                record.unit_id,
                record.token_balance,
                record.token_granted_total,
                record.token_used_total,
                record.last_token_grant_date,
                record.experience_total,
                if record.is_demo { 1 } else { 0 },
                record.created_at,
                record.updated_at,
                record.last_login_at
            ],
        )?;
        Ok(())
    }

    fn get_user_account_impl(&self, user_id: &str) -> Result<Option<UserAccountRecord>> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, token_balance, token_granted_total, token_used_total, last_token_grant_date, \
                 experience_total, is_demo, created_at, updated_at, last_login_at FROM user_accounts WHERE user_id = ?",
                params![cleaned],
                |row| {
                    Ok(UserAccountRecord {
                        user_id: row.get(0)?,
                        username: row.get(1)?,
                        email: row.get(2)?,
                        password_hash: row.get(3)?,
                        roles: Self::parse_string_list(row.get::<_, Option<String>>(4)?),
                        status: row.get(5)?,
                        access_level: row.get(6)?,
                        unit_id: row.get(7)?,
                        token_balance: row.get::<_, Option<i64>>(8)?.unwrap_or(0),
                        token_granted_total: row.get::<_, Option<i64>>(9)?.unwrap_or(0),
                        token_used_total: row.get::<_, Option<i64>>(10)?.unwrap_or(0),
                        last_token_grant_date: row.get(11)?,
                        experience_total: row.get::<_, Option<i64>>(12)?.unwrap_or(0),
                        is_demo: row.get::<_, i64>(13)? != 0,
                        created_at: row.get(14)?,
                        updated_at: row.get(15)?,
                        last_login_at: row.get(16)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn get_user_account_by_username_impl(
        &self,
        username: &str,
    ) -> Result<Option<UserAccountRecord>> {
        self.ensure_initialized()?;
        let cleaned = username.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, token_balance, token_granted_total, token_used_total, last_token_grant_date, \
                 experience_total, is_demo, created_at, updated_at, last_login_at FROM user_accounts WHERE username = ?",
                params![cleaned],
                |row| {
                    Ok(UserAccountRecord {
                        user_id: row.get(0)?,
                        username: row.get(1)?,
                        email: row.get(2)?,
                        password_hash: row.get(3)?,
                        roles: Self::parse_string_list(row.get::<_, Option<String>>(4)?),
                        status: row.get(5)?,
                        access_level: row.get(6)?,
                        unit_id: row.get(7)?,
                        token_balance: row.get::<_, Option<i64>>(8)?.unwrap_or(0),
                        token_granted_total: row.get::<_, Option<i64>>(9)?.unwrap_or(0),
                        token_used_total: row.get::<_, Option<i64>>(10)?.unwrap_or(0),
                        last_token_grant_date: row.get(11)?,
                        experience_total: row.get::<_, Option<i64>>(12)?.unwrap_or(0),
                        is_demo: row.get::<_, i64>(13)? != 0,
                        created_at: row.get(14)?,
                        updated_at: row.get(15)?,
                        last_login_at: row.get(16)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn get_user_account_by_email_impl(&self, email: &str) -> Result<Option<UserAccountRecord>> {
        self.ensure_initialized()?;
        let cleaned = email.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, token_balance, token_granted_total, token_used_total, last_token_grant_date, \
                 experience_total, is_demo, created_at, updated_at, last_login_at FROM user_accounts WHERE email = ?",
                params![cleaned],
                |row| {
                    Ok(UserAccountRecord {
                        user_id: row.get(0)?,
                        username: row.get(1)?,
                        email: row.get(2)?,
                        password_hash: row.get(3)?,
                        roles: Self::parse_string_list(row.get::<_, Option<String>>(4)?),
                        status: row.get(5)?,
                        access_level: row.get(6)?,
                        unit_id: row.get(7)?,
                        token_balance: row.get::<_, Option<i64>>(8)?.unwrap_or(0),
                        token_granted_total: row.get::<_, Option<i64>>(9)?.unwrap_or(0),
                        token_used_total: row.get::<_, Option<i64>>(10)?.unwrap_or(0),
                        last_token_grant_date: row.get(11)?,
                        experience_total: row.get::<_, Option<i64>>(12)?.unwrap_or(0),
                        is_demo: row.get::<_, i64>(13)? != 0,
                        created_at: row.get(14)?,
                        updated_at: row.get(15)?,
                        last_login_at: row.get(16)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn list_user_accounts_impl(
        &self,
        keyword: Option<&str>,
        unit_ids: Option<&[String]>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<UserAccountRecord>, i64)> {
        self.ensure_initialized()?;
        let mut conditions = Vec::new();
        let mut params_list: Vec<SqlValue> = Vec::new();
        if let Some(keyword) = keyword {
            let cleaned = keyword.trim();
            if !cleaned.is_empty() {
                let pattern = format!("%{cleaned}%");
                conditions.push("(username LIKE ? OR email LIKE ?)".to_string());
                params_list.push(SqlValue::from(pattern.clone()));
                params_list.push(SqlValue::from(pattern));
            }
        }
        if let Some(unit_ids) = unit_ids.filter(|ids| !ids.is_empty()) {
            let placeholders = std::iter::repeat_n("?", unit_ids.len())
                .collect::<Vec<_>>()
                .join(", ");
            conditions.push(format!("unit_id IN ({placeholders})"));
            for unit_id in unit_ids {
                params_list.push(SqlValue::from(unit_id.clone()));
            }
        }
        let mut count_sql = String::from("SELECT COUNT(*) FROM user_accounts");
        if !conditions.is_empty() {
            count_sql.push_str(" WHERE ");
            count_sql.push_str(&conditions.join(" AND "));
        }
        let conn = self.open()?;
        let total: i64 =
            conn.query_row(&count_sql, params_from_iter(params_list.iter()), |row| {
                row.get(0)
            })?;

        let mut sql = String::from(
            "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, token_balance, token_granted_total, token_used_total, last_token_grant_date, \
             experience_total, is_demo, created_at, updated_at, last_login_at FROM user_accounts",
        );
        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }
        sql.push_str(" ORDER BY created_at DESC");
        if limit > 0 {
            sql.push_str(" LIMIT ? OFFSET ?");
            params_list.push(SqlValue::from(limit));
            params_list.push(SqlValue::from(offset.max(0)));
        }
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(params_list.iter()), |row| {
                Ok(UserAccountRecord {
                    user_id: row.get(0)?,
                    username: row.get(1)?,
                    email: row.get(2)?,
                    password_hash: row.get(3)?,
                    roles: Self::parse_string_list(row.get::<_, Option<String>>(4)?),
                    status: row.get(5)?,
                    access_level: row.get(6)?,
                    unit_id: row.get(7)?,
                    token_balance: row.get::<_, Option<i64>>(8)?.unwrap_or(0),
                    token_granted_total: row.get::<_, Option<i64>>(9)?.unwrap_or(0),
                    token_used_total: row.get::<_, Option<i64>>(10)?.unwrap_or(0),
                    last_token_grant_date: row.get(11)?,
                    experience_total: row.get::<_, Option<i64>>(12)?.unwrap_or(0),
                    is_demo: row.get::<_, i64>(13)? != 0,
                    created_at: row.get(14)?,
                    updated_at: row.get(15)?,
                    last_login_at: row.get(16)?,
                })
            })?
            .collect::<std::result::Result<Vec<UserAccountRecord>, _>>()?;
        Ok((rows, total))
    }

    fn add_user_experience_impl(
        &self,
        user_id: &str,
        delta: i64,
        updated_at: f64,
    ) -> Result<UserExperienceUpdateResult> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(UserExperienceUpdateResult {
                previous_total: 0,
                current_total: 0,
            });
        }
        let conn = self.open()?;
        let previous_total = conn
            .query_row(
                "SELECT experience_total FROM user_accounts WHERE user_id = ?",
                params![cleaned],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()?
            .flatten()
            .unwrap_or(0)
            .max(0);
        let safe_delta = delta.max(0);
        if safe_delta > 0 {
            conn.execute(
                "UPDATE user_accounts \
                 SET experience_total = COALESCE(experience_total, 0) + ?, updated_at = ? \
                 WHERE user_id = ?",
                params![safe_delta, updated_at, cleaned],
            )?;
        }
        let total = conn
            .query_row(
                "SELECT experience_total FROM user_accounts WHERE user_id = ?",
                params![cleaned],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()?
            .flatten()
            .unwrap_or(0);
        Ok(UserExperienceUpdateResult {
            previous_total,
            current_total: total.max(0),
        })
    }

    fn delete_user_account_impl(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM user_accounts WHERE user_id = ?",
            params![cleaned],
        )?;
        Ok(affected as i64)
    }

    fn list_org_units_impl(&self) -> Result<Vec<OrgUnitRecord>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT unit_id, parent_id, name, level, path, path_name, sort_order, leader_ids, created_at, updated_at \
             FROM org_units ORDER BY path, sort_order, name",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(OrgUnitRecord {
                    unit_id: row.get(0)?,
                    parent_id: row.get(1)?,
                    name: row.get(2)?,
                    level: row.get(3)?,
                    path: row.get(4)?,
                    path_name: row.get(5)?,
                    sort_order: row.get(6)?,
                    leader_ids: Self::parse_string_list(row.get::<_, Option<String>>(7)?),
                    created_at: row.get(8)?,
                    updated_at: row.get(9)?,
                })
            })?
            .collect::<std::result::Result<Vec<OrgUnitRecord>, _>>()?;
        Ok(rows)
    }

    fn get_org_unit_impl(&self, unit_id: &str) -> Result<Option<OrgUnitRecord>> {
        self.ensure_initialized()?;
        let cleaned = unit_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT unit_id, parent_id, name, level, path, path_name, sort_order, leader_ids, created_at, updated_at \
                 FROM org_units WHERE unit_id = ?",
                params![cleaned],
                |row| {
                    Ok(OrgUnitRecord {
                        unit_id: row.get(0)?,
                        parent_id: row.get(1)?,
                        name: row.get(2)?,
                        level: row.get(3)?,
                        path: row.get(4)?,
                        path_name: row.get(5)?,
                        sort_order: row.get(6)?,
                        leader_ids: Self::parse_string_list(row.get::<_, Option<String>>(7)?),
                        created_at: row.get(8)?,
                        updated_at: row.get(9)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn upsert_org_unit_impl(&self, record: &OrgUnitRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let leader_ids = Self::string_list_to_json(&record.leader_ids);
        conn.execute(
            "INSERT INTO org_units (unit_id, parent_id, name, level, path, path_name, sort_order, leader_ids, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(unit_id) DO UPDATE SET parent_id = excluded.parent_id, name = excluded.name, level = excluded.level, \
             path = excluded.path, path_name = excluded.path_name, sort_order = excluded.sort_order, leader_ids = excluded.leader_ids, \
             updated_at = excluded.updated_at",
            params![
                record.unit_id,
                record.parent_id,
                record.name,
                record.level,
                record.path,
                record.path_name,
                record.sort_order,
                leader_ids,
                record.created_at,
                record.updated_at
            ],
        )?;
        Ok(())
    }

    fn upsert_user_accounts_impl(&self, records: &[UserAccountRecord]) -> Result<()> {
        self.ensure_initialized()?;
        if records.is_empty() {
            return Ok(());
        }
        let mut conn = self.open()?;
        let tx = conn.transaction()?;
        for record in records {
            let roles = Self::string_list_to_json(&record.roles);
            tx.execute(
                "INSERT INTO user_accounts (user_id, username, email, password_hash, roles, status, access_level, unit_id, \
                 token_balance, token_granted_total, token_used_total, last_token_grant_date, experience_total, is_demo, created_at, updated_at, last_login_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
                 ON CONFLICT(user_id) DO UPDATE SET username = excluded.username, email = excluded.email, password_hash = excluded.password_hash, \
                 roles = excluded.roles, status = excluded.status, access_level = excluded.access_level, unit_id = excluded.unit_id, \
                 token_balance = excluded.token_balance, token_granted_total = excluded.token_granted_total, token_used_total = excluded.token_used_total, \
                 last_token_grant_date = excluded.last_token_grant_date, \
                 experience_total = excluded.experience_total, \
                 is_demo = excluded.is_demo, created_at = excluded.created_at, updated_at = excluded.updated_at, last_login_at = excluded.last_login_at",
                params![
                    record.user_id,
                    record.username,
                    record.email,
                    record.password_hash,
                    roles,
                    record.status,
                    record.access_level,
                    record.unit_id,
                    record.token_balance,
                    record.token_granted_total,
                    record.token_used_total,
                    record.last_token_grant_date,
                    record.experience_total,
                    if record.is_demo { 1 } else { 0 },
                    record.created_at,
                    record.updated_at,
                    record.last_login_at
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    fn delete_org_unit_impl(&self, unit_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = unit_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute("DELETE FROM org_units WHERE unit_id = ?", params![cleaned])?;
        Ok(affected as i64)
    }

    fn upsert_external_link_impl(&self, record: &ExternalLinkRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let allowed_levels = Self::i32_list_to_json(&record.allowed_levels);
        conn.execute(
            "INSERT INTO external_links (link_id, title, description, url, icon, allowed_levels, sort_order, enabled, created_at, updated_at) \n             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \n             ON CONFLICT(link_id) DO UPDATE SET title = excluded.title, description = excluded.description, \n             url = excluded.url, icon = excluded.icon, allowed_levels = excluded.allowed_levels, \n             sort_order = excluded.sort_order, enabled = excluded.enabled, updated_at = excluded.updated_at",
            params![
                record.link_id,
                record.title,
                record.description,
                record.url,
                record.icon,
                allowed_levels,
                record.sort_order,
                if record.enabled { 1 } else { 0 },
                record.created_at,
                record.updated_at,
            ],
        )?;
        Ok(())
    }

    fn get_external_link_impl(&self, link_id: &str) -> Result<Option<ExternalLinkRecord>> {
        self.ensure_initialized()?;
        let cleaned = link_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT link_id, title, description, url, icon, allowed_levels, sort_order, enabled, created_at, updated_at \n                 FROM external_links WHERE link_id = ?",
                params![cleaned],
                |row| {
                    Ok(ExternalLinkRecord {
                        link_id: row.get(0)?,
                        title: row.get(1)?,
                        description: row.get(2)?,
                        url: row.get(3)?,
                        icon: row.get(4)?,
                        allowed_levels: Self::parse_i32_list(row.get::<_, Option<String>>(5)?),
                        sort_order: row.get(6)?,
                        enabled: row.get::<_, i64>(7)? != 0,
                        created_at: row.get(8)?,
                        updated_at: row.get(9)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn list_external_links_impl(&self, include_disabled: bool) -> Result<Vec<ExternalLinkRecord>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let sql = if include_disabled {
            "SELECT link_id, title, description, url, icon, allowed_levels, sort_order, enabled, created_at, updated_at \n             FROM external_links ORDER BY sort_order ASC, updated_at DESC, link_id ASC"
        } else {
            "SELECT link_id, title, description, url, icon, allowed_levels, sort_order, enabled, created_at, updated_at \n             FROM external_links WHERE enabled = 1 ORDER BY sort_order ASC, updated_at DESC, link_id ASC"
        };
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ExternalLinkRecord {
                    link_id: row.get(0)?,
                    title: row.get(1)?,
                    description: row.get(2)?,
                    url: row.get(3)?,
                    icon: row.get(4)?,
                    allowed_levels: Self::parse_i32_list(row.get::<_, Option<String>>(5)?),
                    sort_order: row.get(6)?,
                    enabled: row.get::<_, i64>(7)? != 0,
                    created_at: row.get(8)?,
                    updated_at: row.get(9)?,
                })
            })?
            .collect::<std::result::Result<Vec<ExternalLinkRecord>, _>>()?;
        Ok(rows)
    }

    fn delete_external_link_impl(&self, link_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = link_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM external_links WHERE link_id = ?",
            params![cleaned],
        )?;
        Ok(affected as i64)
    }

    fn create_user_token_impl(&self, record: &UserTokenRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO user_tokens (token, user_id, session_scope, expires_at, created_at, last_used_at) VALUES (?, ?, ?, ?, ?, ?)",
            params![
                record.token,
                record.user_id,
                record.session_scope,
                record.expires_at,
                record.created_at,
                record.last_used_at
            ],
        )?;
        Ok(())
    }

    fn get_user_token_impl(&self, token: &str) -> Result<Option<UserTokenRecord>> {
        self.ensure_initialized()?;
        let cleaned = token.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT token, user_id, session_scope, expires_at, created_at, last_used_at FROM user_tokens WHERE token = ?",
                params![cleaned],
                |row| {
                    Ok(UserTokenRecord {
                        token: row.get(0)?,
                        user_id: row.get(1)?,
                        session_scope: row.get(2)?,
                        expires_at: row.get(3)?,
                        created_at: row.get(4)?,
                        last_used_at: row.get(5)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn touch_user_token_impl(&self, token: &str, last_used_at: f64) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = token.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        let conn = self.open()?;
        conn.execute(
            "UPDATE user_tokens SET last_used_at = ? WHERE token = ?",
            params![last_used_at, cleaned],
        )?;
        Ok(())
    }

    fn delete_user_token_impl(&self, token: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = token.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute("DELETE FROM user_tokens WHERE token = ?", params![cleaned])?;
        Ok(affected as i64)
    }

    fn upsert_user_session_scope_impl(&self, record: &UserSessionScopeRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO user_session_scopes (user_id, session_scope, last_login_at) VALUES (?, ?, ?)
             ON CONFLICT(user_id, session_scope) DO UPDATE SET last_login_at = excluded.last_login_at",
            params![record.user_id, record.session_scope, record.last_login_at],
        )?;
        Ok(())
    }

    fn get_user_session_scope_impl(
        &self,
        user_id: &str,
        session_scope: &str,
    ) -> Result<Option<UserSessionScopeRecord>> {
        self.ensure_initialized()?;
        let cleaned_user_id = user_id.trim();
        let cleaned_scope = session_scope.trim();
        if cleaned_user_id.is_empty() || cleaned_scope.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT user_id, session_scope, last_login_at FROM user_session_scopes WHERE user_id = ? AND session_scope = ?",
                params![cleaned_user_id, cleaned_scope],
                |row| {
                    Ok(UserSessionScopeRecord {
                        user_id: row.get(0)?,
                        session_scope: row.get(1)?,
                        last_login_at: row.get(2)?,
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
    use crate::storage::*;
    use tempfile::tempdir;

    fn build_storage() -> (SqliteStorage, tempfile::TempDir) {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("user-account-store.db");
        let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
        storage.ensure_initialized().expect("initialize sqlite");
        (storage, dir)
    }

    fn account(
        user_id: &str,
        username: &str,
        email: &str,
        unit_id: Option<&str>,
    ) -> UserAccountRecord {
        UserAccountRecord {
            user_id: user_id.to_string(),
            username: username.to_string(),
            email: Some(email.to_string()),
            password_hash: "hash".to_string(),
            roles: vec!["user".to_string()],
            status: "active".to_string(),
            access_level: "1".to_string(),
            unit_id: unit_id.map(str::to_string),
            token_balance: 10,
            token_granted_total: 20,
            token_used_total: 5,
            last_token_grant_date: Some("2026-01-01".to_string()),
            experience_total: 0,
            is_demo: false,
            created_at: 1.0,
            updated_at: 1.0,
            last_login_at: None,
        }
    }

    #[test]
    fn user_account_directory_token_and_scope_roundtrip() {
        let (storage, _dir) = build_storage();

        storage
            .upsert_org_unit(&OrgUnitRecord {
                unit_id: "unit-1".to_string(),
                parent_id: None,
                name: "Unit".to_string(),
                level: 1,
                path: "unit-1".to_string(),
                path_name: "Unit".to_string(),
                sort_order: 1,
                leader_ids: vec!["user-1".to_string()],
                created_at: 1.0,
                updated_at: 1.0,
            })
            .expect("upsert org unit");
        storage
            .upsert_user_accounts(&[
                account("user-1", "user_one", "one@example.invalid", Some("unit-1")),
                account("user-2", "user_two", "two@example.invalid", None),
            ])
            .expect("upsert accounts");

        assert_eq!(
            storage
                .get_user_account_by_username("user_one")
                .expect("by username")
                .map(|record| record.user_id),
            Some("user-1".to_string())
        );
        assert_eq!(
            storage
                .get_user_account_by_email("two@example.invalid")
                .expect("by email")
                .map(|record| record.user_id),
            Some("user-2".to_string())
        );
        let (unit_accounts, total) = storage
            .list_user_accounts(None, Some(&["unit-1".to_string()]), 0, 16)
            .expect("list unit accounts");
        assert_eq!(total, 1);
        assert_eq!(unit_accounts[0].user_id, "user-1");

        storage
            .upsert_external_link(&ExternalLinkRecord {
                link_id: "link-1".to_string(),
                title: "Link".to_string(),
                description: "".to_string(),
                url: "https://example.invalid".to_string(),
                icon: "".to_string(),
                allowed_levels: vec![1],
                sort_order: 1,
                enabled: false,
                created_at: 1.0,
                updated_at: 1.0,
            })
            .expect("upsert link");
        assert!(storage
            .list_external_links(false)
            .expect("enabled links")
            .is_empty());
        assert_eq!(
            storage.list_external_links(true).expect("all links").len(),
            1
        );

        storage
            .create_user_token(&UserTokenRecord {
                token: "token-1".to_string(),
                user_id: "user-1".to_string(),
                session_scope: "scope-1".to_string(),
                expires_at: 100.0,
                created_at: 1.0,
                last_used_at: 0.0,
            })
            .expect("create token");
        storage
            .touch_user_token("token-1", 2.0)
            .expect("touch token");
        assert_eq!(
            storage
                .get_user_token("token-1")
                .expect("get token")
                .map(|record| record.last_used_at),
            Some(2.0)
        );

        storage
            .upsert_user_session_scope(&UserSessionScopeRecord {
                user_id: "user-1".to_string(),
                session_scope: "scope-1".to_string(),
                last_login_at: 3.0,
            })
            .expect("upsert scope");
        assert_eq!(
            storage
                .get_user_session_scope("user-1", "scope-1")
                .expect("get scope")
                .map(|record| record.last_login_at),
            Some(3.0)
        );
    }
}
