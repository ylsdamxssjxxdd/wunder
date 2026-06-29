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
  profiles: [],
  history: [],
  activeRunId: "",
  activeStatus: "idle",
  viewRunId: "",
  viewDetail: null,
  selectedProfileId: "full",
  progress: { completedAttempts: 0, totalAttempts: 0, currentTaskId: "", hint: "" },
  refreshTimer: null,
  elapsedTimer: null,
  pollTimer: null,
  pollRunId: "",
  selectedAttemptKey: "",
  catalogLoaded: false,
  actionPending: false,
  cancelPending: false,
  exportPending: false,
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
  const normalized = raw.replace(/[\s-]+/g, "_");
  if (normalized === "embedding" || normalized === "embed" || normalized === "emb" || normalized === "embeddings") {
    return "embedding";
  }
  if (
    normalized === "asr" ||
    normalized === "stt" ||
    normalized === "speech_to_text" ||
    normalized === "speech2text" ||
    normalized === "audio_transcription" ||
    normalized === "transcription" ||
    normalized === "audio_to_text"
  ) {
    return "asr";
  }
  if (
    normalized === "tts" ||
    normalized === "speech" ||
    normalized === "text_to_speech" ||
    normalized === "text2speech" ||
    normalized === "audio_speech"
  ) {
    return "tts";
  }
  if (
    normalized === "image" ||
    normalized === "draw" ||
    normalized === "drawing" ||
    normalized === "text_to_image" ||
    normalized === "text2image" ||
    normalized === "image_generation"
  ) {
    return "image";
  }
  return "llm";
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

function formatPercentScore(value) {
  return Number.isFinite(Number(value)) ? `${Math.round(Number(value) * 100)}%` : "-";
}

function formatReadiness(value) {
  const normalized = String(value || "").trim().toLowerCase();
  return {
    production_ready: "生产可用",
    usable: "可用",
    risky: "有风险",
    not_ready: "暂不可用",
  }[normalized] || "-";
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

function parseDownloadFilename(disposition, fallback) {
  const raw = String(disposition || "");
  const utf8Match = raw.match(/filename\*=UTF-8''([^;]+)/i);
  if (utf8Match?.[1]) {
    try {
      return decodeURIComponent(utf8Match[1]);
    } catch (_) {
      return utf8Match[1];
    }
  }
  const asciiMatch = raw.match(/filename="([^"]+)"/i) || raw.match(/filename=([^;]+)/i);
  return String(asciiMatch?.[1] || fallback || "wunderbench-export.json").trim();
}

function downloadBlob(blob, filename) {
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = filename || "wunderbench-export.json";
  document.body.appendChild(link);
  link.click();
  link.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
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
    exportBtn: panel.querySelector("#benchmarkExportBtn"),
    statusIndicator: panel.querySelector("#benchmarkStatusIndicator"),
    userId: panel.querySelector("#benchmarkUserId"),
    profileList: panel.querySelector("#benchmarkProfileList"),
    modelSelect: panel.querySelector("#benchmarkModelSelect"),
    judgeModelSelect: panel.querySelector("#benchmarkJudgeModelSelect"),
    formStatus: panel.querySelector("#benchmarkFormStatus"),
    summaryText: panel.querySelector("#benchmarkSummaryText"),
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
  updateExportAction();
}

