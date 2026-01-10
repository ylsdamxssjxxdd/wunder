import { APP_CONFIG } from "../app.config.js?v=20260110-04";
import { elements } from "./elements.js?v=20260110-04";
import { state } from "./state.js";
import { appendLog } from "./log.js?v=20260108-02";
import {
  formatBytes,
  formatDuration,
  formatDurationLong,
  formatTimestamp,
  formatTokenCount,
} from "./utils.js?v=20251229-02";
import { getWunderBase } from "./api.js";
import { notify } from "./notify.js";
import { getCurrentLanguage, t } from "./i18n.js?v=20260110-03";

const ONE_HOUR_MS = 60 * 60 * 1000;
const DEFAULT_MONITOR_TIME_RANGE_HOURS = 3;
// Token 趋势默认展示的时间桶数量，避免折线图从最早记录开始导致卡顿
const TOKEN_TREND_MAX_BUCKETS = 24;
// Token 趋势保留的最大时间桶数量，避免长期运行累积过多历史数据
const TOKEN_TREND_RETENTION_BUCKETS = 96;
// 用户管理线程列表分页尺寸，避免一次渲染过多行
const DEFAULT_MONITOR_SESSION_PAGE_SIZE = 100;
let tokenTrendChart = null;
let statusChart = null;
let statusChartClickBound = false;
let tokenTrendZoomBound = false;
let mcpToolNameSet = new Set();
// 监控轮询配置：full 为完整监控面板，sessions 为用户管理页轻量轮询
let monitorPollMode = "full";
let monitorPollIntervalMs = APP_CONFIG.monitorPollIntervalMs;
// 工具热力图按总调用次数渐变配色（10/20/30/40 次为蓝/绿/黄/红）
const TOOL_HEATMAP_ZERO_RGB = [230, 233, 240];
const TOOL_HEATMAP_MAX_VALUE = 40;
const TOOL_HEATMAP_MIN_LIGHTNESS = 46;
const TOOL_HEATMAP_MAX_LIGHTNESS = 90;
const TOOL_HEATMAP_MIN_SATURATION = 52;
const TOOL_HEATMAP_MAX_SATURATION = 82;
const TOOL_HEATMAP_HUE_ANCHORS = [
  { value: 10, hue: 210 },
  { value: 20, hue: 135 },
  { value: 30, hue: 50 },
  { value: 40, hue: 5 },
];
const TOOL_HEATMAP_TILE_SIZE = 68;
const TOOL_HEATMAP_GAP = 8;
const TOOL_LIST_CACHE_MS = 5 * 60 * 1000;
// 热力图需要区分常见文件操作工具的图标，避免全部显示为同一文件样式
const TOOL_HEATMAP_ICON_RULES = [
  { keyword: "a2a\u89c2\u5bdf", icon: "fa-glasses" },
  { keyword: "a2a_observe", icon: "fa-glasses" },
  { keyword: "a2a\u7b49\u5f85", icon: "fa-clock" },
  { keyword: "a2a_wait", icon: "fa-clock" },
  { keyword: "a2a@", icon: "fa-diagram-project" },
  { keyword: "a2ui", icon: "fa-image" },
  { keyword: "\u5217\u51fa\u6587\u4ef6", icon: "fa-folder-open" },
  { keyword: "list files", icon: "fa-folder-open" },
  { keyword: "list_file", icon: "fa-folder-open" },
  { keyword: "list_files", icon: "fa-folder-open" },
  { keyword: "\u8bfb\u53d6\u6587\u4ef6", icon: "fa-file-lines" },
  { keyword: "read file", icon: "fa-file-lines" },
  { keyword: "read_file", icon: "fa-file-lines" },
  { keyword: "\u5199\u5165\u6587\u4ef6", icon: "fa-file-circle-plus" },
  { keyword: "write file", icon: "fa-file-circle-plus" },
  { keyword: "write_file", icon: "fa-file-circle-plus" },
  { keyword: "\u7f16\u8f91\u6587\u4ef6", icon: "fa-pen-to-square" },
  { keyword: "edit file", icon: "fa-pen-to-square" },
  { keyword: "edit_file", icon: "fa-pen-to-square" },
  { keyword: "\u66ff\u6362\u6587\u672c", icon: "fa-arrow-right-arrow-left" },
  { keyword: "replace text", icon: "fa-arrow-right-arrow-left" },
  { keyword: "replace_text", icon: "fa-arrow-right-arrow-left" },
];
// 线程状态环图配色与图例配置
const STATUS_CHART_COLORS = ["#38bdf8", "#22c55e", "#fb7185", "#fbbf24"];
const STATUS_CHART_EMPTY_COLOR = "#ffffff";
const getStatusLegend = () => [
  t("monitor.status.active"),
  t("monitor.status.finished"),
  t("monitor.status.failed"),
  t("monitor.status.cancelled"),
];
const STATUS_CHART_EMPTY_NAME = "__empty__";
// 线程状态图例与后端状态字段的映射，便于点击后过滤记录
const getStatusLabelToKey = () => ({
  [t("monitor.status.active")]: "active",
  [t("monitor.status.finished")]: "finished",
  [t("monitor.status.failed")]: "error",
  [t("monitor.status.cancelled")]: "cancelled",
});

// 兼容旧版本状态结构，避免缓存旧 state.js 时导致监控图表异常
const ensureMonitorState = () => {
  if (!state.monitor) {
    state.monitor = {
      sessions: [],
      selected: null,
      tokenTrend: [],
      tokenDeltas: [],
      tokenUsageBySession: {},
      toolStats: [],
      availableTools: [],
      availableToolsUpdatedAt: 0,
      availableToolsLanguage: "",
      tokenZoomLocked: false,
      tokenZoomInitialized: false,
      userFilter: "",
      timeRangeHours: DEFAULT_MONITOR_TIME_RANGE_HOURS,
      serviceSnapshot: null,
      pagination: {
        pageSize: DEFAULT_MONITOR_SESSION_PAGE_SIZE,
        activePage: 1,
        historyPage: 1,
      },
      timeFilter: {
        enabled: false,
        start: "",
        end: "",
      },
    };
  }
  if (!Array.isArray(state.monitor.tokenTrend)) {
    state.monitor.tokenTrend = [];
  }
  if (!Array.isArray(state.monitor.tokenDeltas)) {
    state.monitor.tokenDeltas = [];
  }
  if (!state.monitor.tokenUsageBySession || typeof state.monitor.tokenUsageBySession !== "object") {
    state.monitor.tokenUsageBySession = {};
  }
  if (!Array.isArray(state.monitor.toolStats)) {
    state.monitor.toolStats = [];
  }
  if (!Array.isArray(state.monitor.availableTools)) {
    state.monitor.availableTools = [];
  }
  if (!Number.isFinite(state.monitor.availableToolsUpdatedAt)) {
    state.monitor.availableToolsUpdatedAt = 0;
  }
  if (typeof state.monitor.availableToolsLanguage !== "string") {
    state.monitor.availableToolsLanguage = "";
  }
  if (!Array.isArray(state.monitor.sessions)) {
    state.monitor.sessions = [];
  }
  if (typeof state.monitor.tokenZoomLocked !== "boolean") {
    state.monitor.tokenZoomLocked = false;
  }
  if (typeof state.monitor.tokenZoomInitialized !== "boolean") {
    state.monitor.tokenZoomInitialized = false;
  }
  if (typeof state.monitor.userFilter !== "string") {
    state.monitor.userFilter = "";
  }
  if (!Number.isFinite(state.monitor.timeRangeHours)) {
    state.monitor.timeRangeHours = DEFAULT_MONITOR_TIME_RANGE_HOURS;
  }
  if (!state.monitor.serviceSnapshot) {
    state.monitor.serviceSnapshot = null;
  }
  if (!state.monitor.timeFilter || typeof state.monitor.timeFilter !== "object") {
    state.monitor.timeFilter = {
      enabled: false,
      start: "",
      end: "",
    };
  }
  if (typeof state.monitor.timeFilter.enabled !== "boolean") {
    state.monitor.timeFilter.enabled = false;
  }
  if (typeof state.monitor.timeFilter.start !== "string") {
    state.monitor.timeFilter.start = "";
  }
  if (typeof state.monitor.timeFilter.end !== "string") {
    state.monitor.timeFilter.end = "";
  }
  // 分页状态兼容旧缓存，避免切换用户或刷新后页码异常
  if (!state.monitor.pagination || typeof state.monitor.pagination !== "object") {
    state.monitor.pagination = {
      pageSize: DEFAULT_MONITOR_SESSION_PAGE_SIZE,
      activePage: 1,
      historyPage: 1,
    };
  }
  if (
    !Number.isFinite(state.monitor.pagination.pageSize) ||
    state.monitor.pagination.pageSize <= 0
  ) {
    state.monitor.pagination.pageSize = DEFAULT_MONITOR_SESSION_PAGE_SIZE;
  }
  if (
    !Number.isFinite(state.monitor.pagination.activePage) ||
    state.monitor.pagination.activePage < 1
  ) {
    state.monitor.pagination.activePage = 1;
  }
  if (
    !Number.isFinite(state.monitor.pagination.historyPage) ||
    state.monitor.pagination.historyPage < 1
  ) {
    state.monitor.pagination.historyPage = 1;
  }
};

// 格式化监视时间，保证展示简洁
const formatMonitorHours = (value) => {
  const hours = Number(value);
  if (!Number.isFinite(hours)) {
    return String(DEFAULT_MONITOR_TIME_RANGE_HOURS);
  }
  return hours.toFixed(2).replace(/\.?0+$/, "");
};

// 解析监视时间范围（小时），支持管理员自定义
const resolveMonitorTimeRangeHours = (value) => {
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    return DEFAULT_MONITOR_TIME_RANGE_HOURS;
  }
  return parsed;
};

// 获取当前监视时间范围（小时）
const getMonitorTimeRangeHours = () => resolveMonitorTimeRangeHours(state.monitor.timeRangeHours);

// 获取当前监视时间范围（毫秒），避免小数导致时间戳对齐误差
const getMonitorTimeRangeMs = () =>
  Math.max(1, Math.round(getMonitorTimeRangeHours() * ONE_HOUR_MS));

// 获取 Token 趋势的保留窗口，避免前端长时间运行后堆积过多历史
const getTokenTrendRetentionMs = () => {
  const intervalMs = getMonitorTimeRangeMs();
  if (!intervalMs) {
    return 0;
  }
  return Math.max(intervalMs, intervalMs * TOKEN_TREND_RETENTION_BUCKETS);
};

// 解析筛选时间输入，返回毫秒时间戳
const parseMonitorFilterTimestamp = (value) => {
  if (!value) {
    return null;
  }
  const parsed = new Date(value).getTime();
  return Number.isFinite(parsed) ? parsed : null;
};

// 获取筛选时间范围，未启用或无效时返回 null
const resolveMonitorTimeFilterRange = () => {
  if (!state.monitor.timeFilter?.enabled) {
    return null;
  }
  const start = parseMonitorFilterTimestamp(state.monitor.timeFilter.start);
  const end = parseMonitorFilterTimestamp(state.monitor.timeFilter.end);
  if (!Number.isFinite(start) || !Number.isFinite(end)) {
    return null;
  }
  if (end <= start) {
    return null;
  }
  return { start, end };
};

