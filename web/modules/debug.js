import { APP_CONFIG } from "../app.config.js";
import { elements } from "./elements.js?v=20251231-03";
import { state } from "./state.js";
import { appendLog, appendRequestLog, clearOutput } from "./log.js?v=20251229-02";
import { getWunderBase } from "./api.js";
import { applyPromptToolError, ensureToolSelectionLoaded, getSelectedToolNames } from "./tools.js?v=20251227-13";
import { loadWorkspace } from "./workspace.js?v=20260101-02";
import { notify } from "./notify.js";
import { formatTimestamp } from "./utils.js?v=20251229-02";
import { ensureLlmConfigLoaded } from "./llm.js";

const DEBUG_STATE_KEY = "wunder_debug_state";
const DEBUG_ACTIVE_STATUSES = new Set(["running", "cancelling"]);
const DEBUG_HISTORY_EMPTY_TEXT = "暂无会话";
const DEBUG_HISTORY_LOADING_TEXT = "加载中...";
const DEBUG_STATS_EMPTY_TEXT = "暂无统计信息";
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
const DEBUG_QUESTION_PRESETS = Array.isArray(APP_CONFIG.debugQuestionPresets)
  ? APP_CONFIG.debugQuestionPresets
  : [];
const DEBUG_QUESTION_EMPTY_TEXT = "暂无预设问题";
const DEBUG_RESTORE_EVENT_TYPES = new Set([
  "progress",
  "compaction",
  "tool_call",
  "tool_result",
  "llm_request",
  "llm_response",
  "knowledge_request",
  "llm_output_delta",
  "llm_output",
  "llm_stream_retry",
  // Token 用量事件在刷新后也需要保留，避免调试日志丢失
  "token_usage",
  "final",
  "error",
]);
// 缓冲模型输出，降低频繁 DOM 拼接导致的卡顿
const modelOutputBuffer = {
  chunks: [],
  scheduled: false,
  pendingScroll: false,
  rafId: 0,
};
const debugAttachments = [];
let debugAttachmentBusy = 0;
let debugStats = null;
const pendingRequestLogs = [];
let pendingRequestSeq = 0;

// 重置请求-回复关联状态，避免日志错位
const resetPendingRequestLogs = () => {
  pendingRequestLogs.length = 0;
  pendingRequestSeq = 0;
};

const buildResponseText = (data) => {
  if (!data || typeof data !== "object") {
    return "（无回复内容）";
  }
  const content = data.content ? String(data.content) : "";
  const reasoning = data.reasoning ? String(data.reasoning) : data.reasoning_content ? String(data.reasoning_content) : "";
  const sections = [];
  if (reasoning) {
    sections.push(`【思考】\n${reasoning}`);
  }
  if (content) {
    sections.push(content);
  }
  if (!sections.length) {
    return "（无回复内容）";
  }
  return sections.join("\n\n");
};

// 在请求日志条目上补充耗时标签，保持与事件日志展示一致
const appendRequestDurationBadge = (item, durationText) => {
  if (!item || !durationText || durationText === "-") {
    return;
  }
  const summary = item.querySelector("summary");
  if (!summary) {
    return;
  }
  let rightWrap = summary.querySelector(".log-right");
  if (!rightWrap) {
    rightWrap = document.createElement("span");
    rightWrap.className = "log-right";
    summary.appendChild(rightWrap);
  }
  let durationNode = rightWrap.querySelector(".log-duration");
  if (!durationNode) {
    durationNode = document.createElement("span");
    durationNode.className = "log-duration";
    const durationLabel = document.createElement("span");
    durationLabel.className = "log-duration-label";
    durationLabel.textContent = "耗时";
    const durationValue = document.createElement("span");
    durationValue.className = "log-duration-value";
    durationValue.textContent = durationText;
    durationNode.appendChild(durationLabel);
    durationNode.appendChild(durationValue);
    rightWrap.appendChild(durationNode);
  } else {
    const durationValue = durationNode.querySelector(".log-duration-value");
    if (durationValue) {
      durationValue.textContent = durationText;
    }
  }
};

const attachResponseToRequest = (response, options = {}) => {
  if (!pendingRequestLogs.length) {
    return;
  }
  const entry = pendingRequestLogs.shift();
  if (!entry || !entry.item) {
    return;
  }
  if (entry.responseAttached) {
    return;
  }
  entry.responseAttached = true;
  if (Number.isFinite(entry.requestTimestampMs)) {
    const responseTimestampMs = resolveTimestampMs(options.timestamp);
    const endTimestampMs = Number.isFinite(responseTimestampMs) ? responseTimestampMs : Date.now();
    const durationText = formatDurationSeconds(entry.requestTimestampMs, endTimestampMs);
    appendRequestDurationBadge(entry.item, durationText);
  }
  const responseText = buildResponseText(response);
  const detailNode = entry.item.querySelector(".log-detail");
  if (!detailNode) {
    return;
  }
  const responseNode = document.createElement("div");
  responseNode.className = "log-response";
  responseNode.textContent = `【模型回复】\n${responseText}`;
  detailNode.appendChild(responseNode);
};

const flushPendingRequests = (message, options = {}) => {
  if (!pendingRequestLogs.length) {
    return;
  }
  const content = message ? `请求异常：${message}` : "请求异常，未返回回复。";
  while (pendingRequestLogs.length) {
    attachResponseToRequest({ content, reasoning: "" }, options);
  }
};

// 控制调试日志等待态，便于管理员判断对话是否仍在进行
const setDebugLogWaiting = (waiting) => {
  [elements.eventLog, elements.requestLog].forEach((target) => {
    if (!target) {
      return;
    }
    const card = target.closest(".log-card");
    if (card) {
      card.classList.toggle("is-waiting", waiting);
    }
    target.setAttribute("aria-busy", waiting ? "true" : "false");
  });
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
  const label = isStop ? "停止" : "发送";
  elements.sendBtn.setAttribute("aria-label", label);
  elements.sendBtn.title = label;
};

const updateDebugLogWaiting = (force) => {
  if (typeof force === "boolean") {
    setDebugLogWaiting(force);
    setSendToggleState(force);
    return;
  }
  const status = String(state.runtime.debugSessionStatus || "").trim();
  const shouldWait = Boolean(state.runtime.debugStreaming) || DEBUG_ACTIVE_STATUSES.has(status);
  setDebugLogWaiting(shouldWait);
  setSendToggleState(shouldWait);
};

// 初始化统计信息结构，便于调试面板复用
const createDebugStats = () => ({
  tokenInput: 0,
  tokenOutput: 0,
  tokenTotal: 0,
  toolCalls: 0,
  toolOk: 0,
  toolFailed: 0,
  sandboxCalls: 0,
  llmRequests: 0,
  knowledgeRequests: 0,
  errorCount: 0,
  eventCount: 0,
  hasTokenUsage: false,
  timeRangeStartMs: null,
  timeRangeEndMs: null,
  requestStartMs: null,
  requestEndMs: null,
});

const resetDebugStats = () => {
  debugStats = createDebugStats();
  renderDebugStats();
};

