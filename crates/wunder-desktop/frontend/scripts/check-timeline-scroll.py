"""Exercise real Viewer wheel input and stream following (Windows, no model).

The probe imports the production TimelineView. Recorded offsets must actually
move in both directions; merely disabling follow-output misses zero overflow.
Pillow is used only to save the rendered interaction states for visual review.
"""
import argparse
import ctypes
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import time

from PIL import ImageGrab

ROOT = Path(__file__).resolve().parents[2]
PROBE = """import { TimelineView } from "@TIMELINE@";
import { TimelineRow } from "@THEME@";
export struct Sample { offset: float, following: bool, revision: int, time: float }
export component ScrollProbe inherits Window {
    width: @WIDTH@px; height: @HEIGHT@px;
    in-out property <[TimelineRow]> rows;
    in property <bool> show-activity: false;
    in property <string> greeting;
    out property <[Sample]> samples: [@SAMPLES@];
    out property <int> sample-count: 0;
    private property <int> revision: 0;
    private property <int> ticks: 0;
    VerticalLayout {
        spacing: 0px;
        HorizontalLayout {
            height: 40px;
            Rectangle { Text { text: "Scroll probe"; } }
            Rectangle { Text { text: "Stream 1"; } TouchArea { clicked => { root.ticks = max(0, 12 - root.revision); } } }
            Rectangle { Text { text: "Stream 2"; } TouchArea { clicked => { root.ticks = max(0, 24 - root.revision); } } }
        }
        timeline := TimelineView {
            rows: root.rows;
            greeting: root.greeting;
            show-activity: root.show-activity;
            stream-revision: root.revision;
        }
    }
    Timer {
        interval: 50ms;
        running: root.sample-count < root.samples.length;
        triggered => {
            root.samples[root.sample-count] = {
                offset: timeline.scroll-y / 1px,
                following: timeline.follow-output,
                revision: root.revision,
                time: animation-tick() / 1ms,
            };
            root.sample-count += 1;
        }
    }
    Timer {
        interval: 40ms;
        running: root.ticks > 0;
        triggered => {
            root.rows[root.rows.length - 1].text += "\\nStreamed test line.\\nStreamed test line.";
            root.revision += 1;
            root.ticks -= 1;
        }
    }
}
"""


def fixture_rows():
    # Populate every field: the interpreter expects typed values even for the
    # unused nested models. Deriving defaults keeps this in sync with the API.
    source = (ROOT / "frontend-slint/ui/theme.slint").read_text(encoding="utf-8")
    fields = re.search(r"export struct TimelineRow \{(.*?)\n\}", source, re.S)[1]
    fields = re.sub(r"//[^\n]*", "", fields)
    defaults = {}
    for field, kind in re.findall(r"([\w-]+)\s*:\s*(\[[\w-]+\]|[\w-]+)", fields):
        defaults[field] = ([] if kind.startswith("[") else False if kind == "bool"
                           else "" if kind == "string" else 0)
    return [dict(defaults, id=f"scroll-row-{i}", kind=i % 2, visible=True,
                 text=f"Test message {i + 1:02d}\n" + "Readable test content. " * 45,
                 **{"group-idx": -1, "process-idx": -1}) for i in range(28)]


class Point(ctypes.Structure):
    _fields_ = [("x", ctypes.c_long), ("y", ctypes.c_long)]


class Rect(ctypes.Structure):
    _fields_ = [(name, ctypes.c_long) for name in ("left", "top", "right", "bottom")]


