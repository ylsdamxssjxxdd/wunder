use super::SqliteStorage;
use anyhow::Result;
use chrono::Local;
use rusqlite::{params, Connection};
use std::collections::HashSet;
use std::sync::atomic::Ordering;

pub(super) trait SqliteSchemaStorage {
    fn ensure_initialized_impl(&self) -> Result<()>;
}

impl SqliteStorage {
    fn ensure_user_account_quota_columns(&self, conn: &Connection) -> Result<()> {
        let columns = load_table_columns(conn, "user_accounts")?;
        if columns.is_empty() {
            return Ok(());
        }
        let has_legacy_daily_quota = columns.contains("daily_quota");
        let has_legacy_daily_quota_used = columns.contains("daily_quota_used");
        let has_legacy_daily_quota_date = columns.contains("daily_quota_date");
        if !columns.contains("token_balance") {
            conn.execute(
                "ALTER TABLE user_accounts ADD COLUMN token_balance INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        if !columns.contains("token_granted_total") {
            conn.execute(
                "ALTER TABLE user_accounts ADD COLUMN token_granted_total INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        if !columns.contains("token_used_total") {
            conn.execute(
                "ALTER TABLE user_accounts ADD COLUMN token_used_total INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        if !columns.contains("last_token_grant_date") {
            conn.execute(
                "ALTER TABLE user_accounts ADD COLUMN last_token_grant_date TEXT",
                [],
            )?;
        }
        if !(has_legacy_daily_quota && has_legacy_daily_quota_used && has_legacy_daily_quota_date) {
            return Ok(());
        }
        let today = Local::now().format("%Y-%m-%d").to_string();
        let today_ref = today.as_str();
        conn.execute(
            "UPDATE user_accounts
             SET token_balance = CASE
                     WHEN COALESCE(token_balance, 0) > 0 THEN token_balance
                     WHEN COALESCE(daily_quota_date, '') = ? THEN MAX(COALESCE(daily_quota, 0) - COALESCE(daily_quota_used, 0), 0)
                     ELSE MAX(COALESCE(daily_quota, 0), 0)
                 END,
                 token_granted_total = CASE
                     WHEN COALESCE(token_granted_total, 0) > 0 THEN token_granted_total
                     ELSE MAX(COALESCE(daily_quota, 0), 0)
                 END,
                 token_used_total = CASE
                     WHEN COALESCE(token_used_total, 0) > 0 THEN token_used_total
                     WHEN COALESCE(daily_quota_date, '') = ? THEN MAX(COALESCE(daily_quota_used, 0), 0)
                     ELSE 0
                 END,
                 last_token_grant_date = COALESCE(last_token_grant_date, daily_quota_date)
             WHERE token_balance = 0
                OR token_granted_total = 0
                OR token_used_total = 0
                OR last_token_grant_date IS NULL",
            params![today_ref, today_ref],
        )?;
        Ok(())
    }

    fn ensure_user_account_level_columns(&self, conn: &Connection) -> Result<()> {
        let columns = load_table_columns(conn, "user_accounts")?;
        if columns.is_empty() {
            return Ok(());
        }
        if !columns.contains("experience_total") {
            conn.execute(
                "ALTER TABLE user_accounts ADD COLUMN experience_total INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        Ok(())
    }

    fn ensure_user_account_unit_columns(&self, _conn: &Connection) -> Result<()> {
        Ok(())
    }

    fn ensure_user_account_list_indexes(&self, _conn: &Connection) -> Result<()> {
        Ok(())
    }

    fn ensure_user_token_columns(&self, conn: &Connection) -> Result<()> {
        let columns = load_table_columns(conn, "user_tokens")?;
        if columns.is_empty() {
            return Ok(());
        }
        if !columns.contains("session_scope") {
            conn.execute(
                "ALTER TABLE user_tokens ADD COLUMN session_scope TEXT NOT NULL DEFAULT 'default'",
                [],
            )?;
        }
        conn.execute(
            "UPDATE user_tokens SET session_scope = 'default' WHERE session_scope IS NULL OR trim(session_scope) = ''",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_user_tokens_user_scope_created ON user_tokens (user_id, session_scope, created_at)",
            [],
        )?;
        Ok(())
    }

    fn ensure_user_tool_access_columns(&self, _conn: &Connection) -> Result<()> {
        Ok(())
    }

    fn ensure_chat_session_columns(&self, conn: &Connection) -> Result<()> {
        let columns = load_table_columns(conn, "chat_sessions")?;
        if !columns.contains("status") {
            conn.execute("ALTER TABLE chat_sessions ADD COLUMN status TEXT", [])?;
        }
        Ok(())
    }

    fn ensure_stream_event_workflow_columns(&self, conn: &Connection) -> Result<()> {
        let columns = load_table_columns(conn, "stream_events")?;
        if columns.is_empty() {
            return Ok(());
        }
        let needs_backfill = !columns.contains("event_type") || !columns.contains("user_round");
        if !columns.contains("event_type") {
            conn.execute("ALTER TABLE stream_events ADD COLUMN event_type TEXT", [])?;
        }
        if !columns.contains("user_round") {
            conn.execute(
                "ALTER TABLE stream_events ADD COLUMN user_round INTEGER",
                [],
            )?;
        }
        // Existing event rows predate the indexed workflow fields. Populate them once so
        // history hydration stays bounded even for sessions created before this upgrade.
        if needs_backfill {
            conn.execute(
                "UPDATE stream_events
             SET event_type = LOWER(TRIM(COALESCE(
                     json_extract(payload, '$.event'),
                     json_extract(payload, '$.event_type'),
                     json_extract(payload, '$.type'),
                     ''
                 ))),
                 user_round = CASE
                     WHEN CAST(COALESCE(
                         json_extract(payload, '$.data.data.user_round'),
                         json_extract(payload, '$.data.data.userRound'),
                         json_extract(payload, '$.data.data.round'),
                         json_extract(payload, '$.data.user_round'),
                         json_extract(payload, '$.data.userRound'),
                         json_extract(payload, '$.data.round'),
                         json_extract(payload, '$.user_round'),
                         json_extract(payload, '$.userRound'),
                         json_extract(payload, '$.round')
                     ) AS INTEGER) > 0
                     THEN CAST(COALESCE(
                         json_extract(payload, '$.data.data.user_round'),
                         json_extract(payload, '$.data.data.userRound'),
                         json_extract(payload, '$.data.data.round'),
                         json_extract(payload, '$.data.user_round'),
                         json_extract(payload, '$.data.userRound'),
                         json_extract(payload, '$.data.round'),
                         json_extract(payload, '$.user_round'),
                         json_extract(payload, '$.userRound'),
                         json_extract(payload, '$.round')
                     ) AS INTEGER)
                     ELSE NULL
                 END
             WHERE json_valid(payload) AND (event_type IS NULL OR user_round IS NULL)",
                [],
            )?;
        }
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_stream_events_session_round ON stream_events (session_id, user_round, event_id)",
            [],
        )?;
        Ok(())
    }

    fn ensure_channel_columns(&self, _conn: &Connection) -> Result<()> {
        Ok(())
    }

    fn ensure_session_lock_columns(&self, _conn: &Connection) -> Result<()> {
        Ok(())
    }

    fn ensure_session_run_columns(&self, conn: &Connection) -> Result<()> {
        let columns = load_table_columns(conn, "session_runs")?;
        if columns.is_empty() {
            return Ok(());
        }
        if !columns.contains("dispatch_id") {
            conn.execute("ALTER TABLE session_runs ADD COLUMN dispatch_id TEXT", [])?;
        }
        if !columns.contains("run_kind") {
            conn.execute("ALTER TABLE session_runs ADD COLUMN run_kind TEXT", [])?;
        }
        if !columns.contains("requested_by") {
            conn.execute("ALTER TABLE session_runs ADD COLUMN requested_by TEXT", [])?;
        }
        if !columns.contains("metadata") {
            conn.execute("ALTER TABLE session_runs ADD COLUMN metadata TEXT", [])?;
        }
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_session_runs_dispatch \
             ON session_runs (user_id, dispatch_id, updated_time)",
            [],
        );
        Ok(())
    }

    fn ensure_user_agent_columns(&self, conn: &Connection) -> Result<()> {
        let columns = load_table_columns(conn, "user_agents")?;
        if !columns.contains("model_name") {
            conn.execute("ALTER TABLE user_agents ADD COLUMN model_name TEXT", [])?;
        }
        if !columns.contains("preset_questions") {
            conn.execute(
                "ALTER TABLE user_agents ADD COLUMN preset_questions TEXT",
                [],
            )?;
        }
        if !columns.contains("declared_tool_names") {
            conn.execute(
                "ALTER TABLE user_agents ADD COLUMN declared_tool_names TEXT",
                [],
            )?;
        }
        if !columns.contains("declared_skill_names") {
            conn.execute(
                "ALTER TABLE user_agents ADD COLUMN declared_skill_names TEXT",
                [],
            )?;
        }
        if !columns.contains("visible_unit_ids") {
            conn.execute(
                "ALTER TABLE user_agents ADD COLUMN visible_unit_ids TEXT",
                [],
            )?;
        }
        if !columns.contains("ability_items") {
            conn.execute("ALTER TABLE user_agents ADD COLUMN ability_items TEXT", [])?;
        }
        if !columns.contains("preset_binding") {
            conn.execute("ALTER TABLE user_agents ADD COLUMN preset_binding TEXT", [])?;
        }
        if !columns.contains("silent") {
            conn.execute(
                "ALTER TABLE user_agents ADD COLUMN silent INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        if !columns.contains("prefer_mother") {
            conn.execute(
                "ALTER TABLE user_agents ADD COLUMN prefer_mother INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        if !columns.contains("preview_skill") {
            conn.execute(
                "ALTER TABLE user_agents ADD COLUMN preview_skill INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        Ok(())
    }

    fn ensure_team_run_columns(&self, conn: &Connection) -> Result<()> {
        let columns = load_table_columns(conn, "team_runs")?;
        if !columns.contains("mother_agent_id") {
            conn.execute("ALTER TABLE team_runs ADD COLUMN mother_agent_id TEXT", [])?;
        }
        Ok(())
    }

    fn ensure_team_task_columns(&self, conn: &Connection) -> Result<()> {
        let columns = load_table_columns(conn, "team_tasks")?;
        if !columns.contains("session_run_id") {
            conn.execute("ALTER TABLE team_tasks ADD COLUMN session_run_id TEXT", [])?;
        }
        Ok(())
    }

    fn ensure_user_world_group_columns(&self, _conn: &Connection) -> Result<()> {
        Ok(())
    }

    fn ensure_cron_columns(&self, conn: &Connection) -> Result<()> {
        let columns = load_table_columns(conn, "cron_jobs")?;
        if !columns.contains("consecutive_failures") {
            conn.execute(
                "ALTER TABLE cron_jobs ADD COLUMN consecutive_failures INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        if !columns.contains("auto_disabled_reason") {
            conn.execute(
                "ALTER TABLE cron_jobs ADD COLUMN auto_disabled_reason TEXT",
                [],
            )?;
        }
        if !columns.contains("runner_id") {
            conn.execute("ALTER TABLE cron_jobs ADD COLUMN runner_id TEXT", [])?;
        }
        if !columns.contains("run_token") {
            conn.execute("ALTER TABLE cron_jobs ADD COLUMN run_token TEXT", [])?;
        }
        if !columns.contains("heartbeat_at") {
            conn.execute("ALTER TABLE cron_jobs ADD COLUMN heartbeat_at REAL", [])?;
        }
        if !columns.contains("lease_expires_at") {
            conn.execute("ALTER TABLE cron_jobs ADD COLUMN lease_expires_at REAL", [])?;
        }
        Ok(())
    }

    fn ensure_memory_fragment_columns(&self, conn: &Connection) -> Result<()> {
        let columns = load_table_columns(conn, "memory_fragments")?;
        if columns.is_empty() {
            return Ok(());
        }

        let ensure_column = |name: &str, ddl: &str| -> Result<()> {
            if !columns.contains(name) {
                conn.execute(ddl, [])?;
            }
            Ok(())
        };

        ensure_column(
            "source_round_id",
            "ALTER TABLE memory_fragments ADD COLUMN source_round_id TEXT NOT NULL DEFAULT ''",
        )?;
        ensure_column(
            "tags",
            "ALTER TABLE memory_fragments ADD COLUMN tags TEXT NOT NULL DEFAULT '[]'",
        )?;
        ensure_column(
            "entities",
            "ALTER TABLE memory_fragments ADD COLUMN entities TEXT NOT NULL DEFAULT '[]'",
        )?;
        ensure_column(
            "importance",
            "ALTER TABLE memory_fragments ADD COLUMN importance REAL NOT NULL DEFAULT 0.6",
        )?;
        ensure_column(
            "confidence",
            "ALTER TABLE memory_fragments ADD COLUMN confidence REAL NOT NULL DEFAULT 0.7",
        )?;
        ensure_column(
            "tier",
            "ALTER TABLE memory_fragments ADD COLUMN tier TEXT NOT NULL DEFAULT 'working'",
        )?;
        ensure_column(
            "pinned",
            "ALTER TABLE memory_fragments ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0",
        )?;
        ensure_column(
            "confirmed_by_user",
            "ALTER TABLE memory_fragments ADD COLUMN confirmed_by_user INTEGER NOT NULL DEFAULT 0",
        )?;
        ensure_column(
            "access_count",
            "ALTER TABLE memory_fragments ADD COLUMN access_count INTEGER NOT NULL DEFAULT 0",
        )?;
        ensure_column(
            "hit_count",
            "ALTER TABLE memory_fragments ADD COLUMN hit_count INTEGER NOT NULL DEFAULT 0",
        )?;
        ensure_column(
            "last_accessed_at",
            "ALTER TABLE memory_fragments ADD COLUMN last_accessed_at REAL NOT NULL DEFAULT 0",
        )?;
        ensure_column(
            "valid_from",
            "ALTER TABLE memory_fragments ADD COLUMN valid_from REAL NOT NULL DEFAULT 0",
        )?;
        ensure_column(
            "invalidated_at",
            "ALTER TABLE memory_fragments ADD COLUMN invalidated_at REAL",
        )?;
        ensure_column(
            "supersedes_memory_id",
            "ALTER TABLE memory_fragments ADD COLUMN supersedes_memory_id TEXT",
        )?;
        ensure_column(
            "superseded_by_memory_id",
            "ALTER TABLE memory_fragments ADD COLUMN superseded_by_memory_id TEXT",
        )?;
        ensure_column(
            "embedding_model",
            "ALTER TABLE memory_fragments ADD COLUMN embedding_model TEXT",
        )?;
        ensure_column(
            "vector_ref",
            "ALTER TABLE memory_fragments ADD COLUMN vector_ref TEXT",
        )?;

        let _ = conn.execute(
            "UPDATE memory_fragments SET tags = '[]' WHERE tags IS NULL OR trim(tags) = ''",
            [],
        );
        let _ = conn.execute(
            "UPDATE memory_fragments SET entities = '[]' WHERE entities IS NULL OR trim(entities) = ''",
            [],
        );
        let _ = conn.execute(
            "UPDATE memory_fragments SET valid_from = COALESCE(NULLIF(valid_from, 0), updated_at, created_at, 0)",
            [],
        );
        Ok(())
    }
}

fn load_table_columns(conn: &Connection, table: &str) -> Result<HashSet<String>> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    let mut columns = HashSet::new();
    for name in rows.flatten() {
        columns.insert(name);
    }
    Ok(columns)
}

impl SqliteSchemaStorage for SqliteStorage {
    fn ensure_initialized_impl(&self) -> Result<()> {
        if self.initialized.load(Ordering::SeqCst) {
            return Ok(());
        }
        let _guard = self.init_guard.lock();
        if self.initialized.load(Ordering::SeqCst) {
            return Ok(());
        }
        let conn = self.open()?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS meta (
              key TEXT PRIMARY KEY,
              value TEXT NOT NULL,
              updated_time REAL NOT NULL
            );
            CREATE TABLE IF NOT EXISTS chat_history (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              user_id TEXT NOT NULL,
              session_id TEXT NOT NULL,
              role TEXT NOT NULL,
              content TEXT,
              timestamp TEXT,
              meta TEXT,
              payload TEXT NOT NULL,
              created_time REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_chat_history_session
              ON chat_history (user_id, session_id, id);
            CREATE TABLE IF NOT EXISTS model_context_entries (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              user_id TEXT NOT NULL,
              session_id TEXT NOT NULL,
              role TEXT NOT NULL,
              payload TEXT NOT NULL,
              created_time REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_model_context_entries_session
              ON model_context_entries (user_id, session_id, id);
            CREATE TABLE IF NOT EXISTS tool_logs (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              user_id TEXT NOT NULL,
              session_id TEXT NOT NULL,
              tool TEXT,
              ok INTEGER,
              error TEXT,
              args TEXT,
              data TEXT,
              timestamp TEXT,
              payload TEXT NOT NULL,
              created_time REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_tool_logs_session
              ON tool_logs (user_id, session_id, id);
            CREATE TABLE IF NOT EXISTS artifact_logs (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              user_id TEXT NOT NULL,
              session_id TEXT NOT NULL,
              kind TEXT NOT NULL,
              name TEXT,
              payload TEXT NOT NULL,
              created_time REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_artifact_logs_session
              ON artifact_logs (user_id, session_id, id);
            CREATE TABLE IF NOT EXISTS monitor_sessions (
              session_id TEXT PRIMARY KEY,
              user_id TEXT,
              status TEXT,
              updated_time REAL,
              payload TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_monitor_sessions_status
              ON monitor_sessions (status);
            CREATE TABLE IF NOT EXISTS session_locks (
              session_id TEXT PRIMARY KEY,
              user_id TEXT NOT NULL,
              agent_id TEXT NOT NULL DEFAULT '',
              created_time REAL NOT NULL,
              updated_time REAL NOT NULL,
              expires_at REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_session_locks_user_agent
              ON session_locks (user_id, agent_id);
            CREATE INDEX IF NOT EXISTS idx_session_locks_expires
              ON session_locks (expires_at);
            CREATE TABLE IF NOT EXISTS agent_threads (
              thread_id TEXT PRIMARY KEY,
              user_id TEXT NOT NULL,
              agent_id TEXT NOT NULL DEFAULT '',
              session_id TEXT NOT NULL,
              status TEXT NOT NULL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              UNIQUE(user_id, agent_id)
            );
            CREATE INDEX IF NOT EXISTS idx_agent_threads_user
              ON agent_threads (user_id);
            CREATE TABLE IF NOT EXISTS agent_tasks (
              task_id TEXT PRIMARY KEY,
              thread_id TEXT NOT NULL,
              user_id TEXT NOT NULL,
              agent_id TEXT NOT NULL DEFAULT '',
              session_id TEXT NOT NULL,
              status TEXT NOT NULL,
              request_payload TEXT NOT NULL,
              request_id TEXT,
              retry_count INTEGER NOT NULL DEFAULT 0,
              retry_at REAL NOT NULL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              started_at REAL,
              finished_at REAL,
              last_error TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_agent_tasks_thread_status
              ON agent_tasks (thread_id, status, retry_at);
            CREATE INDEX IF NOT EXISTS idx_agent_tasks_status
              ON agent_tasks (status, retry_at);
            CREATE INDEX IF NOT EXISTS idx_agent_tasks_user
              ON agent_tasks (user_id, agent_id);
            CREATE TABLE IF NOT EXISTS stream_events (
              session_id TEXT NOT NULL,
              event_id INTEGER NOT NULL,
              user_id TEXT NOT NULL,
              event_type TEXT,
              user_round INTEGER,
              payload TEXT NOT NULL,
              created_time REAL NOT NULL,
              PRIMARY KEY (session_id, event_id)
            );
            CREATE TABLE IF NOT EXISTS memory_settings (
              user_id TEXT PRIMARY KEY,
              enabled INTEGER NOT NULL,
              updated_time REAL NOT NULL
            );
            CREATE TABLE IF NOT EXISTS memory_records (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              user_id TEXT NOT NULL,
              session_id TEXT NOT NULL,
              summary TEXT NOT NULL,
              created_time REAL NOT NULL,
              updated_time REAL NOT NULL,
              UNIQUE(user_id, session_id)
            );
            CREATE INDEX IF NOT EXISTS idx_memory_records_user_time
              ON memory_records (user_id, updated_time);
            CREATE TABLE IF NOT EXISTS memory_task_logs (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              task_id TEXT NOT NULL,
              user_id TEXT NOT NULL,
              session_id TEXT NOT NULL,
              status TEXT,
              queued_time REAL,
              started_time REAL,
              finished_time REAL,
              elapsed_s REAL,
              request_payload TEXT,
              result TEXT,
              error TEXT,
              updated_time REAL NOT NULL,
              UNIQUE(user_id, session_id)
            );
            CREATE INDEX IF NOT EXISTS idx_memory_task_logs_updated
              ON memory_task_logs (updated_time);
            CREATE INDEX IF NOT EXISTS idx_memory_task_logs_task_id
              ON memory_task_logs (task_id);
            CREATE TABLE IF NOT EXISTS memory_fragments (
              memory_id TEXT PRIMARY KEY,
              user_id TEXT NOT NULL,
              agent_id TEXT NOT NULL,
              source_session_id TEXT NOT NULL,
              source_round_id TEXT NOT NULL,
              source_type TEXT NOT NULL,
              category TEXT NOT NULL,
              title_l0 TEXT NOT NULL,
              summary_l1 TEXT NOT NULL,
              content_l2 TEXT NOT NULL,
              fact_key TEXT NOT NULL,
              tags TEXT NOT NULL,
              entities TEXT NOT NULL,
              importance REAL NOT NULL,
              confidence REAL NOT NULL,
              tier TEXT NOT NULL,
              status TEXT NOT NULL,
              pinned INTEGER NOT NULL DEFAULT 0,
              confirmed_by_user INTEGER NOT NULL DEFAULT 0,
              access_count INTEGER NOT NULL DEFAULT 0,
              hit_count INTEGER NOT NULL DEFAULT 0,
              last_accessed_at REAL NOT NULL DEFAULT 0,
              valid_from REAL NOT NULL,
              invalidated_at REAL,
              supersedes_memory_id TEXT,
              superseded_by_memory_id TEXT,
              embedding_model TEXT,
              vector_ref TEXT,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_memory_fragments_user_agent
              ON memory_fragments (user_id, agent_id, updated_at DESC);
            CREATE INDEX IF NOT EXISTS idx_memory_fragments_fact_key
              ON memory_fragments (user_id, agent_id, fact_key);
            CREATE INDEX IF NOT EXISTS idx_memory_fragments_status
              ON memory_fragments (user_id, agent_id, status, updated_at DESC);
            CREATE TABLE IF NOT EXISTS memory_fragment_embeddings (
              memory_id TEXT NOT NULL,
              user_id TEXT NOT NULL,
              agent_id TEXT NOT NULL,
              embedding_model TEXT NOT NULL,
              content_hash TEXT NOT NULL,
              vector_json TEXT NOT NULL,
              dimensions INTEGER NOT NULL,
              updated_at REAL NOT NULL,
              PRIMARY KEY (memory_id, embedding_model, content_hash)
            );
            CREATE INDEX IF NOT EXISTS idx_memory_fragment_embeddings_user_agent
              ON memory_fragment_embeddings (user_id, agent_id, updated_at DESC);
            CREATE INDEX IF NOT EXISTS idx_memory_fragment_embeddings_memory
              ON memory_fragment_embeddings (memory_id, updated_at DESC);
            CREATE TABLE IF NOT EXISTS memory_hits (
              hit_id TEXT PRIMARY KEY,
              memory_id TEXT NOT NULL,
              user_id TEXT NOT NULL,
              agent_id TEXT NOT NULL,
              session_id TEXT NOT NULL,
              round_id TEXT NOT NULL,
              query_text TEXT NOT NULL,
              reason_json TEXT NOT NULL,
              lexical_score REAL NOT NULL,
              semantic_score REAL NOT NULL,
              freshness_score REAL NOT NULL,
              importance_score REAL NOT NULL,
              final_score REAL NOT NULL,
              created_at REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_memory_hits_user_agent
              ON memory_hits (user_id, agent_id, created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_memory_hits_session
              ON memory_hits (user_id, agent_id, session_id, created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_memory_hits_memory
              ON memory_hits (memory_id, created_at DESC);
            CREATE TABLE IF NOT EXISTS memory_jobs (
              job_id TEXT PRIMARY KEY,
              user_id TEXT NOT NULL,
              agent_id TEXT NOT NULL,
              session_id TEXT NOT NULL,
              job_type TEXT NOT NULL,
              status TEXT NOT NULL,
              request_payload TEXT NOT NULL,
              result_summary TEXT NOT NULL,
              error_message TEXT NOT NULL,
              queued_at REAL NOT NULL,
              started_at REAL NOT NULL,
              finished_at REAL NOT NULL,
              updated_at REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_memory_jobs_user_agent
              ON memory_jobs (user_id, agent_id, updated_at DESC);
            CREATE INDEX IF NOT EXISTS idx_memory_jobs_session
              ON memory_jobs (session_id, updated_at DESC);
            CREATE TABLE IF NOT EXISTS benchmark_runs (
              run_id TEXT PRIMARY KEY,
              user_id TEXT,
              model_name TEXT,
              judge_model_name TEXT,
              status TEXT,
              total_score REAL,
              started_time REAL,
              finished_time REAL,
              payload TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_benchmark_runs_user
              ON benchmark_runs (user_id);
            CREATE INDEX IF NOT EXISTS idx_benchmark_runs_status
              ON benchmark_runs (status);
            CREATE INDEX IF NOT EXISTS idx_benchmark_runs_started
              ON benchmark_runs (started_time);
            CREATE TABLE IF NOT EXISTS benchmark_attempts (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              run_id TEXT NOT NULL,
              task_id TEXT NOT NULL,
              attempt_no INTEGER NOT NULL,
              status TEXT,
              final_score REAL,
              started_time REAL,
              finished_time REAL,
              payload TEXT NOT NULL,
              UNIQUE(run_id, task_id, attempt_no)
            );
            CREATE INDEX IF NOT EXISTS idx_benchmark_attempts_run
              ON benchmark_attempts (run_id, task_id, attempt_no);
            CREATE INDEX IF NOT EXISTS idx_benchmark_attempts_status
              ON benchmark_attempts (status);
            CREATE TABLE IF NOT EXISTS benchmark_task_aggregates (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              run_id TEXT NOT NULL,
              task_id TEXT NOT NULL,
              status TEXT,
              mean_score REAL,
              payload TEXT NOT NULL,
              UNIQUE(run_id, task_id)
            );
            CREATE INDEX IF NOT EXISTS idx_benchmark_task_aggregates_run
              ON benchmark_task_aggregates (run_id, task_id);
            CREATE TABLE IF NOT EXISTS user_accounts (
              user_id TEXT PRIMARY KEY,
              username TEXT NOT NULL UNIQUE,
              email TEXT,
              password_hash TEXT NOT NULL,
              roles TEXT NOT NULL,
              status TEXT NOT NULL,
              access_level TEXT NOT NULL,
              unit_id TEXT,
              token_balance INTEGER NOT NULL DEFAULT 0,
              token_granted_total INTEGER NOT NULL DEFAULT 0,
              token_used_total INTEGER NOT NULL DEFAULT 0,
              last_token_grant_date TEXT,
              experience_total INTEGER NOT NULL DEFAULT 0,
              is_demo INTEGER NOT NULL DEFAULT 0,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              last_login_at REAL
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_user_accounts_username
              ON user_accounts (username);
            CREATE UNIQUE INDEX IF NOT EXISTS idx_user_accounts_email
              ON user_accounts (email);
            CREATE INDEX IF NOT EXISTS idx_user_accounts_unit
              ON user_accounts (unit_id);
            CREATE INDEX IF NOT EXISTS idx_user_accounts_created
              ON user_accounts (created_at);
            CREATE INDEX IF NOT EXISTS idx_user_accounts_unit_created
              ON user_accounts (unit_id, created_at);
            CREATE TABLE IF NOT EXISTS org_units (
              unit_id TEXT PRIMARY KEY,
              parent_id TEXT,
              name TEXT NOT NULL,
              level INTEGER NOT NULL,
              path TEXT NOT NULL,
              path_name TEXT NOT NULL,
              sort_order INTEGER NOT NULL DEFAULT 0,
              leader_ids TEXT NOT NULL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_org_units_parent
              ON org_units (parent_id);
            CREATE INDEX IF NOT EXISTS idx_org_units_path
              ON org_units (path);
            CREATE TABLE IF NOT EXISTS external_links (
              link_id TEXT PRIMARY KEY,
              title TEXT NOT NULL,
              description TEXT NOT NULL,
              url TEXT NOT NULL,
              icon TEXT NOT NULL,
              allowed_levels TEXT NOT NULL,
              sort_order INTEGER NOT NULL DEFAULT 0,
              enabled INTEGER NOT NULL DEFAULT 1,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_external_links_order
              ON external_links (enabled, sort_order, updated_at);
            CREATE TABLE IF NOT EXISTS user_tokens (
              token TEXT PRIMARY KEY,
              user_id TEXT NOT NULL,
              session_scope TEXT NOT NULL DEFAULT 'default',
              expires_at REAL NOT NULL,
              created_at REAL NOT NULL,
              last_used_at REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_user_tokens_user
              ON user_tokens (user_id);
            CREATE INDEX IF NOT EXISTS idx_user_tokens_expires
              ON user_tokens (expires_at);
            CREATE TABLE IF NOT EXISTS user_session_scopes (
              user_id TEXT NOT NULL,
              session_scope TEXT NOT NULL,
              last_login_at REAL NOT NULL,
              PRIMARY KEY (user_id, session_scope)
            );
            CREATE TABLE IF NOT EXISTS user_tool_access (
              user_id TEXT PRIMARY KEY,
              allowed_tools TEXT,
              updated_at REAL NOT NULL
            );
            CREATE TABLE IF NOT EXISTS chat_sessions (
              session_id TEXT PRIMARY KEY,
              user_id TEXT NOT NULL,
              title TEXT,
              status TEXT,
              agent_id TEXT,
              tool_overrides TEXT,
              parent_session_id TEXT,
              parent_message_id TEXT,
              spawn_label TEXT,
              spawned_by TEXT,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              last_message_at REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_chat_sessions_user
              ON chat_sessions (user_id);
            CREATE INDEX IF NOT EXISTS idx_chat_sessions_updated
              ON chat_sessions (user_id, updated_at);
            CREATE INDEX IF NOT EXISTS idx_chat_sessions_parent
              ON chat_sessions (user_id, parent_session_id, updated_at);
            CREATE TABLE IF NOT EXISTS session_goals (
              session_id TEXT PRIMARY KEY,
              user_id TEXT NOT NULL,
              goal_id TEXT NOT NULL,
              objective TEXT NOT NULL,
              status TEXT NOT NULL,
              token_budget INTEGER,
              tokens_used INTEGER NOT NULL DEFAULT 0,
              time_used_seconds INTEGER NOT NULL DEFAULT 0,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              completed_at REAL,
              last_continued_at REAL,
              source TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_session_goals_user
              ON session_goals (user_id, updated_at DESC);
            CREATE INDEX IF NOT EXISTS idx_session_goals_status
              ON session_goals (user_id, status, updated_at DESC);
            CREATE TABLE IF NOT EXISTS user_world_conversations (
              conversation_id TEXT PRIMARY KEY,
              conversation_type TEXT NOT NULL,
              participant_a TEXT NOT NULL,
              participant_b TEXT NOT NULL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              last_message_at REAL NOT NULL,
              last_message_id INTEGER,
              last_message_preview TEXT
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_user_world_conversations_participants
              ON user_world_conversations (participant_a, participant_b);
            CREATE INDEX IF NOT EXISTS idx_user_world_conversations_updated
              ON user_world_conversations (updated_at DESC);
            CREATE INDEX IF NOT EXISTS idx_user_world_conversations_last_message
              ON user_world_conversations (last_message_at DESC);
            CREATE TABLE IF NOT EXISTS user_world_groups (
              group_id TEXT PRIMARY KEY,
              conversation_id TEXT NOT NULL UNIQUE,
              group_name TEXT NOT NULL,
              owner_user_id TEXT NOT NULL,
              announcement TEXT,
              announcement_updated_at REAL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_user_world_groups_conversation
              ON user_world_groups (conversation_id);
            CREATE INDEX IF NOT EXISTS idx_user_world_groups_owner
              ON user_world_groups (owner_user_id, updated_at DESC);
            CREATE TABLE IF NOT EXISTS user_world_members (
              conversation_id TEXT NOT NULL,
              user_id TEXT NOT NULL,
              peer_user_id TEXT NOT NULL,
              last_read_message_id INTEGER,
              unread_count_cache INTEGER NOT NULL DEFAULT 0,
              pinned INTEGER NOT NULL DEFAULT 0,
              muted INTEGER NOT NULL DEFAULT 0,
              updated_at REAL NOT NULL,
              PRIMARY KEY (conversation_id, user_id)
            );
            CREATE INDEX IF NOT EXISTS idx_user_world_members_user_updated
              ON user_world_members (user_id, updated_at DESC);
            CREATE INDEX IF NOT EXISTS idx_user_world_members_conversation
              ON user_world_members (conversation_id);
            CREATE TABLE IF NOT EXISTS user_world_messages (
              message_id INTEGER PRIMARY KEY AUTOINCREMENT,
              conversation_id TEXT NOT NULL,
              sender_user_id TEXT NOT NULL,
              content TEXT NOT NULL,
              content_type TEXT NOT NULL,
              client_msg_id TEXT,
              created_at REAL NOT NULL
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_user_world_messages_client
              ON user_world_messages (conversation_id, client_msg_id);
            CREATE INDEX IF NOT EXISTS idx_user_world_messages_conversation
              ON user_world_messages (conversation_id, message_id DESC);
            CREATE TABLE IF NOT EXISTS user_world_events (
              conversation_id TEXT NOT NULL,
              event_id INTEGER NOT NULL,
              event_type TEXT NOT NULL,
              payload TEXT NOT NULL,
              created_time REAL NOT NULL,
              PRIMARY KEY (conversation_id, event_id)
            );
            CREATE INDEX IF NOT EXISTS idx_user_world_events_created_time
              ON user_world_events (created_time);
            CREATE INDEX IF NOT EXISTS idx_user_world_events_conversation
              ON user_world_events (conversation_id, event_id);
            CREATE TABLE IF NOT EXISTS beeroom_chat_messages (
              message_id INTEGER PRIMARY KEY AUTOINCREMENT,
              user_id TEXT NOT NULL,
              group_id TEXT NOT NULL,
              sender_kind TEXT NOT NULL,
              sender_name TEXT NOT NULL,
              sender_agent_id TEXT,
              mention_name TEXT,
              mention_agent_id TEXT,
              body TEXT NOT NULL,
              meta TEXT,
              tone TEXT NOT NULL,
              client_msg_id TEXT,
              created_at REAL NOT NULL
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_beeroom_chat_messages_client
              ON beeroom_chat_messages (user_id, group_id, client_msg_id);
            CREATE INDEX IF NOT EXISTS idx_beeroom_chat_messages_group
              ON beeroom_chat_messages (user_id, group_id, message_id DESC);
            CREATE TABLE IF NOT EXISTS session_runs (
              run_id TEXT PRIMARY KEY,
              session_id TEXT NOT NULL,
              parent_session_id TEXT,
              user_id TEXT NOT NULL,
              dispatch_id TEXT,
              run_kind TEXT,
              requested_by TEXT,
              agent_id TEXT,
              model_name TEXT,
              status TEXT NOT NULL,
              queued_time REAL,
              started_time REAL,
              finished_time REAL,
              elapsed_s REAL,
              result TEXT,
              error TEXT,
              updated_time REAL NOT NULL,
              metadata TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_session_runs_session
              ON session_runs (session_id, updated_time);
            CREATE INDEX IF NOT EXISTS idx_session_runs_user
              ON session_runs (user_id, updated_time);
            CREATE INDEX IF NOT EXISTS idx_session_runs_parent
              ON session_runs (parent_session_id, updated_time);
            CREATE INDEX IF NOT EXISTS idx_session_runs_dispatch
              ON session_runs (user_id, dispatch_id, updated_time);
            CREATE TABLE IF NOT EXISTS cron_jobs (
              job_id TEXT PRIMARY KEY,
              user_id TEXT NOT NULL,
              session_id TEXT NOT NULL,
              agent_id TEXT,
              name TEXT,
              session_target TEXT NOT NULL,
              payload TEXT NOT NULL,
              deliver TEXT,
              enabled INTEGER NOT NULL,
              delete_after_run INTEGER NOT NULL,
              schedule_kind TEXT NOT NULL,
              schedule_at TEXT,
              schedule_every_ms INTEGER,
              schedule_cron TEXT,
              schedule_tz TEXT,
              dedupe_key TEXT,
              next_run_at REAL,
              running_at REAL,
              runner_id TEXT,
              run_token TEXT,
              heartbeat_at REAL,
              lease_expires_at REAL,
              last_run_at REAL,
              last_status TEXT,
              last_error TEXT,
              consecutive_failures INTEGER NOT NULL DEFAULT 0,
              auto_disabled_reason TEXT,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_cron_jobs_user
              ON cron_jobs (user_id, updated_at);
            CREATE INDEX IF NOT EXISTS idx_cron_jobs_next
              ON cron_jobs (enabled, next_run_at);
            CREATE INDEX IF NOT EXISTS idx_cron_jobs_dedupe
              ON cron_jobs (user_id, dedupe_key);
            CREATE INDEX IF NOT EXISTS idx_cron_jobs_session
              ON cron_jobs (user_id, session_id);
            CREATE TABLE IF NOT EXISTS cron_runs (
              run_id TEXT PRIMARY KEY,
              job_id TEXT NOT NULL,
              user_id TEXT NOT NULL,
              session_id TEXT,
              agent_id TEXT,
              trigger TEXT,
              status TEXT,
              summary TEXT,
              error TEXT,
              duration_ms INTEGER,
              created_at REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_cron_runs_job
              ON cron_runs (job_id, created_at);
            CREATE INDEX IF NOT EXISTS idx_cron_runs_user
              ON cron_runs (user_id, created_at);
            CREATE TABLE IF NOT EXISTS channel_accounts (
              channel TEXT NOT NULL,
              account_id TEXT NOT NULL,
              config TEXT NOT NULL,
              status TEXT NOT NULL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              PRIMARY KEY (channel, account_id)
            );
            CREATE INDEX IF NOT EXISTS idx_channel_accounts_status
              ON channel_accounts (status);
            CREATE TABLE IF NOT EXISTS channel_bindings (
              binding_id TEXT PRIMARY KEY,
              channel TEXT,
              account_id TEXT,
              peer_kind TEXT,
              peer_id TEXT,
              agent_id TEXT,
              tool_overrides TEXT,
              priority INTEGER NOT NULL DEFAULT 0,
              enabled INTEGER NOT NULL DEFAULT 1,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_channel_bindings_match
              ON channel_bindings (channel, account_id, peer_kind, peer_id, priority);
            CREATE TABLE IF NOT EXISTS channel_user_bindings (
              channel TEXT NOT NULL,
              account_id TEXT NOT NULL,
              peer_kind TEXT NOT NULL,
              peer_id TEXT NOT NULL,
              user_id TEXT NOT NULL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              PRIMARY KEY (channel, account_id, peer_kind, peer_id)
            );
            CREATE INDEX IF NOT EXISTS idx_channel_user_bindings_user
              ON channel_user_bindings (user_id);
            CREATE TABLE IF NOT EXISTS channel_sessions (
              channel TEXT NOT NULL,
              account_id TEXT NOT NULL,
              peer_kind TEXT NOT NULL,
              peer_id TEXT NOT NULL,
              thread_id TEXT NOT NULL DEFAULT '',
              session_id TEXT NOT NULL,
              agent_id TEXT,
              user_id TEXT NOT NULL,
              tts_enabled INTEGER,
              tts_voice TEXT,
              metadata TEXT,
              last_message_at REAL NOT NULL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              PRIMARY KEY (channel, account_id, peer_kind, peer_id, thread_id)
            );
            CREATE INDEX IF NOT EXISTS idx_channel_sessions_session
              ON channel_sessions (session_id);
            CREATE INDEX IF NOT EXISTS idx_channel_sessions_peer
              ON channel_sessions (channel, account_id, peer_id);
            CREATE TABLE IF NOT EXISTS channel_messages (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              channel TEXT NOT NULL,
              account_id TEXT NOT NULL,
              peer_kind TEXT NOT NULL,
              peer_id TEXT NOT NULL,
              thread_id TEXT,
              session_id TEXT,
              message_id TEXT,
              sender_id TEXT,
              message_type TEXT,
              payload TEXT NOT NULL,
              raw_payload TEXT,
              created_at REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_channel_messages_session
              ON channel_messages (session_id, id);
            CREATE INDEX IF NOT EXISTS idx_channel_messages_peer
              ON channel_messages (channel, account_id, peer_id, id);
            CREATE TABLE IF NOT EXISTS channel_outbox (
              outbox_id TEXT PRIMARY KEY,
              channel TEXT NOT NULL,
              account_id TEXT NOT NULL,
              peer_kind TEXT NOT NULL,
              peer_id TEXT NOT NULL,
              thread_id TEXT,
              payload TEXT NOT NULL,
              status TEXT NOT NULL,
              retry_count INTEGER NOT NULL,
              retry_at REAL NOT NULL,
              last_error TEXT,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              delivered_at REAL
            );
            CREATE INDEX IF NOT EXISTS idx_channel_outbox_status
              ON channel_outbox (status, retry_at);
            CREATE INDEX IF NOT EXISTS idx_channel_outbox_peer
              ON channel_outbox (channel, account_id, peer_id);
            CREATE TABLE IF NOT EXISTS bridge_centers (
              center_id TEXT PRIMARY KEY,
              name TEXT NOT NULL UNIQUE,
              code TEXT NOT NULL UNIQUE,
              description TEXT,
              owner_user_id TEXT NOT NULL,
              status TEXT NOT NULL,
              default_preset_agent_name TEXT NOT NULL,
              target_unit_id TEXT,
              default_identity_strategy TEXT NOT NULL,
              username_policy TEXT NOT NULL,
              password_policy TEXT NOT NULL,
              settings_json TEXT NOT NULL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_bridge_centers_status
              ON bridge_centers (status, updated_at);
            CREATE TABLE IF NOT EXISTS bridge_center_accounts (
              center_account_id TEXT PRIMARY KEY,
              center_id TEXT NOT NULL,
              channel TEXT NOT NULL,
              account_id TEXT NOT NULL,
              enabled INTEGER NOT NULL DEFAULT 1,
              default_preset_agent_name_override TEXT,
              identity_strategy TEXT,
              thread_strategy TEXT,
              reply_strategy TEXT,
              fallback_policy TEXT NOT NULL,
              provider_caps_json TEXT,
              status_reason TEXT,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              UNIQUE (channel, account_id),
              UNIQUE (center_id, channel, account_id)
            );
            CREATE INDEX IF NOT EXISTS idx_bridge_center_accounts_center
              ON bridge_center_accounts (center_id, updated_at);
            CREATE TABLE IF NOT EXISTS bridge_user_routes (
              route_id TEXT PRIMARY KEY,
              center_id TEXT NOT NULL,
              center_account_id TEXT NOT NULL,
              channel TEXT NOT NULL,
              account_id TEXT NOT NULL,
              external_identity_key TEXT NOT NULL,
              external_user_key TEXT,
              external_display_name TEXT,
              external_peer_id TEXT,
              external_sender_id TEXT,
              external_thread_id TEXT,
              external_profile_json TEXT,
              wunder_user_id TEXT NOT NULL,
              agent_id TEXT NOT NULL,
              agent_name TEXT NOT NULL,
              user_created INTEGER NOT NULL DEFAULT 0,
              agent_created INTEGER NOT NULL DEFAULT 0,
              status TEXT NOT NULL,
              last_session_id TEXT,
              last_error TEXT,
              first_seen_at REAL NOT NULL,
              last_seen_at REAL NOT NULL,
              last_inbound_at REAL,
              last_outbound_at REAL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              UNIQUE (center_account_id, external_identity_key)
            );
            CREATE INDEX IF NOT EXISTS idx_bridge_user_routes_center
              ON bridge_user_routes (center_id, status, last_seen_at);
            CREATE INDEX IF NOT EXISTS idx_bridge_user_routes_user
              ON bridge_user_routes (wunder_user_id, updated_at);
            CREATE INDEX IF NOT EXISTS idx_bridge_user_routes_agent
              ON bridge_user_routes (agent_id, updated_at);
            CREATE TABLE IF NOT EXISTS bridge_delivery_logs (
              delivery_id TEXT PRIMARY KEY,
              center_id TEXT NOT NULL,
              center_account_id TEXT NOT NULL,
              route_id TEXT,
              direction TEXT NOT NULL,
              stage TEXT NOT NULL,
              provider_message_id TEXT,
              session_id TEXT,
              status TEXT NOT NULL,
              summary TEXT NOT NULL,
              payload_json TEXT,
              created_at REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_bridge_delivery_logs_center
              ON bridge_delivery_logs (center_id, created_at);
            CREATE INDEX IF NOT EXISTS idx_bridge_delivery_logs_route
              ON bridge_delivery_logs (route_id, created_at);
            CREATE TABLE IF NOT EXISTS bridge_route_audit_logs (
              audit_id TEXT PRIMARY KEY,
              center_id TEXT NOT NULL,
              route_id TEXT,
              actor_type TEXT NOT NULL,
              actor_id TEXT NOT NULL,
              action TEXT NOT NULL,
              detail_json TEXT,
              created_at REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_bridge_route_audit_logs_center
              ON bridge_route_audit_logs (center_id, created_at);
            CREATE INDEX IF NOT EXISTS idx_bridge_route_audit_logs_route
              ON bridge_route_audit_logs (route_id, created_at);
            CREATE TABLE IF NOT EXISTS gateway_clients (
              connection_id TEXT PRIMARY KEY,
              role TEXT NOT NULL,
              user_id TEXT,
              node_id TEXT,
              scopes TEXT,
              caps TEXT,
              commands TEXT,
              client_info TEXT,
              status TEXT NOT NULL,
              connected_at REAL NOT NULL,
              last_seen_at REAL NOT NULL,
              disconnected_at REAL
            );
            CREATE INDEX IF NOT EXISTS idx_gateway_clients_status
              ON gateway_clients (status, role);
            CREATE INDEX IF NOT EXISTS idx_gateway_clients_node
              ON gateway_clients (node_id, status);
            CREATE TABLE IF NOT EXISTS gateway_nodes (
              node_id TEXT PRIMARY KEY,
              name TEXT,
              device_fingerprint TEXT,
              status TEXT NOT NULL,
              caps TEXT,
              commands TEXT,
              permissions TEXT,
              metadata TEXT,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              last_seen_at REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_gateway_nodes_status
              ON gateway_nodes (status);
            CREATE TABLE IF NOT EXISTS gateway_node_tokens (
              token TEXT PRIMARY KEY,
              node_id TEXT NOT NULL,
              status TEXT NOT NULL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              last_used_at REAL
            );
            CREATE INDEX IF NOT EXISTS idx_gateway_node_tokens_node
              ON gateway_node_tokens (node_id, status);
            CREATE TABLE IF NOT EXISTS media_assets (
              asset_id TEXT PRIMARY KEY,
              kind TEXT NOT NULL,
              url TEXT NOT NULL,
              mime TEXT,
              size INTEGER,
              hash TEXT,
              source TEXT,
              created_at REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_media_assets_hash
              ON media_assets (hash);
            CREATE TABLE IF NOT EXISTS speech_jobs (
              job_id TEXT PRIMARY KEY,
              job_type TEXT NOT NULL,
              status TEXT NOT NULL,
              input_text TEXT,
              input_url TEXT,
              output_text TEXT,
              output_url TEXT,
              model TEXT,
              error TEXT,
              retry_count INTEGER NOT NULL,
              next_retry_at REAL NOT NULL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              metadata TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_speech_jobs_status
              ON speech_jobs (job_type, status, next_retry_at);
            CREATE TABLE IF NOT EXISTS hives (
              hive_id TEXT PRIMARY KEY,
              user_id TEXT NOT NULL,
              name TEXT NOT NULL,
              description TEXT,
              is_default INTEGER NOT NULL DEFAULT 0,
              status TEXT NOT NULL DEFAULT 'active',
              created_time REAL NOT NULL,
              updated_time REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_hives_user
              ON hives (user_id, updated_time);
            CREATE INDEX IF NOT EXISTS idx_hives_user_status
              ON hives (user_id, status, updated_time);
            CREATE TABLE IF NOT EXISTS user_agents (
              agent_id TEXT PRIMARY KEY,
              user_id TEXT NOT NULL,
              hive_id TEXT NOT NULL DEFAULT 'default',
              name TEXT NOT NULL,
              description TEXT,
              system_prompt TEXT,
              model_name TEXT,
              tool_names TEXT,
              declared_tool_names TEXT,
              declared_skill_names TEXT,
              visible_unit_ids TEXT,
              ability_items TEXT,
              access_level TEXT NOT NULL,
              approval_mode TEXT NOT NULL DEFAULT 'full_auto',
              is_shared INTEGER NOT NULL DEFAULT 0,
              status TEXT NOT NULL,
              icon TEXT,
              sandbox_container_id INTEGER NOT NULL DEFAULT 1,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              preset_questions TEXT,
              preset_binding TEXT,
              silent INTEGER NOT NULL DEFAULT 0,
              prefer_mother INTEGER NOT NULL DEFAULT 0,
              preview_skill INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_user_agents_user
              ON user_agents (user_id, updated_at);
            CREATE INDEX IF NOT EXISTS idx_user_agents_user_hive
              ON user_agents (user_id, hive_id, updated_at);
            CREATE TABLE IF NOT EXISTS user_agent_access (
              user_id TEXT PRIMARY KEY,
              allowed_agent_ids TEXT,
              blocked_agent_ids TEXT,
              updated_at REAL NOT NULL
            );
            CREATE TABLE IF NOT EXISTS team_runs (
              team_run_id TEXT PRIMARY KEY,
              user_id TEXT NOT NULL,
              hive_id TEXT NOT NULL,
              parent_session_id TEXT NOT NULL,
              parent_agent_id TEXT,
              mother_agent_id TEXT,
              strategy TEXT NOT NULL,
              status TEXT NOT NULL,
              task_total INTEGER NOT NULL DEFAULT 0,
              task_success INTEGER NOT NULL DEFAULT 0,
              task_failed INTEGER NOT NULL DEFAULT 0,
              context_tokens_total INTEGER NOT NULL DEFAULT 0,
              context_tokens_peak INTEGER NOT NULL DEFAULT 0,
              model_round_total INTEGER NOT NULL DEFAULT 0,
              started_time REAL,
              finished_time REAL,
              elapsed_s REAL,
              summary TEXT,
              error TEXT,
              updated_time REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_team_runs_user_hive
              ON team_runs (user_id, hive_id, updated_time);
            CREATE INDEX IF NOT EXISTS idx_team_runs_user_status
              ON team_runs (user_id, status, updated_time);
            CREATE INDEX IF NOT EXISTS idx_team_runs_hive_status
              ON team_runs (hive_id, status, updated_time);
            CREATE INDEX IF NOT EXISTS idx_team_runs_status_updated
              ON team_runs (status, updated_time);
            CREATE INDEX IF NOT EXISTS idx_team_runs_hive_parent
              ON team_runs (hive_id, parent_session_id, updated_time);
            CREATE TABLE IF NOT EXISTS team_tasks (
              task_id TEXT PRIMARY KEY,
              team_run_id TEXT NOT NULL,
              user_id TEXT NOT NULL,
              hive_id TEXT NOT NULL,
              agent_id TEXT NOT NULL,
              target_session_id TEXT,
              spawned_session_id TEXT,
              session_run_id TEXT,
              status TEXT NOT NULL,
              retry_count INTEGER NOT NULL DEFAULT 0,
              priority INTEGER NOT NULL DEFAULT 0,
              started_time REAL,
              finished_time REAL,
              elapsed_s REAL,
              result_summary TEXT,
              error TEXT,
              updated_time REAL NOT NULL,
              FOREIGN KEY(team_run_id) REFERENCES team_runs(team_run_id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_team_tasks_hive_run
              ON team_tasks (hive_id, team_run_id, updated_time);
            CREATE INDEX IF NOT EXISTS idx_team_tasks_user_hive_agent
              ON team_tasks (user_id, hive_id, agent_id, updated_time);
            CREATE INDEX IF NOT EXISTS idx_team_tasks_hive_status
              ON team_tasks (hive_id, status, updated_time);
            CREATE TABLE IF NOT EXISTS vector_documents (
              doc_id TEXT PRIMARY KEY,
              owner_id TEXT NOT NULL,
              base_name TEXT NOT NULL,
              doc_name TEXT NOT NULL,
              embedding_model TEXT NOT NULL,
              chunk_size INTEGER NOT NULL,
              chunk_overlap INTEGER NOT NULL,
              chunk_count INTEGER NOT NULL,
              status TEXT NOT NULL,
              created_at REAL NOT NULL,
              updated_at REAL NOT NULL,
              content TEXT NOT NULL,
              chunks_json TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_vector_documents_owner_base
              ON vector_documents (owner_id, base_name, updated_at);
            CREATE TABLE IF NOT EXISTS vector_chunks (
              chunk_id TEXT PRIMARY KEY,
              owner_id TEXT NOT NULL,
              base_name TEXT NOT NULL,
              doc_id TEXT NOT NULL,
              doc_name TEXT NOT NULL,
              chunk_index INTEGER NOT NULL,
              start_pos INTEGER NOT NULL,
              end_pos INTEGER NOT NULL,
              content TEXT NOT NULL,
              embedding_model TEXT NOT NULL,
              vector_json TEXT NOT NULL,
              dimensions INTEGER NOT NULL,
              updated_at REAL NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_vector_chunks_lookup
              ON vector_chunks (owner_id, base_name, embedding_model, updated_at DESC);
            CREATE INDEX IF NOT EXISTS idx_vector_chunks_doc
              ON vector_chunks (owner_id, base_name, doc_id);
            "#,
        )?;
        self.ensure_user_account_quota_columns(&conn)?;
        self.ensure_user_account_level_columns(&conn)?;
        self.ensure_user_account_unit_columns(&conn)?;
        self.ensure_user_account_list_indexes(&conn)?;
        self.ensure_user_token_columns(&conn)?;
        self.ensure_user_tool_access_columns(&conn)?;
        self.ensure_chat_session_columns(&conn)?;
        self.ensure_stream_event_workflow_columns(&conn)?;
        self.ensure_channel_columns(&conn)?;
        self.ensure_session_lock_columns(&conn)?;
        self.ensure_session_run_columns(&conn)?;
        self.ensure_user_agent_columns(&conn)?;
        self.ensure_team_run_columns(&conn)?;
        self.ensure_team_task_columns(&conn)?;
        self.ensure_user_world_group_columns(&conn)?;
        self.ensure_cron_columns(&conn)?;
        self.ensure_memory_fragment_columns(&conn)?;
        self.initialized.store(true, Ordering::SeqCst);
        Ok(())
    }
}
