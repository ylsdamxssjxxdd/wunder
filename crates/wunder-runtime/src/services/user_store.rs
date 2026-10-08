use crate::org_units;
use crate::services::default_agent_protocol::{
    default_agent_meta_key, is_builtin_default_agent_name, record_from_default_agent_config,
    DefaultAgentConfig as DefaultAgentConfigSnapshot, DEFAULT_AGENT_NAME,
};
use crate::services::user_leveling::{build_user_level_snapshot, normalize_total_experience};
use crate::storage::{
    normalize_sandbox_container_id, AgentTaskRecord, ChatSessionRecord, OrgUnitRecord,
    PresetBoundAgentRecord, SessionLockRecord, SessionRunRecord, StorageBackend,
    UpdateAgentTaskStatusParams, UserAccountRecord, UserAgentAccessRecord, UserAgentRecord,
    UserQuotaStatus, UserRefreshTokenRecord, UserSessionScopeRecord, UserTokenRecord,
    UserToolAccessRecord, DEFAULT_SANDBOX_CONTAINER_ID,
};
use anyhow::{anyhow, Result};
use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tracing::warn;
use uuid::Uuid;

const DEFAULT_TOKEN_TTL_S: i64 = 7 * 24 * 3600;
const REFRESH_TOKEN_TTL_S: i64 = 30 * 24 * 3600;
const TOKEN_TOUCH_MIN_INTERVAL_S: f64 = 30.0;
const TOKEN_TOUCH_CACHE_MAX_ITEMS: usize = 4096;
const DEFAULT_ADMIN_USER_ID: &str = "admin";
const DEFAULT_ADMIN_PASSWORD: &str = "admin";
/// Every non-admin user receives the same daily request allowance.
pub const DEFAULT_DAILY_QUOTA: i64 = 1_000;
const DEFAULT_AGENT_ID_ALIAS: &str = "__default__";
const DEFAULT_AGENT_STATUS: &str = "active";
const DEFAULT_AGENT_APPROVAL_MODE: &str = "full_auto";
const DEFAULT_AGENT_ACCESS_LEVEL: &str = "A";
const DEFAULT_AGENT_DESCRIPTION: &str =
    "我是wunder，很高兴帮助你，试着把整理资料，分析数据，写文章等工作交给我吧~";
const DEFAULT_AGENT_SYSTEM_PROMPT: &str = "你是一个乐于助人的智能体";
const DEFAULT_AGENT_PRESET_QUESTION_DRAW_GIF: &str = "制作一个骑自行车的鹈鹕gif";
const LEGACY_AGENT_PRESET_QUESTION_DRAW_HEART: &str = "绘制一个爱心到本地";
const DEFAULT_AGENT_PRESET_QUESTION_TRAVEL_GUIDE: &str = "用公文写作技能写一篇广州旅游攻略";
const SESSION_TIME_EPSILON_MICROS: u64 = 1;
const DEFAULT_SESSION_SCOPE: &str = "default";
const SESSION_SCOPE_MAX_LEN: usize = 32;

static LAST_SESSION_ISSUED_AT_MICROS: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize)]
pub struct UserUnitProfile {
    pub id: String,
    pub name: String,
    pub path: String,
    pub path_name: String,
    pub level: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct UserProfile {
    pub id: String,
    pub username: String,
    pub email: Option<String>,
    pub roles: Vec<String>,
    pub status: String,
    pub access_level: String,
    pub unit_id: Option<String>,
    pub unit: Option<UserUnitProfile>,
    pub quota_balance: i64,
    pub quota_granted_total: i64,
    pub quota_used_total: i64,
    pub daily_quota_grant: i64,
    pub last_quota_grant_date: Option<String>,
    pub level: i64,
    pub max_level: i64,
    pub experience_total: i64,
    pub experience_current: i64,
    pub experience_for_next_level: i64,
    pub experience_remaining: i64,
    pub experience_progress: f64,
    pub reached_max_level: bool,
    pub is_demo: bool,
    pub created_at: f64,
    pub updated_at: f64,
    pub last_login_at: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct UserSession {
    pub user: UserAccountRecord,
    pub token: UserTokenRecord,
}

#[derive(Debug, Clone)]
pub struct AuthenticatedUserToken {
    pub user: UserAccountRecord,
    pub session_scope: String,
    /// Rotation family of the token when it was issued through the
    /// refresh-token chain (local desktop/cli sessions); `None` otherwise.
    pub family_id: Option<String>,
}

pub struct UserStore {
    storage: Arc<dyn StorageBackend>,
    recent_token_touches: Mutex<HashMap<String, f64>>,
}

impl UserStore {
    fn normalize_optional_email(email: Option<String>) -> Option<String> {
        email.and_then(|value| {
            let cleaned = value.trim();
            if cleaned.is_empty() {
                None
            } else {
                Some(cleaned.to_string())
            }
        })
    }

    pub fn new(storage: Arc<dyn StorageBackend>) -> Self {
        Self {
            storage,
            recent_token_touches: Mutex::new(HashMap::new()),
        }
    }

    pub fn storage_backend(&self) -> Arc<dyn StorageBackend> {
        self.storage.clone()
    }

    pub fn is_default_admin(user_id: &str) -> bool {
        user_id.trim() == DEFAULT_ADMIN_USER_ID
    }

    pub fn ensure_default_admin(&self) -> Result<()> {
        if let Some(mut existing) = self.storage.get_user_account(DEFAULT_ADMIN_USER_ID)? {
            let mut changed = false;
            if existing.status.trim().to_lowercase() != "active" {
                existing.status = "active".to_string();
                changed = true;
            }
            if !Self::is_admin(&existing) {
                existing.roles.push("admin".to_string());
                changed = true;
            }
            if changed {
                existing.updated_at = now_ts();
                self.storage.update_user_account_profile(&existing)?;
            }
            return Ok(());
        }
        let _ = self.create_user(
            DEFAULT_ADMIN_USER_ID,
            None,
            DEFAULT_ADMIN_PASSWORD,
            Some("A"),
            None,
            vec!["admin".to_string()],
            "active",
            false,
        )?;
        Ok(())
    }

    pub fn normalize_user_id(raw: &str) -> Option<String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        let mut output = String::with_capacity(trimmed.len());
        for ch in trimmed.chars() {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                output.push(ch);
            } else {
                return None;
            }
        }
        if output.is_empty() {
            None
        } else {
            Some(output)
        }
    }

