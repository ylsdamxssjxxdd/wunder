import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description="Verify bounded service history outside the repository")
    parser.add_argument("--statechart", type=Path, required=True)
    parser.add_argument("--out", type=Path)
    args = parser.parse_args()
    repository = Path(__file__).resolve().parents[1]
    source = repository / "docs" / "形式化证明" / "service-history-20261027"
    scratch = Path(tempfile.mkdtemp(prefix="service-history-formal-"))
    for filename in ("ServiceHistory.tla", "ServiceHistory.cfg", "ServiceHistory.map.mjs"):
        shutil.copyfile(source / filename, scratch / filename)
    spec = (scratch / "ServiceHistory.tla").read_text(encoding="utf-8")
    (scratch / "ServiceHistoryFixed.tla").write_text(spec.replace("MODULE ServiceHistory ", "MODULE ServiceHistoryFixed "), encoding="utf-8")
    config = (scratch / "ServiceHistory.cfg").read_text(encoding="utf-8")
    (scratch / "ServiceHistoryFixed.cfg").write_text(config.replace("FALSE", "TRUE"), encoding="utf-8")
    output = args.out.resolve() if args.out else scratch / "report"
    if repository == output or repository in output.parents:
        parser.error("--out must be outside the repository")
    environment = dict(os.environ)
    environment["JAVA_TOOL_OPTIONS"] = environment.get("JAVA_TOOL_OPTIONS", "") + " -XX:ActiveProcessorCount=1 -Dtlc2.tool.fp.FPSet.impl=tlc2.tool.fp.MemFPSet"
    command = ["node", str(args.statechart.resolve()), "from-spec", str(scratch / "ServiceHistory.tla"), "--fixed", str(scratch / "ServiceHistoryFixed.tla"), "--expect-broken", "BoundedDiagnostics,MetadataNoBodyCopies,ConstantCountRead", "--cover", "--png", "--title", "服务长线程资源边界审计", "--out", str(output)]
    subprocess.run(command, env=environment, check=True)
    logs = list((output / "tlc").rglob("*.out"))
    if len(logs) != 10 or not any("fixed" in path.parts for path in logs):
        raise SystemExit("Missing fixed-model verification logs")
    for path in logs:
        text = path.read_text(encoding="utf-8", errors="replace")
        if "Model checking completed" not in text or "Exception" in text or "TLC threw" in text:
            raise SystemExit("Incomplete TLC run: " + path.name)
    summary = json.loads((output / "summary.json").read_text(encoding="utf-8"))
    if any(summary.get("uncited", {}).values()) or summary["chartStatus"]["wrong"] or summary["brokenProperties"]:
        raise SystemExit("Uncited conditions or unresolved rules")
    jar = environment.get("TLA2TOOLS")
    if jar:
        print("TLC SHA256: " + hashlib.sha256(Path(jar).read_bytes()).hexdigest())
    print("Model states: " + str(summary["modelStates"]))
    print("Verified report: " + str(output))


if __name__ == "__main__":
    main()
