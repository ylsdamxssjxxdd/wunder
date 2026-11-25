"""Compare what the page does while a context menu is open.

native   -> Win32 TrackPopupMenu (#32768), expected to freeze the page
inwindow -> Slint popup, expected to keep repainting behind the menu

Usage: python scripts/probe-menu-live.py [native|inwindow]
"""
import ctypes
import hashlib
import os
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
EXE = ROOT / "crates/wunder-desktop/frontend/target/debug/examples/menu_freeze_probe.exe"
WORK = ROOT / "temp_dir/menu-live-probe"

u = ctypes.windll.user32
g = ctypes.windll.gdi32
u.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))
u.GetDC.restype = ctypes.c_void_p
u.SendMessageTimeoutW.argtypes = [
    ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t, ctypes.c_ssize_t,
    ctypes.c_uint, ctypes.c_uint, ctypes.POINTER(ctypes.c_size_t),
]
u.SendMessageTimeoutW.restype = ctypes.c_ssize_t
g.CreateCompatibleDC.restype = ctypes.c_void_p
g.CreateCompatibleBitmap.restype = ctypes.c_void_p
g.SelectObject.restype = ctypes.c_void_p
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

RD, RU = 0x0008, 0x0010


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


def save_region(x, y, width, height, path):
    screen = u.GetDC(0)
    mem = g.CreateCompatibleDC(screen)
    bitmap = g.CreateCompatibleBitmap(screen, width, height)
    old = g.SelectObject(mem, bitmap)
    g.BitBlt(mem, 0, 0, width, height, screen, x, y, 0x00CC0020)
    info = BitmapInfo()
    info.bmiHeader.biSize = ctypes.sizeof(BitmapInfoHeader)
    info.bmiHeader.biWidth = width
    info.bmiHeader.biHeight = height
    info.bmiHeader.biPlanes = 1
    info.bmiHeader.biBitCount = 32
    buffer = ctypes.create_string_buffer(width * height * 4)
    g.GetDIBits(mem, bitmap, 0, height, buffer, ctypes.byref(info), 0)
    g.SelectObject(mem, old)
    g.DeleteObject(bitmap)
    g.DeleteDC(mem)
    u.ReleaseDC(0, screen)
    header = bytearray(14 + 40)
    header[0:2] = b"BM"
    size = 14 + 40 + len(buffer.raw)
    header[2:6] = size.to_bytes(4, "little")
    header[10:14] = (54).to_bytes(4, "little")
    header[14:18] = (40).to_bytes(4, "little")
    header[18:22] = width.to_bytes(4, "little")
    header[22:26] = height.to_bytes(4, "little")
    header[26:28] = (1).to_bytes(2, "little")
    header[28:30] = (32).to_bytes(2, "little")
    with open(path, "wb") as f:
        f.write(header)
        f.write(buffer.raw)


def process_windows(pid):
    found = []
    proc = ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.c_void_p, ctypes.c_ssize_t)

    def cb(hwnd, _):
        owner = ctypes.c_ulong()
        u.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value == pid and u.IsWindowVisible(hwnd):
            name = ctypes.create_unicode_buffer(64)
            u.GetClassNameW(hwnd, name, 64)
            title = ctypes.create_unicode_buffer(128)
            u.GetWindowTextW(hwnd, title, 128)
            found.append((hwnd, name.value, title.value))
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
    time.sleep(0.03)
    u.keybd_event(vk, 0, 2, 0)
    time.sleep(0.05)


class Point(ctypes.Structure):
    _fields_ = [("x", ctypes.c_long), ("y", ctypes.c_long)]


def client_point(hwnd, x, y):
    point = Point(x, y)
    u.ClientToScreen(hwnd, ctypes.byref(point))
    return point.x, point.y


