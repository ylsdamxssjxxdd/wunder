<template>
  <div class="messenger-agent-runtime">
    <div v-if="!normalizedAgentId" class="messenger-list-empty">
      {{ t('chat.features.agentMissing') }}
    </div>

    <template v-else>
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
          <div class="messenger-agent-runtime-value">{{ formatBilledTokens(summary.billed_tokens) }}</div>
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
        <div v-else ref="heatmapWrapRef" class="messenger-agent-runtime-heatmap-wrap">
          <div class="messenger-agent-runtime-tool-heatmap" :style="{ '--heatmap-rows': String(heatmapRows) }">
            <article
              v-for="item in heatmapTiles"
              :key="item.tool"
              class="messenger-agent-runtime-tool-heatmap-item"
              :style="{ backgroundColor: item.color, color: item.textColor }"
              :title="
                t('messenger.agent.runtime.heatmapTileTitle', {
                  name: item.tool,
                  count: formatHeatmapCount(item.totalCalls)
                })
              "
            >
              <i :class="`fa-solid ${item.icon}`" aria-hidden="true"></i>
              <span class="messenger-agent-runtime-tool-name">{{ item.tool }}</span>
            </article>
          </div>
        </div>
      </section>
    </template>
  </div>
</template>

<script setup lang="ts">
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
  tool_calls: number;
};

type RuntimeHeatmapItem = {
  tool: string;
  total_calls: number;
};