const formatStatNumber = (value, fallback = "-") => {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) {
    return fallback;
  }
  return parsed.toLocaleString();
};

// 解析事件时间为毫秒，用于统计会话耗时
const resolveTimestampMs = (value) => {
  if (!value) {
    return null;
  }
  if (value instanceof Date) {
    return value.getTime();
  }
  if (typeof value === "number") {
    const parsed = new Date(value);
    return Number.isNaN(parsed.getTime()) ? null : parsed.getTime();
  }
  if (typeof value === "string") {
    const parsed = new Date(value);
    return Number.isNaN(parsed.getTime()) ? null : parsed.getTime();
  }
  return null;
};

// 记录事件时间范围，计算整体耗时
const applyEventTimestamp = (timestamp) => {
  const ts = resolveTimestampMs(timestamp);
  if (!Number.isFinite(ts)) {
    return;
  }
  if (!Number.isFinite(debugStats.timeRangeStartMs) || ts < debugStats.timeRangeStartMs) {
    debugStats.timeRangeStartMs = ts;
  }
  if (!Number.isFinite(debugStats.timeRangeEndMs) || ts > debugStats.timeRangeEndMs) {
    debugStats.timeRangeEndMs = ts;
  }
};

// 请求开始/结束时补齐时间范围，避免无事件时耗时为空
const markRequestStart = () => {
  const now = Date.now();
  debugStats.requestStartMs = now;
  if (!Number.isFinite(debugStats.timeRangeStartMs)) {
    debugStats.timeRangeStartMs = now;
  }
};

const markRequestEnd = () => {
  const now = Date.now();
  debugStats.requestEndMs = now;
  if (!Number.isFinite(debugStats.timeRangeEndMs) || now > debugStats.timeRangeEndMs) {
    debugStats.timeRangeEndMs = now;
  }
};

const formatDurationSeconds = (startMs, endMs) => {
  if (!Number.isFinite(startMs) || !Number.isFinite(endMs) || endMs < startMs) {
    return "-";
  }
  return `${((endMs - startMs) / 1000).toFixed(2)}s`;
};

const renderDebugStats = () => {
  if (!elements.finalAnswer) {
    return;
  }
  if (!debugStats) {
    elements.finalAnswer.textContent = DEBUG_STATS_EMPTY_TEXT;
    return;
  }
  const outputState = getModelOutputState();
  const outputChars = Array.isArray(outputState.rounds)
    ? outputState.rounds.reduce(
        (sum, entry) => sum + (Number.isFinite(entry?.contentChars) ? entry.contentChars : 0),
        0
      )
    : 0;
  const sessionId = String(state.runtime.debugSessionId || "").trim();
  const tokenText = debugStats.hasTokenUsage
    ? `${formatStatNumber(debugStats.tokenTotal, "0")}（输入 ${formatStatNumber(
        debugStats.tokenInput,
        "0"
      )} / 输出 ${formatStatNumber(debugStats.tokenOutput, "0")}）`
    : "-";
  const toolText = `${formatStatNumber(debugStats.toolCalls, "0")}（成功 ${formatStatNumber(
    debugStats.toolOk,
    "0"
  )} / 失败 ${formatStatNumber(debugStats.toolFailed, "0")}）`;
  const startMs = Number.isFinite(debugStats.timeRangeStartMs)
    ? debugStats.timeRangeStartMs
    : debugStats.requestStartMs;
  const endMs = state.runtime.debugStreaming && Number.isFinite(startMs)
    ? Date.now()
    : Number.isFinite(debugStats.timeRangeEndMs)
      ? debugStats.timeRangeEndMs
      : debugStats.requestEndMs;
  const durationText = formatDurationSeconds(startMs, endMs);

  // 使用表格呈现统计信息，提升可读性与对齐效果
  const rows = [
    { label: "会话 ID", value: sessionId || "-" },
    { label: "会话耗时", value: durationText },
    { label: "Token 占用", value: tokenText },
    { label: "模型请求次数", value: formatStatNumber(debugStats.llmRequests, "0") },
    { label: "知识库请求次数", value: formatStatNumber(debugStats.knowledgeRequests, "0") },
    { label: "工具调用次数", value: toolText },
    { label: "Sandbox 工具调用", value: formatStatNumber(debugStats.sandboxCalls, "0") },
    { label: "模型输出字数", value: formatStatNumber(outputChars, "0") },
    { label: "错误次数", value: formatStatNumber(debugStats.errorCount, "0") },
    { label: "事件条数", value: formatStatNumber(debugStats.eventCount, "0") },
  ];

  const table = document.createElement("table");
  table.className = "stats-table";
  const thead = document.createElement("thead");
  const headRow = document.createElement("tr");
  const headLabel = document.createElement("th");
  headLabel.textContent = "指标";
  const headValue = document.createElement("th");
  headValue.textContent = "数值";
  headRow.appendChild(headLabel);
  headRow.appendChild(headValue);
  thead.appendChild(headRow);
  table.appendChild(thead);

  const tbody = document.createElement("tbody");
  rows.forEach((row) => {
    const tr = document.createElement("tr");
    const labelCell = document.createElement("td");
    labelCell.className = "stats-label";
    labelCell.textContent = row.label;
    const valueCell = document.createElement("td");
    valueCell.className = "stats-value";
    valueCell.textContent = row.value;
    tr.appendChild(labelCell);
    tr.appendChild(valueCell);
    tbody.appendChild(tr);
  });
  table.appendChild(tbody);

  elements.finalAnswer.textContent = "";
  elements.finalAnswer.appendChild(table);
};

const applyTokenUsage = (usage) => {
  if (!usage || typeof usage !== "object") {
    return;
  }
  const inputTokens = Number(usage.input_tokens ?? 0);
  const outputTokens = Number(usage.output_tokens ?? 0);
  const totalTokens = Number(usage.total_tokens ?? 0);
  if (Number.isFinite(inputTokens)) {
    debugStats.tokenInput += inputTokens;
  }
  if (Number.isFinite(outputTokens)) {
    debugStats.tokenOutput += outputTokens;
  }
  if (Number.isFinite(totalTokens)) {
    debugStats.tokenTotal += totalTokens;
  }
  debugStats.hasTokenUsage = true;
};

const applyTokenUsageSnapshot = (usage, options = {}) => {
  if (!usage || typeof usage !== "object") {
    return;
  }
  const inputTokens = Number(usage.input_tokens ?? 0);
  const outputTokens = Number(usage.output_tokens ?? 0);
  const totalTokens = Number(usage.total_tokens ?? 0);
  const hasMeaningful =
    (Number.isFinite(totalTokens) && totalTokens > 0) ||
    (Number.isFinite(inputTokens) && inputTokens > 0) ||
    (Number.isFinite(outputTokens) && outputTokens > 0);
  if (!hasMeaningful) {
    return;
  }
  if (options.override === true || !debugStats.hasTokenUsage) {
    debugStats.tokenInput = Number.isFinite(inputTokens) ? inputTokens : 0;
    debugStats.tokenOutput = Number.isFinite(outputTokens) ? outputTokens : 0;
    debugStats.tokenTotal = Number.isFinite(totalTokens) ? totalTokens : 0;
    debugStats.hasTokenUsage = true;
    return;
  }
  if (Number.isFinite(inputTokens)) {
    debugStats.tokenInput = Math.max(debugStats.tokenInput, inputTokens);
  }
  if (Number.isFinite(outputTokens)) {
    debugStats.tokenOutput = Math.max(debugStats.tokenOutput, outputTokens);
  }
  if (Number.isFinite(totalTokens)) {
    debugStats.tokenTotal = Math.max(debugStats.tokenTotal, totalTokens);
  }
};

