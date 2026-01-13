import { elements } from "./elements.js?v=20260113-01";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { notify } from "./notify.js";
import { t } from "./i18n.js?v=20260113-01";
import { formatTimestamp } from "./utils.js?v=20251229-02";

const PERFORMANCE_STATE_KEY = "wunder_performance_state";
const DEFAULT_CONFIG = {
  maxConcurrency: 30,
  step: 1,
};
const HISTORY_LIMIT = 30;
const METRICS = [
  {
    key: "prompt_build",
    labelKey: "performance.metric.promptBuild",
    color: "#3b82f6",
  },
  {
    key: "file_ops",
    labelKey: "performance.metric.fileOps",
    color: "#22c55e",
  },
  {
    key: "command_exec",
    labelKey: "performance.metric.commandExec",
    color: "#f97316",
  },
  {
    key: "log_write",
    labelKey: "performance.metric.logWrite",
    color: "#a855f7",
  },
];

let initialized = false;
let chart = null;
let activeController = null;

const ensureState = () => {
  if (!state.performance) {
    state.performance = { running: false, samples: [], history: [] };
  }
  if (!Array.isArray(state.performance.samples)) {
    state.performance.samples = [];
  }
  if (!Array.isArray(state.performance.history)) {
    state.performance.history = [];
  }
};

const readStoredConfig = () => {
  try {
    const raw = localStorage.getItem(PERFORMANCE_STATE_KEY);
    if (!raw) {
      return {};
    }
    const parsed = JSON.parse(raw);
    return parsed && typeof parsed === "object" ? parsed : {};
  } catch (error) {
    return {};
  }
};

const writeStoredConfig = (patch) => {
  const next = { ...readStoredConfig(), ...(patch || {}) };
  try {
    localStorage.setItem(PERFORMANCE_STATE_KEY, JSON.stringify(next));
  } catch (error) {
    // ignore storage failure
  }
  return next;
};

const applyStoredConfig = () => {
  const stored = { ...DEFAULT_CONFIG, ...readStoredConfig() };
  if (elements.performanceMaxConcurrency && !elements.performanceMaxConcurrency.value) {
    elements.performanceMaxConcurrency.value = String(stored.maxConcurrency);
  }
  if (elements.performanceStep && !elements.performanceStep.value) {
    elements.performanceStep.value = String(stored.step);
  }
};

const scheduleConfigSave = () => {
  if (!elements.performanceFormStatus) {
    return;
  }
  if (elements.performanceFormStatus.dataset.syncing === "true") {
    return;
  }
  elements.performanceFormStatus.dataset.syncing = "true";
  setTimeout(() => {
    elements.performanceFormStatus.dataset.syncing = "false";
    persistConfig();
  }, 200);
};

const persistConfig = () => {
  writeStoredConfig({
    maxConcurrency: readPositiveInt(elements.performanceMaxConcurrency, DEFAULT_CONFIG.maxConcurrency),
    step: readPositiveInt(elements.performanceStep, DEFAULT_CONFIG.step),
  });
};

const readPositiveInt = (element, fallback) => {
  if (!element) {
    return fallback;
  }
  const parsed = Number(element.value);
  if (!Number.isFinite(parsed)) {
    return fallback;
  }
  const value = Math.floor(parsed);
  return value > 0 ? value : fallback;
};

const buildHistoryId = () =>
  `perf_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 8)}`;

const cloneMetricMap = (metrics) => {
  if (!metrics || typeof metrics !== "object") {
    return {};
  }
  const cloned = {};
  Object.entries(metrics).forEach(([key, value]) => {
    if (value && typeof value === "object") {
      cloned[key] = { ...value };
    } else {
      cloned[key] = value;
    }
  });
  return cloned;
};

const cloneSamples = (samples) =>
  samples.map((sample) => ({
    concurrency: Number(sample?.concurrency) || 0,
    metrics: cloneMetricMap(sample?.metrics),
  }));

