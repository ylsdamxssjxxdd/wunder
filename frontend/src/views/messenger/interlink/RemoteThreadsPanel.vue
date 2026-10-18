<template>
  <section class="remote-thread-panel is-inline">
    <div class="remote-thread-context">
      <span>{{ text('本地工作线程', 'Local work thread') }}</span>
      <button type="button" :disabled="busy" :title="text('刷新线程', 'Refresh thread')" @click="reload">
        <i class="fa-solid fa-rotate" :class="{ 'fa-spin': busy }" aria-hidden="true"></i>
      </button>
    </div>
    <div ref="historyRef" class="remote-thread-messages" aria-live="polite">
      <div v-if="busy && !messages.length" class="remote-thread-empty">{{ text('正在加载线程…', 'Loading thread…') }}</div>
      <div v-else-if="!messages.length" class="remote-thread-empty">{{ text('还没有消息，发送第一条消息开始工作。', 'No messages yet. Send the first message to start.') }}</div>
      <article v-for="item in messages" :key="item.id" class="messenger-message remote-thread-message" :class="{ mine: item.role === 'user' }">
        <div class="messenger-message-main">
          <div class="messenger-message-bubble" :class="item.role === 'user' ? '' : 'messenger-markdown'">
            <MessageMarkdownBody v-if="item.role !== 'user'" :cache-key="`interlink:${props.target}:${props.workspaceId}:${props.openThreadId}:${item.id}`" :content="item.content" :streaming="false" :explicit-content="true" />
            <span v-else class="remote-thread-user-content">{{ item.content }}</span>
          </div>
        </div>
      </article>
    </div>
    <p v-if="status" class="remote-thread-status" role="status">{{ status }}</p>
    <form class="remote-thread-composer input-container input-container--world" @submit.prevent="send">
      <div class="input-box input-box--world"><textarea v-model="message" class="composer-textarea" :disabled="busy || !selected" :placeholder="text('向本地线程发送消息', 'Send a message to this local thread')" rows="1" maxlength="32000" @keydown="handleInputKeydown"></textarea></div>
      <div class="composer-action-row"><span class="remote-thread-target">{{ text('本地线程', 'Local thread') }}</span><button class="composer-send-btn" data-testid="remote-thread-send" type="submit" :disabled="busy || !selected || !message.trim()" :title="text('发送', 'Send')" :aria-label="text('发送', 'Send')"><i class="fa-solid fa-arrow-up composer-send-icon" aria-hidden="true"></i></button></div>
    </form>
  </section>
</template>

<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, watch } from 'vue';
import { useI18n } from '@/i18n';
import { resolveAccessToken } from '@/api/requestAuth';
import { resolveApiBase } from '@/config/runtime';
import { runInterlinkCommand } from './interlinkCommandRunner';
import { remoteThreadModel, remoteHistoryItems, type RemoteThreadItem } from './remoteThreadModel';
import MessageMarkdownBody from '@/components/chat/MessageMarkdownBody.vue';

const props = withDefaults(defineProps<{ target: string; workspaceId: string; openThreadId?: string }>(), { openThreadId: '' });
const { language: locale } = useI18n();
const text = (zh: string, en: string) => locale.value.startsWith('zh') ? zh : en;
const selected = ref('');
const busy = ref(false);
const message = ref('');
const messages = ref<RemoteThreadItem[]>([]);
const status = ref('');
const historyRef = ref<HTMLElement | null>(null);
let abort: AbortController | null = null, socket: WebSocket | null = null;
let generation = 0, reconnect: ReturnType<typeof setTimeout> | null = null, render: ReturnType<typeof setTimeout> | null = null, retries = 0;

