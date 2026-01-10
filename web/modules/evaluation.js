import { elements } from "./elements.js";
import { normalizeApiBase, formatDuration } from "./utils.js";
import { getCurrentLanguage, t } from "./i18n.js?v=20260110-03";

const DEFAULT_WEIGHTS = {
  tool: 35,
  logic: 25,
  common: 20,
  complex: 20,
};

const evaluationState = {
  runId: "",
  streaming: false,
  controller: null,
  cases: new Map(),
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
};

const clearCaseTable = () => {
  evaluationState.cases.clear();
  if (elements.evaluationCaseBody) {
    elements.evaluationCaseBody.innerHTML = "";
  }
  if (elements.evaluationCaseEmpty) {
    elements.evaluationCaseEmpty.style.display = "block";
    elements.evaluationCaseEmpty.textContent = t("evaluation.cases.empty");
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
  elements.evaluationCaseBody.appendChild(row);
  evaluationState.cases.set(caseId, row);
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
  const cells = row.querySelectorAll("td");
  const dimension = String(item?.dimension || "-");
  const status = String(item?.status || "-");
  const score = Number(item?.score ?? NaN);
  const maxScore = Number(item?.max_score ?? NaN);
  const statusBadge = document.createElement("span");
  statusBadge.textContent = status;
  statusBadge.className = `monitor-status ${status}`;
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
    cells[3].textContent = `${score}/${maxScore}`;
  } else {
    cells[3].textContent = "-";
  }
};

