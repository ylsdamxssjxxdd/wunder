import { APP_CONFIG, applyDefaultConfig } from "./app.config.js?v=20260215-01";

import {

  applyStoredConfig,

  readStoredConfig,

  updateDefaultConfig,

} from "./app.config.js?v=20260215-01";

import { elements } from "./modules/elements.js?v=20260215-01";

import { state } from "./modules/state.js";


import { appendLog } from "./modules/log.js?v=20260215-01";
import { loadI18nConfig } from "./modules/i18n-config.js";

import { initToolDetailModal } from "./modules/tool-detail.js?v=20260215-01";

import { initWorkspace, loadWorkspace, resetWorkspaceState } from "./modules/workspace.js?v=20260215-01";
import {

  applyPromptToolError,

  ensureToolSelectionLoaded,

  loadAvailableTools,

  resetToolSelection,

} from "./modules/tools.js?v=20260215-01";

import {
  ensurePromptTemplatesLoaded,
  initPromptPanel,
  loadSystemPrompt,
} from "./modules/prompt.js?v=20260215-01";

import { initDebugPanel, toggleDebugPolling } from "./modules/debug.js?v=20260215-01";
import { initMonitorPanel, loadMonitorData, toggleMonitorPolling } from "./modules/monitor.js?v=20260215-01";
import { initUserManagementPanel, loadUserStats } from "./modules/users.js?v=20260215-01";
import { initUserAccountsPanel, loadUserAccounts } from "./modules/user-accounts.js?v=20260215-01";
import { initExternalLinksPanel, loadExternalLinks } from "./modules/external-links.js?v=20260215-01";
import { initOrgUnitsPanel, loadOrgUnits } from "./modules/org-units.js?v=20260215-01";
import { initChannelsPanel, loadChannelAccounts } from "./modules/channels.js?v=20260215-01";
import {

  initMemoryPanel,

  loadMemoryStatus,

  loadMemoryUsers,

  toggleMemoryPolling,

} from "./modules/memory.js?v=20260215-01";

import { initMcpPanel, loadMcpServers } from "./modules/mcp.js?v=20260215-01";
import {
  initLspPanel,
  loadLspConfig,
  onLspPanelActivate,
  onLspPanelDeactivate,
} from "./modules/lsp.js?v=20260215-01";

import { initBuiltinPanel, loadBuiltinTools } from "./modules/builtin.js?v=20260215-01";

import { initSkillsPanel, loadSkills } from "./modules/skills.js?v=20260215-01";
import { initKnowledgePanel, loadKnowledgeConfig } from "./modules/knowledge.js?v=20260215-01";

import { initLlmPanel, loadLlmConfig } from "./modules/llm.js?v=20260215-01";
import { initUserTools, resetUserToolsState } from "./modules/user-tools.js?v=20260215-01";

import { initSettingsPanel, loadAdminDefaults } from "./modules/settings.js?v=20260215-01";

import { initA2aServicesPanel, loadA2aServices } from "./modules/a2a-services.js?v=20260215-01";
import { initApiDocsPanel } from "./modules/api-docs.js?v=20260215-01";
import { initPaperPanel } from "./modules/paper.js?v=20260215-01";
import { initThroughputPanel, toggleThroughputPolling } from "./modules/throughput.js?v=20260215-01";
import { initPerformancePanel } from "./modules/performance.js?v=20260215-01";
import { initSimLabPanel } from "./modules/sim-lab.js?v=20260215-01";
import { initEvaluationPanel } from "./modules/evaluation.js?v=20260215-01";
import { applyAuthHeaders, getAuthScope, initAdminAuth } from "./modules/admin-auth.js?v=20260215-01";

import { getCurrentLanguage, setLanguage, t } from "./modules/i18n.js?v=20260215-01";



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

    applyAuthHeaders(headers);

    nextInit.headers = headers;

    return originalFetch(input, nextInit);

  };

};



// Sidebar collapse handling; keep single-page layout stable.


const SIDEBAR_COLLAPSE_WIDTH = 1200;
let sidebarCollapsed = null;

const clearNavGroupHover = () => {
  document.querySelectorAll(".nav-group.is-hovered").forEach((group) => {
    group.classList.remove("is-hovered");
  });
};

