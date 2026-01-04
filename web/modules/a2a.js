import { elements } from "./elements.js?v=20260104-02";
import { state } from "./state.js";
import { parseHeadersValue, normalizeApiBase } from "./utils.js?v=20251229-02";
import { t } from "./i18n.js?v=20260104-02";

const A2A_STATE_KEY = "wunder_a2a_state";
const MAX_LOG_CHARS = 200000;
const STREAM_METHODS = new Set(["SendStreamingMessage", "SubscribeToTask"]);

// 读取本地存储状态，避免刷新丢失输入
const readA2aState = () => {
  try {
    const raw = localStorage.getItem(A2A_STATE_KEY);
    if (!raw) {
      return {};
    }
    const parsed = JSON.parse(raw);
    return parsed && typeof parsed === "object" ? parsed : {};
  } catch (error) {
    return {};
  }
};

// 写入本地存储状态
const writeA2aState = (patch) => {
  const next = { ...readA2aState(), ...(patch || {}) };
  try {
    localStorage.setItem(A2A_STATE_KEY, JSON.stringify(next));
  } catch (error) {
    // 忽略浏览器存储异常
  }
  return next;
};

// 根据 /wunder API 基础路径推导 A2A Endpoint
const buildDefaultEndpoint = () => {
  const base = normalizeApiBase(elements.apiBase?.value || "");
  if (!base) {
    return "";
  }
  const stripped = base.replace(/\/wunder\/?$/i, "");
  return `${stripped}/a2a`;
};

const resolveEndpoint = () => {
  const value = String(elements.a2aEndpoint?.value || "").trim();
  if (value) {
    return value;
  }
  return buildDefaultEndpoint();
};

const resolveAgentCardUrl = () => {
  const endpoint = resolveEndpoint();
  if (!endpoint) {
    return "";
  }
  const base = endpoint.replace(/\/a2a\/?$/i, "");
  return `${base}/.well-known/agent-card.json`;
};

// 生成请求 ID，避免重复冲突
const generateRequestId = () => `a2a-${Date.now()}-${Math.random().toString(16).slice(2, 10)}`;

// 格式化 JSON 输出，避免异常导致页面报错
const stringifyPayload = (payload) => {
  if (payload === undefined) {
    return "";
  }
  if (typeof payload === "string") {
    return payload;
  }
  try {
    return JSON.stringify(payload, null, 2);
  } catch (error) {
    return String(payload);
  }
};

// 追加日志内容并保持滚动到底部
const appendLogBlock = (container, title, payload) => {
  if (!container) {
    return;
  }
  const time = new Date().toLocaleTimeString();
  const detail = stringifyPayload(payload);
  const block = `[${time}] ${title}\n${detail}\n\n`;
  container.textContent += block;
  if (container.textContent.length > MAX_LOG_CHARS) {
    container.textContent = container.textContent.slice(-MAX_LOG_CHARS);
  }
  container.scrollTop = container.scrollHeight;
};

const clearLogs = () => {
  if (elements.a2aRequestPreview) {
    elements.a2aRequestPreview.textContent = "";
  }
  if (elements.a2aEventLog) {
    elements.a2aEventLog.textContent = "";
  }
  if (elements.a2aResponse) {
    elements.a2aResponse.textContent = "";
  }
};

const setStreamingState = (active) => {
  state.runtime.a2aStreaming = active;
  if (elements.a2aRequestCard) {
    elements.a2aRequestCard.classList.toggle("is-waiting", active);
  }
  if (elements.a2aStatus) {
    elements.a2aStatus.textContent = active ? t("a2a.status.streaming") : t("a2a.status.idle");
  }
  if (elements.a2aSendBtn) {
    elements.a2aSendBtn.disabled = active;
  }
  if (elements.a2aStopBtn) {
    elements.a2aStopBtn.disabled = !active;
  }
};