    pub fn normalize_access_level(raw: Option<&str>) -> String {
        let level = raw.unwrap_or("A").trim().to_uppercase();
        if level == "B" || level == "C" {
            level
        } else {
            "A".to_string()
        }
    }

    pub fn default_session_scope() -> &'static str {
        DEFAULT_SESSION_SCOPE
    }

    pub fn normalize_session_scope(raw: Option<&str>) -> String {
        let cleaned = raw
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(DEFAULT_SESSION_SCOPE)
            .to_ascii_lowercase();
        if cleaned.len() > SESSION_SCOPE_MAX_LEN {
            return DEFAULT_SESSION_SCOPE.to_string();
        }
        if cleaned
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
        {
            cleaned
        } else {
            DEFAULT_SESSION_SCOPE.to_string()
        }
    }

    pub fn default_daily_quota() -> i64 {
        DEFAULT_DAILY_QUOTA
    }

    pub fn today_string() -> String {
        chrono::Local::now().format("%Y-%m-%d").to_string()
    }

    pub fn hash_password(password: &str) -> Result<String> {
        let trimmed = password.trim();
        if trimmed.is_empty() {
            return Err(anyhow!("password is empty"));
        }
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        let hash = argon2
            .hash_password(trimmed.as_bytes(), &salt)
            .map_err(|err| anyhow!(err.to_string()))?;
        Ok(hash.to_string())
    }

    pub fn verify_password(hash: &str, password: &str) -> bool {
        let parsed = match PasswordHash::new(hash) {
            Ok(value) => value,
            Err(err) => {
                warn!("password hash parse failed: {err}");
                return false;
            }
        };
        Argon2::default()
            .verify_password(password.trim().as_bytes(), &parsed)
            .is_ok()
    }

    pub fn to_profile(user: &UserAccountRecord) -> UserProfile {
        Self::to_profile_with_unit(user, None)
    }

    pub fn to_profile_with_unit(
        user: &UserAccountRecord,
        unit: Option<&OrgUnitRecord>,
    ) -> UserProfile {
        let level_snapshot = build_user_level_snapshot(user.experience_total);
        let unit_profile = unit.map(|unit| UserUnitProfile {
            id: unit.unit_id.clone(),
            name: unit.name.clone(),
            path: unit.path.clone(),
            path_name: unit.path_name.clone(),
            level: unit.level,
        });
        let quota_status = Self::effective_quota_status(user, None);
        UserProfile {
            id: user.user_id.clone(),
            username: user.username.clone(),
            email: user.email.clone(),
            roles: user.roles.clone(),
            status: user.status.clone(),
            access_level: user.access_level.clone(),
            unit_id: user.unit_id.clone(),
            unit: unit_profile,
            quota_balance: quota_status.balance,
            quota_granted_total: quota_status.granted_total,
            quota_used_total: quota_status.used_total,
            daily_quota_grant: quota_status.daily_grant,
            last_quota_grant_date: quota_status.last_grant_date,
            level: level_snapshot.level,
            max_level: level_snapshot.max_level,
            experience_total: level_snapshot.experience_total,
            experience_current: level_snapshot.experience_current,
            experience_for_next_level: level_snapshot.experience_for_next_level,
            experience_remaining: level_snapshot.experience_remaining,
            experience_progress: level_snapshot.experience_progress,
            reached_max_level: level_snapshot.reached_max_level,
            is_demo: user.is_demo,
            created_at: user.created_at,
            updated_at: user.updated_at,
            last_login_at: user.last_login_at,
        }
    }

    pub fn is_admin(user: &UserAccountRecord) -> bool {
        user.roles
            .iter()
            .any(|role| role == "admin" || role == "super_admin")
    }

    pub fn get_user_by_id(&self, user_id: &str) -> Result<Option<UserAccountRecord>> {
        self.storage.get_user_account(user_id)
    }

    pub fn get_user_by_username(&self, username: &str) -> Result<Option<UserAccountRecord>> {
        self.storage.get_user_account_by_username(username)
    }

    pub fn get_user_by_email(&self, email: &str) -> Result<Option<UserAccountRecord>> {
        self.storage.get_user_account_by_email(email)
    }

    pub fn get_meta(&self, key: &str) -> Result<Option<String>> {
        self.storage.get_meta(key)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.storage.set_meta(key, value)
    }

    pub fn list_users(
        &self,
        keyword: Option<&str>,
        unit_ids: Option<&[String]>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<UserAccountRecord>, i64)> {
        self.storage
            .list_user_accounts(keyword, unit_ids, offset, limit)
    }

    pub fn list_org_units(&self) -> Result<Vec<OrgUnitRecord>> {
        self.storage.list_org_units()
    }

    pub fn get_org_unit(&self, unit_id: &str) -> Result<Option<OrgUnitRecord>> {
        self.storage.get_org_unit(unit_id)
    }

    pub fn upsert_org_unit(&self, record: &OrgUnitRecord) -> Result<()> {
        self.storage.upsert_org_unit(record)
    }

    pub fn delete_org_unit(&self, unit_id: &str) -> Result<i64> {
        self.storage.delete_org_unit(unit_id)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_user(
        &self,
        username: &str,
        email: Option<String>,
        password: &str,
        access_level: Option<&str>,
        unit_id: Option<String>,
        roles: Vec<String>,
        status: &str,
        is_demo: bool,
    ) -> Result<UserAccountRecord> {
        let password_hash = Self::hash_password(password)?;
        self.create_user_with_password_hash(
            username,
            email,
            password_hash,
            access_level,
            unit_id,
            roles,
            status,
            is_demo,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_user_with_password_hash(
        &self,
        username: &str,
        email: Option<String>,
        password_hash: String,
        access_level: Option<&str>,
        unit_id: Option<String>,
        roles: Vec<String>,
        status: &str,
        is_demo: bool,
    ) -> Result<UserAccountRecord> {
        if password_hash.trim().is_empty() {
            return Err(anyhow!("password hash is empty"));
        }
        let email = Self::normalize_optional_email(email);
        let user_id =
            Self::normalize_user_id(username).ok_or_else(|| anyhow!("invalid username"))?;
        if self
            .storage
            .get_user_account_by_username(&user_id)?
            .is_some()
        {
            return Err(anyhow!("username already exists"));
        }
        if let Some(email) = email
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            if self.storage.get_user_account_by_email(email)?.is_some() {
                return Err(anyhow!("email already exists"));
            }
        }
        let now = now_ts();
        let access_level = Self::normalize_access_level(access_level);
        let (unit_id, _) = self.resolve_unit_for_create(unit_id.as_deref())?;
        let record = UserAccountRecord {
            user_id: user_id.clone(),
            username: user_id.clone(),
            email,
            password_hash,
            roles: if roles.is_empty() {
                vec!["user".to_string()]
            } else {
                roles
            },
            status: status.trim().to_string(),
            access_level: access_level.clone(),
            unit_id,
            quota_balance: Self::default_daily_quota(),
            quota_granted_total: Self::default_daily_quota(),
            quota_used_total: 0,
            last_quota_grant_date: Some(Self::today_string()),
            experience_total: 0,
            is_demo,
            created_at: now,
            updated_at: now,
            last_login_at: None,
        };
        self.storage.upsert_user_account(&record)?;
        Ok(record)
    }

    fn resolve_unit_for_create(
        &self,
        unit_id: Option<&str>,
    ) -> Result<(Option<String>, Option<i32>)> {
        let cleaned = unit_id
            .map(|value| value.trim())
            .filter(|value| !value.is_empty());
        if let Some(cleaned) = cleaned {
            let unit = self
                .storage
                .get_org_unit(cleaned)?
                .ok_or_else(|| anyhow!("unit not found"))?;
            return Ok((Some(unit.unit_id), Some(unit.level)));
        }
        let units = self.storage.list_org_units()?;
        if let Some(default_unit) = org_units::resolve_default_root_unit(&units) {
            return Ok((Some(default_unit.unit_id), Some(default_unit.level)));
        }
        Ok((None, None))
    }

    pub fn update_user(&self, record: &UserAccountRecord) -> Result<()> {
        self.storage.update_user_account_profile(record)
    }

    pub fn effective_quota_status(
        user: &UserAccountRecord,
        today: Option<&str>,
    ) -> UserQuotaStatus {
        if Self::is_admin(user) {
            return UserQuotaStatus {
                balance: i64::MAX,
                granted_total: user.quota_granted_total.max(0),
                used_total: user.quota_used_total.max(0),
                daily_grant: 0,
                last_grant_date: user.last_quota_grant_date.clone(),
                allowed: true,
            };
        }
        let today = today
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(Self::today_string);
        let daily_grant = Self::default_daily_quota().max(0);
        let pending_grant =
            if daily_grant > 0 && user.last_quota_grant_date.as_deref() != Some(today.as_str()) {
                daily_grant
            } else {
                0
            };
        let balance = user.quota_balance.max(0).saturating_add(pending_grant);
        let granted_total = user
            .quota_granted_total
            .max(0)
            .saturating_add(pending_grant);
        UserQuotaStatus {
            balance,
            granted_total,
            used_total: user.quota_used_total.max(0),
            daily_grant,
            last_grant_date: Some(today),
            allowed: balance > 0,
        }
    }

    pub fn add_experience(&self, user_id: &str, delta: i64) -> Result<i64> {
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let delta = delta.max(0);
        if delta == 0 {
            let current = self
                .storage
                .get_user_account(cleaned)?
                .map(|user| normalize_total_experience(user.experience_total))
                .unwrap_or(0);
            return Ok(current);
        }
        self.storage
            .add_user_experience(cleaned, delta, now_ts())
            .map(|result| result.current_total)
    }

    pub fn upsert_users(&self, records: &[UserAccountRecord]) -> Result<()> {
        self.storage.upsert_user_accounts(records)
    }

    pub fn delete_user(&self, user_id: &str) -> Result<i64> {
        if Self::is_default_admin(user_id) {
            return Err(anyhow!("default admin account is protected"));
        }
        self.storage.delete_user_account(user_id)
    }

    pub fn set_password(&self, user_id: &str, password: &str) -> Result<()> {
        let mut record = self
            .storage
            .get_user_account(user_id)?
            .ok_or_else(|| anyhow!("user not found"))?;
        record.password_hash = Self::hash_password(password)?;
        record.updated_at = now_ts();
        self.storage.update_user_account_profile(&record)
    }

    pub fn create_session_token(&self, user_id: &str) -> Result<UserTokenRecord> {
        self.create_session_token_with_scope(user_id, DEFAULT_SESSION_SCOPE)
    }

    pub fn create_session_token_with_scope(
        &self,
        user_id: &str,
        session_scope: &str,
    ) -> Result<UserTokenRecord> {
        self.create_session_token_with_scope_and_family(user_id, session_scope, None)
    }

    /// Issue a session token bound to a refresh-token rotation family. The
    /// family id is only set for refresh-token chains (local sessions).
    pub fn create_session_token_with_scope_and_family(
        &self,
        user_id: &str,
        session_scope: &str,
        family_id: Option<&str>,
    ) -> Result<UserTokenRecord> {
        let now = now_ts();
        self.create_session_token_at(user_id, session_scope, now, family_id)
    }

    fn create_session_token_at(
        &self,
        user_id: &str,
        session_scope: &str,
        issued_at: f64,
        family_id: Option<&str>,
    ) -> Result<UserTokenRecord> {
        let expires_at = issued_at + DEFAULT_TOKEN_TTL_S as f64;
        let normalized_scope = Self::normalize_session_scope(Some(session_scope));
        let token = format!("wund_{}", Uuid::new_v4().simple());
        let record = UserTokenRecord {
            token: token.clone(),
            user_id: user_id.to_string(),
            session_scope: normalized_scope,
            family_id: family_id
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
            expires_at,
            created_at: issued_at,
            last_used_at: issued_at,
        };
        self.storage.create_user_token(&record)?;
        Ok(record)
    }

    /// Issue a refresh token sharing `family_id` with the paired access
    /// token. Only local scopes may hold refresh tokens.
    pub fn create_refresh_token(
        &self,
        user_id: &str,
        session_scope: &str,
        family_id: &str,
    ) -> Result<UserRefreshTokenRecord> {
        let issued_at = now_ts();
        let normalized_scope = Self::normalize_session_scope(Some(session_scope));
        let record = UserRefreshTokenRecord {
            refresh_token: format!("wundr_{}", Uuid::new_v4().simple()),
            user_id: user_id.to_string(),
            session_scope: normalized_scope,
            family_id: family_id.to_string(),
            expires_at: issued_at + REFRESH_TOKEN_TTL_S as f64,
            created_at: issued_at,
            last_used_at: issued_at,
            revoked: false,
        };
        self.storage.create_user_refresh_token(&record)?;
        Ok(record)
    }

    /// Scopes allowed to hold refresh tokens and use the refresh endpoint.
    pub fn is_local_refresh_scope(session_scope: &str) -> bool {
        matches!(
            Self::normalize_session_scope(Some(session_scope)).as_str(),
            "local_desktop" | "local_cli"
        )
    }

    pub fn issue_session_for_user(&self, user: UserAccountRecord) -> Result<UserSession> {
        self.issue_session_for_user_with_scope(user, DEFAULT_SESSION_SCOPE)
    }

    pub fn issue_session_for_user_with_scope(
        &self,
        user: UserAccountRecord,
        session_scope: &str,
    ) -> Result<UserSession> {
        self.issue_session_for_user_with_scope_and_family(user, session_scope, None)
    }

    /// Issue a session bound to a refresh-token rotation family; `family_id`
    /// is `None` for ordinary (web/default) sessions.
    pub fn issue_session_for_user_with_scope_and_family(
        &self,
        mut user: UserAccountRecord,
        session_scope: &str,
        family_id: Option<&str>,
    ) -> Result<UserSession> {
        let normalized_scope = Self::normalize_session_scope(Some(session_scope));
        let global_latest_login_at = self
            .storage
            .get_user_account(&user.user_id)?
            .and_then(|record| record.last_login_at)
            .or(user.last_login_at);
        let scoped_latest_login_at = self
            .storage
            .get_user_session_scope(&user.user_id, &normalized_scope)?
            .map(|record| record.last_login_at);
        let latest_login_at = match (scoped_latest_login_at, global_latest_login_at) {
            (Some(scoped), Some(global)) => Some(scoped.max(global)),
            (Some(scoped), None) => Some(scoped),
            (None, Some(global)) => Some(global),
            (None, None) => None,
        };
        let issued_at = next_session_issued_at(latest_login_at);
        user.last_login_at = Some(issued_at);
        user.updated_at = issued_at;
        self.storage.update_user_account_profile(&user)?;
        self.storage
            .upsert_user_session_scope(&UserSessionScopeRecord {
                user_id: user.user_id.clone(),
                session_scope: normalized_scope.clone(),
                last_login_at: issued_at,
            })?;
        let token =
            self.create_session_token_at(&user.user_id, &normalized_scope, issued_at, family_id)?;
        Ok(UserSession { user, token })
    }

    pub fn force_logout_user_scope(&self, user_id: &str, session_scope: &str) -> Result<f64> {
        let cleaned_user_id = user_id.trim();
        if cleaned_user_id.is_empty() {
            return Err(anyhow!("invalid user id"));
        }
        let normalized_scope = Self::normalize_session_scope(Some(session_scope));
        let latest_login_at = self
            .storage
            .get_user_session_scope(cleaned_user_id, &normalized_scope)?
            .map(|record| record.last_login_at);
        let invalidated_at = next_session_issued_at(latest_login_at);
        self.storage
            .upsert_user_session_scope(&UserSessionScopeRecord {
                user_id: cleaned_user_id.to_string(),
                session_scope: normalized_scope,
                last_login_at: invalidated_at,
            })?;
        Ok(invalidated_at)
    }

    pub fn authenticate_token(&self, token: &str) -> Result<Option<UserAccountRecord>> {
        Ok(self
            .authenticate_token_details(token)?
            .map(|authenticated| authenticated.user))
    }

    pub fn authenticate_token_details(
        &self,
        token: &str,
    ) -> Result<Option<AuthenticatedUserToken>> {
        let record = self.storage.get_user_token(token)?;
        let Some(record) = record else {
            return Ok(None);
        };
        let now = now_ts();
        if record.expires_at <= now {
            let _ = self.storage.delete_user_token(&record.token);
            return Ok(None);
        }
        let Some(user) = self.storage.get_user_account(&record.user_id)? else {
            return Ok(None);
        };
        if self
            .storage
            .get_user_session_scope(&record.user_id, &record.session_scope)?
            .is_some_and(|scope| record.created_at + 0.000_001 < scope.last_login_at)
        {
            let _ = self.storage.delete_user_token(&record.token);
            return Ok(None);
        }
        if user.status.trim().to_lowercase() != "active" {
            return Ok(None);
        }
        if self.should_touch_token_at(&record.token, record.last_used_at, now) {
            let _ = self.storage.touch_user_token(&record.token, now);
        }
        Ok(Some(AuthenticatedUserToken {
            user,
            session_scope: Self::normalize_session_scope(Some(&record.session_scope)),
            family_id: record.family_id,
        }))
    }

    fn should_touch_token_at(&self, token: &str, last_used_at: f64, now: f64) -> bool {
        if token.trim().is_empty() || now - last_used_at < TOKEN_TOUCH_MIN_INTERVAL_S {
            return false;
        }
        let mut recent = self
            .recent_token_touches
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        if let Some(previous) = recent.get(token) {
            if now - *previous < TOKEN_TOUCH_MIN_INTERVAL_S {
                return false;
            }
        }
        recent.insert(token.to_string(), now);
        if recent.len() > TOKEN_TOUCH_CACHE_MAX_ITEMS {
            recent.retain(|_, touched_at| now - *touched_at < TOKEN_TOUCH_MIN_INTERVAL_S);
        }
        true
    }

    pub fn login(&self, username: &str, password: &str) -> Result<UserSession> {
        self.login_with_scope(username, password, DEFAULT_SESSION_SCOPE)
    }

    pub fn login_with_scope(
        &self,
        username: &str,
        password: &str,
        session_scope: &str,
    ) -> Result<UserSession> {
        let user = self.verify_login_credentials(username, password)?;
        self.issue_session_for_user_with_scope(user, session_scope)
    }

    /// Password login for local (desktop/cli) scopes: issues the session and
    /// a refresh token that share one rotation family so reuse detection can
    /// revoke the whole chain.
    pub fn login_local_with_refresh_token(
        &self,
        username: &str,
        password: &str,
        session_scope: &str,
    ) -> Result<(UserSession, UserRefreshTokenRecord)> {
        let normalized_scope = Self::normalize_session_scope(Some(session_scope));
        if !Self::is_local_refresh_scope(&normalized_scope) {
            return Err(anyhow!("scope does not support refresh tokens"));
        }
        let user = self.verify_login_credentials(username, password)?;
        // A new local login replaces the previous session of the same scope:
        // revoke the old refresh families (and their access tokens) before
        // the new family exists, so a stale refresh token cannot resurrect
        // the kicked session via rotation.
        let kicked = self
            .storage
            .revoke_refresh_token_families_by_user_scope(&user.user_id, &normalized_scope)?;
        if kicked > 0 {
            tracing::info!(
                user_id = %user.user_id,
                session_scope = %normalized_scope,
                revoked_refresh_tokens = kicked,
                "previous local session families revoked by new login"
            );
        }
        let family_id = Uuid::new_v4().simple().to_string();
        let session = self.issue_session_for_user_with_scope_and_family(
            user,
            &normalized_scope,
            Some(&family_id),
        )?;
        let refresh_token = self.create_refresh_token(
            &session.token.user_id,
            &session.token.session_scope,
            &family_id,
        )?;
        Ok((session, refresh_token))
    }

    fn verify_login_credentials(
        &self,
        username: &str,
        password: &str,
    ) -> Result<UserAccountRecord> {
        let user_id =
            Self::normalize_user_id(username).ok_or_else(|| anyhow!("invalid username"))?;
        let user = match self.storage.get_user_account_by_username(&user_id)? {
            Some(user) => user,
            None => {
                if Self::is_default_admin(&user_id) {
                    self.ensure_default_admin()?;
                    self.storage
                        .get_user_account_by_username(&user_id)?
                        .ok_or_else(|| anyhow!("user not found"))?
                } else {
                    return Err(anyhow!("user not found"));
                }
            }
        };
        if user.status.trim().to_lowercase() != "active" {
            return Err(anyhow!("user disabled"));
        }
        if !Self::verify_password(&user.password_hash, password) {
            return Err(anyhow!("invalid password"));
        }
        Ok(user)
    }

    pub fn demo_login(&self, demo_id: Option<&str>) -> Result<UserSession> {
        self.demo_login_with_scope(demo_id, DEFAULT_SESSION_SCOPE)
    }

    pub fn demo_login_with_scope(
        &self,
        demo_id: Option<&str>,
        session_scope: &str,
    ) -> Result<UserSession> {
        let seed = normalize_demo_seed(demo_id);
        let username = format!("demo_{seed}");
        let maybe_user = self.storage.get_user_account_by_username(&username)?;
        let mut user = if let Some(existing) = maybe_user {
            existing
        } else {
            self.create_user(
                &username,
                Some(format!("{username}@demo.local")),
                &Uuid::new_v4().simple().to_string(),
                Some("A"),
                None,
                vec!["user".to_string()],
                "active",
                true,
            )?
        };
        if user.status.trim().to_lowercase() != "active" {
            user.status = "active".to_string();
        }
        if !user.roles.iter().any(|role| role == "user") {
            user.roles.push("user".to_string());
        }
        self.issue_session_for_user_with_scope(user, session_scope)
    }

    pub fn get_user_tool_access(&self, user_id: &str) -> Result<Option<UserToolAccessRecord>> {
        self.storage.get_user_tool_access(user_id)
    }

    pub fn set_user_tool_access(&self, user_id: &str, allowed: Option<&Vec<String>>) -> Result<()> {
        self.storage.set_user_tool_access(user_id, allowed)
    }

    pub fn get_user_agent_access(&self, user_id: &str) -> Result<Option<UserAgentAccessRecord>> {
        self.storage.get_user_agent_access(user_id)
    }

    pub fn set_user_agent_access(
        &self,
        user_id: &str,
        allowed_agent_ids: Option<&Vec<String>>,
        blocked_agent_ids: Option<&Vec<String>>,
    ) -> Result<()> {
        self.storage
            .set_user_agent_access(user_id, allowed_agent_ids, blocked_agent_ids)
    }

    pub fn upsert_chat_session(&self, record: &ChatSessionRecord) -> Result<()> {
        self.storage.upsert_chat_session(record)
    }

    pub fn get_chat_session(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<Option<ChatSessionRecord>> {
        self.storage.get_chat_session(user_id, session_id)
    }

    pub fn list_chat_sessions(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        parent_session_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ChatSessionRecord>, i64)> {
        self.storage
            .list_chat_sessions(user_id, agent_id, parent_session_id, offset, limit)
    }

    pub fn count_child_chat_sessions(
        &self,
        user_id: &str,
        parent_session_ids: &[String],
    ) -> Result<Vec<(String, i64)>> {
        self.storage
            .count_child_chat_sessions(user_id, parent_session_ids)
    }

    pub fn list_chat_sessions_by_status(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        parent_session_id: Option<&str>,
        status: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ChatSessionRecord>, i64)> {
        self.storage.list_chat_sessions_by_status(
            user_id,
            agent_id,
            parent_session_id,
            status,
            offset,
            limit,
        )
    }

    pub fn list_chat_session_agent_ids(&self, user_id: &str) -> Result<Vec<String>> {
        self.storage.list_chat_session_agent_ids(user_id)
    }

    pub fn update_chat_session_title(
        &self,
        user_id: &str,
        session_id: &str,
        title: &str,
        updated_at: f64,
    ) -> Result<()> {
        self.storage
            .update_chat_session_title(user_id, session_id, title, updated_at)
    }

    pub fn touch_chat_session(
        &self,
        user_id: &str,
        session_id: &str,
        updated_at: f64,
        last_message_at: f64,
    ) -> Result<()> {
        self.storage
            .touch_chat_session(user_id, session_id, updated_at, last_message_at)
    }

    pub fn delete_chat_session(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.storage.delete_chat_session(user_id, session_id)
    }

    pub fn upsert_session_run(&self, record: &SessionRunRecord) -> Result<()> {
        self.storage.upsert_session_run(record)
    }

    pub fn get_session_run(&self, run_id: &str) -> Result<Option<SessionRunRecord>> {
        self.storage.get_session_run(run_id)
    }

    pub fn upsert_user_agent(&self, record: &UserAgentRecord) -> Result<()> {
        self.storage.upsert_user_agent(record)
    }

    pub fn get_user_agent(&self, user_id: &str, agent_id: &str) -> Result<Option<UserAgentRecord>> {
        self.storage.get_user_agent(user_id, agent_id)
    }

    pub fn get_user_agent_by_id(&self, agent_id: &str) -> Result<Option<UserAgentRecord>> {
        self.storage.get_user_agent_by_id(agent_id)
    }

    pub fn resolve_agent_sandbox_container_id(&self, agent_id: Option<&str>) -> Option<i32> {
        let cleaned = agent_id.map(str::trim).filter(|value| !value.is_empty())?;
        let record = self.storage.get_user_agent_by_id(cleaned).ok().flatten()?;
        Some(normalize_sandbox_container_id(record.sandbox_container_id))
    }

    pub fn default_sandbox_container_id(&self) -> i32 {
        DEFAULT_SANDBOX_CONTAINER_ID
    }

    pub fn list_user_agents(&self, user_id: &str) -> Result<Vec<UserAgentRecord>> {
        self.storage.list_user_agents(user_id)
    }

    /// Agents of many users in one query; admin listings must not degrade into
    /// per-user lookups.
    pub fn list_user_agents_for_users(&self, user_ids: &[String]) -> Result<Vec<UserAgentRecord>> {
        self.storage.list_user_agents_for_users(user_ids)
    }

    /// Users whose single agent is bound to `preset_id`, paginated in SQL.
    pub fn list_preset_bound_agents(
        &self,
        preset_id: &str,
        keyword: Option<&str>,
        unit_ids: Option<&[String]>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<PresetBoundAgentRecord>, i64)> {
        self.storage
            .list_preset_bound_agents(preset_id, keyword, unit_ids, offset, limit)
    }

    /// Bound-user count per preset id in one grouped query.
    pub fn count_preset_bound_users(&self, preset_ids: &[String]) -> Result<Vec<(String, i64)>> {
        self.storage.count_preset_bound_users(preset_ids)
    }

    pub fn list_shared_user_agents(&self, user_id: &str) -> Result<Vec<UserAgentRecord>> {
        self.storage.list_shared_user_agents(user_id)
    }

    pub fn delete_user_agent(&self, user_id: &str, agent_id: &str) -> Result<i64> {
        self.storage.delete_user_agent(user_id, agent_id)
    }

    pub fn list_session_locks_by_user(&self, user_id: &str) -> Result<Vec<SessionLockRecord>> {
        self.storage.list_session_locks_by_user(user_id)
    }

    pub fn count_session_locks(&self) -> Result<i64> {
        self.storage.count_session_locks()
    }

    pub fn insert_agent_task(&self, record: &AgentTaskRecord) -> Result<()> {
        self.storage.insert_agent_task(record)
    }

    pub fn get_agent_task(&self, task_id: &str) -> Result<Option<AgentTaskRecord>> {
        self.storage.get_agent_task(task_id)
    }

    pub fn list_pending_agent_tasks(&self, limit: i64) -> Result<Vec<AgentTaskRecord>> {
        self.storage.list_pending_agent_tasks(limit)
    }

    pub fn count_pending_agent_tasks(&self) -> Result<i64> {
        self.storage.count_pending_agent_tasks()
    }

    pub fn count_pending_agent_tasks_ahead(
        &self,
        retry_at: f64,
        created_at: f64,
        task_id: &str,
    ) -> Result<i64> {
        self.storage
            .count_pending_agent_tasks_ahead(retry_at, created_at, task_id)
    }

    pub fn list_agent_tasks_by_thread(
        &self,
        thread_id: &str,
        status: Option<&str>,
        limit: i64,
    ) -> Result<Vec<AgentTaskRecord>> {
        self.storage
            .list_agent_tasks_by_thread(thread_id, status, limit)
    }

    pub fn update_agent_task_status(&self, params: UpdateAgentTaskStatusParams<'_>) -> Result<()> {
        self.storage.update_agent_task_status(params)
    }
}

fn now_ts() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}

fn normalize_demo_seed(value: Option<&str>) -> String {
    let raw = value.unwrap_or("").trim();
    if raw.is_empty() {
        return Uuid::new_v4()
            .simple()
            .to_string()
            .chars()
            .take(8)
            .collect();
    }
    let cleaned: String = raw
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '-')
        .collect();
    let cleaned = if cleaned.is_empty() {
        Uuid::new_v4().simple().to_string()
    } else {
        cleaned
    };
    cleaned.chars().take(24).collect()
}

