<template>
  <div class="interlink-thread-tree mz-thread-tree" role="tree" :aria-label="text('本地工作线程', 'Local threads')">
    <div v-if="loading" class="interlink-thread-state">{{ text('加载中…', 'Loading…') }}</div>
    <div v-else-if="error" class="interlink-thread-state is-error" role="alert">{{ error }}</div>
    <div v-else-if="!items.length" class="interlink-thread-state">{{ text('暂无线程', 'No threads') }}</div>
    <div
      v-for="item in items"
      :key="item.local_thread_id"
      class="mz-thread-row"
      :class="{ 'is-active': activeId === item.local_thread_id }"
      role="treeitem"
      :aria-selected="activeId === item.local_thread_id"
    >
      <button class="interlink-thread-row mz-thread-select" type="button"
        :title="rowTooltip(item)" @click="emit('open', item.local_thread_id)">
        <span class="mz-thread-status" :class="statusOf(item.status).style" aria-hidden="true">
          <i class="fa-solid" :class="statusOf(item.status).icon"></i>
        </span>
        <span class="mz-thread-title">{{ item.title || item.local_thread_id }}</span>
      </button>
    </div>
    <button v-if="error" class="interlink-thread-refresh" type="button" @click="emit('refresh')">
      {{ text('重试', 'Retry') }}
    </button>
    <button v-if="hasMore" class="interlink-thread-refresh" type="button" :disabled="loading" @click="emit('load-more')">
      {{ text('加载更多', 'Load more') }}
    </button>
    <div v-if="truncated" class="interlink-thread-state">{{ text('显示最近 200 个线程', 'Showing the 200 most recent threads') }}</div>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from '@/i18n';
import type { InterlinkShadowThread } from '@/api/interlink';
import { formatDateTime } from '@/utils/formatDateTime';

defineProps<{ items: InterlinkShadowThread[]; loading: boolean; error: string; hasMore: boolean; truncated: boolean; activeId: string }>();
const emit = defineEmits<{ open: [id: string]; refresh: []; 'load-more': [] }>();
const { language } = useI18n();
const text = (zh: string, en: string): string => language.value.startsWith('zh') ? zh : en;
/** 同主侧栏：标题之外补一行最后活动时间，便于按日期找回远端线程。 */
const rowTooltip = (item: InterlinkShadowThread): string => {
  const title = item.title || item.local_thread_id;
  const at = formatDateTime(item.updated_at);
  return at ? `${title} · ${text('最后完成', 'Last finished')} ${at}` : title;
};
const statusOf = (status: string) => {
  if (status === 'working') return { style: 'is-running', icon: 'fa-spinner fa-spin' };
  if (status === 'failed') return { style: 'is-error', icon: 'fa-circle-exclamation' };
  if (status === 'needs_you') return { style: 'is-pending', icon: 'fa-circle-question' };
  if (status === 'finished') return { style: 'is-done', icon: 'fa-check' };
  return { style: 'is-idle', icon: 'fa-circle' };
};
</script>

<style scoped>
.interlink-thread-tree { overflow-y: auto; padding-bottom: 6px; }
.interlink-thread-state { padding: 5px 8px; color: var(--mz-text-muted); font-size: var(--mz-font-xs, 11px); }
.interlink-thread-state.is-error { color: #b33b3b; }
.interlink-thread-refresh { margin: 0 8px 5px; border: 0; color: var(--mz-primary); background: transparent; cursor: pointer; font: inherit; font-size: var(--mz-font-xs, 11px); }
</style>
