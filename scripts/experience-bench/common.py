"""External experience test utilities, compatible with Python 3.8+."""
import csv
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import queue
import subprocess
import threading
import time

PROFILES = {'smoke': (1, 0), 'daily': (10, 1), 'release': (30, 2)}
REPO = Path(__file__).resolve().parents[2]


class CheckFailure(RuntimeError):
    """Only static, driver-owned diagnostic codes belong in this exception."""


def require(condition, message):
    if not condition:
        raise CheckFailure(message)


def digest(path):
    result = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            result.update(block)
    return result.hexdigest()


def write_json(path, value):
    Path(path).write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')


def clean_env():
    return {k: v for k, v in os.environ.items() if not k.startswith('WUNDER_')}


def environment():
    import psutil
    return dict(system=platform.system(), release=platform.release(), machine=platform.machine(),
                logical_cpus=os.cpu_count(), physical_memory_bytes=psutil.virtual_memory().total,
                runner_python=platform.python_version())


def percentile(values, rank):
    return sorted(values)[max(0, math.ceil(len(values) * rank) - 1)]


def run_process(command, cwd, env=None, timeout=120, stdin=None, on_line=None, slow_ms=0):
    """Bounded capture and process-tree cleanup; stdout arrival measured outside CLI."""
    import psutil
    began = time.perf_counter()
    proc = subprocess.Popen([str(v) for v in command], cwd=str(cwd), env=env,
                            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    events = queue.Queue(maxsize=128)
    stopped = threading.Event()

    def reader(stream, channel):
        while not stopped.is_set():
            line = stream.readline(1024 * 1024 + 1)
            if not line:
                break
            while not stopped.is_set():
                try:
                    events.put((channel, line, time.perf_counter()), timeout=.1)
                    break
                except queue.Full:
                    pass

    readers = [threading.Thread(target=reader, args=(stream, name), daemon=True)
               for stream, name in ((proc.stdout, 'stdout'), (proc.stderr, 'stderr'))]
    for thread in readers:
        thread.start()
    try:
        proc.stdin.write((stdin or '').encode('utf-8'))
        proc.stdin.close()
        captured = bytearray()
        stderr_bytes = 0
        peak_rss = peak_handles = peak_threads = 0
        cpu_ms = 0
        resource_samples = 0
        last_sample = 0
        while proc.poll() is None or any(t.is_alive() for t in readers) or not events.empty():
            now = time.perf_counter()
            require(now - began < timeout, 'worker deadline exceeded')
            if now - last_sample >= .01:
                last_sample = now
                try:
                    monitor = psutil.Process(proc.pid)
                    peak_rss = max(peak_rss, monitor.memory_info().rss)
                    cpu = monitor.cpu_times()
                    cpu_ms = max(cpu_ms, (cpu.user + cpu.system) * 1000)
                    peak_threads = max(peak_threads, monitor.num_threads())
                    handles = monitor.num_handles() if os.name == 'nt' else monitor.num_fds()
                    peak_handles = max(peak_handles, handles)
                    resource_samples += 1
                except psutil.Error:
                    pass
            try:
                channel, line, arrived = events.get(timeout=.01)
            except queue.Empty:
                continue
            require(len(line) <= 1024 * 1024, 'worker line budget exceeded')
            if channel == 'stderr':
                stderr_bytes += len(line)
                require(stderr_bytes <= 4 * 1024 * 1024, 'stderr budget exceeded')
            else:
                captured.extend(line)
                require(len(captured) <= 8 * 1024 * 1024, 'stdout budget exceeded')
                if on_line:
                    on_line(line.decode('utf-8'), (arrived - began) * 1000)
                if slow_ms:
                    time.sleep(slow_ms / 1000)
        return dict(exit_code=proc.wait(), elapsed_ms=(time.perf_counter() - began) * 1000,
                    stdout=captured.decode('utf-8'), resources=dict(samples=resource_samples,
                    peak_rss_bytes=peak_rss, peak_handles_or_fds=peak_handles,
                    peak_threads=peak_threads, cpu_ms=cpu_ms))
    finally:
        stopped.set()
        if proc.poll() is None:
            try:
                descendants = psutil.Process(proc.pid).children(recursive=True)
            except psutil.Error:
                descendants = []
            for child in reversed(descendants):
                try:
                    child.kill()
                except psutil.Error:
                    pass
            proc.kill()
            proc.wait(timeout=10)
        for thread in readers:
            thread.join(timeout=1)
        for stream in (proc.stdin, proc.stdout, proc.stderr):
            if not stream.closed:
                stream.close()


class Report:
    def __init__(self, output, suite, workload, execution, baseline=None):
        self.output = Path(output).resolve()
        self.output.mkdir(parents=True, exist_ok=False)
        write_json(self.output / 'fixture-owner.json', dict(profile=suite))
        self.data = dict(schema=1, suite=suite, environment=environment(), workload=workload,
                         execution=execution, samples=[], captured_at_unix=time.time())
        self.baseline = json.loads(Path(baseline).read_text(encoding='utf-8')) if baseline else None

    def sample(self, case, index, warmup, work):
        print('{} {}{}'.format(case, index, ' warmup' if warmup else ''), flush=True)
        try:
            metrics, checks, extra = work()
            require(checks > 0 and metrics, 'missing correctness evidence')
            for name, values in metrics.items():
                require(name.endswith('_ms') and 0 < len(values) <= 10000, 'invalid metric')
                require(all(isinstance(v, (int, float)) and not isinstance(v, bool)
                            and math.isfinite(v) and v >= 0 for v in values), 'invalid timing')
            sample = dict(status='ok', metrics=metrics, assertions=checks, **extra)
        except Exception as error:
            # Exception payloads can contain credentials/paths; only persist type.
            reason = str(error) if isinstance(error, CheckFailure) else type(error).__name__
            sample = dict(status='error', reason=reason)
            print('  failed: {}'.format(reason), flush=True)
        sample.update(case=case, run=index, warmup=warmup)
        self.data['samples'].append(sample)
        with (self.output / 'samples.jsonl').open('a', encoding='utf-8') as stream:
            stream.write(json.dumps(sample, ensure_ascii=False) + '\n')

    def finish(self):
        summary = {}
        for case in self.data['workload']['cases']:
            rows = [s for s in self.data['samples'] if s['case'] == case and not s['warmup']]
            good = [s for s in rows if s['status'] == 'ok']
            metrics = {}
            for name in sorted({k for s in good for k in s['metrics']}):
                values = [v for s in good for v in s['metrics'][name]]
                metrics[name] = dict(count=len(values), p50=percentile(values, .5),
                                     p95=percentile(values, .95), maximum=max(values))
            summary[case] = dict(runs=len(rows), successful_runs=len(good),
                                 failed_runs=len(rows)-len(good), metrics=metrics)
        self.data['summary'] = summary
        self.data['correctness_passed'] = (all(s['status'] == 'ok' for s in self.data['samples'])
                                             and all(v['runs'] > 0 for v in summary.values())
                                             and not self.data.get('setup_failed')
                                             and not self.data.get('cleanup_failed'))
        self.data['passed'] = self.data['correctness_passed']
        if self.baseline:
            self.data['comparison'] = compare(self.data, self.baseline)
            self.data['passed'] &= self.data['comparison']['compatible'] and not self.data['comparison']['regressions']
        write_json(self.output / 'report.json', self.data)
        lines = ['# {}'.format(self.data['suite']), '', '正确性：{}；总结果：{}。'.format(
                 self.data['correctness_passed'], self.data['passed']), '',
                 '| 场景 | 成功/运行 | 指标 | P50 ms | P95 ms | 最大 ms |',
                 '| --- | ---: | --- | ---: | ---: | ---: |']
        with (self.output / 'summary.csv').open('w', encoding='utf-8', newline='') as stream:
            writer = csv.writer(stream)
            writer.writerow(['case', 'runs', 'failures', 'metric', 'p50_ms', 'p95_ms', 'maximum_ms'])
            for case, row in summary.items():
                if not row['metrics']:
                    lines.append('| {} | 0/{} | 失败，无有效指标 | | | |'.format(case, row['runs']))
                for name, metric in row['metrics'].items():
                    lines.append('| {} | {}/{} | {} | {:.3f} | {:.3f} | {:.3f} |'.format(
                        case, row['successful_runs'], row['runs'], name, metric['p50'], metric['p95'], metric['maximum']))
                    writer.writerow([case, row['runs'], row['failed_runs'], name, metric['p50'], metric['p95'], metric['maximum']])
        lines += ['', '计时为外部单调时钟；数据准备单列，不含浏览器/TUI 绘制。smoke 不能建立可靠 P95。',
                  '环境、负载与调用方式一致才比较；失败、超时和降级页不能当成功样本。完整证据见 report.json。']
        if self.data.get('comparison'):
            lines += ['', '基线比较：' + json.dumps(self.data['comparison'], ensure_ascii=False)]
        (self.output / 'report.md').write_text('\n'.join(lines) + '\n', encoding='utf-8')
        return 0 if self.data['passed'] else 1


def compare(now, old):
    mismatch = [k for k in ('schema', 'suite', 'environment', 'workload', 'execution') if now.get(k) != old.get(k)]
    if not old.get('passed'):
        mismatch.append('baseline failed')
    for key in ('user_turns', 'model_requests', 'model_actions', 'tool_calls', 'items', 'rollout_records'):
        if now.get('fixture', {}).get(key) != old.get('fixture', {}).get(key):
            mismatch.append('fixture:' + key)
    regressions = []
    changes = []
    for case, row in now['summary'].items():
        before = old.get('summary', {}).get(case, {})
        if not before or before.get('failed_runs') or before.get('runs', 0) < 1:
            mismatch.append('coverage:' + case)
        if set(row['metrics']) != set(before.get('metrics', {})):
            mismatch.append('metric coverage:' + case)
        for name, metric in row['metrics'].items():
            previous = before.get('metrics', {}).get(name)
            if not previous:
                mismatch.append('metric:' + case + '/' + name)
                continue
            for rank in ('p50', 'p95'):
                a, b = metric[rank], previous[rank]
                delta = a-b
                change = dict(case=case, metric=name, rank=rank, before=b, after=a, delta_ms=delta)
                changes.append(change)
                if delta > 5 and (b == 0 or delta/b > .2):
                    regressions.append(change)
    return dict(compatible=not mismatch, mismatches=mismatch, changes=changes, regressions=regressions)


def model_config(url):
    return dict(llm=dict(default='fixture-model', models={'fixture-model': dict(
        provider='openai', api_mode='chat_completions', base_url=url, model='fixture-model',
        enable=True, api_key='fixture-model-key', temperature=0, max_rounds=4, max_context=1048576, max_output=8192,
        timeout_s=30, stream=True, tool_call_mode='tool_call', model_type='llm')}),
        channels=dict(enabled=False), lsp=dict(enabled=False), browser=dict(enabled=False),
        skills=dict(enabled=[]), mcp=dict(servers=[]))
