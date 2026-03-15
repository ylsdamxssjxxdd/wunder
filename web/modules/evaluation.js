import { elements } from "./elements.js?v=20260215-01";
import { getWunderBase } from "./api.js";
import { formatDuration } from "./utils.js";
import { ensureLlmConfigLoaded } from "./llm.js";
import { state } from "./state.js";
import { getCurrentLanguage } from "./i18n.js?v=20260215-01";

const DEFAULT_USER_ID = "benchmark_admin";
const RUN_POLL_INTERVAL_MS = 2500;

const benchmarkState = {
  initialized: false,
  refs: null,
  suites: [],
  history: [],
  activeRunId: "",
  activeStatus: "idle",
  viewRunId: "",
  viewDetail: null,
  selectedSuiteIds: new Set(),
  suiteSelectionTouched: false,
  progress: { completedAttempts: 0, totalAttempts: 0, currentTaskId: "", hint: "" },
  refreshTimer: null,
  elapsedTimer: null,
  pollTimer: null,
  pollRunId: "",
  selectedAttemptKey: "",
  catalogLoaded: false,
  actionPending: false,
  cancelPending: false,
};

function isFinishedStatus(status) {
  return ["finished", "cancelled", "error", "failed"].includes(String(status || "").trim().toLowerCase());
}

function getControllableRunId() {
  if (benchmarkState.activeRunId && !isFinishedStatus(benchmarkState.activeStatus)) {
    return benchmarkState.activeRunId;
  }
  const viewedStatus = String(benchmarkState.viewDetail?.run?.status || "").trim().toLowerCase();
  if (benchmarkState.viewRunId && viewedStatus === "running") {
    return benchmarkState.viewRunId;
  }
  return "";
}

function isRunning() {
  return Boolean(getControllableRunId());
}

function getDefaultModelLabel() {
  const defaultName = String(state.llm.defaultName || "").trim();
  return defaultName ? `\u9ed8\u8ba4\u6a21\u578b\uff08${defaultName}\uff09` : "\u9ed8\u8ba4\u6a21\u578b";
}

function normalizeModelType(value) {
  const raw = String(value || "").trim().toLowerCase();
  if (!raw) {
    return "llm";
  }
  return raw === "embedding" || raw === "embed" || raw === "embeddings" ? "embedding" : "llm";
}

function getLlmModelNames() {
  return state.llm.order.filter((name) => normalizeModelType(state.llm.configs?.[name]?.model_type) === "llm");
}

function formatDateTime(value) {
  if (!Number.isFinite(value)) {
    return "-";
  }
  const date = new Date(value * 1000);
  return Number.isNaN(date.getTime()) ? "-" : date.toLocaleString(getCurrentLanguage());
}

function formatScore(value) {
  return Number.isFinite(Number(value)) ? Number(value).toFixed(3) : "-";
}

function formatInteger(value) {
  return Number.isFinite(Number(value)) ? String(Math.round(Number(value))) : "-";
}

function trimDecimalText(value) {
  return String(value).replace(/\.0+$/, "").replace(/(\.\d*[1-9])0+$/, "$1");
}

function formatContextTokens(value) {
  const amount = Number(value);
  if (!Number.isFinite(amount) || amount < 0) {
    return "-";
  }
  if (amount >= 1_000_000) {
    const digits = amount >= 10_000_000 ? 0 : 1;
    return `${trimDecimalText((amount / 1_000_000).toFixed(digits))}M`;
  }
  const digits = amount >= 100_000 ? 0 : 1;
  return `${trimDecimalText((amount / 1_000).toFixed(digits))}k`;
}

function buildAttemptKey(attempt) {
  return `${attempt.task_id || ""}#${attempt.attempt_no || 0}`;
}

function apiBase() {
  return getWunderBase();
}

function buildApiUrl(path, params = {}) {
  const search = new URLSearchParams();
  Object.entries(params).forEach(([key, value]) => {
    if (value === undefined || value === null || value === "") {
      return;
    }
    search.set(key, String(value));
  });
  const query = search.toString();
  return `${apiBase()}${path}${query ? `?${query}` : ""}`;
}

