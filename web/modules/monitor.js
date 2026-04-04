import { APP_CONFIG } from "../app.config.js?v=20260110-04";
import { elements } from "./elements.js?v=20260215-01";
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
import { getCurrentLanguage, t } from "./i18n.js?v=20260215-01";

const ONE_HOUR_MS = 60 * 60 * 1000;
const DEFAULT_MONITOR_TIME_RANGE_HOURS = 3;
// Token 趋势默认展示的时间桶数量，避免折线图从最早记录开始导致卡顿
const TOKEN_TREND_MAX_BUCKETS = 24;
// Token 趋势保留的最大时间桶数量，避免长期运行后趋势数据膨胀
const TOKEN_TREND_RETENTION_BUCKETS = 96;
// 用户管理线程列表分页尺寸，避免一次渲染过多行
const DEFAULT_MONITOR_SESSION_PAGE_SIZE = 100;
let tokenTrendChart = null;
let statusChart = null;
let statusChartClickBound = false;
let tokenTrendZoomBound = false;
let mcpToolNameSet = new Set();
let userDashboardLoading = false;
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
const USER_DASHBOARD_TTL_MS = 60 * 1000;
// 热力图需要区分常见文件操作工具的图标，避免全部显示为同一文件样式
const TOOL_HEATMAP_ICON_RULES = [
  { keyword: "用户世界工具", icon: "fa-earth-asia" },
  { keyword: "user_world", icon: "fa-earth-asia" },
  { keyword: "user world", icon: "fa-earth-asia" },
  { keyword: "会话让出", icon: "fa-share-from-square" },
  { keyword: "sessions_yield", icon: "fa-share-from-square" },
  { keyword: "session yield", icon: "fa-share-from-square" },
  { keyword: "yield", icon: "fa-share-from-square" },
  { keyword: "自我状态", icon: "fa-gauge-high" },
  { keyword: "self_status", icon: "fa-gauge-high" },
  { keyword: "self status", icon: "fa-gauge-high" },
  { keyword: "桌面控制器", icon: "fa-computer-mouse" },
  { keyword: "desktop_controller", icon: "fa-computer-mouse" },
  { keyword: "desktop controller", icon: "fa-computer-mouse" },
  { keyword: "桌面监视器", icon: "fa-display" },
  { keyword: "桌面监控", icon: "fa-display" },
  { keyword: "desktop_monitor", icon: "fa-display" },
  { keyword: "desktop monitor", icon: "fa-display" },
  { keyword: "计划面板", icon: "fa-table-columns" },
  { keyword: "计划看板", icon: "fa-table-columns" },
  { keyword: "update_plan", icon: "fa-table-columns" },
  { keyword: "plan board", icon: "fa-table-columns" },
  { keyword: "问询面板", icon: "fa-circle-question" },
  { keyword: "question_panel", icon: "fa-circle-question" },
  { keyword: "ask_panel", icon: "fa-circle-question" },
  { keyword: "question panel", icon: "fa-circle-question" },
  { keyword: "浏览器", icon: "fa-window-maximize" },
  { keyword: "browser", icon: "fa-window-maximize" },
  { keyword: "browser_navigate", icon: "fa-window-maximize" },
  { keyword: "browser_click", icon: "fa-window-maximize" },
  { keyword: "browser_type", icon: "fa-window-maximize" },
  { keyword: "browser_screenshot", icon: "fa-window-maximize" },
  { keyword: "browser_read_page", icon: "fa-window-maximize" },
  { keyword: "节点调用", icon: "fa-diagram-project" },
  { keyword: "node.invoke", icon: "fa-diagram-project" },
  { keyword: "node_invoke", icon: "fa-diagram-project" },
  { keyword: "node invoke", icon: "fa-diagram-project" },
  { keyword: "gateway_invoke", icon: "fa-diagram-project" },
  { keyword: "技能调用", icon: "fa-wand-magic-sparkles" },
  { keyword: "skill_call", icon: "fa-wand-magic-sparkles" },
  { keyword: "skill_get", icon: "fa-wand-magic-sparkles" },
  { keyword: "智能体蜂群", icon: "fa-bee" },
  { keyword: "子智能体控制", icon: "fa-diagram-project" },
  { keyword: "subagent_control", icon: "fa-diagram-project" },
  { keyword: "会话线程控制", icon: "fa-code-branch" },
  { keyword: "thread_control", icon: "fa-code-branch" },
  { keyword: "session_thread", icon: "fa-code-branch" },
  { keyword: "agent_swarm", icon: "fa-bee" },
  { keyword: "swarm_control", icon: "fa-bee" },
  { keyword: "网页抓取", icon: "fa-globe" },
  { keyword: "web_fetch", icon: "fa-globe" },
  { keyword: "web fetch", icon: "fa-globe" },
  { keyword: "webfetch", icon: "fa-globe" },
  { keyword: "a2a观察", icon: "fa-glasses" },
  { keyword: "a2a_observe", icon: "fa-glasses" },
  { keyword: "a2a等待", icon: "fa-clock" },
  { keyword: "a2a_wait", icon: "fa-clock" },
  { keyword: "休眠等待", icon: "fa-hourglass-half" },
  { keyword: "sleep_wait", icon: "fa-hourglass-half" },
  { keyword: "sleep", icon: "fa-hourglass-half" },
  { keyword: "pause", icon: "fa-hourglass-half" },
  { keyword: "记忆管理", icon: "fa-memory" },
  { keyword: "memory_manager", icon: "fa-memory" },
  { keyword: "memory_manage", icon: "fa-memory" },
  { keyword: "memory manager", icon: "fa-memory" },
  { keyword: "a2a@", icon: "fa-diagram-project" },
  { keyword: "a2ui", icon: "fa-image" },
  { keyword: "读图工具", icon: "fa-image" },
  { keyword: "read_image", icon: "fa-image" },
  { keyword: "read image", icon: "fa-image" },
  { keyword: "view_image", icon: "fa-image" },
  { keyword: "view image", icon: "fa-image" },
  { keyword: "渠道工具", icon: "fa-comments" },
  { keyword: "channel_tool", icon: "fa-comments" },
  { keyword: "channel tool", icon: "fa-comments" },
  { keyword: "channel_send", icon: "fa-comments" },
  { keyword: "channel_contacts", icon: "fa-comments" },
  { keyword: "列出文件", icon: "fa-folder-open" },
  { keyword: "list files", icon: "fa-folder-open" },
  { keyword: "list_file", icon: "fa-folder-open" },
  { keyword: "list_files", icon: "fa-folder-open" },
  { keyword: "读取文件", icon: "fa-file-lines" },
  { keyword: "read file", icon: "fa-file-lines" },
  { keyword: "read_file", icon: "fa-file-lines" },
  { keyword: "写入文件", icon: "fa-file-circle-plus" },
  { keyword: "write file", icon: "fa-file-circle-plus" },
  { keyword: "write_file", icon: "fa-file-circle-plus" },
  { keyword: "应用补丁", icon: "fa-pen-to-square" },
  { keyword: "apply patch", icon: "fa-pen-to-square" },
  { keyword: "apply_patch", icon: "fa-pen-to-square" },
  { keyword: "LSP查询", icon: "fa-code" },
  { keyword: "lsp query", icon: "fa-code" },
  { keyword: "lsp", icon: "fa-code" },
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
// 线程状态图例与后端状态字段映射，便于点击后过滤记录
const getStatusLabelToKey = () => ({
  [t("monitor.status.active")]: "active",
  [t("monitor.status.finished")]: "finished",
  [t("monitor.status.failed")]: "error",
  [t("monitor.status.cancelled")]: "cancelled",
});

// 兼容旧版状态结构，避免缓存 state.js 时导致监控图表异常
const MONITOR_DETAIL_TEXT_FALLBACKS = {
  "monitor.detail.filter.allTypes": {
    zh: "全部类型",
    en: "All event types",
  },
  "monitor.detail.filter.keywordPlaceholder": {
    zh: "输入事件关键词",
    en: "Search event payload",
  },
  "monitor.detail.filter.stats": {
    zh: "显示 {visible}/{total} 条",
    en: "Showing {visible}/{total}",
  },
  "monitor.detail.filter.profile.normal": {
    zh: "普通日志",
    en: "Normal logs",
  },
  "monitor.detail.filter.profile.debug": {
    zh: "调试日志",
    en: "Debug logs",
  },
  "monitor.detail.meta.trace": {
    zh: "追踪 {traceId}",
    en: "Trace {traceId}",
  },
  "monitor.detail.repair.badge": {
    zh: "已修复",
    en: "Repaired",
  },
  "monitor.detail.repair.argsSummary": {
    zh: "参数已修复",
    en: "Args repaired",
  },
  "monitor.detail.repair.historySummary": {
    zh: "已清洗 {count} 条历史参数",
    en: "Sanitized {count} history args",
  },
  "monitor.detail.repair.lossyJson": {
    zh: "已在执行前修复损坏的 JSON 参数",
    en: "Repaired malformed JSON arguments before execution",
  },
  "monitor.detail.repair.rawWrapped": {
    zh: "已在执行前包装原始参数，避免上游请求失败",
    en: "Wrapped raw arguments before execution to avoid upstream failures",
  },
  "monitor.detail.repair.nonObjectWrapped": {
    zh: "已在执行前将非对象参数包装为 JSON",
    en: "Wrapped non-object arguments into JSON before execution",
  },
  "monitor.detail.repair.sanitizeBeforeRequest": {
    zh: "已在请求前清洗 {count} 条工具调用参数",
    en: "Sanitized {count} tool-call argument payloads before request",
  },
};

const applyMonitorDetailTextParams = (template, params = {}) => {
  return Object.keys(params).reduce(
    (result, paramKey) =>
      result.replace(new RegExp(`\{${paramKey}\}`, "g"), String(params[paramKey])),
    template
  );
};

const resolveMonitorDetailText = (key, params = {}) => {
  const translated = t(key, params);
  if (translated && translated !== key) {
    return translated;
  }
  const fallback = MONITOR_DETAIL_TEXT_FALLBACKS[key];
  if (!fallback) {
    return translated || key;
  }
  const language = String(getCurrentLanguage() || "").toLowerCase();
  const template = language.startsWith("en") ? fallback.en : fallback.zh;
  return applyMonitorDetailTextParams(template, params);
};

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
      sessionStatusFilter: "all",
      feedbackFilter: "all",
      timeRangeHours: DEFAULT_MONITOR_TIME_RANGE_HOURS,
      serviceSnapshot: null,
      pagination: {
        pageSize: DEFAULT_MONITOR_SESSION_PAGE_SIZE,
        activePage: 1,
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
  if (!Object.prototype.hasOwnProperty.call(state.monitor, "detail")) {
    state.monitor.detail = null;
  } else if (state.monitor.detail && typeof state.monitor.detail !== "object") {
    state.monitor.detail = null;
  }
  if (!state.monitor.detailFilters || typeof state.monitor.detailFilters !== "object") {
    state.monitor.detailFilters = {
      eventType: "",
      keyword: "",
      round: 0,
    };
  }
  if (typeof state.monitor.detailFilters.eventType !== "string") {
    state.monitor.detailFilters.eventType = "";
  }
  if (typeof state.monitor.detailFilters.keyword !== "string") {
    state.monitor.detailFilters.keyword = "";
  }
  const parsedDetailRound = Number.parseInt(
    String(state.monitor.detailFilters.round ?? 0),
    10
  );
  state.monitor.detailFilters.round =
    Number.isFinite(parsedDetailRound) && parsedDetailRound > 0
      ? parsedDetailRound
      : 0;
  if (typeof state.monitor.tokenZoomLocked !== "boolean") {
    state.monitor.tokenZoomLocked = false;
  }
  if (typeof state.monitor.tokenZoomInitialized !== "boolean") {
    state.monitor.tokenZoomInitialized = false;
  }
  if (typeof state.monitor.userFilter !== "string") {
    state.monitor.userFilter = "";
  }
  const normalizedSessionStatusFilter = String(
    state.monitor.sessionStatusFilter || ""
  )
    .trim()
    .toLowerCase();
  state.monitor.sessionStatusFilter = [
    "all",
    "active",
    "history",
    "finished",
    "error",
    "cancelled",
  ].includes(normalizedSessionStatusFilter)
    ? normalizedSessionStatusFilter
    : "all";
  const normalizedFeedbackFilter = String(state.monitor.feedbackFilter || "")
    .trim()
    .toLowerCase();
  state.monitor.feedbackFilter = ["all", "up", "down", "none", "mixed"].includes(
    normalizedFeedbackFilter
  )
    ? normalizedFeedbackFilter
    : "all";
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
  // 鍒嗛〉鐘舵€佸吋瀹规棫缂撳瓨锛岄伩鍏嶅垏鎹㈢敤鎴锋垨鍒锋柊鍚庨〉鐮佸紓甯?
  if (!state.monitor.pagination || typeof state.monitor.pagination !== "object") {
    state.monitor.pagination = {
      pageSize: DEFAULT_MONITOR_SESSION_PAGE_SIZE,
      activePage: 1,
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
};

// 鏍煎紡鍖栫洃瑙嗘椂闂达紝淇濊瘉灞曠ず绠€娲?
const formatMonitorHours = (value) => {
  const hours = Number(value);
  if (!Number.isFinite(hours)) {
    return String(DEFAULT_MONITOR_TIME_RANGE_HOURS);
  }
  return hours.toFixed(2).replace(/\.?0+$/, "");
};

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

const parseMonitorFilterTimestamp = (value) => {
  if (!value) {
    return null;
  }
  const parsed = new Date(value).getTime();
  return Number.isFinite(parsed) ? parsed : null;
};

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

// 搴旂敤绛涢€夋椂闂村苟鍒锋柊鍥捐〃
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

// 鐐瑰嚮绾跨▼鐘舵€佺幆鍥炬椂鎵撳紑瀵瑰簲璁板綍鍒楄〃
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
  const prefix = options.lowerBound ? ">=" : "";
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

const formatDurationSeconds = (seconds) => {
  if (!Number.isFinite(seconds)) {
    return "-";
  }
  const value = Math.max(0, Number(seconds));
  return `${value.toFixed(2)}s`;
};

const parseMetricNumber = (value) => {
  if (value === null || value === undefined) {
    return null;
  }
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
};

const resolveSessionContextTokens = (session) => {
  const peak = parseMetricNumber(session?.context_tokens_peak);
  if (Number.isFinite(peak)) {
    return peak;
  }
  const current = parseMetricNumber(session?.context_tokens);
  if (Number.isFinite(current)) {
    return current;
  }
  const legacy = parseMetricNumber(session?.token_usage);
  return Number.isFinite(legacy) ? legacy : null;
};

// Cumulative consumed tokens for billing/cost tracking
const resolveSessionConsumedTokens = (session) => {
  const consumed = parseMetricNumber(session?.consumed_tokens);
  if (Number.isFinite(consumed) && consumed > 0) {
    return consumed;
  }
  // Fallback: estimate from context tokens peak (legacy sessions)
  const peak = parseMetricNumber(session?.context_tokens_peak);
  if (Number.isFinite(peak)) {
    return peak;
  }
  return parseMetricNumber(session?.context_tokens) || null;
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

const floorToIntervalBoundary = (timestamp, intervalMs) => {
  const date = new Date(timestamp);
  const midnight = new Date(date);
  midnight.setHours(0, 0, 0, 0);
  const offset = timestamp - midnight.getTime();
  const index = Math.floor(offset / intervalMs);
  return midnight.getTime() + index * intervalMs;
};

// 记录 token 澧為噺锛屼究浜庢寜灏忔椂姹囨€?
const recordTokenDeltas = (sessions) => {
  const usageMap = state.monitor.tokenUsageBySession;
  (sessions || []).forEach((session) => {
    const sessionId = session?.session_id;
    if (!sessionId) {
      return;
    }
    const current = resolveSessionConsumedTokens(session) || 0;
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

// 瑁佸壀杩囨棫鐨?token 增量，避免长期运行后趋势数据膨胀
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

// 姹囨€?token 增量，生成按时间间隔的折线图数据
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

// 灏?HSL 转为 RGB，便于计算文字对比色
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
  if (lowerName === "wunder@excute" || lowerName.endsWith("@wunder@excute")) {
    return "fa-dragon";
  }
  if (lowerName === "wunder@doc2md" || lowerName.endsWith("@wunder@doc2md")) {
    return "fa-file-lines";
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
    matchesToolKeyword(lowerName, normalizedName, "执行命令") ||
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
    matchesToolKeyword(lowerName, normalizedName, "定时任务") ||
    matchesToolKeyword(lowerName, normalizedName, "计划任务") ||
    matchesToolKeyword(lowerName, normalizedName, "cron") ||
    matchesToolKeyword(lowerName, normalizedName, "schedule") ||
    matchesToolKeyword(lowerName, normalizedName, "scheduled") ||
    matchesToolKeyword(lowerName, normalizedName, "timer") ||
    matchesToolKeyword(lowerName, normalizedName, "schedule_task")
  ) {
    return "fa-clock";
  }
  if (
    matchesToolKeyword(lowerName, normalizedName, "搜索") ||
    matchesToolKeyword(lowerName, normalizedName, "检索") ||
    matchesToolKeyword(lowerName, normalizedName, "search") ||
    matchesToolKeyword(lowerName, normalizedName, "query") ||
    matchesToolKeyword(lowerName, normalizedName, "retrieve") ||
    matchesToolKeyword(lowerName, normalizedName, "search_content")
  ) {
    return "fa-magnifying-glass";
  }
  if (
    matchesToolKeyword(lowerName, normalizedName, "读取") ||
    matchesToolKeyword(lowerName, normalizedName, "写入") ||
    matchesToolKeyword(lowerName, normalizedName, "编辑") ||
    matchesToolKeyword(lowerName, normalizedName, "替换") ||
    matchesToolKeyword(lowerName, normalizedName, "列出") ||
    matchesToolKeyword(lowerName, normalizedName, "read") ||
    matchesToolKeyword(lowerName, normalizedName, "write") ||
    matchesToolKeyword(lowerName, normalizedName, "edit") ||
    matchesToolKeyword(lowerName, normalizedName, "replace") ||
    matchesToolKeyword(lowerName, normalizedName, "list")
  ) {
    return "fa-file-lines";
  }
  if (
    matchesToolKeyword(lowerName, normalizedName, "知识") ||
    matchesToolKeyword(lowerName, normalizedName, "knowledge")
  ) {
    return "fa-book";
  }
  if (
    matchesToolKeyword(lowerName, normalizedName, "最终回复") ||
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
    const iconToken = resolveToolIcon(item.name, item.category);
    icon.className = `fa-solid ${iconToken}`;
    const name = document.createElement("span");
    name.className = "tool-heatmap-name";
    name.textContent = item.name;
    tile.appendChild(icon);
    tile.appendChild(name);
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
    elements.metricUptime.textContent = "-";
    elements.metricDisk.textContent = "-";
    elements.metricDiskDetail.textContent = "";
    elements.metricLogUsage.textContent = "-";
    elements.metricWorkspaceUsage.textContent = "-";
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
  elements.metricLogUsage.textContent = formatBytes(system.log_used);
  elements.metricWorkspaceUsage.textContent = formatBytes(system.workspace_used);
};

const renderServiceMetrics = (service) => {
  if (!service) {
    elements.metricServiceActive.textContent = "-";
    elements.metricServiceHistory.textContent = "-";
    elements.metricServiceFinished.textContent = "-";
    elements.metricServiceError.textContent = "-";
    elements.metricServiceCancelled.textContent = "-";
    elements.metricServiceTotal.textContent = "-";
    if (elements.metricServiceTokenAvg) {
      elements.metricServiceTokenAvg.textContent = "-";
    }
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
  if (elements.metricServiceTokenAvg) {
    const avgTokens = parseMetricNumber(service.avg_context_tokens);
    elements.metricServiceTokenAvg.textContent = formatTokenCount(avgTokens);
  }
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

// 渲染用户看板指标，复用用户管理页统计并加 TTL 避免频繁请求
const ensureUserDashboardState = () => {
  if (!state.users || typeof state.users !== "object") {
    state.users = { list: [], loaded: false, updatedAt: 0 };
  }
  if (!Array.isArray(state.users.list)) {
    state.users.list = [];
  }
  if (typeof state.users.loaded !== "boolean") {
    state.users.loaded = false;
  }
  if (!Number.isFinite(state.users.updatedAt)) {
    state.users.updatedAt = 0;
  }
  if (!("summary" in state.users)) {
    state.users.summary = null;
  }
  if (!Number.isFinite(state.users.summaryUpdatedAt)) {
    state.users.summaryUpdatedAt = 0;
  }
};

const normalizeUserDashboardStats = (item) => ({
  user_id: String(item?.user_id || ""),
  active_sessions: Number(item?.active_sessions) || 0,
  history_sessions: Number(item?.history_sessions) || 0,
  total_sessions: Number(item?.total_sessions) || 0,
  chat_records: Number(item?.chat_records) || 0,
  tool_calls: Number(item?.tool_calls) || 0,
  context_tokens: Number(item?.context_tokens) || 0,
});

const resolveUserDashboardSummary = () => {
  ensureUserDashboardState();
  if (
    state.users.summary &&
    Number.isFinite(state.users.summaryUpdatedAt) &&
    state.users.summaryUpdatedAt === state.users.updatedAt
  ) {
    const cachedSummary = state.users.summary;
    const hasData = Boolean(state.users.loaded || cachedSummary.user_count > 0);
    return { summary: cachedSummary, hasData };
  }
  const summary = {
    user_count: 0,
    active_sessions: 0,
    history_sessions: 0,
    total_sessions: 0,
    chat_records: 0,
    tool_calls: 0,
    context_tokens: 0,
  };
  if (!Array.isArray(state.users.list)) {
    return { summary, hasData: false };
  }
  summary.user_count = state.users.list.length;
  state.users.list.forEach((item) => {
    summary.active_sessions += Number(item?.active_sessions) || 0;
    summary.history_sessions += Number(item?.history_sessions) || 0;
    summary.total_sessions += Number(item?.total_sessions) || 0;
    summary.chat_records += Number(item?.chat_records) || 0;
    summary.tool_calls += Number(item?.tool_calls) || 0;
    summary.context_tokens += Number(item?.context_tokens) || 0;
  });
  state.users.summary = summary;
  state.users.summaryUpdatedAt = state.users.updatedAt;
  const hasData = Boolean(state.users.loaded || summary.user_count > 0);
  return { summary, hasData };
};

const renderUserDashboardMetrics = (summary, hasData) => {
  if (!elements.metricUserCount) {
    return;
  }
  if (!hasData || !summary) {
    elements.metricUserCount.textContent = "-";
    elements.metricUserSessions.textContent = "-";
    elements.metricUserRecords.textContent = "-";
    elements.metricUserTools.textContent = "-";
    elements.metricUserActive.textContent = "-";
    elements.metricUserTokens.textContent = "-";
    return;
  }
  elements.metricUserCount.textContent = `${summary.user_count}`;
  elements.metricUserSessions.textContent = `${summary.total_sessions}`;
  elements.metricUserRecords.textContent = `${summary.chat_records}`;
  elements.metricUserTools.textContent = `${summary.tool_calls}`;
  elements.metricUserActive.textContent = `${summary.active_sessions}`;
  elements.metricUserTokens.textContent = formatTokenCount(summary.consumed_tokens || summary.context_tokens);
};

const shouldRefreshUserDashboard = (options = {}) => {
  const { force = false } = options;
  if (force) {
    return true;
  }
  const updatedAt = Number(state.users.updatedAt) || 0;
  if (!updatedAt) {
    return true;
  }
  return Date.now() - updatedAt > USER_DASHBOARD_TTL_MS;
};

const refreshUserDashboardSummary = async (options = {}) => {
  ensureUserDashboardState();
  if (!shouldRefreshUserDashboard(options)) {
    const { summary, hasData } = resolveUserDashboardSummary();
    renderUserDashboardMetrics(summary, hasData);
    return summary;
  }
  if (userDashboardLoading) {
    return null;
  }
  userDashboardLoading = true;
  try {
    const wunderBase = getWunderBase();
    const endpoint = `${wunderBase}/admin/users`;
    const response = await fetch(endpoint);
    if (response.ok) {
      const result = await response.json();
      state.users.list = Array.isArray(result.users)
        ? result.users.map(normalizeUserDashboardStats)
        : [];
      state.users.loaded = true;
      state.users.updatedAt = Date.now();
    }
  } catch (error) {
    state.users.updatedAt = Date.now();
  } finally {
    userDashboardLoading = false;
    const { summary, hasData } = resolveUserDashboardSummary();
    renderUserDashboardMetrics(summary, hasData);
  }
  return null;
};

// 计算所有会话累计 consumed token 数量
const resolveTotalTokens = (sessions) =>
  (sessions || []).reduce((sum, session) => sum + (resolveSessionConsumedTokens(session) || 0), 0);

const parseMonitorTimestamp = (value) => {
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : null;
};

const resolveMonitorDetailTimeRange = (session, events) => {
  let startMs = null;
  let endMs = null;
  (Array.isArray(events) ? events : []).forEach((event) => {
    const ts = parseMonitorTimestamp(event?.timestamp);
    if (!Number.isFinite(ts)) {
      return;
    }
    startMs = startMs === null ? ts : Math.min(startMs, ts);
    endMs = endMs === null ? ts : Math.max(endMs, ts);
  });
  const fallbackStart = parseMonitorTimestamp(session?.start_time);
  if (startMs === null && Number.isFinite(fallbackStart)) {
    startMs = fallbackStart;
  }
  const fallbackEnd = parseMonitorTimestamp(session?.updated_time || session?.start_time);
  if (endMs === null && Number.isFinite(fallbackEnd)) {
    endMs = fallbackEnd;
  }
  if (startMs !== null && endMs !== null && endMs < startMs) {
    const swapped = startMs;
    startMs = endMs;
    endMs = swapped;
  }
  return { startMs, endMs };
};

const resolveMonitorDetailElapsedSeconds = (session, events) => {
  const { startMs, endMs } = resolveMonitorDetailTimeRange(session, events);
  if (Number.isFinite(startMs) && Number.isFinite(endMs)) {
    return Math.max(0, (endMs - startMs) / 1000);
  }
  const fallback = parseMetricNumber(session?.elapsed_s);
  return Number.isFinite(fallback) ? fallback : null;
};

const normalizeMonitorDetailCount = (value) => {
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : 0;
};

const resolveMonitorDetailToolCalls = (session, events) => {
  let calls = 0;
  (Array.isArray(events) ? events : []).forEach((event) => {
    if (event?.type === "tool_call") {
      calls += 1;
    }
  });
  if (calls <= 0) {
    const fallback = parseMetricNumber(session?.tool_calls);
    if (Number.isFinite(fallback)) {
      calls = Math.max(0, Math.round(fallback));
    }
  }
  return calls;
};

const resolveMonitorDetailQuota = (session, events) => {
  let consumed = 0;
  (Array.isArray(events) ? events : []).forEach((event) => {
    if (event?.type !== "quota_usage") {
      return;
    }
    const data = event?.data;
    const rawIncrement =
      data && typeof data === "object" ? data.consumed ?? data.count ?? data.used : null;
    const increment = normalizeMonitorDetailCount(rawIncrement);
    consumed += increment > 0 ? increment : 1;
  });
  if (consumed <= 0) {
    const fallback = parseMetricNumber(
      session?.quota_consumed ?? session?.quotaConsumed ?? session?.quota
    );
    if (Number.isFinite(fallback)) {
      consumed = Math.max(0, Math.round(fallback));
    }
  }
  return consumed;
};

const resolveUsageTotals = (payload) => {
  if (!payload || typeof payload !== "object") {
    return null;
  }
  const source =
    payload.usage && typeof payload.usage === "object" ? payload.usage : payload;
  const input = parseMetricNumber(source.input_tokens ?? source.input);
  const output = parseMetricNumber(source.output_tokens ?? source.output);
  let total = parseMetricNumber(source.total_tokens ?? source.total);
  if (!Number.isFinite(total)) {
    if (Number.isFinite(input) || Number.isFinite(output)) {
      total = (input || 0) + (output || 0);
    }
  }
  if (!Number.isFinite(total) || total <= 0) {
    return null;
  }
  return { input, output, total };
};

const resolveMonitorDetailBillingUsage = (session, events) => {
  let usage = null;
  (Array.isArray(events) ? events : []).forEach((event) => {
    if (event?.type === "round_usage") {
      const next = resolveUsageTotals(event?.data);
      if (next) {
        usage = next;
      }
      return;
    }
    if (event?.type === "final" && !usage) {
      const next = resolveUsageTotals(event?.data);
      if (next) {
        usage = next;
      }
    }
  });
  if (!usage) {
    usage =
      resolveUsageTotals(session?.round_usage) ||
      resolveUsageTotals(session?.usage) ||
      null;
  }
  return usage;
};

const buildMonitorDetailSpeedSummary = (
  labelKey,
  speed,
  tokens,
  duration,
  options = {}
) => {
  if (!Number.isFinite(speed) || speed <= 0) {
    return "";
  }
  const speedText = formatTokenRate(speed, { lowerBound: options.lowerBound });
  if (!speedText || speedText === "-") {
    return "";
  }
  let summary = t(labelKey, { speed: speedText });
  const meta = buildSpeedMeta(tokens, duration, {
    cached: options.lowerBound,
    variant: options.variant,
  });
  if (meta) {
    summary = `${summary} (${meta})`;
  }
  return summary;
};

const resolveMonitorSessionAgentDisplay = (session) => {
  const agentName = String(session?.agent_name || "").trim();
  const agentId = String(session?.agent_id || "").trim();
  if (agentName && agentId && agentName !== agentId) {
    return `${agentName} (${agentId})`;
  }
  if (agentName) {
    return agentName;
  }
  if (agentId) {
    return agentId;
  }
  return "-";
};

const buildMonitorDetailMeta = (session, events) => {
  const metaParts = [];
  metaParts.push(session?.user_id || "-");
  metaParts.push(
    t("monitor.session.agent", { agent: resolveMonitorSessionAgentDisplay(session) })
  );
  metaParts.push(getSessionStatusLabel(session?.status));
  const logProfile = String(session?.log_profile || "").trim().toLowerCase();
  if (logProfile) {
    const profileKey =
      logProfile === "debug"
        ? "monitor.detail.filter.profile.debug"
        : "monitor.detail.filter.profile.normal";
    metaParts.push(resolveMonitorDetailText(profileKey));
  }
  const traceId = String(session?.trace_id || "").trim();
  if (traceId) {
    metaParts.push(resolveMonitorDetailText("monitor.detail.meta.trace", { traceId }));
  }
  const elapsedSeconds = resolveMonitorDetailElapsedSeconds(session, events);
  if (Number.isFinite(elapsedSeconds)) {
    metaParts.push(
      t("monitor.session.elapsed", { elapsed: formatDurationSeconds(elapsedSeconds) })
    );
  }
  const toolCalls = resolveMonitorDetailToolCalls(session, events);
  if (toolCalls > 0) {
    metaParts.push(t("monitor.tool.calls", { count: formatHeatmapCount(toolCalls) }));
  }
  const quotaConsumed = resolveMonitorDetailQuota(session, events);
  if (quotaConsumed > 0) {
    metaParts.push(
      t("monitor.session.quota", { count: formatHeatmapCount(quotaConsumed) })
    );
  }
  const consumedTokens = resolveSessionConsumedTokens(session);
  if (Number.isFinite(consumedTokens) && consumedTokens > 0) {
    const tokenText = formatTokenCount(consumedTokens);
    if (tokenText && tokenText !== "-") {
      metaParts.push(t("monitor.session.consumedTokens", { token: tokenText }));
    }
  }
  const prefillSpeed = parseMetricNumber(session?.prefill_speed_tps);
  const prefillTokens = parseMetricNumber(session?.prefill_tokens);
  const prefillDuration = parseMetricNumber(session?.prefill_duration_s);
  const prefillLowerBound = Boolean(session?.prefill_speed_lower_bound);
  const prefillSummary = buildMonitorDetailSpeedSummary(
    "monitor.session.prefillSpeed",
    prefillSpeed,
    prefillTokens,
    prefillDuration,
    { lowerBound: prefillLowerBound }
  );
  if (prefillSummary) {
    metaParts.push(prefillSummary);
  }
  const decodeSpeed = parseMetricNumber(session?.decode_speed_tps);
  const decodeTokens = parseMetricNumber(session?.decode_tokens);
  const decodeDuration = parseMetricNumber(session?.decode_duration_s);
  const decodeSummary = buildMonitorDetailSpeedSummary(
    "monitor.session.decodeSpeed",
    decodeSpeed,
    decodeTokens,
    decodeDuration,
    { variant: "output" }
  );
  if (decodeSummary) {
    metaParts.push(decodeSummary);
  }
  return metaParts.filter(Boolean).join(" · ");
};

// 鑾峰彇浼氳瘽鐨勫彲姣旇緝鏃堕棿鎴?
const resolveSessionTimestamp = (session) => {
  const updated = parseMonitorTimestamp(session?.updated_time);
  if (updated) {
    return updated;
  }
  const started = parseMonitorTimestamp(session?.start_time);
  return started || null;
};

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

// 更新 token 瓒嬪娍鎶樼嚎鍥?
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
    option.dataZoom = [zoomConfig];
  }
  tokenTrendChart.setOption(option, false);
  state.monitor.tokenZoomInitialized = true;
  if (tokenTrendChart) {
    tokenTrendChart.resize();
  }
};

// 姹囨€荤嚎绋嬬姸鎬佸崰姣旓紝渚夸簬鍥捐〃灞曠ず
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

// 鏇存柊鏈嶅姟鐘舵€佸崰姣斿浘琛?
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

const renderServiceCharts = (service, sessions) => {
  updateMonitorChartTitles();
  const scopedSessions = filterSessionsByInterval(sessions);
  const totalTokens = resolveTotalTokens(scopedSessions);
  if (elements.metricServiceTokenTotal) {
    elements.metricServiceTokenTotal.textContent = formatTokenCount(totalTokens);
  }
  if (!ensureMonitorCharts()) {
    return;
  }
  renderTokenTrendChart();
  renderServiceStatusChart(service, scopedSessions);
  resizeMonitorCharts();
};

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

const filterSessionsByUser = (sessions) => {
  const userId = String(state.monitor.userFilter || "").trim();
  if (!userId) {
    return sessions;
  }
  return (sessions || []).filter((session) => String(session?.user_id || "") === userId);
};

const normalizeMonitorSessionStatusFilter = (value) => {
  const normalized = String(value || "")
    .trim()
    .toLowerCase();
  if (
    ["all", "active", "history", "finished", "error", "cancelled"].includes(
      normalized
    )
  ) {
    return normalized;
  }
  return "all";
};

const normalizeMonitorFeedbackFilter = (value) => {
  const normalized = String(value || "")
    .trim()
    .toLowerCase();
  if (["all", "up", "down", "none", "mixed"].includes(normalized)) {
    return normalized;
  }
  return "all";
};

const resolveSessionFeedbackCounts = (session) => {
  const up = Math.max(0, Math.floor(Number(session?.feedback_up_count) || 0));
  const down = Math.max(0, Math.floor(Number(session?.feedback_down_count) || 0));
  const totalRaw = Number(session?.feedback_total_count);
  const total =
    Number.isFinite(totalRaw) && totalRaw >= 0 ? Math.floor(totalRaw) : up + down;
  return { up, down, total };
};

const resolveSessionFeedbackStatus = (session) => {
  const normalized = String(session?.feedback_status || "")
    .trim()
    .toLowerCase();
  if (["up", "down", "mixed", "none"].includes(normalized)) {
    return normalized;
  }
  const counts = resolveSessionFeedbackCounts(session);
  if (counts.total <= 0) return "none";
  if (counts.up > 0 && counts.down > 0) return "mixed";
  if (counts.up > 0) return "up";
  if (counts.down > 0) return "down";
  return "none";
};

const filterSessionsByStatus = (sessions) => {
  const filter = normalizeMonitorSessionStatusFilter(
    state.monitor.sessionStatusFilter
  );
  if (filter === "all") {
    return sessions;
  }
  if (filter === "active") {
    return (sessions || []).filter((session) => ACTIVE_STATUSES.has(session?.status));
  }
  if (filter === "history") {
    return (sessions || []).filter((session) => !ACTIVE_STATUSES.has(session?.status));
  }
  return (sessions || []).filter((session) => {
    if (filter === "finished") return session?.status === "finished";
    if (filter === "error") return session?.status === "error";
    if (filter === "cancelled") return session?.status === "cancelled";
    return true;
  });
};

const filterSessionsByFeedback = (sessions) => {
  const filter = normalizeMonitorFeedbackFilter(state.monitor.feedbackFilter);
  if (filter === "all") {
    return sessions;
  }
  return (sessions || []).filter((session) => {
    const feedbackStatus = resolveSessionFeedbackStatus(session);
    if (filter === "none") return feedbackStatus === "none";
    return feedbackStatus === filter;
  });
};

const buildSessionFeedbackSummary = (session) => {
  const counts = resolveSessionFeedbackCounts(session);
  if (counts.total <= 0) {
    return t("monitor.feedback.none");
  }
  return `${t("monitor.feedback.up")} ${counts.up} / ${t("monitor.feedback.down")} ${counts.down}`;
};

const syncMonitorSessionFilterInputs = () => {
  if (elements.monitorStatusFilter) {
    elements.monitorStatusFilter.value = normalizeMonitorSessionStatusFilter(
      state.monitor.sessionStatusFilter
    );
  }
  if (elements.monitorFeedbackFilter) {
    elements.monitorFeedbackFilter.value = normalizeMonitorFeedbackFilter(
      state.monitor.feedbackFilter
    );
  }
};

// 璇诲彇鍒嗛〉閰嶇疆锛岀‘淇濆垎椤靛昂瀵镐负姝ｆ暣鏁?
const resolveMonitorPageSize = () => {
  const rawValue = Math.floor(Number(state.monitor.pagination?.pageSize));
  if (!Number.isFinite(rawValue) || rawValue <= 0) {
    return DEFAULT_MONITOR_SESSION_PAGE_SIZE;
  }
  return rawValue;
};

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

const getMonitorPaginationElements = (type) => {
  if (type !== "active") return null;
  return {
    container: resolveMonitorPaginationElement(
      "monitorActivePagination",
      "monitorActivePagination"
    ),
    info: resolveMonitorPaginationElement("monitorActivePageInfo", "monitorActivePageInfo"),
    prev: resolveMonitorPaginationElement("monitorActivePrevBtn", "monitorActivePrevBtn"),
    next: resolveMonitorPaginationElement("monitorActiveNextBtn", "monitorActiveNextBtn"),
  };
};

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
  const { emptyText = t("common.noData"), skipSort = false } = options;
  if (!body || !emptyNode) {
    return;
  }
  body.textContent = "";
  if (!Array.isArray(sessions) || sessions.length === 0) {
    emptyNode.textContent = emptyText;
    emptyNode.style.display = "block";
    return;
  }
  emptyNode.style.display = "none";
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
    const feedbackCell = document.createElement("td");
    feedbackCell.textContent = buildSessionFeedbackSummary(session);
    const tokenCell = document.createElement("td");
    tokenCell.textContent = formatTokenCount(resolveSessionConsumedTokens(session));
    const elapsedCell = document.createElement("td");
    elapsedCell.textContent = formatDuration(session.elapsed_s);
    const stageCell = document.createElement("td");
    stageCell.textContent = session.stage || "-";
    const actionCell = document.createElement("td");
    if (ACTIVE_STATUSES.has(session.status)) {
      const btn = document.createElement("button");
      btn.className = "danger";
      btn.textContent = t("monitor.actions.cancel");
      btn.addEventListener("click", (event) => {
        event.stopPropagation();
        requestCancelSession(session.session_id);
      });
      actionCell.appendChild(btn);
    } else {
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
    row.appendChild(feedbackCell);
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
  if (!elements.monitorTableBody || !elements.monitorEmpty) {
    return;
  }
  syncMonitorSessionFilterInputs();
  const filteredByUser = filterSessionsByUser(sessions || []);
  const filteredByStatus = filterSessionsByStatus(filteredByUser);
  const filteredByFeedback = filterSessionsByFeedback(filteredByStatus);
  const activePage = resolveMonitorPageSlice(filteredByFeedback, "activePage");
  renderMonitorTable(elements.monitorTableBody, elements.monitorEmpty, activePage.sessions, {
    emptyText: t("monitor.empty.sessions"),
  });
  renderMonitorPagination("active", activePage);
};

const updateMonitorPage = (pageKey, delta) => {
  ensureMonitorState();
  const current = Number(state.monitor.pagination?.[pageKey]) || 1;
  const nextPage = Math.max(1, current + delta);
  if (state.monitor.pagination) {
    state.monitor.pagination[pageKey] = nextPage;
  }
  renderMonitorSessions(state.monitor.sessions);
};

// 缁戝畾鍒嗛〉鎸夐挳浜嬩欢锛岄伩鍏嶉噸澶嶆煡鎵?DOM
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
};

const bindMonitorSessionFilters = () => {
  if (elements.monitorStatusFilter) {
    elements.monitorStatusFilter.addEventListener("change", () => {
      ensureMonitorState();
      state.monitor.sessionStatusFilter = normalizeMonitorSessionStatusFilter(
        elements.monitorStatusFilter.value
      );
      if (state.monitor.pagination) {
        state.monitor.pagination.activePage = 1;
      }
      renderMonitorSessions(state.monitor.sessions);
    });
  }
  if (elements.monitorFeedbackFilter) {
    elements.monitorFeedbackFilter.addEventListener("change", () => {
      ensureMonitorState();
      state.monitor.feedbackFilter = normalizeMonitorFeedbackFilter(
        elements.monitorFeedbackFilter.value
      );
      if (state.monitor.pagination) {
        state.monitor.pagination.activePage = 1;
      }
      renderMonitorSessions(state.monitor.sessions);
    });
  }
};

const resolveStatusKey = (label) => getStatusLabelToKey()[label] || "";

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
    const tokenText = formatTokenCount(resolveSessionConsumedTokens(session));
    if (tokenText && tokenText !== "-") {
      detailParts.push(t("monitor.session.consumedTokens", { token: tokenText }));
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

// 瑙ｆ瀽宸ュ叿璋冪敤浼氳瘽鐨勬椂闂存埑锛屼紭鍏堜娇鐢ㄦ渶杩戣皟鐢ㄦ椂闂?
const resolveToolSessionTimestamp = (session) => {
  const raw = session?.last_time || session?.updated_time || session?.start_time;
  const parsed = new Date(raw).getTime();
  return Number.isFinite(parsed) ? parsed : 0;
};

// 娓叉煋宸ュ叿璋冪敤浼氳瘽鍒楄〃锛屼繚鎸佷笌绾跨▼鐘舵€佸脊绐椾竴鑷寸殑椋庢牸
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
      const tokenText = formatTokenCount(resolveSessionContextTokens(session));
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

// 鑾峰彇鎸囧畾宸ュ叿鐨勮皟鐢ㄤ細璇濆垪琛?
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

// 鎵撳紑绾跨▼鐘舵€佹槑缁嗗脊绐楋紝鏄剧ず瀵瑰簲鐘舵€佺殑浼氳瘽璁板綍
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

// 鍏抽棴绾跨▼鐘舵€佹槑缁嗗脊绐?
const closeMonitorStatusModal = () => {
  elements.monitorStatusModal?.classList.remove("active");
};

export const loadMonitorData = async (options = {}) => {
  ensureMonitorState();
  const mode = options?.mode === "sessions" ? "sessions" : "full";
  const wunderBase = getWunderBase();
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
    refreshUserDashboardSummary({ silent: true });
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

// 鍒囨崲鐢ㄦ埛绛涢€夋潯浠跺苟鍗虫椂鍒锋柊绾跨▼琛ㄦ牸
export const setMonitorUserFilter = (userId) => {
  ensureMonitorState();
  state.monitor.userFilter = String(userId || "").trim();
  if (state.monitor.pagination) {
    state.monitor.pagination.activePage = 1;
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
      state.runtime.monitorPollTimer = setInterval(() => {
        loadMonitorData({ mode }).catch(() => {});
      }, intervalMs);
    }
  } else if (state.runtime.monitorPollTimer) {
    clearInterval(state.runtime.monitorPollTimer);
    state.runtime.monitorPollTimer = null;
  }
};

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

const highlightMonitorTimestamps = (detailText) =>
  escapeMonitorHtml(detailText).replace(
    MONITOR_TIMESTAMP_RE,
    '<span class="log-timestamp">[$1]</span>'
  );

const normalizeMonitorToolName = (value) => String(value || "").trim().toLowerCase();

const fallbackMonitorEventDataText = (value) => {
  if (value === null || value === undefined) {
    return "";
  }
  if (typeof value === "string") {
    return value;
  }
  if (Array.isArray(value)) {
    return `[array(${value.length})]`;
  }
  if (typeof value === "object") {
    try {
      const keys = Object.keys(value);
      if (keys.length === 0) {
        return "{}";
      }
      const preview = {};
      keys.slice(0, 8).forEach((key) => {
        const field = value[key];
        if (field === null || field === undefined) {
          preview[key] = field;
          return;
        }
        if (typeof field === "string" || typeof field === "number" || typeof field === "boolean") {
          preview[key] = field;
          return;
        }
        if (typeof field === "bigint") {
          preview[key] = field.toString();
          return;
        }
        if (Array.isArray(field)) {
          preview[key] = `[array(${field.length})]`;
          return;
        }
        preview[key] = "[object]";
      });
      if (keys.length > 8) {
        preview.__extra_keys__ = keys.length - 8;
      }
      const text = JSON.stringify(preview);
      return typeof text === "string" ? text : "{...}";
    } catch (_error) {
      return "{...}";
    }
  }
  return String(value);
};

// Safely serialize event payloads to avoid "[object Object]" in log title/detail.
const safeStringifyMonitorEventData = (value, pretty = false) => {
  const seen = new WeakSet();
  try {
    const text = JSON.stringify(
      value,
      (_key, current) => {
        if (typeof current === "bigint") {
          return current.toString();
        }
        if (typeof current === "function") {
          return `[Function ${current.name || "anonymous"}]`;
        }
        if (typeof current === "symbol") {
          return String(current);
        }
        if (current instanceof Error) {
          return {
            name: current.name,
            message: current.message,
            stack: current.stack,
          };
        }
        if (current && typeof current === "object") {
          if (seen.has(current)) {
            return "[Circular]";
          }
          seen.add(current);
          if (current instanceof Map) {
            return Object.fromEntries(current);
          }
          if (current instanceof Set) {
            return Array.from(current);
          }
        }
        return current;
      },
      pretty ? 2 : undefined
    );
    if (typeof text === "string") {
      return text;
    }
  } catch (_error) {
    // Ignore and fallback to structured preview.
  }
  return fallbackMonitorEventDataText(value);
};

// 格式化事件数据为可展示文本，确保异常数据不会打断渲染
const stringifyMonitorEventData = (data) => {
  const resolved = unwrapMonitorEventData(data);
  if (typeof resolved === "string") {
    return resolved;
  }
  return safeStringifyMonitorEventData(resolved, false);
};

const MONITOR_EVENT_TITLE_MAX_LENGTH = 120;

const truncateMonitorEventTitle = (value) => {
  const text = String(value || "").replace(/\s+/g, " ").trim();
  if (!text) {
    return "";
  }
  if (text.length <= MONITOR_EVENT_TITLE_MAX_LENGTH) {
    return text;
  }
  return `${text.slice(0, MONITOR_EVENT_TITLE_MAX_LENGTH)}...`;
};

// Extract readable scalar text from nested summary/error objects.
const extractMonitorEventTitleText = (value, depth = 0) => {
  if (value === null || value === undefined || depth > 3) {
    return "";
  }
  if (
    typeof value === "string" ||
    typeof value === "number" ||
    typeof value === "boolean" ||
    typeof value === "bigint"
  ) {
    return String(value).trim();
  }
  if (Array.isArray(value)) {
    for (const item of value) {
      const text = extractMonitorEventTitleText(item, depth + 1);
      if (text) {
        return text;
      }
    }
    return "";
  }
  if (!value || typeof value !== "object") {
    return "";
  }
  const source = value;
  for (const key of [
    "summary",
    "message",
    "question",
    "reason",
    "error",
    "tool",
    "tool_name",
    "toolName",
    "name",
    "model",
    "model_name",
    "stage",
    "status",
    "code",
    "title",
  ]) {
    const text = extractMonitorEventTitleText(source[key], depth + 1);
    if (text) {
      return text;
    }
  }
  return "";
};

const formatMonitorEventTimestamp = (value) => {
  if (!value) {
    return "-";
  }
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) {
    return String(value);
  }
  return parsed.toLocaleTimeString(getCurrentLanguage());
};

const resolveMonitorEventTitle = (event) => {
  const eventType = String(event?.type || "").trim().toLowerCase();
  const data = unwrapMonitorEventData(event?.data);
  const repairSummary = resolveMonitorRepairSummary(resolveMonitorEventRepair(event));
  if (data && typeof data === "object") {
    const candidates =
      eventType === "user_input"
        ? [
            data.message,
            data.question,
            data.input,
            data.content,
            data.summary,
            data.error,
            data.reason,
            data.tool,
            data.tool_name,
            data.toolName,
            data.name,
            data.model,
            data.model_name,
            data.stage,
            data.status,
          ]
        : [
            data.summary,
            data.message,
            data.question,
            data.error,
            data.reason,
            data.tool,
            data.tool_name,
            data.toolName,
            data.name,
            data.model,
            data.model_name,
            data.stage,
            data.status,
          ];
    let summary = "";
    for (const candidate of candidates) {
      summary = extractMonitorEventTitleText(candidate);
      if (summary) {
        break;
      }
    }
    const title = truncateMonitorEventTitle(
      repairSummary && summary ? `${summary} · ${repairSummary}` : summary || repairSummary
    );
    if (title) {
      return title;
    }
  }
  if (typeof data === "string") {
    const title = truncateMonitorEventTitle(
      repairSummary ? `${data} · ${repairSummary}` : data
    );
    if (title) {
      return title;
    }
  }
  const raw = stringifyMonitorEventData(data);
  const title = truncateMonitorEventTitle(
    repairSummary && raw ? `${raw} · ${repairSummary}` : raw || repairSummary
  );
  return title || "-";
};

// 鎷兼帴鍗曟潯浜嬩欢鏂囨湰锛屼繚鎸佷笌鍘嗗彶灞曠ず涓€鑷?
const buildMonitorEventLine = (event) => {
  const timestamp = event?.timestamp || "";
  const eventType = event?.type || "unknown";
  const eventId = Number(event?.event_id);
  const prefix = Number.isFinite(eventId) && eventId > 0 ? "#" + eventId + " " : "";
  const dataText = stringifyMonitorEventData(event?.data);
  return "[" + timestamp + "] " + prefix + eventType + ": " + dataText;
};

const resolveMonitorEventToolName = (event) => {
  const data = unwrapMonitorEventData(event?.data);
  if (!data || typeof data !== "object") {
    return "";
  }
  const tool = data.tool ?? data.tool_name ?? data.toolName;
  return typeof tool === "string" ? tool.trim() : "";
};

const resolveMonitorEventRepair = (event) => {
  const data = unwrapMonitorEventData(event?.data);
  if (!data || typeof data !== "object") {
    return null;
  }
  const candidates = [data.repair, data.meta?.repair];
  for (const candidate of candidates) {
    if (candidate && typeof candidate === "object" && !Array.isArray(candidate)) {
      return candidate;
    }
  }
  return null;
};

const parseMonitorRepairCount = (value) => {
  const count = Number.parseInt(String(value ?? 0), 10);
  return Number.isFinite(count) && count > 0 ? count : 0;
};

const resolveMonitorRepairSummary = (repair) => {
  if (!repair || typeof repair !== "object") {
    return "";
  }
  const strategy = String(repair.strategy || "")
    .trim()
    .toLowerCase();
  const count = parseMonitorRepairCount(repair.count);
  switch (strategy) {
    case "sanitize_before_request":
      return count > 0
        ? resolveMonitorDetailText("monitor.detail.repair.historySummary", { count })
        : resolveMonitorDetailText("monitor.detail.repair.badge");
    case "lossy_json_string_repair":
    case "raw_arguments_wrapped":
    case "non_object_arguments_wrapped":
      return resolveMonitorDetailText("monitor.detail.repair.argsSummary");
    default:
      return resolveMonitorDetailText("monitor.detail.repair.badge");
  }
};

const resolveMonitorRepairNote = (repair) => {
  if (!repair || typeof repair !== "object") {
    return "";
  }
  const strategy = String(repair.strategy || "")
    .trim()
    .toLowerCase();
  const count = parseMonitorRepairCount(repair.count);
  switch (strategy) {
    case "sanitize_before_request":
      return count > 0
        ? resolveMonitorDetailText("monitor.detail.repair.sanitizeBeforeRequest", { count })
        : resolveMonitorDetailText("monitor.detail.repair.badge");
    case "lossy_json_string_repair":
      return resolveMonitorDetailText("monitor.detail.repair.lossyJson");
    case "raw_arguments_wrapped":
      return resolveMonitorDetailText("monitor.detail.repair.rawWrapped");
    case "non_object_arguments_wrapped":
      return resolveMonitorDetailText("monitor.detail.repair.nonObjectWrapped");
    default:
      return resolveMonitorDetailText("monitor.detail.repair.badge");
  }
};

const normalizeMonitorDetailEventType = (value) => String(value || "").trim();

const parseMonitorDetailRound = (value, fallback = 0) => {
  const parsed = Number.parseInt(String(value ?? fallback), 10);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    return Math.max(0, fallback);
  }
  return parsed;
};

const resolveMonitorEventRound = (event) => {
  const data = event?.data && typeof event.data === "object" ? event.data : {};
  return (
    parseMonitorDetailRound(event?.__userRound) ||
    parseMonitorDetailRound(event?.user_round) ||
    parseMonitorDetailRound(event?.round) ||
    parseMonitorDetailRound(data?.user_round) ||
    parseMonitorDetailRound(data?.round)
  );
};

const inferMonitorDetailRoundForEvent = (event, currentRound) => {
  const explicitRound = resolveMonitorEventRound(event);
  if (explicitRound > 0) {
    return explicitRound;
  }
  const eventType = String(event?.type || "")
    .trim()
    .toLowerCase();
  if (eventType === "round_start" || eventType === "user_input" || eventType === "received") {
    return Math.max(1, currentRound + 1);
  }
  if (currentRound > 0) {
    return currentRound;
  }
  return 1;
};

const normalizeMonitorDetailEvents = (events) => {
  const normalized = [];
  let inferredRound = 0;
  (Array.isArray(events) ? events : []).forEach((event) => {
    const userRound = inferMonitorDetailRoundForEvent(event, inferredRound);
    inferredRound = Math.max(inferredRound, userRound);
    normalized.push({
      ...(event && typeof event === "object" ? event : {}),
      __userRound: userRound,
    });
  });
  return normalized;
};

const resolveMonitorDetailQuestionTextFromPayload = (payload) => {
  const data = unwrapMonitorEventData(payload);
  if (typeof data === "string") {
    return data.trim();
  }
  if (!data || typeof data !== "object" || Array.isArray(data)) {
    return "";
  }
  const candidate =
    data.message || data.question || data.input || data.content || data.prompt || data.text;
  return String(candidate || "").trim();
};

const collectMonitorDetailRoundOptions = (session, events) => {
  const rounds = new Set();
  (Array.isArray(events) ? events : []).forEach((event) => {
    const round = resolveMonitorEventRound(event);
    if (round > 0) {
      rounds.add(round);
    }
  });
  const declaredRoundCount = parseMonitorDetailRound(session?.user_rounds || session?.rounds);
  if (rounds.size === 0 && declaredRoundCount > 0) {
    for (let round = 1; round <= declaredRoundCount; round += 1) {
      rounds.add(round);
    }
  }
  if (rounds.size === 0 && String(session?.question || "").trim()) {
    rounds.add(1);
  }
  return Array.from(rounds).sort((left, right) => left - right);
};

const buildMonitorDetailRoundQuestionMap = (events, roundOptions, sessionQuestion) => {
  const questionByRound = new Map();
  (Array.isArray(events) ? events : []).forEach((event) => {
    const round = resolveMonitorEventRound(event);
    if (round <= 0) {
      return;
    }
    const question = resolveMonitorDetailQuestionTextFromPayload(event?.data);
    if (!question) {
      return;
    }
    const eventType = String(event?.type || "")
      .trim()
      .toLowerCase();
    if (eventType === "user_input" || eventType === "received") {
      if (!questionByRound.has(round)) {
        questionByRound.set(round, question);
      }
      return;
    }
    if (!questionByRound.has(round)) {
      questionByRound.set(round, question);
    }
  });
  const latestRound = Array.isArray(roundOptions) && roundOptions.length
    ? roundOptions[roundOptions.length - 1]
    : 0;
  const cleanedSessionQuestion = String(sessionQuestion || "").trim();
  if (latestRound > 0 && cleanedSessionQuestion && !questionByRound.has(latestRound)) {
    questionByRound.set(latestRound, cleanedSessionQuestion);
  }
  return questionByRound;
};

const resolveMonitorDetailQuestionByRound = () => {
  const detail = state.monitor?.detail;
  if (!detail) {
    return "";
  }
  const selectedRound = parseMonitorDetailRound(state.monitor?.detailFilters?.round);
  if (selectedRound > 0 && detail.roundQuestions instanceof Map) {
    const question = String(detail.roundQuestions.get(selectedRound) || "").trim();
    if (question) {
      return question;
    }
  }
  return String(detail.session?.question || "").trim();
};

const renderMonitorDetailQuestion = () => {
  if (!elements.monitorDetailQuestion) {
    return;
  }
  elements.monitorDetailQuestion.textContent = resolveMonitorDetailQuestionByRound();
};

const normalizeMonitorDetailFeedbackList = (items) =>
  (Array.isArray(items) ? items : [])
    .map((item) => {
      if (!item || typeof item !== "object") {
        return null;
      }
      const vote = String(item.vote || "")
        .trim()
        .toLowerCase();
      if (vote !== "up" && vote !== "down") {
        return null;
      }
      const historyId = Number.parseInt(
        String(item.history_id ?? item.historyId ?? ""),
        10
      );
      const userId = String(item.user_id ?? item.userId ?? "").trim();
      const createdAt = String(item.created_at ?? item.createdAt ?? "").trim();
      return {
        vote,
        historyId: Number.isFinite(historyId) && historyId > 0 ? historyId : 0,
        userId,
        createdAt,
      };
    })
    .filter(Boolean);

const renderMonitorDetailFeedback = () => {
  if (!elements.monitorDetailFeedback || !elements.monitorDetailFeedbackSummary) {
    return;
  }
  const feedback = Array.isArray(state.monitor?.detail?.feedback)
    ? state.monitor.detail.feedback
    : [];
  const up = feedback.filter((item) => item?.vote === "up").length;
  const down = feedback.filter((item) => item?.vote === "down").length;
  const total = up + down;
  if (total <= 0) {
    elements.monitorDetailFeedbackSummary.textContent = t("monitor.detail.feedback.empty");
    elements.monitorDetailFeedback.textContent = t("monitor.detail.feedback.empty");
    return;
  }
  elements.monitorDetailFeedbackSummary.textContent = t(
    "monitor.detail.feedback.summary",
    { total, up, down }
  );
  elements.monitorDetailFeedback.textContent = "";
  feedback.forEach((item) => {
    const row = document.createElement("div");
    row.className = `monitor-detail-feedback-item monitor-detail-feedback-item--${item.vote}`;
    const parts = [
      item.vote === "up"
        ? t("monitor.detail.feedback.vote.up")
        : t("monitor.detail.feedback.vote.down"),
    ];
    if (item.historyId > 0) {
      parts.push(
        t("monitor.detail.feedback.history", { historyId: item.historyId })
      );
    }
    if (item.userId) {
      parts.push(item.userId);
    }
    if (item.createdAt) {
      const timeText = formatTimestamp(item.createdAt);
      if (timeText && timeText !== "-") {
        parts.push(timeText);
      }
    }
    row.textContent = parts.join(" · ");
    elements.monitorDetailFeedback.appendChild(row);
  });
};

const syncMonitorDetailRoundFilter = () => {
  if (!elements.monitorDetailRoundFilter) {
    return;
  }
  const detail = state.monitor?.detail;
  const roundOptions = Array.isArray(detail?.roundOptions) ? detail.roundOptions : [];
  const filterNode = elements.monitorDetailRoundFilter;
  filterNode.textContent = "";
  if (roundOptions.length === 0) {
    const option = document.createElement("option");
    option.value = "0";
    option.textContent = resolveMonitorDetailText("monitor.detail.round.none");
    filterNode.appendChild(option);
    filterNode.disabled = true;
    filterNode.value = "0";
    state.monitor.detailFilters.round = 0;
    return;
  }
  roundOptions.forEach((round) => {
    const option = document.createElement("option");
    option.value = String(round);
    option.textContent = resolveMonitorDetailText("monitor.detail.round", { round });
    filterNode.appendChild(option);
  });
  filterNode.disabled = false;
  const selectedRound = parseMonitorDetailRound(state.monitor?.detailFilters?.round);
  const finalRound = roundOptions.includes(selectedRound)
    ? selectedRound
    : roundOptions[roundOptions.length - 1];
  state.monitor.detailFilters.round = finalRound;
  filterNode.value = String(finalRound);
};

const collectMonitorDetailEventTypes = (events) => {
  const types = new Set();
  (Array.isArray(events) ? events : []).forEach((event) => {
    const eventType = normalizeMonitorDetailEventType(event?.type || "");
    if (eventType) {
      types.add(eventType);
    }
  });
  return Array.from(types).sort((left, right) => left.localeCompare(right));
};

const resetMonitorDetailFilters = () => {
  ensureMonitorState();
  state.monitor.detailFilters.eventType = "";
  state.monitor.detailFilters.keyword = "";
  state.monitor.detailFilters.round = 0;
  if (elements.monitorDetailTypeFilter) {
    elements.monitorDetailTypeFilter.value = "";
  }
  if (elements.monitorDetailKeyword) {
    elements.monitorDetailKeyword.value = "";
  }
  if (elements.monitorDetailRoundFilter) {
    elements.monitorDetailRoundFilter.textContent = "";
    const option = document.createElement("option");
    option.value = "0";
    option.textContent = resolveMonitorDetailText("monitor.detail.round.none");
    elements.monitorDetailRoundFilter.appendChild(option);
    elements.monitorDetailRoundFilter.value = "0";
    elements.monitorDetailRoundFilter.disabled = true;
  }
  if (elements.monitorDetailFilterStats) {
    elements.monitorDetailFilterStats.textContent = "";
  }
};

const syncMonitorDetailFilterControls = (events) => {
  if (!elements.monitorDetailTypeFilter) {
    return;
  }
  const selectedType = normalizeMonitorDetailEventType(state.monitor?.detailFilters?.eventType);
  const eventTypes = collectMonitorDetailEventTypes(events);
  const availableTypes = new Set(eventTypes);
  state.monitor.detailFilters.eventType = availableTypes.has(selectedType) ? selectedType : "";
  const filterNode = elements.monitorDetailTypeFilter;
  filterNode.textContent = "";
  const allOption = document.createElement("option");
  allOption.value = "";
  allOption.textContent = resolveMonitorDetailText("monitor.detail.filter.allTypes");
  filterNode.appendChild(allOption);
  eventTypes.forEach((eventType) => {
    const option = document.createElement("option");
    option.value = eventType;
    option.textContent = eventType;
    filterNode.appendChild(option);
  });
  filterNode.value = state.monitor.detailFilters.eventType;
  if (elements.monitorDetailKeyword) {
    elements.monitorDetailKeyword.setAttribute(
      "placeholder",
      resolveMonitorDetailText("monitor.detail.filter.keywordPlaceholder")
    );
    if (document.activeElement !== elements.monitorDetailKeyword) {
      elements.monitorDetailKeyword.value = state.monitor.detailFilters.keyword || "";
    }
  }
};

const resolveMonitorDetailFilteredEvents = (events) => {
  const selectedType = normalizeMonitorDetailEventType(state.monitor?.detailFilters?.eventType);
  const keyword = String(state.monitor?.detailFilters?.keyword || "")
    .trim()
    .toLowerCase();
  return (Array.isArray(events) ? events : []).filter((event) => {
    const eventType = normalizeMonitorDetailEventType(event?.type || "");
    if (selectedType && eventType !== selectedType) {
      return false;
    }
    if (!keyword) {
      return true;
    }
    const haystack = (eventType + " " + stringifyMonitorEventData(event?.data)).toLowerCase();
    return haystack.includes(keyword);
  });
};

const renderMonitorDetailFilterStats = (visibleCount, totalCount) => {
  if (!elements.monitorDetailFilterStats) {
    return;
  }
  elements.monitorDetailFilterStats.textContent = resolveMonitorDetailText("monitor.detail.filter.stats", {
    visible: visibleCount,
    total: totalCount,
  });
};

const renderMonitorDetailWithFilters = (events, options = {}) => {
  syncMonitorDetailFilterControls(events);
  const filtered = resolveMonitorDetailFilteredEvents(events);
  renderMonitorDetailFilterStats(filtered.length, Array.isArray(events) ? events.length : 0);
  const focusTool = typeof options?.focusTool === "string" ? options.focusTool.trim() : "";
  const selectedRound = parseMonitorDetailRound(
    options?.focusRound ?? state.monitor?.detailFilters?.round
  );
  const focusLine = renderMonitorDetailEvents(filtered, { focusTool, selectedRound });
  if (focusTool) {
    scrollMonitorDetailToLine(focusLine);
    return focusLine;
  }
  if (selectedRound > 0) {
    scrollMonitorDetailToRound(selectedRound);
  }
  return focusLine;
};

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
  const selectedRound = parseMonitorDetailRound(options.selectedRound);
  const fragment = document.createDocumentFragment();
  let focusNode = null;
  let fallbackNode = null;
  events.forEach((event) => {
    const lineText = buildMonitorEventLine(event);
    const normalizedLineText = normalizeMonitorToolName(lineText);
    const eventType = String(event?.type || "unknown");
    const eventTypeLower = eventType.toLowerCase();
    const round = resolveMonitorEventRound(event);
    const repair = resolveMonitorEventRepair(event);
    const repairBadgeText = repair
      ? resolveMonitorDetailText("monitor.detail.repair.badge")
      : "";
    const repairNote = resolveMonitorRepairNote(repair);
    const item = document.createElement("details");
    item.className = "log-item monitor-event-item";
    if (repair) {
      item.classList.add("monitor-event-item--repaired");
    }
    if (round > 0) {
      item.dataset.round = String(round);
      if (selectedRound > 0 && round === selectedRound) {
        item.classList.add("monitor-event-item--round");
      }
    }
    const summary = document.createElement("summary");
    summary.className = "log-summary";
    const timeNode = document.createElement("span");
    timeNode.className = "log-time";
    timeNode.textContent = `[${formatMonitorEventTimestamp(event?.timestamp)}]`;
    summary.appendChild(timeNode);
    const eventNode = document.createElement("span");
    eventNode.className = "log-event";
    const eventId = Number(event?.event_id);
    eventNode.textContent =
      Number.isFinite(eventId) && eventId > 0 ? "#" + eventId + " " + eventType : eventType;
    summary.appendChild(eventNode);
    const titleNode = document.createElement("span");
    titleNode.className = "log-title";
    titleNode.textContent = resolveMonitorEventTitle(event);
    summary.appendChild(titleNode);
    if (repairBadgeText) {
      const badgeNode = document.createElement("span");
      badgeNode.className = "monitor-event-badge monitor-event-badge--repair";
      badgeNode.textContent = repairBadgeText;
      summary.appendChild(badgeNode);
    }
    if (round > 0) {
      const roundNode = document.createElement("span");
      roundNode.className = "monitor-event-round";
      roundNode.textContent = resolveMonitorDetailText("monitor.detail.round", { round });
      summary.appendChild(roundNode);
    }
    item.appendChild(summary);
    if (repairNote) {
      const noteNode = document.createElement("div");
      noteNode.className = "monitor-event-note monitor-event-note--repair";
      noteNode.textContent = repairNote;
      item.appendChild(noteNode);
    }
    const detailNode = document.createElement("div");
    detailNode.className = "log-detail";
    detailNode.innerHTML = highlightMonitorTimestamps(lineText);
    item.appendChild(detailNode);
    const eventTool = resolveMonitorEventToolName(event);
    const matchesToolName =
      focusToolName &&
      (normalizeMonitorToolName(eventTool) === focusToolName ||
        normalizedLineText.includes(focusToolName));
    if (matchesToolName) {
      item.classList.add("monitor-event-item--tool");
      if (!focusNode && eventTypeLower === "tool_call") {
        focusNode = item;
      } else if (!fallbackNode && eventTypeLower === "tool_result") {
        fallbackNode = item;
      }
    }
    fragment.appendChild(item);
  });
  container.appendChild(fragment);
  if (!focusNode && fallbackNode) {
    focusNode = fallbackNode;
  }
  if (focusNode) {
    focusNode.classList.add("monitor-event-item--focus");
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

const scrollMonitorDetailToRound = (round) => {
  if (!elements.monitorDetailEvents || round <= 0) {
    return;
  }
  const selector = `.monitor-event-item[data-round="${round}"]`;
  const target = elements.monitorDetailEvents.querySelector(selector);
  if (!target) {
    return;
  }
  elements.monitorDetailEvents
    .querySelectorAll(".monitor-event-item--round-focus")
    .forEach((node) => node.classList.remove("monitor-event-item--round-focus"));
  target.classList.add("monitor-event-item--round-focus");
  scrollMonitorDetailToLine(target);
  window.setTimeout(() => {
    target.classList.remove("monitor-event-item--round-focus");
  }, 1400);
};

const setMonitorDetailExportEnabled = (enabled) => {
  if (!elements.monitorDetailExport) {
    return;
  }
  elements.monitorDetailExport.disabled = !enabled;
};

const sanitizeFilenamePart = (value, fallback) => {
  const text = String(value || "").trim();
  const safe = text.replace(/[\\/:*?"<>|]+/g, "_");
  if (safe) {
    return safe;
  }
  return fallback || "session";
};

const buildMonitorDetailExportFilename = (sessionId) => {
  const safeSessionId = sanitizeFilenamePart(sessionId, "session");
  const timestamp = new Date().toISOString().replace(/[:.]/g, "-");
  return `monitor-detail-${safeSessionId}-${timestamp}.jsonl`;
};

const downloadBlob = (blob, filename) => {
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
};

const MONITOR_EXPORT_MAX_STRING_CHARS = 1200;
const MONITOR_EXPORT_MAX_ARRAY_ITEMS = 24;
const MONITOR_EXPORT_MAX_OBJECT_KEYS = 24;
const MONITOR_EXPORT_MAX_DEPTH = 6;

const compactMonitorExportText = (value) => {
  const text = String(value || "");
  if (text.length <= MONITOR_EXPORT_MAX_STRING_CHARS) {
    return text;
  }
  return `${text.slice(0, MONITOR_EXPORT_MAX_STRING_CHARS)}...(truncated)`;
};

const compactMonitorExportValue = (value, depth = 0) => {
  if (typeof value === "string") {
    return compactMonitorExportText(value);
  }
  if (value === null || value === undefined) {
    return value ?? null;
  }
  if (typeof value !== "object") {
    return value;
  }
  if (depth >= MONITOR_EXPORT_MAX_DEPTH) {
    return "[truncated depth]";
  }
  if (Array.isArray(value)) {
    if (value.length <= MONITOR_EXPORT_MAX_ARRAY_ITEMS) {
      return value.map((item) => compactMonitorExportValue(item, depth + 1));
    }
    const headCount = Math.max(1, Math.floor(MONITOR_EXPORT_MAX_ARRAY_ITEMS * 0.75));
    const tailCount = Math.max(1, MONITOR_EXPORT_MAX_ARRAY_ITEMS - headCount);
    const omitted = value.length - headCount - tailCount;
    const head = value.slice(0, headCount).map((item) => compactMonitorExportValue(item, depth + 1));
    const tail = value.slice(-tailCount).map((item) => compactMonitorExportValue(item, depth + 1));
    return [...head, { __truncated: true, omitted_items: Math.max(0, omitted) }, ...tail];
  }
  const source = value || {};
  const keys = Object.keys(source);
  const output = {};
  keys.slice(0, MONITOR_EXPORT_MAX_OBJECT_KEYS).forEach((key) => {
    output[key] = compactMonitorExportValue(source[key], depth + 1);
  });
  if (keys.length > MONITOR_EXPORT_MAX_OBJECT_KEYS) {
    output.__truncated = true;
    output.__omitted_keys = keys.length - MONITOR_EXPORT_MAX_OBJECT_KEYS;
  }
  return output;
};

const normalizeMonitorExportTimestamp = (value) => {
  const text = String(value || "").trim();
  if (text) {
    const parsed = new Date(text);
    if (!Number.isNaN(parsed.getTime())) {
      return parsed.toISOString();
    }
  }
  const parsed = parseMonitorTimestamp(value);
  if (Number.isFinite(parsed) && parsed > 0) {
    return new Date(parsed).toISOString();
  }
  return "";
};

const buildMonitorDetailExportLines = () => {
  const detail = state.monitor?.detail;
  if (!detail) {
    return null;
  }
  const session =
    detail.session && typeof detail.session === "object" && !Array.isArray(detail.session)
      ? detail.session
      : {};
  const events = Array.isArray(detail.events) ? detail.events : [];
  const feedback = Array.isArray(detail.feedback) ? detail.feedback : [];
  const eventTypes = Array.from(
    new Set(
      events.map((event) => {
        const eventType = String(event?.type || event?.event || "unknown").trim();
        return eventType || "unknown";
      })
    )
  ).sort((left, right) => left.localeCompare(right));
  const lines = [
    {
      record_type: "meta",
      export_schema_version: 2,
      export_format: "jsonl",
      exported_at: new Date().toISOString(),
      summary: {
        event_count: events.length,
        feedback_count: feedback.length,
        event_types: eventTypes,
      },
      session: compactMonitorExportValue(session),
      compact_policy: {
        max_string_chars: MONITOR_EXPORT_MAX_STRING_CHARS,
        max_array_items: MONITOR_EXPORT_MAX_ARRAY_ITEMS,
        max_object_keys: MONITOR_EXPORT_MAX_OBJECT_KEYS,
        max_depth: MONITOR_EXPORT_MAX_DEPTH,
      },
    },
  ];
  events.forEach((event, index) => {
    const eventType = String(event?.type || event?.event || "unknown").trim() || "unknown";
    lines.push({
      record_type: "event",
      order: index + 1,
      event_id: Number.isFinite(Number(event?.event_id)) ? Number(event.event_id) : null,
      round: resolveMonitorEventRound(event),
      event: eventType,
      timestamp: normalizeMonitorExportTimestamp(event?.timestamp),
      title: resolveMonitorEventTitle(event),
      data: compactMonitorExportValue(unwrapMonitorEventData(event?.data)),
    });
  });
  feedback.forEach((item, index) => {
    lines.push({
      record_type: "feedback",
      order: index + 1,
      data: compactMonitorExportValue(item),
    });
  });
  return lines;
};

const buildMonitorDetailExportPayload = () => {
  const lines = buildMonitorDetailExportLines();
  if (!lines || !lines.length) {
    return null;
  }
  return {
    lines,
    session_id: state.monitor?.detail?.session?.session_id || "",
  };
};

const exportMonitorDetailLogs = () => {
  try {
    const payload = buildMonitorDetailExportPayload();
    if (!payload) {
      notify(t("monitor.detail.exportEmpty"), "warning");
      return;
    }
    const jsonlBody = payload.lines.map((item) => JSON.stringify(item)).join("\n");
    // Prefix UTF-8 BOM for better compatibility with Windows editors/shell defaults.
    const jsonl = `\uFEFF${jsonlBody}\n`;
    const blob = new Blob([jsonl], { type: "application/x-ndjson;charset=utf-8" });
    const filename = buildMonitorDetailExportFilename(payload.session_id);
    downloadBlob(blob, filename);
    notify(t("monitor.detail.exported"), "success");
  } catch (error) {
    const message = error?.message || String(error);
    notify(t("monitor.detail.exportFailed", { message }), "error");
  }
};

export const openMonitorDetail = async (sessionId, options = {}) => {
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/monitor/${encodeURIComponent(sessionId)}`;
  setMonitorDetailExportEnabled(false);
  state.monitor.detail = null;
  try {
    const response = await fetch(endpoint);
    if (response.status === 404) {
      const deletedMessage = t("monitor.detailLoadFailed", { message: t("monitor.deleted") });
      appendLog(deletedMessage);
      notify(deletedMessage, "warning");
      return;
    }
    if (!response.ok) {
      throw new Error(t("common.requestFailed", { status: response.status }));
    }
    const result = await response.json();
    const session = result.session || {};
    if (!session.session_id) {
      const deletedMessage = t("monitor.detailLoadFailed", { message: t("monitor.deleted") });
      appendLog(deletedMessage);
      notify(deletedMessage, "warning");
      return;
    }
    state.monitor.selected = session.session_id;
    elements.monitorDetailTitle.textContent = t("monitor.detail.title", {
      sessionId: session.session_id || "-",
    });
    const events = normalizeMonitorDetailEvents(result.events);
    const roundOptions = collectMonitorDetailRoundOptions(session, events);
    const roundQuestions = buildMonitorDetailRoundQuestionMap(
      events,
      roundOptions,
      session.question
    );
    const feedback = normalizeMonitorDetailFeedbackList(result.feedback);
    elements.monitorDetailMeta.textContent = buildMonitorDetailMeta(session, events);
    state.monitor.detail = {
      session,
      events,
      roundOptions,
      roundQuestions,
      feedback,
    };
    resetMonitorDetailFilters();
    syncMonitorDetailRoundFilter();
    renderMonitorDetailQuestion();
    setMonitorDetailExportEnabled(true);
    const focusTool =
      typeof options?.focusTool === "string" ? options.focusTool.trim() : "";
    renderMonitorDetailWithFilters(events, {
      focusTool,
      focusRound: state.monitor.detailFilters.round,
    });
    elements.monitorDetailModal.classList.add("active");
  } catch (error) {
    const message = t("monitor.detailLoadFailed", { message: error.message });
    appendLog(message);
    notify(message, "error");
  }
};

const closeMonitorDetail = () => {
  elements.monitorDetailModal.classList.remove("active");
  state.monitor.detail = null;
  if (elements.monitorDetailFeedbackSummary) {
    elements.monitorDetailFeedbackSummary.textContent = "-";
  }
  if (elements.monitorDetailFeedback) {
    elements.monitorDetailFeedback.textContent = "";
  }
  resetMonitorDetailFilters();
  setMonitorDetailExportEnabled(false);
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

export const initMonitorPanel = () => {
  ensureMonitorState();
  ensureMonitorCharts();
  bindMonitorPagination();
  bindMonitorSessionFilters();
  syncMonitorSessionFilterInputs();
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
  if (elements.monitorDetailExport) {
    elements.monitorDetailExport.addEventListener("click", exportMonitorDetailLogs);
    setMonitorDetailExportEnabled(false);
  }
  if (elements.monitorDetailTypeFilter) {
    elements.monitorDetailTypeFilter.addEventListener("change", () => {
      if (!state.monitor?.detail) {
        return;
      }
      state.monitor.detailFilters.eventType = normalizeMonitorDetailEventType(
        elements.monitorDetailTypeFilter.value
      );
      renderMonitorDetailWithFilters(state.monitor.detail.events || []);
    });
  }
  if (elements.monitorDetailKeyword) {
    elements.monitorDetailKeyword.addEventListener("input", () => {
      if (!state.monitor?.detail) {
        return;
      }
      state.monitor.detailFilters.keyword = String(elements.monitorDetailKeyword.value || "");
      renderMonitorDetailWithFilters(state.monitor.detail.events || []);
    });
  }
  if (elements.monitorDetailRoundFilter) {
    elements.monitorDetailRoundFilter.addEventListener("change", () => {
      if (!state.monitor?.detail) {
        return;
      }
      state.monitor.detailFilters.round = parseMonitorDetailRound(
        elements.monitorDetailRoundFilter.value
      );
      renderMonitorDetailQuestion();
      renderMonitorDetailWithFilters(state.monitor.detail.events || [], {
        focusRound: state.monitor.detailFilters.round,
      });
    });
  }
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