pub(crate) fn list_user_agents_with_default(
    storage: &dyn StorageBackend,
    user_id: &str,
) -> Result<Vec<UserAgentRecord>> {
    let mut items = storage.list_user_agents(user_id)?;
    for item in &mut items {
        if item
            .agent_id
            .trim()
            .eq_ignore_ascii_case(DEFAULT_AGENT_ID_ALIAS)
        {
            normalize_default_agent_record(item);
        }
    }
    if items.iter().any(|agent| {
        agent
            .agent_id
            .trim()
            .eq_ignore_ascii_case(DEFAULT_AGENT_ID_ALIAS)
    }) {
        return Ok(items);
    }

    items.insert(
        0,
        build_default_agent_record_from_storage(storage, user_id)?,
    );
    Ok(items)
}

pub(crate) fn build_default_agent_record_from_storage(
    storage: &dyn StorageBackend,
    user_id: &str,
) -> Result<UserAgentRecord> {
    if let Some(mut existing) = storage.get_user_agent(user_id, DEFAULT_AGENT_ID_ALIAS)? {
        normalize_default_agent_record(&mut existing);
        return Ok(existing);
    }

    let key = default_agent_meta_key(user_id);
    let mut snapshot = storage
        .get_meta(&key)?
        .and_then(|raw| serde_json::from_str::<DefaultAgentConfigSnapshot>(&raw).ok())
        .unwrap_or_default();
    normalize_default_agent_snapshot(&mut snapshot);

    Ok(record_from_default_agent_config(
        DEFAULT_AGENT_ID_ALIAS,
        user_id,
        DEFAULT_AGENT_ACCESS_LEVEL,
        &snapshot,
    ))
}

