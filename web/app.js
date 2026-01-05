import { APP_CONFIG, applyDefaultConfig } from "./app.config.js";
import {
  applyStoredConfig,
  readStoredConfig,
  updateDefaultConfig,
} from "./app.config.js";
import { elements } from "./modules/elements.js?v=20260105-02";
import { state } from "./modules/state.js";
import { normalizeApiBase } from "./modules/utils.js";
import { appendLog } from "./modules/log.js?v=20251231-01";
import { loadI18nConfig } from "./modules/i18n-config.js";
import { initToolDetailModal } from "./modules/tool-detail.js";
import { initWorkspace, loadWorkspace, resetWorkspaceState } from "./modules/workspace.js?v=20260101-02";
import {
  applyPromptToolError,
  ensureToolSelectionLoaded,
  loadAvailableTools,
  resetToolSelection,
} from "./modules/tools.js?v=20251231-01";
import { initPromptPanel, loadSystemPrompt } from "./modules/prompt.js?v=20251231-01";
import { initDebugPanel, toggleDebugPolling } from "./modules/debug.js?v=20260101-02";
import { initMonitorPanel, loadMonitorData, toggleMonitorPolling } from "./modules/monitor.js?v=20260105-03";
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
import { initSettingsPanel } from "./modules/settings.js?v=20260101-01";
import { initA2aServicesPanel, loadA2aServices } from "./modules/a2a-services.js?v=20260105-01";
import { getCurrentLanguage, setLanguage, t } from "./modules/i18n.js?v=20260105-01";

const patchApiFetch = () => {
  // 统一为前端请求补齐 API Key，避免每处调用手动添加。
  const originalFetch = window.fetch.bind(window);
  window.fetch = (input, init = {}) => {
    const nextInit = { ...init };
    const headers = new Headers(init.headers || (input instanceof Request ? input.headers : undefined));
    const language = getCurrentLanguage();
    if (language && !headers.has("X-Wunder-Language")) {
      headers.set("X-Wunder-Language", language);
    }
    const apiKey = String(elements.apiKey?.value || "").trim();
    if (!headers.has("X-API-Key") && !headers.has("Authorization")) {
      if (apiKey) {
        headers.set("X-API-Key", apiKey);
      }
    }
    nextInit.headers = headers;
    return originalFetch(input, nextInit);
  };
};

// 切换侧边栏面板，保持单页无整体滚动
const panelMap = {
  monitor: { panel: elements.monitorPanel, nav: elements.navMonitor },
  intro: { panel: elements.introPanel, nav: elements.sidebarTitle },
  users: { panel: elements.usersPanel, nav: elements.navUsers },
  memory: { panel: elements.memoryPanel, nav: elements.navMemory },
  llm: { panel: elements.llmPanel, nav: elements.navLlm },
  builtin: { panel: elements.builtinPanel, nav: elements.navBuiltin },
  mcp: { panel: elements.mcpPanel, nav: elements.navMcp },
  a2aServices: { panel: elements.a2aServicesPanel, nav: elements.navA2aServices },
  skills: { panel: elements.skillsPanel, nav: elements.navSkills },
  knowledge: { panel: elements.knowledgePanel, nav: elements.navKnowledge },
  prompt: { panel: elements.promptPanel, nav: elements.navPrompt },
  debug: { panel: elements.debugPanel, nav: elements.navDebug },
  settings: { panel: elements.settingsPanel, nav: elements.navSettings },
};

const switchPanel = (panel) => {
  Object.keys(panelMap).forEach((key) => {
    const entry = panelMap[key];
    const isActive = key === panel;
    entry.panel.classList.toggle("active", isActive);
    if (entry.nav) {
      entry.nav.classList.toggle("active", isActive);
    }
  });
  state.runtime.activePanel = panel;
  toggleMonitorPolling(panel === "monitor", { mode: "full" });
  toggleDebugPolling(panel === "debug");
  toggleMemoryPolling(panel === "memory");
};

// 根据语言切换系统介绍 PPT 地址，同时附带版本号避免浏览器缓存旧页
const INTRO_PPT_VERSION = "20260105-11";
const appendIntroVersion = (src) => `${src}?v=${INTRO_PPT_VERSION}`;
const resolveIntroSrc = (language) => {
  const normalized = String(language || "").toLowerCase();
  if (normalized.startsWith("en")) {
    return appendIntroVersion("/wunder/ppt-en/index.html");
  }
  return appendIntroVersion("/wunder/ppt/index.html");
};

