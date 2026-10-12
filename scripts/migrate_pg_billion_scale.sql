-- One-shot PostgreSQL migration for the billion-scale storage plan.
-- Phase 1: user_usage_summary table + tool_logs.excluded backfill.
-- Phase 2: rebuild the five largest log tables as RANGE(created_time)
--          monthly partitions and backfill existing rows.
-- Usage:
--   docker exec -i wunder-postgres psql -U wunder -d wunder \
--     -v ON_ERROR_STOP=1 < scripts/migrate_pg_billion_scale.sql
-- Prototype stage: no compatibility or downtime guarantees.

\set ON_ERROR_STOP on
SET max_parallel_maintenance_workers = 0;

BEGIN;

-- ===== Phase 1: usage summary + excluded flag =====
CREATE TABLE IF NOT EXISTS user_usage_summary (
  user_id TEXT PRIMARY KEY,
  chat_records BIGINT NOT NULL DEFAULT 0,
  tool_records BIGINT NOT NULL DEFAULT 0,
  updated_time DOUBLE PRECISION NOT NULL DEFAULT 0
);

ALTER TABLE tool_logs ADD COLUMN IF NOT EXISTS excluded BOOLEAN NOT NULL DEFAULT FALSE;
UPDATE tool_logs SET excluded = TRUE
 WHERE excluded = FALSE
   AND (tool IN ('update_plan','sessions_yield','yield','会话让出','计划面板',
                 'question_panel','ask_panel','问询面板','performance_log',
                 'final_response','最终回复')
        OR data LIKE '%"source":"skill_read"%');

-- ===== Phase 2: partitioned rebuild =====
ALTER TABLE thread_items RENAME TO thread_items_old;
ALTER TABLE thread_item_blocks RENAME TO thread_item_blocks_old;
ALTER TABLE thread_log_changes RENAME TO thread_log_changes_old;
ALTER TABLE tool_logs RENAME TO tool_logs_old;
ALTER TABLE artifact_logs RENAME TO artifact_logs_old;
ALTER SEQUENCE tool_logs_id_seq RENAME TO tool_logs_id_seq_migrated;
ALTER SEQUENCE artifact_logs_id_seq RENAME TO artifact_logs_id_seq_migrated;

-- Renamed tables keep their index names, which would collide with the
-- fresh parents below; suffix them and let the table drops remove them.
DO $$
DECLARE r record;
BEGIN
  FOR r IN SELECT indexname FROM pg_indexes
           WHERE tablename IN ('thread_items_old','thread_item_blocks_old',
                               'thread_log_changes_old','tool_logs_old','artifact_logs_old') LOOP
    EXECUTE format('ALTER INDEX %I RENAME TO %I', r.indexname, r.indexname || '_migrated');
  END LOOP;
END $$;

CREATE TABLE thread_items (
  session_id TEXT NOT NULL,
  item_id TEXT NOT NULL,
  turn_id TEXT NOT NULL,
  root_turn_id TEXT NOT NULL,
  visibility TEXT NOT NULL DEFAULT 'user',
  user_id TEXT NOT NULL,
  item_index BIGINT NOT NULL,
  kind TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'completed',
  revision BIGINT NOT NULL DEFAULT 1,
  payload TEXT NOT NULL,
  created_time DOUBLE PRECISION NOT NULL,
  updated_time DOUBLE PRECISION NOT NULL,
  created_seq BIGINT NOT NULL DEFAULT 0,
  PRIMARY KEY (session_id, item_id, created_time)
) PARTITION BY RANGE (created_time);
CREATE INDEX idx_thread_items_turn
  ON thread_items (user_id, session_id, root_turn_id, item_index);
CREATE INDEX idx_thread_items_context_seq
  ON thread_items (user_id, session_id, created_seq DESC)
  WHERE kind IN ('user_message','assistant_message','tool_message','system_message');

CREATE TABLE thread_item_blocks (
  session_id TEXT NOT NULL,
  user_id TEXT NOT NULL,
  item_id TEXT NOT NULL,
  field TEXT NOT NULL DEFAULT 'content',
  block_index BIGINT NOT NULL,
  event_id BIGINT NOT NULL,
  payload TEXT NOT NULL,
  created_time DOUBLE PRECISION NOT NULL DEFAULT 0,
  PRIMARY KEY (session_id, item_id, field, block_index, created_time)
) PARTITION BY RANGE (created_time);
CREATE INDEX idx_thread_blocks_replay ON thread_item_blocks (session_id, event_id);

CREATE TABLE thread_log_changes (
  session_id TEXT NOT NULL,
  change_seq BIGINT NOT NULL,
  user_id TEXT NOT NULL,
  change_type TEXT NOT NULL,
  turn_id TEXT NOT NULL,
  item_id TEXT,
  revision BIGINT NOT NULL DEFAULT 1,
  payload TEXT NOT NULL,
  created_time DOUBLE PRECISION NOT NULL,
  PRIMARY KEY (session_id, change_seq, created_time)
) PARTITION BY RANGE (created_time);
CREATE INDEX idx_thread_changes_cursor ON thread_log_changes (user_id, session_id, change_seq);

