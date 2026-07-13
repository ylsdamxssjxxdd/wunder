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
  sessionId: partial.sessionId,
  clientMessageId: partial.clientMessageId,
  userTurnId: partial.userTurnId,
  modelTurnId: partial.modelTurnId,
  turnOrder: partial.turnOrder,
  messageOrder: partial.messageOrder,
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

test('dispatch chat keeps user message before assistant when initial snapshot timestamps match', () => {
  const messages = [
    buildMessage({
      key: 'session:sess_main:message:assistant:100:2',
      senderName: '默认智能体',
      mention: '用户',
      body: '回复',
      time: 100,
      tone: 'mother'
    }),
    buildMessage({
      key: 'session:sess_main:message:user:100:1',
      senderName: '用户',
      mention: '默认智能体',
      body: '提问',
      time: 100,
      tone: 'user'
    })
  ].sort(compareMissionChatMessages);

  assert.deepEqual(
    messages.map((message) => message.key),
    ['session:sess_main:message:user:100:1', 'session:sess_main:message:assistant:100:2']
  );
});

test('canonical user turn order wins over inverted timestamps', () => {
  const messages = [
    buildMessage({
      key: 'round-2-user',
      userTurnId: 'user-turn:sess_main:round:2',
      turnOrder: 2,
      messageOrder: 3,
      body: 'second',
      time: 10,
      tone: 'user'
    }),
    buildMessage({
      key: 'round-1-assistant',
      userTurnId: 'user-turn:sess_main:round:1',
      modelTurnId: 'model-turn:sess_main:user:1:model:1',
      turnOrder: 1,
      messageOrder: 2,
      body: 'first reply',
      time: 30,
      tone: 'mother'
    }),
    buildMessage({
      key: 'round-1-user',
      userTurnId: 'user-turn:sess_main:round:1',
      turnOrder: 1,
      messageOrder: 1,
      body: 'first',
      time: 20,
      tone: 'user'
    })
  ].sort(compareMissionChatMessages);

  assert.deepEqual(messages.map((message) => message.key), [
    'round-1-user',
    'round-1-assistant',
    'round-2-user'
  ]);
});

test('session reconciliation never preserves a message from another dispatch session', () => {
  const reconciled = reconcileBeeroomSessionBackedManualMessages({
    sessionId: 'sess_current',
    limit: 120,
    current: [
      buildMessage({
        key: 'user:old:1',
        sessionId: 'sess_other',
        senderName: 'User',
        body: 'other session message',
        time: 10,
        tone: 'user'
      })
    ],
    incoming: [
      buildMessage({
        key: 'session:sess_current:message:user:20:1',
        sessionId: 'sess_current',
        senderName: 'User',
        body: 'current session message',
        time: 20,
        tone: 'user'
      })
    ]
  });

  assert.deepEqual(reconciled.map((message) => message.body), ['current session message']);
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

test('empty history hydration keeps the session-scoped optimistic user bubble', () => {
  const reconciled = reconcileBeeroomSessionBackedManualMessages({
    sessionId: 'sess_main',
    limit: 120,
    current: [
      buildMessage({
        key: 'session:sess_main:message:user:300:0',
        sessionId: 'sess_main',
        userTurnId: 'user-turn:sess_main:round:3',
        senderName: 'User',
        body: 'pending request',
        time: 300,
        tone: 'user'
      })
    ],
    incoming: []
  });

  assert.deepEqual(reconciled.map((message) => message.key), [
    'session:sess_main:message:user:300:0'
  ]);
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
      'user:102:1'
    ]
  );
  assert.equal(reconciled[1]?.remoteKey, 'session:sess_main:message:user:102:3');
});

