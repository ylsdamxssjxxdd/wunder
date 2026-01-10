import { elements } from "./elements.js?v=20260110-04";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { isPlainObject, parseHeadersValue, getToolInputSchema } from "./utils.js?v=20251229-02";
import { syncPromptTools } from "./tools.js?v=20251227-13";
import { openToolDetailModal } from "./tool-detail.js";
import { notify } from "./notify.js";
import { appendLog } from "./log.js?v=20260108-02";
import { t } from "./i18n.js?v=20260110-03";

// 规范化 MCP 服务字段，兼容后端与导入结构
const normalizeMcpServer = (server) => {
  const headers = isPlainObject(server.headers) ? server.headers : {};
  const rawToolSpecs = Array.isArray(server.tool_specs)
    ? server.tool_specs
    : Array.isArray(server.toolSpecs)
    ? server.toolSpecs
    : [];
  return {
    name: server.name || "",
    display_name: server.display_name || server.displayName || "",
    endpoint: server.endpoint || server.baseUrl || server.base_url || server.url || "",
    transport: server.transport || server.type || "",
    description: server.description || "",
    headers,
    auth: server.auth || "",
    allow_tools: Array.isArray(server.allow_tools) ? server.allow_tools : [],
    enabled: server.enabled !== false,
    headers_error: server.headers_error || "",
    tool_specs: rawToolSpecs,
  };
};

// 将 mcpServers 结构体转换为内部服务对象
const buildServerFromMcpConfig = (serverId, rawConfig) => {
  const config = rawConfig || {};
  const endpoint = config.baseUrl || config.base_url || config.url || config.endpoint || "";
  const name = (serverId || config.id || config.name || "").trim();
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
  return normalizeMcpServer({
    name,
    display_name: displayName,
    endpoint,
    transport: config.type || config.transport || "",
    description: config.description || "",
    headers,
    auth: config.auth || "",
    allow_tools: config.allow_tools || config.allowTools || [],
    enabled: config.isActive ?? config.enabled ?? true,
  });
};

// 生成可复制的 MCP 结构体预览，方便管理员复用配置
const buildMcpStructPreview = (server) => {
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

// 从编辑弹窗字段中构造预览用的服务对象
const collectModalServer = () => {
  const headersResult = parseHeadersValue(elements.mcpHeaders.value);
  const headers = headersResult.error ? {} : headersResult.headers || {};
  return normalizeMcpServer({
    name: elements.mcpName.value.trim(),
    display_name: elements.mcpDisplayName.value.trim(),
    endpoint: elements.mcpEndpoint.value.trim(),
    transport: elements.mcpTransport.value.trim(),
    description: elements.mcpDescription.value.trim(),
    headers,
    enabled: elements.mcpEnabled.checked,
  });
};

// 更新 MCP 结构体预览内容
const updateMcpStructPreview = () => {
  const server = collectModalServer();
  const preview = buildMcpStructPreview(server);
  elements.mcpStructPreview.value = preview || t("mcp.struct.preview.empty");
};

// 判断指定服务是否已有工具缓存，用于区分“连接/刷新”状态
const isMcpServerConnected = (index) => {
  const tools = state.mcp.toolsByIndex[index];
  return Array.isArray(tools) && tools.length > 0;
};

// 根据当前选中服务更新连接按钮文案与图标
const updateMcpConnectButton = () => {
  const server = state.mcp.servers[state.mcp.selectedIndex];
  const connected = server ? isMcpServerConnected(state.mcp.selectedIndex) : false;
  const iconClass = connected ? "fa-solid fa-arrows-rotate" : "fa-solid fa-link";
  const label = connected ? t("mcp.connect.refresh") : t("mcp.connect.connect");
  elements.mcpConnectBtn.innerHTML = `<i class="${iconClass}"></i>${label}`;
  elements.mcpConnectBtn.disabled = !server;
};

// 根据工具缓存状态启用/禁用“全部刷新”按钮
const updateMcpRefreshAllButton = () => {
  const hasConnected = state.mcp.servers.some((_, index) => isMcpServerConnected(index));
  elements.mcpRefreshAllBtn.disabled = !hasConnected;
};

const resolveMcpServerActionMessage = () => {
  const action = state.mcp.lastAction;
  if (!action || !action.name) {
    return "";
  }
  const name = action.name;
  switch (action.type) {
    case "enabled":
      return t("mcp.server.enabled", { name });
    case "disabled":
      return t("mcp.server.disabled", { name });
    default:
      return "";
  }
};

// 渲染 MCP 服务列表与当前选中项
const renderMcpServers = () => {
  elements.mcpServerList.textContent = "";
  if (!state.mcp.servers.length) {
    elements.mcpServerList.textContent = t("mcp.list.empty");
    updateMcpConnectButton();
    updateMcpRefreshAllButton();
    return;
  }
  state.mcp.servers.forEach((server, index) => {
    const item = document.createElement("button");
    item.type = "button";
    item.className = "list-item";
    if (index === state.mcp.selectedIndex) {
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
      state.mcp.selectedIndex = index;
      renderMcpDetail();
      renderMcpServers();
    });
    elements.mcpServerList.appendChild(item);
  });
  updateMcpConnectButton();
  updateMcpRefreshAllButton();
};

