use super::SqliteStorage;
use crate::storage::{StorageLifecycle, UserTokenBalanceStatus};
use anyhow::Result;
use rusqlite::{params, OptionalExtension, TransactionBehavior};

pub(super) trait SqliteTokenBalanceStorage {
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

impl SqliteTokenBalanceStorage for SqliteStorage {
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
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row = tx
            .query_row(
                "SELECT token_balance, token_granted_total, token_used_total, last_token_grant_date FROM user_accounts WHERE user_id = ?",
                params![cleaned],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?;
        let Some((raw_balance, raw_granted_total, raw_used_total, raw_last_grant_date)) = row
        else {
            tx.commit()?;
            return Ok(None);
        };
        let mut balance = raw_balance.max(0);
        let mut granted_total = raw_granted_total.max(0);
        let used_total = raw_used_total.max(0);
        let mut last_grant_date = raw_last_grant_date;
        let safe_daily_grant = daily_grant.max(0);
        let should_grant = safe_daily_grant > 0 && last_grant_date.as_deref() != Some(today);
        if should_grant {
            balance = balance.saturating_add(safe_daily_grant);
            granted_total = granted_total.saturating_add(safe_daily_grant);
            last_grant_date = Some(today.to_string());
            tx.execute(
                "UPDATE user_accounts
                 SET token_balance = ?, token_granted_total = ?, last_token_grant_date = ?, updated_at = ?
                 WHERE user_id = ?",
                params![balance, granted_total, last_grant_date, Self::now_ts(), cleaned],
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
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row = tx
            .query_row(
                "SELECT token_balance, token_granted_total, token_used_total, last_token_grant_date FROM user_accounts WHERE user_id = ?",
                params![cleaned],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?;
        let Some((raw_balance, raw_granted_total, raw_used_total, raw_last_grant_date)) = row
        else {
            tx.commit()?;
            return Ok(None);
        };
        let safe_daily_grant = daily_grant.max(0);
        let mut balance = raw_balance.max(0);
        let mut granted_total = raw_granted_total.max(0);
        let mut used_total = raw_used_total.max(0);
        let mut last_grant_date = raw_last_grant_date;
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
             SET token_balance = ?, token_granted_total = ?, token_used_total = ?, last_token_grant_date = ?, updated_at = ?
             WHERE user_id = ?",
            params![
                balance,
                granted_total,
                used_total,
                last_grant_date,
                Self::now_ts(),
                cleaned
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
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row = tx
            .query_row(
                "SELECT token_balance, token_granted_total, token_used_total, last_token_grant_date FROM user_accounts WHERE user_id = ?",
                params![cleaned],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?;
        let Some((raw_balance, raw_granted_total, raw_used_total, raw_last_grant_date)) = row
        else {
            tx.commit()?;
            return Ok(None);
        };
        let safe_daily_grant = daily_grant.max(0);
        let mut balance = raw_balance.max(0);
        let mut granted_total = raw_granted_total.max(0);
        let used_total = raw_used_total.max(0);
        let mut last_grant_date = raw_last_grant_date;
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
                 SET token_balance = ?, token_granted_total = ?, last_token_grant_date = ?, updated_at = ?
                 WHERE user_id = ?",
                params![balance, granted_total, last_grant_date, updated_at, cleaned],
            )?;
        } else if safe_daily_grant > 0 && last_grant_date.as_deref() == Some(today) {
            tx.execute(
                "UPDATE user_accounts
                 SET token_balance = ?, token_granted_total = ?, last_token_grant_date = ?, updated_at = ?
                 WHERE user_id = ?",
                params![balance, granted_total, last_grant_date, updated_at, cleaned],
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
