import { nextTick } from 'vue';
import { useChatStore } from '@/stores/chat';
import { getChatThreadState } from '@/realtime/chat/chatThreadRuntime';
import {
  applyThreadChange,
  applyThreadTextBlock,
  installChatThreadFixture
} from './messengerThreadFixture';

const frame = () => new Promise<void>(resolve => requestAnimationFrame(() => resolve()));

const WORKFLOW_ITEMS_PER_TURN = 80;

const buildTurnFixture = (sessionId: string, turn: number) => {
  const userTurnId = `user-turn:${sessionId}:round:${200 + turn}`;
  const modelTurnId = `model-turn:${sessionId}:user:${200 + turn}:model:1`;
  return {
    userTurnId,
    userRound: 200 + turn,
    textItemId: `${userTurnId}:text-1`,
    user: { role: 'user', message_id: `user:${userTurnId}`, user_turn_id: userTurnId, content: `input-${turn}` },
    assistant: {
      role: 'assistant',
      message_id: `assistant:${userTurnId}`,
      user_turn_id: userTurnId,
      model_turn_id: modelTurnId,
      content: '',
      stream_incomplete: true,
      workflowItems: Array.from({ length: WORKFLOW_ITEMS_PER_TURN }, (_, tool) => ({
        id: `call-${turn}-${tool}`,
        eventType: 'tool_result',
        toolName: 'read_file',
        toolCallId: `call-${turn}-${tool}`,
        title: `Tool ${tool}`,
        status: 'completed',
        detail: 'x'.repeat(512),
        toolCallRawDetail: JSON.stringify({ args: { item: tool } })
      }))
    }
  };
};

/**
 * 两个工具密集轮次的渲染/流式压力探针。
 *
 * 走 durable thread 运行时（页面唯一渲染源）：fixture 经
 * `installChatThreadFixture` 装载，正文用 `thread_item_tail` 推增量，轮次终态用
 * `item_upsert` + `turn_upsert` 落库——与线上 send/watch 同一套帧。
 */
