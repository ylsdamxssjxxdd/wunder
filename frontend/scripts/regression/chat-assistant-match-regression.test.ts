import test from 'node:test';
import assert from 'node:assert/strict';

import {
  buildAssistantMatchEntries,
  findAnchoredAssistantContentMatchIndex
} from '../../src/stores/chatAssistantMatch';

test('assistant match anchors keep repeated model rounds isolated across user turns', () => {
  const firstAssistant = {
    role: 'assistant',
    content: 'first answer',
    created_at: '2026-04-12T23:59:52.388Z',
    stream_round: 1
  };
  const secondAssistant = {
    role: 'assistant',
    content: 'second answer',
    created_at: '2026-04-13T00:00:10.720Z',
    stream_round: 1
  };
  const liveMessages = [
    { role: 'assistant', content: 'hello', isGreeting: true },
    { role: 'user', content: 'first user turn' },
    firstAssistant,
    { role: 'user', content: 'second user turn' },
    secondAssistant
  ];

  const hydratedMessages = [
    { role: 'assistant', content: 'hello', isGreeting: true },
    { role: 'user', content: 'first user turn' },
    { role: 'assistant', content: 'first answer from server', stream_round: 1 },
    { role: 'user', content: 'second user turn' },
    { role: 'assistant', content: 'second answer from server', stream_round: 1 }
  ];

  const liveEntries = buildAssistantMatchEntries(liveMessages);
  const hydratedEntries = buildAssistantMatchEntries(hydratedMessages);

  assert.equal(liveEntries.length, 2);
  assert.equal(hydratedEntries.length, 2);
  assert.deepEqual(
    liveEntries.map(({ userTurnIndex, assistantTurnIndex }) => ({ userTurnIndex, assistantTurnIndex })),
    [
      { userTurnIndex: 1, assistantTurnIndex: 0 },
      { userTurnIndex: 2, assistantTurnIndex: 0 }
    ]
  );

  const secondHydratedEntry = hydratedEntries[1];
  const anchoredCandidates = liveEntries.filter(
    (entry) =>
      entry.userTurnIndex === secondHydratedEntry.userTurnIndex &&
      entry.assistantTurnIndex === secondHydratedEntry.assistantTurnIndex
  );

  assert.equal(anchoredCandidates.length, 1);
  assert.equal(anchoredCandidates[0].message, secondAssistant);
  assert.notEqual(anchoredCandidates[0].message, firstAssistant);
});

test('assistant content fallback still matches the same turn when timestamps drift', () => {
  const liveEntries = buildAssistantMatchEntries([
    { role: 'user', content: 'first user turn' },
    {
      role: 'assistant',
      content: 'first answer',
      created_at: '2026-04-13T00:54:25.999Z'
    },
    { role: 'user', content: 'second user turn' },
    {
      role: 'assistant',
      content: 'second formal answer',
      created_at: '2026-04-13T00:54:31.190+08:00'
    }
  ]);
  const hydratedEntries = buildAssistantMatchEntries([
    { role: 'user', content: 'first user turn' },
    {
      role: 'assistant',
      content: 'first answer',
      created_at: '2026-04-13T08:54:15.694396842+08:00'
    },
    { role: 'user', content: 'second user turn' },
    {
      role: 'assistant',
      content: 'second formal answer',
      created_at: '2026-04-13T08:54:31.190437674+08:00'
    }
  ]);

  const matchIndex = findAnchoredAssistantContentMatchIndex(
    hydratedEntries[0],
    hydratedEntries[0]?.message.content,
    liveEntries
  );

  assert.equal(matchIndex, 0);
  assert.equal(liveEntries[matchIndex]?.message.content, 'first answer');
});
