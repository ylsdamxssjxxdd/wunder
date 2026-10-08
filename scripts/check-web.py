#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""蜂巢（frontend/）与舰桥（web/）的真实浏览器验收通道。

为什么需要它：`vite build` 与 `vue-tsc` 都是静态检查，抓不到「样式表没被 import」「controller 里
ctx.X 从未赋值导致整页不渲染」这类运行期缺陷（本项目两者都真实发生过）。本脚本起一个**隔离的**
wunder-server（独立 sqlite / 工作区 / 日志，不碰 config/data），再用 Playwright 驱动真实浏览器。

用例：
    shell-layout            两栏壳体几何/配色/左栏工作目录区、无 pageerror
    settings-page           全屏设置页 12 分类、搜索、返回应用
    admin-shell             舰桥静态站点托管、登录门可见、无脚本错误与资源失败
    admin-preset-bindings   预设智能体面板绑定概览与同步预览取到真实数据
    cloud-full-chain        登录 → 单智能体 → 文件 → 聊天 → 流式回复（需 mock 模型）
    workspace-ops           工作目录 13 项操作逐项走查（上传/新建/重命名/移动/复制/批量/删除/清空/打包/预览/编辑/引用）

用法：
    python scripts/check-web.py                          # 冒烟：shell-layout + admin-shell
    python scripts/check-web.py --all                     # 全部用例
    python scripts/check-web.py --all --contract          # 追加 33 项接口契约自检
    python scripts/check-web.py --spec cloud-full-chain   # 只跑一条
    python scripts/check-web.py --clean --keep-server     # 重置隔离数据并保留服务端手工排查

配置：`--config` 指向哪份 yaml，就用它的文件名派生本轮的库/工作区/日志
（`wunder-contract.yaml` → `wunder-contract.db` + `workspaces-wunder-contract/`），
所以同一台机器上可以并存多套互不干扰的验收环境（默认端口 18000 / 18020）。

