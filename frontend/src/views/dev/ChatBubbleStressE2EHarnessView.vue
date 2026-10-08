<template>
  <main class="chat-bubble-stress-e2e" data-testid="chat-bubble-stress-harness">
    <header class="chat-bubble-stress-toolbar">
      <button type="button" data-testid="scenario-load-120-huge" @click="loadScenario(120, 12)">
        Load 120 Huge
      </button>
      <button type="button" data-testid="scenario-load-240-huge" @click="loadScenario(240, 12)">
        Load 240 Huge
      </button>
      <button type="button" data-testid="append-20-huge" @click="appendMessages(20, 12)">
        Append 20 Huge
      </button>
      <button type="button" data-testid="probe-scroll-jank" @click="runScrollProbe()">
        Probe Scroll
      </button>
      <button type="button" data-testid="reset-bubbles" @click="resetHarness()">Reset</button>
    </header>

    <section class="messenger-chat chat-shell chat-bubble-stress-shell">
      <div
        ref="streamRef"
        class="messenger-chat-body is-messages is-agent chat-bubble-stress-stream"
        data-testid="chat-bubble-stress-stream"
      >
        <div
          v-for="message in messages"
          :key="message.key"
          class="messenger-message"
          :class="{ mine: message.role === 'user' }"
          :data-testid="`chat-bubble-item:${message.key}`"
        >
          <div class="messenger-message-side">
            <div class="chat-bubble-stress-avatar">
              {{ message.role === 'user' ? 'U' : 'A' }}
            </div>
          </div>
          <div class="messenger-message-main">
            <div class="messenger-message-meta">
              <span>{{ message.role === 'user' ? 'User' : 'Assistant' }}</span>
              <span>{{ message.timeLabel }}</span>
            </div>
            <div class="messenger-message-bubble messenger-markdown">
              <div class="markdown-body" v-html="message.html"></div>
            </div>
          </div>
        </div>
      </div>
    </section>

    <pre class="chat-bubble-stress-state" data-testid="chat-bubble-stress-state">{{ stateSnapshot }}</pre>
  </main>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue';

import { prepareMessageMarkdownContent } from '@/utils/messageMarkdown';
import { renderMarkdown } from '@/utils/markdown';

type StressMessage = {
  key: string;
  role: 'user' | 'assistant';
  content: string;
  html: string;
  createdAt: string;
  timeLabel: string;
};

type PerfMetrics = {
  lastAction: string;
  renderDurationMs: number;
  appendDurationMs: number;
  scrollProbeDurationMs: number;
  scrollProbeMaxFrameGapMs: number;
  scrollProbeAverageFrameGapMs: number;
  scrollHeight: number;
  clientHeight: number;
  messageCount: number;
};

type HarnessApi = {
  loadScenario: (count?: number, intensity?: number) => Promise<void>;
  appendMessages: (count?: number, intensity?: number) => Promise<void>;
  runScrollProbe: () => Promise<void>;
  resetHarness: () => void;
  snapshot: () => string;
};

declare global {
  interface Window {
    __chatBubbleStressE2E?: HarnessApi;
  }
}

