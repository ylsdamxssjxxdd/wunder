import {
  APP_CONFIG,
  applyStoredConfig,
  resetStoredConfig,
  updateDefaultConfig,
  updateStoredConfig,
} from "../app.config.js?v=20260110-04";
import { elements } from "./elements.js?v=20260214-01";
import { state } from "./state.js";
import { toggleMonitorPolling } from "./monitor.js?v=20260113-01";
import { notify } from "./notify.js";
import {
  getLanguageLabel,
  getSupportedLanguages,
  normalizeLanguage,
  setLanguage,
  t,
} from "./i18n.js?v=20260214-01";
import { getWunderBase } from "./api.js";
import { getAuthHeaders } from "./admin-auth.js?v=20260120-01";

const MIN_MONITOR_INTERVAL_MS = 500;
const MIN_PROMPT_DELAY_MS = 50;
const MIN_MAX_ACTIVE_SESSIONS = 1;

const serverSettings = {
  maxActiveSessions: null,
  streamChunkSize: null,
  chatStreamChannel: "ws",
};
const securitySettings = {
  apiKey: "",
  allowCommands: [],
  allowPaths: [],
  denyGlobs: [],
};
const sandboxSettings = {
  enabled: null,
  endpoint: "",
  containerRoot: "",
  network: "",
  readonlyRootfs: null,
  idleTtlS: null,
  timeoutS: null,
  resources: {
    cpu: null,
    memoryMb: null,
    pids: null,
  },
};
const observabilitySettings = {
  logLevel: "",
  monitorEventLimit: null,
  monitorPayloadMaxChars: null,
  monitorDropEventTypes: [],
};
const corsSettings = {
  allowOrigins: [],
  allowMethods: [],
  allowHeaders: [],
  allowCredentials: null,
};
let adminDefaultsLoaded = false;
let adminDefaultsLoading = null;

// 解析数字输入，确保落在合理区间内
const resolveNumberInput = (rawValue, fallback, minValue) => {
  const parsed = Number(rawValue);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    return fallback;
  }
  const rounded = Math.round(parsed);
  return Math.max(minValue, rounded);
};

const resolveOptionalNumber = (rawValue, fallback, minValue, options = {}) => {
  const cleaned = String(rawValue ?? "").trim();
  if (!cleaned) {
    return Number.isFinite(fallback) ? fallback : null;
  }
  const parsed = Number(cleaned);
  if (!Number.isFinite(parsed)) {
    return Number.isFinite(fallback) ? fallback : null;
  }
  const round = options.round !== false;
  const value = round ? Math.round(parsed) : parsed;
  const min = Number.isFinite(minValue) ? minValue : 0;
  return Math.max(min, value);
};

const normalizeTextList = (rawValue) =>
  String(rawValue || "")
    .split(/\r?\n|,/)
    .map((item) => item.trim())
    .filter(Boolean);

const renderTextList = (items, fallback = "") => {
  if (!Array.isArray(items) || items.length === 0) {
    return fallback;
  }
  return items.join("\n");
};

const resolveCorsList = (rawValue) => {
  const list = normalizeTextList(rawValue);
  return list.length ? list : ["*"];
};

// 确保下拉框值可用，避免缓存旧值导致异常
const resolveSelectValue = (select, value) => {
  if (!select) {
    return value;
  }
  const options = Array.from(select.options || []).map((option) => option.value);
  if (options.includes(value)) {
    return value;
  }
  return options[0] || "";
};

// 渲染语言下拉选项，保持与后端 i18n 配置一致
const renderLanguageOptions = () => {
  if (!elements.settingsLanguage) {
    return;
  }
  const currentValue = elements.settingsLanguage.value || APP_CONFIG.language;
  const languages = getSupportedLanguages();
  elements.settingsLanguage.innerHTML = "";
  languages.forEach((code) => {
    const option = document.createElement("option");
    option.value = code;
    option.textContent = getLanguageLabel(code);
    elements.settingsLanguage.appendChild(option);
  });
  const resolved = resolveSelectValue(elements.settingsLanguage, normalizeLanguage(currentValue));
  elements.settingsLanguage.value = resolved;
};

// 默认 user_id 变更时同步到调试与提示词输入
const syncDefaultUserId = (nextDefault, previousDefault) => {
  if (!elements.userId) {
    return;
  }
  const current = String(elements.userId.value || "").trim();
  if (current && current !== previousDefault) {
    return;
  }
  elements.userId.value = nextDefault;
  elements.userId.dispatchEvent(new Event("change", { bubbles: true }));
};