const updateSidebarCollapse = () => {
  const width = window.innerWidth || document.documentElement.clientWidth;
  const shouldCollapse = Number.isFinite(width) && width > 0 && width <= SIDEBAR_COLLAPSE_WIDTH;
  if (shouldCollapse === sidebarCollapsed) {
    return;
  }
  sidebarCollapsed = shouldCollapse;
  document.body.classList.toggle("sidebar-collapsed", shouldCollapse);
  if (!shouldCollapse) {
    clearNavGroupHover();
  }
};

const bindSidebarCollapse = () => {
  updateSidebarCollapse();
  window.addEventListener("resize", updateSidebarCollapse);
};

const toggleSidebarCollapse = () => {
  sidebarCollapsed = !sidebarCollapsed;
  document.body.classList.toggle("sidebar-collapsed", sidebarCollapsed);
  if (!sidebarCollapsed) {
    clearNavGroupHover();
  }
};

const panelMap = {

  monitor: { panel: elements.monitorPanel, nav: elements.navMonitor },

  users: { panel: elements.usersPanel, nav: elements.navUsers },
  userAccounts: { panel: elements.userAccountsPanel, nav: elements.navUserAccounts },
  externalLinks: { panel: elements.externalLinksPanel, nav: elements.navExternalLinks },
  orgUnits: { panel: elements.orgUnitsPanel, nav: elements.navOrgUnits },

  memory: { panel: elements.memoryPanel, nav: elements.navMemory },
  channels: { panel: elements.channelsPanel, nav: elements.navChannels },

  llm: { panel: elements.llmPanel, nav: elements.navLlm },

  settings: { panel: elements.settingsPanel, nav: elements.navSettings },

  builtin: { panel: elements.builtinPanel, nav: elements.navBuiltin },

  mcp: { panel: elements.mcpPanel, nav: elements.navBuiltin },

  lsp: { panel: elements.lspPanel, nav: elements.navLsp },

  a2aServices: { panel: elements.a2aServicesPanel, nav: elements.navBuiltin },

  skills: { panel: elements.skillsPanel, nav: elements.navBuiltin },

  knowledge: { panel: elements.knowledgePanel, nav: elements.navBuiltin },

  throughput: { panel: elements.throughputPanel, nav: elements.navThroughput },

  performance: { panel: elements.performancePanel, nav: elements.navPerformance },

  simLab: { panel: elements.simLabPanel, nav: elements.navSimLab },

  prompt: { panel: elements.promptPanel, nav: elements.navPrompt },

  evaluation: { panel: elements.evaluationPanel, nav: elements.navEvaluation },

  debug: { panel: elements.debugPanel, nav: elements.navDebug },

  intro: { panel: elements.introPanel, nav: elements.navIntro },

  paper: { panel: elements.paperPanel, nav: elements.navPaper },

  apiDocs: { panel: elements.apiDocsPanel, nav: elements.navApiDocs },

};

const TOOL_MANAGER_PANELS = new Set(["builtin", "mcp", "knowledge", "a2aServices", "skills"]);

const syncToolManagerShortcutState = (activePanel) => {
  const currentPanel = TOOL_MANAGER_PANELS.has(activePanel) ? activePanel : "";
  const shortcutButtons = document.querySelectorAll(".tool-manager-shortcut[data-tool-panel]");
  shortcutButtons.forEach((button) => {
    const isActive = button.dataset.toolPanel === currentPanel;
    button.classList.toggle("is-active", isActive);
    button.setAttribute("aria-pressed", isActive ? "true" : "false");
    if (isActive) {
      button.setAttribute("aria-current", "true");
    } else {
      button.removeAttribute("aria-current");
    }
  });
};

const setNavGroupExpanded = (group, expanded) => {
  group.classList.toggle("is-collapsed", !expanded);
  const button = group.querySelector(".nav-group-btn");
  if (button) {
    button.setAttribute("aria-expanded", expanded ? "true" : "false");
  }
  if (!expanded) {
    group.classList.remove("is-hovered");
  }
};

const updateNavGroupState = () => {
  const groups = document.querySelectorAll(".nav-group");
  groups.forEach((group) => {
    const hasActive = group.querySelector(".nav-btn.active");
    group.classList.toggle("active", Boolean(hasActive));
    const expanded = !group.classList.contains("is-collapsed");
    const button = group.querySelector(".nav-group-btn");
    if (button) {
      button.setAttribute("aria-expanded", expanded ? "true" : "false");
    }
  });
};

