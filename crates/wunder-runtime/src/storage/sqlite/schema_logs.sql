CREATE TABLE IF NOT EXISTS runtime_tool_metrics (
  id INTEGER PRIMARY KEY, user_id TEXT NOT NULL, session_id TEXT NOT NULL,
  tool TEXT NOT NULL, ok INTEGER NOT NULL, excluded INTEGER NOT NULL,
  created_time REAL NOT NULL, facts TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS runtime_metrics_partition ON runtime_tool_metrics(user_id,session_id,id DESC);
CREATE INDEX IF NOT EXISTS runtime_metrics_time ON runtime_tool_metrics(created_time);
CREATE TABLE IF NOT EXISTS runtime_artifact_resources (
  id INTEGER PRIMARY KEY, user_id TEXT NOT NULL, session_id TEXT NOT NULL,
  created_time REAL NOT NULL, metadata TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS runtime_artifacts_partition ON runtime_artifact_resources(user_id,session_id,id DESC);

CREATE TABLE IF NOT EXISTS deleted_session_log_grace (
              user_id TEXT NOT NULL,
              session_id TEXT NOT NULL,
              deleted_at REAL NOT NULL,
              PRIMARY KEY (user_id, session_id)
            );

CREATE TABLE IF NOT EXISTS monitor_sessions (
              session_id TEXT PRIMARY KEY,
              user_id TEXT,
              status TEXT,
              updated_time REAL,
              payload TEXT NOT NULL
            );

CREATE INDEX IF NOT EXISTS idx_monitor_sessions_status
              ON monitor_sessions (status);

CREATE TABLE IF NOT EXISTS monitor_session_stats (
              session_id TEXT PRIMARY KEY,
              payload TEXT NOT NULL
            );

CREATE TRIGGER IF NOT EXISTS delete_monitor_session_stats
              AFTER DELETE ON monitor_sessions BEGIN
                DELETE FROM monitor_session_stats WHERE session_id=OLD.session_id;
              END;

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

CREATE INDEX IF NOT EXISTS idx_stream_events_session_round ON stream_events(session_id,user_round,event_id);
