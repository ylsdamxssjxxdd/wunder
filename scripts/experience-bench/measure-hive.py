#!/usr/bin/env python3
"""Isolated Compose + actual hive HTTP/WebSocket API experience tests."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import json
import math
from pathlib import Path
import secrets
import shutil
import sys
import time
from urllib.parse import quote
import uuid

import requests
import websocket

from common import (PROFILES, REPO, CheckFailure, Report, clean_env, digest,
                    model_config, require, run_process, write_json)
from mock_model import expected


IDLE_STORAGE_TABLES = (
    'service_threads', 'service_thread_sessions', 'service_thread_records',
    'service_thread_owners', 'service_thread_admissions', 'service_thread_items',
    'service_thread_chat_heads', 'service_thread_display_state')
IDLE_SAMPLE_SECONDS = 12
IDLE_MAX_SAMPLE_SECONDS = 3600
IDLE_MAX_CPU_PERCENT = 1.0  # One fully occupied CPU core is 100%.
# Matched inside PostgreSQL only. Reports never contain query text/parameters.
HISTORY_READ_PATTERN = (r'(^|[^[:alnum:]_])(select|with)([^[:alnum:]_]|$)',
                        r'(^|[^[:alnum:]_])(service_threads|service_thread_[[:alnum:]_]+)([^[:alnum:]_]|$)')

# Executed inside the owned server container. Only PID 1's CPU and resident
# memory are read; the sampler and docker exec processes do not enter totals.
IDLE_PROCESS_SAMPLER = r'''
import json, os, sys, time
clock_ticks = os.sysconf('SC_CLK_TCK')
page_bytes = os.sysconf('SC_PAGE_SIZE')
duration = int(sys.argv[1]) if len(sys.argv) > 1 else 12
began = time.monotonic()
samples = []
for index in range(5):
    deadline = began + index * duration / 4
    time.sleep(max(0, deadline-time.monotonic()))
    fields = open('/proc/1/stat').read().rsplit(')', 1)[1].split()
    resident = int(open('/proc/1/statm').read().split()[1]) * page_bytes
    samples.append(dict(elapsed_ms=(time.monotonic()-began)*1000,
        cpu_ms=(int(fields[11])+int(fields[12]))*1000/clock_ticks,
        start_ticks=int(fields[19]), rss_bytes=resident))
print(json.dumps(samples))
'''


def idle_resource_summary(samples, expected_seconds=IDLE_SAMPLE_SECONDS):
    require(isinstance(expected_seconds, (int, float)) and not isinstance(expected_seconds, bool)
            and math.isfinite(expected_seconds)
            and IDLE_SAMPLE_SECONDS <= expected_seconds <= IDLE_MAX_SAMPLE_SECONDS,
            'idle_observation_argument')
    require(isinstance(samples, list) and 4 <= len(samples) <= 8, 'idle_resource_sample_count')
    for sample in samples:
        require(isinstance(sample, dict) and all(
            isinstance(sample.get(key), (int, float)) and not isinstance(sample.get(key), bool)
            and math.isfinite(sample[key]) and sample[key] >= 0
            for key in ('elapsed_ms', 'cpu_ms', 'start_ticks', 'rss_bytes')), 'idle_resource_sample_invalid')
        require(sample['rss_bytes'] > 0, 'idle_resource_memory_missing')
    first, last = samples[0], samples[-1]
    require(all(s['start_ticks'] == first['start_ticks'] for s in samples), 'idle_server_restarted')
    require(all(b['elapsed_ms'] > a['elapsed_ms'] and b['cpu_ms'] >= a['cpu_ms']
                for a, b in zip(samples, samples[1:])), 'idle_resource_counter_regressed')
    elapsed = last['elapsed_ms']-first['elapsed_ms']
    expected_ms = expected_seconds*1000
    # Preserve the smoke window's original tolerance, but require a long run
    # to cover the requested duration instead of accepting a short smoke sample.
    require(expected_ms-max(2000, expected_ms*.02) <= elapsed <=
            expected_ms+max(3000, expected_ms*.02), 'idle_observation_duration')
    cpu = last['cpu_ms']-first['cpu_ms']
    return dict(observation_ms=elapsed, cpu_ms=cpu, single_core_cpu_percent=cpu/elapsed*100,
                peak_rss_bytes=max(s['rss_bytes'] for s in samples),
                rss_delta_bytes=last['rss_bytes']-first['rss_bytes'], samples=len(samples))


def idle_write_deltas(before, after):
    require(before.get('stats_reset') == after.get('stats_reset'), 'idle_postgres_stats_reset')
    output = {}
    for table in IDLE_STORAGE_TABLES:
        counters = {}
        for key in ('inserted', 'updated', 'deleted'):
            a, b = before.get('tables', {}).get(table, {}).get(key), after.get('tables', {}).get(table, {}).get(key)
            require(all(isinstance(v, int) and not isinstance(v, bool) and v >= 0 for v in (a, b)),
                    'idle_postgres_table_stats_missing')
            require(b >= a, 'idle_postgres_counter_regressed')
            counters[key] = b-a
        output[table] = counters
    return output


def idle_query_deltas(before, after):
    for snapshot in (before, after):
        require(snapshot.get('tracking') is True and snapshot.get('role_isolation') is True,
                'idle_query_audit_unavailable')
        require(isinstance(snapshot.get('stats_reset'), str) and snapshot['stats_reset'],
                'idle_query_stats_epoch_missing')
        for key in ('dealloc', 'service_calls', 'history_read_calls', 'active_history_reads'):
            value = snapshot.get(key)
            require(isinstance(value, int) and not isinstance(value, bool) and value >= 0,
                    'idle_query_counter_invalid')
    require(before['stats_reset'] == after['stats_reset'], 'idle_query_stats_reset')
    require(before['dealloc'] == after['dealloc'], 'idle_query_stats_evicted')
    require(before['active_history_reads'] == 0, 'idle_query_baseline_active')
    delta = {}
    for source, target in (('service_calls', 'service_statement_calls'),
                           ('history_read_calls', 'history_select_calls')):
        require(after[source] >= before[source], 'idle_query_counter_regressed')
        delta[target] = after[source]-before[source]
    delta.update(active_history_reads=after['active_history_reads'],
                 statistics_evictions=0, audit_role_excluded=True)
    return delta


def poll(work, timeout=60):
    end = time.perf_counter() + timeout
    while time.perf_counter() < end:
        value = work()
        if value:
            return value
        time.sleep(.05)
    raise CheckFailure('poll_deadline')


def assert_postgres_only_files(root):
    """Audit only the owned application's data directory, never workspaces."""
    root = Path(root)
    if not root.exists():
        return
    visited = 0
    for path in root.rglob('*'):
        visited += 1
        require(visited <= 4096, 'storage_file_audit_budget')
        name = path.name.lower()
        forbidden = (name.endswith(('.sqlite', '.sqlite-wal', '.sqlite-shm', '.db',
                                    '.db-wal', '.db-shm', '.jsonl', '.jsonl.zst')) or
                     name in ('.canonical-root-id.json', '.canonical-store.lock'))
        require(not forbidden, 'service_local_history_file_created')


