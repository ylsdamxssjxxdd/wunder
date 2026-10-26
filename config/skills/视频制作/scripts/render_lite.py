# -*- coding: utf-8 -*-
"""Render a seek-driven HTML composition to video with the supplement toolchain.

Dependencies are limited to the Wunder supplement pack: Python 3.8 stdlib,
the bundled Chrome (headless, driven over CDP via a minimal stdlib WebSocket
client) and the bundled FFmpeg. No Node.js, no puppeteer, no pip installs.

Page contract:
  window.__video = {width, height, fps, duration}   (optional; CLI flags win)
  window.seekTo(tSeconds)                            (required; may return a
      Promise that resolves once the frame is ready, e.g. after fonts or
      images finish decoding)

Frames are captured deterministically: for every frame index the page is
seeked to t = index / fps, screenshotted via Page.captureScreenshot and the
PNG stream is piped into FFmpeg. Same input always produces the same video.
"""

import argparse
import base64
import json
import os
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import time

try:
    import urllib.request as urlrequest
except ImportError:  # pragma: no cover - python2 guard, supplement is py3.8
    import urllib2 as urlrequest


# ---------------------------------------------------------------------------
# Minimal WebSocket client (RFC 6455, client-masked, stdlib only)
# ---------------------------------------------------------------------------

class WebSocketError(Exception):
    pass


class WebSocket(object):
    def __init__(self, url, timeout=120.0):
        assert url.startswith("ws://"), "only ws:// is supported: %s" % url
        rest = url[len("ws://"):]
        hostport, _, path = rest.partition("/")
        path = "/" + path
        host, _, port = hostport.partition(":")
        port = int(port) if port else 80
        self.sock = socket.create_connection((host, port), timeout=timeout)
        self.sock.settimeout(timeout)
        key = base64.b64encode(os.urandom(16)).decode("ascii")
        request = (
            "GET %s HTTP/1.1\r\n"
            "Host: %s\r\n"
            "Upgrade: websocket\r\n"
            "Connection: Upgrade\r\n"
            "Sec-WebSocket-Key: %s\r\n"
            "Sec-WebSocket-Version: 13\r\n"
            "\r\n" % (path, hostport, key)
        )
        self.sock.sendall(request.encode("ascii"))
        response = self._read_http_head()
        if b" 101" not in response.split(b"\r\n", 1)[0]:
            raise WebSocketError("handshake failed: %r" % response[:200])
        self._recv_buffer = b""

    def _read_http_head(self):
        data = b""
        while b"\r\n\r\n" not in data:
            chunk = self.sock.recv(4096)
            if not chunk:
                raise WebSocketError("connection closed during handshake")
            data += chunk
        head, _, rest = data.partition(b"\r\n\r\n")
        self._recv_buffer = rest
        return head

    def _read_exact(self, count):
        while len(self._recv_buffer) < count:
            chunk = self.sock.recv(min(65536, count - len(self._recv_buffer) + 4096))
            if not chunk:
                raise WebSocketError("connection closed")
            self._recv_buffer += chunk
        data, self._recv_buffer = self._recv_buffer[:count], self._recv_buffer[count:]
        return data

    def _send_frame(self, opcode, payload):
        header = struct.pack("!B", 0x80 | opcode)
        length = len(payload)
        if length < 126:
            header += struct.pack("!B", 0x80 | length)
        elif length < 65536:
            header += struct.pack("!BH", 0x80 | 126, length)
        else:
            header += struct.pack("!BQ", 0x80 | 127, length)
        mask = os.urandom(4)
        masked = bytes(bytearray(b ^ mask[i % 4] for i, b in enumerate(bytearray(payload))))
        self.sock.sendall(header + mask + masked)

    def send_text(self, text):
        self._send_frame(0x1, text.encode("utf-8"))

    def recv_message(self):
        parts = []
        while True:
            head = self._read_exact(2)
            b1, b2 = head[0], head[1]
            fin = bool(b1 & 0x80)
            opcode = b1 & 0x0F
            length = b2 & 0x7F
            if length == 126:
                length = struct.unpack("!H", self._read_exact(2))[0]
            elif length == 127:
                length = struct.unpack("!Q", self._read_exact(8))[0]
            if b2 & 0x80:
                mask = self._read_exact(4)
                payload = bytes(bytearray(b ^ mask[i % 4] for i, b in enumerate(bytearray(self._read_exact(length)))))
            else:
                payload = self._read_exact(length)
            if opcode == 0x9:  # ping -> pong
                self._send_frame(0xA, payload)
                continue
            if opcode == 0x8:
                raise WebSocketError("server closed the connection")
            if opcode in (0x0, 0x1, 0x2):
                parts.append(payload)
                if fin:
                    return b"".join(parts).decode("utf-8")

    def close(self):
        try:
            self._send_frame(0x8, b"")
            self.sock.close()
        except Exception:
            pass


# ---------------------------------------------------------------------------
# Chrome DevTools Protocol client
# ---------------------------------------------------------------------------

