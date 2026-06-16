use super::PostgresStorage;
use crate::storage::{
    ExternalLinkRecord, OrgUnitRecord, StorageLifecycle, UserAccountRecord,
    UserExperienceUpdateResult, UserSessionScopeRecord, UserTokenRecord,
};
use anyhow::Result;

pub(super) trait PostgresUserAccountStorage {
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

impl PostgresUserAccountStorage for PostgresStorage {
    fn upsert_user_account_impl(&self, record: &UserAccountRecord) -> Result<()> {
        self.ensure_initialized()?;
        let roles = Self::string_list_to_json(&record.roles);
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO user_accounts (user_id, username, email, password_hash, roles, status, access_level, unit_id, \
             token_balance, token_granted_total, token_used_total, last_token_grant_date, experience_total, is_demo, created_at, updated_at, last_login_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17) \
             ON CONFLICT(user_id) DO UPDATE SET username = EXCLUDED.username, email = EXCLUDED.email, password_hash = EXCLUDED.password_hash, \
             roles = EXCLUDED.roles, status = EXCLUDED.status, access_level = EXCLUDED.access_level, unit_id = EXCLUDED.unit_id, \
             token_balance = EXCLUDED.token_balance, token_granted_total = EXCLUDED.token_granted_total, token_used_total = EXCLUDED.token_used_total, \
             last_token_grant_date = EXCLUDED.last_token_grant_date, \
             experience_total = EXCLUDED.experience_total, \
             is_demo = EXCLUDED.is_demo, created_at = EXCLUDED.created_at, updated_at = EXCLUDED.updated_at, last_login_at = EXCLUDED.last_login_at",
            &[
                &record.user_id,
                &record.username,
                &record.email,
                &record.password_hash,
                &roles,
                &record.status,
                &record.access_level,
                &record.unit_id,
                &record.token_balance,
                &record.token_granted_total,
                &record.token_used_total,
                &record.last_token_grant_date,
                &record.experience_total,
                &(record.is_demo as i32),
                &record.created_at,
                &record.updated_at,
                &record.last_login_at,
            ],
        )?;
        Ok(())
    }

    fn upsert_user_accounts_impl(&self, records: &[UserAccountRecord]) -> Result<()> {
        self.ensure_initialized()?;
        if records.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        for record in records {
            let roles = Self::string_list_to_json(&record.roles);
            tx.execute(
                "INSERT INTO user_accounts (user_id, username, email, password_hash, roles, status, access_level, unit_id, \
                 token_balance, token_granted_total, token_used_total, last_token_grant_date, experience_total, is_demo, created_at, updated_at, last_login_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17) \
                 ON CONFLICT(user_id) DO UPDATE SET username = EXCLUDED.username, email = EXCLUDED.email, password_hash = EXCLUDED.password_hash, \
                 roles = EXCLUDED.roles, status = EXCLUDED.status, access_level = EXCLUDED.access_level, unit_id = EXCLUDED.unit_id, \
                 token_balance = EXCLUDED.token_balance, token_granted_total = EXCLUDED.token_granted_total, token_used_total = EXCLUDED.token_used_total, \
                 last_token_grant_date = EXCLUDED.last_token_grant_date, \
                 experience_total = EXCLUDED.experience_total, \
                 is_demo = EXCLUDED.is_demo, created_at = EXCLUDED.created_at, updated_at = EXCLUDED.updated_at, last_login_at = EXCLUDED.last_login_at",
                &[
                    &record.user_id,
                    &record.username,
                    &record.email,
                    &record.password_hash,
                    &roles,
                    &record.status,
                    &record.access_level,
                    &record.unit_id,
                    &record.token_balance,
                    &record.token_granted_total,
                    &record.token_used_total,
                    &record.last_token_grant_date,
                    &record.experience_total,
                    &(record.is_demo as i32),
                    &record.created_at,
                    &record.updated_at,
                    &record.last_login_at,
                ],
            )?;
        }
        tx.commit()
    }

