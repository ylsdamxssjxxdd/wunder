#!/usr/bin/env python
from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
import time
from dataclasses import asdict, dataclass
from datetime import datetime
from pathlib import Path

DEFAULT_PROBE_PROMPT = "Reply with READY only."
SAMPLE_DIFF = """diff --git a/src/main.rs b/src/main.rs
index 1111111..2222222 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,2 +1,5 @@
-fn old() {}
+fn new() {
+    println!("hi");
+}
diff --git a/README.md b/README.md
new file mode 100644
index 0000000..3333333
--- /dev/null
+++ b/README.md
@@ -0,0 +1,2 @@
+# Demo
+text
"""


@dataclass
class CommandResult:
    name: str
    exit_code: int
    duration_s: float
    cwd: str
    output_path: str
    timed_out: bool


@dataclass
class VerificationSummary:
    cargo_test_ok: bool = False
    cargo_build_ok: bool = False
    sample_table_ok: bool = False
    sample_json_ok: bool = False


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run wunder-cli end-to-end smoke tests against a real model.")
    parser.add_argument("--model", default="qwen3.5-122b", help="Configured wunder model name.")
    parser.add_argument(
        "--seed-config",
        default="data/config/wunder.override.yaml",
        help="Config file copied into <temp-root>/config/wunder.override.yaml before the run.",
    )
    parser.add_argument(
        "--temp-root",
        default="temp_dir/cli-e2e",
        help="Dedicated temp root used by wunder-cli for this smoke run.",
    )
    parser.add_argument(
        "--prompt-file",
        default="scripts/prompts/wunder_cli_long_task_diff_lens.txt",
        help="Long-task prompt file.",
    )
    parser.add_argument(
        "--approval-mode",
        default="full_auto",
        choices=["suggest", "auto_edit", "full_auto"],
        help="Approval mode passed to wunder-cli.",
    )
    parser.add_argument("--lang", default="en-US", help="Language override passed to wunder-cli.")
    parser.add_argument("--skip-long-task", action="store_true", help="Only run the connectivity probe.")
    parser.add_argument("--build-release", action="store_true", help="Build target/release/wunder-cli before the smoke test.")
    parser.add_argument("--clean", action="store_true", help="Delete the temp root before the run.")
    parser.add_argument("--timeout-probe", type=int, default=180, help="Probe timeout in seconds.")
    parser.add_argument("--timeout-long", type=int, default=1200, help="Long-task timeout in seconds.")
    parser.add_argument("--timeout-verify", type=int, default=300, help="Verification command timeout in seconds.")
    return parser.parse_args()


def repo_root() -> Path:
    return Path(__file__).resolve().parents[1]


def find_cli_binary(root: Path) -> Path:
    candidates = [root / "target" / "release" / "wunder-cli.exe", root / "target" / "release" / "wunder-cli"]
    for candidate in candidates:
        if candidate.is_file():
            return candidate
    raise FileNotFoundError("target/release/wunder-cli(.exe) not found; run with --build-release first")


def run_command(
    name: str,
    command: list[str],
    cwd: Path,
    output_path: Path,
    timeout_s: int,
    input_text: str | None = None,
) -> CommandResult:
    print(f"[run] {name}: {' '.join(command)}")
    output_path.parent.mkdir(parents=True, exist_ok=True)
    started = time.perf_counter()
    timed_out = False
    try:
        completed = subprocess.run(
            command,
            cwd=cwd,
            input=input_text,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout_s,
            check=False,
        )
        exit_code = completed.returncode
        output = completed.stdout or ""
    except subprocess.TimeoutExpired as exc:
        timed_out = True
        exit_code = 124
        output = (exc.stdout or "") if isinstance(exc.stdout, str) else (exc.stdout or b"").decode("utf-8", "replace")
    duration_s = round(time.perf_counter() - started, 3)
    output_path.write_text(output, encoding="utf-8")
    print(f"[done] {name}: exit={exit_code} duration={duration_s:.3f}s log={output_path}")
    return CommandResult(
        name=name,
        exit_code=exit_code,
        duration_s=duration_s,
        cwd=str(cwd),
        output_path=str(output_path),
        timed_out=timed_out,
    )


def seed_override_config(root: Path, temp_root: Path, seed_config: Path) -> Path:
    config_dir = temp_root / "config"
    config_dir.mkdir(parents=True, exist_ok=True)
    override_path = config_dir / "wunder.override.yaml"
    shutil.copyfile(seed_config, override_path)
    return override_path


