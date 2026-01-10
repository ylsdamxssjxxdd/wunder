import { APP_CONFIG } from "../app.config.js?v=20260110-04";
import { elements } from "./elements.js?v=20260110-06";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { t } from "./i18n.js?v=20260110-07";

// 工具勾选状态使用本地缓存，按 user_id 隔离
const TOOL_SELECTION_STORAGE_PREFIX = "wunder_tool_selection:";
const A2UI_TOOL_NAME = "a2ui";
const FINAL_TOOL_NAMES = new Set(["最终回复", "final_response"]);
const DEFAULT_UNSELECTED_TOOLS = new Set([A2UI_TOOL_NAME]);

// 兼容系统提示词/调试面板两处 user_id 输入
const getToolSelectionUserId = () =>
  String(elements.userId?.value || elements.promptUserId?.value || "").trim();

const getToolSelectionStorageKey = (userId) =>
  `${TOOL_SELECTION_STORAGE_PREFIX}${userId || "anonymous"}`;

// 读取缓存，兼容旧格式（仅保存 selected 数组）
const loadCachedSelection = (userId) => {
  if (!userId) {
    return null;
  }
  try {
    const raw = localStorage.getItem(getToolSelectionStorageKey(userId));
    if (!raw) {
      return null;
    }
    const parsed = JSON.parse(raw);
    if (Array.isArray(parsed)) {
      const list = parsed.map((name) => String(name));
      return { selected: new Set(list), known: new Set(list) };
    }
    if (!parsed || typeof parsed !== "object") {
      return null;
    }
    const selectedList = Array.isArray(parsed.selected)
      ? parsed.selected.map((name) => String(name))
      : [];
    const knownList = Array.isArray(parsed.known)
      ? parsed.known.map((name) => String(name))
      : selectedList;
    return {
      selected: new Set(selectedList),
      known: new Set(knownList),
    };
  } catch (error) {
    return null;
  }
};

// 保存勾选状态与已知工具列表，避免刷新后重置选择
const persistToolSelection = () => {
  const userId = getToolSelectionUserId();
  if (!userId) {
    return;
  }
  const known = [
    ...state.toolSelection.builtin,
    ...state.toolSelection.mcp,
    ...state.toolSelection.a2a,
    ...state.toolSelection.skills,
    ...state.toolSelection.knowledge,
    ...state.toolSelection.userTools,
    ...state.toolSelection.sharedTools,
  ].map((item) => item.name);
  try {
    localStorage.setItem(
      getToolSelectionStorageKey(userId),
      JSON.stringify({
        selected: Array.from(state.toolSelection.selected),
        known,
      })
    );
  } catch (error) {
    // 忽略本地存储不可用的情况
  }
};

const ensureToolSelectionState = () => {
  if (!state.toolSelection || typeof state.toolSelection !== "object") {
    state.toolSelection = {
      builtin: [],
      mcp: [],
      a2a: [],
      skills: [],
      knowledge: [],
      userTools: [],
      sharedTools: [],
      selected: new Set(),
      loaded: false,
    };
    return;
  }
  if (!Array.isArray(state.toolSelection.builtin)) {
    state.toolSelection.builtin = [];
  }
  if (!Array.isArray(state.toolSelection.mcp)) {
    state.toolSelection.mcp = [];
  }
  if (!Array.isArray(state.toolSelection.a2a)) {
    state.toolSelection.a2a = [];
  }
  if (!Array.isArray(state.toolSelection.skills)) {
    state.toolSelection.skills = [];
  }
  if (!Array.isArray(state.toolSelection.knowledge)) {
    state.toolSelection.knowledge = [];
  }
  if (!Array.isArray(state.toolSelection.userTools)) {
    state.toolSelection.userTools = [];
  }
  if (!Array.isArray(state.toolSelection.sharedTools)) {
    state.toolSelection.sharedTools = [];
  }
  if (!(state.toolSelection.selected instanceof Set)) {
    const raw = Array.isArray(state.toolSelection.selected)
      ? state.toolSelection.selected
      : [];
    state.toolSelection.selected = new Set(raw);
  }
  if (typeof state.toolSelection.loaded !== "boolean") {
    state.toolSelection.loaded = false;
  }
};

