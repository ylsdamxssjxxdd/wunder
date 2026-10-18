<template>
  <!-- The interlink layer owns data only; MessengerView supplies the normal chat shell. -->
  <section class="messenger-message-panel remote-thread-panel" aria-live="polite">
    <div v-if="busy && !messages.length" class="messenger-chat-empty">{{ text('正在加载线程…', 'Loading thread…') }}</div>
    <div v-else-if="!messages.length" class="messenger-chat-empty-state remote-thread-empty">
      <div class="messenger-chat-empty-icon"><i class="fa-regular fa-comments" aria-hidden="true"></i></div>
      <div class="messenger-chat-empty-title">{{ text('还没有消息，发送第一条消息开始工作。', 'No messages yet. Send the first message to start.') }}</div>
    </div>
    <MessengerTurnRow v-for="slot in slots" :key="slot.key" :controller="props.controller" :turn="slot" />
    <p v-if="status" class="remote-thread-status" role="status">{{ status }}</p>
  </section>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';
import { useI18n } from '@/i18n';
import { resolveAccessToken } from '@/api/requestAuth';
import { resolveApiBase } from '@/config/runtime';
import { runInterlinkCommand } from './interlinkCommandRunner';
import { remoteThreadModel, remoteHistoryItems, type RemoteThreadItem } from './remoteThreadModel';
import MessengerTurnRow from '@/views/messenger/sections/MessengerTurnRow.vue';
import type { MessengerControllerContext } from '@/views/messenger/controller/messengerControllerContext';
import { buildChatThreadTurnSlots } from '@/realtime/chat/chatThreadProjection';

type RemoteThreadAttachment = {
  name?: unknown;
  content?: unknown;
  mime_type?: unknown;
  public_path?: unknown;
};

const props = withDefaults(defineProps<{ target: string; workspaceId: string; openThreadId?: string; controller: MessengerControllerContext }>(), { openThreadId: '' });
const { language: locale } = useI18n();
const text = (zh: string, en: string) => locale.value.startsWith('zh') ? zh : en;
const selected = ref('');
const historyLoading = ref(false);
const sending = ref(false);
const busy = computed(() => historyLoading.value || sending.value);
const messages = ref<RemoteThreadItem[]>([]);
const renderVersion = ref(0);
const slots = computed(() => {
  void renderVersion.value;
  if (!activeModel) return [];
  return buildChatThreadTurnSlots(activeModel.state()).map((slot, index) => ({
    key: slot.key,
    rootTurnId: slot.rootTurnId,
    user: { key: `${slot.key}:user`, sourceIndex: index * 2, message: normalizeProjectionMessage(slot.user) },
    assistant: { key: `${slot.key}:assistant`, sourceIndex: index * 2 + 1, message: normalizeProjectionMessage(slot.assistant) }
  }));
});

const normalizeProjectionMessage = (message: Record<string, unknown>): Record<string, any> => {
  // MessengerAgentBubble consumes the legacy runtime field names while the
  // v2 projection uses camelCase. Preserve both so the native renderer can be
  // reused without a second local-only template.
  const raw = (message.raw && typeof message.raw === 'object') ? message.raw as Record<string, unknown> : {};
  return {
    ...raw,
    ...message,
    role: message.role,
    content: String(message.content || ''),
    reasoning: String(message.reasoning || ''),
    status: String(message.status || ''),
    workflowItems: message.workflowItems || [],
    timeline: message.timeline || [],
    user_turn_id: message.userTurnId || raw.user_turn_id || '',
    model_turn_id: message.modelTurnId || raw.model_turn_id || '',
    root_turn_id: raw.root_turn_id || message.userTurnId || '',
    item_id: raw.item_id || message.id || ''
  };
};
const status = ref('');
let commandAbort: AbortController | null = null;
let historyAbort: AbortController | null = null;
let socket: WebSocket | null = null;
let generation = 0;
let historyRequest = 0;
let reconnect: ReturnType<typeof setTimeout> | null = null;
let render: ReturnType<typeof setTimeout> | null = null;
let retries = 0;
let activeModel: ReturnType<typeof remoteThreadModel> | null = null;

