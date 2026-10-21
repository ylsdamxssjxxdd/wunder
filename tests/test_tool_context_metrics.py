import unittest

from scripts.tool_context_metrics import analyze_records, render_markdown


class ToolContextMetricsTests(unittest.TestCase):
    def test_aggregates_raw_and_model_observation_without_retaining_payload(self) -> None:
        records = [
            {
                "event_type": "tool_result",
                "tool": "browser_click",
                "ok": True,
                "data": {"text": "中🙂" * 200, "next_cursor": "opaque-page"},
                "meta": {"truncated": True},
                "model_observation": '{"tool":"browser","ok":true,"data":{"action":"click"},"truncated":true}',
            },
            {
                "event_type": "tool_result",
                "tool": "browser_type",
                "ok": False,
                "data": {"error": "failed"},
                "model_observation": '{"tool":"browser","ok":true,"data":{}}',
            },
        ]

        report = analyze_records(records)
        self.assertEqual(report["totals"]["calls"], 2)
        self.assertEqual(report["totals"]["errors"], 1)
        self.assertEqual(report["totals"]["truncated"], 1)
        self.assertEqual(report["totals"]["status_mismatches"], 1)
        self.assertEqual([item["tool"] for item in report["tools"]], ["browser"])
        bucket = report["tools"][0]
        self.assertEqual(bucket["calls"], 2)
        self.assertGreater(bucket["raw_observed_tokens"], bucket["observation_tokens"])
        self.assertGreater(bucket["token_reduction_ratio"], 0)
        # The report is aggregate-only; source content must never be copied.
        encoded = render_markdown(report)
        self.assertNotIn("opaque-page", encoded)
        self.assertNotIn("failed", encoded)

    def test_utf8_cost_is_stable_and_invalid_observation_is_counted(self) -> None:
        report = analyze_records([
            {
                "event_type": "tool_result",
                "tool": "read_file",
                "ok": True,
                "data": {"content": "中🙂"},
                "model_observation": "not-json",
            },
        ])
        bucket = report["tools"][0]
        self.assertEqual(bucket["observation_parse_errors"], 1)
        self.assertEqual(bucket["observed"], 0)
        self.assertEqual(bucket["raw_bytes"], len('{"content":"中🙂"}'.encode("utf-8")))
        self.assertEqual(bucket["raw_tokens"], 6)

    def test_legacy_goal_and_schedule_names_are_aggregated_to_current_surfaces(self) -> None:
        report = analyze_records([
            {"event_type": "tool_result", "tool": "create_goal", "data": {}},
            {"event_type": "tool_result", "tool": "update_goal", "data": {}},
            {"event_type": "tool_result", "tool": "get_goal", "data": {}},
            {"event_type": "tool_result", "tool": "定时任务", "data": {}},
            {"event_type": "tool_result", "tool": "schedule_task", "data": {}},
        ])
        self.assertEqual(
            [(item["tool"], item["calls"]) for item in report["tools"]],
            [("goal", 3), ("schedule_task", 2)],
        )

    def test_explicit_non_result_event_is_not_counted_as_a_result(self) -> None:
        report = analyze_records([
            {
                "event_type": "tool_call",
                "kind": "tool_call",
                "tool": "read_file",
                "data": {"placeholder": True},
            },
            {"event_type": "tool_result", "kind": "tool_call", "tool": "read_file", "data": {}},
        ])
        self.assertEqual(report["totals"]["calls"], 1)

    def test_markdown_compares_the_same_observed_samples(self) -> None:
        report = analyze_records([
            {"tool": "read_file", "data": {"content": "x" * 200}},
            {"tool": "read_file", "data": {}, "model_observation": {"ok": True, "data": {}}},
        ])
        markdown = render_markdown(report)
        self.assertIn("| read_file | 2 | 50.0% |", markdown)
        self.assertIn("| 已观测原始 data token |", markdown)
        self.assertIn("| 1 | 1 | 0.00% |", markdown)


if __name__ == "__main__":
    unittest.main()