// 同步 API Key 输入，确保变更后写回本地缓存
const syncApiInputs = (nextKey) => {
  if (elements.apiKey && elements.apiKey.value !== nextKey) {
    elements.apiKey.value = nextKey;
    elements.apiKey.dispatchEvent(new Event("change", { bubbles: true }));
  }
};

// 根据当前面板刷新监控轮询间隔
const refreshMonitorInterval = (intervalMs) => {
  if (state.runtime.activePanel === "monitor") {
    toggleMonitorPolling(true, { mode: "full", intervalMs, immediate: false });
    return;
  }
  if (state.runtime.activePanel === "users") {
    toggleMonitorPolling(true, { mode: "sessions", intervalMs, immediate: false });
  }
};

// 将配置值同步回设置页表单
const applySettingsForm = (config) => {
  if (elements.settingsDefaultUserId) {
    elements.settingsDefaultUserId.value = config.defaultUserId || "";
  }
  if (elements.settingsDefaultPanel) {
    const resolved = resolveSelectValue(elements.settingsDefaultPanel, config.defaultPanel || "");
    elements.settingsDefaultPanel.value = resolved;
  }
  if (elements.settingsMonitorInterval) {
    elements.settingsMonitorInterval.value = String(config.monitorPollIntervalMs || "");
  }
  if (elements.settingsPromptDelay) {
    elements.settingsPromptDelay.value = String(config.promptReloadDelayMs || "");
  }
  if (elements.settingsLanguage) {
    const resolved = resolveSelectValue(
      elements.settingsLanguage,
      String(config.language || "")
    );
    elements.settingsLanguage.value = resolved;
  }
};

const applyMaxActiveSessions = (maxActiveSessions) => {
  if (!elements.settingsMaxActiveSessions) {
    return;
  }
  if (Number.isFinite(maxActiveSessions)) {
    elements.settingsMaxActiveSessions.value = String(Math.max(MIN_MAX_ACTIVE_SESSIONS, maxActiveSessions));
    return;
  }
  elements.settingsMaxActiveSessions.value = "";
};

const applyStreamChunkSize = (streamChunkSize) => {
  if (!elements.settingsStreamChunkSize) {
    return;
  }
  if (Number.isFinite(streamChunkSize)) {
    elements.settingsStreamChunkSize.value = String(streamChunkSize);
    return;
  }
  elements.settingsStreamChunkSize.value = "";
};

const applyChatStreamChannel = (chatStreamChannel) => {
  if (!elements.settingsChatStreamChannel) {
    return;
  }
  const normalized = String(chatStreamChannel || "ws").trim().toLowerCase();
  const resolved = resolveSelectValue(
    elements.settingsChatStreamChannel,
    normalized === "sse" ? "sse" : "ws"
  );
  elements.settingsChatStreamChannel.value = resolved;
};

const applySandboxEnabled = (sandboxEnabled) => {
  if (!elements.settingsSandboxEnabled) {
    return;
  }
  if (typeof sandboxEnabled === "boolean") {
    elements.settingsSandboxEnabled.checked = sandboxEnabled;
    return;
  }
  elements.settingsSandboxEnabled.checked = true;
};

const applyServerSettings = (options = {}) => {
  applyMaxActiveSessions(options.maxActiveSessions);
  applyStreamChunkSize(options.streamChunkSize);
  applyChatStreamChannel(options.chatStreamChannel);
};

const applySecuritySettings = (options = {}) => {
  if (elements.settingsAllowCommands) {
    elements.settingsAllowCommands.value = renderTextList(options.allowCommands);
  }
  if (elements.settingsAllowPaths) {
    elements.settingsAllowPaths.value = renderTextList(options.allowPaths);
  }
  if (elements.settingsDenyGlobs) {
    elements.settingsDenyGlobs.value = renderTextList(options.denyGlobs);
  }
};

