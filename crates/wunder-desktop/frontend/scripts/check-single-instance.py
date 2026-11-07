"""Verify installation-scoped native desktop ownership and window activation on Windows."""

import argparse
import ctypes
from ctypes import wintypes
import os
from pathlib import Path
import shutil
import subprocess
import time

import psutil


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ui", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--screenshot", action="store_true")
    args = parser.parse_args()
    if os.name != "nt":
        parser.error("this native window regression requires Windows")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    user32 = ctypes.WinDLL("user32", use_last_error=True)
    user32.SetProcessDPIAware()
    callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    user32.EnumWindows.argtypes = [callback_type, wintypes.LPARAM]
    user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
    user32.GetWindowTextLengthW.argtypes = [wintypes.HWND]
    user32.IsWindowVisible.argtypes = [wintypes.HWND]
    user32.IsIconic.argtypes = [wintypes.HWND]
    user32.ShowWindow.argtypes = [wintypes.HWND, ctypes.c_int]
    user32.PostMessageW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
    user32.GetForegroundWindow.restype = wintypes.HWND
    user32.GetWindowRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]
    processes = []
    logs = []

    def prepare(name):
        directory = output / name
        directory.mkdir()
        executable = directory / "desktop-ui.exe"
        shutil.copy2(args.ui.resolve(), executable)
        home = directory / "data"
        config = home / "config"
        config.mkdir(parents=True)
        (config / "wunder.yaml").write_text("{}\n", encoding="utf-8")
        environment = dict(os.environ, WUNDER_HOME=str(home), WUNDER_TEMPD=str(home),
                           WUNDER_WORK=str(directory / "workspace"),
                           TOKIO_WORKER_THREADS="4", RAYON_NUM_THREADS="4")
        log = (directory / "launch.log").open("wb")
        logs.append(log)
        return executable, environment, log

    def launch(installation, changed_home=False):
        executable, environment, log = installation
        environment = dict(environment)
        if changed_home:
            environment["WUNDER_HOME"] = str(executable.parent / "other-data")
        process = subprocess.Popen([str(executable)], cwd=executable.parent,
                                   env=environment, stdout=log, stderr=subprocess.STDOUT,
                                   creationflags=subprocess.CREATE_NO_WINDOW)
        processes.append(process)
        return process

    def wait_for(predicate, message, timeout=30):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            result = predicate()
            if result:
                return result
            time.sleep(0.05)
        raise AssertionError(message)

    def main_window(process):
        assert process.poll() is None, "primary exited unexpectedly"
        matches = []

        @callback_type
        def visit(window, _context):
            owner = wintypes.DWORD()
            user32.GetWindowThreadProcessId(window, ctypes.byref(owner))
            if owner.value == process.pid and user32.GetWindowTextLengthW(window):
                matches.append(window)
            return True

        user32.EnumWindows(visit, 0)
        return next((window for window in matches if user32.IsWindowVisible(window)), None)

    def duplicate(installation, primary, window, changed_home=False):
        process = launch(installation, changed_home)
        assert process.wait(timeout=10) == 0, "duplicate launch failed"
        wait_for(lambda: user32.IsWindowVisible(window) and not user32.IsIconic(window),
                 "original main window was not restored")
        wait_for(lambda: user32.GetForegroundWindow() == window,
                 "original main window did not receive foreground focus")
        assert primary.poll() is None, "duplicate launch terminated the primary"

    try:
        first_installation = prepare("install-a")
        second_installation = prepare("install-b")
        first = launch(first_installation)
        early_duplicate = launch(first_installation)
        wait_for(lambda: first.poll() is not None or early_duplicate.poll() is not None,
                 "startup duplicate did not exit", timeout=10)
        if first.poll() is not None:
            assert first.returncode == 0, "startup duplicate failed"
            first = early_duplicate
        else:
            assert early_duplicate.returncode == 0, "startup duplicate failed"
        first_window = wait_for(lambda: main_window(first), "first main window did not appear")
        duplicate(first_installation, first, first_window)
        user32.ShowWindow(first_window, 6)
        wait_for(lambda: user32.IsIconic(first_window), "main window did not minimize")
        duplicate(first_installation, first, first_window)
        assert user32.PostMessageW(first_window, 0x0010, 0, 0)
        wait_for(lambda: first.poll() is None and not user32.IsWindowVisible(first_window),
                 "main window did not hide to the tray")
        duplicate(first_installation, first, first_window, changed_home=True)
        second = launch(second_installation)
        second_window = wait_for(lambda: main_window(second), "second installation did not start")
        assert user32.PostMessageW(first_window, 0x0010, 0, 0)
        wait_for(lambda: first.poll() is None and not user32.IsWindowVisible(first_window),
                 "first installation did not hide before the isolation check")
        duplicate(second_installation, second, second_window)
        assert not user32.IsWindowVisible(first_window), "another installation was activated"
        duplicate(first_installation, first, first_window)
        for process in (first, second):
            watched = psutil.Process(process.pid)
            connections = getattr(watched, "net_connections", None) or watched.connections
            assert not any(connection.status == psutil.CONN_LISTEN for connection in connections()), \
                "native desktop opened a TCP listener"
        first.kill()
        first.wait(timeout=10)
        restarted = launch(first_installation)
        restarted_window = wait_for(lambda: main_window(restarted), "crash left a stale instance lock")
        duplicate(first_installation, restarted, restarted_window)
        if args.screenshot:
            from PIL import ImageGrab
            time.sleep(0.5)
            rectangle = wintypes.RECT()
            assert user32.GetWindowRect(restarted_window, ctypes.byref(rectangle))
            ImageGrab.grab(bbox=(rectangle.left, rectangle.top, rectangle.right, rectangle.bottom),
                           all_screens=True).save(output / "restored-window.png")
        print("PASS: startup duplicate, repeated activation, minimize/hide restore, foreground focus, "
              "path isolation, home-independent ownership, crash recovery; no TCP listener")
    finally:
        for process in reversed(processes):
            if process.poll() is None:
                process.kill()
                process.wait(timeout=10)
        for log in logs:
            log.close()


if __name__ == "__main__":
    main()
