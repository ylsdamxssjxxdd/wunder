import { elements } from "./elements.js?v=20260104-03";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import {
  ensureToolSelectionLoaded,
  schedulePromptReload,
  syncPromptTools,
  ensureUserToolsState,
} from "./tools.js?v=20251227-13";
import { notify } from "./notify.js";
import { openToolDetailModal } from "./tool-detail.js?v=20251227-13";
import {
  buildHeadingHighlightHtml,
  getToolInputSchema,
  isPlainObject,
  parseHeadersValue,
} from "./utils.js?v=20251229-02";
import { t } from "./i18n.js?v=20260104-03";

// 自建工具统一使用输入即保存的节流时间，避免频繁写入
const SAVE_DEBOUNCE_MS = 600;

let mcpSaveTimer = null;
let skillsSaveTimer = null;
let extraPromptSaveTimer = null;
let userKnowledgeEditingIndex = -1;

const updateUserKnowledgeEditorHighlight = () => {
  if (!elements.userKnowledgeFileHighlight || !elements.userKnowledgeFileContent) {
    return;
  }
  const styles = window.getComputedStyle(elements.userKnowledgeFileContent);
  elements.userKnowledgeFileHighlight.style.font = styles.font;
  elements.userKnowledgeFileHighlight.style.letterSpacing = styles.letterSpacing;
  elements.userKnowledgeFileHighlight.style.wordSpacing = styles.wordSpacing;
  elements.userKnowledgeFileHighlight.style.textAlign = styles.textAlign;
  elements.userKnowledgeFileHighlight.style.textTransform = styles.textTransform;
  elements.userKnowledgeFileHighlight.style.textIndent = styles.textIndent;
  elements.userKnowledgeFileHighlight.style.textRendering = styles.textRendering;
  elements.userKnowledgeFileHighlight.style.whiteSpace = styles.whiteSpace;
  elements.userKnowledgeFileHighlight.style.wordBreak = styles.wordBreak;
  elements.userKnowledgeFileHighlight.style.overflowWrap = styles.overflowWrap;
  elements.userKnowledgeFileHighlight.style.tabSize = styles.tabSize;
  elements.userKnowledgeFileHighlight.style.direction = styles.direction;
  elements.userKnowledgeFileHighlight.style.setProperty(
    "--knowledge-editor-padding-top",
    styles.paddingTop
  );
  elements.userKnowledgeFileHighlight.style.setProperty(
    "--knowledge-editor-padding-right",
    styles.paddingRight
  );
  elements.userKnowledgeFileHighlight.style.setProperty(
    "--knowledge-editor-padding-bottom",
    styles.paddingBottom
  );
  elements.userKnowledgeFileHighlight.style.setProperty(
    "--knowledge-editor-padding-left",
    styles.paddingLeft
  );
  const borderX = parseFloat(styles.borderLeftWidth) + parseFloat(styles.borderRightWidth);
  const borderY = parseFloat(styles.borderTopWidth) + parseFloat(styles.borderBottomWidth);
  const scrollbarWidth = Math.max(
    0,
    elements.userKnowledgeFileContent.offsetWidth -
      elements.userKnowledgeFileContent.clientWidth -
      borderX
  );
  const scrollbarHeight = Math.max(
    0,
    elements.userKnowledgeFileContent.offsetHeight -
      elements.userKnowledgeFileContent.clientHeight -
      borderY
  );
  // 同步滚动条占位，避免自动换行宽度不一致导致高亮错位
  elements.userKnowledgeFileHighlight.style.setProperty(
    "--knowledge-scrollbar-width",
    `${scrollbarWidth}px`
  );
  elements.userKnowledgeFileHighlight.style.setProperty(
    "--knowledge-scrollbar-height",
    `${scrollbarHeight}px`
  );
  // 同步渲染一级标题高亮，方便识别知识条目
  elements.userKnowledgeFileHighlight.innerHTML = buildHeadingHighlightHtml(
    elements.userKnowledgeFileContent.value
  );
  syncUserKnowledgeEditorScroll();
};

const syncUserKnowledgeEditorScroll = () => {
  if (!elements.userKnowledgeFileHighlight || !elements.userKnowledgeFileContent) {
    return;
  }
  elements.userKnowledgeFileHighlight.scrollTop = elements.userKnowledgeFileContent.scrollTop;
  elements.userKnowledgeFileHighlight.scrollLeft = elements.userKnowledgeFileContent.scrollLeft;
};

const getUserId = () =>
  String(elements.userId?.value || elements.promptUserId?.value || "").trim();

const clearSaveTimer = (timer) => {
  if (timer) {
    clearTimeout(timer);
  }
  return null;
};

const clearSaveTimers = () => {
  mcpSaveTimer = clearSaveTimer(mcpSaveTimer);
  skillsSaveTimer = clearSaveTimer(skillsSaveTimer);
  extraPromptSaveTimer = clearSaveTimer(extraPromptSaveTimer);
};

const updateModalStatus = (message) => {
  if (elements.userToolSaveStatus) {
    elements.userToolSaveStatus.textContent = message || "";
  }
};

const updateExtraPromptStatus = (message) => {
  if (elements.promptExtraPromptStatus) {
    elements.promptExtraPromptStatus.textContent = message || "";
  }
};

const setActiveTab = (tab) => {
  ensureUserToolsState();
  const next = tab || "mcp";
  state.userTools.modal.activeTab = next;
  const tabMap = [
    { key: "mcp", btn: elements.userToolTabMcp, pane: elements.userToolPaneMcp },
    { key: "skills", btn: elements.userToolTabSkills, pane: elements.userToolPaneSkills },
    {
      key: "knowledge",
      btn: elements.userToolTabKnowledge,
      pane: elements.userToolPaneKnowledge,
    },
  ];
  tabMap.forEach(({ key, btn, pane }) => {
    if (btn) {
      btn.classList.toggle("active", key === next);
    }
    if (pane) {
      pane.classList.toggle("active", key === next);
    }
  });
};

const openUserToolModal = async () => {
  const userId = getUserId();
  if (!userId) {
    notify(t("userTools.userIdRequired"), "warn");
    updateModalStatus(t("common.userIdRequired"));
    return;
  }
  ensureUserToolsState();
  elements.userToolModal.classList.add("active");
  setActiveTab(state.userTools.modal.activeTab || "mcp");
  updateModalStatus(t("common.loading"));
  try {
    await Promise.all([loadUserMcpServers(), loadUserSkills(), loadUserKnowledgeConfig()]);
    updateModalStatus("");
  } catch (error) {
    updateModalStatus(t("common.loadFailedWithMessage", { message: error.message }));
  }
};

const closeUserToolModal = () => {
  elements.userToolModal.classList.remove("active");
};

export const resetUserToolsState = () => {
  ensureUserToolsState();
  clearSaveTimers();
  userKnowledgeEditingIndex = -1;
  state.userTools.extraPrompt = "";
  state.userTools.modal.activeTab = "mcp";
  state.userTools.mcp = {
    servers: [],
    toolsByIndex: [],
    selectedIndex: -1,
    saveVersion: 0,
    loaded: false,
  };
  state.userTools.skills = {
    skills: [],
    enabled: [],
    shared: [],
    detailVersion: 0,
    loaded: false,
  };
  state.userTools.knowledge = {
    bases: [],
    selectedIndex: -1,
    files: [],
    activeFile: "",
    fileContent: "",
    loaded: false,
  };
  if (elements.promptExtraPrompt) {
    elements.promptExtraPrompt.value = "";
  }
  if (elements.promptExtraPromptStatus) {
    elements.promptExtraPromptStatus.textContent = "";
  }
  if (elements.userToolSaveStatus) {
    elements.userToolSaveStatus.textContent = "";
  }
};

// MCP 自建工具：服务配置、工具启用与共享
const buildUserMcpStructPreview = (server) => {
  if (!server || !server.name || !server.endpoint) {
    return "";
  }
  const config = {
    type: server.transport || undefined,
    description: server.description || undefined,
    isActive: server.enabled !== false,
    name: server.display_name || server.name,
    baseUrl: server.endpoint,
    headers: server.headers && Object.keys(server.headers).length ? server.headers : undefined,
  };
  const cleaned = {};
  Object.entries(config).forEach(([key, value]) => {
    if (value !== undefined && value !== "") {
      cleaned[key] = value;
    }
  });
  return JSON.stringify({ mcpServers: { [server.name]: cleaned } }, null, 2);
};

const normalizeUserMcpServer = (server) => {
  const headers = isPlainObject(server?.headers) ? server.headers : {};
  const rawToolSpecs = Array.isArray(server?.tool_specs)
    ? server.tool_specs
    : Array.isArray(server?.toolSpecs)
    ? server.toolSpecs
    : [];
  return {
    name: server?.name || "",
    display_name: server?.display_name || server?.displayName || "",
    endpoint: server?.endpoint || server?.baseUrl || server?.base_url || server?.url || "",
    transport: server?.transport || server?.type || "",
    description: server?.description || "",
    headers,
    auth: server?.auth || "",
    allow_tools: Array.isArray(server?.allow_tools) ? server.allow_tools : [],
    shared_tools: Array.isArray(server?.shared_tools) ? server.shared_tools : [],
    enabled: server?.enabled !== false,
    tool_specs: rawToolSpecs,
  };
};

