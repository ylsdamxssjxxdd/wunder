<template>
  <main class="messenger-heavy-e2e" data-testid="messenger-heavy-history-harness">
    <header class="messenger-heavy-toolbar">
      <button type="button" data-testid="load-400-history" @click="loadHistory(400, 10)">
        Load 400 History
      </button>
      <button type="button" data-testid="load-800-history" @click="loadHistory(800, 10)">
        Load 800 History
      </button>
      <button type="button" data-testid="append-40-history" @click="appendHistory(40, 10)">
        Append 40
      </button>
      <button type="button" data-testid="jump-top" @click="jumpTop()">Jump Top</button>
      <button type="button" data-testid="jump-bottom" @click="jumpBottom()">Jump Bottom</button>
      <button type="button" data-testid="probe-scroll-runtime" @click="runScrollProbe()">
        Probe Scroll
      </button>
      <button type="button" data-testid="reset-heavy-history" @click="resetHarness()">Reset</button>
    </header>

    <section class="messenger-chat chat-shell messenger-heavy-shell">
      <div
        ref="messageListRef"
        class="messenger-chat-body is-messages is-agent messenger-heavy-stream"
        data-testid="messenger-heavy-stream"
        @scroll="handleMessageListScroll"
      >
        <div
          v-for="item in agentRenderableMessages"
          :key="item.key"
          class="messenger-message"
          :class="{ mine: item.message?.role === 'user' }"
          :data-testid="`messenger-heavy-item:${item.key}`"
          :data-virtual-key="item.key"
        >
          <div class="messenger-message-side">
            <div class="messenger-heavy-avatar">
              {{ item.message?.role === 'user' ? 'U' : 'A' }}
            </div>
          </div>
          <div class="messenger-message-main">
            <div class="messenger-message-meta">
              <span>{{ item.message?.role === 'user' ? 'User' : 'Assistant' }}</span>
              <span>{{ String(item.message?.created_at || '') }}</span>
            </div>
            <div class="messenger-message-bubble messenger-markdown">
              <div class="markdown-body" v-html="String(item.message?.renderedHtml || '')"></div>
            </div>
          </div>
        </div>
      </div>
    </section>

    <div class="messenger-heavy-flags">
      <span data-testid="messenger-heavy-flag-top">top: {{ showScrollTopButton }}</span>
      <span data-testid="messenger-heavy-flag-bottom">bottom: {{ showScrollBottomButton }}</span>
      <span data-testid="messenger-heavy-flag-stick">stick: {{ autoStickToBottom }}</span>
    </div>

    <pre class="messenger-heavy-state" data-testid="messenger-heavy-state">{{ stateSnapshot }}</pre>
  </main>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue';

import { prepareMessageMarkdownContent } from '@/utils/messageMarkdown';
import { renderMarkdown } from '@/utils/markdown';
import { createMessageViewportRuntime, type RenderableMessage } from '@/views/messenger/messageViewportRuntime';

type PerfState = {
  lastAction: string;
  loadDurationMs: number;
  appendDurationMs: number;
  scrollProbeDurationMs: number;
  scrollProbeMaxFrameGapMs: number;
  scrollProbeAverageFrameGapMs: number;
  scrollTop: number;
  scrollHeight: number;
  clientHeight: number;
  messageCount: number;
};

type HarnessApi = {
  loadHistory: (count?: number, intensity?: number) => Promise<void>;
  appendHistory: (count?: number, intensity?: number) => Promise<void>;
  jumpTop: () => Promise<void>;
  jumpBottom: () => Promise<void>;
  runScrollProbe: () => Promise<void>;
  resetHarness: () => void;
  snapshot: () => string;
};

declare global {
  interface Window {
    __messengerHeavyHistoryE2E?: HarnessApi;
  }
}

const messageListRef = ref<HTMLElement | null>(null);
const showChatSettingsView = ref(false);
const autoStickToBottom = ref(true);
const showScrollTopButton = ref(false);
const showScrollBottomButton = ref(false);
const isAgentConversationActive = ref(true);
const isWorldConversationActive = ref(false);
const shouldVirtualizeMessages = ref(false);
const agentRenderableMessages = ref<RenderableMessage[]>([]);
const worldRenderableMessages = ref<RenderableMessage[]>([]);
const messageVirtualHeightCache = new Map<string, number>();
const messageVirtualLayoutVersion = ref(0);
const messageVirtualScrollTop = ref(0);
const messageVirtualViewportHeight = ref(0);

