"""Triage helper: list SQLite tables present in a database but absent from the
current schema source, so retired tables can be identified for cleanup."""

import re
import sqlite3
import sys
from pathlib import Path

SCHEMA = Path("crates/wunder-runtime/src/storage/sqlite/schema.rs")


def main() -> int:
    dbs = [Path(p) for p in sys.argv[1:]] or [Path("config/data/wunder.db")]
    known = set(re.findall(r"CREATE TABLE IF NOT EXISTS (\w+)", SCHEMA.read_text(encoding="utf-8")))
    print(f"schema tables: {len(known)}")
    for db in dbs:
        if not db.exists():
            print(f"skip missing {db}")
            continue
        con = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
        rows = [r[0] for r in con.execute("SELECT name FROM sqlite_master WHERE type='table'")]
        extra = sorted(t for t in rows if t not in known and not t.startswith("sqlite_"))
        print(f"\n=== {db} (tables={len(rows)}) ===")
        for table in extra:
            try:
                count = con.execute(f"SELECT COUNT(*) FROM {table}").fetchone()[0]
            except sqlite3.Error as exc:  # pragma: no cover - diagnostics only
                count = f"err {exc}"
            print(f"  retired? {table}: {count} rows")
        cols = [r[1] for r in con.execute("PRAGMA table_info(user_agents)")]
        print(f"  user_agents.hive_id present: {'hive_id' in cols}")
        con.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
