import { elements } from "./elements.js?v=20260210-06";
import { getWunderBase } from "./api.js";
import { resolveApiErrorMessage } from "./api-error.js";
import { notify } from "./notify.js";
import { getCurrentLanguage, t } from "./i18n.js?v=20260210-06";

let initialized = false;
let running = false;
let projectsCache = [];
let selectedProjectIds = new Set();
let lastReport = null;
let currentRunId = "";
let stopping = false;
let cancelRequested = false;
let historyItems = [];
let curveChart = null;

const HISTORY_STORAGE_KEY = "wunder_sim_lab_history";
const HISTORY_LIMIT = 20;
const DEFAULT_WORKER_STEP = 10;
const CURVE_SERIES = [
  {
    key: "wall_time_s",
    labelKey: "simLab.chart.metric.wallTime",
    color: "#3b82f6",
    resolve: (sample) => Number(sample?.wall_time_s),
  },
  {
    key: "peak_concurrency",
    labelKey: "simLab.chart.metric.peakConcurrency",
    color: "#22c55e",
    resolve: (sample) => Number(sample?.report?.session_runs?.peak_concurrency),
  },
  {
    key: "end_to_end_p95",
    labelKey: "simLab.chart.metric.endToEndP95",
    color: "#f97316",
    resolve: (sample) => Number(sample?.report?.worker_latency?.end_to_end_ms_p95),
  },
];

const numberValue = (element, fallback) => {
  if (!element) {
    return fallback;
  }
  const value = Number(element.value);
  return Number.isFinite(value) ? value : fallback;
};

const readPositiveInt = (element, fallback) => {
  const value = Math.floor(numberValue(element, fallback));
  return Number.isFinite(value) && value > 0 ? value : fallback;
};

const formatSeconds = (value) => {
  const numeric = Number(value);
  if (!Number.isFinite(numeric)) {
    return "-";
  }
  return `${numeric.toFixed(3)} s`;
};

const formatDateTime = (value) => {
  if (!value) {
    return "-";
  }
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) {
    return "-";
  }
  return parsed.toLocaleString(getCurrentLanguage());
};

const readHistoryStorage = () => {
  try {
    const raw = localStorage.getItem(HISTORY_STORAGE_KEY);
    if (!raw) {
      return [];
    }
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
};

const saveHistoryStorage = (items) => {
  const normalized = Array.isArray(items) ? items.slice(0, HISTORY_LIMIT) : [];
  historyItems = normalized;
  try {
    localStorage.setItem(HISTORY_STORAGE_KEY, JSON.stringify(normalized));
  } catch {
    // ignore storage failures
  }
};

const hydrateHistory = () => {
  historyItems = readHistoryStorage().slice(0, HISTORY_LIMIT);
};

const buildHistoryItem = (report) => ({
  run_id: String(report?.run_id || createRunId()),
  started_at: report?.started_at || new Date().toISOString(),
  finished_at: report?.finished_at || new Date().toISOString(),
  project_total: Number(report?.project_total || 0),
  project_success: Number(report?.project_success || 0),
  project_failed: Number(report?.project_failed || 0),
  mode: String(report?.mode || "parallel"),
  report,
});

const upsertHistory = (report) => {
  if (!report || typeof report !== "object") {
    return;
  }
  const item = buildHistoryItem(report);
  const withoutCurrent = historyItems.filter((entry) => entry.run_id !== item.run_id);
  saveHistoryStorage([item, ...withoutCurrent]);
  renderHistory();
};

const restoreHistory = (runId) => {
  const selected = historyItems.find((item) => item.run_id === runId);
  if (!selected) {
    return;
  }
  renderReport(selected.report || null);
  setStatus(t("simLab.status.restored", { runId: selected.run_id }));
};

const renderHistory = () => {
  const historyListEl =
    elements.simLabHistoryList || document.getElementById("simLabHistoryList");
  const historyEmptyEl =
    elements.simLabHistoryEmpty || document.getElementById("simLabHistoryEmpty");
  if (!historyListEl || !historyEmptyEl) {
    return;
  }
  historyListEl.innerHTML = "";
  const list = Array.isArray(historyItems) ? historyItems : [];
  historyEmptyEl.hidden = list.length > 0;
  if (!list.length) {
    return;
  }

  list.forEach((item) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "simlab-history-item";

    const title = document.createElement("div");
    title.className = "simlab-history-item-title";
    title.textContent = `${item.run_id}`;

    const meta = document.createElement("div");
    meta.className = "simlab-history-item-meta";
    meta.textContent = t("simLab.history.itemMeta", {
      time: formatDateTime(item.finished_at || item.started_at),
      success: Number(item.project_success || 0),
      total: Number(item.project_total || 0),
    });

    button.appendChild(title);
    button.appendChild(meta);
    button.addEventListener("click", () => restoreHistory(item.run_id));
    historyListEl.appendChild(button);
  });
};