def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else "inwindow"
    WORK.mkdir(parents=True, exist_ok=True)
    log_path = WORK / f"{mode}.log"
    env = dict(os.environ, RCHO_SLINT_DIAGNOSTICS="1", PROBE_LOG=str(log_path))
    if mode == "inwindow":
        env["PROBE_INWINDOW_MENU"] = "1"
    proc = subprocess.Popen([str(EXE)], cwd=str(WORK), env=env,
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    hwnd = None
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        for handle, _, title in process_windows(proc.pid):
            if title:
                hwnd = handle
                break
        if hwnd:
            break
        time.sleep(0.2)
    assert hwnd, "no window"
    for handle, cls, title in process_windows(proc.pid):
        r = Rect()
        u.GetWindowRect(handle, ctypes.byref(r))
        print("win", handle, cls, repr(title), (r.left, r.top, r.right, r.bottom),
              "chosen" if handle == hwnd else "", flush=True)
    u.SetWindowPos(hwnd, 0, 100, 100, 900, 620, 0x0004)
    time.sleep(2.0)

    rect = Rect()
    u.GetWindowRect(hwnd, ctypes.byref(rect))
    print("rect after move", (rect.left, rect.top, rect.right, rect.bottom), flush=True)
    c = Rect()
    u.GetClientRect(hwnd, ctypes.byref(c))
    print("client", (c.right, c.bottom), flush=True)
    save_region(rect.left, rect.top, min(rect.right - rect.left, 520),
                min(rect.bottom - rect.top, 380), str(WORK / f"{mode}-window.bmp"))
    tick = lambda: screen_hash(rect.left + 12, rect.top + 34, 300, 46)

    idle_a, idle_b = tick(), (time.sleep(0.4) or tick())
    print(f"[{mode}] idle repaint {idle_a != idle_b}", flush=True)

    u.SetForegroundWindow(hwnd)
    time.sleep(0.3)
    menu_x, menu_y = client_point(hwnd, 300, 210)
    native_menu, popup, windows = [], [], []
    for _ in range(3):
        click(menu_x, menu_y, right=True)
        time.sleep(0.6)
        windows = process_windows(proc.pid)
        native_menu = [w for w in windows if w[1] == "#32768"]
        popup = [w for w in windows if w[0] != hwnd and w[1] != "Winit Thread Event Target"]
        print("after click", len(native_menu), len(popup),
              "fg", u.GetForegroundWindow() == hwnd, flush=True)
        if native_menu or popup:
            break
    print(f"[{mode}] classes={[(w[1], w[2][:16]) for w in windows if w[0] != hwnd]}",
          flush=True)

    save_region(rect.left, rect.top, min(rect.right - rect.left, 560),
                min(rect.bottom - rect.top, 420), str(WORK / f"{mode}-open.bmp"))
    if mode == "inwindow":
        popup = ["in-window render"]
    open_a = tick()
    time.sleep(0.6)
    open_b = tick()
    print(f"[{mode}] repaint while menu open {open_a != open_b}", flush=True)

    if native_menu:
        key(0x1B)  # Escape dismisses a Win32 menu
    else:
        click(*client_point(hwnd, 700, 480))  # outside click
    time.sleep(0.6)
    remaining = [w for w in process_windows(proc.pid)
                 if w[0] != hwnd and w[1] != "Winit Thread Event Target"]
    closed = not remaining
    after_a = tick()
    time.sleep(0.6)
    after_b = tick()
    print(f"[{mode}] menu closed {closed} repaint after close {after_a != after_b}",
          flush=True)

    # Activate the first entry through the keyboard and confirm the callback ran.
    click(menu_x, menu_y, right=True)
    time.sleep(0.6)
    key(0x28)  # Down
    key(0x0D)  # Return
    time.sleep(0.8)
    log = log_path.read_text(errors="replace") if log_path.exists() else ""
    activated = "action noop" in log
    still = responsive(hwnd)
    print(f"[{mode}] activated={activated} responsive={still}", flush=True)

    proc.kill()
    return 0


if __name__ == "__main__":
    sys.exit(main())
