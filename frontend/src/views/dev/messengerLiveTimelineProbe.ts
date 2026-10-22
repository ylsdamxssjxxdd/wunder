import { nextTick } from 'vue';
import { useChatStore } from '@/stores/chat';
import {
  applyThreadChange,
  applyThreadTail,
  applyThreadCompletedItem,
  installChatThreadFixture
} from './messengerThreadFixture';

const frame = () => new Promise<void>(resolve => requestAnimationFrame(() => resolve()));

type StepReport = {
  step: string;
  hasFold: boolean;
  foldText: string;
  groupCount: number;
  groupTexts: string[];
  bodyTexts: string[];
  toolEntries: number;
  thinkingEntries: number;
};

const capture = async (step: string, turnId: string): Promise<StepReport> => {
  await nextTick();
  await frame();
  await frame();
  const row = document.querySelector(`[data-virtual-key="runtime:assistant:tturn:${turnId}:assistant"]`);
  const fold = row?.querySelector<HTMLElement>('.timeline-process-head') ?? null;
  const groups = Array.from(row?.querySelectorAll<HTMLElement>('.timeline-group') ?? []);
  const bodies = Array.from(row?.querySelectorAll<HTMLElement>('.timeline-body-block') ?? [])
    .map((el) => (el.textContent || '').replace(/\s+/g, ' ').trim().slice(0, 60));
  return {
    step,
    hasFold: Boolean(fold),
    foldText: fold ? (fold.textContent || '').replace(/\s+/g, ' ').trim().slice(0, 120) : '',
    groupCount: groups.length,
    groupTexts: groups.map((el) => (el.textContent || '').replace(/\s+/g, ' ').trim().slice(0, 80)),
    bodyTexts: bodies,
    toolEntries: row?.querySelectorAll('.tl-entry').length ?? 0,
    thinkingEntries: row?.querySelectorAll('.tl-thinking').length ?? 0
  };
};

const scrollToBottom = async () => {
  const list = document.querySelector<HTMLElement>('[data-testid="messenger-message-list"]');
  if (list) {
    list.scrollTop = list.scrollHeight;
    list.dispatchEvent(new Event('scroll'));
  }
  await frame();
  await frame();
};

/**
 * 复现「实时多轮次工具轮」在真实渲染层的表现：
 * 历史一轮（快照装载）→ 新轮次按线上同款帧序实时推进，
 * 每一步抓取助手时间线 DOM，最后与「刷新」（快照重装）对比。
 */