// 生成附件唯一标识，便于删除操作定位
const buildAttachmentId = () => `${Date.now()}_${Math.random().toString(16).slice(2)}`;

// 更新附件提示信息，避免用户忘记当前绑定的文件/图片
const updateAttachmentMeta = () => {
  if (!elements.debugAttachmentMeta) {
    return;
  }
  if (debugAttachmentBusy > 0) {
    elements.debugAttachmentMeta.textContent = `正在处理 ${debugAttachmentBusy} 个附件...`;
    return;
  }
  const total = debugAttachments.length;
  elements.debugAttachmentMeta.textContent = total ? `已附加 ${total} 个附件` : "暂未附加";
};

// 渲染附件列表，提供删除入口与状态提示
const renderAttachmentList = () => {
  if (!elements.debugAttachmentList) {
    return;
  }
  elements.debugAttachmentList.textContent = "";
  if (!debugAttachments.length) {
    elements.debugAttachmentList.textContent = "暂未附加文件或图片";
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
    name.textContent = attachment.name || "未命名附件";

    const meta = document.createElement("div");
    meta.className = "debug-attachment-meta";
    if (attachment.type === "image") {
      meta.textContent = "图片";
    } else if (attachment.converter) {
      meta.textContent = `文件 · ${attachment.converter}`;
    } else {
      meta.textContent = "文件";
    }

    info.appendChild(name);
    info.appendChild(meta);

    const removeBtn = document.createElement("button");
    removeBtn.type = "button";
    removeBtn.className = "danger btn-with-icon btn-compact debug-attachment-remove";
    removeBtn.innerHTML = '<i class="fa-solid fa-trash"></i>删除';
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
  const presets = normalizeQuestionPresets(DEBUG_QUESTION_PRESETS);
  menu.textContent = "";
  if (!presets.length) {
    const empty = document.createElement("button");
    empty.type = "button";
    empty.disabled = true;
    empty.textContent = DEBUG_QUESTION_EMPTY_TEXT;
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
    reader.onerror = () => reject(new Error("图片读取失败"));
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
    throw new Error("API 地址为空");
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
    throw new Error(detail || `请求失败：${response.status}`);
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
        throw new Error("图片内容为空");
      }
      debugAttachments.push({
        id: buildAttachmentId(),
        type: "image",
        name: filename,
        content: dataUrl,
        mimeType: file.type || "",
      });
      renderAttachmentList();
      notify(`已附加图片：${filename}`, "success");
      return;
    }
    const extension = resolveFileExtension(filename);
    if (!extension || !DEBUG_DOC_EXTENSIONS.includes(`.${extension}`)) {
      throw new Error(`不支持的文件类型: .${extension || "未知"}`);
    }
    const result = await convertAttachmentFile(file);
    const content = typeof result?.content === "string" ? result.content : "";
    if (!content.trim()) {
      throw new Error("解析结果为空");
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
      notify(`文件转换存在警告：${warnings[0]}`, "warn");
    } else {
      notify(`已解析文件：${result?.name || filename}`, "success");
    }
  } finally {
    debugAttachmentBusy = Math.max(0, debugAttachmentBusy - 1);
    updateAttachmentMeta();
  }
};

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
    apiBase: elements.apiBase?.value || "",
    apiKey: elements.apiKey?.value || "",
    userId: elements.userId?.value || "",
    sessionId: elements.sessionId?.value || "",
    question: elements.question?.value || "",
    modelName: elements.debugModelName?.value || "",
  });
};

// 格式化事件时间，兼容 ISO 字符串/时间戳
const formatEventTime = (value) => {
  if (!value) {
    return new Date().toLocaleTimeString();
  }
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) {
    return new Date().toLocaleTimeString();
  }
  return parsed.toLocaleTimeString();
};

// 更新会话 ID 并同步存储，确保刷新后能恢复
const updateSessionId = (sessionId) => {
  const trimmed = String(sessionId || "").trim();
  if (!trimmed) {
    return;
  }
  if (elements.sessionId && elements.sessionId.value !== trimmed) {
    elements.sessionId.value = trimmed;
  }
  if (state.runtime.debugSessionId !== trimmed) {
    state.runtime.debugSessionId = trimmed;
    state.runtime.debugEventCursor = 0;
    state.runtime.debugRestored = false;
  }
  writeDebugState({ sessionId: trimmed });
};

// 重置模型输出的流式状态，避免新旧请求串联
const resetModelOutputState = (options = {}) => {
  const resetRound = options.resetRound !== false;
  const resetContent = options.resetContent === true;
  if (!state.runtime.llmOutput) {
    state.runtime.llmOutput = {
      globalRound: 0,
      currentRound: null,
      rounds: [],
      selectedRound: null,
      userSelectedRound: false,
    };
  }
  const outputState = state.runtime.llmOutput;
  if (resetRound) {
    outputState.globalRound = 0;
    outputState.currentRound = null;
  }
  if (resetContent) {
    outputState.rounds = [];
    outputState.selectedRound = null;
    outputState.userSelectedRound = false;
    resetModelOutputBuffer();
    if (elements.modelOutput) {
      elements.modelOutput.textContent = "";
    }
    renderRoundSelectOptions(outputState);
  }
};

// 重置指定轮次的输出内容，避免流式重连时重复拼接
const resetRoundOutput = (roundId) => {
  const outputState = getModelOutputState();
  const targetRound = Number.isFinite(roundId) ? roundId : outputState.currentRound;
  if (!Number.isFinite(targetRound)) {
    return;
  }
  const entry = findRoundEntry(outputState, targetRound);
  if (!entry) {
    return;
  }
  entry.chunks = [];
  entry.totalChars = 0;
  entry.contentChars = 0;
  entry.tail = "";
  entry.lastChar = "";
  entry.section = null;
  entry.headerWritten = false;
  entry.streaming = false;
  entry.reasoningStreaming = false;
  if (outputState.selectedRound === entry.id) {
    resetModelOutputBuffer();
    if (elements.modelOutput) {
      elements.modelOutput.textContent = "";
    }
  }
  renderRoundSelectOptions(outputState);
};

