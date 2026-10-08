<template>
  <div data-testid="messenger-view-performance-harness">
    <MessengerView />
    <pre data-testid="messenger-view-performance-state" class="messenger-view-performance-state">{{ snapshot }}</pre>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue';

import MessengerView from '@/views/MessengerView.vue';
import { runChatWorkerProbe } from './chatWorkerProbe';
import { runMessengerTwoTurnProbe } from './messengerTwoTurnProbe';
import { runMessengerReasoningProbe } from './messengerReasoningProbe';
import { installToolResultsProbe } from './toolResultsProbe';
import { createMessengerToolRetryProbe } from './messengerToolRetryProbe';
import { enableWorkflowHistoryFixture, readWorkflowHistoryFixture } from './messengerWorkflowHistoryFixture';
import { useAgentStore } from '@/stores/agents';
import { useChatStore } from '@/stores/chat';
import { useSessionHubStore } from '@/stores/sessionHub';
import { cacheSessionMessages, markSessionDetailWarm, syncChatRuntimeProjectionFromSnapshot } from '@/stores/chatRuntimeState';
import { resetChatThreadRuntime } from '@/realtime/chat/chatThreadRuntime';
import {
  applyThreadChange,
  applyThreadFrame,
  applyThreadTail,
  applyThreadTextBlock,
  installChatThreadFixture,
  resolveRenderedThreadTextItem
} from './messengerThreadFixture';

type HarnessMetrics = {
  firstInteractiveMs: number;
  maxFrameGapMs: number;
  domNodeCount: number;
  mountedMessageCount: number;
  expandedToolCount: number;
  maxExpandedToolCount: number;
  availableToolSummaryCount: number;
  initialToolSummaryCount: number;
  earlierToolSummaryCount: number;
  requestCount: number;
  heapBytes: number | null;
  historyBackfillCount: number;
  streamedCharacters: number;
  toolStreamFrameGapMs: number;
  composerInputLatencyMs: number;
  toolStreamUpdates: number;
  streamingWorkflowShellVisible: boolean;
};

const SESSION_A = 'perf-session-a';
const SESSION_B = 'perf-session-b';
const AGENT_ID = 'perf-agent';
// Keep unrelated bootstrap requests from replacing the render fixture with empty API mocks.
const fixtureSessions = [
  { id: SESSION_A, agent_id: AGENT_ID, title: 'Session A', updated_at: '2026-01-02T00:00:00Z' },
  { id: SESSION_B, agent_id: AGENT_ID, title: 'Session B', updated_at: '2026-01-01T00:00:00Z' }
];
const fixtureAgent = { id: AGENT_ID, name: 'Performance Agent', display_name: 'Performance Agent' };
const fixtureChat = useChatStore();
const fixtureAgents = useAgentStore();
const originalLoadSessions = fixtureChat.loadSessions;
const originalLoadAgents = fixtureAgents.loadAgents;
fixtureChat.loadSessions = async () => { fixtureChat.sessions = fixtureSessions; return fixtureSessions; };
fixtureAgents.loadAgents = async () => {
  fixtureAgents.agents = [fixtureAgent];
  fixtureAgents.agentMap = { [AGENT_ID]: fixtureAgent };
  return { owned: [fixtureAgent], shared: [] };
};
const metrics = ref<HarnessMetrics>({
  firstInteractiveMs: 0,
  maxFrameGapMs: 0,
  domNodeCount: 0,
  mountedMessageCount: 0,
  expandedToolCount: 0,
  maxExpandedToolCount: 0,
  availableToolSummaryCount: 0,
  initialToolSummaryCount: 0,
  earlierToolSummaryCount: 0,
  requestCount: 0,
  heapBytes: null
  ,historyBackfillCount: 0
  ,streamedCharacters: 0
  ,toolStreamFrameGapMs: 0
  ,composerInputLatencyMs: 0
  ,toolStreamUpdates: 0
  ,streamingWorkflowShellVisible: false
});
let requestCount = 0;
const originalFetch = window.fetch.bind(window);

