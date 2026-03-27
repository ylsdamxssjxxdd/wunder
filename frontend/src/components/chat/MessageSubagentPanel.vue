<template>
  <section v-if="visible" class="subagent-panel">
    <header class="subagent-panel__header">
      <div>
        <div class="subagent-panel__title">子智能体</div>
        <div class="subagent-panel__meta">
          <span>{{ items.length }} 个</span>
          <span v-if="runningTotal > 0">进行中 {{ runningTotal }}</span>
        </div>
      </div>
    </header>

    <div class="subagent-panel__list">
      <button
        v-for="item in items"
        :key="item.key"
        type="button"
        class="subagent-panel__item"
        @click="openDetail(item)"
      >
        <div class="subagent-panel__item-main">
          <div class="subagent-panel__item-top">
            <span class="subagent-panel__item-title">{{ item.title || item.label || item.session_id }}</span>
            <span :class="['subagent-panel__status', statusClass(item.status)]">
              {{ statusText(item.status) }}
            </span>
          </div>
          <div v-if="item.summary" class="subagent-panel__summary">{{ item.summary }}</div>
          <div class="subagent-panel__detail-line">
            <span v-if="item.run_id">Run {{ item.run_id }}</span>
            <span v-if="item.updated_at">{{ formatTime(item.updated_at) }}</span>
          </div>
        </div>
        <div class="subagent-panel__item-actions" @click.stop>
          <button
            v-if="item.canTerminate"
            type="button"
            class="subagent-panel__stop"
            :disabled="terminatingKeys.has(item.key)"
            @click.stop="terminate(item)"
          >
            {{ terminatingKeys.has(item.key) ? '终止中' : '终止' }}
          </button>
        </div>
      </button>
    </div>

    <el-dialog
      v-model="detailVisible"
      title="子智能体详情"
      width="720px"
      class="subagent-panel__dialog"
      destroy-on-close
    >
      <div v-if="activeItem" class="subagent-detail">
        <div class="subagent-detail__top">
          <div class="subagent-detail__headline">{{ activeItem.title || activeItem.session_id }}</div>
          <span :class="['subagent-panel__status', statusClass(activeItem.status)]">
            {{ statusText(activeItem.status) }}
          </span>
        </div>
        <div v-if="activeItem.summary" class="subagent-detail__summary">{{ activeItem.summary }}</div>
        <pre class="subagent-detail__payload">{{ formatPayload(activeItem.detail) }}</pre>
      </div>
    </el-dialog>
  </section>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue';
import { useChatStore } from '@/stores/chat';

const props = defineProps<{
  sessionId?: string | null;
  items?: Array<Record<string, unknown>>;
}>();

const chatStore = useChatStore();
const detailVisible = ref(false);
const activeItem = ref<Record<string, unknown> | null>(null);
const terminatingKeys = ref(new Set<string>());

const items = computed(() =>
  Array.isArray(props.items) ? props.items.filter((item) => item && typeof item === 'object') : []
);

const visible = computed(() => items.value.length > 0);

const runningTotal = computed(() =>
  items.value.filter((item) => {
    const status = String(item.status || '').trim().toLowerCase();
    return ['running', 'queued', 'accepted', 'waiting', 'cancelling'].includes(status);
  }).length
);

const statusText = (value: unknown) => {
  const status = String(value || '').trim().toLowerCase();
  if (['success', 'completed', 'idle'].includes(status)) return '已完成';
  if (['error', 'failed', 'timeout', 'cancelled', 'closed', 'partial', 'not_found'].includes(status)) {
    return '异常';
  }
  if (status === 'cancelling') return '终止中';
  if (status === 'queued' || status === 'accepted') return '排队中';
  return '运行中';
};

const statusClass = (value: unknown) => {
  const status = String(value || '').trim().toLowerCase();
  if (['success', 'completed', 'idle'].includes(status)) return 'is-success';
  if (['error', 'failed', 'timeout', 'cancelled', 'closed', 'partial', 'not_found'].includes(status)) {
    return 'is-failed';
  }
  return 'is-running';
};

const formatTime = (value: unknown) => {
  const text = String(value || '').trim();
  if (!text) return '';
  const parsed = new Date(text);
  if (Number.isNaN(parsed.getTime())) return text;
  return parsed.toLocaleString();
};