const applySandboxSettings = (options = {}) => {
  applySandboxEnabled(options.enabled);
  if (elements.settingsSandboxEndpoint) {
    elements.settingsSandboxEndpoint.value = options.endpoint || "";
  }
  if (elements.settingsSandboxContainerRoot) {
    elements.settingsSandboxContainerRoot.value = options.containerRoot || "";
  }
  if (elements.settingsSandboxNetwork) {
    elements.settingsSandboxNetwork.value = options.network || "";
  }
  if (elements.settingsSandboxReadonly) {
    elements.settingsSandboxReadonly.checked = options.readonlyRootfs === true;
  }
  if (elements.settingsSandboxIdleTtl) {
    elements.settingsSandboxIdleTtl.value = Number.isFinite(options.idleTtlS)
      ? String(options.idleTtlS)
      : "";
  }
  if (elements.settingsSandboxTimeout) {
    elements.settingsSandboxTimeout.value = Number.isFinite(options.timeoutS)
      ? String(options.timeoutS)
      : "";
  }
  if (elements.settingsSandboxCpu) {
    elements.settingsSandboxCpu.value = Number.isFinite(options.resources?.cpu)
      ? String(options.resources.cpu)
      : "";
  }
  if (elements.settingsSandboxMemory) {
    elements.settingsSandboxMemory.value = Number.isFinite(options.resources?.memoryMb)
      ? String(options.resources.memoryMb)
      : "";
  }
  if (elements.settingsSandboxPids) {
    elements.settingsSandboxPids.value = Number.isFinite(options.resources?.pids)
      ? String(options.resources.pids)
      : "";
  }
};

const applyObservabilitySettings = (options = {}) => {
  if (elements.settingsLogLevel) {
    const resolved = resolveSelectValue(
      elements.settingsLogLevel,
      String(options.logLevel || "info").toLowerCase()
    );
    elements.settingsLogLevel.value = resolved;
  }
  if (elements.settingsMonitorEventLimit) {
    elements.settingsMonitorEventLimit.value = Number.isFinite(options.monitorEventLimit)
      ? String(options.monitorEventLimit)
      : "";
  }
  if (elements.settingsMonitorPayloadLimit) {
    elements.settingsMonitorPayloadLimit.value = Number.isFinite(options.monitorPayloadMaxChars)
      ? String(options.monitorPayloadMaxChars)
      : "";
  }
  if (elements.settingsMonitorDropTypes) {
    elements.settingsMonitorDropTypes.value = renderTextList(options.monitorDropEventTypes);
  }
};

const applyCorsSettings = (options = {}) => {
  if (elements.settingsCorsOrigins) {
    elements.settingsCorsOrigins.value = renderTextList(options.allowOrigins, "*");
  }
  if (elements.settingsCorsMethods) {
    elements.settingsCorsMethods.value = renderTextList(options.allowMethods, "*");
  }
  if (elements.settingsCorsHeaders) {
    elements.settingsCorsHeaders.value = renderTextList(options.allowHeaders, "*");
  }
  if (elements.settingsCorsCredentials) {
    elements.settingsCorsCredentials.checked = options.allowCredentials === true;
  }
};

const resolveMaxActiveSessions = () => {
  const raw = String(elements.settingsMaxActiveSessions?.value || "").trim();
  if (!raw && !Number.isFinite(serverSettings.maxActiveSessions)) {
    return null;
  }
  const fallback = Number.isFinite(serverSettings.maxActiveSessions)
    ? serverSettings.maxActiveSessions
    : MIN_MAX_ACTIVE_SESSIONS;
  return resolveNumberInput(
    raw,
    fallback,
    MIN_MAX_ACTIVE_SESSIONS
  );
};

const resolveStreamChunkSize = () =>
  resolveOptionalNumber(
    elements.settingsStreamChunkSize?.value,
    serverSettings.streamChunkSize,
    0
  );

const resolveChatStreamChannel = () => {
  const raw = String(elements.settingsChatStreamChannel?.value || "").trim().toLowerCase();
  if (raw === "sse") {
    return "sse";
  }
  return "ws";
};

const resolveSandboxEnabled = () => {
  if (!elements.settingsSandboxEnabled) {
    return null;
  }
  return Boolean(elements.settingsSandboxEnabled.checked);
};

const fetchSystemSettings = async () => {
  const wunderBase = getWunderBase();
  if (!wunderBase) {
    throw new Error(t("settings.error.apiBase"));
  }
  const response = await fetch(`${wunderBase}/admin/system`, {
    headers: getAuthHeaders(),
  });
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const payload = await response.json();
  return payload || {};
};

