use crate::org_units;
use crate::storage::{
    normalize_hive_id, normalize_sandbox_container_id, AgentTaskRecord, AgentThreadRecord,
    BeeroomChatMessageRecord, ChatSessionRecord, HiveRecord, OrgUnitRecord, SessionLockRecord,
    SessionRunRecord, StorageBackend, TeamRunRecord, TeamTaskRecord, UpdateAgentTaskStatusParams,
    UserAccountRecord, UserAgentAccessRecord, UserAgentRecord, UserTokenRecord,
    UserToolAccessRecord, DEFAULT_HIVE_ID, DEFAULT_SANDBOX_CONTAINER_ID,
};
use anyhow::{anyhow, Result};
use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::warn;
use uuid::Uuid;

const DEFAULT_TOKEN_TTL_S: i64 = 7 * 24 * 3600;
const TOKEN_TOUCH_MIN_INTERVAL_S: f64 = 30.0;
const TOKEN_TOUCH_CACHE_MAX_ITEMS: usize = 4096;
const DEFAULT_ADMIN_USER_ID: &str = "admin";
const DEFAULT_ADMIN_PASSWORD: &str = "admin";
const DEFAULT_DAILY_QUOTA_L1: i64 = 10_000;
const DEFAULT_DAILY_QUOTA_L2: i64 = 5_000;
const DEFAULT_DAILY_QUOTA_L3: i64 = 1_000;
const DEFAULT_DAILY_QUOTA_L4: i64 = 100;
const DEFAULT_HIVE_NAME: &str = "默认蜂群";
const DEFAULT_HIVE_DESCRIPTION: &str = "系统默认蜂群，用于承载初始智能体应用。";
const DEFAULT_AGENT_ID_ALIAS: &str = "__default__";
const DEFAULT_AGENT_META_PREFIX: &str = "default_agent:";
const DEFAULT_AGENT_NAME: &str = "Default Agent";
const DEFAULT_AGENT_STATUS: &str = "active";
const DEFAULT_AGENT_APPROVAL_MODE: &str = "full_auto";
const DEFAULT_AGENT_ACCESS_LEVEL: &str = "A";

#[derive(Debug, Clone, Deserialize, Default)]
struct DefaultAgentConfigSnapshot {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    system_prompt: String,
    #[serde(default)]
    tool_names: Vec<String>,
    #[serde(default)]
    preset_questions: Vec<String>,
    #[serde(default)]
    approval_mode: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    sandbox_container_id: i32,
    #[serde(default)]
    created_at: f64,
    #[serde(default)]
    updated_at: f64,
}

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
    pub daily_quota: i64,
    pub daily_quota_used: i64,
    pub daily_quota_date: Option<String>,
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

pub struct UserStore {
    storage: Arc<dyn StorageBackend>,
    recent_token_touches: Mutex<HashMap<String, f64>>,
}

impl UserStore {
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
                self.storage.upsert_user_account(&existing)?;
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

