<template>
  <div class="messenger-agent-daily-trend">
    <div class="messenger-agent-daily-trend-summary">
      <div
        v-for="card in summaryCards"
        :key="card.key"
        class="messenger-agent-daily-trend-stat"
      >
        <span class="messenger-agent-daily-trend-stat-label">{{ card.label }}</span>
        <span class="messenger-agent-daily-trend-stat-value">{{ card.value }}</span>
      </div>
    </div>
    <div v-if="errorMessage" class="messenger-agent-daily-trend-error">
      {{ errorMessage }}
    </div>
    <div v-if="!dailyRows.length && !loading" class="messenger-list-empty">
      {{ t('messenger.agent.runtime.empty') }}
    </div>
    <div v-else ref="trendChartRef" class="messenger-agent-daily-trend-chart"></div>
  </div>
</template>

<script setup lang="ts">
// AI生成
/**
 * 日运行趋势图（账号设置页）：按天聚合的运行时长 / Token 消耗 / 额度消耗折线。
 *
 * 与桌面版账号页保持同一口径：顶部三张近 14 天汇总卡，下方是同窗口的日趋势折线；
 * 一次请求同时喂给汇总卡和图表，避免两端统计来源不一致。
 */
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { LineChart } from 'echarts/charts';
import { GridComponent, LegendComponent, TooltipComponent } from 'echarts/components';
import { type ECharts, init, use } from 'echarts/core';
import { CanvasRenderer } from 'echarts/renderers';

import { getAgentRuntimeRecords } from '@/api/agents';
import { useI18n } from '@/i18n';
import { useThemeStore } from '@/stores/theme';

use([CanvasRenderer, GridComponent, LegendComponent, LineChart, TooltipComponent]);

type RuntimeDailyRecord = {
  date: string;
  runtime_seconds: number;
  billed_tokens: number;
  quota_consumed: number;
};

type RuntimeSummary = {
  runtime_seconds?: number;
  consumed_tokens?: number;
  quota_consumed?: number;
};

type RuntimePayload = {
  daily?: Array<Record<string, unknown>>;
  summary?: RuntimeSummary;
};

const props = defineProps({
  agentId: {
    type: String,
    default: ''
  },
  active: {
    type: Boolean,
    default: true
  }
});

const { t, language } = useI18n();
const themeStore = useThemeStore();

const TREND_LOAD_DAYS = 14;

const trendChartRef = ref<HTMLElement | null>(null);
const loading = ref(false);
const errorMessage = ref('');
const runtimeData = ref<RuntimePayload | null>(null);
const normalizedAgentId = computed(() => String(props.agentId || '').trim());
const isPanelActive = computed(() => props.active !== false);

let trendChart: ECharts | null = null;
let requestSerial = 0;

const dailyRows = computed<RuntimeDailyRecord[]>(() => {
  const source = Array.isArray(runtimeData.value?.daily) ? runtimeData.value?.daily : [];
  return (source || []).map((item) => ({
    date: String(item?.date || ''),
    runtime_seconds: toSafeNumber(item?.runtime_seconds),
    billed_tokens: Math.max(0, toSafeNumber(item?.consumed_tokens ?? item?.billed_tokens)),
    quota_consumed: Math.max(0, toSafeNumber(item?.quota_consumed))
  }));
});

