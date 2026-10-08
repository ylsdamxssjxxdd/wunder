#!/usr/bin/env python3
"""Inspect or repair emptied historical thread directories in Docker PostgreSQL.

Uses the same conservative predicates as runtime log cleanup. Defaults to a
read-only preview; --apply requires --backup and saves full candidate records
before deletion. No credentials, thread titles or message text are printed.
"""
import argparse
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--container", required=True)
    parser.add_argument("--database", required=True)
    parser.add_argument("--database-user", required=True)
    parser.add_argument("--user-id", required=True)
    parser.add_argument("--apply", action="store_true")
    parser.add_argument("--backup", type=Path)
    args = parser.parse_args()
    if args.apply and not args.backup:
        parser.error("--apply requires --backup")

    def query(sql):
        result = subprocess.run([
            "docker", "exec", "-i", args.container, "psql", "-X", "-qAt",
            "-v", "ON_ERROR_STOP=1", "-v", f"owner={args.user_id}",
            "-U", args.database_user, "-d", args.database,
        ], input=sql, text=True, encoding="utf-8", capture_output=True, check=True)
        return result.stdout.strip()

    sql_root = ROOT / "crates/wunder-runtime/src/storage"
    empty = (sql_root / "session_cleanup_empty.sql").read_text(encoding="utf-8")
    live = (sql_root / "session_cleanup_live.sql").read_text(encoding="utf-8").replace(
        ":now", "EXTRACT(EPOCH FROM NOW())")
    predicate = f"c.user_id = :'owner' AND c.updated_at < EXTRACT(EPOCH FROM NOW()) - 300 AND ({empty}) AND NOT ({live})"
    candidates = json.loads(query(f"SELECT COALESCE(jsonb_agg(to_jsonb(c)), '[]'::jsonb) FROM chat_sessions c WHERE {predicate};"))
    print(json.dumps({"candidates": len(candidates), "applied": False}))
    if not args.apply or not candidates:
        return
    backup = {"chat_sessions": candidates, "session_goals": []}
    ids_sql = ",".join("'" + row["session_id"].replace("'", "''") + "'" for row in candidates)
    backup["session_goals"] = json.loads(query(f"SELECT COALESCE(jsonb_agg(to_jsonb(g)), '[]'::jsonb) FROM session_goals g WHERE user_id=:'owner' AND session_id IN ({ids_sql});"))
    args.backup.parent.mkdir(parents=True, exist_ok=True)
    # Refuse to overwrite an earlier backup. The archive exists before any mutation.
    with args.backup.open("x", encoding="utf-8") as output:
        json.dump(backup, output, ensure_ascii=False, indent=2)
    records_sql = json.dumps(candidates, ensure_ascii=False).replace("'", "''")
    result = query(f"""
BEGIN;
SET LOCAL lock_timeout = '5s';
LOCK TABLE chat_sessions, session_locks, agent_tasks, session_runs, monitor_sessions,
chat_history, model_context_entries, stream_events, tool_logs, artifact_logs, cron_jobs,
session_goals IN SHARE ROW EXCLUSIVE MODE;
CREATE TEMP TABLE repair_candidates ON COMMIT DROP AS
SELECT c.session_id FROM chat_sessions c
WHERE {predicate} AND to_jsonb(c) IN (SELECT value FROM jsonb_array_elements('{records_sql}'::jsonb))
FOR UPDATE OF c;
DELETE FROM session_goals WHERE user_id=:'owner' AND session_id IN (SELECT session_id FROM repair_candidates);
WITH removed AS (DELETE FROM chat_sessions WHERE user_id=:'owner' AND session_id IN (SELECT session_id FROM repair_candidates) RETURNING 1)
SELECT json_build_object('removed', count(*), 'applied', true) FROM removed;
COMMIT;
""")
    print(result)


if __name__ == "__main__":
    main()