type RuntimeHeatmapTile = {
  tool: string;
  totalCalls: number;
  color: string;
  textColor: string;
  icon: string;
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

const TOOL_HEATMAP_ZERO_RGB = [230, 233, 240] as const;
const TOOL_HEATMAP_MAX_VALUE = 40;
const TOOL_HEATMAP_MIN_LIGHTNESS = 46;
const TOOL_HEATMAP_MAX_LIGHTNESS = 90;
const TOOL_HEATMAP_MIN_SATURATION = 52;
const TOOL_HEATMAP_MAX_SATURATION = 82;
const TOOL_HEATMAP_HUE_ANCHORS = [
  { value: 10, hue: 210 },
  { value: 20, hue: 135 },
  { value: 30, hue: 50 },
  { value: 40, hue: 5 }
] as const;
const TOOL_HEATMAP_TILE_SIZE = 68;
const TOOL_HEATMAP_GAP = 8;
const RUNTIME_TREND_WINDOW_DAYS = 14;
const TOOL_HEATMAP_ICON_RULES: ReadonlyArray<{ keyword: string; icon: string }> = [
  { keyword: '计划面板', icon: 'fa-table-columns' },
  { keyword: '计划看板', icon: 'fa-table-columns' },
  { keyword: 'update_plan', icon: 'fa-table-columns' },
  { keyword: 'plan board', icon: 'fa-table-columns' },
  { keyword: '问询面板', icon: 'fa-circle-question' },
  { keyword: 'question_panel', icon: 'fa-circle-question' },
  { keyword: 'ask_panel', icon: 'fa-circle-question' },
  { keyword: 'question panel', icon: 'fa-circle-question' },
  { keyword: '节点调用', icon: 'fa-diagram-project' },
  { keyword: 'node.invoke', icon: 'fa-diagram-project' },
  { keyword: 'node_invoke', icon: 'fa-diagram-project' },
  { keyword: 'node invoke', icon: 'fa-diagram-project' },
  { keyword: 'gateway_invoke', icon: 'fa-diagram-project' },
  { keyword: '技能调用', icon: 'fa-wand-magic-sparkles' },
  { keyword: 'skill_call', icon: 'fa-wand-magic-sparkles' },
  { keyword: 'skill_get', icon: 'fa-wand-magic-sparkles' },
  { keyword: '子智能体控制', icon: 'fa-diagram-project' },
  { keyword: 'subagent_control', icon: 'fa-diagram-project' },
  { keyword: '智能体蜂群', icon: 'fa-bug' },
  { keyword: 'agent_swarm', icon: 'fa-bug' },
  { keyword: 'swarm_control', icon: 'fa-bug' },
  { keyword: 'a2a观察', icon: 'fa-glasses' },
  { keyword: 'a2a_observe', icon: 'fa-glasses' },
  { keyword: 'a2a等待', icon: 'fa-clock' },
  { keyword: 'a2a_wait', icon: 'fa-clock' },
  { keyword: '休眠等待', icon: 'fa-hourglass-half' },
  { keyword: 'sleep_wait', icon: 'fa-hourglass-half' },
  { keyword: 'sleep', icon: 'fa-hourglass-half' },
  { keyword: 'pause', icon: 'fa-hourglass-half' },
  { keyword: '记忆管理', icon: 'fa-memory' },
  { keyword: 'memory_manager', icon: 'fa-memory' },
  { keyword: 'memory_manage', icon: 'fa-memory' },
  { keyword: 'memory manager', icon: 'fa-memory' },
  { keyword: 'a2a@', icon: 'fa-diagram-project' },
  { keyword: 'a2ui', icon: 'fa-image' },
  { keyword: '列出文件', icon: 'fa-folder-open' },
  { keyword: 'list files', icon: 'fa-folder-open' },
  { keyword: 'list_file', icon: 'fa-folder-open' },
  { keyword: 'list_files', icon: 'fa-folder-open' },
  { keyword: '读取文件', icon: 'fa-file-lines' },
  { keyword: 'read file', icon: 'fa-file-lines' },
  { keyword: 'read_file', icon: 'fa-file-lines' },
  { keyword: '写入文件', icon: 'fa-file-circle-plus' },
  { keyword: 'write file', icon: 'fa-file-circle-plus' },
  { keyword: 'write_file', icon: 'fa-file-circle-plus' },
  { keyword: '应用补丁', icon: 'fa-pen-to-square' },
  { keyword: 'apply patch', icon: 'fa-pen-to-square' },
  { keyword: 'apply_patch', icon: 'fa-pen-to-square' }
];

const trendChartRef = ref<HTMLElement | null>(null);
const heatmapWrapRef = ref<HTMLElement | null>(null);
const loading = ref(false);
const errorMessage = ref('');
const runtimeData = ref<RuntimePayload | null>(null);
const selectedDate = ref(resolveTodayDate());
const normalizedAgentId = computed(() => String(props.agentId || '').trim());
const heatmapRows = ref(3);

let trendChart: ECharts | null = null;
let requestSerial = 0;
let heatmapResizeObserver: ResizeObserver | null = null;

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

const heatmapTiles = computed<RuntimeHeatmapTile[]>(() =>
  heatmapItems.value.map((item) => {
    const { color, rgb } = resolveHeatmapColor(item.total_calls);
    return {
      tool: item.tool,
      totalCalls: item.total_calls,
      color,
      textColor: resolveHeatmapTextColor(rgb),
      icon: resolveToolIcon(item.tool)
    };
  })
);

const hasHeatmapData = computed(() => heatmapTiles.value.length > 0);

function toSafeNumber(value: unknown): number {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

function resolveTodayDate() {
  const now = new Date();
  const local = new Date(now.getTime() - now.getTimezoneOffset() * 60_000);
  return local.toISOString().slice(0, 10);
}

function formatNumber(value: number) {
  return new Intl.NumberFormat(language.value || 'zh-CN').format(Math.max(0, Math.round(value)));
}

function formatBilledTokens(value: number) {
  const normalized = Math.max(0, toSafeNumber(value));
  const locale = language.value || 'zh-CN';
  const formatScaled = (scaled: number) =>
    new Intl.NumberFormat(locale, { minimumFractionDigits: 0, maximumFractionDigits: 2 }).format(scaled);
  if (normalized >= 1_000_000) {
    return `${formatScaled(normalized / 1_000_000)}M`;
  }
  if (normalized >= 1_000) {
    return `${formatScaled(normalized / 1_000)}K`;
  }
  return formatNumber(normalized);
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

function formatHeatmapCount(value: number) {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) {
    return '0';
  }
  return String(Math.max(0, Math.round(parsed)));
}

function hslToRgb(hue: number, saturation: number, lightness: number): [number, number, number] {
  const h = ((Number(hue) || 0) % 360) / 360;
  const s = Math.max(0, Math.min(1, (Number(saturation) || 0) / 100));
  const l = Math.max(0, Math.min(1, (Number(lightness) || 0) / 100));
  if (s === 0) {
    const gray = Math.round(l * 255);
    return [gray, gray, gray];
  }
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
  const p = 2 * l - q;
  const hueToRgb = (value: number) => {
    let channel = value;
    if (channel < 0) channel += 1;
    if (channel > 1) channel -= 1;
    if (channel < 1 / 6) return p + (q - p) * 6 * channel;
    if (channel < 1 / 2) return q;
    if (channel < 2 / 3) return p + (q - p) * (2 / 3 - channel) * 6;
    return p;
  };
  return [
    Math.round(hueToRgb(h + 1 / 3) * 255),
    Math.round(hueToRgb(h) * 255),
    Math.round(hueToRgb(h - 1 / 3) * 255)
  ];
}

function resolveHeatmapHue(value: number): number {
  if (value <= TOOL_HEATMAP_HUE_ANCHORS[0].value) {
    return TOOL_HEATMAP_HUE_ANCHORS[0].hue;
  }
  for (let index = 1; index < TOOL_HEATMAP_HUE_ANCHORS.length; index += 1) {
    const next = TOOL_HEATMAP_HUE_ANCHORS[index];
    if (value <= next.value) {
      const prev = TOOL_HEATMAP_HUE_ANCHORS[index - 1];
      const span = next.value - prev.value || 1;
      const ratio = (value - prev.value) / span;
      return prev.hue + (next.hue - prev.hue) * ratio;
    }
  }
  return TOOL_HEATMAP_HUE_ANCHORS[TOOL_HEATMAP_HUE_ANCHORS.length - 1].hue;
}

function resolveHeatmapColor(totalCalls: number): { color: string; rgb: [number, number, number] } {
  const value = Math.max(0, Number(totalCalls) || 0);
  if (value <= 0) {
    return {
      color: `rgb(${TOOL_HEATMAP_ZERO_RGB.join(', ')})`,
      rgb: [...TOOL_HEATMAP_ZERO_RGB] as [number, number, number]
    };
  }
  const clamped = Math.min(value, TOOL_HEATMAP_MAX_VALUE);
  const ratio = clamped / TOOL_HEATMAP_MAX_VALUE;
  const hue = resolveHeatmapHue(clamped);
  const saturation =
    TOOL_HEATMAP_MIN_SATURATION +
    ratio * (TOOL_HEATMAP_MAX_SATURATION - TOOL_HEATMAP_MIN_SATURATION);
  const lightness =
    TOOL_HEATMAP_MAX_LIGHTNESS - ratio * (TOOL_HEATMAP_MAX_LIGHTNESS - TOOL_HEATMAP_MIN_LIGHTNESS);
  const rgb = hslToRgb(hue, saturation, lightness);
  return { color: `rgb(${rgb.join(', ')})`, rgb };
}

function resolveHeatmapTextColor(rgb: [number, number, number]): string {
  const [r, g, b] = rgb;
  const luminance = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
  return luminance >= 0.65 ? '#0f172a' : '#f8fafc';
}

function normalizeToolMatchKey(value: string): string {
  return String(value || '')
    .trim()
    .toLowerCase()
    .replace(/[\s_.-]+/g, '');
}

function matchesToolKeyword(lowerName: string, normalizedName: string, keyword: string): boolean {
  if (!keyword) {
    return false;
  }
  if (lowerName.includes(keyword)) {
    return true;
  }
  const normalizedKeyword = normalizeToolMatchKey(keyword);
  return normalizedKeyword ? normalizedName.includes(normalizedKeyword) : false;
}

function resolveToolIcon(name: string): string {
  const toolName = String(name || '').trim();
  const lowerName = toolName.toLowerCase();
  const normalizedName = normalizeToolMatchKey(lowerName);
  if (lowerName === 'wunder@excute' || lowerName.endsWith('@wunder@excute')) {
    return 'fa-dragon';
  }
  if (lowerName === 'wunder@doc2md' || lowerName.endsWith('@wunder@doc2md')) {
    return 'fa-file-lines';
  }
  for (const rule of TOOL_HEATMAP_ICON_RULES) {
    if (matchesToolKeyword(lowerName, normalizedName, rule.keyword)) {
      return rule.icon;
    }
  }
  if (toolName.includes('@')) {
    const atCount = toolName.split('@').length - 1;
    return atCount >= 2 ? 'fa-wrench' : 'fa-plug';
  }
  if (
    matchesToolKeyword(lowerName, normalizedName, '执行命令') ||
    matchesToolKeyword(lowerName, normalizedName, 'run command') ||
    matchesToolKeyword(lowerName, normalizedName, 'execute command') ||
    matchesToolKeyword(lowerName, normalizedName, 'execute_command') ||
    matchesToolKeyword(lowerName, normalizedName, 'shell')
  ) {
    return 'fa-terminal';
  }
  if (
    matchesToolKeyword(lowerName, normalizedName, 'ptc') ||
    matchesToolKeyword(lowerName, normalizedName, 'programmatic_tool_call')
  ) {
    return 'fa-code';
  }
  if (
    matchesToolKeyword(lowerName, normalizedName, '定时任务') ||
    matchesToolKeyword(lowerName, normalizedName, '计划任务') ||
    matchesToolKeyword(lowerName, normalizedName, 'cron') ||
    matchesToolKeyword(lowerName, normalizedName, 'schedule') ||
    matchesToolKeyword(lowerName, normalizedName, 'scheduled') ||
    matchesToolKeyword(lowerName, normalizedName, 'timer') ||
    matchesToolKeyword(lowerName, normalizedName, 'schedule_task')
  ) {
    return 'fa-clock';
  }
  if (
    matchesToolKeyword(lowerName, normalizedName, '搜索') ||
    matchesToolKeyword(lowerName, normalizedName, '检索') ||
    matchesToolKeyword(lowerName, normalizedName, 'search') ||
    matchesToolKeyword(lowerName, normalizedName, 'query') ||
    matchesToolKeyword(lowerName, normalizedName, 'retrieve') ||
    matchesToolKeyword(lowerName, normalizedName, 'search_content')
  ) {
    return 'fa-magnifying-glass';
  }
  if (
    matchesToolKeyword(lowerName, normalizedName, '读取') ||
    matchesToolKeyword(lowerName, normalizedName, '写入') ||
    matchesToolKeyword(lowerName, normalizedName, '编辑') ||
    matchesToolKeyword(lowerName, normalizedName, '替换') ||
    matchesToolKeyword(lowerName, normalizedName, '列出') ||
    matchesToolKeyword(lowerName, normalizedName, 'read') ||
    matchesToolKeyword(lowerName, normalizedName, 'write') ||
    matchesToolKeyword(lowerName, normalizedName, 'edit') ||
    matchesToolKeyword(lowerName, normalizedName, 'replace') ||
    matchesToolKeyword(lowerName, normalizedName, 'list')
  ) {
    return 'fa-file-lines';
  }
  if (
    matchesToolKeyword(lowerName, normalizedName, '知识') ||
    matchesToolKeyword(lowerName, normalizedName, 'knowledge')
  ) {
    return 'fa-book';
  }
  if (
    matchesToolKeyword(lowerName, normalizedName, '最终回复') ||
    matchesToolKeyword(lowerName, normalizedName, 'final answer') ||
    matchesToolKeyword(lowerName, normalizedName, 'final_response')
  ) {
    return 'fa-flag-checkered';
  }
  return 'fa-toolbox';
}

function updateHeatmapRows() {
  const wrapHeight = heatmapWrapRef.value?.clientHeight || 0;
  if (!wrapHeight) {
    heatmapRows.value = 3;
    return;
  }
  heatmapRows.value = Math.max(
    1,
    Math.floor((wrapHeight + TOOL_HEATMAP_GAP) / (TOOL_HEATMAP_TILE_SIZE + TOOL_HEATMAP_GAP))
  );
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
        tools: '#f59e0b'
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
        tools: '#d97706'
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
      days: RUNTIME_TREND_WINDOW_DAYS,
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
    updateHeatmapRows();
  }
}

function handleTrendChartClick(params: { dataIndex?: number }) {
  const index = Number(params?.dataIndex);
  if (!Number.isInteger(index) || index < 0 || index >= dailyRows.value.length) {
    return;
  }
  const nextDate = String(dailyRows.value[index]?.date || '').trim();
  if (!nextDate || nextDate === selectedDate.value) {
    return;
  }
  selectedDate.value = nextDate;
}

function renderTrendChart() {
  const container = trendChartRef.value;
  if (!container) {
    return;
  }
  if (!trendChart) {
    trendChart = init(container);
    trendChart.on('click', handleTrendChartClick);
  }
  const palette = resolveChartPalette();
  const dates = dailyRows.value.map((item) => String(item.date || '').slice(5));
  const runtimeMinutes = dailyRows.value.map((item) => Number((item.runtime_seconds / 60).toFixed(2)));
  const billedTokens = dailyRows.value.map((item) => item.billed_tokens);
  const quotaConsumed = dailyRows.value.map((item) => item.quota_consumed);
  const toolCalls = dailyRows.value.map((item) => item.tool_calls);
  const runtimeSeriesName = t('messenger.agent.runtime.series.runtime');
  const tokenSeriesName = t('messenger.agent.runtime.series.tokens');
  const quotaSeriesName = t('messenger.agent.runtime.series.quota');
  const toolSeriesName = t('messenger.agent.runtime.series.tools');
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
        selectedMode: 'single',
        data: [runtimeSeriesName, tokenSeriesName, quotaSeriesName, toolSeriesName],
        selected: {
          [runtimeSeriesName]: false,
          [tokenSeriesName]: true,
          [quotaSeriesName]: false,
          [toolSeriesName]: false
        }
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
        },
        {
          name: toolSeriesName,
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

function resizeCharts() {
  trendChart?.resize();
  updateHeatmapRows();
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
    }
  },
  { immediate: true }
);

