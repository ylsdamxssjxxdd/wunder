import { APP_CONFIG } from "../app.config.js?v=20260110-04";
import { elements } from "./elements.js?v=20261011-01";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { ensureToolSelectionLoaded, getSelectedToolNames } from "./tools.js?v=20260214-01";
import { loadWorkspace } from "./workspace.js?v=20260118-07";
import { notify } from "./notify.js";
import { formatTimestamp } from "./utils.js?v=20251229-02";
import { ensureLlmConfigLoaded } from "./llm.js";
import { getCurrentLanguage, t } from "./i18n.js?v=20261011-01";
import { resolveApiErrorMessage } from "./api-error.js";
import { createTrajectoryView, fetchThreadLogSnapshot } from "./trajectory-view.js?v=20261011-01";

const DEBUG_STATE_KEY = "wunder_debug_state";
const DEBUG_ACTIVE_STATUSES = new Set(["running", "cancelling"]);
// 调试面板右侧已由线程轨迹视图承载；事件流只驱动轨迹刷新。
const TRAJECTORY_REFRESH_INTERVAL_MS = 3000;
// 调试面板附件支持：图片走多模态，文件走 doc2md 解析
const DEBUG_IMAGE_EXTENSIONS = new Set(["png", "jpg", "jpeg", "gif", "bmp", "webp", "svg"]);
const DEBUG_DOC_EXTENSIONS = [
  ".txt",
  ".md",
  ".markdown",
  ".html",
  ".htm",
  ".py",
  ".c",
  ".cpp",
  ".cc",
  ".h",
  ".hpp",
  ".json",
  ".js",
  ".ts",
  ".css",
  ".ini",
  ".cfg",
  ".log",
  ".doc",
  ".docx",
  ".odt",
  ".pptx",
  ".odp",
  ".xlsx",
  ".ods",
  ".wps",
  ".et",
  ".dps",
];
const DEBUG_UPLOAD_ACCEPT = ["image/*", ...DEBUG_DOC_EXTENSIONS].join(",");
const resolveQuestionPresets = () => {
  const raw = APP_CONFIG.debugQuestionPresets;
  if (Array.isArray(raw)) {
    return raw;
  }
  if (raw && typeof raw === "object") {
    const language = getCurrentLanguage();
    if (Array.isArray(raw[language])) {
      return raw[language];
    }
    if (Array.isArray(raw["zh-CN"])) {
      return raw["zh-CN"];
    }
    if (Array.isArray(raw["en-US"])) {
      return raw["en-US"];
    }
  }
  return [];
};

const resolveStabilityPresets = () => {
  const raw = APP_CONFIG.debugStabilityPresets;
  if (Array.isArray(raw)) {
    return raw;
  }
  if (raw && typeof raw === "object") {
    const language = getCurrentLanguage();
    if (Array.isArray(raw[language])) {
      return raw[language];
    }
    if (Array.isArray(raw["zh-CN"])) {
      return raw["zh-CN"];
    }
    if (Array.isArray(raw["en-US"])) {
      return raw["en-US"];
    }
  }
  return [];
};

const normalizeStabilityPreset = (preset, index) => {
  if (!preset || typeof preset !== "object") {
    return null;
  }
  const steps = Array.isArray(preset.steps)
    ? preset.steps.map((step) => String(step || "").trim()).filter(Boolean)
    : [];
  if (!steps.length) {
    return null;
  }
  const id = String(preset.id || preset.name || `stability-${index + 1}`).trim();
  const name = String(preset.name || preset.label || preset.id || `preset-${index + 1}`).trim();
  const rawTools = preset.toolNames ?? preset.tool_names ?? [];
  const toolNames = Array.isArray(rawTools)
    ? rawTools.map((tool) => String(tool || "").trim()).filter(Boolean)
    : [];
  return {
    id: id || `stability-${index + 1}`,
    name: name || `preset-${index + 1}`,
    steps,
    toolNames,
    stream: preset.stream !== false,
  };
};

const getStabilityPresets = () =>
  resolveStabilityPresets()
    .map((preset, index) => normalizeStabilityPreset(preset, index))
    .filter(Boolean);

const stabilityRunner = {
  running: false,
  cancelled: false,
  startedAt: 0,
  currentIndex: -1,
  lastStepMs: 0,
  preset: null,
};
let compactionBusy = false;

const resetStabilityRunner = () => {
  stabilityRunner.running = false;
  stabilityRunner.cancelled = false;
  stabilityRunner.startedAt = 0;
  stabilityRunner.currentIndex = -1;
  stabilityRunner.lastStepMs = 0;
  stabilityRunner.preset = null;
};

const formatStabilityTimestamp = () => {
  const now = new Date();
  const pad = (value) => String(value).padStart(2, "0");
  return `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}${pad(
    now.getHours()
  )}${pad(now.getMinutes())}${pad(now.getSeconds())}`;
};

const buildDebugSessionId = (prefix = "debug") => {
  const rand = Math.random().toString(36).slice(2, 6);
  return `${prefix}_${formatStabilityTimestamp()}_${rand}`;
};

const ensureDebugSessionId = (options = {}) => {
  const current = String(elements.sessionId?.value || "").trim();
  if (current) {
    return current;
  }
  const prefix = typeof options.prefix === "string" && options.prefix.trim() ? options.prefix.trim() : "debug";
  const next = buildDebugSessionId(prefix);
  updateSessionId(next, { pin: true, persist: true });
  return next;
};

const applyStabilityTemplate = (text, context) => {
  const raw = String(text ?? "");
  return raw
    .replace(/\{\{timestamp\}\}/g, context.timestamp || "")
    .replace(/\{\{user_id\}\}/g, context.userId || "")
    .replace(/\{\{session_id\}\}/g, context.sessionId || "");
};

const updateStabilityStatus = (text) => {
  if (!elements.debugStabilityStatus) {
    return;
  }
  elements.debugStabilityStatus.textContent = text || "";
};

const setStabilityControls = (running) => {
  if (elements.debugStabilityRun) {
    elements.debugStabilityRun.disabled = running;
  }
  if (elements.debugStabilityStop) {
    elements.debugStabilityStop.disabled = !running;
  }
  if (elements.debugStabilityPreset) {
    elements.debugStabilityPreset.disabled = running;
  }
  if (elements.debugStabilityStream) {
    elements.debugStabilityStream.disabled = running;
  }
  if (elements.debugStabilityNewSession) {
    elements.debugStabilityNewSession.disabled = running;
  }
  if (elements.debugStabilityUseTools) {
    elements.debugStabilityUseTools.disabled = running;
  }
  if (elements.question) {
    elements.question.disabled = running;
  }
  if (elements.sendBtn) {
    elements.sendBtn.disabled = running;
  }
  syncCompactionButton();
};

