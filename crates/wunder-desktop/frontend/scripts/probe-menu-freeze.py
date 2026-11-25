"""Drive the menu-freeze probe: does the frame keep updating after a native menu?"""
import ctypes
import hashlib
import os
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
EXE = ROOT / "crates/wunder-desktop/frontend/target/debug/examples/menu_freeze_probe.exe"
WORK = ROOT / "temp_dir/menu-freeze-probe"

u = ctypes.windll.user32
g = ctypes.windll.gdi32
u.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))
u.SendMessageTimeoutW.argtypes = [
    ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t, ctypes.c_ssize_t,
    ctypes.c_uint, ctypes.c_uint, ctypes.POINTER(ctypes.c_size_t),
]
u.SendMessageTimeoutW.restype = ctypes.c_ssize_t
g.CreateCompatibleDC.restype = ctypes.c_void_p
g.CreateCompatibleBitmap.restype = ctypes.c_void_p
g.SelectObject.restype = ctypes.c_void_p
RD, RU = 0x0008, 0x0010
u.GetDC.restype = ctypes.c_void_p
    
g.CreateCompatibleDC.argtypes = [ctypes.c_void_p]
g.CreateCompatibleBitmap.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int]
g.SelectObject.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
g.DeleteObject.argtypes = [ctypes.c_void_p]
g.DeleteDC.argtypes = [ctypes.c_void_p]
u.ReleaseDC.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
g.GetDIBits.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint,
                        ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint]
g.BitBlt.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int,
                     ctypes.c_int, ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_uint]


class Rect(ctypes.Structure):
    _fields_ = [("left", ctypes.c_long), ("top", ctypes.c_long),
                ("right", ctypes.c_long), ("bottom", ctypes.c_long)]


class BitmapInfoHeader(ctypes.Structure):
    _fields_ = [("biSize", ctypes.c_uint32), ("biWidth", ctypes.c_int32),
                ("biHeight", ctypes.c_int32), ("biPlanes", ctypes.c_uint16),
                ("biBitCount", ctypes.c_uint16), ("biCompression", ctypes.c_uint32),
                ("biSizeImage", ctypes.c_uint32), ("biXPelsPerMeter", ctypes.c_int32),
                ("biYPelsPerMeter", ctypes.c_int32), ("biClrUsed", ctypes.c_uint32),
                ("biClrImportant", ctypes.c_uint32)]


class BitmapInfo(ctypes.Structure):
    _fields_ = [("bmiHeader", BitmapInfoHeader), ("bmiColors", ctypes.c_uint32 * 3)]


def screen_hash(x, y, width, height):
    """Hash the pixels currently on screen, so a stale window is visible here."""
    screen = u.GetDC(0)
    mem = g.CreateCompatibleDC(screen)
    bitmap = g.CreateCompatibleBitmap(screen, width, height)
    old = g.SelectObject(mem, bitmap)
    g.BitBlt(mem, 0, 0, width, height, screen, x, y, 0x00CC0020)
    info = BitmapInfo()
    info.bmiHeader.biSize = ctypes.sizeof(BitmapInfoHeader)
    info.bmiHeader.biWidth = width
    info.bmiHeader.biHeight = -height
    info.bmiHeader.biPlanes = 1
    info.bmiHeader.biBitCount = 32
    buffer = ctypes.create_string_buffer(width * height * 4)
    g.GetDIBits(mem, bitmap, 0, height, buffer, ctypes.byref(info), 0)
    g.SelectObject(mem, old)
    g.DeleteObject(bitmap)
    g.DeleteDC(mem)
    u.ReleaseDC(0, screen)
    return hashlib.sha256(buffer.raw).hexdigest()



def find_window(pid):
    found = []
    proc = ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.c_void_p, ctypes.c_ssize_t)

    def cb(hwnd, _):
        owner = ctypes.c_ulong()
        u.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value == pid and u.IsWindowVisible(hwnd) and u.GetWindowTextLengthW(hwnd):
            found.append(hwnd)
        return True

    u.EnumWindows(proc(cb), 0)
    return found[0] if found else None


