import { elements } from "./elements.js?v=20260115-02";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { isPlainObject, parseHeadersValue, getToolInputSchema } from "./utils.js?v=20251229-02";
import { syncPromptTools } from "./tools.js?v=20251227-13";
import { notify } from "./notify.js";
import { appendLog } from "./log.js?v=20260108-02";
import { t } from "./i18n.js?v=20260115-03";
import { openToolDetailModal, setToolDetailTestMode } from "./tool-detail.js?v=20260115-05";

let mcpTestFields = [];
let mcpTestActiveKey = "";

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

const setMcpTestStatus = (message, status) => {
  if (!elements.mcpTestStatus) {
    return;
  }
  elements.mcpTestStatus.textContent = message || "";
  elements.mcpTestStatus.classList.remove("is-success", "is-error", "is-warning");
  if (status === "success") {
    elements.mcpTestStatus.classList.add("is-success");
  } else if (status === "error") {
    elements.mcpTestStatus.classList.add("is-error");
  } else if (status === "warning") {
    elements.mcpTestStatus.classList.add("is-warning");
  }
};

const setMcpTestResult = (value) => {
  if (!elements.mcpTestResult) {
    return;
  }
  if (value === null || value === undefined || value === "") {
    elements.mcpTestResult.textContent = "";
    return;
  }
  if (typeof value === "string") {
    elements.mcpTestResult.textContent = value;
    return;
  }
  try {
    elements.mcpTestResult.textContent = JSON.stringify(value, null, 2);
  } catch (error) {
    elements.mcpTestResult.textContent = String(value);
  }
};

const resetMcpTestPanel = (options = {}) => {
  const { clearSelection = false, keepTool = false } = options;
  if (clearSelection) {
    state.mcp.selectedTool = null;
  }
  mcpTestActiveKey = "";
  mcpTestFields = [];
  setToolDetailTestMode(false);
  if (!keepTool) {
    if (elements.mcpTestToolTitle) {
      elements.mcpTestToolTitle.textContent = t("mcp.test.empty");
    }
    if (elements.mcpTestToolMeta) {
      elements.mcpTestToolMeta.textContent = "";
    }
    if (elements.mcpTestToolDesc) {
      elements.mcpTestToolDesc.textContent = "";
    }
  }
  if (elements.mcpTestForm) {
    elements.mcpTestForm.textContent = "";
  }
  if (elements.mcpTestRunBtn) {
    elements.mcpTestRunBtn.disabled = true;
  }
  setMcpTestStatus("");
  setMcpTestResult("");
};

const clearMcpTestInputs = () => {
  mcpTestFields.forEach((field) => {
    const { input, defaultValue, type, mode } = field;
    if (!input) {
      return;
    }
    if (input.tagName === "SELECT") {
      if (defaultValue !== undefined) {
        input.value = JSON.stringify(defaultValue);
      } else if (input.options.length) {
        input.selectedIndex = 0;
      }
      return;
    }
    if (type === "boolean") {
      input.value = JSON.stringify(Boolean(defaultValue));
      return;
    }
    if (defaultValue === undefined) {
      input.value = "";
      return;
    }
    if (mode === "json") {
      try {
        input.value = JSON.stringify(defaultValue, null, 2);
      } catch (error) {
        input.value = String(defaultValue);
      }
    } else {
      input.value = String(defaultValue);
    }
  });
};

const normalizeTestSchema = (schema) => {
  if (typeof schema === "string") {
    try {
      const parsed = JSON.parse(schema);
      schema = parsed;
    } catch (error) {
      return null;
    }
  }
  if (!isPlainObject(schema)) {
    return null;
  }
  if (isPlainObject(schema.properties)) {
    return schema;
  }
  return null;
};

