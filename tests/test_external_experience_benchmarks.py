"""Correctness of the external harness, independent of a released application."""
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import threading
import time
from types import SimpleNamespace
import unittest
from unittest.mock import patch
from urllib.request import Request, urlopen

DRIVER = Path(__file__).resolve().parents[1] / 'scripts' / 'experience-bench'
sys.path.insert(0, str(DRIVER))
import common
from mock_model import Model, expected

spec = importlib.util.spec_from_file_location('cli_experience_driver', DRIVER / 'measure-cli.py')
CLI = importlib.util.module_from_spec(spec)
spec.loader.exec_module(CLI)
spec = importlib.util.spec_from_file_location('hive_experience_driver', DRIVER / 'measure-hive.py')
HIVE = importlib.util.module_from_spec(spec)
spec.loader.exec_module(HIVE)


class ReportTests(unittest.TestCase):
    def report(self, root, name='run'):
        return common.Report(Path(root)/name, 'fixture-suite', dict(cases=['stream']), dict(mode='fixture'))

    def test_failed_samples_are_not_hidden_by_fast_success(self):
        with tempfile.TemporaryDirectory() as root:
            report = self.report(root)
            report.sample('stream', 0, False, lambda: ({'first_ms': [1]}, 1, {}))
            def fail():
                raise ValueError('sensitive fixture payload must not be persisted')
            report.sample('stream', 1, False, fail)
            self.assertEqual(report.finish(), 1)
            saved = (report.output/'report.json').read_text(encoding='utf-8')
            self.assertNotIn('sensitive fixture payload', saved)
            self.assertEqual(report.data['summary']['stream']['failed_runs'], 1)

    def test_missing_case_and_failed_warmup_fail(self):
        with tempfile.TemporaryDirectory() as root:
            report = self.report(root)
            report.sample('stream', 0, True, lambda: ({'first_ms': [1]}, 1, {}))
            self.assertEqual(report.finish(), 1)
            failed = self.report(root, 'failed-warmup')
            failed.sample('stream', 0, True, lambda: ({}, 0, {}))
            failed.sample('stream', 1, False, lambda: ({'first_ms': [1]}, 1, {}))
            self.assertEqual(failed.finish(), 1)

    def test_gate_checks_environment_counts_and_meaningful_regression(self):
        with tempfile.TemporaryDirectory() as root:
            old = self.report(root, 'before')
            old.sample('stream', 0, False, lambda: ({'first_ms': [100]}, 1, {}))
            self.assertEqual(old.finish(), 0)
            now = self.report(root, 'after')
            now.sample('stream', 0, False, lambda: ({'first_ms': [130]}, 1, {}))
            now.finish()
            self.assertEqual(len(common.compare(now.data, old.data)['regressions']), 2)
            now.data['fixture'] = dict(model_actions=12)
            self.assertFalse(common.compare(now.data, old.data)['compatible'])

    def test_missing_metric_or_failed_baseline_is_incompatible(self):
        with tempfile.TemporaryDirectory() as root:
            old = self.report(root, 'before')
            old.sample('stream', 0, False, lambda: ({'first_ms': [100], 'terminal_ms': [120]}, 1, {}))
            old.finish()
            now = self.report(root, 'after')
            now.sample('stream', 0, False, lambda: ({'first_ms': [100]}, 1, {}))
            now.finish()
            self.assertFalse(common.compare(now.data, old.data)['compatible'])
            old.data['passed'] = False
            self.assertIn('baseline failed', common.compare(now.data, old.data)['mismatches'])

    def test_invalid_nan_metric_is_failure(self):
        with tempfile.TemporaryDirectory() as root:
            report = self.report(root)
            report.sample('stream', 0, False, lambda: ({'first_ms': [float('nan')]}, 1, {}))
            self.assertEqual(report.finish(), 1)


