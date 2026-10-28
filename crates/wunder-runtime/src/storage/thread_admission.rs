//! Durable accepted submissions, separate from completed execution history.
//! Bodies leave the queue after the rollout acknowledges their first commit.
use anyhow::{bail, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Apply the same admission budget to local and service storage before either
/// backend creates an owner or allocates a user round. Lease proofs authorize
/// this submission only and must never become conversation history.
pub(crate) fn prepare_input_json(input: &Value) -> Result<String> {
    let mut body = input.clone();
    let fields = body
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("accepted user input must be an object"))?;
    fields.remove("local_open_lease");
    fields.remove("inquiry_turn_id");
    if let Some(client) = fields
        .get("client_message_id")
        .filter(|value| !value.is_null())
    {
        if client.as_str().is_none_or(|value| value.len() > 256) {
            bail!("client message identity must be a string of at most 256 bytes");
        }
    }
    if let Some(round) = fields
        .get("root_user_round")
        .filter(|value| !value.is_null())
    {
        if round.as_i64().is_none_or(|round| round <= 0) {
            bail!("continuation requires a positive root user round");
        }
    }
    let content =
        wunder_core::thread_view::message_content_text(&body["content"]).unwrap_or_default();
    let attachments: Option<Vec<crate::schemas::AttachmentPayload>> = body
        .get("attachments")
        .filter(|value| !value.is_null())
        .cloned()
        .map(serde_json::from_value)
        .transpose()?;
    if crate::request_limits::measure_request_text_input_chars(&content, attachments.as_deref())
        > crate::orchestrator_constants::MAX_USER_INPUT_TEXT_CHARS
    {
        bail!("accepted user input exceeds text limit");
    }
    // The user item is committed with lifecycle records in one append. A body
    // admitted above the batch cap could never leave the queue successfully.
    // Leave enough space for both its envelope and the lifecycle records.
    let bytes = serde_json::to_vec(&body)?;
    if bytes.len()
        > super::rollout::ROLLOUT_MAX_RECORD_BYTES.min(super::rollout::ROLLOUT_MAX_APPEND_BYTES)
            - 64 * 1024
    {
        bail!("accepted user input exceeds byte limit");
    }
    Ok(String::from_utf8(bytes)?)
}

pub(crate) fn inquiry_turn_id(input: &Value) -> Result<Option<String>> {
    let Some(value) = input
        .get("inquiry_turn_id")
        .filter(|value| !value.is_null())
    else {
        return Ok(None);
    };
    let value = value
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("inquiry turn identity must be a string"))?
        .trim();
    if value.is_empty() || value.len() > 256 {
        bail!("invalid inquiry turn identity");
    }
    if input
        .get("root_user_round")
        .is_some_and(|value| !value.is_null())
    {
        bail!("inquiry answer must start an independent user round");
    }
    Ok(Some(value.to_owned()))
}

pub(crate) fn can_settle_inquiry(previous: &Value, payload: &Value) -> bool {
    if payload["stop_reason"].as_str() != Some("inquiry_answered") {
        return true;
    }
    previous["status"].as_str() == Some("waiting_input")
        && previous["answered_by_turn_id"].as_str().is_some()
        && previous["answered_by_turn_id"] == payload["answered_by_turn_id"]
}

pub(crate) fn inquiry_answer_control(turn: &str) -> Result<String> {
    if turn.trim().is_empty() || turn.len() > 256 {
        bail!("invalid inquiry answer turn identity");
    }
    Ok(serde_json::to_string(&json!({"answered_by_turn_id":turn}))?)
}

/// Queue control contains a small whitelist, never another copy of the input
/// body or an arbitrary runtime payload. Repeated updates preserve stop_reason.
pub(crate) fn bounded_control_json(
    previous: Option<&str>,
    status: &str,
    payload: &Value,
) -> Result<Option<String>> {
    let previous: Value = previous
        .map(serde_json::from_str)
        .transpose()?
        .unwrap_or(Value::Null);
    let mut control = serde_json::Map::new();
    if let Some(answer) = previous["answered_by_turn_id"].as_str() {
        control.insert(
            "answered_by_turn_id".into(),
            json!(bounded_text(answer, 256)),
        );
    }
    if status == "queued" {
        if let Some(queued) = payload
            .get("queued_input")
            .or_else(|| previous.get("queued_input"))
        {
            let mut bounded = serde_json::Map::new();
            if let Some(queue_id) = queued["queue_id"].as_str() {
                bounded.insert("queue_id".into(), json!(bounded_text(queue_id, 256)));
            }
            if let Some(reason) = queued["reason"].as_str() {
                bounded.insert("reason".into(), json!(bounded_text(reason, 1024)));
            }
            if let Some(priority) = queued["priority"].as_i64() {
                bounded.insert("priority".into(), json!(priority));
            }
            if !bounded.is_empty() {
                bounded.entry("queue_id").or_insert_with(|| json!(""));
                bounded.entry("reason").or_insert_with(|| json!(""));
                bounded.entry("priority").or_insert_with(|| json!(0));
                control.insert("queued_input".into(), Value::Object(bounded));
            }
        }
    }
    if let Some(reason) = payload["stop_reason"]
        .as_str()
        .or_else(|| previous["stop_reason"].as_str())
    {
        control.insert("stop_reason".into(), json!(bounded_text(reason, 1024)));
    }
    if control.is_empty() {
        Ok(None)
    } else {
        Ok(Some(serde_json::to_string(&control)?))
    }
}

#[derive(Clone)]
pub(crate) struct TurnAdmissions {
    root: PathBuf,
    pool: super::sqlite_pool::SqlitePool,
}

/// 孤儿受理结算宽限：非终态受理行必须比桌面打开租约 TTL（15s，见
/// wunder-desktop/native.rs LOCAL_THREAD_LEASE_TTL）更旧，才可能是
/// “受理已提交、续程随进程死亡”的残留。存活执行持有会话锁，且领取→执行
/// 交接会立即刷新受理行/任务行的更新时间（runtime.rs claim 后 update_turn），
/// 宽限把这些存活交接排除在结算窗口之外。
pub(crate) const ORPHAN_SETTLE_MARGIN_MS: i64 = 15_000;

impl TurnAdmissions {
    pub fn open(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root)?;
        let store = Self {
            root: root.to_owned(),
            pool: super::sqlite_pool::SqlitePool::new(root.join("queue_1.sqlite")),
        };
        let conn = store.connect()?;
        conn.execute_batch("PRAGMA journal_mode=WAL;
        CREATE TABLE IF NOT EXISTS canonical_thread_owners (
            thread_id TEXT PRIMARY KEY, user_id TEXT NOT NULL, latest_user_round INTEGER NOT NULL DEFAULT 0,
            mutation_pending TEXT
        );
        CREATE TABLE IF NOT EXISTS canonical_turn_admissions (
            thread_id TEXT NOT NULL, turn_id TEXT NOT NULL, root_turn_id TEXT NOT NULL,
            user_round INTEGER NOT NULL, client_message_id TEXT, status TEXT NOT NULL,
            input_json TEXT, control_json TEXT, created_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL,
            UNIQUE(thread_id,client_message_id), PRIMARY KEY(thread_id,turn_id)
        );
        CREATE INDEX IF NOT EXISTS canonical_thread_owner_user ON canonical_thread_owners(user_id,thread_id);
        CREATE INDEX IF NOT EXISTS canonical_admission_status ON canonical_turn_admissions(thread_id,status,user_round);
        CREATE INDEX IF NOT EXISTS canonical_admission_round ON canonical_turn_admissions(thread_id,user_round);")?;
        Ok(store)
    }

    fn connect(&self) -> Result<super::sqlite_pool::ConnectionLease> {
        self.pool.get()
    }