async function fetchJson(path, options = {}) {
  const response = await fetch(buildApiUrl(path), {
    headers: { "Content-Type": "application/json", ...(options.headers || {}) },
    ...options,
  });
  const payload = await response.json().catch(() => ({}));
  if (!response.ok) {
    throw new Error(payload?.error?.message || payload?.detail?.message || `HTTP ${response.status}`);
  }
  return payload;
}

function cacheRefs() {
  const panel = elements.evaluationPanel;
  if (!panel) {
    return null;
  }
  return {
    panel,
    startBtn: panel.querySelector("#benchmarkStartBtn"),
    historyBtn: panel.querySelector("#benchmarkHistoryBtn"),
    statusIndicator: panel.querySelector("#benchmarkStatusIndicator"),
    userId: panel.querySelector("#benchmarkUserId"),
    modelSelect: panel.querySelector("#benchmarkModelSelect"),
    judgeModelSelect: panel.querySelector("#benchmarkJudgeModelSelect"),
    runsPerTask: panel.querySelector("#benchmarkRunsPerTask"),
    captureArtifacts: panel.querySelector("#benchmarkCaptureArtifacts"),
    captureTranscript: panel.querySelector("#benchmarkCaptureTranscript"),
    suiteList: panel.querySelector("#benchmarkSuiteList"),
    formStatus: panel.querySelector("#benchmarkFormStatus"),
    runId: panel.querySelector("#benchmarkRunId"),
    runStatus: panel.querySelector("#benchmarkRunStatus"),
    startedAt: panel.querySelector("#benchmarkRunStartedAt"),
    elapsed: panel.querySelector("#benchmarkRunElapsed"),
    totalScore: panel.querySelector("#benchmarkTotalScore"),
    contextTokens: panel.querySelector("#benchmarkContextTokens"),
    progressFill: panel.querySelector("#benchmarkProgressFill"),
    progressText: panel.querySelector("#benchmarkProgressText"),
    currentTask: panel.querySelector("#benchmarkCurrentTask"),
    runHint: panel.querySelector("#benchmarkRunHint"),
    historyBody: panel.querySelector("#benchmarkHistoryBody"),
    historyEmpty: panel.querySelector("#benchmarkHistoryEmpty"),
    historyModal: panel.querySelector("#benchmarkHistoryModal"),
    historyModalClose: panel.querySelector("#benchmarkHistoryModalClose"),
    historyModalOk: panel.querySelector("#benchmarkHistoryModalOk"),
    attemptBody: panel.querySelector("#benchmarkAttemptBody"),
    attemptEmpty: panel.querySelector("#benchmarkAttemptEmpty"),
    detailModal: panel.querySelector("#benchmarkDetailModal"),
    detailModalTitle: panel.querySelector("#benchmarkDetailModalTitle"),
    detailModalClose: panel.querySelector("#benchmarkDetailModalClose"),
    detailModalOk: panel.querySelector("#benchmarkDetailModalOk"),
    detailPre: panel.querySelector("#benchmarkDetailPre"),
  };
}

function setFormStatus(message) {
  if (benchmarkState.refs?.formStatus) {
    benchmarkState.refs.formStatus.textContent = String(message || "");
  }
}

function setRunHint(message) {
  benchmarkState.progress.hint = String(message || "");
  if (benchmarkState.refs?.runHint) {
    benchmarkState.refs.runHint.textContent = benchmarkState.progress.hint;
  }
}

function updateIndicator(status) {
  const normalized = String(status || "idle").trim().toLowerCase() || "idle";
  const indicator = benchmarkState.refs?.statusIndicator;
  if (!indicator) {
    return;
  }
  const text = indicator.querySelector(".status-text");
  indicator.dataset.status = normalized;
  if (text) {
    text.textContent = {
      idle: "\u7a7a\u95f2",
      running: "\u8fd0\u884c\u4e2d",
      finished: "\u5df2\u5b8c\u6210",
      cancelled: "\u5df2\u53d6\u6d88",
      error: "\u5f02\u5e38",
      failed: "\u5931\u8d25",
    }[normalized] || normalized;
  }
}

