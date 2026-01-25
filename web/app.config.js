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
        name: "长对话稳定性（自动化图表链路）",
        stream: true,
        toolNames: [
          "最终回复",
          "执行命令",
          "ptc",
          "列出文件",
          "读取文件",
          "写入文件",
          "搜索内容",
          "替换文本",
          "编辑文件",
        ],
        steps: [
          "你现在是稳定性测试助手，请简短回答 'ready'，并声明将持续执行任务；不要使用问询面板或 a2ui。",
          "列出当前工作区根目录文件，说明是否为空。",
          "创建 stability_run_{{timestamp}} 目录，并在其中创建 work、deliverables/images、deliverables/charts、deliverables/docs 子目录。",
          "用 ptc 绘制雷达图 PNG：指标为 速度/稳定性/工具编排/可观测性/可扩展性/安全性，分值为 [80,72,88,70,76,85]，标题为 `Wunder Stability Radar v1`，保存为 stability_run_{{timestamp}}/work/radar_v1.png，并汇报文件大小（字节）。",
          "调整雷达图内容：新增指标 成本控制=65，其他指标改为 [78,74,90,72,79,88]，标题改为 `Wunder Stability Radar v2`，保存为 stability_run_{{timestamp}}/work/radar_v2.png。",
          "将 radar_v2.png 移动到 stability_run_{{timestamp}}/deliverables/images/radar.png（覆盖同名文件），并确认 work 目录仅保留 radar_v1.png。",
          "用 ptc 绘制组织架构图 SVG，结构为 CEO -> (CTO, COO, CPO)，CTO -> (Platform Lead, Infra Lead)，COO -> (Ops Lead, QA Lead)，CPO -> (UX Lead)，保存为 stability_run_{{timestamp}}/work/org_v1.svg。",
          "调整组织架构图：将 COO 改为 Chief Ops，新增 Data Lead 归属 CTO，下方，同时标题改为 `Wunder Org v2`，保存为 stability_run_{{timestamp}}/work/org_v2.svg。",
          "将 org_v2.svg 移动到 stability_run_{{timestamp}}/deliverables/charts/org.svg，并确认 work 目录仅保留 org_v1.svg。",
          "在 stability_run_{{timestamp}}/deliverables/docs 创建 manifest.json，包含 run_id/user_id/session_id 与 items 数组（name/type/path/size_bytes/sha256/notes），为 radar.png 和 org.svg 计算 size 与 sha256 并写入。",
          "创建 stability_run_{{timestamp}}/deliverables/docs/README.md，说明生成步骤，并用表格列出文件名、路径、大小、sha256 前 8 位。",
          "创建 stability_run_{{timestamp}}/deliverables/docs/notes.txt，写 5 行：雷达图指标与数值变化、组织架构调整点、两张图的最终路径、manifest.json 的用途、压缩包名称。",
          "将 stability_run_{{timestamp}}/deliverables 打包为 stability_run_{{timestamp}}/stability_bundle_{{timestamp}}.zip。",
          "列出 stability_run_{{timestamp}} 根目录与 deliverables 目录内容，确认 zip 与产物存在。",
          "最后用两句话总结你完成的自动化操作，并输出一句话：stable-run-complete。"
        ],
      },
    ],
    "en-US": [
      {
        id: "long-dialogue-file",
        name: "Long Conversation Stability (automation charts)",
        stream: true,
        toolNames: [
          "最终回复",
          "执行命令",
          "ptc",
          "列出文件",
          "读取文件",
          "写入文件",
          "搜索内容",
          "替换文本",
          "编辑文件",
        ],
        steps: [
          "You are a stability test agent. Reply with 'ready' and confirm you will keep executing tasks; do not use question panel or a2ui.",
          "List files in the workspace root and say whether it is empty.",
          "Create a directory stability_run_{{timestamp}} with subfolders work, deliverables/images, deliverables/charts, and deliverables/docs.",
          "Use ptc to render a radar chart PNG with metrics Speed/Stability/Tooling/Observability/Scalability/Security and values [80,72,88,70,76,85], title `Wunder Stability Radar v1`, save to stability_run_{{timestamp}}/work/radar_v1.png, then report its size in bytes.",
          "Update the radar chart: add Cost Control=65, set values to [78,74,90,72,79,88] for the original six metrics, change title to `Wunder Stability Radar v2`, save to stability_run_{{timestamp}}/work/radar_v2.png.",
          "Move radar_v2.png to stability_run_{{timestamp}}/deliverables/images/radar.png (overwrite if exists), and confirm work only keeps radar_v1.png.",
          "Use ptc to create an org chart SVG with structure CEO -> (CTO, COO, CPO), CTO -> (Platform Lead, Infra Lead), COO -> (Ops Lead, QA Lead), CPO -> (UX Lead), save to stability_run_{{timestamp}}/work/org_v1.svg.",
          "Adjust the org chart: rename COO to Chief Ops, add Data Lead under CTO, change title to `Wunder Org v2`, save to stability_run_{{timestamp}}/work/org_v2.svg.",
          "Move org_v2.svg to stability_run_{{timestamp}}/deliverables/charts/org.svg, and confirm work only keeps org_v1.svg.",
          "Create stability_run_{{timestamp}}/deliverables/docs/manifest.json with run_id/user_id/session_id and items array (name/type/path/size_bytes/sha256/notes) for radar.png and org.svg, computing size and sha256.",
          "Create stability_run_{{timestamp}}/deliverables/docs/README.md with a short step summary and a table listing filename, path, size, and sha256 first 8 chars.",
          "Create stability_run_{{timestamp}}/deliverables/docs/notes.txt with 5 lines covering radar metric changes, org chart changes, final asset paths, manifest purpose, and the zip name.",
          "Zip stability_run_{{timestamp}}/deliverables into stability_run_{{timestamp}}/stability_bundle_{{timestamp}}.zip.",
          "List stability_run_{{timestamp}} root and deliverables contents to confirm the zip and outputs exist.",
          "Finally summarize the automation you performed in two sentences and output one line: stable-run-complete."
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