const updateSystemSettings = async (requestBody = {}) => {
  const wunderBase = getWunderBase();
  if (!wunderBase) {
    throw new Error(t("settings.error.apiBase"));
  }
  if (!Object.keys(requestBody).length) {
    return {};
  }
  const response = await fetch(`${wunderBase}/admin/system`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      ...getAuthHeaders(),
    },
    body: JSON.stringify(requestBody),
  });
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  return response.json();
};

const applySystemSettings = (payload = {}) => {
  const server = payload.server || {};
  serverSettings.maxActiveSessions = Number.isFinite(server.max_active_sessions)
    ? server.max_active_sessions
    : null;
  serverSettings.streamChunkSize = Number.isFinite(server.stream_chunk_size)
    ? server.stream_chunk_size
    : null;
  serverSettings.chatStreamChannel =
    String(server.chat_stream_channel || "ws").trim().toLowerCase() === "sse" ? "sse" : "ws";
  applyServerSettings({
    maxActiveSessions: serverSettings.maxActiveSessions,
    streamChunkSize: serverSettings.streamChunkSize,
    chatStreamChannel: serverSettings.chatStreamChannel,
  });

  const security = payload.security || {};
  securitySettings.apiKey = typeof security.api_key === "string" ? security.api_key.trim() : "";
  securitySettings.allowCommands = Array.isArray(security.allow_commands)
    ? security.allow_commands
    : [];
  securitySettings.allowPaths = Array.isArray(security.allow_paths)
    ? security.allow_paths
    : [];
  securitySettings.denyGlobs = Array.isArray(security.deny_globs)
    ? security.deny_globs
    : [];
  applySecuritySettings(securitySettings);
  if (Object.prototype.hasOwnProperty.call(security, "api_key")) {
    applyDefaultApiKey(security.api_key);
  }

  const sandbox = payload.sandbox || {};
  sandboxSettings.enabled = typeof sandbox.enabled === "boolean" ? sandbox.enabled : true;
  sandboxSettings.endpoint = String(sandbox.endpoint || "").trim();
  sandboxSettings.containerRoot = String(sandbox.container_root || "").trim();
  sandboxSettings.network = String(sandbox.network || "").trim();
  sandboxSettings.readonlyRootfs = sandbox.readonly_rootfs === true;
  sandboxSettings.idleTtlS = Number.isFinite(sandbox.idle_ttl_s) ? sandbox.idle_ttl_s : null;
  sandboxSettings.timeoutS = Number.isFinite(sandbox.timeout_s) ? sandbox.timeout_s : null;
  const resources = sandbox.resources || {};
  sandboxSettings.resources.cpu = Number.isFinite(resources.cpu) ? resources.cpu : null;
  sandboxSettings.resources.memoryMb = Number.isFinite(resources.memory_mb)
    ? resources.memory_mb
    : null;
  sandboxSettings.resources.pids = Number.isFinite(resources.pids) ? resources.pids : null;
  applySandboxSettings(sandboxSettings);

  const observability = payload.observability || {};
  observabilitySettings.logLevel = String(observability.log_level || "").trim();
  observabilitySettings.monitorEventLimit = Number.isFinite(observability.monitor_event_limit)
    ? observability.monitor_event_limit
    : null;
  observabilitySettings.monitorPayloadMaxChars = Number.isFinite(
    observability.monitor_payload_max_chars
  )
    ? observability.monitor_payload_max_chars
    : null;
  observabilitySettings.monitorDropEventTypes = Array.isArray(
    observability.monitor_drop_event_types
  )
    ? observability.monitor_drop_event_types
    : [];
  applyObservabilitySettings(observabilitySettings);

  const cors = payload.cors || {};
  corsSettings.allowOrigins = Array.isArray(cors.allow_origins) ? cors.allow_origins : [];
  corsSettings.allowMethods = Array.isArray(cors.allow_methods) ? cors.allow_methods : [];
  corsSettings.allowHeaders = Array.isArray(cors.allow_headers) ? cors.allow_headers : [];
  corsSettings.allowCredentials =
    typeof cors.allow_credentials === "boolean" ? cors.allow_credentials : false;
  applyCorsSettings(corsSettings);
};

const loadSystemSettings = async (options = {}) => {
  const silent = options.silent === true;
  try {
    const payload = await fetchSystemSettings();
    applySystemSettings(payload);
  } catch (error) {
    if (!silent) {
      notify(t("settings.toast.systemLoadFailed", { message: error.message }), "error");
    }
  }
};

