import test from 'node:test';
import assert from 'node:assert/strict';

import {
  BEEROOM_SUBAGENT_REPLY_SORT_ORDER,
  BEEROOM_SUBAGENT_REQUEST_SORT_ORDER,
  compareMissionChatMessages,
  type MissionChatMessage
} from '../../src/components/beeroom/beeroomCanvasChatModel';

const buildMessage = (partial: Partial<MissionChatMessage>): MissionChatMessage => ({
  key: partial.key || 'message',
  senderName: partial.senderName || 'sender',
  senderAgentId: partial.senderAgentId || '',
  mention: partial.mention || '',
  body: partial.body || 'body',
  meta: partial.meta || '',
  time: partial.time || 1,
  timeLabel: partial.timeLabel || '2026-04-08 00:00:00',
  tone: partial.tone || 'worker',
  sortOrder: partial.sortOrder
});

test('subagent request stays before child reply and final assistant even when timestamps match', () => {
  const messages = [
    buildMessage({
      key: 'assistant-final',
      senderName: '默认智能体',
      body: '最终汇总',
      time: 100,
      tone: 'mother'
    }),
    buildMessage({
      key: 'subagent-reply',
      senderName: '子智能体',
      mention: '默认智能体',
      body: '子智能体回复',
      time: 100,
      tone: 'worker',
      sortOrder: BEEROOM_SUBAGENT_REPLY_SORT_ORDER
    }),
    buildMessage({
      key: 'subagent-request',
      senderName: '默认智能体',
      mention: '子智能体',
      body: '请绘制一个爱心',
      time: 100,
      tone: 'mother',
      sortOrder: BEEROOM_SUBAGENT_REQUEST_SORT_ORDER
    })
  ].sort(compareMissionChatMessages);

  assert.deepEqual(
    messages.map((message) => message.key),
    ['subagent-request', 'subagent-reply', 'assistant-final']
  );
});
