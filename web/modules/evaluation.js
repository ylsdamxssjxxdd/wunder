import { elements } from "./elements.js?v=20260110-07";
import { openMonitorDetail } from "./monitor.js?v=20260110-08";
import { normalizeApiBase, formatDuration } from "./utils.js";
import { getCurrentLanguage, t } from "./i18n.js?v=20260110-08";

const evaluationState = {
  activeRunId: "",
  activeStatus: "",
  viewRunId: "",
  streaming: false,
  controller: null,
  cases: new Map(),
  caseOrder: [],
  caseInfo: new Map(),
};

const isFinishedStatus = (status) => ["finished", "failed", "cancelled"].includes(String(status || ""));
const MAX_CASE_LABEL = 180;

const truncateText = (value, maxLen) => {
  const text = String(value || "").trim();
  if (!text) {
    return "";
  }
  if (text.length <= maxLen) {
    return text;
  }
  return `${text.slice(0, maxLen - 1)}…`;
};

const formatEpochSeconds = (value) => {
  if (!Number.isFinite(value)) {
    return "-";
  }
  const date = new Date(value * 1000);
  if (Number.isNaN(date.getTime())) {
    return "-";
  }
  return date.toLocaleString(getCurrentLanguage());
};

const formatScore = (value) => {
  if (!Number.isFinite(value)) {
    return "-";
  }
  return value.toFixed(1);
};

const setFormStatus = (message) => {
  if (!elements.evaluationFormStatus) {
    return;
  }
  elements.evaluationFormStatus.textContent = message || "";
};

const setRunHint = (message) => {
  if (!elements.evaluationRunHint) {
    return;
  }
  elements.evaluationRunHint.textContent = message || "";
};

const isActiveRunning = () => {
  if (!evaluationState.activeRunId) {
    return false;
  }
  const status = String(evaluationState.activeStatus || "running");
  return !["finished", "failed", "cancelled"].includes(status);
};

const isViewingActive = () =>
  !evaluationState.viewRunId || evaluationState.viewRunId === evaluationState.activeRunId;

const updateActionButton = () => {
  if (!elements.evaluationActionBtn) {
    return;
  }
  const icon = elements.evaluationActionBtn.querySelector("i");
  const label = elements.evaluationActionBtn.querySelector("span");
  if (isActiveRunning()) {
    elements.evaluationActionBtn.classList.add("secondary");
    if (icon) {
      icon.className = "fa-solid fa-stop";
    }
    if (label) {
      label.textContent = t("evaluation.action.cancel");
    }
  } else {
    elements.evaluationActionBtn.classList.remove("secondary");
    if (icon) {
      icon.className = "fa-solid fa-play";
    }
    if (label) {
      label.textContent = t("evaluation.action.start");
    }
  }
};

const updateRunSpinner = () => {
  if (!elements.evaluationRunSpinner) {
    return;
  }
  const active = isActiveRunning() && isViewingActive();
  elements.evaluationRunSpinner.classList.toggle("active", active);
  elements.evaluationRunSpinner.setAttribute("aria-hidden", active ? "false" : "true");
};

const updateCurrentCaseDisplay = () => {
  if (!elements.evaluationCurrentCase) {
    return;
  }
  if (!isActiveRunning() || !isViewingActive()) {
    elements.evaluationCurrentCase.textContent = "";
    return;
  }
  const order =
    evaluationState.caseOrder.length > 0
      ? evaluationState.caseOrder
      : Array.from(evaluationState.cases.keys());
  let currentCase = "";
  for (const caseId of order) {
    const info = evaluationState.caseInfo.get(caseId);
    const status = String(info?.status || "").trim();
    if (!status || status === "active") {
      currentCase = caseId;
      break;
    }
  }
  if (!currentCase) {
    elements.evaluationCurrentCase.textContent = "";
    return;
  }
  const info = evaluationState.caseInfo.get(currentCase);
  const prompt = truncateText(info?.prompt, MAX_CASE_LABEL);
  const label = prompt ? `${currentCase} - ${prompt}` : currentCase;
  elements.evaluationCurrentCase.textContent = t("evaluation.currentCase", { case: label });
};

const updateProgressAnimation = () => {
  if (!elements.evaluationProgressFill) {
    return;
  }
  const active = isActiveRunning() && isViewingActive();
  elements.evaluationProgressFill.classList.toggle("active", active);
};