const buildUserMcpServerFromConfig = (serverId, rawConfig) => {
  const config = rawConfig || {};
  const endpoint = config.baseUrl || config.base_url || config.url || config.endpoint || "";
  const name = String(serverId || config.id || config.name || "").trim();
  if (!name || !endpoint) {
    return null;
  }
  let displayName = config.display_name || config.displayName || "";
  displayName = String(displayName || "").trim();
  let headers = config.headers || {};
  if (typeof headers === "string") {
    try {
      headers = JSON.parse(headers);
    } catch (error) {
      headers = {};
    }
  }
  if (!isPlainObject(headers)) {
    headers = {};
  }
  return normalizeUserMcpServer({
    name,
    display_name: displayName,
    endpoint,
    transport: config.type || config.transport || "",
    description: config.description || "",
    headers,
    auth: config.auth || "",
    allow_tools: config.allow_tools || config.allowTools || [],
    enabled: config.isActive ?? config.enabled ?? true,
    tool_specs: [],
  });
};

const upsertUserMcpServer = (incoming) => {
  const targetIndex = state.userTools.mcp.servers.findIndex((item) => item.name === incoming.name);
  if (targetIndex >= 0) {
    const previous = state.userTools.mcp.servers[targetIndex];
    const allowTools =
      Array.isArray(incoming.allow_tools) && incoming.allow_tools.length
        ? incoming.allow_tools
        : previous.allow_tools;
    const toolSpecs =
      Array.isArray(incoming.tool_specs) && incoming.tool_specs.length
        ? incoming.tool_specs
        : previous.tool_specs;
    state.userTools.mcp.servers[targetIndex] = {
      ...previous,
      ...incoming,
      allow_tools: allowTools,
      tool_specs: toolSpecs,
    };
    state.userTools.mcp.toolsByIndex[targetIndex] = toolSpecs || [];
    return targetIndex;
  }
  state.userTools.mcp.servers.push(incoming);
  state.userTools.mcp.toolsByIndex.push(incoming.tool_specs || []);
  return state.userTools.mcp.servers.length - 1;
};

const openUserMcpImportModal = () => {
  elements.userMcpImportInput.value = "";
  elements.userMcpImportModal.classList.add("active");
};

const closeUserMcpImportModal = () => {
  elements.userMcpImportModal.classList.remove("active");
};

const openUserMcpModal = (title) => {
  if (elements.userMcpModalTitle) {
    elements.userMcpModalTitle.textContent = title || t("userTools.mcp.modal.editTitle");
  }
  elements.userMcpModal.classList.add("active");
  updateUserMcpStructPreview();
};

const closeUserMcpModal = () => {
  elements.userMcpModal.classList.remove("active");
};

const applyUserMcpModal = async () => {
  if (elements.userMcpHeadersError?.textContent) {
    notify(t("userTools.mcp.headers.invalid"), "warn");
    return;
  }
  const saved = await saveUserMcpServers();
  if (saved) {
    closeUserMcpModal();
    notify(t("userTools.mcp.saved"), "success");
  }
};

const importUserMcpServers = async (raw) => {
  const content = (raw || "").trim();
  if (!content) {
    notify(t("userTools.mcp.struct.required"), "warn");
    return false;
  }
  let parsed = null;
  try {
    parsed = JSON.parse(content);
  } catch (error) {
    notify(t("userTools.mcp.struct.parseFailed"), "error");
    return false;
  }
  const imported = [];
  if (parsed.mcpServers && isPlainObject(parsed.mcpServers)) {
    Object.entries(parsed.mcpServers).forEach(([serverId, config]) => {
      const server = buildUserMcpServerFromConfig(serverId, config);
      if (server) {
        imported.push(server);
      }
    });
  } else {
    const serverId = parsed.id || parsed.name || "";
    const server = buildUserMcpServerFromConfig(serverId, parsed);
    if (server) {
      imported.push(server);
    }
  }
  if (!imported.length) {
    notify(t("userTools.mcp.struct.noValid"), "warn");
    return false;
  }
  let lastIndex = state.userTools.mcp.selectedIndex;
  imported.forEach((server) => {
    lastIndex = upsertUserMcpServer(server);
  });
  state.userTools.mcp.selectedIndex = lastIndex;
  renderUserMcpServers();
  renderUserMcpDetail();
  const saved = await saveUserMcpServers();
  if (!saved) {
    return false;
  }
  notify(t("userTools.mcp.import.success"), "success");
  return true;
};

const applyUserMcpImportModal = async () => {
  const ok = await importUserMcpServers(elements.userMcpImportInput.value);
  if (ok) {
    closeUserMcpImportModal();
  }
};

const getActiveUserMcpServer = () =>
  state.userTools.mcp.servers[state.userTools.mcp.selectedIndex] || null;

const updateUserMcpStructPreview = () => {
  const server = getActiveUserMcpServer();
  if (!elements.userMcpStructPreview) {
    return;
  }
  const preview = buildUserMcpStructPreview(server);
  elements.userMcpStructPreview.value = preview || t("mcp.struct.preview.empty");
};

const renderUserMcpServers = () => {
  elements.userMcpServerList.textContent = "";
  const servers = state.userTools.mcp.servers;
  if (!servers.length) {
    elements.userMcpServerList.textContent = t("userTools.mcp.list.empty");
    renderUserMcpDetail();
    updateUserMcpConnectButton();
    updateUserMcpRefreshAllButton();
    return;
  }
  servers.forEach((server, index) => {
    const item = document.createElement("button");
    item.type = "button";
    item.className = "list-item";
    if (index === state.userTools.mcp.selectedIndex) {
      item.classList.add("active");
    }
    const title = server.display_name || server.name || t("mcp.server.unnamed");
    const subtitleParts = [];
    if (server.display_name && server.name) {
      subtitleParts.push(`ID: ${server.name}`);
    }
    subtitleParts.push(server.endpoint || "-");
    item.innerHTML = `<div>${title}</div><small>${subtitleParts.join(" · ")}</small>`;
    item.addEventListener("click", () => {
      state.userTools.mcp.selectedIndex = index;
      renderUserMcpServers();
      renderUserMcpDetail();
    });
    elements.userMcpServerList.appendChild(item);
  });
  renderUserMcpDetail();
  updateUserMcpConnectButton();
  updateUserMcpRefreshAllButton();
};

const toggleUserMcpDetailDisabled = (disabled) => {
  const fields = [
    elements.userMcpName,
    elements.userMcpDisplayName,
    elements.userMcpEndpoint,
    elements.userMcpTransport,
    elements.userMcpDescription,
    elements.userMcpHeaders,
    elements.userMcpEnabled,
  ];
  fields.forEach((field) => {
    if (field) {
      field.disabled = disabled;
    }
  });
  if (elements.userMcpConnectBtn) {
    elements.userMcpConnectBtn.disabled = disabled;
  }
  if (elements.userMcpEnableAllBtn) {
    elements.userMcpEnableAllBtn.disabled = disabled;
  }
  if (elements.userMcpDisableAllBtn) {
    elements.userMcpDisableAllBtn.disabled = disabled;
  }
  if (elements.userMcpEditBtn) {
    elements.userMcpEditBtn.disabled = disabled;
  }
  if (elements.userMcpDeleteBtn) {
    elements.userMcpDeleteBtn.disabled = disabled;
  }
  if (elements.userMcpModalSave) {
    elements.userMcpModalSave.disabled = disabled;
  }
};

const renderUserMcpDetail = () => {
  const server = getActiveUserMcpServer();
  if (!server) {
    elements.userMcpDetailTitle.textContent = t("mcp.detail.none");
    elements.userMcpDetailMeta.textContent = "";
    elements.userMcpDetailDesc.textContent = "";
    elements.userMcpToolList.textContent = t("userTools.mcp.tools.select");
    elements.userMcpName.value = "";
    elements.userMcpDisplayName.value = "";
    elements.userMcpEndpoint.value = "";
    elements.userMcpTransport.value = "";
    elements.userMcpDescription.value = "";
    elements.userMcpHeaders.value = "";
    elements.userMcpHeadersError.textContent = "";
    elements.userMcpEnabled.checked = false;
    updateUserMcpStructPreview();
    toggleUserMcpDetailDisabled(true);
    updateUserMcpConnectButton();
    return;
  }
  const title = server.display_name || server.name || t("mcp.server.unnamed");
  const metaParts = [];
  if (server.display_name && server.name) {
    metaParts.push(`ID: ${server.name}`);
  }
  if (server.endpoint) {
    metaParts.push(server.endpoint);
  }
  if (server.transport) {
    metaParts.push(`transport=${server.transport}`);
  }
  metaParts.push(
    server.enabled !== false ? t("mcp.status.enabled") : t("mcp.status.disabled")
  );
  elements.userMcpDetailTitle.textContent = title;
  elements.userMcpDetailMeta.textContent = metaParts.join(" · ");
  elements.userMcpDetailDesc.textContent = server.description || "";
  elements.userMcpName.value = server.name || "";
  elements.userMcpDisplayName.value = server.display_name || "";
  elements.userMcpEndpoint.value = server.endpoint || "";
  elements.userMcpTransport.value = server.transport || "";
  elements.userMcpDescription.value = server.description || "";
  elements.userMcpHeaders.value =
    server.headers && Object.keys(server.headers).length
      ? JSON.stringify(server.headers, null, 2)
      : "";
  elements.userMcpHeadersError.textContent = "";
  elements.userMcpEnabled.checked = server.enabled !== false;
  updateUserMcpStructPreview();
  toggleUserMcpDetailDisabled(false);
  renderUserMcpTools();
  updateUserMcpConnectButton();
};