def find_generated_project(task_root: Path) -> Path | None:
    candidates: list[Path] = []
    for cargo_toml in task_root.rglob("Cargo.toml"):
        if "target" in cargo_toml.parts:
            continue
        candidates.append(cargo_toml.parent)
    if not candidates:
        return None
    return min(candidates, key=lambda path: (len(path.parts), len(str(path))))


def find_generated_binary(project_root: Path) -> Path | None:
    release_dir = project_root / "target" / "release"
    if not release_dir.is_dir():
        return None

    candidate_names: list[str] = []
    cargo_toml = project_root / "Cargo.toml"
    if cargo_toml.is_file():
        for line in cargo_toml.read_text(encoding="utf-8").splitlines():
            stripped = line.strip()
            if not stripped.startswith("name") or "=" not in stripped:
                continue
            package_name = stripped.split("=", 1)[1].strip().strip('\"')
            if package_name:
                candidate_names.append(package_name)
                candidate_names.append(package_name.replace("-", "_"))
            break

    executable_names = []
    for name in candidate_names:
        executable_names.append(f"{name}.exe" if sys.platform.startswith("win") else name)
        executable_names.append(name)
        executable_names.append(f"{name}.exe")

    for name in executable_names:
        candidate = release_dir / name
        if candidate.is_file():
            return candidate

    for candidate in release_dir.iterdir():
        if not candidate.is_file():
            continue
        if sys.platform.startswith("win"):
            if candidate.suffix.lower() == ".exe":
                return candidate
            continue
        if candidate.stat().st_mode & 0o111:
            return candidate
    return None


