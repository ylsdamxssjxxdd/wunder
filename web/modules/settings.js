import {
  APP_CONFIG,
  applyStoredConfig,
  resetStoredConfig,
  updateStoredConfig,
} from "../app.config.js?v=20260110-04";
import { elements } from "./elements.js?v=20260110-06";
import { state } from "./state.js";
import { toggleMonitorPolling } from "./monitor.js?v=20260110-08";
import { notify } from "./notify.js";
import {
  getLanguageLabel,
  getSupportedLanguages,
  normalizeLanguage,
  setLanguage,
  t,
} from "./i18n.js?v=20260110-06";
import { normalizeApiBase } from "./utils.js?v=20251229-02";
import { getWunderBase } from "./api.js";

const MIN_MONITOR_INTERVAL_MS = 500;
const MIN_PROMPT_DELAY_MS = 50;
const MIN_MAX_ACTIVE_SESSIONS = 1;

const serverSettings = {
  maxActiveSessions: null,
};

// 解析数字输入，确保落在合理区间内
const resolveNumberInput = (rawValue, fallback, minValue) => {
  const parsed = Number(rawValue);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    return fallback;
  }
  const rounded = Math.round(parsed);
  return Math.max(minValue, rounded);
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

// 同步 API 配置输入，确保变更后写回本地缓存
const syncApiInputs = (nextBase, nextKey) => {
  if (elements.apiBase && elements.apiBase.value !== nextBase) {
    elements.apiBase.value = nextBase;
    elements.apiBase.dispatchEvent(new Event("change", { bubbles: true }));
  }
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

const applyServerSettings = (maxActiveSessions) => {
  if (!elements.settingsMaxActiveSessions) {
    return;
  }
  if (Number.isFinite(maxActiveSessions)) {
    elements.settingsMaxActiveSessions.value = String(Math.max(MIN_MAX_ACTIVE_SESSIONS, maxActiveSessions));
    return;
  }
  elements.settingsMaxActiveSessions.value = "";
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

const getAuthHeaders = () => {
  const apiKey = String(elements.apiKey?.value || "").trim();
  if (!apiKey) {
    return undefined;
  }
  return { "X-API-Key": apiKey };
};

const fetchServerSettings = async () => {
  const wunderBase = getWunderBase();
  if (!wunderBase) {
    throw new Error(t("settings.error.apiBase"));
  }
  const response = await fetch(`${wunderBase}/admin/server`, {
    headers: getAuthHeaders(),
  });
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const payload = await response.json();
  return payload?.server || {};
};

const updateServerSettings = async (maxActiveSessions) => {
  const wunderBase = getWunderBase();
  if (!wunderBase) {
    throw new Error(t("settings.error.apiBase"));
  }
  const response = await fetch(`${wunderBase}/admin/server`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      ...getAuthHeaders(),
    },
    body: JSON.stringify({ max_active_sessions: maxActiveSessions }),
  });
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const payload = await response.json();
  return payload?.server || {};
};

const loadServerSettings = async (options = {}) => {
  if (!elements.settingsMaxActiveSessions) {
    return;
  }
  const silent = options.silent === true;
  try {
    const server = await fetchServerSettings();
    serverSettings.maxActiveSessions = server.max_active_sessions ?? null;
    applyServerSettings(serverSettings.maxActiveSessions);
  } catch (error) {
    if (!silent) {
      notify(t("settings.toast.serverLoadFailed", { message: error.message }), "error");
    }
  }
};

// 保存设置并应用到运行时
const handleSaveSettings = async () => {
  const previous = { ...APP_CONFIG };
  const nextApiBase = normalizeApiBase(elements.apiBase?.value || "");
  const nextApiKey = String(elements.apiKey?.value || "");
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
  syncApiInputs(updated.defaultApiBase, updated.defaultApiKey);
  syncDefaultUserId(updated.defaultUserId, previous.defaultUserId);

  if (updated.language !== previous.language) {
    setLanguage(updated.language, { force: true });
    state.runtime.promptNeedsRefresh = true;
  }

  if (updated.monitorPollIntervalMs !== previous.monitorPollIntervalMs) {
    refreshMonitorInterval(updated.monitorPollIntervalMs);
  }

  if (!updated.defaultApiBase) {
    notify(t("settings.toast.apiBaseEmpty"), "warn");
  }
  notify(t("settings.toast.saved"), "success");

  if (elements.settingsMaxActiveSessions) {
    const nextMaxActiveSessions = resolveMaxActiveSessions();
    if (nextMaxActiveSessions === null) {
      applyServerSettings(serverSettings.maxActiveSessions);
      return;
    }
    if (nextMaxActiveSessions !== serverSettings.maxActiveSessions) {
      try {
        const server = await updateServerSettings(nextMaxActiveSessions);
        serverSettings.maxActiveSessions = server.max_active_sessions ?? nextMaxActiveSessions;
        applyServerSettings(serverSettings.maxActiveSessions);
      } catch (error) {
        notify(
          t("settings.toast.serverUpdateFailed", { message: error.message }),
          "error"
        );
        applyServerSettings(serverSettings.maxActiveSessions);
      }
    } else {
      applyServerSettings(serverSettings.maxActiveSessions);
    }
  }
};

// 恢复默认设置并同步到界面
const handleResetSettings = async () => {
  const previous = { ...APP_CONFIG };
  const defaults = resetStoredConfig();
  applySettingsForm(defaults);
  syncApiInputs(defaults.defaultApiBase, defaults.defaultApiKey);
  syncDefaultUserId(defaults.defaultUserId, previous.defaultUserId);
  refreshMonitorInterval(defaults.monitorPollIntervalMs);
  setLanguage(defaults.language, { force: true });
  state.runtime.promptNeedsRefresh = true;
  await loadServerSettings({ silent: true });
  notify(t("settings.toast.reset"), "success");
};

// 初始化设置面板交互
export const initSettingsPanel = () => {
  applyStoredConfig();
  renderLanguageOptions();
  applySettingsForm(APP_CONFIG);
  loadServerSettings({ silent: true }).catch(() => {});
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
  window.addEventListener("wunder:language-changed", renderLanguageOptions);
};


