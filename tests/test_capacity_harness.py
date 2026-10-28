import importlib.util
import json
from pathlib import Path
import sys
import unittest
from unittest.mock import Mock, patch


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    'capacity_driver', ROOT / 'scripts/experience-bench/capacity_driver.py')
DRIVER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(DRIVER)
MEASURE_SPEC = importlib.util.spec_from_file_location(
    'measure_capacity', ROOT / 'scripts/experience-bench/measure-capacity.py')
MEASURE = importlib.util.module_from_spec(MEASURE_SPEC)
with patch.object(sys, 'path', [str(ROOT / 'scripts/experience-bench')] + sys.path):
    MEASURE_SPEC.loader.exec_module(MEASURE)


class CapacitySummaryTests(unittest.TestCase):
    def test_nearest_rank_includes_slow_tail(self):
        values = list(range(100))
        result = DRIVER.summary(values)
        self.assertEqual(result['samples'], 100)
        self.assertEqual(result['p50_ms'], 49)
        self.assertEqual(result['p95_ms'], 94)
        self.assertEqual(result['maximum_ms'], 99)

    def test_empty_summary_does_not_invent_a_fast_measurement(self):
        self.assertEqual(DRIVER.summary([]), {'samples': 0})

    def test_runtime_diagnostics_does_not_hide_queue_timeouts_absent_from_log_tail(self):
        stages = [dict(runtime_metrics=dict(blocking=[dict(kind='fs', queue_timeouts=3),
            dict(kind='view', queue_timeouts=8), dict(kind='db', exec_timeouts=2, join_errors=1)]))]
        result = MEASURE.runtime_diagnostics(dict(stages=stages))
        self.assertTrue(result['metrics_present'])
        self.assertEqual(result['backend_queue_timeouts'], 3)
        self.assertEqual(result['display_queue_timeouts'], 8)
        self.assertEqual(result['execution_timeouts'], 2)
        self.assertEqual(result['join_errors'], 1)

    def test_empty_runtime_diagnostics_is_not_a_valid_zero_timeout_measurement(self):
        self.assertFalse(MEASURE.runtime_diagnostics({})['metrics_present'])

    def test_expected_fixture_has_all_segments(self):
        self.assertEqual(len(DRIVER.expected.splitlines()), 64)
        self.assertTrue(DRIVER.expected.endswith('fixture-0063-片段\n'))

    def test_capacity_driver_is_opt_in_not_a_startup_service(self):
        source = (ROOT / 'scripts/experience-bench/measure-capacity.py').read_text(encoding='utf-8')
        self.assertIn("profiles=['capacity']", source)
        self.assertIn("['run', '--rm', '--no-deps', 'capacity_driver']", source)

    def test_workspace_storage_variants_are_explicit_in_workload(self):
        source = (ROOT / 'scripts/experience-bench/measure-capacity.py').read_text(encoding='utf-8')
        self.assertIn("choices=['bind', 'volume']", source)
        self.assertIn('workspace_storage=args.workspace_storage', source)
        self.assertIn('capacity_workspaces:/fixture/workspaces', source)

    def test_runtime_metrics_are_recorded_but_not_dumped_to_console(self):
        source = (ROOT / 'scripts/experience-bench/capacity_driver.py').read_text(encoding='utf-8')
        self.assertIn('runtime_metrics=runtime', source)
        self.assertIn("key not in ('probes', 'runtime_metrics')", source)

    def test_resource_sampler_removes_generated_container_identity(self):
        stack = Mock()
        stack.compose.side_effect = ['server-id', 'database-id', 'model-id']
        stack.docker.return_value = '\n'.join(json.dumps(dict(
            ID=identity, Name='fixture-private-name', CPUPerc='12.5%',
            MemUsage='32MiB / 1GiB', PIDs='8'))
            for identity in ('server-id', 'database-id', 'model-id'))
        stopped = Mock()
        stopped.is_set.side_effect = [False, True]
        samples = []
        MEASURE.sample_resources(stack, stopped, samples)
        self.assertEqual(len(samples), 1)
        self.assertEqual([row['service'] for row in samples[0]['containers']],
                         ['server', 'postgres', 'model'])
        self.assertNotIn('fixture-private-name', json.dumps(samples))

    def test_resource_sampler_reports_missing_container_as_failure(self):
        stack = Mock()
        stack.compose.return_value = ''
        samples = []
        MEASURE.sample_resources(stack, Mock(), samples)
        self.assertEqual(samples, [dict(error='CheckFailure')])


if __name__ == '__main__':
    unittest.main()