fn normalize_default_agent_record(existing: &mut UserAgentRecord) {
    existing.access_level = DEFAULT_AGENT_ACCESS_LEVEL.to_string();
    if is_builtin_default_agent_name(&existing.name) {
        existing.name = DEFAULT_AGENT_NAME.to_string();
    }
    if existing.approval_mode.trim().is_empty() {
        existing.approval_mode = DEFAULT_AGENT_APPROVAL_MODE.to_string();
    }
    if existing.status.trim().is_empty() {
        existing.status = DEFAULT_AGENT_STATUS.to_string();
    }
    if existing.description.trim().is_empty() {
        existing.description = DEFAULT_AGENT_DESCRIPTION.to_string();
    }
    if existing.system_prompt.trim().is_empty() {
        existing.system_prompt = DEFAULT_AGENT_SYSTEM_PROMPT.to_string();
    }
    if existing.preset_questions.is_empty() {
        existing.preset_questions = default_agent_preset_questions();
    }
    for question in &mut existing.preset_questions {
        if question == LEGACY_AGENT_PRESET_QUESTION_DRAW_HEART {
            *question = DEFAULT_AGENT_PRESET_QUESTION_DRAW_GIF.to_string();
        }
    }
    existing.is_shared = false;
}

fn normalize_default_agent_snapshot(config: &mut DefaultAgentConfigSnapshot) {
    if is_builtin_default_agent_name(&config.name) {
        config.name = DEFAULT_AGENT_NAME.to_string();
    }
    if config.description.trim().is_empty() {
        config.description = DEFAULT_AGENT_DESCRIPTION.to_string();
    }
    if config.system_prompt.trim().is_empty() {
        config.system_prompt = DEFAULT_AGENT_SYSTEM_PROMPT.to_string();
    }
    if config.status.trim().is_empty() {
        config.status = DEFAULT_AGENT_STATUS.to_string();
    }
    if config.approval_mode.trim().is_empty() {
        config.approval_mode = DEFAULT_AGENT_APPROVAL_MODE.to_string();
    }
    config.tool_names = crate::services::user_agent_presets::normalize_tool_list(std::mem::take(
        &mut config.tool_names,
    ));
    config.ability_items = crate::services::agent_abilities::normalize_ability_items(
        std::mem::take(&mut config.ability_items),
    );
    config.declared_tool_names = crate::services::user_agent_presets::normalize_tool_list(
        std::mem::take(&mut config.declared_tool_names),
    );
    config.declared_skill_names = crate::services::user_agent_presets::normalize_tool_list(
        std::mem::take(&mut config.declared_skill_names),
    );
    config.preset_questions = crate::services::user_agent_presets::normalize_preset_questions(
        std::mem::take(&mut config.preset_questions),
    );
    for question in &mut config.preset_questions {
        if question == LEGACY_AGENT_PRESET_QUESTION_DRAW_HEART {
            *question = DEFAULT_AGENT_PRESET_QUESTION_DRAW_GIF.to_string();
        }
    }
    if config.preset_questions.is_empty() {
        config.preset_questions = default_agent_preset_questions();
    }
    config.sandbox_container_id = normalize_sandbox_container_id(config.sandbox_container_id);
    let now = now_ts();
    if config.created_at <= 0.0 {
        config.created_at = now;
    }
    if config.updated_at <= 0.0 {
        config.updated_at = config.created_at;
    }
}

