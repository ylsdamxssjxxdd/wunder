"""Read-only reproduction of the current SQLite window query using synthetic data."""
from pathlib import Path
import json
import re
import sqlite3
import time

repo = Path(__file__).resolve().parents[2]
source = (repo / "crates/wunder-runtime/src/storage/sqlite/thread_log_store.rs").read_text(encoding="utf-8-sig")
match = re.search(r'tx\.prepare\("(SELECT item_id,item_index,kind,status,revision,payload,created_time,updated_time,turn_id,visibility,root_turn_id,created_seq FROM thread_items WHERE user_id=\? AND session_id=\? AND visibility=\'user\' AND turn_id IN .*? ORDER BY created_seq DESC, item_index DESC LIMIT \?)"\)', source)
if match is None:
    raise SystemExit("Window query changed; update the diagnostic before comparing results.")
query = match.group(1)

conn = sqlite3.connect(":memory:")
conn.executescript("""
CREATE TABLE thread_items(
  user_id TEXT, session_id TEXT, item_id TEXT, turn_id TEXT, root_turn_id TEXT,
  visibility TEXT, item_index INTEGER, created_seq INTEGER, kind TEXT,
  status TEXT, revision INTEGER, payload TEXT, created_time REAL, updated_time REAL,
  PRIMARY KEY(session_id,item_id), UNIQUE(session_id,root_turn_id,item_index)
);
CREATE INDEX idx_thread_items_turn ON thread_items(user_id,session_id,root_turn_id,item_index);
CREATE INDEX idx_thread_items_context_seq ON thread_items(user_id,session_id,created_seq DESC)
  WHERE kind IN ('user_message','assistant_message','tool_message','system_message');
CREATE TABLE thread_turns(
  user_id TEXT, session_id TEXT, turn_id TEXT, user_turn_index INTEGER,
  created_time REAL, trigger_kind TEXT, PRIMARY KEY(session_id,turn_id)
);
CREATE INDEX idx_thread_turns_page ON thread_turns(user_id,session_id,user_turn_index DESC);
CREATE UNIQUE INDEX idx_thread_root_round ON thread_turns(session_id,user_turn_index)
  WHERE trigger_kind='user';
""")
args = ("fixture-user", "fixture-thread", "fixture-user", "fixture-thread", 99999, 2, 20)
plan = [row[3] for row in conn.execute("EXPLAIN QUERY PLAN " + query, args)]
results = []
prior = 0
for count in (20_000, 200_000, 500_000):
    conn.executemany("INSERT INTO thread_items VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?)", (
        ("fixture-user", "fixture-thread", str(i), "turn-" + str(i // 100),
         "turn-" + str(i // 100), "user", i % 100, i + 1, "tool_call",
         "completed", 1, "{}", float(i), float(i)) for i in range(prior, count)
    ))
    conn.executemany("INSERT INTO thread_turns VALUES (?,?,?,?,?,?)", (
        ("fixture-user", "fixture-thread", "turn-" + str(i), i + 1, float(i), "user")
        for i in range(prior // 100, count // 100)
    ))
    conn.commit()
    samples = []
    for _ in range(3):
        started = time.perf_counter()
        rows = conn.execute(query, args).fetchall()
        samples.append(round((time.perf_counter() - started) * 1000, 2))
    results.append({"total_items": count, "window_turns": 2,
                    "returned_items": len(rows), "query_ms": samples})
    prior = count
print(json.dumps({"scope": "synthetic in-memory SQLite query shape; not production latency",
                  "sqlite_version": sqlite3.sqlite_version, "query_plan": plan,
                  "results": results}, indent=2))
conn.close()
