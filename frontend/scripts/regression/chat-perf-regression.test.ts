import test from 'node:test';
import assert from 'node:assert/strict';
import { emptyPerfMetrics, recordPerfDuration, incrementPerfCounter, summarizePerfMetrics } from '../../src/utils/chatPerfMetrics';
import { chatPerf } from '../../src/utils/chatPerf';

test('capture is inert before start and freezes after stop; starting again resets the report', () => {
  chatPerf.count('chat_shell_render');
  assert.deepEqual(chatPerf.snapshot().counters, {});
  chatPerf.start();
  chatPerf.count('chat_shell_render', 3);
  chatPerf.start();
  assert.deepEqual(chatPerf.snapshot().counters, { chat_shell_render: 3 });
  chatPerf.stop();
  const stopped = chatPerf.snapshot();
  chatPerf.count('chat_shell_render');
  chatPerf.recordDuration('chat_snapshot_flush', 100);
  assert.deepEqual(chatPerf.snapshot(), stopped);
  chatPerf.start();
  assert.deepEqual(chatPerf.snapshot().counters, {});
  chatPerf.stop();
});

test('normal and slow paths share one distribution with explicit percentile bounds', () => {
  const state = emptyPerfMetrics();
  for (let index = 0; index < 95; index++) recordPerfDuration(state, 'chat_stream_plain_text_flush', 10, index);
  for (let index = 0; index < 5; index++) recordPerfDuration(state, 'chat_stream_plain_text_slow_flush', 100, index);
  assert.deepEqual(summarizePerfMetrics(state).durations, {
    chat_stream_plain_text_flush: { count: 100, avgMs: 14.5, maxMs: 100, totalMs: 1450, p95UpperMs: 16 }
  });
});

test('large captures stay bounded and never retain arbitrary metadata or metric names', () => {
  const state = emptyPerfMetrics();
  for (let index = 0; index < 10000; index++) {
    incrementPerfCounter(state, 'chat_stream_event');
    incrementPerfCounter(state, `unknown-${index}`);
    recordPerfDuration(state, 'chat_workflow_entries_build', 60 + index, index,
      { contentLength: 64, content: 'private-value', sessionId: 'private-value', path: 'private-value' });
  }
  const report = summarizePerfMetrics(state);
  assert.deepEqual(report.counters, { chat_stream_event: 10000 });
  assert.equal(report.slowest.length, 30);
  assert.equal(report.slowest[0].durationMs, 10059);
  assert.equal(JSON.stringify(report).includes('private-value'), false);
  assert.ok(JSON.stringify(state).length < 10000);
  recordPerfDuration(state, 'chat_snapshot_flush', NaN, 0);
  assert.deepEqual(summarizePerfMetrics(state), report);
});