const normalizePropertySchema = (property) => {
  if (!isPlainObject(property)) {
    return {};
  }
  if (Array.isArray(property.anyOf) && property.anyOf.length) {
    const enumItem = property.anyOf.find((item) => Array.isArray(item?.enum));
    if (enumItem) {
      return { ...property, ...enumItem };
    }
    const typed = property.anyOf.find((item) => item && item.type);
    if (typed) {
      return { ...property, ...typed };
    }
  }
  if (Array.isArray(property.oneOf) && property.oneOf.length) {
    const enumItem = property.oneOf.find((item) => Array.isArray(item?.enum));
    if (enumItem) {
      return { ...property, ...enumItem };
    }
    const typed = property.oneOf.find((item) => item && item.type);
    if (typed) {
      return { ...property, ...typed };
    }
  }
  return property;
};

const resolveSchemaType = (property) => {
  const rawType = property?.type;
  if (Array.isArray(rawType)) {
    return rawType.find((item) => item && item !== "null") || rawType[0];
  }
  if (rawType) {
    return rawType;
  }
  return "string";
};

const buildMcpTestField = (name, property, required, index) => {
  const schema = normalizePropertySchema(property);
  const type = resolveSchemaType(schema);
  const row = document.createElement("div");
  row.className = "form-row mcp-test-field";
  const label = document.createElement("label");
  const fieldId = `mcpTestParam_${index}`;
  label.htmlFor = fieldId;
  label.textContent = name;
  if (required) {
    const mark = document.createElement("span");
    mark.className = "mcp-test-required";
    mark.textContent = "*";
    label.appendChild(mark);
  }
  row.appendChild(label);
  if (schema.description) {
    const desc = document.createElement("div");
    desc.className = "mcp-test-hint";
    desc.textContent = schema.description;
    row.appendChild(desc);
  }

  const isEnum = Array.isArray(schema.enum);
  const input = isEnum || type === "boolean" ? document.createElement("select") : null;
  let control = input;
  let mode = "text";
  if (input) {
    const values = isEnum ? schema.enum : [true, false];
    if (!required) {
      const placeholder = document.createElement("option");
      placeholder.value = "";
      placeholder.textContent = t("settings.placeholder.optional");
      input.appendChild(placeholder);
    }
    values.forEach((value) => {
      const option = document.createElement("option");
      option.value = JSON.stringify(value);
      option.textContent = String(value);
      input.appendChild(option);
    });
    control = input;
    mode = "enum";
  } else if (type === "number" || type === "integer") {
    control = document.createElement("input");
    control.type = "number";
    control.step = type === "integer" ? "1" : "any";
  } else if (type === "object" || type === "array") {
    control = document.createElement("textarea");
    control.placeholder = t("mcp.test.placeholder.json");
    mode = "json";
  } else {
    control = document.createElement("input");
    control.type = "text";
  }

  control.id = fieldId;
  row.appendChild(control);

  let defaultValue = schema.default;
  if (defaultValue === undefined && type === "boolean" && required) {
    defaultValue = false;
  }
  if (control.tagName === "SELECT") {
    if (defaultValue !== undefined) {
      control.value = JSON.stringify(defaultValue);
    }
  } else if (defaultValue !== undefined) {
    if (mode === "json") {
      try {
        control.value = JSON.stringify(defaultValue, null, 2);
      } catch (error) {
        control.value = String(defaultValue);
      }
    } else {
      control.value = String(defaultValue);
    }
  }

  return {
    row,
    field: {
      name,
      type,
      mode,
      input: control,
      required,
      defaultValue,
    },
  };
};