const closeWatch = () => {
  if (reconnect) clearTimeout(reconnect);
  reconnect = null;
  if (render) clearTimeout(render);
  render = null;
  const old = socket;
  socket = null;
  old?.close();
};
const stop = () => {
  generation += 1;
  historyRequest += 1;
  commandAbort?.abort();
  commandAbort = null;
  historyAbort?.abort();
  historyAbort = null;
  closeWatch();
  historyLoading.value = false;
  sending.value = false;
};
const setMessages = (next: RemoteThreadItem[]) => {
  const el = document.querySelector<HTMLElement>('[data-testid="messenger-message-list"]');
  const follow = !el || el.scrollHeight - el.scrollTop - el.clientHeight < 120;
  messages.value = next;
  void nextTick(() => {
    if (follow && el) el.scrollTop = el.scrollHeight;
  });
};
async function command(kind: string, args: Record<string, unknown>) {
  commandAbort = new AbortController();
  return runInterlinkCommand({ to: props.target, kind, args: { ...args, ...(props.workspaceId ? { workspace_id: props.workspaceId } : {}) }, signal: commandAbort.signal });
}
async function loadHistory() {
  if (!selected.value) return;
  historyAbort?.abort();
  const controller = new AbortController();
  historyAbort = controller;
  historyLoading.value = true;
  const current = generation;
  const request = ++historyRequest;
  try {
    const result = await runInterlinkCommand({
      to: props.target,
      kind: 'threads.get',
      args: { local_thread_id: selected.value, limit: 100, ...(props.workspaceId ? { workspace_id: props.workspaceId } : {}) },
      signal: controller.signal
    });
    if (current !== generation || request !== historyRequest) return;
    if (result.status !== 'succeeded') throw new Error(result.errorSummary || result.errorCode);
    const snapshot = result.result?.snapshot;
    if (activeModel && snapshot && activeModel.replace(snapshot)) {
      renderVersion.value += 1;
      setMessages(activeModel.items());
    } else {
      setMessages(remoteHistoryItems(result.result?.items));
    }
    status.value = '';
    await nextTick();
    watchThread();
  } catch (error) {
    if (current === generation && request === historyRequest && !controller.signal.aborted) status.value = String(error);
  } finally {
    if (current === generation && request === historyRequest) historyLoading.value = false;
  }
}
function watchThread() {
  closeWatch();
  const model = activeModel || remoteThreadModel(selected.value);
  activeModel = model;
  const base = new URL(resolveApiBase() || '/wunder', window.location.origin);
  const url = new URL(`${base.pathname.replace(/\/$/, '')}/interlink/remote_ws`, base.origin);
  url.protocol = base.protocol === 'https:' ? 'wss:' : 'ws:';
  url.searchParams.set('target', props.target);
  url.searchParams.set('thread', selected.value);
  const connection = new WebSocket(url, ['wunder-interlink-remote', `wunder-auth.${resolveAccessToken()}`]);
  socket = connection;
  connection.onopen = () => { status.value = ''; retries = 0; };
  connection.onmessage = event => {
    if (socket !== connection) return;
    try {
      const frame = JSON.parse(String(event.data));
      if (frame.type === 'close' || frame.type === 'error') { status.value = String(frame.reason || frame.code || ''); connection.close(); return; }
      if (model.apply(frame)) {
        renderVersion.value += 1;
        if (!render) render = setTimeout(() => { render = null; if (socket === connection) setMessages(model.items()); }, 33);
      }
    } catch { status.value = text('实时数据无效，请刷新线程', 'Invalid live data; refresh the thread'); }
  };
  connection.onclose = () => {
    if (socket !== connection) return;
    status.value = text('连接中断，正在恢复…', 'Connection interrupted; reconnecting…');
    reconnect = setTimeout(() => { reconnect = null; if (socket === connection && selected.value) void loadHistory(); }, Math.min(30_000, 1000 * 2 ** Math.min(retries++, 5)));
  };
}
function remoteAttachments(value: unknown): Array<{ name?: string; content?: string; mime_type?: string; public_path?: string }> {
  if (!Array.isArray(value)) return [];
  return value.slice(0, 8).flatMap((entry) => {
    if (!entry || typeof entry !== 'object') return [];
    const source = entry as RemoteThreadAttachment;
    const name = String(source.name || '').trim().slice(0, 256);
    const content = String(source.content || '');
    const mimeType = String(source.mime_type || '').trim().slice(0, 128);
    const publicPath = String(source.public_path || '').trim();
    if (!name || (!content.trim() && !publicPath)) return [];
    const attachment: { name?: string; content?: string; mime_type?: string; public_path?: string } = { name };
    if (content) attachment.content = content;
    if (mimeType) attachment.mime_type = mimeType;
    if (publicPath) attachment.public_path = publicPath;
    return [attachment];
  });
}
async function sendMessage(content: string, attachments: unknown[] = []): Promise<boolean> {
  const normalized = String(content || '').trim();
  const remote = remoteAttachments(attachments);
  if (busy.value || !selected.value || (!normalized && !remote.length)) return false;
  sending.value = true;
  const current = generation;
  try {
    const result = await command('thread.message', { local_thread_id: selected.value, message: normalized, ...(remote.length ? { attachments: remote } : {}) });
    if (current !== generation) return false;
    if (result.status !== 'succeeded') throw new Error(result.errorSummary || result.errorCode);
    status.value = '';
    // The command is accepted before the model finishes.  Keep the live
    // subscription open and refresh once so the user's turn appears
    // immediately; subsequent assistant deltas arrive through the socket.
    await loadHistory();
    return true;
  } catch (error) {
    if (current === generation) status.value = String(error);
    return false;
  } finally {
    if (current === generation) sending.value = false;
  }
}
async function stopMessage(): Promise<void> {
  const thread = selected.value;
  if (!thread) return;
  generation += 1;
  historyRequest += 1;
  commandAbort?.abort();
  commandAbort = null;
  historyAbort?.abort();
  historyAbort = null;
  historyLoading.value = false;
  sending.value = false;
  try {
    const result = await runInterlinkCommand({ to: props.target, kind: 'thread.cancel', args: { local_thread_id: thread, ...(props.workspaceId ? { workspace_id: props.workspaceId } : {}) } });
    if (result.status !== 'succeeded') throw new Error(result.errorSummary || result.errorCode);
    await loadHistory();
  } catch (error) { status.value = String(error); }
}
defineExpose({ messages, busy, sendMessage, stopMessage, reload: loadHistory });
watch(() => [props.target, props.workspaceId, props.openThreadId], ([target, workspaceId, id], previous) => {
  if (target === previous?.[0] && workspaceId === previous?.[1] && id === previous?.[2]) return;
  stop();
  selected.value = String(id || '').trim();
  activeModel = selected.value ? remoteThreadModel(selected.value) : null;
  renderVersion.value += 1;
  messages.value = [];
  status.value = '';
  if (selected.value) void loadHistory();
}, { immediate: true });
onBeforeUnmount(stop);
</script>

<style scoped>
.remote-thread-panel { width: min(100%, var(--mz-chat-column-width, 900px)); margin-inline: auto; }
.remote-thread-message { margin: 0; }
.remote-thread-message .messenger-message-main { min-width: 0; }
.remote-thread-message .messenger-message-bubble { max-width: min(70%, 860px); }
.remote-thread-user-content { white-space: pre-wrap; overflow-wrap: anywhere; }
.remote-thread-empty { flex: 1 1 auto; min-height: 220px; }
.remote-thread-status { margin: 8px 24px; color: var(--mz-text-muted, #7b8494); font-size: 12px; }
</style>