const getModelOutputState = () => {
  if (!state.runtime.llmOutput) {
    state.runtime.llmOutput = {
      globalRound: 0,
      currentRound: null,
      rounds: [],
      selectedRound: null,
      userSelectedRound: false,
    };
  }
  if (!Number.isFinite(state.runtime.llmOutput.globalRound)) {
    state.runtime.llmOutput.globalRound = 0;
  }
  if (!Array.isArray(state.runtime.llmOutput.rounds)) {
    state.runtime.llmOutput.rounds = [];
  }
  if (!Number.isFinite(state.runtime.llmOutput.currentRound)) {
    state.runtime.llmOutput.currentRound = null;
  }
  if (!Number.isFinite(state.runtime.llmOutput.selectedRound)) {
    state.runtime.llmOutput.selectedRound = null;
  }
  if (typeof state.runtime.llmOutput.userSelectedRound !== "boolean") {
    state.runtime.llmOutput.userSelectedRound = false;
  }
  return state.runtime.llmOutput;
};

// 统一推进模型轮次，并准备对应的轮次容器
const advanceModelRound = (timestamp) => {
  const outputState = getModelOutputState();
  outputState.globalRound = (Number.isFinite(outputState.globalRound) ? outputState.globalRound : 0) + 1;
  outputState.currentRound = outputState.globalRound;
  ensureRoundEntry(outputState, outputState.currentRound, timestamp, { autoSelect: true });
  return outputState.currentRound;
};

// 重置模型输出缓冲，避免清空后仍写入旧数据
const resetModelOutputBuffer = () => {
  if (modelOutputBuffer.rafId) {
    cancelAnimationFrame(modelOutputBuffer.rafId);
  }
  modelOutputBuffer.rafId = 0;
  modelOutputBuffer.chunks = [];
  modelOutputBuffer.scheduled = false;
  modelOutputBuffer.pendingScroll = false;
};

// 合并缓冲并刷新到 DOM，集中处理滚动
const flushModelOutput = () => {
  if (!elements.modelOutput) {
    return;
  }
  if (modelOutputBuffer.chunks.length) {
    const text = modelOutputBuffer.chunks.join("");
    modelOutputBuffer.chunks = [];
    const lastNode = elements.modelOutput.lastChild;
    if (lastNode && lastNode.nodeType === Node.TEXT_NODE) {
      lastNode.appendData(text);
    } else {
      elements.modelOutput.appendChild(document.createTextNode(text));
    }
  }
  if (modelOutputBuffer.pendingScroll) {
    elements.modelOutput.scrollTop = elements.modelOutput.scrollHeight;
    modelOutputBuffer.pendingScroll = false;
  }
};

// 计划在下一帧刷新输出，避免每个 token 都触发 DOM 更新
const scheduleModelOutputFlush = () => {
  if (modelOutputBuffer.scheduled) {
    return;
  }
  modelOutputBuffer.scheduled = true;
  modelOutputBuffer.rafId = requestAnimationFrame(() => {
    modelOutputBuffer.scheduled = false;
    modelOutputBuffer.rafId = 0;
    flushModelOutput();
  });
};

// 仅触发滚动到底部，不追加新内容
const scheduleModelOutputScroll = () => {
  modelOutputBuffer.pendingScroll = true;
  scheduleModelOutputFlush();
};

// 将输出追加到当前可见的模型输出区
const appendModelOutputChunk = (text, options = {}) => {
  if (!text) {
    return;
  }
  const textValue = String(text);
  modelOutputBuffer.chunks.push(textValue);
  if (options.scroll !== false) {
    modelOutputBuffer.pendingScroll = true;
  }
  scheduleModelOutputFlush();
};

// 记录轮次输出尾部字符，避免频繁读取完整字符串
const updateRoundTail = (entry, text) => {
  if (!text) {
    return;
  }
  const textValue = String(text);
  if (textValue.length >= 2) {
    entry.tail = textValue.slice(-2);
    entry.lastChar = textValue.slice(-1);
    return;
  }
  const tailSource = `${entry.tail || ""}${textValue}`;
  entry.tail = tailSource.slice(-2);
  entry.lastChar = tailSource.slice(-1);
};

// 组装下拉框展示文案
const buildRoundLabel = (entry) => {
  if (!entry) {
    return "";
  }
  return entry.timeText ? `第 ${entry.id} 轮 · ${entry.timeText}` : `第 ${entry.id} 轮`;
};

// 获取轮次输出文本，切换轮次时用于重建可视区域
const buildRoundText = (entry) => {
  if (!entry || !Array.isArray(entry.chunks)) {
    return "";
  }
  return entry.chunks.join("");
};

// 同步轮次下拉框选项，保持 UI 与运行时一致
const renderRoundSelectOptions = (outputState) => {
  if (!elements.modelOutputRoundSelect) {
    return;
  }
  const select = elements.modelOutputRoundSelect;
  const rounds = Array.isArray(outputState.rounds) ? outputState.rounds : [];
  select.textContent = "";
  if (!rounds.length) {
    const emptyOption = document.createElement("option");
    emptyOption.value = "";
    emptyOption.textContent = "暂无轮次";
    emptyOption.disabled = true;
    emptyOption.selected = true;
    select.appendChild(emptyOption);
    select.disabled = true;
    return;
  }
  select.disabled = false;
  const hasSelected = rounds.some((entry) => entry.id === outputState.selectedRound);
  if (!hasSelected) {
    outputState.selectedRound = rounds[rounds.length - 1].id;
    outputState.userSelectedRound = false;
  }
  rounds.forEach((entry) => {
    const option = document.createElement("option");
    option.value = String(entry.id);
    option.textContent = buildRoundLabel(entry);
    if (entry.id === outputState.selectedRound) {
      option.selected = true;
    }
    select.appendChild(option);
  });
};

// 查找已有轮次记录
const findRoundEntry = (outputState, roundId) => {
  if (!Number.isFinite(roundId)) {
    return null;
  }
  const rounds = Array.isArray(outputState.rounds) ? outputState.rounds : [];
  return rounds.find((entry) => entry.id === roundId) || null;
};

// 创建新的轮次输出容器
const buildRoundEntry = (roundId, timestamp) => ({
  id: roundId,
  timeText: timestamp ? formatEventTime(timestamp) : "",
  chunks: [],
  totalChars: 0,
  contentChars: 0,
  section: null,
  streaming: false,
  reasoningStreaming: false,
  tail: "",
  lastChar: "",
  headerWritten: false,
});

// 判断是否自动切换到新轮次
const shouldAutoSelectRound = (outputState, roundId) => {
  if (!outputState.userSelectedRound) {
    return true;
  }
  return outputState.selectedRound === roundId;
};

// 渲染指定轮次的输出内容
const renderSelectedRound = (outputState, entry, options = {}) => {
  if (!elements.modelOutput) {
    return;
  }
  resetModelOutputBuffer();
  elements.modelOutput.textContent = entry ? buildRoundText(entry) : "";
  const scrollTo = options.scrollTo || (entry && entry.id === outputState.currentRound ? "bottom" : "top");
  if (scrollTo === "bottom") {
    scheduleModelOutputScroll();
  } else {
    elements.modelOutput.scrollTop = 0;
  }
};

// 切换当前选中的轮次，更新下拉框与输出区域
const selectRound = (outputState, roundId, options = {}) => {
  const entry = findRoundEntry(outputState, roundId);
  outputState.selectedRound = entry ? entry.id : null;
  if (options.manual) {
    outputState.userSelectedRound = outputState.selectedRound !== outputState.currentRound;
  } else if (options.auto) {
    outputState.userSelectedRound = false;
  }
  renderRoundSelectOptions(outputState);
  const finalEntry = findRoundEntry(outputState, outputState.selectedRound);
  renderSelectedRound(outputState, finalEntry, { scrollTo: options.scrollTo });
};

