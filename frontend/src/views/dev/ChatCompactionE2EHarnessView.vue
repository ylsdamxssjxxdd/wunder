<template>
  <main class="chat-compaction-e2e" data-testid="chat-compaction-e2e-harness">
    <header class="chat-compaction-e2e-toolbar">
      <button type="button" data-testid="scenario-manual-running" @click="loadManualRunningScenario()">
        Manual Running
      </button>
      <button type="button" data-testid="hydrate-manual-terminal" @click="hydrateManualTerminal()">
        Hydrate Terminal
      </button>
      <button type="button" data-testid="append-next-turn-busy" @click="appendNextTurnBusy()">
        Append Next Turn
      </button>
      <button
        type="button"
        data-testid="rehydrate-after-next-turn"
        @click="rehydrateAfterNextTurn()"
      >
        Rehydrate Next Turn
      </button>
      <button type="button" data-testid="reset-harness" @click="resetHarness()">Reset</button>
    </header>

    <section class="chat-compaction-e2e-stream" data-testid="chat-compaction-stream">
      <div
        v-for="message in messages"
        :key="message.key"
        class="chat-compaction-e2e-message"
        :data-testid="`chat-compaction-message:${message.key}`"
      >
        <template v-if="shouldRenderDivider(message)">
          <MessageCompactionDivider
            :items="Array.isArray(message.workflowItems) ? message.workflowItems : []"
            :is-streaming="
              Boolean(message.workflowStreaming || message.reasoningStreaming || message.stream_incomplete)
            "
            :manual-marker="
              message.manual_compaction_marker === true || message.manualCompactionMarker === true
            "
            :session-busy="sessionBusy"
          />
        </template>
        <template v-else>
          <div class="chat-compaction-e2e-bubble" :data-role="message.role">
            <strong>{{ message.role }}</strong>
            <span>{{ message.content || message.key }}</span>
          </div>
        </template>
      </div>
    </section>

    <pre class="chat-compaction-e2e-state" data-testid="chat-compaction-state">{{ stateSnapshot }}</pre>
  </main>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';

import MessageCompactionDivider from '@/components/chat/MessageCompactionDivider.vue';
import { mergeCompactionMarkersIntoMessages, isCompactionMarkerAssistantMessage } from '@/stores/chatCompactionMarker';
import { resolveLatestCompactionSnapshot } from '@/utils/chatCompactionWorkflow';

type HarnessMessage = Record<string, any>;

type HarnessApi = {
  loadManualRunningScenario: () => void;
  hydrateManualTerminal: () => void;
  appendNextTurnBusy: () => void;
  rehydrateAfterNextTurn: () => void;
  resetHarness: () => void;
  snapshot: () => string;
};

declare global {
  interface Window {
    __chatCompactionE2EHarness?: HarnessApi;
  }
}

const messages = ref<HarnessMessage[]>([]);
const sessionBusy = ref(false);

const buildDetail = (detail: Record<string, unknown>) => JSON.stringify(detail);

const buildManualRunningMarker = (): HarnessMessage => ({
  key: 'cmp-running',
  role: 'assistant',
  content: '',
  reasoning: '',
  created_at: '2026-04-10T10:00:00.000Z',
  stream_round: 2,
  workflowStreaming: true,
  stream_incomplete: true,
  manual_compaction_marker: true,
  workflowItems: [
    {
      eventType: 'compaction_progress',
      status: 'loading',
      toolName: 'context_compaction',
      toolCallId: 'compaction:manual:demo-2',
      detail: buildDetail({
        status: 'loading',
        stage: 'compacting',
        trigger_mode: 'manual',
        user_round: 2
      })
    }
  ]
});

const buildManualTerminalMarker = (): HarnessMessage => ({
  key: 'cmp-terminal',
  role: 'assistant',
  content: '',
  reasoning: '',
  created_at: '2026-04-10T10:00:02.000Z',
  stream_round: 2,
  workflowStreaming: false,
  stream_incomplete: false,
  manual_compaction_marker: true,
  workflowItems: [
    {
      eventType: 'compaction',
      status: 'completed',
      toolName: 'context_compaction',
      toolCallId: 'compaction:manual:demo-2',
      detail: buildDetail({
        status: 'done',
        trigger_mode: 'manual',
        user_round: 2,
        projected_request_tokens: 16249,
        projected_request_tokens_after: 5670,
        summary_text: 'Compressed summary baseline'
      })
    }
  ]
});

const buildBaseMessages = (): HarnessMessage[] => [
  {
    key: 'user-1',
    role: 'user',
    content: 'Need a compact summary before continuing.',
    created_at: '2026-04-10T09:59:50.000Z'
  },
  {
    key: 'assistant-1',
    role: 'assistant',
    content: 'Preparing the conversation state.',
    created_at: '2026-04-10T09:59:55.000Z'
  }
];

const buildNextTurnUserMessage = (): HarnessMessage => ({
  key: 'user-2',
  role: 'user',
  content: 'Continue the task after compaction.',
  created_at: '2026-04-10T10:00:10.000Z'
});

const buildPendingAssistantShell = (): HarnessMessage => ({
  key: 'assistant-pending-2',
  role: 'assistant',
  content: '',
  reasoning: '',
  created_at: '2026-04-10T10:00:11.000Z',
  workflowStreaming: true,
  stream_incomplete: true,
  workflowItems: []
});