const normalizeHistorySample = (sample) => {
  if (!sample || typeof sample !== "object") {
    return null;
  }
  const concurrency = Number(sample.concurrency) || 0;
  if (concurrency <= 0) {
    return null;
  }
  if (Array.isArray(sample.metrics)) {
    return normalizeSample(sample);
  }
  const metrics =
    sample.metrics && typeof sample.metrics === "object" ? sample.metrics : {};
  return { concurrency, metrics };
};

const inferMaxConcurrency = (samples) =>
  samples.reduce((max, sample) => Math.max(max, sample.concurrency || 0), 0);

const inferStep = (samples) => {
  const concurrencies = samples
    .map((sample) => Number(sample.concurrency))
    .filter((value) => Number.isFinite(value) && value > 0)
    .sort((left, right) => left - right);
  if (concurrencies.length < 2) {
    return 1;
  }
  let step = null;
  for (let i = 1; i < concurrencies.length; i += 1) {
    const diff = concurrencies[i] - concurrencies[i - 1];
    if (diff > 0) {
      step = step === null ? diff : Math.min(step, diff);
    }
  }
  return step || 1;
};

const normalizeHistoryEntry = (entry) => {
  if (!entry || typeof entry !== "object") {
    return null;
  }
  const samplesRaw = Array.isArray(entry.samples) ? entry.samples : [];
  const samples = samplesRaw.map(normalizeHistorySample).filter(Boolean);
  if (!samples.length) {
    return null;
  }
  const maxConcurrency = Number(entry.max_concurrency ?? entry.maxConcurrency);
  const step = Number(entry.step);
  const createdAt =
    entry.created_at || entry.createdAt || entry.started_at || entry.timestamp || "";
  return {
    id: entry.id || entry.run_id || buildHistoryId(),
    created_at: createdAt || new Date().toISOString(),
    max_concurrency: maxConcurrency > 0 ? maxConcurrency : inferMaxConcurrency(samples),
    step: step > 0 ? step : inferStep(samples),
    samples,
  };
};

const readStoredHistory = () => {
  const stored = readStoredConfig();
  const history = Array.isArray(stored.history) ? stored.history : [];
  return history.map(normalizeHistoryEntry).filter(Boolean);
};

const syncHistoryState = (history) => {
  ensureState();
  state.performance.history = history;
  return history;
};

const saveHistory = (history) => {
  const trimmed = Array.isArray(history) ? history.slice(-HISTORY_LIMIT) : [];
  writeStoredConfig({ history: trimmed });
  return syncHistoryState(trimmed);
};

const appendHistoryEntry = (entry) => {
  if (!entry) {
    return;
  }
  ensureState();
  const history = Array.isArray(state.performance.history)
    ? [...state.performance.history]
    : [];
  history.push(entry);
  saveHistory(history);
};

const buildHistoryEntry = (samples, config, startedAt) => ({
  id: buildHistoryId(),
  created_at: startedAt || new Date().toISOString(),
  max_concurrency: Number(config?.maxConcurrency) || inferMaxConcurrency(samples),
  step: Number(config?.step) || inferStep(samples),
  samples: cloneSamples(samples),
});

const loadHistory = () => {
  const history = readStoredHistory();
  syncHistoryState(history);
  renderHistoryList();
  return history;
};

const setFormStatus = (text) => {
  if (!elements.performanceFormStatus) {
    return;
  }
  elements.performanceFormStatus.textContent = text || "";
};

const updateStartButton = (running) => {
  if (!elements.performanceStartBtn) {
    return;
  }
  const label = elements.performanceStartBtn.querySelector("[data-role='label']");
  const icon = elements.performanceStartBtn.querySelector("[data-role='icon']");
  elements.performanceStartBtn.disabled = running;
  if (label) {
    const key = running ? "performance.action.running" : "performance.action.start";
    label.setAttribute("data-i18n", key);
    label.textContent = t(key);
  }
  if (icon) {
    icon.className = `fa-solid ${running ? "fa-spinner fa-spin" : "fa-play"}`;
  }
};