const buildWorkflowItems = (sessionId: string, messageIndex: number, count: number) =>
  Array.from({ length: count }, (_, toolIndex) => ({
    id: `${sessionId}-tool-${messageIndex}-${toolIndex}`,
    eventType: 'tool_result',
    toolName: 'read_file',
    toolCallId: `${sessionId}-tool-${messageIndex}-${toolIndex}`,
    title: `Tool ${toolIndex}`,
    status: 'completed',
    detail: 'Bounded tool detail output.',
    toolCallRawDetail: JSON.stringify({ args: { item: toolIndex } })
  }));

const buildMessages = (sessionId: string, count: number) =>
  Array.from({ length: count }, (_, index) => {
    const assistant = index % 2 === 1;
    return {
      id: `${sessionId}-message-${index}`,
      message_id: `${sessionId}-message-${index}`,
      created_seq: index + 1,
      user_turn_id: `user-turn:${sessionId}:round:${Math.floor(index / 2) + 1}`,
      model_turn_id: assistant ? `model-turn:${sessionId}:user:${Math.floor(index / 2) + 1}:model:1` : undefined,
      turn_index: index + 1,
      role: assistant ? 'assistant' : 'user',
      content: assistant
        ? `## Message ${index}\n\n${'Long markdown paragraph for viewport performance. '.repeat(18)}\n\n| key | value |\n| --- | --- |\n| index | ${index} |`
        : `User message ${index}`,
      created_at: new Date(Date.UTC(2026, 0, 1, 0, 0, index)).toISOString(),
      workflowItems: assistant && (index >= count - 8 || index % 20 === 19)
        ? buildWorkflowItems(sessionId, index, 5)
        : []
    };
  });

const installSession = async (sessionId: string, count = 320) => {
  const chat = useChatStore();
  const hub = useSessionHubStore();
  chat.activeSessionId = sessionId;
  chat.draftAgentId = AGENT_ID;
  chat.sessions = fixtureSessions;
  const messages = readWorkflowHistoryFixture(sessionId) || buildMessages(sessionId, count);
  chat.messages = messages;
  cacheSessionMessages(sessionId, messages);
  markSessionDetailWarm(sessionId);
  syncChatRuntimeProjectionFromSnapshot(chat, sessionId, messages, {
    immediate: true,
    authoritative: true
  });
  // The rendered page reads the durable thread state, so the fixture must enter
  // through the same reload snapshot the session-open path uses.
  installChatThreadFixture(chat, sessionId, messages);
  hub.setSection('messages');
  hub.setActiveConversation({ kind: 'agent', id: sessionId, agentId: AGENT_ID });
  await nextTick();
  await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
  collectMetrics();
};

const collectMetrics = () => {
  const memory = performance as Performance & { memory?: { usedJSHeapSize?: number } };
  const expandedToolCount = document.querySelectorAll('.tl-entry-body').length;
  metrics.value = {
    ...metrics.value,
    domNodeCount: document.querySelectorAll('*').length,
    mountedMessageCount: document.querySelectorAll('.messenger-message').length,
    expandedToolCount,
    maxExpandedToolCount: Math.max(metrics.value.maxExpandedToolCount, expandedToolCount),
    availableToolSummaryCount: document.querySelectorAll('.tl-entry').length,
    requestCount,
    heapBytes: Number.isFinite(Number(memory.memory?.usedJSHeapSize))
      ? Number(memory.memory?.usedJSHeapSize)
      : null
  };
};

const runScrollProbe = async () => {
  const list = document.querySelector<HTMLElement>('[data-testid="messenger-message-list"]');
  if (!list) return;
  const gaps: number[] = [];
  let previous = performance.now();
  const maxTop = Math.max(0, list.scrollHeight - list.clientHeight);
  for (let index = 0; index <= 30; index += 1) {
    list.scrollTop = index % 2 === 0 ? maxTop : Math.round(maxTop * (index / 30));
    list.dispatchEvent(new Event('scroll'));
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    const now = performance.now();
    gaps.push(now - previous);
    previous = now;
  }
  metrics.value.maxFrameGapMs = Math.max(...gaps, 0);
  collectMetrics();
};