fn default_agent_preset_questions() -> Vec<String> {
    vec![
        DEFAULT_AGENT_PRESET_QUESTION_DRAW_GIF.to_string(),
        DEFAULT_AGENT_PRESET_QUESTION_TRAVEL_GUIDE.to_string(),
    ]
}

fn next_session_issued_at(last_login_at: Option<f64>) -> f64 {
    let wall_micros = ts_to_micros(now_ts());
    let baseline_micros = last_login_at
        .map(ts_to_micros)
        .unwrap_or(0)
        .saturating_add(SESSION_TIME_EPSILON_MICROS);
    let mut candidate = wall_micros.max(baseline_micros);
    loop {
        let previous = LAST_SESSION_ISSUED_AT_MICROS.load(Ordering::Relaxed);
        let next = candidate.max(previous.saturating_add(SESSION_TIME_EPSILON_MICROS));
        match LAST_SESSION_ISSUED_AT_MICROS.compare_exchange(
            previous,
            next,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => return micros_to_ts(next),
            Err(observed) => {
                candidate = candidate.max(observed.saturating_add(SESSION_TIME_EPSILON_MICROS));
            }
        }
    }
}

fn ts_to_micros(value: f64) -> u64 {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    (value * 1_000_000.0).round().max(0.0) as u64
}

