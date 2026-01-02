import {
  APP_CONFIG,
  applyStoredConfig,
  resetStoredConfig,
  updateStoredConfig,
} from "../app.config.js";
import { elements } from "./elements.js?v=20251231-03";
import { state } from "./state.js";
import { toggleMonitorPolling } from "./monitor.js?v=20251231-01";
import { notify } from "./notify.js";
import { normalizeApiBase } from "./utils.js?v=20251229-02";

const MIN_MONITOR_INTERVAL_MS = 500;
const MIN_PROMPT_DELAY_MS = 50;

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
};

// 保存设置并应用到运行时
const handleSaveSettings = () => {
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
  const updated = updateStoredConfig({
    defaultApiBase: nextApiBase,
    defaultApiKey: nextApiKey,
    defaultUserId: nextDefaultUserId,
    defaultPanel: nextDefaultPanel,
    monitorPollIntervalMs: nextMonitorInterval,
    promptReloadDelayMs: nextPromptDelay,
  });

  applySettingsForm(updated);
  syncApiInputs(updated.defaultApiBase, updated.defaultApiKey);
  syncDefaultUserId(updated.defaultUserId, previous.defaultUserId);

  if (updated.monitorPollIntervalMs !== previous.monitorPollIntervalMs) {
    refreshMonitorInterval(updated.monitorPollIntervalMs);
  }

  if (!updated.defaultApiBase) {
    notify("API 地址为空，可能导致调试请求失败。", "warn");
  }
  notify("系统设置已保存。", "success");
};

// 恢复默认设置并同步到界面
const handleResetSettings = () => {
  const previous = { ...APP_CONFIG };
  const defaults = resetStoredConfig();
  applySettingsForm(defaults);
  syncApiInputs(defaults.defaultApiBase, defaults.defaultApiKey);
  syncDefaultUserId(defaults.defaultUserId, previous.defaultUserId);
  refreshMonitorInterval(defaults.monitorPollIntervalMs);
  notify("已恢复默认设置。", "success");
};

// 初始化设置面板交互
export const initSettingsPanel = () => {
  applyStoredConfig();
  applySettingsForm(APP_CONFIG);
  if (elements.settingsSaveBtn) {
    elements.settingsSaveBtn.addEventListener("click", handleSaveSettings);
  }
  if (elements.settingsResetBtn) {
    elements.settingsResetBtn.addEventListener("click", handleResetSettings);
  }
};