const renderMcpTestForm = (schema) => {
  if (!elements.mcpTestForm) {
    return;
  }
  elements.mcpTestForm.textContent = "";
  mcpTestFields = [];
  const normalized = normalizeTestSchema(schema);
  if (!normalized || !isPlainObject(normalized.properties) || !Object.keys(normalized.properties).length) {
    const hint = document.createElement("div");
    hint.className = "mcp-test-hint";
    hint.textContent = t("mcp.test.noParams");
    elements.mcpTestForm.appendChild(hint);
    return;
  }
  const requiredSet = new Set(Array.isArray(normalized.required) ? normalized.required : []);
  const fragment = document.createDocumentFragment();
  Object.entries(normalized.properties).forEach(([name, property], index) => {
    const { row, field } = buildMcpTestField(name, property, requiredSet.has(name), index);
    fragment.appendChild(row);
    mcpTestFields.push(field);
  });
  elements.mcpTestForm.appendChild(fragment);
};

const renderMcpTestPanel = (tool, server, options = {}) => {
  if (!tool || !server) {
    resetMcpTestPanel({ clearSelection: true });
    return;
  }
  const key = `${server.name}::${tool.name}`;
  if (!options.force && key === mcpTestActiveKey) {
    return;
  }
  mcpTestActiveKey = key;
  if (elements.mcpTestToolTitle) {
    elements.mcpTestToolTitle.textContent = tool.name || t("mcp.test.empty");
  }
  if (elements.mcpTestToolMeta) {
    const serverTitle = server.display_name || server.name || t("mcp.server.unnamed");
    elements.mcpTestToolMeta.textContent = t("mcp.tool.server", { name: serverTitle });
  }
  if (elements.mcpTestToolDesc) {
    elements.mcpTestToolDesc.textContent = tool.description || "";
  }
  renderMcpTestForm(getToolInputSchema(tool));
  setMcpTestStatus("");
  setMcpTestResult("");
  if (elements.mcpTestRunBtn) {
    elements.mcpTestRunBtn.disabled = false;
  }
};

const syncMcpTestPanel = () => {
  const selected = state.mcp.selectedTool;
  if (!selected || selected.serverIndex !== state.mcp.selectedIndex) {
    resetMcpTestPanel({ clearSelection: true });
    return;
  }
  const tools = state.mcp.toolsByIndex[state.mcp.selectedIndex] || [];
  const tool = tools.find((item) => item.name === selected.name);
  if (!tool) {
    resetMcpTestPanel({ clearSelection: true });
    return;
  }
  const server = state.mcp.servers[state.mcp.selectedIndex];
  renderMcpTestPanel(tool, server);
};

const collectMcpTestArgs = () => {
  const args = {};
  for (const field of mcpTestFields) {
    const { name, type, mode, input, required } = field;
    if (!input) {
      continue;
    }
    let value;
    if (input.tagName === "SELECT") {
      const raw = input.value;
      if (!raw) {
        if (required) {
          return { error: t("mcp.test.error.missing", { name }) };
        }
        continue;
      }
      try {
        value = JSON.parse(raw);
      } catch (error) {
        value = raw;
      }
    } else if (type === "number" || type === "integer") {
      const raw = input.value;
      if (raw.trim() === "") {
        if (required) {
          return { error: t("mcp.test.error.missing", { name }) };
        }
        continue;
      }
      const parsed = Number(raw);
      if (!Number.isFinite(parsed)) {
        return { error: t("mcp.test.error.invalidNumber", { name }) };
      }
      if (type === "integer" && !Number.isInteger(parsed)) {
        return { error: t("mcp.test.error.invalidNumber", { name }) };
      }
      value = parsed;
    } else if (mode === "json") {
      const raw = input.value;
      if (raw.trim() === "") {
        if (required) {
          return { error: t("mcp.test.error.missing", { name }) };
        }
        continue;
      }
      try {
        value = JSON.parse(raw);
      } catch (error) {
        return { error: t("mcp.test.error.invalidJson", { name }) };
      }
    } else {
      const raw = input.value.trim();
      if (!raw) {
        if (required) {
          return { error: t("mcp.test.error.missing", { name }) };
        }
        continue;
      }
      value = raw;
    }
    args[name] = value;
  }
  return { args };
};

