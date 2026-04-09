import test from 'node:test';
import assert from 'node:assert/strict';

import {
  collectBeeroomHistoricalSubagentItems,
  mergeBeeroomMissionSubagentItems,
  type BeeroomMissionSubagentItem,
  type BeeroomSessionEventRecord
} from '../../src/components/beeroom/beeroomMissionSubagentState';

const buildDispatchEvent = (
  sessionId: string,
  runId: string,
  updatedTime: number,
  partial: Partial<Record<string, unknown>> = {}
): BeeroomSessionEventRecord => ({
  event: 'subagent_dispatch_item_update',
  timestamp_ms: updatedTime * 1000,
  data: {
    session_id: sessionId,
    run_id: runId,
    updated_time: updatedTime,
    title: partial.title ?? sessionId,
    status: partial.status ?? 'completed',
    terminal: partial.terminal ?? true,
    failed: partial.failed ?? false,
    cleanup: partial.cleanup ?? 'delete',
    requested_by: partial.requested_by ?? 'subagent_control',
    run_kind: partial.run_kind ?? 'subagent',
    parent_turn_ref: partial.parent_turn_ref ?? 'subagent_turn:2:1',
    parent_user_round: partial.parent_user_round ?? 2,
    parent_model_round: partial.parent_model_round ?? 1,
    summary: partial.summary ?? String(partial.status ?? 'completed'),
    assistant_message: partial.assistant_message ?? '',
    error_message: partial.error_message ?? ''
  }
});

const buildSubagent = (partial: Partial<BeeroomMissionSubagentItem>): BeeroomMissionSubagentItem => ({
  key: String(partial.key || partial.runId || partial.sessionId || 'subagent-key'),
  sessionId: String(partial.sessionId || 'sess_subagent'),
  runId: String(partial.runId || 'run_subagent'),
  runKind: String(partial.runKind || 'subagent'),
  requestedBy: String(partial.requestedBy || 'subagent_control'),
  spawnedBy: String(partial.spawnedBy || 'model'),
  agentId: String(partial.agentId || 'subagent-agent'),
  title: String(partial.title || 'Subagent'),
  label: String(partial.label || ''),
  status: String(partial.status || 'running'),
  summary: String(partial.summary || ''),
  userMessage: String(partial.userMessage || ''),
  assistantMessage: String(partial.assistantMessage || ''),
  errorMessage: String(partial.errorMessage || ''),
  updatedTime: Number(partial.updatedTime || 0),
  terminal: partial.terminal === true,
  failed: partial.failed === true,
  depth: partial.depth ?? null,
  role: String(partial.role || ''),
  controlScope: String(partial.controlScope || ''),
  spawnMode: String(partial.spawnMode || ''),
  strategy: String(partial.strategy || ''),
  dispatchLabel: String(partial.dispatchLabel || ''),
  controllerSessionId: String(partial.controllerSessionId || ''),
  parentSessionId: String(partial.parentSessionId || ''),
  parentTurnRef: String(partial.parentTurnRef || 'subagent_turn:2:1'),
  parentUserRound: partial.parentUserRound ?? 2,
  parentModelRound: partial.parentModelRound ?? 1,
  workflowItems: Array.isArray(partial.workflowItems) ? partial.workflowItems : []
});

test('historical child sessions remain available after cleanup delete terminal events', () => {
  const items = collectBeeroomHistoricalSubagentItems([
    buildDispatchEvent('sess_child_timeout', 'run_timeout', 100, {
      status: 'timeout',
      failed: true,
      error_message: 'timeout'
    }),
    buildDispatchEvent('sess_child_success', 'run_success', 140, {
      status: 'completed',
      assistant_message: 'done'
    })
  ], { latestTurnOnly: true });

  assert.equal(items.length, 2);
  assert.deepEqual(
    items.map((item) => item.sessionId),
    ['sess_child_success', 'sess_child_timeout']
  );
  assert.equal(items[0]?.terminal, true);
  assert.equal(items[1]?.failed, true);
});

test('historical latest-turn filter drops older parent turn children only', () => {
  const items = collectBeeroomHistoricalSubagentItems([
    buildDispatchEvent('sess_old_turn', 'run_old_turn', 90, {
      parent_turn_ref: 'subagent_turn:1:1',
      parent_user_round: 1,
      parent_model_round: 1
    }),
    buildDispatchEvent('sess_latest_a', 'run_latest_a', 110, {
      parent_turn_ref: 'subagent_turn:2:1',
      parent_user_round: 2,
      parent_model_round: 1
    }),
    buildDispatchEvent('sess_latest_b', 'run_latest_b', 120, {
      parent_turn_ref: 'subagent_turn:2:1',
      parent_user_round: 2,
      parent_model_round: 1
    })
  ], { latestTurnOnly: true });

  assert.deepEqual(
    items.map((item) => item.sessionId),
    ['sess_latest_b', 'sess_latest_a']
  );
});

test('live active child state is not overwritten by older historical terminal snapshots', () => {
  const merged = mergeBeeroomMissionSubagentItems(
    [
      buildSubagent({
        key: 'run_same',
        sessionId: 'sess_same',
        runId: 'run_same',
        status: 'running',
        updatedTime: 200,
        terminal: false,
        summary: 'still running'
      })
    ],
    [
      buildSubagent({
        key: 'run_same',
        sessionId: 'sess_same',
        runId: 'run_same',
        status: 'completed',
        updatedTime: 180,
        terminal: true,
        summary: 'stale terminal'
      })
    ]
  );

  assert.equal(merged.length, 1);
  assert.equal(merged[0]?.status, 'running');
  assert.equal(merged[0]?.terminal, false);
  assert.equal(merged[0]?.updatedTime, 200);
});
