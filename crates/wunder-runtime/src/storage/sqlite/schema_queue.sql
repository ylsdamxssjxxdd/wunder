CREATE TABLE IF NOT EXISTS thread_open_leases (
              session_id TEXT PRIMARY KEY,
              user_id TEXT NOT NULL,
              owner_instance_id TEXT NOT NULL,
              client_kind TEXT NOT NULL,
              fencing_token INTEGER NOT NULL DEFAULT 0,
              updated_time REAL NOT NULL,
              expires_at REAL NOT NULL
            );

CREATE TABLE IF NOT EXISTS thread_open_lease_generations (
              session_id TEXT PRIMARY KEY,
              last_token INTEGER NOT NULL
            );

CREATE INDEX IF NOT EXISTS idx_thread_open_leases_user
              ON thread_open_leases (user_id, expires_at);

CREATE TABLE IF NOT EXISTS session_locks (
              session_id TEXT PRIMARY KEY,
              user_id TEXT NOT NULL,
              agent_id TEXT NOT NULL DEFAULT '',
              lock_token TEXT NOT NULL DEFAULT '',
              created_time REAL NOT NULL,
              updated_time REAL NOT NULL,
              expires_at REAL NOT NULL
            , suspended INTEGER NOT NULL DEFAULT 0);

CREATE INDEX IF NOT EXISTS idx_session_locks_user_agent
              ON session_locks (user_id, agent_id);

CREATE INDEX IF NOT EXISTS idx_session_locks_expires
              ON session_locks (expires_at);

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
            , priority INTEGER NOT NULL DEFAULT 0);

CREATE INDEX IF NOT EXISTS idx_agent_tasks_thread_status
              ON agent_tasks (thread_id, status, retry_at);

CREATE INDEX IF NOT EXISTS idx_agent_tasks_status
              ON agent_tasks (status, retry_at);

CREATE INDEX IF NOT EXISTS idx_agent_tasks_user
              ON agent_tasks (user_id, agent_id);

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

CREATE INDEX IF NOT EXISTS idx_agent_tasks_session_dispatch ON agent_tasks(session_id,status,priority DESC,retry_at);

CREATE INDEX IF NOT EXISTS idx_agent_tasks_dispatch ON agent_tasks(status,priority DESC,retry_at,created_at,task_id);