const setStabilityPanelOpen = (open) => {
  if (!elements.debugStabilityPanel || !elements.debugStabilityToggleBtn) {
    return;
  }
  elements.debugStabilityPanel.classList.toggle("is-hidden", !open);
  elements.debugStabilityToggleBtn.classList.toggle("is-active", open);
  elements.debugStabilityToggleBtn.setAttribute("aria-expanded", open ? "true" : "false");
};

const toggleStabilityPanel = () => {
  if (!elements.debugStabilityPanel) {
    return;
  }
  const shouldOpen = elements.debugStabilityPanel.classList.contains("is-hidden");
  setStabilityPanelOpen(shouldOpen);
};

const renderStabilityPresetOptions = () => {
  if (!elements.debugStabilityPreset) {
    return null;
  }
  const presets = getStabilityPresets();
  const previous = elements.debugStabilityPreset.value;
  elements.debugStabilityPreset.innerHTML = "";
  if (!presets.length) {
    const option = document.createElement("option");
    option.value = "";
    option.textContent = t("debug.stability.noPreset");
    elements.debugStabilityPreset.appendChild(option);
    elements.debugStabilityPreset.disabled = true;
    updateStabilityStatus(t("debug.stability.noPreset"));
    renderStabilitySteps(null);
    return null;
  }
  elements.debugStabilityPreset.disabled = false;
  presets.forEach((preset) => {
    const option = document.createElement("option");
    option.value = preset.id;
    option.textContent = preset.name;
    elements.debugStabilityPreset.appendChild(option);
  });
  const selected = presets.find((preset) => preset.id === previous) || presets[0];
  elements.debugStabilityPreset.value = selected.id;
  renderStabilitySteps(selected, stabilityRunner.currentIndex);
  updateStabilityStatus(t("debug.stability.ready"));
  return selected;
};

const resolveSelectedStabilityPreset = () => {
  const presets = getStabilityPresets();
  if (!presets.length) {
    return null;
  }
  const selectedId = String(elements.debugStabilityPreset?.value || "").trim();
  return presets.find((preset) => preset.id === selectedId) || presets[0];
};

const renderStabilitySteps = (preset, activeIndex = -1) => {
  if (!elements.debugStabilitySteps) {
    return;
  }
  elements.debugStabilitySteps.innerHTML = "";
  if (!preset || !Array.isArray(preset.steps)) {
    elements.debugStabilitySteps.textContent = t("debug.stability.noPreset");
    return;
  }
  const list = document.createElement("ol");
  list.className = "debug-stability-step-list";
  const completedIndex = activeIndex >= preset.steps.length ? preset.steps.length : activeIndex;
  preset.steps.forEach((step, index) => {
    const item = document.createElement("li");
    item.textContent = step;
    if (index < completedIndex) {
      item.classList.add("is-done");
    }
    if (index === activeIndex) {
      item.classList.add("is-active");
    }
    list.appendChild(item);
  });
  elements.debugStabilitySteps.appendChild(list);
};

const buildStabilityPayload = (question, options = {}) => {
  const payload = {
    user_id: elements.userId?.value.trim() || "",
    question: String(question || "").trim(),
    session_id: options.sessionId || elements.sessionId?.value.trim() || null,
    stream: options.stream !== false,
  };
  const modelName = String(elements.debugModelName?.value || "").trim();
  if (modelName) {
    payload.model_name = modelName;
  }
  const toolNames =
    Array.isArray(options.toolNames) && options.toolNames.length
      ? options.toolNames
      : getSelectedToolNames();
  if (toolNames.length) {
    payload.tool_names = toolNames;
  }
  return payload;
};

const formatDurationSeconds = (startMs, endMs) => {
  if (!Number.isFinite(startMs) || !Number.isFinite(endMs) || endMs < startMs) {
    return "-";
  }
  return `${((endMs - startMs) / 1000).toFixed(2)}s`;
};

const buildStabilityStatusText = (current, total, lastStepMs) => {
  const elapsedText = formatDurationSeconds(stabilityRunner.startedAt, Date.now());
  const lastText =
    Number.isFinite(lastStepMs) && lastStepMs > 0
      ? `${(lastStepMs / 1000).toFixed(2)}s`
      : "-";
  return t("debug.stability.progress", {
    current,
    total,
    elapsed: elapsedText,
    last: lastText,
  });
};

