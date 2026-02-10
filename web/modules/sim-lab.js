import { elements } from "./elements.js?v=20260118-07";
import { getWunderBase } from "./api.js";
import { resolveApiErrorMessage } from "./api-error.js";
import { notify } from "./notify.js";
import { getCurrentLanguage, t } from "./i18n.js?v=20260118-07";

let initialized = false;
let running = false;
let projectsCache = [];
let selectedProjectIds = new Set();
let lastReport = null;
let currentRunId = "";
let stopping = false;
let historyItems = [];
let detailPayload = null;
let detailLabel = "";

const HISTORY_STORAGE_KEY = "wunder_sim_lab_history";
const HISTORY_LIMIT = 20;

const numberValue = (element, fallback) => {
  if (!element) {
    return fallback;
  }
  const value = Number(element.value);
  return Number.isFinite(value) ? value : fallback;
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

const setDetail = (label, payload) => {
  detailLabel = label || "";
  detailPayload = payload ?? null;
  renderDetail();
};

const renderDetail = () => {
  if (elements.simLabDetailLabel) {
    elements.simLabDetailLabel.textContent = detailLabel;
  }
  if (!elements.simLabDetailReport) {
    return;
  }
  if (!detailPayload) {
    elements.simLabDetailReport.textContent = t("simLab.detail.empty");
    return;
  }
  try {
    elements.simLabDetailReport.textContent = JSON.stringify(detailPayload, null, 2);
  } catch {
    elements.simLabDetailReport.textContent = String(detailPayload);
  }
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
  if (!elements.simLabHistoryList || !elements.simLabHistoryEmpty) {
    return;
  }
  elements.simLabHistoryList.innerHTML = "";
  const list = Array.isArray(historyItems) ? historyItems : [];
  elements.simLabHistoryEmpty.hidden = list.length > 0;
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
    elements.simLabHistoryList.appendChild(button);
  });
};

