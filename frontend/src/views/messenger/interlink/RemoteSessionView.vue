<template>
  <section class="interlink-remote-session" data-testid="interlink-remote-session">
    <header class="interlink-remote-session-bar">
      <span class="interlink-remote-session-badge">
        <i class="fa-solid fa-satellite-dish" aria-hidden="true"></i>
        <span>{{ t('interlink.session.bar', { label: nodeLabel }) }}</span>
      </span>
      <span class="interlink-remote-session-thread" :title="threadTitle">{{ threadTitle }}</span>
      <span class="interlink-remote-session-feed" :class="`is-${feedStatus}`">
        <span class="interlink-status-dot" :class="feedDotClass" aria-hidden="true"></span>
        {{ t(feedStatusLabelKey) }}
      </span>
      <button
        v-if="feedStatus === 'error' || feedStatus === 'closed'"
        class="interlink-remote-session-btn"
        type="button"
        @click="reconnect"
      >
        {{ t('interlink.session.reconnect') }}
      </button>
      <span class="interlink-remote-session-spacer" aria-hidden="true"></span>
      <button class="interlink-remote-session-btn" type="button" @click="emit('back-to-cloud')">
        <i class="fa-solid fa-cloud" aria-hidden="true"></i>
        {{ t('interlink.session.backToCloud') }}
      </button>
      <button
        class="interlink-remote-session-btn"
        type="button"
        :aria-label="t('common.close')"
        @click="emit('close')"
      >
        <i class="fa-solid fa-xmark" aria-hidden="true"></i>
      </button>
    </header>

    <div ref="bodyRef" class="interlink-remote-session-body" @scroll.passive="handleBodyScroll">
      <div v-if="!messages.length" class="interlink-remote-session-empty">
        {{ emptyLabel }}
      </div>
      <article
        v-for="message in messages"
        :key="message.id"
        class="interlink-remote-message"
        :class="`is-${message.role}`"
      >
        <div class="interlink-remote-message-head">
          <span class="interlink-remote-message-role">{{ t(`interlink.session.role.${message.role}`) }}</span>
          <span v-if="message.status" class="interlink-remote-message-status">{{ message.status }}</span>
        </div>
        <pre class="interlink-remote-message-text">{{ message.text }}</pre>
      </article>
      <div v-if="droppedFrames" class="interlink-remote-session-note">
        {{ t('interlink.session.droppedFrames', { count: droppedFrames }) }}
      </div>
      <div v-if="feedDetail" class="interlink-remote-session-note is-warn">{{ feedDetail }}</div>
    </div>

    <footer class="interlink-remote-session-compose">
      <div v-if="sendState.text" class="interlink-remote-send-state" aria-live="polite">
        <span :class="{ 'is-pending': sendState.pending }">{{ sendState.text }}</span>
        <button
          v-if="sendState.cancellable"
          class="interlink-remote-session-btn"
          type="button"
          :disabled="canceling"
          @click="cancelSend"
        >
          {{ t('common.cancel') }}
        </button>
      </div>
      <div v-if="sendBlockedReason" class="interlink-remote-send-state is-disabled">
        {{ sendBlockedReason }}
      </div>
      <textarea
        v-model="draft"
        class="interlink-remote-session-input"
        rows="2"
        :maxlength="REMOTE_MESSAGE_MAX_CHARS"
        :disabled="sendDisabled"
        :placeholder="t('interlink.session.placeholder')"
        @keydown.ctrl.enter.prevent="sendMessage"
        @keydown.meta.enter.prevent="sendMessage"
      ></textarea>
      <div class="interlink-remote-session-actions">
        <button
          class="interlink-remote-session-btn"
          type="button"
          :disabled="!!cancelThreadReason || runningAction"
          :title="cancelThreadReason || t('interlink.session.cancelThread')"
          @click="cancelThread"
        >
          {{ t('interlink.session.cancelThread') }}
        </button>
        <button
          class="interlink-remote-session-btn is-primary"
          type="button"
          :disabled="sendDisabled || sending"
          @click="sendMessage"
        >
          {{ sending ? t('interlink.session.sending') : t('interlink.session.send') }}
        </button>
      </div>
    </footer>
  </section>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';