def exercise(viewer, output, width, height, empty=False):
    prefix = output / (f"empty-{width}x{height}" if empty else f"chat-{width}x{height}")
    source = PROBE.replace("@TIMELINE@", (ROOT / "frontend-slint/ui/timeline.slint").as_posix())
    source = source.replace("@THEME@", (ROOT / "frontend-slint/ui/theme.slint").as_posix())
    source = source.replace("@WIDTH@", str(width)).replace("@HEIGHT@", str(height))
    source = source.replace("@SAMPLES@", ",".join(["{offset: 0, following: true, revision: 0, time: 0}"] * 600))
    prefix.with_suffix(".slint").write_text(source, encoding="utf-8")
    data = {"rows": [] if empty else fixture_rows(), "show-activity": empty}
    if empty:
        data["greeting"] = "Long test greeting. " * 160
    prefix.with_suffix(".json").write_text(json.dumps(data), encoding="utf-8")
    spec = importlib.util.spec_from_file_location("launch_check", Path(__file__).with_name("check-launch.py"))
    helper = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helper)
    user32 = ctypes.windll.user32
    user32.SendMessageW.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_size_t, ctypes.c_ssize_t]
    user32.SetWindowPos.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_int,
                                   ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_uint]
    user32.SetForegroundWindow.argtypes = [ctypes.c_void_p]
    state_file = prefix.with_suffix(".state.json")
    with prefix.with_suffix(".log").open("w", encoding="utf-8") as log:
        process = subprocess.Popen(
            [viewer, "--load-data", str(prefix.with_suffix(".json")), "--save-data", str(state_file),
             str(prefix.with_suffix(".slint"))], cwd=ROOT,
            stdout=log, stderr=subprocess.STDOUT, creationflags=subprocess.CREATE_NO_WINDOW,
            env=dict(os.environ, SLINT_BACKEND="winit-software", SLINT_SCALE_FACTOR="1"),
        )
        try:
            hwnd = None
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline and not hwnd:
                assert process.poll() is None, "Viewer exited before opening"
                hwnd = helper.visible_window(process.pid)
                time.sleep(0.1)
            assert hwnd, "Viewer window missing"
            # Keep the declared client size; SetWindowPos sizes include the
            # native frame and would otherwise clip the bottom action.
            user32.ShowWindow(ctypes.c_void_p(hwnd), 9)
            user32.SetWindowPos(hwnd, -1, 40, 40, 0, 0, 0x0041)
            user32.SetForegroundWindow(hwnd)
            time.sleep(1.5)
            rect = Rect()
            user32.GetClientRect(ctypes.c_void_p(hwnd), ctypes.byref(rect))
            origin = Point(0, 0)
            user32.ClientToScreen(ctypes.c_void_p(hwnd), ctypes.byref(origin))

            def click(x, y):
                user32.SetCursorPos(origin.x + x, origin.y + y)
                time.sleep(0.08)
                point = (y << 16) | x
                user32.SendMessageW(hwnd, 0x0200, 0, point)
                time.sleep(0.08)
                user32.SendMessageW(hwnd, 0x0201, 1, point)
                time.sleep(0.05)
                user32.SendMessageW(hwnd, 0x0202, 0, point)
                time.sleep(0.35)

            checkpoints = []

            def record():
                checkpoints.append(time.monotonic())

            def wheel(delta):
                x, y = rect.right // 2, rect.bottom // 2
                user32.SetCursorPos(origin.x + x, origin.y + y)
                time.sleep(0.1)
                for _ in range(abs(delta) // 120):
                    user32.mouse_event(0x0800, 0, 0, ctypes.c_ulong(120 if delta > 0 else -120).value, 0)
                    time.sleep(0.08)
                time.sleep(1.2)

            def snapshot(suffix):
                ImageGrab.grab(bbox=(origin.x, origin.y, origin.x + rect.right,
                                     origin.y + rect.bottom)).save(prefix.with_name(prefix.name + suffix + ".png"))

            record()                        # 0: initially following the tail
            snapshot("-tail")
            wheel(960)
            record()                        # 1: wheel up must leave the tail
            if empty:
                wheel(-360)
                record()                    # 2: long empty greeting also scrolls
            else:
                wheel(-360)
                record()                    # 2: wheel down moves toward the tail
                click(rect.right // 2, 20)
                time.sleep(0.9)
                record()                    # 3: streamed growth preserves history
                snapshot("-history")
                click(rect.right // 2, rect.bottom - 21)
                time.sleep(0.3)
                record()                    # 4: built-in back-to-latest action
                click(rect.right * 5 // 6, 20)
                time.sleep(0.9)
                record()                    # 5: new streamed lines follow again
                click(rect.right // 2, 61)
                record()                    # 6: built-in jump to top
                wheel(-360)
                record()                    # 7: scrolling from the top works too
            closed_at = time.monotonic()
            user32.SendMessageW(hwnd, 0x0010, 0, 0)
            assert process.wait(timeout=5) == 0
        finally:
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=5)
    state = json.loads(state_file.read_text(encoding="utf-8"))
    recorded = state["samples"][:state["sample-count"]]
    assert len(recorded) > 10, "Viewer did not record the interaction"
    samples = [min(recorded, key=lambda sample: abs(sample["time"] - recorded[-1]["time"]
               - (checkpoint - closed_at) * 1000)) for checkpoint in checkpoints]
    offsets = [row["offset"] for row in samples]
    assert offsets[0] < -100, f"content has no usable overflow: {samples}"
    assert offsets[1] > offsets[0] + 100, f"wheel up did not move: {samples}"
    assert not samples[1]["following"], f"wheel up did not pause following: {samples}"
    assert offsets[2] < offsets[1] - 30, f"wheel down did not move: {samples}"
    if not empty:
        assert samples[3]["revision"] == 12, f"stream did not finish: {samples}"
        assert abs(offsets[3] - offsets[2]) <= 1, f"history jumped during stream: {samples}"
        assert not samples[3]["following"], f"stream stole scroll following: {samples}"
        assert samples[4]["following"] and offsets[4] < offsets[0] - 100, f"latest action failed: {samples}"
        assert samples[5]["following"] and offsets[5] < offsets[4] - 100, f"tail did not follow growth: {samples}"
        assert abs(offsets[6]) <= 1 and not samples[6]["following"], f"top action failed: {samples}"
        assert offsets[7] < -30, f"cannot scroll down from top: {samples}"
    print(f"PASS {'empty greeting' if empty else 'transcript'} {width}x{height}: offsets={offsets}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "target/.scratch/timeline-scroll-check")
    args = parser.parse_args()
    if os.name != "nt":
        parser.error("this wheel driver requires Windows")
    viewer = shutil.which("slint-viewer")
    assert viewer, "slint-viewer is required"
    ctypes.windll.user32.SetProcessDPIAware()
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=True)
    for width, height in [(1000, 760), (640, 460)]:
        exercise(viewer, args.output, width, height)
    exercise(viewer, args.output, 640, 460, empty=True)


if __name__ == "__main__":
    main()
