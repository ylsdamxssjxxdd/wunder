"""Exercise the in-process SQLite runtime with a local model and real Slint UI."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class ModelHandler(BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def do_POST(self):
        payload = json.loads(self.rfile.read(int(self.headers.get("Content-Length", "0"))))
        last = next((m.get("content") for m in reversed(payload.get("messages", [])) if m.get("role") == "user"), "")
        delta = "增量测试内容。" * 4 + "\n"
        if last not in ("native-long", "native-cancel", "native-queue-first", "native-queue-second"):
            self.send_error(400, "unexpected test input")
            return
        count = 600 if last == "native-long" else 2000
        if last == "native-queue-first":
            count, delta = 80, "测试第一轮"
        elif last == "native-queue-second":
            count, delta = 10, "测试第二轮"
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        try:
            if last == "native-long":
                # Keep the model in reasoning-only output long enough to prove
                # that the real native/UI chain renders before the first body.
                for _ in range(80):
                    chunk = {"choices": [{"index": 0, "delta": {"reasoning_content": "测试思考增量。"}, "finish_reason": None}]}
                    self.wfile.write(("data: " + json.dumps(chunk, ensure_ascii=False) + "\n\n").encode())
                    self.wfile.flush()
                    time.sleep(0.08)
            for _ in range(count):
                chunk = {"choices": [{"index": 0, "delta": {"content": delta}, "finish_reason": None}]}
                self.wfile.write(("data: " + json.dumps(chunk, ensure_ascii=False) + "\n\n").encode())
                self.wfile.flush()
                time.sleep(0.05 if last == "native-queue-first" else 0.01)
            self.wfile.write(b'data: {"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}\n\ndata: [DONE]\n\n')
            self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
            pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ui", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--reasoning-only", action="store_true")
    parser.add_argument("--models-only", action="store_true")
    args = parser.parse_args()
    import psutil
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    config = output / "runtime" / "config"
    config.mkdir(parents=True)
    model = ThreadingHTTPServer(("127.0.0.1", 0), ModelHandler)
    threading.Thread(target=model.serve_forever, daemon=True).start()
    settings = {"workspace_root": "", "desktop_token": "", "updated_at": 0,
                "lan_mesh": {"enabled": False}, "llm": {"default": "test-model", "models": {
                    "test-model": {"provider": "openai", "model": "test-model", "model_type": "llm",
                                   "base_url": f"http://127.0.0.1:{model.server_port}/v1", "timeout_s": 30,
                                   "support_vision": False}}}}
    # The shared engine config owns model definitions. Settings only owns
    # desktop preferences, and --temp-root does not isolate persistent state.
    (config / "wunder.yaml").write_text(json.dumps({"llm": settings["llm"]}), encoding="utf-8")
    (config / "desktop.settings.json").write_text(json.dumps(settings), encoding="utf-8")
    runtime_env = dict(os.environ, WUNDER_HOME=str(config.parent),
                       TOKIO_WORKER_THREADS="8", RAYON_NUM_THREADS="8",
                       RUST_MIN_STACK=os.environ.get("RUST_MIN_STACK", "33554432"))
    process = None
    peak = 0
    try:
        with (output / "process.log").open("wb") as log:
            mode = "--native-model-smoke" if args.models_only else "--native-reasoning-smoke" if args.reasoning_only else "--native-smoke"
            process = subprocess.Popen([str(args.ui.resolve()), mode, str(output)],
                                       stdout=log, stderr=subprocess.STDOUT,
                                       env=runtime_env,
                                       creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
            watched = psutil.Process(process.pid)
            deadline = time.monotonic() + 110
            while process.poll() is None:
                try:
                    peak = max(peak, watched.memory_info().rss)
                    # Debug builds keep a console, so Windows auto-spawns
                    # conhost. Only a real bridge/runtime child is a failure.
                    children = [c for c in watched.children()
                                if c.name().lower() not in ("conhost.exe", "openconsole.exe")]
                    assert not children, f"native runtime spawned a child: {[c.name() for c in children]}"
                    connections = getattr(watched, "net_connections", None) or watched.connections
                    assert not any(c.status == psutil.CONN_LISTEN for c in connections()), "native UI opened a listening socket"
                except psutil.NoSuchProcess:
                    pass
                if time.monotonic() > deadline:
                    raise RuntimeError("native smoke timeout")
                time.sleep(0.1)
        report = (output / "smoke.txt").read_text(encoding="utf-8") if (output / "smoke.txt").exists() else "missing report"
        if process.returncode != 0 or not report.startswith("PASS"):
            raise RuntimeError(report)
        if args.reasoning_only or args.models_only:
            print(report.strip())
            return
        metrics = json.loads((output / "stream-metrics.json").read_text())
        metrics["peak_working_set_bytes"] = peak
        (output / "stream-metrics.json").write_text(json.dumps(metrics, indent=2), encoding="utf-8")
        with (output / "restore.log").open("wb") as log:
            subprocess.run([str(args.ui.resolve()), "--native-restore", str(output)], stdout=log,
                           stderr=subprocess.STDOUT, check=True, timeout=40,
                           env=runtime_env,
                           creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
        assert (output / "restore.txt").read_text(encoding="utf-8").startswith("PASS")
        print(report.strip())
        print(json.dumps(metrics))
    finally:
        if process is not None and process.poll() is None:
            process.kill()
            process.wait(timeout=10)
        model.shutdown()
        model.server_close()


if __name__ == "__main__":
    main()