const prependHistory = async () => {
  const chat = useChatStore();
  const older = buildMessages('older-page', 40).map((message, index) => ({
    ...message,
    id: `older-${index}`,
    message_id: `older-${index}`,
    created_seq: index + 1
  }));
  const current = Array.isArray(chat.messages) ? chat.messages : [];
  const merged = [...older, ...current];
  chat.messages = merged;
  syncChatRuntimeProjectionFromSnapshot(chat, SESSION_A, merged, { immediate: true });
  // Backfill arrives as a longer durable snapshot, exactly like a reload that
  // now knows more history; the rendered rows are rebuilt from thread state.
  installChatThreadFixture(chat, SESSION_A, merged);
  metrics.value.historyBackfillCount += older.length;
  await nextTick();
  collectMetrics();
};

const scrollMessageListToBottom = async () => {
  const list = document.querySelector<HTMLElement>('[data-testid="messenger-message-list"]');
  if (!list) return;
  list.scrollTop = list.scrollHeight;
  list.dispatchEvent(new Event('scroll'));
  await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
};

const streamLatestMessage = async () => {
  const chat = useChatStore();
  const target = resolveRenderedThreadTextItem(SESSION_A);
  if (!target) throw new Error('Missing assistant projection');
  // Live output belongs to a running turn; a settled turn is not a stream target.
  applyThreadChange(chat, SESSION_A, 'turn_upsert', {
    turn_id: target.turnId, root_turn_id: target.turnId, status: 'running'
  });
  applyThreadChange(chat, SESSION_A, 'item_upsert', {
    ...target.item, item_id: target.itemId, turn_id: target.turnId, kind: 'assistant_message',
    role: 'assistant', status: 'running', visibility: 'user',
    revision: Number(target.item.revision || 2) + 1, content: target.content
  });
  await scrollMessageListToBottom();
  for (let index = 0; index < 48; index += 1) {
    applyThreadTail(chat, SESSION_A, target.itemId, ` stream-${index}`);
    metrics.value.streamedCharacters = resolveRenderedThreadTextItem(SESSION_A)?.content.length || 0;
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  }
  await new Promise<void>((resolve) => setTimeout(resolve, 1200));
  await scrollMessageListToBottom();
  if (!document.querySelector('[data-testid="messenger-message-list"]')?.textContent?.includes('stream-47')) {
    throw new Error('Canonical stream did not reach the rendered message');
  }
  // Durable settle: the block snapshot heals the ephemeral tail.
  applyThreadTextBlock(chat, SESSION_A, target.itemId, target.turnId,
    resolveRenderedThreadTextItem(SESSION_A)?.content || '');
  applyThreadChange(chat, SESSION_A, 'item_upsert', {
    ...target.item, item_id: target.itemId, turn_id: target.turnId, kind: 'assistant_message',
    role: 'assistant', status: 'completed', visibility: 'user',
    revision: Number(target.item.revision || 2) + 2,
    content: resolveRenderedThreadTextItem(SESSION_A)?.content || ''
  });
  applyThreadChange(chat, SESSION_A, 'turn_upsert', {
    turn_id: target.turnId, root_turn_id: target.turnId, status: 'completed'
  });
  await nextTick();
  collectMetrics();
};

