#!/usr/bin/env python3
"""验证"思考行结算"形式化模型（TLC 穷举）。

缺陷模型 ThoughtStream 必须能复现三类反例：
  1. 工具结果帧先到，思考行不结算；
  2. 迟到思考帧被丢弃（文本永久缺失）；
  3. 已结算的工具行之后的思考行一直转圈（截图形态）。

修复模型 ThoughtStreamFixed 必须让同样的不变式全部成立。

状态、日志与报告一律写到仓库外；仓库内只保留模型、配置与结论摘要。
"""

from __future__ import annotations

import argparse
import glob
import os
import shutil
import subprocess
import tempfile
from pathlib import Path

# (模型, 不变式, 是否应当被违反)
CASES = (
    ("ThoughtStream", "NoRunningBehindAction", True),
    ("ThoughtStream", "NoStaleRunning", True),
    ("ThoughtStream", "TextKept", True),
    ("ThoughtStreamFixed", "TypeOK", False),
    ("ThoughtStreamFixed", "NoRunningBehindAction", False),
    ("ThoughtStreamFixed", "NoReasonAfterCalled", False),
    ("ThoughtStreamFixed", "TextKept", False),
)

CONSTANTS = {
    "Rounds": "{1, 2}",
    "Tools": "{1, 2}",
    "MaxSeen": "3",
}

CONFIG_TEMPLATE = """SPECIFICATION Spec
CONSTANTS
  Rounds = {Rounds}
  Tools = {Tools}
  MaxSeen = {MaxSeen}
INVARIANT {invariant}
"""


def find_jar(explicit: str | None) -> Path:
    if explicit:
        return Path(explicit).resolve()
    environment = os.environ.get("WUNDER_TLA_JAR")
    if environment:
        return Path(environment).resolve()
    for pattern in (
        "/tmp/wunder-formal-tools/tla2tools-*.jar",
        str(Path.home() / ".wunder/tools/tla2tools-*.jar"),
    ):
        found = sorted(glob.glob(pattern))
        if found:
            return Path(found[-1])
    raise SystemExit("找不到 tla2tools jar；可用 --jar 或 WUNDER_TLA_JAR 指定")


def main() -> None:
    parser = argparse.ArgumentParser(description="Verify thought-row settlement models")
    parser.add_argument("--jar", help="tla2tools jar 路径")
    parser.add_argument("--out", type=Path, help="报告输出目录（必须在仓库外）")
    parser.add_argument("--keep", action="store_true", help="保留临时工作目录")
    args = parser.parse_args()

    repository = Path(__file__).resolve().parents[1]
    models = repository / "docs" / "形式化证明" / "思考流一致性验证" / "models"
    jar = find_jar(args.jar)
    scratch = Path(tempfile.mkdtemp(prefix="thought-stream-formal-"))
    report = args.out.resolve() if args.out else scratch / "report"
    if report == repository or repository in report.parents:
        parser.error("--out 必须位于仓库之外")
    report.mkdir(parents=True, exist_ok=True)

    environment = dict(os.environ)
    environment["JAVA_TOOL_OPTIONS"] = (
        environment.get("JAVA_TOOL_OPTIONS", "") + " -XX:ActiveProcessorCount=1"
    )

    failures = 0
    for model, invariant, expected_broken in CASES:
        source = models / (model + ".tla")
        if not source.is_file():
            raise SystemExit("缺少模型：" + str(source))
        work = report / (model + "-" + invariant)
        work.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, work / (model + ".tla"))
        (work / (model + ".cfg")).write_text(
            CONFIG_TEMPLATE.format(invariant=invariant, **CONSTANTS), encoding="utf-8"
        )
        result = subprocess.run(
            [
                "java",
                "-cp",
                str(jar),
                "tlc2.TLC",
                "-config",
                model + ".cfg",
                "-workers",
                "1",
                "-deadlock",
                "-metadir",
                str(work / "states"),
                model + ".tla",
            ],
            cwd=work,
            env=environment,
            capture_output=True,
            text=True,
            timeout=900,
        )
        output = result.stdout + result.stderr
        (work / "tlc.out").write_text(output, encoding="utf-8")
        violated = "is violated" in output
        incomplete = not violated and "Model checking completed" not in output
        if incomplete:
            failures += 1
            print(f"[INCOMPLETE] {model}/{invariant}: 探索未完成，见 {work / 'tlc.out'}")
            continue
        if violated != expected_broken:
            failures += 1
            print(f"[UNEXPECTED] {model}/{invariant}: violated={violated} expected={expected_broken}")
            continue
        states = ""
        for line in output.splitlines():
            if "distinct states found" in line:
                states = line.strip()
        verdict = "反例已复现" if expected_broken else "不变量成立"
        print(f"[OK] {model}/{invariant}: {verdict} {states}")

    if args.keep:
        print("工作目录：" + str(scratch))
    else:
        shutil.rmtree(scratch, ignore_errors=True)
    print("报告目录：" + str(report))
    if failures:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