import type { InterlinkNode, InterlinkShadowThread } from '@/api/interlink';
import { useI18n } from '@/i18n';
import {
  DISABLED_REASON_KEY,
  resolveDisabledReason
} from './interlinkNodeModel';
import {
  cancelInterlinkCommandRun,
  runInterlinkCommand
} from './interlinkCommandRunner';
import { createRemoteSessionFeed } from './remoteSessionFeed';
import type { RemoteFeedStatus } from './remoteSessionFeed';
import { applyRemoteFrame, createRemoteSessionState } from './remoteSessionModel';
import type { RemoteSessionState } from './remoteSessionModel';

/** 发送框输入上限：远程会话是控制通道，不是文档编辑器。 */
const REMOTE_MESSAGE_MAX_CHARS = 4000;

const props = defineProps<{
  node: InterlinkNode | null;
  target: string;
  thread: InterlinkShadowThread;
}>();

const emit = defineEmits<{
  close: [];
  'back-to-cloud': [];
}>();

const { t } = useI18n();

const state: RemoteSessionState = reactive(createRemoteSessionState());
const bodyRef = ref<HTMLElement | null>(null);
const feedStatus = ref<RemoteFeedStatus>('idle');
const feedDetail = ref('');
const draft = ref('');
const sending = ref(false);
const canceling = ref(false);
const runningAction = ref(false);
const sendState = reactive({ text: '', pending: false, cancellable: false, commandId: '' });

let feed: ReturnType<typeof createRemoteSessionFeed> | null = null;
let sendController: AbortController | null = null;
// 用户上滚查看历史时不再被新消息拽回底部。
let pinnedToBottom = true;

const messages = computed(() => state.messages);
const droppedFrames = computed(() => state.droppedFrames);
const nodeLabel = computed(() => props.node?.label || props.node?.node_id || props.target);
const threadTitle = computed(
  () => props.thread?.title || props.thread?.local_thread_id || t('interlink.session.untitledThread')
);
const feedDotClass = computed(() =>
  feedStatus.value === 'open'
    ? 'is-online'
    : feedStatus.value === 'reconnecting' || feedStatus.value === 'connecting'
      ? 'is-reconnecting'
      : feedStatus.value === 'error'
        ? 'is-busy'
        : 'is-offline'
);
const feedStatusLabelKey = computed(() => `interlink.session.feed.${feedStatus.value}`);

const emptyLabel = computed(() => {
  if (feedStatus.value === 'connecting' || feedStatus.value === 'reconnecting') {
    return t('interlink.session.waitingSnapshot');
  }
  if (!state.gotSnapshot) return t('interlink.session.noSnapshot');
  return t('interlink.session.emptyTranscript');
});

const sendBlockedReason = computed(() => {
  const reason = resolveDisabledReason(props.node, 'thread.message');
  if (!reason) return '';
  return t(DISABLED_REASON_KEY[reason]);
});

const sendDisabled = computed(() => Boolean(sendBlockedReason.value));

const cancelThreadReason = computed(() => {
  const reason = resolveDisabledReason(props.node, 'thread.cancel');
  if (!reason) return '';
  return t(DISABLED_REASON_KEY[reason]);
});

const scrollToSelector = () => {
  if (!pinnedToBottom) return;
  const element = bodyRef.value;
  if (!element) return;
  element.scrollTop = element.scrollHeight;
};

const handleBodyScroll = () => {
  const element = bodyRef.value;
  if (!element) return;
  const distance = element.scrollHeight - element.scrollTop - element.clientHeight;
  pinnedToBottom = distance < 48;
};

const teardownFeed = (): void => {
  feed?.stop();
  feed = null;
};