export const ensureUserToolsState = () => {
  if (!state.userTools || typeof state.userTools !== "object") {
    state.userTools = {
      extraPrompt: "",
      modal: { activeTab: "mcp" },
      mcp: {
        servers: [],
        toolsByIndex: [],
        selectedIndex: -1,
        saveVersion: 0,
        loaded: false,
      },
      skills: {
        skills: [],
        enabled: [],
        shared: [],
        detailVersion: 0,
        loaded: false,
      },
      knowledge: {
        bases: [],
        selectedIndex: -1,
        files: [],
        activeFile: "",
        fileContent: "",
        loaded: false,
      },
    };
    return;
  }
  if (typeof state.userTools.extraPrompt !== "string") {
    state.userTools.extraPrompt = "";
  }
  if (!state.userTools.modal || typeof state.userTools.modal !== "object") {
    state.userTools.modal = { activeTab: "mcp" };
  }
  if (!state.userTools.mcp || typeof state.userTools.mcp !== "object") {
    state.userTools.mcp = {
      servers: [],
      toolsByIndex: [],
      selectedIndex: -1,
      saveVersion: 0,
      loaded: false,
    };
  }
  if (!Array.isArray(state.userTools.mcp.servers)) {
    state.userTools.mcp.servers = [];
  }
  if (!Array.isArray(state.userTools.mcp.toolsByIndex)) {
    state.userTools.mcp.toolsByIndex = [];
  }
  if (!Number.isFinite(state.userTools.mcp.selectedIndex)) {
    state.userTools.mcp.selectedIndex = -1;
  }
  if (!Number.isFinite(state.userTools.mcp.saveVersion)) {
    state.userTools.mcp.saveVersion = 0;
  }
  if (typeof state.userTools.mcp.loaded !== "boolean") {
    state.userTools.mcp.loaded = false;
  }
  if (!state.userTools.skills || typeof state.userTools.skills !== "object") {
    state.userTools.skills = {
      skills: [],
      enabled: [],
      shared: [],
      detailVersion: 0,
      loaded: false,
    };
  }
  if (!Array.isArray(state.userTools.skills.skills)) {
    state.userTools.skills.skills = [];
  }
  if (!Array.isArray(state.userTools.skills.enabled)) {
    state.userTools.skills.enabled = [];
  }
  if (!Array.isArray(state.userTools.skills.shared)) {
    state.userTools.skills.shared = [];
  }
  if (!Number.isFinite(state.userTools.skills.detailVersion)) {
    state.userTools.skills.detailVersion = 0;
  }
  if (typeof state.userTools.skills.loaded !== "boolean") {
    state.userTools.skills.loaded = false;
  }
  if (!state.userTools.knowledge || typeof state.userTools.knowledge !== "object") {
    state.userTools.knowledge = {
      bases: [],
      selectedIndex: -1,
      files: [],
      activeFile: "",
      fileContent: "",
      loaded: false,
    };
  }
  if (!Array.isArray(state.userTools.knowledge.bases)) {
    state.userTools.knowledge.bases = [];
  }
  if (!Number.isFinite(state.userTools.knowledge.selectedIndex)) {
    state.userTools.knowledge.selectedIndex = -1;
  }
  if (!Array.isArray(state.userTools.knowledge.files)) {
    state.userTools.knowledge.files = [];
  }
  if (typeof state.userTools.knowledge.activeFile !== "string") {
    state.userTools.knowledge.activeFile = "";
  }
  if (typeof state.userTools.knowledge.fileContent !== "string") {
    state.userTools.knowledge.fileContent = "";
  }
  if (typeof state.userTools.knowledge.loaded !== "boolean") {
    state.userTools.knowledge.loaded = false;
  }
};

ensureToolSelectionState();
ensureUserToolsState();

// 获取当前已选择的工具名称列表
export const getSelectedToolNames = () => {
  ensureToolSelectionState();
  return Array.from(state.toolSelection.selected);
};