function updatePrimaryAction() {
  const button = benchmarkState.refs?.startBtn;
  if (!button) {
    return;
  }
  const icon = button.querySelector("i");
  const label = button.querySelector("span");
  const running = isRunning();
  const disabled = benchmarkState.actionPending || (!running && !benchmarkState.catalogLoaded) || benchmarkState.cancelPending;

  button.disabled = disabled;
  button.classList.toggle("secondary", running);

  if (icon) {
    icon.className = running ? "fa-solid fa-stop" : "fa-solid fa-play";
  }
  if (label) {
    if (running) {
      label.textContent = benchmarkState.cancelPending ? "\u505c\u6b62\u4e2d..." : "\u505c\u6b62\u8fd0\u884c";
    } else {
      label.textContent = benchmarkState.catalogLoaded ? "\u5f00\u59cb\u8fd0\u884c" : "\u52a0\u8f7d\u4efb\u52a1\u7ec4\u4e2d...";
    }
  }
}

function getSelectedSuiteIds() {
  const ids = Array.from(benchmarkState.selectedSuiteIds).filter(Boolean);
  if (!benchmarkState.suiteSelectionTouched) {
    return benchmarkState.suites.map((suite) => suite.suite_id).filter(Boolean);
  }
  return ids;
}

function renderModelOptions() {
  const refs = benchmarkState.refs;
  if (!refs?.modelSelect || !refs?.judgeModelSelect) {
    return;
  }
  const modelNames = getLlmModelNames();

  [refs.modelSelect, refs.judgeModelSelect].forEach((select) => {
    const current = String(select.value || "").trim();
    select.textContent = "";

    const defaultOption = document.createElement("option");
    defaultOption.value = "";
    defaultOption.textContent = getDefaultModelLabel();
    select.appendChild(defaultOption);

    modelNames.forEach((name) => {
      const option = document.createElement("option");
      option.value = name;
      option.textContent = name;
      select.appendChild(option);
    });

    if (current && modelNames.includes(current)) {
      select.value = current;
    }
  });
}

function buildSuiteSummary(suite) {
  const taskCount = Number(suite.task_count) || 0;
  const recommendedRuns = Number(suite.recommended_runs) || 0;
  const categoryNames = Object.keys(suite.categories || {}).filter(Boolean);
  const compactCategories = categoryNames.slice(0, 3).join(" / ");
  const parts = [`${taskCount} \u4e2a\u4efb\u52a1`, `\u63a8\u8350 ${recommendedRuns || 1} \u8f6e`];
  if (compactCategories) {
    parts.push(compactCategories);
  }
  return parts.join(" \u00b7 ");
}

function renderSuiteList() {
  const refs = benchmarkState.refs;
  if (!refs?.suiteList) {
    return;
  }

  refs.suiteList.textContent = "";
  if (!benchmarkState.suites.length) {
    refs.suiteList.textContent = "\u6682\u65e0\u53ef\u7528\u4efb\u52a1\u7ec4";
    return;
  }

  benchmarkState.suites.forEach((suite) => {
    const checked = benchmarkState.suiteSelectionTouched
      ? benchmarkState.selectedSuiteIds.has(suite.suite_id)
      : true;
    const item = document.createElement("label");
    item.className = "benchmark-check-item";
    item.innerHTML = `
      <input type="checkbox" data-suite-id="${suite.suite_id}" ${checked ? "checked" : ""} />
      <div>
        <strong>${suite.suite_id}</strong>
        <span>${buildSuiteSummary(suite)}</span>
      </div>
    `;
    refs.suiteList.appendChild(item);
  });
}