test('session hydration coalesces history and runtime keys for the same canonical user turn', () => {
  const reconciled = reconcileBeeroomSessionBackedManualMessages({
    sessionId: 'sess_main',
    limit: 120,
    current: [],
    incoming: [
      buildMessage({
        key: 'session:sess_main:message:user:200:0',
        sessionId: 'sess_main',
        userTurnId: 'user-turn:sess_main:round:9',
        senderName: 'User',
        body: 'request',
        time: 200,
        tone: 'user'
      }),
      buildMessage({
        key: 'session:sess_main:history:42',
        sessionId: 'sess_main',
        userTurnId: 'user-turn:sess_main:round:9',
        senderName: 'User',
        body: 'request',
        time: 200,
        tone: 'user'
      })
    ]
  });

  assert.deepEqual(reconciled.map((message) => message.key), ['session:sess_main:history:42']);
});

test('session hydration preserves optimistic render key when matching remote message arrives', () => {
  const reconciled = reconcileBeeroomSessionBackedManualMessages({
    sessionId: 'sess_main',
    limit: 120,
    current: [
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
        key: 'session:sess_main:message:user:102:3',
        senderName: '用户',
        mention: '默认智能体',
        body: '第二问',
        time: 102,
        tone: 'user'
      })
    ]
  });

  assert.equal(reconciled.length, 1);
  assert.equal(reconciled[0]?.key, 'user:102:1');
  assert.equal(reconciled[0]?.remoteKey, 'session:sess_main:message:user:102:3');
});

test('partial streaming projection does not remove earlier observed dispatch turns', () => {
  const reconciled = reconcileBeeroomSessionBackedManualMessages({
    sessionId: 'sess_main',
    limit: 120,
    current: [
      buildMessage({
        key: 'session:sess_main:message:user:100:1',
        sessionId: 'sess_main',
        userTurnId: 'user-turn:sess_main:round:1',
        body: 'first request',
        time: 100,
        tone: 'user'
      }),
      buildMessage({
        key: 'session:sess_main:message:user:200:2',
        sessionId: 'sess_main',
        userTurnId: 'user-turn:sess_main:round:2',
        body: 'second request',
        time: 200,
        tone: 'user'
      })
    ],
    incoming: [
      buildMessage({
        key: 'session:sess_main:message:assistant:201:3',
        sessionId: 'sess_main',
        userTurnId: 'user-turn:sess_main:round:2',
        modelTurnId: 'model-turn:sess_main:round:2:1',
        body: 'streaming reply',
        time: 201,
        tone: 'mother'
      })
    ]
  });

  assert.deepEqual(reconciled.map((message) => message.body), [
    'first request',
    'second request',
    'streaming reply'
  ]);
});

test('mixed optimistic user and canonical assistant stay in chronological role order', () => {
  const messages = [
    buildMessage({
      key: 'assistant-canonical',
      userTurnId: 'user-turn:sess_main:round:3',
      turnOrder: 3,
      body: 'reply',
      time: 301,
      tone: 'mother'
    }),
    buildMessage({
      key: 'user-optimistic',
      body: 'request',
      time: 300,
      tone: 'user'
    })
  ].sort(compareMissionChatMessages);

  assert.deepEqual(messages.map((message) => message.key), ['user-optimistic', 'assistant-canonical']);
});

test('same client submission is coalesced before canonical turn promotion', () => {
  const reconciled = reconcileBeeroomSessionBackedManualMessages({
    sessionId: 'sess_main',
    limit: 120,
    current: [],
    incoming: [
      buildMessage({
        key: 'session:sess_main:message:user:400:2',
        sessionId: 'sess_main',
        clientMessageId: 'client-message-1',
        body: 'request',
        time: 400,
        tone: 'user'
      }),
      buildMessage({
        key: 'session:sess_main:message:user:400:0',
        sessionId: 'sess_main',
        clientMessageId: 'client-message-1',
        body: 'request',
        time: 400,
        tone: 'user'
      })
    ]
  });

  assert.equal(reconciled.length, 1);
  assert.equal(reconciled[0]?.body, 'request');
});