// 打开连接设置弹窗，确保默认 Endpoint 已同步
const openA2aConnectionModal = () => {
  if (!elements.a2aConnectionModal) {
    return;
  }
  syncEndpointDefault();
  elements.a2aConnectionModal.classList.add("active");
  elements.a2aEndpoint?.focus();
};

// 关闭连接设置弹窗
const closeA2aConnectionModal = () => {
  elements.a2aConnectionModal?.classList.remove("active");
};

// 打开高级设置弹窗
const openA2aAdvancedModal = () => {
  if (!elements.a2aAdvancedModal) {
    return;
  }
  elements.a2aAdvancedModal.classList.add("active");
};

// 关闭高级设置弹窗
const closeA2aAdvancedModal = () => {
  elements.a2aAdvancedModal?.classList.remove("active");
};

const syncEndpointDefault = () => {
  if (!elements.a2aEndpoint) {
    return;
  }
  const isManual = elements.a2aEndpoint.dataset.manual === "true";
  if (isManual) {
    return;
  }
  const next = buildDefaultEndpoint();
  if (next && elements.a2aEndpoint.value !== next) {
    elements.a2aEndpoint.value = next;
  }
};

const updateHeaderError = (message) => {
  if (elements.a2aHeadersError) {
    elements.a2aHeadersError.textContent = message || "";
  }
};

const resolveAuthKey = () => {
  const useGlobal = Boolean(elements.a2aUseGlobalKey?.checked);
  if (useGlobal) {
    return String(elements.apiKey?.value || "").trim();
  }
  return String(elements.a2aApiKey?.value || "").trim();
};

// 根据输入构造请求头
const buildHeaders = (streaming) => {
  const headers = new Headers();
  headers.set("Content-Type", "application/json");
  if (streaming) {
    headers.set("Accept", "text/event-stream");
  }
  const version = String(elements.a2aVersion?.value || "").trim();
  if (version) {
    headers.set("A2A-Version", version);
  }
  const { headers: extraHeaders, error } = parseHeadersValue(elements.a2aHeaders?.value || "");
  updateHeaderError(error);
  if (extraHeaders) {
    Object.entries(extraHeaders).forEach(([key, value]) => {
      headers.set(key, String(value));
    });
  }

  const authType = String(elements.a2aAuthType?.value || "apiKey");
  const useGlobal = Boolean(elements.a2aUseGlobalKey?.checked);
  const apiKey = resolveAuthKey();
  if (authType === "none") {
    headers.set("X-API-Key", "");
    return headers;
  }
  if (authType === "bearer" && apiKey) {
    headers.set("Authorization", `Bearer ${apiKey}`);
  } else if (authType === "apiKey" && apiKey) {
    headers.set("X-API-Key", apiKey);
  } else if (!useGlobal) {
    // 禁用全局 API Key 注入，避免误带鉴权头
    headers.set("X-API-Key", "");
  }
  return headers;
};

const normalizeTaskName = (taskId) => {
  const trimmed = String(taskId || "").trim();
  if (!trimmed) {
    return "";
  }
  if (trimmed.startsWith("tasks/")) {
    return trimmed;
  }
  return `tasks/${trimmed}`;
};

const splitToolNames = (raw) => {
  const items = String(raw || "")
    .split(",")
    .map((item) => item.trim())
    .filter((item) => item);
  return items.length ? items : null;
};

const resolveUserId = () => {
  const userId = String(elements.a2aUserId?.value || "").trim();
  if (userId) {
    return userId;
  }
  return String(elements.userId?.value || "").trim() || "a2a";
};