const bindNavGroupToggles = () => {
  const groups = document.querySelectorAll(".nav-group");
  groups.forEach((group) => {
    const button = group.querySelector(".nav-group-btn");
    if (!button) {
      return;
    }
    setNavGroupExpanded(group, !group.classList.contains("is-collapsed"));
    button.addEventListener("click", () => {
      const shouldExpand = group.classList.contains("is-collapsed");
      setNavGroupExpanded(group, shouldExpand);
    });
  });
};

const bindNavGroupHover = () => {
  const groups = Array.from(document.querySelectorAll(".nav-group"));
  if (!groups.length) {
    return;
  }
  const activate = (group) => {
    if (!document.body.classList.contains("sidebar-collapsed")) {
      return;
    }
    if (group.classList.contains("is-collapsed")) {
      return;
    }
    groups.forEach((item) => {
      if (item !== group) {
        item.classList.remove("is-hovered");
      }
    });
    group.classList.add("is-hovered");
  };
  const deactivate = (group) => {
    group.classList.remove("is-hovered");
  };
  groups.forEach((group) => {
    group.addEventListener("mouseenter", () => activate(group));
    group.addEventListener("mouseleave", () => deactivate(group));
    group.addEventListener("focusin", () => activate(group));
    group.addEventListener("focusout", (event) => {
      if (!group.contains(event.relatedTarget)) {
        deactivate(group);
      }
    });
  });
  document.body.addEventListener("mouseleave", () => {
    if (document.body.classList.contains("sidebar-collapsed")) {
      clearNavGroupHover();
    }
  });
};

const expandActiveNavGroupOnly = () => {
  const groups = Array.from(document.querySelectorAll(".nav-group"));
  if (!groups.length) {
    return;
  }
  const activeGroup = groups.find((group) => group.querySelector(".nav-btn.active"));
  if (!activeGroup) {
    return;
  }
  groups.forEach((group) => {
    setNavGroupExpanded(group, group === activeGroup);
  });
};



const switchPanel = (panel) => {
  if (!panelMap[panel]) {
    return;
  }

  const previousPanel = state.runtime.activePanel;
  const navButtons = new Set();

  Object.values(panelMap).forEach((entry) => {
    if (entry.nav) {
      navButtons.add(entry.nav);
    }
  });

  Object.keys(panelMap).forEach((key) => {
    const entry = panelMap[key];
    const isActive = key === panel;
    if (entry.panel) {
      entry.panel.classList.toggle("active", isActive);
    }
  });

  navButtons.forEach((button) => button.classList.remove("active"));
  const activeNav = panelMap[panel].nav;
  if (activeNav) {
    activeNav.classList.add("active");
  }

  updateNavGroupState();

  state.runtime.activePanel = panel;
  syncToolManagerShortcutState(panel);

  if (previousPanel === "lsp" && panel !== "lsp") {
    onLspPanelDeactivate();
  } else if (panel === "lsp" && previousPanel !== "lsp") {
    onLspPanelActivate();
  }

  toggleMonitorPolling(panel === "monitor", { mode: "full" });

  toggleDebugPolling(panel === "debug");

  toggleMemoryPolling(panel === "memory");

  toggleThroughputPolling(panel === "throughput");

};



// 根据语言切换系统介绍 PPT 地址，同时附带版本号避免浏览器缓存旧页