const runStabilitySequence = async () => {
  if (stabilityRunner.running) {
    return;
  }
  const preset = resolveSelectedStabilityPreset();
  if (!preset) {
    updateStabilityStatus(t("debug.stability.noPreset"));
    return;
  }
  const userId = elements.userId?.value.trim() || "";
  if (!userId) {
    updateStabilityStatus(t("debug.stability.userIdEmpty"));
    notify(t("debug.stability.userIdEmpty"), "warn");
    return;
  }

  try {
    await ensureToolSelectionLoaded();
  } catch (error) {
    console.warn("debug tool list load failed", error);
  }

  const shouldNewSession = Boolean(elements.debugStabilityNewSession?.checked);
  if (shouldNewSession) {
    await handleNewSession();
  }

  let sessionId = String(elements.sessionId?.value || "").trim();
  if (!sessionId) {
    sessionId = `stability_${formatStabilityTimestamp()}`;
    updateSessionId(sessionId, { pin: true });
  }
  syncDebugInputs();

  stabilityRunner.running = true;
  stabilityRunner.cancelled = false;
  stabilityRunner.startedAt = Date.now();
  stabilityRunner.currentIndex = -1;
  stabilityRunner.lastStepMs = 0;
  stabilityRunner.preset = preset;
  setStabilityControls(true);
  renderStabilitySteps(preset, -1);

  const usePresetTools = Boolean(elements.debugStabilityUseTools?.checked);
  const toolNames =
    usePresetTools && preset.toolNames.length ? preset.toolNames : getSelectedToolNames();
  const streamEnabled =
    elements.debugStabilityStream?.checked ?? (preset.stream !== false);
  const total = preset.steps.length;
  const context = { userId, sessionId };

  for (let index = 0; index < total; index += 1) {
    if (stabilityRunner.cancelled) {
      break;
    }
    const stepTimestamp = formatStabilityTimestamp();
    const question = applyStabilityTemplate(preset.steps[index], {
      ...context,
      timestamp: stepTimestamp,
    });
    stabilityRunner.currentIndex = index;
    if (elements.question) {
      elements.question.value = question;
    }
    syncDebugInputs();
    renderStabilitySteps(preset, index);
    updateStabilityStatus(buildStabilityStatusText(index + 1, total, stabilityRunner.lastStepMs));

    const payload = buildStabilityPayload(question, {
      sessionId,
      stream: streamEnabled,
      toolNames,
    });
    const endpoint = getWunderBase();
    const stepStart = Date.now();
    try {
      if (payload.stream) {
        await sendStreamRequest(endpoint, payload);
      } else {
        await sendNonStreamRequest(endpoint, payload);
      }
    } catch (error) {
      updateStabilityStatus(
        t("debug.stability.stepFailed", {
          current: index + 1,
          total,
          message: error.message,
        })
      );
      stabilityRunner.cancelled = true;
      break;
    }
    stabilityRunner.lastStepMs = Date.now() - stepStart;
    updateStabilityStatus(buildStabilityStatusText(index + 1, total, stabilityRunner.lastStepMs));
  }

  const cancelled = stabilityRunner.cancelled;
  resetStabilityRunner();
  setStabilityControls(false);
  if (cancelled) {
    updateStabilityStatus(t("debug.stability.cancelled"));
  } else {
    renderStabilitySteps(preset, preset.steps.length);
    updateStabilityStatus(t("debug.stability.finished"));
  }
};

const handleStabilityStop = async () => {
  if (!stabilityRunner.running) {
    return;
  }
  stabilityRunner.cancelled = true;
  if (state.runtime.debugStreaming) {
    await handleStop();
    await waitForStreamStop();
  }
  updateStabilityStatus(t("debug.stability.cancelled"));
};

/* ------------------------------------------------------------------ */
/* 线程轨迹视图                                                        */
/* ------------------------------------------------------------------ */

let debugTrajectoryView = null;
let trajectoryRequestId = 0;
let trajectoryLastFetchMs = 0;
let trajectoryRetryTimer = null;

const loadTrajectorySnapshot = async () => {
  const sessionId = resolveDebugSessionId();
  if (!sessionId || !debugTrajectoryView) {
    return;
  }
  const requestId = ++trajectoryRequestId;
  try {
    const turns = await fetchThreadLogSnapshot(sessionId);
    if (requestId !== trajectoryRequestId) {
      return;
    }
    debugTrajectoryView.setRawTurns(turns);
  } catch (error) {
    if (requestId !== trajectoryRequestId) {
      return;
    }
    debugTrajectoryView.setLoadError(true);
  }
};

// 轨迹刷新节流：流式事件高频到达时按最小间隔合并，final/error 立即刷新。
const refreshTrajectory = (immediate = false) => {
  if (!debugTrajectoryView) {
    return;
  }
  if (!immediate && state.runtime.activePanel !== "debug") {
    return;
  }
  if (!resolveDebugSessionId()) {
    debugTrajectoryView.setRawTurns([]);
    return;
  }
  const now = Date.now();
  const elapsed = now - trajectoryLastFetchMs;
  if (immediate || elapsed >= TRAJECTORY_REFRESH_INTERVAL_MS) {
    trajectoryLastFetchMs = now;
    if (trajectoryRetryTimer) {
      clearTimeout(trajectoryRetryTimer);
      trajectoryRetryTimer = null;
    }
    void loadTrajectorySnapshot();
    return;
  }
  if (trajectoryRetryTimer) {
    return;
  }
  trajectoryRetryTimer = setTimeout(() => {
    trajectoryRetryTimer = null;
    trajectoryLastFetchMs = Date.now();
    void loadTrajectorySnapshot();
  }, TRAJECTORY_REFRESH_INTERVAL_MS - elapsed);
};

/* ------------------------------------------------------------------ */
/* 附件                                                                */
/* ------------------------------------------------------------------ */

const debugAttachments = [];
let debugAttachmentBusy = 0;

// 生成附件唯一标识，便于删除操作定位
const buildAttachmentId = () => `${Date.now()}_${Math.random().toString(16).slice(2)}`;

// 更新附件提示信息，避免用户忘记当前绑定的文件/图片
const updateAttachmentMeta = () => {
  if (!elements.debugAttachmentMeta) {
    return;
  }
  if (debugAttachmentBusy > 0) {
    elements.debugAttachmentMeta.textContent = t("debug.attachments.processing", {
      count: debugAttachmentBusy,
    });
    return;
  }
  const total = debugAttachments.length;
  elements.debugAttachmentMeta.textContent = total
    ? t("debug.attachments.added", { count: total })
    : t("debug.attachments.none");
};

// 渲染附件列表，提供删除入口与状态提示
const renderAttachmentList = () => {
  if (!elements.debugAttachmentList) {
    return;
  }
  elements.debugAttachmentList.textContent = "";
  if (!debugAttachments.length) {
    elements.debugAttachmentList.textContent = t("debug.attachments.empty");
    updateAttachmentMeta();
    return;
  }
  debugAttachments.forEach((attachment) => {
    const item = document.createElement("div");
    item.className = "debug-attachment-item";

    const icon = document.createElement("i");
    icon.className = `debug-attachment-icon fa-solid ${
      attachment.type === "image" ? "fa-image" : "fa-file-lines"
    }`;

    const info = document.createElement("div");
    info.className = "debug-attachment-info";

    const name = document.createElement("div");
    name.className = "debug-attachment-name";
    name.textContent = attachment.name || t("debug.attachments.unnamed");

    const meta = document.createElement("div");
    meta.className = "debug-attachment-meta";
    if (attachment.type === "image") {
      meta.textContent = t("debug.attachments.type.image");
    } else if (attachment.converter) {
      meta.textContent = t("debug.attachments.type.fileWithConverter", {
        converter: attachment.converter,
      });
    } else {
      meta.textContent = t("debug.attachments.type.file");
    }

    info.appendChild(name);
    info.appendChild(meta);

    const removeBtn = document.createElement("button");
    removeBtn.type = "button";
    removeBtn.className = "danger btn-with-icon btn-compact debug-attachment-remove";
    removeBtn.innerHTML = `<i class="fa-solid fa-trash"></i>${t("common.delete")}`;
    removeBtn.addEventListener("click", () => {
      removeDebugAttachment(attachment.id);
    });

    item.appendChild(icon);
    item.appendChild(info);
    item.appendChild(removeBtn);
    elements.debugAttachmentList.appendChild(item);
  });
  updateAttachmentMeta();
};

