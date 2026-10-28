#!/usr/bin/env python3
"""Exercise the actual CLI in Windows ConPTY using an isolated streaming provider."""
import argparse
import json
import os
from pathlib import Path
import queue
import subprocess
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Provider(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_POST(self):
        payload = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        user = next((row.get('content', '') for row in reversed(payload['messages'])
                     if row.get('role') == 'user'), '')
        if not isinstance(user, str):
            user = json.dumps(user)
        import re
        match = re.search(r'fixture-turn-(\d+)', user)
        turn = int(match.group(1)) if match else 0
        self.server.calls.append(turn)
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.send_header('Connection', 'close')
        self.end_headers()
        self.close_connection = True

        def emit(delta, finish=None):
            chunk = dict(id='fixture-stream', model='fixture-model',
                         choices=[dict(index=0, delta=delta, finish_reason=finish)])
            self.wfile.write(('data: ' + json.dumps(chunk, ensure_ascii=False) + '\n\n').encode())
            self.wfile.flush()

        try:
            for index in range(40):
                emit(dict(reasoning_content='private-thought-{:02d}-{:03d}\n'.format(turn, index)))
                time.sleep(.005)
            for index in range(18):
                emit(dict(content='answer-{:02d}-{:03d} 中文 {} tail-{:02d}-{:03d}\n\n'.format(
                    turn, index, 'value ' * 5, turn, index)))
                time.sleep(.02)
            emit(dict(content='complete-{:02d}\n'.format(turn)))
            emit({}, 'stop')
            self.wfile.write(b'data: [DONE]\n\n')
            self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            pass


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--turns', type=int, default=6)
    args = parser.parse_args()
    if os.name != 'nt':
        parser.error('This driver requires Windows ConPTY')
    if not 3 <= args.turns <= 12:
        parser.error('turns must be between 3 and 12')
    from winpty import Backend, PtyProcess
    import pyte

    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    home, workspace = output / 'home', output / 'workspace'
    home.mkdir(exist_ok=True)
    workspace.mkdir(exist_ok=True)
    server = ThreadingHTTPServer(('127.0.0.1', 0), Provider)
    server.daemon_threads = True
    server.calls = []
    threading.Thread(target=server.serve_forever, daemon=True).start()
    config = home / 'config.toml'
    config.write_text('model = "fixture-model"\nproject_doc = false\n'
                      '[provider]\nbase_url = "http://127.0.0.1:{}/v1"\n'
                      'api_key = ""\nmax_context = 65536\n'.format(server.server_port), encoding='utf-8')
    env = {key: value for key, value in os.environ.items() if not key.startswith('WUNDER_')}
    env.update(WUNDER_HOME=str(home), TERM='xterm-256color', RUST_LOG='warn')
    command = [str(args.binary.resolve()), '--cd', str(workspace), '--lang', 'en-US',
               '--user', 'fixture-user', '--approval-mode', 'never']
    process = PtyProcess.spawn(subprocess.list2cmdline(command), cwd=str(workspace), env=env,
                               dimensions=(28, 90), backend=Backend.ConPTY)
    screen = pyte.HistoryScreen(90, 28, history=10000)
    stream = pyte.Stream(screen)
    chunks = queue.Queue(maxsize=256)
    stopped = threading.Event()

    def reader():
        try:
            while not stopped.is_set():
                text = process.read(8192)
                if not text:
                    break
                while not stopped.is_set():
                    try:
                        chunks.put(text, timeout=.1)
                        break
                    except queue.Full:
                        pass
        except (EOFError, OSError):
            pass

    threading.Thread(target=reader, daemon=True).start()
    captures = []
    raw_parts = []
    raw_size = 0
    transcript_open = False

    def pump(seconds):
        nonlocal raw_size
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            try:
                text = chunks.get(timeout=min(.05, max(.001, end - time.monotonic())))
            except queue.Empty:
                continue
            raw_parts.append(text)
            raw_size += len(text)
            if raw_size > 12 * 1024 * 1024:
                raise AssertionError('terminal output exceeded capture bound')
            stream.feed(text)
            if not transcript_open and 'private-thought-' in '\n'.join(screen.display):
                raise AssertionError('reasoning body leaked into the main terminal')

    try:
        end = time.monotonic() + 30
        while 'Ask anything' not in '\n'.join(screen.display) and time.monotonic() < end:
            pump(.1)
        assert 'Ask anything' in '\n'.join(screen.display), 'composer did not open'
        captures.append(('welcome', '\n'.join(screen.display)))
        for turn in range(1, args.turns + 1):
            process.write('fixture-turn-{:02d}'.format(turn))
            pump(.2)
            process.write('\r')
            if turn % 2 == 0:
                pump(.35)
                cols, rows = [(72, 20), (54, 36), (100, 30)][(turn // 2 - 1) % 3]
                process.setwinsize(rows, cols)
                screen.resize(lines=rows, columns=cols)
            end = time.monotonic() + 45
            marker = 'complete-{:02d}'.format(turn)
            while marker not in '\n'.join(screen.display) and time.monotonic() < end:
                pump(.1)
            assert marker in '\n'.join(screen.display), 'turn did not complete: {}'.format(turn)
            pump(.4)
            assert 'context left' in screen.display[-1], 'footer did not follow the terminal height'
            captures.append(('turn-{:02d}'.format(turn), '\n'.join(screen.display)))
        for turn, text in captures:
            if turn == 'welcome':
                continue
            assert 'private-thought-' not in text, 'reasoning leaked into the main terminal: ' + turn
            assert 'complete-' + turn[-2:] in text, 'completed turn missing from frame: ' + turn
        transcript_open = True
        process.write('\x14')
        pump(.8)
        document = '\n'.join(screen.display)
        assert 'transcript ' in document, 'transcript view did not open'
        assert 'complete-{:02d}'.format(args.turns) in document, 'latest turn missing in transcript view'
        process.write('\x1b[H')
        pump(.3)
        transcript_pages = []
        for _ in range(160):
            page = '\n'.join(screen.display)
            transcript_pages.append(page)
            status = screen.display[0].split()[1]
            shown, total = map(int, status.split('/'))
            if shown >= total:
                break
            process.write('\x1b[6~')
            pump(.04)
        else:
            raise AssertionError('transcript paging did not reach the end')
        complete_document = '\n'.join(transcript_pages)
        for turn in range(1, args.turns + 1):
            for index in range(18):
                assert 'answer-{:02d}-{:03d}'.format(turn, index) in complete_document, 'streamed row missing from transcript'
            assert 'private-thought-{:02d}-039'.format(turn) in complete_document, 'thinking text was not retained'
        for turn in range(1, args.turns + 1):
            latest = next(text for name, text in captures if name == 'turn-{:02d}'.format(turn))
            assert 'answer-{:02d}-017'.format(turn) in latest, 'final streamed row missing: {}'.format(turn)
        assert server.calls == list(range(1, args.turns + 1)), 'provider calls not one per turn'
        for name, text in captures:
            (output / (name + '.txt')).write_text(text, encoding='utf-8')
        (output / 'terminal.txt').write_text(document, encoding='utf-8')
        (output / 'transcript-pages.txt').write_text(complete_document, encoding='utf-8')
        report = dict(ok=True, turns=args.turns, provider_calls=len(server.calls),
                      answer_rows=args.turns * 18, reasoning_rows=args.turns * 40,
                      resized_turns=args.turns // 2, reasoning_leaks=0,
                      transcript_rows_verified=args.turns * 18, footer_follows_height=True)
        (output / 'report.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
        print(json.dumps(report))
    finally:
        (output / 'last-screen.txt').write_text('\n'.join(screen.display), encoding='utf-8')
        process.write('\x03')
        time.sleep(.15)
        if process.isalive():
            process.write('\x03')
            time.sleep(.15)
        stopped.set()
        process.close(force=True)
        server.shutdown()
        server.server_close()
        (output / 'raw.ansi').write_text(''.join(raw_parts), encoding='utf-8')
        for name, text in captures:
            (output / (name + '.txt')).write_text(text, encoding='utf-8')


if __name__ == '__main__':
    main()
