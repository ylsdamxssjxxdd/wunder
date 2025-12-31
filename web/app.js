import { APP_CONFIG, applyDefaultConfig } from "./app.config.js";
import { elements } from "./modules/elements.js?v=20251231-03";
import { state } from "./modules/state.js";
import { normalizeApiBase } from "./modules/utils.js";
import { appendLog } from "./modules/log.js?v=20251231-01";
import { initToolDetailModal } from "./modules/tool-detail.js";
import { initWorkspace, loadWorkspace, resetWorkspaceState } from "./modules/workspace.js?v=20251231-01";
import {
  applyPromptToolError,
  ensureToolSelectionLoaded,
  loadAvailableTools,
  resetToolSelection,
} from "./modules/tools.js?v=20251231-01";
import { initPromptPanel, loadSystemPrompt } from "./modules/prompt.js?v=20251231-01";
import { initDebugPanel, toggleDebugPolling } from "./modules/debug.js?v=20251231-01";
import { initMonitorPanel, loadMonitorData, toggleMonitorPolling } from "./modules/monitor.js?v=20251231-01";
import { initUserManagementPanel, loadUserStats } from "./modules/users.js?v=20251231-01";
import {
  initMemoryPanel,
  loadMemoryStatus,
  loadMemoryUsers,
  toggleMemoryPolling,
} from "./modules/memory.js?v=20251231-05";
import { initMcpPanel, loadMcpServers } from "./modules/mcp.js";
import { initBuiltinPanel, loadBuiltinTools } from "./modules/builtin.js";
import { initSkillsPanel, loadSkills } from "./modules/skills.js?v=20251231-01";
import { initKnowledgePanel, loadKnowledgeConfig } from "./modules/knowledge.js?v=20251231-01";
import { initLlmPanel, loadLlmConfig } from "./modules/llm.js?v=20251231-01";
import { initUserTools, resetUserToolsState } from "./modules/user-tools.js?v=20251231-01";

const patchApiFetch = () => {
  // 统一为前端请求补齐 API Key，避免每处调用手动添加。
  const originalFetch = window.fetch.bind(window);
  window.fetch = (input, init = {}) => {
    const apiKey = String(elements.apiKey?.value || "").trim();
    if (!apiKey) {
      return originalFetch(input, init);
    }
    const nextInit = { ...init };
    const headers = new Headers(init.headers || (input instanceof Request ? input.headers : undefined));
    if (!headers.has("X-API-Key") && !headers.has("Authorization")) {
      headers.set("X-API-Key", apiKey);
    }
    nextInit.headers = headers;
    return originalFetch(input, nextInit);
  };
};

// 切换侧边栏面板，保持单页无整体滚动
const panelMap = {
  monitor: { panel: elements.monitorPanel, nav: elements.navMonitor },
  users: { panel: elements.usersPanel, nav: elements.navUsers },
  memory: { panel: elements.memoryPanel, nav: elements.navMemory },
  llm: { panel: elements.llmPanel, nav: elements.navLlm },
  builtin: { panel: elements.builtinPanel, nav: elements.navBuiltin },
  mcp: { panel: elements.mcpPanel, nav: elements.navMcp },
  skills: { panel: elements.skillsPanel, nav: elements.navSkills },
  knowledge: { panel: elements.knowledgePanel, nav: elements.navKnowledge },
  prompt: { panel: elements.promptPanel, nav: elements.navPrompt },
  debug: { panel: elements.debugPanel, nav: elements.navDebug },
};

const switchPanel = (panel) => {
  Object.keys(panelMap).forEach((key) => {
    const entry = panelMap[key];
    const isActive = key === panel;
    entry.panel.classList.toggle("active", isActive);
    entry.nav.classList.toggle("active", isActive);
  });
  state.runtime.activePanel = panel;
  toggleMonitorPolling(panel === "monitor", { mode: "full" });
  toggleDebugPolling(panel === "debug");
  toggleMemoryPolling(panel === "memory");
};