const shouldRenderDivider = (message: HarnessMessage): boolean => {
  if (!isCompactionMarkerAssistantMessage(message)) return false;
  if (
    (message?.manual_compaction_marker === true || message?.manualCompactionMarker === true) &&
    Boolean(message?.workflowStreaming || message?.reasoningStreaming || message?.stream_incomplete)
  ) {
    return true;
  }
  const snapshot = resolveLatestCompactionSnapshot(message?.workflowItems);
  if (!snapshot) return false;
  const detailStatus = String(snapshot.detail?.status || '').trim().toLowerCase();
  if (detailStatus === 'skipped') return false;
  return true;
};

const sortMessages = (items: HarnessMessage[]): HarnessMessage[] =>
  [...items].sort((left, right) => {
    const leftTime = Date.parse(String(left.created_at || '')) || 0;
    const rightTime = Date.parse(String(right.created_at || '')) || 0;
    return leftTime - rightTime;
  });

const setMessages = (nextMessages: HarnessMessage[]) => {
  messages.value = sortMessages(nextMessages);
};

const resetHarness = () => {
  sessionBusy.value = false;
  setMessages(buildBaseMessages());
};

const loadManualRunningScenario = () => {
  sessionBusy.value = true;
  setMessages([...buildBaseMessages(), buildManualRunningMarker()]);
};

const hydrateManualTerminal = () => {
  const remoteMessages = [
    ...messages.value.filter((message) => !(message.manual_compaction_marker === true || message.manualCompactionMarker === true)),
    buildManualTerminalMarker()
  ];
  sessionBusy.value = false;
  setMessages(mergeCompactionMarkersIntoMessages(remoteMessages, messages.value));
};

const appendNextTurnBusy = () => {
  const next = [...messages.value];
  if (!next.some((message) => message.key === 'user-2')) {
    next.push(buildNextTurnUserMessage());
  }
  if (!next.some((message) => message.key === 'assistant-pending-2')) {
    next.push(buildPendingAssistantShell());
  }
  sessionBusy.value = true;
  setMessages(next);
};

const rehydrateAfterNextTurn = () => {
  const remoteMessages = [
    ...buildBaseMessages(),
    buildManualTerminalMarker(),
    buildNextTurnUserMessage()
  ];
  setMessages(mergeCompactionMarkersIntoMessages(remoteMessages, messages.value));
};

const stateSnapshot = computed(() =>
  JSON.stringify(
    {
      sessionBusy: sessionBusy.value,
      dividerCount: messages.value.filter((message) => shouldRenderDivider(message)).length,
      messages: messages.value.map((message) => ({
        key: message.key,
        role: message.role,
        manual: message.manual_compaction_marker === true || message.manualCompactionMarker === true,
        streaming: Boolean(message.workflowStreaming || message.reasoningStreaming || message.stream_incomplete),
        divider: shouldRenderDivider(message)
      }))
    },
    null,
    2
  )
);

const harnessApi: HarnessApi = {
  loadManualRunningScenario,
  hydrateManualTerminal,
  appendNextTurnBusy,
  rehydrateAfterNextTurn,
  resetHarness,
  snapshot: () => stateSnapshot.value
};

onMounted(() => {
  resetHarness();
  window.__chatCompactionE2EHarness = harnessApi;
});

onBeforeUnmount(() => {
  if (window.__chatCompactionE2EHarness === harnessApi) {
    delete window.__chatCompactionE2EHarness;
  }
});
</script>

<style scoped>
.chat-compaction-e2e {
  min-height: 100vh;
  padding: 20px;
  background:
    radial-gradient(circle at top left, rgba(56, 189, 248, 0.16), transparent 34%),
    linear-gradient(180deg, #f8fafc 0%, #eef6ff 100%);
  color: #0f172a;
}

.chat-compaction-e2e-toolbar {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  margin-bottom: 16px;
}

.chat-compaction-e2e-toolbar button {
  border: 1px solid rgba(148, 163, 184, 0.5);
  background: #ffffff;
  color: #0f172a;
  border-radius: 999px;
  padding: 8px 14px;
  font-size: 13px;
  cursor: pointer;
}

.chat-compaction-e2e-stream {
  display: flex;
  flex-direction: column;
  gap: 12px;
  max-width: 960px;
  padding: 18px;
  border: 1px solid rgba(148, 163, 184, 0.28);
  border-radius: 18px;
  background: rgba(255, 255, 255, 0.92);
}

.chat-compaction-e2e-bubble {
  display: flex;
  gap: 10px;
  align-items: center;
  padding: 10px 12px;
  border-radius: 14px;
  background: #f8fafc;
  border: 1px solid rgba(226, 232, 240, 0.9);
  font-size: 13px;
}

.chat-compaction-e2e-bubble[data-role='assistant'] {
  background: #ecfeff;
}

.chat-compaction-e2e-state {
  margin-top: 16px;
  max-width: 960px;
  padding: 14px;
  border-radius: 14px;
  background: #0f172a;
  color: #dbeafe;
  font-size: 12px;
  overflow: auto;
}
</style>