const applyRunPayload = (run) => {
  if (!run) {
    return;
  }
  const runId = run.run_id || run.runId || "";
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
  updateProgress({ completed: run.passed_count + run.failed_count + run.skipped_count + run.error_count, total: run.case_count });
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
      const selectCell = document.createElement("td");
      const checkbox = document.createElement("input");
      checkbox.type = "checkbox";
      checkbox.value = run.run_id || "";
      checkbox.className = "evaluation-compare-checkbox";
      selectCell.appendChild(checkbox);
      const cells = [
        selectCell,
        document.createElement("td"),
        document.createElement("td"),
        document.createElement("td"),
        document.createElement("td"),
        document.createElement("td"),
        document.createElement("td"),
      ];
      cells[1].textContent = run.run_id || "-";
      cells[2].textContent = run.status || "-";
      cells[3].textContent = formatScore(run.total_score);
      cells[4].textContent = formatEpochSeconds(run.started_time);
      cells[5].textContent = run.model_name || "-";
      cells[6].textContent = run.language || "-";
      cells.forEach((cell) => row.appendChild(cell));
      elements.evaluationHistoryBody.appendChild(row);
    });
  } catch (error) {
    elements.evaluationHistoryEmpty.textContent = t("common.loadFailedWithMessage", {
      message: error.message,
    });
    elements.evaluationHistoryEmpty.style.display = "block";
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
  evaluationState.controller = new AbortController();
  evaluationState.streaming = true;
  try {
    const response = await fetch(`${apiBase}/admin/evaluation/stream/${runId}`, {
      signal: evaluationState.controller.signal,
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
    if (evaluationState.controller?.signal?.aborted) {
      return;
    }
    setFormStatus(t("evaluation.message.streamFailed", { message: error.message }));
  } finally {
    evaluationState.streaming = false;
  }
};

const handleStreamEvent = (eventType, payload) => {
  if (eventType === "eval_started") {
    const runId = payload?.run_id || payload?.runId || "";
    evaluationState.runId = runId;
    resetRunSummary();
    clearCaseTable();
    if (elements.evaluationRunId) {
      elements.evaluationRunId.textContent = runId || "-";
    }
  } else if (eventType === "eval_item") {
    renderCaseItem(payload);
  } else if (eventType === "eval_progress") {
    updateProgress(payload);
  } else if (eventType === "eval_finished") {
    applyRunPayload(payload);
    stopStream();
    loadEvaluationHistory();
    setFormStatus(t("evaluation.message.finished"));
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
  const weightInputs = {
    tool: Number(elements.evaluationWeightTool?.value),
    logic: Number(elements.evaluationWeightLogic?.value),
    common: Number(elements.evaluationWeightCommon?.value),
    complex: Number(elements.evaluationWeightComplex?.value),
  };
  const hasWeight = Object.values(weightInputs).some((value) => Number.isFinite(value));
  const payload = {
    user_id: userId,
    case_set: caseSet,
    language,
    dimensions,
  };
  if (modelName) {
    payload.model_name = modelName;
  }
  if (hasWeight) {
    payload.weights = {
      tool: Number.isFinite(weightInputs.tool) ? weightInputs.tool : 0,
      logic: Number.isFinite(weightInputs.logic) ? weightInputs.logic : 0,
      common: Number.isFinite(weightInputs.common) ? weightInputs.common : 0,
      complex: Number.isFinite(weightInputs.complex) ? weightInputs.complex : 0,
    };
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
    evaluationState.runId = data.run_id || "";
    clearCaseTable();
    resetRunSummary();
    setFormStatus(t("evaluation.message.started"));
    if (evaluationState.runId) {
      streamEvaluation(evaluationState.runId);
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
  if (!evaluationState.runId) {
    setFormStatus(t("evaluation.message.noActiveRun"));
    return;
  }
  try {
    const response = await fetch(
      `${apiBase}/admin/evaluation/${evaluationState.runId}/cancel`,
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

const compareRuns = async () => {
  if (!elements.evaluationCompareSummary || !elements.evaluationCompareBody || !elements.evaluationCompareHead) {
    return;
  }
  const selected = Array.from(document.querySelectorAll(".evaluation-compare-checkbox:checked"))
    .map((item) => item.value)
    .filter(Boolean);
  if (selected.length !== 2) {
    elements.evaluationCompareEmpty.style.display = "block";
    elements.evaluationCompareSummary.textContent = "";
    elements.evaluationCompareBody.innerHTML = "";
    elements.evaluationCompareHead.innerHTML = "";
    setFormStatus(t("evaluation.message.compareSelectTwo"));
    return;
  }
  const apiBase = buildApiBase();
  if (!apiBase) {
    setFormStatus(t("evaluation.message.apiBaseEmpty"));
    return;
  }
  try {
    const response = await fetch(`${apiBase}/admin/evaluation/compare?run_ids=${selected.join(",")}`);
    if (!response.ok) {
      throw new Error(await response.text());
    }
    const payload = await response.json();
    const runs = payload.runs || [];
    const cases = payload.cases || [];
    const runMap = new Map(runs.map((run) => [run.run_id, run]));
    const summaryItems = selected
      .map((runId) => runMap.get(runId))
      .filter(Boolean)
      .map((run) => {
        const scores = run.dimension_scores || {};
        return `<div class="summary-item"><strong>${run.run_id}</strong> · ${formatScore(
          run.total_score
        )} <span>${formatScore(scores.tool)}/${formatScore(scores.logic)}/${formatScore(
          scores.common
        )}/${formatScore(scores.complex)}</span></div>`;
      });
    elements.evaluationCompareSummary.innerHTML = summaryItems.join("");
    elements.evaluationCompareHead.innerHTML = `
      <tr>
        <th>${t("evaluation.compare.table.case")}</th>
        <th>${t("evaluation.compare.table.dimension")}</th>
        <th>${selected[0]}</th>
        <th>${selected[1]}</th>
      </tr>
    `;
    elements.evaluationCompareBody.innerHTML = "";
    cases.forEach((item) => {
      const row = document.createElement("tr");
      const caseId = item.case_id || "-";
      const dimension = item.dimension || "-";
      const left = item.items?.[selected[0]];
      const right = item.items?.[selected[1]];
      const leftText = left ? `${left.status || "-"} (${left.score ?? "-"})` : "-";
      const rightText = right ? `${right.status || "-"} (${right.score ?? "-"})` : "-";
      row.innerHTML = `
        <td>${caseId}</td>
        <td>${dimension}</td>
        <td>${leftText}</td>
        <td>${rightText}</td>
      `;
      elements.evaluationCompareBody.appendChild(row);
    });
    elements.evaluationCompareEmpty.style.display = cases.length ? "none" : "block";
  } catch (error) {
    setFormStatus(t("evaluation.message.compareFailed", { message: error.message }));
  }
};

const syncDefaults = () => {
  if (elements.evaluationUserId && elements.userId && !elements.evaluationUserId.value) {
    elements.evaluationUserId.value = elements.userId.value;
  }
  if (elements.evaluationWeightTool && !elements.evaluationWeightTool.value) {
    elements.evaluationWeightTool.value = DEFAULT_WEIGHTS.tool;
  }
  if (elements.evaluationWeightLogic && !elements.evaluationWeightLogic.value) {
    elements.evaluationWeightLogic.value = DEFAULT_WEIGHTS.logic;
  }
  if (elements.evaluationWeightCommon && !elements.evaluationWeightCommon.value) {
    elements.evaluationWeightCommon.value = DEFAULT_WEIGHTS.common;
  }
  if (elements.evaluationWeightComplex && !elements.evaluationWeightComplex.value) {
    elements.evaluationWeightComplex.value = DEFAULT_WEIGHTS.complex;
  }
};

const initEvaluationPanel = async () => {
  if (!elements.evaluationPanel) {
    return;
  }
  syncDefaults();
  if (elements.evaluationStartBtn) {
    elements.evaluationStartBtn.addEventListener("click", startEvaluation);
  }
  if (elements.evaluationCancelBtn) {
    elements.evaluationCancelBtn.addEventListener("click", cancelEvaluation);
  }
  if (elements.evaluationRefreshBtn) {
    elements.evaluationRefreshBtn.addEventListener("click", loadEvaluationHistory);
  }
  if (elements.evaluationCompareBtn) {
    elements.evaluationCompareBtn.addEventListener("click", compareRuns);
  }
  await loadCaseSets();
  await loadEvaluationHistory();
  resetRunSummary();
  clearCaseTable();
};

export { initEvaluationPanel, loadEvaluationHistory };