export const runMessengerTwoTurnProbe = async (sessionId: string) => {
  const chat = useChatStore();
  const gaps: number[] = [];
  let previous = performance.now();
  let maxNodes = 0;
  let peakNodeBreakdown: unknown = null;
  const sample = async () => {
    await frame();
    const now = performance.now();
    gaps.push(now - previous);
    previous = now;
    const nodes = document.querySelectorAll('*').length;
    if (nodes > maxNodes) {
      maxNodes = nodes;
      peakNodeBreakdown = {
        messages: document.querySelectorAll('.messenger-message').length,
        toolEntries: document.querySelectorAll('.tl-entry').length,
        messageNodes: document.querySelectorAll('.messenger-message-panel *').length,
        viewport: document.querySelector('[data-testid="messenger-message-list"]')?.clientHeight,
        heights: [...document.querySelectorAll<HTMLElement>('.messenger-message')].slice(0, 10).map(node => node.offsetHeight)
      };
    }
  };
  const turns = [1, 2].map((turn) => buildTurnFixture(sessionId, turn));
  // Turn 1 only: turn 2 is accepted later, so "earlier workflows retained" is
  // still a real observation rather than a fixture that was there all along.
  installChatThreadFixture(chat, sessionId, [turns[0].user, turns[0].assistant]);
  await nextTick();
  const toolItemsOfTurn = (userTurnId: string): number => {
    const state = getChatThreadState(sessionId);
    if (!state) return 0;
    let count = 0;
    state.items.forEach((item) => {
      if (item.turnId === userTurnId && item.kind === 'tool_call') count += 1;
    });
    return count;
  };
  const retainedWorkflowCounts: number[] = [];
  for (const turn of turns) {
    if (turn !== turns[0]) {
      // Accept the next user turn through the durable frames the server sends.
      applyThreadChange(chat, sessionId, 'turn_upsert', {
        turn_id: turn.userTurnId, root_turn_id: turn.userTurnId, user_round: turn.userRound,
        status: 'running', content: String(turn.user.content)
      });
      applyThreadChange(chat, sessionId, 'item_upsert', {
        item_id: `${turn.userTurnId}:user`, turn_id: turn.userTurnId, kind: 'user_message',
        role: 'user', status: 'completed', visibility: 'user', revision: 1,
        user_round: turn.userRound, content: String(turn.user.content)
      });
      applyThreadChange(chat, sessionId, 'item_upsert', {
        item_id: turn.textItemId, turn_id: turn.userTurnId, kind: 'assistant_message',
        role: 'assistant', model_round: 1, status: 'running', visibility: 'user', revision: 1, content: ''
      });
      (turn.assistant.workflowItems || []).forEach((item, index) => {
        applyThreadChange(chat, sessionId, 'item_upsert', {
          ...item, item_id: `${turn.userTurnId}:item-${index}`, turn_id: turn.userTurnId,
          kind: 'tool_call', status: 'completed', visibility: 'user',
          revision: index + 1, model_round: 1, event_type: 'tool_result'
        });
      });
      await nextTick();
    }
    if (!chat.isSessionBusy(sessionId)) throw new Error('Tool round lost running status');
    const list = document.querySelector<HTMLElement>('[data-testid="messenger-message-list"]');
    await nextTick();
    if (list) { list.scrollTop = list.scrollHeight; list.dispatchEvent(new Event('scroll')); }
    let workflow: Element | null = null;
    for (let attempt = 0; attempt < 12; attempt++) {
      workflow = document.querySelector(
        `[data-virtual-key="runtime:assistant:tturn:${turn.userTurnId}:assistant"] .timeline-group`
      );
      if (workflow) break;
      await frame();
    }
    if (!workflow) throw new Error('Active workflow shell is missing');
    const seed = 'stream-text '.repeat(5600);
    const marker = `tail-${turn.userTurnId.slice(-1)}-`;
    // Durable block snapshots drive the rendered body (the ephemeral tail is an
    // optimisation healed by the next block).
    let streamed = seed;
    for (let chunk = 0; chunk < 24; chunk++) {
      streamed = `${streamed} ${marker}${chunk}`;
      applyThreadTextBlock(chat, sessionId, turn.textItemId, turn.userTurnId, streamed);
      const input = document.querySelector<HTMLTextAreaElement>('.messenger-agent-composer textarea');
      if (input) { input.value = `draft-${chunk}`; input.dispatchEvent(new Event('input', { bubbles: true })); }
      await sample();
    }
    await new Promise(resolve => setTimeout(resolve, 160));
    if (!list?.textContent?.includes(`${marker}23`)) throw new Error('Long stream failed to reach DOM');
    applyThreadChange(chat, sessionId, 'item_upsert', {
      item_id: turn.textItemId, turn_id: turn.userTurnId, kind: 'assistant_message', role: 'assistant',
      model_round: 1, status: 'completed', visibility: 'user', revision: 3,
      content: streamed
    });
    applyThreadChange(chat, sessionId, 'turn_upsert', {
      turn_id: turn.userTurnId, root_turn_id: turn.userTurnId, user_round: turn.userRound,
      status: 'completed', content: String(turn.user.content)
    });
    await nextTick();
    // Earlier turns keep their durable workflow rows while a later turn streams.
    retainedWorkflowCounts.push(turns.filter((entry) =>
      toolItemsOfTurn(entry.userTurnId) >= WORKFLOW_ITEMS_PER_TURN).length);
    previous = performance.now();
    for (let index = 0; index < 12; index++) {
      if (list) {
        list.scrollTop = index % 2 ? list.scrollHeight : list.scrollHeight / 2;
        list.dispatchEvent(new Event('scroll'));
      }
      await sample();
    }
  }
  return {
    turns: turns.length,
    toolCalls: turns.length * WORKFLOW_ITEMS_PER_TURN,
    maxFrameGapMs: Math.max(...gaps),
    maxNodes,
    retainedWorkflowCounts,
    peakNodeBreakdown
  };
};
