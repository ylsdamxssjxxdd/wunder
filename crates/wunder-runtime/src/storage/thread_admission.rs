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
        let body = prepare_input_json(input)?;
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        self.check_owner_row(&tx, user, thread)?;
        Self::check_mutation(&tx, thread)?;
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
        tx.commit()?;
        Ok(json!({"turn_id":turn,"root_turn_id":root,"user_turn_index":round,"created":true}))
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
                for key in ["queued_input", "stop_reason"] {
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
}
