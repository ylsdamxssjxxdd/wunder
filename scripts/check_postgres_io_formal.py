import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description="Check PostgreSQL caller-runtime starvation")
    parser.add_argument("--statechart", type=Path, required=True)
    parser.add_argument("--out", type=Path)
    args = parser.parse_args()
    repository = Path(__file__).resolve().parents[1]
    source = repository / "docs/形式化证明/postgres-io/models"
    scratch = Path(tempfile.mkdtemp(prefix="postgres-io-formal-"))
    for filename in ("PostgresIo.tla", "PostgresIo.cfg", "PostgresIo.map.mjs"):
        shutil.copyfile(source / filename, scratch / filename)
    spec = (scratch / "PostgresIo.tla").read_text(encoding="utf-8")
    (scratch / "PostgresIoFixed.tla").write_text(
        spec.replace("MODULE PostgresIo ", "MODULE PostgresIoFixed "), encoding="utf-8")
    config = (scratch / "PostgresIo.cfg").read_text(encoding="utf-8")
    (scratch / "PostgresIoFixed.cfg").write_text(config.replace("FALSE", "TRUE"), encoding="utf-8")
    output = args.out.resolve() if args.out else scratch / "report"
    if repository == output or repository in output.parents:
        parser.error("--out must be outside the repository")
    environment = dict(os.environ)
    environment["JAVA_TOOL_OPTIONS"] = environment.get("JAVA_TOOL_OPTIONS", "") + (
        " -XX:ActiveProcessorCount=1 -Dtlc2.tool.fp.FPSet.impl=tlc2.tool.fp.MemFPSet")
    subprocess.run(["node", str(args.statechart.resolve()), "from-spec", str(scratch / "PostgresIo.tla"),
                    "--fixed", str(scratch / "PostgresIoFixed.tla"), "--expect-broken", "NoIoStarvation",
                    "--cover", "--png", "--title", "数据库 I/O 与业务运行时隔离", "--out", str(output)],
                   env=environment, check=True)
    summary = json.loads((output / "summary.json").read_text(encoding="utf-8"))
    if any(summary.get("uncited", {}).values()) or summary["chartStatus"]["wrong"] or summary["brokenProperties"]:
        raise SystemExit("Uncited conditions or unresolved rules")
    logs = list((output / "tlc").rglob("*.out"))
    if len(logs) != 6 or not any("fixed" in path.parts for path in logs):
        raise SystemExit("Missing model checks")
    for path in logs:
        text = path.read_text(encoding="utf-8", errors="replace")
        if "Model checking completed" not in text or "Exception" in text or "TLC threw" in text:
            raise SystemExit("Incomplete TLC run: " + path.name)
    print("Verified report: " + str(output))


if __name__ == "__main__":
    main()