const renderMcpHeader = () => {
  const server = state.mcp.servers[state.mcp.selectedIndex];
  if (!server) {
    elements.mcpDetailTitle.textContent = t("mcp.detail.none");
    elements.mcpDetailMeta.textContent = "";
    elements.mcpDetailDesc.textContent = "";
    elements.mcpEditBtn.disabled = true;
    elements.mcpDeleteBtn.disabled = true;
    updateMcpConnectButton();
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
  elements.mcpDetailTitle.textContent = title;
  elements.mcpDetailMeta.textContent = metaParts.join(" · ");
  elements.mcpDetailDesc.textContent = server.description || "";
  elements.mcpEditBtn.disabled = false;
  elements.mcpDeleteBtn.disabled = false;
  updateMcpConnectButton();
};

// 渲染 MCP 工具勾选列表
const renderMcpTools = () => {
  elements.mcpToolList.textContent = "";
  const server = state.mcp.servers[state.mcp.selectedIndex];
  if (!server) {
    elements.mcpToolList.textContent = t("mcp.tools.select");
    renderMcpHeader();
    return;
  }
  const tools = state.mcp.toolsByIndex[state.mcp.selectedIndex];
  if (!tools || !tools.length) {
    elements.mcpToolList.textContent = t("mcp.tools.notLoaded");
    renderMcpHeader();
    return;
  }
  tools.forEach((tool) => {
    const item = document.createElement("div");
    item.className = "tool-item";
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    const allowList = Array.isArray(server.allow_tools) ? server.allow_tools : [];
    // 启用且未显式配置 allow_tools 时，默认视为全选
    const implicitAll = server.enabled !== false && allowList.length === 0;
    const checked = implicitAll || allowList.includes(tool.name);
    checkbox.checked = checked;
    checkbox.addEventListener("change", (event) => {
      const allTools = tools.map((t) => t.name);
      const currentAllow = Array.isArray(server.allow_tools) ? server.allow_tools : [];
      const currentImplicitAll = server.enabled !== false && currentAllow.length === 0;
      let nextAllow = currentAllow.slice();
      if (currentImplicitAll) {
        nextAllow = allTools.slice();
      }
      if (event.target.checked) {
        if (!nextAllow.includes(tool.name)) {
          nextAllow.push(tool.name);
        }
        if (server.enabled === false) {
          server.enabled = true;
          renderMcpHeader();
        }
      } else {
        nextAllow = nextAllow.filter((name) => name !== tool.name);
      }
      server.allow_tools = nextAllow;
      if (server.enabled !== false && nextAllow.length === 0) {
        server.enabled = false;
        renderMcpHeader();
      }
      const serverTitle = server.display_name || server.name || t("mcp.server.unnamed");
      const actionMessage = event.target.checked
        ? t("mcp.tool.enabled", { name: tool.name, server: serverTitle })
        : t("mcp.tool.disabled", { name: tool.name, server: serverTitle });
      // 勾选状态变更后立即保存，避免依赖手动保存按钮
      saveMcpServers({ refreshUI: false })
        .then((ok) => {
          if (ok === false) {
            return;
          }
          appendLog(actionMessage);
          notify(actionMessage, "success");
        })
        .catch((error) => {
          console.error(t("mcp.saveFailed", { message: error.message }), error);
          notify(t("mcp.saveFailed", { message: error.message }), "error");
        });
    });
    const label = document.createElement("label");
    label.innerHTML = `<strong>${tool.name}</strong><span class="muted">${tool.description || ""}</span>`;
    // 点击工具条目查看详情，避免与勾选动作冲突
    item.addEventListener("click", (event) => {
      if (event.target === checkbox) {
        return;
      }
      const serverTitle = server.display_name || server.name || t("mcp.server.unnamed");
      const metaParts = [
        t("mcp.tool.label"),
        t("mcp.tool.server", { name: serverTitle }),
      ];
      metaParts.push(
        server.enabled !== false ? t("mcp.tool.serverEnabled") : t("mcp.tool.serverDisabled")
      );
      metaParts.push(
        checkbox.checked ? t("mcp.tool.selected") : t("mcp.tool.unselected")
      );
      openToolDetailModal({
        title: tool.name || t("tool.detail.title"),
        meta: metaParts.join(" · "),
        description: tool.description || "",
        schema: getToolInputSchema(tool),
      });
    });
    item.appendChild(checkbox);
    item.appendChild(label);
    elements.mcpToolList.appendChild(item);
  });
  renderMcpHeader();
};

// 渲染 MCP 服务详情与工具列表
const renderMcpDetail = () => {
  const server = state.mcp.servers[state.mcp.selectedIndex];
  if (!server) {
    elements.mcpToolList.textContent = t("mcp.tools.select");
    renderMcpHeader();
    return;
  }
  renderMcpTools();
};

// 从后端加载 MCP 配置
export const loadMcpServers = async () => {
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/mcp`;
  const response = await fetch(endpoint);
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const result = await response.json();
  state.mcp.servers = Array.isArray(result.servers) ? result.servers.map(normalizeMcpServer) : [];
  state.mcp.toolsByIndex = state.mcp.servers.map((server) =>
    Array.isArray(server.tool_specs) && server.tool_specs.length ? server.tool_specs : null
  );
  state.mcp.selectedIndex = state.mcp.servers.length ? 0 : -1;
  renderMcpServers();
  renderMcpDetail();
};

// 保存 MCP 配置到后端，避免频繁覆盖当前选择的服务
const saveMcpServers = async (options = {}) => {
  const { refreshUI = true } = options;
  const selectedName = state.mcp.servers[state.mcp.selectedIndex]?.name || "";
  const saveVersion = ++state.mcp.saveVersion;
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/mcp`;
  const payloadServers = state.mcp.servers.map((server) => ({
    name: server.name,
    display_name: server.display_name,
    endpoint: server.endpoint,
    transport: server.transport || null,
    description: server.description,
    headers: server.headers || {},
    auth: server.auth || null,
    tool_specs: Array.isArray(server.tool_specs) ? server.tool_specs : [],
    allow_tools: Array.isArray(server.allow_tools) ? server.allow_tools : [],
    enabled: server.enabled !== false,
  }));
  const response = await fetch(endpoint, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ servers: payloadServers }),
  });
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const result = await response.json();
  if (saveVersion !== state.mcp.saveVersion) {
    return false;
  }
  syncPromptTools();
  if (!refreshUI) {
    return true;
  }
  state.mcp.servers = Array.isArray(result.servers) ? result.servers.map(normalizeMcpServer) : [];
  state.mcp.toolsByIndex = state.mcp.servers.map((server) =>
    Array.isArray(server.tool_specs) && server.tool_specs.length ? server.tool_specs : null
  );
  if (selectedName) {
    const nextIndex = state.mcp.servers.findIndex((server) => server.name === selectedName);
    if (nextIndex >= 0) {
      state.mcp.selectedIndex = nextIndex;
    } else {
      state.mcp.selectedIndex = state.mcp.servers.length ? 0 : -1;
    }
  } else {
    state.mcp.selectedIndex = state.mcp.servers.length ? 0 : -1;
  }
  if (refreshUI) {
    renderMcpServers();
    renderMcpDetail();
  }
  return true;
};