class Stack:
    def __init__(self, args, report):
        self.args, self.report = args, report
        self.project = 'wunder-exp-' + uuid.uuid4().hex[:12]
        self.root = report.output / 'fixture'
        (self.root / 'runtime').mkdir(parents=True)
        (self.root / 'workspaces').mkdir()
        (self.root / 'sandbox-data').mkdir()
        assets = self.root / 'runtime' / 'config'
        assets.mkdir()
        write_json(assets / 'org_units.json', [dict(name='fixture-unit')])
        shutil.copytree(REPO / 'config/prompts', assets / 'prompts')
        self.key, self.password = secrets.token_hex(24), secrets.token_hex(24)
        # Separate SQL principals make audit queries invisible to service-role
        # counters, including our own history verification and active-state polls.
        init = self.root / 'postgres-init'
        init.mkdir()
        (init / '10-fixture.sql').write_text(
            "CREATE ROLE fixture LOGIN PASSWORD '{}';\n".format(self.password) +
            'ALTER DATABASE fixture OWNER TO fixture;\n' +
            'CREATE EXTENSION pg_stat_statements;\n', encoding='utf-8')
        config = model_config('http://model:18090/v1')
        config.update(server=dict(host='0.0.0.0', port=18000, stream_chunk_size=1024,
                                  max_active_sessions=16),
                      security=dict(api_key=self.key),
                      storage=dict(backend='postgres', data_root='/fixture/data'),
                      agent_queue=dict(enabled=True))
        write_json(self.root / 'config.json', config)
        envfile = self.root / 'compose.env'
        values = dict(FIXTURE_ROOT=self.root.as_posix(), FIXTURE_BINARY=args.server_binary.as_posix(),
                      FIXTURE_DRIVER=Path(__file__).resolve().parent.as_posix(), FIXTURE_IMAGE=args.image,
                      FIXTURE_PASSWORD=self.password, FIXTURE_CHUNKS=str(args.chunks))
        # Compose's single-quoted env values preserve spaces and dollars literally.
        require(all("'" not in v and '\n' not in v for v in values.values()), 'unsupported_fixture_argument')
        envfile.write_text(''.join("{}='{}'\n".format(k, v) for k, v in values.items()), encoding='utf-8')
        self.command = ['docker', 'compose', '--project-name', self.project, '--file',
                        str(REPO / 'packaging/docker/docker-compose-experience.yml'), '--env-file', str(envfile)]
        self.env = clean_env()
        for key in list(self.env):
            if key.startswith('COMPOSE_') or key.startswith('FIXTURE_'):
                del self.env[key]
        self.http = requests.Session()
        self.http.trust_env = False
        self.attempted = False

    def compose(self, args, timeout=180):
        result = run_process(self.command + args, REPO, env=self.env, timeout=timeout)
        require(result['exit_code'] == 0, 'compose_command_failed')
        return result['stdout']

    def start(self):
        self.compose(['config', '--quiet'])
        self.attempted = True
        began = time.perf_counter()
        self.compose(['up', '-d', '--pull', 'never'], timeout=180)
        self.url = 'http://' + self.compose(['port', 'server', '18000']).strip()
        self.model_url = 'http://' + self.compose(['port', 'model', '18090']).strip()
        self.sandbox_url = 'http://' + self.compose(['port', 'sandbox', '9001']).strip()
        def ready():
            try:
                return self.http.get(self.url + '/wunder/i18n', timeout=2).status_code == 200 and self.http.get(
                    self.model_url + '/health', timeout=2).status_code == 200 and self.http.get(
                    self.sandbox_url + '/health', timeout=2).status_code == 200
            except requests.RequestException:
                return False
        poll(ready, 120)
        self.report.data['compose_to_ready_ms'] = (time.perf_counter()-began)*1000
        self.report.data['artifacts'] = dict(server_sha256=digest(self.args.server_binary))
        # No container env, logs, host paths or generated credentials in reports.
        self.report.data['docker'] = dict(
            engine=self.docker(['version', '--format', '{{.Server.Version}}']).strip(),
            compose=self.docker(['compose', 'version', '--short']).strip(),
            runtime_image_id=self.docker(['image', 'inspect', self.args.image, '--format', '{{.Id}}']).strip(),
            postgres_image_id=self.docker(['image', 'inspect', 'postgres:16', '--format', '{{.Id}}']).strip())
        self.report.data['docker']['capacity'] = self.docker(
            ['info', '--format', '{{.NCPU}} {{.MemTotal}} {{.Architecture}} {{.OSType}}']).strip()
        self.report.data['execution'].update(self.report.data['docker'])

    def docker(self, args):
        result = run_process(['docker'] + args, REPO, env=self.env, timeout=60)
        require(result['exit_code'] == 0, 'docker_command_failed')
        return result['stdout']

    def database_json(self, sql, role='fixture_audit'):
        # Fixed, driver-owned SQL against this run's separate database volume.
        # Neither the DSN nor generated password is printed or saved in reports.
        require(role in ('fixture_audit', 'fixture'), 'invalid_fixture_database_role')
        raw = self.compose(['exec', '-T', 'postgres', 'psql', '-X', '-A', '-t',
                            '-v', 'ON_ERROR_STOP=1', '-U', role, '-d', 'fixture',
                            '-c', sql])
        return json.loads(raw.strip())

    def idle_query_snapshot(self):
        read, relation = HISTORY_READ_PATTERN
        # Only aggregates leave PostgreSQL; bounded pg_stat_statements storage is
        # checked for resets/evictions so lost history cannot masquerade as quiet.
        return self.database_json("""WITH service AS (
            SELECT calls,query ~* '""" + read + "' AND query ~* '" + relation + """' AS history_read
            FROM pg_stat_statements WHERE userid=(SELECT oid FROM pg_roles WHERE rolname='fixture')
              AND dbid=(SELECT oid FROM pg_database WHERE datname=current_database()))
            SELECT json_build_object(
              'tracking',current_setting('pg_stat_statements.track')='all'
                AND current_setting('compute_query_id')='on',
              'role_isolation',current_user='fixture_audit' AND COALESCE(
                (SELECT NOT rolsuper FROM pg_roles WHERE rolname='fixture'),false),
              'stats_reset',(SELECT stats_reset::text FROM pg_stat_statements_info),
              'dealloc',(SELECT dealloc FROM pg_stat_statements_info),
              'service_calls',(SELECT COALESCE(sum(calls),0) FROM service),
              'history_read_calls',(SELECT COALESCE(sum(calls) FILTER(WHERE history_read),0) FROM service),
              'active_history_reads',(SELECT count(*) FROM pg_stat_activity
                WHERE datname=current_database() AND usename='fixture' AND state='active'
                  AND query ~* '""" + read + "' AND query ~* '" + relation + "'))")

    def verify_query_audit(self):
        if getattr(self, '_query_audit_verified', False):
            return
        probe = "SELECT json_build_object('rows',count(*)) FROM service_thread_records WHERE false"
        before = self.idle_query_snapshot()
        self.database_json(probe)  # Same history read under the audit principal.
        middle = self.idle_query_snapshot()
        audit_delta = idle_query_deltas(before, middle)
        require(audit_delta['history_select_calls'] == 0, 'idle_query_auditor_not_excluded')
        self.database_json(probe, role='fixture')
        after = self.idle_query_snapshot()
        service_delta = idle_query_deltas(middle, after)
        require(service_delta['history_select_calls'] >= 1, 'idle_query_service_probe_missing')
        self.report.data['history_query_audit'] = dict(backend='pg_stat_statements',
            service_role_probe_calls=service_delta['history_select_calls'],
            audit_role_probe_calls=audit_delta['history_select_calls'],
            scope='service-role-history-select-with', bounded_statistics=True)
        self._query_audit_verified = True

    def idle_write_snapshot(self):
        # Read-only table DML counters exclude reads, connection/transaction
        # checks, vacuum and analyze. Never use global WAL or disk write bytes:
        # those would also include PostgreSQL maintenance and this audit.
        names = ','.join("'" + name + "'" for name in IDLE_STORAGE_TABLES)
        return self.database_json("""SELECT json_build_object(
            'stats_reset',(SELECT stats_reset::text FROM pg_stat_database WHERE datname=current_database()),
            'tables',(SELECT json_object_agg(relname,json_build_object(
                'inserted',n_tup_ins,'updated',n_tup_upd,'deleted',n_tup_del))
              FROM pg_stat_user_tables WHERE schemaname='public' AND relname IN (""" + names + ')))')

    def idle_process_samples(self):
        seconds = getattr(self.args, 'idle_seconds', IDLE_SAMPLE_SECONDS)
        raw = self.compose(['exec', '-T', 'server', 'python3', '-c', IDLE_PROCESS_SAMPLER,
                            str(seconds)], timeout=seconds+15)
        return json.loads(raw.strip())

    def sandbox_storage_audit(self):
        # Audit both the configured root and the old default inside this owned
        # container. Do not scan workspaces, where user-created .db is legitimate.
        source = r'''
import json
from pathlib import Path
visited, forbidden = 0, 0
for root in (Path('/fixture/data'), Path('/app/config/data')):
    if not root.exists():
        continue
    for path in root.rglob('*'):
        visited += 1
        if visited > 4096:
            raise RuntimeError('sandbox_storage_audit_budget')
        name = path.name.lower()
        forbidden += int(name.endswith(('.sqlite', '.sqlite-wal', '.sqlite-shm',
            '.db', '.db-wal', '.db-shm', '.jsonl', '.jsonl.zst')) or
            name in ('.canonical-root-id.json', '.canonical-store.lock'))
print(json.dumps(dict(checked_entries=visited, local_history_files=forbidden)))
'''
        return json.loads(self.compose(['exec', '-T', 'sandbox', 'python3', '-c', source]).strip())

    def close(self):
        try:
            if self.attempted:
                self.compose(['down', '--volumes', '--remove-orphans', '--timeout', '10'])
        finally:
            self.http.close()
            # Delete only generated credentials; preserve synthetic data for diagnosis.
            for filename in ('compose.env', 'config.json'):
                (self.root / filename).unlink(missing_ok=True)
            (self.root / 'postgres-init' / '10-fixture.sql').unlink(missing_ok=True)


