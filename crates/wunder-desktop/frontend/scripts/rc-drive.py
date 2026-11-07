"""Drive the running repro app with correct physical-pixel coordinates.

The python process must be DPI-aware so screenshots, window rects and
SetCursorPos all agree on physical pixels; the app itself is DPI-unaware and
Windows bitmap-scales it, which otherwise shifts every click.
"""
import ctypes
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
WORK = ROOT / "temp_dir/right-click-repro"

DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 = ctypes.c_void_p(-4)
ctypes.windll.user32.SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)

user32 = ctypes.windll.user32
kernel32 = ctypes.windll.kernel32
user32.GetWindowThreadProcessId.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_ulong)]
user32.GetWindowThreadProcessId.restype = ctypes.c_ulong


class Rect(ctypes.Structure):
    _fields_ = [("left", ctypes.c_long), ("top", ctypes.c_long),
                ("right", ctypes.c_long), ("bottom", ctypes.c_long)]


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


def attach():
    import psutil
    pid = int((WORK / "pid.txt").read_text().strip())
    alive = psutil.pid_exists(pid)
    hwnd = find_main_window(pid) if alive else None
    return pid, alive, hwnd


def foreground(hwnd):
    """Reliable SetForegroundWindow from a background console."""
    our_thread = kernel32.GetCurrentThreadId()
    app_thread = user32.GetWindowThreadProcessId(hwnd, None)
    foreground_thread = user32.GetWindowThreadProcessId(user32.GetForegroundWindow(), None)
    attached = []
    for tid in {our_thread, app_thread, foreground_thread}:
        if tid and tid != our_thread:
            if user32.AttachThreadInput(our_thread, tid, True):
                attached.append(tid)
    user32.ShowWindow(hwnd, 9)  # SW_RESTORE
    user32.BringWindowToTop(hwnd)
    user32.SetForegroundWindow(hwnd)
    user32.SetActiveWindow(hwnd)
    for tid in attached:
        user32.AttachThreadInput(our_thread, tid, False)
    time.sleep(0.3)


def app_rect(hwnd):
    r = Rect()
    user32.GetWindowRect(hwnd, ctypes.byref(r))
    return r


def do_click(x, y, button="left", hold=0.05, settle=0.4):
    user32.SetCursorPos(x, y)
    time.sleep(0.06)
    down, up = (0x0002, 0x0004) if button == "left" else (0x0008, 0x0010)
    user32.mouse_event(down, 0, 0, 0, 0)
    time.sleep(hold)
    user32.mouse_event(up, 0, 0, 0, 0)
    time.sleep(settle)


def alive(pid):
    import psutil
    return psutil.pid_exists(pid)


def do_shot(name, box=None):
    from PIL import ImageGrab
    path = WORK / name
    if box:
        ImageGrab.grab(bbox=box).save(path)
    else:
        ImageGrab.grab().save(path)
    print(f"screenshot: {path}")


def main():
    args = sys.argv[1:]
    pid, ok, hwnd = attach()
    print(f"pid={pid} alive={ok} hwnd={hwnd}")
    if not ok:
        return 2
    if not args:
        r = app_rect(hwnd)
        print(f"rect: {r.left},{r.top} - {r.right},{r.bottom}")
        return 0
    cmd = args[0]
    if cmd == "front":
        foreground(hwnd)
        r = app_rect(hwnd)
        print(f"rect: {r.left},{r.top} - {r.right},{r.bottom}")
    elif cmd == "shot":
        do_shot(args[1] if len(args) > 1 else "shot.png")
    elif cmd == "shot-region":
        x, y, w, h = map(int, args[1:5])
        do_shot(args[5] if len(args) > 5 else "region.png", (x, y, x + w, y + h))
    elif cmd in ("click", "rclick"):
        do_click(int(args[1]), int(args[2]), "left" if cmd == "click" else "right")
        if not alive(pid):
            print(f"CRASH after {cmd}")
            return 3
    elif cmd == "combo":
        # fast right, left, right at the same point
        x, y = int(args[1]), int(args[2])
        for i, button in enumerate(["right", "left", "right"]):
            do_click(x, y, button, hold=0.03, settle=0.08)
            if not alive(pid):
                print(f"CRASH at combo step {i} ({button})")
                return 3
        print("combo survived")
    return 0


if __name__ == "__main__":
    sys.exit(main())