export const runMessengerLiveTimelineProbe = async (sessionId: string) => {
  const chat = useChatStore();
  const historyTurnId = `user-turn:${sessionId}:round:900`;
  const liveTurnId = `user-turn:${sessionId}:round:901`;
  const reports: StepReport[] = [];

  // 1) 历史一轮：快照装载（等价打开会话）
  installChatThreadFixture(chat, sessionId, [
    {
      role: 'user',
      message_id: `user:${historyTurnId}`,
      user_turn_id: historyTurnId,
      user_round: 900,
      content: '历史问题'
    },
    {
      role: 'assistant',
      message_id: `assistant:${historyTurnId}`,
      user_turn_id: historyTurnId,
      model_turn_id: `model-turn:${historyTurnId}:1`,
      model_round: 1,
      content: '历史回答',
      workflowItems: []
    }
  ]);
  await nextTick();
  await scrollToBottom();

  // 2) 新轮次接受（乐观轮次 + ack + turn_upsert）
  chat.activeSessionId = sessionId;
  applyThreadChange(chat, sessionId, 'turn_upsert', {
    turn_id: liveTurnId, root_turn_id: liveTurnId, user_round: 901,
    status: 'running', content: '实时问题'
  });
  applyThreadChange(chat, sessionId, 'item_upsert', {
    item_id: `${liveTurnId}:user`, turn_id: liveTurnId, kind: 'user_message',
    role: 'user', status: 'completed', visibility: 'user', revision: 1,
    user_round: 901, content: '实时问题'
  });
  // llm_request：注册第一轮文本条目（running）
  applyThreadChange(chat, sessionId, 'item_upsert', {
    item_id: `${liveTurnId}:text-1`, turn_id: liveTurnId, kind: 'assistant_message',
    role: 'assistant', model_round: 1, status: 'running', visibility: 'user',
    revision: 1, content: '', reasoning: ''
  });
  await scrollToBottom();
  reports.push(await capture('after-text-1-registered', liveTurnId));

  // 3) 思考增量（thread_item_tail）
  applyThreadTail(chat, sessionId, `${liveTurnId}:text-1`, '先想一下', 'reasoning');
  await frame();
  reports.push(await capture('after-reasoning-tail', liveTurnId));

  // 4) 第一轮正文增量
  applyThreadTail(chat, sessionId, `${liveTurnId}:text-1`, '中间输出正文', 'content');
  await frame();
  reports.push(await capture('after-content-tail-round1', liveTurnId));

  // 5) 工具调用（running）
  applyThreadChange(chat, sessionId, 'item_upsert', {
    item_id: `${liveTurnId}:tool-call-1`, turn_id: liveTurnId, kind: 'tool_call',
    role: 'assistant', model_round: 1, status: 'running', visibility: 'user',
    revision: 1, event_type: 'tool_call', tool_call_id: 'call-1', tool: 'read_file',
    args: { path: 'a.txt' }
  });
  await frame();
  reports.push(await capture('after-tool-call-running', liveTurnId));

  // 6) 工具结果（completed）
  applyThreadChange(chat, sessionId, 'item_upsert', {
    item_id: `${liveTurnId}:tool-call-1`, turn_id: liveTurnId, kind: 'tool_call',
    role: 'assistant', model_round: 1, status: 'completed', visibility: 'user',
    revision: 2, event_type: 'tool_result', tool_call_id: 'call-1', tool: 'read_file',
    ok: true, content: '文件内容'
  });
  await frame();
  reports.push(await capture('after-tool-result', liveTurnId));

  // 7) 第一轮完成项正文落库（item_upsert）
  applyThreadCompletedItem(chat, sessionId, `${liveTurnId}:text-1`, liveTurnId, '中间输出正文', 'content');
  await frame();
  reports.push(await capture('after-text-block-round1', liveTurnId));

  // 8) 第二轮文本条目注册（llm_request round 2）
  applyThreadChange(chat, sessionId, 'item_upsert', {
    item_id: `${liveTurnId}:text-2`, turn_id: liveTurnId, kind: 'assistant_message',
    role: 'assistant', model_round: 2, status: 'running', visibility: 'user',
    revision: 1, content: '', reasoning: ''
  });
  await frame();
  reports.push(await capture('after-text-2-registered', liveTurnId));

  // 9) 最终正文增量
  applyThreadTail(chat, sessionId, `${liveTurnId}:text-2`, '最终回答', 'content');
  await frame();
  reports.push(await capture('after-final-content-tail', liveTurnId));

  // 10) 最终正文落库 + 轮次完成
  applyThreadCompletedItem(chat, sessionId, `${liveTurnId}:text-2`, liveTurnId, '最终回答', 'content');
  applyThreadChange(chat, sessionId, 'turn_upsert', {
    turn_id: liveTurnId, root_turn_id: liveTurnId, user_round: 901, status: 'completed'
  });
  await frame();
  await frame();
  reports.push(await capture('after-turn-completed', liveTurnId));

  // 11) 「刷新」：同数据快照重装（两轮文本 + 工具条目，等价重新打开会话）
  installChatThreadFixture(chat, sessionId, [
    {
      role: 'user',
      message_id: `user:${historyTurnId}`,
      user_turn_id: historyTurnId,
      user_round: 900,
      content: '历史问题'
    },
    {
      role: 'assistant',
      message_id: `assistant:${historyTurnId}`,
      user_turn_id: historyTurnId,
      model_turn_id: `model-turn:${historyTurnId}:1`,
      model_round: 1,
      content: '历史回答',
      workflowItems: []
    },
    {
      role: 'user',
      message_id: `user:${liveTurnId}`,
      user_turn_id: liveTurnId,
      user_round: 901,
      content: '实时问题'
    },
    {
      role: 'assistant',
      message_id: `assistant:${liveTurnId}`,
      user_turn_id: liveTurnId,
      model_turn_id: `model-turn:${liveTurnId}:1`,
      model_round: 1,
      content: '中间输出正文',
      reasoning: '先想一下',
      workflowItems: [
        {
          id: `${liveTurnId}:tool-call-1`,
          eventType: 'tool_result',
          toolName: 'read_file',
          toolCallId: 'call-1',
          title: 'read_file',
          status: 'completed',
          detail: '文件内容',
          toolCallRawDetail: JSON.stringify({ args: { path: 'a.txt' } })
        }
      ]
    },
    {
      role: 'assistant',
      message_id: `assistant:${liveTurnId}:2`,
      user_turn_id: liveTurnId,
      model_turn_id: `model-turn:${liveTurnId}:2`,
      model_round: 2,
      content: '最终回答',
      workflowItems: []
    }
  ]);
  await nextTick();
  await scrollToBottom();
  reports.push(await capture('after-refresh-snapshot', liveTurnId));

  return reports;
};
