<template>
  <div class="messenger-agent-runtime">
    <div v-if="!normalizedAgentId" class="messenger-list-empty">
      {{ t('chat.features.agentMissing') }}
    </div>

    <template v-else>
      <div class="messenger-agent-runtime-toolbar">
        <div class="messenger-agent-runtime-range">
          {{ runtimeRangeText }}
        </div>
        <label class="messenger-agent-runtime-control">
          <span>{{ t('messenger.agent.runtime.days') }}</span>
          <select v-model.number="windowDays" :disabled="loading">
            <option v-for="option in dayOptions" :key="option" :value="option">{{ option }}d</option>
          </select>
        </label>
        <label class="messenger-agent-runtime-control">
          <span>{{ t('messenger.agent.runtime.date') }}</span>
          <input v-model="selectedDate" type="date" :disabled="loading" />
        </label>
        <button class="messenger-inline-btn" type="button" :disabled="loading" @click="loadRuntimeRecords">
          {{ loading ? t('common.loading') : t('common.refresh') }}
        </button>
      </div>

      <div v-if="errorMessage" class="messenger-agent-runtime-error">
        {{ errorMessage }}
      </div>

      <div class="messenger-agent-runtime-cards">
        <article class="messenger-agent-runtime-card">
          <div class="messenger-agent-runtime-label">{{ t('messenger.agent.runtime.metric.runtime') }}</div>
          <div class="messenger-agent-runtime-value">{{ formatDuration(summary.runtime_seconds) }}</div>
        </article>
        <article class="messenger-agent-runtime-card">
          <div class="messenger-agent-runtime-label">{{ t('messenger.agent.runtime.metric.tokens') }}</div>
          <div class="messenger-agent-runtime-value">{{ formatNumber(summary.billed_tokens) }}</div>
        </article>
        <article class="messenger-agent-runtime-card">
          <div class="messenger-agent-runtime-label">{{ t('messenger.agent.runtime.metric.quota') }}</div>
          <div class="messenger-agent-runtime-value">{{ formatNumber(summary.quota_consumed) }}</div>
        </article>
        <article class="messenger-agent-runtime-card">
          <div class="messenger-agent-runtime-label">{{ t('messenger.agent.runtime.metric.tools') }}</div>
          <div class="messenger-agent-runtime-value">{{ formatNumber(summary.tool_calls) }}</div>
        </article>
      </div>

      <section class="messenger-agent-runtime-panel">
        <header class="messenger-agent-runtime-panel-head">
          <div>
            <div class="messenger-agent-runtime-panel-title">{{ t('messenger.agent.runtime.trendTitle') }}</div>
            <div class="messenger-agent-runtime-panel-meta">{{ t('messenger.agent.runtime.trendMeta') }}</div>
          </div>
        </header>
        <div v-if="!dailyRows.length && !loading" class="messenger-list-empty">
          {{ t('messenger.agent.runtime.empty') }}
        </div>
        <div v-else ref="trendChartRef" class="messenger-agent-runtime-chart"></div>
      </section>

      <section class="messenger-agent-runtime-panel">
        <header class="messenger-agent-runtime-panel-head">
          <div>
            <div class="messenger-agent-runtime-panel-title">{{ t('messenger.agent.runtime.heatmapTitle') }}</div>
            <div class="messenger-agent-runtime-panel-meta">{{ t('messenger.agent.runtime.heatmapMeta') }}</div>
          </div>
          <div class="messenger-agent-runtime-panel-meta">
            {{ runtimeData?.heatmap?.date || selectedDate }}
          </div>
        </header>
        <div v-if="!hasHeatmapData && !loading" class="messenger-list-empty">
          {{ t('messenger.agent.runtime.heatmapEmpty') }}
        </div>
        <div
          v-else
          ref="heatmapChartRef"
          class="messenger-agent-runtime-heatmap"
          :style="{ minHeight: `${heatmapHeight}px` }"
        ></div>
      </section>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import * as echarts from 'echarts';

import { getAgentRuntimeRecords } from '@/api/agents';
import { useI18n } from '@/i18n';
import { useThemeStore } from '@/stores/theme';

