#!/usr/bin/env python3
"""Bounded OpenAI-compatible fixture for throughput API acceptance; no prompt logging."""
import argparse
import json
import math
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PREFILL_TOKENS_PER_SECOND = 2000
DECODE_TOKENS_PER_SECOND = 200
MAX_REQUEST_BYTES = 8 * 1024 * 1024
SLOTS = threading.BoundedSemaphore(8)
STATS_LOCK = threading.Lock()
STATS = {"requests": 0, "completed": 0, "disconnected": 0, "last": None}


def input_tokens(messages):
    return sum(math.ceil(len(str(message.get("content", "")).encode("utf-8")) / 4) + 4 for message in messages)


def prefill_seconds(tokens):
    return tokens / PREFILL_TOKENS_PER_SECOND


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_):
        pass

    def json_response(self, status, value):
        body = json.dumps(value).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path == "/health":
            self.json_response(200, {"ok": True, "prefill_tps": PREFILL_TOKENS_PER_SECOND, "decode_tps": DECODE_TOKENS_PER_SECOND})
        elif self.path == "/metrics":
            with STATS_LOCK:
                value = dict(STATS)
            self.json_response(200, value)
        else:
            self.json_response(404, {"error": "not_found"})

    def do_POST(self):
        if self.path != "/v1/chat/completions":
            self.json_response(404, {"error": "not_found"})
            return
        try:
            length = int(self.headers.get("Content-Length", "0"))
            if not 0 < length <= MAX_REQUEST_BYTES:
                raise ValueError()
            payload = json.loads(self.rfile.read(length))
            messages = payload["messages"]
            target = int(payload.get("max_tokens", 0))
            if not isinstance(messages, list) or not 1 <= target <= 8192 or payload.get("stream") is not True:
                raise ValueError()
            tokens = input_tokens(messages)
        except (ValueError, TypeError, KeyError):
            self.json_response(400, {"error": {"message": "invalid_request"}})
            return
        if not SLOTS.acquire(blocking=False):
            self.json_response(429, {"error": {"message": "capacity"}})
            return
        try:
            self.stream_response(payload, tokens, target)
        except (BrokenPipeError, ConnectionResetError):
            with STATS_LOCK:
                STATS["disconnected"] += 1
        finally:
            self.close_connection = True
            SLOTS.release()

    def stream_response(self, payload, tokens, target):
        mode = str(payload.get("model", "model"))
        if mode == "reject" or ("min_tokens" in payload and int(payload["min_tokens"]) != target):
            self.json_response(400, {"error": {"message": "unsupported_controls"}})
            return
        with STATS_LOCK:
            STATS["requests"] += 1
            STATS["last"] = {"input_tokens": tokens, "output_target": target,
                             "min_tokens": payload.get("min_tokens"), "ignore_eos": payload.get("ignore_eos"),
                             "prefill_s": prefill_seconds(tokens)}
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-cache")
        self.send_header("Connection", "close")
        self.end_headers()
        # Role and keepalive frames must not be counted as the first generated token.
        self.event({"choices": [{"delta": {"role": "assistant"}}]})
        deadline = time.monotonic() + prefill_seconds(tokens)
        while (remaining := deadline - time.monotonic()) > 0:
            time.sleep(min(1, remaining))
            self.wfile.write(b": prefill\n\n")
            self.wfile.flush()
        count = min(target, 64) if mode in ("short", "disconnect") else target
        start = time.monotonic()
        emitted = 0
        reasoning_target = count // 4
        while emitted < count:
            thinking = emitted < reasoning_target
            phase_end = reasoning_target if thinking else count
            batch = 1 if emitted == 0 else min(10, phase_end - emitted)
            next_count = emitted + batch
            deadline = start + (next_count - 1) / DECODE_TOKENS_PER_SECOND
            time.sleep(max(0, deadline - time.monotonic()))
            self.event({"choices": [{"delta": {"reasoning_content" if thinking else "content": " one" * batch}}]})
            emitted = next_count
        if mode == "disconnect":
            return
        self.event({"choices": [{"delta": {}, "finish_reason": "length" if count == target else "stop"}]})
        if mode != "missing":
            self.event({"choices": [], "usage": {"prompt_tokens": tokens, "completion_tokens": count,
                        "completion_tokens_details": {"reasoning_tokens": reasoning_target}, "total_tokens": tokens + count}})
        self.wfile.write(b"data: [DONE]\n\n")
        self.wfile.flush()
        with STATS_LOCK:
            STATS["completed"] += 1

    def event(self, value):
        self.wfile.write(f"data: {json.dumps(value)}\n\n".encode())
        self.wfile.flush()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=18090)
    args = parser.parse_args()
    ThreadingHTTPServer((args.host, args.port), Handler).serve_forever()
