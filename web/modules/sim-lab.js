import { elements } from "./elements.js?v=20260124-01";
import { getWunderBase } from "./api.js";
import { resolveApiErrorMessage } from "./api-error.js";
import { notify } from "./notify.js";
import { t } from "./i18n.js?v=20260124-01";

let initialized = false;
let running = false;
let projectsCache = [];
let selectedProjectIds = new Set();
let lastReport = null;
let currentRunId = "";
let stopping = false;

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

const formatIsoTime = (value) => {
  if (!value) {
    return "-";
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return String(value);
  }
  return date.toLocaleString();
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
  if (elements.simLabRunBtn) {
    elements.simLabRunBtn.disabled = running;
    const runLabel = elements.simLabRunBtn.querySelector("span");
    if (runLabel) {
      runLabel.textContent = running ? t("simLab.action.running") : t("simLab.action.run");
    }
  }

  if (elements.simLabStopBtn) {
    elements.simLabStopBtn.hidden = !running;
    elements.simLabStopBtn.disabled = !running || stopping;
    const stopLabel = elements.simLabStopBtn.querySelector("span");
    if (stopLabel) {
      stopLabel.textContent = stopping ? t("simLab.action.stopping") : t("simLab.action.stop");
    }
  }

  if (elements.simLabRunningIndicator) {
    elements.simLabRunningIndicator.hidden = !running;
  }
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
  if (elements.simLabKeepArtifacts) {
    elements.simLabKeepArtifacts.checked = Boolean(defaults.keep_artifacts);
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

const renderSummary = (report) => {
  if (!elements.simLabSummaryGrid) {
    return;
  }
  elements.simLabSummaryGrid.innerHTML = "";

  const modeValue =
    report?.mode === "sequential" ? t("simLab.mode.sequential") : t("simLab.mode.parallel");
  const summaryRows = [
    {
      label: t("simLab.result.summary.runId"),
      value: report?.run_id || "-",
    },
    {
      label: t("simLab.result.summary.mode"),
      value: report ? modeValue : "-",
    },
    {
      label: t("simLab.result.summary.wallTime"),
      value: report ? formatSeconds(report.wall_time_s) : "-",
    },
    {
      label: t("simLab.result.summary.success"),
      value: report
        ? `${Number(report.project_success || 0)}/${Number(report.project_total || 0)}`
        : "-",
    },
    {
      label: t("simLab.result.summary.failed"),
      value: report ? String(Number(report.project_failed || 0)) : "-",
    },
    {
      label: t("simLab.result.summary.startedAt"),
      value: report ? formatIsoTime(report.started_at) : "-",
    },
  ];

  summaryRows.forEach((row) => {
    const card = document.createElement("div");
    card.className = "simlab-summary-card";

    const label = document.createElement("div");
    label.className = "simlab-summary-label";
    label.textContent = row.label;

    const value = document.createElement("div");
    value.className = "simlab-summary-value";
    value.textContent = row.value;

    card.appendChild(label);
    card.appendChild(value);
    elements.simLabSummaryGrid.appendChild(card);
  });
};

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

  list.forEach((project, index) => {
    const row = document.createElement("details");
    row.className = "simlab-report-item";
    if (index === 0) {
      row.open = true;
    }

    const summary = document.createElement("summary");
    summary.className = "simlab-report-summary";

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

    right.appendChild(duration);
    right.appendChild(status);

    summary.appendChild(left);
    summary.appendChild(right);
    row.appendChild(summary);

    const content = document.createElement("div");
    content.className = "simlab-report-content";

    const hint = document.createElement("div");
    hint.className = "simlab-expand-hint";
    hint.textContent = t("simLab.result.expandHint");
    content.appendChild(hint);

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

    if (project.report) {
      const raw = document.createElement("details");
      raw.className = "simlab-project-raw";
      const rawSummary = document.createElement("summary");
      rawSummary.textContent = t("simLab.result.projectRawToggle");
      const rawPre = document.createElement("pre");
      rawPre.className = "code-preview";
      rawPre.textContent = JSON.stringify(project.report, null, 2);
      raw.appendChild(rawSummary);
      raw.appendChild(rawPre);
      content.appendChild(raw);
    }

    row.appendChild(content);
    elements.simLabReportList.appendChild(row);
  });
};

const renderReport = (report) => {
  lastReport = report || null;
  renderSummary(lastReport);
  renderProjectReports(lastReport?.projects || []);
  if (elements.simLabResult) {
    elements.simLabResult.textContent = lastReport
      ? JSON.stringify(lastReport, null, 2)
      : t("simLab.result.rawEmpty");
  }
};

const buildRunPayload = (runId) => {
  const projects = selectedProjects();
  if (!projects.length) {
    throw new Error(t("simLab.error.noProject"));
  }
  return {
    run_id: runId,
    projects,
    mode: elements.simLabMode?.value || "parallel",
    keep_artifacts: Boolean(elements.simLabKeepArtifacts?.checked),
    options: {
      swarm_flow: {
        workers: Math.max(1, Math.floor(numberValue(elements.simLabWorkers, 100))),
        max_wait_s: Math.max(10, Math.floor(numberValue(elements.simLabMaxWait, 180))),
        mother_wait_s: Math.max(1, numberValue(elements.simLabMotherWait, 30)),
        poll_ms: Math.max(40, Math.floor(numberValue(elements.simLabPollMs, 120))),
        keep_artifacts: Boolean(elements.simLabKeepArtifacts?.checked),
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
      elements.simLabRunBtn.addEventListener("click", runSimulation);
    }
    if (elements.simLabStopBtn) {
      elements.simLabStopBtn.addEventListener("click", stopSimulation);
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
      updateRunButton();
    });
    updateRunButton();
    renderReport(null);
  }
  await reconcileRunningState();
  await fetchProjects();
  await reconcileRunningState();
};
