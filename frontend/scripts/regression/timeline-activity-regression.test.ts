import test from 'node:test';
import assert from 'node:assert/strict';
import { processActivityLabel, reasoningPreview } from '../../src/components/chat/timelineActivity';
import { resolveTaskIndicatorState } from '../../src/views/messenger/taskRuntimeState';

const t = (key: string, params?: Record<string, unknown>) => `${key}${params?.tool ? `:${params.tool}` : ''}`;

test('process phases describe current activity and terminal state wins over stale activity', () => {
  assert.equal(processActivityLabel({kind:'tool',tool:'fixture_tool'}, 'running', t), 'chat.timeline.executingTool:fixture_tool');
  assert.equal(processActivityLabel({kind:'thinking'}, 'running', t), 'chat.timeline.thinkingRunning');
  assert.equal(processActivityLabel({kind:'responding'}, 'running', t), 'chat.timeline.generatingReply');
  assert.equal(processActivityLabel({kind:'thinking'}, 'completed', t), 'chat.timeline.processCompleted');
  assert.equal(processActivityLabel({kind:'tool',tool:'fixture_tool'}, 'failed', t), 'messenger.thread.state.error');
  assert.equal(processActivityLabel(null, 'waiting_user_input', t), 'messenger.thread.state.inquiry');
});

test('live reasoning preview stays bounded and shows the latest stream tail', () => {
  const preview = reasoningPreview(`${'fixture earlier '.repeat(200)}fixture latest`);
  assert.ok(preview.length <= 641);
  assert.ok(preview.endsWith('fixture latest'));
  assert.ok(preview.startsWith('…'));
});

test('thread indicators distinguish inquiry and queue while retaining settled results', () => {
  assert.equal(resolveTaskIndicatorState('running', 'running', '', 'waiting_user_input'), 'inquiry');
  assert.equal(resolveTaskIndicatorState('pending', 'waiting_approval', ''), 'approval');
  assert.equal(resolveTaskIndicatorState('running', 'queued', ''), 'pending');
  assert.equal(resolveTaskIndicatorState('done', 'completed', 'waiting_user_input'), 'done');
  assert.equal(resolveTaskIndicatorState('idle', 'idle', 'queued'), 'idle');
  assert.equal(resolveTaskIndicatorState('error', 'failed', 'running'), 'error');
});