class ProcessTests(unittest.TestCase):
    def test_external_arrival_and_resource_capture(self):
        with tempfile.TemporaryDirectory() as root:
            lines = []
            result = common.run_process([sys.executable, '-u', '-c',
                'import time; print("fixture-first"); time.sleep(.12); print("fixture-last")'],
                root, on_line=lambda line, ms: lines.append((line.strip(), ms)))
            self.assertEqual(result['exit_code'], 0)
            self.assertEqual([v[0] for v in lines], ['fixture-first', 'fixture-last'])
            self.assertGreater(lines[1][1]-lines[0][1], 60)
            self.assertGreater(result['resources']['samples'], 0)

    def test_timeout_kills_owned_process_tree(self):
        import psutil
        with tempfile.TemporaryDirectory() as root:
            pids = []
            source = ('import subprocess, sys, time; '
                      'p=subprocess.Popen([sys.executable,"-c","import time;time.sleep(30)"]); '
                      'print(p.pid, flush=True); time.sleep(30)')
            with self.assertRaises(common.CheckFailure):
                common.run_process([sys.executable, '-u', '-c', source], root, timeout=.5,
                                   on_line=lambda line, _: pids.append(int(line)))
            self.assertTrue(pids)
            end = time.monotonic()+2
            while psutil.pid_exists(pids[0]) and time.monotonic() < end:
                time.sleep(.02)
            self.assertFalse(psutil.pid_exists(pids[0]))


class JsonlTests(unittest.TestCase):
    def test_cumulative_updates_and_terminal_are_required(self):
        parser = CLI.JsonlTurn('fixture-output')
        for row in [dict(type='thread.started', thread_id='fixture-thread'), dict(type='turn.started'),
                    dict(type='item.updated', item=dict(item_type='agent_message', text='fixture')),
                    dict(type='item.updated', item=dict(item_type='agent_message', text='fixture-out')),
                    dict(type='item.completed', item=dict(item_type='agent_message', text='fixture-output')),
                    dict(type='turn.completed')]:
            parser.feed(json.dumps(row), 10)
        self.assertEqual(parser.validate(), 6)
        parser.types.remove('turn.completed')
        with self.assertRaises(common.CheckFailure):
            parser.validate()

    def test_out_of_order_text_is_rejected(self):
        parser = CLI.JsonlTurn('fixture-output')
        parser.feed(json.dumps(dict(type='item.updated', item=dict(item_type='agent_message', text='fixture'))), 1)
        with self.assertRaises(common.CheckFailure):
            parser.feed(json.dumps(dict(type='item.updated', item=dict(item_type='agent_message', text='fix'))), 2)


class HiveChatTests(unittest.TestCase):
    def test_fixture_counts_match_canonical_storage_including_metadata(self):
        stats = dict(state='completed', user_turns=25, model_requests=2500,
                     model_actions=2501, tool_calls=1, items=2526,
                     rollout_records=6001, storage_bytes=None)
        for records, next_ordinal in ((6001, 6001), (6000, 6000), (6001, 6002)):
            with self.subTest(records=records, next_ordinal=next_ordinal):
                suite = HIVE.HiveSuite.__new__(HIVE.HiveSuite)
                suite.args = SimpleNamespace(user_rounds=25, model_rounds=100)
                suite.admin = 'fixture-token'
                suite.report = SimpleNamespace(data={})
                suite.api = lambda method, *args: ((dict(job_id='fixture-job', session_id='fixture-thread')
                    if method == 'POST' else dict(status=stats)), 1, 100)
                suite.canonical_fingerprint = lambda *args: dict(records=records, next_ordinal=next_ordinal)
                if records == next_ordinal == stats['rollout_records']:
                    suite.prepare_large()
                    self.assertEqual(suite.report.data['fixture']['rollout_records'], 6001)
                else:
                    with self.assertRaisesRegex(common.CheckFailure, 'canonical_count'):
                        suite.prepare_large()
                    self.assertNotIn('fixture', suite.report.data)

    def suite(self):
        suite = HIVE.HiveSuite.__new__(HIVE.HiveSuite)
        suite.args = SimpleNamespace(user_rounds=25, model_rounds=100)
        suite.large_session, suite.admin = 'fixture-thread', 'fixture-token'
        def row(number):
            return dict(root_turn_id='fixture-root-{}'.format(number), user_round=number,
                        execution_turn_id='fixture-turn-{}'.format(number), status='completed',
                        user=dict(content='fixture input'), assistant=dict(content='fixture output',
                        item_id='fixture-turn-{}:text-99'.format(number), body={'body_id': 'fixture-body'},
                        stats=dict(model_request_count=100)))
        suite.tail = dict(chat=[row(24), row(25)], latest_round=25, has_more=True, next_before=24)
        suite.older = dict(chat=[row(22), row(23)], next_after=23)
        def page(session, token, suffix=None):
            result = suite.tail if suffix is None or 'after=' in suffix else suite.older
            return result, 1, 100
        suite.page = page
        suite.body = lambda *args: ('fixture output', [1], 100)
        suite.api = lambda *args: (dict(data=dict(item_id='fixture-turn-25:text-99',
                                               message=dict(content='fixture output'))), 1, 100)
        return suite

    def test_large_chat_rejects_fast_empty_or_wrong_final_answers(self):
        suite = self.suite()
        self.assertIn('newer_chat_page_ms', suite.large_chat()[0])
        suite.tail['chat'][0]['assistant']['content'] = ''
        with self.assertRaisesRegex(common.CheckFailure, 'empty_message'):
            suite.large_chat()
        suite = self.suite()
        suite.tail['chat'][0]['assistant']['item_id'] = 'fixture-intermediate'
        with self.assertRaisesRegex(common.CheckFailure, 'final_answer_identity'):
            suite.large_chat()

    def test_large_chat_rejects_wrong_body_and_forward_cursor(self):
        suite = self.suite()
        suite.body = lambda *args: ('fixture truncated', [1], 100)
        with self.assertRaisesRegex(common.CheckFailure, 'body_mismatch'):
            suite.large_chat()
        suite = self.suite()
        page = suite.page
        suite.page = lambda session, token, suffix=None: ((suite.older, 1, 100)
            if suffix and 'after=' in suffix else page(session, token, suffix))
        with self.assertRaisesRegex(common.CheckFailure, 'forward_page'):
            suite.large_chat()