// 调用后端接口拉取指定 MCP 服务的工具清单
const requestMcpTools = async (server) => {
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/mcp/tools`;
  const response = await fetch(endpoint, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      name: server.name,
      endpoint: server.endpoint,
      transport: server.transport || null,
      headers: server.headers || {},
      auth: server.auth || null,
    }),
  });
  if (!response.ok) {
    return { ok: false, status: response.status };
  }
  const result = await response.json();
  return { ok: true, tools: result.tools || [] };
};

// 按索引连接/刷新单个 MCP 服务，并根据需要同步 UI
const connectMcpServerAtIndex = async (index, options = {}) => {
  const { updateUI = false } = options;
  const server = state.mcp.servers[index];
  if (!server || !server.name || !server.endpoint) {
    if (updateUI) {
      elements.mcpToolList.textContent = t("mcp.form.required");
    }
    return false;
  }
  if (updateUI) {
    elements.mcpToolList.textContent = isMcpServerConnected(index)
      ? t("mcp.connect.refreshing")
      : t("mcp.connect.connecting");
  }
  const result = await requestMcpTools(server);
  if (!result.ok) {
    if (updateUI) {
      elements.mcpToolList.textContent = t("common.requestFailed", { status: result.status });
    }
    return false;
  }
  const tools = result.tools || [];
  state.mcp.toolsByIndex[index] = tools;
  server.tool_specs = tools;
  updateMcpRefreshAllButton();
  if (updateUI) {
    renderMcpTools();
  }
  return true;
};

// 连接 MCP 服务并拉取工具列表
const connectMcpServer = async () => {
  const index = state.mcp.selectedIndex;
  const wasConnected = index >= 0 ? isMcpServerConnected(index) : false;
  const ok = await connectMcpServerAtIndex(index, { updateUI: true });
  if (!ok) {
    notify(t("mcp.connect.failed"), "error");
    return;
  }
  notify(
    wasConnected ? t("mcp.connect.refreshed") : t("mcp.connect.connected"),
    "success"
  );
  saveMcpServers({ refreshUI: false }).catch((error) => {
    console.error(t("mcp.cache.saveFailed", { message: error.message }), error);
    notify(t("mcp.cache.saveFailed", { message: error.message }), "error");
  });
};

// 刷新所有已连接 MCP 服务的工具缓存
const refreshAllMcpServers = async () => {
  const connectedIndexes = state.mcp.servers
    .map((_, index) => index)
    .filter((index) => isMcpServerConnected(index));
  if (!connectedIndexes.length) {
    return;
  }
  const selectedIndex = state.mcp.selectedIndex;
  let updated = false;
  for (const index of connectedIndexes) {
    const ok = await connectMcpServerAtIndex(index, { updateUI: index === selectedIndex });
    if (ok) {
      updated = true;
    }
  }
  if (!updated) {
    notify(t("mcp.refresh.failed"), "error");
    return;
  }
  notify(t("mcp.refresh.allSuccess"), "success");
  saveMcpServers({ refreshUI: false }).catch((error) => {
    console.error(t("mcp.cache.saveFailed", { message: error.message }), error);
    notify(t("mcp.cache.saveFailed", { message: error.message }), "error");
  });
};

const openMcpModal = (index) => {
  state.mcpModal.index = index;
  state.mcp.lastAction = null;
  const server = index !== null ? state.mcp.servers[index] : null;
  elements.mcpModalTitle.textContent = t("mcp.modal.editTitle");
  elements.mcpName.value = server?.name || "";
  elements.mcpDisplayName.value = server?.display_name || "";
  elements.mcpEndpoint.value = server?.endpoint || "";
  elements.mcpTransport.value = server?.transport || "";
  elements.mcpDescription.value = server?.description || "";
  elements.mcpHeaders.value =
    server?.headers && Object.keys(server.headers).length
      ? JSON.stringify(server.headers, null, 2)
      : "";
  elements.mcpHeadersError.textContent = "";
  elements.mcpEnabled.checked = server ? server.enabled !== false : true;
  elements.mcpModal.classList.add("active");
  updateMcpStructPreview();
};

const closeMcpModal = () => {
  elements.mcpModal.classList.remove("active");
};

const openMcpImportModal = () => {
  elements.mcpImportInput.value = "";
  elements.mcpImportModal.classList.add("active");
};

const closeMcpImportModal = () => {
  elements.mcpImportModal.classList.remove("active");
};

const applyMcpImportModal = async () => {
  const raw = elements.mcpImportInput.value;
  const ok = await importMcpServers(raw);
  if (ok) {
    closeMcpImportModal();
  }
};

const applyMcpModal = () => {
  const name = elements.mcpName.value.trim();
  const endpoint = elements.mcpEndpoint.value.trim();
  if (!name || !endpoint) {
    notify(t("mcp.form.required"), "warn");
    state.mcp.lastAction = null;
    return false;
  }
  const headersResult = parseHeadersValue(elements.mcpHeaders.value);
  if (headersResult.error) {
    elements.mcpHeadersError.textContent = headersResult.error;
    state.mcp.lastAction = null;
    return false;
  }
  const previousAuth =
    state.mcpModal.index !== null ? state.mcp.servers[state.mcpModal.index]?.auth : null;
  const server = normalizeMcpServer({
    name,
    display_name: elements.mcpDisplayName.value.trim(),
    endpoint,
    transport: elements.mcpTransport.value.trim(),
    description: elements.mcpDescription.value.trim(),
    headers: headersResult.headers || {},
    auth: previousAuth,
    allow_tools:
      state.mcpModal.index !== null ? state.mcp.servers[state.mcpModal.index]?.allow_tools : [],
    tool_specs:
      state.mcpModal.index !== null ? state.mcp.servers[state.mcpModal.index]?.tool_specs : [],
    enabled: elements.mcpEnabled.checked,
  });
  if (state.mcpModal.index === null) {
    state.mcp.lastAction = null;
    return false;
  }
  const index = state.mcpModal.index;
  const previous = state.mcp.servers[index];
  const previousEnabled = previous ? previous.enabled !== false : true;
  const nextEnabled = server.enabled !== false;
  if (previous && previousEnabled !== nextEnabled) {
    const displayName =
      server.display_name || server.name || previous.display_name || previous.name;
    state.mcp.lastAction = {
      type: nextEnabled ? "enabled" : "disabled",
      name: displayName || t("mcp.server.defaultName"),
    };
  } else {
    state.mcp.lastAction = null;
  }
  state.mcp.servers[index] = { ...previous, ...server };
  state.mcp.selectedIndex = index;
  renderMcpServers();
  renderMcpDetail();
  return true;
};

// 按服务名覆盖或新增 MCP 服务
const upsertMcpServer = (incoming) => {
  const targetIndex = state.mcp.servers.findIndex((item) => item.name === incoming.name);
  if (targetIndex >= 0) {
    const previous = state.mcp.servers[targetIndex];
    const allowTools =
      Array.isArray(incoming.allow_tools) && incoming.allow_tools.length
        ? incoming.allow_tools
        : previous.allow_tools;
    const toolSpecs =
      Array.isArray(incoming.tool_specs) && incoming.tool_specs.length
        ? incoming.tool_specs
        : previous.tool_specs;
    state.mcp.servers[targetIndex] = {
      ...previous,
      ...incoming,
      allow_tools: allowTools,
      tool_specs: toolSpecs,
    };
    return targetIndex;
  }
  state.mcp.servers.push(incoming);
  state.mcp.toolsByIndex.push(null);
  return state.mcp.servers.length - 1;
};

// 从 MCP 结构体 JSON 导入服务配置
const importMcpServers = async (raw) => {
  const content = (raw || "").trim();
  if (!content) {
    return false;
  }
  let parsed = null;
  try {
    parsed = JSON.parse(content);
  } catch (error) {
    notify(t("mcp.import.parseFailed"), "error");
    return false;
  }
  const imported = [];
  if (parsed.mcpServers && isPlainObject(parsed.mcpServers)) {
    Object.entries(parsed.mcpServers).forEach(([serverId, config]) => {
      const server = buildServerFromMcpConfig(serverId, config);
      if (server) {
        imported.push(server);
      }
    });
  } else {
    const serverId = parsed.id || parsed.name || "";
    const server = buildServerFromMcpConfig(serverId, parsed);
    if (server) {
      imported.push(server);
    }
  }
  if (!imported.length) {
    notify(t("mcp.import.noValid"), "warn");
    return false;
  }
  let lastIndex = state.mcp.selectedIndex;
  imported.forEach((server) => {
    lastIndex = upsertMcpServer(server);
  });
  state.mcp.selectedIndex = lastIndex;
  renderMcpServers();
  renderMcpDetail();
  try {
    await saveMcpServers();
    notify(t("mcp.import.success"), "success");
  } catch (error) {
    console.error(t("mcp.saveFailed", { message: error.message }), error);
    notify(t("mcp.saveFailed", { message: error.message }), "error");
    return false;
  }
  return true;
};

// 删除当前选中的 MCP 服务
const deleteMcpServer = async () => {
  if (state.mcp.selectedIndex < 0) {
    return;
  }
  const removed = state.mcp.servers[state.mcp.selectedIndex];
  const removedName = removed?.display_name || removed?.name || t("mcp.server.defaultName");
  state.mcp.servers.splice(state.mcp.selectedIndex, 1);
  state.mcp.toolsByIndex.splice(state.mcp.selectedIndex, 1);
  if (!state.mcp.servers.length) {
    state.mcp.selectedIndex = -1;
  } else {
    state.mcp.selectedIndex = Math.max(0, state.mcp.selectedIndex - 1);
  }
  renderMcpServers();
  renderMcpDetail();
  try {
    await saveMcpServers();
    notify(t("mcp.delete.success", { name: removedName }), "success");
  } catch (error) {
    console.error(t("mcp.saveFailed", { message: error.message }), error);
    notify(t("mcp.saveFailed", { message: error.message }), "error");
  }
};

// 初始化 MCP 管理面板交互
export const initMcpPanel = () => {
  elements.mcpConnectBtn.addEventListener("click", connectMcpServer);
  elements.mcpRefreshAllBtn.addEventListener("click", refreshAllMcpServers);
  elements.mcpImportBtn.addEventListener("click", openMcpImportModal);
  elements.mcpEditBtn.addEventListener("click", () => {
    if (state.mcp.selectedIndex < 0) {
      return;
    }
    openMcpModal(state.mcp.selectedIndex);
  });
  elements.mcpDeleteBtn.addEventListener("click", deleteMcpServer);
  elements.mcpEnableAllBtn.addEventListener("click", async () => {
    const server = state.mcp.servers[state.mcp.selectedIndex];
    const tools = state.mcp.toolsByIndex[state.mcp.selectedIndex] || [];
    if (!server || !tools.length) {
      return;
    }
    server.enabled = true;
    server.allow_tools = tools.map((tool) => tool.name);
    renderMcpDetail();
    try {
      const ok = await saveMcpServers({ refreshUI: false });
      if (ok === false) {
        return;
      }
      const message = t("mcp.tools.enableAllSuccess");
      appendLog(message);
      notify(message, "success");
    } catch (error) {
      console.error(t("mcp.saveFailed", { message: error.message }), error);
      notify(t("mcp.saveFailed", { message: error.message }), "error");
    }
  });
  elements.mcpDisableAllBtn.addEventListener("click", async () => {
    const server = state.mcp.servers[state.mcp.selectedIndex];
    if (!server) {
      return;
    }
    server.allow_tools = [];
    server.enabled = false;
    renderMcpDetail();
    try {
      const ok = await saveMcpServers({ refreshUI: false });
      if (ok === false) {
        return;
      }
      const message = t("mcp.tools.disableAllSuccess");
      appendLog(message);
      notify(message, "success");
    } catch (error) {
      console.error(t("mcp.saveFailed", { message: error.message }), error);
      notify(t("mcp.saveFailed", { message: error.message }), "error");
    }
  });
  elements.mcpModalSave.addEventListener("click", async () => {
    const ok = applyMcpModal();
    if (!ok) {
      return;
    }
    try {
      const saved = await saveMcpServers();
      if (saved === false) {
        return;
      }
      const actionMessage = resolveMcpServerActionMessage();
      const message = actionMessage || t("mcp.save.success");
      appendLog(message);
      notify(message, "success");
      state.mcp.lastAction = null;
    } catch (error) {
      console.error(t("mcp.saveFailed", { message: error.message }), error);
      notify(t("mcp.saveFailed", { message: error.message }), "error");
    }
  });
  elements.mcpModalCancel.addEventListener("click", closeMcpModal);
  elements.mcpModalClose.addEventListener("click", closeMcpModal);
  elements.mcpModal.addEventListener("click", (event) => {
    if (event.target === elements.mcpModal) {
      closeMcpModal();
    }
  });
  elements.mcpHeaders.addEventListener("input", () => {
    elements.mcpHeadersError.textContent = "";
    updateMcpStructPreview();
  });
  elements.mcpName.addEventListener("input", updateMcpStructPreview);
  elements.mcpDisplayName.addEventListener("input", updateMcpStructPreview);
  elements.mcpEndpoint.addEventListener("input", updateMcpStructPreview);
  elements.mcpDescription.addEventListener("input", updateMcpStructPreview);
  elements.mcpTransport.addEventListener("change", updateMcpStructPreview);
  elements.mcpEnabled.addEventListener("change", updateMcpStructPreview);
  elements.mcpImportConfirm.addEventListener("click", applyMcpImportModal);
  elements.mcpImportCancel.addEventListener("click", closeMcpImportModal);
  elements.mcpImportClose.addEventListener("click", closeMcpImportModal);
  elements.mcpImportModal.addEventListener("click", (event) => {
    if (event.target === elements.mcpImportModal) {
      closeMcpImportModal();
    }
  });
};




