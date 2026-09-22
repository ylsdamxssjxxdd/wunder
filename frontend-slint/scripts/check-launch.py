"""Check native Windows close events and bridge ownership in isolated directories.

Requires psutil; runs a real desktop bridge without calling an external model.
"""
import argparse
import ctypes
import json
import os
from pathlib import Path
import shutil
import subprocess
import time
from urllib.request import urlopen

import psutil


def wait_for(probe, message, timeout=60):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = probe()
        if result:
            return result
        time.sleep(0.05)
    raise RuntimeError(message)


def visible_window(pid):
    user32 = ctypes.windll.user32
    user32.GetWindowThreadProcessId.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_ulong)]
    user32.IsWindowVisible.argtypes = [ctypes.c_void_p]
    user32.GetWindowTextLengthW.argtypes = [ctypes.c_void_p]
    callback_type = ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.c_void_p, ctypes.c_void_p)
    windows = []

    @callback_type
    def visit(hwnd, _):
        owner = ctypes.c_ulong()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value == pid and user32.IsWindowVisible(hwnd) and user32.GetWindowTextLengthW(hwnd):
            windows.append(hwnd)
        return True

    user32.EnumWindows(visit, 0)
    return windows[0] if windows else None


def bridge_base(process):
    try:
        for connection in process.connections(kind="tcp"):
            if connection.status == psutil.CONN_LISTEN and connection.laddr.ip == "127.0.0.1":
                base = f"http://127.0.0.1:{connection.laddr.port}"
                try:
                    with urlopen(base + "/config.json", timeout=0.5) as response:
                        if json.load(response).get("desktop_token"):
                            return base
                except (OSError, ValueError):
                    pass
    except psutil.NoSuchProcess:
        raise RuntimeError("bridge exited before readiness") from None
    return None


def check_case(ui, bridge, directory, mode):
    directory.mkdir()
    # Copies isolate runtime defaults and avoid sharing writable build outputs.
    local_ui = directory / "desktop-ui.exe"
    local_bridge = directory / "wunder-desktop-bridge.exe"
    shutil.copy2(ui, local_ui)
    shutil.copy2(bridge, local_bridge)
    runtime = directory / "WUNDER_TEMPD"
    config_dir = runtime / "config"
    config_dir.mkdir(parents=True)
    (config_dir / "wunder.yaml").write_text("{}\n", encoding="utf-8")
    (config_dir / "desktop.settings.json").write_text(json.dumps({
        "workspace_root": "", "desktop_token": "", "updated_at": 0,
        "lan_mesh": {"enabled": False},
    }), encoding="utf-8")
    environment = dict(os.environ, TOKIO_WORKER_THREADS="8", RAYON_NUM_THREADS="8")
    ui_process = None
    bridge_process = None
    owned_children = []
    try:
        with (directory / "process.log").open("wb") as log:
            arguments = [str(local_ui)]
            if mode == "external":
                bridge_process = subprocess.Popen([
                    str(local_bridge), "--port", "0", "--temp-root", str(runtime),
                ], cwd=directory, env=environment, creationflags=subprocess.CREATE_NO_WINDOW,
                    stdout=log, stderr=subprocess.STDOUT)
                watched_bridge = psutil.Process(bridge_process.pid)
                base = wait_for(lambda: bridge_base(watched_bridge), "external bridge not ready")
                arguments += ["--connect", base]
            ui_process = subprocess.Popen(arguments, cwd=directory, env=environment,
                                          stdout=log, stderr=subprocess.STDOUT)
            watched_ui = psutil.Process(ui_process.pid)
            hwnd = wait_for(lambda: visible_window(ui_process.pid), "native window not shown")
            if mode == "owned":
                owned_children = wait_for(watched_ui.children, "owned bridge not spawned")
                wait_for(lambda: bridge_base(owned_children[0]), "owned bridge not ready")
                time.sleep(1)
            else:
                owned_children = watched_ui.children(recursive=True)
            user32 = ctypes.windll.user32
            user32.PostMessageW.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t, ctypes.c_ssize_t]
            if not user32.PostMessageW(hwnd, 0x0010, 0, 0):  # WM_CLOSE, like the titlebar close button.
                raise RuntimeError("native close message failed")
            if ui_process.wait(timeout=10) != 0:
                raise RuntimeError("desktop exited with an error")
            _, alive = psutil.wait_procs(owned_children, timeout=5)
            if alive:
                raise RuntimeError("owned bridge survived window close")
            if bridge_process is not None:
                if bridge_process.poll() is not None or not bridge_base(watched_bridge):
                    raise RuntimeError("closing UI affected external bridge")
    finally:
        if ui_process is not None and ui_process.poll() is None:
            try:
                owned_children.extend(psutil.Process(ui_process.pid).children(recursive=True))
            except psutil.NoSuchProcess:
                pass
            ui_process.kill()
            ui_process.wait(timeout=10)
        for child in owned_children:
            try:
                child.kill()
                child.wait(timeout=10)
            except psutil.NoSuchProcess:
                pass
        if bridge_process is not None and bridge_process.poll() is None:
            bridge_process.kill()
            bridge_process.wait(timeout=10)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ui", type=Path, required=True)
    parser.add_argument("--bridge", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if os.name != "nt":
        parser.error("native window close checks require Windows")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    results = []
    for mode in ["owned", "early-close", "external"]:
        check_case(args.ui.resolve(), args.bridge.resolve(), output / mode, mode)
        results.append(mode)
        print(f"PASS: {mode} native window close and bridge ownership", flush=True)
    (output / "launch.json").write_text(json.dumps({"passed": results}), encoding="utf-8")


if __name__ == "__main__":
    main()