// 根据表单生成 params，允许被 Params JSON 覆盖
const buildParamsFromForm = (method) => {
  if (method === "SendMessage" || method === "SendStreamingMessage") {
    const messageText = String(elements.a2aMessage?.value || "").trim();
    if (!messageText) {
      throw new Error(t("a2a.error.messageRequired"));
    }
    const message = {
      role: "user",
      parts: [{ text: messageText }],
    };
    const taskId = String(elements.a2aTaskId?.value || "").trim();
    const contextId = String(elements.a2aContextId?.value || "").trim();
    if (taskId) {
      message.taskId = taskId.startsWith("tasks/") ? taskId.replace("tasks/", "") : taskId;
    }
    if (contextId) {
      message.contextId = contextId;
    }
    const params = {
      userId: resolveUserId(),
      message,
    };
    const configuration = {};
    const historyLength = String(elements.a2aHistoryLength?.value || "").trim();
    if (historyLength) {
      configuration.historyLength = Number(historyLength);
    }
    if (elements.a2aBlocking?.checked) {
      configuration.blocking = true;
    }
    if (Object.keys(configuration).length) {
      params.configuration = configuration;
    }
    const toolNames = splitToolNames(elements.a2aToolNames?.value || "");
    if (toolNames) {
      params.toolNames = toolNames;
    }
    const modelName = String(elements.a2aModelName?.value || "").trim();
    if (modelName) {
      params.modelName = modelName;
    }
    return params;
  }

  if (method === "GetTask") {
    return {
      name: normalizeTaskName(elements.a2aTaskId?.value || ""),
      historyLength: Number(elements.a2aHistoryLength?.value || 0) || undefined,
    };
  }

  if (method === "ListTasks") {
    const params = {};
    const contextId = String(elements.a2aContextId?.value || "").trim();
    if (contextId) {
      params.contextId = contextId;
    }
    const status = String(elements.a2aStatusFilter?.value || "").trim();
    if (status) {
      params.status = status;
    }
    const pageSize = Number(elements.a2aPageSize?.value || 0);
    if (Number.isFinite(pageSize) && pageSize > 0) {
      params.pageSize = pageSize;
    }
    const pageToken = String(elements.a2aPageToken?.value || "").trim();
    if (pageToken) {
      params.pageToken = pageToken;
    }
    const historyLength = Number(elements.a2aHistoryLength?.value || 0);
    if (Number.isFinite(historyLength) && historyLength > 0) {
      params.historyLength = historyLength;
    }
    if (elements.a2aIncludeArtifacts?.checked) {
      params.includeArtifacts = true;
    }
    return params;
  }

  if (method === "CancelTask" || method === "SubscribeToTask") {
    return {
      name: normalizeTaskName(elements.a2aTaskId?.value || ""),
    };
  }

  return {};
};

const buildJsonRpcPayload = () => {
  const method = String(elements.a2aMethod?.value || "").trim() || "SendMessage";
  const paramsJson = String(elements.a2aParamsJson?.value || "").trim();
  let params = null;
  if (paramsJson) {
    try {
      params = JSON.parse(paramsJson);
    } catch (error) {
      throw new Error(t("a2a.error.paramsJson"));
    }
  }
  if (!params) {
    params = buildParamsFromForm(method);
  }
  const requestId = String(elements.a2aRequestId?.value || "").trim() || generateRequestId();
  return {
    jsonrpc: "2.0",
    id: requestId,
    method,
    params,
  };
};

// 解析 SSE 块，提取 data 行
const parseSseBlock = (block) => {
  const lines = block.split(/\r?\n/);
  const dataLines = [];
  lines.forEach((line) => {
    if (line.startsWith("data:")) {
      dataLines.push(line.slice(5).trim());
    }
  });
  if (!dataLines.length) {
    return null;
  }
  const text = dataLines.join("\n");
  try {
    return JSON.parse(text);
  } catch (error) {
    return text;
  }
};

const summarizeStreamEvent = (payload) => {
  if (!payload || typeof payload !== "object") {
    return t("a2a.event.unknown");
  }
  if (payload.task) {
    const taskId = payload.task.id || "";
    const state = payload.task.status?.state || "";
    return `task ${taskId} ${state}`.trim();
  }
  if (payload.statusUpdate) {
    const state = payload.statusUpdate.status?.state || "";
    const final = payload.statusUpdate.final ? "final" : "";
    return `status ${state} ${final}`.trim();
  }
  if (payload.artifactUpdate) {
    const name = payload.artifactUpdate.artifact?.name || payload.artifactUpdate.artifact?.artifactId || "";
    return `artifact ${name}`.trim();
  }
  if (payload.message) {
    const role = payload.message.role || "";
    return `message ${role}`.trim();
  }
  return t("a2a.event.unknown");
};