const isUserMcpServerConnected = (index) => {
  const tools = state.userTools.mcp.toolsByIndex[index];
  return Array.isArray(tools) && tools.length > 0;
};

const updateUserMcpConnectButton = () => {
  if (!elements.userMcpConnectBtn) {
    return;
  }
  const server = getActiveUserMcpServer();
  const connected = server ? isUserMcpServerConnected(state.userTools.mcp.selectedIndex) : false;
  const iconClass = connected ? "fa-solid fa-arrows-rotate" : "fa-solid fa-link";
  const label = connected ? t("mcp.connect.refresh") : t("mcp.connect.connect");
  elements.userMcpConnectBtn.innerHTML = `<i class="${iconClass}"></i>${label}`;
  elements.userMcpConnectBtn.disabled = !server;
};

const updateUserMcpRefreshAllButton = () => {
  if (!elements.userMcpRefreshAllBtn) {
    return;
  }
  const hasConnected = state.userTools.mcp.servers.some((_, index) =>
    isUserMcpServerConnected(index)
  );
  elements.userMcpRefreshAllBtn.disabled = !hasConnected;
};

const renderUserMcpTools = () => {
  elements.userMcpToolList.textContent = "";
  const server = getActiveUserMcpServer();
  if (!server) {
    elements.userMcpToolList.textContent = t("userTools.mcp.tools.select");
    return;
  }
  const tools = state.userTools.mcp.toolsByIndex[state.userTools.mcp.selectedIndex] || [];
  if (!tools.length) {
    elements.userMcpToolList.textContent = t("mcp.tools.notLoaded");
    return;
  }
  const allowList = Array.isArray(server.allow_tools) ? server.allow_tools : [];
  const sharedList = Array.isArray(server.shared_tools) ? server.shared_tools : [];
  const implicitAll = server.enabled !== false && allowList.length === 0;
  tools.forEach((tool) => {
    const item = document.createElement("div");
    item.className = "tool-item tool-item-dual";
    const enableLabel = document.createElement("label");
    enableLabel.className = "tool-check";
    const enableCheckbox = document.createElement("input");
    enableCheckbox.type = "checkbox";
    const enabled = implicitAll || allowList.includes(tool.name);
    enableCheckbox.checked = enabled;
    const enableText = document.createElement("span");
    enableText.textContent = t("userTools.action.enable");
    enableLabel.appendChild(enableCheckbox);
    enableLabel.appendChild(enableText);

    const shareLabel = document.createElement("label");
    shareLabel.className = "tool-check";
    const shareCheckbox = document.createElement("input");
    shareCheckbox.type = "checkbox";
    shareCheckbox.checked = sharedList.includes(tool.name);
    const shareText = document.createElement("span");
    shareText.textContent = t("userTools.action.share");
    shareLabel.appendChild(shareCheckbox);
    shareLabel.appendChild(shareText);

    const info = document.createElement("label");
    info.className = "tool-item-info";
    const desc = tool.description ? `<span class="muted">${tool.description}</span>` : "";
    info.innerHTML = `<strong>${tool.name}</strong>${desc}`;

    item.addEventListener("click", (event) => {
      if (enableLabel.contains(event.target) || shareLabel.contains(event.target)) {
        return;
      }
      const serverTitle = server.display_name || server.name || t("mcp.server.unnamed");
      const metaParts = [
        t("userTools.mcp.tool.label"),
        t("mcp.tool.server", { name: serverTitle }),
      ];
      metaParts.push(
        server.enabled !== false ? t("mcp.tool.serverEnabled") : t("mcp.tool.serverDisabled")
      );
      metaParts.push(
        enableCheckbox.checked ? t("mcp.status.enabled") : t("mcp.status.disabled")
      );
      metaParts.push(
        shareCheckbox.checked ? t("userTools.shared.on") : t("userTools.shared.off")
      );
      openToolDetailModal({
        title: tool.name || t("tool.detail.title"),
        meta: metaParts.join(" · "),
        description: tool.description || "",
        schema: getToolInputSchema(tool),
      });
    });

    enableCheckbox.addEventListener("change", (event) => {
      const allTools = tools.map((entry) => entry.name);
      let nextAllow = allowList.slice();
      if (implicitAll) {
        nextAllow = allTools.slice();
      }
      if (event.target.checked) {
        if (!nextAllow.includes(tool.name)) {
          nextAllow.push(tool.name);
        }
        server.enabled = true;
      } else {
        nextAllow = nextAllow.filter((name) => name !== tool.name);
        server.shared_tools = (Array.isArray(server.shared_tools) ? server.shared_tools : []).filter(
          (name) => name !== tool.name
        );
        if (nextAllow.length === 0) {
          server.enabled = false;
        }
      }
      server.allow_tools = nextAllow;
      scheduleUserMcpSave();
      renderUserMcpDetail();
    });

    shareCheckbox.addEventListener("change", (event) => {
      const allTools = tools.map((entry) => entry.name);
      let nextAllow = allowList.slice();
      if (implicitAll) {
        nextAllow = allTools.slice();
      }
      let nextShared = Array.isArray(server.shared_tools) ? server.shared_tools.slice() : [];
      if (event.target.checked) {
        if (!nextShared.includes(tool.name)) {
          nextShared.push(tool.name);
        }
        if (!nextAllow.includes(tool.name)) {
          nextAllow.push(tool.name);
        }
        server.enabled = true;
      } else {
        nextShared = nextShared.filter((name) => name !== tool.name);
      }
      server.allow_tools = nextAllow;
      server.shared_tools = nextShared;
      scheduleUserMcpSave();
      renderUserMcpDetail();
    });

    item.appendChild(enableLabel);
    item.appendChild(shareLabel);
    item.appendChild(info);
    elements.userMcpToolList.appendChild(item);
  });
  updateUserMcpRefreshAllButton();
};

const loadUserMcpServers = async () => {
  ensureUserToolsState();
  const userId = getUserId();
  if (!userId) {
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/user_tools/mcp?user_id=${encodeURIComponent(userId)}`;
  const response = await fetch(endpoint);
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const result = await response.json();
  const servers = Array.isArray(result.servers) ? result.servers : [];
  state.userTools.mcp.servers = servers.map(normalizeUserMcpServer);
  state.userTools.mcp.toolsByIndex = state.userTools.mcp.servers.map(
    (server) => server.tool_specs || []
  );
  state.userTools.mcp.selectedIndex = state.userTools.mcp.servers.length ? 0 : -1;
  state.userTools.mcp.loaded = true;
  renderUserMcpServers();
};

const saveUserMcpServers = async () => {
  const userId = getUserId();
  if (!userId) {
    updateModalStatus(t("common.userIdRequired"));
    return false;
  }
  const saveVersion = ++state.userTools.mcp.saveVersion;
  updateModalStatus(t("userTools.saving"));
  const payload = {
    user_id: userId,
    servers: state.userTools.mcp.servers.map((server) => ({
      name: server.name,
      display_name: server.display_name,
      endpoint: server.endpoint,
      transport: server.transport,
      description: server.description,
      headers: server.headers || {},
      auth: server.auth || "",
      tool_specs: Array.isArray(server.tool_specs) ? server.tool_specs : [],
      allow_tools: Array.isArray(server.allow_tools) ? server.allow_tools : [],
      shared_tools: Array.isArray(server.shared_tools) ? server.shared_tools : [],
      enabled: server.enabled !== false,
    })),
  };
  try {
    const wunderBase = getWunderBase();
    const endpoint = `${wunderBase}/user_tools/mcp`;
    const response = await fetch(endpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    if (!response.ok) {
      throw new Error(t("common.requestFailed", { status: response.status }));
    }
    if (saveVersion !== state.userTools.mcp.saveVersion) {
      return;
    }
    const result = await response.json();
    const servers = Array.isArray(result.servers) ? result.servers : [];
    state.userTools.mcp.servers = servers.map(normalizeUserMcpServer);
    state.userTools.mcp.toolsByIndex = state.userTools.mcp.servers.map(
      (server) => server.tool_specs || []
    );
    if (state.userTools.mcp.servers.length === 0) {
      state.userTools.mcp.selectedIndex = -1;
    } else if (state.userTools.mcp.selectedIndex >= state.userTools.mcp.servers.length) {
      state.userTools.mcp.selectedIndex = 0;
    }
    renderUserMcpServers();
    updateModalStatus(t("userTools.autoSaved"));
    syncPromptTools();
    return true;
  } catch (error) {
    updateModalStatus(t("userTools.saveFailed", { message: error.message }));
    notify(t("userTools.mcp.saveFailed", { message: error.message }), "error");
    return false;
  }
};

const scheduleUserMcpSave = () => {
  if (mcpSaveTimer) {
    clearTimeout(mcpSaveTimer);
  }
  mcpSaveTimer = setTimeout(() => {
    mcpSaveTimer = null;
    saveUserMcpServers();
  }, SAVE_DEBOUNCE_MS);
};

const connectUserMcpServerAtIndex = async (index, options = {}) => {
  const updateUI = options.updateUI !== false;
  const server = state.userTools.mcp.servers[index];
  if (!server || !server.name || !server.endpoint) {
    return false;
  }
  const payload = {
    name: server.name,
    endpoint: server.endpoint,
    transport: server.transport || null,
    headers: server.headers || {},
    auth: server.auth || null,
  };
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/user_tools/mcp/tools`;
  const response = await fetch(endpoint, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });
  if (!response.ok) {
    if (updateUI) {
      elements.userMcpToolList.textContent = t("common.requestFailed", { status: response.status });
    }
    return false;
  }
  const result = await response.json();
  const tools = Array.isArray(result.tools) ? result.tools : [];
  state.userTools.mcp.toolsByIndex[index] = tools;
  server.tool_specs = tools;
  if (updateUI) {
    renderUserMcpTools();
  }
  updateUserMcpConnectButton();
  updateUserMcpRefreshAllButton();
  scheduleUserMcpSave();
  return true;
};

