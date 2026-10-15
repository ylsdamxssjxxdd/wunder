import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import {
  applyLocalQueueReorder,
  filterConsumedQueueItems,
  hasOnlyQueueMarkers,
  isChatQueueRuntimeEvent,
  isParkedQueueTurn,
  projectQueueItems,
  queueItemIdentities,
  resolveEmbeddedChatQueueRuntimeEvent,
  resolveConsumedQueueEventIdentities,
  selectParkedTurnMessageIds,
  type QueuedTurn
} from '../../src/stores/chatQueueProjection';

const parked = (queueId: string, content: string, createdAt: number): QueuedTurn =>
  projectQueueItems([{ queue_id: queueId, content, created_at: createdAt, position: createdAt }])[0];

test('projectQueueItems 丢弃无效行并按派发序补齐位次', () => {
  const items = projectQueueItems([
    { queue_id: 'task-b', content: '哈哈', created_at: 200, position: 1 },
    { queue_id: 'task-a', content: '额', created_at: 100, position: 0 },
    { queue_id: '   ', content: 'ignored', created_at: 50 },
    null,
    'not-an-object'
  ]);
  assert.deepEqual(items.map((item) => item.queueId), ['task-a', 'task-b']);
  assert.deepEqual(items.map((item) => item.position), [0, 1]);
  assert.equal(items[0].content, '额');
  assert.equal(items[0].status, 'pending');
});

test('projectQueueItems 让刚插话的条目排到最前', () => {
  const items = projectQueueItems([
    { queueId: 'task-a', createdAt: 100, position: 0, priority: 0 },
    { queueId: 'task-b', createdAt: 200, position: 1, priority: 1 }
  ]);
  assert.deepEqual(items.map((item) => item.queueId), ['task-b', 'task-a']);
});

test('applyLocalQueueReorder 保留用户给定的顺序，且只在完整覆盖时生效', () => {
  const items = [parked('task-a', 'first', 100), parked('task-b', 'second', 200), parked('task-c', 'third', 300)];
  const next = applyLocalQueueReorder(items, ['task-c', 'task-a', 'task-b']);
  assert.ok(next);
  assert.deepEqual(next?.map((item) => item.queueId), ['task-c', 'task-a', 'task-b']);
  assert.deepEqual(next?.map((item) => item.position), [0, 1, 2]);
  assert.equal(applyLocalQueueReorder(items, ['task-c', 'task-a']), null);
  assert.equal(applyLocalQueueReorder(items, ['task-c', 'task-a', 'unknown']), null);
});

test('isChatQueueRuntimeEvent 只认排队相关事件', () => {
  assert.ok(isChatQueueRuntimeEvent('queue_enter'));
  assert.ok(isChatQueueRuntimeEvent(' Queue_Update '));
  assert.ok(isChatQueueRuntimeEvent('queue_cancel'));
  assert.ok(isChatQueueRuntimeEvent('queued'));
  assert.equal(isChatQueueRuntimeEvent('llm_output_delta'), false);
  assert.equal(isChatQueueRuntimeEvent('thread_status'), false);
  assert.equal(isChatQueueRuntimeEvent(''), false);
});

test('queue_start 消费身份可挡住旧队列快照复活', () => {
  const items = projectQueueItems([
    { queue_id: 'task-running', client_message_id: 'cm-running', root_turn_id: 'root-running', position: 0 },
    { queue_id: 'task-next', client_message_id: 'cm-next', root_turn_id: 'root-next', position: 1 }
  ]);
  const identities = new Set(queueItemIdentities({ queue_id: 'task-running', client_message_id: 'cm-running' }));
  assert.deepEqual(filterConsumedQueueItems(items, identities).map((item) => item.queueId), ['task-next']);
  const started = resolveConsumedQueueEventIdentities(items, 'queue_start', {
    queue_id: 'task-running', client_message_id: 'cm-running', root_turn_id: 'root-running'
  });
  assert.ok(started.includes('root:root-running'));
  assert.ok(filterConsumedQueueItems(items, new Set(started)).some((item) => item.queueId === 'task-next'));
  const withoutQueueId = resolveConsumedQueueEventIdentities(items, 'queue_start', { root_turn_id: 'root-running' });
  assert.ok(withoutQueueId.includes('queue:task-running'), 'root identity should resolve the corresponding queue row');
  assert.deepEqual(
    resolveConsumedQueueEventIdentities(items, 'queue_start', {}),
    ['queue:task-running', 'client:cm-running', 'root:root-running'],
    'an unidentified start consumes only the queue head'
  );
});