CREATE TABLE tool_logs (
  id BIGSERIAL,
  user_id TEXT NOT NULL,
  session_id TEXT NOT NULL,
  tool TEXT,
  ok INTEGER,
  error TEXT,
  args TEXT,
  data TEXT,
  timestamp TEXT,
  payload TEXT NOT NULL,
  created_time DOUBLE PRECISION NOT NULL,
  excluded BOOLEAN NOT NULL DEFAULT FALSE,
  PRIMARY KEY (id, created_time)
) PARTITION BY RANGE (created_time);
CREATE INDEX idx_tool_logs_session ON tool_logs (user_id, session_id, id);
CREATE INDEX idx_tool_logs_time ON tool_logs (created_time);
CREATE INDEX idx_tool_logs_tool_time ON tool_logs (tool, created_time);

CREATE TABLE artifact_logs (
  id BIGSERIAL,
  user_id TEXT NOT NULL,
  session_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  name TEXT,
  payload TEXT NOT NULL,
  created_time DOUBLE PRECISION NOT NULL,
  PRIMARY KEY (id, created_time)
) PARTITION BY RANGE (created_time);
CREATE INDEX idx_artifact_logs_session ON artifact_logs (user_id, session_id, id);
CREATE INDEX idx_artifact_logs_time ON artifact_logs (created_time);

-- Monthly range partitions covering [-3, +3] months around the current
-- month (UTC), plus DEFAULT landing zones for anything outside the window.
DO $$
DECLARE
  base timestamp := date_trunc('month', now() at time zone 'utc');
  tbl text;
  i int;
  part text;
  start_ts double precision;
  end_ts double precision;
BEGIN
  FOREACH tbl IN ARRAY ARRAY['thread_items','thread_item_blocks',
                             'thread_log_changes','tool_logs','artifact_logs'] LOOP
    FOR i IN -3..3 LOOP
      part := tbl || '_y' || to_char(base + make_interval(months => i), 'YYYY')
                   || 'm' || to_char(base + make_interval(months => i), 'MM');
      start_ts := extract(epoch FROM base + make_interval(months => i));
      end_ts := extract(epoch FROM base + make_interval(months => i + 1));
      EXECUTE format(
        'CREATE TABLE IF NOT EXISTS %I PARTITION OF %I FOR VALUES FROM (%L) TO (%L) WITH (autovacuum_vacuum_scale_factor = 0.02)',
        part, tbl, start_ts, end_ts);
    END LOOP;
    EXECUTE format('CREATE TABLE IF NOT EXISTS %I PARTITION OF %I DEFAULT',
                   tbl || '_default', tbl);
  END LOOP;
END $$;

-- ===== Backfill =====
INSERT INTO thread_items (session_id,item_id,turn_id,root_turn_id,visibility,user_id,
                          item_index,kind,status,revision,payload,created_time,updated_time,created_seq)
SELECT session_id,item_id,turn_id,root_turn_id,visibility,user_id,
       item_index,kind,status,revision,payload,created_time,updated_time,created_seq
FROM thread_items_old;

INSERT INTO thread_item_blocks (session_id,user_id,item_id,field,block_index,event_id,payload,created_time)
SELECT b.session_id,b.user_id,b.item_id,b.field,b.block_index,b.event_id,b.payload,
       COALESCE(i.created_time, 0)
FROM thread_item_blocks_old b
LEFT JOIN thread_items i ON i.session_id = b.session_id AND i.item_id = b.item_id;

INSERT INTO thread_log_changes (session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time)
SELECT session_id,change_seq,user_id,change_type,turn_id,item_id,revision,payload,created_time
FROM thread_log_changes_old;

INSERT INTO tool_logs (id,user_id,session_id,tool,ok,error,args,data,timestamp,payload,created_time,excluded)
SELECT id,user_id,session_id,tool,ok,error,args,data,timestamp,payload,created_time,excluded
FROM tool_logs_old;

INSERT INTO artifact_logs (id,user_id,session_id,kind,name,payload,created_time)
SELECT id,user_id,session_id,kind,name,payload,created_time
FROM artifact_logs_old;

-- ===== Drop legacy heaps, re-align sequences =====
DROP TABLE thread_items_old;
DROP TABLE thread_item_blocks_old;
DROP TABLE thread_log_changes_old;
DROP TABLE tool_logs_old;
DROP TABLE artifact_logs_old;
DROP SEQUENCE IF EXISTS tool_logs_id_seq_migrated;
DROP SEQUENCE IF EXISTS artifact_logs_id_seq_migrated;
SELECT setval('tool_logs_id_seq', (SELECT COALESCE(MAX(id), 1) FROM tool_logs));
SELECT setval('artifact_logs_id_seq', (SELECT COALESCE(MAX(id), 1) FROM artifact_logs));

COMMIT;