// 渲染系统提示词页的工具选择列表
const renderPromptToolList = (container, items, emptyText) => {
  container.textContent = "";
  if (!Array.isArray(items) || items.length === 0) {
    container.textContent = emptyText;
    return;
  }
  items.forEach((item) => {
    const row = document.createElement("div");
    row.className = "tool-item";
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = state.toolSelection.selected.has(item.name);
    checkbox.addEventListener("change", (event) => {
      let needsRerender = false;
      if (event.target.checked) {
        state.toolSelection.selected.add(item.name);
        if (item.name === A2UI_TOOL_NAME) {
          FINAL_TOOL_NAMES.forEach((name) => state.toolSelection.selected.delete(name));
          needsRerender = true;
        } else if (FINAL_TOOL_NAMES.has(item.name)) {
          state.toolSelection.selected.delete(A2UI_TOOL_NAME);
          needsRerender = true;
        }
      } else {
        state.toolSelection.selected.delete(item.name);
      }
      state.runtime.promptNeedsRefresh = true;
      if (needsRerender) {
        renderPromptTools();
      }
      schedulePromptReload();
      persistToolSelection();
    });
    const label = document.createElement("label");
    const metaParts = [];
    if (item.description) {
      metaParts.push(item.description);
    }
    if (item.owner_id) {
      metaParts.push(t("tools.owner", { owner: item.owner_id }));
    }
    const description = metaParts.length ? `<span class="muted">${metaParts.join(" · ")}</span>` : "";
    label.innerHTML = `<strong>${item.name}</strong>${description}`;
    row.appendChild(checkbox);
    row.appendChild(label);
    container.appendChild(row);
  });
};

export const renderPromptTools = () => {
  ensureToolSelectionState();
  renderPromptToolList(
    elements.promptBuiltinTools,
    state.toolSelection.builtin,
    t("tools.empty.builtin")
  );
  renderPromptToolList(elements.promptMcpTools, state.toolSelection.mcp, t("tools.empty.mcp"));
  renderPromptToolList(elements.promptA2aTools, state.toolSelection.a2a, t("tools.empty.a2a"));
  renderPromptToolList(elements.promptSkills, state.toolSelection.skills, t("tools.empty.skills"));
  renderPromptToolList(
    elements.promptKnowledgeTools,
    state.toolSelection.knowledge,
    t("tools.empty.knowledge")
  );
  renderPromptToolList(
    elements.promptUserTools,
    state.toolSelection.userTools,
    t("tools.empty.user")
  );
  renderPromptToolList(
    elements.promptSharedTools,
    state.toolSelection.sharedTools,
    t("tools.empty.shared")
  );
};

export const applyPromptToolError = (message) => {
  const text = message
    ? t("common.loadFailedWithMessage", { message })
    : t("common.loadFailed");
  elements.promptBuiltinTools.textContent = text;
  elements.promptMcpTools.textContent = text;
  if (elements.promptA2aTools) {
    elements.promptA2aTools.textContent = text;
  }
  elements.promptSkills.textContent = text;
  elements.promptKnowledgeTools.textContent = text;
  if (elements.promptUserTools) {
    elements.promptUserTools.textContent = text;
  }
  if (elements.promptSharedTools) {
    elements.promptSharedTools.textContent = text;
  }
};