const startFeed = (): void => {
  teardownFeed();
  if (!props.target || !props.thread?.local_thread_id) return;
  feedDetail.value = '';
  feed = createRemoteSessionFeed({
    target: props.target,
    threadId: props.thread.local_thread_id,
    onFrame: (frame) => {
      applyRemoteFrame(state, frame);
      void nextTick(scrollToSelector);
    },
    onStatus: (status, detail) => {
      feedStatus.value = status;
      feedDetail.value = detail && status === 'error' ? t('interlink.session.feedError') : '';
    }
  });
  feed.start();
};

const reconnect = (): void => {
  state.lastSeq = -1;
  startFeed();
};

const resetSendState = (): void => {
  sendState.text = '';
  sendState.pending = false;
  sendState.cancellable = false;
  sendState.commandId = '';
};

const cancelSend = async (): Promise<void> => {
  if (!sendState.commandId || canceling.value) return;
  canceling.value = true;
  sendController?.abort();
  await cancelInterlinkCommandRun(sendState.commandId);
  canceling.value = false;
  sending.value = false;
  sendState.text = t('interlink.session.sendCanceled');
  sendState.pending = false;
  sendState.cancellable = false;
};

const sendMessage = async (): Promise<void> => {
  const text = draft.value.trim();
  if (!text || sending.value || sendDisabled.value) return;
  sending.value = true;
  resetSendState();
  const controller = new AbortController();
  sendController = controller;
  sendState.pending = true;
  sendState.cancellable = true;
  const outcome = await runInterlinkCommand({
    to: props.target,
    kind: 'thread.message',
    args: {
      local_thread_id: props.thread.local_thread_id,
      agent: props.thread.agent || 'default',
      message: text
    },
    signal: controller.signal,
    onState: ({ submit, record }) => {
      sendState.commandId = record?.command_id || submit.command_id;
      const approval = String(record?.approval_state || submit.approval_state || '');
      if (approval === 'pending') {
        sendState.text = t('interlink.session.approvalPending');
        return;
      }
      const status = String(record?.status || submit.status || '');
      if (status === 'queued') {
        sendState.text = t('interlink.session.statusQueued');
        return;
      }
      if (status === 'issued' || status === 'acked' || status === 'running') {
        sendState.text = t(`interlink.session.status.${status}`);
      }
    }
  });

  sendState.pending = false;
  sendState.cancellable = false;
  sending.value = false;
  sendController = null;

  if (outcome.status === 'succeeded') {
    draft.value = '';
    sendState.text = t('interlink.session.sendAccepted');
    return;
  }
  sendState.text =
    outcome.errorSummary ||
    (outcome.errorCode
      ? t('interlink.session.sendFailed', { code: outcome.errorCode })
      : t('interlink.session.sendFailedGeneric'));
};

const cancelThread = async (): Promise<void> => {
  if (runningAction.value || cancelThreadReason.value) return;
  runningAction.value = true;
  const outcome = await runInterlinkCommand({
    to: props.target,
    kind: 'thread.cancel',
    args: { local_thread_id: props.thread.local_thread_id }
  });
  runningAction.value = false;
  sendState.text =
    outcome.status === 'succeeded'
      ? t('interlink.session.threadCancelSent')
      : outcome.errorSummary || t('interlink.session.sendFailedGeneric');
  sendState.pending = false;
  sendState.cancellable = false;
};

onMounted(() => {
  startFeed();
});

onBeforeUnmount(() => {
  teardownFeed();
  sendController?.abort();
  sendController = null;
});

// 切换远程线程＝换订阅目标：旧通道立刻关，正文不留在组件外（§7.4）。
watch(
  () => [props.target, props.thread?.local_thread_id || ''] as const,
  () => {
    Object.assign(state, createRemoteSessionState());
    resetSendState();
    pinnedToBottom = true;
    startFeed();
  }
);
</script>
