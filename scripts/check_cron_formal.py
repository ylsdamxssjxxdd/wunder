#!/usr/bin/env python
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description="Run bounded cron lifecycle verification outside the repository")
    parser.add_argument("--statechart", type=Path, required=True)
    parser.add_argument("--out", type=Path)
    args = parser.parse_args()
    source = Path(__file__).resolve().parents[1] / "docs" / "形式化证明" / "cron" / "models"
    scratch = Path(tempfile.mkdtemp(prefix="cron-formal-"))
    for filename in ("CronLifecycle.tla", "CronLifecycle.cfg", "CronLifecycleFixed.tla", "CronLifecycleFixed.cfg", "CronLifecycle.map.mjs"):
        shutil.copyfile(str(source / filename), str(scratch / filename))
    output = args.out.resolve() if args.out else scratch / "report"
    repository = Path(__file__).resolve().parents[1]
    if repository == output or repository in output.parents:
        parser.error("--out must be outside the repository")
    command = ["node", str(args.statechart.resolve()), "from-spec", str(scratch / "CronLifecycle.tla"), "--fixed", str(scratch / "CronLifecycleFixed.tla"), "--expect-broken", "NoResurrection,ExclusiveClaim,NoStaleCompletion,AtomicHistory", "--title", "定时任务并发状态审计", "--cover", "--png", "--out", str(output)]
    environment = dict(os.environ)
    environment["JAVA_TOOL_OPTIONS"] = environment.get("JAVA_TOOL_OPTIONS", "") + " -XX:ActiveProcessorCount=1 -Dtlc2.tool.fp.FPSet.impl=tlc2.tool.fp.MemFPSet"
    result = subprocess.run(command, env=environment, check=False)
    if result.returncode:
        raise SystemExit(result.returncode)
    logs = list((output / "tlc").rglob("*.out"))
    if not logs or not any("fixed" in path.parts for path in logs):
        raise SystemExit("Missing fixed-model verification logs")
    for path in logs:
        text = path.read_text(encoding="utf-8", errors="replace")
        if "Exception" in text or "TLC threw" in text or "Model checking completed" not in text:
            raise SystemExit("Incomplete TLC exploration: " + path.name)
    summary = json.loads((output / "summary.json").read_text(encoding="utf-8"))
    if any(summary.get("uncited", {}).values()):
        raise SystemExit("Model contains uncited action conditions")
    if summary["chartStatus"]["wrong"] or summary["brokenProperties"]:
        raise SystemExit("Fixed model still violates a checked rule")
    jar = environment.get("TLA2TOOLS")
    if jar:
        print("TLC SHA256: " + hashlib.sha256(Path(jar).read_bytes()).hexdigest())
    print("Verified report: " + str(output))
    print("Model states: " + str(summary["modelStates"]))


if __name__ == "__main__":
    main()