const updateRunHint = () => {
  setRunHint("");
  updateRunSpinner();
  updateProgressAnimation();
  updateCurrentCaseDisplay();
};

const resetRunSummary = () => {
  if (elements.evaluationRunId) {
    elements.evaluationRunId.textContent = "-";
  }
  if (elements.evaluationRunStatus) {
    elements.evaluationRunStatus.textContent = "-";
    elements.evaluationRunStatus.className = "monitor-status";
  }
  if (elements.evaluationRunStartedAt) {
    elements.evaluationRunStartedAt.textContent = "-";
  }
  if (elements.evaluationRunElapsed) {
    elements.evaluationRunElapsed.textContent = "-";
  }
  if (elements.evaluationTotalScore) {
    elements.evaluationTotalScore.textContent = "-";
  }
  if (elements.evaluationScoreTool) {
    elements.evaluationScoreTool.textContent = "-";
  }
  if (elements.evaluationScoreLogic) {
    elements.evaluationScoreLogic.textContent = "-";
  }
  if (elements.evaluationScoreCommon) {
    elements.evaluationScoreCommon.textContent = "-";
  }
  if (elements.evaluationScoreComplex) {
    elements.evaluationScoreComplex.textContent = "-";
  }
  updateProgress({ completed: 0, total: 0 });
  if (elements.evaluationCurrentCase) {
    elements.evaluationCurrentCase.textContent = "";
  }
};

const updateProgress = (payload) => {
  const completed = Number(payload?.completed ?? 0);
  const total = Number(payload?.total ?? 0);
  const ratio = total > 0 ? Math.min(1, Math.max(0, completed / total)) : 0;
  if (elements.evaluationProgressFill) {
    elements.evaluationProgressFill.style.width = `${Math.round(ratio * 100)}%`;
  }
  if (elements.evaluationProgressText) {
    elements.evaluationProgressText.textContent = `${completed}/${total}`;
  }
  updateCurrentCaseDisplay();
};

const clearCaseTable = () => {
  evaluationState.cases.clear();
  evaluationState.caseOrder = [];
  evaluationState.caseInfo.clear();
  if (elements.evaluationCaseBody) {
    elements.evaluationCaseBody.innerHTML = "";
  }
  if (elements.evaluationCaseEmpty) {
    elements.evaluationCaseEmpty.style.display = "block";
    elements.evaluationCaseEmpty.textContent = t("evaluation.cases.empty");
  }
  if (elements.evaluationCurrentCase) {
    elements.evaluationCurrentCase.textContent = "";
  }
};

const ensureCaseRow = (caseId) => {
  if (!elements.evaluationCaseBody) {
    return null;
  }
  if (evaluationState.cases.has(caseId)) {
    return evaluationState.cases.get(caseId);
  }
  const row = document.createElement("tr");
  const cols = [document.createElement("td"), document.createElement("td"), document.createElement("td"), document.createElement("td")];
  cols.forEach((col) => row.appendChild(col));
  cols[0].textContent = caseId;
  row.addEventListener("click", () => {
    const sessionId = row.dataset.sessionId || "";
    if (sessionId) {
      openMonitorDetail(sessionId);
    }
  });
  elements.evaluationCaseBody.appendChild(row);
  evaluationState.cases.set(caseId, row);
  if (!evaluationState.caseOrder.includes(caseId)) {
    evaluationState.caseOrder.push(caseId);
  }
  if (elements.evaluationCaseEmpty) {
    elements.evaluationCaseEmpty.style.display = "none";
  }
  return row;
};