def menu_windows(pid):
    found = []
    proc = ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.c_void_p, ctypes.c_ssize_t)

    def cb(hwnd, _):
        owner = ctypes.c_ulong()
        u.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value == pid and u.IsWindowVisible(hwnd):
            name = ctypes.create_unicode_buffer(64)
            u.GetClassNameW(hwnd, name, 64)
            if name.value == "#32768":
                found.append(hwnd)
        return True

    u.EnumWindows(proc(cb), 0)
    return found


def responsive(hwnd, timeout_ms=1200):
    result = ctypes.c_size_t(0)
    rc = u.SendMessageTimeoutW(hwnd, 0x0000, 0, 0, 0x0002, timeout_ms, ctypes.byref(result))
    return rc != 0


def click(x, y, right=False):
    u.SetCursorPos(int(x), int(y))
    time.sleep(0.08)
    u.mouse_event(RD if right else 0x0002, 0, 0, 0, 0)
    time.sleep(0.05)
    u.mouse_event(RU if right else 0x0004, 0, 0, 0, 0)


def key(vk):
    u.keybd_event(vk, 0, 0, 0)
    time.sleep(0.02)
    u.keybd_event(vk, 0, 2, 0)
    time.sleep(0.05)


def crop(rect, top, height, name):
    return screen_hash(rect.left + 10, rect.top + top, 380, height)


def main():
    WORK.mkdir(parents=True, exist_ok=True)
    log_path = WORK / "probe.log"
    trace_path = WORK / "trace.log"
    env = dict(os.environ, RCHO_SLINT_DIAGNOSTICS="1", PROBE_LOG=str(log_path),
               WUNDER_UI_TRACE="1")
    trace = trace_path.open("wb")
    proc = subprocess.Popen([str(EXE)], cwd=str(WORK), env=env,
                            stdout=trace, stderr=trace)
    hwnd = None
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        hwnd = find_window(proc.pid)
        if hwnd:
            break
        time.sleep(0.2)
    assert hwnd, "no window"
    u.SetWindowPos(hwnd, 0, 100, 100, 900, 620, 0x0004)
    time.sleep(2.0)

    rect = Rect()
    u.GetWindowRect(hwnd, ctypes.byref(rect))
    client = Rect()
    u.GetClientRect(hwnd, ctypes.byref(client))
    origin = ctypes.wintypes.POINT(0, 0) if hasattr(ctypes, "wintypes") else None
    print("rect", rect.left, rect.top, rect.right, rect.bottom,
          "client", client.right, client.bottom, flush=True)

    def tick_hash(name):
        return crop(rect, 0, 210, name)

    before = tick_hash("before.png")
    time.sleep(0.4)
    after = tick_hash("before2.png")
    print("idle repaint:", before != after, before[:12], after[:12], flush=True)

    # Probe several heights inside the panel band to survive caption/DPI drift.
    panel_points = [rect.top + 140, rect.top + 170, rect.top + 200]
    results = []
    for presses, label in ((1, "noop"), (2, "text"), (3, "popup")):
        for point in panel_points:
            click(rect.left + 300, point, right=True)
            time.sleep(0.6)
            if menu_windows(proc.pid):
                break
        menus = menu_windows(proc.pid)
        click_alive = responsive(hwnd)
        print(f"[{label}] menu={len(menus)} responsive={click_alive}", flush=True)
        if menus:
            for _ in range(presses - 1):
                key(0x28)  # Down
            key(0x0D)      # Enter
        time.sleep(1.5)
        live = responsive(hwnd)
        shot_a = tick_hash(f"{label}-a.png")
        time.sleep(0.6)
        shot_b = tick_hash(f"{label}-b.png")
        changing = shot_a != shot_b
        print(f"[{label}] after-action responsive={live} ticking={changing}", flush=True)
        results.append((label, live, changing))

    # Minimize/restore: the user's own workaround.
    u.ShowWindow(hwnd, 6)  # SW_MINIMIZE
    time.sleep(0.8)
    u.ShowWindow(hwnd, 9)  # SW_RESTORE
    time.sleep(1.0)
    shot_a = tick_hash("restore-a.png")
    time.sleep(0.6)
    shot_b = tick_hash("restore-b.png")
    print("after restore ticking:", shot_a != shot_b, flush=True)

    print("RESULTS", results, flush=True)
    trace.close()
    lines = trace_path.read_text(errors="replace").splitlines()
    print("TRACE tail:", lines[-40:], flush=True)
    proc.kill()
    print("log:", log_path, flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
