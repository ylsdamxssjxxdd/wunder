<template>
  <section
    class="messenger-split-pane"
    :class="{ 'is-active': active }"
    :data-session-id="sessionId"
    @pointerdown="emit('activate', sessionId)"
  >
    <header class="messenger-split-pane-head">
      <span class="messenger-split-pane-state" :class="{ 'is-running': busy }" aria-hidden="true"></span>
      <span class="messenger-split-pane-title" :title="title">{{ title }}</span>
      <button
        class="messenger-split-pane-close"
        type="button"
        :title="t('messenger.split.closePane')"
        :aria-label="t('messenger.split.closePane')"
        @pointerdown.stop
        @click="emit('close', slotIndex)"
      >
        <i class="fa-solid fa-xmark" aria-hidden="true"></i>
      </button>
    </header>

    <div ref="messageList" class="messenger-split-pane-messages" @scroll.passive="handleScroll">
      <div v-if="phase === 'opening' && !rows.length" class="messenger-split-pane-empty">
        {{ t('common.loading') }}
      </div>
      <div v-else-if="phase === 'error' && !rows.length" class="messenger-split-pane-empty is-error">
        <span>{{ error || t('messenger.error.openConversation') }}</span>
        <button type="button" @click="load">{{ t('messenger.tasks.retry') }}</button>
      </div>
      <div v-else-if="!rows.length" class="messenger-split-pane-empty">
        {{ t('messenger.empty.selectConversation') }}
      </div>
      <MessengerTurnRow
        v-for="row in rows"
        v-else
        :key="row.key"
        :controller="controller"
        :turn="row"
        :session-id="sessionId"
      />
    </div>

    <footer class="messenger-split-pane-composer messenger-composer-scope chat-shell">
      <ChatComposer
        :session-id="sessionId"
        :loading="busy"
        :send-key="controller.messengerSendKey.value"
        :draft-key="draftKey"
        :model-name="modelName"
        :apply-model="applyModel"
        :reasoning-effort="reasoningEffort"
        :context-messages="contextMessages"
        :workspace-agent-id="agentId"
        :approval-mode="approvalMode"
        :approval-mode-editable="false"
        :voice-supported="false"
        @send="emit('send', { sessionId, payload: $event })"
        @stop="emit('stop', sessionId)"
      />
    </footer>
  </section>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';
import ChatComposer from '@/components/chat/ChatComposer.vue';
import MessengerTurnRow from './MessengerTurnRow.vue';
import { openThreadViewDetached } from '@/stores/threadViewLoader';
import { getSessionMessages } from '@/stores/chatRuntimeState';
import {
  setThreadViewVisible,
  threadViewCommitted,
  threadViewError,
  threadViewPhase,
  threadViewSlots,
  threadViewStatus
} from '@/realtime/chat/threadViewStore';
import { buildTranscriptTurnRows } from '../controller/legacyTurnRowAdapter';
import { chatQueueForSession } from '@/stores/chatQueueState';
import { isParkedQueueTurn } from '@/stores/chatQueueProjection';
import type { MessengerControllerContext } from '../controller/messengerControllerContext';

const props = defineProps<{
  controller: MessengerControllerContext;
  sessionId: string;
  slotIndex: number;
  active: boolean;
}>();
const emit = defineEmits<{
  activate: [sessionId: string];
  close: [slotIndex: number];
  send: [payload: { sessionId: string; payload: Record<string, unknown> }];
  stop: [sessionId: string];
}>();

const chatStore = props.controller.chatStore;
const t = props.controller.t;
const messageList = ref<HTMLElement | null>(null);
const followOutput = ref(true);
const loadSerial = ref(0);

const session = computed(() =>
  (chatStore.sessions || []).find((item: any) => String(item?.id || '') === props.sessionId) || null
);
const title = computed(() => String(session.value?.title || t('chat.newSession')));
const agentId = computed(() => String(session.value?.agent_id || ''));
const modelName = computed(() => String(
  session.value?.session_model_name || session.value?.model_name || session.value?.model || ''
));
const reasoningEffort = computed(() => String(session.value?.reasoning_effort || 'default'));
const approvalMode = computed(() => String(session.value?.approval_mode || 'full_auto'));
const phase = computed(() => threadViewPhase(props.sessionId) || 'opening');
const error = computed(() => threadViewError(props.sessionId));
const busy = computed(() => {
  const status = String(threadViewStatus(props.sessionId) || session.value?.runtime_status || '');
  return Boolean(chatStore.loadingBySession?.[props.sessionId]) ||
    ['accepted', 'running', 'queued', 'waiting_input', 'waiting_approval'].includes(status);
});
const rows = computed(() => {
  const slots = threadViewSlots(props.sessionId);
  if (threadViewCommitted(props.sessionId) && slots) {
    // 与主时间线同一口径：停在输入区排队条里的轮次不进入聊天记录。
    const queuedItems = chatQueueForSession(props.sessionId).items;
    return slots
      .filter((slot) => !isParkedQueueTurn(queuedItems, slot.user, slot.assistant))
      .map((slot, index) => ({
        key: slot.key,
        rootTurnId: slot.rootTurnId,
        kind: 'turn' as const,
        user: { key: String(slot.user.id || `${slot.key}:user`), sourceIndex: index * 2, message: slot.user },
        assistant: { key: String(slot.assistant.id || `${slot.key}:assistant`), sourceIndex: index * 2 + 1, message: slot.assistant }
      }));
  }
  return buildTranscriptTurnRows(getSessionMessages(props.sessionId));
});
const contextMessages = computed(() => rows.value.flatMap((row) => [row.user.message, row.assistant.message]));
const draftKey = computed(() =>
  `messenger:agent:${props.controller.resolveCurrentUserScope()}:conversation:${props.sessionId}`
);

const load = async () => {
  const id = props.sessionId;
  const serial = ++loadSerial.value;
  if (!id) return;
  try {
    await openThreadViewDetached(chatStore, id);
  } catch {
    // threadViewStore owns the visible error state.
  }
  if (serial === loadSerial.value) await nextTick(scrollToBottom);
};
const scrollToBottom = () => {
  const element = messageList.value;
  if (element && followOutput.value) element.scrollTop = element.scrollHeight;
};
const handleScroll = () => {
  const element = messageList.value;
  if (!element) return;
  followOutput.value = element.scrollHeight - element.scrollTop - element.clientHeight < 48;
};
const applyModel = async (payload: Record<string, unknown>) => {
  await props.controller.openSessionById(props.sessionId);
  return props.controller.applyComposerModelSelection(payload);
};

watch(() => props.sessionId, (id, previous) => {
  if (previous) setThreadViewVisible(previous, false);
  if (id) setThreadViewVisible(id, true);
}, { immediate: true });
onBeforeUnmount(() => setThreadViewVisible(props.sessionId, false));
watch(() => props.sessionId, load, { immediate: true });
watch(
  () => {
    const last = rows.value.at(-1);
    return `${rows.value.length}:${String(last?.assistant.message?.content || '').length}`;
  },
  () => nextTick(scrollToBottom)
);
</script>
