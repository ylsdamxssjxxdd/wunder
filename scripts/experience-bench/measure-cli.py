#!/usr/bin/env python3
"""External CLI experience driver; never reads the user's model config."""
import argparse
import json
import re
import sys
import threading
import time
import uuid
from pathlib import Path

from common import (PROFILES, REPO, Report, clean_env, digest, model_config,
                    require, run_process, write_json)
from mock_model import Model, expected


class JsonlTurn:
    """CLI item.updated text is cumulative, not a delta."""
    def __init__(self, answer):
        self.answer = answer
        self.types = []
        self.thread = None
        self.updates = []
        self.complete = None
        self.first_event = None
        self.first_text = None

    def feed(self, line, elapsed):
        row = json.loads(line)
        kind = row['type']
        self.types.append(kind)
        if self.first_event is None:
            self.first_event = elapsed
        if kind == 'thread.started':
            self.thread = row['thread_id']
        item = row.get('item', {})
        if item.get('item_type') == 'agent_message' and item.get('text'):
            text = item['text']
            require(self.answer.startswith(text), 'cli_text_mismatch')
            require(not item.get('truncated'), 'cli_unexpected_truncation')
            if self.first_text is None:
                self.first_text = elapsed
            if kind == 'item.updated':
                require(not self.updates or text.startswith(self.updates[-1]), 'cli_text_reordered')
                self.updates.append(text)
            if kind == 'item.completed':
                self.complete = text

    def validate(self):
        require(self.thread and self.types[:2] == ['thread.started', 'turn.started'], 'cli_start_contract')
        require('turn.completed' in self.types and 'turn.failed' not in self.types
                and 'error' not in self.types, 'cli_terminal_contract')
        require(self.complete == self.answer, 'cli_final_text')
        require(len(set(self.updates)) >= 2 and len(self.updates[0]) < len(self.answer), 'cli_not_streaming')
        return 6


