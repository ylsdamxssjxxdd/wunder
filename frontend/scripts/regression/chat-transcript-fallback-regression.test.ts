import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import {
  LEGACY_TURN_ROW_KEY_PREFIX,
  TRANSCRIPT_FALLBACK_ENABLED_KEY,
  buildTranscriptTurnRows,
  isTranscriptFallbackEnabled,
  resolveAgentTurnRenderRows,
  resolveShowThreadActivityCard
} from '../../src/views/messenger/controller/legacyTurnRowAdapter';

// Fixture-only values: no real business names, identities or secrets.
const greeting = { role: 'assistant', content: 'Fixture greeting', isGreeting: true, message_id: 'g-1' };
const userOne = { role: 'user', content: 'Fixture input one', message_id: 'u-1' };
const assistantOne = { role: 'assistant', content: 'Fixture answer one', message_id: 'a-1' };
const userTwo = { role: 'user', content: 'Fixture input two', message_id: 'u-2' };
const assistantTwo = { role: 'assistant', content: 'Fixture answer two', message_id: 'a-2' };
const transcript = [greeting, userOne, assistantOne, userTwo, assistantTwo];

const windowRow = (id: string) => ({
  key: `turn:${id}`,
  rootTurnId: id,
  kind: 'turn' as const,
  user: { key: `runtime:user:${id}`, sourceIndex: 0, message: { role: 'user', content: `Fixture ${id}` } },
  assistant: { key: `runtime:assistant:${id}`, sourceIndex: 1, message: { role: 'assistant', content: `Fixture ${id}` } }
});

test('a failed snapshot renders turn rows instead of the activity grid', () => {
  const plan = resolveAgentTurnRenderRows({
    windowRows: [],
    // Registry has no entry: the durable render source is missing entirely.
    windowAvailable: false,
    messages: transcript,
    enabled: true
  });
  assert.equal(plan.source, 'transcript_fallback');
  assert.equal(plan.rows.length, 2, 'two user turns must produce two rows');
  for (const row of plan.rows) {
    assert.equal(row.kind, 'turn');
    assert.ok(row.key.startsWith('turn:'), 'row key must keep the v2 turn: prefix for height estimation');
    assert.ok(row.key.startsWith(LEGACY_TURN_ROW_KEY_PREFIX));
    assert.equal(row.user.message.role, 'user');
    assert.equal(row.assistant.message.role, 'assistant');
    assert.equal(row.assistant.message.content.length > 0, true);
  }
  assert.equal(
    resolveShowThreadActivityCard({ renderKind: 'agent', hasTurnContent: plan.rows.length > 0, sessionId: 'sess-1', historyLoading: false }),
    false,
    'rows rendered -> the grid must stay hidden'
  );
});

test('the synthetic greeting is never rendered twice by the fallback', () => {
  const rows = buildTranscriptTurnRows(transcript);
  assert.equal(rows.length, 2);
  assert.equal(
    rows.some((row) => row.user.message.isGreeting === true || row.assistant.message.isGreeting === true),
    false,
    'the greeting belongs to resolveSyntheticGreetingRenderable only'
  );
  assert.equal(
    buildTranscriptTurnRows([greeting]).length,
    0,
    'a greeting-only transcript must not invent a turn row'
  );
});

test('an available but empty window projection is a real empty thread, never a fallback', () => {
  const plan = resolveAgentTurnRenderRows({
    windowRows: [],
    // Registry owns the session and reports zero turns: do not overwrite it.
    windowAvailable: true,
    messages: transcript,
    enabled: true
  });
  assert.equal(plan.source, 'empty');
  assert.equal(plan.rows.length, 0);
  assert.equal(
    resolveShowThreadActivityCard({ renderKind: 'agent', hasTurnContent: false, sessionId: 'sess-1', historyLoading: false }),
    true,
    'legitimately empty thread keeps the grid'
  );
});

test('window projection and transcript fallback are mutually exclusive, never concatenated', () => {
  const windowRows = [windowRow('window-a'), windowRow('window-b')];
  const plan = resolveAgentTurnRenderRows({
    windowRows,
    windowAvailable: true,
    messages: transcript,
    enabled: true
  });
  assert.equal(plan.source, 'window');
  assert.equal(plan.rows.length, windowRows.length, 'fallback rows must not be appended to window rows');
  assert.equal(
    plan.rows.some((row) => row.key.startsWith(LEGACY_TURN_ROW_KEY_PREFIX)),
    false,
    'no un-alignable legacy key may appear next to v2 rows'
  );
  assert.equal(plan.rows[0], windowRows[0], 'window rows are passed through, not copied');
});

test('the switch forces the pure grid behaviour', () => {
  const plan = resolveAgentTurnRenderRows({
    windowRows: [],
    windowAvailable: false,
    messages: transcript,
    enabled: false
  });
  assert.equal(plan.source, 'empty');
  assert.equal(plan.rows.length, 0);
});