    fn get_user_account_impl(&self, user_id: &str) -> Result<Option<UserAccountRecord>> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, token_balance, token_granted_total, token_used_total, last_token_grant_date, \
             experience_total, is_demo, created_at, updated_at, last_login_at FROM user_accounts WHERE user_id = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| UserAccountRecord {
            user_id: row.get(0),
            username: row.get(1),
            email: row.get(2),
            password_hash: row.get(3),
            roles: Self::parse_string_list(row.get::<_, Option<String>>(4)),
            status: row.get(5),
            access_level: row.get(6),
            unit_id: row.get(7),
            token_balance: row.get::<_, Option<i64>>(8).unwrap_or(0),
            token_granted_total: row.get::<_, Option<i64>>(9).unwrap_or(0),
            token_used_total: row.get::<_, Option<i64>>(10).unwrap_or(0),
            last_token_grant_date: row.get(11),
            experience_total: row.get::<_, Option<i64>>(12).unwrap_or(0),
            is_demo: row.get::<_, i32>(13) != 0,
            created_at: row.get(14),
            updated_at: row.get(15),
            last_login_at: row.get(16),
        }))
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
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, token_balance, token_granted_total, token_used_total, last_token_grant_date, \
             experience_total, is_demo, created_at, updated_at, last_login_at FROM user_accounts WHERE username = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| UserAccountRecord {
            user_id: row.get(0),
            username: row.get(1),
            email: row.get(2),
            password_hash: row.get(3),
            roles: Self::parse_string_list(row.get::<_, Option<String>>(4)),
            status: row.get(5),
            access_level: row.get(6),
            unit_id: row.get(7),
            token_balance: row.get::<_, Option<i64>>(8).unwrap_or(0),
            token_granted_total: row.get::<_, Option<i64>>(9).unwrap_or(0),
            token_used_total: row.get::<_, Option<i64>>(10).unwrap_or(0),
            last_token_grant_date: row.get(11),
            experience_total: row.get::<_, Option<i64>>(12).unwrap_or(0),
            is_demo: row.get::<_, i32>(13) != 0,
            created_at: row.get(14),
            updated_at: row.get(15),
            last_login_at: row.get(16),
        }))
    }

    fn get_user_account_by_email_impl(&self, email: &str) -> Result<Option<UserAccountRecord>> {
        self.ensure_initialized()?;
        let cleaned = email.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, token_balance, token_granted_total, token_used_total, last_token_grant_date, \
             experience_total, is_demo, created_at, updated_at, last_login_at FROM user_accounts WHERE email = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| UserAccountRecord {
            user_id: row.get(0),
            username: row.get(1),
            email: row.get(2),
            password_hash: row.get(3),
            roles: Self::parse_string_list(row.get::<_, Option<String>>(4)),
            status: row.get(5),
            access_level: row.get(6),
            unit_id: row.get(7),
            token_balance: row.get::<_, Option<i64>>(8).unwrap_or(0),
            token_granted_total: row.get::<_, Option<i64>>(9).unwrap_or(0),
            token_used_total: row.get::<_, Option<i64>>(10).unwrap_or(0),
            last_token_grant_date: row.get(11),
            experience_total: row.get::<_, Option<i64>>(12).unwrap_or(0),
            is_demo: row.get::<_, i32>(13) != 0,
            created_at: row.get(14),
            updated_at: row.get(15),
            last_login_at: row.get(16),
        }))
    }

    fn list_user_accounts_impl(
        &self,
        keyword: Option<&str>,
        unit_ids: Option<&[String]>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<UserAccountRecord>, i64)> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let cleaned_keyword = keyword
            .map(|value| value.trim())
            .filter(|value| !value.is_empty());
        let unit_ids = unit_ids
            .filter(|ids| !ids.is_empty())
            .map(|ids| ids.to_vec());

        let total: i64 = match (&cleaned_keyword, unit_ids.as_ref()) {
            (Some(keyword), Some(unit_ids)) => {
                let pattern = format!("%{keyword}%");
                conn.query_one(
                    "SELECT COUNT(*) FROM user_accounts WHERE (username ILIKE $1 OR email ILIKE $1) AND unit_id = ANY($2)",
                    &[&pattern, unit_ids],
                )?
                .get(0)
            }
            (Some(keyword), None) => {
                let pattern = format!("%{keyword}%");
                conn.query_one(
                    "SELECT COUNT(*) FROM user_accounts WHERE username ILIKE $1 OR email ILIKE $1",
                    &[&pattern],
                )?
                .get(0)
            }
            (None, Some(unit_ids)) => conn
                .query_one(
                    "SELECT COUNT(*) FROM user_accounts WHERE unit_id = ANY($1)",
                    &[unit_ids],
                )?
                .get(0),
            (None, None) => conn
                .query_one("SELECT COUNT(*) FROM user_accounts", &[])?
                .get(0),
        };

        let rows = match (&cleaned_keyword, unit_ids.as_ref()) {
            (Some(keyword), Some(unit_ids)) => {
                let pattern = format!("%{keyword}%");
                if limit > 0 {
                    conn.query(
                        "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, token_balance, token_granted_total, token_used_total, last_token_grant_date, \
                         experience_total, is_demo, created_at, updated_at, last_login_at FROM user_accounts \
                         WHERE (username ILIKE $1 OR email ILIKE $1) AND unit_id = ANY($2) \
                         ORDER BY created_at DESC LIMIT $3 OFFSET $4",
                        &[&pattern, unit_ids, &limit, &offset.max(0)],
                    )?
                } else {
                    conn.query(
                        "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, token_balance, token_granted_total, token_used_total, last_token_grant_date, \
                         experience_total, is_demo, created_at, updated_at, last_login_at FROM user_accounts \
                         WHERE (username ILIKE $1 OR email ILIKE $1) AND unit_id = ANY($2) \
                         ORDER BY created_at DESC",
                        &[&pattern, unit_ids],
                    )?
                }
            }
            (Some(keyword), None) => {
                let pattern = format!("%{keyword}%");
                if limit > 0 {
                    conn.query(
                        "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, token_balance, token_granted_total, token_used_total, last_token_grant_date, \
                         experience_total, is_demo, created_at, updated_at, last_login_at FROM user_accounts \
                         WHERE username ILIKE $1 OR email ILIKE $1 \
                         ORDER BY created_at DESC LIMIT $2 OFFSET $3",
                        &[&pattern, &limit, &offset.max(0)],
                    )?
                } else {
                    conn.query(
                        "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, token_balance, token_granted_total, token_used_total, last_token_grant_date, \
                         experience_total, is_demo, created_at, updated_at, last_login_at FROM user_accounts \
                         WHERE username ILIKE $1 OR email ILIKE $1 \
                         ORDER BY created_at DESC",
                        &[&pattern],
                    )?
                }
            }
            (None, Some(unit_ids)) => {
                if limit > 0 {
                    conn.query(
                        "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, token_balance, token_granted_total, token_used_total, last_token_grant_date, \
                         experience_total, is_demo, created_at, updated_at, last_login_at FROM user_accounts \
                         WHERE unit_id = ANY($1) \
                         ORDER BY created_at DESC LIMIT $2 OFFSET $3",
                        &[unit_ids, &limit, &offset.max(0)],
                    )?
                } else {
                    conn.query(
                        "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, token_balance, token_granted_total, token_used_total, last_token_grant_date, \
                         experience_total, is_demo, created_at, updated_at, last_login_at FROM user_accounts \
                         WHERE unit_id = ANY($1) ORDER BY created_at DESC",
                        &[unit_ids],
                    )?
                }
            }
            (None, None) => {
                if limit > 0 {
                    conn.query(
                        "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, token_balance, token_granted_total, token_used_total, last_token_grant_date, \
                         experience_total, is_demo, created_at, updated_at, last_login_at FROM user_accounts \
                         ORDER BY created_at DESC LIMIT $1 OFFSET $2",
                        &[&limit, &offset.max(0)],
                    )?
                } else {
                    conn.query(
                        "SELECT user_id, username, email, password_hash, roles, status, access_level, unit_id, token_balance, token_granted_total, token_used_total, last_token_grant_date, \
                         experience_total, is_demo, created_at, updated_at, last_login_at FROM user_accounts ORDER BY created_at DESC",
                        &[],
                    )?
                }
            }
        };

        let mut output = Vec::new();
        for row in rows {
            output.push(UserAccountRecord {
                user_id: row.get(0),
                username: row.get(1),
                email: row.get(2),
                password_hash: row.get(3),
                roles: Self::parse_string_list(row.get::<_, Option<String>>(4)),
                status: row.get(5),
                access_level: row.get(6),
                unit_id: row.get(7),
                token_balance: row.get::<_, Option<i64>>(8).unwrap_or(0),
                token_granted_total: row.get::<_, Option<i64>>(9).unwrap_or(0),
                token_used_total: row.get::<_, Option<i64>>(10).unwrap_or(0),
                last_token_grant_date: row.get(11),
                experience_total: row.get::<_, Option<i64>>(12).unwrap_or(0),
                is_demo: row.get::<_, i32>(13) != 0,
                created_at: row.get(14),
                updated_at: row.get(15),
                last_login_at: row.get(16),
            });
        }
        Ok((output, total))
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
        let mut conn = self.conn()?;
        let previous_total = conn
            .query_opt(
                "SELECT experience_total FROM user_accounts WHERE user_id = $1",
                &[&cleaned],
            )?
            .map(|value| value.get::<_, Option<i64>>(0).unwrap_or(0))
            .unwrap_or(0)
            .max(0);
        let safe_delta = delta.max(0);
        if safe_delta > 0 {
            let row = conn.query_one(
                "UPDATE user_accounts \
                 SET experience_total = COALESCE(experience_total, 0) + $1, updated_at = $2 \
                 WHERE user_id = $3 \
                 RETURNING experience_total",
                &[&safe_delta, &updated_at, &cleaned],
            )?;
            let total: i64 = row.get::<_, Option<i64>>(0).unwrap_or(0);
            return Ok(UserExperienceUpdateResult {
                previous_total,
                current_total: total.max(0),
            });
        }
        let row = conn.query_opt(
            "SELECT experience_total FROM user_accounts WHERE user_id = $1",
            &[&cleaned],
        )?;
        Ok(UserExperienceUpdateResult {
            previous_total,
            current_total: row
                .map(|value| value.get::<_, Option<i64>>(0).unwrap_or(0))
                .unwrap_or(0)
                .max(0),
        })
    }

    fn delete_user_account_impl(&self, user_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute("DELETE FROM user_accounts WHERE user_id = $1", &[&cleaned])?;
        Ok(affected as i64)
    }

    fn list_org_units_impl(&self) -> Result<Vec<OrgUnitRecord>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT unit_id, parent_id, name, level, path, path_name, sort_order, leader_ids, created_at, updated_at \
             FROM org_units ORDER BY path, sort_order, name",
            &[],
        )?;
        let mut output = Vec::new();
        for row in rows {
            output.push(OrgUnitRecord {
                unit_id: row.get(0),
                parent_id: row.get(1),
                name: row.get(2),
                level: row.get(3),
                path: row.get(4),
                path_name: row.get(5),
                sort_order: row.get(6),
                leader_ids: Self::parse_string_list(row.get::<_, Option<String>>(7)),
                created_at: row.get(8),
                updated_at: row.get(9),
            });
        }
        Ok(output)
    }

    fn get_org_unit_impl(&self, unit_id: &str) -> Result<Option<OrgUnitRecord>> {
        self.ensure_initialized()?;
        let cleaned = unit_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT unit_id, parent_id, name, level, path, path_name, sort_order, leader_ids, created_at, updated_at \
             FROM org_units WHERE unit_id = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| OrgUnitRecord {
            unit_id: row.get(0),
            parent_id: row.get(1),
            name: row.get(2),
            level: row.get(3),
            path: row.get(4),
            path_name: row.get(5),
            sort_order: row.get(6),
            leader_ids: Self::parse_string_list(row.get::<_, Option<String>>(7)),
            created_at: row.get(8),
            updated_at: row.get(9),
        }))
    }

    fn upsert_org_unit_impl(&self, record: &OrgUnitRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let leader_ids = Self::string_list_to_json(&record.leader_ids);
        conn.execute(
            "INSERT INTO org_units (unit_id, parent_id, name, level, path, path_name, sort_order, leader_ids, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
             ON CONFLICT(unit_id) DO UPDATE SET parent_id = EXCLUDED.parent_id, name = EXCLUDED.name, level = EXCLUDED.level, \
             path = EXCLUDED.path, path_name = EXCLUDED.path_name, sort_order = EXCLUDED.sort_order, leader_ids = EXCLUDED.leader_ids, \
             updated_at = EXCLUDED.updated_at",
            &[
                &record.unit_id,
                &record.parent_id,
                &record.name,
                &record.level,
                &record.path,
                &record.path_name,
                &record.sort_order,
                &leader_ids,
                &record.created_at,
                &record.updated_at,
            ],
        )?;
        Ok(())
    }

    fn delete_org_unit_impl(&self, unit_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = unit_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute("DELETE FROM org_units WHERE unit_id = $1", &[&cleaned])?;
        Ok(affected as i64)
    }

    fn upsert_external_link_impl(&self, record: &ExternalLinkRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let allowed_levels = Self::i32_list_to_json(&record.allowed_levels);
        conn.execute(
            "INSERT INTO external_links (link_id, title, description, url, icon, allowed_levels, sort_order, enabled, created_at, updated_at) \n             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \n             ON CONFLICT(link_id) DO UPDATE SET title = EXCLUDED.title, description = EXCLUDED.description, \n             url = EXCLUDED.url, icon = EXCLUDED.icon, allowed_levels = EXCLUDED.allowed_levels, \n             sort_order = EXCLUDED.sort_order, enabled = EXCLUDED.enabled, updated_at = EXCLUDED.updated_at",
            &[
                &record.link_id,
                &record.title,
                &record.description,
                &record.url,
                &record.icon,
                &allowed_levels,
                &record.sort_order,
                &(if record.enabled { 1_i32 } else { 0_i32 }),
                &record.created_at,
                &record.updated_at,
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
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT link_id, title, description, url, icon, allowed_levels, sort_order, enabled, created_at, updated_at \n             FROM external_links WHERE link_id = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| ExternalLinkRecord {
            link_id: row.get(0),
            title: row.get(1),
            description: row.get(2),
            url: row.get(3),
            icon: row.get(4),
            allowed_levels: Self::parse_i32_list(row.get::<_, Option<String>>(5)),
            sort_order: row.get(6),
            enabled: row.get::<_, i32>(7) != 0,
            created_at: row.get(8),
            updated_at: row.get(9),
        }))
    }

    fn list_external_links_impl(&self, include_disabled: bool) -> Result<Vec<ExternalLinkRecord>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let rows = if include_disabled {
            conn.query(
                "SELECT link_id, title, description, url, icon, allowed_levels, sort_order, enabled, created_at, updated_at \n                 FROM external_links ORDER BY sort_order ASC, updated_at DESC, link_id ASC",
                &[],
            )?
        } else {
            conn.query(
                "SELECT link_id, title, description, url, icon, allowed_levels, sort_order, enabled, created_at, updated_at \n                 FROM external_links WHERE enabled = 1 ORDER BY sort_order ASC, updated_at DESC, link_id ASC",
                &[],
            )?
        };
        let mut output = Vec::new();
        for row in rows {
            output.push(ExternalLinkRecord {
                link_id: row.get(0),
                title: row.get(1),
                description: row.get(2),
                url: row.get(3),
                icon: row.get(4),
                allowed_levels: Self::parse_i32_list(row.get::<_, Option<String>>(5)),
                sort_order: row.get(6),
                enabled: row.get::<_, i32>(7) != 0,
                created_at: row.get(8),
                updated_at: row.get(9),
            });
        }
        Ok(output)
    }

    fn delete_external_link_impl(&self, link_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = link_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected =
            conn.execute("DELETE FROM external_links WHERE link_id = $1", &[&cleaned])?;
        Ok(affected as i64)
    }

    fn create_user_token_impl(&self, record: &UserTokenRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO user_tokens (token, user_id, session_scope, expires_at, created_at, last_used_at) VALUES ($1, $2, $3, $4, $5, $6)",
            &[
                &record.token,
                &record.user_id,
                &record.session_scope,
                &record.expires_at,
                &record.created_at,
                &record.last_used_at,
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
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT token, user_id, session_scope, expires_at, created_at, last_used_at FROM user_tokens WHERE token = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| UserTokenRecord {
            token: row.get(0),
            user_id: row.get(1),
            session_scope: row.get(2),
            expires_at: row.get(3),
            created_at: row.get(4),
            last_used_at: row.get(5),
        }))
    }

    fn touch_user_token_impl(&self, token: &str, last_used_at: f64) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = token.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn()?;
        conn.execute(
            "UPDATE user_tokens SET last_used_at = $1 WHERE token = $2",
            &[&last_used_at, &cleaned],
        )?;
        Ok(())
    }

    fn delete_user_token_impl(&self, token: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = token.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute("DELETE FROM user_tokens WHERE token = $1", &[&cleaned])?;
        Ok(affected as i64)
    }

    fn upsert_user_session_scope_impl(&self, record: &UserSessionScopeRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO user_session_scopes (user_id, session_scope, last_login_at)
             VALUES ($1, $2, $3)
             ON CONFLICT (user_id, session_scope) DO UPDATE
             SET last_login_at = EXCLUDED.last_login_at",
            &[
                &record.user_id,
                &record.session_scope,
                &record.last_login_at,
            ],
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
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT user_id, session_scope, last_login_at
             FROM user_session_scopes
             WHERE user_id = $1 AND session_scope = $2",
            &[&cleaned_user_id, &cleaned_scope],
        )?;
        Ok(row.map(|row| UserSessionScopeRecord {
            user_id: row.get(0),
            session_scope: row.get(1),
            last_login_at: row.get(2),
        }))
    }
}