const streamRef = ref<HTMLElement | null>(null);
const messages = ref<StressMessage[]>([]);
const perf = ref<PerfMetrics>({
  lastAction: 'idle',
  renderDurationMs: 0,
  appendDurationMs: 0,
  scrollProbeDurationMs: 0,
  scrollProbeMaxFrameGapMs: 0,
  scrollProbeAverageFrameGapMs: 0,
  scrollHeight: 0,
  clientHeight: 0,
  messageCount: 0
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

const updateContainerMetrics = () => {
  const container = streamRef.value;
  perf.value.scrollHeight = Math.round(container?.scrollHeight || 0);
  perf.value.clientHeight = Math.round(container?.clientHeight || 0);
  perf.value.messageCount = messages.value.length;
};

const buildMarkdownChunk = (index: number, repeat: number): string => {
  const paragraphs = Array.from({ length: repeat }, (_, blockIndex) => [
    `### Bubble ${index + 1} / Section ${blockIndex + 1}`,
    '',
    `This is a deliberately large markdown bubble used for responsiveness testing. It includes repeated prose, lists, tables, and code blocks so layout and paint work are closer to the real chat page.`,
    '',
    `- Bullet A for message ${index + 1}`,
    `- Bullet B with more descriptive content for section ${blockIndex + 1}`,
    `- Bullet C containing inline code like \`message_${index + 1}_${blockIndex + 1}\``,
    '',
    '| Column | Value | Note |',
    '| --- | --- | --- |',
    `| message | ${index + 1} | stress test |`,
    `| section | ${blockIndex + 1} | large bubble |`,
    `| payload | ${repeat} | markdown |`,
    '',
    '```ts',
    `const stressBubble${index}_${blockIndex} = {`,
    `  message: ${index + 1},`,
    `  section: ${blockIndex + 1},`,
    `  summary: 'Large markdown bubble for chat render stress',`,
    `  values: ['alpha', 'beta', 'gamma', 'delta']`,
    '};',
    '```',
    '',
    '> Long quote block to stretch line wrapping and paragraph layout across the chat bubble. The point is to simulate real assistant outputs that are large rather than synthetic one-line blobs.'
  ].join('\n'));
  return paragraphs.join('\n\n');
};

const createMessage = (index: number, intensity: number): StressMessage => {
  const createdAt = new Date(Date.UTC(2026, 3, 11, 8, 0, index)).toISOString();
  const content = buildMarkdownChunk(index, intensity);
  return {
    key: `stress-${index}`,
    role: index % 2 === 0 ? 'assistant' : 'user',
    content,
    html: renderMarkdown(prepareMessageMarkdownContent(content, null)),
    createdAt,
    timeLabel: `${String(8 + Math.floor(index / 60)).padStart(2, '0')}:${String(index % 60).padStart(2, '0')}`
  };
};

const buildMessages = (startIndex: number, count: number, intensity: number): StressMessage[] =>
  Array.from({ length: count }, (_, offset) => createMessage(startIndex + offset, intensity));

const loadScenario = async (count = 120, intensity = 12) => {
  const startedAt = performance.now();
  messages.value = buildMessages(0, count, intensity);
  await waitForSettle();
  updateContainerMetrics();
  perf.value.lastAction = `load:${count}`;
  perf.value.renderDurationMs = Math.round((performance.now() - startedAt) * 100) / 100;
};

const appendMessages = async (count = 20, intensity = 12) => {
  const startedAt = performance.now();
  const nextStart = messages.value.length;
  messages.value = [...messages.value, ...buildMessages(nextStart, count, intensity)];
  await waitForSettle();
  updateContainerMetrics();
  perf.value.lastAction = `append:${count}`;
  perf.value.appendDurationMs = Math.round((performance.now() - startedAt) * 100) / 100;
};

const runScrollProbe = async () => {
  const container = streamRef.value;
  if (!container) return;
  const probeStart = performance.now();
  const frameGaps: number[] = [];
  let previousFrame = performance.now();
  const maxTop = Math.max(0, container.scrollHeight - container.clientHeight);

  for (let step = 0; step <= 24; step += 1) {
    const ratio = step / 24;
    container.scrollTop = Math.round(maxTop * ratio);
    await waitFrame();
    const currentFrame = performance.now();
    frameGaps.push(currentFrame - previousFrame);
    previousFrame = currentFrame;
  }

  const totalGap = frameGaps.reduce((sum, value) => sum + value, 0);
  perf.value.lastAction = 'scroll-probe';
  perf.value.scrollProbeDurationMs = Math.round((performance.now() - probeStart) * 100) / 100;
  perf.value.scrollProbeMaxFrameGapMs = Math.round(Math.max(...frameGaps, 0) * 100) / 100;
  perf.value.scrollProbeAverageFrameGapMs = Math.round((totalGap / Math.max(frameGaps.length, 1)) * 100) / 100;
  updateContainerMetrics();
};

const resetHarness = () => {
  messages.value = [];
  perf.value = {
    lastAction: 'reset',
    renderDurationMs: 0,
    appendDurationMs: 0,
    scrollProbeDurationMs: 0,
    scrollProbeMaxFrameGapMs: 0,
    scrollProbeAverageFrameGapMs: 0,
    scrollHeight: 0,
    clientHeight: 0,
    messageCount: 0
  };
};

const stateSnapshot = computed(() =>
  JSON.stringify(
    {
      perf: perf.value,
      sample: messages.value.slice(0, 3).map((message) => ({
        key: message.key,
        role: message.role,
        contentLength: message.content.length,
        htmlLength: message.html.length
      }))
    },
    null,
    2
  )
);

const harnessApi: HarnessApi = {
  loadScenario,
  appendMessages,
  runScrollProbe,
  resetHarness,
  snapshot: () => stateSnapshot.value
};

onMounted(() => {
  resetHarness();
  window.__chatBubbleStressE2E = harnessApi;
});

onBeforeUnmount(() => {
  if (window.__chatBubbleStressE2E === harnessApi) {
    delete window.__chatBubbleStressE2E;
  }
});
</script>

<style scoped>
.chat-bubble-stress-e2e {
  min-height: 100vh;
  padding: 20px;
  background:
    radial-gradient(circle at top left, rgba(14, 165, 233, 0.18), transparent 28%),
    linear-gradient(180deg, #f8fafc 0%, #edf4ff 100%);
  color: #0f172a;
}

.chat-bubble-stress-toolbar {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  margin-bottom: 16px;
}

.chat-bubble-stress-toolbar button {
  border: 1px solid rgba(148, 163, 184, 0.5);
  background: #ffffff;
  color: #0f172a;
  border-radius: 999px;
  padding: 8px 14px;
  font-size: 13px;
  cursor: pointer;
}

.chat-bubble-stress-shell {
  max-width: 1120px;
  min-height: 72vh;
  border-radius: 24px;
  border: 1px solid rgba(148, 163, 184, 0.2);
  overflow: hidden;
}

.chat-bubble-stress-stream {
  height: 72vh;
  padding: 18px;
}

.chat-bubble-stress-avatar {
  width: 32px;
  height: 32px;
  border-radius: 999px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: linear-gradient(135deg, #0ea5e9, #2563eb);
  color: #ffffff;
  font-size: 12px;
  font-weight: 700;
}

.chat-bubble-stress-state {
  margin-top: 16px;
  max-width: 1120px;
  padding: 14px;
  border-radius: 14px;
  background: #0f172a;
  color: #dbeafe;
  font-size: 12px;
  overflow: auto;
}
</style>
