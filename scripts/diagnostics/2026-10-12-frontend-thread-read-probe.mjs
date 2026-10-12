// Read-only synthetic diagnostics. No server or user data is accessed.
import { build } from 'esbuild';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';

const frontendRoot = fileURLToPath(new URL('../../frontend/', import.meta.url));
const localRequire = createRequire(resolve(frontendRoot, 'package.json'));
const source = `
import { performance } from 'node:perf_hooks';
import { emptyChatThreadState } from './src/realtime/chat/chatThreadTypes';
import { applyChatThreadSnapshot, applyChatThreadSnapshotPlanSharded, planChatThreadSnapshot } from './src/realtime/chat/chatThreadState';
import { buildChatThreadTurnSlots } from './src/realtime/chat/chatThreadProjection';
import { ensureChatThreadRuntime, resetChatThreadRuntime, hydrateChatThreadRuntime, buildChatThreadMaterializedSlots } from './src/realtime/chat/chatThreadRuntime';
import { resolveAgentTurnRenderRows, resolveShowThreadActivityCard } from './src/views/messenger/controller/legacyTurnRowAdapter';

(async () => {
  const turns = Array.from({ length: 4 }, (_, i) => ({
    turn_id: 'turn-' + i, root_turn_id: 'turn-' + i,
    user_turn_index: i + 1, status: 'completed',
    payload: { content: 'fixture-input-' + i }
  }));
  const items = turns.flatMap((turn, i) => [
    { item_id: turn.turn_id + ':user', turn_id: turn.turn_id,
      root_turn_id: turn.turn_id, kind: 'user_message', visibility: 'user',
      status: 'completed', revision: 1, payload: { content: 'fixture-input-' + i } },
    ...Array.from({ length: 40 }, (_, n) => ({
      item_id: turn.turn_id + ':text-' + n, turn_id: turn.turn_id,
      root_turn_id: turn.turn_id, kind: 'assistant_message', visibility: 'user',
      status: 'completed', revision: 1,
      payload: { content: 'fixture-output', model_round: n }
    }))
  ]);
  const full = emptyChatThreadState('fixture-thread');
  applyChatThreadSnapshot(full, { cursor: 200, turns, items, blocks: [] });
  const capped = emptyChatThreadState('fixture-thread');
  applyChatThreadSnapshot(capped, {
    cursor: 200, turns, items: items.slice(-20), blocks: [], truncated: true
  } as any);

  const key = 'fixture-thread';
  const store = { runtimeProjectionVersion: 0, runtimeProjectionVersionBySession: {},
    runtimeProjectionContentVersionBySession: {} } as any;
  const messages = [{ role: 'user', content: 'fixture-input' },
    { role: 'assistant', content: 'fixture-output' }];
  const decide = () => {
    const slots = buildChatThreadMaterializedSlots(key);
    const plan = resolveAgentTurnRenderRows({ windowRows: slots as any,
      windowAvailable: slots !== null, messages, enabled: true });
    return { source: plan.source, rows: plan.rows.length,
      activity_card: resolveShowThreadActivityCard({ renderKind: 'agent',
        hasTurnContent: plan.rows.length > 0, sessionId: key, historyLoading: false }) };
  };
  resetChatThreadRuntime(key);
  const absent = decide();
  ensureChatThreadRuntime(key);
  const allocated = decide();
  hydrateChatThreadRuntime(store, key, { cursor: 200, turns, items, blocks: [] } as any);
  const complete = decide();
  hydrateChatThreadRuntime(store, key, {
    cursor: 201, turns, items: items.slice(-20), blocks: [], truncated: true
  } as any);
  const truncated = decide();
  resetChatThreadRuntime(key);

  let stop = false;
  const interrupted = emptyChatThreadState('fixture-thread');
  applyChatThreadSnapshot(interrupted, { cursor: 200, turns, items, blocks: [] });
  const outcome = await applyChatThreadSnapshotPlanSharded(interrupted,
    planChatThreadSnapshot({ cursor: 201, turns, items, blocks: [] },
      { maxTurnsPerShard: 1, maxItemsPerShard: 1 }),
    { yieldBudget: 1, shouldAbort: () => stop, yieldToMain: async () => { stop = true; } });

  const timing = [];
  for (const roots of [20, 200]) {
    const sampleTurns = Array.from({ length: roots }, (_, i) => ({
      turn_id: 'sample-' + i, root_turn_id: 'sample-' + i,
      user_turn_index: i + 1, status: 'completed', content: 'fixture-input'
    }));
    const sampleItems = sampleTurns.flatMap(turn => [
      { item_id: turn.turn_id + ':user', turn_id: turn.turn_id,
        kind: 'user_message', content: 'fixture-input', revision: 1 },
      ...Array.from({ length: 100 }, (_, n) => ({ item_id: turn.turn_id + ':text-' + n,
        turn_id: turn.turn_id, kind: 'assistant_message', model_round: n,
        content: 'x'.repeat(4096), revision: 1 }))
    ]);
    const state = emptyChatThreadState('fixture-thread');
    applyChatThreadSnapshot(state, { cursor: 100000, turns: sampleTurns, items: sampleItems, blocks: [] });
    buildChatThreadTurnSlots(state);
    const samples = [];
    for (let i = 0; i < 3; i++) {
      const started = performance.now();
      buildChatThreadTurnSlots(state);
      samples.push(Number((performance.now() - started).toFixed(2)));
    }
    timing.push({ roots, items: sampleItems.length, unchanged_projection_ms: samples });
  }
  console.log(JSON.stringify({ scope: 'synthetic Node diagnostics; not production browser latency',
    snapshot: {
      complete: { turns: full.turns.size, items: full.items.size, slots: buildChatThreadTurnSlots(full).length },
      capped: { turns: capped.turns.size, items: capped.items.size,
        slots: buildChatThreadTurnSlots(capped).length, cursor: capped.lastSeq }
    },
    render_source: { absent, allocated, complete, truncated },
    interrupted_recovery: { outcome, turns: interrupted.turns.size,
      items: interrupted.items.size, cursor: interrupted.lastSeq },
    cache_hit_timing: timing
  }, null, 2));
})();
`;

const result = await build({
  stdin: { contents: source, resolveDir: frontendRoot, loader: 'ts' },
  bundle: true, platform: 'node', format: 'cjs', write: false,
  absWorkingDir: frontendRoot,
  alias: { '@': './src' }, define: { 'import.meta.env': '{}' }, logLevel: 'silent'
});
const module = { exports: {} };
new Function('require', 'module', 'exports', result.outputFiles[0].text)(
  localRequire, module, module.exports
);
