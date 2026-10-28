"""Isolated 2000-account PostgreSQL/WebSocket capacity experiment."""
import argparse
import importlib.util
import json
from pathlib import Path
import sys
import threading
import time

from common import CheckFailure, Report, require


def runtime_diagnostics(result):
    stages = result.get('stages', [])
    metrics = stages[-1].get('runtime_metrics', {}).get('blocking', []) if stages else []
    return dict(
        metrics_present=bool(metrics),
        backend_queue_timeouts=sum(row.get('queue_timeouts', 0) for row in metrics
                                   if row.get('kind') != 'view'),
        display_queue_timeouts=sum(row.get('queue_timeouts', 0) for row in metrics
                                   if row.get('kind') == 'view'),
        execution_timeouts=sum(row.get('exec_timeouts', 0) for row in metrics),
        join_errors=sum(row.get('join_errors', 0) for row in metrics))


def sample_resources(stack, stopped, samples):
    try:
        containers = {stack.compose(['ps', '-q', role]).strip(): role
                      for role in ('server', 'postgres', 'model')}
        require(all(containers), 'capacity_resource_container_missing')
        began = time.monotonic()
        while not stopped.is_set() and len(samples) < 240:
            raw = stack.docker(['stats', '--no-stream', '--format', '{{json .}}', *containers])
            rows = []
            for line in raw.splitlines():
                row = json.loads(line)
                role = next((name for identity, name in containers.items()
                             if identity.startswith(row.get('ID', 'unavailable'))), None)
                require(role is not None, 'capacity_resource_identity_mismatch')
                rows.append(dict(service=role, cpu_percent=row['CPUPerc'],
                                 memory_usage=row['MemUsage'], pids=row['PIDs']))
            require(len(rows) == len(containers), 'capacity_resource_sample_incomplete')
            samples.append(dict(elapsed_s=time.monotonic()-began, containers=rows))
            stopped.wait(3)
    except Exception as error:
        samples.append(dict(error=type(error).__name__))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--server-binary', type=Path, required=True)
    parser.add_argument('--image', default='wunder-x86:latest')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--levels', default='100,500,2000')
    parser.add_argument('--workspace-storage', choices=['bind', 'volume'], default='bind')
    args = parser.parse_args()
    args.server_binary = args.server_binary.resolve()
    args.chunks = 64
    require(args.server_binary.is_file(), 'capacity_binary_missing')
    levels = [int(value) for value in args.levels.split(',')]
    require(levels and all(1 <= value <= 2000 for value in levels), 'invalid_capacity_levels')
    source = Path(__file__).resolve().parent
    spec = importlib.util.spec_from_file_location('capacity_hive', source / 'measure-hive.py')
    hive = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(hive)
    report = Report(args.output, 'service-capacity', dict(cases=['capacity'], users=2000,
        levels=levels, task_slots=300, chunks=64, interval_ms=8,
        workspace_storage=args.workspace_storage,
        excludes=['login-password-hashing', 'real-inference']), dict(mode='compose-postgres-websocket'))
    stack = None
    client = None
    sampler = None
    stopped = threading.Event()
    try:
        stack = hive.Stack(args, report)
        config_path = stack.root / 'config.json'
        config = json.loads(config_path.read_text(encoding='utf-8'))
        config['server']['max_active_sessions'] = 300
        config['storage']['postgres'] = dict(dsn='', connect_timeout_s=5, pool_size=64)
        config['llm']['models']['fixture-model']['timeout_s'] = 120
        config_path.write_text(json.dumps(config), encoding='utf-8')
        override = stack.root / 'capacity-compose.json'
        compose_override = dict(services=dict(
            server=dict(environment=dict(WUNDER_SERVER_WORKER_THREADS='8')),
            model=dict(command=['python3', '/driver/capacity_model.py']),
            capacity_driver=dict(image=args.image, entrypoint=[],
                profiles=['capacity'],
                command=['python3', '/driver/capacity_driver.py', '--fixture', '/fixture',
                         '--label', 'measured', '--levels', args.levels],
                volumes=[source.as_posix()+':/driver:ro', stack.root.as_posix()+':/fixture'])
        ))
        if args.workspace_storage == 'volume':
            compose_override['volumes'] = dict(capacity_workspaces={}, capacity_tools={})
            compose_override['services']['server']['volumes'] = [
                'capacity_workspaces:/fixture/workspaces', 'capacity_tools:/fixture/user-tools']
            compose_override['services']['sandbox'] = dict(volumes=[
                'capacity_workspaces:/fixture/workspaces'])
        override.write_text(json.dumps(compose_override), encoding='utf-8')
        stack.command += ['--file', str(override)]
        stack.start()
        client = hive.HiveSuite(args, report, stack)
        client.setup()
        stack.compose(['exec', '-T', 'postgres', 'psql', '-X', '-q', '-v', 'ON_ERROR_STOP=1',
            '-U', 'fixture_audit', '-d', 'fixture', '-c', """
            INSERT INTO user_accounts(user_id,username,password_hash,roles,status,access_level,
              quota_balance,quota_granted_total,quota_used_total,experience_total,is_demo,created_at,updated_at)
            SELECT 'fixture-load-'||number,'fixture-load-'||number,template.password_hash,'["user"]','active',
              template.access_level,1000000,1000000,0,0,1,extract(epoch FROM now()),extract(epoch FROM now())
            FROM generate_series(1,2000) number CROSS JOIN
              (SELECT password_hash,access_level FROM user_accounts WHERE roles='["user"]' LIMIT 1) template;
            INSERT INTO user_tokens(token,user_id,session_scope,expires_at,created_at,last_used_at)
            SELECT 'wund_'||md5(random()::text||clock_timestamp()::text||number),'fixture-load-'||number,
              'default',extract(epoch FROM now())+86400,extract(epoch FROM now()),extract(epoch FROM now())
            FROM generate_series(1,2000) number;
            """])
        users = stack.database_json("""SELECT json_agg(json_build_object('user_id',user_id,'token',token)
            ORDER BY user_id) FROM user_tokens WHERE user_id LIKE 'fixture-load-%'""")
        require(len(users) == 2000, 'capacity_account_count')
        (stack.root / 'capacity-credentials.json').write_text(json.dumps(dict(
            base_url='http://server:18000', model_url='http://model:18090', users=users,
            admin=client.credentials[0])), encoding='utf-8')
        report.data['resource_samples'] = []
        sampler = threading.Thread(target=sample_resources,
            args=(stack, stopped, report.data['resource_samples']), daemon=True)
        sampler.start()
        stack.compose(['run', '--rm', '--no-deps', 'capacity_driver'], timeout=1200)
        stopped.set()
        sampler.join(65)
        result = json.loads((stack.root / 'capacity-measured.json').read_text(encoding='utf-8'))
        (report.output / 'capacity.json').write_text(json.dumps(result, indent=2), encoding='utf-8')
        report.data['capacity'] = result
        report.data['runtime_diagnostics'] = runtime_diagnostics(result)
        log = stack.compose(['logs', '--no-color', '--tail', '2000', 'server'])
        report.data['server_diagnostics'] = dict(
            scope='last-2000-log-lines',
            validation_failures=log.count('transport credential validation failed'),
            queue_timeouts=log.count('queue timeout'),
            execution_timeouts=log.count('timed out after'),
            missing_threads=log.count('thread not found'))

        def validate():
            require(result.get('passed') is True, 'capacity_correctness_failed')
            diagnostics = report.data['runtime_diagnostics']
            require(diagnostics['metrics_present'] and all(diagnostics[name] == 0 for name in
                ('backend_queue_timeouts', 'execution_timeouts', 'join_errors')),
                'capacity_backend_runtime_failed')
            require(report.data['resource_samples'] and all('error' not in sample
                for sample in report.data['resource_samples']), 'capacity_resource_samples_failed')
            return dict(completed_ms=[stage['completed_ms']['p95_ms'] for stage in result['stages']]), 1, {}

        report.sample('capacity', 0, False, validate)
    except Exception as error:
        report.data['setup_failed'] = str(error) if isinstance(error, CheckFailure) else type(error).__name__
    finally:
        stopped.set()
        if sampler:
            sampler.join(65)
        if client:
            client.http.close()
        if stack:
            (stack.root / 'capacity-credentials.json').unlink(missing_ok=True)
            try:
                stack.close()
            except Exception as error:
                report.data['cleanup_failed'] = type(error).__name__
    return report.finish()


if __name__ == '__main__':
    sys.exit(main())