class ServiceStorageAuditTests(unittest.TestCase):
    def test_local_database_and_canonical_files_are_rejected(self):
        for filename in ('state_1.sqlite', 'queue_1.sqlite-wal', 'history.db',
                         'rollout.jsonl', 'rollout.jsonl.zst', '.canonical-root-id.json'):
            with self.subTest(filename=filename), tempfile.TemporaryDirectory() as root:
                folder = Path(root) / 'nested'
                folder.mkdir()
                (folder / filename).write_bytes(b'fixture')
                with self.assertRaisesRegex(common.CheckFailure, 'service_local_history_file_created'):
                    HIVE.assert_postgres_only_files(root)

    def test_resource_files_and_a_missing_data_directory_are_valid(self):
        with tempfile.TemporaryDirectory() as root:
            folder = Path(root)
            (folder / 'fixture.txt').write_text('fixture', encoding='utf-8')
            HIVE.assert_postgres_only_files(folder)
            HIVE.assert_postgres_only_files(folder / 'missing')


class SandboxFileHarnessTests(unittest.TestCase):
    def suite(self, root, misroute=False, local_history_files=0):
        suite = HIVE.HiveSuite.__new__(HIVE.HiveSuite)
        suite.credentials = [dict(user_id='fixture-admin'), dict(user_id='fixture-user-a'),
                             dict(user_id='fixture-user-b')]
        suite.stack = SimpleNamespace(root=Path(root), sandbox_storage_audit=lambda:
            dict(checked_entries=0, local_history_files=local_history_files))
        suite.report = SimpleNamespace(data={})
        barrier = threading.Barrier(4)
        calls = []
        lock = threading.Lock()

        def tool(request):
            # Both phases must have all four operations in flight. This fails
            # if the harness silently serializes its purported concurrency case.
            barrier.wait(timeout=5)
            workspace = request['workspace_root'].rsplit('/', 1)[1]
            path = request['args']['path']
            target = Path(root) / 'workspaces' / workspace / path
            with lock:
                calls.append((request['user_id'], workspace, request['tool']))
            if request['tool'] == '写入文件':
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_text(request['args']['content'], encoding='utf-8')
                return {}, 1
            content = 'fixture-wrong-scope' if misroute else target.read_text(encoding='utf-8')
            return dict(content='>>> {}\n1: {}'.format(path, content)), 2

        suite.sandbox_tool = tool
        return suite, calls

    def test_two_users_two_containers_are_concurrent_and_evidence_is_sanitized(self):
        with tempfile.TemporaryDirectory() as root:
            suite, calls = self.suite(root)
            metrics, checks, extra = suite.sandbox_files()
            self.assertEqual(metrics['sandbox_write_ms'], [1]*4)
            self.assertEqual(metrics['sandbox_read_ms'], [2]*4)
            self.assertGreater(checks, 0)
            self.assertEqual(len({row[0] for row in calls}), 2)
            self.assertEqual(len({row[1] for row in calls}), 4)
            self.assertEqual([row[2] for row in calls[:4]], ['写入文件']*4)
            self.assertEqual(extra['sandbox_files']['local_history_files'], 0)
            saved = json.dumps(suite.report.data)
            self.assertNotIn('fixture-user-', saved)
            self.assertNotIn(str(Path(root)), saved)
            self.assertNotIn('fixture-value-', saved)

    def test_wrong_scope_content_and_local_fallback_fail(self):
        with tempfile.TemporaryDirectory() as root:
            suite, _ = self.suite(root, misroute=True)
            with self.assertRaisesRegex(common.CheckFailure, 'sandbox_read_content_mismatch'):
                suite.sandbox_files()
            suite, _ = self.suite(root, local_history_files=1)
            with self.assertRaisesRegex(common.CheckFailure, 'sandbox_local_history_file_created'):
                suite.sandbox_files()
            self.assertNotIn('sandbox_files', suite.report.data)