const streamResponse = async (response) => {
  const reader = response.body?.getReader();
  if (!reader) {
    throw new Error(t("a2a.error.streamNotSupported"));
  }
  const decoder = new TextDecoder();
  let buffer = "";
  const events = [];
  while (true) {
    const { value, done } = await reader.read();
    if (done) {
      break;
    }
    buffer += decoder.decode(value, { stream: true });
    const parts = buffer.split(/\n\n/);
    buffer = parts.pop() || "";
    parts.forEach((part) => {
      const parsed = parseSseBlock(part);
      if (parsed === null) {
        return;
      }
      events.push(parsed);
      const summary = summarizeStreamEvent(parsed);
      appendLogBlock(elements.a2aEventLog, summary, parsed);
      if (elements.a2aResponse) {
        elements.a2aResponse.textContent = stringifyPayload(events);
      }
    });
  }
  if (buffer.trim()) {
    const parsed = parseSseBlock(buffer);
    if (parsed !== null) {
      events.push(parsed);
      const summary = summarizeStreamEvent(parsed);
      appendLogBlock(elements.a2aEventLog, summary, parsed);
      if (elements.a2aResponse) {
        elements.a2aResponse.textContent = stringifyPayload(events);
      }
    }
  }
};

const handleSend = async () => {
  updateHeaderError("");
  clearLogs();
  if (state.runtime.a2aStreaming) {
    return;
  }
  const endpoint = resolveEndpoint();
  if (!endpoint) {
    appendLogBlock(elements.a2aEventLog, t("a2a.error.endpointRequired"), {});
    return;
  }
  let payload;
  try {
    payload = buildJsonRpcPayload();
  } catch (error) {
    appendLogBlock(elements.a2aEventLog, t("a2a.event.error"), error.message || String(error));
    return;
  }
  if (elements.a2aRequestPreview) {
    elements.a2aRequestPreview.textContent = stringifyPayload(payload);
  }

  const method = payload.method;
  const isStreaming = STREAM_METHODS.has(method);
  const headers = buildHeaders(isStreaming);
  const controller = new AbortController();
  state.runtime.a2aController = controller;
  setStreamingState(isStreaming);

  try {
    const response = await fetch(endpoint, {
      method: "POST",
      headers,
      body: JSON.stringify(payload),
      signal: controller.signal,
    });
    if (!response.ok) {
      const text = await response.text();
      appendLogBlock(elements.a2aEventLog, t("a2a.event.error"), text || response.status);
      if (elements.a2aResponse) {
        elements.a2aResponse.textContent = text || String(response.status);
      }
      return;
    }
    if (isStreaming) {
      await streamResponse(response);
      appendLogBlock(elements.a2aEventLog, t("a2a.event.streamFinished"), {});
      return;
    }
    const data = await response.json();
    if (elements.a2aResponse) {
      elements.a2aResponse.textContent = stringifyPayload(data);
    }
    appendLogBlock(elements.a2aEventLog, t("a2a.event.response"), data);
  } catch (error) {
    if (error.name === "AbortError") {
      appendLogBlock(elements.a2aEventLog, t("a2a.event.aborted"), {});
    } else {
      appendLogBlock(elements.a2aEventLog, t("a2a.event.error"), error.message || String(error));
    }
  } finally {
    setStreamingState(false);
    state.runtime.a2aController = null;
  }
};

const handleStop = () => {
  if (state.runtime.a2aController) {
    state.runtime.a2aController.abort();
  }
  setStreamingState(false);
};