function renderHistory() {
  const refs = benchmarkState.refs;
  if (!refs?.historyBody || !refs?.historyEmpty) {
    return;
  }

  refs.historyBody.textContent = "";
  refs.historyEmpty.style.display = benchmarkState.history.length ? "none" : "block";

  benchmarkState.history.forEach((run) => {
    const row = document.createElement("tr");
    row.dataset.runId = String(run.run_id || "");
    if (run.run_id === benchmarkState.viewRunId) {
      row.classList.add("is-active");
    }
    row.innerHTML = `
      <td>${formatDateTime(Number(run.started_time))}</td>
      <td><span class="benchmark-status-pill" data-status="${run.status || "idle"}">${run.status || "-"}</span></td>
      <td>${formatScore(run.total_score)}</td>
      <td>${run.model_name || "\u9ed8\u8ba4"}</td>
      <td class="benchmark-row-actions">
        <button type="button" class="icon-btn" data-action="delete" data-run-id="${run.run_id}" title="\u5220\u9664"><i class="fa-solid fa-trash"></i></button>
      </td>
    `;
    refs.historyBody.appendChild(row);
  });
}

function stopRunPolling() {
  if (benchmarkState.pollTimer) {
    window.clearTimeout(benchmarkState.pollTimer);
    benchmarkState.pollTimer = null;
  }
  benchmarkState.pollRunId = "";
}

function scheduleRunPolling(runId) {
  stopRunPolling();
  if (!runId) {
    return;
  }
  benchmarkState.pollRunId = runId;
  benchmarkState.pollTimer = window.setTimeout(async () => {
    if (benchmarkState.pollRunId !== runId) {
      return;
    }
    try {
      await loadRunDetail(runId, { followRunning: true, silent: true });
    } catch (error) {
      setRunHint(error.message || "\u5237\u65b0\u8fd0\u884c\u72b6\u6001\u5931\u8d25");
    }
  }, RUN_POLL_INTERVAL_MS);
}

function clearElapsedClock() {
  if (benchmarkState.elapsedTimer) {
    window.clearInterval(benchmarkState.elapsedTimer);
    benchmarkState.elapsedTimer = null;
  }
}

function closeHistoryModal() {
  if (benchmarkState.refs?.historyModal) {
    benchmarkState.refs.historyModal.classList.remove("active");
  }
}

function openHistoryModal() {
  if (benchmarkState.refs?.historyModal) {
    benchmarkState.refs.historyModal.classList.add("active");
  }
}

function closeDetailModal() {
  if (benchmarkState.refs?.detailModal) {
    benchmarkState.refs.detailModal.classList.remove("active");
  }
}

function openDetailModal(title, content) {
  const refs = benchmarkState.refs;
  if (!refs?.detailModal || !refs?.detailPre) {
    return;
  }
  if (refs.detailModalTitle) {
    refs.detailModalTitle.textContent = title || "Attempt \u660e\u7ec6";
  }
  refs.detailPre.textContent = content || "\u6682\u65e0\u660e\u7ec6";
  refs.detailModal.classList.add("active");
}

function clearDetail() {
  const refs = benchmarkState.refs;
  if (!refs) {
    return;
  }

  [refs.runId, refs.runStatus, refs.startedAt, refs.elapsed, refs.totalScore, refs.contextTokens, refs.efficiency].forEach((node) => {
    if (node) {
      node.textContent = "-";
    }
  });
  refs.progressFill.style.width = "0%";
  refs.progressText.textContent = "0 / 0";
  refs.currentTask.textContent = "";
  refs.runHint.textContent = "";
  refs.attemptBody.textContent = "";
  refs.attemptEmpty.style.display = "block";
  if (refs.detailPre) {
    refs.detailPre.textContent = "\u6682\u65e0\u660e\u7ec6";
  }
  closeDetailModal();
  clearElapsedClock();
}

function buildAttemptDetailText(attempt) {
  return JSON.stringify(
    {
      task_id: attempt.task_id,
      attempt_no: attempt.attempt_no,
      status: attempt.status,
      final_score: attempt.final_score,
      elapsed_s: attempt.elapsed_s,
      usage: attempt.usage,
      automated: attempt.automated,
      judge: attempt.judge,
      transcript_summary: attempt.transcript_summary,
      final_answer: attempt.final_answer,
      artifacts: Array.isArray(attempt.artifacts) ? attempt.artifacts.map((item) => item.path) : [],
      error: attempt.error,
    },
    null,
    2
  );
}

