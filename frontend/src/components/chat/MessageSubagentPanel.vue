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
      <div
        v-for="item in items"
        :key="item.key"
        role="button"
        tabindex="0"
        class="subagent-panel__item"
        @click="openDetail(item)"
        @keydown.enter.self="openDetail(item)"
        @keydown.space.self.prevent="openDetail(item)"
      >
        <div class="subagent-panel__item-main">
          <div class="subagent-panel__item-top">
            <span class="subagent-panel__item-title">{{ item.title || item.label || item.session_id }}</span>
            <span :class="['subagent-panel__status', statusClass(item.status, item)]">
              {{ statusText(item.status, item) }}
            </span>
          </div>
          <div class="subagent-panel__summary">{{ latestPreview(item) || '等待消息…' }}</div>
          <!-- 与气泡底部同一套统计图标：时长 / 额度 / 上下文，只留图标与数字，细节放 title。 -->
          <div class="subagent-panel__stats">
            <span class="subagent-panel__stat" :title="'运行时长 ' + durationText(item)">
              <i class="fa-solid fa-stopwatch" aria-hidden="true"></i><span>{{ durationText(item) }}</span>
            </span>
            <span class="subagent-panel__stat" :title="creditsHint(item)">
              <i class="fa-solid fa-coins" aria-hidden="true"></i><span>{{ creditsText(item) }}</span>
            </span>
            <span class="subagent-panel__stat" :title="contextHint(item)">
              <i class="fa-solid fa-layer-group" aria-hidden="true"></i><span>{{ contextText(item) }}</span>
            </span>
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
            {{ terminatingKeys.has(item.key) ? '中断中' : '中断' }}
          </button>
        </div>
      </div>
    </div>

    <el-dialog
      v-model="detailVisible"
      title="子智能体详情"
      width="720px"
      top="clamp(10px, 4vh, 36px)"
      class="subagent-panel__dialog"
      destroy-on-close
      append-to-body
    >
      <div v-if="activeItem" class="subagent-detail">
        <div class="subagent-detail__top">
          <div class="subagent-detail__headline">{{ activeItem.title || activeItem.session_id }}</div>
          <span :class="['subagent-panel__status', statusClass(activeItem.status, activeItem)]">
            {{ statusText(activeItem.status, activeItem) }}
          </span>
        </div>
        <div class="subagent-panel__stats">
          <span class="subagent-panel__stat" :title="'运行时长 ' + durationText(activeItem)">
            <i class="fa-solid fa-stopwatch" aria-hidden="true"></i><span>{{ durationText(activeItem) }}</span>
          </span>
          <span class="subagent-panel__stat" :title="creditsHint(activeItem)">
            <i class="fa-solid fa-coins" aria-hidden="true"></i><span>{{ creditsText(activeItem) }}</span>
          </span>
          <span class="subagent-panel__stat" :title="contextHint(activeItem)">
            <i class="fa-solid fa-layer-group" aria-hidden="true"></i><span>{{ contextText(activeItem) }}</span>
          </span>
        </div>
        <div class="subagent-detail__timeline">
          <SubagentRunDetail v-if="detailVisible && activeItem.session_id"
            :session-id="activeItem.session_id" :run-id="activeItem.run_id || activeItem.key"
            :turn-id="childTurnId(activeItem)" />
        </div>
      </div>
    </el-dialog>
  </section>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import { formatCompactCount } from '@/utils/compactNumber';
import { normalizeChatTimestampMs } from '@/utils/chatTiming';
import { formatDurationSeconds } from '@/utils/messageStats';
import { useChatStore } from '@/stores/chat';
import SubagentRunDetail from './SubagentRunDetail.vue';
import {
  isSubagentItemActive,
  isSubagentStatusFailed,
  isSubagentStatusSuccessful,
  normalizeSubagentRuntimeStatus
} from '@/utils/subagentRuntime';

type SubagentPanelItem = Record<string, unknown> & {
  key: string;
  status?: string;
  title?: string;
  label?: string;
  summary?: string;
  run_id?: string;
  session_id?: string;
  updated_at?: string;
  canTerminate?: boolean;
  detail?: unknown;
};

const props = defineProps<{
  sessionId?: string | null;
  items?: Array<Record<string, unknown>>;
}>();

