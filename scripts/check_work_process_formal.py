import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description="Verify bounded work-process identity and delivery models")
    parser.add_argument("--statechart", type=Path, required=True)
    parser.add_argument("--out", type=Path)
    args = parser.parse_args()
    repository = Path(__file__).resolve().parents[1]
    source = repository / "docs" / "形式化证明" / "聊天状态一致性验证" / "models"
    scratch = Path(tempfile.mkdtemp(prefix="work-process-formal-"))
    output = args.out.resolve() if args.out else scratch / "report"
    if output == repository or repository in output.parents:
        parser.error("--out must be outside the repository")
    environment = dict(os.environ)
    environment["JAVA_TOOL_OPTIONS"] = environment.get("JAVA_TOOL_OPTIONS", "") + " -XX:ActiveProcessorCount=1 -Dtlc2.tool.fp.FPSet.impl=tlc2.tool.fp.MemFPSet"
    cases = (
        ("ProcessIdentity", "identity", "CompletedReasoningImmutable,NoTailIdentityRollback,NoDurableIdentityRollback", False),
        ("ProcessDelivery", "delivery", "FirstDeadlineNotPostponed,LiveToolVisible,ToolOrderStable", False),
        ("ThreadHead", "head", "ImmediateTerminalAgreement,NoUnverifiedSuccess,TerminalDoesNotReopen", True),
    )
    for name, report, expected, has_dedicated_fixed_model in cases:
        for extension in ("tla", "cfg", "map.mjs"):
            shutil.copyfile(str(source / (name + "." + extension)), str(scratch / (name + "." + extension)))
        fixed = scratch / (name + "Fixed.tla")
        fixed_config = scratch / (name + "Fixed.cfg")
        if has_dedicated_fixed_model:
            shutil.copyfile(str(source / (name + "Fixed.tla")), str(fixed))
            shutil.copyfile(str(source / (name + "Fixed.cfg")), str(fixed_config))
        else:
            fixed.write_text((scratch / (name + ".tla")).read_text(encoding="utf-8").replace("MODULE " + name + " ", "MODULE " + name + "Fixed "), encoding="utf-8")
            fixed_config.write_text((scratch / (name + ".cfg")).read_text(encoding="utf-8").replace("Fixed = FALSE", "Fixed = TRUE"), encoding="utf-8")
        target = output / report
        command = ["node", str(args.statechart.resolve()), "from-spec", str(scratch / (name + ".tla")), "--fixed", str(fixed), "--map", str(scratch / (name + ".map.mjs")), "--expect-broken", expected, "--out", str(target), "--png"]
        subprocess.run(command, env=environment, check=True)
        summary = json.loads((target / "summary.json").read_text(encoding="utf-8"))
        if any(summary.get("uncited", {}).values()) or summary["chartStatus"]["wrong"] or summary["brokenProperties"]:
            raise SystemExit("Unverified model: " + name)
        logs = list((target / "tlc").rglob("*.out"))
        invariants = [line.split()[1] for line in (scratch / (name + ".cfg")).read_text(encoding="utf-8").splitlines() if line.startswith("INVARIANT ")]
        required = [target / "tlc" / (name + ".out"), target / "tlc/fixed" / (name + "Fixed.out")]
        required += [target / "tlc" / (name + "-" + invariant + ".out") for invariant in invariants]
        required += [target / "tlc/fixed" / (name + "Fixed-" + invariant + ".out") for invariant in invariants]
        if any(not path.is_file() for path in required):
            raise SystemExit("Missing invariant verification: " + name)
        for path in logs:
            text = path.read_text(encoding="utf-8", errors="replace")
            if "Model checking completed" not in text or "Exception" in text or "TLC threw" in text:
                raise SystemExit("Incomplete exploration: " + path.name)
        print(name + ": " + str(summary["modelStates"]) + " model states; " + str(len(summary["classes"])) + " reproduced counterexample classes")
    print("Verified report: " + str(output))


if __name__ == "__main__":
    main()
