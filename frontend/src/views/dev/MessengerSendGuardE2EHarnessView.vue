<template>
  <main class="messenger-send-guard-e2e" data-testid="messenger-send-guard-harness">
    <header class="messenger-send-guard-toolbar">
      <button type="button" data-testid="load-send-guard-base" @click="loadBaseScenario()">
        Load Base
      </button>
      <button type="button" data-testid="simulate-send-glitch" @click="simulateSendGlitch()">
        Simulate Send Glitch
      </button>
      <button type="button" data-testid="reset-send-guard" @click="resetHarness()">
        Reset
      </button>
    </header>

    <section class="messenger-chat chat-shell messenger-send-guard-shell">
      <div class="messenger-chat-body is-messages is-agent messenger-send-guard-stream" data-testid="messenger-send-guard-stream">
        <div v-if="!showChatSettingsView && !hasRetainedMessageConversationContext && !activeConversation" class="messenger-chat-empty-state">
          <div class="messenger-chat-empty" data-testid="send-guard-empty">no-active-agent-view</div>
        </div>
        <template v-else-if="isAgentConversationActive">
          <div
            v-for="item in agentRenderableMessages"
            :key="item.key"
            class="messenger-message"
            :class="{ mine: item.message.role === 'user' }"
            :data-testid="`send-guard-item:${item.key}`"
          >
            <div class="messenger-message-side">
              <div class="messenger-send-guard-avatar">
                {{ item.message.role === 'user' ? 'U' : 'A' }}
              </div>
            </div>
            <div class="messenger-message-main">
              <div class="messenger-message-meta">
                <span>{{ item.message.role === 'user' ? 'User' : 'Assistant' }}</span>
                <span>{{ item.message.created_at }}</span>
              </div>
              <div class="messenger-message-bubble messenger-markdown">
                <div class="markdown-body">{{ item.message.content || '[pending]' }}</div>
              </div>
            </div>
          </div>
        </template>
        <div v-else class="messenger-chat-empty" data-testid="send-guard-empty">
          no-active-agent-view
        </div>
      </div>
    </section>

    <pre class="messenger-send-guard-state" data-testid="messenger-send-guard-state">{{ stateSnapshot }}</pre>
  </main>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';

import {
  hasRetainedAgentConversationContext,
  hasRetainedMessageConversationContext as resolveRetainedMessageConversationContext
} from '@/views/messenger/messageConversationRetention';

type HarnessMessage = {
  key: string;
  message: {
    role: 'user' | 'assistant';
    content: string;
    created_at: string;
    stream_incomplete?: boolean;
    workflowStreaming?: boolean;
  };
};

type ActiveConversation = {
  kind: 'agent' | 'direct' | 'group';
  id: string;
  agentId?: string;
} | null;

type HarnessApi = {
  loadBaseScenario: () => void;
  simulateSendGlitch: () => Promise<void>;
  resetHarness: () => void;
  snapshot: () => string;
};

declare global {
  interface Window {
    __messengerSendGuardE2E?: HarnessApi;
  }
}

const waitFrame = () =>
  new Promise<void>((resolve) => {
    requestAnimationFrame(() => resolve());
  });

const activeSection = ref<'messages' | 'agents'>('messages');
const activeConversation = ref<ActiveConversation>({
  kind: 'agent',
  id: 'sess-demo',
  agentId: 'agent_demo'
});
const activeSessionId = ref('sess-demo');
const draftAgentId = ref('agent_demo');
const foregroundLock = ref(false);
const worldConversationId = ref('');
const worldMessageCount = ref(0);
const agentRenderableMessages = ref<HarnessMessage[]>([]);

const showChatSettingsView = computed(() => activeSection.value !== 'messages');

const hasRetainedMessageConversationContext = computed(() =>
  resolveRetainedMessageConversationContext({
    foregroundLock: foregroundLock.value,
    activeConversationKind: activeConversation.value?.kind,
    activeConversationId: activeConversation.value?.id,
    activeSessionId: activeSessionId.value,
    draftAgentId: draftAgentId.value,
    messageCount: agentRenderableMessages.value.length,
    worldConversationId: worldConversationId.value,
    worldMessageCount: worldMessageCount.value
  })
);

