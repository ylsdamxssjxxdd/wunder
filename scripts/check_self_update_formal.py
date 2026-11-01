import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description="Verify the self-update pipeline outside the repository")
    parser.add_argument("--statechart", type=Path, required=True)
    parser.add_argument("--out", type=Path)
    args = parser.parse_args()
    repository = Path(__file__).resolve().parents[1]
    source = repository / "docs" / "形式化证明" / "self-update-20261101" / "models"
    scratch = Path(tempfile.mkdtemp(prefix="self-update-formal-"))
    for filename in ("SelfUpdate.tla", "SelfUpdate.cfg", "SelfUpdate.map.mjs"):
        shutil.copyfile(source / filename, scratch / filename)
    spec = (scratch / "SelfUpdate.tla").read_text(encoding="utf-8")
    (scratch / "SelfUpdateFixed.tla").write_text(
        spec.replace("MODULE SelfUpdate ", "MODULE SelfUpdateFixed "), encoding="utf-8"
    )
    config = (scratch / "SelfUpdate.cfg").read_text(encoding="utf-8")
    (scratch / "SelfUpdateFixed.cfg").write_text(
        config.replace("Fixed = FALSE", "Fixed = TRUE"), encoding="utf-8"
    )
    output = args.out.resolve() if args.out else scratch / "report"
    if repository == output or repository in output.parents:
        parser.error("--out must be outside the repository")
    invariants = [line.split()[1] for line in config.splitlines() if line.startswith("INVARIANT ")]
    environment = dict(os.environ)
    environment["JAVA_TOOL_OPTIONS"] = environment.get("JAVA_TOOL_OPTIONS", "") + " -XX:ActiveProcessorCount=1 -Dtlc2.tool.fp.FPSet.impl=tlc2.tool.fp.MemFPSet"
    command = [
        "node",
        str(args.statechart.resolve()),
        "from-spec",
        str(scratch / "SelfUpdate.tla"),
        "--fixed",
        str(scratch / "SelfUpdateFixed.tla"),
        "--expect-broken",
        "StagedAlwaysVerified",
        "--cover",
        "--png",
        "--title",
        "蛹在线更新机制审计",
        "--out",
        str(output),
    ]
    subprocess.run(command, env=environment, check=True)
    logs = list((output / "tlc").rglob("*.out"))
    expected = 2 * (len(invariants) + 1)
    if len(logs) != expected or not any("fixed" in path.parts for path in logs):
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