// 删除指定附件，避免无效内容随请求发送
const removeDebugAttachment = (id) => {
  const index = debugAttachments.findIndex((item) => item.id === id);
  if (index < 0) {
    return;
  }
  debugAttachments.splice(index, 1);
  renderAttachmentList();
};

// 归一化预设问题列表，避免空值与无效内容
const normalizeQuestionPresets = (presets) =>
  (Array.isArray(presets) ? presets : [])
    .map((item) => String(item || "").trim())
    .filter(Boolean);

// 渲染右键预设问题菜单，支持动态配置与空态提示
const renderQuestionPresetMenu = () => {
  if (!elements.debugQuestionMenu) {
    return;
  }
  const menu = elements.debugQuestionMenu;
  const presets = normalizeQuestionPresets(resolveQuestionPresets());
  menu.textContent = "";
  if (!presets.length) {
    const empty = document.createElement("button");
    empty.type = "button";
    empty.disabled = true;
    empty.textContent = t("debug.question.presets.empty");
    menu.appendChild(empty);
    return;
  }
  presets.forEach((preset) => {
    const item = document.createElement("button");
    item.type = "button";
    item.textContent = preset;
    item.addEventListener("click", () => {
      applyQuestionPreset(preset);
    });
    menu.appendChild(item);
  });
};

// 应用预设问题并触发输入同步
const applyQuestionPreset = (preset) => {
  if (!elements.question) {
    return;
  }
  elements.question.value = preset;
  elements.question.dispatchEvent(new Event("input", { bubbles: true }));
  elements.question.focus();
  closeQuestionPresetMenu();
};

// 打开右键菜单，确保不会超出视口
const openQuestionPresetMenu = (event) => {
  if (!elements.debugQuestionMenu) {
    return;
  }
  renderQuestionPresetMenu();
  const menu = elements.debugQuestionMenu;
  menu.style.display = "flex";
  const menuRect = menu.getBoundingClientRect();
  const maxLeft = window.innerWidth - menuRect.width - 8;
  const maxTop = window.innerHeight - menuRect.height - 8;
  const left = Math.min(event.clientX, maxLeft);
  const top = Math.min(event.clientY, maxTop);
  menu.style.left = `${Math.max(8, left)}px`;
  menu.style.top = `${Math.max(8, top)}px`;
};

// 关闭右键菜单
const closeQuestionPresetMenu = () => {
  if (!elements.debugQuestionMenu) {
    return;
  }
  elements.debugQuestionMenu.style.display = "none";
};

// 提取文件扩展名，统一用于图片与文档判断
const resolveFileExtension = (filename) => {
  const parts = String(filename || "").trim().split(".");
  if (parts.length < 2) {
    return "";
  }
  return parts.pop().toLowerCase();
};

// 判断是否为图片文件，优先使用 MIME 类型兜底扩展名
const isImageFile = (file) => {
  if (file?.type && file.type.startsWith("image/")) {
    return true;
  }
  const ext = resolveFileExtension(file?.name);
  return ext ? DEBUG_IMAGE_EXTENSIONS.has(ext) : false;
};

// 读取图片为 data URL，便于按多模态格式发送
const readFileAsDataUrl = (file) =>
  new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result || ""));
    reader.onerror = () => reject(new Error(t("debug.attachment.imageReadFailed")));
    reader.readAsDataURL(file);
  });

// 构建附件载荷，发送时只透出必要字段
const buildAttachmentPayload = () => {
  return debugAttachments
    .filter((item) => String(item?.content || "").trim())
    .map((item) => {
      const payload = {
        type: item.type,
        name: String(item.name || ""),
        content: item.content,
      };
      if (item.mimeType) {
        payload.mime_type = item.mimeType;
      }
      return payload;
    });
};

// 调用后端转换附件为 Markdown，确保走 doc2md 解析链路
const convertAttachmentFile = async (file) => {
  const wunderBase = getWunderBase();
  if (!wunderBase) {
    throw new Error(t("debug.apiBaseEmpty"));
  }
  const endpoint = `${wunderBase}/attachments/convert`;
  const formData = new FormData();
  formData.append("file", file, file.name || "upload");
  const response = await fetch(endpoint, {
    method: "POST",
    body: formData,
  });
  if (!response.ok) {
    let detail = "";
    try {
      const payload = await response.json();
      detail = payload?.message || payload?.detail?.message || payload?.detail || "";
    } catch (error) {
      detail = "";
    }
    throw new Error(detail || t("common.requestFailed", { status: response.status }));
  }
  return response.json();
};

// 处理用户选择的附件，区分图片与文件解析
const handleAttachmentSelection = async (file) => {
  if (!file) {
    return;
  }
  const filename = file.name || "upload";
  debugAttachmentBusy += 1;
  updateAttachmentMeta();
  try {
    if (isImageFile(file)) {
      const dataUrl = await readFileAsDataUrl(file);
      if (!dataUrl) {
        throw new Error(t("debug.attachment.imageEmpty"));
      }
      debugAttachments.push({
        id: buildAttachmentId(),
        type: "image",
        name: filename,
        content: dataUrl,
        mimeType: file.type || "",
      });
      renderAttachmentList();
      notify(t("debug.attachment.imageAdded", { name: filename }), "success");
      return;
    }
    const extension = resolveFileExtension(filename);
    if (!extension || !DEBUG_DOC_EXTENSIONS.includes(`.${extension}`)) {
      throw new Error(
        t("debug.attachment.unsupportedType", {
          ext: extension || t("debug.attachment.unknownExt"),
        })
      );
    }
    const result = await convertAttachmentFile(file);
    const content = typeof result?.content === "string" ? result.content : "";
    if (!content.trim()) {
      throw new Error(t("debug.attachment.emptyResult"));
    }
    debugAttachments.push({
      id: buildAttachmentId(),
      type: "file",
      name: result?.name || filename,
      content,
      mimeType: file.type || "",
      converter: result?.converter || "",
    });
    renderAttachmentList();
    const warnings = Array.isArray(result?.warnings) ? result.warnings : [];
    if (warnings.length) {
      notify(t("debug.attachment.convertWarning", { message: warnings[0] }), "warn");
    } else {
      notify(t("debug.attachment.fileParsed", { name: result?.name || filename }), "success");
    }
  } finally {
    debugAttachmentBusy = Math.max(0, debugAttachmentBusy - 1);
    updateAttachmentMeta();
  }
};