// 确保轮次存在，并在需要时自动切换
const ensureRoundEntry = (outputState, roundId, timestamp, options = {}) => {
  if (!Number.isFinite(roundId)) {
    return null;
  }
  let entry = findRoundEntry(outputState, roundId);
  let needsRender = false;
  if (!entry) {
    entry = buildRoundEntry(roundId, timestamp);
    outputState.rounds.push(entry);
    needsRender = true;
  }
  if (timestamp && !entry.timeText) {
    entry.timeText = formatEventTime(timestamp);
    needsRender = true;
  }
  if (!Number.isFinite(entry.contentChars)) {
    entry.contentChars = 0;
  }
  if (needsRender) {
    renderRoundSelectOptions(outputState);
  }
  if (options.autoSelect && shouldAutoSelectRound(outputState, roundId)) {
    selectRound(outputState, roundId, { auto: true });
  }
  return entry;
};

// 将轮次抬头补齐到输出中，保证每轮有独立起始标记
const ensureRoundHeader = (outputState, entry, timestamp) => {
  if (!entry || entry.headerWritten) {
    if (entry && timestamp && !entry.timeText) {
      entry.timeText = formatEventTime(timestamp);
      renderRoundSelectOptions(outputState);
    }
    return;
  }
  if (timestamp && !entry.timeText) {
    entry.timeText = formatEventTime(timestamp);
    renderRoundSelectOptions(outputState);
  }
  const timeText = entry.timeText ? `[${entry.timeText}] ` : "";
  appendRoundText(outputState, entry, `${timeText}第 ${entry.id} 轮\n`);
  entry.headerWritten = true;
  entry.section = null;
};

// 确保思考/输出分区标题存在，避免混杂显示
const ensureRoundSection = (outputState, entry, label) => {
  if (!entry || entry.section === label) {
    return;
  }
  if (entry.totalChars > 0 && entry.lastChar !== "\n") {
    appendRoundText(outputState, entry, "\n");
  }
  appendRoundText(outputState, entry, `[${label}]\n`);
  entry.section = label;
};

// 追加轮次输出，同时在当前选中轮次时刷新 DOM
const appendRoundText = (outputState, entry, text, options = {}) => {
  if (!entry || !text) {
    return;
  }
  const textValue = String(text);
  entry.chunks.push(textValue);
  entry.totalChars += textValue.length;
  if (options.countContent) {
    if (!Number.isFinite(entry.contentChars)) {
      entry.contentChars = 0;
    }
    entry.contentChars += textValue.length;
  }
  updateRoundTail(entry, textValue);
  if (entry.id === outputState.selectedRound) {
    appendModelOutputChunk(textValue, { scroll: options.scroll !== false });
  }
};

// 解析事件携带的轮次编号，确保能与当前轮次保持同步
const resolveOutputRoundId = (outputState, dataRound) => {
  const fallbackRound = Number.isFinite(dataRound) ? dataRound : null;
  if (Number.isFinite(outputState.currentRound)) {
    return outputState.currentRound;
  }
  if (Number.isFinite(fallbackRound)) {
    outputState.currentRound = fallbackRound;
    if (!Number.isFinite(outputState.globalRound) || outputState.globalRound < fallbackRound) {
      outputState.globalRound = fallbackRound;
    }
    return fallbackRound;
  }
  return null;
};

// 下拉框切换轮次时只展示选中内容
const handleModelOutputRoundChange = () => {
  if (!elements.modelOutputRoundSelect) {
    return;
  }
  const outputState = getModelOutputState();
  const value = String(elements.modelOutputRoundSelect.value || "").trim();
  const roundId = value ? Number(value) : Number.NaN;
  if (!Number.isFinite(roundId)) {
    selectRound(outputState, null, { manual: true, scrollTo: "top" });
    return;
  }
  selectRound(outputState, roundId, { manual: true, scrollTo: "top" });
};

// 将模型增量输出追加到调试面板，保持流式阅读体验
const appendModelOutputDelta = (data, timestamp) => {
  const delta = typeof data?.delta === "string" ? data.delta : "";
  const reasoningDelta = typeof data?.reasoning_delta === "string" ? data.reasoning_delta : "";
  if (!delta && !reasoningDelta) {
    return;
  }
  const outputState = getModelOutputState();
  const displayRound = resolveOutputRoundId(outputState, data?.round);
  if (!Number.isFinite(displayRound)) {
    return;
  }
  const entry = ensureRoundEntry(outputState, displayRound, timestamp, { autoSelect: true });
  ensureRoundHeader(outputState, entry, timestamp);
  if (reasoningDelta) {
    ensureRoundSection(outputState, entry, "思考过程");
    appendRoundText(outputState, entry, reasoningDelta);
    entry.reasoningStreaming = true;
  }
  if (delta) {
    ensureRoundSection(outputState, entry, "模型输出");
    appendRoundText(outputState, entry, delta, { countContent: true });
    entry.streaming = true;
  }
};

// 根据工具名称判断所属类别，便于与系统提示词高亮颜色保持一致
const resolveToolCategory = (toolName) => {
  const name = String(toolName || "").trim();
  if (!name) {
    return "default";
  }
  if (state.toolSelection?.builtin?.some((item) => item.name === name)) {
    return "builtin";
  }
  if (state.toolSelection?.knowledge?.some((item) => item.name === name)) {
    return "knowledge";
  }
  if (state.toolSelection?.userTools?.some((item) => item.name === name)) {
    return "user";
  }
  if (state.toolSelection?.sharedTools?.some((item) => item.name === name)) {
    return "shared";
  }
  if (state.toolSelection?.skills?.some((item) => item.name === name)) {
    return "skill";
  }
  if (state.toolSelection?.mcp?.some((item) => item.name === name)) {
    return "mcp";
  }
  if (name.includes("@")) {
    return "mcp";
  }
  return "default";
};

