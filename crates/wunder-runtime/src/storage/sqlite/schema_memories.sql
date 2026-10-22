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