class IdleStorageTests(unittest.TestCase):
    def samples(self, cpu_ms=24):
        return [dict(elapsed_ms=index*3000, cpu_ms=100+cpu_ms*index/4,
                     start_ticks=42, rss_bytes=1024+index) for index in range(5)]

    def counters(self, inserted=3):
        return dict(stats_reset='fixture-epoch', tables={name: dict(inserted=inserted,
            updated=4, deleted=1, autovacuum_count=100) for name in HIVE.IDLE_STORAGE_TABLES})

    def query_counters(self, service_calls=20, history_read_calls=10, **changes):
        value = dict(tracking=True, role_isolation=True, stats_reset='fixture-query-epoch',
                     dealloc=0, service_calls=service_calls, history_read_calls=history_read_calls,
                     active_history_reads=0)
        value.update(changes)
        return value

    def test_query_audit_detects_reads_and_rejects_reset_eviction_and_incomplete_scope(self):
        before = self.query_counters()
        quiet = HIVE.idle_query_deltas(before, self.query_counters(service_calls=22))
        self.assertEqual(quiet['service_statement_calls'], 2)
        self.assertEqual(quiet['history_select_calls'], 0)
        active = HIVE.idle_query_deltas(before, self.query_counters(
            service_calls=23, history_read_calls=12, active_history_reads=1))
        self.assertEqual(active['history_select_calls'], 2)
        self.assertEqual(active['active_history_reads'], 1)
        for changes, code in ((dict(stats_reset='fixture-other-epoch'), 'stats_reset'),
                              (dict(dealloc=1), 'stats_evicted'),
                              (dict(tracking=False), 'audit_unavailable'),
                              (dict(role_isolation=False), 'audit_unavailable'),
                              (dict(history_read_calls=9), 'counter_regressed'),
                              (dict(service_calls=-1), 'counter_invalid')):
            with self.subTest(code=code), self.assertRaisesRegex(common.CheckFailure, code):
                HIVE.idle_query_deltas(before, self.query_counters(**changes))

    def test_query_audit_self_test_requires_service_probe_and_excludes_auditor(self):
        for audit_calls, probe_calls, expected in ((10, 11, None),
                (11, 12, 'auditor_not_excluded'), (10, 10, 'service_probe_missing')):
            stack = HIVE.Stack.__new__(HIVE.Stack)
            stack.report = SimpleNamespace(data={})
            values = iter([self.query_counters(), self.query_counters(history_read_calls=audit_calls),
                           self.query_counters(service_calls=21, history_read_calls=probe_calls)])
            stack.idle_query_snapshot = lambda: next(values)
            requests = []
            stack.database_json = lambda sql, role='fixture_audit': requests.append(role)
            if expected:
                with self.assertRaisesRegex(common.CheckFailure, expected):
                    stack.verify_query_audit()
                self.assertNotIn('history_query_audit', stack.report.data)
            else:
                stack.verify_query_audit()
                self.assertEqual(requests, ['fixture_audit', 'fixture'])
                stack.verify_query_audit()  # A successful probe is performed once.
                self.assertEqual(len(requests), 2)
                proof = stack.report.data['history_query_audit']
                self.assertEqual(proof['service_role_probe_calls'], 1)
                self.assertEqual(proof['audit_role_probe_calls'], 0)

    def test_cpu_uses_process_delta_and_rejects_restart_or_short_observation(self):
        resources = HIVE.idle_resource_summary(self.samples())
        self.assertEqual(resources['cpu_ms'], 24)
        self.assertEqual(resources['single_core_cpu_percent'], .2)
        self.assertEqual(resources['rss_delta_bytes'], 4)
        changed = self.samples()
        changed[-1]['start_ticks'] += 1
        with self.assertRaisesRegex(common.CheckFailure, 'idle_server_restarted'):
            HIVE.idle_resource_summary(changed)
        changed = self.samples()
        changed[-1]['elapsed_ms'] = 9500
        with self.assertRaisesRegex(common.CheckFailure, 'idle_observation_duration'):
            HIVE.idle_resource_summary(changed)
        changed = self.samples()
        changed[-1]['cpu_ms'] = float('nan')
        with self.assertRaisesRegex(common.CheckFailure, 'idle_resource_sample_invalid'):
            HIVE.idle_resource_summary(changed)

    def test_dml_counts_detect_writes_and_exclude_maintenance(self):
        before, after = self.counters(), self.counters()
        after['tables'][HIVE.IDLE_STORAGE_TABLES[0]]['autovacuum_count'] += 9
        delta = HIVE.idle_write_deltas(before, after)
        self.assertEqual(sum(sum(v.values()) for v in delta.values()), 0)
        after['tables']['service_thread_admissions']['updated'] += 2
        self.assertEqual(HIVE.idle_write_deltas(before, after)['service_thread_admissions']['updated'], 2)
        after['stats_reset'] = 'fixture-new-epoch'
        with self.assertRaisesRegex(common.CheckFailure, 'idle_postgres_stats_reset'):
            HIVE.idle_write_deltas(before, after)
        after = self.counters(inserted=0)
        with self.assertRaisesRegex(common.CheckFailure, 'idle_postgres_counter_regressed'):
            HIVE.idle_write_deltas(before, after)
        del after['tables']['service_thread_owners']
        with self.assertRaises(common.CheckFailure):
            HIVE.idle_write_deltas(before, after)

    def test_long_observation_rejects_smoke_evidence_and_keeps_cpu_normalized(self):
        samples = [dict(elapsed_ms=index*150000, cpu_ms=100+300*index,
                        start_ticks=42, rss_bytes=1024+index) for index in range(5)]
        resources = HIVE.idle_resource_summary(samples, 600)
        self.assertEqual(resources['observation_ms'], 600000)
        self.assertEqual(resources['cpu_ms'], 1200)
        self.assertEqual(resources['single_core_cpu_percent'], .2)
        self.assertEqual(resources['samples'], 5)
        with self.assertRaisesRegex(common.CheckFailure, 'idle_observation_duration'):
            HIVE.idle_resource_summary(self.samples(), 600)
        with self.assertRaisesRegex(common.CheckFailure, 'idle_observation_duration'):
            HIVE.idle_resource_summary(samples)
        samples[-1]['elapsed_ms'] = 580000
        with self.assertRaisesRegex(common.CheckFailure, 'idle_observation_duration'):
            HIVE.idle_resource_summary(samples, 600)
        for invalid in (0, True, float('nan'), HIVE.IDLE_MAX_SAMPLE_SECONDS+1):
            with self.subTest(duration=invalid), self.assertRaisesRegex(
                    common.CheckFailure, 'idle_observation_argument'):
                HIVE.idle_resource_summary(self.samples(), invalid)

    def test_sampler_passes_requested_duration_with_bounded_output(self):
        stack = HIVE.Stack.__new__(HIVE.Stack)
        stack.args = SimpleNamespace(idle_seconds=600)
        captured = []
        def compose(command, timeout):
            captured.append((command, timeout))
            return '[]'
        stack.compose = compose
        self.assertEqual(stack.idle_process_samples(), [])
        self.assertEqual(captured[0][0][-1], '600')
        self.assertEqual(captured[0][1], 615)
        self.assertIn('range(5)', captured[0][0][-2])
        stack.args = SimpleNamespace()
        stack.idle_process_samples()
        self.assertEqual(captured[1][0][-1], '12')

    def suite(self, root, writes=0, cpu_ms=24, history_reads=0, active_history_reads=0):
        suite = HIVE.HiveSuite.__new__(HIVE.HiveSuite)
        suite.report = SimpleNamespace(data={})
        suite.create = lambda: ('fixture-session-private', 0)
        calls = []
        suite.stream = lambda session: calls.append(('stream', session))
        before, after = self.counters(), self.counters()
        after['tables']['service_thread_items']['inserted'] += writes
        values = iter([before, after])
        query_values = iter([self.query_counters(), self.query_counters(
            service_calls=20+history_reads, history_read_calls=10+history_reads,
            active_history_reads=active_history_reads)])
        suite.stack = SimpleNamespace(root=Path(root),
            database_json=lambda sql: dict(active=0),
            idle_write_snapshot=lambda: next(values),
            verify_query_audit=lambda: None,
            idle_query_snapshot=lambda: next(query_values),
            idle_process_samples=lambda: self.samples(cpu_ms))
        return suite, calls

    def test_quiet_case_preserves_sanitized_evidence_and_fails_real_activity(self):
        with tempfile.TemporaryDirectory() as root, patch.object(HIVE.time, 'sleep'):
            suite, calls = self.suite(root)
            metrics, checks, extra = suite.idle_storage()
            self.assertEqual(calls, [('stream', 'fixture-session-private')])
            self.assertEqual(metrics['idle_cpu_ms'], [24])
            self.assertGreater(checks, 0)
            saved = json.dumps(suite.report.data)
            self.assertNotIn('fixture-session-private', saved)
            self.assertNotIn(str(Path(root)), saved)
            self.assertEqual(extra['idle_storage']['total_business_writes'], 0)
            self.assertEqual(extra['idle_storage']['history_query_activity']['history_select_calls'], 0)
            suite, _ = self.suite(root, writes=1)
            with self.assertRaisesRegex(common.CheckFailure, 'idle_business_table_writes'):
                suite.idle_storage()
            self.assertEqual(suite.report.data['idle_storage']['total_business_writes'], 1)
            suite, _ = self.suite(root, cpu_ms=240)
            with self.assertRaisesRegex(common.CheckFailure, 'idle_server_cpu_busy'):
                suite.idle_storage()
            self.assertEqual(suite.report.data['idle_storage']['process']['single_core_cpu_percent'], 2)
            for completed, active in ((1, 0), (0, 1)):
                suite, _ = self.suite(root, history_reads=completed, active_history_reads=active)
                with self.assertRaisesRegex(common.CheckFailure, 'idle_history_query_activity'):
                    suite.idle_storage()
                self.assertEqual(suite.report.data['idle_storage']['total_business_writes'], 0)

    def test_acceptance_case_validates_requested_window_and_records_it(self):
        with tempfile.TemporaryDirectory() as root, patch.object(HIVE.time, 'sleep'):
            suite, _ = self.suite(root)
            suite.args = SimpleNamespace(idle_seconds=600)
            with self.assertRaisesRegex(common.CheckFailure, 'idle_observation_duration'):
                suite.idle_storage()
            suite, _ = self.suite(root)
            suite.args = SimpleNamespace(idle_seconds=600)
            suite.stack.idle_process_samples = lambda: [dict(elapsed_ms=index*150000,
                cpu_ms=100+300*index, start_ticks=42, rss_bytes=1024+index) for index in range(5)]
            metrics, _, extra = suite.idle_storage()
            self.assertEqual(metrics['idle_observation_ms'], [600000])
            self.assertEqual(extra['idle_storage']['requested_observation_seconds'], 600)
            self.assertEqual(extra['idle_storage']['process']['single_core_cpu_percent'], .2)


