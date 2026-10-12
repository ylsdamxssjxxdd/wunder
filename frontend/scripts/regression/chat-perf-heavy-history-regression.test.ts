// W4-L heavy-history thread rendering metrics gate.
//
// The guard rail: metric names that are not present in the chatPerfMetrics.ts whitelist are
// dropped silently at record time. These tests assert the six W4-L names are registered, that
// registered names survive into the report, and (negative control) that an unregistered name is
// still dropped. Reverting the whitelist registration makes the first two tests fail.
import test from 'node:test';
import assert from 'node:assert/strict';
import {
  emptyPerfMetrics,
  incrementPerfCounter,
  isPerfMetricRegistered,
  recordPerfDuration,
  summarizePerfMetrics
} from '../../src/utils/chatPerfMetrics';
import { CHAT_PERF_HEAVY_HISTORY_METRICS, chatPerf } from '../../src/utils/chatPerf';

// The six names frozen by W4-L. Keep this list aligned with CHAT_PERF_HEAVY_HISTORY_METRICS.
const HEAVY_HISTORY_DURATIONS = [
  'snapshot_apply_ms',
  'thread_projection_ms',
  'virtual_window_recompute_ms',
  'trajectory_layout_ms'
];
const HEAVY_HISTORY_COUNTERS = ['snapshot_abort', 'workflow_events_items'];
const HEAVY_HISTORY_METRICS = [...HEAVY_HISTORY_DURATIONS, ...HEAVY_HISTORY_COUNTERS];

// Negative control: names that must stay unregistered so dropping is still observable.
const UNREGISTERED_DURATION = 'heavy_history_projection_probe_ms';
const UNREGISTERED_COUNTER = 'heavy_history_probe_counter';

const hasOwn = (record: Record<string, unknown>, key: string) =>
  Object.prototype.hasOwnProperty.call(record, key);

test('all six heavy-history metric names are registered in the whitelist', () => {
  for (const name of HEAVY_HISTORY_METRICS) {
    assert.equal(isPerfMetricRegistered(name), true, `${name} must be registered`);
  }
  assert.equal(isPerfMetricRegistered(UNREGISTERED_DURATION), false);
  assert.equal(isPerfMetricRegistered(UNREGISTERED_COUNTER), false);
  assert.deepEqual(Object.values(CHAT_PERF_HEAVY_HISTORY_METRICS), HEAVY_HISTORY_METRICS);
});

test('registered names are recorded while unregistered names stay dropped', () => {
  const state = emptyPerfMetrics();
  for (const name of HEAVY_HISTORY_DURATIONS) recordPerfDuration(state, name, 42, 0);
  for (const name of HEAVY_HISTORY_COUNTERS) incrementPerfCounter(state, name, 7);
  recordPerfDuration(state, UNREGISTERED_DURATION, 42, 0);
  incrementPerfCounter(state, UNREGISTERED_COUNTER, 7);

  const report = summarizePerfMetrics(state);
  for (const name of HEAVY_HISTORY_DURATIONS) {
    assert.equal(report.durations[name]?.count, 1, `${name} must be recorded`);
  }
  for (const name of HEAVY_HISTORY_COUNTERS) {
    assert.equal(report.counters[name], 7, `${name} must be recorded`);
  }
  assert.equal(hasOwn(report.durations, UNREGISTERED_DURATION), false);
  assert.equal(hasOwn(report.counters, UNREGISTERED_COUNTER), false);
});

test('chatPerf capture reports the heavy-history metrics end to end', () => {
  chatPerf.start();
  chatPerf.recordSnapshotApply(184.5, { itemCount: 4800 });
  chatPerf.recordThreadProjection(233.1, { roundCount: 480 });
  chatPerf.recordVirtualWindowRecompute(12.4, { messageCount: 980 });
  chatPerf.recordTrajectoryLayout(96.7, { itemCount: 1000 });
  chatPerf.countSnapshotAbort(2);
  chatPerf.countWorkflowEventsItems(25000);
  chatPerf.recordDuration(UNREGISTERED_DURATION, 5);
  chatPerf.count(UNREGISTERED_COUNTER, 5);

  const report = chatPerf.snapshot();
  chatPerf.stop();

  assert.equal(report.durations.snapshot_apply_ms?.count, 1);
  assert.equal(report.durations.thread_projection_ms?.count, 1);
  assert.equal(report.durations.virtual_window_recompute_ms?.count, 1);
  assert.equal(report.durations.trajectory_layout_ms?.count, 1);
  assert.equal(report.durations.snapshot_apply_ms?.maxMs, 184.5);
  assert.equal(report.durations.thread_projection_ms?.maxMs, 233.1);
  assert.equal(report.counters.snapshot_abort, 2);
  assert.equal(report.counters.workflow_events_items, 25000);
  assert.equal(hasOwn(report.durations, UNREGISTERED_DURATION), false);
  assert.equal(hasOwn(report.counters, UNREGISTERED_COUNTER), false);
  assert.equal(report.summary.heavyHistory.snapshotAborts, 2);
  assert.equal(report.summary.heavyHistory.workflowEventItems, 25000);
  assert.equal(report.summary.heavyHistory.snapshotApply?.count, 1);
});

test('capture is inert while stopped, so instrumentation is safe to leave in place', () => {
  chatPerf.stop();
  const before = chatPerf.snapshot();
  chatPerf.recordSnapshotApply(1000);
  chatPerf.recordThreadProjection(1000);
  chatPerf.recordVirtualWindowRecompute(1000);
  chatPerf.recordTrajectoryLayout(1000);
  chatPerf.countSnapshotAbort(5);
  chatPerf.countWorkflowEventsItems(1000);
  const after = chatPerf.snapshot();
  assert.deepEqual(after, before);
});

// Archive hook: prints one deterministic heavy-history report so the W4-L baseline doc has a
// reproducible payload. Inert unless explicitly requested, so normal runs stay quiet.
test('emits the archived heavy-history sample report on request', () => {
  if (process.env.W4L_EMIT_REPORT !== '1') return;
  chatPerf.start();
  chatPerf.recordSnapshotApply(184.5, { itemCount: 4800 });
  chatPerf.recordThreadProjection(233.1, { roundCount: 480 });
  chatPerf.recordVirtualWindowRecompute(12.4, { messageCount: 980 });
  chatPerf.recordTrajectoryLayout(96.7, { itemCount: 1000 });
  chatPerf.countSnapshotAbort(2);
  chatPerf.countWorkflowEventsItems(25000);
  chatPerf.stop();
  const report = chatPerf.snapshot();
  assert.equal(report.summary.heavyHistory.workflowEventItems, 25000);
  process.stdout.write(`HEAVY_HISTORY_PERF_REPORT ${JSON.stringify(report)}\n`);
});
