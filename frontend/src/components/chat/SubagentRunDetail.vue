<template>
  <div class="subagent-run-detail">
    <p v-if="loading" role="status">正在加载运行过程…</p>
    <div v-if="error" role="alert" class="subagent-run-detail__error">
      {{ error }} <button type="button" @click="reload++">重试</button>
    </div>
    <p v-if="!loading && !error && !assistant" class="subagent-run-detail__hint">
      等待子智能体开始执行…
    </p>
    <template v-else-if="assistant">
      <p v-if="userTask" class="subagent-run-detail__task" :title="userTask">{{ userTask }}</p>
      <!-- 与主时间线同一套块渲染（正文 + 思考/工具批次），不做第二套运行记录形态。 -->
      <MessageTimelineBlocks
        :blocks="assistant.timeline ?? []"
        :workflow-items="assistant.workflowItems ?? []"
        :session-id="sessionId"
        :identity-key="identityKey"
        :default-open="true"
        :streaming="streaming"
        :content-version="revision"
        :cache-key-prefix="`subagent:${sessionId}:${runId}:`"
        :message="assistant"
        :resolve-workspace-path="resolveWorkspacePath"
        :body-text-transform="transformBodyText"
      />
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, shallowRef, watch } from 'vue';

import MessageTimelineBlocks from './MessageTimelineBlocks.vue';
import { getThreadLogSnapshot } from '@/api/chat';
import { chatWsClient } from '@/stores/chatWatcher';
import { emptyChatThreadState } from '@/realtime/chat/chatThreadTypes';
import { applyChatThreadSnapshot, applyChatThreadFrame } from '@/realtime/chat/chatThreadState';
import { toChatThreadFrame } from '@/realtime/chat/chatThreadRuntime';
import { buildChatThreadTurnSlots } from '@/realtime/chat/chatThreadProjection';
import type { ChatRuntimeMessageProjection } from '@/realtime/chat/chatRuntimeTypes';
import { buildAssistantDisplayContent } from '@/utils/assistantFailureNotice';
import { t } from '@/i18n';

const props = defineProps<{ sessionId: string; runId: string; turnId?: string }>();

const assistant = shallowRef<ChatRuntimeMessageProjection | null>(null);
const userTask = ref('');
const revision = ref(0);
const loading = ref(true);
const error = ref('');
const reload = ref(0);

const identityKey = computed(() => `${props.sessionId}:${props.runId}:${props.turnId || ''}`);
const streaming = computed(() => {
  const current = assistant.value;
  return Boolean(current && !current.final);
});
/** 失败提示是整轮信息，只挂在最后一段正文上（与主时间线同源）。 */
const transformBodyText = (text: string, isLast: boolean): string =>
  isLast && assistant.value
    ? buildAssistantDisplayContent(assistant.value as Record<string, unknown>, t, text)
    : text;
const resolveWorkspacePath = (rawPath: string): string => String(rawPath || '');

watch(() => [props.sessionId, props.runId, props.turnId, reload.value], (_, __, cleanup) => {
  const state = emptyChatThreadState(props.sessionId);
  const controller = new AbortController();
  let subscription: AbortController | null = null;
  let renderTimer: ReturnType<typeof setTimeout> | undefined;
  let retryTimer: ReturnType<typeof setTimeout> | undefined;
  let recovery = false;
  assistant.value = null;
  userTask.value = '';
  loading.value = true;
  error.value = '';
  const render = () => {
    renderTimer = undefined;
    if (controller.signal.aborted) return;
    const turnId = props.turnId || [...state.turns.values()].at(-1)?.turnId || '';
    const slots = buildChatThreadTurnSlots(state);
    const slot = slots.find(entry => entry.rootTurnId === turnId) ?? slots.at(-1) ?? null;
    assistant.value = slot?.assistant ?? null;
    userTask.value = String(slot?.user?.content || '');
    revision.value++;
  };
  const scheduleRender = () => { renderTimer ??= setTimeout(render, 24); };
  const connect = async (snapshot: boolean) => {
    if (controller.signal.aborted) return;
    recovery = false;
    try {
      if (snapshot) {
        const response = await getThreadLogSnapshot(props.sessionId, { signal: controller.signal });
        if (controller.signal.aborted) return;
        applyChatThreadSnapshot(state, response.data?.data ?? response.data, Date.now());
        render();
      }
      loading.value = false;
      error.value = '';
      const current = new AbortController();
      subscription = current;
      const requestId = `subagent-detail-${crypto.randomUUID()}`;
      await chatWsClient.request({ requestId, signal: current.signal, closeOnFinal: false,
        // Only the request ID is cancelled: this is a watch task, never a run.
        message: () => ({ type: 'watch', request_id: requestId, session_id: props.sessionId,
          payload: { after_change_seq: state.lastSeq } }),
        onEvent: (event, text) => {
          if (controller.signal.aborted || subscription !== current) return;
          if (event === 'error') { error.value = '运行过程连接失败，请重试'; current.abort(); return; }
          if (['thread_snapshot_required', 'stream_overflow'].includes(event)) {
            recovery = true; current.abort(); return;
          }
          try {
            const frame = toChatThreadFrame(event, JSON.parse(text));
            if (!frame || frame.event === 'stream_started') return;
            const result = applyChatThreadFrame(state, frame, Date.now());
            if (result.changed) scheduleRender();
            if (result.needResume || state.gap.length) { recovery = true; current.abort(); }
          } catch { error.value = '运行过程数据读取失败，请重试'; current.abort(); }
        }
      });
      recovery = true;
    } catch (cause) {
      if (controller.signal.aborted) return;
      if (!recovery) error.value = '运行过程暂时无法加载，请重试';
    } finally {
      loading.value = false;
      if (!controller.signal.aborted && recovery) retryTimer = setTimeout(() => void connect(true), 800);
    }
  };
  cleanup(() => {
    controller.abort(); subscription?.abort();
    clearTimeout(renderTimer); clearTimeout(retryTimer);
  });
  void connect(true);
}, { immediate: true });
</script>

<style scoped>
.subagent-run-detail { min-height: 100px; }
.subagent-run-detail__task {
  margin: 0 0 4px;
  padding: 0 24px;
  color: var(--chat-text-secondary, #6b7280);
  font-size: 12px;
  line-height: 1.5;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
  word-break: break-word;
}
.subagent-run-detail__hint { color: var(--chat-text-secondary, #6b7280); font-size: 12px; }
.subagent-run-detail__error { color: #b42318; }
</style>