const handleAgentCard = async () => {
  updateHeaderError("");
  const url = resolveAgentCardUrl();
  if (!url) {
    appendLogBlock(elements.a2aEventLog, t("a2a.error.endpointRequired"), {});
    return;
  }
  const headers = buildHeaders(false);
  try {
    const response = await fetch(url, { headers });
    const data = await response.json();
    if (elements.a2aResponse) {
      elements.a2aResponse.textContent = stringifyPayload(data);
    }
    appendLogBlock(elements.a2aEventLog, t("a2a.event.agentCard"), data);
  } catch (error) {
    appendLogBlock(elements.a2aEventLog, t("a2a.event.error"), error.message || String(error));
  }
};

const syncInputs = () => {
  writeA2aState({
    endpoint: elements.a2aEndpoint?.value || "",
    version: elements.a2aVersion?.value || "",
    authType: elements.a2aAuthType?.value || "apiKey",
    useGlobalKey: Boolean(elements.a2aUseGlobalKey?.checked),
    apiKey: elements.a2aApiKey?.value || "",
    headers: elements.a2aHeaders?.value || "",
    method: elements.a2aMethod?.value || "",
    requestId: elements.a2aRequestId?.value || "",
    userId: elements.a2aUserId?.value || "",
    taskId: elements.a2aTaskId?.value || "",
    contextId: elements.a2aContextId?.value || "",
    message: elements.a2aMessage?.value || "",
    toolNames: elements.a2aToolNames?.value || "",
    modelName: elements.a2aModelName?.value || "",
    historyLength: elements.a2aHistoryLength?.value || "",
    pageSize: elements.a2aPageSize?.value || "",
    pageToken: elements.a2aPageToken?.value || "",
    statusFilter: elements.a2aStatusFilter?.value || "",
    blocking: Boolean(elements.a2aBlocking?.checked),
    includeArtifacts: Boolean(elements.a2aIncludeArtifacts?.checked),
    paramsJson: elements.a2aParamsJson?.value || "",
  });
};

const applyStoredState = () => {
  const stored = readA2aState();
  if (elements.a2aEndpoint) {
    if (stored.endpoint) {
      elements.a2aEndpoint.value = stored.endpoint;
      elements.a2aEndpoint.dataset.manual = "true";
    } else {
      syncEndpointDefault();
    }
  }
  if (elements.a2aVersion) {
    elements.a2aVersion.value = stored.version || "1.0";
  }
  if (elements.a2aAuthType) {
    elements.a2aAuthType.value = stored.authType || "apiKey";
  }
  if (elements.a2aUseGlobalKey) {
    elements.a2aUseGlobalKey.checked = stored.useGlobalKey !== false;
  }
  if (elements.a2aApiKey) {
    elements.a2aApiKey.value = stored.apiKey || "";
  }
  if (elements.a2aHeaders) {
    elements.a2aHeaders.value = stored.headers || "";
  }
  if (elements.a2aMethod) {
    elements.a2aMethod.value = stored.method || "SendMessage";
  }
  if (elements.a2aRequestId) {
    elements.a2aRequestId.value = stored.requestId || "";
  }
  if (elements.a2aUserId) {
    elements.a2aUserId.value = stored.userId || elements.userId?.value || "";
  }
  if (elements.a2aTaskId) {
    elements.a2aTaskId.value = stored.taskId || "";
  }
  if (elements.a2aContextId) {
    elements.a2aContextId.value = stored.contextId || "";
  }
  if (elements.a2aMessage) {
    elements.a2aMessage.value = stored.message || "";
  }
  if (elements.a2aToolNames) {
    elements.a2aToolNames.value = stored.toolNames || "";
  }
  if (elements.a2aModelName) {
    elements.a2aModelName.value = stored.modelName || "";
  }
  if (elements.a2aHistoryLength) {
    elements.a2aHistoryLength.value = stored.historyLength || "";
  }
  if (elements.a2aPageSize) {
    elements.a2aPageSize.value = stored.pageSize || "";
  }
  if (elements.a2aPageToken) {
    elements.a2aPageToken.value = stored.pageToken || "";
  }
  if (elements.a2aStatusFilter) {
    elements.a2aStatusFilter.value = stored.statusFilter || "";
  }
  if (elements.a2aBlocking) {
    elements.a2aBlocking.checked = Boolean(stored.blocking);
  }
  if (elements.a2aIncludeArtifacts) {
    elements.a2aIncludeArtifacts.checked = Boolean(stored.includeArtifacts);
  }
  if (elements.a2aParamsJson) {
    elements.a2aParamsJson.value = stored.paramsJson || "";
  }
};