class HiveSuite:
    def __init__(self, args, report, stack):
        self.args, self.report, self.stack = args, report, stack
        self.url = stack.url
        self.http = requests.Session()
        self.http.trust_env = False
        self.credentials = []
        self.large_session = None

    def api(self, method, path, token=None, data=None, statuses=(200,), admin=False):
        headers = {}
        if token:
            headers['Authorization'] = 'Bearer ' + token
        if admin:
            headers['X-API-Key'] = self.stack.key
        began = time.perf_counter()
        with self.http.request(method, self.url + path, json=data, headers=headers, timeout=(5, 60), stream=True) as response:
            chunks, size = [], 0
            for chunk in response.iter_content(65536):
                size += len(chunk)
                require(size <= 2*1024*1024, 'api_response_budget')
                chunks.append(chunk)
            elapsed = (time.perf_counter()-began)*1000
            require(response.status_code in statuses, 'api_status_{}'.format(response.status_code))
            payload = json.loads(b''.join(chunks))
        return payload, elapsed, size

    def setup(self):
        for index in range(3):
            credentials = dict(username='fixture-' + uuid.uuid4().hex[:16], password=secrets.token_hex(20))
            if index == 0:
                self.api('POST', '/wunder/admin/user_accounts', data=dict(**credentials, roles=['admin']), admin=True)
            else:
                self.api('POST', '/wunder/auth/register', data=credentials)
            payload, _, _ = self.api('POST', '/wunder/auth/login', data=credentials)
            self.credentials.append(dict(**credentials, token=payload['data']['access_token'],
                                         user_id=payload['data']['user']['id']))
        self.admin, self.user, self.other = [c['token'] for c in self.credentials]

    def create(self, token=None):
        payload, ms, _ = self.api('POST', '/wunder/chat/sessions', token or self.user, dict(title='fixture-thread'))
        session = payload['data']['id']
        require(isinstance(session, str) and session, 'api_session_identity')
        return session, ms

    def page(self, session, token=None, suffix='/view/bootstrap?kind=chat'):
        payload, ms, size = self.api('GET', '/wunder/chat/sessions/' + session + suffix, token or self.user)
        page = payload['data']['page']
        if page['session_id'] != session:
            # Keep identity proof without persisting synthetic identifiers.
            raise CheckFailure('api_page_wrong_session_len_{}_{}'.format(
                len(str(page.get('session_id', ''))), len(str(session))))
        require(not page.get('degraded') and page.get('projection_state') != 'degraded',
                'api_page_degraded_{}'.format(page.get('diagnostic') or 'unspecified'))
        require(len(page['chat']) <= 20, 'api_chat_page_unbounded')
        return page, ms, size

    def body(self, session, message, token=None):
        ref = message.get('body')
        if not ref:
            return message['content'], [], 0
        text, cursor, timings, total = [], 0, [], 0
        for _ in range(256):
            path = '/wunder/chat/sessions/{}/bodies/{}?revision={}&cursor={}'.format(
                session, quote(ref['body_id'], safe=''), ref['revision'], cursor)
            result, ms, size = self.api('GET', path, token or self.user)
            data = result['data']
            require(data['body_id'] == ref['body_id'] and data['revision'] == ref['revision'], 'api_body_identity')
            text.extend(data['chunks'])
            total += size
            require(total <= 2*1024*1024, 'api_body_budget')
            timings.append(ms)
            if not data['has_more']:
                return ''.join(text), timings, total
            require(data['next_cursor'] > cursor, 'api_body_cursor_stalled')
            cursor = data['next_cursor']
        raise CheckFailure('api_body_page_limit')

    def login(self):
        c = self.credentials[1]
        result, ms, _ = self.api('POST', '/wunder/auth/login', data=dict(username=c['username'], password=c['password']))
        require(result['data']['access_token'] and result['data']['user']['id'], 'api_login_contract')
        # Login rotates this account's credential; subsequent cases must use it.
        self.user = result['data']['access_token']
        c['token'] = self.user
        return dict(login_ms=[ms]), 2, {}

    def session_catalog(self):
        session, create_ms = self.create()
        result, list_ms, size = self.api('GET', '/wunder/chat/sessions?offset=0&limit=12', self.user)
        rows = result['data']['items']
        require(0 < len(rows) <= 12 and any(r['id'] == session for r in rows), 'api_catalog_missing_session')
        page, bootstrap_ms, _ = self.page(session)
        require(not page['chat'], 'api_new_thread_not_empty')
        return dict(create_ms=[create_ms], catalog_ms=[list_ms], empty_bootstrap_ms=[bootstrap_ms]), 4, dict(response_bytes=size)

    def socket(self, token=None):
        ws = websocket.create_connection(self.url.replace('http://', 'ws://') + '/wunder/chat/ws',
                header=['Authorization: Bearer ' + (token or self.user)], subprotocols=['wunder'],
                timeout=30, http_proxy_host=None, http_no_proxy=['127.0.0.1', 'localhost'])
        try:
            require(json.loads(ws.recv())['type'] == 'ready', 'ws_initial_ready')
            ws.send(json.dumps(dict(type='connect', request_id='fixture-connect', payload=dict(protocol_version=1))))
            require(json.loads(ws.recv())['type'] == 'ready', 'ws_connect_ready')
            return ws
        except Exception:
            ws.close()
            raise

    def start(self, ws, session, key):
        ws.send(json.dumps(dict(type='start', request_id=key, session_id=session,
                payload=dict(session_id=session, content=key, client_message_id=key, stream=True, view_seq=0))))

    def stream(self, session=None, token=None, slow_ms=0):
        session = session or self.create(token)[0]
        key = 'fixture-run-' + uuid.uuid4().hex
        ws = self.socket(token)
        began, began_unix_ns, first, texts, events = time.perf_counter(), time.time_ns(), None, [], 0
        terminal_ms = None
        try:
            self.start(ws, session, key)
            for _ in range(4096):
                require(time.perf_counter()-began < 60, 'ws_stream_deadline')
                raw = ws.recv()
                require(len(raw) <= 1024*1024, 'ws_frame_budget')
                envelope = json.loads(raw)
                require(envelope['type'] != 'error', 'ws_error')
                if envelope.get('request_id') != key or envelope['type'] != 'event':
                    continue
                event = envelope['payload']
                name, data = event['event'], event.get('data', {})
                if isinstance(data.get('data'), dict):
                    data = data['data']
                events += 1
                if name == 'thread_item_tail' and data.get('field') == 'content' and data.get('text'):
                    if first is None:
                        first = (time.perf_counter()-began)*1000
                    require(data['offset'] == len(''.join(texts).encode('utf-16-le')) // 2, 'ws_tail_offset')
                    texts.append(data['text'])
                if name == 'final':
                    terminal_ms = (time.perf_counter()-began)*1000
                    break
                if slow_ms:
                    time.sleep(slow_ms / 1000)
            else:
                raise CheckFailure('ws_frame_count_limit')
        finally:
            ws.close()
        require(terminal_ms is not None, 'ws_terminal_missing')
        require(first is not None and len(texts) >= 2, 'ws_not_streaming')
        require(''.join(texts) == expected(self.args.chunks), 'ws_tail_text_mismatch')
        page, durable_ms, size = self.page(session, token)
        require(page['chat'], 'api_completed_turn_missing')
        require(page['chat'][-1]['user']['content'] == key, 'api_durable_prompt_mismatch')
        text, _, _ = self.body(session, page['chat'][-1]['assistant'], token)
        require(text == expected(self.args.chunks), 'api_durable_answer_mismatch')
        with self.http.get(self.stack.model_url + '/trace/' + key, timeout=5) as response:
            trace = response.json()
        require(trace.get('calls') == 1 and trace.get('completed'), 'api_provider_calls')
        return dict(send_to_first_text_ms=[first], send_to_provider_ms=[(trace['request_unix_ns'] - began_unix_ns) / 1e6],
                    send_to_terminal_ms=[terminal_ms], durable_read_ms=[durable_ms],
                    provider_to_first_ms=[(trace['first_unix_ns']-trace['request_unix_ns'])/1e6],
                    provider_stream_ms=[(trace['last_unix_ns']-trace['first_unix_ns'])/1e6]), 8, dict(
            events=events, deltas=len(texts), response_bytes=size)

    def slow_consumer(self):
        return self.stream(slow_ms=20)

    def parallel_threads(self):
        # Separate clients avoid sharing a requests connection pool across workers.
        def run(token):
            client = HiveSuite(self.args, self.report, self.stack)
            client.user = token
            try:
                return client.stream(token=token)
            finally:
                client.http.close()
        began = time.perf_counter()
        with ThreadPoolExecutor(max_workers=2) as executor:
            values = list(executor.map(run, [self.user, self.other]))
        metrics = dict(batch_ms=[(time.perf_counter()-began)*1000])
        for result, _, _ in values:
            for name, numbers in result.items():
                metrics.setdefault(name, []).extend(numbers)
        return metrics, sum(v[1] for v in values), dict(workers=2)

    def isolation(self):
        session, _ = self.create()
        times = []
        for path in ['/view/bootstrap?kind=chat', '/queue']:
            _, ms, _ = self.api('GET', '/wunder/chat/sessions/' + session + path, self.other,
                                statuses=(403, 404))
            times.append(ms)
        _, ms, _ = self.api('GET', '/wunder/chat/sessions', statuses=(401,))
        return dict(cross_user_rejection_ms=times, anonymous_rejection_ms=[ms]), 3, {}

    def cancel_resume(self):
        session, _ = self.create()
        ws = self.socket()
        key = 'fixture-hold-' + uuid.uuid4().hex
        try:
            self.start(ws, session, key)
            def generating():
                with self.http.get(self.stack.model_url + '/trace/' + key, timeout=5) as response:
                    return response.json().get('first_unix_ns')
            poll(generating, 30)
            began = time.perf_counter()
            result, ack_ms, _ = self.api('POST', '/wunder/chat/sessions/' + session + '/cancel', self.user)
            require(result['data']['cancelled'], 'api_cancel_not_acknowledged')
            def settled():
                page, _, _ = self.page(session)
                return page if page['chat'] and page['chat'][-1]['status'] in ('cancelled', 'interrupted') else None
            poll(settled, 30)
            settled_ms = (time.perf_counter()-began)*1000
        finally:
            ws.close()
        metrics, checks, extra = self.stream(session)
        metrics.update(cancel_ack_ms=[ack_ms], cancel_to_settled_ms=[settled_ms])
        return metrics, checks+2, extra

    def queue(self):
        session, _ = self.create()
        ws = self.socket()
        key = 'fixture-hold-' + uuid.uuid4().hex
        queued = []
        timings = []
        try:
            self.start(ws, session, key)
            def generating():
                with self.http.get(self.stack.model_url + '/trace/' + key, timeout=5) as response:
                    return response.json().get('first_unix_ns')
            poll(generating, 30)
            base = '/wunder/chat/sessions/' + session
            for _ in range(2):
                content = 'fixture-queued-' + uuid.uuid4().hex
                payload, ms, _ = self.api('POST', base + '/messages', self.user,
                    dict(content=content, client_message_id=content, stream=False), statuses=(202,))
                require(payload['data']['queued'], 'api_queue_not_accepted')
                queued.append(payload['data']['queue_id'])
                timings.append(ms)
            result, read_ms, _ = self.api('GET', base + '/queue', self.user)
            require({r['queue_id'] for r in result['data']['items']} == set(queued), 'api_queue_missing_items')
            _, reorder_ms, _ = self.api('POST', base + '/queue/reorder', self.user, dict(queue_ids=list(reversed(queued))))
            result, _, _ = self.api('GET', base + '/queue', self.user)
            require([r['queue_id'] for r in result['data']['items']] == list(reversed(queued)), 'api_queue_order')
            _, cancel_ms, _ = self.api('POST', base + '/queue/' + queued[0] + '/cancel', self.user)
            result, _, _ = self.api('GET', base + '/queue', self.user)
            require([r['queue_id'] for r in result['data']['items']] == [queued[1]], 'api_queue_cancel_missing')
        finally:
            self.api('POST', '/wunder/chat/sessions/' + session + '/cancel', self.user)
            ws.close()
        return dict(enqueue_ms=timings, queue_read_ms=[read_ms], reorder_ms=[reorder_ms],
                    queued_cancel_ms=[cancel_ms]), 6, {}

    def workspace(self):
        # Fixture writes are ordinary APIs and excluded from list/read timing.
        folder = 'fixture-' + uuid.uuid4().hex[:12]
        self.api('POST', '/wunder/workspace/dir', self.user, dict(path=folder))
        for i in range(24):
            self.api('POST', '/wunder/workspace/file', self.user, dict(
                path=folder + '/fixture-{:03d}.txt'.format(i), content='fixture-content-{}'.format(i),
                create_if_missing=True))
        path = '/wunder/workspace?path=' + folder + '&limit=12&sort_by=name&order=asc'
        first, first_ms, first_bytes = self.api('GET', path + '&offset=0&refresh_tree=true', self.user)
        older, older_ms, older_bytes = self.api('GET', path + '&offset=12', self.user)
        require(first['total'] == older['total'] == 24 and
                len(first['entries']) == len(older['entries']) == 12, 'api_workspace_page_count')
        a, b = [e['name'] for e in first['entries']], [e['name'] for e in older['entries']]
        require(not set(a).intersection(b) and a + b == sorted(a+b), 'api_workspace_page_order')
        content, content_ms, _ = self.api('GET', '/wunder/workspace/content?path=' + folder + '/fixture-000.txt', self.user)
        require(content['content'] == 'fixture-content-0', 'api_workspace_content')
        return dict(first_directory_page_ms=[first_ms], older_directory_page_ms=[older_ms],
                    file_read_ms=[content_ms]), 4, dict(entries=24, response_bytes=[first_bytes, older_bytes])

    def sandbox_tool(self, request):
        # Each concurrent operation owns its HTTP session. Never retain response
        # bodies or errors in reports; they may include content or tool paths.
        with requests.Session() as client:
            client.trust_env = False
            began = time.perf_counter()
            with client.post(self.stack.sandbox_url + '/sandboxes/execute_tool', json=request,
                             timeout=(5, 60), stream=True) as response:
                chunks, size = [], 0
                for chunk in response.iter_content(65536):
                    size += len(chunk)
                    require(size <= 256*1024, 'sandbox_response_budget')
                    chunks.append(chunk)
                require(response.status_code == 200, 'sandbox_http_status')
                payload = json.loads(b''.join(chunks))
            require(payload.get('ok') is True, 'sandbox_file_tool_failed')
            result = payload.get('data')
            require(isinstance(result, dict) and result.get('ok') is True,
                    'sandbox_file_result_failed')
            return result.get('data', {}), (time.perf_counter()-began)*1000

    def sandbox_files(self):
        folder = 'fixture-' + uuid.uuid4().hex
        path = folder + '/value.txt'
        scopes = []
        for credentials in self.credentials[1:3]:
            for container in (1, 2):
                user_id = credentials['user_id']
                workspace = '{}__c__{}'.format(user_id, container)
                scopes.append(dict(user_id=user_id, workspace=workspace,
                                   content='fixture-value-' + uuid.uuid4().hex))
        require(len(scopes) == 4 and len({s['workspace'] for s in scopes}) == 4,
                'sandbox_isolation_fixture')

        def execute(scope, write):
            request = dict(user_id=scope['user_id'], session_id='fixture-file-tools',
                language='en', workspace_root='/fixture/workspaces/' + scope['workspace'],
                container_root='/fixture/workspaces', tool='写入文件' if write else '读取文件',
                args=dict(path=path, content=scope['content']) if write else
                     dict(path=path, start_line=1, end_line=1))
            data, elapsed = self.sandbox_tool(request)
            if not write:
                require(data.get('content') == '>>> {}\n1: {}'.format(path, scope['content']),
                        'sandbox_read_content_mismatch')
            return elapsed

        # Complete all concurrent writes before any reads. A shared routing table
        # or root then exposes the last writer instead of four isolated values.
        with ThreadPoolExecutor(max_workers=4) as pool:
            writes = list(pool.map(lambda scope: execute(scope, True), scopes))
            reads = list(pool.map(lambda scope: execute(scope, False), scopes))
        for scope in scopes:
            target = self.stack.root / 'workspaces' / scope['workspace'] / path
            require(target.is_file() and target.read_text(encoding='utf-8') == scope['content'],
                    'sandbox_workspace_isolation')
        audit = self.stack.sandbox_storage_audit()
        require(audit['local_history_files'] == 0, 'sandbox_local_history_file_created')
        assert_postgres_only_files(self.stack.root / 'sandbox-data')
        evidence = dict(users=2, containers_per_user=2, concurrent_scopes=4,
                        successful_writes=len(writes), successful_reads=len(reads),
                        local_history_files=audit['local_history_files'])
        self.report.data['sandbox_files'] = evidence
        return dict(sandbox_write_ms=writes, sandbox_read_ms=reads), 14, dict(sandbox_files=evidence)

    def restart_recovery(self):
        session, _ = self.create()
        self.stream(session)
        before = self.canonical_fingerprint(session)
        began = time.perf_counter()
        self.stack.compose(['restart', 'server'], timeout=90)
        # Docker may reassign an automatically published port on container restart.
        self.url = self.stack.url = 'http://' + self.stack.compose(['port', 'server', '18000']).strip()
        def recovered():
            try:
                return self.stack.http.get(self.url + '/wunder/i18n', timeout=2).status_code == 200
            except requests.RequestException:
                return False
        poll(recovered, 90)
        ready_ms = (time.perf_counter()-began)*1000
        page, read_ms, _ = self.page(session)
        content, _, _ = self.body(session, page['chat'][-1]['assistant'])
        require(content == expected(self.args.chunks), 'api_restart_data_loss')
        require(self.canonical_fingerprint(session) == before, 'postgres_restart_history_changed')
        assert_postgres_only_files(self.stack.root / 'data')
        metrics, checks, extra = self.stream(session)
        metrics.update(restart_to_ready_ms=[ready_ms], restored_chat_read_ms=[read_ms])
        return metrics, checks+3, extra

    def canonical_fingerprint(self, session):
        # Session identity is generated by the isolated service. Quote it as a
        # SQL literal; the returned proof contains counts/hash only.
        literal = "'" + str(session).replace("'", "''") + "'"
        return self.stack.database_json("""SELECT json_build_object(
            'records',count(*),'next_ordinal',COALESCE(max(ordinal)+1,0),
            'digest',md5(COALESCE(string_agg(md5(record_json),',' ORDER BY ordinal),'')))
            FROM service_thread_records WHERE thread_id=""" + literal)

    def storage_audit(self):
        session, _ = self.create()
        self.stream(session)
        began = time.perf_counter()
        audit = self.stack.database_json("""SELECT json_build_object(
            'canonical_records',(SELECT count(*) FROM service_thread_records),
            'canonical_logical_bytes',(SELECT COALESCE(sum(octet_length(record_json)),0) FROM service_thread_records),
            'admissions',(SELECT count(*) FROM service_thread_admissions),
            'tool_metrics',(SELECT count(*) FROM runtime_tool_metrics),
            'artifact_resources',(SELECT count(*) FROM runtime_artifact_resources),
            'database_bytes',pg_database_size(current_database()),
            'relation_bytes',(SELECT json_object_agg(name,pg_total_relation_size(name::regclass)) FROM
              (VALUES ('service_threads'),('service_thread_sessions'),('service_thread_records'),
                ('service_thread_owners'),('service_thread_admissions'),('service_thread_items'),
                ('service_thread_chat_heads'),('service_thread_display_state'),
                ('runtime_tool_metrics'),('runtime_artifact_resources')) AS relations(name)),
            'invalid_ordinals',(SELECT count(*) FROM service_threads t LEFT JOIN
              (SELECT thread_id,count(*) AS records,min(ordinal) AS first,max(ordinal)+1 AS next
                 FROM service_thread_records GROUP BY thread_id) r ON r.thread_id=t.thread_id
              WHERE COALESCE(r.records,0)<>t.history_revision OR COALESCE(r.next,0)<>t.history_revision
                OR COALESCE(r.first,0)<>0),
            'projection_ahead',(SELECT count(*) FROM service_thread_display_state p
              JOIN service_threads t USING(thread_id) WHERE p.next_ordinal>t.history_revision))""")
        require(audit['canonical_records'] > 0 and audit['admissions'] > 0, 'postgres_history_missing')
        require(audit['canonical_logical_bytes'] > 0 and audit['database_bytes'] > 0,
                'postgres_storage_bytes_missing')
        require(audit['invalid_ordinals'] == 0, 'postgres_canonical_ordinal_invariant')
        require(audit['projection_ahead'] == 0, 'postgres_projection_ahead')
        require(len(audit['relation_bytes']) == 10 and all(v > 0 for v in audit['relation_bytes'].values()),
                'postgres_storage_relations_missing')
        assert_postgres_only_files(self.stack.root / 'data')
        self.report.data['postgres_storage'] = audit
        return dict(storage_audit_ms=[(time.perf_counter()-began)*1000]), 6, {}

    def idle_storage(self):
        # Finish a real turn and close its socket before the quiet window.
        # During sampling no HTTP/page poll can warm or mutate application state.
        session, _ = self.create()
        self.stream(session)
        def settled():
            state = self.stack.database_json("""SELECT json_build_object('active',count(*))
                FROM service_thread_admissions WHERE status IN
                ('accepted','queued','running','waiting_input','waiting_approval')""")
            return state['active'] == 0
        poll(settled, 30)
        time.sleep(3)  # Let request cleanup and PostgreSQL statistics publish.
        self.stack.verify_query_audit()
        query_before = self.stack.idle_query_snapshot()
        before = self.stack.idle_write_snapshot()
        seconds = getattr(getattr(self, 'args', None), 'idle_seconds', IDLE_SAMPLE_SECONDS)
        resources = idle_resource_summary(self.stack.idle_process_samples(), seconds)
        time.sleep(1.1)  # PG_STAT_MIN_INTERVAL: include writes near the final sample.
        writes = idle_write_deltas(before, self.stack.idle_write_snapshot())
        queries = idle_query_deltas(query_before, self.stack.idle_query_snapshot())
        evidence = dict(process=resources, business_table_writes=writes,
                        history_query_activity=queries,
                        total_business_writes=sum(sum(v.values()) for v in writes.values()),
                        requested_observation_seconds=seconds,
                        cpu_limit_single_core_percent=IDLE_MAX_CPU_PERCENT,
                        counter_scope='table-insert-update-delete',
                        observation='completed-request-no-http-no-websocket')
        # Preserve evidence before assertions so failed samples remain actionable;
        # no command lines, paths, credentials or generated session ids are saved.
        self.report.data['idle_storage'] = evidence
        require(evidence['total_business_writes'] == 0, 'idle_business_table_writes')
        require(queries['history_select_calls'] == 0 and queries['active_history_reads'] == 0,
                'idle_history_query_activity')
        require(resources['single_core_cpu_percent'] <= IDLE_MAX_CPU_PERCENT, 'idle_server_cpu_busy')
        assert_postgres_only_files(self.stack.root / 'data')
        return dict(idle_observation_ms=[resources['observation_ms']],
                    idle_cpu_ms=[resources['cpu_ms']]), 10, dict(idle_storage=evidence)

    def prepare_large(self):
        began = time.perf_counter()
        job, _, _ = self.api('POST', '/wunder/chat/stress-threads', self.admin, dict(
            user_rounds=self.args.user_rounds, model_rounds=self.args.model_rounds,
            seed=42, scenario='typical', title='fixture-large'))
        def completed():
            payload, _, _ = self.api('GET', '/wunder/chat/stress-threads/' + job['job_id'], self.admin)
            state = payload['status']
            require(state['state'] != 'failed', 'api_fixture_generation_failed')
            return state if state['state'] == 'completed' else None
        stats = poll(completed, 600)
        require(stats['user_turns'] == self.args.user_rounds and
                stats['model_requests'] == self.args.user_rounds*self.args.model_rounds, 'api_fixture_size')
        require(stats['model_actions'] == stats['model_requests'] + stats['tool_calls'] and
                stats['items'] == stats['user_turns'] + stats['model_requests'] + stats['tool_calls'],
                'api_fixture_action_counts')
        if self.args.model_rounds >= 10:
            require(stats['tool_calls'] > 0, 'api_fixture_missing_tools')
        self.large_session = job['session_id']
        proof = self.canonical_fingerprint(self.large_session)
        require(proof['records'] == proof['next_ordinal'] == stats['rollout_records'],
                'api_fixture_canonical_count')
        self.report.data['fixture'] = dict(generation_ms=(time.perf_counter()-began)*1000,
            **{k: stats[k] for k in ('user_turns', 'model_requests', 'model_actions', 'tool_calls',
                                     'items', 'rollout_records', 'storage_bytes')})

    def large_chat(self):
        if not self.large_session:
            self.prepare_large()
        session = self.large_session
        a, first_ms, first_bytes = self.page(session, self.admin)
        require(a['chat'] and a['has_more'] and a['next_before'] is not None, 'api_large_first_page')
        require(a['latest_round'] == self.args.user_rounds, 'api_large_round_count')
        require(max(r['user_round'] for r in a['chat']) == self.args.user_rounds, 'api_large_tail_round')
        b, older_ms, older_bytes = self.page(session, self.admin, '/view/chat?limit=12&before=' + str(a['next_before']))
        ids = {r['root_turn_id'] for r in a['chat']}
        require(b['chat'] and not ids.intersection(r['root_turn_id'] for r in b['chat']), 'api_page_overlap')
        require(max(r['user_round'] for r in b['chat']) < min(r['user_round'] for r in a['chat']), 'api_page_order')
        for row in a['chat'] + b['chat']:
            require(row['user']['content'].strip() and row['assistant']['content'].strip(), 'api_large_empty_message')
            require(row['status'] == 'completed' and row['assistant']['item_id'] ==
                    '{}:text-{}'.format(row['execution_turn_id'], self.args.model_rounds-1),
                    'api_large_final_answer_identity')
            require(row['assistant'].get('stats', {}).get('model_request_count') == self.args.model_rounds,
                    'api_large_model_request_count')
        forward, forward_ms, forward_bytes = self.page(session, self.admin,
            '/view/chat?limit={}&after={}'.format(len(a['chat']), b['next_after']))
        require([r['root_turn_id'] for r in forward['chat']] == [r['root_turn_id'] for r in a['chat']],
                'api_large_forward_page')
        require(a['chat'][-1]['assistant'].get('body'), 'api_large_body_reference_missing')
        content, body_ms, body_bytes = self.body(session, a['chat'][-1]['assistant'], self.admin)
        require(content.strip(), 'api_large_body_empty')
        item, _, _ = self.api('GET', '/wunder/chat/sessions/{}/thread-log/items/{}'.format(
            session, quote(a['chat'][-1]['assistant']['item_id'], safe='')), self.admin)
        require(item['data']['item_id'] == a['chat'][-1]['assistant']['item_id'] and
                content == item['data']['message']['content'], 'api_large_body_mismatch')
        return dict(chat_bootstrap_ms=[first_ms], older_chat_page_ms=[older_ms],
                    newer_chat_page_ms=[forward_ms], body_read_ms=[sum(body_ms)]), 12+3*(len(a['chat'])+len(b['chat'])), dict(
                    rows=[len(a['chat']), len(b['chat'])], response_bytes=[first_bytes, older_bytes, forward_bytes, body_bytes])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--server-binary', type=Path, required=True, help='Linux release wunder-server')
    parser.add_argument('--image', default='wunder-x86:latest')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--profile', choices=PROFILES, default='smoke')
    parser.add_argument('--baseline', type=Path)
    parser.add_argument('--cases', default='login,session_catalog,stream,slow_consumer,cancel_resume,queue,parallel_threads,isolation,workspace,sandbox_files,large_chat,restart_recovery,storage_audit,idle_storage')
    parser.add_argument('--chunks', type=int, default=8)
    parser.add_argument('--user-rounds', type=int, default=100)
    parser.add_argument('--model-rounds', type=int, default=100)
    parser.add_argument('--idle-seconds', type=int, default=IDLE_SAMPLE_SECONDS,
                        help='Quiet observation duration: 12 for smoke, 600 for acceptance (max 3600)')
    args = parser.parse_args()
    args.server_binary = args.server_binary.resolve()
    cases = args.cases.split(',')
    require(args.server_binary.is_file() and 2 <= args.chunks <= 256 and
            1 <= args.user_rounds <= 1000 and 1 <= args.model_rounds <= 1000 and
            IDLE_SAMPLE_SECONDS <= args.idle_seconds <= IDLE_MAX_SAMPLE_SECONDS, 'invalid_hive_arguments')
    require(cases and len(cases) == len(set(cases)) and all(c in (
        'login', 'session_catalog', 'stream', 'slow_consumer', 'cancel_resume', 'parallel_threads',
        'isolation', 'large_chat', 'queue', 'workspace', 'sandbox_files', 'restart_recovery', 'storage_audit', 'idle_storage') for c in cases), 'invalid_hive_cases')
    report = Report(args.output, 'hive-api-experience', dict(cases=cases, profile=args.profile,
        user_rounds=args.user_rounds, model_requests_per_turn=args.model_rounds, chunks=args.chunks,
        interval_ms=8, seed=42, fixture_contract='postgres-canonical-with-tools-v2',
        idle_observation_seconds=args.idle_seconds, idle_max_single_core_cpu_percent=IDLE_MAX_CPU_PERCENT),
        dict(mode='compose-postgres-http-websocket', worker_threads=4), args.baseline)
    stack = None
    client = None
    try:
        stack = Stack(args, report)
        stack.start()
        client = HiveSuite(args, report, stack)
        client.setup()
        repeats, warmup = PROFILES[args.profile]
        for case in cases:
            for index in range(repeats + warmup):
                report.sample(case, index, index < warmup, getattr(client, case))
    except Exception as error:
        report.data['setup_failed'] = str(error) if isinstance(error, CheckFailure) else type(error).__name__
        print('setup failed: ' + report.data['setup_failed'], flush=True)
    finally:
        if client:
            client.http.close()
        if stack:
            try:
                stack.close()
            except Exception as error:
                report.data['cleanup_failed'] = type(error).__name__
    return report.finish()


if __name__ == '__main__':
    sys.exit(main())
