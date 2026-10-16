#!/usr/bin/env python3
"""Exercise a standalone CLI release in isolated homes, without real credentials.

Measures command completion, not the TUI's first visible frame. The executable
is copied away from the checkout to verify that bootstrap uses embedded assets.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time


def run_group(executable, root, count):
    environment = {k: v for k, v in os.environ.items() if not k.startswith("WUNDER_")}
    environment["WUNDER_HOME"] = str(root)
    started = time.perf_counter()
    processes = []
    try:
        for _ in range(count):
            processes.append(subprocess.Popen(
                [str(executable), "config", "show"], cwd=executable.parent,
                env=environment, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
                creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0,
            ))
        completed = [process.communicate(timeout=45) for process in processes]
        codes = [process.returncode for process in processes]
        errors = [stderr.decode("utf-8", errors="replace").replace(str(root), "<isolated-home>").replace(str(executable.parent), "<isolated-release>")[-600:] for _, stderr in completed]
    finally:
        for process in processes:
            if process.poll() is None:
                process.kill()
                process.wait()
    directories = ["config", "data", "logs", "skills", "user_tools",
                   "vector_knowledge", "workspace", "temp"]
    return {
        "exit_codes": codes,
        "stderr": errors,
        "command_elapsed_ms": round((time.perf_counter() - started) * 1000),
        "shared_directories": all((root / name).is_dir() for name in directories),
        "config": (root / "config/wunder.yaml").is_file(),
        "database": (root / "data/wunder.db").is_file(),
        "skill_runner": (root / "scripts/skill_runner.py").is_file(),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cli", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="local-release-") as temporary:
        root = Path(temporary)
        executable = root / args.cli.name
        shutil.copy2(args.cli, executable)
        home = root / "home"
        results = {"cold": run_group(executable, home, 1),
                   "warm": run_group(executable, home, 1)}
        for index in range(3):
            results[f"parallel_cold_{index + 1}"] = run_group(
                executable, root / f"parallel-{index}", 2)
    passed = all(all(code == 0 for code in run["exit_codes"])
                 and all(run[key] for key in ["shared_directories", "config", "database", "skill_runner"])
                 for run in results.values())
    report = {"passed": passed, "measure": "config command completion, not first frame",
              "runs": results}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(json.dumps(report))
    raise SystemExit(0 if passed else 1)


if __name__ == "__main__":
    main()