const connectUserMcpServer = async () => {
  const index = state.userTools.mcp.selectedIndex;
  const wasConnected = index >= 0 ? isUserMcpServerConnected(index) : false;
  const ok = await connectUserMcpServerAtIndex(index, { updateUI: true });
  if (!ok) {
    notify(t("mcp.connect.failed"), "error");
    return;
  }
  notify(
    wasConnected ? t("mcp.connect.refreshed") : t("mcp.connect.connected"),
    "success"
  );
};

const refreshAllUserMcpServers = async () => {
  const connectedIndexes = state.userTools.mcp.servers
    .map((_, index) => index)
    .filter((index) => isUserMcpServerConnected(index));
  if (!connectedIndexes.length) {
    return;
  }
  let updated = false;
  const selectedIndex = state.userTools.mcp.selectedIndex;
  for (const index of connectedIndexes) {
    const ok = await connectUserMcpServerAtIndex(index, { updateUI: index === selectedIndex });
    if (ok) {
      updated = true;
    }
  }
  if (!updated) {
    notify(t("mcp.refresh.failed"), "error");
    return;
  }
  updateUserMcpConnectButton();
  updateUserMcpRefreshAllButton();
  notify(t("mcp.refresh.allSuccess"), "success");
};

const addUserMcpServer = () => {
  const next = normalizeUserMcpServer({
    name: "",
    display_name: "",
    endpoint: "",
    transport: "",
    description: "",
    headers: {},
    allow_tools: [],
    shared_tools: [],
    enabled: true,
    tool_specs: [],
  });
  state.userTools.mcp.servers.push(next);
  state.userTools.mcp.toolsByIndex.push([]);
  state.userTools.mcp.selectedIndex = state.userTools.mcp.servers.length - 1;
  renderUserMcpServers();
  if (elements.userMcpName) {
    elements.userMcpName.focus();
  }
};

const deleteUserMcpServer = () => {
  if (state.userTools.mcp.selectedIndex < 0) {
    return;
  }
  const removed = state.userTools.mcp.servers[state.userTools.mcp.selectedIndex];
  const removedName = removed?.display_name || removed?.name || t("mcp.server.defaultName");
  if (!window.confirm(t("userTools.mcp.deleteConfirm", { name: removedName }))) {
    return;
  }
  state.userTools.mcp.servers.splice(state.userTools.mcp.selectedIndex, 1);
  state.userTools.mcp.toolsByIndex.splice(state.userTools.mcp.selectedIndex, 1);
  if (!state.userTools.mcp.servers.length) {
    state.userTools.mcp.selectedIndex = -1;
  } else {
    state.userTools.mcp.selectedIndex = Math.max(0, state.userTools.mcp.selectedIndex - 1);
  }
  renderUserMcpServers();
  scheduleUserMcpSave();
  notify(t("mcp.delete.success", { name: removedName }), "success");
};

const bindUserMcpInputs = () => {
  elements.userMcpName.addEventListener("input", (event) => {
    const server = getActiveUserMcpServer();
    if (!server) {
      return;
    }
    server.name = event.target.value.trim();
    renderUserMcpServers();
    updateUserMcpStructPreview();
    scheduleUserMcpSave();
  });
  elements.userMcpDisplayName.addEventListener("input", (event) => {
    const server = getActiveUserMcpServer();
    if (!server) {
      return;
    }
    server.display_name = event.target.value.trim();
    renderUserMcpServers();
    updateUserMcpStructPreview();
    scheduleUserMcpSave();
  });
  elements.userMcpEndpoint.addEventListener("input", (event) => {
    const server = getActiveUserMcpServer();
    if (!server) {
      return;
    }
    server.endpoint = event.target.value.trim();
    renderUserMcpServers();
    updateUserMcpStructPreview();
    scheduleUserMcpSave();
  });
  elements.userMcpTransport.addEventListener("change", (event) => {
    const server = getActiveUserMcpServer();
    if (!server) {
      return;
    }
    server.transport = event.target.value.trim();
    updateUserMcpStructPreview();
    scheduleUserMcpSave();
  });
  elements.userMcpDescription.addEventListener("input", (event) => {
    const server = getActiveUserMcpServer();
    if (!server) {
      return;
    }
    server.description = event.target.value.trim();
    renderUserMcpDetail();
    updateUserMcpStructPreview();
    scheduleUserMcpSave();
  });
  elements.userMcpHeaders.addEventListener("input", () => {
    const server = getActiveUserMcpServer();
    if (!server) {
      return;
    }
    const parsed = parseHeadersValue(elements.userMcpHeaders.value);
    if (parsed.error) {
      elements.userMcpHeadersError.textContent = parsed.error;
      return;
    }
    elements.userMcpHeadersError.textContent = "";
    server.headers = parsed.headers || {};
    updateUserMcpStructPreview();
    scheduleUserMcpSave();
  });
  elements.userMcpEnabled.addEventListener("change", (event) => {
    const server = getActiveUserMcpServer();
    if (!server) {
      return;
    }
    server.enabled = event.target.checked;
    renderUserMcpDetail();
    updateUserMcpStructPreview();
    scheduleUserMcpSave();
  });
};

