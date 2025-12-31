// 前端统一配置：集中管理默认值与页面行为参数
export const APP_CONFIG = {
  // 默认 API 地址：用于初始化调试面板输入框
  defaultApiBase: "http://127.0.0.1:8000/wunder",
  // 默认 API Key：为空表示由用户自行输入
  defaultApiKey: "",
  // 默认用户 ID：用于初始化调试面板输入框
  defaultUserId: "demo_user",
  // 调试面板问题预设：右键问题区域快速填充
  debugQuestionPresets: [
    "你好，介绍一下 wunder 的核心能力。",
    "请列出当前可用工具，并说明用途。",
    "用python绘制一个爱心保存到本地png",
    "广州今天的天气如何？",
  ],
  // 默认首屏面板
  defaultPanel: "monitor",
  // 监控轮询间隔（毫秒）
  monitorPollIntervalMs: 3000,
  // 系统提示词自动刷新延迟（毫秒）
  promptReloadDelayMs: 300,
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