/* ------------------------------------------------------------------ */
/* 请求与状态                                                          */
/* ------------------------------------------------------------------ */

// 组装请求体，统一处理输入字段与可选参数
const buildPayload = () => {
  const payload = {
    user_id: elements.userId.value.trim(),
    question: elements.question.value.trim(),
    session_id: elements.sessionId.value.trim() || null,
    stream: true,
  };
  const modelName = String(elements.debugModelName?.value || "").trim();
  if (modelName) {
    payload.model_name = modelName;
  }
  const toolNames = getSelectedToolNames();
  if (toolNames.length) {
    payload.tool_names = toolNames;
  }
  const attachments = buildAttachmentPayload();
  if (attachments.length) {
    payload.attachments = attachments;
  }
  return payload;
};

// 将 SSE 块解析为事件类型与数据内容
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

// 读取本地持久化的调试面板状态
const readDebugState = () => {
  try {
    const raw = localStorage.getItem(DEBUG_STATE_KEY);
    if (!raw) {
      return {};
    }
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") {
      return {};
    }
    return parsed;
  } catch (error) {
    return {};
  }
};

// 写入本地调试面板状态，避免刷新后丢失输入
const writeDebugState = (patch) => {
  const next = { ...readDebugState(), ...patch };
  try {
    localStorage.setItem(DEBUG_STATE_KEY, JSON.stringify(next));
  } catch (error) {
    // 忽略浏览器存储异常，避免打断交互
  }
  return next;
};

// 将当前输入同步到本地存储
const syncDebugInputs = () => {
  writeDebugState({
    apiKey: elements.apiKey?.value || "",
    userId: elements.userId?.value || "",
    sessionId: elements.sessionId?.value || "",
    question: elements.question?.value || "",
    modelName: elements.debugModelName?.value || "",
  });
};

// 还原本地保存的调试输入，便于刷新后继续查看
const applyStoredDebugInputs = () => {
  const stored = readDebugState();
  if (stored.apiKey && elements.apiKey) {
    elements.apiKey.value = stored.apiKey;
  }
  if (stored.userId && elements.userId) {
    elements.userId.value = stored.userId;
  }
  if (stored.sessionId && elements.sessionId) {
    elements.sessionId.value = stored.sessionId;
  }
  if (stored.question && elements.question) {
    elements.question.value = stored.question;
  }
  if (stored.modelName && elements.debugModelName) {
    elements.debugModelName.value = stored.modelName;
  }
  if (stored.sessionId) {
    updateSessionId(stored.sessionId, { pin: true });
  }
  return stored;
};

// 更新会话 ID 并同步存储，确保刷新后能恢复
const updateSessionId = (sessionId, options = {}) => {
  const trimmed = String(sessionId || "").trim();
  if (!trimmed) {
    return;
  }
  const pin = options.pin === true;
  const persist =
    typeof options.persist === "boolean" ? options.persist : Boolean(state.runtime.debugSessionPinned || pin);
  if (pin) {
    state.runtime.debugSessionPinned = true;
  }
  if (persist && elements.sessionId && elements.sessionId.value !== trimmed) {
    elements.sessionId.value = trimmed;
  }
  if (state.runtime.debugSessionId !== trimmed) {
    state.runtime.debugSessionId = trimmed;
  }
  if (persist) {
    writeDebugState({ sessionId: trimmed });
  }
  syncCompactionButton();
};

const setSendToggleState = (active) => {
  if (!elements.sendBtn) {
    return;
  }
  const isStop = Boolean(active);
  const icon = elements.sendBtn.querySelector("i");
  if (icon) {
    icon.className = isStop ? "fa-solid fa-stop" : "fa-solid fa-paper-plane";
  }
  elements.sendBtn.classList.toggle("danger", isStop);
  const label = isStop ? t("debug.send.stop") : t("debug.send.send");
  elements.sendBtn.setAttribute("aria-label", label);
  elements.sendBtn.title = label;
};

const resolveDebugSessionId = () =>
  String(state.runtime.debugSessionId || elements.sessionId?.value || "").trim();

const isDebugSessionBusy = () => {
  const status = String(state.runtime.debugSessionStatus || "").trim();
  return (
    compactionBusy ||
    stabilityRunner.running ||
    state.runtime.debugStreaming ||
    DEBUG_ACTIVE_STATUSES.has(status)
  );
};

const syncCompactionButton = () => {
  if (!elements.debugCompactionBtn) {
    return;
  }
  const sessionId = resolveDebugSessionId();
  const busy = isDebugSessionBusy();
  const disabled = !sessionId || busy;
  elements.debugCompactionBtn.disabled = disabled;
  const label = !sessionId
    ? t("debug.compaction.missingSession")
    : busy
    ? t("debug.compaction.busy")
    : t("debug.compaction.action");
  elements.debugCompactionBtn.title = label;
  elements.debugCompactionBtn.setAttribute("aria-label", label);
};

const syncDebugControls = (waiting) => {
  const shouldWait =
    typeof waiting === "boolean"
      ? waiting
      : Boolean(state.runtime.debugStreaming) ||
        DEBUG_ACTIVE_STATUSES.has(String(state.runtime.debugSessionStatus || "").trim());
  setSendToggleState(shouldWait);
  syncCompactionButton();
};

/* ------------------------------------------------------------------ */
/* SSE 事件 → 轨迹刷新                                                 */
/* ------------------------------------------------------------------ */

// 统一处理 SSE 事件：会话 ID 落地；final/error 收尾；其余仅驱动轨迹刷新。
const handleEvent = (eventType, dataText) => {
  if (!dataText) {
    return;
  }
  let payload = null;
  try {
    payload = JSON.parse(dataText);
  } catch (error) {
    return;
  }
  const sessionId = typeof payload?.session_id === "string" ? payload.session_id : "";
  if (sessionId) {
    updateSessionId(sessionId);
  }
  if (eventType === "final" || eventType === "error") {
    loadWorkspace({ refreshTree: true });
    syncDebugControls();
    refreshTrajectory(true);
    return;
  }
  refreshTrajectory();
};

