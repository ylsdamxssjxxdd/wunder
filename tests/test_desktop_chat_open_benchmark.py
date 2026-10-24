"""Regression guarantees for the external benchmark's accounting and gating."""
import importlib.util
import queue
import time
import tempfile
import sys
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location(
    "chat_open_bench", Path(__file__).resolve().parents[1] / "scripts/desktop-bench/measure-chat-open.py"
)
BENCH = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BENCH)


class ChatOpenBenchmarkTests(unittest.TestCase):
    def test_timeouts_are_failures_and_warmups_do_not_bias_percentiles(self):
        samples = [
            {"case": "first_open", "warmup": True, "status": "ok", "samples_ms": [999]},
            {"case": "first_open", "warmup": False, "status": "ok", "samples_ms": [10]},
            {"case": "first_open", "warmup": False, "status": "timeout", "samples_ms": []},
        ]
        result = BENCH.summarize(samples)["first_open"]
        self.assertEqual((result["p50_ms"], result["p95_ms"]), (10, 10))
        self.assertEqual((result["successful_runs"], result["failed_runs"]), (1, 1))

    def report(self, value):
        return {
            "schema": 1, "fixture_key": {"rollout_sha256": "fixture-digest"},
            "environment": {"system": "fixture-os"}, "build": {"profile": "release"},
            "measurement": {"profile": "none"}, "summary": {
                "first_open": {"p50_ms": value, "p95_ms": value, "failed_runs": 0}
            },
        }

    def test_regression_requires_relative_and_absolute_thresholds(self):
        self.assertFalse(BENCH.compare(self.report(12), self.report(10), 10, 5)["regressions"])
        self.assertFalse(BENCH.compare(self.report(110), self.report(100), 20, 5)["regressions"])
        self.assertEqual(len(BENCH.compare(self.report(130), self.report(100), 20, 5)["regressions"]), 2)

    def test_different_fixtures_or_diagnostic_modes_cannot_pass_a_comparison(self):
        for field in ("fixture_key", "environment", "build", "measurement"):
            changed = self.report(10)
            changed[field] = {"different": True}
            result = BENCH.compare(changed, self.report(100), 20, 5)
            self.assertFalse(result["compatible"])
            self.assertIn(field, result["mismatches"])

    def test_failed_baseline_is_not_a_valid_reference(self):
        baseline = self.report(100)
        baseline["summary"]["first_open"]["failed_runs"] = 1
        self.assertFalse(BENCH.compare(self.report(10), baseline, 20, 5)["compatible"])

    def test_no_success_never_produces_a_zero_time(self):
        result = BENCH.summarize([
            {"case": "first_open", "warmup": False, "status": "error", "samples_ms": []}
        ])["first_open"]
        self.assertIsNone(result["p50_ms"])
        self.assertIsNone(result["p95_ms"])

    def test_report_distinguishes_valid_data_from_performance_regression(self):
        current = self.report(130)
        current["summary"] = BENCH.summarize([
            {"case": "first_open", "warmup": False, "status": "ok", "samples_ms": [130]}
        ])
        current.update(correctness_passed=True, passed=False,
                       comparison=BENCH.compare(current, self.report(100), 20, 5))
        rendered = BENCH.render(current)
        self.assertIn("数据与调用校验：通过", rendered)
        self.assertIn("基准结果：未通过", rendered)
        self.assertIn("检测到 2 项退化", rendered)
        self.assertIn("（退化）", rendered)

    def test_worker_exit_and_errors_fail_without_waiting_for_full_timeout(self):
        for name in ("eof", "error"):
            events = queue.Queue()
            events.put({"event": name})
            with self.assertRaises(RuntimeError):
                BENCH.wait_event(events, "result", time.monotonic() + 10)

    def test_resource_accounting_uses_deltas_and_keeps_peak_separate(self):
        before = dict(cpu_ms=100, rss_bytes=1000, private_bytes=2000,
                      read_bytes=100, read_count=10, write_bytes=50, write_count=5)
        after = dict(cpu_ms=120, rss_bytes=1500, private_bytes=2100,
                     read_bytes=180, read_count=12, write_bytes=50, write_count=5)
        result = BENCH.resource_delta(before, [after])
        self.assertEqual((result["cpu_ms"], result["read_bytes"], result["rss_delta_bytes"]), (20, 80, 500))
        self.assertEqual(result["peak_private_bytes"], 2100)

    def test_a_hung_external_worker_is_killed_and_reported_as_timeout(self):
        # A worker that starts and then never publishes ready must not leave a
        # subprocess behind or convert a timeout into a successful measurement.
        import psutil
        with tempfile.TemporaryDirectory() as directory:
            original = BENCH.subprocess.Popen
            launched = []

            def spawn_hung(*args, **kwargs):
                worker = original([sys.executable, "-c", "import time; time.sleep(30)"], **kwargs)
                launched.append(worker)
                return worker

            BENCH.subprocess.Popen = spawn_hung
            try:
                with self.assertRaises(TimeoutError):
                    BENCH.run_sample(Path("fixture-probe"), Path(directory), "first_open", 1, .1, .01, psutil)
            finally:
                BENCH.subprocess.Popen = original
            self.assertIsNotNone(launched[0].poll())


if __name__ == "__main__":
    unittest.main()