const chatStore = useChatStore();
const detailVisible = ref(false);
const activeKey = ref('');
const activeItem = computed(() => items.value.find(item => item.key === activeKey.value) ?? null);
const terminatingKeys = ref<Set<string>>(new Set());

watch(() => props.sessionId, () => {
  detailVisible.value = false;
  activeKey.value = '';
  terminatingKeys.value = new Set();
});

const items = computed<SubagentPanelItem[]>(() =>
  Array.isArray(props.items)
    ? props.items
        .filter((item): item is SubagentPanelItem => Boolean(item && typeof item === 'object'))
        .map((item) => item as SubagentPanelItem)
    : []
);

const visible = computed(() => items.value.length > 0);

/* 运行中的条目要按秒刷新时长；没有在跑的条目就不再挂定时器。 */
const nowMs = ref(Date.now());
let clockTimer: number | null = null;
const syncClock = () => {
  const live = items.value.some((item) => isSubagentItemActive(item));
  if (live && clockTimer === null) {
    clockTimer = window.setInterval(() => { nowMs.value = Date.now(); }, 1000);
  } else if (!live && clockTimer !== null) {
    window.clearInterval(clockTimer);
    clockTimer = null;
  }
};
watch(items, syncClock, { immediate: true });
onBeforeUnmount(() => {
  if (clockTimer !== null) window.clearInterval(clockTimer);
});

const runningTotal = computed(() => items.value.filter((item) => isSubagentItemActive(item)).length);

const statusText = (value: unknown, item: SubagentPanelItem | null = null) => {
  const status = normalizeSubagentRuntimeStatus(value);
  if (item && isSubagentItemActive(item)) {
    if (status === 'queued' || status === 'accepted') return '排队中';
    if (status === 'cancelling') return '终止中';
    return '运行中';
  }
  if (['cancelled', 'canceled', 'interrupted'].includes(status)) return '已中断';
  if (isSubagentStatusSuccessful(status)) return '已完成';
  if (isSubagentStatusFailed(status) || status === 'timeout') {
    return '异常';
  }
  if (status === 'cancelling') return '终止中';
  if (status === 'queued' || status === 'accepted') return '排队中';
  return '状态未知';
};

const statusClass = (value: unknown, item: SubagentPanelItem | null = null) => {
  const status = normalizeSubagentRuntimeStatus(value);
  if (item && isSubagentItemActive(item)) return 'is-running';
  if (isSubagentStatusSuccessful(status)) return 'is-success';
  if (isSubagentStatusFailed(status) || status === 'timeout') {
    return 'is-failed';
  }
  return 'is-unknown';
};

const resolveItemDetail = (item: SubagentPanelItem): Record<string, unknown> => {
  const detail = item?.detail;
  return detail && typeof detail === 'object' ? (detail as Record<string, unknown>) : {};
};

const pickSubagentText = (item: SubagentPanelItem, ...keys: string[]): string => {
  const detail = resolveItemDetail(item);
  for (const key of keys) {
    const direct = String((item as Record<string, unknown>)[key] || '').trim();
    if (direct) return direct;
    const nested = String(detail[key] || '').trim();
    if (nested) return nested;
  }
  return '';
};

const latestPreview = (item: SubagentPanelItem) =>
  pickSubagentText(item, 'latest_message', 'assistant_message', 'summary', 'user_message');

const metric = (item: SubagentPanelItem, key: string): string => {
  // Display quota as provider request count; account debit can be zero for exempt calls.
  const metrics = (item.metrics ?? resolveItemDetail(item).metrics) as Record<string, unknown> | undefined;
  const value = metrics?.[key];
  return typeof value === 'number' && Number.isFinite(value) && value >= 0
    ? value.toLocaleString() : '--';
};

const numericMetric = (item: SubagentPanelItem, key: string): number | null => {
  const metrics = (item.metrics ?? resolveItemDetail(item).metrics) as Record<string, unknown> | undefined;
  const value = Number(metrics?.[key]);
  return Number.isFinite(value) && value >= 0 ? value : null;
};

/**
 * 时间戳在子智能体条目里有多种落点（detail 里的 started_at/started_time，
 * 或 runtime 展开后的顶层字段），这里统一挑第一个能解析的墙钟时间。
 */
