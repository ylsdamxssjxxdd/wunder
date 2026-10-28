#!/usr/bin/env python
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


MODELS = (
    ("FailureRecovery", "NoPermanentRetry,NoHiddenTransientFallback,NoIncompleteSuccess", 18),
    ("ToolTimeout", "UnknownNeverAdvertisedRetryable,NoUnverifiedDuplicate", 8),
    ("ProviderProtocol", "NoFailedPayloadSuccess,NullSuccessRemainsSuccess,Utf8ContentPreserved,InvalidPayloadNeverSucceeds,PermanentCodeNotReclassified", 14),
)


def main():
    parser = argparse.ArgumentParser(description="Check bounded failure recovery models outside the repository")
    parser.add_argument("--statechart", type=Path, required=True)
    parser.add_argument("--out", type=Path)
    args = parser.parse_args()
    repository = Path(__file__).resolve().parents[1]
    source = repository / "docs" / "形式化证明" / "failure-recovery" / "models"
    scratch = Path(tempfile.mkdtemp(prefix="failure-recovery-formal-"))
    output = args.out.resolve() if args.out else scratch / "reports"
    if repository == output or repository in output.parents:
        parser.error("--out must be outside the repository")
    environment = dict(os.environ)
    environment["JAVA_TOOL_OPTIONS"] = environment.get("JAVA_TOOL_OPTIONS", "") + " -XX:ActiveProcessorCount=1 -Dtlc2.tool.fp.FPSet.impl=tlc2.tool.fp.MemFPSet"
    for name, broken, log_count in MODELS:
        for suffix in (".tla", ".cfg", ".map.mjs"):
            shutil.copyfile(source / (name + suffix), scratch / (name + suffix))
        spec = (scratch / (name + ".tla")).read_text(encoding="utf-8")
        (scratch / (name + "Fixed.tla")).write_text(spec.replace("MODULE " + name + " ", "MODULE " + name + "Fixed "), encoding="utf-8")
        config = (scratch / (name + ".cfg")).read_text(encoding="utf-8")
        (scratch / (name + "Fixed.cfg")).write_text(config.replace("FALSE", "TRUE"), encoding="utf-8")
        report = output / name
        command = ["node", str(args.statechart.resolve()), "from-spec", str(scratch / (name + ".tla")),
                   "--fixed", str(scratch / (name + "Fixed.tla")), "--expect-broken", broken,
                   "--cover", "--png", "--title", name, "--out", str(report)]
        subprocess.run(command, env=environment, check=True)
        logs = list((report / "tlc").rglob("*.out"))
        if len(logs) != log_count or not any("fixed" in path.parts for path in logs):
            raise SystemExit("Missing complete fixed-model verification logs: " + name)
        for path in logs:
            text = path.read_text(encoding="utf-8", errors="replace")
            if "Model checking completed" not in text or "Exception" in text or "TLC threw" in text:
                raise SystemExit("Incomplete TLC exploration: " + path.name)
        summary = json.loads((report / "summary.json").read_text(encoding="utf-8"))
        if any(summary.get("uncited", {}).values()) or summary["chartStatus"]["wrong"] or summary["brokenProperties"]:
            raise SystemExit("Uncited conditions or unresolved rules: " + name)
        print(name + " model states: " + str(summary["modelStates"]))
    jar = environment.get("TLA2TOOLS")
    if jar:
        print("TLC SHA256: " + hashlib.sha256(Path(jar).read_bytes()).hexdigest())
    print("Verified reports: " + str(output))


if __name__ == "__main__":
    main()
