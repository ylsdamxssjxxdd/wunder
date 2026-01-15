use crate::storage::{ChatSessionRecord, StorageBackend, UserAccountRecord, UserTokenRecord};
use anyhow::{anyhow, Result};
use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

const DEFAULT_TOKEN_TTL_S: i64 = 7 * 24 * 3600;

#[derive(Debug, Clone, Serialize)]
pub struct UserProfile {
    pub id: String,
    pub username: String,
    pub email: Option<String>,
    pub roles: Vec<String>,
    pub status: String,
    pub access_level: String,
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
}

impl UserStore {
    pub fn new(storage: Arc<dyn StorageBackend>) -> Self {
        Self { storage }
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
        let parsed = PasswordHash::new(hash);
        if parsed.is_err() {
            return false;
        }
        Argon2::default()
            .verify_password(password.trim().as_bytes(), &parsed.unwrap())
            .is_ok()
    }

    pub fn to_profile(user: &UserAccountRecord) -> UserProfile {
        UserProfile {
            id: user.user_id.clone(),
            username: user.username.clone(),
            email: user.email.clone(),
            roles: user.roles.clone(),
            status: user.status.clone(),
            access_level: user.access_level.clone(),
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

    pub fn list_users(
        &self,
        keyword: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<UserAccountRecord>, i64)> {
        self.storage.list_user_accounts(keyword, offset, limit)
    }

    pub fn create_user(
        &self,
        username: &str,
        email: Option<String>,
        password: &str,
        access_level: Option<&str>,
        roles: Vec<String>,
        status: &str,
        is_demo: bool,
    ) -> Result<UserAccountRecord> {
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
        let record = UserAccountRecord {
            user_id: user_id.clone(),
            username: user_id.clone(),
            email,
            password_hash: Self::hash_password(password)?,
            roles: if roles.is_empty() {
                vec!["user".to_string()]
            } else {
                roles
            },
            status: status.trim().to_string(),
            access_level: Self::normalize_access_level(access_level),
            is_demo,
            created_at: now,
            updated_at: now,
            last_login_at: None,
        };
        self.storage.upsert_user_account(&record)?;
        Ok(record)
    }

    pub fn update_user(&self, record: &UserAccountRecord) -> Result<()> {
        self.storage.upsert_user_account(record)
    }

    pub fn delete_user(&self, user_id: &str) -> Result<i64> {
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
        let _ = self.storage.touch_user_token(&record.token, now);
        Ok(Some(user))
    }

    pub fn login(&self, username: &str, password: &str) -> Result<UserSession> {
        let user_id =
            Self::normalize_user_id(username).ok_or_else(|| anyhow!("invalid username"))?;
        let mut user = self
            .storage
            .get_user_account_by_username(&user_id)?
            .ok_or_else(|| anyhow!("user not found"))?;
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

    pub fn get_user_tool_access(&self, user_id: &str) -> Result<Option<Vec<String>>> {
        self.storage.get_user_tool_access(user_id)
    }

    pub fn set_user_tool_access(&self, user_id: &str, allowed: Option<&Vec<String>>) -> Result<()> {
        self.storage.set_user_tool_access(user_id, allowed)
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
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ChatSessionRecord>, i64)> {
        self.storage.list_chat_sessions(user_id, offset, limit)
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