type RuntimeDailyRecord = {
  date: string;
  runtime_seconds: number;
  billed_tokens: number;
  quota_consumed: number;
  tool_calls: number;
};

type RuntimeHeatmapItem = {
  tool: string;
  hourly_calls: number[];
  total_calls: number;
};

type RuntimeSummary = {
  runtime_seconds: number;
  billed_tokens: number;
  quota_consumed: number;
  tool_calls: number;
};

type RuntimePayload = {
  range?: {
    start_date?: string;
    end_date?: string;
    days?: number;
  };
  summary?: RuntimeSummary;
  daily?: RuntimeDailyRecord[];
  heatmap?: {
    date?: string;
    max_calls?: number;
    items?: RuntimeHeatmapItem[];
  };
};

const props = defineProps({
  agentId: {
    type: String,
    default: ''
  }
});

const { t, language } = useI18n();
const themeStore = useThemeStore();

const dayOptions = [7, 14, 30];
const trendChartRef = ref<HTMLElement | null>(null);
const heatmapChartRef = ref<HTMLElement | null>(null);
const loading = ref(false);
const errorMessage = ref('');
const runtimeData = ref<RuntimePayload | null>(null);
const windowDays = ref(14);
const selectedDate = ref(resolveTodayDate());
const normalizedAgentId = computed(() => String(props.agentId || '').trim());

let trendChart: echarts.ECharts | null = null;
let heatmapChart: echarts.ECharts | null = null;
let requestSerial = 0;

const dailyRows = computed<RuntimeDailyRecord[]>(() => {
  const source = Array.isArray(runtimeData.value?.daily) ? runtimeData.value?.daily : [];
  return (source || []).map((item) => ({
    date: String(item?.date || ''),
    runtime_seconds: toSafeNumber(item?.runtime_seconds),
    billed_tokens: Math.max(0, toSafeNumber(item?.billed_tokens)),
    quota_consumed: Math.max(0, toSafeNumber(item?.quota_consumed)),
    tool_calls: Math.max(0, toSafeNumber(item?.tool_calls))
  }));
});

const heatmapItems = computed<RuntimeHeatmapItem[]>(() => {
  const source = Array.isArray(runtimeData.value?.heatmap?.items) ? runtimeData.value?.heatmap?.items : [];
  return (source || []).map((item) => ({
    tool: String(item?.tool || '').trim() || 'unknown',
    hourly_calls: normalizeHourlyCalls(item?.hourly_calls),
    total_calls: Math.max(0, toSafeNumber(item?.total_calls))
  }));
});

const summary = computed<RuntimeSummary>(() => {
  const source = runtimeData.value?.summary;
  if (source) {
    return {
      runtime_seconds: Math.max(0, toSafeNumber(source.runtime_seconds)),
      billed_tokens: Math.max(0, toSafeNumber(source.billed_tokens)),
      quota_consumed: Math.max(0, toSafeNumber(source.quota_consumed)),
      tool_calls: Math.max(0, toSafeNumber(source.tool_calls))
    };
  }
  return dailyRows.value.reduce<RuntimeSummary>(
    (acc, item) => ({
      runtime_seconds: acc.runtime_seconds + item.runtime_seconds,
      billed_tokens: acc.billed_tokens + item.billed_tokens,
      quota_consumed: acc.quota_consumed + item.quota_consumed,
      tool_calls: acc.tool_calls + item.tool_calls
    }),
    { runtime_seconds: 0, billed_tokens: 0, quota_consumed: 0, tool_calls: 0 }
  );
});

const hasHeatmapData = computed(() => heatmapItems.value.some((item) => item.total_calls > 0));
const heatmapHeight = computed(() => Math.max(300, heatmapItems.value.length * 26 + 120));
const runtimeRangeText = computed(() => {
  const range = runtimeData.value?.range;
  const start = String(range?.start_date || '').trim();
  const end = String(range?.end_date || '').trim();
  const days = Number(range?.days || 0);
  if (start && end) {
    return `${start} ~ ${end} (${days || windowDays.value}d)`;
  }
  return t('messenger.agent.runtime.rangePlaceholder', { days: windowDays.value });
});

