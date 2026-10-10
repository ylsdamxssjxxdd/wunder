# AI生成
#!/usr/bin/env python3
"""Collect "concurrent agent threads" capacity for one form.

Definition of the load unit
---------------------------
One *agent session running one chat round* is treated as one "agent thread".
Capacity = the maximum number of such units the platform sustains while
  * success rate is at least --min-success-rate, and
  * p95 latency stays within --p95-budget-ms.

Because the five forms share the same engine (crates/wunder-runtime), capacity is
measured against a running backend and reported for every form. Two engines are
supported and both are thin wrappers over existing repo tooling:

  boundary  scripts/runtime_boundary_stress.py  (drives a *live* server over HTTP)
  sim       cargo ... backend_sim --report ...   (in-process engine simulation)

The step search starts at --start, increases by --step up to --max, and stops at
the first concurrency that misses the budget; the last good value wins.

Always prints one JSON envelope to stdout (also written to --out-json when set):
  { form, collector, generated, records: [ { metric, value, unit, source, ... } ] }

Usage (dry run, no server needed):
  python scripts/form-bench/collect-concurrency.py --form-id hull --dry-run
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, List, Optional

DEFAULT_BASE_URL = "http://127.0.0.1:18000/wunder"
LOAD_UNIT = "agent-session-running-one-chat-round"


def _now() -> str:
    return datetime.now(timezone.utc).isoformat()


def _extract_json_candidates(text: str) -> List[Any]:
    """Best-effort: parse whole text, then each line, then the last balanced block."""
    results: List[Any] = []
    try:
        results.append(json.loads(text))
    except Exception:
        pass

    lines = [ln.strip() for ln in text.splitlines() if ln.strip()]
    for ln in reversed(lines[-200:]):
        if ln[0] in "{[":
            try:
                results.append(json.loads(ln))
            except Exception:
                pass

    # last balanced {...}
    start = text.rfind("{")
    while start != -1:
        depth = 0
        for idx in range(start, len(text)):
            ch = text[idx]
            if ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
                if depth == 0:
                    try:
                        results.append(json.loads(text[start:idx + 1]))
                    except Exception:
                        pass
                    break
        start = text.rfind("{", 0, start)
    return results


def _first_key(obj: Any, keys: List[str]) -> Optional[Any]:
    if not isinstance(obj, dict):
        return None
    for key in keys:
        if key in obj and obj[key] is not None:
            return obj[key]
    # nested under common wrappers
    for wrapper in ("summary", "data", "result", "stats", "latency", "delta"):
        if isinstance(obj.get(wrapper), dict):
            found = _first_key(obj[wrapper], keys)
            if found is not None:
                return found
    return None


def parse_engine_output(text: str) -> Dict[str, Any]:
    for candidate in _extract_json_candidates(text):
        success = _first_key(candidate, ["success_rate", "successRate", "ok_rate"])
        p95 = _first_key(candidate, ["p95_ms", "p95", "p95Ms", "latency_p95_ms", "p95_latency_ms"])
        alerts = _first_key(candidate, ["alerts", "alert_count"])
        if success is not None or p95 is not None:
            return {
                "success_rate": float(success) if success is not None else None,
                "p95_ms": float(p95) if p95 is not None else None,
                "alerts": float(alerts) if alerts is not None else None,
                "raw": candidate,
            }
    return {"success_rate": None, "p95_ms": None, "alerts": None, "raw": None, "unparsed": True}


def build_command(args: argparse.Namespace, concurrency: int) -> List[str]:
    root = Path(args.repo_root).resolve()
    if args.engine == "boundary":
        cmd = [
            args.python,
            str(root / "scripts" / "runtime_boundary_stress.py"),
            "--base-url", args.base_url,
            "--concurrency", str(concurrency),
            "--rounds", str(args.rounds),
            "--scenario", args.scenario,
        ]
        if args.api_key:
            cmd += ["--api-key", args.api_key]
        if args.auth_token:
            cmd += ["--auth-token", args.auth_token]
        if args.user_id:
            cmd += ["--user-id", args.user_id]
        return cmd

    # sim engine: drive backend_sim directly so we can set concurrency + report.
    report = str(root / "target" / "form-bench" / "raw" / f"backend_sim_c{concurrency}.json")
    return [
        args.cargo, "run", "--release", "-p", "wunder-runtime",
        "--features", "sim-bins", "--bin", "backend_sim", "--",
        "--concurrency", str(concurrency),
        "--requests", str(concurrency * args.rounds),
        "--report", report,
    ]


def run_step(args: argparse.Namespace, concurrency: int) -> Dict[str, Any]:
    cmd = build_command(args, concurrency)
    display = " ".join(cmd)
    try:
        proc = subprocess.run(
            cmd,
            cwd=args.repo_root,
            capture_output=True,
            text=True,
            timeout=args.timeout_sec,
        )
    except Exception as exc:  # noqa: BLE001
        return {"concurrency": concurrency, "command": display, "error": str(exc)}

    parsed = parse_engine_output((proc.stdout or "") + "\n" + (proc.stderr or ""))
    parsed["concurrency"] = concurrency
    parsed["command"] = display
    parsed["exit_code"] = proc.returncode
    if proc.returncode != 0 and "error" not in parsed:
        parsed["error"] = f"exit code {proc.returncode}"
    return parsed


def within_budget(step: Dict[str, Any], args: argparse.Namespace) -> bool:
    if step.get("error"):
        return False
    success = step.get("success_rate")
    p95 = step.get("p95_ms")
    if success is None and p95 is None:
        return False
    if success is not None and success < args.min_success_rate:
        return False
    if p95 is not None and p95 > args.p95_budget_ms:
        return False
    if args.max_alerts is not None and step.get("alerts") is not None and step["alerts"] > args.max_alerts:
        return False
    return True


def step_search(args: argparse.Namespace) -> Dict[str, Any]:
    plan: List[int] = []
    n = args.start
    while n <= args.max:
        plan.append(n)
        n += args.step

    results: List[Dict[str, Any]] = []
    best: Optional[int] = None
    for c in plan:
        if args.dry_run:
            results.append({"concurrency": c, "command": " ".join(build_command(args, c)), "dry_run": True})
            continue
        step = run_step(args, c)
        results.append(step)
        if within_budget(step, args):
            best = c
        else:
            break
    return {"plan": plan, "best": best, "steps": results}


def emit(args: argparse.Namespace, search: Dict[str, Any]) -> None:
    value = search["best"]
    record = {
        "metric": "concurrent_agent_threads",
        "value": value,
        "unit": "sessions",
        "source": f"engine={args.engine}",
        "command": " ".join(build_command(args, args.start)) if not args.dry_run else "(dry-run)",
        "details": {
            "load_unit": LOAD_UNIT,
            "engine": args.engine,
            "base_url": args.base_url,
            "budget": {
                "p95_ms": args.p95_budget_ms,
                "min_success_rate": args.min_success_rate,
                "max_alerts": args.max_alerts,
            },
            "plan": search["plan"],
            "dry_run": args.dry_run,
            "steps": search["steps"],
        },
    }
    envelope = {
        "form": args.form_id,
        "collector": "concurrency",
        "generated": _now(),
        "records": [record],
    }
    text = json.dumps(envelope, ensure_ascii=False, indent=2)
    if args.out_json:
        out = Path(args.out_json)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(text + "\n", encoding="utf-8")
    sys.stdout.write(text + "\n")


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(description="form-bench concurrency collector")
    p.add_argument("--form-id", required=True)
    p.add_argument("--repo-root", default=".")
    p.add_argument("--out-json", default="")
    p.add_argument("--engine", choices=["boundary", "sim"], default="boundary")
    p.add_argument("--base-url", default=DEFAULT_BASE_URL)
    p.add_argument("--scenario", default="performance")
    p.add_argument("--start", type=int, default=1)
    p.add_argument("--step", type=int, default=1)
    p.add_argument("--max", type=int, default=16)
    p.add_argument("--rounds", type=int, default=3)
    p.add_argument("--p95-budget-ms", type=float, default=2000.0)
    p.add_argument("--min-success-rate", type=float, default=1.0)
    p.add_argument("--max-alerts", type=float, default=0.0)
    p.add_argument("--timeout-sec", type=int, default=600)
    p.add_argument("--python", default="python")
    p.add_argument("--cargo", default="cargo")
    p.add_argument("--api-key", default="")
    p.add_argument("--auth-token", default="")
    p.add_argument("--user-id", default="")
    p.add_argument("--dry-run", action="store_true")
    return p


def main() -> int:
    args = build_parser().parse_args()
    search = step_search(args)
    emit(args, search)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())