function renderAttempts(attempts) {
  const refs = benchmarkState.refs;
  if (!refs?.attemptBody || !refs?.attemptEmpty) {
    return;
  }

  refs.attemptBody.textContent = "";
  refs.attemptEmpty.style.display = attempts.length ? "none" : "block";

  attempts.forEach((attempt) => {
    const key = buildAttemptKey(attempt);
    const row = document.createElement("tr");
    row.dataset.attemptKey = key;
    if (key === benchmarkState.selectedAttemptKey) {
      row.classList.add("is-active");
    }
    row.innerHTML = `
      <td>${attempt.task_id || "-"}</td>
      <td>${attempt.attempt_no || "-"}</td>
      <td><span class="benchmark-status-pill" data-status="${attempt.status || "idle"}">${attempt.status || "-"}</span></td>
      <td>${formatScore(attempt.final_score)}</td>
      <td>${formatDuration(Number(attempt.elapsed_s))}</td>
      <td>${Array.isArray(attempt.tool_calls) ? attempt.tool_calls.length : 0}</td>
    `;
    refs.attemptBody.appendChild(row);
  });
}

function renderRunDetail(detail) {
  const refs = benchmarkState.refs;
  if (!refs) {
    return;
  }

  const run = detail?.run || {};
  const attempts = Array.isArray(detail?.attempts) ? detail.attempts : [];
  const summary = run.summary || {};
  const efficiency = summary.efficiency || {};
  const viewingActiveRun = Boolean(run.run_id) && run.run_id === benchmarkState.activeRunId;
  const trackedTotalAttempts = viewingActiveRun ? Number(benchmarkState.progress.totalAttempts) || 0 : 0;
  const trackedCompletedAttempts = viewingActiveRun ? Number(benchmarkState.progress.completedAttempts) || 0 : 0;
  const totalAttempts = trackedTotalAttempts || Number(run.attempt_count) || Number(summary.attempt_count) || attempts.length;
  const completedAttempts = viewingActiveRun ? Math.max(trackedCompletedAttempts, attempts.length) : attempts.length;
  const ratio = totalAttempts > 0 ? Math.min(1, completedAttempts / totalAttempts) : 0;

  refs.runId.textContent = run.run_id || "-";
  refs.runStatus.textContent = run.status || "-";
  refs.startedAt.textContent = formatDateTime(Number(run.started_time));
  refs.elapsed.textContent = formatDuration(Number(run.elapsed_s));
  refs.totalScore.textContent = formatScore(run.total_score ?? summary.total_score);
  refs.contextTokens.textContent = formatContextTokens(efficiency.total_context_tokens);
  refs.progressFill.style.width = `${Math.round(ratio * 100)}%`;
  refs.progressText.textContent = `${completedAttempts} / ${totalAttempts || attempts.length}`;
  refs.currentTask.textContent = viewingActiveRun && benchmarkState.progress.currentTaskId ? `\u5f53\u524d\u4efb\u52a1\uff1a${benchmarkState.progress.currentTaskId}` : "";
  refs.runHint.textContent = viewingActiveRun ? benchmarkState.progress.hint || "" : "";
  renderAttempts(attempts);
  updateIndicator(run.status || benchmarkState.activeStatus);
}

function refreshElapsedClock() {
  clearElapsedClock();

  const run = benchmarkState.viewDetail?.run;
  if (!run || !Number.isFinite(Number(run.started_time)) || isFinishedStatus(run.status)) {
    return;
  }

  benchmarkState.elapsedTimer = window.setInterval(() => {
    if (!benchmarkState.refs?.elapsed) {
      clearElapsedClock();
      return;
    }
    benchmarkState.refs.elapsed.textContent = formatDuration(Math.max(0, Date.now() / 1000 - Number(run.started_time)));
  }, 1000);
}

function scheduleDetailRefresh(followRunning = false) {
  if (!benchmarkState.viewRunId) {
    return;
  }
  if (benchmarkState.refreshTimer) {
    window.clearTimeout(benchmarkState.refreshTimer);
  }
  benchmarkState.refreshTimer = window.setTimeout(() => {
    benchmarkState.refreshTimer = null;
    loadRunDetail(benchmarkState.viewRunId, { followRunning, silent: true }).catch((error) => {
      setRunHint(error.message || "\u5237\u65b0\u8be6\u60c5\u5931\u8d25");
    });
  }, 220);
}