function toSafeNumber(value: unknown): number {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

function formatRuntime(seconds: number) {
  const normalized = Math.max(0, Math.round(toSafeNumber(seconds)));
  return `${Math.floor(normalized / 3600)}h ${Math.floor((normalized % 3600) / 60)}m`;
}

function formatBilledTokens(value: number) {
  const normalized = Math.max(0, toSafeNumber(value));
  const locale = language.value || 'zh-CN';
  // Same precision as the desktop usage cards: 1 decimal for K, 2 for M.
  const formatScaled = (scaled: number, digits: number) =>
    new Intl.NumberFormat(locale, { minimumFractionDigits: 0, maximumFractionDigits: digits }).format(scaled);
  if (normalized >= 1_000_000) {
    return `${formatScaled(normalized / 1_000_000, 2)}M`;
  }
  if (normalized >= 1_000) {
    return `${formatScaled(normalized / 1_000, 1)}K`;
  }
  return new Intl.NumberFormat(locale).format(Math.max(0, Math.round(normalized)));
}

// 汇总卡与图表共用同一份 summary/daily，窗口由后端 TREND_LOAD_DAYS 决定。
const summaryCards = computed(() => {
  const summary: RuntimeSummary = runtimeData.value?.summary || {};
  return [
    {
      key: 'runtime',
      label: t('messenger.agent.runtime.summary.runtime'),
      value: formatRuntime(toSafeNumber(summary.runtime_seconds))
    },
    {
      key: 'tokens',
      label: t('messenger.agent.runtime.summary.tokens'),
      value: formatBilledTokens(toSafeNumber(summary.consumed_tokens))
    },
    {
      key: 'quota',
      label: t('messenger.agent.runtime.summary.quota'),
      value: formatBilledTokens(toSafeNumber(summary.quota_consumed))
    }
  ];
});

function resolveChartPalette() {
  const dark = themeStore.palette === 'tech-blue';
  return dark
    ? {
        text: '#d1d5db',
        axis: '#4b5563',
        split: 'rgba(148, 163, 184, 0.2)',
        tooltipBg: '#111827',
        tooltipBorder: '#374151',
        runtime: '#60a5fa',
        tokens: '#a78bfa',
        quota: '#f59e0b'
      }
    : {
        text: '#475569',
        axis: '#cbd5e1',
        split: '#ecedf0',
        tooltipBg: '#ffffff',
        tooltipBorder: '#dbe1ea',
        runtime: '#2563eb',
        tokens: '#7c3aed',
        quota: '#d97706'
      };
}

async function loadTrendRecords() {
  if (!normalizedAgentId.value) {
    runtimeData.value = null;
    return;
  }
  const serial = ++requestSerial;
  loading.value = true;
  errorMessage.value = '';
  try {
    const { data } = await getAgentRuntimeRecords(normalizedAgentId.value, {
      days: TREND_LOAD_DAYS
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
      errorMessage.value =
        (error as { response?: { data?: { detail?: string } } })?.response?.data?.detail ||
        t('messenger.agent.runtime.loadFailed');
    }
  } finally {
    if (serial !== requestSerial) {
      return;
    }
    loading.value = false;
    await nextTick();
    renderTrendChart();
  }
}

function renderTrendChart() {
  const container = trendChartRef.value;
  if (!container) {
    return;
  }
  if (!trendChart) {
    trendChart = init(container);
  }
  const palette = resolveChartPalette();
  const dates = dailyRows.value.map((item) => String(item.date || '').slice(5));
  const runtimeMinutes = dailyRows.value.map((item) => Number((item.runtime_seconds / 60).toFixed(2)));
  const billedTokens = dailyRows.value.map((item) => item.billed_tokens);
  const quotaConsumed = dailyRows.value.map((item) => item.quota_consumed);
  const runtimeSeriesName = t('messenger.agent.runtime.series.runtime');
  const tokenSeriesName = t('messenger.agent.runtime.series.tokens');
  const quotaSeriesName = t('messenger.agent.runtime.series.quota');
  trendChart.setOption(
    {
      animation: false,
      color: [palette.runtime, palette.tokens, palette.quota],
      tooltip: {
        trigger: 'axis',
        backgroundColor: palette.tooltipBg,
        borderColor: palette.tooltipBorder,
        textStyle: { color: palette.text }
      },
      legend: {
        top: 0,
        textStyle: { color: palette.text },
        selectedMode: 'single',
        data: [runtimeSeriesName, tokenSeriesName, quotaSeriesName],
        selected: {
          [runtimeSeriesName]: false,
          [tokenSeriesName]: true,
          [quotaSeriesName]: false
        }
      },
      grid: { left: 44, right: 24, top: 28, bottom: 28 },
      xAxis: {
        type: 'category',
        data: dates,
        axisLine: { lineStyle: { color: palette.axis } },
        axisLabel: { color: palette.text }
      },
      yAxis: {
        type: 'value',
        axisLine: { lineStyle: { color: palette.axis } },
        axisLabel: {
          color: palette.text,
          formatter: (value: number) => formatBilledTokens(value)
        },
        splitLine: { lineStyle: { color: palette.split } }
      },
      series: [
        {
          name: runtimeSeriesName,
          type: 'line',
          smooth: true,
          symbolSize: 6,
          data: runtimeMinutes
        },
        {
          name: tokenSeriesName,
          type: 'line',
          smooth: true,
          symbolSize: 6,
          data: billedTokens
        },
        {
          name: quotaSeriesName,
          type: 'line',
          smooth: true,
          symbolSize: 6,
          data: quotaConsumed
        }
      ]
    },
    true
  );
}

function resizeChart() {
  trendChart?.resize();
}

watch(
  () => [normalizedAgentId.value, isPanelActive.value] as const,
  ([agentId, active], previous) => {
    if (!agentId || !active) {
      return;
    }
    if (previous?.[1] === false && previous?.[0] === agentId) {
      // KeepAlive 重新激活：容器尺寸可能已变化，重渲染并复位。
      void nextTick(() => {
        resizeChart();
        renderTrendChart();
      });
      return;
    }
    void loadTrendRecords();
  },
  { immediate: true }
);

watch(
  () => themeStore.palette,
  () => {
    renderTrendChart();
  }
);

onMounted(() => {
  window.addEventListener('resize', resizeChart);
});

onBeforeUnmount(() => {
  window.removeEventListener('resize', resizeChart);
  trendChart?.dispose();
  trendChart = null;
});
</script>

<style scoped>
.messenger-agent-daily-trend {
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-height: 0;
}

.messenger-agent-daily-trend-summary {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 12px;
}

.messenger-agent-daily-trend-stat {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 12px;
  border: 1px solid var(--mz-border, #e8e6e3);
  border-radius: 12px;
  background: var(--mz-sidebar, #f6f5f3);
}

.messenger-agent-daily-trend-stat-label {
  color: var(--mz-text-muted, #8a8f99);
  font-size: 11px;
}

.messenger-agent-daily-trend-stat-value {
  color: var(--mz-text, #1f2329);
  font-size: 22px;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}

.messenger-agent-daily-trend-error {
  border: 1px solid rgba(193, 64, 83, 0.26);
  border-radius: 9px;
  background: rgba(193, 64, 83, 0.08);
  color: var(--hula-danger);
  font-size: 12px;
  padding: 8px 10px;
}

.messenger-agent-daily-trend-chart {
  width: 100%;
  min-height: 260px;
}
</style>