const setRunning = (running) => {
  ensureState();
  state.performance.running = running;
  updateStartButton(running);
  if (elements.performanceMaxConcurrency) {
    elements.performanceMaxConcurrency.disabled = running;
  }
  if (elements.performanceStep) {
    elements.performanceStep.disabled = running;
  }
  renderHistoryList();
};

const resetResults = () => {
  ensureState();
  state.performance.samples = [];
  renderAll();
};

const ensureChart = () => {
  if (!elements.performanceChart || !window.echarts) {
    return null;
  }
  if (!chart) {
    chart = window.echarts.init(elements.performanceChart);
  }
  return chart;
};

const buildSequence = (maxConcurrency, step) => {
  if (maxConcurrency <= 0 || step <= 0) {
    return [];
  }
  const sequence = [];
  let current = 1;
  while (current < maxConcurrency) {
    sequence.push(current);
    current += step;
  }
  if (!sequence.length || sequence[sequence.length - 1] !== maxConcurrency) {
    sequence.push(maxConcurrency);
  }
  return sequence;
};

const parseErrorMessage = async (response) => {
  try {
    const payload = await response.json();
    return payload?.detail?.message || payload?.message || "";
  } catch (error) {
    try {
      return await response.text();
    } catch (innerError) {
      return "";
    }
  }
};

const fetchSample = async (concurrency, signal) => {
  const wunderBase = getWunderBase();
  if (!wunderBase) {
    throw new Error(t("performance.error.apiBase"));
  }
  const response = await fetch(`${wunderBase}/admin/performance/sample`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ concurrency }),
    signal,
  });
  if (!response.ok) {
    const message = await parseErrorMessage(response);
    throw new Error(message || t("common.requestFailed", { status: response.status }));
  }
  return response.json();
};

const normalizeSample = (payload) => {
  const metricsMap = {};
  const metrics = Array.isArray(payload?.metrics) ? payload.metrics : [];
  metrics.forEach((metric) => {
    if (!metric || !metric.key) {
      return;
    }
    metricsMap[metric.key] = metric;
  });
  return {
    concurrency: Number(payload?.concurrency) || 0,
    metrics: metricsMap,
  };
};

const getBaselineMap = (samples) => {
  const baseline = {};
  const first = samples.find((sample) => sample.concurrency === 1);
  if (!first) {
    return baseline;
  }
  METRICS.forEach((metric) => {
    const entry = first.metrics?.[metric.key];
    const value = Number(entry?.avg_ms);
    baseline[metric.key] = Number.isFinite(value) && value > 0 ? value : null;
  });
  return baseline;
};

const formatMs = (value) => {
  if (!Number.isFinite(value)) {
    return "-";
  }
  return `${Math.max(0, Math.round(value))} ms`;
};

const renderChart = () => {
  if (!ensureChart()) {
    return;
  }
  const samples = Array.isArray(state.performance?.samples) ? state.performance.samples : [];
  const baseline = getBaselineMap(samples);
  const xValues = samples.map((sample) => sample.concurrency);
  const series = METRICS.map((metric) => {
    const values = samples.map((sample) => {
      const entry = sample.metrics?.[metric.key];
      const avg = Number(entry?.avg_ms);
      const base = baseline[metric.key];
      if (!Number.isFinite(avg) || !Number.isFinite(base) || base <= 0) {
        return null;
      }
      const delta = ((avg - base) / base) * 100;
      return Number.isFinite(delta) ? Number(delta.toFixed(2)) : null;
    });
    return {
      name: t(metric.labelKey),
      type: "line",
      smooth: true,
      showSymbol: false,
      data: values,
      lineStyle: { color: metric.color, width: 2 },
      itemStyle: { color: metric.color },
    };
  });
  const option = {
    tooltip: {
      trigger: "axis",
      valueFormatter: (value) =>
        Number.isFinite(value) ? `${Number(value).toFixed(1)}%` : "-",
    },
    legend: {
      data: METRICS.map((metric) => t(metric.labelKey)),
      textStyle: { color: "#64748b" },
    },
    grid: { left: 50, right: 24, top: 30, bottom: 30 },
    xAxis: {
      type: "category",
      name: t("performance.chart.axis.concurrency"),
      data: xValues,
      axisLabel: { color: "#94a3b8" },
      axisLine: { lineStyle: { color: "#e2e8f0" } },
    },
    yAxis: {
      type: "value",
      name: t("performance.chart.axis.delta"),
      axisLabel: {
        color: "#94a3b8",
        formatter: (value) => `${value}%`,
      },
      splitLine: { lineStyle: { color: "#e2e8f0" } },
    },
    series,
  };
  chart.setOption(option, false);
  chart.resize();
};

