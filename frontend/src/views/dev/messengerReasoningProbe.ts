import { nextTick } from 'vue';
import { useChatStore } from '@/stores/chat';
import { syncChatRuntimeProjectionFromSnapshot } from '@/stores/chatRuntimeState';
import { chatPerf } from '@/utils/chatPerf';
import { applyThreadChange, applyThreadTextBlock, installChatThreadFixture } from './messengerThreadFixture';

const settle = (ms = 200) => new Promise(resolve => setTimeout(resolve, ms));

/**
 * 推理流在**真实渲染链**上的自检。
 *
 * 渲染源是 durable thread 状态（`buildChatThreadMaterializedSlots`），因此这里用
 * `installChatThreadFixture` 装载 fixture、用 `thread_item_tail` 推推理增量——
 * 与线上 watch/send 路径同一套帧。推理在时间线里由「思考条目」呈现：
 * 条目摘要是有界预览（160 字），展开后的正文必须跟着流更新。
 */
export const runMessengerReasoningProbe = async (sessionId: string) => {
  const chat = useChatStore();
  const userTurnId = 'reasoning-turn';
  const modelTurnId = 'reasoning-model';
  const assistantMessageId = 'reasoning-message';
  const modelRound = 1;
  const textItemId = `${userTurnId}:text-${modelRound}`;
  const fixture = [
    { role: 'user', message_id: 'reasoning-user', user_turn_id: userTurnId, content: 'input' },
    { role: 'assistant', message_id: assistantMessageId, user_turn_id: userTurnId,
      model_turn_id: modelTurnId, content: 'output', stream_incomplete: true }
  ];
  syncChatRuntimeProjectionFromSnapshot(chat, sessionId, fixture,
    { immediate: true, authoritative: true, running: true });
  installChatThreadFixture(chat, sessionId, fixture);
  await settle();
  applyThreadTextBlock(chat, sessionId, textItemId, userTurnId, 'output', 'content');
  await settle();
  // The first reasoning chunk after visible content must mount the thinking row.
  let reasoning = 'reasoning '.repeat(12000);
  applyThreadTextBlock(chat, sessionId, textItemId, userTurnId, reasoning, 'reasoning');
  await settle(600);
  // 条目区默认展开由轮次行的「有内容」上报决定；探针直接展开以稳定断言。
  document.querySelector<HTMLElement>('.timeline-group-head')?.click();
  await nextTick();
  await settle(60);
  if (!document.querySelector('.tl-thinking')) throw new Error('Thinking row failed to mount');
  chatPerf.start();
  for (let index = 0; index < 100; index++) {
    reasoning += ` chunk-${index}`;
    applyThreadTextBlock(chat, sessionId, textItemId, userTurnId, reasoning, 'reasoning');
    await settle(5);
  }
  await settle();
  const summary = document.querySelector('.tl-thinking-summary')?.textContent || '';
  document.querySelector<HTMLElement>('.tl-thinking-head')?.click();
  await nextTick();
  await settle(20);
  const body = document.querySelector('.tl-thinking-text')?.textContent || '';
  if (!body.includes('chunk-99')) throw new Error('Reasoning clock failed to update DOM');
  const report = chatPerf.snapshot();
  chatPerf.stop();
  applyThreadChange(chat, sessionId, 'item_upsert', {
    item_id: textItemId, turn_id: userTurnId, kind: 'assistant_message', role: 'assistant',
    model_round: modelRound, status: 'completed', visibility: 'user', revision: 3, content: 'output'
  });
  applyThreadChange(chat, sessionId, 'turn_upsert', {
    turn_id: userTurnId, root_turn_id: userTurnId, user_round: 1, status: 'completed', content: 'input'
  });
  await settle();
  await nextTick();
  return { previewLength: summary.length,
    bodyContainsLastChunk: body.includes('chunk-99'),
    panelUpdates: report.summary.rendering.messagePanelUpdates,
    contentFlushes: report.durations.chat_stream_plain_text_flush?.count || 0,
    busy: chat.isSessionBusy(sessionId) };
};
