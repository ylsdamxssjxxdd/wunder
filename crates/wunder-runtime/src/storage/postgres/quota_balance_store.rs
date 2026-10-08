use super::PostgresStorage;
use crate::storage::{StorageLifecycle, UserQuotaStatus};
use anyhow::Result;

pub(super) trait PostgresQuotaBalanceStorage {
    fn set_user_quota_balance_impl(
        &self,
        user_id: &str,
        today: &str,
        daily_grant: i64,
        balance: i64,
    ) -> Result<Option<UserQuotaStatus>>;
    fn prepare_user_quota_impl(
        &self,
        user_id: &str,
        today: &str,
        daily_grant: i64,
    ) -> Result<Option<UserQuotaStatus>>;
    fn consume_user_quota_impl(
        &self,
        user_id: &str,
        today: &str,
        daily_grant: i64,
        amount: i64,
    ) -> Result<Option<UserQuotaStatus>>;
    fn grant_user_quota_impl(
        &self,
        user_id: &str,
        today: &str,
        daily_grant: i64,
        amount: i64,
        updated_at: f64,
    ) -> Result<Option<UserQuotaStatus>>;
}

impl PostgresQuotaBalanceStorage for PostgresStorage {
    fn prepare_user_quota_impl(
        &self,
        user_id: &str,
        today: &str,
        daily_grant: i64,
    ) -> Result<Option<UserQuotaStatus>> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let today = today.trim();
        if today.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        let row = tx.query_opt(
            "SELECT quota_balance, quota_granted_total, quota_used_total, last_quota_grant_date \
             FROM user_accounts WHERE user_id = $1 FOR UPDATE",
            &[&cleaned],
        )?;
        let Some(row) = row else {
            tx.commit()?;
            return Ok(None);
        };
        let mut balance: i64 = row.get::<_, Option<i64>>(0).unwrap_or(0).max(0);
        let mut granted_total: i64 = row.get::<_, Option<i64>>(1).unwrap_or(0).max(0);
        let used_total: i64 = row.get::<_, Option<i64>>(2).unwrap_or(0).max(0);
        let mut last_grant_date: Option<String> = row.get(3);
        let safe_daily_grant = daily_grant.max(0);
        if safe_daily_grant > 0 && last_grant_date.as_deref() != Some(today) {
            balance = balance.saturating_add(safe_daily_grant);
            granted_total = granted_total.saturating_add(safe_daily_grant);
            last_grant_date = Some(today.to_string());
            tx.execute(
                "UPDATE user_accounts
                 SET quota_balance = $1, quota_granted_total = $2, last_quota_grant_date = $3, updated_at = $4
                 WHERE user_id = $5",
                &[&balance, &granted_total, &last_grant_date, &Self::now_ts(), &cleaned],
            )?;
        }
        tx.commit()?;
        Ok(Some(UserQuotaStatus {
            balance,
            granted_total,
            used_total,
            daily_grant: safe_daily_grant,
            last_grant_date,
            allowed: balance > 0,
        }))
    }

    fn consume_user_quota_impl(
        &self,
        user_id: &str,
        today: &str,
        daily_grant: i64,
        amount: i64,
    ) -> Result<Option<UserQuotaStatus>> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let today = today.trim();
        if today.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        let row = tx.query_opt(
            "SELECT quota_balance, quota_granted_total, quota_used_total, last_quota_grant_date \
             FROM user_accounts WHERE user_id = $1 FOR UPDATE",
            &[&cleaned],
        )?;
        let Some(row) = row else {
            tx.commit()?;
            return Ok(None);
        };
        let mut balance: i64 = row.get::<_, Option<i64>>(0).unwrap_or(0).max(0);
        let mut granted_total: i64 = row.get::<_, Option<i64>>(1).unwrap_or(0).max(0);
        let mut used_total: i64 = row.get::<_, Option<i64>>(2).unwrap_or(0).max(0);
        let mut last_grant_date: Option<String> = row.get(3);
        let safe_daily_grant = daily_grant.max(0);
        if safe_daily_grant > 0 && last_grant_date.as_deref() != Some(today) {
            balance = balance.saturating_add(safe_daily_grant);
            granted_total = granted_total.saturating_add(safe_daily_grant);
            last_grant_date = Some(today.to_string());
        }
        let safe_amount = amount.max(0);
        // Admission and debit share the same lock: concurrent threads cannot overspend.
        let allowed = balance >= safe_amount;
        if allowed {
            balance -= safe_amount;
            used_total = used_total.saturating_add(safe_amount);
        }
        tx.execute(
            "UPDATE user_accounts
             SET quota_balance = $1, quota_granted_total = $2, quota_used_total = $3, last_quota_grant_date = $4, updated_at = $5
             WHERE user_id = $6",
            &[
                &balance,
                &granted_total,
                &used_total,
                &last_grant_date,
                &Self::now_ts(),
                &cleaned,
            ],
        )?;
        tx.commit()?;
        Ok(Some(UserQuotaStatus {
            balance,
            granted_total,
            used_total,
            daily_grant: safe_daily_grant,
            last_grant_date,
            allowed,
        }))
    }

    fn grant_user_quota_impl(
        &self,
        user_id: &str,
        today: &str,
        daily_grant: i64,
        amount: i64,
        updated_at: f64,
    ) -> Result<Option<UserQuotaStatus>> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let today = today.trim();
        if today.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        let row = tx.query_opt(
            "SELECT quota_balance, quota_granted_total, quota_used_total, last_quota_grant_date \
             FROM user_accounts WHERE user_id = $1 FOR UPDATE",
            &[&cleaned],
        )?;
        let Some(row) = row else {
            tx.commit()?;
            return Ok(None);
        };
        let mut balance: i64 = row.get::<_, Option<i64>>(0).unwrap_or(0).max(0);
        let mut granted_total: i64 = row.get::<_, Option<i64>>(1).unwrap_or(0).max(0);
        let used_total: i64 = row.get::<_, Option<i64>>(2).unwrap_or(0).max(0);
        let mut last_grant_date: Option<String> = row.get(3);
        let safe_daily_grant = daily_grant.max(0);
        if safe_daily_grant > 0 && last_grant_date.as_deref() != Some(today) {
            balance = balance.saturating_add(safe_daily_grant);
            granted_total = granted_total.saturating_add(safe_daily_grant);
            last_grant_date = Some(today.to_string());
        }
        let safe_amount = amount.max(0);
        if safe_amount > 0 {
            balance = balance.saturating_add(safe_amount);
            granted_total = granted_total.saturating_add(safe_amount);
            tx.execute(
                "UPDATE user_accounts
                 SET quota_balance = $1, quota_granted_total = $2, last_quota_grant_date = $3, updated_at = $4
                 WHERE user_id = $5",
                &[&balance, &granted_total, &last_grant_date, &updated_at, &cleaned],
            )?;
        } else if safe_daily_grant > 0 && last_grant_date.as_deref() == Some(today) {
            tx.execute(
                "UPDATE user_accounts
                 SET quota_balance = $1, quota_granted_total = $2, last_quota_grant_date = $3, updated_at = $4
                 WHERE user_id = $5",
                &[&balance, &granted_total, &last_grant_date, &updated_at, &cleaned],
            )?;
        }
        tx.commit()?;
        Ok(Some(UserQuotaStatus {
            balance,
            granted_total,
            used_total,
            daily_grant: safe_daily_grant,
            last_grant_date,
            allowed: balance > 0,
        }))
    }
    fn set_user_quota_balance_impl(
        &self,
        user_id: &str,
        today: &str,
        daily_grant: i64,
        target_balance: i64,
    ) -> Result<Option<UserQuotaStatus>> {
        self.ensure_initialized()?;
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let today = today.trim();
        if today.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        let row = tx.query_opt(
            "SELECT quota_balance, quota_granted_total, quota_used_total, last_quota_grant_date \
             FROM user_accounts WHERE user_id = $1 FOR UPDATE",
            &[&cleaned],
        )?;
        let Some(row) = row else {
            tx.commit()?;
            return Ok(None);
        };
        let mut granted_total: i64 = row.get::<_, Option<i64>>(1).unwrap_or(0).max(0);
        let used_total: i64 = row.get::<_, Option<i64>>(2).unwrap_or(0).max(0);
        let mut last_grant_date: Option<String> = row.get(3);
        let safe_daily_grant = daily_grant.max(0);
        if safe_daily_grant > 0 && last_grant_date.as_deref() != Some(today) {
            granted_total = granted_total.saturating_add(safe_daily_grant);
        }
        let balance = target_balance.max(0);
        // The explicit value is the final balance after today's automatic grant.
        last_grant_date = Some(today.to_string());
        let updated_at = Self::now_ts();
        tx.execute(
            "UPDATE user_accounts SET quota_balance = $1, quota_granted_total = $2, last_quota_grant_date = $3, updated_at = $4 WHERE user_id = $5",
            &[&balance, &granted_total, &last_grant_date, &updated_at, &cleaned],
        )?;
        tx.commit()?;
        Ok(Some(UserQuotaStatus {
            balance,
            granted_total,
            used_total,
            daily_grant: safe_daily_grant,
            last_grant_date,
            allowed: balance > 0,
        }))
    }
}