const renderCaseItem = (item) => {
  const caseId = String(item?.case_id || "").trim();
  if (!caseId) {
    return;
  }
  const row = ensureCaseRow(caseId);
  if (!row) {
    return;
  }
  const sessionId = String(item?.session_id || "").trim();
  if (sessionId) {
    row.dataset.sessionId = sessionId;
  } else {
    row.dataset.sessionId = "";
  }
  const cells = row.querySelectorAll("td");
  const dimension = String(item?.dimension || "-");
  const status = String(item?.status || "-");
  const score = Number(item?.score ?? NaN);
  const maxScore = Number(item?.max_score ?? NaN);
  const prompt = String(item?.prompt || "").trim();
  const existing = evaluationState.caseInfo.get(caseId) || {};
  evaluationState.caseInfo.set(caseId, {
    ...existing,
    status,
    dimension,
    prompt: prompt || existing.prompt || "",
  });
  const statusBadge = document.createElement("span");
  statusBadge.textContent = status;
  statusBadge.className = `monitor-status ${status}`;
  if (status === "active") {
    statusBadge.className = "monitor-status running";
  }
  if (status === "passed") {
    statusBadge.className = "monitor-status finished";
  }
  if (status === "failed" || status === "error") {
    statusBadge.className = "monitor-status error";
  }
  if (status === "skipped") {
    statusBadge.className = "monitor-status waiting";
  }
  if (status === "cancelled") {
    statusBadge.className = "monitor-status cancelled";
  }
  cells[1].textContent = dimension;
  cells[2].textContent = "";
  cells[2].appendChild(statusBadge);
  if (Number.isFinite(score) && Number.isFinite(maxScore)) {
    cells[3].textContent = `${formatScore(score)}/${formatScore(maxScore)}`;
  } else {
    cells[3].textContent = "-";
  }
  updateCurrentCaseDisplay();
};

const applyRunPayload = (run) => {
  if (!run) {
    return;
  }
  const runId = run.run_id || run.runId || "";
  if (Array.isArray(run.case_ids) && run.case_ids.length) {
    evaluationState.caseOrder = run.case_ids
      .map((value) => String(value || "").trim())
      .filter(Boolean);
  }
  if (elements.evaluationRunId) {
    elements.evaluationRunId.textContent = runId || "-";
  }
  if (elements.evaluationRunStatus) {
    const status = String(run.status || "-");
    elements.evaluationRunStatus.textContent = status;
    elements.evaluationRunStatus.className = `monitor-status ${status}`;
  }
  if (elements.evaluationRunStartedAt) {
    elements.evaluationRunStartedAt.textContent = formatEpochSeconds(run.started_time);
  }
  if (elements.evaluationRunElapsed) {
    elements.evaluationRunElapsed.textContent = formatDuration(run.elapsed_s);
  }
  if (elements.evaluationTotalScore) {
    elements.evaluationTotalScore.textContent = formatScore(run.total_score);
  }
  const scores = run.dimension_scores || {};
  if (elements.evaluationScoreTool) {
    elements.evaluationScoreTool.textContent = formatScore(scores.tool);
  }
  if (elements.evaluationScoreLogic) {
    elements.evaluationScoreLogic.textContent = formatScore(scores.logic);
  }
  if (elements.evaluationScoreCommon) {
    elements.evaluationScoreCommon.textContent = formatScore(scores.common);
  }
  if (elements.evaluationScoreComplex) {
    elements.evaluationScoreComplex.textContent = formatScore(scores.complex);
  }
  const passed = Number(run.passed_count ?? 0);
  const failed = Number(run.failed_count ?? 0);
  const skipped = Number(run.skipped_count ?? 0);
  const errors = Number(run.error_count ?? 0);
  const total = Number(run.case_count ?? 0);
  updateProgress({ completed: passed + failed + skipped + errors, total });
  updateCurrentCaseDisplay();
};

const buildApiBase = () => normalizeApiBase(elements.apiBase?.value || "");

const fillSelect = (select, options, fallback) => {
  if (!select) {
    return;
  }
  select.innerHTML = "";
  const values = options.length ? options : [fallback].filter(Boolean);
  values.forEach((value) => {
    const option = document.createElement("option");
    option.value = value;
    option.textContent = value;
    select.appendChild(option);
  });
};

const loadCaseSets = async () => {
  const apiBase = buildApiBase();
  if (!apiBase) {
    setFormStatus(t("evaluation.message.apiBaseEmpty"));
    return;
  }
  const response = await fetch(`${apiBase}/admin/evaluation/cases`);
  if (!response.ok) {
    throw new Error(await response.text());
  }
  const payload = await response.json();
  const caseSets = payload.case_sets || [];
  const uniqueSets = Array.from(new Set(caseSets.map((item) => item.case_set).filter(Boolean)));
  const uniqueLangs = Array.from(new Set(caseSets.map((item) => item.language).filter(Boolean)));
  fillSelect(elements.evaluationCaseSet, uniqueSets, "default");
  const preferredLang = getCurrentLanguage();
  fillSelect(elements.evaluationLanguage, uniqueLangs, preferredLang);
  if (elements.evaluationLanguage && uniqueLangs.includes(preferredLang)) {
    elements.evaluationLanguage.value = preferredLang;
  }
};

