import test from 'node:test';
import assert from 'node:assert/strict';

import {
  BEEROOM_SUBAGENT_REPLY_SORT_ORDER,
  BEEROOM_SUBAGENT_REQUEST_SORT_ORDER,
  collapseMissionChatAssistantTurns,
  compareMissionChatMessages,
  type MissionChatMessage
} from '../../src/components/beeroom/beeroomCanvasChatModel';
import { reconcileBeeroomSessionBackedManualMessages } from '../../src/components/beeroom/beeroomMissionChatSync';

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

test('dispatch chat keeps only the final assistant reply for each user turn', () => {
  const messages = collapseMissionChatAssistantTurns([
    buildMessage({
      key: 'user-1',
      senderName: '用户',
      body: '第一问',
      time: 10,
      tone: 'user'
    }),
    buildMessage({
      key: 'assistant-1-mid',
      senderName: '默认智能体',
      body: '中间回复',
      time: 11,
      tone: 'mother'
    }),
    buildMessage({
      key: 'assistant-1-final',
      senderName: '默认智能体',
      body: '最终回复',
      time: 12,
      tone: 'mother'
    }),
    buildMessage({
      key: 'user-2',
      senderName: '用户',
      body: '第二问',
      time: 20,
      tone: 'user'
    }),
    buildMessage({
      key: 'assistant-2-final',
      senderName: '默认智能体',
      body: '第二次最终回复',
      time: 21,
      tone: 'mother'
    })
  ]);

  assert.deepEqual(
    messages.map((message) => message.key),
    ['user-1', 'assistant-1-final', 'user-2', 'assistant-2-final']
  );
});

test('session hydration preserves optimistic second user message until remote history catches up', () => {
  const reconciled = reconcileBeeroomSessionBackedManualMessages({
    sessionId: 'sess_main',
    limit: 120,
    current: [
      buildMessage({
        key: 'session:sess_main:message:user:100:1',
        senderName: '用户',
        mention: '默认智能体',
        body: '第一问',
        time: 100,
        tone: 'user'
      }),
      buildMessage({
        key: 'session:sess_main:message:assistant:101:2',
        senderName: '默认智能体',
        mention: '用户',
        body: '第一答',
        time: 101,
        tone: 'mother'
      }),
      buildMessage({
        key: 'user:102:1',
        senderName: '用户',
        mention: '默认智能体',
        body: '第二问',
        time: 102,
        tone: 'user'
      })
    ],
    incoming: [
      buildMessage({
        key: 'session:sess_main:message:user:100:1',
        senderName: '用户',
        mention: '默认智能体',
        body: '第一问',
        time: 100,
        tone: 'user'
      }),
      buildMessage({
        key: 'session:sess_main:message:assistant:101:2',
        senderName: '默认智能体',
        mention: '用户',
        body: '第一答',
        time: 101,
        tone: 'mother'
      })
    ]
  });

  assert.deepEqual(
    reconciled.map((message) => message.key),
    [
      'session:sess_main:message:user:100:1',
      'session:sess_main:message:assistant:101:2',
      'user:102:1'
    ]
  );
});

test('session hydration drops optimistic user message after matching remote message arrives', () => {
  const reconciled = reconcileBeeroomSessionBackedManualMessages({
    sessionId: 'sess_main',
    limit: 120,
    current: [
      buildMessage({
        key: 'session:sess_main:message:user:100:1',
        senderName: '用户',
        mention: '默认智能体',
        body: '第一问',
        time: 100,
        tone: 'user'
      }),
      buildMessage({
        key: 'user:102:1',
        senderName: '用户',
        mention: '默认智能体',
        body: '第二问',
        time: 102,
        tone: 'user'
      })
    ],
    incoming: [
      buildMessage({
        key: 'session:sess_main:message:user:100:1',
        senderName: '用户',
        mention: '默认智能体',
        body: '第一问',
        time: 100,
        tone: 'user'
      }),
      buildMessage({
        key: 'session:sess_main:message:user:102:3',
        senderName: '用户',
        mention: '默认智能体',
        body: '第二问',
        time: 102,
        tone: 'user'
      })
    ]
  });

  assert.deepEqual(
    reconciled.map((message) => message.key),
    [
      'session:sess_main:message:user:100:1',
      'session:sess_main:message:user:102:3'
    ]
  );
});