/* ------------------------------------------------------------------ */
/* 历史会话                                                            */
/* ------------------------------------------------------------------ */

// 获取历史会话使用的 user_id，空值表示不限定用户
const getHistoryUserId = () => String(elements.userId?.value || "").trim();

const resolveHistoryTime = (session) => session?.updated_time || session?.start_time || "";

// 按更新时间倒序排列，便于快速定位最新会话
const sortSessionsByUpdate = (sessions) =>
  [...sessions].sort(
    (a, b) => new Date(resolveHistoryTime(b)).getTime() - new Date(resolveHistoryTime(a)).getTime()
  );

// 拉取调试历史会话列表，支持按用户过滤
const fetchDebugSessions = async () => {
  const wunderBase = getWunderBase();
  const userId = getHistoryUserId();
  const endpoint = userId
    ? `${wunderBase}/admin/users/${encodeURIComponent(userId)}/sessions?active_only=false`
    : `${wunderBase}/admin/monitor?active_only=false`;
  const response = await fetch(endpoint);
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const result = await response.json();
  return {
    userId,
    sessions: Array.isArray(result.sessions) ? result.sessions : [],
  };
};

// 渲染历史会话列表，支持点击恢复
const renderDebugHistoryList = (sessions, options = {}) => {
  if (!elements.debugHistoryList) {
    return;
  }
  elements.debugHistoryList.textContent = "";
  if (!Array.isArray(sessions) || sessions.length === 0) {
    elements.debugHistoryList.textContent = t("debug.history.empty");
    return;
  }
  const userId = options.userId || "";
  sortSessionsByUpdate(sessions).forEach((session) => {
    const item = document.createElement("button");
    item.type = "button";
    item.className = "list-item";
    const sessionId = String(session?.session_id || "").trim();
    if (sessionId && sessionId === state.runtime.debugSessionId) {
      item.classList.add("active");
    }

    const title = document.createElement("div");
    title.textContent = session?.question || t("debug.question.noQuestion");

    const metaParts = [];
    metaParts.push(sessionId || "-");
    metaParts.push(session?.user_id || userId || "-");
    metaParts.push(session?.status || "-");
    const timeText = formatTimestamp(resolveHistoryTime(session));
    if (timeText && timeText !== "-") {
      metaParts.push(timeText);
    }
    const meta = document.createElement("small");
    meta.textContent = metaParts.join(" · ");

    item.appendChild(title);
    item.appendChild(meta);
    item.addEventListener("click", async () => {
      if (!sessionId) {
        notify(t("debug.history.missingSessionId"), "warn");
        return;
      }
      if (state.runtime.debugStreaming) {
        notify(t("debug.history.restoreBusy"), "warn");
        return;
      }
      if (elements.userId) {
        elements.userId.value = session?.user_id || elements.userId.value || "";
      }
      if (elements.sessionId) {
        elements.sessionId.value = sessionId;
      }
      if (elements.question) {
        elements.question.value = session?.question || "";
      }
      updateSessionId(sessionId, { pin: true });
      syncDebugInputs();
      closeDebugHistoryModal();
      const status = await restoreDebugPanel({ refresh: true, syncInputs: false });
      if (!status) {
        notify(t("debug.history.restoreFailed"), "error");
        return;
      }
      notify(t("debug.history.restoreSuccess"), "success");
    });
    elements.debugHistoryList.appendChild(item);
  });
};

const updateDebugHistoryMeta = (sessions, userId) => {
  if (!elements.debugHistoryMeta) {
    return;
  }
  const count = Array.isArray(sessions) ? sessions.length : 0;
  elements.debugHistoryMeta.textContent = userId
    ? t("debug.history.metaWithUser", { userId, count })
    : t("debug.history.metaAll", { count });
};

const loadDebugHistory = async () => {
  if (!elements.debugHistoryList) {
    return;
  }
  elements.debugHistoryList.textContent = t("common.loading");
  try {
    const { sessions, userId } = await fetchDebugSessions();
    updateDebugHistoryMeta(sessions, userId);
    renderDebugHistoryList(sessions, { userId });
  } catch (error) {
    elements.debugHistoryList.textContent = t("common.loadFailedWithMessage", {
      message: error.message,
    });
  }
};

const openDebugHistoryModal = async () => {
  if (!elements.debugHistoryModal) {
    return;
  }
  elements.debugHistoryModal.classList.add("active");
  await loadDebugHistory();
};

const closeDebugHistoryModal = () => {
  elements.debugHistoryModal?.classList.remove("active");
};