const renderTable = () => {
  if (!elements.performanceTableBody || !elements.performanceTableEmpty) {
    return;
  }
  const body = elements.performanceTableBody;
  body.textContent = "";
  const samples = Array.isArray(state.performance?.samples) ? state.performance.samples : [];
  if (!samples.length) {
    elements.performanceTableEmpty.style.display = "block";
    return;
  }
  elements.performanceTableEmpty.style.display = "none";
  samples.forEach((sample) => {
    const row = document.createElement("tr");
    const concurrencyCell = document.createElement("td");
    concurrencyCell.textContent = sample.concurrency ? String(sample.concurrency) : "-";
    row.appendChild(concurrencyCell);
    METRICS.forEach((metric) => {
      const cell = document.createElement("td");
      const entry = sample.metrics?.[metric.key];
      const avg = Number(entry?.avg_ms);
      if (Number.isFinite(avg)) {
        cell.textContent = formatMs(avg);
      } else {
        cell.textContent = entry?.error ? t("performance.table.error") : "-";
      }
      if (entry?.error) {
        cell.title = entry.error;
      }
      row.appendChild(cell);
    });
    body.appendChild(row);
  });
  if (elements.performanceTableScroll) {
    elements.performanceTableScroll.scrollTop = elements.performanceTableScroll.scrollHeight;
  }
};

const renderHistoryMeta = (count) => {
  if (!elements.performanceHistoryMeta) {
    return;
  }
  elements.performanceHistoryMeta.textContent = t("performance.history.meta", { count });
};

const applyHistoryEntry = (entry) => {
  if (state.performance?.running) {
    const message = t("performance.history.restoreBusy");
    setFormStatus(message);
    notify(message, "warn");
    return false;
  }
  if (!entry) {
    return false;
  }
  const samples = Array.isArray(entry.samples)
    ? entry.samples.map(normalizeHistorySample).filter(Boolean)
    : [];
  if (!samples.length) {
    return false;
  }
  state.performance.samples = samples;
  const maxConcurrency =
    Number(entry.max_concurrency) || inferMaxConcurrency(samples) || DEFAULT_CONFIG.maxConcurrency;
  const step = Number(entry.step) || inferStep(samples) || DEFAULT_CONFIG.step;
  if (elements.performanceMaxConcurrency) {
    elements.performanceMaxConcurrency.value = String(maxConcurrency);
  }
  if (elements.performanceStep) {
    elements.performanceStep.value = String(step);
  }
  persistConfig();
  renderAll();
  setFormStatus(t("performance.history.restored"));
  notify(t("performance.history.restored"), "success");
  return true;
};

const renderHistoryList = () => {
  if (!elements.performanceHistoryList || !elements.performanceHistoryEmpty) {
    return;
  }
  const history = Array.isArray(state.performance?.history) ? state.performance.history : [];
  elements.performanceHistoryList.textContent = "";
  if (!history.length) {
    elements.performanceHistoryEmpty.style.display = "block";
    renderHistoryMeta(0);
    return;
  }
  elements.performanceHistoryEmpty.style.display = "none";
  renderHistoryMeta(history.length);
  const running = Boolean(state.performance?.running);
  [...history].reverse().forEach((entry) => {
    const row = document.createElement("tr");
    if (running) {
      row.classList.add("is-disabled");
    }
    const startedCell = document.createElement("td");
    startedCell.textContent = formatTimestamp(entry.created_at);
    const maxCell = document.createElement("td");
    maxCell.textContent = entry.max_concurrency ? String(entry.max_concurrency) : "-";
    const stepCell = document.createElement("td");
    stepCell.textContent = entry.step ? String(entry.step) : "-";
    const samplesCell = document.createElement("td");
    samplesCell.textContent = entry.samples?.length ? String(entry.samples.length) : "-";
    row.addEventListener("click", () => {
      try {
        const ok = applyHistoryEntry(entry);
        return ok;
      } catch (error) {
        const message = error?.message || String(error);
        setFormStatus(t("performance.history.restoreFailed", { message }));
        notify(t("performance.history.restoreFailed", { message }), "error");
      }
    });
    row.appendChild(startedCell);
    row.appendChild(maxCell);
    row.appendChild(stepCell);
    row.appendChild(samplesCell);
    elements.performanceHistoryList.appendChild(row);
  });
};