const loadEvaluationHistory = async () => {
  if (!elements.evaluationHistoryBody || !elements.evaluationHistoryEmpty) {
    return;
  }
  elements.evaluationHistoryBody.innerHTML = "";
  elements.evaluationHistoryEmpty.textContent = t("common.loading");
  elements.evaluationHistoryEmpty.style.display = "block";
  try {
    const apiBase = buildApiBase();
    if (!apiBase) {
      throw new Error(t("evaluation.message.apiBaseEmpty"));
    }
    const response = await fetch(`${apiBase}/admin/evaluation/runs?limit=50`);
    if (!response.ok) {
      throw new Error(await response.text());
    }
    const payload = await response.json();
    const runs = payload.runs || [];
    if (!runs.length) {
      elements.evaluationHistoryEmpty.textContent = t("evaluation.history.empty");
      elements.evaluationHistoryEmpty.style.display = "block";
      return;
    }
    elements.evaluationHistoryEmpty.style.display = "none";
    runs.forEach((run) => {
      const row = document.createElement("tr");
      const runId = run.run_id || "";
      const cells = [
        document.createElement("td"),
        document.createElement("td"),
        document.createElement("td"),
        document.createElement("td"),
        document.createElement("td"),
        document.createElement("td"),
      ];
      cells[0].textContent = runId || "-";
      cells[1].textContent = run.status || "-";
      cells[2].textContent = formatScore(run.total_score);
      cells[3].textContent = formatEpochSeconds(run.started_time);
      cells[4].textContent = run.model_name || "-";
      const deleteBtn = document.createElement("button");
      deleteBtn.className = "icon-btn";
      deleteBtn.type = "button";
      deleteBtn.title = t("evaluation.history.delete");
      deleteBtn.setAttribute("aria-label", t("evaluation.history.delete"));
      deleteBtn.innerHTML = '<i class="fa-solid fa-trash"></i>';
      deleteBtn.addEventListener("click", (event) => {
        event.stopPropagation();
        if (runId) {
          deleteHistoryRun(runId);
        }
      });
      cells[5].appendChild(deleteBtn);
      cells.forEach((cell) => row.appendChild(cell));
      row.dataset.runId = runId;
      row.addEventListener("click", () => {
        const runId = row.dataset.runId;
        if (runId) {
          restoreHistoryRun(runId);
        }
      });
      elements.evaluationHistoryBody.appendChild(row);
    });
  } catch (error) {
    elements.evaluationHistoryEmpty.textContent = t("common.loadFailedWithMessage", {
      message: error.message,
    });
    elements.evaluationHistoryEmpty.style.display = "block";
  }
};

const refreshActiveRun = async (runId, options = {}) => {
  const cleaned = String(runId || "").trim();
  if (!cleaned) {
    return;
  }
  const apiBase = buildApiBase();
  if (!apiBase) {
    if (!options.silent) {
      setFormStatus(t("evaluation.message.apiBaseEmpty"));
    }
    return;
  }
  try {
    const response = await fetch(`${apiBase}/admin/evaluation/${cleaned}`);
    if (!response.ok) {
      throw new Error(await response.text());
    }
    const payload = await response.json();
    const run = payload.run || {};
    const viewMatches = !evaluationState.viewRunId || evaluationState.viewRunId === cleaned;
    if (viewMatches) {
      applyRunPayload(run);
      (payload.items || []).forEach((item) => renderCaseItem(item));
    }
    const status = String(run.status || "");
    if (cleaned === evaluationState.activeRunId && status) {
      evaluationState.activeStatus = status;
      if (isFinishedStatus(status)) {
        evaluationState.activeRunId = "";
      }
    }
    updateActionButton();
    updateRunHint();
  } catch (error) {
    if (!options.silent) {
      setFormStatus(t("evaluation.message.historyRestoreFailed", { message: error.message }));
    }
  }
};