test('orphan assistant and hidden/internal records never open a row', () => {
  const rows = buildTranscriptTurnRows([
    { role: 'assistant', content: 'Fixture orphan reply' },
    { role: 'user', content: '', message_id: 'u-3' },
    { role: 'assistant', content: 'Fixture partial reply', message_id: 'a-3' },
    { role: 'assistant', content: 'Fixture internal note', meta: { type: 'model_context_internal' } }
  ]);
  assert.equal(rows.length, 1, 'only the accepted user input opens a page slot');
  assert.equal(rows[0].assistant.message.content, 'Fixture partial reply');
});

test('a user turn without a reply yet still opens a row (streaming placeholder)', () => {
  const rows = buildTranscriptTurnRows([{ role: 'user', content: 'Fixture pending input', message_id: 'u-4' }]);
  assert.equal(rows.length, 1);
  assert.equal(rows[0].kind, 'turn');
  assert.equal(rows[0].assistant.message.role, 'assistant');
  assert.equal(rows[0].assistant.message.content, '');
});

test('multiple assistant records between two user inputs collapse into one slot', () => {
  const rows = buildTranscriptTurnRows([
    { role: 'user', content: 'Fixture input', message_id: 'u-5' },
    { role: 'assistant', content: 'Fixture part one', message_id: 'a-5' },
    { role: 'assistant', content: 'Fixture part two', message_id: 'a-6' }
  ]);
  assert.equal(rows.length, 1);
  assert.equal(rows[0].assistant.message.content, 'Fixture part one\n\nFixture part two');
});

test('the grid decision keeps loading threads and other render kinds off the grid', () => {
  assert.equal(resolveShowThreadActivityCard({ renderKind: 'world', hasTurnContent: false, sessionId: 'sess-1', historyLoading: false }), false);
  assert.equal(resolveShowThreadActivityCard({ renderKind: 'agent', hasTurnContent: false, sessionId: '', historyLoading: false }), true);
  assert.equal(
    resolveShowThreadActivityCard({ renderKind: 'agent', hasTurnContent: false, sessionId: 'sess-1', historyLoading: true }),
    false,
    'history still loading must not flash the grid over a thread with turns'
  );
});

test('the fallback switch is fail-open and can be turned off with one key', () => {
  const original = (globalThis as { localStorage?: unknown }).localStorage;
  try {
    (globalThis as { localStorage?: unknown }).localStorage = { getItem: () => null };
    assert.equal(isTranscriptFallbackEnabled(), true, 'unset must fail open');
    (globalThis as { localStorage?: unknown }).localStorage = { getItem: () => '0' };
    assert.equal(isTranscriptFallbackEnabled(), false);
    (globalThis as { localStorage?: unknown }).localStorage = { getItem: () => 'false' };
    assert.equal(isTranscriptFallbackEnabled(), false);
    (globalThis as { localStorage?: unknown }).localStorage = {
      getItem: () => {
        throw new Error('fixture storage failure');
      }
    };
    assert.equal(isTranscriptFallbackEnabled(), true, 'unreadable storage must fail open');
    assert.equal(TRANSCRIPT_FALLBACK_ENABLED_KEY, 'wunder_chat_transcript_fallback');
  } finally {
    if (original === undefined) delete (globalThis as { localStorage?: unknown }).localStorage;
    else (globalThis as { localStorage?: unknown }).localStorage = original;
  }
});

const readSource = (relative: string): string => readFileSync(resolve(process.cwd(), 'src', relative), 'utf8');

test('the fallback is really wired into the controller render source and the panel grid decision', () => {
  const controller = readSource('views/messenger/controller/messengerControllerInstall.ts');
  assert.ok(controller.includes('ctx.agentThreadSlotSource = computed'), 'the raw window source is not kept');
  assert.ok(
    controller.includes('windowAvailable: ctx.agentThreadSlotSource.value !== null'),
    'null (failed snapshot) is not told apart from [] (real empty thread)'
  );
  assert.ok(
    controller.includes('isTranscriptFallbackEnabled') &&
      controller.includes('resolveAgentTurnRenderRows({'),
    'the controller does not arbitrate the render source'
  );
  assert.ok(
    controller.includes('.filter((slot) => !isParkedQueueTurn(queuedItems, slot.user, slot.assistant))'),
    'queue yielding was lost while moving the source'
  );

  const panel = readSource('views/messenger/sections/MessengerMessagePanel.vue');
  const gridStart = panel.indexOf('const showThreadActivityCard');
  const grid = gridStart < 0 ? '' : panel.slice(gridStart, panel.indexOf('onUpdated(', gridStart));
  assert.ok(grid, 'showThreadActivityCard not found');
  assert.ok(grid.includes('resolveShowThreadActivityCard('), 'the grid decision is not routed through the adapter');
  assert.ok(grid.includes('agentTurnRowsHaveContent'), 'fallback rows do not hide the grid');
  assert.ok(grid.includes('hasTurnContent'), 'the grid decision ignores turn content');
});