const runMcpToolTest = async () => {
  const server = state.mcp.servers[state.mcp.selectedIndex];
  const selected = state.mcp.selectedTool;
  if (!server || !selected) {
    return;
  }
  const tools = state.mcp.toolsByIndex[state.mcp.selectedIndex] || [];
  const tool = tools.find((item) => item.name === selected.name);
  if (!tool) {
    return;
  }
  const collected = collectMcpTestArgs();
  if (collected.error) {
    setMcpTestStatus(collected.error, "error");
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/mcp/tools/call`;
  setMcpTestStatus(t("mcp.test.running"));
  if (elements.mcpTestRunBtn) {
    elements.mcpTestRunBtn.disabled = true;
  }
  try {
    const response = await fetch(endpoint, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify({
        server: server.name,
        tool: tool.name,
        args: collected.args || {},
      }),
    });
    const payload = await response.json().catch(() => ({}));
    if (!response.ok) {
      const message =
        payload?.detail?.message ||
        payload?.detail?.code ||
        payload?.detail ||
        response.statusText;
      throw new Error(message || t("common.requestFailed", { status: response.status }));
    }
    setMcpTestResult(payload.result ?? payload);
    if (payload.warning) {
      setMcpTestStatus(
        `${t("mcp.test.success")} ${t("mcp.test.warning", { message: payload.warning })}`,
        "warning"
      );
    } else {
      setMcpTestStatus(t("mcp.test.success"), "success");
    }
  } catch (error) {
    setMcpTestStatus(
      t("mcp.test.failed", { message: error?.message || String(error) }),
      "error"
    );
  } finally {
    if (elements.mcpTestRunBtn) {
      elements.mcpTestRunBtn.disabled = false;
    }
  }
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
    resetMcpTestPanel({ clearSelection: true });
    return;
  }
  const tools = state.mcp.toolsByIndex[state.mcp.selectedIndex];
  if (!tools || !tools.length) {
    elements.mcpToolList.textContent = t("mcp.tools.notLoaded");
    renderMcpHeader();
    resetMcpTestPanel({ clearSelection: true });
    return;
  }
  tools.forEach((tool) => {
    const item = document.createElement("div");
    item.className = "tool-item";
    if (
      state.mcp.selectedTool?.serverIndex === state.mcp.selectedIndex &&
      state.mcp.selectedTool?.name === tool.name
    ) {
      item.classList.add("is-active");
    }
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
    // 点击工具条目弹出详情 + 测试面板，避免与勾选动作冲突
    item.addEventListener("click", (event) => {
      if (event.target === checkbox) {
        return;
      }
      state.mcp.selectedTool = {
        serverIndex: state.mcp.selectedIndex,
        name: tool.name,
      };
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
      setToolDetailTestMode(true);
      renderMcpTestPanel(tool, server, { force: true });
      elements.mcpToolList
        .querySelectorAll(".tool-item.is-active")
        .forEach((node) => node.classList.remove("is-active"));
      item.classList.add("is-active");
    });
    item.appendChild(checkbox);
    item.appendChild(label);
    elements.mcpToolList.appendChild(item);
  });
  renderMcpHeader();
  syncMcpTestPanel();
};

// 渲染 MCP 服务详情与工具列表
const renderMcpDetail = () => {
  const server = state.mcp.servers[state.mcp.selectedIndex];
  if (!server) {
    elements.mcpToolList.textContent = t("mcp.tools.select");
    renderMcpHeader();
    resetMcpTestPanel({ clearSelection: true });
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
  if (elements.mcpTestRunBtn) {
    elements.mcpTestRunBtn.addEventListener("click", runMcpToolTest);
  }
  if (elements.mcpTestClearBtn) {
    elements.mcpTestClearBtn.addEventListener("click", () => {
      if (!state.mcp.selectedTool) {
        resetMcpTestPanel({ clearSelection: true });
        return;
      }
      clearMcpTestInputs();
      setMcpTestStatus("");
      setMcpTestResult("");
    });
  }
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