test('持久 thread_change 内的 queue_finish 也能解析出被消费的队列身份', () => {
  const items = projectQueueItems([
    { queue_id: 'task-durable', client_message_id: 'cm-durable', root_turn_id: 'root-durable', position: 0 },
    { queue_id: 'task-next', client_message_id: 'cm-next', root_turn_id: 'root-next', position: 1 }
  ]);
  const embedded = resolveEmbeddedChatQueueRuntimeEvent('thread_change', {
    data: {
      change_type: 'item_upsert',
      item: {
        kind: 'queue',
        payload: {
          event_type: 'queue_finish',
          queue_id: 'task-durable',
          client_message_id: 'cm-durable',
          root_turn_id: 'root-durable'
        }
      }
    }
  });
  assert.equal(embedded?.eventType, 'queue_finish');
  const identities = resolveConsumedQueueEventIdentities(items, embedded?.eventType, embedded?.payload);
  assert.ok(identities.includes('queue:task-durable'));
  assert.deepEqual(
    filterConsumedQueueItems(items, new Set(identities)).map((item) => item.queueId),
    ['task-next']
  );
});

test('目标 thread_change.payload 形状同样能解析 queue 终态', () => {
  const embedded = resolveEmbeddedChatQueueRuntimeEvent('thread_change', {
    data: {
      change_type: 'item_upsert',
      payload: {
        item_id: 'turn:queue-item',
        kind: 'queue',
        payload: {
          event_type: 'queue_finish',
          queue_id: 'task-target',
          client_message_id: 'cm-target'
        }
      }
    }
  });
  assert.equal(embedded?.eventType, 'queue_finish');
  assert.equal(embedded?.payload.queue_id, 'task-target');
  assert.equal(embedded?.payload.client_message_id, 'cm-target');
});

const parkedItem = (overrides: Partial<QueuedTurn> = {}): QueuedTurn => ({
  ...projectQueueItems([{ queue_id: 'task-a', content: '后面那条', created_at: 100 }])[0],
  clientMessageId: 'cm-1',
  ...overrides
});

const queuedUser = { role: 'user', message_id: 'cm-1', client_message_id: 'cm-1', user_turn_id: 'user-turn:s:round:2' };
const queuedAssistant = {
  role: 'assistant',
  message_id: 'local-assistant:m-2',
  user_turn_id: 'user-turn:s:round:2',
  status: 'queued',
  content: '',
  workflowItems: [{ id: 'queue:task-a', eventType: 'queued', status: 'pending' }]
};

test('挂起中的轮次由排队条承载，时间线不放这一行', () => {
  const items = [parkedItem()];
  assert.ok(isParkedQueueTurn(items, queuedUser, queuedAssistant));
  // 补水后的形状没有 client_message_id，只能靠 `queue:<queue_id>` 标记对上。
  const hydrated = { ...queuedAssistant, workflowItems: [{ id: 'queue:task-a', event: 'queue_enter' }] };
  assert.ok(isParkedQueueTurn(items, { role: 'user', message_id: 'cm-1' }, hydrated));
});

