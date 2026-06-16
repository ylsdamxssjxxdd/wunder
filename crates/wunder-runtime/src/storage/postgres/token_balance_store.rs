use super::PostgresStorage;
use crate::storage::{StorageLifecycle, UserTokenBalanceStatus};
use anyhow::Result;

pub(super) trait PostgresTokenBalanceStorage {
    fn prepare_user_token_balance_impl(
        &self,
        user_id: &str,
        today: &str,
        daily_grant: i64,
    ) -> Result<Option<UserTokenBalanceStatus>>;
    fn consume_user_tokens_impl(
        &self,
        user_id: &str,
        today: &str,
        daily_grant: i64,
        amount: i64,
    ) -> Result<Option<UserTokenBalanceStatus>>;
    fn grant_user_tokens_impl(
        &self,
        user_id: &str,
        today: &str,
        daily_grant: i64,
        amount: i64,
        updated_at: f64,
    ) -> Result<Option<UserTokenBalanceStatus>>;
}

impl PostgresTokenBalanceStorage for PostgresStorage {
    fn prepare_user_token_balance_impl(
        &self,
        user_id: &str,
        today: &str,
        daily_grant: i64,
    ) -> Result<Option<UserTokenBalanceStatus>> {
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
            "SELECT token_balance, token_granted_total, token_used_total, last_token_grant_date \
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
                 SET token_balance = $1, token_granted_total = $2, last_token_grant_date = $3, updated_at = $4
                 WHERE user_id = $5",
                &[&balance, &granted_total, &last_grant_date, &Self::now_ts(), &cleaned],
            )?;
        }
        tx.commit()?;
        Ok(Some(UserTokenBalanceStatus {
            balance,
            granted_total,
            used_total,
            daily_grant: safe_daily_grant,
            last_grant_date,
            allowed: balance > 0,
            overspent_tokens: 0,
        }))
    }

    fn consume_user_tokens_impl(
        &self,
        user_id: &str,
        today: &str,
        daily_grant: i64,
        amount: i64,
    ) -> Result<Option<UserTokenBalanceStatus>> {
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
            "SELECT token_balance, token_granted_total, token_used_total, last_token_grant_date \
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
        let charged = balance.min(safe_amount);
        let overspent_tokens = safe_amount.saturating_sub(charged);
        balance = balance.saturating_sub(charged);
        used_total = used_total.saturating_add(safe_amount);
        tx.execute(
            "UPDATE user_accounts
             SET token_balance = $1, token_granted_total = $2, token_used_total = $3, last_token_grant_date = $4, updated_at = $5
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
        Ok(Some(UserTokenBalanceStatus {
            balance,
            granted_total,
            used_total,
            daily_grant: safe_daily_grant,
            last_grant_date,
            allowed: balance > 0,
            overspent_tokens,
        }))
    }

    fn grant_user_tokens_impl(
        &self,
        user_id: &str,
        today: &str,
        daily_grant: i64,
        amount: i64,
        updated_at: f64,
    ) -> Result<Option<UserTokenBalanceStatus>> {
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
            "SELECT token_balance, token_granted_total, token_used_total, last_token_grant_date \
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
                 SET token_balance = $1, token_granted_total = $2, last_token_grant_date = $3, updated_at = $4
                 WHERE user_id = $5",
                &[&balance, &granted_total, &last_grant_date, &updated_at, &cleaned],
            )?;
        } else if safe_daily_grant > 0 && last_grant_date.as_deref() == Some(today) {
            tx.execute(
                "UPDATE user_accounts
                 SET token_balance = $1, token_granted_total = $2, last_token_grant_date = $3, updated_at = $4
                 WHERE user_id = $5",
                &[&balance, &granted_total, &last_grant_date, &updated_at, &cleaned],
            )?;
        }
        tx.commit()?;
        Ok(Some(UserTokenBalanceStatus {
            balance,
            granted_total,
            used_total,
            daily_grant: safe_daily_grant,
            last_grant_date,
            allowed: balance > 0,
            overspent_tokens: 0,
        }))
    }
}