function updateExportAction() {
  const button = benchmarkState.refs?.exportBtn;
  if (!button) {
    return;
  }
  const label = button.querySelector("span");
  const runId = String(benchmarkState.viewRunId || benchmarkState.activeRunId || "").trim();
  button.disabled = benchmarkState.exportPending || !runId;
  if (label) {
    label.textContent = benchmarkState.exportPending ? "导出中..." : "导出评测记录";
  }
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

function renderProfileOptions() {
  const list = benchmarkState.refs?.profileList;
  if (!list) {
    return;
  }
  list.textContent = "";
  const profiles = benchmarkState.profiles.length
    ? benchmarkState.profiles
    : [
        { id: "full", name: "Full Suite", task_count: 0, recommended_runs: 2 },
      ];
  const fallback = profiles.find((profile) => profile.default)?.id || profiles[0]?.id || "full";
  if (!profiles.some((profile) => profile.id === benchmarkState.selectedProfileId)) {
    benchmarkState.selectedProfileId = fallback;
  }
  profiles.forEach((profile) => {
    const profileId = String(profile.id || "");
    const button = document.createElement("button");
    button.type = "button";
    button.className = "benchmark-profile-option";
    button.dataset.profileId = profileId;
    button.classList.toggle("is-active", profileId === benchmarkState.selectedProfileId);
    const taskText = Number(profile.task_count) > 0 ? `${profile.task_count} 题` : "自动题集";
    const runsText = Number(profile.recommended_runs) > 0 ? `${profile.recommended_runs} 轮` : "推荐轮次";
    button.innerHTML = `
      <strong>${formatProfileName(profileId, profile.name)}</strong>
      <span>${taskText} · ${runsText}</span>
      <small>${formatProfileDescription(profileId, profile.description)}</small>
    `;
    list.appendChild(button);
  });
}

function formatProfileName(profileId, fallback) {
  return {
    quick: "全量",
    core: "全量",
    standard: "全量",
    full: "全量",
  }[profileId] || fallback || profileId || "-";
}

function formatProfileDescription(profileId, fallback) {
  return {
    quick: "历史档位已归一为全量题库",
    core: "历史档位已归一为全量题库",
    standard: "历史档位已归一为全量题库",
    full: "运行全部可用题目，适合发布前确认和模型对比",
  }[profileId] || fallback || "";
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
        <button type="button" class="icon-btn" data-action="export" data-run-id="${run.run_id}" title="导出评测记录"><i class="fa-solid fa-file-export"></i></button>
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

  if (refs.summaryText) {
    refs.summaryText.textContent = "暂无运行。WunderBench 将运行全量题库。";
  }
  refs.progressFill.style.width = "0%";
  refs.progressText.textContent = "0 / 0";
  refs.currentTask.textContent = "";
  refs.runHint.textContent = "";
  refs.attemptBody.textContent = "";
  refs.attemptEmpty.style.display = "block";
  updateExportAction();
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

function buildSummaryText(run = {}, summary = {}, scorecard = {}, efficiency = {}, attempts = []) {
  const weakestSuites = Array.isArray(scorecard.weakest_suites) ? scorecard.weakest_suites : [];
  const topFailures = Array.isArray(scorecard.top_failures) ? scorecard.top_failures : [];
  const lines = [
    `Run ID：${run.run_id || "-"}`,
    `档位：${formatProfileName(String(run.profile || summary.profile || ""), run.profile || summary.profile)}`,
    `状态：${run.status || "-"}    开始：${formatDateTime(Number(run.started_time))}    耗时：${formatDuration(Number(run.elapsed_s))}`,
    `总分：${formatScore(run.total_score ?? summary.total_score)}    结论：${formatReadiness(scorecard.readiness)}`,
    `可靠性：${formatPercentScore(scorecard.reliability_score)}    工具成功率：${formatPercentScore(scorecard.tool_success_score)}    稳定性：${formatPercentScore(scorecard.stability_score)}    效率：${formatPercentScore(scorecard.efficiency_score)}`,
    `上下文 Token：${formatContextTokens(efficiency.total_context_tokens)}    任务：${Number(summary.task_count || run.task_count || 0)}    Attempt：${attempts.length || Number(summary.attempt_count || run.attempt_count || 0)}`,
  ];
  if (weakestSuites.length) {
    lines.push(`薄弱任务组：${weakestSuites.map((item) => `${item.suite || "-"} ${formatScore(item.mean_score)}`).join(" / ")}`);
  }
  if (topFailures.length) {
    lines.push(`需复查任务：${topFailures.map((item) => `${item.task_id || "-"} ${formatScore(item.score)}`).join(" / ")}`);
  }
  return lines.join("\n");
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
  const scorecard = summary.scorecard || {};
  const viewingActiveRun = Boolean(run.run_id) && run.run_id === benchmarkState.activeRunId;
  const trackedTotalAttempts = viewingActiveRun ? Number(benchmarkState.progress.totalAttempts) || 0 : 0;
  const trackedCompletedAttempts = viewingActiveRun ? Number(benchmarkState.progress.completedAttempts) || 0 : 0;
  const totalAttempts = trackedTotalAttempts || Number(run.attempt_count) || Number(summary.attempt_count) || attempts.length;
  const completedAttempts = viewingActiveRun ? Math.max(trackedCompletedAttempts, attempts.length) : attempts.length;
  const ratio = totalAttempts > 0 ? Math.min(1, completedAttempts / totalAttempts) : 0;

  if (refs.summaryText) {
    refs.summaryText.textContent = buildSummaryText(run, summary, scorecard, efficiency, attempts);
  }
  refs.progressFill.style.width = `${Math.round(ratio * 100)}%`;
  refs.progressText.textContent = `${completedAttempts} / ${totalAttempts || attempts.length}`;
  refs.currentTask.textContent = viewingActiveRun && benchmarkState.progress.currentTaskId ? `\u5f53\u524d\u4efb\u52a1\uff1a${benchmarkState.progress.currentTaskId}` : "";
  refs.runHint.textContent = viewingActiveRun ? benchmarkState.progress.hint || "" : "";
  renderAttempts(attempts);
  updateIndicator(run.status || benchmarkState.activeStatus);
  updateExportAction();
}

function refreshElapsedClock() {
  clearElapsedClock();

  const run = benchmarkState.viewDetail?.run;
  if (!run || !Number.isFinite(Number(run.started_time)) || isFinishedStatus(run.status)) {
    return;
  }

  benchmarkState.elapsedTimer = window.setInterval(() => {
    if (!benchmarkState.refs?.summaryText) {
      clearElapsedClock();
      return;
    }
    const detail = benchmarkState.viewDetail || {};
    const currentRun = detail.run || run;
    const summary = currentRun.summary || {};
    const displayRun = {
      ...currentRun,
      elapsed_s: Math.max(0, Date.now() / 1000 - Number(currentRun.started_time)),
    };
    benchmarkState.refs.summaryText.textContent = buildSummaryText(
      displayRun,
      summary,
      summary.scorecard || {},
      summary.efficiency || {},
      Array.isArray(detail.attempts) ? detail.attempts : []
    );
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

  const payload = await fetchJson("/admin/wunderbench/profiles");
  benchmarkState.profiles = Array.isArray(payload.profiles) ? payload.profiles : [];

  benchmarkState.catalogLoaded = true;
  renderProfileOptions();
  updatePrimaryAction();
  setFormStatus("WunderBench 现在统一运行全量题库");
}

async function loadHistory() {
  const payload = await fetchJson("/admin/wunderbench/runs");
  benchmarkState.history = Array.isArray(payload.runs) ? payload.runs : [];
  renderHistory();
  return benchmarkState.history;
}

async function loadRunDetail(runId, options = {}) {
  if (!runId) {
    return null;
  }

  const payload = await fetchJson(`/admin/wunderbench/runs/${encodeURIComponent(runId)}`);
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

  if (refs.userId && !String(refs.userId.value || "").trim()) {
    refs.userId.value = userId;
  }

  return {
    user_id: userId,
    profile: "full",
    model_name: String(refs.modelSelect?.value || "").trim() || undefined,
    judge_model_name: String(refs.judgeModelSelect?.value || "").trim() || undefined,
    capture_artifacts: true,
    capture_transcript: true,
  };
}

async function startBenchmark() {
  if (isRunning()) {
    throw new Error("当前已有运行中的 WunderBench，请先停止或等待完成");
  }

  benchmarkState.actionPending = true;
  benchmarkState.cancelPending = false;
  updatePrimaryAction();
  setFormStatus("正在启动 WunderBench...");

  try {
    const response = await fetchJson("/admin/wunderbench/start", {
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
    setFormStatus("WunderBench 已启动");
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
    const response = await fetchJson(`/admin/wunderbench/runs/${encodeURIComponent(runId)}/cancel`, { method: "POST" });
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

  await fetchJson(`/admin/wunderbench/runs/${encodeURIComponent(runId)}`, { method: "DELETE" });

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

async function exportBenchmarkRun(runId) {
  const cleaned = String(runId || benchmarkState.viewRunId || benchmarkState.activeRunId || "").trim();
  if (!cleaned) {
    throw new Error("请先选择一条评测记录");
  }
  benchmarkState.exportPending = true;
  updateExportAction();
  setFormStatus("正在导出评测记录...");
  try {
    const response = await fetch(buildApiUrl(`/admin/wunderbench/runs/${encodeURIComponent(cleaned)}/export`));
    if (!response.ok) {
      const payload = await response.json().catch(() => ({}));
      throw new Error(payload?.error?.message || payload?.detail?.message || `HTTP ${response.status}`);
    }
    const blob = await response.blob();
    const filename = parseDownloadFilename(
      response.headers.get("Content-Disposition"),
      `wunderbench-${cleaned}-export.json`
    );
    downloadBlob(blob, filename);
    setFormStatus(`已导出评测记录 ${cleaned}`);
  } finally {
    benchmarkState.exportPending = false;
    updateExportAction();
  }
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

  refs.exportBtn?.addEventListener("click", () => {
    exportBenchmarkRun().catch((error) => {
      setFormStatus(error.message || "导出评测记录失败");
    });
  });

  refs.profileList?.addEventListener("click", (event) => {
    const button = event.target instanceof Element ? event.target.closest("button[data-profile-id]") : null;
    if (!button) {
      return;
    }
    benchmarkState.selectedProfileId = String(button.dataset.profileId || "full") || "full";
    renderProfileOptions();
    const selected = benchmarkState.profiles.find((profile) => profile.id === benchmarkState.selectedProfileId);
    setFormStatus(`当前档位：${formatProfileName(benchmarkState.selectedProfileId, selected?.name)}`);
  });

  refs.historyBody?.addEventListener("click", (event) => {
    const target = event.target instanceof Element ? event.target : null;
    const button = target?.closest("button[data-action]");
    if (button) {
      const action = button.dataset.action;
      const runId = button.dataset.runId;
      if (action === "export") {
        exportBenchmarkRun(runId).catch((error) => setFormStatus(error.message || "导出评测记录失败"));
      } else if (action === "delete") {
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
  renderProfileOptions();
  clearDetail();
  updateIndicator(benchmarkState.activeStatus);
  updatePrimaryAction();
  updateExportAction();
  setFormStatus("正在加载评测档位...");

  try {
    await Promise.all([loadCatalog(), loadHistory()]);
    const initial = benchmarkState.history[0];
    if (initial?.run_id) {
      await loadRunDetail(initial.run_id, { followRunning: false, silent: true });
    }
  } catch (error) {
    benchmarkState.catalogLoaded = false;
    updatePrimaryAction();
    setFormStatus(error.message || "初始化 WunderBench 面板失败");
  }
}