// 技能自建工具：上传、启用与共享
const loadUserSkills = async () => {
  ensureUserToolsState();
  const userId = getUserId();
  if (!userId) {
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/user_tools/skills?user_id=${encodeURIComponent(userId)}`;
  const response = await fetch(endpoint);
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const result = await response.json();
  state.userTools.skills.enabled = Array.isArray(result.enabled) ? result.enabled : [];
  state.userTools.skills.shared = Array.isArray(result.shared) ? result.shared : [];
  state.userTools.skills.skills = Array.isArray(result.skills) ? result.skills : [];
  state.userTools.skills.loaded = true;
  renderUserSkills();
};

const openUserSkillDetailModal = async (skill) => {
  const userId = getUserId();
  if (!userId || !skill?.name) {
    return;
  }
  const currentVersion = ++state.userTools.skills.detailVersion;
  elements.skillModalTitle.textContent = skill.name || t("skills.detail.title");
  elements.skillModalMeta.textContent = skill.path || "";
  elements.skillModalContent.textContent = t("common.loading");
  elements.skillModal.classList.add("active");
  try {
    const wunderBase = getWunderBase();
    const endpoint = `${wunderBase}/user_tools/skills/content?user_id=${encodeURIComponent(
      userId
    )}&name=${encodeURIComponent(skill.name)}`;
    const response = await fetch(endpoint);
    if (!response.ok) {
      throw new Error(t("common.requestFailed", { status: response.status }));
    }
    const result = await response.json();
    if (currentVersion !== state.userTools.skills.detailVersion) {
      return;
    }
    elements.skillModalContent.textContent = result.content || t("skills.detail.empty");
  } catch (error) {
    if (currentVersion !== state.userTools.skills.detailVersion) {
      return;
    }
    elements.skillModalContent.textContent = t("common.loadFailedWithMessage", {
      message: error.message,
    });
  }
};

const renderUserSkills = () => {
  elements.userSkillsList.textContent = "";
  if (!state.userTools.skills.skills.length) {
    elements.userSkillsList.textContent = t("userTools.skills.list.empty");
    return;
  }
  state.userTools.skills.skills.forEach((skill) => {
    const item = document.createElement("div");
    item.className = "skill-item tool-item-dual";
    const enableLabel = document.createElement("label");
    enableLabel.className = "tool-check";
    const enableCheckbox = document.createElement("input");
    enableCheckbox.type = "checkbox";
    enableCheckbox.checked = Boolean(skill.enabled);
    const enableText = document.createElement("span");
    enableText.textContent = t("userTools.action.enable");
    enableLabel.appendChild(enableCheckbox);
    enableLabel.appendChild(enableText);

    const shareLabel = document.createElement("label");
    shareLabel.className = "tool-check";
    const shareCheckbox = document.createElement("input");
    shareCheckbox.type = "checkbox";
    shareCheckbox.checked = Boolean(skill.shared);
    const shareText = document.createElement("span");
    shareText.textContent = t("userTools.action.share");
    shareLabel.appendChild(shareCheckbox);
    shareLabel.appendChild(shareText);

    const info = document.createElement("label");
    info.className = "tool-item-info";
    const descParts = [];
    if (skill.description) {
      descParts.push(skill.description);
    }
    if (skill.path) {
      descParts.push(skill.path);
    }
    const desc = descParts.length ? `<span class="muted">${descParts.join(" · ")}</span>` : "";
    info.innerHTML = `<strong>${skill.name}</strong>${desc}`;

    enableCheckbox.addEventListener("change", (event) => {
      skill.enabled = event.target.checked;
      if (!skill.enabled) {
        skill.shared = false;
      }
      renderUserSkills();
      scheduleUserSkillsSave();
    });
    shareCheckbox.addEventListener("change", (event) => {
      skill.shared = event.target.checked;
      if (skill.shared) {
        skill.enabled = true;
      }
      renderUserSkills();
      scheduleUserSkillsSave();
    });
    info.addEventListener("click", () => openUserSkillDetailModal(skill));

    item.appendChild(enableLabel);
    item.appendChild(shareLabel);
    item.appendChild(info);
    elements.userSkillsList.appendChild(item);
  });
};

const saveUserSkills = async () => {
  const userId = getUserId();
  if (!userId) {
    updateModalStatus(t("common.userIdRequired"));
    return;
  }
  updateModalStatus(t("userTools.saving"));
  const enabled = state.userTools.skills.skills
    .filter((skill) => skill.enabled)
    .map((skill) => skill.name);
  const shared = state.userTools.skills.skills
    .filter((skill) => skill.shared)
    .map((skill) => skill.name);
  const payload = { user_id: userId, enabled, shared };
  try {
    const wunderBase = getWunderBase();
    const endpoint = `${wunderBase}/user_tools/skills`;
    const response = await fetch(endpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    if (!response.ok) {
      throw new Error(t("common.requestFailed", { status: response.status }));
    }
    const result = await response.json();
    state.userTools.skills.enabled = Array.isArray(result.enabled) ? result.enabled : [];
    state.userTools.skills.shared = Array.isArray(result.shared) ? result.shared : [];
    state.userTools.skills.skills = Array.isArray(result.skills) ? result.skills : [];
    renderUserSkills();
    updateModalStatus(t("userTools.autoSaved"));
    syncPromptTools();
  } catch (error) {
    updateModalStatus(t("userTools.saveFailed", { message: error.message }));
    notify(t("userTools.skills.saveFailed", { message: error.message }), "error");
  }
};

const scheduleUserSkillsSave = () => {
  if (skillsSaveTimer) {
    clearTimeout(skillsSaveTimer);
  }
  skillsSaveTimer = setTimeout(() => {
    skillsSaveTimer = null;
    saveUserSkills();
  }, SAVE_DEBOUNCE_MS);
};

const uploadUserSkillZip = async (file) => {
  if (!file) {
    return;
  }
  const userId = getUserId();
  if (!userId) {
    throw new Error(t("common.userIdRequired"));
  }
  const filename = file.name || "";
  if (!filename.toLowerCase().endsWith(".zip")) {
    throw new Error(t("skills.upload.zipOnly"));
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/user_tools/skills/upload`;
  const form = new FormData();
  form.append("user_id", userId);
  form.append("file", file, filename);
  const response = await fetch(endpoint, {
    method: "POST",
    body: form,
  });
  if (!response.ok) {
    throw new Error(t("userTools.skills.uploadFailed", { message: response.status }));
  }
  await loadUserSkills();
  syncPromptTools();
};

// 知识库自建工具：配置、共享与文档管理
const normalizeUserKnowledgeConfig = (raw) => {
  const config = raw || {};
  return {
    bases: Array.isArray(config.bases)
      ? config.bases
          .filter((base) => String(base?.name || "").trim())
          .map((base) => ({
            name: base.name || "",
            description: base.description || "",
            root: base.root || "",
            enabled: base.enabled !== false,
            shared: Boolean(base.shared),
          }))
      : [],
  };
};

// doc2md 支持的扩展名列表（用于前端选择过滤）
const USER_KNOWLEDGE_UPLOAD_EXTENSIONS = [
  ".txt",
  ".md",
  ".markdown",
  ".html",
  ".htm",
  ".py",
  ".c",
  ".cpp",
  ".cc",
  ".h",
  ".hpp",
  ".json",
  ".js",
  ".ts",
  ".css",
  ".ini",
  ".cfg",
  ".log",
  ".doc",
  ".docx",
  ".odt",
  ".pptx",
  ".odp",
  ".xlsx",
  ".ods",
  ".wps",
  ".et",
  ".dps",
];
const USER_KNOWLEDGE_UPLOAD_ACCEPT = USER_KNOWLEDGE_UPLOAD_EXTENSIONS.join(",");

const getActiveUserKnowledgeBase = () =>
  state.userTools.knowledge.bases[state.userTools.knowledge.selectedIndex] || null;

// 打开用户知识库配置弹窗
const openUserKnowledgeModal = (base = null, index = -1) => {
  if (!elements.userKnowledgeModal) {
    return;
  }
  userKnowledgeEditingIndex = Number.isInteger(index) ? index : -1;
  const payload = base || { name: "", description: "", enabled: true, shared: false };
  if (elements.userKnowledgeModalTitle) {
    elements.userKnowledgeModalTitle.textContent =
      userKnowledgeEditingIndex >= 0
        ? t("knowledge.modal.editTitle")
        : t("knowledge.modal.addTitle");
  }
  if (elements.userKnowledgeModalName) {
    elements.userKnowledgeModalName.value = payload.name || "";
  }
  if (elements.userKnowledgeModalDesc) {
    elements.userKnowledgeModalDesc.value = payload.description || "";
  }
  if (elements.userKnowledgeModalEnabled) {
    elements.userKnowledgeModalEnabled.checked = payload.enabled !== false;
  }
  if (elements.userKnowledgeModalShared) {
    elements.userKnowledgeModalShared.checked = payload.shared === true;
  }
  elements.userKnowledgeModal.classList.add("active");
  elements.userKnowledgeModalName?.focus();
};

// 关闭用户知识库配置弹窗并清理状态
const closeUserKnowledgeModal = () => {
  if (!elements.userKnowledgeModal) {
    return;
  }
  elements.userKnowledgeModal.classList.remove("active");
  userKnowledgeEditingIndex = -1;
};

// 从弹窗中读取用户知识库配置
const getUserKnowledgeModalPayload = () => ({
  name: elements.userKnowledgeModalName?.value?.trim() || "",
  description: elements.userKnowledgeModalDesc?.value?.trim() || "",
  enabled: elements.userKnowledgeModalEnabled
    ? elements.userKnowledgeModalEnabled.checked
    : true,
  shared: elements.userKnowledgeModalShared?.checked === true,
});

// 校验用户知识库配置，避免空值或重名
const validateUserKnowledgeBase = (payload, index) => {
  if (!payload.name) {
    return t("knowledge.name.required");
  }
  for (let i = 0; i < state.userTools.knowledge.bases.length; i += 1) {
    if (i === index) {
      continue;
    }
    if (state.userTools.knowledge.bases[i].name.trim() === payload.name) {
      return t("knowledge.name.duplicate", { name: payload.name });
    }
  }
  return "";
};

const renderUserKnowledgeBaseList = () => {
  elements.userKnowledgeBaseList.textContent = "";
  if (!state.userTools.knowledge.bases.length) {
    elements.userKnowledgeBaseList.textContent = t("userTools.knowledge.list.empty");
    renderUserKnowledgeDetail();
    return;
  }
  state.userTools.knowledge.bases.forEach((base, index) => {
    const item = document.createElement("button");
    item.type = "button";
    item.className = "list-item";
    if (index === state.userTools.knowledge.selectedIndex) {
      item.classList.add("active");
    }
    const title = base.name || t("knowledge.name.unnamed");
    const subtitle = base.root || t("userTools.knowledge.root.uncreated");
    item.innerHTML = `<div>${title}</div><small>${subtitle}</small>`;
    item.addEventListener("click", async () => {
      state.userTools.knowledge.selectedIndex = index;
      state.userTools.knowledge.files = [];
      state.userTools.knowledge.activeFile = "";
      state.userTools.knowledge.fileContent = "";
      renderUserKnowledgeBaseList();
      renderUserKnowledgeDetail();
      await loadUserKnowledgeFiles();
    });
    elements.userKnowledgeBaseList.appendChild(item);
  });
};

const renderUserKnowledgeDetailHeader = () => {
  const base = getActiveUserKnowledgeBase();
  if (!base) {
    elements.userKnowledgeDetailTitle.textContent = t("knowledge.detail.empty");
    elements.userKnowledgeDetailMeta.textContent = "";
    if (elements.userKnowledgeDetailDesc) {
      elements.userKnowledgeDetailDesc.textContent = "";
    }
    if (elements.userKnowledgeEditBtn) {
      elements.userKnowledgeEditBtn.disabled = true;
    }
    elements.userKnowledgeDeleteBtn.disabled = true;
    return;
  }
  elements.userKnowledgeDetailTitle.textContent = base.name || t("knowledge.name.unnamed");
  const metaParts = [base.root || t("userTools.knowledge.root.uncreated")];
  metaParts.push(
    base.enabled !== false ? t("knowledge.status.enabled") : t("knowledge.status.disabled")
  );
  if (base.shared) {
    metaParts.push(t("userTools.shared.on"));
  }
  elements.userKnowledgeDetailMeta.textContent = metaParts.join(" · ");
  if (elements.userKnowledgeDetailDesc) {
    elements.userKnowledgeDetailDesc.textContent = base.description || "";
  }
  if (elements.userKnowledgeEditBtn) {
    elements.userKnowledgeEditBtn.disabled = false;
  }
  elements.userKnowledgeDeleteBtn.disabled = false;
};

const renderUserKnowledgeDetail = () => {
  renderUserKnowledgeDetailHeader();
  renderUserKnowledgeFiles();
};

const renderUserKnowledgeFiles = () => {
  elements.userKnowledgeFileList.textContent = "";
  if (!state.userTools.knowledge.files.length) {
    elements.userKnowledgeFileList.textContent = t("knowledge.file.empty");
  } else {
    state.userTools.knowledge.files.forEach((filePath) => {
      const item = document.createElement("div");
      item.className = "knowledge-file-item";
      if (filePath === state.userTools.knowledge.activeFile) {
        item.classList.add("active");
      }
      const name = document.createElement("span");
      name.className = "knowledge-file-name";
      name.textContent = filePath;
      const deleteBtn = document.createElement("button");
      deleteBtn.type = "button";
      deleteBtn.className = "knowledge-file-delete-btn";
      deleteBtn.title = t("knowledge.file.delete");
      deleteBtn.innerHTML = '<i class="fa-solid fa-trash"></i>';
      deleteBtn.addEventListener("click", async (event) => {
        event.stopPropagation();
        try {
          await deleteUserKnowledgeFile(filePath);
        } catch (error) {
          notify(t("knowledge.file.deleteFailed", { message: error.message }), "error");
        }
      });
      item.append(name, deleteBtn);
      item.addEventListener("click", () => {
        selectUserKnowledgeFile(filePath);
      });
      elements.userKnowledgeFileList.appendChild(item);
    });
  }
  elements.userKnowledgeFileName.textContent =
    state.userTools.knowledge.activeFile || t("knowledge.file.none");
  elements.userKnowledgeFileContent.value = state.userTools.knowledge.fileContent || "";
  updateUserKnowledgeEditorHighlight();
};

const buildUserKnowledgePayload = () => ({
  bases: state.userTools.knowledge.bases
    .map((base) => ({
      name: base.name.trim(),
      description: base.description || "",
      enabled: base.enabled !== false,
      shared: base.shared === true,
    }))
    .filter((base) => base.name),
});

const validateUserKnowledgePayload = (payload) => {
  const invalid = payload.bases.filter((base) => !base.name);
  if (invalid.length) {
    return t("knowledge.payload.invalid");
  }
  const nameSet = new Set();
  for (const base of payload.bases) {
    if (nameSet.has(base.name)) {
      return t("knowledge.name.duplicate", { name: base.name });
    }
    nameSet.add(base.name);
  }
  return "";
};

const saveUserKnowledgeConfig = async () => {
  const userId = getUserId();
  if (!userId) {
    updateModalStatus(t("common.userIdRequired"));
    return false;
  }
  updateModalStatus(t("userTools.saving"));
  const payload = buildUserKnowledgePayload();
  const error = validateUserKnowledgePayload(payload);
  if (error) {
    updateModalStatus(error);
    notify(error, "warn");
    return false;
  }
  try {
    const wunderBase = getWunderBase();
    const endpoint = `${wunderBase}/user_tools/knowledge`;
    const response = await fetch(endpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ user_id: userId, knowledge: payload }),
    });
    if (!response.ok) {
      throw new Error(t("common.requestFailed", { status: response.status }));
    }
    const result = await response.json();
    const normalized = normalizeUserKnowledgeConfig(result.knowledge || {});
    const currentBase = getActiveUserKnowledgeBase();
    const currentName = currentBase?.name || "";
    state.userTools.knowledge.bases = normalized.bases;
    if (!state.userTools.knowledge.bases.length) {
      state.userTools.knowledge.selectedIndex = -1;
    } else if (currentName) {
      const nextIndex = state.userTools.knowledge.bases.findIndex(
        (base) => base.name === currentName
      );
      state.userTools.knowledge.selectedIndex = nextIndex >= 0 ? nextIndex : 0;
    } else {
      state.userTools.knowledge.selectedIndex = 0;
    }
    state.userTools.knowledge.files = [];
    state.userTools.knowledge.activeFile = "";
    state.userTools.knowledge.fileContent = "";
    renderUserKnowledgeBaseList();
    renderUserKnowledgeDetail();
    updateModalStatus(t("userTools.saved"));
    syncPromptTools();
    return true;
  } catch (error) {
    updateModalStatus(t("userTools.saveFailed", { message: error.message }));
    notify(t("userTools.knowledge.saveFailed", { message: error.message }), "error");
    return false;
  }
};

