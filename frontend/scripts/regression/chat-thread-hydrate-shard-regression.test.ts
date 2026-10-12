// W2-F regression: sharded, abortable snapshot hydrate + Worker pool hygiene.
// Every assertion below is written so the pre-W2-F writing fails it:
//   - the old full-snapshot apply was one unbounded synchronous pass (zero
//     yields, signal ignored, cursor always advanced);
//   - the old hydrate restored pending turns with an O(pending x turns) nested
//     scan (one turn-map walk per pending turn);
//   - there was no snapshot worker pool and therefore no teardown guarantee.
import test from 'node:test';
import assert from 'node:assert/strict';

import {
  applyChatThreadSnapshotPlanSharded,
  applyChatThreadSnapshotPlanSync,
  planChatThreadSnapshot
} from '../../src/realtime/chat/chatThreadState';
import { emptyChatThreadState } from '../../src/realtime/chat/chatThreadTypes';
import type { ThreadTurnState } from '../../src/realtime/chat/chatThreadTypes';
import {
  ensureChatThreadRuntime,
  getChatThreadState,
  hydrateChatThreadRuntime,
  resetChatThreadRuntime
} from '../../src/realtime/chat/chatThreadRuntime';
import {
  getChatThreadSnapshotWorkerStats,
  planChatThreadSnapshotInWorker,
  setChatThreadSnapshotWorkerFactory,
  terminateChatThreadSnapshotWorker
} from '../../src/workers/chatThreadSnapshotClient';
import { createChatRuntimeProjection, applyChatRuntimeEvent } from '../../src/realtime/chat/chatRuntimeReducer';
import { collectChatRuntimeInvariantViolations } from '../../src/realtime/chat/chatRuntimeInvariants';

const makeStore = (key: string) => ({
  activeSessionId: key,
  sessionVersions: {} as Record<string, number>,
  contentVersions: {} as Record<string, number>,
  reasoningVersions: {} as Record<string, number>,
  runtimeProjectionVersionBySession: {} as Record<string, number>,
  runtimeProjectionContentVersionByMessage: {} as Record<string, number>,
  runtimeProjectionReasoningVersionByMessage: {} as Record<string, number>
});

// Generic fixtures only: no business names, identities or secrets.
const buildSnapshot = (turnCount: number, itemsPerTurn: number) => {
  const turns: Array<Record<string, unknown>> = [];
  const items: Array<Record<string, unknown>> = [];
  const blocks: Array<Record<string, unknown>> = [];
  for (let t = 0; t < turnCount; t += 1) {
    const turnId = `turn-${t}`;
    turns.push({ turn_id: turnId, user_round: t + 1, status: 'completed', content: `input ${t}` });
    for (let i = 0; i < itemsPerTurn; i += 1) {
      items.push({
        item_id: `${turnId}:text-${i}`, turn_id: turnId, kind: 'assistant_message',
        model_round: i + 1, revision: 1, visibility: 'user', role: 'assistant',
        content: `output ${t}-${i}`, reasoning: ''
      });
    }
    blocks.push({ item_id: `${turnId}:text-0`, field: 'content', block_index: 0, content_offset: 0, content: `block ${t}` });
  }
  return { cursor: 5000, turns, items, blocks };
};

test('sharded snapshot apply yields the main thread and matches the single-pass result', async () => {
  const snapshot = buildSnapshot(90, 6);
  const plan = planChatThreadSnapshot(snapshot, { maxTurnsPerShard: 5, maxItemsPerShard: 10, maxBlocksPerShard: 5 });
  assert.ok(plan.shards.length >= 60, `plan must be finely sharded, got ${plan.shards.length}`);

  const state = emptyChatThreadState('shard-a');
  const slices: number[] = [];
  let last = process.hrtime.bigint();
  const outcome = await applyChatThreadSnapshotPlanSharded(state, plan, {
    yieldToMain: async () => {
      const now = process.hrtime.bigint();
      slices.push(Number(now - last) / 1e6);
      last = now;
    },
    yieldBudget: 8
  });
  slices.push(Number(process.hrtime.bigint() - last) / 1e6);

  assert.equal(outcome.applied, true);
  assert.equal(outcome.aborted, false);
  // Negative guard: the pre-W2-F apply yielded zero times in one giant task.
  assert.ok(outcome.yields >= 20, `expected many main-thread yields, got ${outcome.yields}`);
  assert.ok(Math.max(...slices) < 200, `longest main-thread slice was ${Math.max(...slices)}ms`);

  // Order-preserving shards must reproduce the synchronous single-pass apply.
  const reference = emptyChatThreadState('shard-ref');
  applyChatThreadSnapshotPlanSync(reference, plan, 0);
  assert.equal(state.lastSeq, reference.lastSeq);
  assert.equal(state.turns.size, reference.turns.size);
  assert.equal(state.items.size, reference.items.size);
  assert.equal(state.blocks.size, reference.blocks.size);
  for (const [itemId, item] of reference.items) {
    const mirror = state.items.get(itemId);
    assert.equal(mirror?.content, item.content);
    assert.equal(mirror?.order, item.order, `registration order drifted for ${itemId}`);
  }
});

