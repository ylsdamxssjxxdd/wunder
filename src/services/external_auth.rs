use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use tokio::sync::Mutex;
use uuid::Uuid;

/// One-time login codes for embedding / SSO exchange.
///
/// Notes:
/// - Codes are stored in-memory with a short TTL (best-effort cleanup).
/// - This is intentionally ephemeral; a server restart invalidates issued codes.
#[derive(Clone, Default)]
pub struct ExternalAuthCodeStore {
    inner: Arc<Mutex<HashMap<String, ExternalAuthCodeRecord>>>,
}

#[derive(Debug, Clone)]
pub struct ExternalAuthCodeRecord {
    pub code: String,
    pub user_id: String,
    pub token: String,
    pub expires_at: f64,
}

impl ExternalAuthCodeStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn issue(&self, user_id: String, token: String, ttl_s: f64) -> ExternalAuthCodeRecord {
        let now = now_ts();
        let expires_at = now + ttl_s.max(1.0);
        let record = ExternalAuthCodeRecord {
            code: format!("wundc_{}", Uuid::new_v4().simple()),
            user_id,
            token,
            expires_at,
        };

        let mut guard = self.inner.lock().await;
        cleanup_expired(&mut guard, now);
        guard.insert(record.code.clone(), record.clone());
        record
    }

    /// Take a code and delete it from the store (one-time).
    pub async fn take(&self, code: &str) -> Option<ExternalAuthCodeRecord> {
        let cleaned = code.trim();
        if cleaned.is_empty() {
            return None;
        }
        let now = now_ts();
        let mut guard = self.inner.lock().await;
        cleanup_expired(&mut guard, now);
        let record = guard.remove(cleaned)?;
        if record.expires_at <= now {
            return None;
        }
        Some(record)
    }
}

fn cleanup_expired(map: &mut HashMap<String, ExternalAuthCodeRecord>, now: f64) {
    map.retain(|_, record| record.expires_at > now);
    // Keep a hard cap to avoid unbounded growth when attacked.
    const MAX_CODES: usize = 4096;
    if map.len() > MAX_CODES {
        map.clear();
    }
}

fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}