fn micros_to_ts(value: u64) -> f64 {
    value as f64 / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use super::UserStore;
    use crate::storage::*;
    use serde_json::json;
    use std::sync::Arc;
    use tempfile::tempdir;

    #[test]
    fn verify_password_invalid_hash_returns_false() {
        assert!(!UserStore::verify_password("invalid-hash", "secret"));
    }

    #[test]
    fn verify_password_checks_expected_password() {
        let hash = UserStore::hash_password("secret").expect("hash password");
        assert!(UserStore::verify_password(&hash, "secret"));
        assert!(!UserStore::verify_password(&hash, "wrong"));
    }

    #[test]
    fn should_touch_token_at_throttles_recent_updates() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("user-store-token-touch.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let store = UserStore::new(storage);

        assert!(!store.should_touch_token_at("token-a", 95.0, 100.0));
        assert!(store.should_touch_token_at("token-a", 0.0, 100.0));
        assert!(!store.should_touch_token_at("token-a", 0.0, 110.0));
        assert!(store.should_touch_token_at("token-a", 0.0, 131.0));
    }

    #[test]
    fn list_user_agents_with_default_injects_default_agent_snapshot() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("user-store-default-agent.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let store = UserStore::new(storage.clone());

        let user = store
            .create_user(
                "bob",
                None,
                "secret",
                Some("A"),
                None,
                vec!["user".to_string()],
                "active",
                false,
            )
            .expect("create user");

        storage
            .set_meta(
                &format!("default_agent:{}", user.user_id),
                &json!({
                    "name": "默认智能体",
                    "description": "系统级默认成员",
                    "preset_questions": ["帮我总结今天的待办", "先列一个三步执行方案"],
                    "approval_mode": "full_auto",
                    "status": "active"
                })
                .to_string(),
            )
            .expect("set default agent meta");

        let agents = crate::services::user_store::list_user_agents_with_default(
            store.storage_backend().as_ref(),
            &user.user_id,
        )
        .expect("list user agents");

        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].agent_id, "__default__");
        assert_eq!(agents[0].name, "默认智能体");
        assert_eq!(agents[0].description, "系统级默认成员");
        assert_eq!(
            agents[0].preset_questions,
            vec![
                "帮我总结今天的待办".to_string(),
                "先列一个三步执行方案".to_string()
            ]
        );
    }

    #[test]
    fn user_agent_roundtrip_preserves_preset_questions() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("user-store-agent-preset-questions.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let store = UserStore::new(storage);

        let user = store
            .create_user(
                "preset-agent-user",
                None,
                "secret",
                Some("A"),
                None,
                vec!["user".to_string()],
                "active",
                false,
            )
            .expect("create user");

        let record = crate::storage::UserAgentRecord {
            agent_id: "agent_preset_questions".to_string(),
            user_id: user.user_id.clone(),
            name: "预设问题测试".to_string(),
            description: String::new(),
            system_prompt: String::new(),
            preview_skill: false,
            model_name: None,
            ability_items: Vec::new(),
            tool_names: vec!["file_read".to_string()],
            declared_tool_names: Vec::new(),
            declared_skill_names: Vec::new(),
            visible_unit_ids: Vec::new(),
            preset_questions: vec![
                "请先帮我梳理现状".to_string(),
                "给我一个执行清单".to_string(),
            ],
            access_level: "A".to_string(),
            approval_mode: "full_auto".to_string(),
            is_shared: false,
            status: "active".to_string(),
            icon: None,
            sandbox_container_id: 1,
            created_at: 1.0,
            updated_at: 1.0,
            preset_binding: None,
            silent: false,
            prefer_mother: false,
        };

        store.upsert_user_agent(&record).expect("upsert agent");
        let loaded = store
            .get_user_agent(&user.user_id, &record.agent_id)
            .expect("get agent")
            .expect("agent exists");

        assert_eq!(loaded.preset_questions, record.preset_questions);
    }

    #[test]
    fn effective_quota_status_applies_pending_daily_grant_for_non_admin() {
        let user = UserAccountRecord {
            user_id: "alice".to_string(),
            username: "alice".to_string(),
            email: None,
            password_hash: "hash".to_string(),
            roles: vec!["user".to_string()],
            status: "active".to_string(),
            access_level: "A".to_string(),
            unit_id: Some("unit-l2".to_string()),
            quota_balance: 5,
            quota_granted_total: 20,
            quota_used_total: 7,
            last_quota_grant_date: Some("2026-04-09".to_string()),
            experience_total: 0,
            is_demo: false,
            created_at: 1.0,
            updated_at: 1.0,
            last_login_at: None,
        };

        let status = UserStore::effective_quota_status(&user, Some("2026-04-10"));

        assert_eq!(status.daily_grant, 1_000);
        assert_eq!(status.balance, 1_005);
        assert_eq!(status.granted_total, 1_020);
        assert_eq!(status.used_total, 7);
        assert_eq!(status.last_grant_date.as_deref(), Some("2026-04-10"));
        assert!(status.allowed);
    }

    #[test]
    fn effective_quota_status_is_unbounded_for_admins() {
        let user = UserAccountRecord {
            user_id: "admin-user".to_string(),
            username: "admin-user".to_string(),
            email: None,
            password_hash: "hash".to_string(),
            roles: vec!["admin".to_string()],
            status: "active".to_string(),
            access_level: "A".to_string(),
            unit_id: None,
            quota_balance: 0,
            quota_granted_total: 12,
            quota_used_total: 4,
            last_quota_grant_date: None,
            experience_total: 0,
            is_demo: false,
            created_at: 1.0,
            updated_at: 1.0,
            last_login_at: None,
        };

        let status = UserStore::effective_quota_status(&user, Some("2026-04-10"));

        assert_eq!(status.balance, i64::MAX);
        assert_eq!(status.granted_total, 12);
        assert_eq!(status.used_total, 4);
        assert_eq!(status.daily_grant, 0);
        assert!(status.allowed);
    }

    #[test]
    fn authenticate_token_rejects_previous_login_generation() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("user-store-session-generation.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let store = UserStore::new(storage);

        store
            .create_user(
                "alice",
                None,
                "secret",
                Some("A"),
                None,
                vec!["user".to_string()],
                "active",
                false,
            )
            .expect("create user");

        let first = store
            .login_with_scope("alice", "secret", "user_web")
            .expect("first login");
        let second = store
            .login_with_scope("alice", "secret", "user_web")
            .expect("second login");

        assert!(store
            .authenticate_token(&first.token.token)
            .expect("authenticate first token")
            .is_none());
        assert_eq!(
            store
                .authenticate_token(&second.token.token)
                .expect("authenticate second token")
                .expect("second token should be valid")
                .user_id,
            "alice"
        );
    }

    #[test]
    fn authenticate_token_keeps_other_scope_generation_valid() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("user-store-scope-isolation.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let store = UserStore::new(storage);

        store
            .create_user(
                "alice",
                None,
                "secret",
                Some("A"),
                None,
                vec!["user".to_string()],
                "active",
                false,
            )
            .expect("create user");

        let user_web = store
            .login_with_scope("alice", "secret", "user_web")
            .expect("user login");
        let admin_web = store
            .login_with_scope("alice", "secret", "admin_web")
            .expect("admin login");

        assert_eq!(
            store
                .authenticate_token_details(&user_web.token.token)
                .expect("authenticate user token")
                .expect("user token should stay valid")
                .session_scope,
            "user_web"
        );
        assert_eq!(
            store
                .authenticate_token_details(&admin_web.token.token)
                .expect("authenticate admin token")
                .expect("admin token should stay valid")
                .session_scope,
            "admin_web"
        );
    }

    #[test]
    fn force_logout_user_scope_invalidates_current_scope_tokens() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("user-store-force-logout.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let store = UserStore::new(storage);

        store
            .create_user(
                "alice",
                None,
                "secret",
                Some("A"),
                None,
                vec!["user".to_string()],
                "active",
                false,
            )
            .expect("create user");

        let user_web = store
            .login_with_scope("alice", "secret", "user_web")
            .expect("user login");
        let admin_web = store
            .login_with_scope("alice", "secret", "admin_web")
            .expect("admin login");

        let invalidated_at = store
            .force_logout_user_scope("alice", "user_web")
            .expect("force logout user scope");
        assert!(invalidated_at > 0.0);
        assert!(store
            .authenticate_token_details(&user_web.token.token)
            .expect("authenticate user token")
            .is_none());
        assert_eq!(
            store
                .authenticate_token_details(&admin_web.token.token)
                .expect("authenticate admin token")
                .expect("admin token should stay valid")
                .session_scope,
            "admin_web"
        );
    }

    #[test]
    fn create_user_treats_blank_email_as_none() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("user-store-blank-email.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let store = UserStore::new(storage);

        let first = store
            .create_user(
                "alice",
                Some("   ".to_string()),
                "secret",
                Some("A"),
                None,
                vec!["user".to_string()],
                "active",
                false,
            )
            .expect("create first user");
        let second = store
            .create_user(
                "bob",
                Some(String::new()),
                "secret",
                Some("A"),
                None,
                vec!["user".to_string()],
                "active",
                false,
            )
            .expect("create second user");

        assert_eq!(first.email, None);
        assert_eq!(second.email, None);
    }
}
