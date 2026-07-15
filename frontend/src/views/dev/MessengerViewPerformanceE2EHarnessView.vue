<template>
  <div data-testid="messenger-view-performance-harness">
    <MessengerView />
    <pre data-testid="messenger-view-performance-state" class="messenger-view-performance-state">{{ snapshot }}</pre>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue';

import MessengerView from '@/views/MessengerView.vue';
import { useAgentStore } from '@/stores/agents';
import { useChatStore } from '@/stores/chat';
import { useSessionHubStore } from '@/stores/sessionHub';
import { syncChatRuntimeProjectionFromSnapshot } from '@/stores/chatRuntimeState';

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
    detail: 'Bounded tool detail output.'
  }));

const buildMessages = (sessionId: string, count: number) =>
  Array.from({ length: count }, (_, index) => {
    const assistant = index % 2 === 1;
    return {
      id: `${sessionId}-message-${index}`,
      message_id: `${sessionId}-message-${index}`,
      history_id: index + 1,
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
  chat.sessions = [
    { id: SESSION_A, agent_id: AGENT_ID, title: 'Session A', updated_at: '2026-01-02T00:00:00Z' },
    { id: SESSION_B, agent_id: AGENT_ID, title: 'Session B', updated_at: '2026-01-01T00:00:00Z' }
  ];
  const messages = buildMessages(sessionId, count);
  chat.messages = messages;
  syncChatRuntimeProjectionFromSnapshot(chat, sessionId, messages, {
    immediate: true,
    authoritative: true
  });
  hub.setSection('messages');
  hub.setActiveConversation({ kind: 'agent', id: sessionId, agentId: AGENT_ID });
  await nextTick();
  await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
  collectMetrics();
};

const collectMetrics = () => {
  const memory = performance as Performance & { memory?: { usedJSHeapSize?: number } };
  const expandedToolCount = document.querySelectorAll('.tool-workflow-entry-body').length;
  metrics.value = {
    ...metrics.value,
    domNodeCount: document.querySelectorAll('*').length,
    mountedMessageCount: document.querySelectorAll('.messenger-message').length,
    expandedToolCount,
    maxExpandedToolCount: Math.max(metrics.value.maxExpandedToolCount, expandedToolCount),
    availableToolSummaryCount: document.querySelectorAll('.tool-workflow-entry-summary').length,
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
    history_id: index + 1
  }));
  const current = Array.isArray(chat.messages) ? chat.messages : [];
  chat.messages = [...older, ...current];
  syncChatRuntimeProjectionFromSnapshot(chat, SESSION_A, chat.messages, { immediate: true });
  metrics.value.historyBackfillCount += older.length;
  await nextTick();
  collectMetrics();
};

const streamLatestMessage = async () => {
  const chat = useChatStore();
  const messages = Array.isArray(chat.messages) ? chat.messages : [];
  const latest = messages[messages.length - 1] as Record<string, unknown> | undefined;
  if (!latest) return;
  latest.stream_incomplete = true;
  for (let index = 0; index < 12; index += 1) {
    latest.content = `${String(latest.content || '')} stream-${index}`;
    metrics.value.streamedCharacters = String(latest.content).length;
    chat.runtimeProjectionContentVersion += 1;
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  }
  latest.stream_incomplete = false;
  syncChatRuntimeProjectionFromSnapshot(chat, SESSION_A, messages, { immediate: true, running: false });
  collectMetrics();
};