const loadUserKnowledgeConfig = async () => {
  ensureUserToolsState();
  const userId = getUserId();
  if (!userId) {
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/user_tools/knowledge?user_id=${encodeURIComponent(userId)}`;
  const response = await fetch(endpoint);
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const result = await response.json();
  const normalized = normalizeUserKnowledgeConfig(result.knowledge || {});
  state.userTools.knowledge.bases = normalized.bases;
  state.userTools.knowledge.selectedIndex = state.userTools.knowledge.bases.length ? 0 : -1;
  state.userTools.knowledge.files = [];
  state.userTools.knowledge.activeFile = "";
  state.userTools.knowledge.fileContent = "";
  state.userTools.knowledge.loaded = true;
  renderUserKnowledgeBaseList();
  renderUserKnowledgeDetail();
  if (state.userTools.knowledge.selectedIndex >= 0) {
    await loadUserKnowledgeFiles();
  }
};

const loadUserKnowledgeFiles = async () => {
  const base = getActiveUserKnowledgeBase();
  const userId = getUserId();
  if (!userId || !base || !base.name) {
    state.userTools.knowledge.files = [];
    state.userTools.knowledge.activeFile = "";
    state.userTools.knowledge.fileContent = "";
    renderUserKnowledgeFiles();
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/user_tools/knowledge/files?user_id=${encodeURIComponent(
    userId
  )}&base=${encodeURIComponent(base.name)}`;
  elements.userKnowledgeFileList.textContent = t("common.loading");
  const response = await fetch(endpoint);
  if (!response.ok) {
    elements.userKnowledgeFileList.textContent = t("common.loadFailedWithMessage", {
      message: response.status,
    });
    return;
  }
  const result = await response.json();
  state.userTools.knowledge.files = Array.isArray(result.files) ? result.files : [];
  if (!state.userTools.knowledge.files.includes(state.userTools.knowledge.activeFile)) {
    state.userTools.knowledge.activeFile = "";
    state.userTools.knowledge.fileContent = "";
  }
  renderUserKnowledgeFiles();
};

const selectUserKnowledgeFile = async (filePath) => {
  const base = getActiveUserKnowledgeBase();
  const userId = getUserId();
  if (!userId || !base || !base.name) {
    notify(t("knowledge.base.selectRequired"), "warn");
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/user_tools/knowledge/file?user_id=${encodeURIComponent(
    userId
  )}&base=${encodeURIComponent(base.name)}&path=${encodeURIComponent(filePath)}`;
  elements.userKnowledgeFileName.textContent = t("common.loading");
  const response = await fetch(endpoint);
  if (!response.ok) {
    notify(t("knowledge.file.readFailed", { status: response.status }), "error");
    return;
  }
  const result = await response.json();
  state.userTools.knowledge.activeFile = result.path || filePath;
  state.userTools.knowledge.fileContent = result.content || "";
  renderUserKnowledgeFiles();
};

const saveUserKnowledgeFile = async () => {
  const base = getActiveUserKnowledgeBase();
  const userId = getUserId();
  if (!userId || !base || !base.name) {
    notify(t("knowledge.base.selectRequired"), "warn");
    return;
  }
  if (!state.userTools.knowledge.activeFile) {
    notify(t("knowledge.file.saveRequired"), "warn");
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/user_tools/knowledge/file`;
  const response = await fetch(endpoint, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      user_id: userId,
      base: base.name,
      path: state.userTools.knowledge.activeFile,
      content: elements.userKnowledgeFileContent.value,
    }),
  });
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  await loadUserKnowledgeFiles();
  notify(t("knowledge.file.saved"), "success");
};