const streamToolOutputWhileTyping = async () => {
  const chat = useChatStore();
  const target = resolveRenderedThreadTextItem(SESSION_A);
  if (!target) return;
  const liveItemId = `${target.turnId}:live-tool`;
  const workflowItems = buildWorkflowItems(SESSION_A, 319, 50);
  const seeded = workflowItems[workflowItems.length - 1];
  // Seed the durable tool row, then stream its output as item revisions: the
  // only path that reaches the rendered timeline.
  applyThreadChange(chat, SESSION_A, 'item_upsert', {
    ...seeded, item_id: liveItemId, turn_id: target.turnId, kind: 'tool_call',
    visibility: 'user', status: 'loading', revision: 1, event_type: 'tool_result'
  });
  await nextTick();
  metrics.value.streamingWorkflowShellVisible = Boolean(
    document.querySelector(`[data-virtual-key="runtime:assistant:tturn:${target.turnId}:assistant"] .timeline-group`)
  );

  const input = document.querySelector<HTMLTextAreaElement>('.messenger-agent-composer textarea');
  const frameGaps: number[] = [];
  let previousFrameAt = performance.now();
  const inputStartedAt = performance.now();
  for (let index = 0; index < 24; index += 1) {
    const detail = `Live output ${index}: ${'x'.repeat(384)}`;
    applyThreadChange(chat, SESSION_A, 'item_upsert', {
      ...seeded, item_id: liveItemId, turn_id: target.turnId, kind: 'tool_call',
      visibility: 'user', status: 'loading', revision: index + 2,
      event_type: 'tool_result', detail, updated_seq: index + 1
    });
    if (input) {
      input.value = `typing-${index}`;
      input.dispatchEvent(new Event('input', { bubbles: true }));
    }
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    const now = performance.now();
    frameGaps.push(now - previousFrameAt);
    previousFrameAt = now;
  }
  await nextTick();
  metrics.value.toolStreamFrameGapMs = Math.max(...frameGaps, 0);
  metrics.value.composerInputLatencyMs = performance.now() - inputStartedAt;
  metrics.value.toolStreamUpdates = 24;
  applyThreadChange(chat, SESSION_A, 'item_upsert', {
    ...seeded, item_id: liveItemId, turn_id: target.turnId, kind: 'tool_call',
    visibility: 'user', status: 'completed', revision: 26,
    event_type: 'tool_result', detail: `Live output 23: ${'x'.repeat(384)}`
  });
  collectMetrics();
};

const expandToolDetails = async () => {
  const list = document.querySelector<HTMLElement>('[data-testid="messenger-message-list"]');
  if (list) {
    list.scrollTop = list.scrollHeight;
    list.dispatchEvent(new Event('scroll'));
    await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
  }
  const group = Array.from(document.querySelectorAll<HTMLElement>('.timeline-group')).at(-1);
  if (group && !group.classList.contains('is-open')) {
    group.querySelector<HTMLElement>('.timeline-group-head')?.click();
    await new Promise(resolve => setTimeout(resolve, 50));
  }
  const heads = Array.from(group?.querySelectorAll<HTMLElement>('.tl-entry-head') || []).slice(0, 5);
  heads.forEach((head) => head.click());
  await nextTick();
  collectMetrics();
};

const showEarlierToolEntries = async () => {
  const chat = useChatStore();
  const target = resolveRenderedThreadTextItem(SESSION_A);
  if (target) {
    const seeded = buildWorkflowItems(SESSION_A, 319, 260);
    seeded.forEach((item, index) => {
      applyThreadChange(chat, SESSION_A, 'item_upsert', {
        ...item, item_id: `${target.turnId}:item-${index}`, turn_id: target.turnId,
        kind: 'tool_call', visibility: 'user', status: 'completed',
        revision: index + 1, event_type: 'tool_result'
      });
    });
    await nextTick();
  }
  const group = Array.from(document.querySelectorAll<HTMLElement>('.timeline-group')).at(-1);
  if (group && !group.classList.contains('is-open')) {
    group.querySelector<HTMLElement>('.timeline-group-head')?.click();
    await nextTick();
  }
  metrics.value.initialToolSummaryCount = group?.querySelectorAll('.tl-entry').length || 0;
  group?.querySelector<HTMLElement>('.timeline-group-more')?.click();
  await nextTick();
  metrics.value.earlierToolSummaryCount = group?.querySelectorAll('.tl-entry').length || 0;
  collectMetrics();
};

const switchSessionAndReturn = async () => {
  await installSession(SESSION_B, 120);
  await installSession(SESSION_A, 320);
};

const nextFrame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));

/**
 * Browser-level durable pipeline probe. It deliberately enters through the
 * production frame adapter and renders MessengerView, while simulating a
 * dropped live tail followed by the durable block used on reconnect.
 */
