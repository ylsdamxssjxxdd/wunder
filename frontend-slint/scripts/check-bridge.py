"""Run the native UI against an isolated desktop bridge and loopback model."""
import argparse
import json
import os
from pathlib import Path
import socket
import subprocess
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.request import Request, urlopen


class ModelHandler(BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def do_POST(self):
        payload = json.loads(self.rfile.read(int(self.headers.get("Content-Length", "0"))))
        if payload.get("stream"):
            chunks = [
                {"choices": [{"index": 0, "delta": {"role": "assistant", "content": "测试回复"}, "finish_reason": None}]},
                {"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]},
            ]
            body = "".join(f"data: {json.dumps(chunk, ensure_ascii=False)}\n\n" for chunk in chunks) + "data: [DONE]\n\n"
            content_type = "text/event-stream"
        else:
            body = json.dumps({"id": "test-response", "object": "chat.completion", "choices": [
                {"index": 0, "message": {"role": "assistant", "content": "测试回复"}, "finish_reason": "stop"}
            ], "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}}, ensure_ascii=False)
            content_type = "application/json"
        encoded = body.encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ui", type=Path, required=True)
    parser.add_argument("--bridge", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    # Require a fresh artifact directory; no cleanup can touch user data.
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    runtime = output / "runtime"
    (runtime / "config").mkdir(parents=True)
    (runtime / "config" / "wunder.yaml").write_text("{}\n", encoding="utf-8")
    model_server = ThreadingHTTPServer(("127.0.0.1", 0), ModelHandler)
    model_thread = threading.Thread(target=model_server.serve_forever, daemon=True)
    model_thread.start()
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        bridge_port = probe.getsockname()[1]
    model_url = f"http://127.0.0.1:{model_server.server_port}/v1"
    secret = uuid.uuid4().hex
    settings = {"workspace_root": "", "desktop_token": "", "updated_at": 0, "llm": {"default": "test-model", "models": {
        "test-model": {"provider": "openai", "model": "test-model", "base_url": model_url,
                       "api_key": secret, "model_type": "llm", "max_output": 64, "timeout_s": 15,
                       "support_vision": False}
    }}, "lan_mesh": {"enabled": False}}
    (runtime / "config" / "desktop.settings.json").write_text(json.dumps(settings), encoding="utf-8")
    environment = dict(os.environ, TOKIO_WORKER_THREADS="8", RAYON_NUM_THREADS="8")
    flags = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
    process = None
    try:
        with (output / "bridge.log").open("wb") as log:
            process = subprocess.Popen([
                str(args.bridge.resolve()), "--port", str(bridge_port),
                "--temp-root", str(runtime), "--workspace", str(output / "workspace"),
            ], stdout=log, stderr=subprocess.STDOUT, env=environment, creationflags=flags)
            base = f"http://127.0.0.1:{bridge_port}"
            deadline = time.monotonic() + 60
            while True:
                try:
                    with urlopen(base + "/config.json", timeout=1) as response:
                        config = json.load(response)
                    break
                except (OSError, ValueError):
                    if process.poll() is not None or time.monotonic() >= deadline:
                        raise RuntimeError("isolated bridge did not start; inspect bridge.log")
                    time.sleep(0.2)
            completed = subprocess.run([
                str(args.ui.resolve()), "--bridge-smoke", base, str(output / "ui"),
            ], timeout=110, creationflags=flags, capture_output=True)
            report = (output / "ui" / "smoke.txt").read_text(encoding="utf-8")
            print(report.strip())
            if completed.returncode or not report.startswith("PASS:"):
                raise RuntimeError("native bridge smoke failed")
            request = Request(base + "/wunder/desktop/settings", headers={
                "Authorization": "Bearer " + config["desktop_token"],
            })
            with urlopen(request, timeout=5) as response:
                llm = json.load(response)["data"]["llm"]
            assert llm["models"]["test-model"]["api_key"] == secret, "existing credentials changed"
            assert llm["models"]["test-model"]["max_output"] == 64, "unrelated model configuration changed"
            assert llm["default"] == "测试配置", "default model was not persisted"
            assert llm["models"]["测试配置"]["model"] == "test-model-edited", "model edit was not persisted"
            print("PASS: persisted model/default and unrelated credentials/configuration preserved")
    finally:
        if process is not None and process.poll() is None:
            process.terminate()
            process.wait(timeout=15)
        model_server.shutdown()
        model_server.server_close()


if __name__ == "__main__":
    main()