class CdpError(Exception):
    pass


class CdpClient(object):
    def __init__(self, ws_url):
        self.ws = WebSocket(ws_url)
        self.next_id = 0

    def call(self, method, params=None, session_id=None, timeout=120.0):
        self.next_id += 1
        message = {"id": self.next_id, "method": method, "params": params or {}}
        if session_id:
            message["sessionId"] = session_id
        self.ws.send_text(json.dumps(message))
        deadline = time.time() + timeout
        while True:
            if time.time() > deadline:
                raise CdpError("timeout waiting for %s" % method)
            reply = json.loads(self.ws.recv_message())
            if reply.get("id") != self.next_id:
                continue  # event or unrelated response
            if "error" in reply:
                raise CdpError("%s failed: %s" % (method, reply["error"].get("message")))
            return reply.get("result", {})

    def evaluate(self, expression, session_id, await_promise=False):
        result = self.call("Runtime.evaluate", {
            "expression": expression,
            "returnByValue": True,
            "awaitPromise": await_promise,
        }, session_id=session_id)
        if "exceptionDetails" in result:
            details = result["exceptionDetails"]
            text = details.get("text", "")
            exception = details.get("exception") or {}
            raise CdpError("page evaluation failed: %s %s" % (text, exception.get("description", "")))
        return result.get("result", {}).get("value")


def wait_for_debugger(port, process, timeout=30.0):
    url = "http://127.0.0.1:%d/json/version" % port
    deadline = time.time() + timeout
    while time.time() < deadline:
        if process.poll() is not None:
            raise CdpError("chrome exited with code %s before the debugger was ready" % process.returncode)
        try:
            with urlrequest.urlopen(url, timeout=2) as response:
                return json.loads(response.read().decode("utf-8"))["webSocketDebuggerUrl"]
        except Exception:
            time.sleep(0.3)
    raise CdpError("chrome devtools endpoint did not come up on port %d" % port)