// 统一处理 SSE 事件，按类型更新界面
const handleEvent = (eventType, dataText, options = {}) => {
  if (!dataText) {
    return;
  }
  if (!debugStats) {
    resetDebugStats();
  }
  let payload = null;
  try {
    payload = JSON.parse(dataText);
  } catch (error) {
    appendLog(`未能解析事件 JSON: ${dataText}`);
    return;
  }
  const eventTimestamp = options.timestamp || payload.timestamp;
  const sessionId = typeof payload?.session_id === "string" ? payload.session_id : "";
  if (sessionId) {
    updateSessionId(sessionId);
  }
  debugStats.eventCount += 1;
  applyEventTimestamp(eventTimestamp || Date.now());

  if (eventType === "final") {
    const usage = payload.data?.usage;
    // 最终事件里包含的 usage 也要写入事件日志，避免漏看整体用量
    applyTokenUsageSnapshot(usage, { override: true });
    renderDebugStats();
    const summary = "收到最终回复。";
    const detail =
      usage && typeof usage === "object"
        ? JSON.stringify({ usage }, null, 2)
        : undefined;
    appendLog(summary, { detail, timestamp: eventTimestamp });
    resetPendingRequestLogs();
    loadWorkspace({ refreshTree: true });
    return;
  }

  if (eventType === "error") {
    debugStats.errorCount += 1;
    renderDebugStats();
    const errorMessage =
      payload?.data?.message || payload?.message || payload?.data?.detail?.error || "";
    flushPendingRequests(errorMessage, { timestamp: eventTimestamp });
    appendLog("发生错误。", {
      detail: JSON.stringify(payload.data || payload, null, 2),
      timestamp: eventTimestamp,
    });
    loadWorkspace({ refreshTree: true });
    return;
  }

  if (eventType === "progress") {
    const data = payload.data || payload;
    let detailData = data;
    const stage = typeof data?.stage === "string" ? data.stage : "";
    let summary = typeof data?.summary === "string" ? data.summary : "";
    const showStageBadge = stage && !["received", "llm_call", "compacting"].includes(stage);
    if (stage === "llm_call") {
      const roundNumber = advanceModelRound(eventTimestamp);
      summary = `调用模型（第 ${roundNumber} 轮）`;
      if (Number.isFinite(data?.round)) {
        if (data.round !== roundNumber) {
          detailData = { ...data, request_round: data.round, round: roundNumber };
        } else {
          detailData = { ...data };
        }
      } else {
        detailData = { ...data, round: roundNumber };
      }
      renderDebugStats();
    }
    appendLog(summary || "进度更新。", {
      stage: showStageBadge ? stage : "",
      detail: JSON.stringify(detailData, null, 2),
      timestamp: eventTimestamp,
    });
    return;
  }

  if (eventType === "compaction") {
    const data = payload.data || payload;
    const reason = data?.reason === "history" ? "历史阈值" : "上下文超限";
    const status = typeof data?.status === "string" ? data.status : "";
    const title = status ? `上下文压缩（${reason} / ${status}）` : `上下文压缩（${reason}）`;
    const detail = JSON.stringify(data, null, 2);
    appendLog(title, {
      detail,
      timestamp: eventTimestamp,
      showEventBadge: false,
    });
    return;
  }

  if (eventType === "tool_call") {
    const data = payload.data || payload;
    const toolName = typeof data?.tool === "string" ? data.tool : "";
    const title = toolName ? `tool_call - ${toolName}` : "tool_call";
    const category = resolveToolCategory(toolName);
    debugStats.toolCalls += 1;
    renderDebugStats();
    appendLog(title, {
      eventType: "tool_call",
      highlight: true,
      highlightClass: category,
      showEventBadge: false,
      detail: JSON.stringify(data, null, 2),
      timestamp: eventTimestamp,
    });
    return;
  }

  if (eventType === "tool_result") {
    const data = payload.data || payload;
    const toolName = typeof data?.tool === "string" ? data.tool : "";
    const sandboxed = data?.sandbox === true;
    const title = toolName ? `tool_result - ${toolName}` : "tool_result";
    if (data?.ok === true) {
      debugStats.toolOk += 1;
    } else if (data?.ok === false) {
      debugStats.toolFailed += 1;
    }
    if (sandboxed) {
      debugStats.sandboxCalls += 1;
    }
    renderDebugStats();
    appendLog(title, {
      eventType: "tool_result",
      showEventBadge: false,
      rightTag: sandboxed ? "sandbox" : "",
      rightTagClass: sandboxed ? "log-tag--sandbox" : "",
      detail: JSON.stringify(data, null, 2),
      timestamp: eventTimestamp,
    });
    return;
  }

  if (eventType === "llm_request") {
    const data = payload.data || payload;
    const hasPayload = data && typeof data === "object" && "payload" in data;
    const hasSummary = data && typeof data === "object" && "payload_summary" in data;
    const purpose = typeof data?.purpose === "string" ? data.purpose : "";
    let title = hasSummary && !hasPayload ? "模型请求摘要" : "模型请求体";
    if (purpose === "compaction_summary") {
      title = "上下文压缩请求体";
    }
    const detail = JSON.stringify(data, null, 2);
    debugStats.llmRequests += 1;
    renderDebugStats();
    const item = appendRequestLog(title, detail, { eventType: "llm_request", timestamp: eventTimestamp });
    if (item) {
      const requestTimestampMs = resolveTimestampMs(eventTimestamp);
      pendingRequestLogs.push({
        id: ++pendingRequestSeq,
        item,
        purpose,
        responseAttached: false,
        requestTimestampMs: Number.isFinite(requestTimestampMs) ? requestTimestampMs : Date.now(),
      });
    }
    return;
  }

  if (eventType === "llm_response") {
    const data = payload.data || payload;
    attachResponseToRequest(data, { timestamp: eventTimestamp });
    return;
  }

  if (eventType === "knowledge_request") {
    const data = payload.data || payload;
    const detail = JSON.stringify(data, null, 2);
    const title = data?.knowledge_base ? `知识库请求体（${data.knowledge_base}）` : "知识库请求体";
    debugStats.knowledgeRequests += 1;
    renderDebugStats();
    appendRequestLog(title, detail, { eventType: "knowledge_request", timestamp: eventTimestamp });
    return;
  }

  if (eventType === "llm_output_delta") {
    const data = payload.data || payload;
    renderDebugStats();
    appendModelOutputDelta(data, eventTimestamp);
    return;
  }

  if (eventType === "llm_stream_retry") {
    const data = payload.data || payload;
    const attempt = Number.isFinite(data?.attempt) ? data.attempt : 0;
    const maxAttempts = Number.isFinite(data?.max_attempts) ? data.max_attempts : 0;
    const delayText = Number.isFinite(data?.delay_s) ? `${data.delay_s}s` : "";
    const willRetry = data?.will_retry !== false;
    let summary = "流式重连中";
    if (maxAttempts) {
      summary = willRetry
        ? `流式重连：${attempt}/${maxAttempts}${delayText ? `（等待 ${delayText}）` : ""}`
        : `流式重连失败：${attempt}/${maxAttempts}`;
    } else if (!willRetry) {
      summary = "流式重连失败";
    }
    if (data?.reset_output === true) {
      resetRoundOutput(data?.round);
    }
    appendLog(summary, {
      detail: JSON.stringify(data, null, 2),
      timestamp: eventTimestamp,
    });
    return;
  }

  if (eventType === "llm_output") {
    const data = payload.data || payload;
    renderDebugStats();
    attachResponseToRequest(data, { timestamp: eventTimestamp });
    const outputState = getModelOutputState();
    const displayRound = resolveOutputRoundId(outputState, data?.round);
    if (!Number.isFinite(displayRound)) {
      return;
    }
    const entry = ensureRoundEntry(outputState, displayRound, eventTimestamp, { autoSelect: true });
    ensureRoundHeader(outputState, entry, eventTimestamp);
    const content = data?.content ? String(data.content) : "";
    const reasoning = data?.reasoning ? String(data.reasoning) : "";
    const hasContent = Boolean(content);
    const hasReasoning = Boolean(reasoning);
    const isContentStreaming = entry.streaming;
    const isReasoningStreaming = entry.reasoningStreaming;

    if (isContentStreaming && (!hasReasoning || isReasoningStreaming) && !hasContent) {
      // 已通过增量输出渲染过内容时，仅补齐换行并结束该轮流式状态
      if (entry.totalChars > 0 && entry.tail !== "\n\n") {
        appendRoundText(outputState, entry, "\n\n");
      }
      entry.streaming = false;
      entry.reasoningStreaming = false;
      entry.section = null;
      if (entry.id === outputState.selectedRound) {
        scheduleModelOutputScroll();
      }
      return;
    }

    if (hasReasoning && !isReasoningStreaming) {
      ensureRoundSection(outputState, entry, "思考过程");
      appendRoundText(outputState, entry, reasoning);
    }
    if (hasContent && !isContentStreaming) {
      ensureRoundSection(outputState, entry, "模型输出");
      appendRoundText(outputState, entry, content, { countContent: true });
    }

    if (entry.totalChars > 0 && entry.tail !== "\n\n") {
      appendRoundText(outputState, entry, "\n\n");
    }
    entry.streaming = false;
    entry.reasoningStreaming = false;
    entry.section = null;
    if (entry.id === outputState.selectedRound) {
      scheduleModelOutputScroll();
    }
    return;
  }

  if (eventType === "token_usage") {
    const data = payload.data || payload;
    // 流式 token_usage 仅记录日志，统计信息等待 final usage 再对齐
    if (!state.runtime.debugStreaming) {
      applyTokenUsage(data);
      renderDebugStats();
    }
    const summary = data?.total_tokens ? `token_usage: ${data.total_tokens}` : "token_usage";
    appendLog(summary, { detail: JSON.stringify(data, null, 2), timestamp: eventTimestamp });
    return;
  }

  const data = payload.data || payload;
  const summary = data?.name ? `${eventType}: ${data.name}` : eventType;
  appendLog(summary, { detail: JSON.stringify(data, null, 2), timestamp: eventTimestamp });
};