    pub fn default_daily_quota_by_level(level: Option<i32>) -> i64 {
        match level.unwrap_or(1) {
            2 => DEFAULT_DAILY_QUOTA_L2,
            3 => DEFAULT_DAILY_QUOTA_L3,
            4 => DEFAULT_DAILY_QUOTA_L4,
            _ => DEFAULT_DAILY_QUOTA_L1,
        }
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
        let unit_profile = unit.map(|unit| UserUnitProfile {
            id: unit.unit_id.clone(),
            name: unit.name.clone(),
            path: unit.path.clone(),
            path_name: unit.path_name.clone(),
            level: unit.level,
        });
        UserProfile {
            id: user.user_id.clone(),
            username: user.username.clone(),
            email: user.email.clone(),
            roles: user.roles.clone(),
            status: user.status.clone(),
            access_level: user.access_level.clone(),
            unit_id: user.unit_id.clone(),
            unit: unit_profile,
            daily_quota: user.daily_quota,
            daily_quota_used: user.daily_quota_used,
            daily_quota_date: user.daily_quota_date.clone(),
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
        let (unit_id, unit_level) = self.resolve_unit_for_create(unit_id.as_deref())?;
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
            daily_quota: Self::default_daily_quota_by_level(unit_level),
            daily_quota_used: 0,
            daily_quota_date: None,
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
        self.storage.upsert_user_account(record)
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
        self.storage.upsert_user_account(&record)
    }

    pub fn create_session_token(&self, user_id: &str) -> Result<UserTokenRecord> {
        let now = now_ts();
        let expires_at = now + DEFAULT_TOKEN_TTL_S as f64;
        let token = format!("wund_{}", Uuid::new_v4().simple());
        let record = UserTokenRecord {
            token: token.clone(),
            user_id: user_id.to_string(),
            expires_at,
            created_at: now,
            last_used_at: now,
        };
        self.storage.create_user_token(&record)?;
        Ok(record)
    }

    pub fn authenticate_token(&self, token: &str) -> Result<Option<UserAccountRecord>> {
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
        if user.status.trim().to_lowercase() != "active" {
            return Ok(None);
        }
        if self.should_touch_token_at(&record.token, record.last_used_at, now) {
            let _ = self.storage.touch_user_token(&record.token, now);
        }
        Ok(Some(user))
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
        let user_id =
            Self::normalize_user_id(username).ok_or_else(|| anyhow!("invalid username"))?;
        let mut user = match self.storage.get_user_account_by_username(&user_id)? {
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
        let now = now_ts();
        user.last_login_at = Some(now);
        user.updated_at = now;
        self.storage.upsert_user_account(&user)?;
        let token = self.create_session_token(&user.user_id)?;
        Ok(UserSession { user, token })
    }

    pub fn demo_login(&self, demo_id: Option<&str>) -> Result<UserSession> {
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
        let now = now_ts();
        if user.status.trim().to_lowercase() != "active" {
            user.status = "active".to_string();
        }
        if !user.roles.iter().any(|role| role == "user") {
            user.roles.push("user".to_string());
        }
        user.last_login_at = Some(now);
        user.updated_at = now;
        self.storage.upsert_user_account(&user)?;
        let token = self.create_session_token(&user.user_id)?;
        Ok(UserSession { user, token })
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

    pub fn list_beeroom_chat_messages(
        &self,
        user_id: &str,
        group_id: &str,
        before_message_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<BeeroomChatMessageRecord>> {
        self.storage
            .list_beeroom_chat_messages(user_id, group_id, before_message_id, limit)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn append_beeroom_chat_message(
        &self,
        user_id: &str,
        group_id: &str,
        sender_kind: &str,
        sender_name: &str,
        sender_agent_id: Option<&str>,
        mention_name: Option<&str>,
        mention_agent_id: Option<&str>,
        body: &str,
        meta: Option<&str>,
        tone: &str,
        client_msg_id: Option<&str>,
        created_at: f64,
    ) -> Result<BeeroomChatMessageRecord> {
        self.storage.append_beeroom_chat_message(
            user_id,
            group_id,
            sender_kind,
            sender_name,
            sender_agent_id,
            mention_name,
            mention_agent_id,
            body,
            meta,
            tone,
            client_msg_id,
            created_at,
        )
    }

    pub fn delete_beeroom_chat_messages(&self, user_id: &str, group_id: &str) -> Result<i64> {
        self.storage.delete_beeroom_chat_messages(user_id, group_id)
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

    pub fn default_hive_id(&self) -> String {
        DEFAULT_HIVE_ID.to_string()
    }

    pub fn resolve_agent_hive_id(&self, agent_id: Option<&str>) -> Option<String> {
        let cleaned = agent_id.map(str::trim).filter(|value| !value.is_empty())?;
        let record = self.storage.get_user_agent_by_id(cleaned).ok().flatten()?;
        Some(normalize_hive_id(&record.hive_id))
    }

    pub fn ensure_default_hive(&self, user_id: &str) -> Result<HiveRecord> {
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Err(anyhow!("user_id is empty"));
        }
        if let Some(mut existing) = self.storage.get_hive(cleaned_user, DEFAULT_HIVE_ID)? {
            let mut changed = false;
            if !existing.is_default {
                existing.is_default = true;
                changed = true;
            }
            if existing.status.trim().eq_ignore_ascii_case("archived") {
                existing.status = "active".to_string();
                changed = true;
            }
            if existing.name.trim().is_empty() {
                existing.name = DEFAULT_HIVE_NAME.to_string();
                changed = true;
            }
            if existing.description.trim().is_empty() {
                existing.description = DEFAULT_HIVE_DESCRIPTION.to_string();
                changed = true;
            }
            if changed {
                existing.updated_time = now_ts();
                self.storage.upsert_hive(&existing)?;
            }
            return Ok(existing);
        }

        let now = now_ts();
        let default = HiveRecord {
            hive_id: DEFAULT_HIVE_ID.to_string(),
            user_id: cleaned_user.to_string(),
            name: DEFAULT_HIVE_NAME.to_string(),
            description: DEFAULT_HIVE_DESCRIPTION.to_string(),
            is_default: true,
            status: "active".to_string(),
            created_time: now,
            updated_time: now,
        };
        self.storage.upsert_hive(&default)?;
        Ok(default)
    }

    pub fn upsert_hive(&self, record: &HiveRecord) -> Result<()> {
        self.storage.upsert_hive(record)
    }

    pub fn get_hive(&self, user_id: &str, hive_id: &str) -> Result<Option<HiveRecord>> {
        self.storage.get_hive(user_id, hive_id)
    }

    pub fn list_hives(&self, user_id: &str, include_archived: bool) -> Result<Vec<HiveRecord>> {
        self.storage.list_hives(user_id, include_archived)
    }

    pub fn delete_hive(&self, user_id: &str, hive_id: &str) -> Result<i64> {
        self.storage.delete_hive(user_id, hive_id)
    }

    pub fn move_agents_to_hive(
        &self,
        user_id: &str,
        hive_id: &str,
        agent_ids: &[String],
    ) -> Result<i64> {
        self.storage
            .move_agents_to_hive(user_id, hive_id, agent_ids)
    }

    pub fn list_user_agents(&self, user_id: &str) -> Result<Vec<UserAgentRecord>> {
        self.storage.list_user_agents(user_id)
    }

    pub fn list_user_agents_by_hive(
        &self,
        user_id: &str,
        hive_id: &str,
    ) -> Result<Vec<UserAgentRecord>> {
        self.storage.list_user_agents_by_hive(user_id, hive_id)
    }

    pub fn list_user_agents_by_hive_with_default(
        &self,
        user_id: &str,
        hive_id: &str,
    ) -> Result<Vec<UserAgentRecord>> {
        list_user_agents_by_hive_with_default(self.storage.as_ref(), user_id, hive_id)
    }

    pub fn list_shared_user_agents(&self, user_id: &str) -> Result<Vec<UserAgentRecord>> {
        self.storage.list_shared_user_agents(user_id)
    }

    pub fn delete_user_agent(&self, user_id: &str, agent_id: &str) -> Result<i64> {
        self.storage.delete_user_agent(user_id, agent_id)
    }

    pub fn upsert_team_run(&self, record: &TeamRunRecord) -> Result<()> {
        self.storage.upsert_team_run(record)
    }

    pub fn delete_team_runs_by_hive(&self, user_id: &str, hive_id: &str) -> Result<i64> {
        self.storage.delete_team_runs_by_hive(user_id, hive_id)
    }

    pub fn get_team_run(&self, team_run_id: &str) -> Result<Option<TeamRunRecord>> {
        self.storage.get_team_run(team_run_id)
    }

    pub fn list_team_runs(
        &self,
        user_id: &str,
        hive_id: Option<&str>,
        parent_session_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<TeamRunRecord>, i64)> {
        self.storage
            .list_team_runs(user_id, hive_id, parent_session_id, offset, limit)
    }

    pub fn list_team_runs_by_status(
        &self,
        statuses: &[&str],
        offset: i64,
        limit: i64,
    ) -> Result<Vec<TeamRunRecord>> {
        self.storage
            .list_team_runs_by_status(statuses, offset, limit)
    }

    pub fn upsert_team_task(&self, record: &TeamTaskRecord) -> Result<()> {
        self.storage.upsert_team_task(record)
    }

    pub fn list_team_tasks(&self, team_run_id: &str) -> Result<Vec<TeamTaskRecord>> {
        self.storage.list_team_tasks(team_run_id)
    }

    pub fn get_team_task(&self, task_id: &str) -> Result<Option<TeamTaskRecord>> {
        self.storage.get_team_task(task_id)
    }

    pub fn list_session_locks_by_user(&self, user_id: &str) -> Result<Vec<SessionLockRecord>> {
        self.storage.list_session_locks_by_user(user_id)
    }

    pub fn upsert_agent_thread(&self, record: &AgentThreadRecord) -> Result<()> {
        self.storage.upsert_agent_thread(record)
    }

    pub fn get_agent_thread(
        &self,
        user_id: &str,
        agent_id: &str,
    ) -> Result<Option<AgentThreadRecord>> {
        self.storage.get_agent_thread(user_id, agent_id)
    }

    pub fn delete_agent_thread(&self, user_id: &str, agent_id: &str) -> Result<i64> {
        self.storage.delete_agent_thread(user_id, agent_id)
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

fn default_agent_meta_key(user_id: &str) -> String {
    format!("{DEFAULT_AGENT_META_PREFIX}{}", user_id.trim())
}

pub(crate) fn list_user_agents_by_hive_with_default(
    storage: &dyn StorageBackend,
    user_id: &str,
    hive_id: &str,
) -> Result<Vec<UserAgentRecord>> {
    let mut items = storage.list_user_agents_by_hive(user_id, hive_id)?;
    if normalize_hive_id(hive_id) != DEFAULT_HIVE_ID {
        return Ok(items);
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
        existing.hive_id = DEFAULT_HIVE_ID.to_string();
        existing.access_level = DEFAULT_AGENT_ACCESS_LEVEL.to_string();
        if existing.approval_mode.trim().is_empty() {
            existing.approval_mode = DEFAULT_AGENT_APPROVAL_MODE.to_string();
        }
        if existing.status.trim().is_empty() {
            existing.status = DEFAULT_AGENT_STATUS.to_string();
        }
        existing.is_shared = false;
        return Ok(existing);
    }

    let key = default_agent_meta_key(user_id);
    let mut snapshot = storage
        .get_meta(&key)?
        .and_then(|raw| serde_json::from_str::<DefaultAgentConfigSnapshot>(&raw).ok())
        .unwrap_or_default();
    normalize_default_agent_snapshot(&mut snapshot);

    Ok(UserAgentRecord {
        agent_id: DEFAULT_AGENT_ID_ALIAS.to_string(),
        user_id: user_id.trim().to_string(),
        hive_id: DEFAULT_HIVE_ID.to_string(),
        name: snapshot.name,
        description: snapshot.description,
        system_prompt: snapshot.system_prompt,
        model_name: None,
        tool_names: snapshot.tool_names.clone(),
        declared_tool_names: Vec::new(),
        declared_skill_names: Vec::new(),
        preset_questions: snapshot.preset_questions,
        access_level: DEFAULT_AGENT_ACCESS_LEVEL.to_string(),
        approval_mode: snapshot.approval_mode,
        is_shared: false,
        status: snapshot.status,
        icon: snapshot.icon,
        sandbox_container_id: snapshot.sandbox_container_id,
        created_at: snapshot.created_at,
        updated_at: snapshot.updated_at,
        preset_binding: None,
    })
}

fn normalize_default_agent_snapshot(config: &mut DefaultAgentConfigSnapshot) {
    if config.name.trim().is_empty() {
        config.name = DEFAULT_AGENT_NAME.to_string();
    }
    if config.status.trim().is_empty() {
        config.status = DEFAULT_AGENT_STATUS.to_string();
    }
    if config.approval_mode.trim().is_empty() {
        config.approval_mode = DEFAULT_AGENT_APPROVAL_MODE.to_string();
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

#[cfg(test)]
mod tests {
    use super::UserStore;
    use crate::storage::{HiveRecord, SqliteStorage, StorageBackend, DEFAULT_HIVE_ID};
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
    fn ensure_default_hive_creates_default_even_when_other_groups_exist() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("user-store-default-hive.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let store = UserStore::new(storage.clone());

        let user = store
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

        let now = 1_710_000_000_f64;
        let other_hive = HiveRecord {
            hive_id: "intel".to_string(),
            user_id: user.user_id.clone(),
            name: "鎯呮姤铚傜兢".to_string(),
            description: "宸叉湁铚傜兢".to_string(),
            is_default: false,
            status: "active".to_string(),
            created_time: now,
            updated_time: now,
        };
        storage.upsert_hive(&other_hive).expect("upsert other hive");

        let default_hive = store
            .ensure_default_hive(&user.user_id)
            .expect("ensure default hive");
        let hives = store.list_hives(&user.user_id, false).expect("list hives");

        assert_eq!(default_hive.hive_id, DEFAULT_HIVE_ID);
        assert_eq!(default_hive.name, "默认蜂群");
        assert_eq!(hives.len(), 2);
        assert_eq!(hives[0].hive_id, DEFAULT_HIVE_ID);
        assert!(hives.iter().any(|item| item.hive_id == "intel"));
    }

    #[test]
    fn list_user_agents_by_hive_with_default_injects_default_agent_snapshot() {
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

        store
            .ensure_default_hive(&user.user_id)
            .expect("ensure default hive");

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

        let agents = store
            .list_user_agents_by_hive_with_default(&user.user_id, DEFAULT_HIVE_ID)
            .expect("list hive agents");

        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].agent_id, "__default__");
        assert_eq!(agents[0].hive_id, DEFAULT_HIVE_ID);
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
            hive_id: DEFAULT_HIVE_ID.to_string(),
            name: "预设问题测试".to_string(),
            description: String::new(),
            system_prompt: String::new(),
            model_name: None,
            tool_names: vec!["file_read".to_string()],
            declared_tool_names: Vec::new(),
            declared_skill_names: Vec::new(),
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
        };

        store.upsert_user_agent(&record).expect("upsert agent");
        let loaded = store
            .get_user_agent(&user.user_id, &record.agent_id)
            .expect("get agent")
            .expect("agent exists");

        assert_eq!(loaded.preset_questions, record.preset_questions);
    }

    #[test]
    fn beeroom_chat_messages_can_append_and_list() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("user-store-beeroom-chat.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let store = UserStore::new(storage.clone());

        store
            .append_beeroom_chat_message(
                "alice",
                DEFAULT_HIVE_ID,
                "user",
                "User",
                None,
                Some("Ops"),
                Some("agent-ops"),
                "请开始任务",
                Some("default"),
                "user",
                Some("msg-1"),
                100.0,
            )
            .expect("append first beeroom chat message");
        store
            .append_beeroom_chat_message(
                "alice",
                DEFAULT_HIVE_ID,
                "agent",
                "Ops",
                Some("agent-ops"),
                None,
                None,
                "收到，开始执行",
                Some("result"),
                "worker",
                Some("msg-2"),
                101.0,
            )
            .expect("append second beeroom chat message");

        let messages = store
            .list_beeroom_chat_messages("alice", DEFAULT_HIVE_ID, None, 20)
            .expect("list beeroom chat messages");

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].body, "请开始任务");
        assert_eq!(messages[1].sender_agent_id.as_deref(), Some("agent-ops"));
    }

    #[test]
    fn beeroom_chat_messages_can_clear_by_group() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("user-store-beeroom-chat-clear.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let store = UserStore::new(storage.clone());

        store
            .append_beeroom_chat_message(
                "alice",
                DEFAULT_HIVE_ID,
                "system",
                "铚傜兢",
                None,
                None,
                None,
                "寰呭懡",
                None,
                "system",
                Some("msg-clear"),
                100.0,
            )
            .expect("append beeroom chat message");

        let deleted = store
            .delete_beeroom_chat_messages("alice", DEFAULT_HIVE_ID)
            .expect("clear beeroom chat messages");
        let remaining = store
            .list_beeroom_chat_messages("alice", DEFAULT_HIVE_ID, None, 20)
            .expect("list remaining beeroom chat messages");

        assert_eq!(deleted, 1);
        assert!(remaining.is_empty());
    }
}