const INTRO_PPT_VERSION = "20260110-09";
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

  elements.navUserAccounts.addEventListener("click", async () => {
    switchPanel("userAccounts");
    if (!state.panelLoaded.userAccounts) {
      try {
        await loadUserAccounts();
        state.panelLoaded.userAccounts = true;
      } catch (error) {
        appendLog(t("app.panelLoadFailed", { panel: t("panel.userAccounts"), message: error.message }));
      }
    }
  });

  elements.navExternalLinks.addEventListener("click", async () => {
    switchPanel("externalLinks");
    if (!state.panelLoaded.externalLinks) {
      try {
        await loadExternalLinks({ silent: true });
        state.panelLoaded.externalLinks = true;
      } catch (error) {
        appendLog(t("app.panelLoadFailed", { panel: t("panel.externalLinks"), message: error.message }));
      }
    }
  });

  elements.navOrgUnits.addEventListener("click", async () => {
    switchPanel("orgUnits");
    if (!state.panelLoaded.orgUnits) {
      try {
        await loadOrgUnits();
        state.panelLoaded.orgUnits = true;
      } catch (error) {
        appendLog(t("app.panelLoadFailed", { panel: t("panel.orgUnits"), message: error.message }));
      }
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

  elements.navChannels.addEventListener("click", async () => {
    switchPanel("channels");
    if (!state.panelLoaded.channels) {
      try {
        await loadChannelAccounts();
        state.panelLoaded.channels = true;
      } catch (error) {
        appendLog(t("app.panelLoadFailed", { panel: t("panel.channels"), message: error.message }));
      }
    }
  });

  // 点击侧边栏标题进入系统介绍面板

  if (elements.navThroughput) {
    elements.navThroughput.addEventListener("click", async () => {
      switchPanel("throughput");
      if (!state.panelLoaded.throughput) {
        try {
          await initThroughputPanel();
          state.panelLoaded.throughput = true;
        } catch (error) {
          appendLog(
            t("app.panelLoadFailed", { panel: t("panel.throughput"), message: error.message })
          );
        }
      }
    });
  }

  if (elements.navPerformance) {
    elements.navPerformance.addEventListener("click", () => {
      switchPanel("performance");
      try {
        initPerformancePanel();
        state.panelLoaded.performance = true;
      } catch (error) {
        appendLog(
          t("app.panelLoadFailed", { panel: t("panel.performance"), message: error.message })
        );
      }
    });
  }

  if (elements.navSimLab) {
    elements.navSimLab.addEventListener("click", async () => {
      switchPanel("simLab");
      try {
        await initSimLabPanel();
        state.panelLoaded.simLab = true;
      } catch (error) {
        appendLog(
          t("app.panelLoadFailed", { panel: t("panel.simLab"), message: error.message })
        );
      }
    });
  }

  elements.navDebug.addEventListener("click", () => switchPanel("debug"));

  elements.navDebug.addEventListener("click", () => {

    loadWorkspace();

  });

  const openA2aServicesPanel = async () => {

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

  };

  const openMcpPanel = async () => {

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

  };

  const openBuiltinPanel = async () => {

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

  };

  const openSkillsPanel = async () => {

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

  };

  const openKnowledgePanel = async () => {

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

  };

  const toolManagerOpeners = {
    builtin: openBuiltinPanel,
    mcp: openMcpPanel,
    knowledge: openKnowledgePanel,
    a2aServices: openA2aServicesPanel,
    skills: openSkillsPanel,
  };

  const toolShortcutIdMap = {
    toolManagerOpenBuiltin: "builtin",
    toolManagerOpenMcp: "mcp",
    toolManagerOpenKnowledge: "knowledge",
    toolManagerOpenA2aServices: "a2aServices",
    toolManagerOpenSkills: "skills",
  };

  const ensureToolManagerShortcutMirrors = () => {
    const sourcePanel = elements.builtinPanel;
    if (!sourcePanel) {
      return;
    }
    const sourceShortcuts = sourcePanel.querySelector(".tool-manager-shortcuts");
    if (!sourceShortcuts) {
      return;
    }
    sourceShortcuts.querySelectorAll(".tool-manager-shortcut").forEach((button) => {
      const panelKey = button.dataset.toolPanel || toolShortcutIdMap[button.id];
      if (panelKey) {
        button.dataset.toolPanel = panelKey;
      }
    });
    const sourceButtons = Array.from(
      sourceShortcuts.querySelectorAll(".tool-manager-shortcut[data-tool-panel]")
    );
    if (!sourceButtons.length) {
      return;
    }
    const sourceHint = sourcePanel.querySelector(".tool-manager-shortcuts-hint");
    const mirrorTargets = [
      elements.mcpPanel,
      elements.knowledgePanel,
      elements.a2aServicesPanel,
      elements.skillsPanel,
    ];
    mirrorTargets.forEach((panel) => {
      if (!panel || panel.querySelector('.tool-manager-shortcuts[data-tool-shortcuts="mirrored"]')) {
        return;
      }
      const mirroredShortcuts = document.createElement("div");
      mirroredShortcuts.className = "tool-manager-shortcuts tool-manager-shortcuts--embedded";
      mirroredShortcuts.dataset.toolShortcuts = "mirrored";
      sourceButtons.forEach((sourceButton) => {
        const clone = sourceButton.cloneNode(true);
        clone.removeAttribute("id");
        clone.removeAttribute("aria-current");
        clone.setAttribute("aria-pressed", "false");
        clone.dataset.toolPanel = sourceButton.dataset.toolPanel;
        mirroredShortcuts.appendChild(clone);
      });
      const anchor = panel.querySelector(".tips");
      if (anchor && anchor.parentNode) {
        anchor.parentNode.insertBefore(mirroredShortcuts, anchor.nextSibling);
      } else {
        panel.insertBefore(mirroredShortcuts, panel.firstChild);
      }
      if (sourceHint) {
        const hintClone = sourceHint.cloneNode(true);
        hintClone.dataset.toolShortcuts = "mirrored";
        mirroredShortcuts.insertAdjacentElement("afterend", hintClone);
      }
    });
  };

  const bindToolManagerShortcutButtons = () => {
    document.querySelectorAll(".tool-manager-shortcut[data-tool-panel]").forEach((button) => {
      if (button.dataset.bound === "1") {
        return;
      }
      const targetPanel = button.dataset.toolPanel;
      const opener = toolManagerOpeners[targetPanel];
      if (!opener) {
        return;
      }
      button.dataset.bound = "1";
      button.addEventListener("click", (event) => {
        event.preventDefault();
        opener();
      });
    });
    syncToolManagerShortcutState(state.runtime.activePanel);
  };

  ensureToolManagerShortcutMirrors();
  bindToolManagerShortcutButtons();

  if (elements.navA2aServices) {
    elements.navA2aServices.addEventListener("click", openA2aServicesPanel);
  }

  elements.navSettings.addEventListener("click", () => switchPanel("settings"));

  if (elements.navMcp) {
    elements.navMcp.addEventListener("click", openMcpPanel);
  }

  if (elements.navLsp) {
    elements.navLsp.addEventListener("click", async () => {

      switchPanel("lsp");

      if (!state.panelLoaded.lsp) {

        try {

          await loadLspConfig();

          state.panelLoaded.lsp = true;

        } catch (error) {

          elements.lspStatusList.textContent = t("common.loadFailedWithMessage", {

            message: error.message,

          });

        }

      }

    });
  }

  if (elements.navBuiltin) {
    elements.navBuiltin.addEventListener("click", openBuiltinPanel);
  }

  if (elements.navSkills) {
    elements.navSkills.addEventListener("click", openSkillsPanel);
  }

  if (elements.navKnowledge) {
    elements.navKnowledge.addEventListener("click", openKnowledgePanel);
  }

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

      await ensurePromptTemplatesLoaded();

    } catch (error) {

      elements.systemPrompt.textContent = t("common.loadFailedWithMessage", { message: error.message });

    }

    if (!elements.systemPrompt.textContent.trim() || state.runtime.promptNeedsRefresh) {

      loadSystemPrompt();

    }

  });

  if (elements.navEvaluation) {
    elements.navEvaluation.addEventListener("click", async () => {
      switchPanel("evaluation");
      if (!state.panelLoaded.evaluation) {
        try {
          await initEvaluationPanel();
          state.panelLoaded.evaluation = true;
        } catch (error) {
          appendLog(
            t("app.panelLoadFailed", { panel: t("panel.evaluation"), message: error.message })
          );
        }
      }
    });
  }

  if (elements.navIntro) {
    elements.navIntro.addEventListener("click", () => switchPanel("intro"));
  }

  if (elements.navPaper) {
    elements.navPaper.addEventListener("click", () => {
      switchPanel("paper");
      if (!state.panelLoaded.paper) {
        initPaperPanel()
          .then((loaded) => {
            state.panelLoaded.paper = loaded;
          })
          .catch(() => {});
      }
    });
  }

  if (elements.navApiDocs) {
    elements.navApiDocs.addEventListener("click", () => switchPanel("apiDocs"));
  }

};

