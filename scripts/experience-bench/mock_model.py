"""Bounded deterministic SSE provider, used only by external experience tests."""
import argparse
from collections import OrderedDict
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import re
import threading
import time


def expected(chunks):
    return ''.join('fixture-{:04d}-片段\n'.format(i) for i in range(chunks))


class Model(ThreadingHTTPServer):
    daemon_threads = True
    block_on_close = False
    def __init__(self, address, chunks=8, interval_ms=8):
        super().__init__(address, Handler)
        self.chunks, self.interval_ms = chunks, interval_ms
        self.traces = OrderedDict()
        self.lock = threading.Lock()
        self.slots = threading.BoundedSemaphore(8)

    def trace(self, key, **values):
        with self.lock:
            row = self.traces.setdefault(key, dict(calls=0))
            row.update(values)
            while len(self.traces) > 128:
                self.traces.popitem(last=False)
            return dict(row)

    def handle_error(self, request, client_address):
        # Never emit tracebacks with host paths from an external test fixture.
        pass


class Handler(BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'
    def setup(self):
        super().setup()
        self.connection.settimeout(10)
    def log_message(self, *_):
        pass

    def reply(self, status, value):
        body = json.dumps(value).encode('utf-8')
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path == '/health':
            return self.reply(200, dict(ok=True))
        key = self.path.removeprefix('/trace/') if hasattr(str, 'removeprefix') else self.path[len('/trace/'):]
        if self.path.startswith('/trace/') and re.fullmatch(r'fixture-[a-z0-9-]{1,80}', key):
            with self.server.lock:
                row = dict(self.server.traces.get(key, {}))
            return self.reply(200, row)
        self.reply(404, dict(error='not_found'))

    def do_POST(self):
        if self.path != '/v1/chat/completions':
            return self.reply(404, dict(error='not_found'))
        if not self.server.slots.acquire(blocking=False):
            return self.reply(429, dict(error='capacity'))
        key = None
        try:
            length = int(self.headers.get('Content-Length', '0'))
            if not 0 < length <= 8 * 1024 * 1024:
                return self.reply(400, dict(error='body_budget'))
            payload = json.loads(self.rfile.read(length))
            user = next(m for m in reversed(payload['messages']) if m['role'] == 'user')
            text = user['content']
            if isinstance(text, list):
                text = ' '.join(p.get('text', '') for p in text if isinstance(p, dict))
            match = re.search(r'fixture-[a-z0-9-]{1,80}', text)
            if not match or payload.get('stream') is not True:
                return self.reply(400, dict(error='fixture_protocol'))
            key = match.group(0)
            row = self.server.trace(key)
            self.server.trace(key, calls=row['calls']+1, request_unix_ns=time.time_ns(), completed=False)
            if 'fixture-fail-' in text:
                return self.reply(503, dict(error=dict(message='fixture-unavailable')))
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.send_header('Connection', 'close')
            self.end_headers()
            self.close_connection = True
            for i in range(self.server.chunks):
                time.sleep(self.server.interval_ms / 1000)
                if i == 0:
                    self.server.trace(key, first_unix_ns=time.time_ns())
                part = dict(id='fixture-completion', object='chat.completion.chunk', model='fixture-model',
                            choices=[dict(index=0, delta=dict(content='fixture-{:04d}-片段\n'.format(i)), finish_reason=None)])
                self.wfile.write(('data: ' + json.dumps(part, ensure_ascii=False) + '\n\n').encode('utf-8'))
                self.wfile.flush()
                if i == 0 and 'fixture-hold-' in text:
                    # Bounded hold; periodic bytes detect disconnected callers.
                    for _ in range(120):
                        time.sleep(.25)
                        self.wfile.write(b': fixture-wait\n\n')
                        self.wfile.flush()
            self.server.trace(key, last_unix_ns=time.time_ns())
            self.wfile.write(('data: ' + json.dumps(dict(id='fixture-completion', choices=[
                dict(index=0, delta={}, finish_reason='stop')], usage=dict(prompt_tokens=32,
                completion_tokens=self.server.chunks*4, total_tokens=32+self.server.chunks*4))) + '\n\ndata: [DONE]\n\n').encode())
            self.wfile.flush()
            self.server.trace(key, completed=True)
        except (ValueError, KeyError, TypeError, StopIteration):
            self.close_connection = True
            self.reply(400, dict(error='fixture_protocol'))
        except (BrokenPipeError, ConnectionResetError, TimeoutError):
            if key:
                self.server.trace(key, disconnected=True)
        finally:
            self.server.slots.release()


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--port', type=int, default=18090)
    parser.add_argument('--chunks', type=int, default=8)
    parser.add_argument('--interval-ms', type=int, default=8)
    args = parser.parse_args()
    if not 2 <= args.chunks <= 256 or not 1 <= args.interval_ms <= 100:
        parser.error('invalid fixture workload')
    Model(('0.0.0.0', args.port), args.chunks, args.interval_ms).serve_forever()