const bindInputs = () => {
  if (elements.a2aEndpoint) {
    elements.a2aEndpoint.addEventListener("input", () => {
      elements.a2aEndpoint.dataset.manual = elements.a2aEndpoint.value.trim() ? "true" : "false";
      syncInputs();
    });
  }
  if (elements.a2aUseGlobalKey && elements.a2aApiKey) {
    const toggleApiKey = () => {
      const useGlobal = elements.a2aUseGlobalKey.checked;
      elements.a2aApiKey.disabled = useGlobal;
    };
    toggleApiKey();
    elements.a2aUseGlobalKey.addEventListener("change", () => {
      toggleApiKey();
      syncInputs();
    });
  }

  [
    "a2aVersion",
    "a2aAuthType",
    "a2aApiKey",
    "a2aHeaders",
    "a2aMethod",
    "a2aRequestId",
    "a2aUserId",
    "a2aTaskId",
    "a2aContextId",
    "a2aMessage",
    "a2aToolNames",
    "a2aModelName",
    "a2aHistoryLength",
    "a2aPageSize",
    "a2aPageToken",
    "a2aStatusFilter",
    "a2aBlocking",
    "a2aIncludeArtifacts",
    "a2aParamsJson",
  ].forEach((key) => {
    const el = elements[key];
    if (!el) {
      return;
    }
    el.addEventListener("change", syncInputs);
  });

  if (elements.apiBase) {
    elements.apiBase.addEventListener("change", syncEndpointDefault);
  }
};

export const initA2aPanel = () => {
  applyStoredState();
  syncEndpointDefault();
  bindInputs();
  setStreamingState(false);
  state.panelLoaded.a2a = true;
  if (elements.a2aConnectBtn) {
    elements.a2aConnectBtn.addEventListener("click", openA2aConnectionModal);
  }
  if (elements.a2aAdvancedBtn) {
    elements.a2aAdvancedBtn.addEventListener("click", openA2aAdvancedModal);
  }
  if (elements.a2aConnectionClose) {
    elements.a2aConnectionClose.addEventListener("click", closeA2aConnectionModal);
  }
  if (elements.a2aConnectionCloseBtn) {
    elements.a2aConnectionCloseBtn.addEventListener("click", closeA2aConnectionModal);
  }
  if (elements.a2aConnectionModal) {
    elements.a2aConnectionModal.addEventListener("click", (event) => {
      if (event.target === elements.a2aConnectionModal) {
        closeA2aConnectionModal();
      }
    });
  }
  if (elements.a2aAdvancedClose) {
    elements.a2aAdvancedClose.addEventListener("click", closeA2aAdvancedModal);
  }
  if (elements.a2aAdvancedCloseBtn) {
    elements.a2aAdvancedCloseBtn.addEventListener("click", closeA2aAdvancedModal);
  }
  if (elements.a2aAdvancedModal) {
    elements.a2aAdvancedModal.addEventListener("click", (event) => {
      if (event.target === elements.a2aAdvancedModal) {
        closeA2aAdvancedModal();
      }
    });
  }
  if (elements.a2aSendBtn) {
    elements.a2aSendBtn.addEventListener("click", handleSend);
  }
  if (elements.a2aStopBtn) {
    elements.a2aStopBtn.addEventListener("click", handleStop);
  }
  if (elements.a2aClearBtn) {
    elements.a2aClearBtn.addEventListener("click", clearLogs);
  }
  if (elements.a2aAgentCardBtn) {
    elements.a2aAgentCardBtn.addEventListener("click", handleAgentCard);
  }
};