const perf = ref<PerfState>({
  lastAction: 'idle',
  loadDurationMs: 0,
  appendDurationMs: 0,
  scrollProbeDurationMs: 0,
  scrollProbeMaxFrameGapMs: 0,
  scrollProbeAverageFrameGapMs: 0,
  scrollTop: 0,
  scrollHeight: 0,
  clientHeight: 0,
  messageCount: 0
});

const runtime = createMessageViewportRuntime({
  messageListRef,
  showChatSettingsView,
  autoStickToBottom,
  showScrollTopButton,
  showScrollBottomButton,
  isAgentConversationActive,
  isWorldConversationActive,
  shouldVirtualizeMessages,
  agentRenderableMessages,
  worldRenderableMessages,
  messageVirtualHeightCache,
  messageVirtualLayoutVersion,
  messageVirtualScrollTop,
  messageVirtualViewportHeight,
  estimateVirtualOffsetTop: () => 0,
  resolveVirtualMessageHeight: () => 220
});

const waitFrame = () =>
  new Promise<void>((resolve) => {
    requestAnimationFrame(() => resolve());
  });

const waitForSettle = async () => {
  await nextTick();
  await waitFrame();
  await waitFrame();
};

const updateMetrics = () => {
  const container = messageListRef.value;
  perf.value.scrollTop = Math.round(container?.scrollTop || 0);
  perf.value.scrollHeight = Math.round(container?.scrollHeight || 0);
  perf.value.clientHeight = Math.round(container?.clientHeight || 0);
  perf.value.messageCount = agentRenderableMessages.value.length;
};

const buildMarkdown = (index: number, intensity: number): string =>
  Array.from({ length: intensity }, (_, section) => [
    `## Thread ${index + 1} / Block ${section + 1}`,
    '',
    `This message simulates a realistic assistant bubble in MessengerView. It intentionally contains enough markdown structure to stress layout, wrapping, paint and scroll bookkeeping.`,
    '',
    `- History index: ${index + 1}`,
    `- Section: ${section + 1}`,
    `- Kind: messenger-heavy-history`,
    '',
    '| Metric | Value |',
    '| --- | --- |',
    `| index | ${index + 1} |`,
    `| section | ${section + 1} |`,
    '',
    '```md',
    `message_${index + 1}_${section + 1}: large messenger history payload`,
    '```',
    '',
    '> Repeated markdown content keeps the bubble shape close to real long-form answers rather than synthetic one-liners.'
  ].join('\n')).join('\n\n');

const createRenderableMessage = (index: number, intensity: number): RenderableMessage => {
  const content = buildMarkdown(index, intensity);
  const role = index % 3 === 0 ? 'user' : 'assistant';
  const createdAt = new Date(Date.UTC(2026, 3, 11, 9, Math.floor(index / 60), index % 60)).toISOString();
  const renderedHtml = renderMarkdown(prepareMessageMarkdownContent(content, null));
  return {
    key: `heavy-${index}`,
    message: {
      role,
      content,
      created_at: createdAt,
      renderedHtml
    }
  };
};

const buildMessages = (startIndex: number, count: number, intensity: number): RenderableMessage[] =>
  Array.from({ length: count }, (_, offset) => createRenderableMessage(startIndex + offset, intensity));

const syncViewportAfterMutation = async (latestKey?: string) => {
  await waitForSettle();
  runtime.scheduleMessageViewportRefresh({
    updateScrollState: true,
    measure: true,
    measureKeys: latestKey ? [latestKey] : undefined
  });
  await waitForSettle();
  updateMetrics();
};

const loadHistory = async (count = 400, intensity = 10) => {
  const startedAt = performance.now();
  agentRenderableMessages.value = buildMessages(0, count, intensity);
  await syncViewportAfterMutation(`heavy-${Math.max(count - 1, 0)}`);
  await runtime.jumpToMessageBottom();
  await waitForSettle();
  updateMetrics();
  perf.value.lastAction = `load:${count}`;
  perf.value.loadDurationMs = Math.round((performance.now() - startedAt) * 100) / 100;
};

const appendHistory = async (count = 40, intensity = 10) => {
  const startedAt = performance.now();
  const nextStart = agentRenderableMessages.value.length;
  agentRenderableMessages.value = [
    ...agentRenderableMessages.value,
    ...buildMessages(nextStart, count, intensity)
  ];
  await syncViewportAfterMutation(`heavy-${Math.max(nextStart + count - 1, 0)}`);
  if (autoStickToBottom.value) {
    await runtime.scrollMessagesToBottom(true);
    await waitForSettle();
  }
  updateMetrics();
  perf.value.lastAction = `append:${count}`;
  perf.value.appendDurationMs = Math.round((performance.now() - startedAt) * 100) / 100;
};

const jumpTop = async () => {
  await runtime.jumpToMessageTop();
  await waitForSettle();
  updateMetrics();
  perf.value.lastAction = 'jump-top';
};

