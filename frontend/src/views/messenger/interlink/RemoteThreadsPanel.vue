<template>
  <div class="remote-thread-controls">
    <button type="button" @click="open">{{ text('设备线程', 'Device threads') }}</button>
    <span v-if="node?.meta?.sharing === 'minimal'">{{ text('摘要共享', 'Summary sharing') }}</span>
  </div>
  <el-dialog v-model="visible" :title="text('设备线程', 'Device threads')" width="760px" destroy-on-close @closed="stop">
    <div class="remote-thread-toolbar">
      <button type="button" :disabled="busy" @click="reload">{{ text('刷新', 'Refresh') }}</button>
      <select v-model="selected" :disabled="busy" @change="loadThread">
        <option value="">{{ text('新线程', 'New thread') }}</option>
        <option v-for="thread in threads" :key="thread.local_thread_id" :value="thread.local_thread_id">{{ thread.title || thread.local_thread_id }}</option>
      </select>
      <button type="button" :disabled="!selected || busy" @click="drive('thread.cancel')">{{ text('停止线程', 'Stop thread') }}</button>
    </div>
    <p role="status">{{ status }}</p>
    <pre class="remote-thread-history">{{ history }}</pre>
    <textarea v-model="message" :placeholder="text('向此设备发送消息', 'Send a message to this device')" rows="3" maxlength="32000"></textarea>
    <div class="remote-thread-toolbar">
      <button type="button" :disabled="busy || !message.trim()" @click="drive('thread.message')">{{ text('发送', 'Send') }}</button>
      <button v-if="busy" type="button" @click="abort?.abort()">{{ text('取消操作', 'Cancel operation') }}</button>
      <button type="button" :disabled="busy || !selected" @click="answer">{{ text('处理工具审批', 'Answer tool approval') }}</button>
    </div>
    <details v-if="events.length"><summary>{{ text('实时事件', 'Live events') }}</summary><pre class="remote-thread-events">{{ events.join('\n') }}</pre></details>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import { ElMessageBox } from 'element-plus';
import { useI18n } from '@/i18n';
import type { InterlinkNode } from '@/api/interlink';
import { resolveAccessToken } from '@/api/requestAuth';
import { resolveApiBase } from '@/config/runtime';
import { runInterlinkCommand } from './interlinkCommandRunner';
import { remoteThreadModel, remoteHistoryText } from './remoteThreadModel';