const createRunId = () =>
  `simlab_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;

const selectedProjects = () => {
  if (!elements.simLabProjects) {
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
  return values;
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
  const normalized = String(status || "").trim().toLowerCase();
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
  const normalized = String(status || "").trim().toLowerCase();
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
  if (!elements.simLabProjects) {
    return;
  }
  ensureProjectSelection();
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

const projectTitle = (projectId) => {
  const project = projectsCache.find((item) => item.project_id === projectId);
  return project?.title || projectId;
};

const boolLabel = (value) => (value ? t("common.yes") : t("common.no"));

const extractHighlights = (project) => {
  const report = project?.report;
  if (!report || typeof report !== "object") {
    return [];
  }
  const highlights = [];
  if (Number.isFinite(Number(report?.session_runs?.peak_concurrency))) {
    highlights.push({
      label: t("simLab.result.metric.peakConcurrency"),
      value: String(Number(report.session_runs.peak_concurrency)),
    });
  }
  if (Number.isFinite(Number(report?.worker_sessions?.expected))) {
    highlights.push({
      label: t("simLab.result.metric.workersExpected"),
      value: String(Number(report.worker_sessions.expected)),
    });
  }
  if (Number.isFinite(Number(report?.worker_sessions?.created))) {
    highlights.push({
      label: t("simLab.result.metric.workersCreated"),
      value: String(Number(report.worker_sessions.created)),
    });
  }
  if (Number.isFinite(Number(report?.llm_calls?.total))) {
    highlights.push({
      label: t("simLab.result.metric.llmCalls"),
      value: String(Number(report.llm_calls.total)),
    });
  }
  if (report?.checks && typeof report.checks === "object") {
    highlights.push({
      label: t("simLab.result.metric.workersAllSuccess"),
      value: boolLabel(Boolean(report.checks.all_worker_runs_success)),
    });
    highlights.push({
      label: t("simLab.result.metric.noActiveRuns"),
      value: boolLabel(Boolean(report.checks.no_active_runs_left)),
    });
  }
  return highlights;
};

const renderProjectReports = (projects) => {
  if (!elements.simLabReportList) {
    return;
  }
  elements.simLabReportList.innerHTML = "";
  const list = Array.isArray(projects) ? projects : [];
  if (!list.length) {
    const empty = document.createElement("div");
    empty.className = "muted";
    empty.textContent = t("simLab.result.projectsEmpty");
    elements.simLabReportList.appendChild(empty);
    return;
  }

  list.forEach((project) => {
    const row = document.createElement("div");
    row.className = "simlab-report-item";

    const header = document.createElement("div");
    header.className = "simlab-report-summary";

    const left = document.createElement("div");
    left.className = "simlab-report-left";

    const title = document.createElement("div");
    title.className = "simlab-report-title";
    title.textContent = projectTitle(project.project_id);

    const subtitle = document.createElement("div");
    subtitle.className = "simlab-report-subtitle";
    subtitle.textContent = project.project_id;

    left.appendChild(title);
    left.appendChild(subtitle);

    const right = document.createElement("div");
    right.className = "simlab-report-right";

    const duration = document.createElement("span");
    duration.className = "simlab-report-duration";
    duration.textContent = formatSeconds(project.wall_time_s);

    const status = document.createElement("span");
    status.className = `simlab-status-pill ${statusClass(project.status)}`;
    status.textContent = statusLabel(project.status);

    const detailBtn = document.createElement("button");
    detailBtn.type = "button";
    detailBtn.className = "simlab-detail-btn";
    detailBtn.textContent = t("simLab.result.viewDetail");
    detailBtn.addEventListener("click", () => {
      const label = t("simLab.detail.projectLabel", {
        title: projectTitle(project.project_id),
        runId: lastReport?.run_id || "-",
      });
      setDetail(label, {
        project_id: project.project_id,
        status: project.status,
        wall_time_s: project.wall_time_s,
        error: project.error,
        report: project.report,
      });
    });

    right.appendChild(duration);
    right.appendChild(status);
    right.appendChild(detailBtn);

    header.appendChild(left);
    header.appendChild(right);
    row.appendChild(header);

    const content = document.createElement("div");
    content.className = "simlab-report-content";

    if (project.error) {
      const errorBox = document.createElement("div");
      errorBox.className = "simlab-report-error";
      errorBox.textContent = `${t("simLab.result.error")}: ${project.error}`;
      content.appendChild(errorBox);
    }

    const highlights = extractHighlights(project);
    if (highlights.length) {
      const grid = document.createElement("div");
      grid.className = "simlab-highlight-grid";
      highlights.forEach((item) => {
        const card = document.createElement("div");
        card.className = "simlab-highlight-item";
        const label = document.createElement("span");
        label.className = "simlab-highlight-label";
        label.textContent = item.label;
        const value = document.createElement("strong");
        value.className = "simlab-highlight-value";
        value.textContent = item.value;
        card.appendChild(label);
        card.appendChild(value);
        grid.appendChild(card);
      });
      content.appendChild(grid);
    }

    row.appendChild(content);
    elements.simLabReportList.appendChild(row);
  });
};

const renderReport = (report) => {
  lastReport = report || null;
  renderProjectReports(lastReport?.projects || []);
  if (!lastReport) {
    setDetail("", null);
    return;
  }
  setDetail(
    t("simLab.detail.runLabel", {
      runId: String(lastReport.run_id || "-"),
    }),
    lastReport
  );
};

const buildRunPayload = (runId) => {
  const projects = selectedProjects();
  if (!projects.length) {
    throw new Error(t("simLab.error.noProject"));
  }
  return {
    run_id: runId,
    projects,
    options: {
      swarm_flow: {
        workers: Math.max(1, Math.floor(numberValue(elements.simLabWorkers, 100))),
        max_wait_s: Math.max(10, Math.floor(numberValue(elements.simLabMaxWait, 180))),
        mother_wait_s: Math.max(1, numberValue(elements.simLabMotherWait, 30)),
        poll_ms: Math.max(40, Math.floor(numberValue(elements.simLabPollMs, 120))),
      },
    },
  };
};

const stopSimulation = async () => {
  if (!running || stopping || !currentRunId) {
    return;
  }
  stopping = true;
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
    const message = error?.message || String(error);
    setStatus(t("simLab.status.stopFailed", { message }));
    notify(t("simLab.status.stopFailed", { message }), "error");
  } finally {
    stopping = false;
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
  running = true;
  stopping = false;
  currentRunId = createRunId();
  updateRunButton();
  setStatus(t("simLab.status.running"));
  try {
    const payload = buildRunPayload(currentRunId);
    const response = await fetch(`${getWunderBase()}/admin/sim_lab/runs`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    if (!response.ok) {
      throw new Error(await resolveApiErrorMessage(response, t("simLab.error.runFailed")));
    }
    const data = await response.json();
    const report = data?.data || {};
    renderReport(report);
    upsertHistory(report);

    const projects = Array.isArray(report?.projects) ? report.projects : [];
    const cancelledCount = projects.filter(
      (item) => String(item?.status || "").toLowerCase() === "cancelled"
    ).length;

    if (cancelledCount > 0) {
      setStatus(t("simLab.status.cancelled", { cancelled: cancelledCount }));
      notify(t("simLab.status.cancelledNotify"), "warning");
    } else {
      setStatus(
        t("simLab.status.completed", {
          success: Number(report?.project_success || 0),
          total: Number(report?.project_total || 0),
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
      renderDetail();
      updateRunButton();
    });
    hydrateHistory();
    updateRunButton();
    renderReport(null);
    renderHistory();
    renderDetail();
  }
  await reconcileRunningState();
  await fetchProjects();
  await reconcileRunningState();
};