watch([() => normalizedAgentId.value, selectedDate], () => {
  if (!normalizedAgentId.value) {
    return;
  }
  void loadRuntimeRecords();
}, { immediate: true });

watch(
  () => themeStore.mode,
  () => {
    renderTrendChart();
    updateHeatmapRows();
  }
);

watch(
  () => heatmapTiles.value.length,
  async () => {
    await nextTick();
    updateHeatmapRows();
  }
);

watch(
  () => heatmapWrapRef.value,
  (value, previous) => {
    if (heatmapResizeObserver && previous) {
      heatmapResizeObserver.unobserve(previous);
    }
    if (heatmapResizeObserver && value) {
      heatmapResizeObserver.observe(value);
    }
    updateHeatmapRows();
  }
);

onMounted(() => {
  window.addEventListener('resize', resizeCharts);
  if (typeof ResizeObserver !== 'undefined') {
    heatmapResizeObserver = new ResizeObserver(() => {
      updateHeatmapRows();
    });
    if (heatmapWrapRef.value) {
      heatmapResizeObserver.observe(heatmapWrapRef.value);
    }
  }
  updateHeatmapRows();
});

onBeforeUnmount(() => {
  window.removeEventListener('resize', resizeCharts);
  trendChart?.dispose();
  trendChart = null;
  if (heatmapResizeObserver) {
    heatmapResizeObserver.disconnect();
    heatmapResizeObserver = null;
  }
});
</script>

<style scoped>
.messenger-agent-runtime {
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-height: 0;
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

.messenger-agent-runtime-heatmap-wrap {
  width: 100%;
  min-height: 320px;
  overflow-x: auto;
  overflow-y: hidden;
  padding-bottom: 2px;
}

.messenger-agent-runtime-tool-heatmap {
  width: max-content;
  min-width: 100%;
  display: grid;
  gap: 8px;
  grid-auto-flow: column;
  grid-auto-columns: 72px;
  grid-template-rows: repeat(var(--heatmap-rows, 3), 68px);
  grid-auto-rows: 68px;
}

.messenger-agent-runtime-tool-heatmap-item {
  border: 0;
  border-radius: 10px;
  padding: 6px;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-direction: column;
  gap: 6px;
  text-align: center;
  font-size: 11px;
}

.messenger-agent-runtime-tool-heatmap-item i {
  font-size: 16px;
}

.messenger-agent-runtime-tool-name {
  width: 100%;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

@media (max-width: 1024px) {
  .messenger-agent-runtime-chart {
    min-height: 220px;
  }

  .messenger-agent-runtime-heatmap-wrap {
    min-height: 280px;
  }
}
</style>
