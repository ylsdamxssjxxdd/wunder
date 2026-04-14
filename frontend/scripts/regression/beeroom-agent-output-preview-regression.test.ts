import test from 'node:test';
import assert from 'node:assert/strict';

import { listRecentBeeroomAgentOutputs } from '../../src/components/beeroom/beeroomAgentOutputPreview';
import type { MissionChatMessage } from '../../src/components/beeroom/beeroomCanvasChatModel';

const buildMessage = (partial: Partial<MissionChatMessage>): MissionChatMessage => ({
  key: String(partial.key || `msg:${Math.random().toString(36).slice(2, 8)}`),
  senderName: String(partial.senderName || 'Agent'),
  senderAgentId: String(partial.senderAgentId || ''),
  avatarImageUrl: partial.avatarImageUrl,
  mention: String(partial.mention || ''),
  body: String(partial.body || ''),
  meta: String(partial.meta || ''),
  time: Number(partial.time || 0),
  timeLabel: String(partial.timeLabel || ''),
  tone: partial.tone || 'worker',
  sortOrder: partial.sortOrder
});

test('agent output preview keeps only the latest assistant-like messages for the selected agent', () => {
  const outputs = listRecentBeeroomAgentOutputs(
    [
      buildMessage({
        key: 'user-1',
        senderAgentId: '',
        senderName: 'User',
        body: 'question',
        time: 1,
        tone: 'user'
      }),
      buildMessage({
        key: 'agent-a-1',
        senderAgentId: 'agent-a',
        senderName: 'Agent A',
        body: 'first reply',
        time: 2,
        tone: 'worker'
      }),
      buildMessage({
        key: 'system-1',
        senderAgentId: 'agent-a',
        senderName: 'System',
        body: 'internal note',
        time: 3,
        tone: 'system'
      }),
      buildMessage({
        key: 'agent-b-1',
        senderAgentId: 'agent-b',
        senderName: 'Agent B',
        body: 'other agent reply',
        time: 4,
        tone: 'worker'
      }),
      buildMessage({
        key: 'agent-a-2',
        senderAgentId: 'agent-a',
        senderName: 'Agent A',
        body: 'latest reply',
        time: 5,
        tone: 'mother'
      })
    ],
    {
      agentId: 'agent-a',
      limit: 4
    }
  );

  assert.deepEqual(
    outputs.map((message) => message.key),
    ['agent-a-2', 'agent-a-1']
  );
});

test('agent output preview enforces the requested limit and ignores empty bodies', () => {
  const outputs = listRecentBeeroomAgentOutputs(
    [
      buildMessage({
        key: 'agent-a-1',
        senderAgentId: 'agent-a',
        body: 'one',
        time: 1,
        tone: 'worker'
      }),
      buildMessage({
        key: 'agent-a-empty',
        senderAgentId: 'agent-a',
        body: '   ',
        time: 2,
        tone: 'worker'
      }),
      buildMessage({
        key: 'agent-a-2',
        senderAgentId: 'agent-a',
        body: 'two',
        time: 3,
        tone: 'worker'
      }),
      buildMessage({
        key: 'agent-a-3',
        senderAgentId: 'agent-a',
        body: 'three',
        time: 4,
        tone: 'worker'
      })
    ],
    {
      agentId: 'agent-a',
      limit: 2
    }
  );

  assert.deepEqual(
    outputs.map((message) => message.key),
    ['agent-a-3', 'agent-a-2']
  );
  assert.deepEqual(listRecentBeeroomAgentOutputs(outputs, { agentId: '', limit: 2 }), []);
});