class ModelTests(unittest.TestCase):
    def test_sse_content_and_private_trace(self):
        model = Model(('127.0.0.1', 0), chunks=3, interval_ms=1)
        thread = threading.Thread(target=model.serve_forever, daemon=True)
        thread.start()
        try:
            url = 'http://127.0.0.1:{}'.format(model.server_port)
            payload = dict(stream=True, messages=[dict(role='user', content='fixture-run-123')])
            req = Request(url+'/v1/chat/completions', data=json.dumps(payload).encode(),
                          headers={'Content-Type': 'application/json'})
            with urlopen(req, timeout=5) as response:
                data = response.read().decode()
            chunks = [json.loads(line[6:]) for line in data.splitlines()
                      if line.startswith('data: ') and line != 'data: [DONE]']
            text = ''.join(c['choices'][0]['delta'].get('content', '') for c in chunks)
            self.assertEqual(text, expected(3))
            trace = model.trace('fixture-run-123')
            self.assertTrue(trace['completed'])
            self.assertEqual(trace['calls'], 1)
            self.assertLess(trace['request_unix_ns'], trace['first_unix_ns'])
            self.assertNotIn('messages', trace)
        finally:
            model.shutdown()
            model.server_close()
            thread.join(2)


if __name__ == '__main__':
    unittest.main()
