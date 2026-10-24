"""Meaningful gates for the external UX suite; no product or model required."""
import copy
import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location(
    "desktop_experience_benchmark", Path(__file__).resolve().parents[1] / "scripts/desktop-bench/measure-experience.py")
BENCH = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BENCH)


class ExperienceBenchmarkTests(unittest.TestCase):
    def sample(self, value=10):
        return dict(case="startup", warmup=False, status="ok", resources=dict(available=True),
                    check=dict(scope="backend", model="loopback-fixed-v1", assertions=1,
                               metrics=dict(startup_ready_ms=[value])))

    def report(self, value=10):
        return dict(schema=1, environment=dict(os="fixture"), build=dict(profile="release"),
                    workload=dict(runs=10), cases=["startup"], correctness_passed=True, passed=True,
                    summary=BENCH.summarize([self.sample(value)]))

    def test_missing_content_checks_missing_metrics_and_sampling_loss_fail(self):
        for mutate in (lambda s: s["check"].update(assertions=0),
                       lambda s: s["check"].update(metrics={}),
                       lambda s: s["check"].update(scope="frontend"),
                       lambda s: s["resources"].update(available=False),
                       lambda s: s["resources"].update(sampling_truncated=True)):
            sample = self.sample()
            mutate(sample)
            with self.assertRaises(RuntimeError):
                BENCH.validate(sample, "startup")

    def test_nan_negative_boolean_and_empty_measurements_fail(self):
        for values in ([float("nan")], [-1], [True], [], [float("inf")]):
            sample = self.sample()
            sample["check"]["metrics"]["startup_ready_ms"] = values
            with self.assertRaises(RuntimeError):
                BENCH.validate(sample, "startup")

    def test_failures_and_warmups_do_not_become_fast_measurements(self):
        warmup = self.sample(999)
        warmup["warmup"] = True
        failed = dict(case="startup", status="timeout", warmup=False)
        summary = BENCH.summarize([warmup, failed, self.sample(12)])["startup"]
        self.assertEqual((summary["runs"], summary["failed_runs"]), (2, 1))
        self.assertEqual(summary["metrics"]["startup_ready_ms"]["p95"], 12)
        only_failed = BENCH.summarize([failed])["startup"]
        self.assertEqual(only_failed["metrics"], {})

    def test_relative_and_absolute_regression_and_coverage_gates(self):
        before = self.report(100)
        self.assertFalse(BENCH.compare(self.report(110), before, 20, 5)["regressions"])
        self.assertEqual(len(BENCH.compare(self.report(130), before, 20, 5)["regressions"]), 2)
        self.assertFalse(BENCH.compare(self.report(12), self.report(10), 10, 5)["regressions"])
        for field in ("environment", "build", "workload", "cases"):
            changed = self.report()
            changed[field] = None
            self.assertFalse(BENCH.compare(changed, before, 20, 5)["compatible"])
        before["summary"]["startup"]["metrics"] = {}
        self.assertFalse(BENCH.compare(self.report(), before, 20, 5)["compatible"])

    def test_failed_baseline_cannot_establish_a_performance_pass(self):
        for key in ("passed", "correctness_passed"):
            old = self.report()
            old[key] = False
            self.assertFalse(BENCH.compare(self.report(), old, 20, 5)["compatible"])

    def test_chat_regression_is_part_of_unified_suite_gate(self):
        now = self.report()
        now["chat_comparison"] = dict(compatible=True, mismatches=[], regressions=[dict(case="first_open")])
        self.assertEqual(BENCH.compare(now, self.report(), 20, 5)["regressions"], [dict(case="first_open")])
        now["chat_comparison"] = dict(compatible=False, mismatches=["fixture_key"], regressions=[])
        self.assertFalse(BENCH.compare(now, self.report(), 20, 5)["compatible"])

    def test_units_do_not_turn_queue_counts_into_milliseconds(self):
        sample = self.sample()
        sample["check"]["metrics"]["pending_events_peak"] = [128]
        summary = BENCH.summarize([sample])["startup"]["metrics"]
        self.assertEqual(summary["pending_events_peak"]["unit"], "events")
        self.assertEqual(summary["startup_ready_ms"]["unit"], "ms")

    def test_resource_growth_excludes_warmup_and_marks_short_runs(self):
        series = [dict(since_start_ms=i*10000, rss_bytes=(999999 if i < 2 else i*1000),
                       handles=10+i, threads=4, private_bytes=None) for i in range(11)]
        result = BENCH.trends(series)
        self.assertTrue(result["long_enough"])
        self.assertAlmostEqual(result["rss_bytes"]["per_minute"], 6000)
        self.assertAlmostEqual(result["handles"]["per_minute"], 6)
        self.assertEqual(result["threads"]["end_minus_start"], 0)
        short = copy.deepcopy(series)
        for row in short:
            row["since_start_ms"] /= 10
        self.assertFalse(BENCH.trends(short)["long_enough"])


if __name__ == "__main__":
    unittest.main()
