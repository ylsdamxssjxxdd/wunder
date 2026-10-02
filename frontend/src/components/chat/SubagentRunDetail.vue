<template>
  <div class="subagent-run-detail">
    <p v-if="loading" role="status">正在加载运行过程…</p>
    <div v-if="error" role="alert" class="subagent-run-detail__error">
      {{ error }} <button type="button" @click="reload++">重试</button>
    </div>
    <p v-if="!loading && !error && !entries.length">等待子智能体开始执行…</p>
    <p v-if="truncated" class="subagent-run-detail__hint">显示最近 200 项运行记录</p>
    <article v-for="entry in entries" :key="entry.id" class="subagent-run-detail__entry">
      <MessageToolWorkflow v-if="entry.workflow" :items="[entry.workflow]" :visible="true"
        :session-id="sessionId" :state-key="`subagent:${runId}:${entry.id}`" :render-version="revision" />
      <template v-else>
        <header>{{ entry.title }} <span>{{ entry.status }}</span></header>
        <details v-if="entry.reasoning"><summary>思考过程</summary><pre>{{ entry.reasoning }}</pre></details>
        <pre v-if="entry.content">{{ entry.content }}</pre>
        <p v-else-if="!entry.reasoning" class="subagent-run-detail__hint">正在生成…</p>
      </template>
    </article>
  </div>
</template>

<script setup lang="ts">
import { ref, shallowRef, watch } from 'vue';
import { getThreadLogSnapshot } from '@/api/chat';
import { chatWsClient } from '@/stores/chatWatcher';
import { emptyChatThreadState } from '@/realtime/chat/chatThreadTypes';
import { applyChatThreadSnapshot, applyChatThreadFrame, composeItemText } from '@/realtime/chat/chatThreadState';
import { toChatThreadFrame } from '@/realtime/chat/chatThreadRuntime';
import { buildWorkflowRecord } from '@/realtime/chat/chatThreadProjection';
import type { ChatRuntimeWorkflowItemProjection } from '@/realtime/chat/chatRuntimeTypes';
import MessageToolWorkflow from './MessageToolWorkflow.vue';

const props = defineProps<{ sessionId: string; runId: string; turnId?: string }>();
type Entry = { id: string; title: string; status: string; content?: string; reasoning?: string;
  workflow?: ChatRuntimeWorkflowItemProjection };
const entries = shallowRef<Entry[]>([]);
const revision = ref(0);
const truncated = ref(false);
const loading = ref(true);
const error = ref('');
const reload = ref(0);
const workflowKinds = new Set(['tool_call', 'approval', 'plan', 'compaction', 'queue']);

watch(() => [props.sessionId, props.runId, props.turnId, reload.value], (_, __, cleanup) => {
  const state = emptyChatThreadState(props.sessionId);
  const controller = new AbortController();
  let subscription: AbortController | null = null;
  let renderTimer: ReturnType<typeof setTimeout> | undefined;
  let retryTimer: ReturnType<typeof setTimeout> | undefined;
  let recovery = false;
  entries.value = [];
  loading.value = true;
  error.value = '';
  const render = () => {
    renderTimer = undefined;
    if (controller.signal.aborted) return;
    const turnId = props.turnId || [...state.turns.values()].at(-1)?.turnId;
    const visible = [...state.items.values()].filter(item => item.turnId === turnId &&
      !['admin', 'model_internal'].includes(item.visibility ?? '') &&
      (workflowKinds.has(item.kind) || (item.kind === 'assistant_message' &&
        item.itemId === `${item.turnId}:text-${item.modelRound}`)));
    truncated.value = visible.length > 200;
    entries.value = visible.slice(-200).map(item => workflowKinds.has(item.kind)
      ? { id: item.itemId, title: '', status: '', workflow: buildWorkflowRecord(item, item.turnId) }
      : { id: item.itemId, title: `模型输出 · 第 ${item.modelRound} 轮`,
          status: ['completed', 'success'].includes(item.status) ? '已完成' :
            ['cancelled', 'interrupted'].includes(item.status) ? '已中断' : item.status === 'failed' ? '失败' : '生成中',
          content: composeItemText(state, item.itemId, 'content'),
          reasoning: composeItemText(state, item.itemId, 'reasoning') });
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
.subagent-run-detail__entry { margin: 12px 0; }
.subagent-run-detail__entry header { font-size: 13px; font-weight: 600; }
.subagent-run-detail__entry header span, .subagent-run-detail__hint { color: var(--chat-text-secondary, #6b7280); font-size: 12px; }
.subagent-run-detail__entry pre { white-space: pre-wrap; overflow-wrap: anywhere; font: inherit; font-size: 13px; line-height: 1.6; }
.subagent-run-detail__entry details { margin-top: 8px; }
.subagent-run-detail__error { color: #b42318; }
</style>