const isAgentConversationActive = computed(() =>
  hasRetainedAgentConversationContext({
    foregroundLock: foregroundLock.value,
    activeConversationKind: activeConversation.value?.kind,
    activeConversationId: activeConversation.value?.id,
    activeSessionId: activeSessionId.value,
    draftAgentId: draftAgentId.value,
    messageCount: agentRenderableMessages.value.length,
    worldConversationId: worldConversationId.value,
    worldMessageCount: worldMessageCount.value
  })
);

const loadBaseScenario = () => {
  activeSection.value = 'messages';
  activeConversation.value = {
    kind: 'agent',
    id: 'sess-demo',
    agentId: 'agent_demo'
  };
  activeSessionId.value = 'sess-demo';
  draftAgentId.value = 'agent_demo';
  foregroundLock.value = false;
  worldConversationId.value = '';
  worldMessageCount.value = 0;
  agentRenderableMessages.value = [
    {
      key: 'user-1',
      message: {
        role: 'user',
        content: 'Need the next answer.',
        created_at: '2026-05-09T10:10:00.000Z'
      }
    }
  ];
};

const simulateSendGlitch = async () => {
  loadBaseScenario();
  foregroundLock.value = true;
  activeConversation.value = null;
  activeSessionId.value = '';
  draftAgentId.value = '';
  await waitFrame();
  agentRenderableMessages.value = [
    ...agentRenderableMessages.value,
    {
      key: 'assistant-pending',
      message: {
        role: 'assistant',
        content: '',
        created_at: '2026-05-09T10:10:01.000Z',
        stream_incomplete: true,
        workflowStreaming: true
      }
    }
  ];
  await waitFrame();
  foregroundLock.value = false;
};

const resetHarness = () => {
  activeSection.value = 'messages';
  activeConversation.value = null;
  activeSessionId.value = '';
  draftAgentId.value = '';
  foregroundLock.value = false;
  worldConversationId.value = '';
  worldMessageCount.value = 0;
  agentRenderableMessages.value = [];
};

const stateSnapshot = computed(() =>
  JSON.stringify(
    {
      activeSection: activeSection.value,
      activeConversation: activeConversation.value,
      activeSessionId: activeSessionId.value,
      draftAgentId: draftAgentId.value,
      foregroundLock: foregroundLock.value,
      hasRetainedMessageConversationContext: hasRetainedMessageConversationContext.value,
      isAgentConversationActive: isAgentConversationActive.value,
      messageCount: agentRenderableMessages.value.length,
      keys: agentRenderableMessages.value.map((item) => item.key)
    },
    null,
    2
  )
);

const harnessApi: HarnessApi = {
  loadBaseScenario,
  simulateSendGlitch,
  resetHarness,
  snapshot: () => stateSnapshot.value
};

onMounted(() => {
  resetHarness();
  window.__messengerSendGuardE2E = harnessApi;
});

onBeforeUnmount(() => {
  if (window.__messengerSendGuardE2E === harnessApi) {
    delete window.__messengerSendGuardE2E;
  }
});
</script>

<style scoped>
.messenger-send-guard-e2e {
  min-height: 100vh;
  padding: 20px;
  background:
    radial-gradient(circle at top left, rgba(59, 130, 246, 0.18), transparent 32%),
    linear-gradient(180deg, #f8fafc 0%, #eef6ff 100%);
  color: #0f172a;
}

.messenger-send-guard-toolbar {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  margin-bottom: 16px;
}

.messenger-send-guard-toolbar button {
  border: 1px solid rgba(148, 163, 184, 0.5);
  background: #ffffff;
  color: #0f172a;
  border-radius: 999px;
  padding: 8px 14px;
  font-size: 13px;
  cursor: pointer;
}

.messenger-send-guard-shell {
  max-width: 960px;
  min-height: 420px;
  border-radius: 20px;
  border: 1px solid rgba(148, 163, 184, 0.24);
  overflow: hidden;
}

.messenger-send-guard-stream {
  display: flex;
  flex-direction: column;
  gap: 14px;
  min-height: 420px;
  padding: 18px;
}

.messenger-send-guard-avatar {
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

.messenger-send-guard-state {
  margin-top: 16px;
  padding: 14px;
  border-radius: 14px;
  background: #0f172a;
  color: #e2e8f0;
  font-size: 12px;
  line-height: 1.5;
  overflow: auto;
}
</style>