def read_prompt(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def last_non_empty_line(text: str) -> str:
    for line in reversed(text.splitlines()):
        stripped = line.strip()
        if stripped:
            return stripped
    return ""


def summarize_observations(long_output: str) -> list[str]:
    observations: list[str] = []
    if "InvalidEndOfLine" in long_output or "??????????????" in long_output:
        observations.append("PowerShell execute_command does not support &&; the model must switch to ; or multi-line commands.")
    if ("All 8 tests passed!" in long_output or "test result: ok." in long_output) and (
        "?? (exit=1)" in long_output or "failed (exit=1)" in long_output
    ):
        observations.append("PowerShell stderr merging can produce false failures; cargo may succeed while the transcript still shows exit=1.")
    if '"bytes":' in long_output or '"items":[' in long_output or '"content":' in long_output:
        observations.append("Some built-in tool results still render as raw JSON in the transcript; this remains a Codex-alignment gap.")
    last_line = last_non_empty_line(long_output)
    if last_line.endswith("<empty>") or '"items":[' in last_line or '"content":' in last_line or '"bytes":' in last_line:
        observations.append("The long task ended right after the last tool call without a natural-language closing summary.")
    return observations


def main() -> int:
    args = parse_args()
    root = repo_root()
    temp_root = (root / args.temp_root).resolve()
    seed_config = (root / args.seed_config).resolve()
    prompt_file = (root / args.prompt_file).resolve()

    if args.clean and temp_root.exists():
        shutil.rmtree(temp_root)
    temp_root.mkdir(parents=True, exist_ok=True)
    seed_override_config(root, temp_root, seed_config)

    run_id = datetime.now().strftime("%Y%m%d-%H%M%S")
    run_root = temp_root / "runs" / run_id
    workspace_root = run_root / "workspace"
    probe_root = workspace_root / "probe"
    task_root = workspace_root / "long-task"
    logs_root = run_root / "logs"
    probe_root.mkdir(parents=True, exist_ok=True)
    task_root.mkdir(parents=True, exist_ok=True)
    logs_root.mkdir(parents=True, exist_ok=True)

    if args.build_release:
        build_log = logs_root / "build-wunder-cli.log"
        build_result = run_command(
            name="build_wunder_cli_release",
            command=["cargo", "build", "--release", "--bin", "wunder-cli"],
            cwd=root,
            output_path=build_log,
            timeout_s=args.timeout_verify,
        )
        if build_result.exit_code != 0:
            summary = {
                "run_id": run_id,
                "status": "failed",
                "reason": "build_wunder_cli_release failed",
                "commands": [asdict(build_result)],
                "run_root": str(run_root),
            }
            (run_root / "summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2), encoding="utf-8")
            print(json.dumps(summary, ensure_ascii=False, indent=2))
            return 1

    cli_binary = find_cli_binary(root)
    common_args = [
        str(cli_binary),
        "--temp-root",
        str(temp_root),
        "-m",
        args.model,
        "--approval-mode",
        args.approval_mode,
        "--lang",
        args.lang,
    ]

    command_results: list[CommandResult] = []
    probe_result = run_command(
        name="probe",
        command=common_args + ["--no-stream", DEFAULT_PROBE_PROMPT],
        cwd=probe_root,
        output_path=logs_root / "probe.log",
        timeout_s=args.timeout_probe,
    )
    command_results.append(probe_result)

    long_result: CommandResult | None = None
    verification = VerificationSummary()
    generated_project: Path | None = None
    long_observations: list[str] = []

    if not args.skip_long_task and probe_result.exit_code == 0:
        long_prompt = read_prompt(prompt_file)
        long_result = run_command(
            name="long_task",
            command=common_args + [long_prompt],
            cwd=task_root,
            output_path=logs_root / "long-task.log",
            timeout_s=args.timeout_long,
        )
        command_results.append(long_result)
        long_output = Path(long_result.output_path).read_text(encoding="utf-8")
        long_observations = summarize_observations(long_output)
        generated_project = find_generated_project(task_root)

        if generated_project is not None:
            cargo_test_result = run_command(
                name="verify_cargo_test",
                command=["cargo", "test"],
                cwd=generated_project,
                output_path=logs_root / "verify-cargo-test.log",
                timeout_s=args.timeout_verify,
            )
            command_results.append(cargo_test_result)
            verification.cargo_test_ok = cargo_test_result.exit_code == 0

            cargo_build_result = run_command(
                name="verify_cargo_build_release",
                command=["cargo", "build", "--release"],
                cwd=generated_project,
                output_path=logs_root / "verify-cargo-build-release.log",
                timeout_s=args.timeout_verify,
            )
            command_results.append(cargo_build_result)
            verification.cargo_build_ok = cargo_build_result.exit_code == 0

            sample_diff = generated_project / "sample.diff"
            sample_diff.write_text(SAMPLE_DIFF, encoding="utf-8")
            generated_binary = find_generated_binary(generated_project)
            if generated_binary is not None:
                sample_table_result = run_command(
                    name="verify_sample_table",
                    command=[str(generated_binary), "--file", str(sample_diff)],
                    cwd=generated_project,
                    output_path=logs_root / "verify-sample-table.log",
                    timeout_s=args.timeout_verify,
                )
                command_results.append(sample_table_result)
                sample_table_output = Path(sample_table_result.output_path).read_text(encoding="utf-8")
                verification.sample_table_ok = sample_table_result.exit_code == 0 and ("Total" in sample_table_output or "TOTAL" in sample_table_output)

                sample_json_result = run_command(
                    name="verify_sample_json",
                    command=[str(generated_binary), "--json"],
                    cwd=generated_project,
                    output_path=logs_root / "verify-sample-json.log",
                    timeout_s=args.timeout_verify,
                    input_text=SAMPLE_DIFF,
                )
                command_results.append(sample_json_result)
                try:
                    sample_json = json.loads(Path(sample_json_result.output_path).read_text(encoding="utf-8"))
                except json.JSONDecodeError:
                    sample_json = None
                verification.sample_json_ok = sample_json_result.exit_code == 0 and (
                    (isinstance(sample_json, dict) and sample_json.get("total_added", 0) > 0)
                    or (isinstance(sample_json, list) and any(item.get("added", 0) > 0 for item in sample_json if isinstance(item, dict)))
                )

    status = "passed"
    if probe_result.exit_code != 0:
        status = "failed"
    if long_result is not None and long_result.exit_code != 0:
        status = "failed"
    if generated_project is None and not args.skip_long_task:
        status = "failed"
    if not args.skip_long_task and generated_project is not None:
        if not all(asdict(verification).values()):
            status = "failed"

    summary = {
        "run_id": run_id,
        "status": status,
        "model": args.model,
        "temp_root": str(temp_root),
        "run_root": str(run_root),
        "prompt_file": str(prompt_file),
        "seed_config": str(seed_config),
        "generated_project": str(generated_project) if generated_project else None,
        "probe_ok": probe_result.exit_code == 0,
        "long_task_ok": None if long_result is None else long_result.exit_code == 0,
        "verification": asdict(verification),
        "observations": long_observations,
        "commands": [asdict(item) for item in command_results],
    }
    summary_path = run_root / "summary.json"
    summary_path.write_text(json.dumps(summary, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(summary, ensure_ascii=False, indent=2))
    return 0 if status == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
