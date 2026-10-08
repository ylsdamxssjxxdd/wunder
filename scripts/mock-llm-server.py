#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""E2E 用的最小 OpenAI 兼容模型服务（无外部依赖）。

用途：让 `scripts/check-web.py` 的真机验收能跑通**完整聊天链路**（发送 → 流式回复 → 时间线渲染），
不需要真实大模型。默认监听 127.0.0.1:18040，只实现 `POST /v1/chat/completions`
（`stream=true` 时按 SSE 逐块返回，`stream=false` 时返回一次性 JSON）。

用法：
    python scripts/mock-llm-server.py [--port 18040] [--reply "固定回复文本"]
"""

from __future__ import annotations

import argparse
import json
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

DEFAULT_REPLY = "这是 mock 模型回复：收到你的消息了。"


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    reply_text = DEFAULT_REPLY

    def log_message(self, fmt: str, *args) -> None:  # 静音访问日志
        return

    def _read_payload(self) -> dict:
        length = int(self.headers.get("Content-Length") or 0)
        raw = self.rfile.read(length) if length else b"{}"
        try:
            return json.loads(raw.decode("utf-8", "replace"))
        except json.JSONDecodeError:
            return {}

    def _chunk(self, delta: dict, finish_reason: str | None = None) -> bytes:
        payload = {
            "id": "chatcmpl-mock",
            "object": "chat.completion.chunk",
            "created": int(time.time()),
            "model": "mock-model",
            "choices": [{"index": 0, "delta": delta, "finish_reason": finish_reason}],
        }
        return f"data: {json.dumps(payload, ensure_ascii=False)}\n\n".encode("utf-8")

    def do_POST(self) -> None:  # noqa: N802
        if not self.path.rstrip("/").endswith("chat/completions"):
            self.send_error(404, "not found")
            return
        payload = self._read_payload()
        stream = bool(payload.get("stream"))
        print(
            f"[mock-llm] POST {self.path} stream={stream} model={payload.get('model')} "
            f"messages={len(payload.get('messages') or [])}",
            flush=True,
        )
        if not stream:
            body = json.dumps(
                {
                    "id": "chatcmpl-mock",
                    "object": "chat.completion",
                    "created": int(time.time()),
                    "model": "mock-model",
                    "choices": [
                        {
                            "index": 0,
                            "message": {"role": "assistant", "content": self.reply_text},
                            "finish_reason": "stop",
                        }
                    ],
                    "usage": {"prompt_tokens": 8, "completion_tokens": 12, "total_tokens": 20},
                },
                ensure_ascii=False,
            ).encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return

        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream; charset=utf-8")
        self.send_header("Cache-Control", "no-cache")
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(self._chunk({"role": "assistant", "content": ""}))
        for piece in _split_chunks(self.reply_text):
            self.wfile.write(self._chunk({"content": piece}))
            self.wfile.flush()
            time.sleep(0.02)
        self.wfile.write(self._chunk({}, finish_reason="stop"))
        self.wfile.write(b"data: [DONE]\n\n")
        self.wfile.flush()

    def do_GET(self) -> None:  # noqa: N802
        if self.path.rstrip("/").endswith("/models"):
            body = json.dumps({"object": "list", "data": [{"id": "mock-model", "object": "model"}]}).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        self.send_error(404, "not found")


def _split_chunks(text: str, size: int = 4) -> list[str]:
    return [text[index : index + size] for index in range(0, len(text), size)] or [""]


def main() -> int:
    parser = argparse.ArgumentParser(description="E2E mock 模型服务")
    parser.add_argument("--port", type=int, default=18040)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--reply", default=DEFAULT_REPLY)
    args = parser.parse_args()
    Handler.reply_text = args.reply
    server = ThreadingHTTPServer((args.host, args.port), Handler)
    print(f"[mock-llm] listening on http://{args.host}:{args.port}/v1/chat/completions", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