const fetchSecurityDefaults = async () => {
  const wunderBase = getWunderBase();
  if (!wunderBase) {
    throw new Error(t("settings.error.apiBase"));
  }
  const response = await fetch(`${wunderBase}/admin/security`, {
    headers: getAuthHeaders(),
  });
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const payload = await response.json();
  return payload?.security || {};
};

const applyDefaultApiKey = (apiKey) => {
  const cleaned = String(apiKey || "").trim();
  updateDefaultConfig({ defaultApiKey: cleaned });
  if (!elements.apiKey || elements.apiKey.value.trim()) {
    return;
  }
  elements.apiKey.value = cleaned;
  elements.apiKey.dispatchEvent(new Event("change", { bubbles: true }));
};

const setAdvancedModalVisible = (visible) => {
  if (!elements.settingsAdvancedModal) {
    return;
  }
  elements.settingsAdvancedModal.classList.toggle("active", visible);
  elements.settingsAdvancedModal.setAttribute("aria-hidden", visible ? "false" : "true");
  if (visible && elements.apiKey) {
    elements.apiKey.focus();
  }
};

const openAdvancedModal = () => {
  setAdvancedModalVisible(true);
  loadAdminDefaults({ silent: false }).catch(() => {});
};

const closeAdvancedModal = () => {
  setAdvancedModalVisible(false);
};

export const loadAdminDefaults = async (options = {}) => {
  if (adminDefaultsLoaded && !options.force) {
    return;
  }
  if (adminDefaultsLoading) {
    return adminDefaultsLoading;
  }
  adminDefaultsLoading = (async () => {
    try {
      const security = await fetchSecurityDefaults();
      applyDefaultApiKey(security.api_key);
      adminDefaultsLoaded = true;
    } catch (error) {
      if (!options.silent) {
        notify(t("settings.toast.advancedLoadFailed", { message: error.message }), "error");
      }
    } finally {
      adminDefaultsLoading = null;
    }
  })();
  return adminDefaultsLoading;
};

