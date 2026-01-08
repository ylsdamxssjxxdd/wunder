// 前端统一配置：集中管理默认值与页面行为参数
const APP_CONFIG_DEFAULTS = {
  // 默认 API 地址：用于初始化调试面板输入�?
  defaultApiBase: "http://127.0.0.1:8000/wunder",
  // 默认 API Key：为空表示由用户自行输入
  defaultApiKey: "",
  // 默认用户 ID：用于初始化调试面板输入�?
  defaultUserId: "demo_user",
  // 调试面板问题预设：右键问题区域快速填�?
  debugQuestionPresets: {
    "zh-CN": [
      "你好，介绍一�?wunder 的核心能力�?,
      "请列出当前可用工具，并说明用途�?,
      "用python绘制一个爱心保存到本地png",
      "广州今天的天气如何？",
    ],
    "en-US": [
      "Hi, introduce wunder's core capabilities.",
      "List the available tools and explain their purposes.",
      "Use Python to draw a heart and save it as a local PNG.",
      "What's the weather in Guangzhou today?",
    ],
  },
  // 默认首屏面板
  defaultPanel: "monitor",
  // 监控轮询间隔（毫秒）
  monitorPollIntervalMs: 3000,
  // 系统提示词自动刷新延迟（毫秒�?
  promptReloadDelayMs: 300,
  // 默认语言：用于控制前端显示与请求语言
  language: "zh-CN",
};

export const APP_CONFIG = { ...APP_CONFIG_DEFAULTS };

const CONFIG_STORAGE_KEY = "wunder_app_config";

// 规范化本地存储配置，避免异常字段污染
const sanitizeConfig = (raw) => {
  if (!raw || typeof raw !== "object") {
    return {};
  }
  const next = {};
  if (typeof raw.defaultApiBase === "string") {
    next.defaultApiBase = raw.defaultApiBase.trim();
  }
  if (typeof raw.defaultApiKey === "string") {
    next.defaultApiKey = raw.defaultApiKey.trim();
  }
  if (typeof raw.defaultUserId === "string") {
    next.defaultUserId = raw.defaultUserId.trim();
  }
  if (typeof raw.defaultPanel === "string") {
    next.defaultPanel = raw.defaultPanel.trim();
  }
  if (typeof raw.language === "string") {
    next.language = raw.language.trim();
  }
  const monitorInterval = Number(raw.monitorPollIntervalMs);
  if (Number.isFinite(monitorInterval) && monitorInterval > 0) {
    next.monitorPollIntervalMs = Math.round(monitorInterval);
  }
  const promptDelay = Number(raw.promptReloadDelayMs);
  if (Number.isFinite(promptDelay) && promptDelay > 0) {
    next.promptReloadDelayMs = Math.round(promptDelay);
  }
  return next;
};

// 读取本地存储配置，合并到默认配置�?
export const readStoredConfig = () => {
  try {
    const raw = localStorage.getItem(CONFIG_STORAGE_KEY);
    if (!raw) {
      return {};
    }
    const parsed = JSON.parse(raw);
    return sanitizeConfig(parsed);
  } catch (error) {
    return {};
  }
};

// 应用本地配置�?APP_CONFIG，用于初始化默认�?
export const applyStoredConfig = () => {
  const stored = readStoredConfig();
  Object.assign(APP_CONFIG, APP_CONFIG_DEFAULTS, stored);
  return { ...APP_CONFIG };
};

// 写入新的配置补丁，并同步 APP_CONFIG
export const updateStoredConfig = (patch) => {
  const current = readStoredConfig();
  const next = sanitizeConfig({ ...current, ...(patch || {}) });
  try {
    localStorage.setItem(CONFIG_STORAGE_KEY, JSON.stringify(next));
  } catch (error) {
    // 忽略本地存储不可用的情况
  }
  Object.assign(APP_CONFIG, APP_CONFIG_DEFAULTS, next);
  return { ...APP_CONFIG };
};

// 清空本地配置并恢复默认�?
export const resetStoredConfig = () => {
  try {
    localStorage.removeItem(CONFIG_STORAGE_KEY);
  } catch (error) {
    // 忽略本地存储不可用的情况
  }
  Object.assign(APP_CONFIG, APP_CONFIG_DEFAULTS);
  return { ...APP_CONFIG };
};

export const getDefaultConfig = () => ({ ...APP_CONFIG_DEFAULTS });

// 更新默认配置（不覆盖已存储配置）
export const updateDefaultConfig = (patch) => {
  const nextDefaults = sanitizeConfig({ ...APP_CONFIG_DEFAULTS, ...(patch || {}) });
  Object.assign(APP_CONFIG_DEFAULTS, nextDefaults);
  return { ...APP_CONFIG_DEFAULTS };
};

// 应用默认配置到界面输入框，避免在 HTML 中硬编码默认�?
export const applyDefaultConfig = (elements) => {
  if (elements.apiBase && !elements.apiBase.value.trim()) {
    elements.apiBase.value = APP_CONFIG.defaultApiBase;
  }
  if (elements.apiKey && !elements.apiKey.value.trim()) {
    elements.apiKey.value = APP_CONFIG.defaultApiKey;
  }
  if (elements.userId && !elements.userId.value.trim()) {
    elements.userId.value = APP_CONFIG.defaultUserId;
  }
};