// 加载可用工具清单，默认全选并渲染提示词页
export const loadAvailableTools = async () => {
  ensureToolSelectionState();
  ensureUserToolsState();
  const wunderBase = getWunderBase();
  const userId = String(
    elements.userId?.value || elements.promptUserId?.value || ""
  ).trim();
  const endpoint = userId
    ? `${wunderBase}/tools?user_id=${encodeURIComponent(userId)}`
    : `${wunderBase}/tools`;
  const response = await fetch(endpoint);
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const result = await response.json();
  const builtin = Array.isArray(result.builtin_tools) ? result.builtin_tools : [];
  const mcp = Array.isArray(result.mcp_tools) ? result.mcp_tools : [];
  const a2a = Array.isArray(result.a2a_tools) ? result.a2a_tools : [];
  const skills = Array.isArray(result.skills) ? result.skills : [];
  const knowledge = Array.isArray(result.knowledge_tools) ? result.knowledge_tools : [];
  const userTools = Array.isArray(result.user_tools) ? result.user_tools : [];
  const sharedTools = Array.isArray(result.shared_tools) ? result.shared_tools : [];
  const extraPrompt = typeof result.extra_prompt === "string" ? result.extra_prompt : "";
  const allNames = [
    ...builtin,
    ...mcp,
    ...a2a,
    ...skills,
    ...knowledge,
    ...userTools,
    ...sharedTools,
  ].map((item) => item.name);
  const allSet = new Set(allNames);
  const sharedSet = new Set(sharedTools.map((item) => item.name));
  const previousKnown = state.toolSelection.loaded
    ? new Set(
        [
          ...state.toolSelection.builtin,
          ...state.toolSelection.mcp,
          ...state.toolSelection.a2a,
          ...state.toolSelection.skills,
          ...state.toolSelection.knowledge,
          ...state.toolSelection.userTools,
          ...state.toolSelection.sharedTools,
        ].map((item) => item.name)
      )
    : new Set();

  // 首次加载优先用缓存；无缓存时默认全选，但共享工具默认不选
  if (!state.toolSelection.loaded) {
    const cached = loadCachedSelection(userId);
    if (cached && cached.selected.size) {
      const keep = new Set();
      cached.selected.forEach((name) => {
        if (allSet.has(name)) {
          keep.add(name);
        }
      });
      allNames.forEach((name) => {
        if (!cached.known.has(name) && !sharedSet.has(name) && !DEFAULT_UNSELECTED_TOOLS.has(name)) {
          keep.add(name);
        }
      });
      state.toolSelection.selected = keep;
    } else {
      state.toolSelection.selected = new Set(
        allNames.filter((name) => !sharedSet.has(name) && !DEFAULT_UNSELECTED_TOOLS.has(name))
      );
    }
  } else {
    const keep = new Set();
    state.toolSelection.selected.forEach((name) => {
      if (allSet.has(name)) {
        keep.add(name);
      }
    });
    allNames.forEach((name) => {
      if (!previousKnown.has(name) && !sharedSet.has(name) && !DEFAULT_UNSELECTED_TOOLS.has(name)) {
        keep.add(name);
      }
    });
    state.toolSelection.selected = keep;
  }

  state.toolSelection.builtin = builtin;
  state.toolSelection.mcp = mcp;
  state.toolSelection.a2a = a2a;
  state.toolSelection.skills = skills;
  state.toolSelection.knowledge = knowledge;
  state.toolSelection.userTools = userTools;
  state.toolSelection.sharedTools = sharedTools;
  state.toolSelection.loaded = true;
  persistToolSelection();
  const isEditingExtraPrompt =
    elements.promptExtraPrompt && document.activeElement === elements.promptExtraPrompt;
  if (!isEditingExtraPrompt) {
    state.userTools.extraPrompt = extraPrompt;
    if (elements.promptExtraPrompt) {
      elements.promptExtraPrompt.value = extraPrompt;
    }
  }
  renderPromptTools();
  state.runtime.promptNeedsRefresh = true;
  schedulePromptReload();
};

// 对外同步工具清单（用于 MCP/技能/内置工具变更后刷新提示词面板）
export const syncPromptTools = () => {
  loadAvailableTools().catch((error) => {
    applyPromptToolError(error.message);
  });
};

export const ensureToolSelectionLoaded = async () => {
  if (state.toolSelection.loaded) {
    return;
  }
  await loadAvailableTools();
};

export const resetToolSelection = () => {
  ensureToolSelectionState();
  state.toolSelection.loaded = false;
  state.toolSelection.selected = new Set();
};

// 在提示词页面可见时自动刷新提示词内容
export const schedulePromptReload = () => {
  if (state.runtime.activePanel !== "prompt") {
    return;
  }
  if (state.runtime.promptReloadTimer) {
    clearTimeout(state.runtime.promptReloadTimer);
  }
  state.runtime.promptReloadTimer = setTimeout(() => {
    if (typeof state.runtime.promptReloadHandler === "function") {
      state.runtime.promptReloadHandler();
    }
    state.runtime.promptReloadTimer = null;
  }, APP_CONFIG.promptReloadDelayMs);
};