// 发送流式请求并解析 SSE

// 还原本地保存的调试输入，便于刷新后继续查看
const applyStoredDebugInputs = () => {
  const stored = readDebugState();
  if (stored.apiBase && elements.apiBase) {
    elements.apiBase.value = stored.apiBase;
  }
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
    updateSessionId(stored.sessionId);
  }
  return stored;
};

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
    throw new Error(`请求失败：${response.status}`);
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
    elements.debugHistoryList.textContent = DEBUG_HISTORY_EMPTY_TEXT;
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
    title.textContent = session?.question || "（无问题）";

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
        notify("会话编号缺失，无法恢复。", "warn");
        return;
      }
      if (state.runtime.debugStreaming) {
        notify("当前正在请求中，请先停止后再恢复历史。", "warn");
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
      updateSessionId(sessionId);
      state.runtime.debugEventCursor = 0;
      state.runtime.debugRestored = false;
      syncDebugInputs();
      closeDebugHistoryModal();
      const status = await restoreDebugPanel({ refresh: true, syncInputs: false });
      if (!status) {
        notify("历史会话恢复失败，请稍后重试。", "error");
        return;
      }
      notify("已恢复历史会话。", "success");
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
    ? `用户 ${userId} · 共 ${count} 条`
    : `全部会话 · 共 ${count} 条`;
};

const loadDebugHistory = async () => {
  if (!elements.debugHistoryList) {
    return;
  }
  elements.debugHistoryList.textContent = DEBUG_HISTORY_LOADING_TEXT;
  try {
    const { sessions, userId } = await fetchDebugSessions();
    updateDebugHistoryMeta(sessions, userId);
    renderDebugHistoryList(sessions, { userId });
  } catch (error) {
    elements.debugHistoryList.textContent = `加载失败：${error.message}`;
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
    const error = new Error(`请求失败：${response.status}`);
    error.status = response.status;
    throw error;
  }
  return response.json();
};

// 使用监控事件恢复调试面板日志
const applyMonitorDetail = (detail, options = {}) => {
  const session = detail?.session || {};
  const events = Array.isArray(detail?.events) ? detail.events : [];
  const sessionId = session.session_id || state.runtime.debugSessionId || "";
  if (sessionId) {
    updateSessionId(sessionId);
  }
  if (session.user_id && elements.userId && !elements.userId.value.trim()) {
    elements.userId.value = session.user_id;
  }
  if (session.question && elements.question && !elements.question.value.trim()) {
    elements.question.value = session.question;
  }
  state.runtime.debugSessionStatus = session.status || "";
  updateDebugLogWaiting();
  let appendOnly = options.appendOnly === true;
  if (!Number.isFinite(state.runtime.debugEventCursor) || state.runtime.debugEventCursor <= 0) {
    appendOnly = false;
  }
  if (appendOnly && state.runtime.debugEventCursor > events.length) {
    appendOnly = false;
  }
  if (!appendOnly) {
    clearOutput();
    resetPendingRequestLogs();
    resetModelOutputState({ resetContent: true });
    resetDebugStats();
  }
  const startIndex = appendOnly ? state.runtime.debugEventCursor : 0;
  events.slice(startIndex).forEach((item) => {
    if (!DEBUG_RESTORE_EVENT_TYPES.has(item.type)) {
      return;
    }
    const dataText = JSON.stringify({ data: item.data, session_id: sessionId });
    handleEvent(item.type, dataText, { timestamp: item.timestamp });
  });
  state.runtime.debugEventCursor = events.length;
  state.runtime.debugRestored = true;
  syncDebugInputs();
};

// 刷新调试面板并恢复历史事件
export const restoreDebugPanel = async (options = {}) => {
  const refresh = options.refresh === true;
  const syncInputs = options.syncInputs !== false;
  const stored = syncInputs ? applyStoredDebugInputs() : readDebugState();
  const sessionId = state.runtime.debugSessionId || stored.sessionId || "";
  if (!sessionId || state.runtime.debugStreaming) {
    return null;
  }
  try {
    const detail = await fetchMonitorDetail(sessionId);
    applyMonitorDetail(detail, { appendOnly: refresh && state.runtime.debugRestored });
    return state.runtime.debugSessionStatus;
  } catch (error) {
    if (error?.status == 404) {
      writeDebugState({ sessionId: "" });
      state.runtime.debugSessionId = "";
    }
    appendLog(`工具列表加载失败：${error.message}`);
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

// 控制调试面板自动刷新
export const toggleDebugPolling = (enabled) => {
  if (!enabled || state.runtime.debugStreaming) {
    stopDebugPolling();
    return;
  }
  restoreDebugPanel({ refresh: true }).then((status) => {
    if (status && DEBUG_ACTIVE_STATUSES.has(status)) {
      startDebugPolling();
    } else {
      stopDebugPolling();
    }
  });
};

const sendStreamRequest = async (endpoint, payload) => {
  stopDebugPolling();
  state.runtime.debugStreaming = true;
  updateDebugLogWaiting();
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
      throw new Error(`请求失败：${response.status}`);
    }

    appendLog("SSE 连接已建立，开始接收事件..");

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

    appendLog("SSE 连接已结束。");
  } finally {
    state.runtime.debugStreaming = false;
    updateDebugLogWaiting();
    state.runtime.activeController = null;
    if (!debugStats) {
      resetDebugStats();
    }
    markRequestEnd();
    renderDebugStats();
    if (state.runtime.debugSyncAfterStream) {
      state.runtime.debugSyncAfterStream = false;
      state.runtime.debugEventCursor = 0;
      state.runtime.debugRestored = false;
      await restoreDebugPanel({ refresh: true, syncInputs: false });
    }
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
    throw new Error(`请求失败：${response.status}`);
  }

  const result = await response.json();
  if (result?.session_id) {
    updateSessionId(result.session_id);
  }
  if (!debugStats) {
    resetDebugStats();
  }
  markRequestEnd();
  applyTokenUsageSnapshot(result?.usage, { override: true });
  renderDebugStats();
  appendLog(`非流式响应：${JSON.stringify(result)}`);
};