def launch_chrome(chrome_bin, width, height):
    port_socket = socket.socket()
    port_socket.bind(("127.0.0.1", 0))
    port = port_socket.getsockname()[1]
    port_socket.close()
    profile_dir = tempfile.mkdtemp(prefix="wunder-lite-chrome-")
    command = [
        chrome_bin,
        "--headless",
        "--disable-gpu",
        "--hide-scrollbars",
        "--mute-audio",
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-extensions",
        "--remote-debugging-port=%d" % port,
        "--remote-allow-origins=*",
        "--window-size=%d,%d" % (width, height),
        "--user-data-dir=%s" % profile_dir,
        "about:blank",
    ]
    process = subprocess.Popen(
        command,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    ws_url = wait_for_debugger(port, process)
    return process, profile_dir, ws_url


# ---------------------------------------------------------------------------
# Render pipeline
# ---------------------------------------------------------------------------

def file_url(path):
    return "file:///" + os.path.abspath(path).replace("\\", "/")


def log(message):
    sys.stderr.write("[render-lite] %s\n" % message)
    sys.stderr.flush()


def render(args):
    html_path = os.path.abspath(args.html)
    if not os.path.isfile(html_path):
        raise SystemExit("input html not found: %s" % html_path)

    initial_width = args.width or 1920
    initial_height = args.height or 1080
    process, profile_dir, ws_url = launch_chrome(args.chrome, initial_width, initial_height)
    ffmpeg_log = None
    try:
        client = CdpClient(ws_url)
        target = client.call("Target.createTarget", {"url": "about:blank"})
        session = client.call("Target.attachToTarget", {
            "targetId": target["targetId"], "flatten": True,
        })["sessionId"]
        client.call("Page.enable", session_id=session)
        client.call("Runtime.enable", session_id=session)
        client.call("Emulation.setDeviceMetricsOverride", {
            "width": initial_width, "height": initial_height,
            "deviceScaleFactor": args.scale, "mobile": False,
        }, session_id=session)
        client.call("Page.navigate", {"url": file_url(html_path)}, session_id=session)

        deadline = time.time() + 30
        while time.time() < deadline:
            if client.evaluate("document.readyState", session) == "complete":
                break
            time.sleep(0.2)
        else:
            raise CdpError("page did not finish loading within 30s")
        client.evaluate("document.fonts ? document.fonts.ready.then(function(){return true}) : true",
                        session, await_promise=True)

        if client.evaluate("typeof window.seekTo", session) != "function":
            raise CdpError(
                "page does not expose window.seekTo(t) - the lite render "
                "contract requires it; see references/lite-render.md")

        config = client.evaluate("JSON.stringify(window.__video || null)", session)
        config = json.loads(config) if config else {}
        fps = args.fps or config.get("fps") or 30
        width = args.width or config.get("width") or 1920
        height = args.height or config.get("height") or 1080
        duration = args.duration or config.get("duration")
        if not duration:
            raise CdpError("duration unknown: pass --duration or set window.__video.duration")
        if (width, height) != (initial_width, initial_height):
            client.call("Emulation.setDeviceMetricsOverride", {
                "width": width, "height": height,
                "deviceScaleFactor": args.scale, "mobile": False,
            }, session_id=session)

        total = max(1, int(round(duration * fps)))
        output = os.path.abspath(args.output)
        out_dir = os.path.dirname(output)
        if out_dir and not os.path.isdir(out_dir):
            os.makedirs(out_dir)

        needs_mux = bool(args.audio)
        video_only = output if not needs_mux else output + ".video-only.tmp" + os.path.splitext(output)[1]
        codec_args = ["-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", str(args.crf),
                      "-preset", "medium", "-movflags", "+faststart"]
        ffmpeg_log = tempfile.NamedTemporaryFile(prefix="wunder-lite-ffmpeg-", suffix=".log", delete=False)
        command = [args.ffmpeg, "-y", "-f", "image2pipe", "-framerate", str(fps), "-i", "-"] + codec_args + [video_only]
        log("ffmpeg: %s" % " ".join(command))
        encoder = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=ffmpeg_log)

        started = time.time()
        try:
            for index in range(total):
                t = index / float(fps)
                client.evaluate("window.seekTo(%.6f)" % t, session, await_promise=True)
                shot = client.call("Page.captureScreenshot", {"format": "png"}, session_id=session)
                encoder.stdin.write(base64.b64decode(shot["data"]))
                if index % max(1, fps) == 0 or index == total - 1:
                    elapsed = time.time() - started
                    log("frame %d/%d (t=%.2fs, %.1f fps)" % (index + 1, total, t,
                                                             (index + 1) / max(elapsed, 0.01)))
            encoder.stdin.close()
            if encoder.wait() != 0:
                raise CdpError("ffmpeg failed, see %s" % ffmpeg_log.name)
        except BrokenPipeError:
            raise CdpError("ffmpeg closed the frame pipe, see %s" % ffmpeg_log.name)
        except IOError as exc:
            if encoder.poll() is not None:
                raise CdpError("ffmpeg died mid-render (code %s), see %s" % (encoder.returncode, ffmpeg_log.name))
            raise

        if args.audio:
            mux_log = ffmpeg_log
            mux = [args.ffmpeg, "-y", "-i", video_only, "-i", os.path.abspath(args.audio),
                   "-c:v", "copy", "-c:a", "aac", "-shortest", output]
            log("mux audio: %s" % " ".join(mux))
            if subprocess.call(mux, stdout=subprocess.DEVNULL, stderr=mux_log) != 0:
                raise CdpError("audio mux failed, see %s" % mux_log.name)
            os.remove(video_only)
        elif video_only != output:
            os.replace(video_only, output)

        if not os.path.isfile(output) or os.path.getsize(output) == 0:
            raise CdpError("render finished but output is empty: %s" % output)
        log("done: %s (%d bytes, %dx%d@%dfps, %.2fs)" % (
            output, os.path.getsize(output), width, height, fps, total / float(fps)))
        print(output)
    finally:
        if ffmpeg_log is not None:
            ffmpeg_log.close()
        try:
            process.terminate()
            process.wait(timeout=10)
        except Exception:
            try:
                process.kill()
            except Exception:
                pass
        shutil.rmtree(profile_dir, ignore_errors=True)


def main():
    parser = argparse.ArgumentParser(
        description="Render a seek-driven HTML composition to video (Chrome CDP + FFmpeg, no Node.js)")
    parser.add_argument("html", help="composition html file exposing window.seekTo(t)")
    parser.add_argument("-o", "--output", required=True, help="output .mp4 file")
    parser.add_argument("--chrome", default=os.environ.get("WUNDER_CHROME_BIN", "chrome"),
                        help="chrome binary; defaults to WUNDER_CHROME_BIN or PATH")
    parser.add_argument("--ffmpeg", default=os.environ.get("WUNDER_FFMPEG_BIN", "ffmpeg"),
                        help="ffmpeg binary; defaults to WUNDER_FFMPEG_BIN or PATH")
    parser.add_argument("--width", type=int, default=0, help="viewport width; overrides window.__video")
    parser.add_argument("--height", type=int, default=0, help="viewport height; overrides window.__video")
    parser.add_argument("--fps", type=int, default=0, help="frames per second; overrides window.__video")
    parser.add_argument("--duration", type=float, default=0.0, help="seconds; overrides window.__video")
    parser.add_argument("--scale", type=float, default=1.0, help="device scale factor for sharper capture")
    parser.add_argument("--crf", type=int, default=18, help="x264 quality (lower is better)")
    parser.add_argument("--audio", default="", help="optional audio file muxed into the output (-shortest)")
    args = parser.parse_args()
    try:
        render(args)
    except (CdpError, WebSocketError) as exc:
        log("ERROR: %s" % exc)
        sys.exit(1)


if __name__ == "__main__":
    main()