function toSafeNumber(value: unknown): number {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

function normalizeHourlyCalls(value: unknown): number[] {
  if (!Array.isArray(value)) {
    return Array.from({ length: 24 }, () => 0);
  }
  const base = Array.from({ length: 24 }, (_, index) => Math.max(0, toSafeNumber(value[index])));
  if (base.length >= 24) {
    return base.slice(0, 24);
  }
  while (base.length < 24) {
    base.push(0);
  }
  return base;
}

function resolveTodayDate() {
  const now = new Date();
  const local = new Date(now.getTime() - now.getTimezoneOffset() * 60_000);
  return local.toISOString().slice(0, 10);
}

function formatNumber(value: number) {
  return new Intl.NumberFormat(language.value || 'zh-CN').format(Math.max(0, Math.round(value)));
}

function formatDuration(seconds: number) {
  const total = Math.max(0, Math.round(seconds));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const remainSeconds = total % 60;
  if (hours > 0) {
    return `${hours}h ${String(minutes).padStart(2, '0')}m`;
  }
  if (minutes > 0) {
    return `${minutes}m ${String(remainSeconds).padStart(2, '0')}s`;
  }
  return `${remainSeconds}s`;
}

function resolveChartPalette() {
  const dark = themeStore.mode === 'dark';
  return dark
    ? {
        text: '#d1d5db',
        axis: '#4b5563',
        split: 'rgba(148, 163, 184, 0.2)',
        tooltipBg: '#111827',
        tooltipBorder: '#374151',
        runtime: '#60a5fa',
        tokens: '#a78bfa',
        quota: '#34d399',
        tools: '#f59e0b',
        heatmapStart: 'rgba(71, 85, 105, 0.18)',
        heatmapEnd: 'rgba(96, 165, 250, 0.95)'
      }
    : {
        text: '#475569',
        axis: '#cbd5e1',
        split: '#ecedf0',
        tooltipBg: '#ffffff',
        tooltipBorder: '#dbe1ea',
        runtime: '#2563eb',
        tokens: '#7c3aed',
        quota: '#059669',
        tools: '#d97706',
        heatmapStart: '#f1f5f9',
        heatmapEnd: '#3b82f6'
      };
}

async function loadRuntimeRecords() {
  if (!normalizedAgentId.value) {
    runtimeData.value = null;
    return;
  }
  const serial = ++requestSerial;
  loading.value = true;
  errorMessage.value = '';
  try {
    const { data } = await getAgentRuntimeRecords(normalizedAgentId.value, {
      days: windowDays.value,
      date: selectedDate.value
    });
    if (serial !== requestSerial) {
      return;
    }
    runtimeData.value = (data?.data as RuntimePayload) || null;
  } catch (error) {
    if (serial !== requestSerial) {
      return;
    }
    runtimeData.value = null;
    const status = (error as { response?: { status?: number } })?.response?.status;
    if (status === 404) {
      errorMessage.value = t('messenger.agent.runtime.unsupported');
    } else {
      const message =
        (error as { response?: { data?: { detail?: string } }; message?: string })?.response?.data?.detail ||
        t('messenger.agent.runtime.loadFailed');
      errorMessage.value = message || t('messenger.agent.runtime.loadFailed');
    }
  } finally {
    if (serial !== requestSerial) {
      return;
    }
    loading.value = false;
    await nextTick();
    renderTrendChart();
    renderHeatmapChart();
  }
}

function renderTrendChart() {
  const container = trendChartRef.value;
  if (!container) {
    return;
  }
  if (!trendChart) {
    trendChart = echarts.init(container);
  }
  const palette = resolveChartPalette();
  const dates = dailyRows.value.map((item) => String(item.date || '').slice(5));
  const runtimeMinutes = dailyRows.value.map((item) => Number((item.runtime_seconds / 60).toFixed(2)));
  const billedTokens = dailyRows.value.map((item) => item.billed_tokens);
  const quotaConsumed = dailyRows.value.map((item) => item.quota_consumed);
  const toolCalls = dailyRows.value.map((item) => item.tool_calls);
  trendChart.setOption(
    {
      animation: false,
      color: [palette.runtime, palette.tokens, palette.quota, palette.tools],
      tooltip: {
        trigger: 'axis',
        backgroundColor: palette.tooltipBg,
        borderColor: palette.tooltipBorder,
        textStyle: { color: palette.text }
      },
      grid: { left: 44, right: 24, top: 28, bottom: 28 },
      legend: {
        top: 0,
        textStyle: { color: palette.text },
        data: [
          t('messenger.agent.runtime.series.runtime'),
          t('messenger.agent.runtime.series.tokens'),
          t('messenger.agent.runtime.series.quota'),
          t('messenger.agent.runtime.series.tools')
        ]
      },
      xAxis: {
        type: 'category',
        data: dates,
        axisLine: { lineStyle: { color: palette.axis } },
        axisLabel: { color: palette.text }
      },
      yAxis: {
        type: 'value',
        axisLine: { lineStyle: { color: palette.axis } },
        axisLabel: { color: palette.text },
        splitLine: { lineStyle: { color: palette.split } }
      },
      series: [
        {
          name: t('messenger.agent.runtime.series.runtime'),
          type: 'line',
          smooth: true,
          symbolSize: 6,
          data: runtimeMinutes
        },
        {
          name: t('messenger.agent.runtime.series.tokens'),
          type: 'line',
          smooth: true,
          symbolSize: 6,
          data: billedTokens
        },
        {
          name: t('messenger.agent.runtime.series.quota'),
          type: 'line',
          smooth: true,
          symbolSize: 6,
          data: quotaConsumed
        },
        {
          name: t('messenger.agent.runtime.series.tools'),
          type: 'line',
          smooth: true,
          symbolSize: 6,
          data: toolCalls
        }
      ]
    },
    true
  );
}

function renderHeatmapChart() {
  const container = heatmapChartRef.value;
  if (!container) {
    return;
  }
  if (!heatmapChart) {
    heatmapChart = echarts.init(container);
  }
  const palette = resolveChartPalette();
  const tools = heatmapItems.value.map((item) => item.tool);
  const hours = Array.from({ length: 24 }, (_, index) => String(index).padStart(2, '0'));
  const heatmapData: [number, number, number][] = [];
  heatmapItems.value.forEach((item, row) => {
    for (let hour = 0; hour < 24; hour += 1) {
      heatmapData.push([hour, row, item.hourly_calls[hour] || 0]);
    }
  });
  const visualMax = Math.max(
    1,
    toSafeNumber(runtimeData.value?.heatmap?.max_calls),
    ...heatmapItems.value.map((item) => item.total_calls)
  );
  heatmapChart.setOption(
    {
      animation: false,
      grid: { left: 84, right: 26, top: 14, bottom: 34 },
      tooltip: {
        position: 'top',
        backgroundColor: palette.tooltipBg,
        borderColor: palette.tooltipBorder,
        textStyle: { color: palette.text },
        formatter: (params: { data?: [number, number, number] }) => {
          const point = params?.data;
          if (!point) return '';
          const [hourIndex, toolIndex, count] = point;
          const toolName = tools[toolIndex] || 'unknown';
          return `${toolName}<br/>${hours[hourIndex]}:00 - ${hours[hourIndex]}:59<br/>${count}`;
        }
      },
      xAxis: {
        type: 'category',
        data: hours,
        splitArea: { show: false },
        axisLine: { lineStyle: { color: palette.axis } },
        axisLabel: { color: palette.text }
      },
      yAxis: {
        type: 'category',
        data: tools,
        axisLine: { lineStyle: { color: palette.axis } },
        axisLabel: {
          color: palette.text,
          width: 120,
          overflow: 'truncate'
        }
      },
      visualMap: {
        min: 0,
        max: visualMax,
        calculable: false,
        orient: 'horizontal',
        left: 'center',
        bottom: 2,
        inRange: {
          color: [palette.heatmapStart, palette.heatmapEnd]
        },
        textStyle: { color: palette.text }
      },
      series: [
        {
          name: t('messenger.agent.runtime.heatmapSeries'),
          type: 'heatmap',
          data: heatmapData,
          label: {
            show: false
          },
          emphasis: {
            itemStyle: {
              borderColor: palette.axis,
              borderWidth: 1
            }
          }
        }
      ]
    },
    true
  );
}

function resizeCharts() {
  trendChart?.resize();
  heatmapChart?.resize();
}

watch(
  () => normalizedAgentId.value,
  (value, previous) => {
    if (!value) {
      runtimeData.value = null;
      errorMessage.value = '';
      return;
    }
    if (previous && value !== previous) {
      selectedDate.value = resolveTodayDate();
      windowDays.value = 14;
    }
  },
  { immediate: true }
);

watch([() => normalizedAgentId.value, windowDays, selectedDate], () => {
  if (!normalizedAgentId.value) {
    return;
  }
  void loadRuntimeRecords();
}, { immediate: true });

watch(
  () => themeStore.mode,
  () => {
    renderTrendChart();
    renderHeatmapChart();
  }
);

onMounted(() => {
  window.addEventListener('resize', resizeCharts);
});

onBeforeUnmount(() => {
  window.removeEventListener('resize', resizeCharts);
  trendChart?.dispose();
  heatmapChart?.dispose();
  trendChart = null;
  heatmapChart = null;
});
</script>

<style scoped>
.messenger-agent-runtime {
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-height: 0;
}

.messenger-agent-runtime-toolbar {
  display: flex;
  flex-wrap: wrap;
  justify-content: space-between;
  align-items: flex-end;
  gap: 10px;
}

.messenger-agent-runtime-range {
  display: inline-flex;
  align-items: center;
  padding: 0 12px;
  height: 34px;
  border-radius: 9px;
  border: 1px solid var(--hula-border);
  background: var(--hula-center-bg);
  color: var(--hula-muted);
  font-size: 12px;
}

.messenger-agent-runtime-control {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 130px;
  font-size: 12px;
  color: var(--hula-muted);
}

.messenger-agent-runtime-control select,
.messenger-agent-runtime-control input {
  border: 1px solid var(--hula-border);
  border-radius: 9px;
  background: var(--hula-center-bg);
  color: var(--hula-text);
  height: 34px;
  padding: 0 10px;
  font-size: 12px;
}

.messenger-agent-runtime-control select:focus-visible,
.messenger-agent-runtime-control input:focus-visible {
  outline: 2px solid rgba(var(--ui-accent-rgb), 0.32);
  outline-offset: 1px;
}

.messenger-agent-runtime-error {
  border: 1px solid rgba(193, 64, 83, 0.26);
  border-radius: 9px;
  background: rgba(193, 64, 83, 0.08);
  color: var(--hula-danger);
  font-size: 12px;
  padding: 8px 10px;
}

.messenger-agent-runtime-cards {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(130px, 1fr));
  gap: 8px;
}

