"""Drive real right-clicks into the context-menu probe window and watch for a crash."""
import ctypes
import os
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
EXE = ROOT / "crates/wunder-desktop/frontend/target/debug/examples/context_menu_probe.exe"

user32 = ctypes.windll.user32


class Rect(ctypes.Structure):
    _fields_ = [("left", ctypes.c_long), ("top", ctypes.c_long),
                ("right", ctypes.c_long), ("bottom", ctypes.c_long)]


class Point(ctypes.Structure):
    _fields_ = [("x", ctypes.c_long), ("y", ctypes.c_long)]


def find_window(pid):
    found = []
    EnumWindowsProc = ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.c_void_p, ctypes.c_ssize_t)

    def cb(hwnd, _lparam):
        if not user32.IsWindowVisible(hwnd):
            return True
        wpid = ctypes.c_ulong()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(wpid))
        if wpid.value == pid:
            found.append(hwnd)
        return True

    user32.EnumWindows(EnumWindowsProc(cb), 0)
    return found[0] if found else None


def right_click(x, y, hold=0.06):
    user32.SetCursorPos(x, y)
    user32.mouse_event(0x0008, 0, 0, 0, 0)  # RIGHTDOWN
    time.sleep(hold)
    user32.mouse_event(0x0010, 0, 0, 0, 0)  # RIGHTUP


def main():
    log_path = ROOT / "temp_dir/context-menu-probe.log"
    log_path.parent.mkdir(exist_ok=True)
    env = dict(os.environ)
    if "--no-muda" in sys.argv:
        env["SLINT_NO_MUDA"] = "1"
    with log_path.open("wb") as log:
        proc = subprocess.Popen([str(EXE)], stdout=log, stderr=subprocess.STDOUT, env=env)
    hwnd = None
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            print(f"FAIL: probe exited early, code={proc.returncode}")
            return 1
        hwnd = find_window(proc.pid)
        if hwnd:
            break
        time.sleep(0.1)
    if not hwnd:
        print("FAIL: window not found")
        proc.kill()
        return 1
    user32.ShowWindow(hwnd, 9)
    user32.SetForegroundWindow(hwnd)
    time.sleep(1.0)
    rect = Rect()
    user32.GetClientRect(hwnd, ctypes.byref(rect))
    origin = Point(0, 0)
    user32.ClientToScreen(hwnd, ctypes.byref(origin))
    cx = origin.x + 210
    cy = origin.y + 118  # inside the input field

    # Phase 1: hold the right button down and screenshot while held — proves
    # whether the menu shows on press, and whether release dismisses it.
    user32.SetCursorPos(cx, cy)
    user32.mouse_event(0x0008, 0, 0, 0, 0)  # RIGHTDOWN, keep held
    time.sleep(0.8)
    from PIL import ImageGrab
    shot = ImageGrab.grab(bbox=(cx - 60, cy - 30, cx + 320, cy + 260))
    shot_path = ROOT / "temp_dir/context-menu-probe-held.png"
    shot.save(shot_path)
    print(f"phase1-held: process alive={proc.poll() is None}, screenshot={shot_path}")
    user32.mouse_event(0x0010, 0, 0, 0, 0)  # RIGHTUP
    time.sleep(0.8)
    shot = ImageGrab.grab(bbox=(cx - 60, cy - 30, cx + 320, cy + 260))
    shot_path = ROOT / "temp_dir/context-menu-probe-released.png"
    shot.save(shot_path)
    print(f"phase1-released: process alive={proc.poll() is None}, screenshot={shot_path}")
    # dismiss any open menu
    user32.keybd_event(0x1B, 0, 0, 0)  # ESC
    user32.keybd_event(0x1B, 0, 0x0002, 0)
    time.sleep(0.5)

    # Phase 2: overlapping buttons — left held while right presses (the freeze combo).
    for i in range(10):
        user32.SetCursorPos(cx, cy)
        user32.mouse_event(0x0002, 0, 0, 0, 0)  # LEFT DOWN
        time.sleep(0.02)
        user32.mouse_event(0x0008, 0, 0, 0, 0)  # RIGHT DOWN while left held
        time.sleep(0.03)
        user32.mouse_event(0x0004, 0, 0, 0, 0)  # LEFT UP
        time.sleep(0.02)
        user32.mouse_event(0x0010, 0, 0, 0, 0)  # RIGHT UP
        time.sleep(0.08)
        if proc.poll() is not None:
            print(f"CRASH: probe died at combo {i}, exit={proc.returncode}")
            return 2
        # hang detection: a stuck UI thread cannot answer WM_NULL in time
        result = ctypes.c_size_t(0)
        hwnd_now = find_window(proc.pid)
        user32.SendMessageTimeoutW.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t, ctypes.c_ssize_t, ctypes.c_uint, ctypes.c_uint, ctypes.POINTER(ctypes.c_size_t)]
        if hwnd_now and user32.SendMessageTimeoutW(hwnd_now, 0, 0, 0, 0x0002, 1200, ctypes.byref(result)) == 0:
            print(f"FROZEN at combo {i}")
            return 3
    print("OK: probe survived 10 overlapping L/R combos")
    proc.terminate()
    return 0


if __name__ == "__main__":
    sys.exit(main())
