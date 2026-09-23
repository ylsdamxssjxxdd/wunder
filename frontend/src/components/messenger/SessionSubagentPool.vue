<template>
  <section class="subagent-pool" :aria-label="t('chat.subagentPool.title')">
    <div class="subagent-pool-heading">
      <strong>{{ t('chat.subagentPool.title') }}</strong>
      <span>{{ items.length }}</span>
      <button type="button" :disabled="loading || !sessionId" @click="refresh">{{ t('common.refresh') }}</button>
    </div>
    <p v-if="error" role="alert">{{ error }}</p>
    <p v-else-if="!items.length">{{ t('chat.subagentPool.empty') }}</p>
    <div class="subagent-pool-items">
      <button v-for="item in items" :key="item.session_id" type="button" class="subagent-pool-item"
        @click="$emit('detail', item.session_id)">
        <span>{{ item.label || item.title || item.session_id }}</span>
        <small>{{ statusLabel(item.status) }}</small>
      </button>
    </div>
  </section>
</template>

<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { getSessionSubagents } from '@/api/chat';
import { useI18n } from '@/i18n';
import { onSubagentPoolChanged } from '@/utils/subagentPoolEvents';

const props = defineProps<{ sessionId: string }>();
defineEmits<{ (event: 'detail', id: string): void }>();
const { t } = useI18n();
const items = ref<Array<Record<string, any>>>([]);
const loading = ref(false);
const error = ref('');
let request: AbortController | null = null;
let timer: ReturnType<typeof setTimeout> | undefined;
let dispose = () => {};
let mounted = false;
const statusLabel = (status: unknown) => {
  const value = String(status || 'idle');
  const key = ['cancelled', 'canceled', 'cancelling', 'interrupted'].includes(value) ? 'interrupted'
    : ['success', 'finished', 'completed'].includes(value) ? 'completed'
    : ['running', 'queued', 'waiting'].includes(value) ? 'running'
    : ['error', 'failed', 'timeout'].includes(value) ? 'failed' : 'idle';
  return t(`chat.subagentPool.${key}`);
};
async function refresh() {
  request?.abort();
  const controller = new AbortController();
  request = controller;
  const id = props.sessionId;
  if (!id || !mounted) { loading.value = false; return; }
  loading.value = true;
  error.value = '';
  try {
    const { data } = await getSessionSubagents(id, { limit: 200 }, { signal: controller.signal });
    if (!mounted || controller.signal.aborted || props.sessionId !== id) return;
    items.value = Array.isArray(data?.data?.items) ? data.data.items : [];
  } catch {
    if (mounted && !controller.signal.aborted) error.value = t('chat.subagentPool.loadFailed');
  } finally {
    if (mounted && request === controller) loading.value = false;
  }
}
watch(() => props.sessionId, () => { items.value = []; error.value = ''; if (mounted) void refresh(); });
onMounted(() => {
  mounted = true;
  dispose = onSubagentPoolChanged((id) => {
    if (id !== props.sessionId || timer) return;
    timer = setTimeout(() => { timer = undefined; void refresh(); }, 250);
  });
  void refresh();
});
onBeforeUnmount(() => { mounted = false; request?.abort(); clearTimeout(timer); dispose(); });
</script>

<style scoped>
.subagent-pool { flex: 0 0 auto; max-height: 240px; overflow: auto; padding: 12px; border-top: 1px solid var(--messenger-panel-border); }
.subagent-pool-heading { display: flex; align-items: center; gap: 8px; }
.subagent-pool-heading strong { flex: 1; }
.subagent-pool button { color: inherit; background: transparent; border: 1px solid var(--messenger-panel-border); border-radius: 6px; padding: 5px 8px; cursor: pointer; }
.subagent-pool button:focus-visible { outline: 2px solid var(--ui-accent); outline-offset: 2px; }
.subagent-pool p, .subagent-pool small { color: var(--messenger-polish-muted); font-size: 12px; }
.subagent-pool-items { display: grid; gap: 6px; margin-top: 8px; }
.subagent-pool-item { display: flex; justify-content: space-between; gap: 8px; min-width: 0; text-align: left; }
.subagent-pool-item span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.subagent-pool-item small { flex-shrink: 0; }
</style>