// 统一入口：根据是否开启 SSE 选择请求方式
const handleSend = async () => {
  if (!elements.question.value.trim()) {
    appendLog("请先填写 question。");
    return;
  }
  if (debugAttachmentBusy > 0) {
    appendLog("附件解析中，请稍后再发送。");
    notify("附件解析中，请稍后再发送。", "warn");
    return;
  }

  let payload = null;
  try {
    try {
      await ensureToolSelectionLoaded();
    } catch (error) {
      appendLog(`工具列表加载失败：${error.message}`);
      applyPromptToolError(error.message);
    }
    payload = buildPayload();
  } catch (error) {
    appendLog(error.message);
    return;
  }

  const requestedSessionId = String(payload.session_id || "").trim();
  const previousSessionId = String(state.runtime.debugSessionId || "").trim();
  const hasSessionId = Boolean(requestedSessionId);
  if (!hasSessionId) {
    clearOutput();
    resetPendingRequestLogs();
    resetModelOutputState({ resetContent: true });
    resetDebugStats();
    state.runtime.debugEventCursor = 0;
    state.runtime.debugRestored = false;
    state.runtime.debugSessionStatus = "";
    state.runtime.debugSessionId = "";
    writeDebugState({ sessionId: "" });
  } else {
    const sessionChanged = requestedSessionId !== previousSessionId;
    updateSessionId(requestedSessionId);
    if (sessionChanged || !state.runtime.debugRestored) {
      await restoreDebugPanel({ refresh: true, syncInputs: false });
    }
    if (!state.runtime.debugRestored) {
      clearOutput();
      resetPendingRequestLogs();
      resetModelOutputState({ resetContent: true });
      resetDebugStats();
      state.runtime.debugEventCursor = 0;
      state.runtime.debugSessionStatus = "";
    } else {
      resetModelOutputState({ resetRound: false });
    }
  }
  syncDebugInputs();
  if (!debugStats) {
    resetDebugStats();
  }
  markRequestStart();
  renderDebugStats();
  updateDebugLogWaiting(true);

  const wunderBase = getWunderBase();
  const endpoint = wunderBase;

  try {
    if (payload.stream) {
      state.runtime.debugSyncAfterStream = hasSessionId && state.runtime.debugRestored === true;
      await sendStreamRequest(endpoint, payload);
    } else {
      await sendNonStreamRequest(endpoint, payload);
    }
  } catch (error) {
    appendLog(`请求异常：${error.message}`);
  } finally {
    updateDebugLogWaiting();
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
    throw new Error(`终止请求失败：${response.status}`);
  }
  const result = await response.json();
  appendLog(result.message || "已请求终止。");
};

// 停止流式请求：前端中断连接并通知后端取消执行
const handleStop = async () => {
  if (state.runtime.activeController) {
    state.runtime.activeController.abort();
    appendLog("已请求停止 SSE 流。");
  }
  const sessionId = String(state.runtime.debugSessionId || elements.sessionId?.value || "").trim();
  if (!sessionId) {
    appendLog("未找到会话 ID，无法请求终止。");
    return;
  }
  try {
    await requestCancelSession(sessionId);
  } catch (error) {
    appendLog(`终止请求失败：${error.message}`);
    notify(`终止请求失败：${error.message}`, "error");
  }
};

// 等待流式状态完全结束，避免清空后又被流式回写日志
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
  state.runtime.debugEventCursor = 0;
  state.runtime.debugRestored = false;
  state.runtime.debugSyncAfterStream = false;
  writeDebugState({ sessionId: "" });
};

// 新会话：清空日志与统计，并清除会话 ID，避免旧上下文残留
const handleNewSession = async () => {
  const status = String(state.runtime.debugSessionStatus || "").trim();
  const shouldStop = Boolean(state.runtime.debugStreaming) || DEBUG_ACTIVE_STATUSES.has(status);
  state.runtime.debugSyncAfterStream = false;
  if (shouldStop) {
    await handleStop();
    await waitForStreamStop();
  }
  stopDebugPolling();
  clearOutput();
  resetPendingRequestLogs();
  resetModelOutputState({ resetContent: true });
  resetDebugStats();
  resetDebugSessionState();
  syncDebugInputs();
  updateDebugLogWaiting(false);
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

// 清空输出时同步重置流式渲染状态
const handleClear = () => {
  clearOutput();
  resetPendingRequestLogs();
  resetModelOutputState({ resetContent: true });
  resetDebugStats();
};

// 初始化调试面板交互
export const initDebugPanel = () => {
  resetDebugStats();
  applyStoredDebugInputs();
  ensureLlmConfigLoaded().catch((error) => {
    appendLog(`模型配置加载失败：${error.message}`);
  });

  let syncTimer = null;
  const scheduleSync = () => {
    if (syncTimer) {
      clearTimeout(syncTimer);
    }
    syncTimer = setTimeout(() => {
      syncTimer = null;
      syncDebugInputs();
    }, 200);
  };

  if (elements.apiBase) {
    elements.apiBase.addEventListener("change", syncDebugInputs);
  }
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
          notify(`附件处理失败：${error.message}`, "error");
        }
      }
    });
  }

  if (elements.debugNewSessionBtn) {
    elements.debugNewSessionBtn.addEventListener("click", handleNewSession);
  }
  if (elements.sendBtn) {
    elements.sendBtn.addEventListener("click", handleSendToggle);
  }
  if (elements.clearBtn) {
    elements.clearBtn.addEventListener("click", handleClear);
  }
  if (elements.modelOutputRoundSelect) {
    elements.modelOutputRoundSelect.addEventListener("change", handleModelOutputRoundChange);
  }

  if (elements.debugHistoryBtn) {
    elements.debugHistoryBtn.addEventListener("click", openDebugHistoryModal);
  }
  if (elements.debugHistoryClose) {
    elements.debugHistoryClose.addEventListener("click", closeDebugHistoryModal);
  }
  if (elements.debugHistoryCloseBtn) {
    elements.debugHistoryCloseBtn.addEventListener("click", closeDebugHistoryModal);
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
  renderRoundSelectOptions(getModelOutputState());
};