// 支持从列表项直接删除指定文档
const deleteUserKnowledgeFile = async (targetPath = "") => {
  const base = getActiveUserKnowledgeBase();
  const userId = getUserId();
  if (!userId || !base || !base.name) {
    notify(t("knowledge.base.selectRequired"), "warn");
    return;
  }
  const path = targetPath || state.userTools.knowledge.activeFile;
  if (!path) {
    notify(t("knowledge.file.deleteRequired"), "warn");
    return;
  }
  if (!window.confirm(t("knowledge.file.deleteConfirm", { name: path }))) {
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/user_tools/knowledge/file?user_id=${encodeURIComponent(
    userId
  )}&base=${encodeURIComponent(base.name)}&path=${encodeURIComponent(path)}`;
  const response = await fetch(endpoint, { method: "DELETE" });
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  if (path === state.userTools.knowledge.activeFile) {
    state.userTools.knowledge.activeFile = "";
    state.userTools.knowledge.fileContent = "";
  }
  await loadUserKnowledgeFiles();
  notify(t("userTools.knowledge.file.deleted"), "success");
};

const createUserKnowledgeFile = async () => {
  const base = getActiveUserKnowledgeBase();
  if (!base || !base.name) {
    notify(t("knowledge.base.selectRequired"), "warn");
    return;
  }
  const filename = window.prompt(t("knowledge.file.newPrompt"), "example.md");
  if (!filename) {
    return;
  }
  const trimmed = filename.trim();
  if (!trimmed) {
    notify(t("userTools.knowledge.file.nameRequired"), "warn");
    return;
  }
  if (!trimmed.toLowerCase().endsWith(".md")) {
    notify(t("userTools.knowledge.file.mdOnly"), "warn");
    return;
  }
  state.userTools.knowledge.activeFile = trimmed;
  state.userTools.knowledge.fileContent = "";
  await saveUserKnowledgeFile();
  await selectUserKnowledgeFile(trimmed);
};

const normalizeUserKnowledgeUploadExtension = (filename) => {
  const parts = String(filename || "").trim().split(".");
  if (parts.length <= 1) {
    return "";
  }
  return `.${parts.pop().toLowerCase()}`;
};

const uploadUserKnowledgeFile = async (file) => {
  const base = getActiveUserKnowledgeBase();
  const userId = getUserId();
  if (!userId) {
    notify(t("common.userIdRequired"), "warn");
    return;
  }
  if (!base || !base.name) {
    notify(t("knowledge.base.selectRequired"), "warn");
    return;
  }
  if (!file) {
    return;
  }
  const extension = normalizeUserKnowledgeUploadExtension(file.name);
  if (!extension) {
    notify(t("knowledge.file.extensionMissing"), "warn");
    return;
  }
  if (!USER_KNOWLEDGE_UPLOAD_EXTENSIONS.includes(extension)) {
    notify(t("knowledge.file.unsupported", { extension }), "warn");
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/user_tools/knowledge/upload`;
  const formData = new FormData();
  formData.append("user_id", userId);
  formData.append("base", base.name);
  formData.append("file", file, file.name);
    const response = await fetch(endpoint, {
      method: "POST",
      body: formData,
    });
    if (!response.ok) {
      let detail = "";
      try {
        const errorPayload = await response.json();
        detail = errorPayload?.detail?.message || errorPayload?.message || "";
      } catch (error) {
        detail = "";
      }
      if (response.status === 404) {
        throw new Error(t("userTools.upload.endpointMissing"));
      }
      const detailText = detail ? `, ${detail}` : "";
      throw new Error(
        t("userTools.knowledge.uploadFailed", { message: `${response.status}${detailText}` })
      );
    }
  const result = await response.json();
  await loadUserKnowledgeFiles();
  if (result?.path) {
    await selectUserKnowledgeFile(result.path);
  }
  notify(t("knowledge.file.uploaded", { name: result?.path || file.name }), "success");
  const warnings = Array.isArray(result?.warnings) ? result.warnings : [];
  if (warnings.length) {
    notify(t("knowledge.file.warnings", { message: warnings.join(" | ") }), "warn");
  }
};

const applyUserKnowledgeModal = async () => {
  const payload = getUserKnowledgeModalPayload();
  const error = validateUserKnowledgeBase(payload, userKnowledgeEditingIndex);
  if (error) {
    notify(error, "warn");
    return;
  }
  const snapshot = {
    bases: state.userTools.knowledge.bases.map((base) => ({ ...base })),
    selectedIndex: state.userTools.knowledge.selectedIndex,
    files: [...state.userTools.knowledge.files],
    activeFile: state.userTools.knowledge.activeFile,
    fileContent: state.userTools.knowledge.fileContent,
  };
  if (userKnowledgeEditingIndex >= 0) {
    const current = state.userTools.knowledge.bases[userKnowledgeEditingIndex] || {};
    const nextRoot = current.name === payload.name ? current.root || "" : "";
    state.userTools.knowledge.bases[userKnowledgeEditingIndex] = {
      ...payload,
      root: nextRoot,
    };
    state.userTools.knowledge.selectedIndex = userKnowledgeEditingIndex;
  } else {
    state.userTools.knowledge.bases.push({ ...payload, root: "" });
    state.userTools.knowledge.selectedIndex = state.userTools.knowledge.bases.length - 1;
  }
  state.userTools.knowledge.files = [];
  state.userTools.knowledge.activeFile = "";
  state.userTools.knowledge.fileContent = "";
  renderUserKnowledgeBaseList();
  renderUserKnowledgeDetail();
  try {
    const saved = await saveUserKnowledgeConfig();
    if (!saved) {
      state.userTools.knowledge.bases = snapshot.bases;
      state.userTools.knowledge.selectedIndex = snapshot.selectedIndex;
      state.userTools.knowledge.files = snapshot.files;
      state.userTools.knowledge.activeFile = snapshot.activeFile;
      state.userTools.knowledge.fileContent = snapshot.fileContent;
      renderUserKnowledgeBaseList();
      renderUserKnowledgeDetail();
      return;
    }
    await loadUserKnowledgeFiles();
    notify(
      userKnowledgeEditingIndex >= 0 ? t("knowledge.base.updated") : t("knowledge.base.added"),
      "success"
    );
    closeUserKnowledgeModal();
  } catch (error) {
    state.userTools.knowledge.bases = snapshot.bases;
    state.userTools.knowledge.selectedIndex = snapshot.selectedIndex;
    state.userTools.knowledge.files = snapshot.files;
    state.userTools.knowledge.activeFile = snapshot.activeFile;
    state.userTools.knowledge.fileContent = snapshot.fileContent;
    renderUserKnowledgeBaseList();
    renderUserKnowledgeDetail();
    notify(t("knowledge.saveFailed", { message: error.message }), "error");
  }
};

const addUserKnowledgeBase = () => {
  openUserKnowledgeModal();
};

const editUserKnowledgeBase = () => {
  const base = getActiveUserKnowledgeBase();
  if (!base) {
    notify(t("knowledge.base.selectRequired"), "warn");
    return;
  }
  openUserKnowledgeModal(base, state.userTools.knowledge.selectedIndex);
};

const deleteUserKnowledgeBase = async () => {
  const base = getActiveUserKnowledgeBase();
  if (!base) {
    return;
  }
  const name = base.name || t("knowledge.name.unnamed");
  if (!window.confirm(t("knowledge.base.deleteConfirm", { name }))) {
    return;
  }
  const snapshot = {
    bases: state.userTools.knowledge.bases.map((item) => ({ ...item })),
    selectedIndex: state.userTools.knowledge.selectedIndex,
    files: [...state.userTools.knowledge.files],
    activeFile: state.userTools.knowledge.activeFile,
    fileContent: state.userTools.knowledge.fileContent,
  };
  state.userTools.knowledge.bases.splice(state.userTools.knowledge.selectedIndex, 1);
  if (!state.userTools.knowledge.bases.length) {
    state.userTools.knowledge.selectedIndex = -1;
  } else {
    state.userTools.knowledge.selectedIndex = Math.max(0, state.userTools.knowledge.selectedIndex - 1);
  }
  state.userTools.knowledge.files = [];
  state.userTools.knowledge.activeFile = "";
  state.userTools.knowledge.fileContent = "";
  renderUserKnowledgeBaseList();
  renderUserKnowledgeDetail();
  try {
    const saved = await saveUserKnowledgeConfig();
    if (!saved) {
      state.userTools.knowledge.bases = snapshot.bases;
      state.userTools.knowledge.selectedIndex = snapshot.selectedIndex;
      state.userTools.knowledge.files = snapshot.files;
      state.userTools.knowledge.activeFile = snapshot.activeFile;
      state.userTools.knowledge.fileContent = snapshot.fileContent;
      renderUserKnowledgeBaseList();
      renderUserKnowledgeDetail();
      return;
    }
    await loadUserKnowledgeFiles();
    notify(t("knowledge.base.deleted"), "success");
  } catch (error) {
    state.userTools.knowledge.bases = snapshot.bases;
    state.userTools.knowledge.selectedIndex = snapshot.selectedIndex;
    state.userTools.knowledge.files = snapshot.files;
    state.userTools.knowledge.activeFile = snapshot.activeFile;
    state.userTools.knowledge.fileContent = snapshot.fileContent;
    renderUserKnowledgeBaseList();
    renderUserKnowledgeDetail();
    notify(t("knowledge.deleteFailed", { message: error.message }), "error");
  }
};

// 附加提示词：输入即保存并触发提示词刷新
const saveExtraPrompt = async () => {
  const userId = getUserId();
  if (!userId) {
    updateExtraPromptStatus(t("common.userIdRequired"));
    return;
  }
  updateExtraPromptStatus(t("userTools.saving"));
  const payload = {
    user_id: userId,
    extra_prompt: state.userTools.extraPrompt || "",
  };
  try {
    const wunderBase = getWunderBase();
    const endpoint = `${wunderBase}/user_tools/extra_prompt`;
    const response = await fetch(endpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    if (!response.ok) {
      throw new Error(t("common.requestFailed", { status: response.status }));
    }
    updateExtraPromptStatus(t("userTools.autoSaved"));
    state.runtime.promptNeedsRefresh = true;
    schedulePromptReload();
  } catch (error) {
    updateExtraPromptStatus(t("userTools.saveFailed", { message: error.message }));
    notify(t("userTools.extraPrompt.saveFailed", { message: error.message }), "error");
  }
};

const scheduleExtraPromptSave = () => {
  if (extraPromptSaveTimer) {
    clearTimeout(extraPromptSaveTimer);
  }
  extraPromptSaveTimer = setTimeout(() => {
    extraPromptSaveTimer = null;
    saveExtraPrompt();
  }, SAVE_DEBOUNCE_MS);
};

// 初始化自建工具弹窗交互
export const initUserTools = () => {
  ensureUserToolsState();
  elements.promptUserToolAdd.addEventListener("click", openUserToolModal);
  elements.userToolModalClose.addEventListener("click", closeUserToolModal);
  elements.userToolModalCloseBtn.addEventListener("click", closeUserToolModal);
  elements.userToolModal.addEventListener("click", (event) => {
    if (event.target === elements.userToolModal) {
      closeUserToolModal();
    }
  });
  elements.userToolTabMcp.addEventListener("click", () => setActiveTab("mcp"));
  elements.userToolTabSkills.addEventListener("click", () => setActiveTab("skills"));
  elements.userToolTabKnowledge.addEventListener("click", () => setActiveTab("knowledge"));

  if (elements.userMcpAddBtn) {
    elements.userMcpAddBtn.addEventListener("click", () => {
      addUserMcpServer();
      openUserMcpModal(t("userTools.mcp.modal.addTitle"));
    });
  }
  elements.userMcpConnectBtn.addEventListener("click", connectUserMcpServer);
  elements.userMcpRefreshAllBtn.addEventListener("click", refreshAllUserMcpServers);
  elements.userMcpImportBtn.addEventListener("click", openUserMcpImportModal);
  elements.userMcpEditBtn.addEventListener("click", () => {
    if (!getActiveUserMcpServer()) {
      return;
    }
    openUserMcpModal(t("userTools.mcp.modal.editTitle"));
  });
  elements.userMcpEnableAllBtn.addEventListener("click", () => {
    const server = getActiveUserMcpServer();
    const tools = state.userTools.mcp.toolsByIndex[state.userTools.mcp.selectedIndex] || [];
    if (!server || !tools.length) {
      return;
    }
    server.enabled = true;
    server.allow_tools = tools.map((tool) => tool.name);
    scheduleUserMcpSave();
    renderUserMcpDetail();
  });
  elements.userMcpDisableAllBtn.addEventListener("click", () => {
    const server = getActiveUserMcpServer();
    if (!server) {
      return;
    }
    server.allow_tools = [];
    server.shared_tools = [];
    server.enabled = false;
    scheduleUserMcpSave();
    renderUserMcpDetail();
  });
  elements.userMcpDeleteBtn.addEventListener("click", deleteUserMcpServer);
  bindUserMcpInputs();
  elements.userMcpModalSave.addEventListener("click", () => {
    applyUserMcpModal().catch((error) => notify(error.message, "error"));
  });
  elements.userMcpModalCancel.addEventListener("click", closeUserMcpModal);
  elements.userMcpModalClose.addEventListener("click", closeUserMcpModal);
  elements.userMcpModal.addEventListener("click", (event) => {
    if (event.target === elements.userMcpModal) {
      closeUserMcpModal();
    }
  });
  elements.userMcpImportConfirm.addEventListener("click", applyUserMcpImportModal);
  elements.userMcpImportCancel.addEventListener("click", closeUserMcpImportModal);
  elements.userMcpImportClose.addEventListener("click", closeUserMcpImportModal);
  elements.userMcpImportModal.addEventListener("click", (event) => {
    if (event.target === elements.userMcpImportModal) {
      closeUserMcpImportModal();
    }
  });

  elements.userSkillUploadBtn.addEventListener("click", () => {
    elements.userSkillUploadInput.value = "";
    elements.userSkillUploadInput.click();
  });
  elements.userSkillUploadInput.addEventListener("change", async () => {
    const file = elements.userSkillUploadInput.files?.[0];
    if (!file) {
      return;
    }
    try {
      await uploadUserSkillZip(file);
      notify(t("userTools.skills.upload.success"), "success");
    } catch (error) {
      notify(t("userTools.skills.upload.failed", { message: error.message }), "error");
    }
  });
  elements.userSkillRefreshBtn.addEventListener("click", async () => {
    try {
      await loadUserSkills();
      notify(t("userTools.skills.refresh.success"), "success");
    } catch (error) {
      elements.userSkillsList.textContent = t("common.loadFailedWithMessage", {
        message: error.message,
      });
      notify(t("userTools.skills.refresh.failed", { message: error.message }), "error");
    }
  });

  elements.userKnowledgeAddBtn.addEventListener("click", addUserKnowledgeBase);
  elements.userKnowledgeEditBtn?.addEventListener("click", editUserKnowledgeBase);
  elements.userKnowledgeRefreshBtn.addEventListener("click", async () => {
    try {
      await loadUserKnowledgeConfig();
      notify(t("userTools.knowledge.refresh.success"), "success");
    } catch (error) {
      elements.userKnowledgeBaseList.textContent = t("common.loadFailedWithMessage", {
        message: error.message,
      });
      notify(t("userTools.knowledge.refresh.failed", { message: error.message }), "error");
    }
  });
  elements.userKnowledgeDeleteBtn.addEventListener("click", () => {
    deleteUserKnowledgeBase().catch((error) =>
      notify(t("knowledge.deleteFailed", { message: error.message }), "error")
    );
  });
  elements.userKnowledgeModalSave?.addEventListener("click", () => {
    applyUserKnowledgeModal();
  });
  elements.userKnowledgeModalCancel?.addEventListener("click", closeUserKnowledgeModal);
  elements.userKnowledgeModalClose?.addEventListener("click", closeUserKnowledgeModal);
  elements.userKnowledgeModal?.addEventListener("click", (event) => {
    if (event.target === elements.userKnowledgeModal) {
      closeUserKnowledgeModal();
    }
  });
  if (elements.userKnowledgeFileUploadInput) {
    elements.userKnowledgeFileUploadInput.accept = USER_KNOWLEDGE_UPLOAD_ACCEPT;
  }
  elements.userKnowledgeFileNewBtn.addEventListener("click", () => {
    createUserKnowledgeFile().catch((error) => notify(error.message, "error"));
  });
  elements.userKnowledgeFileSaveBtn.addEventListener("click", () => {
    saveUserKnowledgeFile().catch((error) => notify(error.message, "error"));
  });
  elements.userKnowledgeFileUploadBtn.addEventListener("click", () => {
    const base = getActiveUserKnowledgeBase();
    if (!base || !base.name) {
      notify(t("knowledge.base.selectRequired"), "warn");
      return;
    }
    elements.userKnowledgeFileUploadInput.value = "";
    elements.userKnowledgeFileUploadInput.click();
  });
  elements.userKnowledgeFileUploadInput.addEventListener("change", async () => {
    const file = elements.userKnowledgeFileUploadInput.files?.[0];
    if (!file) {
      return;
    }
    try {
      await uploadUserKnowledgeFile(file);
      notify(t("userTools.knowledge.file.uploaded"), "success");
    } catch (error) {
      notify(t("userTools.knowledge.file.uploadFailed", { message: error.message }), "error");
    }
  });
  elements.userKnowledgeFileContent?.addEventListener("input", () => {
    state.userTools.knowledge.fileContent = elements.userKnowledgeFileContent.value;
    updateUserKnowledgeEditorHighlight();
  });
  elements.userKnowledgeFileContent?.addEventListener("scroll", syncUserKnowledgeEditorScroll);
  window.addEventListener("resize", updateUserKnowledgeEditorHighlight);

  elements.promptExtraPrompt.addEventListener("input", () => {
    state.userTools.extraPrompt = elements.promptExtraPrompt.value;
    scheduleExtraPromptSave();
  });

  ensureToolSelectionLoaded().catch(() => {});
};