// 格式化筛选区间标签，便于图表标题提示
const formatMonitorFilterLabel = (range) => {
  const locale = getCurrentLanguage();
  const format = (timestamp) =>
    new Date(timestamp).toLocaleString(locale, {
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
    });
  return t("monitor.filter.range", { start: format(range.start), end: format(range.end) });
};

// 生成监视时间范围的文案标签
const getMonitorTimeRangeLabel = () => {
  const hours = getMonitorTimeRangeHours();
  return t("monitor.window.everyHours", { hours: formatMonitorHours(hours) });
};

// 生成监视时间窗口的文案标签，用于近况统计
const getMonitorTimeWindowLabel = () => {
  const range = resolveMonitorTimeFilterRange();
  if (range) {
    return formatMonitorFilterLabel(range);
  }
  const hours = getMonitorTimeRangeHours();
  return t("monitor.window.recentHours", { hours: formatMonitorHours(hours) });
};

// 同步监视时间标题文案，保持图表描述一致
const updateMonitorChartTitles = () => {
  const label = getMonitorTimeRangeLabel();
  if (elements.serviceTokenTitle) {
    elements.serviceTokenTitle.textContent = t("monitor.chart.tokenTrend", { label });
  }
  if (elements.serviceStatusTitle) {
    const windowLabel = getMonitorTimeWindowLabel();
    elements.serviceStatusTitle.textContent = t("monitor.chart.statusRatio", {
      label: windowLabel,
    });
  }
  if (elements.metricServiceRecentLabel) {
    const windowLabel = getMonitorTimeWindowLabel();
    elements.metricServiceRecentLabel.textContent = t("monitor.metric.recentComplete", {
      label: windowLabel,
    });
  }
  if (elements.toolHeatmapTitle) {
    const windowLabel = getMonitorTimeWindowLabel();
    const text = t("monitor.chart.toolHeatmap", { label: windowLabel });
    const label = elements.toolHeatmapTitle.querySelector("[data-role='title']");
    if (label) {
      label.textContent = text;
    } else {
      elements.toolHeatmapTitle.textContent = text;
    }
  }
  if (elements.metricSandboxCallsLabel) {
    const windowLabel = getMonitorTimeWindowLabel();
    elements.metricSandboxCallsLabel.textContent = t("monitor.metric.recentCalls", {
      label: windowLabel,
    });
  }
  if (elements.metricSandboxSessionsLabel) {
    const windowLabel = getMonitorTimeWindowLabel();
    elements.metricSandboxSessionsLabel.textContent = t("monitor.metric.recentSessions", {
      label: windowLabel,
    });
  }
};

// 规范化监视时间设置并刷新相关展示
const applyMonitorTimeRange = (value, options = {}) => {
  const { resetTrend = false } = options;
  const hours = resolveMonitorTimeRangeHours(value);
  state.monitor.timeRangeHours = hours;
  state.monitor.tokenZoomLocked = false;
  const hoursText = formatMonitorHours(hours);
  if (elements.monitorTimeRange && elements.monitorTimeRange.value !== hoursText) {
    elements.monitorTimeRange.value = hoursText;
  }
  updateMonitorChartTitles();
  if (resetTrend) {
    state.monitor.tokenDeltas = [];
    state.monitor.tokenUsageBySession = {};
  }
  if (state.monitor.sessions.length || state.monitor.serviceSnapshot) {
    renderServiceCharts(state.monitor.serviceSnapshot, state.monitor.sessions);
  } else {
    renderTokenTrendChart();
  }
};

// 同步筛选时间输入框状态
const syncMonitorTimeFilterInputs = () => {
  if (!elements.monitorTimeFilterToggle || !elements.monitorTimeStart || !elements.monitorTimeEnd) {
    return;
  }
  const filter = state.monitor.timeFilter || { enabled: false, start: "", end: "" };
  elements.monitorTimeFilterToggle.checked = Boolean(filter.enabled);
  if (elements.monitorTimeStart.value !== filter.start) {
    elements.monitorTimeStart.value = filter.start;
  }
  if (elements.monitorTimeEnd.value !== filter.end) {
    elements.monitorTimeEnd.value = filter.end;
  }
  const disabled = !filter.enabled;
  elements.monitorTimeStart.disabled = disabled;
  elements.monitorTimeEnd.disabled = disabled;
};

// 应用筛选时间并刷新图表
const applyMonitorTimeFilter = async (options = {}) => {
  const { refresh = false } = options;
  if (!elements.monitorTimeFilterToggle || !elements.monitorTimeStart || !elements.monitorTimeEnd) {
    return;
  }
  state.monitor.timeFilter = {
    enabled: Boolean(elements.monitorTimeFilterToggle.checked),
    start: String(elements.monitorTimeStart.value || ""),
    end: String(elements.monitorTimeEnd.value || ""),
  };
  state.monitor.tokenZoomLocked = false;
  syncMonitorTimeFilterInputs();
  updateMonitorChartTitles();
  const range = resolveMonitorTimeFilterRange();
  if (
    state.monitor.timeFilter.enabled &&
    state.monitor.timeFilter.start &&
    state.monitor.timeFilter.end &&
    !range
  ) {
    notify(t("monitor.filter.invalidRange"), "warning");
    return;
  }
  if (refresh) {
    try {
      await loadMonitorData();
    } catch (error) {
      appendLog(t("monitor.refreshFailed", { message: error.message }));
    }
    return;
  }
  if (state.monitor.sessions.length || state.monitor.serviceSnapshot) {
    renderServiceCharts(state.monitor.serviceSnapshot, state.monitor.sessions);
  } else {
    renderTokenTrendChart();
  }
};

// 初始化图表实例，避免重复创建导致内存占用增长
const ensureMonitorCharts = () => {
  if (!window.echarts) {
    return false;
  }
  if (elements.serviceTokenChart && !tokenTrendChart) {
    tokenTrendChart = window.echarts.init(elements.serviceTokenChart);
  }
  if (elements.serviceStatusChart && !statusChart) {
    statusChart = window.echarts.init(elements.serviceStatusChart);
    statusChartClickBound = false;
  }
  bindStatusChartClick();
  bindTokenTrendZoom();
  return Boolean(tokenTrendChart || statusChart);
};

// 点击线程状态环图时打开对应记录列表
const handleStatusChartClick = (params) => {
  const label = String(params?.name || "");
  if (!label || label === STATUS_CHART_EMPTY_NAME) {
    return;
  }
  const statusKey = getStatusLabelToKey()[label];
  if (!statusKey) {
    return;
  }
  openMonitorStatusModal(label);
};

// 仅绑定一次点击事件，避免重复注册导致多次弹窗
const bindStatusChartClick = () => {
  if (!statusChart || statusChartClickBound) {
    return;
  }
  statusChartClickBound = true;
  statusChart.on("click", handleStatusChartClick);
};

// 监听 Token 趋势图缩放，避免刷新时覆盖用户视图
const bindTokenTrendZoom = () => {
  if (!tokenTrendChart || tokenTrendZoomBound) {
    return;
  }
  tokenTrendZoomBound = true;
  tokenTrendChart.on("datazoom", () => {
    if (state.monitor) {
      state.monitor.tokenZoomLocked = true;
    }
  });
};

// 格式化趋势图时间标签，保留日期便于跨天对比
const formatTokenTrendLabel = (timestamp) =>
  new Date(timestamp).toLocaleString(getCurrentLanguage(), {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });

const formatTokenRate = (value, options = {}) => {
  if (!Number.isFinite(value)) {
    return "-";
  }
  const tokens = Math.max(0, Number(value));
  const useMillion = tokens >= 1_000_000;
  const useThousand = tokens >= 1_000 && tokens < 1_000_000;
  const base = useMillion ? 1_000_000 : useThousand ? 1_000 : 1;
  const unit = useMillion ? "m" : useThousand ? "k" : "";
  const scaled = tokens / base;
  let decimals = 2;
  if (scaled >= 100) {
    decimals = 0;
  } else if (scaled >= 10) {
    decimals = 1;
  }
  const prefix = options.lowerBound ? "≥" : "";
  return `${prefix}${scaled.toFixed(decimals)}${unit} ${t("monitor.detail.tokenRate.unit")}`;
};

const formatDurationPrecise = (seconds) => {
  if (!Number.isFinite(seconds)) {
    return "-";
  }
  const value = Math.max(0, Number(seconds));
  if (value < 1) {
    return `${value.toFixed(2)}s`;
  }
  if (value < 10) {
    return `${value.toFixed(2)}s`;
  }
  if (value < 60) {
    return `${value.toFixed(1)}s`;
  }
  return formatDuration(value);
};

const parseMetricNumber = (value) => {
  if (value === null || value === undefined) {
    return null;
  }
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
};

const buildSpeedMeta = (tokens, duration, options = {}) => {
  if (!Number.isFinite(tokens) || !Number.isFinite(duration)) {
    return options.cached ? t("monitor.detail.speedCached") : "";
  }
  const tokenText = formatTokenCount(tokens);
  const durationText = formatDurationPrecise(duration);
  const key = options.variant === "output" ? "monitor.detail.speedMeta.output" : "monitor.detail.speedMeta.context";
  const parts = [t(key, { tokens: tokenText, duration: durationText })];
  if (options.cached) {
    parts.push(t("monitor.detail.speedCached"));
  }
  return parts.join(" · ");
};

// 按监视时间粒度对齐时间戳，保证刻度从整点开始
const floorToIntervalBoundary = (timestamp, intervalMs) => {
  const date = new Date(timestamp);
  const midnight = new Date(date);
  midnight.setHours(0, 0, 0, 0);
  const offset = timestamp - midnight.getTime();
  const index = Math.floor(offset / intervalMs);
  return midnight.getTime() + index * intervalMs;
};

// 记录 token 增量，便于按小时汇总
const recordTokenDeltas = (sessions) => {
  const usageMap = state.monitor.tokenUsageBySession;
  (sessions || []).forEach((session) => {
    const sessionId = session?.session_id;
    if (!sessionId) {
      return;
    }
    const current = Number(session?.token_usage) || 0;
    const previous = Number(usageMap[sessionId]) || 0;
    const delta = current - previous;
    if (delta > 0) {
      const timestamp = resolveSessionTimestamp(session) || Date.now();
      state.monitor.tokenDeltas.push({ timestamp, value: delta });
    }
    usageMap[sessionId] = current;
  });
  pruneTokenDeltas(Date.now());
};

// 裁剪过旧的 token 增量，避免长期运行后趋势数据膨胀
const pruneTokenDeltas = (nowMs) => {
  const deltas = state.monitor.tokenDeltas;
  if (!Array.isArray(deltas) || !deltas.length) {
    return;
  }
  const timeRange = resolveMonitorTimeFilterRange();
  let cutoff = null;
  if (timeRange && Number.isFinite(timeRange.start)) {
    cutoff = timeRange.start;
  } else {
    const retentionMs = getTokenTrendRetentionMs();
    if (retentionMs) {
      cutoff = nowMs - retentionMs;
    }
  }
  if (!Number.isFinite(cutoff)) {
    return;
  }
  const filtered = deltas.filter((item) => {
    const timestamp = Number(item?.timestamp);
    return Number.isFinite(timestamp) && timestamp >= cutoff;
  });
  if (filtered.length !== deltas.length) {
    state.monitor.tokenDeltas = filtered;
  }
};