async function loadCatalog() {
  benchmarkState.catalogLoaded = false;
  updatePrimaryAction();

  const payload = await fetchJson("/admin/benchmark/suites");
  benchmarkState.suites = Array.isArray(payload.suites) ? payload.suites : [];
  if (!benchmarkState.suiteSelectionTouched) {
    benchmarkState.selectedSuiteIds = new Set(benchmarkState.suites.map((suite) => suite.suite_id).filter(Boolean));
  }

  benchmarkState.catalogLoaded = true;
  renderSuiteList();
  updatePrimaryAction();
  setFormStatus(`\u5df2\u52a0\u8f7d ${benchmarkState.suites.length} \u7ec4\u4efb\u52a1`);
}

async function loadHistory() {
  const payload = await fetchJson("/admin/benchmark/runs");
  benchmarkState.history = Array.isArray(payload.runs) ? payload.runs : [];
  renderHistory();
  return benchmarkState.history;
}

async function loadRunDetail(runId, options = {}) {
  if (!runId) {
    return null;
  }

  const payload = await fetchJson(`/admin/benchmark/runs/${encodeURIComponent(runId)}`);
  benchmarkState.viewRunId = runId;
  benchmarkState.viewDetail = payload;

  const run = payload.run || {};
  const followRunning = Boolean(options.followRunning);
  benchmarkState.activeStatus = String(run.status || benchmarkState.activeStatus || "idle");

  if (run.status === "running") {
    benchmarkState.activeRunId = runId;
    if (followRunning) {
      scheduleRunPolling(runId);
    } else {
      stopRunPolling();
    }
  } else {
    if (benchmarkState.activeRunId === runId) {
      benchmarkState.activeRunId = "";
    }
    benchmarkState.cancelPending = false;
    stopRunPolling();
  }

  renderHistory();
  renderRunDetail(payload);
  refreshElapsedClock();
  updatePrimaryAction();
  updateIndicator(run.status || benchmarkState.activeStatus);

  if (run.status === "running" && !followRunning) {
    setRunHint("\u8fd9\u662f\u4e00\u6761\u8fd0\u884c\u4e2d\u8bb0\u5f55\uff1b\u82e5\u4e3a\u91cd\u542f\u524d\u6b8b\u7559\uff0c\u53ef\u4ee5\u76f4\u63a5\u70b9\u51fb\u505c\u6b62\u3002");
  } else if (!followRunning) {
    setRunHint("");
  }

  return payload;
}

function buildStartPayload() {
  const refs = benchmarkState.refs;
  const userId = String(refs.userId?.value || DEFAULT_USER_ID).trim() || DEFAULT_USER_ID;
  const suiteIds = getSelectedSuiteIds();

  if (!suiteIds.length) {
    throw new Error("\u8bf7\u81f3\u5c11\u9009\u62e9\u4e00\u7ec4\u4efb\u52a1");
  }

  if (refs.userId && !String(refs.userId.value || "").trim()) {
    refs.userId.value = userId;
  }

  return {
    user_id: userId,
    model_name: String(refs.modelSelect?.value || "").trim() || undefined,
    judge_model_name: String(refs.judgeModelSelect?.value || "").trim() || undefined,
    suite_ids: suiteIds,
    runs_per_task: Math.max(1, Math.min(10, Number(refs.runsPerTask?.value || 1) || 1)),
    capture_artifacts: Boolean(refs.captureArtifacts?.checked),
    capture_transcript: Boolean(refs.captureTranscript?.checked),
  };
}