// 读取监控详情并返回事件列表
const fetchMonitorDetail = async (sessionId) => {
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/monitor/${encodeURIComponent(sessionId)}`;
  const response = await fetch(endpoint);
  if (!response.ok) {
    const error = new Error(t("common.requestFailed", { status: response.status }));
    error.status = response.status;
    throw error;
  }
  return response.json();
};

// 刷新调试面板：同步会话状态与输入，并立即刷新轨迹视图。
export const restoreDebugPanel = async (options = {}) => {
  const syncInputs = options.syncInputs !== false;
  const stored = syncInputs ? applyStoredDebugInputs() : readDebugState();
  const sessionId = state.runtime.debugSessionId || stored.sessionId || "";
  if (!sessionId || state.runtime.debugStreaming) {
    return null;
  }
  try {
    const detail = await fetchMonitorDetail(sessionId);
    const session = detail?.session || {};
    if (session.status) {
      state.runtime.debugSessionStatus = session.status;
    }
    if (session.user_id && elements.userId && !elements.userId.value.trim()) {
      elements.userId.value = session.user_id;
    }
    if (session.question && elements.question && !elements.question.value.trim()) {
      elements.question.value = session.question;
    }
    syncDebugInputs();
    syncDebugControls();
    refreshTrajectory(true);
    return state.runtime.debugSessionStatus;
  } catch (error) {
    if (error?.status == 404) {
      writeDebugState({ sessionId: "" });
      state.runtime.debugSessionId = "";
    }
    return null;
  }
};

const stopDebugPolling = () => {
  if (state.runtime.debugPollTimer) {
    clearInterval(state.runtime.debugPollTimer);
    state.runtime.debugPollTimer = null;
  }
};

const startDebugPolling = () => {
  if (state.runtime.debugPollTimer) {
    return;
  }
  state.runtime.debugPollTimer = setInterval(async () => {
    if (state.runtime.debugStreaming) {
      return;
    }
    const status = await restoreDebugPanel({ refresh: true, syncInputs: false });
    if (status && !DEBUG_ACTIVE_STATUSES.has(status)) {
      stopDebugPolling();
    }
  }, APP_CONFIG.monitorPollIntervalMs);
};

// 控制调试面板自动刷新与轨迹视图激活
export const toggleDebugPolling = (enabled) => {
  if (!enabled || state.runtime.debugStreaming) {
    stopDebugPolling();
    return;
  }
  debugTrajectoryView?.refreshSizes();
  refreshTrajectory(true);
  restoreDebugPanel({ refresh: true }).then((status) => {
    if (status && DEBUG_ACTIVE_STATUSES.has(status)) {
      startDebugPolling();
    } else {
      stopDebugPolling();
    }
  });
};

const readErrorMessage = async (response) => {
  if (!response) {
    return "";
  }
  return resolveApiErrorMessage(response, "");
};

const handleManualCompaction = async () => {
  if (!elements.debugCompactionBtn) {
    return;
  }
  const sessionId = resolveDebugSessionId();
  if (!sessionId) {
    notify(t("debug.compaction.missingSession"), "warn");
    return;
  }
  if (isDebugSessionBusy()) {
    notify(t("debug.compaction.busy"), "warn");
    return;
  }
  const wunderBase = getWunderBase();
  if (!wunderBase) {
    notify(t("debug.apiBaseEmpty"), "warn");
    return;
  }
  compactionBusy = true;
  syncCompactionButton();
  const payload = {};
  const modelName = String(elements.debugModelName?.value || "").trim();
  if (modelName) {
    payload.model_name = modelName;
  }
  const endpoint = `${wunderBase}/admin/monitor/${encodeURIComponent(sessionId)}/compaction`;
  try {
    const response = await fetch(endpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    if (!response.ok) {
      const message = await readErrorMessage(response);
      throw new Error(message || t("common.requestFailed", { status: response.status }));
    }
    const result = await response.json().catch(() => ({}));
    notify(result.message || t("debug.compaction.success"), "success");
    await restoreDebugPanel({ refresh: true, syncInputs: false });
  } catch (error) {
    notify(t("debug.compaction.failed", { message: error.message }), "error");
  } finally {
    compactionBusy = false;
    syncCompactionButton();
  }
};

const sendStreamRequest = async (endpoint, payload) => {
  stopDebugPolling();
  state.runtime.debugStreaming = true;
  syncDebugControls(true);
  state.runtime.activeController = new AbortController();
  try {
    const response = await fetch(endpoint, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(payload),
      signal: state.runtime.activeController.signal,
    });

    if (!response.ok || !response.body) {
      const message = await readErrorMessage(response);
      throw new Error(message || t("common.requestFailed", { status: response.status }));
    }

    const reader = response.body.getReader();
    const decoder = new TextDecoder("utf-8");
    let buffer = "";

    while (true) {
      const { value, done } = await reader.read();
      if (done) {
        break;
      }
      buffer += decoder.decode(value, { stream: true });
      const parts = buffer.split("\n\n");
      buffer = parts.pop() || "";
      parts.forEach((part) => {
        if (!part.trim()) {
          return;
        }
        const { eventType, dataText } = parseSseBlock(part);
        handleEvent(eventType, dataText);
      });
    }
  } finally {
    state.runtime.debugStreaming = false;
    syncDebugControls();
    state.runtime.activeController = null;
    refreshTrajectory(true);
  }
};

// 发送非流式请求，直接解析 JSON 响应
const sendNonStreamRequest = async (endpoint, payload) => {
  const response = await fetch(endpoint, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify(payload),
  });

  if (!response.ok) {
    const message = await readErrorMessage(response);
    throw new Error(message || t("common.requestFailed", { status: response.status }));
  }

  const result = await response.json();
  if (result?.session_id) {
    updateSessionId(result.session_id);
  }
  refreshTrajectory(true);
};

// 统一入口：根据是否开启 SSE 选择请求方式
const handleSend = async () => {
  if (!elements.question.value.trim()) {
    notify(t("debug.question.empty"), "warn");
    return;
  }
  if (debugAttachmentBusy > 0) {
    notify(t("debug.attachments.busy"), "warn");
    return;
  }

  let payload = null;
  const sessionId = ensureDebugSessionId();
  try {
    try {
      await ensureToolSelectionLoaded();
    } catch (error) {
      console.warn("debug tool list load failed", error);
    }
    payload = buildPayload();
    if (sessionId) {
      payload.session_id = sessionId;
    }
  } catch (error) {
    notify(error.message, "error");
    return;
  }

  const requestedSessionId = String(payload.session_id || "").trim();
  if (requestedSessionId) {
    updateSessionId(requestedSessionId, { pin: true });
  }
  syncDebugInputs();
  syncDebugControls(true);
  state.runtime.debugSessionStatus = "running";

  const endpoint = getWunderBase();

  try {
    if (payload.stream) {
      await sendStreamRequest(endpoint, payload);
    } else {
      await sendNonStreamRequest(endpoint, payload);
    }
  } catch (error) {
    notify(t("debug.request.error", { message: error.message }), "error");
  } finally {
    syncDebugControls();
  }
};

// 请求后端终止指定会话，确保真正停止智能体线程
const requestCancelSession = async (sessionId) => {
  if (!sessionId) {
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/monitor/${encodeURIComponent(sessionId)}/cancel`;
  const response = await fetch(endpoint, { method: "POST" });
  if (!response.ok) {
    throw new Error(t("debug.stopFailed", { status: response.status }));
  }
};

// 停止流式请求：前端中断连接并通知后端取消执行
const handleStop = async () => {
  if (state.runtime.activeController) {
    state.runtime.activeController.abort();
  }
  const sessionId = String(state.runtime.debugSessionId || elements.sessionId?.value || "").trim();
  if (!sessionId) {
    return;
  }
  try {
    await requestCancelSession(sessionId);
  } catch (error) {
    notify(t("debug.stopFailedWithMessage", { message: error.message }), "error");
  }
};

