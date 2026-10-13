import json
import subprocess
import unittest
from unittest.mock import patch

from scripts.server_capacity_sample import cumulative_cpu, docker_stats, runtime_calls


class CapacitySampleTests(unittest.TestCase):
    def test_cumulative_cpu_captures_bursts_without_host_core_division(self) -> None:
        before = {"read": "2026-01-01T00:00:00Z", "cpu_stats": {"cpu_usage": {"total_usage": 1_000_000_000}}}
        after = {"read": "2026-01-01T00:15:00Z", "cpu_stats": {"cpu_usage": {"total_usage": 14_500_000_000}}, "online_cpus": 32}
        self.assertEqual(cumulative_cpu(before, after), 1.5)
        after["cpu_stats"]["cpu_usage"]["total_usage"] = 0
        with self.assertRaises(ValueError):
            cumulative_cpu(before, after)

    def test_idle_activity_excludes_only_the_metrics_probe(self) -> None:
        snapshot = {"runtime": {
            "blocking": [{"label": "api.chat.view.body", "calls": 3}, {"label": "thread_view.rebuild.page", "calls": 6}],
            "loop_ticks": [{"label": "api.admin.runtime_metrics", "ticks": 2}, {"label": "api.ws.thread_view", "ticks": 4}],
        }}
        self.assertEqual(runtime_calls(snapshot), {"api.chat.view.body": 3, "api.ws.thread_view": 4})

    @patch("scripts.server_capacity_sample.subprocess.run")
    def test_cpu_uses_single_core_container_values(self, run) -> None:
        run.return_value = subprocess.CompletedProcess([], 0, stdout="\n".join([
            json.dumps({"Name": "fixture-server", "CPUPerc": "150.25%"}),
            json.dumps({"Name": "fixture-postgres", "CPUPerc": "0.50%"}),
        ]))
        self.assertEqual(docker_stats("fixture-server", "fixture-postgres"), {
            "fixture-server": 150.25, "fixture-postgres": 0.5,
        })

    @patch("scripts.server_capacity_sample.subprocess.run")
    def test_missing_container_is_a_failed_sample(self, run) -> None:
        run.return_value = subprocess.CompletedProcess([], 0, stdout=json.dumps({
            "Name": "fixture-server", "CPUPerc": "0.00%",
        }))
        with self.assertRaises(ValueError):
            docker_stats("fixture-server", "fixture-postgres")


if __name__ == "__main__":
    unittest.main()