const jumpBottom = async () => {
  await runtime.jumpToMessageBottom();
  await waitForSettle();
  updateMetrics();
  perf.value.lastAction = 'jump-bottom';
};

const runScrollProbe = async () => {
  const container = messageListRef.value;
  if (!container) return;
  const probeStart = performance.now();
  const frameGaps: number[] = [];
  let previous = performance.now();
  const maxTop = Math.max(0, container.scrollHeight - container.clientHeight);
  for (let step = 0; step <= 24; step += 1) {
    container.scrollTop = Math.round(maxTop * (step / 24));
    runtime.handleMessageListScroll();
    await waitFrame();
    const current = performance.now();
    frameGaps.push(current - previous);
    previous = current;
  }
  runtime.handleMessageListScroll();
  await waitForSettle();
  updateMetrics();
  const total = frameGaps.reduce((sum, value) => sum + value, 0);
  perf.value.lastAction = 'scroll-probe';
  perf.value.scrollProbeDurationMs = Math.round((performance.now() - probeStart) * 100) / 100;
  perf.value.scrollProbeMaxFrameGapMs = Math.round(Math.max(...frameGaps, 0) * 100) / 100;
  perf.value.scrollProbeAverageFrameGapMs = Math.round((total / Math.max(frameGaps.length, 1)) * 100) / 100;
};

const handleMessageListScroll = () => {
  runtime.handleMessageListScroll();
};

const resetHarness = () => {
  agentRenderableMessages.value = [];
  autoStickToBottom.value = true;
  showScrollTopButton.value = false;
  showScrollBottomButton.value = false;
  perf.value = {
    lastAction: 'reset',
    loadDurationMs: 0,
    appendDurationMs: 0,
    scrollProbeDurationMs: 0,
    scrollProbeMaxFrameGapMs: 0,
    scrollProbeAverageFrameGapMs: 0,
    scrollTop: 0,
    scrollHeight: 0,
    clientHeight: 0,
    messageCount: 0
  };
};

const stateSnapshot = computed(() =>
  JSON.stringify(
    {
      perf: perf.value,
      flags: {
        showScrollTopButton: showScrollTopButton.value,
        showScrollBottomButton: showScrollBottomButton.value,
        autoStickToBottom: autoStickToBottom.value,
        messageVirtualScrollTop: messageVirtualScrollTop.value,
        messageVirtualViewportHeight: messageVirtualViewportHeight.value
      }
    },
    null,
    2
  )
);

const harnessApi: HarnessApi = {
  loadHistory,
  appendHistory,
  jumpTop,
  jumpBottom,
  runScrollProbe,
  resetHarness,
  snapshot: () => stateSnapshot.value
};

onMounted(() => {
  resetHarness();
  window.__messengerHeavyHistoryE2E = harnessApi;
});

onBeforeUnmount(() => {
  runtime.dispose();
  if (window.__messengerHeavyHistoryE2E === harnessApi) {
    delete window.__messengerHeavyHistoryE2E;
  }
});
</script>

<style scoped>
.messenger-heavy-e2e {
  min-height: 100vh;
  padding: 20px;
  background:
    radial-gradient(circle at top right, rgba(59, 130, 246, 0.18), transparent 32%),
    linear-gradient(180deg, #f8fafc 0%, #eef4ff 100%);
  color: #0f172a;
}

.messenger-heavy-toolbar {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  margin-bottom: 16px;
}

.messenger-heavy-toolbar button {
  border: 1px solid rgba(148, 163, 184, 0.5);
  background: #ffffff;
  color: #0f172a;
  border-radius: 999px;
  padding: 8px 14px;
  font-size: 13px;
  cursor: pointer;
}

.messenger-heavy-shell {
  max-width: 1120px;
  min-height: 72vh;
  border-radius: 24px;
  overflow: hidden;
  border: 1px solid rgba(148, 163, 184, 0.22);
}

.messenger-heavy-stream {
  height: 72vh;
  padding: 18px;
}

.messenger-heavy-avatar {
  width: 32px;
  height: 32px;
  border-radius: 999px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: linear-gradient(135deg, #0f766e, #0284c7);
  color: #ffffff;
  font-size: 12px;
  font-weight: 700;
}

.messenger-heavy-flags {
  display: flex;
  gap: 14px;
  margin-top: 14px;
  font-size: 12px;
  color: #334155;
}

.messenger-heavy-state {
  margin-top: 14px;
  max-width: 1120px;
  padding: 14px;
  border-radius: 14px;
  background: #0f172a;
  color: #dbeafe;
  font-size: 12px;
  overflow: auto;
}
</style>