// 等待流式状态完全结束，避免清空后又被流式回写
const waitForStreamStop = async (timeoutMs = 4000) => {
  const start = Date.now();
  while (state.runtime.debugStreaming) {
    if (Date.now() - start >= timeoutMs) {
      break;
    }
    await new Promise((resolve) => setTimeout(resolve, 120));
  }
};

// 重置会话标识与本地缓存，确保下一次请求是全新会话
const resetDebugSessionState = () => {
  if (elements.sessionId) {
    elements.sessionId.value = "";
  }
  state.runtime.debugSessionId = "";
  state.runtime.debugSessionStatus = "";
  state.runtime.debugSessionPinned = false;
  writeDebugState({ sessionId: "" });
};

// 新会话：停止进行中的执行，清除会话 ID 并清空轨迹视图
const handleNewSession = async () => {
  const status = String(state.runtime.debugSessionStatus || "").trim();
  const shouldStop = Boolean(state.runtime.debugStreaming) || DEBUG_ACTIVE_STATUSES.has(status);
  if (shouldStop) {
    await handleStop();
    await waitForStreamStop();
  }
  stopDebugPolling();
  resetDebugSessionState();
  syncDebugInputs();
  syncDebugControls(false);
  debugTrajectoryView?.setRawTurns([]);
};

const handleSendToggle = async () => {
  const status = String(state.runtime.debugSessionStatus || "").trim();
  const shouldStop = Boolean(state.runtime.debugStreaming) || DEBUG_ACTIVE_STATUSES.has(status);
  if (shouldStop) {
    await handleStop();
    return;
  }
  await handleSend();
};

// 初始化调试面板交互
export const initDebugPanel = () => {
  if (elements.debugTrajectoryMount && !debugTrajectoryView) {
    debugTrajectoryView = createTrajectoryView(elements.debugTrajectoryMount, {
      showBack: false,
    });
  }
  applyStoredDebugInputs();
  ensureLlmConfigLoaded().catch((error) => {
    console.warn("debug llm config load failed", error);
  });

  let syncTimer = null;
  const scheduleSync = () => {
    if (syncTimer) {
      clearTimeout(syncTimer);
    }
    syncTimer = setTimeout(() => {
      syncTimer = null;
      const sessionValue = String(elements.sessionId?.value || "").trim();
      state.runtime.debugSessionPinned = Boolean(sessionValue);
      syncDebugInputs();
    }, 200);
  };

  if (elements.apiKey) {
    elements.apiKey.addEventListener("change", syncDebugInputs);
  }
  if (elements.userId) {
    elements.userId.addEventListener("input", scheduleSync);
  }
  if (elements.sessionId) {
    elements.sessionId.addEventListener("input", scheduleSync);
  }
  if (elements.debugModelName) {
    elements.debugModelName.addEventListener("change", syncDebugInputs);
  }
  if (elements.question) {
    elements.question.addEventListener("input", scheduleSync);
  }
  if (elements.question && elements.debugQuestionMenu) {
    elements.question.addEventListener("contextmenu", (event) => {
      event.preventDefault();
      event.stopPropagation();
      openQuestionPresetMenu(event);
    });
  }
  if (elements.debugUploadInput) {
    elements.debugUploadInput.accept = DEBUG_UPLOAD_ACCEPT;
  }
  if (elements.debugUploadBtn && elements.debugUploadInput) {
    elements.debugUploadBtn.addEventListener("click", () => {
      // 重置 input 值，确保重复选择同一文件也能触发 change
      elements.debugUploadInput.value = "";
      elements.debugUploadInput.click();
    });
  }
  if (elements.debugUploadInput) {
    elements.debugUploadInput.addEventListener("change", async () => {
      const files = Array.from(elements.debugUploadInput.files || []);
      if (!files.length) {
        return;
      }
      for (const file of files) {
        try {
          await handleAttachmentSelection(file);
        } catch (error) {
          notify(t("debug.attachments.failed", { message: error.message }), "error");
        }
      }
    });
  }

  if (elements.debugStabilityPanel && elements.debugStabilityToggleBtn) {
    const isOpen = !elements.debugStabilityPanel.classList.contains("is-hidden");
    setStabilityPanelOpen(isOpen);
    elements.debugStabilityToggleBtn.addEventListener("click", toggleStabilityPanel);
  }

  renderStabilityPresetOptions();
  setStabilityControls(false);
  if (elements.debugStabilityPreset) {
    elements.debugStabilityPreset.addEventListener("change", () => {
      const preset = resolveSelectedStabilityPreset();
      renderStabilitySteps(preset, stabilityRunner.currentIndex);
      updateStabilityStatus(t("debug.stability.ready"));
    });
  }
  if (elements.debugStabilityRun) {
    elements.debugStabilityRun.addEventListener("click", runStabilitySequence);
  }
  if (elements.debugStabilityStop) {
    elements.debugStabilityStop.addEventListener("click", handleStabilityStop);
  }
  if (elements.debugCompactionBtn) {
    elements.debugCompactionBtn.addEventListener("click", handleManualCompaction);
  }

  if (elements.debugNewSessionBtn) {
    elements.debugNewSessionBtn.addEventListener("click", handleNewSession);
  }
  if (elements.sendBtn) {
    elements.sendBtn.addEventListener("click", handleSendToggle);
  }

  syncCompactionButton();

  if (elements.debugHistoryBtn) {
    elements.debugHistoryBtn.addEventListener("click", openDebugHistoryModal);
  }
  if (elements.debugHistoryClose) {
    elements.debugHistoryClose.addEventListener("click", closeDebugHistoryModal);
  }
  if (elements.debugHistoryModal) {
    elements.debugHistoryModal.addEventListener("click", (event) => {
      if (event.target === elements.debugHistoryModal) {
        closeDebugHistoryModal();
      }
    });
  }
  if (elements.debugQuestionMenu) {
    document.addEventListener("click", (event) => {
      if (elements.debugQuestionMenu.contains(event.target)) {
        return;
      }
      closeQuestionPresetMenu();
    });
    document.addEventListener("scroll", closeQuestionPresetMenu, true);
    window.addEventListener("resize", closeQuestionPresetMenu);
    document.addEventListener("keydown", (event) => {
      if (event.key === "Escape") {
        closeQuestionPresetMenu();
      }
    });
  }
  renderAttachmentList();
  void restoreDebugPanel({ refresh: true, syncInputs: false });
};
