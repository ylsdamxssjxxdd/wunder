import json
import tempfile
import unittest
from pathlib import Path

from scripts.performance_score import (
    SCORECARDS,
    apply_baseline,
    evaluate,
    read_csv,
    score_metric,
    template,
)


class PerformanceScoreTests(unittest.TestCase):
    def test_each_scorecard_weights_sum_to_100(self) -> None:
        self.assertEqual({key: sum(item.weight for item in specs) for key, specs in SCORECARDS.items()}, {"beehive": 100, "desktop": 100, "cli": 100, "server": 100})

    def test_capacity_report_cannot_hide_missing_idle_measurement(self) -> None:
        data = template("server")
        data["scenarios"]["S1"] = {"metrics": {"storage_startup_p95_ms": 50, "capacity_added_p95_ms": 10}, "gates": {"index_oids_stable": True, "no_history_scan": True, "no_row_rewrite": True}}
        result = evaluate(data)
        self.assertEqual(result["coverage_percent"], 40)
        self.assertIsNone(result["score"])
        self.assertIn("S2", result["missing_scenarios"])
        data["scenarios"]["S1"]["gates"]["index_oids_stable"] = False
        self.assertTrue(evaluate(data)["hard_failures"])

    def test_normalization_clamps_at_target_and_boundary(self) -> None:
        spec = SCORECARDS["desktop"][0].metrics[0]
        self.assertEqual(score_metric(spec.target, spec), 100.0)
        self.assertEqual(score_metric(spec.boundary, spec), 0.0)
        self.assertEqual(score_metric(spec.boundary + 1000, spec), 0.0)

    def test_template_has_all_scenarios_and_empty_values(self) -> None:
        data = template("蜂巢")
        self.assertEqual(data["shape"], "蜂巢")
        self.assertEqual(set(data["scenarios"]), {item.key for item in SCORECARDS["beehive"]})
        self.assertIsNone(data["scenarios"]["H1"]["metrics"]["tti_p95_ms"])

    def test_partial_report_has_coverage_and_no_fake_total(self) -> None:
        report = evaluate({"shape": "蛹", "scenarios": {"C1": {"metrics": {"first_frame_p95_ms": 350}, "gates": {"no_heavy_work_before_frame": True}}}})
        self.assertIsNone(report["score"])
        self.assertEqual(report["coverage_percent"], 8.0)
        self.assertIn("C2", report["missing_scenarios"])

    def test_hard_gate_failure_is_reported(self) -> None:
        report = evaluate({"shape": "舵机", "scenarios": {"D1": {"metrics": {"first_line_p95_ms": 300}, "gates": {"exit_code_zero": False}}}})
        self.assertIn("D1.exit_code_zero: 退出码正确", report["hard_failures"])

    def test_baseline_detects_total_and_scenario_regression(self) -> None:
        current = evaluate({"shape": "蜂巢", "scenarios": {"H1": {"metrics": {"tti_p95_ms": 2000}, "gates": {"no_uncaught_errors": True}}}})
        baseline = evaluate({"shape": "蜂巢", "scenarios": {"H1": {"metrics": {"tti_p95_ms": 1200}, "gates": {"no_uncaught_errors": True}}}})
        apply_baseline(current, baseline, "beehive")
        self.assertTrue(any(item["key"] == "H1" for item in current["regressions"]))

    def test_csv_row_contract(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "sample.csv"
            path.write_text("scenario,metric,value,gate,passed\nC1,first_frame_p95_ms,350,,\nC1,,,no_heavy_work_before_frame,true\n", encoding="utf-8")
            data = read_csv(path, "蛹")
        self.assertEqual(data["scenarios"]["C1"]["metrics"]["first_frame_p95_ms"], 350.0)
        self.assertTrue(data["scenarios"]["C1"]["gates"]["no_heavy_work_before_frame"])


if __name__ == "__main__":
    unittest.main()