const runDurableStreamProbe = async () => {
  resetChatThreadRuntime(SESSION_A);
  const chat = useChatStore();
  const turnId = 'durable-probe-turn';
  const itemId = `${turnId}:text-1`;
  const apply = (event: string, data: Record<string, unknown>) => {
    const accepted = applyThreadFrame(chat, SESSION_A, event, data);
    if (!accepted) throw new Error(`durable frame was not consumed: ${event}`);
  };
  apply('thread_change', {
    cursor: 1, change_type: 'turn_upsert', turn_id: turnId,
    payload: { turn_id: turnId, user_round: 1001, status: 'running', content: 'probe input' }
  });
  apply('thread_change', {
    cursor: 2, change_type: 'item_upsert', turn_id: turnId, item_id: `${turnId}:user`, revision: 1,
    payload: {
      item_id: `${turnId}:user`, turn_id: turnId, kind: 'user_message', status: 'completed',
      visibility: 'user', revision: 1, payload: { role: 'user', content: 'probe input', user_round: 1001 }
    }
  });
  apply('thread_change', {
    cursor: 3, change_type: 'item_upsert', turn_id: turnId, item_id: itemId, revision: 1,
    payload: {
      item_id: itemId, turn_id: turnId, kind: 'assistant_message', status: 'running',
      visibility: 'user', revision: 1,
      payload: { role: 'assistant', content: '', reasoning: '', user_round: 1001, model_round: 1 }
    }
  });
  await nextFrame();
  // Let initial MessengerView work, module hydration and virtual-list layout
  // settle before measuring streaming work.
  await new Promise<void>((resolve) => setTimeout(resolve, 250));

  const samples: number[] = [];
  const longTasks: number[] = [];
  const longTaskDetails: Array<{ startTime: number; duration: number; name: string }> = [];
  const observer = typeof PerformanceObserver === 'undefined' ? null : new PerformanceObserver((list) => {
    for (const entry of list.getEntries()) {
      longTasks.push(entry.duration);
      if (longTaskDetails.length < 100) longTaskDetails.push({ startTime: entry.startTime, duration: entry.duration, name: entry.name });
    }
  });
  try { observer?.observe({ type: 'longtask' } as PerformanceObserverInit); } catch { /* optional API */ }
  const list = document.querySelector<HTMLElement>('[data-testid="messenger-message-list"]');
  const composer = document.querySelector<HTMLTextAreaElement>('.messenger-agent-composer textarea');
  let copied = false;
  const onCopy = () => { copied = true; };
  document.addEventListener('copy', onCopy);
  const chunks: string[] = [];
  try {
    for (let index = 0; index < 1000; index += 10) {
      const startedAt = performance.now();
      for (let part = 0; part < 10; part += 1) {
        const token = index + part;
        const text = ` ${token % 10}`;
        chunks.push(text);
        apply('thread_item_tail', { item_id: itemId, field: 'content', offset: token * 2, base_seq: 3, text });
      }
      if (composer && index % 100 === 0) {
        composer.value = `typing-${index}`;
        composer.dispatchEvent(new Event('input', { bubbles: true }));
      }
      if (list && index % 100 === 0) {
        list.scrollTop = list.scrollHeight;
        list.dispatchEvent(new Event('scroll'));
      }
      if (index === 500) {
        const selection = window.getSelection();
        selection?.removeAllRanges();
        const range = document.createRange();
        range.selectNodeContents(document.body);
        selection?.addRange(range);
        document.dispatchEvent(new Event('copy', { bubbles: true }));
        selection?.removeAllRanges();
      }
      await nextFrame();
      samples.push(performance.now() - startedAt);
    }
    const recoveryStartedAt = performance.now();
    // The active connection is considered lost here. Its unsent tail is
    // healed by the next durable block delivered by the resumed watch.
    apply('thread_change', {
      cursor: 4, change_type: 'text_block', turn_id: turnId, item_id: itemId,
      payload: { item_id: itemId, field: 'content', block_index: 0, content_offset: 0, content: chunks.join('') }
    });
    await nextTick();
    await nextFrame();
    await nextFrame();
    const ordered = [...samples].sort((left, right) => left - right);
    const p95 = ordered[Math.max(0, Math.ceil(ordered.length * 0.95) - 1)] || 0;
    // Content clocks and Vue paint can land on different frames. Measure actual
    // DOM recovery, bounded by the same one-second acceptance threshold.
    const isRendered = () => document.body.textContent?.includes('probe input') === true
      && document.body.textContent?.includes(chunks.join('').trim()) === true;
    while (!isRendered() && performance.now() - recoveryStartedAt < 1000) await nextFrame();
    const rendered = isRendered();
    return {
      tokens: chunks.length,
      p95FrameLatencyMs: p95,
      maxFrameLatencyMs: Math.max(...samples, 0),
      reconnectRecoveryMs: performance.now() - recoveryStartedAt,
      longTasksOver50Ms: longTasks.filter((duration) => duration > 50).length,
      longTaskDetails,
      inputApplied: composer?.value === 'typing-900',
      scrollApplied: Boolean(list && list.scrollTop >= 0),
      copyObserved: copied,
      rendered
    };
  } finally {
    document.removeEventListener('copy', onCopy);
    observer?.disconnect();
  }
};