const buildSystemUpdatePayload = () => {
  const payload = {};

  if (
    elements.settingsMaxActiveSessions ||
    elements.settingsStreamChunkSize ||
    elements.settingsChatStreamChannel
  ) {
    const server = {};
    const nextMaxActiveSessions = resolveMaxActiveSessions();
    if (Number.isFinite(nextMaxActiveSessions)) {
      server.max_active_sessions = nextMaxActiveSessions;
    }
    const nextStreamChunkSize = resolveStreamChunkSize();
    if (Number.isFinite(nextStreamChunkSize)) {
      server.stream_chunk_size = nextStreamChunkSize;
    }
    if (elements.settingsChatStreamChannel) {
      server.chat_stream_channel = resolveChatStreamChannel();
    }
    if (Object.keys(server).length) {
      payload.server = server;
    }
  }

  if (
    elements.apiKey ||
    elements.settingsAllowCommands ||
    elements.settingsAllowPaths ||
    elements.settingsDenyGlobs
  ) {
    const security = {};
    if (elements.apiKey) {
      security.api_key = String(elements.apiKey.value || "").trim();
    }
    if (elements.settingsAllowCommands) {
      security.allow_commands = normalizeTextList(elements.settingsAllowCommands.value);
    }
    if (elements.settingsAllowPaths) {
      security.allow_paths = normalizeTextList(elements.settingsAllowPaths.value);
    }
    if (elements.settingsDenyGlobs) {
      security.deny_globs = normalizeTextList(elements.settingsDenyGlobs.value);
    }
    if (Object.keys(security).length) {
      payload.security = security;
    }
  }

  if (
    elements.settingsSandboxEnabled ||
    elements.settingsSandboxEndpoint ||
    elements.settingsSandboxContainerRoot ||
    elements.settingsSandboxNetwork ||
    elements.settingsSandboxReadonly ||
    elements.settingsSandboxIdleTtl ||
    elements.settingsSandboxTimeout ||
    elements.settingsSandboxCpu ||
    elements.settingsSandboxMemory ||
    elements.settingsSandboxPids
  ) {
    const sandbox = {};
    if (elements.settingsSandboxEnabled) {
      sandbox.enabled = resolveSandboxEnabled();
    }
    if (elements.settingsSandboxEndpoint) {
      sandbox.endpoint = String(elements.settingsSandboxEndpoint.value || "").trim();
    }
    if (elements.settingsSandboxContainerRoot) {
      sandbox.container_root = String(elements.settingsSandboxContainerRoot.value || "").trim();
    }
    if (elements.settingsSandboxNetwork) {
      sandbox.network = String(elements.settingsSandboxNetwork.value || "").trim();
    }
    if (elements.settingsSandboxReadonly) {
      sandbox.readonly_rootfs = Boolean(elements.settingsSandboxReadonly.checked);
    }
    if (elements.settingsSandboxIdleTtl) {
      const value = resolveOptionalNumber(
        elements.settingsSandboxIdleTtl.value,
        sandboxSettings.idleTtlS,
        0
      );
      if (value !== null) {
        sandbox.idle_ttl_s = value;
      }
    }
    if (elements.settingsSandboxTimeout) {
      const value = resolveOptionalNumber(
        elements.settingsSandboxTimeout.value,
        sandboxSettings.timeoutS,
        0
      );
      if (value !== null) {
        sandbox.timeout_s = value;
      }
    }
    const resources = {};
    if (elements.settingsSandboxCpu) {
      const value = resolveOptionalNumber(
        elements.settingsSandboxCpu.value,
        sandboxSettings.resources.cpu,
        0,
        { round: false }
      );
      if (value !== null) {
        resources.cpu = value;
      }
    }
    if (elements.settingsSandboxMemory) {
      const value = resolveOptionalNumber(
        elements.settingsSandboxMemory.value,
        sandboxSettings.resources.memoryMb,
        0
      );
      if (value !== null) {
        resources.memory_mb = value;
      }
    }
    if (elements.settingsSandboxPids) {
      const value = resolveOptionalNumber(
        elements.settingsSandboxPids.value,
        sandboxSettings.resources.pids,
        0
      );
      if (value !== null) {
        resources.pids = value;
      }
    }
    if (Object.keys(resources).length) {
      sandbox.resources = resources;
    }
    if (Object.keys(sandbox).length) {
      payload.sandbox = sandbox;
    }
  }

  if (
    elements.settingsLogLevel ||
    elements.settingsMonitorEventLimit ||
    elements.settingsMonitorPayloadLimit ||
    elements.settingsMonitorDropTypes
  ) {
    const observability = {};
    if (elements.settingsLogLevel) {
      observability.log_level = String(elements.settingsLogLevel.value || "").trim();
    }
    if (elements.settingsMonitorEventLimit) {
      const value = resolveOptionalNumber(
        elements.settingsMonitorEventLimit.value,
        observabilitySettings.monitorEventLimit,
        0
      );
      if (value !== null) {
        observability.monitor_event_limit = value;
      }
    }
    if (elements.settingsMonitorPayloadLimit) {
      const value = resolveOptionalNumber(
        elements.settingsMonitorPayloadLimit.value,
        observabilitySettings.monitorPayloadMaxChars,
        0
      );
      if (value !== null) {
        observability.monitor_payload_max_chars = value;
      }
    }
    if (elements.settingsMonitorDropTypes) {
      observability.monitor_drop_event_types = normalizeTextList(
        elements.settingsMonitorDropTypes.value
      );
    }
    if (Object.keys(observability).length) {
      payload.observability = observability;
    }
  }

  if (
    elements.settingsCorsOrigins ||
    elements.settingsCorsMethods ||
    elements.settingsCorsHeaders ||
    elements.settingsCorsCredentials
  ) {
    const cors = {};
    if (elements.settingsCorsOrigins) {
      cors.allow_origins = resolveCorsList(elements.settingsCorsOrigins.value);
    }
    if (elements.settingsCorsMethods) {
      cors.allow_methods = resolveCorsList(elements.settingsCorsMethods.value);
    }
    if (elements.settingsCorsHeaders) {
      cors.allow_headers = resolveCorsList(elements.settingsCorsHeaders.value);
    }
    if (elements.settingsCorsCredentials) {
      cors.allow_credentials = Boolean(elements.settingsCorsCredentials.checked);
    }
    if (Object.keys(cors).length) {
      payload.cors = cors;
    }
  }

  return payload;
};