const subagentTimestampMs = (item: SubagentPanelItem, ...keys: string[]): number | null => {
  const detail = resolveItemDetail(item);
  for (const key of keys) {
    for (const source of [detail, item as Record<string, unknown>]) {
      const raw = source?.[key];
      if (raw === undefined || raw === null || raw === '') continue;
      const parsed = normalizeChatTimestampMs(raw);
      if (parsed !== null) return parsed;
    }
  }
  return null;
};

/** 运行时长：在跑的按开始时间实时推算，已结束用 elapsed_s 或结束-开始。 */
const durationSeconds = (item: SubagentPanelItem): number | null => {
  const detail = resolveItemDetail(item);
  const elapsed = Number(detail.elapsed_s);
  const started = subagentTimestampMs(item, 'started_at', 'started_time', 'queued_at', 'queued_time')
    ?? subagentTimestampMs(item, 'updated_at');
  if (isSubagentItemActive(item)) {
    if (started !== null) return Math.max(0, (nowMs.value - started) / 1000);
    return Number.isFinite(elapsed) && elapsed >= 0 ? elapsed : null;
  }
  const finished = subagentTimestampMs(item, 'finished_at', 'finished_time');
  if (started !== null && finished !== null && finished >= started) {
    return (finished - started) / 1000;
  }
  return Number.isFinite(elapsed) && elapsed >= 0 ? elapsed : null;
};

const durationText = (item: SubagentPanelItem): string => {
  const seconds = durationSeconds(item);
  return seconds === null ? '--' : formatDurationSeconds(seconds);
};

const creditsText = (item: SubagentPanelItem): string =>
  formatCompactCount(numericMetric(item, 'account_credits_consumed'));

const creditsHint = (item: SubagentPanelItem): string =>
  '额度消耗 ' + creditsText(item) + ' · 模型请求 ' + metric(item, 'model_request_count')
  + ' 次 · 工具 ' + metric(item, 'tool_calls') + ' 次';

const contextText = (item: SubagentPanelItem): string => {
  const tokens = numericMetric(item, 'context_tokens');
  const limit = numericMetric(item, 'max_context');
  if (tokens === null) return '--';
  if (limit !== null && limit > 0) return Math.round((tokens / limit) * 100) + '%';
  return formatCompactCount(tokens);
};

const contextHint = (item: SubagentPanelItem): string => {
  const tokens = numericMetric(item, 'context_tokens');
  const limit = numericMetric(item, 'max_context');
  if (tokens === null) return '上下文占用 --';
  const left = formatCompactCount(tokens);
  const right = limit !== null && limit > 0 ? formatCompactCount(limit) : null;
  return '上下文占用 ' + (right ? left + ' / ' + right + ' Token' : left + ' Token');
};

const childTurnId = (item: SubagentPanelItem): string =>
  String((item.metrics as Record<string, unknown> | undefined)?.child_turn_id || '');

const openDetail = (item: SubagentPanelItem) => {
  activeKey.value = item.key;
  detailVisible.value = true;
};