class CliSuite:
    def __init__(self, args, report, model):
        self.args, self.report, self.model = args, report, model
        self.fixture = report.output / 'fixture'
        self.fixture.mkdir()
        self.large_worker = None
        self.session = None
        self.serial = 0

    def worker(self):
        self.serial += 1
        root = self.fixture / 'worker-{:04d}'.format(self.serial)
        config = root / 'home' / 'config'
        config.mkdir(parents=True)
        workspace = root / 'workspace'
        workspace.mkdir()
        write_json(config / 'wunder.yaml', model_config('http://127.0.0.1:{}/v1'.format(self.model.server_port)))
        env = clean_env()
        env.update(WUNDER_HOME=str(root / 'home'), WUNDER_CLI_PROJECT_ROOT=str(root))
        command = [str(self.args.binary), '--user', 'fixture-user', '--lang', 'en-US',
                   '--temp-root', str(root / 'temp'), '-C', str(workspace),
                   '--approval-mode', 'never']
        return root, env, command

    def call(self, worker, command, timeout=180, **kwargs):
        return run_process(worker[2] + command, worker[0] / 'workspace', env=worker[1], timeout=timeout, **kwargs)

    def offline(self):
        worker = self.worker()
        metrics, resources = {}, []
        for name, command in [('help', ['--help']), ('doctor', ['doctor']),
                              ('tool_list', ['tool', 'list']), ('config', ['config', 'show'])]:
            result = self.call(worker, command)
            require(result['exit_code'] == 0 and result['stdout'].strip(), 'cli_offline_command')
            if name == 'help':
                require(all(word in result['stdout'] for word in ('exec', 'resume', 'doctor')), 'cli_help_surface')
            metrics[name + '_ms'] = [result['elapsed_ms']]
            resources.append(result['resources'])
        return metrics, 5, dict(resources=resources)

    def turn(self, worker, resume=None, slow_ms=0, stdin=False):
        key = 'fixture-run-' + uuid.uuid4().hex
        parser = JsonlTurn(expected(self.args.chunks))
        output = worker[0] / ('answer-' + uuid.uuid4().hex + '.txt')
        command = (['resume', resume, key] if resume else ['exec', '-' if stdin else key])
        command += ['--json']
        if not resume:
            command += ['--output-last-message', str(output)]
        began_unix_ns = time.time_ns()
        result = self.call(worker, command, stdin=key if stdin else None,
                           on_line=parser.feed, slow_ms=slow_ms)
        require(result['exit_code'] == 0, 'cli_turn_exit')
        checks = parser.validate()
        if not resume:
            require(output.read_text(encoding='utf-8').rstrip('\n') == expected(self.args.chunks).rstrip('\n'), 'cli_output_file')
        trace = self.model.trace(key)
        require(trace.get('calls') == 1 and trace.get('completed'), 'cli_provider_calls')
        if resume:
            require(parser.thread == resume, 'cli_resume_identity')
        metrics = dict(process_to_event_ms=[parser.first_event], process_to_text_ms=[parser.first_text],
                       process_to_provider_ms=[(trace['request_unix_ns'] - began_unix_ns) / 1e6],
                       process_to_exit_ms=[result['elapsed_ms']],
                       provider_to_first_ms=[(trace['first_unix_ns']-trace['request_unix_ns'])/1e6],
                       provider_stream_ms=[(trace['last_unix_ns']-trace['first_unix_ns'])/1e6])
        return metrics, checks+3, dict(resources=result['resources'],
                                       updates=len(parser.updates)), parser.thread

    def stream(self):
        return self.turn(self.worker())[:3]

    def stdin(self):
        return self.turn(self.worker(), stdin=True)[:3]

    def resume(self):
        worker = self.worker()
        _, _, _, session = self.turn(worker)
        return self.turn(worker, resume=session)[:3]

    def slow_consumer(self):
        return self.turn(self.worker(), slow_ms=10)[:3]

    def failure_recovery(self):
        worker = self.worker()
        events = []
        result = self.call(worker, ['exec', 'fixture-fail-' + uuid.uuid4().hex, '--json'],
                           on_line=lambda line, _: events.append(json.loads(line)))
        kinds = [e['type'] for e in events]
        require(result['exit_code'] != 0 and ('error' in kinds or 'turn.failed' in kinds), 'cli_failure_not_reported')
        metrics, checks, extra, _ = self.turn(worker)
        metrics['failure_to_exit_ms'] = [result['elapsed_ms']]
        return metrics, checks+1, extra

    def prepare_large(self):
        self.large_worker = self.worker()
        result = self.call(self.large_worker, ['stress-thread', '--user-rounds', str(self.args.user_rounds),
                           '--model-rounds', str(self.args.model_rounds), '--seed', '42',
                           '--profile', 'typical', '--title', 'fixture-large'], timeout=600)
        require(result['exit_code'] == 0, 'cli_fixture_generation')
        match = re.search(r'会话 ID:\s*([0-9a-f-]{36})', result['stdout'])
        require(match, 'cli_fixture_identity')
        self.session = match.group(1)
        counts = {}
        for field, label in [('user_turns', '用户轮次'), ('model_requests', '模型请求'),
                             ('model_actions', '执行动作'), ('tool_calls', '工具调用')]:
            found = re.search(label + r':\s*(\d+)', result['stdout'])
            require(found, 'cli_fixture_counts')
            counts[field] = int(found.group(1))
        require(counts['user_turns'] == self.args.user_rounds and
                counts['model_requests'] == self.args.user_rounds * self.args.model_rounds, 'cli_fixture_size')
        self.report.data['fixture'] = dict(generation_ms=result['elapsed_ms'], **counts)

    def large_trajectory(self):
        if not self.session:
            self.prepare_large()
        def page(extra):
            result = self.call(self.large_worker, ['trajectory', self.session, '--limit', '12', '--json'] + extra)
            require(result['exit_code'] == 0, 'cli_page_exit')
            row = json.loads(result['stdout'])
            require(row['session_id'] == self.session and not row.get('degraded'), 'cli_page_identity')
            require(0 < len(row['trajectory']) <= 12, 'cli_page_bounds')
            return result, row
        first, a = page([])
        require(a['has_more'] and a['next_before'] is not None, 'cli_cursor_missing')
        second, b = page(['--before', str(a['next_before'])])
        ids = {r['item_id'] for r in a['trajectory']}
        require(not ids.intersection(r['item_id'] for r in b['trajectory']), 'cli_page_overlap')
        return dict(first_trajectory_page_ms=[first['elapsed_ms']], older_trajectory_page_ms=[second['elapsed_ms']]), 7, dict(
            resources=[first['resources'], second['resources']], rows=[len(a['trajectory']), len(b['trajectory'])])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--profile', choices=PROFILES, default='smoke')
    parser.add_argument('--baseline', type=Path)
    parser.add_argument('--cases', default='offline,stream,stdin,resume,slow_consumer,failure_recovery,large_trajectory')
    parser.add_argument('--chunks', type=int, default=8)
    parser.add_argument('--user-rounds', type=int, default=100)
    parser.add_argument('--model-rounds', type=int, default=100)
    args = parser.parse_args()
    args.binary = args.binary.resolve()
    cases = args.cases.split(',')
    require(args.binary.is_file() and 2 <= args.chunks <= 256 and
            1 <= args.user_rounds <= 1000 and 1 <= args.model_rounds <= 1000, 'invalid_cli_arguments')
    require(all(c in ('offline', 'stream', 'stdin', 'resume', 'slow_consumer', 'failure_recovery', 'large_trajectory')
                for c in cases) and len(cases) == len(set(cases)) and cases, 'invalid_cli_cases')
    report = Report(args.output, 'cli-experience', dict(cases=cases, profile=args.profile,
                    user_rounds=args.user_rounds, model_requests_per_turn=args.model_rounds,
                    chunks=args.chunks, interval_ms=8, seed=42),
                    dict(mode='external-process', build='release', concurrency=1), args.baseline)
    report.data['artifacts'] = dict(binary_sha256=digest(args.binary))
    model = thread = None
    try:
        model = Model(('127.0.0.1', 0), args.chunks)
        thread = threading.Thread(target=model.serve_forever, daemon=True)
        thread.start()
        suite = CliSuite(args, report, model)
        repeats, warmup = PROFILES[args.profile]
        for case in cases:
            for index in range(repeats + warmup):
                report.sample(case, index, index < warmup, getattr(suite, case))
    except Exception as error:
        report.data['setup_failed'] = type(error).__name__
    finally:
        if model:
            model.shutdown()
            model.server_close()
        if thread:
            thread.join(timeout=3)
    return report.finish()


if __name__ == '__main__':
    sys.exit(main())