function closeWatch() { if (reconnect) clearTimeout(reconnect); reconnect = null; if (render) clearTimeout(render); render = null; const old = socket; socket = null; old?.close(); }
function stop() { generation += 1; abort?.abort(); abort = null; closeWatch(); busy.value = false; }
function setMessages(next: RemoteThreadItem[]) {
  const follow = !historyRef.value || historyRef.value.scrollHeight - historyRef.value.scrollTop - historyRef.value.clientHeight < 80;
  messages.value = next;
  if (follow) void nextTick(() => { const el = historyRef.value; if (el) el.scrollTop = el.scrollHeight; });
}
async function command(kind: string, args: Record<string, unknown>) {
  abort = new AbortController();
  return runInterlinkCommand({ to: props.target, kind, args: { ...args, ...(props.workspaceId ? { workspace_id: props.workspaceId } : {}) }, signal: abort.signal });
}
async function reload() {
  if (busy.value || !selected.value) return;
  busy.value = true; const current = generation;
  try {
    const result = await command('threads.get', { local_thread_id: selected.value, limit: 100 });
    if (current !== generation) return;
    if (result.status !== 'succeeded') throw new Error(result.errorSummary || result.errorCode);
    setMessages(remoteHistoryItems(result.result?.items)); status.value = ''; watchThread();
  } catch (error) { if (current === generation) status.value = String(error); }
  finally { if (current === generation) busy.value = false; }
}
function watchThread() {
  closeWatch(); const model = remoteThreadModel(selected.value); const base = new URL(resolveApiBase() || '/wunder', window.location.origin);
  const url = new URL(`${base.pathname.replace(/\/$/, '')}/interlink/remote_ws`, base.origin); url.protocol = base.protocol === 'https:' ? 'wss:' : 'ws:'; url.searchParams.set('target', props.target); url.searchParams.set('thread', selected.value);
  const connection = new WebSocket(url, ['wunder-interlink-remote', `wunder-auth.${resolveAccessToken()}`]); socket = connection;
  connection.onopen = () => { status.value = ''; retries = 0; };
  connection.onmessage = event => { if (socket !== connection) return; try { const frame = JSON.parse(String(event.data)); if (frame.type === 'close' || frame.type === 'error') { status.value = String(frame.reason || frame.code || ''); connection.close(); return; } if (model.apply(frame)) { void reload(); return; } if (!render) render = setTimeout(() => { render = null; if (socket === connection) setMessages(model.items()); }, 33); } catch { status.value = text('实时数据无效，请刷新线程', 'Invalid live data; refresh the thread'); } };
  connection.onclose = () => { if (socket !== connection) return; status.value = text('连接中断，正在恢复…', 'Connection interrupted; reconnecting…'); reconnect = setTimeout(() => { reconnect = null; if (socket === connection && selected.value) void reload(); }, Math.min(30_000, 1000 * 2 ** Math.min(retries++, 5))); };
}
async function send() {
  if (busy.value || !selected.value || !message.value.trim()) return;
  busy.value = true; const current = generation; const content = message.value.trim();
  try { const result = await command('thread.message', { local_thread_id: selected.value, message: content }); if (current !== generation) return; if (result.status !== 'succeeded') throw new Error(result.errorSummary || result.errorCode); message.value = ''; status.value = ''; busy.value = false; await reload(); }
  catch (error) { if (current === generation) status.value = String(error); }
  finally { if (current === generation) busy.value = false; }
}
function handleInputKeydown(event: KeyboardEvent) { if (event.key === 'Enter' && !event.shiftKey) { event.preventDefault(); void send(); } }
watch(() => props.openThreadId, id => { stop(); selected.value = String(id || '').trim(); messages.value = []; status.value = ''; if (selected.value) void reload(); }, { immediate: true });
onBeforeUnmount(stop);
</script>

<style scoped>
.remote-thread-panel.is-inline { display:flex; flex-direction:column; height:100%; min-height:0; padding:20px 28px 16px; box-sizing:border-box; background:var(--mz-bg,#fff); }
.remote-thread-context,.remote-thread-composer,.remote-thread-messages,.remote-thread-status { width:min(900px,100%); margin-left:auto; margin-right:auto; }
.remote-thread-context { display:flex; align-items:center; justify-content:space-between; min-height:28px; color:var(--mz-text-muted,#7b8494); font-size:12px; }
.remote-thread-context button { border:0; background:transparent; color:inherit; cursor:pointer; padding:4px 6px; }
.remote-thread-messages { flex:1; min-height:0; overflow:auto; padding:16px 0 24px; box-sizing:border-box; }
.remote-thread-message { margin:0 0 18px; }.remote-thread-message.mine .messenger-message-main { display:flex; justify-content:flex-end; }.remote-thread-message .messenger-message-bubble { max-width:min(760px,92%); }.remote-thread-user-content { white-space:pre-wrap; overflow-wrap:anywhere; }.remote-thread-empty { display:grid; place-items:center; min-height:160px; color:var(--mz-text-muted,#7b8494); font-size:13px; text-align:center; }.remote-thread-status { flex:0 0 auto; margin-top:0; margin-bottom:6px; color:var(--mz-text-muted,#7b8494); font-size:12px; }.remote-thread-composer { flex:0 0 auto; }.remote-thread-composer .input-box { width:100%; }.remote-thread-composer .composer-textarea { min-height:44px; max-height:180px; resize:vertical; }.remote-thread-composer .composer-action-row { display:flex; align-items:center; justify-content:flex-end; min-height:38px; }.remote-thread-target { margin-right:10px; color:var(--mz-text-muted,#7b8494); font-size:12px; }
</style>
