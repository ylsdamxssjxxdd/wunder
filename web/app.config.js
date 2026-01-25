// 前端统一配置：集中管理默认值与页面行为参数
const DEFAULT_API_BASE = (() => {
  if (typeof window !== "undefined" && window.location?.origin) {
    return `${window.location.origin}/wunder`;
  }
  return "http://127.0.0.1:18000/wunder";
})();

const APP_CONFIG_DEFAULTS = {
  // 默认 API 地址：用于初始化调试面板输入框
  defaultApiBase: DEFAULT_API_BASE,
  // 默认 API Key：为空表示由用户自行输入
  defaultApiKey: "",
  // 默认用户 ID：用于初始化调试面板输入框
  defaultUserId: "demo_user",
  // 调试面板问题预设：右键问题区域快速填充
  debugQuestionPresets: {
    "zh-CN": [
      "你好，介绍一下 wunder 的核心能力。",
      "请列出当前可用工具，并说明用途。",
      "用ptc绘制一个爱心保存到本地png",
      "请写一篇公文，下发寒假放假的通知，内容随意我只是测试",
    ],
    "en-US": [
      "Hi, introduce wunder's core capabilities.",
      "List the available tools and explain their purposes.",
      "Use Python to draw a heart and save it as a local PNG.",
      "What's the weather in Guangzhou today?",
    ],
  },

  // 长对话稳定性测试预设
  debugStabilityPresets: {
    "zh-CN": [
      {
        id: "long-dialogue-file",
        name: "长对话稳定性（文件链路）",
        stream: true,
        toolNames: ["最终回复", "列出文件", "读取文件", "写入文件", "搜索内容", "替换文本", "编辑文件"],
        steps: [
          "你现在是稳定性测试助手，请简短回答 'ready'，并声明将持续执行任务；不要使用问询面板或 a2ui。",
          "列出当前工作区根目录文件，说明是否为空。",
          "创建文件 health_check.txt，内容为 ok-{{timestamp}}，然后读取并回复内容。",
          "创建 a1.txt 到 a5.txt 五个小文件，内容分别为 file-1 到 file-5，并统计文件总数。",
          "搜索包含 'file-' 的文件并返回路径列表。",
          "将 a1.txt 到 a5.txt 的内容改为 ok-file-1 到 ok-file-5（可用替换或重写）。",
          "删除 a1.txt 到 a5.txt，并再次列出当前工作区文件。",
          "用三句话总结你刚才完成的操作。",
          "再次读取 health_check.txt 并确认内容未变化。",
          "最后输出一句话：stable-run-complete。"
        ],
      },
    ],
    "en-US": [
      {
        id: "long-dialogue-file",
        name: "Long Conversation Stability (file workflow)",
        stream: true,
        toolNames: ["最终回复", "列出文件", "读取文件", "写入文件", "搜索内容", "替换文本", "编辑文件"],
        steps: [
          "You are a stability test agent. Reply with 'ready' and confirm you will keep executing tasks; do not use question panel or a2ui.",
          "List files in the workspace root and say whether it is empty.",
          "Create health_check.txt with content ok-{{timestamp}}, then read and echo the content.",
          "Create five small files a1.txt to a5.txt with content file-1 to file-5, and report the total file count.",
          "Search for files containing 'file-' and return the path list.",
          "Update a1.txt to a5.txt to ok-file-1 through ok-file-5 (replace or rewrite).",
          "Delete a1.txt to a5.txt and list the workspace files again.",
          "Summarize what you did in three sentences.",
          "Read health_check.txt again and confirm the content has not changed.",
          "Finally output one line: stable-run-complete."
        ],
      },
    ],
  },
  // 默认首屏面板
  defaultPanel: "monitor",
  // 监控轮询间隔（毫秒）
  monitorPollIntervalMs: 3000,
  // 系统提示词自动刷新延迟（毫秒）
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

// 读取本地存储配置，合并到默认配置中
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

// 应用本地配置到 APP_CONFIG，用于初始化默认值
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

// 清空本地配置并恢复默认值
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

// 应用默认配置到界面输入框，避免在 HTML 中硬编码默认值
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