const streamToolOutputWhileTyping = async () => {
  const chat = useChatStore();
  const session = chat.runtimeProjection?.sessions?.[SESSION_A];
  const latestAssistantId = [...(session?.messages || [])]
    .reverse()
    .find((messageId) => session?.messageById?.[messageId]?.role === 'assistant');
  const latestAssistant = latestAssistantId ? session?.messageById?.[latestAssistantId] : null;
  if (!latestAssistant) return;
  latestAssistant.workflowItems = buildWorkflowItems(SESSION_A, 319, 50);
  const liveItem = latestAssistant.workflowItems[latestAssistant.workflowItems.length - 1];
  if (!liveItem) return;
  latestAssistant.status = 'tooling';
  liveItem.status = 'loading';
  latestAssistant.structureVersion = Number(latestAssistant.structureVersion || 0) + 1;
  chat.runtimeProjectionVersion += 1;
  await nextTick();
  metrics.value.streamingWorkflowShellVisible = Boolean(
    document.querySelector('.message-tool-workflow')?.querySelector('.tool-workflow-entry-summary')
  );

  const input = document.querySelector<HTMLTextAreaElement>('.messenger-agent-composer textarea');
  const frameGaps: number[] = [];
  let previousFrameAt = performance.now();
  const inputStartedAt = performance.now();
  for (let index = 0; index < 24; index += 1) {
    liveItem.detail = `Live output ${index}: ${'x'.repeat(384)}`;
    liveItem.updatedSeq = index + 1;
    chat.runtimeProjectionContentVersion += 1;
    chat.runtimeProjectionContentVersionByMessage[latestAssistant.id] =
      Number(chat.runtimeProjectionContentVersionByMessage[latestAssistant.id] || 0) + 1;
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
  latestAssistant.status = 'streaming';
  liveItem.status = 'completed';
  chat.runtimeProjectionVersion += 1;
  collectMetrics();
};

const expandToolDetails = async () => {
  const list = document.querySelector<HTMLElement>('[data-testid="messenger-message-list"]');
  if (list) {
    list.scrollTop = list.scrollHeight;
    list.dispatchEvent(new Event('scroll'));
    await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
  }
  const summaries = Array.from(document.querySelectorAll<HTMLElement>('.tool-workflow-entry-summary')).slice(0, 5);
  summaries.forEach((summary) => summary.click());
  await nextTick();
  collectMetrics();
};

const showEarlierToolEntries = async () => {
  const chat = useChatStore();
  const session = chat.runtimeProjection?.sessions?.[SESSION_A];
  const latestAssistantId = [...(session?.messages || [])]
    .reverse()
    .find((messageId) => session?.messageById?.[messageId]?.role === 'assistant');
  const latestAssistant = latestAssistantId ? session?.messageById?.[latestAssistantId] : null;
  if (latestAssistant) {
    latestAssistant.workflowItems = buildWorkflowItems(SESSION_A, 319, 260);
    latestAssistant.structureVersion = Number(latestAssistant.structureVersion || 0) + 1;
    chat.runtimeProjectionVersion += 1;
    await nextTick();
  }
  const workflow = Array.from(document.querySelectorAll<HTMLElement>('.message-tool-workflow'))
    .find((node) => Boolean(node.querySelector('.tool-workflow-load-earlier')));
  const summary = workflow?.querySelector<HTMLElement>('summary');
  if (workflow && !workflow.hasAttribute('open')) {
    summary?.click();
    await nextTick();
  }
  const workflowEntries = workflow?.querySelectorAll('.tool-workflow-entry-summary');
  metrics.value.initialToolSummaryCount = workflowEntries?.length || 0;
  workflow?.querySelector<HTMLElement>('.tool-workflow-load-earlier')?.click();
  await nextTick();
  metrics.value.earlierToolSummaryCount = workflow?.querySelectorAll('.tool-workflow-entry-summary').length || 0;
  collectMetrics();
};

const switchSessionAndReturn = async () => {
  await installSession(SESSION_B, 120);
  await installSession(SESSION_A, 320);
};

const snapshot = computed(() => JSON.stringify(metrics.value, null, 2));

onMounted(async () => {
  const startedAt = performance.now();
  window.fetch = (async (...args: Parameters<typeof fetch>) => {
    requestCount += 1;
    return originalFetch(...args);
  }) as typeof fetch;
  const agents = useAgentStore();
  const agent = { id: AGENT_ID, name: 'Performance Agent', display_name: 'Performance Agent' };
  agents.agents = [agent];
  agents.agentMap = { [AGENT_ID]: agent };
  await installSession(SESSION_A);
  metrics.value.firstInteractiveMs = performance.now() - startedAt;
  collectMetrics();
  (window as Window & { __messengerViewPerformanceE2E?: unknown }).__messengerViewPerformanceE2E = {
    installSession,
    runScrollProbe,
    prependHistory,
    streamLatestMessage,
    streamToolOutputWhileTyping,
    expandToolDetails,
    showEarlierToolEntries,
    switchSessionAndReturn,
    collectMetrics
  };
});

onBeforeUnmount(() => {
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