const props = defineProps<{ target: string; workspaceId: string; node: InterlinkNode | null }>();
const { language: locale } = useI18n();
const text = (zh: string, en: string) => locale.value.startsWith('zh') ? zh : en;
const visible = ref(false), busy = ref(false), selected = ref(''), message = ref(''), history = ref(''), status = ref('');
const threads = ref<Array<{ local_thread_id: string; title: string }>>([]);
const events = ref<string[]>([]);
let abort: AbortController | null = null, socket: WebSocket | null = null;
let generation = 0;
let reconnect: ReturnType<typeof setTimeout> | null = null;
let render: ReturnType<typeof setTimeout> | null = null;
let retries = 0;
const target = computed(() => props.target);
function closeWatch() { if (reconnect) clearTimeout(reconnect); reconnect = null; if (render) clearTimeout(render); render = null; const previous = socket; socket = null; previous?.close(); }
function stop() { generation++; abort?.abort(); abort = null; closeWatch(); busy.value = false; }
async function command(kind: string, args: Record<string, unknown>) {
  abort = new AbortController();
  return runInterlinkCommand({ to: props.target, kind, args: { ...args, ...(props.workspaceId ? { workspace_id: props.workspaceId } : {}) }, signal: abort.signal,
    onState: ({ record }) => { status.value = record?.approval_state === 'pending' ? text('等待本地设备批准，可取消操作', 'Waiting for approval on the local device') : String(record?.status || ''); }
  });
}
async function reload() {
  if (busy.value) return;
  busy.value = true; const current = generation;
  try {
    const result = await command('threads.list', { limit: 50, offset: 0 });
    if (current !== generation) return;
    if (result.status !== 'succeeded') throw new Error(result.errorSummary || result.errorCode);
    threads.value = (Array.isArray(result.result?.threads) ? result.result.threads : []).slice(0, 50) as typeof threads.value;
    status.value = text('最近 50 个线程', 'Up to 50 recent threads');
  } catch (error) { if (current === generation) status.value = String(error); }
  finally { if (current === generation) { busy.value = false; if (selected.value) await loadThread(); } }
}
async function loadThread() {
  closeWatch(); events.value = [];
  if (!selected.value) { history.value = ''; return; }
  busy.value = true; const current = generation;
  try {
    const result = await command('threads.get', { local_thread_id: selected.value, limit: 50 });
    if (current !== generation) return;
    if (result.status !== 'succeeded') throw new Error(result.errorSummary || result.errorCode);
    history.value = remoteHistoryText(result.result?.items);
    watchThread();
  } catch (error) { if (current === generation) status.value = String(error); }
  finally { if (current === generation) busy.value = false; }
}
function watchThread() {
  const model = remoteThreadModel(selected.value);
  const base = new URL(resolveApiBase() || '/wunder', window.location.origin);
  const url = new URL(`${base.pathname.replace(/\/$/, '')}/interlink/remote_ws`, base.origin);
  url.protocol = base.protocol === 'https:' ? 'wss:' : 'ws:';
  url.searchParams.set('target', props.target); url.searchParams.set('thread', selected.value);
  const connection = new WebSocket(url, ['wunder-interlink-remote', `wunder-auth.${resolveAccessToken()}`]);
  socket = connection;
  connection.onopen = () => { status.value = text('实时连接已恢复', 'Live connection restored'); };
  connection.onmessage = event => {
    if (socket !== connection) return;
    events.value = [...events.value.slice(-79), String(event.data).slice(0, 12000)];
    try {
      const frame = JSON.parse(String(event.data));
      retries = 0;
      if (frame.type === 'close' || frame.type === 'error') {
        status.value = String(frame.reason || frame.code || '');
        if (['session_revoked','AUTH_REQUIRED','PERMISSION_DENIED','SUBSCRIBER_LIMIT'].includes(String(frame.reason || frame.code))) socket = null;
        connection.close(); return;
      }
      if (model.apply(frame)) { connection.close(); return; }
      if (!render) render = setTimeout(() => { render = null; if (socket === connection) { const text = model.text(); if (text) history.value = text; } }, 33);
    } catch { status.value = text('实时数据无效，请刷新', 'Invalid live data; refresh the thread'); }
  };
  connection.onclose = () => {
    if (socket !== connection || !visible.value) return;
    status.value = text('实时连接中断，正在重连…', 'Live connection interrupted; reconnecting…');
    reconnect = setTimeout(() => { reconnect = null; if (socket === connection && visible.value) void loadThread(); }, Math.min(30_000, 1000 * 2 ** Math.min(retries++, 5)));
  };
}
async function drive(kind: string, extra: Record<string, unknown> = {}) {
  if (busy.value) return;
  busy.value = true; const current = generation;
  let succeeded = false;
  try {
    const result = await command(kind, { ...(selected.value ? { local_thread_id: selected.value } : {}), ...(kind === 'thread.message' ? { message: message.value } : {}), ...extra });
    if (current !== generation) return;
    if (result.status !== 'succeeded') throw new Error(result.errorSummary || result.errorCode);
    if (kind === 'thread.message') { message.value = ''; selected.value = String(result.result?.local_thread_id || selected.value); }
    status.value = text('操作成功', 'Completed');
    succeeded = true;
  } catch (error) { if (current === generation) status.value = String(error); }
  finally { if (current === generation) { busy.value = false; if (succeeded && selected.value) await loadThread(); } }
}
async function answer() {
  try {
    const { value } = await ElMessageBox.prompt(text('输入待处理的工具审批 ID', 'Enter the pending tool approval ID'), text('工具审批', 'Tool approval'));
    if (!value.trim()) return;
    let decision = 'approve_once';
    try {
      await ElMessageBox.confirm(text('允许此工具执行一次？也可以明确拒绝。', 'Allow this tool once, or explicitly reject it.'), text('确认', 'Confirm'), { confirmButtonText: text('允许一次', 'Allow once'), cancelButtonText: text('拒绝', 'Reject'), distinguishCancelAndClose: true });
    } catch (action) { if (action !== 'cancel') return; decision = 'deny'; }
    await drive('thread.answer', { approval_id: value.trim(), decision });
  } catch { /* Dialog canceled. */ }
}
function open() { visible.value = true; void reload(); }
watch([target, () => props.workspaceId], () => { stop(); visible.value = false; selected.value = ''; history.value = ''; });
onBeforeUnmount(stop);
</script>

<style scoped>
.remote-thread-controls,.remote-thread-toolbar { display:flex; gap:8px; align-items:center; padding:6px; }
.remote-thread-controls span { font-size:11px; opacity:.65; }
.remote-thread-history { max-height:340px; overflow:auto; white-space:pre-wrap; background:#f5f7fa; padding:12px; }
.remote-thread-events { max-height:180px; overflow:auto; white-space:pre-wrap; }
textarea { width:100%; box-sizing:border-box; }
</style>