const deleteHistoryRun = async (runId) => {
  const cleaned = String(runId || "").trim();
  if (!cleaned) {
    return;
  }
  const confirmed = window.confirm(t("evaluation.history.deleteConfirm", { runId: cleaned }));
  if (!confirmed) {
    return;
  }
  const apiBase = buildApiBase();
  if (!apiBase) {
    setFormStatus(t("evaluation.message.apiBaseEmpty"));
    return;
  }
  try {
    const response = await fetch(`${apiBase}/admin/evaluation/${cleaned}`, {
      method: "DELETE",
    });
    if (!response.ok) {
      throw new Error(await response.text());
    }
    if (evaluationState.viewRunId === cleaned) {
      evaluationState.viewRunId = evaluationState.activeRunId;
      clearCaseTable();
      resetRunSummary();
      updateRunHint();
    }
    setFormStatus(t("evaluation.message.historyDeleted", { runId: cleaned }));
    await loadEvaluationHistory();
  } catch (error) {
    setFormStatus(t("evaluation.message.historyDeleteFailed", { message: error.message }));
  }
};

const stopStream = () => {
  if (evaluationState.controller) {
    evaluationState.controller.abort();
  }
  evaluationState.streaming = false;
  evaluationState.controller = null;
};

const parseSseBlock = (block) => {
  const lines = block.split(/\r?\n/);
  let eventType = "message";
  const dataLines = [];
  lines.forEach((line) => {
    if (line.startsWith("event:")) {
      eventType = line.slice(6).trim();
    } else if (line.startsWith("data:")) {
      dataLines.push(line.slice(5).trim());
    }
  });
  return {
    eventType,
    dataText: dataLines.join("\n"),
  };
};

const streamEvaluation = async (runId) => {
  const apiBase = buildApiBase();
  if (!apiBase || !runId) {
    return;
  }
  stopStream();
  const controller = new AbortController();
  evaluationState.controller = controller;
  evaluationState.streaming = true;
  try {
    const response = await fetch(`${apiBase}/admin/evaluation/stream/${runId}`, {
      signal: controller.signal,
    });
    if (!response.ok || !response.body) {
      throw new Error(await response.text());
    }
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";
    while (true) {
      const { value, done } = await reader.read();
      if (done) {
        break;
      }
      buffer += decoder.decode(value, { stream: true });
      let index = buffer.indexOf("\n\n");
      while (index !== -1) {
        const chunk = buffer.slice(0, index).trim();
        buffer = buffer.slice(index + 2);
        if (chunk) {
          const { eventType, dataText } = parseSseBlock(chunk);
          if (dataText) {
            let payload = null;
            try {
              payload = JSON.parse(dataText);
            } catch (error) {
              payload = { raw: dataText };
            }
            handleStreamEvent(eventType, payload);
          }
        }
        index = buffer.indexOf("\n\n");
      }
    }
  } catch (error) {
    if (controller.signal.aborted || error?.name === "AbortError") {
      return;
    }
    void refreshActiveRun(runId, { silent: true });
    setFormStatus(t("evaluation.message.streamFailed", { message: error.message }));
  } finally {
    evaluationState.streaming = false;
  }
};

const handleStreamEvent = (eventType, payload) => {
  if (eventType === "eval_started") {
    const runId = payload?.run_id || payload?.runId || "";
    evaluationState.activeRunId = runId;
    evaluationState.activeStatus = "running";
    evaluationState.viewRunId = runId;
    resetRunSummary();
    clearCaseTable();
    if (elements.evaluationRunId) {
      elements.evaluationRunId.textContent = runId || "-";
    }
    updateActionButton();
    updateRunHint();
  } else if (eventType === "eval_item") {
    if (isViewingActive()) {
      renderCaseItem(payload);
    }
  } else if (eventType === "eval_progress") {
    if (isViewingActive()) {
      updateProgress(payload);
    }
  } else if (eventType === "eval_finished") {
    const payloadRunId = payload?.run_id || payload?.runId || "";
    const viewMatches = !evaluationState.viewRunId || evaluationState.viewRunId === payloadRunId;
    if (payloadRunId && payloadRunId === evaluationState.activeRunId) {
      evaluationState.activeStatus = payload?.status || "finished";
      evaluationState.activeRunId = "";
    }
    updateActionButton();
    updateRunHint();
    if (viewMatches) {
      applyRunPayload(payload);
    }
    stopStream();
    loadEvaluationHistory();
    if (viewMatches) {
      setFormStatus(t("evaluation.message.finished"));
      void refreshActiveRun(payloadRunId, { silent: true });
    }
  } else if (eventType === "eval_log") {
    if (payload?.message) {
      setFormStatus(payload.message);
    }
  }
};

