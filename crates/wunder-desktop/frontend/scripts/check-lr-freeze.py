"""Reproduce the L/R fast-click freeze with an external hang detector.

Hang detection: SendMessageTimeoutW(WM_NULL) — a stuck UI thread cannot answer.
Patterns tried:
  overlap-lr: left down, right down, left up, right up
  overlap-rl: right down (menu opens, modal), left down, ...
  fast-alt:   alternating full clicks with tiny gaps
"""
import ctypes
import sys
import time

import psutil
from PIL import ImageGrab
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WORK = ROOT / "temp_dir/right-click-repro"

u = ctypes.windll.user32
u.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))
u.SendMessageTimeoutW.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t, ctypes.c_ssize_t, ctypes.c_uint, ctypes.c_uint, ctypes.POINTER(ctypes.c_size_t)]
u.SendMessageTimeoutW.restype = ctypes.c_ssize_t


def responsive(hwnd, timeout_ms=1000):
    result = ctypes.c_size_t(0)
    rc = u.SendMessageTimeoutW(hwnd, 0x0000, 0, 0, 0x0002, timeout_ms, ctypes.byref(result))  # WM_NULL, SMTO_BLOCK
    return rc != 0


def find_main_window(pid):
    found = []
    WNDENUMPROC = ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.c_void_p, ctypes.c_ssize_t)

    def cb(hwnd, _lp):
        if u.IsWindowVisible(hwnd) and u.GetWindowTextLengthW(hwnd):
            wpid = ctypes.c_ulong()
            u.GetWindowThreadProcessId(hwnd, ctypes.byref(wpid))
            if wpid.value == pid:
                found.append(hwnd)
        return True

    u.EnumWindows(WNDENUMPROC(cb), 0)
    return found[0] if found else None


RD, RU, LD, LU = 0x0008, 0x0010, 0x0002, 0x0004


def ev(code):
    u.mouse_event(code, 0, 0, 0, 0)


def main():
    pid = int((WORK / "pid.txt").read_text().strip())
    hwnd = None
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        hwnd = find_main_window(pid)
        if hwnd:
            break
        time.sleep(0.2)
    assert hwnd, "no window"
    time.sleep(3.0)

    # Navigate: settings gear -> models tab (physical coords from earlier session;
    # window position is stable across launches on this machine).
    def click(x, y, settle=0.8):
        u.SetCursorPos(x, y)
        time.sleep(0.08)
        ev(LD)
        time.sleep(0.05)
        ev(LU)
        time.sleep(settle)

    click(499, 1364)  # 设置
    click(653, 704)   # 模型设置
    ImageGrab.grab().save(WORK / "combo-models.png")

    x, y = 1587, 1024  # token field center
    print("window responsive before:", responsive(hwnd), flush=True)

    def watch(step):
        if not psutil.pid_exists(pid):
            print(f"DEAD at {step}", flush=True)
            return False
        if not responsive(hwnd, 1500):
            print(f"FROZEN at {step}", flush=True)
            ImageGrab.grab().save(WORK / "combo-frozen.png")
            return False
        return True

    # Pattern A: overlapping buttons (left held while right presses)
    for i in range(20):
        u.SetCursorPos(x, y)
        ev(LD); time.sleep(0.02)
        ev(RD); time.sleep(0.03)
        ev(LU); time.sleep(0.02)
        ev(RU); time.sleep(0.08)
        if not watch(f"A{i}"):
            return 3
    print("pattern A survived", flush=True)

    # Pattern B: right (menu modal), then fast left, then right
    for i in range(20):
        u.SetCursorPos(x, y)
        ev(RD); time.sleep(0.05); ev(RU)
        time.sleep(0.04)
        ev(LD); time.sleep(0.03); ev(LU)
        time.sleep(0.04)
        ev(RD); time.sleep(0.03); ev(RU)
        time.sleep(0.06)
        if not watch(f"B{i}"):
            return 3
    print("pattern B survived", flush=True)

    # Pattern C: right held, left click while menu open, release right after
    for i in range(20):
        u.SetCursorPos(x, y)
        ev(RD); time.sleep(0.06)
        ev(LD); time.sleep(0.02); ev(LU); time.sleep(0.02)
        ev(RU); time.sleep(0.08)
        if not watch(f"C{i}"):
            return 3
    print("pattern C survived", flush=True)
    print("ALL SURVIVED", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