const terminate = async (item: SubagentPanelItem) => {
  const key = String(item.key || item.session_id || '').trim();
  if (!key || !props.sessionId) return;
  terminatingKeys.value.add(key);
  try {
    await chatStore.controlSubagent(props.sessionId, item, 'interrupt');
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
  padding: 10px 12px 8px;
  border-bottom: 1px solid rgba(120, 130, 150, 0.12);
}

.subagent-panel__title {
  font-size: 13px;
  font-weight: 700;
}

.subagent-panel__meta {
  display: flex;
  gap: 10px;
  margin-top: 2px;
  font-size: 11px;
  color: var(--chat-text-secondary, #6b7280);
}

.subagent-panel__list {
  display: flex;
  flex-direction: column;
  max-height: 280px;
  overflow-y: auto;
}

.subagent-panel__item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  width: 100%;
  padding: 8px 12px;
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
  overflow: hidden;
}

.subagent-panel__item-top {
  display: flex;
  align-items: center;
  gap: 8px;
}

.subagent-panel__item-title {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  font-weight: 600;
  line-height: 1.35;
}

.subagent-panel__summary {
  margin-top: 4px;
  font-size: 11px;
  line-height: 1.4;
  color: var(--chat-text-secondary, #6b7280);
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
  word-break: break-word;
}

.subagent-panel__role {
  display: inline-block;
  width: 16px;
  font-weight: 700;
  color: var(--chat-text-muted, #8a92a2);
}

/* 统计行：与气泡底部同一套图标（耗时 / 额度 / 上下文），只留图标与数字。 */
.subagent-panel__stats {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 10px;
  margin-top: 4px;
  font-size: 11px;
  line-height: 1.5;
  color: var(--chat-text-muted, #8a92a2);
}

.subagent-panel__stat {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-variant-numeric: tabular-nums;
}

.subagent-panel__stat i {
  flex: 0 0 12px;
  width: 12px;
  font-size: 10px;
  line-height: 1;
  text-align: center;
  opacity: 0.9;
}

.subagent-panel__stat span {
  color: var(--chat-text, #2f2f2f);
}

.subagent-panel__item-actions {
  display: flex;
  align-items: center;
  flex-shrink: 0;
}

.subagent-panel__stop {
  height: 24px;
  padding: 0 8px;
  border: 1px solid rgba(210, 70, 70, 0.28);
  border-radius: 999px;
  background: rgba(210, 70, 70, 0.08);
  color: #b42318;
  font-size: 11px;
  cursor: pointer;
}

.subagent-panel__stop:disabled {
  opacity: 0.6;
  cursor: default;
}

.subagent-panel__status {
  flex: none;
  padding: 1px 7px;
  border-radius: 999px;
  font-size: 10px;
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

.subagent-panel__status.is-unknown {
  background: rgba(107, 114, 128, 0.12);
  color: #4b5563;
}
</style>

<style>
/* Dialog styles are global because el-dialog teleports to <body> */
.subagent-panel__dialog.el-dialog {
  /* Teleport leaves the chat shell: derive all text colors from the dialog theme. */
  --chat-text: var(--el-text-color-primary, #303133);
  --chat-muted: var(--el-text-color-regular, #606266);
  --chat-text-secondary: var(--el-text-color-regular, #606266);
  --chat-text-muted: var(--el-text-color-secondary, #909399);
  --chat-panel: var(--el-bg-color, #ffffff);
  width: min(720px, calc(100vw - 24px)) !important;
  max-height: calc(var(--app-viewport-height, 100vh) - 24px);
  margin: 12px auto !important;
  display: flex;
  flex-direction: column;
}

.subagent-panel__dialog .el-dialog__body {
  flex: 1 1 auto;
  min-height: 0;
  padding: 14px 18px;
  overflow: hidden;
  display: flex;
}

.subagent-detail {
  width: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.subagent-detail__top {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.subagent-detail__headline {
  min-width: 0;
  overflow-wrap: anywhere;
  font-size: 15px;
  font-weight: 700;
}

.subagent-detail__summary {
  margin-top: 10px;
  font-size: 13px;
  line-height: 1.55;
  color: var(--chat-text-secondary, #6b7280);
}

.subagent-detail__timeline {
  margin-top: 12px;
  flex: 1 1 auto;
  min-height: 0;
  max-height: 65vh;
  overflow: auto;
  padding-top: 4px;
  border-top: 1px solid var(--el-border-color-lighter, #ebeef5);
  word-break: break-word;
}

.subagent-panel__dialog .subagent-panel__stats {
  gap: 14px;
  margin-top: 8px;
  font-size: 12px;
  color: var(--chat-muted);
}

.subagent-panel__dialog .subagent-panel__status {
  flex: none;
  padding: 2px 8px;
  border-radius: 999px;
  font-size: 11px;
  font-weight: 600;
}

.subagent-panel__dialog .subagent-panel__status.is-running {
  background: rgba(21, 128, 61, 0.12);
  color: #166534;
}

.subagent-panel__dialog .subagent-panel__status.is-success {
  background: rgba(3, 105, 161, 0.12);
  color: #075985;
}

.subagent-panel__dialog .subagent-panel__status.is-failed {
  background: rgba(185, 28, 28, 0.12);
  color: #b91c1c;
}

.subagent-panel__status.is-unknown {
  background: rgba(107, 114, 128, 0.12);
  color: #4b5563;
}
</style>