test('sharded snapshot apply aborts mid-flight without advancing the durable cursor', async () => {
  const snapshot = buildSnapshot(40, 4);
  const plan = planChatThreadSnapshot(snapshot, { maxTurnsPerShard: 2, maxItemsPerShard: 4, maxBlocksPerShard: 2 });
  const state = emptyChatThreadState('shard-abort');
  let checks = 0;
  const outcome = await applyChatThreadSnapshotPlanSharded(state, plan, {
    shouldAbort: () => (++checks > 4),
    yieldToMain: async () => {},
    yieldBudget: 1
  });

  assert.equal(outcome.aborted, true);
  assert.ok(outcome.shardsApplied > 0 && outcome.shardsApplied < plan.shards.length);
  // Negative guard: the pre-W2-F apply could not be stopped and always bound
  // the snapshot cursor as its last act.
  assert.equal(state.lastSeq, 0, 'an aborted rebuild must not claim the snapshot cursor');
});

test('hydrate restores pending turns with a bounded number of turn-map scans', () => {
  class CountingTurns extends Map<string, ThreadTurnState> {
    valuesCalls = 0;
    values(): IterableIterator<ThreadTurnState> {
      this.valuesCalls += 1;
      return super.values();
    }
  }
  const key = 'hydrate-scan';
  resetChatThreadRuntime(key);
  const entry = ensureChatThreadRuntime(key);
  const counting = new CountingTurns();
  (entry.state as unknown as { turns: Map<string, ThreadTurnState> }).turns = counting;

  const pendingCount = 40;
  for (let i = 0; i < pendingCount; i += 1) {
    counting.set(`pending:c${i}`, {
      turnId: `pending:c${i}`, clientMessageId: `c${i}`, userRound: i + 1,
      status: 'running', userContent: `local ${i}`
    });
  }
  const turns: Array<Record<string, unknown>> = [];
  for (let t = 0; t < 30; t += 1) {
    turns.push({ turn_id: `server-${t}`, user_round: t + 100, status: 'completed', content: `server ${t}` });
  }

  const changed = hydrateChatThreadRuntime(makeStore(key), key, { cursor: 100, turns, items: [], blocks: [] });
  assert.equal(changed, true);
  // Negative guard: the pre-W2-F pending loop walked the turn map once per
  // pending turn (1 + pendingCount scans); the fix is a constant number.
  assert.ok(counting.valuesCalls <= 3, `hydrate walked the turn map ${counting.valuesCalls} times`);

  const state = getChatThreadState(key)!;
  assert.equal(state.turns.has('server-0'), true);
  assert.equal(state.turns.get('pending:c0')?.userContent, 'local 0');
  assert.equal(state.turns.get('pending:c39')?.userContent, 'local 39');
});

test('snapshot planner falls back to the main thread when no worker exists', async () => {
  const planned = await planChatThreadSnapshotInWorker({ cursor: 3, turns: [], items: [], blocks: [] });
  assert.equal(planned, null);
});

test('snapshot worker pool leaves no residual state after termination', async () => {
  class FakeWorker {
    onmessage: ((event: MessageEvent) => void) | null = null;
    onerror: ((event: unknown) => void) | null = null;
    terminated = false;
    postMessage(_message: unknown): void {}
    terminate(): void { this.terminated = true; }
  }
  setChatThreadSnapshotWorkerFactory(() => new FakeWorker() as unknown as Worker);
  try {
    const first = planChatThreadSnapshotInWorker({ cursor: 1, turns: [], items: [], blocks: [] });
    const second = planChatThreadSnapshotInWorker({ cursor: 2, turns: [], items: [], blocks: [] });
    const busy = getChatThreadSnapshotWorkerStats();
    assert.equal(busy.available, true);
    assert.equal(busy.workerAlive, true);
    assert.equal(busy.active, true, 'first request owns the worker');
    assert.equal(busy.queued, 1, 'second request waits in the queue');

    terminateChatThreadSnapshotWorker();
    const idle = getChatThreadSnapshotWorkerStats();
    // Negative guard: a teardown that only stops the worker would leave the
    // queued/active jobs dangling; no queue entry may survive.
    assert.equal(idle.workerAlive, false);
    assert.equal(idle.active, false);
    assert.equal(idle.queued, 0);
    assert.deepEqual(await Promise.all([first, second]), [null, null]);
  } finally {
    setChatThreadSnapshotWorkerFactory(null);
  }
});

test('hydrated durable state keeps chatRuntimeInvariants green', () => {
  const user = { message_id: 'u-1', role: 'user', content: 'hello', user_turn_id: 'turn-1' };
  const assistant = {
    message_id: 'a-1', role: 'assistant', content: 'hi', user_turn_id: 'turn-1', model_turn_id: 'turn-1:model:1'
  };
  const projection = createChatRuntimeProjection();
  applyChatRuntimeEvent(projection, {
    event_type: 'session_snapshot', source: 'snapshot', strict: false,
    session_id: 'invariant-session', messages: [user, assistant], payload: { runtime_status: 'idle' }
  } as never);
  const session = projection.sessions['invariant-session'];
  assert.ok(session, 'session projection must exist');
  assert.deepEqual(collectChatRuntimeInvariantViolations(session), []);
});