async function startBenchmark() {
  if (isRunning()) {
    throw new Error("\u5f53\u524d\u5df2\u6709\u8fd0\u884c\u4e2d\u7684 benchmark\uff0c\u8bf7\u5148\u505c\u6b62\u6216\u7b49\u5f85\u5b8c\u6210");
  }

  benchmarkState.actionPending = true;
  benchmarkState.cancelPending = false;
  updatePrimaryAction();
  setFormStatus("\u6b63\u5728\u542f\u52a8 benchmark...");

  try {
    const response = await fetchJson("/admin/benchmark/start", {
      method: "POST",
      body: JSON.stringify(buildStartPayload()),
    });
    benchmarkState.activeRunId = response.run_id || "";
    benchmarkState.activeStatus = response.status || "running";
    benchmarkState.progress = {
      completedAttempts: 0,
      totalAttempts: Number(response.attempt_count) || 0,
      currentTaskId: "",
      hint: "",
    };
    updateIndicator(benchmarkState.activeStatus);
    setRunHint(`\u5df2\u521b\u5efa\u8fd0\u884c ${response.run_id}`);
    setFormStatus("Benchmark \u5df2\u542f\u52a8");
    await loadHistory();
    await loadRunDetail(benchmarkState.activeRunId, { followRunning: true });
  } finally {
    benchmarkState.actionPending = false;
    updatePrimaryAction();
  }
}

async function cancelBenchmark() {
  const runId = getControllableRunId();
  if (!runId) {
    throw new Error("\u5f53\u524d\u6ca1\u6709\u53ef\u505c\u6b62\u7684\u8fd0\u884c");
  }

  benchmarkState.actionPending = true;
  benchmarkState.cancelPending = true;
  updatePrimaryAction();
  setFormStatus("\u6b63\u5728\u53d1\u9001\u505c\u6b62\u8bf7\u6c42...");

  try {
    const response = await fetchJson(`/admin/benchmark/runs/${encodeURIComponent(runId)}/cancel`, { method: "POST" });
    setRunHint(response?.message || "\u505c\u6b62\u8bf7\u6c42\u5df2\u53d1\u9001");
    setFormStatus("\u505c\u6b62\u8bf7\u6c42\u5df2\u53d1\u9001");
    await loadRunDetail(runId, { followRunning: true, silent: true });
    await loadHistory();
  } catch (error) {
    const message = String(error?.message || "");
    if (message.toLowerCase().includes("run not found")) {
      setRunHint("\u8be5\u8fd0\u884c\u53ef\u80fd\u662f\u91cd\u542f\u524d\u6b8b\u7559\uff0c\u53ef\u76f4\u63a5\u5220\u9664\u5386\u53f2\u8bb0\u5f55\u3002");
    }
    throw error;
  } finally {
    benchmarkState.actionPending = false;
    benchmarkState.cancelPending = false;
    updatePrimaryAction();
  }
}

async function deleteBenchmarkRun(runId) {
  if (!runId) {
    return;
  }

  await fetchJson(`/admin/benchmark/runs/${encodeURIComponent(runId)}`, { method: "DELETE" });

  if (benchmarkState.viewRunId === runId) {
    benchmarkState.viewRunId = "";
    benchmarkState.viewDetail = null;
    benchmarkState.selectedAttemptKey = "";
    clearDetail();
  }
  if (benchmarkState.activeRunId === runId) {
    benchmarkState.activeRunId = "";
    benchmarkState.activeStatus = "idle";
    benchmarkState.cancelPending = false;
    stopRunPolling();
  }

  await loadHistory();
  updatePrimaryAction();
  updateIndicator(benchmarkState.activeStatus);
  setFormStatus(`\u5df2\u5220\u9664\u8fd0\u884c ${runId}`);
}

async function handlePrimaryAction() {
  if (isRunning()) {
    await cancelBenchmark();
  } else {
    await startBenchmark();
  }
}