const renderAll = () => {
  renderChart();
  renderTable();
};

const cancelRun = () => {
  if (activeController) {
    activeController.abort();
    activeController = null;
  }
  setRunning(false);
};

const handleReset = () => {
  if (state.performance?.running) {
    cancelRun();
    setFormStatus(t("performance.status.cancelled"));
  } else {
    setFormStatus("");
  }
  resetResults();
};

const handleStart = async () => {
  ensureState();
  if (state.performance.running) {
    setFormStatus(t("performance.error.running"));
    return;
  }
  const maxConcurrency = readPositiveInt(elements.performanceMaxConcurrency, 0);
  if (maxConcurrency <= 0) {
    const message = t("performance.error.maxConcurrency");
    setFormStatus(message);
    notify(message, "error");
    return;
  }
  const step = readPositiveInt(elements.performanceStep, 0);
  if (step <= 0) {
    const message = t("performance.error.step");
    setFormStatus(message);
    notify(message, "error");
    return;
  }
  const sequence = buildSequence(maxConcurrency, step);
  if (!sequence.length) {
    return;
  }
  const startedAt = new Date().toISOString();
  persistConfig();
  resetResults();
  setRunning(true);
  setFormStatus(t("performance.status.starting"));
  let completed = 0;
  try {
    for (const concurrency of sequence) {
      if (!state.performance.running) {
        break;
      }
      const controller = new AbortController();
      activeController = controller;
      const payload = await fetchSample(concurrency, controller.signal);
      const sample = normalizeSample(payload);
      state.performance.samples.push(sample);
      renderAll();
      completed += 1;
      setFormStatus(
        t("performance.status.running", { current: completed, total: sequence.length })
      );
    }
    if (state.performance.running) {
      appendHistoryEntry(
        buildHistoryEntry(state.performance.samples, { maxConcurrency, step }, startedAt)
      );
      setFormStatus(t("performance.status.completed"));
      notify(t("performance.status.completed"), "success");
    }
  } catch (error) {
    if (error?.name === "AbortError") {
      setFormStatus(t("performance.status.cancelled"));
      notify(t("performance.status.cancelled"), "info");
    } else {
      const message = error?.message || String(error);
      setFormStatus(t("performance.status.failed", { message }));
      notify(t("performance.status.failed", { message }), "error");
    }
  } finally {
    activeController = null;
    setRunning(false);
  }
};

export const initPerformancePanel = () => {
  if (initialized) {
    renderAll();
    return;
  }
  initialized = true;
  ensureState();
  applyStoredConfig();
  loadHistory();
  if (elements.performanceStartBtn) {
    elements.performanceStartBtn.addEventListener("click", handleStart);
  }
  if (elements.performanceResetBtn) {
    elements.performanceResetBtn.addEventListener("click", handleReset);
  }
  [elements.performanceMaxConcurrency, elements.performanceStep].forEach((input) => {
    if (!input) {
      return;
    }
    input.addEventListener("input", scheduleConfigSave);
    input.addEventListener("change", scheduleConfigSave);
  });
  window.addEventListener("wunder:language-changed", () => {
    updateStartButton(state.performance?.running);
    renderAll();
    renderHistoryList();
  });
  updateStartButton(false);
  renderAll();
};
