"""Verify no-argument native startup, restart persistence and window close in isolation."""
import argparse
import ctypes
import json
import os
from pathlib import Path
import shutil
import subprocess
import time
import psutil

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



def check(ui, directory, early=False):
    executable = directory / "desktop-ui.exe"
    if not executable.exists():
        shutil.copy2(ui, executable)
    process = None
    try:
        with (directory / "launch.log").open("ab") as log:
            process = subprocess.Popen([str(executable)], cwd=directory, stdout=log,
                stderr=subprocess.STDOUT, creationflags=subprocess.CREATE_NO_WINDOW,
                env=dict(os.environ, RAYON_NUM_THREADS="8"))
            watched = psutil.Process(process.pid)
            deadline = time.monotonic() + 30
            shown = None
            while time.monotonic() < deadline:
                if process.poll() is not None:
                    raise RuntimeError("native process exited before window close")
                assert not watched.children(), "unexpected desktop child process"
                connections = getattr(watched, "net_connections", None) or watched.connections
                assert not any(c.status == psutil.CONN_LISTEN for c in connections()), "native desktop listens on TCP"
                hwnd = visible_window(process.pid)
                if hwnd:
                    shown = shown or time.monotonic()
                    if early or time.monotonic() - shown > 6:
                        user32 = ctypes.windll.user32
                        user32.PostMessageW.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t, ctypes.c_ssize_t]
                        assert user32.PostMessageW(hwnd, 0x0010, 0, 0)
                        assert process.wait(timeout=10) == 0
                        return
                time.sleep(0.1)
            raise RuntimeError("native startup timed out")
    finally:
        if process is not None and process.poll() is None:
            process.kill()
            process.wait(timeout=10)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ui", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    config = output / "WUNDER_TEMPD/config"
    config.mkdir(parents=True)
    (config / "wunder.yaml").write_text("{}\n", encoding="utf-8")
    (config / "desktop.settings.json").write_text(json.dumps({
        "workspace_root": "", "desktop_token": "", "updated_at": 0,
        "lan_mesh": {"enabled": True},
    }), encoding="utf-8")
    check(args.ui.resolve(), output)
    assert (config / "desktop.settings.json").is_file()
    check(args.ui.resolve(), output)
    check(args.ui.resolve(), output, early=True)
    print("PASS: no-argument startup/restart/early-close; no bridge process or listener")

if __name__ == "__main__":
    main()
