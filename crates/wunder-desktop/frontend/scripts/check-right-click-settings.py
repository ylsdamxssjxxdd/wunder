"""Stage driver: launch the real desktop app with a temp home and screenshot it.

Usage: python check-right-click-settings.py stage1
"""
import ctypes
import json
import os
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
EXE = ROOT / "crates/wunder-desktop/frontend/target/debug/wunder-desktop.exe"
WORK = ROOT / "temp_dir/right-click-repro"

user32 = ctypes.windll.user32
user32.GetWindowThreadProcessId.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_ulong)]


class Rect(ctypes.Structure):
    _fields_ = [("left", ctypes.c_long), ("top", ctypes.c_long),
                ("right", ctypes.c_long), ("bottom", ctypes.c_long)]


class Point(ctypes.Structure):
    _fields_ = [("x", ctypes.c_long), ("y", ctypes.c_long)]


def find_main_window(pid):
    found = []
    WNDENUMPROC = ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.c_void_p, ctypes.c_ssize_t)

    def cb(hwnd, _lp):
        if user32.IsWindowVisible(hwnd) and user32.GetWindowTextLengthW(hwnd):
            wpid = ctypes.c_ulong()
            user32.GetWindowThreadProcessId(hwnd, ctypes.byref(wpid))
            if wpid.value == pid:
                found.append(hwnd)
        return True

    user32.EnumWindows(WNDENUMPROC(cb), 0)
    return found[0] if found else None


def launch():
    home = WORK / "home"
    (home / "config").mkdir(parents=True, exist_ok=True)
    (home / "config/wunder.yaml").write_text("{}\n", encoding="utf-8")
    log = open(WORK / "app.log", "wb")
    env = dict(os.environ, WUNDER_HOME=str(home))
    proc = subprocess.Popen([str(EXE)], stdout=log, stderr=subprocess.STDOUT, env=env)
    (WORK / "pid.txt").write_text(str(proc.pid))
    return proc


def client_origin(hwnd):
    rect = Rect()
    user32.GetClientRect(hwnd, ctypes.byref(rect))
    origin = Point(0, 0)
    user32.ClientToScreen(hwnd, ctypes.byref(origin))
    return origin, rect


def shot(hwnd, name):
    from PIL import ImageGrab
    origin, rect = client_origin(hwnd)
    path = WORK / name
    ImageGrab.grab(bbox=(origin.x, origin.y, origin.x + rect.right, origin.y + rect.bottom)).save(path)
    print(f"screenshot: {path}")
    return origin, rect


def click(x, y):
    user32.SetCursorPos(x, y)
    time.sleep(0.05)
    user32.mouse_event(0x0002, 0, 0, 0, 0)  # LEFTDOWN
    time.sleep(0.04)
    user32.mouse_event(0x0004, 0, 0, 0, 0)  # LEFTUP
    time.sleep(0.5)


def main():
    WORK.mkdir(parents=True, exist_ok=True)
    proc = launch()
    hwnd = None
    deadline = time.monotonic() + 40
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            print(f"FAIL: app exited early code={proc.returncode}")
            return 1
        hwnd = find_main_window(proc.pid)
        if hwnd:
            break
        time.sleep(0.2)
    if not hwnd:
        print("FAIL: no window")
        proc.kill()
        return 1
    user32.SetForegroundWindow(hwnd)
    time.sleep(3.0)  # let startup settle
    shot(hwnd, "01-main.png")
    print("stage1 done; pid", proc.pid)


if __name__ == "__main__":
    sys.exit(main())

def attach():
    import psutil
    pid = int((WORK / "pid.txt").read_text().strip())
    if not psutil.pid_exists(pid):
        raise SystemExit("app not running")
    hwnd = find_main_window(pid)
    if not hwnd:
        raise SystemExit("window gone")
    return pid, hwnd