const createRunId = () =>
  `simlab_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;

const buildWorkerSequence = (maxWorkers, step) => {
  if (maxWorkers <= 0 || step <= 0) {
    return [];
  }
  const sequence = [];
  let current = 1;
  while (current < maxWorkers) {
    sequence.push(current);
    current += step;
  }
  if (!sequence.length || sequence[sequence.length - 1] !== maxWorkers) {
    sequence.push(maxWorkers);
  }
  return sequence;
};

const ensureCurveChart = () => {
  if (!elements.simLabChart || !window.echarts) {
    return null;
  }
  if (!curveChart) {
    curveChart = window.echarts.init(elements.simLabChart);
  }
  return curveChart;
};

const listSamplesFromReport = (report) => {
  if (!report || typeof report !== "object") {
    return [];
  }
  const normalized = [];
  if (Array.isArray(report.samples)) {
    report.samples.forEach((sample, index) => {
      const workers = Number(sample?.workers);
      if (!Number.isFinite(workers) || workers <= 0) {
        return;
      }
      normalized.push({
        workers,
        wall_time_s: Number(sample?.wall_time_s) || 0,
        report: sample?.report || null,
        status: sample?.status || "unknown",
        run_id: sample?.run_id || report?.run_id || "",
        project_id: sample?.project_id || "swarm_flow",
        error: sample?.error || null,
        order: Number(sample?.order) || index + 1,
      });
    });
    if (normalized.length) {
      return normalized.sort((left, right) => left.workers - right.workers);
    }
  }

  const projects = Array.isArray(report.projects) ? report.projects : [];
  projects.forEach((project, index) => {
    const workers = Number(project?.report?.config?.workers);
    if (!Number.isFinite(workers) || workers <= 0) {
      return;
    }
    normalized.push({
      workers,
      wall_time_s: Number(project?.wall_time_s) || 0,
      report: project?.report || null,
      status: project?.status || "unknown",
      run_id: report?.run_id || "",
      project_id: project?.project_id || "swarm_flow",
      error: project?.error || null,
      order: index + 1,
    });
  });
  return normalized.sort((left, right) => left.workers - right.workers);
};

const renderCurveChart = (report) => {
  const chart = ensureCurveChart();
  if (!chart) {
    return;
  }
  const samples = listSamplesFromReport(report);
  if (!samples.length) {
    chart.clear();
    chart.resize();
    return;
  }

  const baseline = samples[0];
  const baselineValues = Object.fromEntries(
    CURVE_SERIES.map((series) => {
      const value = Number(series.resolve(baseline));
      return [series.key, Number.isFinite(value) && value > 0 ? value : null];
    })
  );

  const xValues = samples.map((sample) => sample.workers);
  const series = CURVE_SERIES.map((seriesDef) => ({
    name: t(seriesDef.labelKey),
    type: "line",
    smooth: true,
    showSymbol: false,
    lineStyle: { color: seriesDef.color, width: 2 },
    itemStyle: { color: seriesDef.color },
    data: samples.map((sample) => {
      const current = Number(seriesDef.resolve(sample));
      const base = baselineValues[seriesDef.key];
      if (!Number.isFinite(current) || !Number.isFinite(base) || base <= 0) {
        return null;
      }
      const delta = ((current - base) / base) * 100;
      return Number.isFinite(delta) ? Number(delta.toFixed(2)) : null;
    }),
  }));

  chart.setOption(
    {
      tooltip: {
        trigger: "axis",
        valueFormatter: (value) =>
          Number.isFinite(value) ? `${Number(value).toFixed(1)}%` : "-",
      },
      legend: {
        data: CURVE_SERIES.map((item) => t(item.labelKey)),
        textStyle: { color: "#64748b" },
      },
      grid: { left: 50, right: 24, top: 30, bottom: 30 },
      xAxis: {
        type: "category",
        name: t("simLab.chart.axis.workers"),
        data: xValues,
        axisLabel: { color: "#94a3b8" },
        axisLine: { lineStyle: { color: "#e2e8f0" } },
      },
      yAxis: {
        type: "value",
        name: t("simLab.chart.axis.delta"),
        axisLabel: {
          color: "#94a3b8",
          formatter: (value) => `${value}%`,
        },
        splitLine: { lineStyle: { color: "#e2e8f0" } },
      },
      series,
    },
    false
  );
  chart.resize();
};

const selectedProjects = () => {
  const fromState = [...selectedProjectIds].filter((projectId) => projectId && projectId.trim());
  if (!elements.simLabProjects) {
    if (fromState.length) {
      return fromState;
    }
    if (projectsCache.length) {
      return [projectsCache[0].project_id];
    }
    return [];
  }
  const values = [];
  elements.simLabProjects
    .querySelectorAll('input[type="checkbox"][data-project-id]:checked')
    .forEach((input) => {
      const projectId = input.dataset.projectId?.trim();
      if (projectId) {
        values.push(projectId);
      }
    });
  if (values.length) {
    return values;
  }
  if (fromState.length) {
    return fromState;
  }
  if (projectsCache.length) {
    return [projectsCache[0].project_id];
  }
  return [];
};

const setStatus = (message = "") => {
  if (elements.simLabStatus) {
    elements.simLabStatus.textContent = message;
  }
};

const updateRunButton = () => {
  if (!elements.simLabRunBtn) {
    return;
  }

  elements.simLabRunBtn.disabled = stopping;
  elements.simLabRunBtn.classList.toggle("is-running", running);
  elements.simLabRunBtn.classList.toggle("danger", running);

  [
    elements.simLabWorkers,
    elements.simLabWorkerStep,
    elements.simLabMaxWait,
    elements.simLabMotherWait,
    elements.simLabPollMs,
    elements.simLabRefreshProjectsBtn,
  ].forEach((field) => {
    if (field) {
      field.disabled = running;
    }
  });
  if (elements.simLabProjects) {
    elements.simLabProjects
      .querySelectorAll('input[type="checkbox"][data-project-id]')
      .forEach((input) => {
        input.disabled = running;
      });
  }

  const icon = elements.simLabRunBtn.querySelector("i");
  if (icon) {
    icon.className = `fa-solid ${running ? "fa-stop" : "fa-play"}`;
  }

  const runLabel = elements.simLabRunBtn.querySelector("span");
  if (!runLabel) {
    return;
  }
  if (stopping) {
    runLabel.textContent = t("simLab.action.stopping");
    return;
  }
  runLabel.textContent = running ? t("simLab.action.stop") : t("simLab.action.run");
};

const resetRunState = () => {
  running = false;
  stopping = false;
  cancelRequested = false;
  currentRunId = "";
  updateRunButton();
};

const reconcileRunningState = async () => {
  if (!running) {
    return;
  }
  if (!currentRunId) {
    resetRunState();
    setStatus(t("simLab.status.recovered"));
    return;
  }
  try {
    const response = await fetch(
      `${getWunderBase()}/admin/sim_lab/runs/${encodeURIComponent(currentRunId)}/status`
    );
    if (!response.ok) {
      if (response.status === 404) {
        resetRunState();
        setStatus(t("simLab.status.recovered"));
      }
      return;
    }
    const payload = await response.json();
    const active = Boolean(payload?.data?.active);
    if (!active) {
      resetRunState();
      setStatus(t("simLab.status.recovered"));
    }
  } catch {
    // keep current state when status probing fails due transient network issues
  }
};

const statusLabel = (status) => {
  const normalized = normalizeRunStatus(status);
  if (normalized === "success") {
    return t("simLab.result.status.success");
  }
  if (normalized === "failed") {
    return t("simLab.result.status.failed");
  }
  if (normalized === "cancelled") {
    return t("simLab.result.status.cancelled");
  }
  return status || "-";
};

const statusClass = (status) => {
  const normalized = normalizeRunStatus(status);
  if (normalized === "success") {
    return "is-success";
  }
  if (normalized === "failed") {
    return "is-failed";
  }
  if (normalized === "cancelled") {
    return "is-cancelled";
  }
  return "is-unknown";
};

const applyDefaults = () => {
  const swarm = projectsCache.find((item) => item.project_id === "swarm_flow");
  const defaults = swarm?.defaults || {};
  if (elements.simLabWorkers) {
    elements.simLabWorkers.value = String(defaults.workers || 100);
  }
  if (elements.simLabWorkerStep) {
    elements.simLabWorkerStep.value = String(DEFAULT_WORKER_STEP);
  }
  if (elements.simLabMaxWait) {
    elements.simLabMaxWait.value = String(defaults.max_wait_s || 180);
  }
  if (elements.simLabMotherWait) {
    elements.simLabMotherWait.value = String(defaults.mother_wait_s || 30);
  }
  if (elements.simLabPollMs) {
    elements.simLabPollMs.value = String(defaults.poll_ms || 120);
  }
};

const ensureProjectSelection = () => {
  const available = new Set(projectsCache.map((item) => item.project_id));
  selectedProjectIds = new Set(
    [...selectedProjectIds].filter((projectId) => available.has(projectId))
  );
  if (!selectedProjectIds.size && projectsCache.length) {
    selectedProjectIds.add(projectsCache[0].project_id);
  }
};

const renderProjects = () => {
  ensureProjectSelection();
  if (!elements.simLabProjects) {
    return;
  }
  elements.simLabProjects.innerHTML = "";
  if (!projectsCache.length) {
    elements.simLabProjects.textContent = t("simLab.projects.empty");
    return;
  }

  projectsCache.forEach((project) => {
    const projectId = project.project_id;
    const item = document.createElement("label");
    item.className = "simlab-project-item";
    item.dataset.projectId = projectId;

    const head = document.createElement("div");
    head.className = "simlab-project-head";

    const input = document.createElement("input");
    input.type = "checkbox";
    input.dataset.projectId = projectId;
    input.checked = selectedProjectIds.has(projectId);

    const titleBlock = document.createElement("div");
    titleBlock.className = "simlab-project-title-block";

    const title = document.createElement("div");
    title.className = "simlab-project-title";
    title.textContent = project.title || projectId;

    const subtitle = document.createElement("div");
    subtitle.className = "simlab-project-subtitle";
    subtitle.textContent = projectId;

    titleBlock.appendChild(title);
    titleBlock.appendChild(subtitle);

    head.appendChild(input);
    head.appendChild(titleBlock);
    item.appendChild(head);

    if (project.description) {
      const desc = document.createElement("div");
      desc.className = "simlab-project-desc";
      desc.textContent = project.description;
      item.appendChild(desc);
    }

    const defaults = project.defaults && typeof project.defaults === "object" ? project.defaults : {};
    const chips = [];
    if (Number.isFinite(Number(defaults.workers))) {
      chips.push(t("simLab.project.chip.workers", { value: Number(defaults.workers) }));
    }
    if (Number.isFinite(Number(defaults.max_wait_s))) {
      chips.push(t("simLab.project.chip.maxWait", { value: Number(defaults.max_wait_s) }));
    }
    if (chips.length) {
      const chipList = document.createElement("div");
      chipList.className = "simlab-project-chips";
      chips.forEach((chipText) => {
        const chip = document.createElement("span");
        chip.className = "simlab-chip";
        chip.textContent = chipText;
        chipList.appendChild(chip);
      });
      item.appendChild(chipList);
    }

    const syncSelectedStyle = () => {
      item.classList.toggle("is-selected", input.checked);
    };

    input.addEventListener("change", () => {
      if (input.checked) {
        selectedProjectIds.add(projectId);
      } else {
        selectedProjectIds.delete(projectId);
      }
      syncSelectedStyle();
    });

    syncSelectedStyle();
    elements.simLabProjects.appendChild(item);
  });
};

const fetchProjects = async () => {
  if (!running) {
    setStatus(t("simLab.status.loadingProjects"));
  }
  const response = await fetch(`${getWunderBase()}/admin/sim_lab/projects`);
  if (!response.ok) {
    throw new Error(await resolveApiErrorMessage(response, t("simLab.error.loadProjects")));
  }
  const payload = await response.json();
  projectsCache = payload?.data?.items || [];
  renderProjects();
  applyDefaults();
  if (!running) {
    setStatus(t("simLab.status.projectsReady", { count: projectsCache.length }));
  }
};

const boolLabel = (value) => (value ? t("common.yes") : t("common.no"));

const formatInteger = (value) => {
  const numeric = Number(value);
  return Number.isFinite(numeric) ? String(Math.floor(numeric)) : "-";
};

const formatWorkerSessions = (report) => {
  const expected = Number(report?.worker_sessions?.expected);
  const created = Number(report?.worker_sessions?.created);
  if (Number.isFinite(created) && Number.isFinite(expected)) {
    return `${Math.floor(created)}/${Math.floor(expected)}`;
  }
  if (Number.isFinite(created)) {
    return `${Math.floor(created)}/-`;
  }
  if (Number.isFinite(expected)) {
    return `-/${Math.floor(expected)}`;
  }
  return "-";
};

const formatChecks = (report) => {
  if (!report?.checks || typeof report.checks !== "object") {
    return "-";
  }
  const workerAllSuccess = boolLabel(Boolean(report.checks.all_worker_runs_success));
  const noActiveRunsLeft = boolLabel(Boolean(report.checks.no_active_runs_left));
  return `${workerAllSuccess}/${noActiveRunsLeft}`;
};

const normalizeRunStatus = (status) => {
  const normalized = String(status || "").trim().toLowerCase();
  if (!normalized) {
    return "unknown";
  }
  if (normalized === "error") {
    return "failed";
  }
  return normalized;
};

const extractPrimaryProject = (runReport) => {
  const projects = Array.isArray(runReport?.projects) ? runReport.projects : [];
  if (!projects.length) {
    return null;
  }
  return projects.find((item) => item?.project_id === "swarm_flow") || projects[0] || null;
};

const buildStepSample = (workers, order, runReport) => {
  const project = extractPrimaryProject(runReport);
  const projectStatus = normalizeRunStatus(project?.status);
  const status = projectStatus === "unknown" ? "failed" : projectStatus;
  return {
    order,
    workers,
    run_id: String(runReport?.run_id || ""),
    started_at: runReport?.started_at || new Date().toISOString(),
    finished_at: runReport?.finished_at || new Date().toISOString(),
    wall_time_s: Number(runReport?.wall_time_s || 0),
    project_id: project?.project_id || "swarm_flow",
    status,
    error: project?.error || null,
    report: project?.report || null,
  };
};

const toProjectReportRow = (sample) => ({
  project_id: sample.project_id || "swarm_flow",
  status: sample.status || "unknown",
  wall_time_s: Number(sample.wall_time_s || 0),
  error: sample.error || null,
  report: sample.report || null,
  workers: Number(sample.workers) || 0,
  run_id: sample.run_id || "",
  order: Number(sample.order) || 0,
});

const buildSuiteReport = ({
  suiteRunId,
  startedAt,
  finishedAt,
  maxWorkers,
  workerStep,
  sequence,
  samples,
}) => {
  const rows = (Array.isArray(samples) ? samples : []).map(toProjectReportRow);
  const success = rows.filter((item) => normalizeRunStatus(item.status) === "success").length;
  const failed = rows.length - success;
  const wallTimeS =
    (new Date(finishedAt).getTime() - new Date(startedAt).getTime()) / 1000;
  return {
    run_id: suiteRunId,
    mode: "parallel",
    started_at: startedAt,
    finished_at: finishedAt,
    wall_time_s: Number.isFinite(wallTimeS) ? Math.max(0, wallTimeS) : 0,
    project_total: rows.length,
    planned_total: Array.isArray(sequence) ? sequence.length : rows.length,
    project_success: success,
    project_failed: failed,
    worker_max: maxWorkers,
    worker_step: workerStep,
    workers_sequence: Array.isArray(sequence) ? sequence : [],
    samples: rows.map((item) => ({
      order: item.order,
      workers: item.workers,
      run_id: item.run_id,
      project_id: item.project_id,
      status: item.status,
      wall_time_s: item.wall_time_s,
      error: item.error,
      report: item.report,
    })),
    projects: rows,
  };
};

const renderProjectReports = (projects) => {
  if (!elements.simLabReportList) {
    return;
  }
  elements.simLabReportList.innerHTML = "";
  const list = (Array.isArray(projects) ? projects.slice() : []).sort(
    (left, right) => (Number(left?.order) || 0) - (Number(right?.order) || 0)
  );
  if (!list.length) {
    const empty = document.createElement("div");
    empty.className = "muted";
    empty.textContent = t("simLab.result.projectsEmpty");
    elements.simLabReportList.appendChild(empty);
    return;
  }

  const tableWrap = document.createElement("div");
  tableWrap.className = "simlab-report-table-wrap";

  const table = document.createElement("table");
  table.className = "simlab-report-table";

  const thead = document.createElement("thead");
  const headRow = document.createElement("tr");
  const columns = [
    { key: "simLab.table.col.step", className: "is-center" },
    { key: "simLab.table.col.workers", className: "is-center" },
    { key: "simLab.table.col.status", className: "is-center" },
    { key: "simLab.table.col.wallTime", className: "is-right" },
    { key: "simLab.table.col.peakConcurrency", className: "is-right" },
    { key: "simLab.table.col.workerSessions", className: "is-center" },
    { key: "simLab.table.col.llmCalls", className: "is-right" },
    { key: "simLab.table.col.checks", className: "is-center" },
    { key: "simLab.table.col.runId" },
    { key: "simLab.table.col.error" },
  ];
  columns.forEach((column) => {
    const th = document.createElement("th");
    th.textContent = t(column.key);
    if (column.className) {
      th.className = column.className;
    }
    headRow.appendChild(th);
  });
  thead.appendChild(headRow);

  const tbody = document.createElement("tbody");
  list.forEach((project, index) => {
    const row = document.createElement("tr");
    row.className = `simlab-report-row ${statusClass(project.status)}`;

    const report = project?.report && typeof project.report === "object" ? project.report : null;
    const peakConcurrency = formatInteger(report?.session_runs?.peak_concurrency);
    const workerSessions = formatWorkerSessions(report);
    const llmCalls = formatInteger(report?.llm_calls?.total);
    const checks = formatChecks(report);

    const cells = [
      { text: String(Number(project?.order) || index + 1), className: "is-center" },
      {
        text:
          Number.isFinite(Number(project?.workers)) && Number(project.workers) > 0
            ? String(Math.floor(Number(project.workers)))
            : "-",
        className: "is-center",
      },
      {
        text: statusLabel(project.status),
        className: "is-center simlab-cell-status",
        pillClass: statusClass(project.status),
      },
      { text: formatSeconds(project.wall_time_s), className: "is-right" },
      { text: peakConcurrency, className: "is-right" },
      { text: workerSessions, className: "is-center" },
      { text: llmCalls, className: "is-right" },
      { text: checks, className: "is-center" },
      { text: project.run_id || "-", className: "simlab-cell-runid" },
      { text: project.error || "-", className: "simlab-cell-error" },
    ];

    cells.forEach((cell) => {
      const td = document.createElement("td");
      if (cell.className) {
        td.className = cell.className;
      }
      if (cell.pillClass) {
        const pill = document.createElement("span");
        pill.className = `simlab-status-pill ${cell.pillClass}`;
        pill.textContent = cell.text;
        td.appendChild(pill);
      } else {
        td.textContent = cell.text;
      }
      row.appendChild(td);
    });

    tbody.appendChild(row);
  });

  table.appendChild(thead);
  table.appendChild(tbody);
  tableWrap.appendChild(table);
  elements.simLabReportList.appendChild(tableWrap);
};

const renderReport = (report) => {
  lastReport = report || null;
  renderProjectReports(lastReport?.projects || []);
  renderCurveChart(lastReport);
};

const buildRunPayload = (runId, workersOverride) => {
  const projects = selectedProjects();
  if (!projects.length) {
    throw new Error(t("simLab.error.noProject"));
  }
  const workers = Number.isFinite(Number(workersOverride))
    ? Number(workersOverride)
    : numberValue(elements.simLabWorkers, 100);
  return {
    run_id: runId,
    projects,
    options: {
      swarm_flow: {
        workers: Math.max(1, Math.floor(workers)),
        max_wait_s: Math.max(10, Math.floor(numberValue(elements.simLabMaxWait, 180))),
        mother_wait_s: Math.max(1, numberValue(elements.simLabMotherWait, 30)),
        poll_ms: Math.max(40, Math.floor(numberValue(elements.simLabPollMs, 120))),
      },
    },
  };
};

const executeRunPayload = async (payload) => {
  const response = await fetch(`${getWunderBase()}/admin/sim_lab/runs`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });
  if (!response.ok) {
    throw new Error(await resolveApiErrorMessage(response, t("simLab.error.runFailed")));
  }
  const data = await response.json();
  return data?.data || {};
};

const stopSimulation = async () => {
  if (!running || stopping || !currentRunId) {
    return;
  }
  stopping = true;
  cancelRequested = true;
  updateRunButton();
  setStatus(t("simLab.status.stopping"));
  try {
    const response = await fetch(
      `${getWunderBase()}/admin/sim_lab/runs/${encodeURIComponent(currentRunId)}/cancel`,
      {
        method: "POST",
      }
    );
    if (!response.ok) {
      if (response.status === 404) {
        resetRunState();
        setStatus(t("simLab.status.recovered"));
        return;
      }
      throw new Error(await resolveApiErrorMessage(response, t("simLab.error.stopFailed")));
    }
  } catch (error) {
    cancelRequested = false;
    stopping = false;
    const message = error?.message || String(error);
    setStatus(t("simLab.status.stopFailed", { message }));
    notify(t("simLab.status.stopFailed", { message }), "error");
  } finally {
    updateRunButton();
  }
};

const handleRunButtonClick = async () => {
  if (running) {
    await stopSimulation();
    return;
  }
  await runSimulation();
};

const runSimulation = async () => {
  if (running) {
    return;
  }

  const maxWorkers = readPositiveInt(elements.simLabWorkers, 100);
  const workerStep = readPositiveInt(elements.simLabWorkerStep, DEFAULT_WORKER_STEP);
  if (workerStep <= 0) {
    const message = t("simLab.error.step");
    setStatus(message);
    notify(message, "error");
    return;
  }

  const sequence = buildWorkerSequence(maxWorkers, workerStep);
  if (!sequence.length) {
    const message = t("simLab.error.step");
    setStatus(message);
    notify(message, "error");
    return;
  }

  running = true;
  stopping = false;
  cancelRequested = false;
  currentRunId = createRunId();
  const suiteRunId = currentRunId;
  const startedAt = new Date().toISOString();
  const samples = [];
  updateRunButton();
  setStatus(t("simLab.status.running"));

  try {
    renderReport(
      buildSuiteReport({
        suiteRunId,
        startedAt,
        finishedAt: startedAt,
        maxWorkers,
        workerStep,
        sequence,
        samples,
      })
    );

    for (let index = 0; index < sequence.length; index += 1) {
      if (!running || stopping || cancelRequested) {
        break;
      }
      const workers = sequence[index];
      currentRunId = `${suiteRunId}_${workers}_${index + 1}`;
      setStatus(
        t("simLab.status.runningStep", {
          current: index + 1,
          total: sequence.length,
          workers,
        })
      );

      const payload = buildRunPayload(currentRunId, workers);
      const runReport = await executeRunPayload(payload);
      const sample = buildStepSample(workers, index + 1, runReport);
      samples.push(sample);

      renderReport(
        buildSuiteReport({
          suiteRunId,
          startedAt,
          finishedAt: new Date().toISOString(),
          maxWorkers,
          workerStep,
          sequence,
          samples,
        })
      );

      if (normalizeRunStatus(sample.status) === "cancelled") {
        stopping = true;
        cancelRequested = true;
        break;
      }
    }

    const finalReport = buildSuiteReport({
      suiteRunId,
      startedAt,
      finishedAt: new Date().toISOString(),
      maxWorkers,
      workerStep,
      sequence,
      samples,
    });
    renderReport(finalReport);
    if (finalReport.project_total > 0) {
      upsertHistory(finalReport);
    }

    const cancelledCount = finalReport.projects.filter(
      (item) => normalizeRunStatus(item?.status) === "cancelled"
    ).length;

    if (stopping || cancelRequested || cancelledCount > 0) {
      setStatus(t("simLab.status.cancelled", { cancelled: cancelledCount || 1 }));
      notify(t("simLab.status.cancelledNotify"), "warning");
    } else {
      setStatus(
        t("simLab.status.completed", {
          success: Number(finalReport?.project_success || 0),
          total: Number(finalReport?.project_total || 0),
        })
      );
      notify(t("simLab.status.completedNotify"), "success");
    }
  } catch (error) {
    const message = error?.message || String(error);
    setStatus(t("simLab.status.failed", { message }));
    notify(t("simLab.status.failed", { message }), "error");
  } finally {
    resetRunState();
  }
};

export const initSimLabPanel = async () => {
  if (!initialized) {
    initialized = true;
    if (elements.simLabRunBtn) {
      elements.simLabRunBtn.addEventListener("click", handleRunButtonClick);
    }
    if (elements.simLabRefreshProjectsBtn) {
      elements.simLabRefreshProjectsBtn.addEventListener("click", () => {
        fetchProjects().catch((error) => {
          const message = error?.message || String(error);
          setStatus(message);
          notify(message, "error");
        });
      });
    }
    window.addEventListener("wunder:language-changed", () => {
      renderProjects();
      renderReport(lastReport);
      renderHistory();
      updateRunButton();
    });
    hydrateHistory();
    updateRunButton();
    renderReport(null);
    renderHistory();
  }
  await reconcileRunningState();
  await fetchProjects();
  await reconcileRunningState();
};