// 汇总 token 增量，生成按时间间隔的折线图数据
const buildTokenSeries = (sessions) => {
  const deltas = state.monitor.tokenDeltas || [];
  const intervalMs = getMonitorTimeRangeMs();
  if (!intervalMs) {
    return { labels: [], values: [], latestValue: 0 };
  }
  pruneTokenDeltas(Date.now());
  const timeRange = resolveMonitorTimeFilterRange();
  let now = Date.now();
  if (timeRange) {
    now = timeRange.end;
    const startBoundary = floorToIntervalBoundary(timeRange.start, intervalMs);
    if (!Number.isFinite(startBoundary) || !Number.isFinite(now)) {
      return { labels: [], values: [], latestValue: 0 };
    }
    const totals = new Map();
    if (Array.isArray(deltas)) {
      deltas.forEach((item) => {
        const timestamp = Number(item?.timestamp);
        if (!Number.isFinite(timestamp)) {
          return;
        }
        if (timestamp < timeRange.start || timestamp > timeRange.end) {
          return;
        }
        const bucketIndex = Math.max(0, Math.floor((timestamp - startBoundary) / intervalMs));
        totals.set(bucketIndex, (totals.get(bucketIndex) || 0) + (Number(item?.value) || 0));
      });
    }
    const labels = [formatTokenTrendLabel(startBoundary)];
    const values = [0];
    let cursor = startBoundary;
    let bucketIndex = 0;
    while (cursor + intervalMs <= now) {
      const bucketValue = totals.get(bucketIndex) || 0;
      cursor += intervalMs;
      labels.push(formatTokenTrendLabel(cursor));
      values.push(bucketValue);
      bucketIndex += 1;
    }
    if (cursor < now) {
      const bucketValue = totals.get(bucketIndex) || 0;
      labels.push(formatTokenTrendLabel(now));
      values.push(bucketValue);
    }
    const latestValue = values.length ? values[values.length - 1] : 0;
    return { labels, values, latestValue };
  }
  const sessionStartTimes = (sessions || [])
    .map((session) => parseMonitorTimestamp(session?.start_time))
    .filter((value) => Number.isFinite(value));
  const deltaTimes = Array.isArray(deltas) ? deltas.map((item) => item.timestamp) : [];
  const earliest = Math.min(...[...sessionStartTimes, ...deltaTimes].filter(Number.isFinite));
  const retentionMs = getTokenTrendRetentionMs();
  const retentionStart =
    Number.isFinite(retentionMs) && retentionMs > 0 ? now - retentionMs : null;
  const startBase = Number.isFinite(retentionStart)
    ? Number.isFinite(earliest)
      ? Math.max(earliest, retentionStart)
      : retentionStart
    : earliest;
  if (!Number.isFinite(startBase)) {
    return { labels: [], values: [], latestValue: 0 };
  }
  const startBoundary = floorToIntervalBoundary(startBase, intervalMs);
  // 使用区间索引聚合，避免小数间隔导致的时间戳精度误差
  const totals = new Map();
  if (Array.isArray(deltas)) {
    deltas.forEach((item) => {
      const timestamp = Number(item?.timestamp);
      if (!Number.isFinite(timestamp)) {
        return;
      }
      if (timestamp < startBoundary || timestamp > now) {
        return;
      }
      const bucketIndex = Math.max(0, Math.floor((timestamp - startBoundary) / intervalMs));
      totals.set(bucketIndex, (totals.get(bucketIndex) || 0) + (Number(item?.value) || 0));
    });
  }
  const labels = [formatTokenTrendLabel(startBoundary)];
  const values = [0];
  let cursor = startBoundary;
  let bucketIndex = 0;
  while (cursor + intervalMs <= now) {
    const bucketValue = totals.get(bucketIndex) || 0;
    cursor += intervalMs;
    labels.push(formatTokenTrendLabel(cursor));
    values.push(bucketValue);
    bucketIndex += 1;
  }
  if (cursor < now) {
    const bucketValue = totals.get(bucketIndex) || 0;
    labels.push(formatTokenTrendLabel(now));
    values.push(bucketValue);
  }
  const latestValue = values.length ? values[values.length - 1] : 0;
  return { labels, values, latestValue };
};

// 规范化工具列表，保留类别用于图标选择
const normalizeAvailableTools = (payload) => {
  const tools = [];
  mcpToolNameSet = new Set();
  const pushList = (items, category) => {
    (Array.isArray(items) ? items : []).forEach((item) => {
      const name = String(item?.name ?? "").trim();
      if (!name) {
        return;
      }
      tools.push({ name, category });
      if (category === "mcp") {
        mcpToolNameSet.add(name);
      }
    });
  };
  pushList(payload?.builtin_tools, "builtin");
  pushList(payload?.mcp_tools, "mcp");
  pushList(payload?.knowledge_tools, "knowledge");
  pushList(payload?.skills, "skill");
  pushList(payload?.user_tools, "user");
  pushList(payload?.shared_tools, "shared");
  return tools;
};