test('开跑、已产出或不再明确排队的轮次照常渲染', () => {
  const items = [parkedItem()];
  assert.ok(isParkedQueueTurn([], queuedUser, queuedAssistant), '队列列表尚未同步时仍由排队条承载');
  assert.equal(isParkedQueueTurn(items, queuedUser, { ...queuedAssistant, content: '开始回答了' }), false);
  assert.equal(
    isParkedQueueTurn(items, queuedUser, {
      ...queuedAssistant,
      workflowItems: [{ id: 'tool-1', eventType: 'tool_call' }]
    }),
    false,
    '已经有非排队条目'
  );
  assert.equal(isParkedQueueTurn(items, queuedUser, { ...queuedAssistant, reasoningStreaming: true }), false);
  // 标记里的 queue_id 也能钉住这一轮，即使用户侧 id 对不上（补水形状）。
  assert.ok(isParkedQueueTurn(items, { role: 'user', message_id: 'other' }, queuedAssistant));
  assert.equal(
    isParkedQueueTurn(items, { role: 'user', message_id: 'other' }, {
      ...queuedAssistant,
      status: 'running',
      workflowItems: []
    }),
    false,
    '已经开跑且没有排队标记'
  );
  assert.equal(hasOnlyQueueMarkers({ role: 'user', content: '' }), false, '用户消息不是助手侧判定对象');
});

test('撤下一条没开跑的轮次会挑出本地占位的两条消息', () => {
  const messages = [
    { role: 'user', message_id: 'cm-0', client_message_id: 'cm-0' },
    { role: 'assistant', message_id: 'local-assistant:m-1', client_message_id: 'cm-0' },
    queuedUser,
    queuedAssistant,
    { role: 'assistant', message_id: 'local-assistant:m-3', content: '上一轮的回答' }
  ];
  assert.deepEqual(selectParkedTurnMessageIds(messages, 'cm-1'), ['cm-1', 'local-assistant:m-2']);
  // 已经产出的那一轮不属于排队条，一条都不许剪。
  assert.deepEqual(selectParkedTurnMessageIds(messages, 'cm-0'), []);
  assert.deepEqual(selectParkedTurnMessageIds(messages, 'missing'), []);
  assert.deepEqual(selectParkedTurnMessageIds(messages, ''), []);
  assert.deepEqual(selectParkedTurnMessageIds(undefined, 'cm-1'), []);
});

const readSource = (relative: string): string =>
  readFileSync(resolve(process.cwd(), 'src', relative), 'utf8');

test('排队让位与剪枝真的接在渲染与撤下路径上', () => {
  const controller = readSource('views/messenger/controller/messengerControllerInstall.ts');
  const slotsStart = controller.indexOf('ctx.agentTurnSlots = computed');
  const slots = slotsStart < 0 ? '' : controller.slice(slotsStart, controller.indexOf('ctx.agentConversationRows', slotsStart));
  assert.ok(slots, 'agentTurnSlots computed not found');
  assert.ok(
    slots.includes('.filter((slot) => !isParkedQueueTurn(queuedItems, slot.user, slot.assistant))'),
    '时间线没有按排队条让位'
  );
  assert.ok(slots.includes('chatQueueState.items'), '让位判定没有读排队条状态');

  const strip = readSource('components/chat/ComposerQueueStrip.vue');
  assert.ok(/emit\('withdrawn', result\.item\)/.test(strip), '撤下后没有广播 withdrawn');
  assert.ok(/handleWithdraw\(item, false\)/.test(strip), '删除路径没有走同一条撤下逻辑');

  const composer = readSource('components/chat/ChatComposer.vue');
  assert.ok(
    /@withdrawn="handleQueueTurnWithdrawn"/.test(composer) &&
      /chatStore\.dropParkedQueueTurn\(clientMessageId\)/.test(composer),
    '输入区没有剪掉撤下的轮次'
  );
  const sendActions = readSource('stores/chatSendActions.ts');
  assert.ok(
    /dropParkedQueueTurn\(clientMessageId: unknown\)/.test(sendActions) &&
      /selectParkedTurnMessageIds\(this\.messages/.test(sendActions) &&
      /removeThreadViewTurnByClientMessageId\(sessionId,cleanedClientMessageId\)/.test(sendActions),
    'store 上的剪枝动作缺失'
  );
  const queueState = readSource('stores/chatQueueState.ts');
  assert.ok(/resolveEmbeddedChatQueueRuntimeEvent/.test(queueState) && /consumedQueueIds/.test(queueState), '队列终态没有消费并屏蔽旧快照');
  assert.ok(/noteChatQueueRuntimeEvent\(targetId, eventType, payload\)/.test(readSource('stores/chatRuntimeState.ts')), '队列事件没有透传 payload 身份');
});