const snapshot = computed(() => JSON.stringify(metrics.value, null, 2));

onMounted(async () => {
  const startedAt = performance.now();
  window.fetch = (async (...args: Parameters<typeof fetch>) => {
    requestCount += 1;
    return originalFetch(...args);
  }) as typeof fetch;
  const agents = useAgentStore();
  const agent = fixtureAgent;
  agents.agents = [agent];
  agents.agentMap = { [AGENT_ID]: agent };
  await installSession(SESSION_A);
  metrics.value.firstInteractiveMs = performance.now() - startedAt;
  collectMetrics();
  (window as Window & { __messengerViewPerformanceE2E?: unknown }).__messengerViewPerformanceE2E = {
    runChatWorkerProbe,
    installToolResultsProbe,
    installSession,
    installWorkflowHistory: async () => { enableWorkflowHistoryFixture(); await installSession(SESSION_A); },
    runTwoTurnProbe: () => runMessengerTwoTurnProbe(SESSION_A),
    runReasoningProbe: () => runMessengerReasoningProbe(SESSION_A),
    toolRetry: createMessengerToolRetryProbe(SESSION_A),
    setSection: (section: 'messages' | 'more') => useSessionHubStore().setSection(section),
    streamInBackground: async () => {
      const chat = useChatStore();
      const target = resolveRenderedThreadTextItem(SESSION_A);
      if (!target) throw new Error('Missing assistant');
      // A live turn keeps streaming while the chat view is unmounted.
      applyThreadChange(chat, SESSION_A, 'turn_upsert', {
        turn_id: target.turnId, root_turn_id: target.turnId, status: 'running'
      });
      applyThreadChange(chat, SESSION_A, 'item_upsert', {
        ...target.item, item_id: target.itemId, turn_id: target.turnId, kind: 'assistant_message',
        role: 'assistant', status: 'running', visibility: 'user',
        revision: Number(target.item.revision || 2) + 1, content: target.content
      });
      for (let index = 0; index < 24; index++) {
        applyThreadTail(chat, SESSION_A, target.itemId, ` background-${index}`);
        await new Promise<void>(resolve => requestAnimationFrame(() => resolve()));
        if (document.querySelector('.messenger-message-panel')) throw new Error('Hidden chat remounted');
      }
      return chat.isSessionBusy(SESSION_A);
    },
    runScrollProbe,
    prependHistory,
    streamLatestMessage,
    streamToolOutputWhileTyping,
    expandToolDetails,
    showEarlierToolEntries,
    switchSessionAndReturn,
    runDurableStreamProbe,
    collectMetrics
  };
});

onBeforeUnmount(() => {
  fixtureChat.loadSessions = originalLoadSessions;
  fixtureAgents.loadAgents = originalLoadAgents;
  window.fetch = originalFetch;
  delete (window as Window & { __messengerViewPerformanceE2E?: unknown }).__messengerViewPerformanceE2E;
});
</script>

<style scoped>
.messenger-view-performance-state {
  position: fixed;
  right: 8px;
  bottom: 8px;
  z-index: 9999;
  max-width: 260px;
  max-height: 180px;
  overflow: auto;
  padding: 8px;
  font-size: 10px;
  pointer-events: none;
  opacity: 0.08;
}
</style>