const syncIntroFrameLanguage = (language) => {
  if (!elements.introFrame) {
    return;
  }
  const nextSrc = resolveIntroSrc(language);
  if (elements.introFrame.getAttribute("src") !== nextSrc) {
    elements.introFrame.setAttribute("src", nextSrc);
  }
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
        appendLog(t("app.panelLoadFailed", { panel: t("panel.monitor"), message: error.message }));
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
        appendLog(t("app.panelLoadFailed", { panel: t("panel.users"), message: error.message }));
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
        appendLog(t("app.panelLoadFailed", { panel: t("panel.memory"), message: error.message }));
      }
    }
  });
  // 点击侧边栏标题进入系统介绍面板
  if (elements.sidebarTitle) {
    elements.sidebarTitle.addEventListener("click", () => {
      switchPanel("intro");
    });
  }
  elements.navDebug.addEventListener("click", () => switchPanel("debug"));
  elements.navDebug.addEventListener("click", () => {
    loadWorkspace();
  });
  elements.navA2aServices.addEventListener("click", async () => {
    switchPanel("a2aServices");
    if (!state.panelLoaded.a2aServices) {
      try {
        await loadA2aServices();
        state.panelLoaded.a2aServices = true;
      } catch (error) {
        elements.a2aServiceList.textContent = t("common.loadFailedWithMessage", {
          message: error.message,
        });
      }
    }
  });
  elements.navSettings.addEventListener("click", () => switchPanel("settings"));
  elements.navMcp.addEventListener("click", async () => {
    switchPanel("mcp");
    if (!state.panelLoaded.mcp) {
      try {
        await loadMcpServers();
        state.panelLoaded.mcp = true;
      } catch (error) {
        elements.mcpServerList.textContent = t("common.loadFailedWithMessage", {
          message: error.message,
        });
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
        elements.builtinToolsList.textContent = t("common.loadFailedWithMessage", {
          message: error.message,
        });
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
        elements.skillsList.textContent = t("common.loadFailedWithMessage", {
          message: error.message,
        });
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
        elements.knowledgeBaseList.textContent = t("common.loadFailedWithMessage", {
          message: error.message,
        });
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
        appendLog(t("app.panelLoadFailed", { panel: t("panel.llm"), message: error.message }));
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

// 系统介绍面板：全屏按钮与展示容器绑定
const bindIntroPanel = () => {
  if (!elements.introFullscreenBtn || !elements.introFrameWrap) {
    return;
  }
  elements.introFullscreenBtn.addEventListener("click", () => {
    const target = elements.introFrameWrap;
    if (document.fullscreenElement) {
      if (document.fullscreenElement === target && document.exitFullscreen) {
        document.exitFullscreen();
        return;
      }
    }
    if (target.requestFullscreen) {
      target.requestFullscreen().catch(() => {});
    }
  });
  syncIntroFrameLanguage(getCurrentLanguage());
  window.addEventListener("wunder:language-changed", (event) => {
    syncIntroFrameLanguage(event.detail?.language);
  });
};

const bindLanguageRefresh = () => {
  window.addEventListener("wunder:language-changed", () => {
    state.runtime.promptNeedsRefresh = true;
    loadAvailableTools()
      .then(() => {
        if (state.runtime.activePanel === "prompt") {
          loadSystemPrompt();
        }
      })
      .catch((error) => {
        applyPromptToolError(error.message);
      });

    if (state.panelLoaded.builtin) {
      loadBuiltinTools().catch((error) => {
        elements.builtinToolsList.textContent = t("common.loadFailedWithMessage", {
          message: error.message,
        });
      });
    }

    if (state.panelLoaded.monitor || state.panelLoaded.users) {
      const mode = state.runtime.activePanel === "users" ? "sessions" : "full";
      loadMonitorData({ mode }).catch((error) => {
        appendLog(t("monitor.refreshFailed", { message: error.message }));
      });
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
      const label = hidden ? t("ui.apiKey.show") : t("ui.apiKey.hide");
      elements.apiKeyToggle.setAttribute("aria-label", label);
      elements.apiKeyToggle.title = label;
    };
    const initialHidden = elements.apiKey.type !== "text";
    syncApiKeyToggle(initialHidden);
    window.addEventListener("wunder:language-changed", () => {
      const hidden = elements.apiKey.type !== "text";
      syncApiKeyToggle(hidden);
    });
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
const bootstrap = async () => {
  const stored = readStoredConfig();
  const i18nConfig = await loadI18nConfig({
    apiBase: stored.defaultApiBase || APP_CONFIG.defaultApiBase,
    apiKey: stored.defaultApiKey || APP_CONFIG.defaultApiKey,
    language: stored.language || APP_CONFIG.language,
  });
  if (i18nConfig?.default_language) {
    updateDefaultConfig({ language: i18nConfig.default_language });
  }
  applyStoredConfig();
  setLanguage(APP_CONFIG.language, { force: true });
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
  initA2aServicesPanel();
  initUserTools();
  initSettingsPanel();
  bindNavigation();
  bindIntroPanel();
  bindLanguageRefresh();
  bindGlobalInputs();
  const initialPanel = panelMap[APP_CONFIG.defaultPanel] ? APP_CONFIG.defaultPanel : "monitor";
  switchPanel(initialPanel);
  if (initialPanel === "users") {
    loadUserStats().catch((error) => {
      appendLog(t("app.panelLoadFailed", { panel: t("panel.users"), message: error.message }));
    });
  }
  loadWorkspace();
  loadAvailableTools().catch((error) => {
    applyPromptToolError(error.message);
  });
};

bootstrap();