const formatPayload = (value: unknown) => {
  try {
    return JSON.stringify(value ?? {}, null, 2);
  } catch (error) {
    return String(value || '');
  }
};

const openDetail = (item: Record<string, unknown>) => {
  activeItem.value = item;
  detailVisible.value = true;
};

const terminate = async (item: Record<string, unknown>) => {
  const key = String(item.key || item.session_id || '').trim();
  if (!key || !props.sessionId) return;
  terminatingKeys.value.add(key);
  try {
    await chatStore.controlSubagent(props.sessionId, item, 'terminate');
  } finally {
    terminatingKeys.value.delete(key);
  }
};
</script>

<style scoped>
.subagent-panel {
  margin-top: 12px;
  border: 1px solid var(--chat-message-border, rgba(120, 130, 150, 0.18));
  border-radius: 14px;
  background: var(--chat-subpanel-bg, rgba(246, 248, 252, 0.9));
  overflow: hidden;
}

.subagent-panel__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 14px 10px;
  border-bottom: 1px solid rgba(120, 130, 150, 0.12);
}

.subagent-panel__title {
  font-size: 13px;
  font-weight: 700;
}

.subagent-panel__meta {
  display: flex;
  gap: 10px;
  margin-top: 4px;
  font-size: 12px;
  color: var(--chat-text-secondary, #6b7280);
}

.subagent-panel__list {
  display: flex;
  flex-direction: column;
}

.subagent-panel__item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  width: 100%;
  padding: 12px 14px;
  border: 0;
  border-top: 1px solid rgba(120, 130, 150, 0.08);
  background: transparent;
  text-align: left;
  cursor: pointer;
}

.subagent-panel__item:first-child {
  border-top: 0;
}

.subagent-panel__item:hover {
  background: rgba(120, 130, 150, 0.06);
}

.subagent-panel__item-main {
  min-width: 0;
  flex: 1;
}

.subagent-panel__item-top {
  display: flex;
  align-items: center;
  gap: 10px;
}

.subagent-panel__item-title {
  min-width: 0;
  font-size: 13px;
  font-weight: 600;
  line-height: 1.35;
}

.subagent-panel__summary {
  margin-top: 6px;
  font-size: 12px;
  line-height: 1.45;
  color: var(--chat-text-secondary, #6b7280);
  display: -webkit-box;
  overflow: hidden;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
}

.subagent-panel__detail-line {
  display: flex;
  gap: 12px;
  margin-top: 6px;
  font-size: 11px;
  color: var(--chat-text-muted, #8a92a2);
}

.subagent-panel__item-actions {
  display: flex;
  align-items: center;
}

.subagent-panel__stop {
  height: 28px;
  padding: 0 10px;
  border: 1px solid rgba(210, 70, 70, 0.28);
  border-radius: 999px;
  background: rgba(210, 70, 70, 0.08);
  color: #b42318;
  font-size: 12px;
  cursor: pointer;
}

.subagent-panel__stop:disabled {
  opacity: 0.6;
  cursor: default;
}

.subagent-panel__status {
  flex: none;
  padding: 2px 8px;
  border-radius: 999px;
  font-size: 11px;
  font-weight: 600;
}

.subagent-panel__status.is-running {
  background: rgba(21, 128, 61, 0.12);
  color: #166534;
}

.subagent-panel__status.is-success {
  background: rgba(3, 105, 161, 0.12);
  color: #075985;
}

.subagent-panel__status.is-failed {
  background: rgba(185, 28, 28, 0.12);
  color: #b91c1c;
}

.subagent-detail__top {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.subagent-detail__headline {
  font-size: 15px;
  font-weight: 700;
}

.subagent-detail__summary {
  margin-top: 10px;
  font-size: 13px;
  line-height: 1.55;
  color: var(--chat-text-secondary, #6b7280);
}

.subagent-detail__payload {
  margin-top: 14px;
  padding: 14px;
  border-radius: 12px;
  background: rgba(15, 23, 42, 0.94);
  color: #dce7f7;
  font-size: 12px;
  line-height: 1.55;
  overflow: auto;
  white-space: pre-wrap;
  word-break: break-word;
}
</style>