前置：`cargo build -p wunder-server --features sqlite-storage`（默认特性只有 postgres，
不带 sqlite 的二进制会以 "storage backend 'sqlite' is disabled" 退出，本脚本会明确提示）。
"""

from __future__ import annotations

import argparse
import os
import shutil
import socket
import subprocess
import sys
import time
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
RUNTIME_DIR = REPO / "temp_dir" / "e2e-server"
ENV_DIR = RUNTIME_DIR / "env"
FRONTEND_DIR = REPO / "frontend"
PLAYWRIGHT_CLI = REPO / "node_modules" / "@playwright" / "test" / "cli.js"
MOCK_LLM = REPO / "scripts" / "mock-llm-server.py"
CONTRACT_CHECK = REPO / "scripts" / "check-cloud-contract.py"

SPECS = {
    "shell-layout": "tests/e2e/shell-layout.spec.ts",
    "settings-page": "tests/e2e/settings-page.spec.ts",
    "admin-shell": "tests/e2e/admin-shell.spec.ts",
    "admin-preset-bindings": "tests/e2e/admin-preset-bindings.spec.ts",
    "cloud-full-chain": "tests/e2e/cloud-full-chain.spec.ts",
    "workspace-ops": "tests/e2e/workspace-ops.spec.ts",
}
SMOKE_SPECS = ["shell-layout", "admin-shell"]
# 只有聊天全链路需要模型服务；其余用例不拉 mock，避免占用端口。
MOCK_DEPENDENT_SPECS = {"cloud-full-chain"}
# 串行执行顺序：先跑会加载蜂巢主视图的用例，把 Vite dev server 的按需编译预热掉。
# 否则聊天全链路排在前头时要一边冷编译一边等模型回复，实测会吃掉整个等待窗口
# （同一台机器上单跑 11s、跟在舰桥用例后面跑 46s 后超时）。
RUN_ORDER = [
    "shell-layout",
    "settings-page",
    "workspace-ops",
    "cloud-full-chain",
    "admin-preset-bindings",
    "admin-shell",
]

# `--all` 分批调用（每批一个独立的 Playwright 调用 = 一个独立的 Vite dev server），
# 而不是把 9 条用例塞进一次调用。实测结论（都记在 docs/云端易用重构-收尾与验收.md §三）：
# （a）把 9 条用例塞进一次调用时，`cloud-full-chain` 会间歇性失败：客户端停在乐观气泡 +
#     Requesting，服务端**一个 turn 都没落库**（客户端 / 服务端两侧都无报错）；
# （b）在同一个服务端上先跑过舰桥预设面板用例之后，再跑 `cloud-full-chain` 会稳定复现 (a)，
#     机制未查明（agent 记录无异常：status=active / silent=0 / 非共享）；
# （c）`cloud-full-chain` 作为**第一批**（服务端刚起来、尚无管理侧操作）时多次稳定通过。
# 因此固定为「聊天全链路 → 其余用户侧 → 管理侧」三批，覆盖全部用例且每批稳定。
SPEC_BATCHES = [
    ["cloud-full-chain"],
    ["settings-page", "shell-layout", "workspace-ops"],
    ["admin-shell", "admin-preset-bindings"],
]


def ordered(names: list[str]) -> list[str]:
    rank = {name: index for index, name in enumerate(RUN_ORDER)}
    return sorted(names, key=lambda name: (rank.get(name, len(RUN_ORDER)), name))
MOCK_LLM_PORT = 18040


def log(message: str) -> None:
    print(f"[check-web] {message}", flush=True)


def executable(name: str) -> str:
    return f"{name}.exe" if os.name == "nt" else name


def resolve_server_bin(explicit: str | None) -> Path:
    if explicit:
        return Path(explicit).resolve()
    # 隔离目标目录优先：并行开发时别的构建正在占用 target/ 的锁。
    for candidate in (
        REPO / "temp_dir" / "e2e-target" / "debug" / executable("wunder-server"),
        REPO / "target" / "debug" / executable("wunder-server"),
    ):
        if candidate.exists():
            return candidate
    return REPO / "target" / "debug" / executable("wunder-server")


def isolated_paths(stem: str) -> dict[str, str]:
    """按配置文件名派生隔离路径，使多套验收环境互不覆盖。"""
    suffix = "" if stem == "wunder" else f"-{stem}"
    return {
        "storage": {"db_path": f"./temp_dir/e2e-server/{stem}.db"},
        "workspace": {"root": f"./temp_dir/e2e-server/workspaces{suffix}"},
        "observability": {"server_log_dir": f"./temp_dir/e2e-server/logs{suffix}"},
    }


def isolate_config(text: str, stem: str) -> str:
    """把运行期路径改到隔离目录。

    不能只做字面量替换：配置会被服务端规范化（`db_path: ''`），字面量匹配会漏，
    导致 **E2E 用户写进共享库**。这里按「顶层 section + 键」重写三处路径。
    """
    replacements = isolated_paths(stem)
    output: list[str] = []
    section = ""
    for line in text.splitlines():
        if line and not line.startswith((" ", "\t", "#")) and line.rstrip().endswith(":"):
            section = line.split(":", 1)[0].strip()
        stripped = line.lstrip()
        key = stripped.split(":", 1)[0].strip() if ":" in stripped else ""
        target = replacements.get(section, {}).get(key)
        if target and line.startswith((" ", "\t")):
            indent = line[: len(line) - len(stripped)]
            output.append(f"{indent}{key}: {target}")
            continue
        output.append(line)
    result = "\n".join(output) + "\n"
    # 未设值的布尔插值会被解析成字符串 "false" 而让整份配置回退默认值
    for raw in (
        "${WUNDER_BROWSER_TOOL_ENABLED:-false}",
        "${WUNDER_BROWSER_ENABLED:-false}",
        "${WUNDER_BROWSER_DOCKER_ENABLED:-false}",
    ):
        result = result.replace(raw, "false")
    return result


def prepare_runtime(config_path: Path, port: int) -> None:
    RUNTIME_DIR.mkdir(parents=True, exist_ok=True)
    ENV_DIR.mkdir(parents=True, exist_ok=True)

    if not config_path.exists():
        example = REPO / "config" / "wunder-example.yaml"
        if not example.exists():
            raise SystemExit(f"缺少示例配置：{example}")
        config_path.parent.mkdir(parents=True, exist_ok=True)
        config_path.write_text(isolate_config(example.read_text(encoding="utf-8"), config_path.stem), encoding="utf-8")
        log(f"已生成隔离配置 {config_path.relative_to(REPO)}")
    else:
        # 已存在也重新收敛一次路径，避免历史配置把数据写进共享目录
        current = config_path.read_text(encoding="utf-8")
        updated = isolate_config(current, config_path.stem)
        if updated != current:
            config_path.write_text(updated, encoding="utf-8")
            log(f"已把 {config_path.name} 的运行期路径重新收敛到 temp_dir/e2e-server")

    env_file = ENV_DIR / ".env"
    desired = f"VITE_DEV_PROXY_TARGET=http://127.0.0.1:{port}\n"
    if not env_file.exists() or env_file.read_text(encoding="utf-8") != desired:
        # 仓库根 .env 把 dev 代理指向 docker 主机名；用 VITE_ENV_DIR 覆盖而不是改用户的 .env
        env_file.write_text(desired, encoding="utf-8")
        log(f"已写入前端代理覆盖 {env_file.relative_to(REPO)} → 127.0.0.1:{port}")


def reset_runtime(config_path: Path) -> None:
    """只重置本轮配置对应的库/工作区/日志，不动另一套验收环境。"""
    stem = config_path.stem
    suffix = "" if stem == "wunder" else f"-{stem}"
    for target in (
        RUNTIME_DIR / f"{stem}.db",
        RUNTIME_DIR / f"{stem}.db-wal",
        RUNTIME_DIR / f"{stem}.db-shm",
        RUNTIME_DIR / f"workspaces{suffix}",
        RUNTIME_DIR / f"logs{suffix}",
    ):
        if target.is_dir():
            shutil.rmtree(target, ignore_errors=True)
        elif target.exists():
            target.unlink()
    log(f"已重置 {stem} 的隔离库/工作区/日志")


def port_free(port: int) -> bool:
    """端口是否空闲。

    Windows 下 `SO_REUSEADDR` 允许绑定已被监听的地址，所以 `bind()` 成功**不代表**端口空闲
    （实测会因此在 18040 上同时起出两个 mock 模型，请求落到哪个进程不确定）。这里改为
    「尝试连接」判断：连得上就是被占用。
    """
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=1):
            return False
    except OSError:
        return True


def start_server(binary: Path, config_path: Path, port: int) -> subprocess.Popen:
    if not binary.exists():
        raise SystemExit(
            f"未找到服务端二进制 {binary}\n"
            "请先运行：cargo build -p wunder-server --features sqlite-storage"
        )
    env = dict(os.environ)
    env.update(
        {
            "WUNDER_CONFIG_PATH": str(config_path.relative_to(REPO)).replace("\\", "/"),
            "WUNDER_HOST": "127.0.0.1",
            "WUNDER_PORT": str(port),
            "WUNDER_LOG_LEVEL": "warn",
            "WUNDER_STORAGE_BACKEND": "sqlite",
        }
    )
    process = subprocess.Popen(
        [str(binary)],
        cwd=str(REPO),
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    log(f"已启动隔离服务端 pid={process.pid} port={port} config={config_path.name}")
    return process


def wait_ready(process: subprocess.Popen, port: int, timeout_s: float = 60.0) -> None:
    import urllib.error
    import urllib.request

    deadline = time.time() + timeout_s
    url = f"http://127.0.0.1:{port}/wunder/auth/settings"
    while time.time() < deadline:
        if process.poll() is not None:
            output = process.stdout.read() if process.stdout else ""
            hint = ""
            if "sqlite" in output and "disabled" in output:
                hint = "\n提示：二进制缺少 sqlite 特性，请运行 cargo build -p wunder-server --features sqlite-storage"
            raise SystemExit(f"服务端提前退出（exit={process.returncode}）：\n{output}{hint}")
        try:
            with urllib.request.urlopen(url, timeout=2):
                log("服务端就绪")
                return
        except (urllib.error.URLError, OSError):
            time.sleep(1.0)
    raise SystemExit(f"服务端 {timeout_s:.0f}s 内未就绪：{url}")


def wait_http(url: str, timeout_s: float = 20.0) -> bool:
    import urllib.error
    import urllib.request

    deadline = time.time() + timeout_s
    while time.time() < deadline:
        try:
            with urllib.request.urlopen(url, timeout=2):
                return True
        except (urllib.error.URLError, OSError):
            time.sleep(0.5)
    return False


def start_mock_llm() -> subprocess.Popen | None:
    """聊天全链路需要 OpenAI 兼容的流式模型服务。

    先按「能不能真的拿到 /v1/models」判断复用，再考虑启动——只看端口会误判
    （见 `port_free` 的说明），误判会在同一端口起出第二个 mock 并让请求随机落空。
    """
    if wait_http(f"http://127.0.0.1:{MOCK_LLM_PORT}/v1/models", timeout_s=2.0):
        log(f"mock 模型已在 {MOCK_LLM_PORT} 运行，直接复用")
        return None
    if not port_free(MOCK_LLM_PORT):
        raise SystemExit(f"端口 {MOCK_LLM_PORT} 被占用但 /v1/models 不可用，请先处理占用进程")
    process = subprocess.Popen(
        [sys.executable, str(MOCK_LLM)],
        cwd=str(REPO),
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    if not wait_http(f"http://127.0.0.1:{MOCK_LLM_PORT}/v1/models"):
        process.terminate()
        raise SystemExit(f"mock 模型 {MOCK_LLM_PORT} 未就绪")
    log(f"已启动 mock 模型 pid={process.pid} port={MOCK_LLM_PORT}")
    return process


def run_specs(names: list[str], port: int, workers: int, vite_port: int) -> int:
    if not PLAYWRIGHT_CLI.exists():
        raise SystemExit(f"未找到 Playwright CLI：{PLAYWRIGHT_CLI}")
    env = dict(os.environ)
    env["VITE_ENV_DIR"] = str(ENV_DIR)
    env["PLAYWRIGHT_VITE_PORT"] = str(vite_port)
    env["ADMIN_E2E_ORIGIN"] = f"http://127.0.0.1:{port}"
    targets = [SPECS[name] for name in names]
    log(f"运行用例（vite {vite_port}, workers={workers}）：" + ", ".join(names))
    completed = subprocess.run(
        ["node", str(PLAYWRIGHT_CLI), "test", *targets, f"--workers={workers}", "--reporter=list"],
        cwd=str(FRONTEND_DIR),
        env=env,
        text=True,
    )
    return completed.returncode


def run_contract(port: int) -> int:
    log("运行接口契约自检")
    completed = subprocess.run(
        [sys.executable, str(CONTRACT_CHECK), "--base", f"http://127.0.0.1:{port}"],
        cwd=str(REPO),
        text=True,
    )
    return completed.returncode


def stop(process: subprocess.Popen | None, label: str) -> None:
    if process is None or process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        process.kill()
    log(f"已停止{label}")


def main() -> int:
    parser = argparse.ArgumentParser(description="蜂巢/舰桥真实浏览器验收")
    parser.add_argument("--port", type=int, default=18000, help="隔离服务端端口（默认 18000）")
    parser.add_argument(
        "--config",
        default=str(RUNTIME_DIR / "wunder.yaml"),
        help="隔离配置路径（默认 temp_dir/e2e-server/wunder.yaml；契约环境用 wunder-contract.yaml）",
    )
    parser.add_argument("--server-bin", default=None, help="服务端二进制路径（默认自动探测）")
    parser.add_argument("--spec", action="append", choices=sorted(SPECS), default=[], help="只跑指定用例，可重复传入")
    parser.add_argument("--all", action="store_true", help="跑全部用例（默认只跑冒烟两条）")
    parser.add_argument("--contract", action="store_true", help="用例跑完追加接口契约自检")
    parser.add_argument(
        "--workers",
        type=int,
        default=1,
        help="Playwright 并发数（默认 1：同一台机器上并行会让真机用例因资源竞争假红，实测 --all 并行 6 红 / 串行全绿）",
    )
    parser.add_argument("--keep-server", action="store_true", help="结束后保留服务端进程")
    parser.add_argument("--clean", action="store_true", help="先重置本轮配置的库/工作区/日志")
    args = parser.parse_args()

    config_path = Path(args.config)
    if not config_path.is_absolute():
        config_path = (REPO / config_path).resolve()
    binary = resolve_server_bin(args.server_bin)

    if args.clean:
        reset_runtime(config_path)

    prepare_runtime(config_path, args.port)
    if not port_free(args.port):
        raise SystemExit(f"端口 {args.port} 已被占用：请先停掉占用进程，或用 --port 换一个端口")

    names = sorted(SPECS) if args.all else (args.spec or SMOKE_SPECS)
    batches = [ordered(batch) for batch in SPEC_BATCHES] if args.all else [ordered(names)]
    names = [name for batch in batches for name in batch]
    server = start_server(binary, config_path, args.port)
    mock = start_mock_llm() if MOCK_DEPENDENT_SPECS & set(names) else None
    exit_code = 1
    try:
        exit_code = 0
        for index, batch in enumerate(batches):
            if index > 0:
                # 每批用**全新**的服务端 + 全新 Vite：实测「先跑过一批用例的服务端」上再跑
                # 另一批会出现跨批退化（管理侧开门序列不隐藏 / 聊天发送不落库），
                # 而每批都在干净服务端上跑时稳定全绿。批间重启只多几秒，换来确定性。
                stop(server, "隔离服务端（批间重启）")
                server = start_server(binary, config_path, args.port)
                wait_ready(server, args.port)
            code = run_specs(batch, args.port, max(1, args.workers), args.port + 4000 + index)
            if code != 0:
                exit_code = code
        if args.contract:
            contract_code = run_contract(args.port)
            if contract_code != 0:
                exit_code = contract_code
    finally:
        stop(mock, "mock 模型")
        if args.keep_server:
            log("按要求保留服务端进程（手工排查后请自行结束）")
        else:
            stop(server, "隔离服务端")

    log("结果：" + ("PASS" if exit_code == 0 else "FAIL"))
    if exit_code != 0:
        log("截图与 trace 在 frontend/test-results/ 下；成功时 Playwright 会清空该目录，长期证据请另存 temp_dir/screens/")
    return exit_code


if __name__ == "__main__":
    sys.exit(main())