const applyAuthScopeVisibility = () => {
  const scope = getAuthScope();
  if (scope !== "leader") {
    return;
  }
  document.body.classList.add("leader-mode");
  const allowedNavIds = new Set(["navUserAccounts", "navOrgUnits"]);
  document.querySelectorAll(".nav-btn").forEach((btn) => {
    if (!allowedNavIds.has(btn.id)) {
      btn.style.display = "none";
    }
  });
  document.querySelectorAll(".nav-group").forEach((group) => {
    const hasVisible = Array.from(group.querySelectorAll(".nav-btn")).some(
      (btn) => btn.style.display !== "none"
    );
    group.style.display = hasVisible ? "" : "none";
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

};



// 启动入口：初始化默认值、模块交互与首屏数据

const bootstrap = async () => {

  const stored = readStoredConfig();

  const i18nConfig = await loadI18nConfig({
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

  initThroughputPanel();

  initUserManagementPanel();
  initUserAccountsPanel();
  initExternalLinksPanel();
  initOrgUnitsPanel();
  initChannelsPanel();

  initMemoryPanel();

  initMcpPanel();

  initLspPanel();

  initBuiltinPanel();

  initSkillsPanel();

  initKnowledgePanel();

  initLlmPanel();

  initPromptPanel();

  initA2aServicesPanel();

  initUserTools();

  initSettingsPanel();
  initApiDocsPanel();
  bindNavigation();
  bindNavGroupToggles();
  bindNavGroupHover();

  bindIntroPanel();

  bindLanguageRefresh();

  bindGlobalInputs();
  bindSidebarCollapse();
  
  // 绑定侧边栏标题点击事件
  if (elements.sidebarTitle) {
    elements.sidebarTitle.addEventListener("click", toggleSidebarCollapse);
  }

  await initAdminAuth();
  applyAuthScopeVisibility();
  loadAdminDefaults({ silent: true }).catch(() => {});
  const authScope = getAuthScope();
  const leaderPanels = ["orgUnits", "userAccounts"];
  let initialPanel = panelMap[APP_CONFIG.defaultPanel] ? APP_CONFIG.defaultPanel : "monitor";
  if (authScope === "leader") {
    initialPanel = leaderPanels.includes(initialPanel) ? initialPanel : "orgUnits";
  }

  switchPanel(initialPanel);
  expandActiveNavGroupOnly();
  if (authScope === "leader") {
    if (initialPanel === "orgUnits") {
      loadOrgUnits()
        .then(() => {
          state.panelLoaded.orgUnits = true;
        })
        .catch(() => {});
    } else if (initialPanel === "userAccounts") {
      loadUserAccounts()
        .then(() => {
          state.panelLoaded.userAccounts = true;
        })
        .catch(() => {});
    }
  }

  if (initialPanel === "paper" && !state.panelLoaded.paper) {
    initPaperPanel()
      .then((loaded) => {
        state.panelLoaded.paper = loaded;
      })
      .catch(() => {});
  }

  if (initialPanel === "performance" && !state.panelLoaded.performance) {
    try {
      initPerformancePanel();
      state.panelLoaded.performance = true;
    } catch (error) {
      appendLog(
        t("app.panelLoadFailed", { panel: t("panel.performance"), message: error.message })
      );
    }
  }

  if (initialPanel === "simLab" && !state.panelLoaded.simLab) {
    initSimLabPanel()
      .then(() => {
        state.panelLoaded.simLab = true;
      })
      .catch((error) => {
        appendLog(
          t("app.panelLoadFailed", { panel: t("panel.simLab"), message: error.message })
        );
      });
  }

  if (initialPanel === "evaluation" && !state.panelLoaded.evaluation) {
    initEvaluationPanel()
      .then(() => {
        state.panelLoaded.evaluation = true;
      })
      .catch((error) => {
        appendLog(
          t("app.panelLoadFailed", { panel: t("panel.evaluation"), message: error.message })
        );
      });
  }

  if (initialPanel === "users") {

    loadUserStats().catch((error) => {

      appendLog(t("app.panelLoadFailed", { panel: t("panel.users"), message: error.message }));

    });

  }

  if (initialPanel === "userAccounts") {

    loadUserAccounts().catch((error) => {

      appendLog(t("app.panelLoadFailed", { panel: t("panel.userAccounts"), message: error.message }));

    });

  }

  if (initialPanel === "externalLinks") {

    loadExternalLinks({ silent: true }).catch((error) => {

      appendLog(t("app.panelLoadFailed", { panel: t("panel.externalLinks"), message: error.message }));

    });

  }

  loadWorkspace();

  loadAvailableTools().catch((error) => {

    applyPromptToolError(error.message);

  });

};



bootstrap();











