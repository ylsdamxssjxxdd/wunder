import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description="Verify the win32 message-frame reentry discipline outside the repository")
    parser.add_argument("--statechart", type=Path, required=True)
    parser.add_argument("--out", type=Path)
    args = parser.parse_args()
    repository = Path(__file__).resolve().parents[1]
    source = repository / "docs" / "形式化证明" / "win32-reentry-20261104" / "models"
    scratch = Path(tempfile.mkdtemp(prefix="win32-reentry-formal-"))
    name = "Win32Reentry"
    for extension in ("tla", "cfg", "map.mjs"):
        shutil.copyfile(source / (name + "." + extension), scratch / (name + "." + extension))
    fixed = scratch / (name + "Fixed.tla")
    fixed_config = scratch / (name + "Fixed.cfg")
    fixed.write_text((scratch / (name + ".tla")).read_text(encoding="utf-8").replace("MODULE " + name + " ", "MODULE " + name + "Fixed "), encoding="utf-8")
    fixed_config.write_text((scratch / (name + ".cfg")).read_text(encoding="utf-8").replace("Fixed = FALSE", "Fixed = TRUE"), encoding="utf-8")
    output = args.out.resolve() if args.out else scratch / "report"
    if repository == output or repository in output.parents:
        parser.error("--out must be outside the repository")
    environment = dict(os.environ)
    environment["JAVA_TOOL_OPTIONS"] = environment.get("JAVA_TOOL_OPTIONS", "") + " -XX:ActiveProcessorCount=1 -Dtlc2.tool.fp.FPSet.impl=tlc2.tool.fp.MemFPSet"
    broken = ["NoImeWriteUnderImeFrame", "ImeWriteAtFrameBoundary", "FrameStackBounded", "NoLockWedge"]
    command = [
        "node", str(args.statechart.resolve()), "from-spec", str(scratch / (name + ".tla")),
        "--fixed", str(fixed), "--fixed-config", str(fixed_config),
        "--map", str(scratch / (name + ".map.mjs")),
        *(flag for rule in broken for flag in ("--expect-broken", rule)),
        "--out", str(output), "--png", "--cover",
        "--title", "蛹UI线程消息帧重入审计",
    ]
    subprocess.run(command, env=environment, check=True)
    summary = json.loads((output / "summary.json").read_text(encoding="utf-8"))
    if any(summary.get("uncited", {}).values()) or summary["chartStatus"]["wrong"] or summary["brokenProperties"]:
        raise SystemExit("Unverified model: " + name)
    logs = list((output / "tlc").rglob("*.out"))
    invariants = [line.split()[1] for line in (scratch / (name + ".cfg")).read_text(encoding="utf-8").splitlines() if line.startswith("INVARIANT ")]
    required = [output / "tlc" / (name + ".out"), output / "tlc/fixed" / (name + "Fixed.out")]
    required += [output / "tlc" / (name + "-" + invariant + ".out") for invariant in invariants]
    required += [output / "tlc/fixed" / (name + "Fixed-" + invariant + ".out") for invariant in invariants]
    if any(not path.is_file() for path in required):
        raise SystemExit("Missing invariant verification: " + name)
    for path in logs:
        text = path.read_text(encoding="utf-8", errors="replace")
        if "Model checking completed" not in text or "Exception" in text or "TLC threw" in text:
            raise SystemExit("Incomplete exploration: " + path.name)
    reproduced = {entry["invariant"] for entry in summary["classes"]}
    if reproduced != set(broken):
        raise SystemExit("Counterexample classes do not match the four family rules: " + str(reproduced))
    jar = environment.get("TLA2TOOLS")
    if jar:
        print("TLC SHA256: " + hashlib.sha256(Path(jar).read_bytes()).hexdigest())
    print(name + ": " + str(summary["modelStates"]) + " model states; " + str(len(summary["classes"])) + " reproduced counterexample classes; fixed chart wrong states: " + str(summary["chartStatus"]["wrong"]))
    print("Verified report: " + str(output))


if __name__ == "__main__":
    main()