function bindEvents() {
  const refs = benchmarkState.refs;
  if (!refs || benchmarkState.initialized) {
    return;
  }

  refs.startBtn?.addEventListener("click", () => {
    handlePrimaryAction().catch((error) => {
      benchmarkState.actionPending = false;
      benchmarkState.cancelPending = false;
      updatePrimaryAction();
      setFormStatus(error.message || "Benchmark \u64cd\u4f5c\u5931\u8d25");
    });
  });

  refs.historyBtn?.addEventListener("click", async () => {
    openHistoryModal();
    try {
      await loadHistory();
    } catch (error) {
      setFormStatus(error.message || "\u52a0\u8f7d\u5386\u53f2\u5931\u8d25");
    }
  });

  refs.suiteList?.addEventListener("change", (event) => {
    const input = event.target;
    if (!(input instanceof HTMLInputElement) || input.type !== "checkbox") {
      return;
    }
    const suiteId = String(input.dataset.suiteId || "");
    benchmarkState.suiteSelectionTouched = true;
    if (input.checked) {
      benchmarkState.selectedSuiteIds.add(suiteId);
    } else {
      benchmarkState.selectedSuiteIds.delete(suiteId);
    }
    setFormStatus(`\u5f53\u524d\u9009\u4e2d ${getSelectedSuiteIds().length} \u7ec4\u4efb\u52a1`);
  });

  refs.historyBody?.addEventListener("click", (event) => {
    const target = event.target instanceof Element ? event.target : null;
    const button = target?.closest("button[data-action]");
    if (button) {
      const action = button.dataset.action;
      const runId = button.dataset.runId;
      if (action === "delete") {
        deleteBenchmarkRun(runId).catch((error) => setFormStatus(error.message || "\u5220\u9664\u8fd0\u884c\u5931\u8d25"));
      }
      return;
    }
    const row = target?.closest("tr[data-run-id]");
    const runId = row?.dataset.runId;
    if (!runId) {
      return;
    }
    loadRunDetail(runId, { followRunning: false })
      .then(() => closeHistoryModal())
      .catch((error) => setFormStatus(error.message || "\u52a0\u8f7d\u8fd0\u884c\u8be6\u60c5\u5931\u8d25"));
  });

  refs.historyModalClose?.addEventListener("click", closeHistoryModal);
  refs.historyModalOk?.addEventListener("click", closeHistoryModal);
  refs.historyModal?.addEventListener("click", (event) => {
    if (event.target === refs.historyModal) {
      closeHistoryModal();
    }
  });

  refs.attemptBody?.addEventListener("click", (event) => {
    const row = event.target instanceof Element ? event.target.closest("tr[data-attempt-key]") : null;
    if (!row) {
      return;
    }
    const attempts = Array.isArray(benchmarkState.viewDetail?.attempts) ? benchmarkState.viewDetail.attempts : [];
    const key = String(row.dataset.attemptKey || "");
    const selected = attempts.find((attempt) => buildAttemptKey(attempt) === key);
    benchmarkState.selectedAttemptKey = key;
    renderAttempts(attempts);
    if (selected) {
      openDetailModal(
        `${selected.task_id || "attempt"} \u00b7 \u7b2c ${selected.attempt_no || 0} \u8f6e`,
        buildAttemptDetailText(selected)
      );
    }
  });

  refs.detailModalClose?.addEventListener("click", closeDetailModal);
  refs.detailModalOk?.addEventListener("click", closeDetailModal);
  refs.detailModal?.addEventListener("click", (event) => {
    if (event.target === refs.detailModal) {
      closeDetailModal();
    }
  });

  benchmarkState.initialized = true;
}

export async function initEvaluationPanel() {
  await ensureLlmConfigLoaded();
  benchmarkState.refs = cacheRefs();
  if (!benchmarkState.refs) {
    return;
  }

  if (!String(benchmarkState.refs.userId?.value || "").trim()) {
    benchmarkState.refs.userId.value = DEFAULT_USER_ID;
  }

  bindEvents();
  renderModelOptions();
  clearDetail();
  updateIndicator(benchmarkState.activeStatus);
  updatePrimaryAction();
  setFormStatus("\u6b63\u5728\u52a0\u8f7d\u4efb\u52a1\u7ec4...");

  try {
    await Promise.all([loadCatalog(), loadHistory()]);
    const initial = benchmarkState.history[0];
    if (initial?.run_id) {
      await loadRunDetail(initial.run_id, { followRunning: false, silent: true });
    }
  } catch (error) {
    benchmarkState.catalogLoaded = false;
    updatePrimaryAction();
    setFormStatus(error.message || "\u521d\u59cb\u5316 benchmark \u9762\u677f\u5931\u8d25");
  }
}