// 绑定导航事件与跨页面交互
const bindNavigation = () => {
  elements.navMonitor.addEventListener("click", async () => {
    switchPanel("monitor");
    if (!state.panelLoaded.monitor) {
      try {
        await loadMonitorData();
        state.panelLoaded.monitor = true;
      } catch (error) {
        appendLog(`监控加载失败：${error.message}`);
      }
    }
  });
  elements.navUsers.addEventListener("click", async () => {
    switchPanel("users");
    let ready = state.panelLoaded.users;
    if (!state.panelLoaded.users) {
      try {
        await loadUserStats();
        state.panelLoaded.users = true;
        ready = true;
      } catch (error) {
        appendLog(`用户统计加载失败：${error.message}`);
      }
    }
    if (ready) {
      toggleMonitorPolling(true, { mode: "sessions" });
    }
  });
  elements.navMemory.addEventListener("click", async () => {
    switchPanel("memory");
    if (!state.panelLoaded.memory) {
      try {
        await loadMemoryUsers();
        await loadMemoryStatus();
        state.panelLoaded.memory = true;
      } catch (error) {
        appendLog(`记忆体加载失败：${error.message}`);
      }
    }
  });
  elements.navDebug.addEventListener("click", () => switchPanel("debug"));
  elements.navDebug.addEventListener("click", () => {
    loadWorkspace();
  });
  elements.navMcp.addEventListener("click", async () => {
    switchPanel("mcp");
    if (!state.panelLoaded.mcp) {
      try {
        await loadMcpServers();
        state.panelLoaded.mcp = true;
      } catch (error) {
        elements.mcpServerList.textContent = `加载失败：${error.message}`;
      }
    }
  });
  elements.navBuiltin.addEventListener("click", async () => {
    switchPanel("builtin");
    if (!state.panelLoaded.builtin) {
      try {
        await loadBuiltinTools();
        state.panelLoaded.builtin = true;
      } catch (error) {
        elements.builtinToolsList.textContent = `加载失败：${error.message}`;
      }
    }
  });
  elements.navSkills.addEventListener("click", async () => {
    switchPanel("skills");
    if (!state.panelLoaded.skills) {
      try {
        await loadSkills();
        state.panelLoaded.skills = true;
      } catch (error) {
        elements.skillsList.textContent = `加载失败：${error.message}`;
      }
    }
  });
  elements.navKnowledge.addEventListener("click", async () => {
    switchPanel("knowledge");
    if (!state.panelLoaded.knowledge) {
      try {
        await loadKnowledgeConfig();
        state.panelLoaded.knowledge = true;
      } catch (error) {
        elements.knowledgeBaseList.textContent = `加载失败：${error.message}`;
      }
    }
  });
  elements.navLlm.addEventListener("click", async () => {
    switchPanel("llm");
    if (!state.panelLoaded.llm) {
      try {
        await loadLlmConfig();
        state.panelLoaded.llm = true;
      } catch (error) {
        appendLog(`模型配置加载失败：${error.message}`);
      }
    }
  });
  elements.navPrompt.addEventListener("click", async () => {
    switchPanel("prompt");
    try {
      await ensureToolSelectionLoaded();
    } catch (error) {
      applyPromptToolError(error.message);
    }
    if (!elements.systemPrompt.textContent.trim() || state.runtime.promptNeedsRefresh) {
      loadSystemPrompt();
    }
  });
};

// 绑定基础输入与全局行为
const bindGlobalInputs = () => {
  // API Key 显示/隐藏切换，便于确认输入是否正确。
  if (elements.apiKeyToggle && elements.apiKey) {
    const syncApiKeyToggle = (hidden) => {
      const icon = elements.apiKeyToggle.querySelector("i");
      if (icon) {
        icon.classList.toggle("fa-eye", hidden);
        icon.classList.toggle("fa-eye-slash", !hidden);
      }
      const label = hidden ? "显示 API Key" : "隐藏 API Key";
      elements.apiKeyToggle.setAttribute("aria-label", label);
      elements.apiKeyToggle.title = label;
    };
    const initialHidden = elements.apiKey.type !== "text";
    syncApiKeyToggle(initialHidden);
    elements.apiKeyToggle.addEventListener("click", () => {
      const hidden = elements.apiKey.type !== "text";
      elements.apiKey.type = hidden ? "text" : "password";
      syncApiKeyToggle(!hidden);
    });
  }

  const applyUserIdChange = (rawValue) => {
    const nextValue = String(rawValue || "").trim();
    if (elements.userId && elements.userId.value !== nextValue) {
      elements.userId.value = nextValue;
    }
    if (elements.promptUserId && elements.promptUserId.value !== nextValue) {
      elements.promptUserId.value = nextValue;
    }
    resetWorkspaceState();
    loadWorkspace({ refreshTree: true });
    resetToolSelection();
    resetUserToolsState();
    loadAvailableTools().catch((error) => {
      applyPromptToolError(error.message);
    });
  };

  elements.userId.addEventListener("change", (event) => {
    applyUserIdChange(event.target.value);
  });
  if (elements.promptUserId) {
    elements.promptUserId.addEventListener("change", (event) => {
      applyUserIdChange(event.target.value);
    });
    if (elements.userId && elements.userId.value) {
      elements.promptUserId.value = elements.userId.value;
    }
  }
  elements.apiBase.addEventListener("change", () => {
    const normalized = normalizeApiBase(elements.apiBase.value);
    if (normalized) {
      elements.apiBase.value = normalized;
    }
    loadWorkspace();
    resetToolSelection();
    resetUserToolsState();
    loadAvailableTools().catch((error) => {
      applyPromptToolError(error.message);
    });
  });
};

// 启动入口：初始化默认值、模块交互与首屏数据
const bootstrap = () => {
  applyDefaultConfig(elements);
  patchApiFetch();
  initToolDetailModal();
  initWorkspace();
  initDebugPanel();
  initMonitorPanel();
  initUserManagementPanel();
  initMemoryPanel();
  initMcpPanel();
  initBuiltinPanel();
  initSkillsPanel();
  initKnowledgePanel();
  initLlmPanel();
  initPromptPanel();
  initUserTools();
  bindNavigation();
  bindGlobalInputs();
  switchPanel(APP_CONFIG.defaultPanel);
  if (APP_CONFIG.defaultPanel === "users") {
    loadUserStats().catch((error) => {
      appendLog(`用户统计加载失败：${error.message}`);
    });
  }
  loadWorkspace();
  loadAvailableTools().catch((error) => {
    applyPromptToolError(error.message);
  });
};

bootstrap();