const startEvaluation = async () => {
  const apiBase = buildApiBase();
  if (!apiBase) {
    setFormStatus(t("evaluation.message.apiBaseEmpty"));
    return;
  }
  const userId =
    String(elements.evaluationUserId?.value || "").trim() ||
    String(elements.userId?.value || "").trim();
  if (!userId) {
    setFormStatus(t("evaluation.message.userIdEmpty"));
    return;
  }
  const dimensions = [];
  if (elements.evaluationDimTool?.checked) dimensions.push("tool");
  if (elements.evaluationDimLogic?.checked) dimensions.push("logic");
  if (elements.evaluationDimCommon?.checked) dimensions.push("common");
  if (elements.evaluationDimComplex?.checked) dimensions.push("complex");
  if (!dimensions.length) {
    setFormStatus(t("evaluation.message.dimensionEmpty"));
    return;
  }
  const caseSet = String(elements.evaluationCaseSet?.value || "default").trim();
  const language = String(elements.evaluationLanguage?.value || getCurrentLanguage()).trim();
  const modelName = String(elements.evaluationModelName?.value || "").trim();
  const payload = {
    user_id: userId,
    case_set: caseSet,
    language,
    dimensions,
  };
  if (modelName) {
    payload.model_name = modelName;
  }
  setFormStatus(t("evaluation.message.starting"));
  try {
    const response = await fetch(`${apiBase}/admin/evaluation/start`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(payload),
    });
    if (!response.ok) {
      throw new Error(await response.text());
    }
    const data = await response.json();
    evaluationState.activeRunId = data.run_id || "";
    evaluationState.activeStatus = "running";
    evaluationState.viewRunId = evaluationState.activeRunId;
    clearCaseTable();
    resetRunSummary();
    updateActionButton();
    updateRunHint();
    setFormStatus(t("evaluation.message.started"));
    if (evaluationState.activeRunId) {
      streamEvaluation(evaluationState.activeRunId);
      void refreshActiveRun(evaluationState.activeRunId, { silent: true });
    }
  } catch (error) {
    setFormStatus(t("evaluation.message.startFailed", { message: error.message }));
  }
};

const cancelEvaluation = async () => {
  const apiBase = buildApiBase();
  if (!apiBase) {
    setFormStatus(t("evaluation.message.apiBaseEmpty"));
    return;
  }
  if (!evaluationState.activeRunId) {
    setFormStatus(t("evaluation.message.noActiveRun"));
    return;
  }
  try {
    const response = await fetch(
      `${apiBase}/admin/evaluation/${evaluationState.activeRunId}/cancel`,
      {
        method: "POST",
      }
    );
    if (!response.ok) {
      throw new Error(await response.text());
    }
    setFormStatus(t("evaluation.message.cancelled"));
  } catch (error) {
    setFormStatus(t("evaluation.message.cancelFailed", { message: error.message }));
  }
};

const restoreHistoryRun = async (runId) => {
  const apiBase = buildApiBase();
  if (!apiBase) {
    setFormStatus(t("evaluation.message.apiBaseEmpty"));
    return;
  }
  try {
    const response = await fetch(`${apiBase}/admin/evaluation/${runId}`);
    if (!response.ok) {
      throw new Error(await response.text());
    }
    const payload = await response.json();
    evaluationState.viewRunId = runId;
    clearCaseTable();
    applyRunPayload(payload.run);
    (payload.items || []).forEach((item) => renderCaseItem(item));
    updateRunHint();
    setFormStatus(t("evaluation.message.historyRestored", { runId }));
  } catch (error) {
    setFormStatus(t("evaluation.message.historyRestoreFailed", { message: error.message }));
  }
};

const syncDefaults = () => {
  if (elements.evaluationUserId && elements.userId && !elements.evaluationUserId.value) {
    elements.evaluationUserId.value = elements.userId.value;
  }
  updateActionButton();
  updateRunHint();
};

const initEvaluationPanel = async () => {
  if (!elements.evaluationPanel) {
    return;
  }
  syncDefaults();
  if (elements.evaluationActionBtn) {
    elements.evaluationActionBtn.addEventListener("click", () => {
      if (isActiveRunning()) {
        cancelEvaluation();
      } else {
        startEvaluation();
      }
    });
  }
  window.addEventListener("wunder:language-changed", () => {
    updateActionButton();
    updateRunHint();
  });
  await loadCaseSets();
  await loadEvaluationHistory();
  resetRunSummary();
  clearCaseTable();
};

export { initEvaluationPanel, loadEvaluationHistory };