.messenger-agent-runtime-card {
  border: 1px solid var(--hula-border);
  border-radius: 10px;
  background: var(--hula-center-bg);
  padding: 10px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  transition:
    border-color var(--messenger-motion-fast) var(--messenger-ease-standard),
    background-color var(--messenger-motion-fast) var(--messenger-ease-standard);
}

.messenger-agent-runtime-label {
  font-size: 12px;
  color: var(--hula-muted);
}

.messenger-agent-runtime-value {
  font-size: 16px;
  font-weight: 700;
  color: var(--hula-text);
}

.messenger-agent-runtime-panel {
  border: 1px solid var(--hula-border);
  border-radius: 12px;
  background: var(--hula-center-bg);
  padding: 10px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.messenger-agent-runtime-panel-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.messenger-agent-runtime-panel-title {
  font-size: 13px;
  font-weight: 700;
  color: var(--hula-text);
}

.messenger-agent-runtime-panel-meta {
  font-size: 12px;
  color: var(--hula-muted);
}

.messenger-agent-runtime-chart {
  width: 100%;
  min-height: 260px;
}

.messenger-agent-runtime-heatmap {
  width: 100%;
  min-height: 320px;
}

@media (max-width: 1024px) {
  .messenger-agent-runtime-chart {
    min-height: 220px;
  }

  .messenger-agent-runtime-heatmap {
    min-height: 280px;
  }
}
</style>