// 保存设置并应用到运行时
const handleSaveSettings = async () => {
  const previous = { ...APP_CONFIG };
  const nextApiBase = getWunderBase();
  const nextApiKey = String(elements.apiKey?.value || "").trim();
  const nextDefaultUserId = String(elements.settingsDefaultUserId?.value || "").trim();
  const nextDefaultPanel = resolveSelectValue(
    elements.settingsDefaultPanel,
    String(elements.settingsDefaultPanel?.value || "").trim()
  );
  const nextMonitorInterval = resolveNumberInput(
    elements.settingsMonitorInterval?.value,
    APP_CONFIG.monitorPollIntervalMs,
    MIN_MONITOR_INTERVAL_MS
  );
  const nextPromptDelay = resolveNumberInput(
    elements.settingsPromptDelay?.value,
    APP_CONFIG.promptReloadDelayMs,
    MIN_PROMPT_DELAY_MS
  );
  const nextLanguage = normalizeLanguage(
    elements.settingsLanguage?.value || APP_CONFIG.language
  );
  const updated = updateStoredConfig({
    defaultApiBase: nextApiBase,
    defaultApiKey: nextApiKey,
    defaultUserId: nextDefaultUserId,
    defaultPanel: nextDefaultPanel,
    monitorPollIntervalMs: nextMonitorInterval,
    promptReloadDelayMs: nextPromptDelay,
    language: nextLanguage,
  });

  applySettingsForm(updated);
  syncApiInputs(updated.defaultApiKey);
  syncDefaultUserId(updated.defaultUserId, previous.defaultUserId);

  if (updated.language !== previous.language) {
    setLanguage(updated.language, { force: true });
    state.runtime.promptNeedsRefresh = true;
  }

  if (updated.monitorPollIntervalMs !== previous.monitorPollIntervalMs) {
    refreshMonitorInterval(updated.monitorPollIntervalMs);
  }

  notify(t("settings.toast.saved"), "success");

  const systemPayload = buildSystemUpdatePayload();
  if (!Object.keys(systemPayload).length) {
    return;
  }
  try {
    const system = await updateSystemSettings(systemPayload);
    applySystemSettings(system);
  } catch (error) {
    notify(t("settings.toast.systemUpdateFailed", { message: error.message }), "error");
    await loadSystemSettings({ silent: true });
  }
};

// 恢复默认设置并同步到界面
const handleResetSettings = async () => {
  const previous = { ...APP_CONFIG };
  const defaults = resetStoredConfig();
  applySettingsForm(defaults);
  syncApiInputs(defaults.defaultApiKey);
  syncDefaultUserId(defaults.defaultUserId, previous.defaultUserId);
  refreshMonitorInterval(defaults.monitorPollIntervalMs);
  setLanguage(defaults.language, { force: true });
  state.runtime.promptNeedsRefresh = true;
  await loadSystemSettings({ silent: true });
  notify(t("settings.toast.reset"), "success");
};

// 初始化设置面板交互
export const initSettingsPanel = () => {
  applyStoredConfig();
  renderLanguageOptions();
  applySettingsForm(APP_CONFIG);
  loadSystemSettings({ silent: true }).catch(() => {});
  if (elements.settingsSaveBtn) {
    elements.settingsSaveBtn.addEventListener("click", () => {
      handleSaveSettings().catch(() => {});
    });
  }
  if (elements.settingsResetBtn) {
    elements.settingsResetBtn.addEventListener("click", () => {
      handleResetSettings().catch(() => {});
    });
  }
  if (elements.settingsAdvancedBtn) {
    elements.settingsAdvancedBtn.addEventListener("click", openAdvancedModal);
  }
  if (elements.settingsAdvancedModal) {
    elements.settingsAdvancedModal.addEventListener("click", (event) => {
      if (event.target === elements.settingsAdvancedModal) {
        closeAdvancedModal();
      }
    });
  }
  if (elements.settingsAdvancedModalClose) {
    elements.settingsAdvancedModalClose.addEventListener("click", closeAdvancedModal);
  }
  if (elements.settingsAdvancedCancel) {
    elements.settingsAdvancedCancel.addEventListener("click", closeAdvancedModal);
  }
  if (elements.settingsAdvancedSave) {
    elements.settingsAdvancedSave.addEventListener("click", () => {
      handleSaveSettings()
        .then(closeAdvancedModal)
        .catch(() => {});
    });
  }
  window.addEventListener("wunder:language-changed", renderLanguageOptions);
};