    fn canonical_thread_exists(&self, thread: &str) -> Result<bool> {
        let path = self.root.join("state_1.sqlite");
        if !path.try_exists()? {
            return Ok(false);
        }
        let conn = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(Duration::from_secs(2))?;
        let has_catalog: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='canonical_threads')",
            [],
            |row| row.get(0),
        )?;
        if !has_catalog {
            return Ok(false);
        }
        Ok(conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM canonical_threads WHERE thread_id=?1)",
            [thread],
            |row| row.get(0),
        )?)
    }

    fn check_owner_row(&self, conn: &Connection, user: &str, thread: &str) -> Result<()> {
        if user.trim().is_empty() || thread.trim().is_empty() {
            bail!("missing thread identity");
        }
        let owner: Option<String> = conn
            .query_row(
                "SELECT user_id FROM canonical_thread_owners WHERE thread_id=?1",
                [thread],
                |row| row.get(0),
            )
            .optional()?;
        match owner.as_deref() {
            Some(owner) if owner != user => bail!("thread owner mismatch"),
            None if self.canonical_thread_exists(thread)? => bail!("thread owner is missing"),
            _ => Ok(()),
        }
    }

    pub fn check_owner(&self, user: &str, thread: &str) -> Result<()> {
        let conn = self.connect()?;
        self.check_owner_row(&conn, user, thread)
    }

    /// Records thread ownership before a generator or runtime accepts work.
    /// An existing owner can never be silently replaced by another caller.
    /// The return value states whether this call created the owner row.
    pub fn register_owner(&self, user: &str, thread: &str) -> Result<bool> {
        if user.trim().is_empty() || thread.trim().is_empty() {
            bail!("missing thread identity");
        }
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        self.check_owner_row(&tx, user, thread)?;
        Self::check_mutation(&tx, thread)?;
        let created = tx.execute(
            "INSERT OR IGNORE INTO canonical_thread_owners(thread_id,user_id) VALUES(?1,?2)",
            params![thread, user],
        )? > 0;
        let owner: String = tx.query_row(
            "SELECT user_id FROM canonical_thread_owners WHERE thread_id=?1",
            [thread],
            |row| row.get(0),
        )?;
        if owner != user {
            bail!("thread owner mismatch");
        }
        tx.commit()?;
        Ok(created)
    }

    /// Removes an owner row created ahead of thread creation only when no
    /// admission has started using it. This is the safe compensation path for
    /// a failed create across the separate state and queue databases.
    pub fn remove_owner_if_empty(&self, user: &str, thread: &str) -> Result<bool> {
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let owner: Option<String> = tx
            .query_row(
                "SELECT user_id FROM canonical_thread_owners WHERE thread_id=?1",
                [thread],
                |row| row.get(0),
            )
            .optional()?;
        let Some(owner) = owner else {
            tx.commit()?;
            return Ok(false);
        };
        if owner != user {
            bail!("thread owner mismatch");
        }
        let has_admissions: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM canonical_turn_admissions WHERE thread_id=?1)",
            [thread],
            |row| row.get(0),
        )?;
        let removed = if has_admissions {
            false
        } else {
            tx.execute(
                "DELETE FROM canonical_thread_owners WHERE thread_id=?1 AND user_id=?2",
                params![thread, user],
            )? > 0
        };
        tx.commit()?;
        Ok(removed)
    }

    pub fn thread_ids_for_user(
        &self,
        user: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Vec<String>> {
        let conn = self.connect()?;
        let mut statement = conn.prepare(
            "SELECT thread_id FROM canonical_thread_owners
             WHERE user_id=?1 AND (?2 IS NULL OR thread_id>?2)
             ORDER BY thread_id LIMIT ?3",
        )?;
        let ids = statement
            .query_map(params![user, after, limit.clamp(1, 200) as i64], |row| {
                row.get(0)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(ids)
    }

    pub fn delete_thread(&self, thread: &str) -> Result<()> {
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "DELETE FROM canonical_turn_admissions WHERE thread_id=?1",
            [thread],
        )?;
        tx.execute(
            "DELETE FROM canonical_thread_owners WHERE thread_id=?1",
            [thread],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Keep ownership discoverable until all state and physical history has
    /// been reclaimed. A failed delete can then be retried by its owner.
    pub fn delete_admissions(&self, thread: &str) -> Result<()> {
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::check_mutation_kind(&tx, thread, "delete")?;
        tx.execute(
            "UPDATE canonical_thread_owners SET mutation_pending='delete' WHERE thread_id=?1",
            [thread],
        )?;
        tx.execute(
            "DELETE FROM canonical_turn_admissions WHERE thread_id=?1",
            [thread],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn check_mutation_kind(conn: &Connection, thread: &str, permitted: &str) -> Result<()> {
        let pending: Option<String> = conn
            .query_row(
                "SELECT mutation_pending FROM canonical_thread_owners WHERE thread_id=?1",
                [thread],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        if pending.as_deref().is_some_and(|kind| kind != permitted) {
            bail!("thread mutation is pending; retry after recovery");
        }
        Ok(())
    }

    pub(crate) fn ensure_mutable(&self, thread: &str) -> Result<()> {
        Self::check_mutation(&*self.connect()?, thread)
    }

    fn check_mutation(conn: &Connection, thread: &str) -> Result<()> {
        Self::check_mutation_kind(conn, thread, "")
    }

    /// Copies queue identity for a completed fork prefix. The copied rows
    /// deliberately have no input bodies: the JSONL rollout is authoritative
    /// for historical input and queue bodies only exist until first commit.
    pub fn copy_prefix(&self, source: &str, target: &str, through_turn: &str) -> Result<()> {
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let source_owner: Option<String> = tx
            .query_row(
                "SELECT user_id FROM canonical_thread_owners WHERE thread_id=?1",
                [source],
                |row| row.get(0),
            )
            .optional()?;
        let through: Option<i64> = tx
            .query_row(
                "SELECT user_round FROM canonical_turn_admissions WHERE thread_id=?1 AND turn_id=?2",
                params![source, through_turn],
                |row| row.get(0),
            )
            .optional()?;

        let owner =
            source_owner.ok_or_else(|| anyhow::anyhow!("source thread owner is missing"))?;
        let through = through
            .ok_or_else(|| anyhow::anyhow!("completed source turn is absent from admissions"))?;
        let target_owner: Option<String> = tx
            .query_row(
                "SELECT user_id FROM canonical_thread_owners WHERE thread_id=?1",
                [target],
                |row| row.get(0),
            )
            .optional()?;
        let target_has_admissions: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM canonical_turn_admissions WHERE thread_id=?1)",
            [target],
            |row| row.get(0),
        )?;
        if target_owner.is_some() || target_has_admissions {
            bail!("fork target already has admission state");
        }
        tx.execute(
            "INSERT INTO canonical_thread_owners(thread_id,user_id,latest_user_round) VALUES(?1,?2,?3)",
            params![target, owner, through],
        )?;
        let mut statement = tx.prepare(
            "SELECT turn_id,root_turn_id,user_round,client_message_id,status,created_at_ms,updated_at_ms
             FROM canonical_turn_admissions
             WHERE thread_id=?1 AND user_round<=?2
             ORDER BY user_round,created_at_ms",
        )?;
        let rows = statement.query_map(params![source, through], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })?;
        for row in rows {
            let (turn, root, round, client, status, created, updated) = row?;
            tx.execute(
                "INSERT INTO canonical_turn_admissions(thread_id,turn_id,root_turn_id,user_round,client_message_id,status,input_json,created_at_ms,updated_at_ms)
                 VALUES(?1,?2,?3,?4,?5,?6,NULL,?7,?8)",
                params![target, turn, root, round, client, status, created, updated],
            )?;
        }
        // The rows iterator is consumed by the loop; release its statement
        // before committing the owning transaction.
        drop(statement);
        tx.commit()?;
        Ok(())
    }

    /// Keep the control-plane prefix selected by a new physical revision.
    /// Canonical history remains in immutable rollout ranges, never copied.
    pub fn revert_prefix(&self, thread: &str, through_turn: &str) -> Result<()> {
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let round:Option<i64>=tx.query_row("SELECT user_round FROM canonical_turn_admissions WHERE thread_id=?1 AND turn_id=?2",params![thread,through_turn],|row|row.get(0)).optional()?;
        let round =
            round.ok_or_else(|| anyhow::anyhow!("revert turn is absent from admissions"))?;
        if tx.query_row("SELECT EXISTS(SELECT 1 FROM canonical_turn_admissions WHERE thread_id=?1 AND status='running')",[thread],|row|row.get::<_,bool>(0))? {
            bail!("cannot revert a thread with an active turn");
        }
        tx.execute(
            "DELETE FROM canonical_turn_admissions WHERE thread_id=?1 AND user_round>?2",
            params![thread, round],
        )?;
        tx.execute(
            "UPDATE canonical_thread_owners SET latest_user_round=?2 WHERE thread_id=?1",
            params![thread, round],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Freeze acceptance and lifecycle transitions in one queue transaction.
    /// A running execution wins the race and leaves the queue unchanged.
    pub fn freeze_revert(&self, thread: &str, through_turn: &str) -> Result<Option<i64>> {
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::check_mutation_kind(&tx, thread, "revert")?;
        let owner: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM canonical_thread_owners WHERE thread_id=?1)",
            [thread],
            |row| row.get(0),
        )?;
        if !owner {
            bail!("revert thread owner is absent");
        }
        let running: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM canonical_turn_admissions WHERE thread_id=?1 AND status='running')",
            [thread], |row| row.get(0),
        )?;
        if running {
            return Ok(None);
        }
        let round: i64 = tx.query_row(
            "SELECT user_round FROM canonical_turn_admissions WHERE thread_id=?1 AND turn_id=?2",
            params![thread, through_turn],
            |row| row.get(0),
        )?;
        tx.execute(
            "UPDATE canonical_thread_owners SET mutation_pending='revert' WHERE thread_id=?1 AND mutation_pending IS NULL",
            [thread],
        )?;
        tx.commit()?;
        Ok(Some(round))
    }

    /// Idempotent queue half of an already-published history revert.
    pub fn commit_revert(&self, thread: &str, through_turn: &str, round: i64) -> Result<()> {
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let frozen: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM canonical_thread_owners WHERE thread_id=?1 AND mutation_pending='revert')",
            [thread], |row| row.get(0),
        )?;
        if !frozen {
            bail!("revert queue is not frozen");
        }
        let actual: i64 = tx.query_row(
            "SELECT user_round FROM canonical_turn_admissions WHERE thread_id=?1 AND turn_id=?2",
            params![thread, through_turn],
            |row| row.get(0),
        )?;
        if actual != round {
            bail!("revert queue boundary changed");
        }
        tx.execute(
            "DELETE FROM canonical_turn_admissions WHERE thread_id=?1 AND user_round>?2",
            params![thread, round],
        )?;
        tx.execute(
            "UPDATE canonical_thread_owners SET latest_user_round=?2 WHERE thread_id=?1 AND latest_user_round<>?2",
            params![thread, round],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// The state intent is removed first. A queue-only marker means that its
    /// idempotent trim finished and this final acknowledgement can be retried.
    pub fn finish_revert(&self, thread: &str) -> Result<()> {
        let conn = self.connect()?;
        conn.execute(
            "UPDATE canonical_thread_owners SET mutation_pending=NULL WHERE thread_id=?1 AND mutation_pending='revert'",
            [thread],
        )?;
        Ok(())
    }

    pub fn revert_pending(&self, thread: &str) -> Result<bool> {
        Ok(self.connect()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM canonical_thread_owners WHERE thread_id=?1 AND mutation_pending='revert')",
            [thread], |row| row.get(0),
        )?)
    }

    pub fn accept(&self, user: &str, thread: &str, input: &Value) -> Result<Value> {
        if user.trim().is_empty() || thread.trim().is_empty() {
            bail!("missing thread identity");
        }
        let inquiry_turn_id = inquiry_turn_id(input)?;
        let body = prepare_input_json(input)?;
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        self.check_owner_row(&tx, user, thread)?;
        Self::check_mutation(&tx, thread)?;
        // 受理新输入前先结算本线程的崩溃孤儿（同一 Immediate 事务原子生效）：
        // 重放命中孤儿行时客户端收到的 created:false 对应一个已终态化的轮次。
        Self::settle_orphaned_turns_in_tx(&tx, thread, chrono::Utc::now().timestamp_millis())?;
        if let Some(lease) = input
            .get("local_open_lease")
            .filter(|value| !value.is_null())
        {
            let owner = lease
                .get("owner")
                .and_then(Value::as_str)
                .filter(|owner| !owner.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("missing execution lease owner"))?;
            let token = lease
                .get("token")
                .and_then(Value::as_u64)
                .ok_or_else(|| anyhow::anyhow!("missing execution lease token"))?;
            let valid = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM thread_open_leases
                 WHERE session_id=?1 AND user_id=?2 AND owner_instance_id=?3
                   AND fencing_token=?4
                   AND expires_at>(julianday('now')-2440587.5)*86400.0)",
                params![thread, user, owner, token],
                |row| row.get::<_, bool>(0),
            )?;
            if !valid {
                bail!("execution lease is expired or superseded");
            }
        }
        tx.execute(
            "INSERT OR IGNORE INTO canonical_thread_owners(thread_id,user_id) VALUES(?1,?2)",
            params![thread, user],
        )?;
        let owner: String = tx.query_row(
            "SELECT user_id FROM canonical_thread_owners WHERE thread_id=?1",
            [thread],
            |r| r.get(0),
        )?;
        if owner != user {
            bail!("thread owner mismatch");
        }
        let client = input
            .get("client_message_id")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty());
        if let Some(client) = client {
            let previous:Option<(String,String,i64)>=tx.query_row("SELECT turn_id,root_turn_id,user_round FROM canonical_turn_admissions WHERE thread_id=?1 AND client_message_id=?2",params![thread,client],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
            if let Some((turn, root, round)) = previous {
                return Ok(
                    json!({"turn_id":turn,"root_turn_id":root,"user_turn_index":round,"created":false}),
                );
            }
        }
        if let Some(inquiry_turn_id) = inquiry_turn_id.as_deref() {
            let previous_control: Option<String> = tx.query_row(
                "SELECT control_json FROM canonical_turn_admissions WHERE thread_id=?1 AND turn_id=?2",
                params![thread, inquiry_turn_id], |row| row.get(0),
            ).optional()?.flatten();
            if let Some(control) = previous_control {
                let control: Value = serde_json::from_str(&control)?;
                if let Some(answer) = control["answered_by_turn_id"].as_str() {
                    let existing: (String, i64) = tx.query_row(
                        "SELECT root_turn_id,user_round FROM canonical_turn_admissions WHERE thread_id=?1 AND turn_id=?2",
                        params![thread, answer], |row| Ok((row.get(0)?, row.get(1)?)),
                    )?;
                    return Ok(
                        json!({"turn_id":answer,"root_turn_id":existing.0,"user_turn_index":existing.1,"created":false}),
                    );
                }
            }
            let waiting: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM canonical_turn_admissions
                 WHERE thread_id=?1 AND turn_id=?2 AND status='waiting_input')",
                params![thread, inquiry_turn_id],
                |row| row.get(0),
            )?;
            if !waiting {
                bail!("inquiry is no longer waiting for input");
            }
        }
        let turn = uuid::Uuid::new_v4().to_string();
        let parent = input.get("root_user_round").and_then(Value::as_i64);
        let (root, round) = if let Some(round) = parent {
            let root:String=tx.query_row("SELECT root_turn_id FROM canonical_turn_admissions WHERE thread_id=?1 AND user_round=?2 ORDER BY created_at_ms LIMIT 1",params![thread,round],|r|r.get(0))?;
            (root, round)
        } else {
            tx.execute("UPDATE canonical_thread_owners SET latest_user_round=latest_user_round+1 WHERE thread_id=?1",[thread])?;
            let round: i64 = tx.query_row(
                "SELECT latest_user_round FROM canonical_thread_owners WHERE thread_id=?1",
                [thread],
                |r| r.get(0),
            )?;
            (turn.clone(), round)
        };
        let now = chrono::Utc::now().timestamp_millis();
        tx.execute("INSERT INTO canonical_turn_admissions(thread_id,turn_id,root_turn_id,user_round,client_message_id,status,input_json,created_at_ms,updated_at_ms) VALUES(?1,?2,?3,?4,?5,'accepted',?6,?7,?7)",params![thread,turn,root,round,client,body,now])?;
        if let Some(inquiry_turn_id) = inquiry_turn_id.as_deref() {
            let control = inquiry_answer_control(&turn)?;
            let updated = tx.execute(
                "UPDATE canonical_turn_admissions
                 SET control_json=?3,updated_at_ms=?4
                 WHERE thread_id=?1 AND turn_id=?2 AND status='waiting_input'",
                params![thread, inquiry_turn_id, control, now],
            )?;
            if updated != 1 {
                bail!("inquiry is no longer waiting for input");
            }
        }
        tx.commit()?;
        Ok(json!({"turn_id":turn,"root_turn_id":root,"user_turn_index":round,"created":true}))
    }

    /// 崩溃窗口孤儿结算：受理已提交但排队/执行续程随进程死亡的非终态受理，
    /// 在本线程无存活会话锁、无存活任务且超过宽限期后落 interrupted 终态。
    /// 在调用方事务内执行；session_locks/agent_tasks 与受理行同库
    ///（queue_1.sqlite），表尚不存在（队列 schema 未初始化）等价于无锁无任务。
    /// input_json 保留：已物化的用户气泡内容不随结算丢失。
    pub(crate) fn settle_orphaned_turns_in_tx(
        conn: &Connection,
        thread: &str,
        now_ms: i64,
    ) -> Result<usize> {
        if thread.trim().is_empty() {
            return Ok(0);
        }
        let now_s = now_ms as f64 / 1000.0;
        if Self::queue_table_exists(conn, "session_locks")?
            && conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM session_locks WHERE session_id=?1 AND expires_at>?2)",
                params![thread, now_s],
                |row| row.get::<_, bool>(0),
            )?
        {
            // 有存活会话锁的线程仍有执行在推进，不是孤儿。
            return Ok(0);
        }
        let stale_before_ms = now_ms - ORPHAN_SETTLE_MARGIN_MS;
        let task_fresh_s = now_s - ORPHAN_SETTLE_MARGIN_MS as f64 / 1000.0;
        let has_tasks = Self::queue_table_exists(conn, "agent_tasks")?;
        let candidates = {
            let mut stmt = conn.prepare(
                "SELECT turn_id,control_json FROM canonical_turn_admissions
                 WHERE thread_id=?1 AND status IN ('accepted','queued','running') AND updated_at_ms<?2",
            )?;
            let rows = stmt
                .query_map(params![thread, stale_before_ms], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        let mut settled = 0usize;
        for (turn, control) in candidates {
            if has_tasks {
                // agent_tasks.thread_id 形如 thread_<session>；受理与任务的链接是
                // request_payload.config_overrides.__thread_log_turn_id。
                // pending/retry 会被派发；刚刷新的 running 处于领取→执行交接窗口，视为存活。
                // 崩溃残留的 running 行更新时间停在崩溃时刻，宽限后不再挡住结算。
                let blocked: bool = conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM agent_tasks
                     WHERE thread_id=?1
                       AND json_extract(request_payload,'$.config_overrides.__thread_log_turn_id')=?2
                       AND (status IN ('pending','retry') OR (status='running' AND updated_at>?3)))",
                    params![format!("thread_{thread}"), turn, task_fresh_s],
                    |row| row.get(0),
                )?;
                if blocked {
                    continue;
                }
            }
            let control = bounded_control_json(
                control.as_deref(),
                "interrupted",
                &json!({"stop_reason":"process_restart"}),
            )?;
            settled += conn.execute(
                "UPDATE canonical_turn_admissions SET status='interrupted',control_json=?3,updated_at_ms=?4
                 WHERE thread_id=?1 AND turn_id=?2 AND status IN ('accepted','queued','running')",
                params![thread, turn, control, now_ms],
            )?;
        }
        Ok(settled)
    }

    /// 崩溃孤儿结算入口：单个 Immediate 事务。桌面打开线程成功路径与
    /// accept 事务内（见 accept）都会触发；重复调用幂等（终态行不再命中候选）。
    pub fn settle_orphaned_turns(&self, thread: &str) -> Result<usize> {
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let settled =
            Self::settle_orphaned_turns_in_tx(&tx, thread, chrono::Utc::now().timestamp_millis())?;
        tx.commit()?;
        Ok(settled)
    }

    fn queue_table_exists(conn: &Connection, name: &str) -> Result<bool> {
        Ok(conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
            [name],
            |row| row.get(0),
        )?)
    }

    /// Parking a fresh accepted submission is a state transition, not another
    /// chat body. Never overwrite an execution which already started running.
    pub fn park_queued(&self, user: &str, thread: &str, turn: &str) -> Result<bool> {
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::check_mutation(&tx, thread)?;
        let owner: Option<String> = tx
            .query_row(
                "SELECT user_id FROM canonical_thread_owners WHERE thread_id=?1",
                [thread],
                |row| row.get(0),
            )
            .optional()?;
        if owner.as_deref() != Some(user) {
            bail!("thread owner mismatch");
        }
        let changed = tx.execute(
            "UPDATE canonical_turn_admissions SET status='queued',updated_at_ms=?3
             WHERE thread_id=?1 AND turn_id=?2 AND status='accepted'",
            params![thread, turn, chrono::Utc::now().timestamp_millis()],
        )? > 0;
        tx.commit()?;
        Ok(changed)
    }

    pub fn pending(&self, thread: &str, turn: &str) -> Result<Option<Value>> {
        self.connect()?.query_row("SELECT input_json FROM canonical_turn_admissions WHERE thread_id=?1 AND turn_id=?2",params![thread,turn],|r|r.get::<_,Option<String>>(0)).optional()?.flatten().map(|s|serde_json::from_str(&s).map_err(Into::into)).transpose()
    }

    /// Lifecycle is monotonic: completed admissions are immutable and a stale
    /// queue notification cannot reclaim an execution that has started.
    pub(crate) fn can_transition(previous: &str, next: &str) -> bool {
        if !matches!(
            previous,
            "accepted" | "queued" | "running" | "waiting_input" | "waiting_approval"
        ) {
            return false;
        }
        match next {
            "accepted" => previous == "accepted",
            "queued" => matches!(previous, "accepted" | "queued"),
            "running" | "waiting_input" | "waiting_approval" => true,
            _ => Self::is_terminal(next),
        }
    }

    pub(crate) fn is_terminal(status: &str) -> bool {
        matches!(
            status,
            "completed" | "failed" | "cancelled" | "interrupted" | "rejected" | "aborted"
        )
    }

    pub fn update(&self, thread: &str, turn: &str, status: &str, committed: bool) -> Result<bool> {
        self.update_with_control(thread, turn, status, committed, &Value::Null)
    }

    /// Queue display metadata has its own small projection, never another copy
    /// of the submitted body or arbitrary runtime payload.
    pub fn update_with_control(
        &self,
        thread: &str,
        turn: &str,
        status: &str,
        committed: bool,
        payload: &Value,
    ) -> Result<bool> {
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::check_mutation(&tx, thread)?;
        let previous: Option<(String, Option<String>, bool)> = tx
            .query_row(
                "SELECT status,control_json,input_json IS NOT NULL FROM canonical_turn_admissions
                 WHERE thread_id=?1 AND turn_id=?2",
                params![thread, turn],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((previous, previous_control, has_input)) = previous else {
            return Ok(false);
        };
        let mut inquiry_state: Value = previous_control
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?
            .unwrap_or_else(|| json!({}));
        inquiry_state["status"] = json!(previous);
        if !can_settle_inquiry(&inquiry_state, payload) {
            return Ok(false);
        }
        if !Self::can_transition(&previous, status) {
            return Ok(false);
        }
        let control = bounded_control_json(previous_control.as_deref(), status, payload)?;
        let discard_input = committed;
        if previous == status && previous_control == control && !(discard_input && has_input) {
            return Ok(false);
        }
        tx.execute(
            "UPDATE canonical_turn_admissions SET status=?3,control_json=?4,updated_at_ms=?5,
             input_json=CASE WHEN ?6 THEN NULL ELSE input_json END
             WHERE thread_id=?1 AND turn_id=?2",
            params![
                thread,
                turn,
                status,
                control,
                chrono::Utc::now().timestamp_millis(),
                discard_input
            ],
        )?;
        tx.commit()?;
        Ok(true)
    }

    pub fn turn(&self, thread: &str, turn: &str) -> Result<Option<Value>> {
        let row = self.connect()?.query_row(
            "SELECT root_turn_id,user_round,status,client_message_id,created_at_ms,updated_at_ms,control_json
             FROM canonical_turn_admissions WHERE thread_id=?1 AND turn_id=?2",
            params![thread,turn],
            |row| Ok((json!({"turn_id":turn,"root_turn_id":row.get::<_,String>(0)?,
                "user_turn_index":row.get::<_,i64>(1)?,"status":row.get::<_,String>(2)?,
                "client_message_id":row.get::<_,Option<String>>(3)?,
                "created_time":row.get::<_,i64>(4)? as f64/1000.0,
                "updated_time":row.get::<_,i64>(5)? as f64/1000.0}),row.get::<_,Option<String>>(6)?)),
        ).optional()?;
        row.map(|(mut turn, control)| {
            if let Some(control) = control {
                let control: Value = serde_json::from_str(&control)?;
                for key in ["queued_input", "stop_reason", "answered_by_turn_id"] {
                    if let Some(value) = control.get(key) {
                        turn[key] = value.clone();
                    }
                }
            }
            Ok(turn)
        })
        .transpose()
    }

    pub fn latest_round(&self, thread: &str) -> Result<i64> {
        Ok(self
            .connect()?
            .query_row(
                "SELECT latest_user_round FROM canonical_thread_owners WHERE thread_id=?1",
                [thread],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0))
    }
    pub fn find_turn(&self, thread: &str, round: i64) -> Result<Option<String>> {
        Ok(self.connect()?.query_row("SELECT turn_id FROM canonical_turn_admissions WHERE thread_id=?1 AND user_round=?2 ORDER BY created_at_ms LIMIT 1",params![thread,round],|r|r.get(0)).optional()?)
    }
    pub fn ids_by_status(&self, thread: &str, status: &str, limit: usize) -> Result<Vec<String>> {
        let conn = self.connect()?;
        let mut stmt=conn.prepare("SELECT turn_id FROM canonical_turn_admissions WHERE thread_id=?1 AND status=?2 ORDER BY user_round DESC LIMIT ?3")?;
        let rows = stmt
            .query_map(params![thread, status, limit.clamp(1, 100) as i64], |r| {
                r.get(0)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}

/// Clamp by UTF-8 bytes without splitting a code point.
fn bounded_text(value: &str, limit: usize) -> &str {
    let mut end = value.len().min(limit);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

#[cfg(test)]
mod tests {
    #[test]
    fn inquiry_concurrent_clients_share_one_durable_consumption_identity() {
        let root = tempfile::tempdir().unwrap();
        let admissions = super::TurnAdmissions::open(root.path()).unwrap();
        let waiting = admissions
            .accept(
                "fixture-user",
                "fixture-thread",
                &serde_json::json!({"content":"fixture question"}),
            )
            .unwrap();
        let turn = waiting["turn_id"].as_str().unwrap().to_owned();
        admissions
            .update("fixture-thread", &turn, "waiting_input", false)
            .unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let clients = (0..2).map(|index| {
            let store = super::TurnAdmissions::open(root.path()).unwrap();
            let barrier = barrier.clone();
            let turn = turn.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.accept("fixture-user", "fixture-thread", &serde_json::json!({"content":"fixture answer", "client_message_id":format!("fixture-client-{index}"), "inquiry_turn_id":turn})).unwrap()
            })
        }).collect::<Vec<_>>();
        let answers = clients
            .into_iter()
            .map(|client| client.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(answers[0]["turn_id"], answers[1]["turn_id"]);
        assert_eq!(
            answers
                .iter()
                .filter(|answer| answer["created"] == true)
                .count(),
            1
        );
        assert_eq!(admissions.latest_round("fixture-thread").unwrap(), 2);
        assert!(!admissions.update_with_control("fixture-thread", &turn, "completed", false, &serde_json::json!({"stop_reason":"inquiry_answered", "answered_by_turn_id":"fixture-forged"})).unwrap());
    }

    #[test]
    fn inquiry_answer_is_consumed_once_across_distinct_clients_and_reopen() {
        let root = tempfile::tempdir().unwrap();
        let admissions = super::TurnAdmissions::open(root.path()).unwrap();
        let waiting = admissions
            .accept(
                "fixture-user",
                "fixture-thread",
                &serde_json::json!({"content":"fixture question"}),
            )
            .unwrap();
        let waiting_id = waiting["turn_id"].as_str().unwrap();
        admissions
            .update("fixture-thread", waiting_id, "waiting_input", false)
            .unwrap();
        let input = serde_json::json!({"content":"fixture answer", "client_message_id":"fixture-client-a", "inquiry_turn_id":waiting_id});
        let first = admissions
            .accept("fixture-user", "fixture-thread", &input)
            .unwrap();
        let reopened = super::TurnAdmissions::open(root.path()).unwrap();
        let replay = reopened.accept("fixture-user", "fixture-thread", &serde_json::json!({"content":"fixture answer", "client_message_id":"fixture-client-b", "inquiry_turn_id":waiting_id})).unwrap();
        assert_eq!(first["turn_id"], replay["turn_id"]);
        assert_eq!(replay["created"], false);
        assert_eq!(reopened.latest_round("fixture-thread").unwrap(), 2);
        assert_eq!(first["root_turn_id"], first["turn_id"]);
    }

    #[test]
    fn inquiry_answer_rejects_nonwaiting_and_other_thread_targets() {
        let root = tempfile::tempdir().unwrap();
        let admissions = super::TurnAdmissions::open(root.path()).unwrap();
        let accepted = admissions
            .accept(
                "fixture-user",
                "fixture-thread",
                &serde_json::json!({"content":"fixture input"}),
            )
            .unwrap();
        let turn = accepted["turn_id"].as_str().unwrap();
        for thread in ["fixture-thread", "fixture-other-thread"] {
            assert!(admissions
                .accept(
                    "fixture-user",
                    thread,
                    &serde_json::json!({"content":"fixture answer", "inquiry_turn_id":turn})
                )
                .is_err());
        }
        assert_eq!(admissions.latest_round("fixture-thread").unwrap(), 1);
    }

    #[test]
    fn shared_input_budget_covers_atomic_lifecycle_envelope() {
        use serde_json::json;
        let budget = super::super::rollout::ROLLOUT_MAX_APPEND_BYTES - 64 * 1024;
        assert!(super::prepare_input_json(
            &json!({"content":"question","metadata":"x".repeat(budget)})
        )
        .is_err());
        assert!(super::prepare_input_json(
            &json!({"content":"question","metadata":"x".repeat(budget-128)})
        )
        .is_ok());
        assert!(super::prepare_input_json(&json!("invalid")).is_err());
        for invalid in [json!(0), json!(-1), json!("1"), json!(1.5)] {
            assert!(super::prepare_input_json(
                &json!({"content":"question","root_user_round":invalid})
            )
            .is_err());
        }
        assert!(!super::TurnAdmissions::can_transition("running", "unknown"));
        assert!(!super::TurnAdmissions::can_transition("unknown", "running"));
        assert!(super::prepare_input_json(
            &json!({"content":"question","client_message_id":"x".repeat(256)})
        )
        .is_ok());
        assert!(super::prepare_input_json(
            &json!({"content":"question","client_message_id":"x".repeat(257)})
        )
        .is_err());
        assert!(
            super::prepare_input_json(&json!({"content":"question","client_message_id":1}))
                .is_err()
        );
    }

    use super::TurnAdmissions;

    #[test]
    fn absent_execution_lease_does_not_require_a_lease_table() {
        let root = tempfile::tempdir().unwrap();
        let admissions = TurnAdmissions::open(root.path()).unwrap();
        let accepted = admissions
            .accept(
                "fixture-user",
                "fixture-thread",
                &serde_json::json!({"content":"sample", "local_open_lease":null}),
            )
            .unwrap();
        let turn = accepted["turn_id"].as_str().unwrap();
        assert_eq!(
            admissions.pending("fixture-thread", turn).unwrap(),
            Some(serde_json::json!({"content":"sample"}))
        );
    }

    #[test]
    fn existing_ownerless_history_is_neither_readable_nor_claimable() {
        let root = tempfile::tempdir().unwrap();
        let admissions = TurnAdmissions::open(root.path()).unwrap();
        // An unmaterialized new identity remains a valid empty view/init path.
        admissions
            .check_owner("fixture-user", "fixture-new")
            .unwrap();
        admissions
            .register_owner("fixture-user", "fixture-owned")
            .unwrap();
        let state = rusqlite::Connection::open(root.path().join("state_1.sqlite")).unwrap();
        state.execute_batch("CREATE TABLE canonical_threads(thread_id TEXT PRIMARY KEY); INSERT INTO canonical_threads(thread_id) VALUES('fixture-ownerless'),('fixture-owned');").unwrap();
        admissions
            .check_owner("fixture-user", "fixture-owned")
            .unwrap();
        assert!(admissions
            .check_owner("fixture-other", "fixture-owned")
            .is_err());
        assert!(admissions
            .check_owner("fixture-user", "fixture-ownerless")
            .is_err());
        assert!(admissions
            .register_owner("fixture-user", "fixture-ownerless")
            .is_err());
        assert!(admissions
            .accept(
                "fixture-user",
                "fixture-ownerless",
                &serde_json::json!({"content":"sample"})
            )
            .is_err());
        assert_eq!(admissions.latest_round("fixture-ownerless").unwrap(), 0);
        assert!(admissions
            .accept(
                "fixture-user",
                "fixture-new",
                &serde_json::json!({"content":"sample"})
            )
            .is_ok());
    }

    #[test]
    fn owner_registration_can_be_compensated_only_before_admission() {
        let root = tempfile::tempdir().unwrap();
        let admissions = TurnAdmissions::open(root.path()).unwrap();
        assert!(admissions.register_owner("user-a", "thread-a").unwrap());
        assert!(!admissions.register_owner("user-a", "thread-a").unwrap());
        assert!(admissions
            .remove_owner_if_empty("user-a", "thread-a")
            .unwrap());
        assert!(!admissions
            .remove_owner_if_empty("user-a", "thread-a")
            .unwrap());

        admissions.register_owner("user-a", "thread-a").unwrap();
        admissions
            .accept(
                "user-a",
                "thread-a",
                &serde_json::json!({"content":"sample"}),
            )
            .unwrap();
        assert!(!admissions
            .remove_owner_if_empty("user-a", "thread-a")
            .unwrap());
    }

    #[test]
    fn queue_parking_preserves_input_and_never_overwrites_running_status() {
        let root = tempfile::tempdir().unwrap();
        let admissions = TurnAdmissions::open(root.path()).unwrap();
        let input = serde_json::json!({"content":"sample"});
        let accepted = admissions.accept("user-a", "thread-a", &input).unwrap();
        let turn = accepted["turn_id"].as_str().unwrap();
        assert!(admissions.park_queued("user-a", "thread-a", turn).unwrap());
        assert_eq!(admissions.pending("thread-a", turn).unwrap(), Some(input));
        assert!(!admissions.park_queued("user-a", "thread-a", turn).unwrap());
        admissions
            .update("thread-a", turn, "running", true)
            .unwrap();
        assert!(!admissions.park_queued("user-a", "thread-a", turn).unwrap());
        assert_eq!(
            admissions.turn("thread-a", turn).unwrap().unwrap()["status"],
            "running"
        );
        assert!(admissions.park_queued("user-b", "thread-a", turn).is_err());
    }

    #[test]
    fn lifecycle_control_is_bounded_and_never_reopens_finished_admissions() {
        use serde_json::json;
        let root = tempfile::tempdir().unwrap();
        let admissions = TurnAdmissions::open(root.path()).unwrap();
        let accepted = admissions
            .accept(
                "fixture-user",
                "fixture-thread",
                &json!({"content":"sample"}),
            )
            .unwrap();
        let turn = accepted["turn_id"].as_str().unwrap();
        let payload = json!({"queued_input":{"queue_id":"fixture-task","reason":"wait",
            "priority":1,"body":"must not be stored"},"other":"must not be stored"});
        assert!(admissions
            .update_with_control("fixture-thread", turn, "queued", false, &payload)
            .unwrap());
        assert!(!admissions
            .update_with_control("fixture-thread", turn, "queued", false, &payload)
            .unwrap());
        let mut changed = payload;
        changed["queued_input"]["reason"] = json!("界".repeat(1000));
        assert!(admissions
            .update_with_control("fixture-thread", turn, "queued", false, &changed)
            .unwrap());
        let queued = admissions.turn("fixture-thread", turn).unwrap().unwrap();
        assert!(queued["queued_input"]["body"].is_null());
        assert!(queued["queued_input"]["reason"].as_str().unwrap().len() <= 1024);
        assert!(queued["other"].is_null());
        assert!(admissions
            .pending("fixture-thread", turn)
            .unwrap()
            .is_some());
        assert!(admissions
            .update("fixture-thread", turn, "running", true)
            .unwrap());
        assert!(
            admissions.turn("fixture-thread", turn).unwrap().unwrap()["queued_input"].is_null()
        );
        assert!(!admissions
            .update_with_control("fixture-thread", turn, "queued", false, &changed)
            .unwrap());
        assert!(admissions
            .update_with_control(
                "fixture-thread",
                turn,
                "cancelled",
                true,
                &json!({"stop_reason":"fixture-stop"})
            )
            .unwrap());
        let finished = admissions.turn("fixture-thread", turn).unwrap().unwrap();
        assert_eq!(finished["status"], "cancelled");
        assert_eq!(finished["stop_reason"], "fixture-stop");
        for status in [
            "accepted",
            "queued",
            "running",
            "completed",
            "cancelled",
            "failed",
        ] {
            assert!(!admissions
                .update_with_control(
                    "fixture-thread",
                    turn,
                    status,
                    true,
                    &json!({"stop_reason":"stale"})
                )
                .unwrap());
            assert_eq!(
                admissions.turn("fixture-thread", turn).unwrap().unwrap(),
                finished
            );
        }
    }

    #[test]
    fn cancellation_without_rollout_preserves_pending_body_and_stays_terminal() {
        let root = tempfile::tempdir().unwrap();
        let admissions = TurnAdmissions::open(root.path()).unwrap();
        let accepted = admissions
            .accept(
                "fixture-user",
                "fixture-thread",
                &serde_json::json!({"content":"sample"}),
            )
            .unwrap();
        let turn = accepted["turn_id"].as_str().unwrap();
        assert!(admissions
            .update("fixture-thread", turn, "cancelled", false)
            .unwrap());
        assert!(admissions
            .pending("fixture-thread", turn)
            .unwrap()
            .is_some());
        assert!(!admissions
            .update("fixture-thread", turn, "running", true)
            .unwrap());
        assert_eq!(
            admissions.turn("fixture-thread", turn).unwrap().unwrap()["status"],
            "cancelled"
        );
    }

    #[test]
    fn oversized_admissions_leave_no_owner_or_round_state() {
        let root = tempfile::tempdir().unwrap();
        let admissions = TurnAdmissions::open(root.path()).unwrap();
        let text = serde_json::json!({"content":"x".repeat(crate::orchestrator_constants::MAX_USER_INPUT_TEXT_CHARS+1)});
        assert!(admissions
            .accept("fixture-user", "fixture-thread", &text)
            .is_err());
        assert_eq!(admissions.latest_round("fixture-thread").unwrap(), 0);
        let binary = serde_json::json!({"content":"sample","extra":"x".repeat(super::super::rollout::ROLLOUT_MAX_RECORD_BYTES)});
        assert!(admissions
            .accept("fixture-user", "fixture-thread", &binary)
            .is_err());
        assert_eq!(admissions.latest_round("fixture-thread").unwrap(), 0);
        assert!(admissions
            .register_owner("fixture-user", "fixture-thread")
            .unwrap());
    }

    #[test]
    fn fork_copy_requires_the_completed_turn_in_an_existing_queue() {
        let root = tempfile::tempdir().unwrap();
        let admissions = TurnAdmissions::open(root.path()).unwrap();
        admissions.register_owner("user-a", "source").unwrap();
        assert!(admissions
            .copy_prefix("source", "target", "turn-missing")
            .is_err());
        // The failed transaction must not leave a target owner behind.
        assert!(admissions.register_owner("user-b", "target").unwrap());
        assert_eq!(admissions.latest_round("target").unwrap(), 0);
    }

    #[test]
    fn fork_copy_keeps_identity_but_discards_committed_queue_body() {
        let root = tempfile::tempdir().unwrap();
        let admissions = TurnAdmissions::open(root.path()).unwrap();
        let accepted = admissions
            .accept(
                "user-a",
                "source",
                &serde_json::json!({"content":"sample", "client_message_id":"msg-a"}),
            )
            .unwrap();
        let turn = accepted["turn_id"].as_str().unwrap();
        admissions
            .update("source", turn, "completed", true)
            .unwrap();

        admissions.copy_prefix("source", "target", turn).unwrap();
        assert_eq!(admissions.latest_round("target").unwrap(), 1);
        assert_eq!(admissions.pending("target", turn).unwrap(), None);
        assert_eq!(
            admissions.turn("target", turn).unwrap().unwrap()["status"],
            "completed"
        );
    }

    fn queue_conn(root: &std::path::Path) -> rusqlite::Connection {
        rusqlite::Connection::open(root.join("queue_1.sqlite")).unwrap()
    }

    fn apply_queue_schema(root: &std::path::Path) {
        queue_conn(root)
            .execute_batch(include_str!("sqlite/schema_queue.sql"))
            .unwrap();
    }

    fn backdate_admission(root: &std::path::Path, thread: &str, turn: &str, age_ms: i64) {
        let backdated = chrono::Utc::now().timestamp_millis() - age_ms;
        queue_conn(root)
            .execute(
                "UPDATE canonical_turn_admissions SET updated_at_ms=?3 WHERE thread_id=?1 AND turn_id=?2",
                rusqlite::params![thread, turn, backdated],
            )
            .unwrap();
    }

    fn insert_session_lock(root: &std::path::Path, session: &str, ttl_s: f64) {
        apply_queue_schema(root);
        let now = chrono::Utc::now().timestamp_millis() as f64 / 1000.0;
        queue_conn(root)
            .execute(
                "INSERT OR REPLACE INTO session_locks(session_id,user_id,agent_id,lock_token,created_time,updated_time,expires_at)
                 VALUES(?1,'fixture-user','','',?2,?2,?3)",
                rusqlite::params![session, now, now + ttl_s],
            )
            .unwrap();
    }

    fn expire_session_locks(root: &std::path::Path) {
        let past = chrono::Utc::now().timestamp_millis() as f64 / 1000.0 - 3600.0;
        queue_conn(root)
            .execute(
                "UPDATE session_locks SET expires_at=?1",
                rusqlite::params![past],
            )
            .unwrap();
    }

    fn insert_agent_task(
        root: &std::path::Path,
        task_id: &str,
        session: &str,
        turn: &str,
        status: &str,
        age_s: f64,
    ) {
        apply_queue_schema(root);
        let now = chrono::Utc::now().timestamp_millis() as f64 / 1000.0;
        let payload =
            serde_json::json!({"config_overrides":{"__thread_log_turn_id":turn}}).to_string();
        queue_conn(root)
            .execute(
                "INSERT INTO agent_tasks(task_id,thread_id,user_id,session_id,status,request_payload,retry_at,created_at,updated_at)
                 VALUES(?1,?2,'fixture-user',?3,?4,?5,?6,?6,?7)",
                rusqlite::params![
                    task_id,
                    format!("thread_{session}"),
                    session,
                    status,
                    payload,
                    now,
                    now - age_s
                ],
            )
            .unwrap();
    }

    #[test]
    fn concurrent_duplicate_accepts_share_one_round_and_one_row() {
        let root = tempfile::tempdir().unwrap();
        // 两个实例共享同一 queue_1.sqlite，等价两个本地进程并发受理同键消息。
        let first = TurnAdmissions::open(root.path()).unwrap();
        let second = TurnAdmissions::open(root.path()).unwrap();
        let input = serde_json::json!({"content":"sample","client_message_id":"msg-dup"});
        let mut handles = Vec::new();
        for admissions in [first.clone(), second.clone()] {
            let input = input.clone();
            handles.push(std::thread::spawn(move || {
                (0..8)
                    .map(|_| {
                        admissions
                            .accept("fixture-user", "fixture-thread", &input)
                            .unwrap()
                    })
                    .collect::<Vec<_>>()
            }));
        }
        let results: Vec<_> = handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap())
            .collect();
        assert_eq!(results.len(), 16);
        assert_eq!(
            results
                .iter()
                .filter(|accepted| accepted["created"] == true)
                .count(),
            1,
            "同键并发受理只创建一次"
        );
        let turn = results[0]["turn_id"].as_str().unwrap();
        assert!(results
            .iter()
            .all(|accepted| accepted["turn_id"] == turn && accepted["user_turn_index"] == 1));
        let store = TurnAdmissions::open(root.path()).unwrap();
        assert_eq!(store.latest_round("fixture-thread").unwrap(), 1);
        let rows: i64 = queue_conn(root.path())
            .query_row(
                "SELECT COUNT(*) FROM canonical_turn_admissions WHERE thread_id='fixture-thread'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(rows, 1);
    }

    #[test]
    fn replay_after_lost_response_returns_first_identity_without_new_round() {
        let root = tempfile::tempdir().unwrap();
        let admissions = TurnAdmissions::open(root.path()).unwrap();
        let input = serde_json::json!({"content":"sample","client_message_id":"msg-retry"});
        let accepted = admissions
            .accept("fixture-user", "fixture-thread", &input)
            .unwrap();
        assert_eq!(accepted["created"], true);
        // 客户端没收到受理结论（响应丢失）后重发同键。
        let replay = admissions
            .accept("fixture-user", "fixture-thread", &input)
            .unwrap();
        assert_eq!(replay["created"], false);
        assert_eq!(replay["turn_id"], accepted["turn_id"]);
        assert_eq!(replay["user_turn_index"], 1);
        assert_eq!(admissions.latest_round("fixture-thread").unwrap(), 1);
        let turn = accepted["turn_id"].as_str().unwrap();
        assert_eq!(
            admissions.pending("fixture-thread", turn).unwrap(),
            Some(serde_json::json!({"content":"sample","client_message_id":"msg-retry"}))
        );
    }

    #[test]
    fn same_key_with_altered_content_keeps_the_first_write() {
        let root = tempfile::tempdir().unwrap();
        let admissions = TurnAdmissions::open(root.path()).unwrap();
        let first = serde_json::json!({"content":"first","client_message_id":"msg-alter"});
        let accepted = admissions
            .accept("fixture-user", "fixture-thread", &first)
            .unwrap();
        let altered = serde_json::json!({"content":"altered","client_message_id":"msg-alter"});
        let replay = admissions
            .accept("fixture-user", "fixture-thread", &altered)
            .unwrap();
        assert_eq!(replay["created"], false);
        let turn = accepted["turn_id"].as_str().unwrap();
        // 先写者胜且可观察：存储正文不被替换。
        assert_eq!(
            admissions.pending("fixture-thread", turn).unwrap(),
            Some(first)
        );
        assert_eq!(admissions.latest_round("fixture-thread").unwrap(), 1);
    }

    #[test]
    fn restart_settles_crash_window_orphan_and_keeps_dedup_identity() {
        let root = tempfile::tempdir().unwrap();
        let admissions = TurnAdmissions::open(root.path()).unwrap();
        let input = serde_json::json!({"content":"sample","client_message_id":"msg-orphan"});
        let accepted = admissions
            .accept("fixture-user", "fixture-thread", &input)
            .unwrap();
        let turn = accepted["turn_id"].as_str().unwrap().to_string();
        // 崩溃窗口：受理已提交但进程在排队/执行前死亡。受理行时间停在崩溃时刻。
        backdate_admission(root.path(), "fixture-thread", &turn, 60_000);
        // 重启：新实例重开同一数据目录后结算。
        let reopened = TurnAdmissions::open(root.path()).unwrap();
        assert_eq!(reopened.settle_orphaned_turns("fixture-thread").unwrap(), 1);
        let settled = reopened.turn("fixture-thread", &turn).unwrap().unwrap();
        assert_eq!(settled["status"], "interrupted");
        assert_eq!(settled["stop_reason"], "process_restart");
        // input_json 保留：已物化的用户气泡内容不随结算丢失。
        assert_eq!(
            reopened.pending("fixture-thread", &turn).unwrap(),
            Some(input.clone())
        );
        // 重复结算幂等。
        assert_eq!(reopened.settle_orphaned_turns("fixture-thread").unwrap(), 0);
        // 宽限期保护：新鲜的非终态受理不被结算。
        let fresh = reopened
            .accept(
                "fixture-user",
                "fixture-thread",
                &serde_json::json!({"content":"later","client_message_id":"msg-fresh"}),
            )
            .unwrap();
        let fresh_turn = fresh["turn_id"].as_str().unwrap().to_string();
        assert_eq!(reopened.settle_orphaned_turns("fixture-thread").unwrap(), 0);
        assert_eq!(
            reopened
                .turn("fixture-thread", &fresh_turn)
                .unwrap()
                .unwrap()["status"],
            "accepted"
        );
        // 轮次分配不因结算改变。
        assert_eq!(reopened.latest_round("fixture-thread").unwrap(), 2);
        // 响应丢失后的同键重发仍命中去重，返回已终态化的首轮身份。
        let replay = reopened
            .accept("fixture-user", "fixture-thread", &input)
            .unwrap();
        assert_eq!(replay["created"], false);
        assert_eq!(replay["turn_id"].as_str().unwrap(), turn);
        assert_eq!(reopened.latest_round("fixture-thread").unwrap(), 2);
    }

    #[test]
    fn settle_respects_live_lock_and_live_or_unrelated_tasks() {
        let root = tempfile::tempdir().unwrap();
        let admissions = TurnAdmissions::open(root.path()).unwrap();
        let accepted = admissions
            .accept(
                "fixture-user",
                "fixture-thread",
                &serde_json::json!({"content":"sample","client_message_id":"msg-guard"}),
            )
            .unwrap();
        let turn = accepted["turn_id"].as_str().unwrap().to_string();
        backdate_admission(root.path(), "fixture-thread", &turn, 60_000);
        // 存活会话锁：执行仍在推进，不结算。
        insert_session_lock(root.path(), "fixture-thread", 120.0);
        assert_eq!(
            admissions.settle_orphaned_turns("fixture-thread").unwrap(),
            0
        );
        assert_eq!(
            admissions.turn("fixture-thread", &turn).unwrap().unwrap()["status"],
            "accepted"
        );
        // 锁行过期（崩溃后心跳停止）后结算生效。
        expire_session_locks(root.path());
        // 但一个 pending 任务仍会驱动该轮，挡住结算。
        insert_agent_task(
            root.path(),
            "task-guard-1",
            "fixture-thread",
            &turn,
            "pending",
            60.0,
        );
        assert_eq!(
            admissions.settle_orphaned_turns("fixture-thread").unwrap(),
            0
        );
        // 刚领取的 running 任务（领取→执行交接窗口）同样挡住。
        queue_conn(root.path())
            .execute(
                "UPDATE agent_tasks SET status='running', updated_at=?1",
                rusqlite::params![chrono::Utc::now().timestamp_millis() as f64 / 1000.0],
            )
            .unwrap();
        assert_eq!(
            admissions.settle_orphaned_turns("fixture-thread").unwrap(),
            0
        );
        // 崩溃残留的 running 行（更新时间停在宽限期之前）不再挡住。
        queue_conn(root.path())
            .execute(
                "UPDATE agent_tasks SET updated_at=?1",
                rusqlite::params![chrono::Utc::now().timestamp_millis() as f64 / 1000.0 - 60.0],
            )
            .unwrap();
        assert_eq!(
            admissions.settle_orphaned_turns("fixture-thread").unwrap(),
            1
        );
        assert_eq!(
            admissions.turn("fixture-thread", &turn).unwrap().unwrap()["status"],
            "interrupted"
        );
        // 链接到其他轮次的任务不影响本轮之外的结算语义：新孤儿照常结算。
        let other = admissions
            .accept(
                "fixture-user",
                "fixture-thread",
                &serde_json::json!({"content":"later","client_message_id":"msg-other"}),
            )
            .unwrap();
        let other_turn = other["turn_id"].as_str().unwrap().to_string();
        backdate_admission(root.path(), "fixture-thread", &other_turn, 60_000);
        insert_agent_task(
            root.path(),
            "task-guard-2",
            "fixture-thread",
            &turn,
            "pending",
            0.0,
        );
        assert_eq!(
            admissions.settle_orphaned_turns("fixture-thread").unwrap(),
            1
        );
        assert_eq!(
            admissions
                .turn("fixture-thread", &other_turn)
                .unwrap()
                .unwrap()["status"],
            "interrupted"
        );
        // 终态行永远不会被结算改动。
        backdate_admission(root.path(), "fixture-thread", &turn, 120_000);
        let terminal_before = admissions.turn("fixture-thread", &turn).unwrap().unwrap();
        assert_eq!(
            admissions.settle_orphaned_turns("fixture-thread").unwrap(),
            0
        );
        assert_eq!(
            admissions.turn("fixture-thread", &turn).unwrap().unwrap(),
            terminal_before
        );
    }

    #[test]
    fn settle_inside_accept_is_atomic_with_dedup() {
        let root = tempfile::tempdir().unwrap();
        let admissions = TurnAdmissions::open(root.path()).unwrap();
        let input = serde_json::json!({"content":"sample","client_message_id":"msg-atomic"});
        let accepted = admissions
            .accept("fixture-user", "fixture-thread", &input)
            .unwrap();
        let turn = accepted["turn_id"].as_str().unwrap().to_string();
        backdate_admission(root.path(), "fixture-thread", &turn, 60_000);
        // accept 事务内先结算孤儿再查重：新键受理成功后，孤儿已是 interrupted，
        // 同键重放命中的是终态行。
        let next = admissions
            .accept(
                "fixture-user",
                "fixture-thread",
                &serde_json::json!({"content":"next","client_message_id":"msg-next"}),
            )
            .unwrap();
        assert_eq!(next["created"], true);
        assert_eq!(next["user_turn_index"], 2);
        assert_eq!(
            admissions.turn("fixture-thread", &turn).unwrap().unwrap()["status"],
            "interrupted"
        );
        let replay = admissions
            .accept("fixture-user", "fixture-thread", &input)
            .unwrap();
        assert_eq!(replay["created"], false);
        assert_eq!(replay["user_turn_index"], 1);
    }
}