// 获取所有可用工具列表，避免轮询时重复请求
const loadAvailableTools = async (options = {}) => {
  const { force = false } = options;
  const now = Date.now();
  const language = getCurrentLanguage();
  const languageChanged = state.monitor.availableToolsLanguage !== language;
  if (
    !force &&
    !languageChanged &&
    state.monitor.availableTools.length &&
    now - state.monitor.availableToolsUpdatedAt < TOOL_LIST_CACHE_MS
  ) {
    return state.monitor.availableTools;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/tools`;
  const response = await fetch(endpoint);
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const result = await response.json();
  state.monitor.availableTools = normalizeAvailableTools(result);
  state.monitor.availableToolsUpdatedAt = now;
  state.monitor.availableToolsLanguage = language;
  return state.monitor.availableTools;
};

// 将 HSL 转为 RGB，便于计算文字对比色
const hslToRgb = (hue, saturation, lightness) => {
  const h = ((Number(hue) || 0) % 360) / 360;
  const s = Math.max(0, Math.min(1, (Number(saturation) || 0) / 100));
  const l = Math.max(0, Math.min(1, (Number(lightness) || 0) / 100));
  if (s === 0) {
    const gray = Math.round(l * 255);
    return [gray, gray, gray];
  }
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
  const p = 2 * l - q;
  const hueToRgb = (t) => {
    let value = t;
    if (value < 0) value += 1;
    if (value > 1) value -= 1;
    if (value < 1 / 6) return p + (q - p) * 6 * value;
    if (value < 1 / 2) return q;
    if (value < 2 / 3) return p + (q - p) * (2 / 3 - value) * 6;
    return p;
  };
  return [
    Math.round(hueToRgb(h + 1 / 3) * 255),
    Math.round(hueToRgb(h) * 255),
    Math.round(hueToRgb(h - 1 / 3) * 255),
  ];
};

// 计算热力图配色的基础色相，保证从蓝过渡到红
const resolveHeatmapHue = (value) => {
  const anchors = TOOL_HEATMAP_HUE_ANCHORS;
  if (!anchors.length) {
    return 210;
  }
  if (value <= anchors[0].value) {
    return anchors[0].hue;
  }
  for (let i = 1; i < anchors.length; i += 1) {
    const next = anchors[i];
    if (value <= next.value) {
      const prev = anchors[i - 1];
      const span = next.value - prev.value || 1;
      const ratio = (value - prev.value) / span;
      return prev.hue + (next.hue - prev.hue) * ratio;
    }
  }
  return anchors[anchors.length - 1].hue;
};

// 生成热力图颜色，按总次数渐变且次数越多越深
const resolveHeatmapColor = (totalCalls) => {
  const value = Math.max(0, Number(totalCalls) || 0);
  if (value <= 0) {
    return { color: `rgb(${TOOL_HEATMAP_ZERO_RGB.join(", ")})`, rgb: TOOL_HEATMAP_ZERO_RGB };
  }
  const clamped = Math.min(value, TOOL_HEATMAP_MAX_VALUE);
  const ratio = clamped / TOOL_HEATMAP_MAX_VALUE;
  const hue = resolveHeatmapHue(clamped);
  const saturation =
    TOOL_HEATMAP_MIN_SATURATION +
    ratio * (TOOL_HEATMAP_MAX_SATURATION - TOOL_HEATMAP_MIN_SATURATION);
  const lightness =
    TOOL_HEATMAP_MAX_LIGHTNESS -
    ratio * (TOOL_HEATMAP_MAX_LIGHTNESS - TOOL_HEATMAP_MIN_LIGHTNESS);
  const rgb = hslToRgb(hue, saturation, lightness);
  return { color: `rgb(${rgb.join(", ")})`, rgb };
};

// 按亮度对比调整文字颜色，保证可读性
const resolveHeatmapTextColor = (rgb) => {
  const [r, g, b] = rgb;
  const luminance = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
  return luminance >= 0.65 ? "#0f172a" : "#f8fafc";
};


const normalizeToolMatchKey = (value) =>
  String(value || "")
    .trim()
    .toLowerCase()
    .replace(/[\s_.-]+/g, "");

const matchesToolKeyword = (lowerName, normalizedName, keyword) => {
  if (!keyword) {
    return false;
  }
  if (lowerName.includes(keyword)) {
    return true;
  }
  const normalizedKeyword = normalizeToolMatchKey(keyword);
  return normalizedKeyword ? normalizedName.includes(normalizedKeyword) : false;
};

// 根据工具名称选择更贴合的图标
const resolveToolIcon = (name, category) => {
  const toolName = String(name || "").trim();
  const lowerName = toolName.toLowerCase();
  const normalizedName = normalizeToolMatchKey(lowerName);
  if (lowerName === "wunder@run" || lowerName.endsWith("@wunder@run")) {
    return "fa-dragon";
  }
  for (const rule of TOOL_HEATMAP_ICON_RULES) {
    if (matchesToolKeyword(lowerName, normalizedName, rule.keyword)) {
      return rule.icon;
    }
  }
  if (toolName.includes("@")) {
    const atCount = toolName.split("@").length - 1;
    if (atCount >= 2 || !mcpToolNameSet.has(toolName)) {
      return "fa-wrench";
    }
    return "fa-plug";
  }
  if (category === "mcp") {
    return "fa-plug";
  }
  if (category === "knowledge") {
    return "fa-book";
  }
  if (category === "skill") {
    return "fa-wand-magic-sparkles";
  }
  if (category === "user" || category === "shared") {
    return "fa-wrench";
  }
  if (
    matchesToolKeyword(lowerName, normalizedName, "\u6267\u884c\u547d\u4ee4") ||
    matchesToolKeyword(lowerName, normalizedName, "run command") ||
    matchesToolKeyword(lowerName, normalizedName, "execute command") ||
    matchesToolKeyword(lowerName, normalizedName, "execute_command") ||
    matchesToolKeyword(lowerName, normalizedName, "shell")
  ) {
    return "fa-terminal";
  }
  if (
    matchesToolKeyword(lowerName, normalizedName, "ptc") ||
    matchesToolKeyword(lowerName, normalizedName, "programmatic_tool_call")
  ) {
    return "fa-code";
  }
  if (
    matchesToolKeyword(lowerName, normalizedName, "\u641c\u7d22") ||
    matchesToolKeyword(lowerName, normalizedName, "\u68c0\u7d22") ||
    matchesToolKeyword(lowerName, normalizedName, "search") ||
    matchesToolKeyword(lowerName, normalizedName, "query") ||
    matchesToolKeyword(lowerName, normalizedName, "retrieve") ||
    matchesToolKeyword(lowerName, normalizedName, "search_content")
  ) {
    return "fa-magnifying-glass";
  }
  if (
    matchesToolKeyword(lowerName, normalizedName, "\u8bfb\u53d6") ||
    matchesToolKeyword(lowerName, normalizedName, "\u5199\u5165") ||
    matchesToolKeyword(lowerName, normalizedName, "\u7f16\u8f91") ||
    matchesToolKeyword(lowerName, normalizedName, "\u66ff\u6362") ||
    matchesToolKeyword(lowerName, normalizedName, "\u5217\u51fa") ||
    matchesToolKeyword(lowerName, normalizedName, "read") ||
    matchesToolKeyword(lowerName, normalizedName, "write") ||
    matchesToolKeyword(lowerName, normalizedName, "edit") ||
    matchesToolKeyword(lowerName, normalizedName, "replace") ||
    matchesToolKeyword(lowerName, normalizedName, "list")
  ) {
    return "fa-file-lines";
  }
  if (
    matchesToolKeyword(lowerName, normalizedName, "\u77e5\u8bc6") ||
    matchesToolKeyword(lowerName, normalizedName, "knowledge")
  ) {
    return "fa-book";
  }
  if (
    matchesToolKeyword(lowerName, normalizedName, "\u6700\u7ec8\u56de\u590d") ||
    matchesToolKeyword(lowerName, normalizedName, "final answer") ||
    matchesToolKeyword(lowerName, normalizedName, "final_response")
  ) {
    return "fa-flag-checkered";
  }
  if (category === "builtin") {
    return "fa-toolbox";
  }
  return "fa-toolbox";
};

// 规范化工具调用次数，避免使用 k 单位
const formatHeatmapCount = (value) => {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) {
    return "0";
  }
  return String(Math.max(0, Math.round(parsed)));
};

// 规整工具统计结构，避免缺字段导致渲染异常
const normalizeToolStats = (toolStats) =>
  (Array.isArray(toolStats) ? toolStats : [])
    .map((item) => ({
      name: String(item?.tool ?? item?.name ?? "").trim(),
      calls: Number(item?.calls ?? item?.count ?? item?.tool_calls ?? 0),
    }))
    .filter((item) => item.name);

// 合并工具列表与调用次数，确保未调用工具也展示
const buildHeatmapItems = (toolStats) => {
  const normalized = normalizeToolStats(toolStats);
  const callsMap = new Map(normalized.map((item) => [item.name, item.calls]));
  const items = [];
  const seen = new Set();
  (state.monitor.availableTools || []).forEach((tool) => {
    const name = String(tool?.name ?? "").trim();
    if (!name || seen.has(name)) {
      return;
    }
    items.push({
      name,
      calls: callsMap.get(name) ?? 0,
      category: tool?.category || "other",
    });
    seen.add(name);
  });
  normalized.forEach((item) => {
    if (seen.has(item.name)) {
      return;
    }
    items.push({ name: item.name, calls: item.calls, category: "other" });
    seen.add(item.name);
  });
  return items;
};

// 渲染工具调用热力图
const renderToolHeatmap = (toolStats) => {
  if (!elements.toolHeatmapGrid || !elements.toolHeatmapEmpty) {
    return;
  }
  const normalized = buildHeatmapItems(toolStats);
  elements.toolHeatmapGrid.textContent = "";
  if (!normalized.length) {
    elements.toolHeatmapEmpty.style.display = "block";
    elements.toolHeatmapGrid.style.display = "none";
    return;
  }
  elements.toolHeatmapEmpty.style.display = "none";
  elements.toolHeatmapGrid.style.display = "grid";
  const wrapHeight = elements.toolHeatmapWrap?.clientHeight || 0;
  const rows = Math.max(
    1,
    Math.floor((wrapHeight + TOOL_HEATMAP_GAP) / (TOOL_HEATMAP_TILE_SIZE + TOOL_HEATMAP_GAP))
  );
  elements.toolHeatmapGrid.style.setProperty("--heatmap-rows", String(rows));
  normalized.forEach((item) => {
    const { color, rgb } = resolveHeatmapColor(item.calls);
    const tile = document.createElement("div");
    tile.className = "tool-heatmap-item";
    tile.style.backgroundColor = color;
    tile.style.color = resolveHeatmapTextColor(rgb);
    tile.title = t("monitor.toolHeatmap.tileTitle", {
      name: item.name,
      count: formatHeatmapCount(item.calls),
    });
    const icon = document.createElement("i");
    icon.className = `fa-solid ${resolveToolIcon(item.name, item.category)}`;
    const name = document.createElement("span");
    name.className = "tool-heatmap-name";
    name.textContent = item.name;
    tile.appendChild(icon);
    tile.appendChild(name);
    // 点击热力图块时弹出该工具的调用线程列表
    tile.addEventListener("click", () => {
      openMonitorToolModal(item.name);
    });
    elements.toolHeatmapGrid.appendChild(tile);
  });
};

// 渲染系统监视指标
const renderMonitorMetrics = (system) => {
  if (!system) {
    elements.metricCpu.textContent = "-";
    elements.metricMemory.textContent = "-";
    elements.metricMemoryDetail.textContent = "";
    elements.metricProcessMemory.textContent = "-";
    elements.metricProcessCpu.textContent = "-";
    elements.metricLoad1.textContent = "-";
    elements.metricLoad5.textContent = "-";
    elements.metricLoad15.textContent = "-";
    elements.metricUptime.textContent = "-";
    elements.metricDisk.textContent = "-";
    elements.metricDiskDetail.textContent = "";
    elements.metricDiskRead.textContent = "-";
    elements.metricDiskWrite.textContent = "-";
    elements.metricNetSent.textContent = "-";
    elements.metricNetRecv.textContent = "-";
    return;
  }
  elements.metricCpu.textContent = `${system.cpu_percent.toFixed(1)}%`;
  elements.metricMemory.textContent = formatBytes(system.memory_used);
  elements.metricMemoryDetail.textContent = t("monitor.metric.memory.detail", {
    total: formatBytes(system.memory_total),
    available: formatBytes(system.memory_available),
  });
  elements.metricProcessMemory.textContent = formatBytes(system.process_rss);
  elements.metricProcessCpu.textContent = `${system.process_cpu_percent.toFixed(1)}%`;
  const loadValues = [
    system.load_avg_1,
    system.load_avg_5,
    system.load_avg_15,
  ].map((value) => (Number.isFinite(value) ? value.toFixed(2) : "-"));
  [elements.metricLoad1.textContent, elements.metricLoad5.textContent, elements.metricLoad15.textContent] =
    loadValues;
  elements.metricUptime.textContent = formatDurationLong(system.uptime_s);
  const hasDisk = Number.isFinite(system.disk_total) && system.disk_total > 0;
  elements.metricDisk.textContent =
    hasDisk && Number.isFinite(system.disk_percent)
      ? `${system.disk_percent.toFixed(1)}%`
      : "-";
  elements.metricDiskDetail.textContent = hasDisk
    ? t("monitor.metric.disk.detail", {
        used: formatBytes(system.disk_used),
        total: formatBytes(system.disk_total),
        free: formatBytes(system.disk_free),
      })
    : "";
  elements.metricDiskRead.textContent = formatBytes(system.disk_read_bytes);
  elements.metricDiskWrite.textContent = formatBytes(system.disk_write_bytes);
  elements.metricNetSent.textContent = formatBytes(system.net_sent_bytes);
  elements.metricNetRecv.textContent = formatBytes(system.net_recv_bytes);
};

// 渲染服务层线程指标，统一保持数值与展示文案分离
const renderServiceMetrics = (service) => {
  if (!service) {
    elements.metricServiceActive.textContent = "-";
    elements.metricServiceHistory.textContent = "-";
    elements.metricServiceFinished.textContent = "-";
    elements.metricServiceError.textContent = "-";
    elements.metricServiceCancelled.textContent = "-";
    elements.metricServiceTotal.textContent = "-";
    elements.metricServiceRecent.textContent = "-";
    elements.metricServiceAvg.textContent = "-";
    if (elements.metricServicePrefillSpeed) {
      elements.metricServicePrefillSpeed.textContent = "-";
    }
    if (elements.metricServiceDecodeSpeed) {
      elements.metricServiceDecodeSpeed.textContent = "-";
    }
    return;
  }
  elements.metricServiceActive.textContent = `${service.active_sessions ?? 0}`;
  elements.metricServiceHistory.textContent = `${service.history_sessions ?? 0}`;
  elements.metricServiceFinished.textContent = `${service.finished_sessions ?? 0}`;
  elements.metricServiceError.textContent = `${service.error_sessions ?? 0}`;
  elements.metricServiceCancelled.textContent = `${service.cancelled_sessions ?? 0}`;
  elements.metricServiceTotal.textContent = `${service.total_sessions ?? 0}`;
  elements.metricServiceRecent.textContent = `${service.recent_completed ?? 0}`;
  elements.metricServiceAvg.textContent = formatDurationLong(service.avg_elapsed_s);
  if (elements.metricServicePrefillSpeed) {
    const prefillSpeed = parseMetricNumber(service.avg_prefill_speed_tps);
    elements.metricServicePrefillSpeed.textContent = formatTokenRate(prefillSpeed);
  }
  if (elements.metricServiceDecodeSpeed) {
    const decodeSpeed = parseMetricNumber(service.avg_decode_speed_tps);
    elements.metricServiceDecodeSpeed.textContent = formatTokenRate(decodeSpeed);
  }
};

// 渲染沙盒状态指标，兼顾配置与近期调用统计
const renderSandboxMetrics = (sandbox) => {
  if (!elements.metricSandboxMode) {
    return;
  }
  if (!sandbox) {
    elements.metricSandboxMode.textContent = "-";
    elements.metricSandboxNetwork.textContent = "-";
    elements.metricSandboxReadonly.textContent = "-";
    elements.metricSandboxResources.textContent = "-";
    elements.metricSandboxResourcesDetail.textContent = "";
    elements.metricSandboxCalls.textContent = "-";
    elements.metricSandboxSessions.textContent = "-";
    return;
  }
  const mode = String(sandbox.mode || "").toLowerCase();
  elements.metricSandboxMode.textContent =
    mode === "sandbox"
      ? t("monitor.sandbox.mode.sandbox")
      : mode
        ? t("monitor.sandbox.mode.local")
        : "-";
  elements.metricSandboxNetwork.textContent = sandbox.network || "-";
  elements.metricSandboxReadonly.textContent = sandbox.readonly_rootfs
    ? t("common.yes")
    : t("common.no");
  const cpuValue = Number(sandbox.resources?.cpu);
  const cpuText =
    Number.isFinite(cpuValue) && cpuValue > 0
      ? Number.isInteger(cpuValue)
        ? cpuValue.toFixed(0)
        : cpuValue.toFixed(cpuValue >= 10 ? 0 : 1)
      : "-";
  const memoryMb = Number(sandbox.resources?.memory_mb);
  const memoryBytes = Number.isFinite(memoryMb) ? memoryMb * 1024 * 1024 : NaN;
  const memoryText =
    Number.isFinite(memoryMb) && memoryMb > 0 ? formatBytes(memoryBytes) : "-";
  elements.metricSandboxResources.textContent = t("monitor.metric.sandbox.resources.detail", {
    cpu: cpuText,
    memory: memoryText,
  });
  const pids = Number(sandbox.resources?.pids);
  elements.metricSandboxResourcesDetail.textContent =
    Number.isFinite(pids) && pids > 0 ? `PID ${pids}` : "";
  elements.metricSandboxCalls.textContent = `${sandbox.recent_calls ?? 0}`;
  elements.metricSandboxSessions.textContent = `${sandbox.recent_sessions ?? 0}`;
};

// 计算所有会话累计 token 数量
const resolveTotalTokens = (sessions) =>
  (sessions || []).reduce((sum, session) => sum + (Number(session?.token_usage) || 0), 0);

// 解析监控时间字段，避免格式异常导致筛选失败
const parseMonitorTimestamp = (value) => {
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : null;
};

// 获取会话的可比较时间戳
const resolveSessionTimestamp = (session) => {
  const updated = parseMonitorTimestamp(session?.updated_time);
  if (updated) {
    return updated;
  }
  const started = parseMonitorTimestamp(session?.start_time);
  return started || null;
};

// 根据监视时间范围筛选会话，用于当前区间的状态统计
const filterSessionsByInterval = (sessions) => {
  const timeRange = resolveMonitorTimeFilterRange();
  if (timeRange) {
    return (sessions || []).filter((session) => {
      const timestamp = resolveSessionTimestamp(session);
      if (!timestamp) {
        return true;
      }
      return timestamp >= timeRange.start && timestamp <= timeRange.end;
    });
  }
  const windowMs = getMonitorTimeRangeMs();
  if (!windowMs) {
    return sessions || [];
  }
  const cutoff = Date.now() - windowMs;
  return (sessions || []).filter((session) => {
    const timestamp = resolveSessionTimestamp(session);
    if (!timestamp) {
      return true;
    }
    return timestamp >= cutoff;
  });
};

// 更新 token 趋势折线图
const renderTokenTrendChart = () => {
  if (!tokenTrendChart) {
    return;
  }
  const { labels, values } = buildTokenSeries(state.monitor.sessions);
  if (elements.serviceTokenChart) {
    elements.serviceTokenChart.style.width = "100%";
  }
  const shouldApplyZoom = !state.monitor.tokenZoomLocked || !state.monitor.tokenZoomInitialized;
  const option = {
    grid: {
      left: 42,
      right: 16,
      top: 20,
      bottom: 24,
    },
    tooltip: {
      trigger: "axis",
      formatter: (params) => {
        const point = params?.[0];
        if (!point) {
          return "";
        }
        return `${point.axisValue}<br/>Token ${formatTokenCount(point.data)}`;
      },
    },
    xAxis: {
      type: "category",
      data: labels,
      boundaryGap: false,
      axisLabel: {
        color: "#94a3b8",
      },
      axisTick: { show: false },
      axisLine: { lineStyle: { color: "#e2e8f0" } },
    },
    yAxis: {
      type: "value",
      axisLabel: {
        color: "#94a3b8",
        formatter: (value) => formatTokenCount(value),
      },
      splitLine: {
        lineStyle: { color: "#e2e8f0" },
      },
    },
    series: [
      {
        name: "Token",
        type: "line",
        data: values,
        smooth: true,
        showSymbol: false,
        lineStyle: { color: "#3b82f6", width: 2 },
        areaStyle: { color: "rgba(59, 130, 246, 0.15)" },
      },
    ],
  };
  if (shouldApplyZoom) {
    const zoomConfig = {
      id: "tokenZoom",
      type: "inside",
      xAxisIndex: 0,
      filterMode: "none",
    };
    if (labels.length) {
      const visiblePoints = Math.min(labels.length, TOKEN_TREND_MAX_BUCKETS + 1);
      const startIndex = Math.max(0, labels.length - visiblePoints);
      zoomConfig.startValue = labels[startIndex];
      zoomConfig.endValue = labels[labels.length - 1];
    }
    // 启用内置缩放，默认聚焦最近窗口
    option.dataZoom = [zoomConfig];
  }
  tokenTrendChart.setOption(option, false);
  state.monitor.tokenZoomInitialized = true;
  if (tokenTrendChart) {
    tokenTrendChart.resize();
  }
};

// 汇总线程状态占比，便于图表展示
const resolveStatusCounts = (sessions) => {
  const counts = {
    active: 0,
    finished: 0,
    error: 0,
    cancelled: 0,
  };
  (sessions || []).forEach((session) => {
    const status = session?.status;
    if (ACTIVE_STATUSES.has(status)) {
      counts.active += 1;
      return;
    }
    if (status === "finished") {
      counts.finished += 1;
    } else if (status === "error") {
      counts.error += 1;
    } else if (status === "cancelled") {
      counts.cancelled += 1;
    }
  });
  return counts;
};

// 生成状态环图数据，空数据时返回白色空心环占位
const buildStatusChartData = (counts) => {
  const [activeLabel, finishedLabel, failedLabel, cancelledLabel] = getStatusLegend();
  const raw = [
    { value: counts.active, name: activeLabel },
    { value: counts.finished, name: finishedLabel },
    { value: counts.error, name: failedLabel },
    { value: counts.cancelled, name: cancelledLabel },
  ];
  const total = raw.reduce((sum, item) => sum + item.value, 0);
  const visibleCount = raw.filter((item) => item.value > 0).length;
  const normalized = raw.map((item) => {
    if (item.value > 0) {
      return item;
    }
    return {
      ...item,
      itemStyle: {
        borderWidth: 0,
      },
      emphasis: { disabled: true },
    };
  });
  if (total <= 0) {
    return {
      data: [
        ...normalized,
        {
          value: 1,
          name: STATUS_CHART_EMPTY_NAME,
          itemStyle: {
            color: STATUS_CHART_EMPTY_COLOR,
            borderColor: "#e2e8f0",
            borderWidth: 2,
            borderRadius: 8,
          },
        },
      ],
      isEmpty: true,
      visibleCount: 0,
    };
  }
  return { data: normalized, isEmpty: false, visibleCount };
};

// 更新服务状态占比图表
const renderServiceStatusChart = (service, sessions) => {
  if (!statusChart) {
    return;
  }
  const counts = Array.isArray(sessions)
    ? resolveStatusCounts(sessions)
    : {
        active: Number(service?.active_sessions) || 0,
        finished: Number(service?.finished_sessions) || 0,
        error: Number(service?.error_sessions) || 0,
        cancelled: Number(service?.cancelled_sessions) || 0,
      };
  const { data, isEmpty, visibleCount } = buildStatusChartData(counts);
  const padAngle = isEmpty || visibleCount <= 1 ? 0 : 1;
  const ringStyle = isEmpty
    ? {
        borderColor: "#e2e8f0",
        borderWidth: 2,
        borderRadius: 8,
        shadowBlur: 0,
      }
    : {
        borderColor: "rgba(15, 23, 42, 0.6)",
        borderWidth: 2,
        borderRadius: 6,
        shadowBlur: 0,
      };
  statusChart.setOption(
    {
      tooltip: {
        trigger: "item",
        show: !isEmpty,
        backgroundColor: "rgba(15, 23, 42, 0.95)",
        borderColor: "rgba(59, 130, 246, 0.35)",
        textStyle: { color: "#e2e8f0" },
      },
      legend: {
        bottom: 2,
        show: true,
        icon: "circle",
        itemWidth: 8,
        itemHeight: 8,
        data: getStatusLegend(),
        textStyle: {
          color: "#64748b",
          fontSize: 13,
        },
      },
      series: [
        {
          type: "pie",
          radius: ["52%", "78%"],
          center: ["50%", "45%"],
          avoidLabelOverlap: true,
          label: { show: false },
          emphasis: {
            label: {
              show: !isEmpty,
              fontSize: 12,
              formatter: "{b}: {c}",
              color: "#f8fafc",
            },
          },
          labelLine: { show: false },
          padAngle,
          itemStyle: ringStyle,
          data,
          color: STATUS_CHART_COLORS,
          silent: isEmpty,
        },
      ],
    },
    true
  );
};

// 汇总服务图表数据并刷新渲染
const renderServiceCharts = (service, sessions) => {
  updateMonitorChartTitles();
  const totalTokens = resolveTotalTokens(sessions);
  if (elements.metricServiceTokenTotal) {
    elements.metricServiceTokenTotal.textContent = formatTokenCount(totalTokens);
  }
  if (!ensureMonitorCharts()) {
    return;
  }
  renderTokenTrendChart();
  const scopedSessions = filterSessionsByInterval(sessions);
  renderServiceStatusChart(service, scopedSessions);
  resizeMonitorCharts();
};

// 图表尺寸随容器变化自动适配
const resizeMonitorCharts = () => {
  if (tokenTrendChart) {
    renderTokenTrendChart();
  }
  if (statusChart) {
    statusChart.resize();
  }
  renderToolHeatmap(state.monitor.toolStats);
};

const getSessionStatusLabel = (status) => {
  const normalized = String(status || "");
  const mapping = {
    running: t("monitor.sessionStatus.running"),
    cancelling: t("monitor.sessionStatus.cancelling"),
    finished: t("monitor.sessionStatus.finished"),
    error: t("monitor.sessionStatus.error"),
    cancelled: t("monitor.sessionStatus.cancelled"),
  };
  return mapping[normalized] || normalized || "-";
};

const buildStatusBadge = (status) => {
  const span = document.createElement("span");
  span.className = `monitor-status ${status}`;
  span.textContent = getSessionStatusLabel(status);
  return span;
};

const ACTIVE_STATUSES = new Set(["running", "cancelling"]);

const sortSessionsByUpdate = (sessions) =>
  [...sessions].sort((a, b) => new Date(b.updated_time).getTime() - new Date(a.updated_time).getTime());

// 根据用户筛选线程列表，空值时返回全量
const filterSessionsByUser = (sessions) => {
  const userId = String(state.monitor.userFilter || "").trim();
  if (!userId) {
    return sessions;
  }
  return (sessions || []).filter((session) => String(session?.user_id || "") === userId);
};

// 读取分页配置，确保分页尺寸为正整数
const resolveMonitorPageSize = () => {
  const rawValue = Math.floor(Number(state.monitor.pagination?.pageSize));
  if (!Number.isFinite(rawValue) || rawValue <= 0) {
    return DEFAULT_MONITOR_SESSION_PAGE_SIZE;
  }
  return rawValue;
};

// 统一约束页码范围，避免越界导致分页为空
const clampMonitorPage = (value, totalPages) => {
  const page = Number(value);
  if (!Number.isFinite(page) || page < 1) {
    return 1;
  }
  if (!Number.isFinite(totalPages) || totalPages <= 0) {
    return 1;
  }
  return Math.min(page, totalPages);
};

// 根据分页状态切片线程列表，并回写合法页码
const resolveMonitorPageSlice = (sessions, pageKey, options = {}) => {
  const { sorted = false } = options;
  const pageSize = resolveMonitorPageSize();
  const total = Array.isArray(sessions) ? sessions.length : 0;
  const totalPages = Math.max(1, Math.ceil(total / pageSize));
  const currentPage = clampMonitorPage(state.monitor.pagination?.[pageKey], totalPages);
  if (state.monitor.pagination) {
    state.monitor.pagination[pageKey] = currentPage;
  }
  const ordered = sorted ? sessions || [] : sortSessionsByUpdate(sessions || []);
  const startIndex = (currentPage - 1) * pageSize;
  const pageSessions = ordered.slice(startIndex, startIndex + pageSize);
  return { total, totalPages, currentPage, pageSize, sessions: pageSessions };
};

// 兼容旧版本 elements.js 未包含分页元素的情况
const resolveMonitorPaginationElement = (key, id) => {
  if (elements[key]) {
    return elements[key];
  }
  const node = document.getElementById(id);
  if (node) {
    elements[key] = node;
  }
  return node;
};

// 获取分页控件 DOM，便于复用更新逻辑
const getMonitorPaginationElements = (type) => {
  if (type === "active") {
    return {
      container: resolveMonitorPaginationElement("monitorActivePagination", "monitorActivePagination"),
      info: resolveMonitorPaginationElement("monitorActivePageInfo", "monitorActivePageInfo"),
      prev: resolveMonitorPaginationElement("monitorActivePrevBtn", "monitorActivePrevBtn"),
      next: resolveMonitorPaginationElement("monitorActiveNextBtn", "monitorActiveNextBtn"),
    };
  }
  if (type === "history") {
    return {
      container: resolveMonitorPaginationElement("monitorHistoryPagination", "monitorHistoryPagination"),
      info: resolveMonitorPaginationElement("monitorHistoryPageInfo", "monitorHistoryPageInfo"),
      prev: resolveMonitorPaginationElement("monitorHistoryPrevBtn", "monitorHistoryPrevBtn"),
      next: resolveMonitorPaginationElement("monitorHistoryNextBtn", "monitorHistoryNextBtn"),
    };
  }
  return null;
};

// 同步分页区域文案与按钮状态
const renderMonitorPagination = (type, pageData) => {
  const controls = getMonitorPaginationElements(type);
  if (!controls?.container || !controls.info || !controls.prev || !controls.next) {
    return;
  }
  if (!pageData || pageData.total <= 0) {
    controls.container.style.display = "none";
    return;
  }
  controls.container.style.display = "flex";
  controls.info.textContent = t("pagination.info", {
    total: pageData.total,
    current: pageData.currentPage,
    pages: pageData.totalPages,
    size: pageData.pageSize,
  });
  controls.prev.disabled = pageData.currentPage <= 1;
  controls.next.disabled = pageData.currentPage >= pageData.totalPages;
};

const renderMonitorTable = (body, emptyNode, sessions, options = {}) => {
  const {
    showCancel = false,
    showDelete = false,
    emptyText = t("common.noData"),
    skipSort = false,
  } = options;
  body.textContent = "";
  if (!Array.isArray(sessions) || sessions.length === 0) {
    emptyNode.textContent = emptyText;
    emptyNode.style.display = "block";
    return;
  }
  emptyNode.style.display = "none";
  // 分页逻辑已排序时跳过二次排序，减少渲染开销
  const sorted = skipSort ? sessions : sortSessionsByUpdate(sessions);
  sorted.forEach((session) => {
    const row = document.createElement("tr");
    const startCell = document.createElement("td");
    startCell.textContent = formatTimestamp(session.start_time);
    const sessionCell = document.createElement("td");
    const rawSessionId = session.session_id || "";
    sessionCell.textContent = rawSessionId ? rawSessionId.slice(0, 4) : "-";
    if (rawSessionId) {
      sessionCell.title = rawSessionId;
    }
    const userCell = document.createElement("td");
    userCell.textContent = session.user_id || "-";
    const questionCell = document.createElement("td");
    questionCell.textContent = session.question || "-";
    const statusCell = document.createElement("td");
    statusCell.appendChild(buildStatusBadge(session.status || ""));
    const tokenCell = document.createElement("td");
    tokenCell.textContent = formatTokenCount(session.token_usage);
    const elapsedCell = document.createElement("td");
    elapsedCell.textContent = formatDuration(session.elapsed_s);
    const stageCell = document.createElement("td");
    stageCell.textContent = session.stage || "-";
    const actionCell = document.createElement("td");
    if (showCancel && ACTIVE_STATUSES.has(session.status)) {
      const btn = document.createElement("button");
      btn.className = "danger";
      btn.textContent = t("monitor.actions.cancel");
      btn.addEventListener("click", (event) => {
        event.stopPropagation();
        requestCancelSession(session.session_id);
      });
      actionCell.appendChild(btn);
    }
    if (showDelete && !ACTIVE_STATUSES.has(session.status)) {
      const btn = document.createElement("button");
      btn.className = "danger";
      btn.textContent = t("monitor.actions.delete");
      btn.addEventListener("click", (event) => {
        event.stopPropagation();
        requestDeleteSession(session.session_id);
      });
      actionCell.appendChild(btn);
    }
    row.appendChild(startCell);
    row.appendChild(sessionCell);
    row.appendChild(userCell);
    row.appendChild(questionCell);
    row.appendChild(statusCell);
    row.appendChild(tokenCell);
    row.appendChild(elapsedCell);
    row.appendChild(stageCell);
    row.appendChild(actionCell);
    row.addEventListener("click", () => {
      if (!session.session_id) {
        return;
      }
      openMonitorDetail(session.session_id);
    });
    body.appendChild(row);
  });
};

const renderMonitorSessions = (sessions) => {
  const filtered = filterSessionsByUser(sessions || []);
  const sorted = sortSessionsByUpdate(filtered);
  const active = [];
  const history = [];
  sorted.forEach((session) => {
    if (ACTIVE_STATUSES.has(session.status)) {
      active.push(session);
    } else {
      history.push(session);
    }
  });
  // 活动线程分页渲染，避免一次输出过多记录
  const activePage = resolveMonitorPageSlice(active, "activePage", { sorted: true });
  renderMonitorTable(elements.monitorTableBody, elements.monitorEmpty, activePage.sessions, {
    showCancel: true,
    emptyText: t("monitor.empty.active"),
    skipSort: true,
  });
  renderMonitorPagination("active", activePage);

  // 历史线程分页渲染，保持排序与页码一致
  const historyPage = resolveMonitorPageSlice(history, "historyPage", { sorted: true });
  renderMonitorTable(
    elements.monitorHistoryBody,
    elements.monitorHistoryEmpty,
    historyPage.sessions,
    {
      showDelete: true,
      emptyText: t("monitor.empty.history"),
      skipSort: true,
    }
  );
  renderMonitorPagination("history", historyPage);
};

// 切换分页页码并触发列表刷新
const updateMonitorPage = (pageKey, delta) => {
  ensureMonitorState();
  const current = Number(state.monitor.pagination?.[pageKey]) || 1;
  const nextPage = Math.max(1, current + delta);
  if (state.monitor.pagination) {
    state.monitor.pagination[pageKey] = nextPage;
  }
  renderMonitorSessions(state.monitor.sessions);
};

// 绑定分页按钮事件，避免重复查找 DOM
const bindMonitorPagination = () => {
  if (elements.monitorActivePrevBtn) {
    elements.monitorActivePrevBtn.addEventListener("click", () => {
      updateMonitorPage("activePage", -1);
    });
  }
  if (elements.monitorActiveNextBtn) {
    elements.monitorActiveNextBtn.addEventListener("click", () => {
      updateMonitorPage("activePage", 1);
    });
  }
  if (elements.monitorHistoryPrevBtn) {
    elements.monitorHistoryPrevBtn.addEventListener("click", () => {
      updateMonitorPage("historyPage", -1);
    });
  }
  if (elements.monitorHistoryNextBtn) {
    elements.monitorHistoryNextBtn.addEventListener("click", () => {
      updateMonitorPage("historyPage", 1);
    });
  }
};

// 根据图例标签解析对应的状态 key
const resolveStatusKey = (label) => getStatusLabelToKey()[label] || "";

// 判断会话是否属于指定状态分组
const matchSessionByStatusKey = (session, key) => {
  const status = session?.status;
  if (key === "active") {
    return ACTIVE_STATUSES.has(status);
  }
  if (key === "finished") {
    return status === "finished";
  }
  if (key === "error") {
    return status === "error";
  }
  if (key === "cancelled") {
    return status === "cancelled";
  }
  return false;
};

// 渲染状态详情列表，支持点击打开线程详情
const renderMonitorStatusList = (sessions) => {
  if (!elements.monitorStatusList) {
    return;
  }
  elements.monitorStatusList.textContent = "";
  if (!Array.isArray(sessions) || sessions.length === 0) {
    elements.monitorStatusList.textContent = t("common.noRecords");
    return;
  }
  sortSessionsByUpdate(sessions).forEach((session) => {
    const item = document.createElement("button");
    item.type = "button";
    item.className = "list-item monitor-status-item";

    const header = document.createElement("div");
    header.className = "monitor-status-item-header";
    const title = document.createElement("div");
    title.className = "monitor-status-item-title";
    title.textContent = session?.question || t("monitor.session.noQuestion");
    const badge = buildStatusBadge(session?.status || "");
    header.appendChild(title);
    header.appendChild(badge);

    const metaParts = [];
    metaParts.push(session?.session_id || "-");
    metaParts.push(session?.user_id || "-");
    const timeText = formatTimestamp(session?.updated_time || session?.start_time);
    if (timeText && timeText !== "-") {
      metaParts.push(timeText);
    }
    const meta = document.createElement("small");
    meta.textContent = metaParts.join(" · ");

    const detailParts = [];
    const tokenText = formatTokenCount(session?.token_usage);
    if (tokenText && tokenText !== "-") {
      detailParts.push(t("monitor.session.token", { token: tokenText }));
    }
    const elapsedText = formatDuration(session?.elapsed_s);
    if (elapsedText && elapsedText !== "-") {
      detailParts.push(t("monitor.session.elapsed", { elapsed: elapsedText }));
    }
    const prefillSpeed = parseMetricNumber(session?.prefill_speed_tps);
    if (Number.isFinite(prefillSpeed) && prefillSpeed > 0) {
      detailParts.push(
        t("monitor.session.prefillSpeed", { speed: formatTokenRate(prefillSpeed) })
      );
    }
    const decodeSpeed = parseMetricNumber(session?.decode_speed_tps);
    if (Number.isFinite(decodeSpeed) && decodeSpeed > 0) {
      detailParts.push(
        t("monitor.session.decodeSpeed", { speed: formatTokenRate(decodeSpeed) })
      );
    }
    if (session?.stage) {
      detailParts.push(t("monitor.session.stage", { stage: session.stage }));
    }
    const detail = document.createElement("small");
    detail.textContent = detailParts.join(" · ");

    item.appendChild(header);
    item.appendChild(meta);
    if (detailParts.length) {
      item.appendChild(detail);
    }
    item.addEventListener("click", () => {
      if (!session?.session_id) {
        return;
      }
      openMonitorDetail(session.session_id);
    });
    elements.monitorStatusList.appendChild(item);
  });
};

// 解析工具调用会话的时间戳，优先使用最近调用时间
const resolveToolSessionTimestamp = (session) => {
  const raw = session?.last_time || session?.updated_time || session?.start_time;
  const parsed = new Date(raw).getTime();
  return Number.isFinite(parsed) ? parsed : 0;
};

// 渲染工具调用会话列表，保持与线程状态弹窗一致的风格
const renderMonitorToolList = (sessions, toolName = "") => {
  if (!elements.monitorToolList) {
    return;
  }
  const focusToolName = String(toolName || "").trim();
  elements.monitorToolList.textContent = "";
  if (!Array.isArray(sessions) || sessions.length === 0) {
    elements.monitorToolList.textContent = t("common.noRecords");
    return;
  }
  [...sessions]
    .sort((a, b) => resolveToolSessionTimestamp(b) - resolveToolSessionTimestamp(a))
    .forEach((session) => {
      const item = document.createElement("button");
      item.type = "button";
      item.className = "list-item monitor-status-item";

      const header = document.createElement("div");
      header.className = "monitor-status-item-header";
      const title = document.createElement("div");
      title.className = "monitor-status-item-title";
    title.textContent = session?.question || t("monitor.session.noQuestion");
      const badge = buildStatusBadge(session?.status || "");
      header.appendChild(title);
      header.appendChild(badge);

      const metaParts = [];
      metaParts.push(session?.session_id || "-");
      metaParts.push(session?.user_id || "-");
      const timeText = formatTimestamp(
        session?.last_time || session?.updated_time || session?.start_time
      );
      if (timeText && timeText !== "-") {
        metaParts.push(timeText);
      }
      const meta = document.createElement("small");
      meta.textContent = metaParts.join(" · ");

      const detailParts = [];
      detailParts.push(
        t("monitor.tool.calls", { count: formatHeatmapCount(session?.tool_calls) })
      );
      const tokenText = formatTokenCount(session?.token_usage);
      if (tokenText && tokenText !== "-") {
        detailParts.push(`Token ${tokenText}`);
      }
      const elapsedText = formatDuration(session?.elapsed_s);
      if (elapsedText && elapsedText !== "-") {
        detailParts.push(t("monitor.session.elapsed", { elapsed: elapsedText }));
      }
      const prefillSpeed = parseMetricNumber(session?.prefill_speed_tps);
      if (Number.isFinite(prefillSpeed) && prefillSpeed > 0) {
        detailParts.push(
          t("monitor.session.prefillSpeed", { speed: formatTokenRate(prefillSpeed) })
        );
      }
      const decodeSpeed = parseMetricNumber(session?.decode_speed_tps);
      if (Number.isFinite(decodeSpeed) && decodeSpeed > 0) {
        detailParts.push(
          t("monitor.session.decodeSpeed", { speed: formatTokenRate(decodeSpeed) })
        );
      }
      if (session?.stage) {
        detailParts.push(t("monitor.session.stage", { stage: session.stage }));
      }
      const detail = document.createElement("small");
      detail.textContent = detailParts.join(" · ");

      item.appendChild(header);
      item.appendChild(meta);
      if (detailParts.length) {
        item.appendChild(detail);
      }
      item.addEventListener("click", () => {
        if (!session?.session_id) {
          return;
        }
        openMonitorDetail(session.session_id, { focusTool: focusToolName });
      });
      elements.monitorToolList.appendChild(item);
    });
};

// 获取指定工具的调用会话列表
const fetchMonitorToolSessions = async (toolName) => {
  const wunderBase = getWunderBase();
  const params = new URLSearchParams({ tool: toolName });
  const timeRange = resolveMonitorTimeFilterRange();
  if (timeRange) {
    params.set("start_time", (timeRange.start / 1000).toFixed(3));
    params.set("end_time", (timeRange.end / 1000).toFixed(3));
  } else {
    params.set("tool_hours", String(getMonitorTimeRangeHours()));
  }
  const endpoint = `${wunderBase}/admin/monitor/tool_usage?${params.toString()}`;
  const response = await fetch(endpoint);
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const result = await response.json();
  return {
    sessions: Array.isArray(result.sessions) ? result.sessions : [],
    toolName: String(result.tool_name || toolName || "").trim(),
  };
};

// 打开工具调用明细弹窗
const openMonitorToolModal = async (toolName) => {
  if (!elements.monitorToolModal) {
    return;
  }
  const cleaned = String(toolName || "").trim();
  if (!cleaned) {
    return;
  }
  if (elements.monitorToolTitle) {
    elements.monitorToolTitle.textContent = t("monitor.toolModal.title", { tool: cleaned });
  }
  if (elements.monitorToolMeta) {
    const windowLabel = getMonitorTimeWindowLabel();
    elements.monitorToolMeta.textContent = t("monitor.toolModal.meta.loading", {
      label: windowLabel,
    });
  }
  if (elements.monitorToolList) {
    elements.monitorToolList.textContent = t("common.loading");
  }
  elements.monitorToolModal.classList.add("active");
  try {
    const { sessions, toolName: focusToolName } = await fetchMonitorToolSessions(cleaned);
    if (elements.monitorToolMeta) {
      const windowLabel = getMonitorTimeWindowLabel();
      elements.monitorToolMeta.textContent = t("monitor.toolModal.meta.total", {
        label: windowLabel,
        total: sessions.length,
      });
    }
    renderMonitorToolList(sessions, focusToolName || cleaned);
  } catch (error) {
    appendLog(t("monitor.toolDetailLoadFailed", { message: error.message }));
    if (elements.monitorToolList) {
      elements.monitorToolList.textContent = t("common.loadFailed");
    }
  }
};

// 关闭工具调用弹窗
const closeMonitorToolModal = () => {
  elements.monitorToolModal?.classList.remove("active");
};

// 打开线程状态明细弹窗，显示对应状态的会话记录
const openMonitorStatusModal = (label) => {
  if (!elements.monitorStatusModal) {
    return;
  }
  const key = resolveStatusKey(label);
  if (!key) {
    return;
  }
  const scopedSessions = filterSessionsByInterval(state.monitor.sessions || []);
  const matchedSessions = scopedSessions.filter((session) => matchSessionByStatusKey(session, key));
  if (elements.monitorStatusTitle) {
    elements.monitorStatusTitle.textContent = t("monitor.statusModal.title", { status: label });
  }
  if (elements.monitorStatusMeta) {
    const windowLabel = getMonitorTimeWindowLabel();
    elements.monitorStatusMeta.textContent = t("monitor.statusModal.meta.total", {
      label: windowLabel,
      total: matchedSessions.length,
    });
  }
  renderMonitorStatusList(matchedSessions);
  elements.monitorStatusModal.classList.add("active");
};

// 关闭线程状态明细弹窗
const closeMonitorStatusModal = () => {
  elements.monitorStatusModal?.classList.remove("active");
};

export const loadMonitorData = async (options = {}) => {
  ensureMonitorState();
  // 用户管理页仅需会话列表，使用 sessions 模式避免无关的图表与热力图刷新
  const mode = options?.mode === "sessions" ? "sessions" : "full";
  const wunderBase = getWunderBase();
  // 工具列表在后台加载，避免阻塞图表首屏渲染。
  const toolListPromise =
    mode === "full"
      ? loadAvailableTools().catch((error) => {
          appendLog(t("monitor.toolListLoadFailed", { message: error.message }));
          return null;
        })
      : Promise.resolve(null);
  const params = new URLSearchParams({ active_only: "false" });
  const timeRange = resolveMonitorTimeFilterRange();
  if (timeRange) {
    params.set("start_time", (timeRange.start / 1000).toFixed(3));
    params.set("end_time", (timeRange.end / 1000).toFixed(3));
  } else {
    const toolHours = getMonitorTimeRangeHours();
    params.set("tool_hours", String(toolHours));
  }
  const endpoint = `${wunderBase}/admin/monitor?${params.toString()}`;
  const response = await fetch(endpoint);
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const result = await response.json();
  const sessions = Array.isArray(result.sessions) ? result.sessions : [];
  state.monitor.sessions = sessions;
  if (mode === "full") {
    renderMonitorMetrics(result.system);
    renderServiceMetrics(result.service);
    renderSandboxMetrics(result.sandbox);
    state.monitor.serviceSnapshot = result.service || null;
    state.monitor.toolStats = Array.isArray(result.tool_stats) ? result.tool_stats : [];
    recordTokenDeltas(sessions);
  }
  renderMonitorSessions(state.monitor.sessions);
  if (mode === "full") {
    if (elements.metricServiceTokenTotal) {
      renderServiceCharts(result.service, state.monitor.sessions);
    }
    renderToolHeatmap(state.monitor.toolStats);
    toolListPromise.then((tools) => {
      if (!tools) {
        return;
      }
      renderToolHeatmap(state.monitor.toolStats);
    });
  }
};

// 切换用户筛选条件并即时刷新线程表格
export const setMonitorUserFilter = (userId) => {
  ensureMonitorState();
  state.monitor.userFilter = String(userId || "").trim();
  // 切换用户后重置分页，避免页码落在空页面
  if (state.monitor.pagination) {
    state.monitor.pagination.activePage = 1;
    state.monitor.pagination.historyPage = 1;
  }
  renderMonitorSessions(state.monitor.sessions);
};

export const toggleMonitorPolling = (enabled, options = {}) => {
  const mode = options?.mode === "sessions" ? "sessions" : "full";
  const intervalMs =
    typeof options?.intervalMs === "number" && options.intervalMs > 0
      ? options.intervalMs
      : APP_CONFIG.monitorPollIntervalMs;
  const immediate = options?.immediate !== false;
  if (enabled) {
    const shouldRestart =
      !state.runtime.monitorPollTimer ||
      monitorPollMode !== mode ||
      monitorPollIntervalMs !== intervalMs;
    if (state.runtime.monitorPollTimer && shouldRestart) {
      clearInterval(state.runtime.monitorPollTimer);
      state.runtime.monitorPollTimer = null;
    }
    monitorPollMode = mode;
    monitorPollIntervalMs = intervalMs;
    if (!state.runtime.monitorPollTimer) {
      if (immediate) {
        loadMonitorData({ mode }).catch((error) => {
          appendLog(t("monitor.refreshFailed", { message: error.message }));
        });
      }
      // 用户管理页使用 sessions 模式轮询，降低对图表渲染的影响
      state.runtime.monitorPollTimer = setInterval(() => {
        loadMonitorData({ mode }).catch(() => {});
      }, intervalMs);
    }
  } else if (state.runtime.monitorPollTimer) {
    clearInterval(state.runtime.monitorPollTimer);
    state.runtime.monitorPollTimer = null;
  }
};

// 转义 HTML，避免事件详情中的用户内容污染展示
const escapeMonitorHtml = (value) =>
  String(value || "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");

const unwrapMonitorEventData = (payload) => {
  if (!payload || typeof payload !== "object" || Array.isArray(payload)) {
    return payload;
  }
  const hasSessionId = typeof payload.session_id === "string" && payload.session_id.trim();
  const hasTimestamp = typeof payload.timestamp === "string" && payload.timestamp.trim();
  const inner = payload.data;
  if (hasSessionId && hasTimestamp && inner && typeof inner === "object") {
    return inner;
  }
  return payload;
};

const MONITOR_TIMESTAMP_RE =
  /\[(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?(?:Z|[+-]\d{2}:?\d{2})?)\]/g;

// 高亮事件详情里的时间戳，方便快速定位条目
const highlightMonitorTimestamps = (detailText) =>
  escapeMonitorHtml(detailText).replace(
    MONITOR_TIMESTAMP_RE,
    '<span class="log-timestamp">[$1]</span>'
  );

// 统一规整工具名称，避免大小写或多余空格导致定位失败
const normalizeMonitorToolName = (value) => String(value || "").trim().toLowerCase();

// 格式化事件数据为可展示文本，确保异常数据不会打断渲染
const stringifyMonitorEventData = (data) => {
  try {
    const resolved = unwrapMonitorEventData(data);
    const text = JSON.stringify(resolved);
    return typeof text === "string" ? text : String(text);
  } catch (error) {
    return String(data);
  }
};

// 拼接单条事件文本，保持与历史展示一致
const buildMonitorEventLine = (event) => {
  const timestamp = event?.timestamp || "";
  const eventType = event?.type || "unknown";
  const dataText = stringifyMonitorEventData(event?.data);
  return `[${timestamp}] ${eventType}: ${dataText}`;
};

// 从事件数据中提取工具名称，便于定位工具调用位置
const resolveMonitorEventToolName = (event) => {
  const data = unwrapMonitorEventData(event?.data);
  if (!data || typeof data !== "object") {
    return "";
  }
  const tool = data.tool ?? data.tool_name ?? data.toolName;
  return typeof tool === "string" ? tool.trim() : "";
};

// 渲染线程事件列表，并返回需要定位的目标节点
const renderMonitorDetailEvents = (events, options = {}) => {
  if (!elements.monitorDetailEvents) {
    return null;
  }
  const container = elements.monitorDetailEvents;
  container.textContent = "";
  if (!Array.isArray(events) || events.length === 0) {
    container.textContent = t("monitor.detail.noEvents");
    return null;
  }
  const focusToolName = normalizeMonitorToolName(options.focusTool);
  const fragment = document.createDocumentFragment();
  let focusNode = null;
  let fallbackNode = null;
  events.forEach((event) => {
    const lineText = buildMonitorEventLine(event);
    const normalizedLineText = normalizeMonitorToolName(lineText);
    const line = document.createElement("div");
    line.className = "monitor-event-line";
    line.innerHTML = highlightMonitorTimestamps(lineText);
    const eventTool = resolveMonitorEventToolName(event);
    const matchesToolName =
      focusToolName &&
      (normalizeMonitorToolName(eventTool) === focusToolName ||
        normalizedLineText.includes(focusToolName));
    if (matchesToolName) {
      line.classList.add("monitor-event-line--tool");
      const eventType = String(event?.type || "").toLowerCase();
      if (!focusNode && eventType === "tool_call") {
        focusNode = line;
      } else if (!fallbackNode && eventType === "tool_result") {
        fallbackNode = line;
      }
    }
    fragment.appendChild(line);
  });
  container.appendChild(fragment);
  if (!focusNode && fallbackNode) {
    focusNode = fallbackNode;
  }
  if (focusNode) {
    focusNode.classList.add("monitor-event-line--focus");
  }
  return focusNode;
};

// 滚动事件列表到目标位置，避免用户手动查找
// 查找可滚动的父容器，兼容弹窗内部多级滚动布局
const resolveMonitorScrollContainer = (line) => {
  let current = line?.parentElement || null;
  while (current && current !== document.body) {
    const style = window.getComputedStyle(current);
    const overflowY = style?.overflowY || "";
    if (
      (overflowY === "auto" || overflowY === "scroll") &&
      current.scrollHeight > current.clientHeight
    ) {
      return current;
    }
    current = current.parentElement;
  }
  return elements.monitorDetailEvents || null;
};

// 滚动事件列表到目标位置，避免用户手动查找
const scrollMonitorDetailToLine = (line) => {
  if (!line) {
    return;
  }
  requestAnimationFrame(() => {
    requestAnimationFrame(() => {
      const container = resolveMonitorScrollContainer(line);
      if (!container) {
        return;
      }
      const containerRect = container.getBoundingClientRect();
      const lineRect = line.getBoundingClientRect();
      const offset = lineRect.top - containerRect.top;
      const target =
        container.scrollTop + offset - container.clientHeight / 2 + lineRect.height / 2;
      container.scrollTop = Math.max(0, target);
    });
  });
};

export const openMonitorDetail = async (sessionId, options = {}) => {
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/monitor/${encodeURIComponent(sessionId)}`;
  try {
    const response = await fetch(endpoint);
    if (!response.ok) {
      throw new Error(t("common.requestFailed", { status: response.status }));
    }
    const result = await response.json();
    const session = result.session || {};
    state.monitor.selected = session.session_id;
    elements.monitorDetailTitle.textContent = t("monitor.detail.title", {
      sessionId: session.session_id || "-",
    });
    elements.monitorDetailMeta.textContent = `${session.user_id || "-"} · ${getSessionStatusLabel(
      session.status
    )} · ${formatDuration(session.elapsed_s)}`;
    elements.monitorDetailQuestion.textContent = session.question || "";
    const prefillSpeed = parseMetricNumber(session.prefill_speed_tps);
    const prefillTokens = parseMetricNumber(session.prefill_tokens);
    const prefillDuration = parseMetricNumber(session.prefill_duration_s);
    const prefillLowerBound = Boolean(session.prefill_speed_lower_bound);
    if (elements.monitorDetailPrefillSpeed) {
      elements.monitorDetailPrefillSpeed.textContent = formatTokenRate(prefillSpeed, {
        lowerBound: prefillLowerBound,
      });
    }
    if (elements.monitorDetailPrefillMeta) {
      elements.monitorDetailPrefillMeta.textContent = buildSpeedMeta(
        prefillTokens,
        prefillDuration,
        { cached: prefillLowerBound }
      );
    }
    const decodeSpeed = parseMetricNumber(session.decode_speed_tps);
    const decodeTokens = parseMetricNumber(session.decode_tokens);
    const decodeDuration = parseMetricNumber(session.decode_duration_s);
    if (elements.monitorDetailDecodeSpeed) {
      elements.monitorDetailDecodeSpeed.textContent = formatTokenRate(decodeSpeed);
    }
    if (elements.monitorDetailDecodeMeta) {
      elements.monitorDetailDecodeMeta.textContent = buildSpeedMeta(decodeTokens, decodeDuration, {
        variant: "output",
      });
    }
    const events = Array.isArray(result.events) ? result.events : [];
    const focusTool =
      typeof options?.focusTool === "string" ? options.focusTool.trim() : "";
    const focusLine = renderMonitorDetailEvents(events, { focusTool });
    elements.monitorDetailModal.classList.add("active");
    if (focusTool) {
      scrollMonitorDetailToLine(focusLine);
    }
  } catch (error) {
    appendLog(t("monitor.detailLoadFailed", { message: error.message }));
  }
};

const closeMonitorDetail = () => {
  elements.monitorDetailModal.classList.remove("active");
};

const requestDeleteSession = async (sessionId) => {
  if (!sessionId) {
    return;
  }
  const confirmed = window.confirm(t("monitor.deleteConfirm", { sessionId }));
  if (!confirmed) {
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/monitor/${encodeURIComponent(sessionId)}`;
  const response = await fetch(endpoint, { method: "DELETE" });
  if (!response.ok) {
    appendLog(t("monitor.deleteFailed", { status: response.status }));
    return;
  }
  const result = await response.json();
  appendLog(result.message || t("monitor.deleted"));
  await loadMonitorData();
  if (state.monitor.selected === sessionId) {
    state.monitor.selected = null;
    closeMonitorDetail();
  }
};

const requestCancelSession = async (sessionId) => {
  if (!sessionId) {
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/monitor/${encodeURIComponent(sessionId)}/cancel`;
  const response = await fetch(endpoint, { method: "POST" });
  if (!response.ok) {
    appendLog(t("monitor.cancelFailed", { status: response.status }));
    notify(t("monitor.cancelFailed", { status: response.status }), "error");
    return;
  }
  const result = await response.json();
  appendLog(result.message || t("monitor.cancelRequested"));
  notify(result.message || t("monitor.cancelRequested"), "info");
  await loadMonitorData();
  if (state.monitor.selected === sessionId) {
    await openMonitorDetail(sessionId);
  }
};

// 初始化监控面板交互
export const initMonitorPanel = () => {
  ensureMonitorState();
  ensureMonitorCharts();
  bindMonitorPagination();
  window.addEventListener("resize", resizeMonitorCharts);
  if (elements.monitorTimeRange) {
    applyMonitorTimeRange(elements.monitorTimeRange.value || state.monitor.timeRangeHours);
    const applyInputValue = () => {
      applyMonitorTimeRange(elements.monitorTimeRange.value || state.monitor.timeRangeHours);
    };
    elements.monitorTimeRange.addEventListener("change", applyInputValue);
    elements.monitorTimeRange.addEventListener("keydown", (event) => {
      if (event.key === "Enter") {
        applyInputValue();
      }
    });
  } else {
    updateMonitorChartTitles();
  }
  syncMonitorTimeFilterInputs();
  if (elements.monitorTimeFilterToggle && elements.monitorTimeStart && elements.monitorTimeEnd) {
    const applyFilter = () => applyMonitorTimeFilter({ refresh: true });
    elements.monitorTimeFilterToggle.addEventListener("change", applyFilter);
    elements.monitorTimeStart.addEventListener("change", applyFilter);
    elements.monitorTimeEnd.addEventListener("change", applyFilter);
  }
  elements.monitorRefreshBtn.addEventListener("click", async () => {
    try {
      await loadMonitorData();
      notify(t("monitor.refreshSuccess"), "success");
    } catch (error) {
      appendLog(t("monitor.refreshFailed", { message: error.message }));
      notify(t("monitor.refreshFailed", { message: error.message }), "error");
    }
  });
  elements.monitorDetailClose.addEventListener("click", closeMonitorDetail);
  elements.monitorDetailCloseBtn.addEventListener("click", closeMonitorDetail);
  elements.monitorDetailModal.addEventListener("click", (event) => {
    if (event.target === elements.monitorDetailModal) {
      closeMonitorDetail();
    }
  });
  if (elements.monitorStatusClose) {
    elements.monitorStatusClose.addEventListener("click", closeMonitorStatusModal);
  }
  if (elements.monitorStatusCloseBtn) {
    elements.monitorStatusCloseBtn.addEventListener("click", closeMonitorStatusModal);
  }
  if (elements.monitorStatusModal) {
    elements.monitorStatusModal.addEventListener("click", (event) => {
      if (event.target === elements.monitorStatusModal) {
        closeMonitorStatusModal();
      }
    });
  }
  if (elements.monitorToolClose) {
    elements.monitorToolClose.addEventListener("click", closeMonitorToolModal);
  }
  if (elements.monitorToolCloseBtn) {
    elements.monitorToolCloseBtn.addEventListener("click", closeMonitorToolModal);
  }
  if (elements.monitorToolModal) {
    elements.monitorToolModal.addEventListener("click", (event) => {
      if (event.target === elements.monitorToolModal) {
        closeMonitorToolModal();
      }
    });
  }
};
