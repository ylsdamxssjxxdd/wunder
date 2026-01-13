import { elements } from "./elements.js?v=20260113-02";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { isPlainObject, parseHeadersValue } from "./utils.js?v=20251229-02";
import { syncPromptTools } from "./tools.js?v=20251231-01";
import { notify } from "./notify.js";
import { appendLog } from "./log.js?v=20260108-02";
import { t } from "./i18n.js?v=20260113-01";

// 规范化 A2A 服务信息，兼容字段命名并补齐默认值。
const normalizeA2aService = (service) => {
  const headers = isPlainObject(service.headers) ? service.headers : {};
  const agentCard = isPlainObject(service.agent_card)
    ? service.agent_card
    : isPlainObject(service.agentCard)
    ? service.agentCard
    : {};
  const maxDepthRaw = service.max_depth ?? service.maxDepth;
  const maxDepth = Number.isFinite(Number(maxDepthRaw)) ? Number(maxDepthRaw) : 0;
  const rawServiceType = String(service.service_type || service.serviceType || "")
    .trim()
    .toLowerCase();
  const serviceName = String(service.name || "").trim().toLowerCase();
  const serviceType =
    rawServiceType === "internal" || rawServiceType === "external"
      ? rawServiceType
      : serviceName === "wunder"
      ? "internal"
      : "external";
  const userId = String(service.user_id || service.userId || "").trim();
  return {
    name: String(service.name || "").trim(),
    display_name: String(service.display_name || service.displayName || "").trim(),
    service_type: serviceType,
    endpoint: String(service.endpoint || service.url || service.baseUrl || service.base_url || "").trim(),
    user_id: userId,
    description: String(service.description || "").trim(),
    headers,
    auth: service.auth || "",
    enabled: service.enabled !== false,
    agent_card: agentCard,
    allow_self: Boolean(service.allow_self || service.allowSelf),
    max_depth: maxDepth,
    default_method: String(service.default_method || service.defaultMethod || "SendMessage").trim() || "SendMessage",
  };
};

const isA2aServiceConnected = (service) =>
  Boolean(service && isPlainObject(service.agent_card) && Object.keys(service.agent_card).length);

const resolveA2aServiceType = (service) =>
  service?.service_type === "internal" ? "internal" : "external";

const formatA2aServiceType = (serviceType) =>
  serviceType === "internal" ? t("a2aServices.type.internal") : t("a2aServices.type.external");

const readAgentCardValue = (card, keys) => {
  if (!card) {
    return undefined;
  }
  for (const key of keys) {
    if (Object.prototype.hasOwnProperty.call(card, key)) {
      return card[key];
    }
  }
  return undefined;
};

const formatAgentCardValue = (value) => {
  if (value === undefined || value === null || value === "") {
    return "-";
  }
  if (Array.isArray(value)) {
    const cleaned = value.filter(Boolean).map((item) => String(item));
    return cleaned.length ? cleaned.join(", ") : "-";
  }
  if (typeof value === "object") {
    return JSON.stringify(value, null, 2);
  }
  return String(value);
};

const renderAgentCardEmpty = (container) => {
  if (!container) {
    return;
  }
  container.textContent = "";
  const empty = document.createElement("div");
  empty.className = "agentcard-empty";
  empty.textContent = t("a2aServices.card.empty");
  container.appendChild(empty);
};

const renderAgentCardTable = (container, rows) => {
  if (!container) {
    return;
  }
  container.textContent = "";
  if (!Array.isArray(rows) || !rows.length) {
    renderAgentCardEmpty(container);
    return;
  }
  const table = document.createElement("table");
  table.className = "agentcard-kv-table";
  const tbody = document.createElement("tbody");
  rows.forEach((row) => {
    const tr = document.createElement("tr");
    const th = document.createElement("th");
    th.textContent = row.label;
    const td = document.createElement("td");
    td.textContent = formatAgentCardValue(row.value);
    tr.appendChild(th);
    tr.appendChild(td);
    tbody.appendChild(tr);
  });
  table.appendChild(tbody);
  container.appendChild(table);
};

const renderAgentCardList = (container, items, mapper) => {
  if (!container) {
    return;
  }
  container.textContent = "";
  if (!Array.isArray(items) || items.length === 0) {
    renderAgentCardEmpty(container);
    return;
  }
  items.forEach((item) => {
    const mapped = mapper(item || {});
    if (!mapped) {
      return;
    }
    const wrapper = document.createElement("div");
    wrapper.className = "agentcard-item";
    const title = document.createElement("div");
    title.className = "agentcard-item-title";
    title.textContent = mapped.title || "-";
    wrapper.appendChild(title);
    if (mapped.description) {
      const desc = document.createElement("div");
      desc.className = "agentcard-item-desc";
      desc.textContent = mapped.description;
      wrapper.appendChild(desc);
    }
    if (mapped.meta) {
      const meta = document.createElement("div");
      meta.className = "agentcard-item-meta";
      meta.textContent = mapped.meta;
      wrapper.appendChild(meta);
    }
    container.appendChild(wrapper);
  });
};

// 渲染 AgentCard.tooling 的工具分组，覆盖 MCP/A2A/内置/知识库。
const renderAgentCardTools = (container, tooling) => {
  if (!container) {
    return;
  }
  container.textContent = "";
  if (!tooling || typeof tooling !== "object") {
    renderAgentCardEmpty(container);
    return;
  }
  const groups = [
    { key: "builtin", label: t("a2a.agentCard.group.builtin") },
    { key: "mcp", label: t("a2a.agentCard.group.mcp") },
    { key: "a2a", label: t("a2a.agentCard.group.a2a") },
    { key: "knowledge", label: t("a2a.agentCard.group.knowledge") },
  ];
  let hasAny = false;
  groups.forEach((group) => {
    const items = Array.isArray(tooling[group.key]) ? tooling[group.key] : [];
    if (!items.length) {
      return;
    }
    hasAny = true;
    const groupWrap = document.createElement("div");
    const title = document.createElement("div");
    title.className = "agentcard-tool-group-title";
    title.textContent = `${group.label} (${items.length})`;
    groupWrap.appendChild(title);
    const list = document.createElement("div");
    list.className = "agentcard-list";
    items.forEach((tool) => {
      const item = document.createElement("details");
      item.className = "agentcard-item";
      const summary = document.createElement("summary");
      const name = String(tool?.tool || tool?.name || "-");
      const server = String(tool?.server || "").trim();
      summary.textContent = server ? `${name} @ ${server}` : name;
      item.appendChild(summary);
      const desc = document.createElement("div");
      desc.className = "agentcard-item-desc";
      desc.textContent = String(tool?.description || "");
      item.appendChild(desc);
      list.appendChild(item);
    });
    groupWrap.appendChild(list);
    container.appendChild(groupWrap);
  });
  if (!hasAny) {
    renderAgentCardEmpty(container);
  }
};

const renderA2aAgentCard = (service) => {
  const card = isPlainObject(service?.agent_card) ? service.agent_card : null;
  if (!card) {
    renderAgentCardEmpty(elements.a2aServiceCardBasic);
    if (elements.a2aServiceCardCapabilities) {
      elements.a2aServiceCardCapabilities.textContent = "";
    }
    if (elements.a2aServiceCardInterfaces) {
      elements.a2aServiceCardInterfaces.textContent = "";
    }
    if (elements.a2aServiceCardSkills) {
      elements.a2aServiceCardSkills.textContent = "";
    }
    if (elements.a2aServiceCardTools) {
      elements.a2aServiceCardTools.textContent = "";
    }
    return;
  }
  const protocolVersion = readAgentCardValue(card, ["protocolVersion", "protocol_version"]);
  const version = readAgentCardValue(card, ["version"]);
  const provider = readAgentCardValue(card, ["provider"]);
  const documentationUrl = readAgentCardValue(card, ["documentationUrl", "documentation_url"]);
  const inputModes = readAgentCardValue(card, ["defaultInputModes", "default_input_modes"]);
  const outputModes = readAgentCardValue(card, ["defaultOutputModes", "default_output_modes"]);
  const supportedInterfaces = readAgentCardValue(card, ["supportedInterfaces", "supported_interfaces"]);
  const capabilities = readAgentCardValue(card, ["capabilities"]) || {};
  const skills = readAgentCardValue(card, ["skills"]);
  const tooling = readAgentCardValue(card, ["tooling"]) || {};

  const providerText =
    provider && typeof provider === "object"
      ? [provider.organization, provider.name, provider.url].filter(Boolean).join(" · ")
      : provider;

  renderAgentCardTable(elements.a2aServiceCardBasic, [
    { label: t("a2a.agentCard.field.protocolVersion"), value: protocolVersion },
    { label: t("a2a.agentCard.field.version"), value: version },
    { label: t("a2a.agentCard.field.provider"), value: providerText },
    { label: t("a2a.agentCard.field.documentation"), value: documentationUrl },
    { label: t("a2a.agentCard.field.inputModes"), value: inputModes },
    { label: t("a2a.agentCard.field.outputModes"), value: outputModes },
  ]);

  renderAgentCardTable(elements.a2aServiceCardCapabilities, [
    {
      label: t("a2a.agentCard.field.streaming"),
      value: readAgentCardValue(capabilities, ["streaming"]),
    },
    {
      label: t("a2a.agentCard.field.pushNotifications"),
      value: readAgentCardValue(capabilities, ["pushNotifications", "push_notifications"]),
    },
    {
      label: t("a2a.agentCard.field.stateTransitionHistory"),
      value: readAgentCardValue(capabilities, ["stateTransitionHistory", "state_transition_history"]),
    },
    {
      label: t("a2a.agentCard.field.supportsExtended"),
      value: readAgentCardValue(card, ["supportsExtendedAgentCard"]),
    },
  ]);

  renderAgentCardList(elements.a2aServiceCardInterfaces, supportedInterfaces, (item) => ({
    title: String(item.protocolBinding || item.protocol_binding || "-"),
    description: String(item.url || "-"),
  }));

  renderAgentCardList(elements.a2aServiceCardSkills, skills, (item) => ({
    title: String(item.name || item.id || "-"),
    description: String(item.description || ""),
  }));

  renderAgentCardTools(elements.a2aServiceCardTools, tooling);
};

const updateA2aConnectButton = () => {
  const service = state.a2aServices.services[state.a2aServices.selectedIndex];
  if (!elements.a2aServiceConnectBtn) {
    return;
  }
  const connected = service ? isA2aServiceConnected(service) : false;
  const iconClass = connected ? "fa-solid fa-arrows-rotate" : "fa-solid fa-link";
  const label = connected ? t("a2aServices.action.refresh") : t("a2aServices.action.connect");
  elements.a2aServiceConnectBtn.innerHTML = `<i class="${iconClass}"></i>${label}`;
  elements.a2aServiceConnectBtn.disabled = !service;
};

const renderA2aHeader = () => {
  const service = state.a2aServices.services[state.a2aServices.selectedIndex];
  if (!service) {
    elements.a2aServiceDetailTitle.textContent = t("a2aServices.detail.none");
    elements.a2aServiceDetailMeta.textContent = "";
    elements.a2aServiceDetailDesc.textContent = "";
    elements.a2aServiceEditBtn.disabled = true;
    elements.a2aServiceDeleteBtn.disabled = true;
    if (elements.a2aServiceEnabled) {
      elements.a2aServiceEnabled.checked = false;
      elements.a2aServiceEnabled.disabled = true;
    }
    updateA2aConnectButton();
    renderAgentCardEmpty(elements.a2aServiceCardBasic);
    return;
  }
  const title = service.display_name || service.name || t("a2aServices.detail.none");
  const metaParts = [];
  if (service.display_name && service.name) {
    metaParts.push(`ID: ${service.name}`);
  }
  if (service.endpoint) {
    metaParts.push(service.endpoint);
  }
  const serviceType = resolveA2aServiceType(service);
  metaParts.push(formatA2aServiceType(serviceType));
  if (serviceType === "internal" && service.user_id) {
    metaParts.push(t("a2aServices.meta.userId", { user_id: service.user_id }));
  }
  metaParts.push(service.enabled !== false ? t("a2aServices.status.enabled") : t("a2aServices.status.disabled"));
  elements.a2aServiceDetailTitle.textContent = title;
  elements.a2aServiceDetailMeta.textContent = metaParts.join(" · ");
  elements.a2aServiceDetailDesc.textContent = service.description || "";
  elements.a2aServiceEditBtn.disabled = false;
  elements.a2aServiceDeleteBtn.disabled = false;
  if (elements.a2aServiceEnabled) {
    elements.a2aServiceEnabled.checked = service.enabled !== false;
    elements.a2aServiceEnabled.disabled = false;
  }
  updateA2aConnectButton();
};

const renderA2aServices = () => {
  elements.a2aServiceList.textContent = "";
  if (!state.a2aServices.services.length) {
    elements.a2aServiceList.textContent = t("a2aServices.list.empty");
    renderA2aHeader();
    return;
  }
  state.a2aServices.services.forEach((service, index) => {
    const item = document.createElement("button");
    item.type = "button";
    item.className = "list-item";
    if (index === state.a2aServices.selectedIndex) {
      item.classList.add("active");
    }
    const title = service.display_name || service.name || t("a2aServices.detail.none");
    const subtitleParts = [];
    if (service.display_name && service.name) {
      subtitleParts.push(`ID: ${service.name}`);
    }
    subtitleParts.push(formatA2aServiceType(resolveA2aServiceType(service)));
    subtitleParts.push(service.endpoint || "-");
    item.innerHTML = `<div>${title}</div><small>${subtitleParts.join(" · ")}</small>`;
    item.addEventListener("click", () => {
      state.a2aServices.selectedIndex = index;
      renderA2aServices();
      renderA2aDetail();
    });
    elements.a2aServiceList.appendChild(item);
  });
  renderA2aHeader();
};

const renderA2aDetail = () => {
  const service = state.a2aServices.services[state.a2aServices.selectedIndex];
  renderA2aHeader();
  renderA2aAgentCard(service);
};

const requestA2aAgentCard = async (service) => {
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/a2a/card`;
  const response = await fetch(endpoint, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      endpoint: service.endpoint,
      headers: service.headers || {},
      auth: service.auth || null,
    }),
  });
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const result = await response.json();
  return result.agent_card || {};
};

const saveA2aServices = async (options = {}) => {
  const { refreshUI = true } = options;
  const selectedName = state.a2aServices.services[state.a2aServices.selectedIndex]?.name || "";
  const saveVersion = ++state.a2aServices.saveVersion;
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/a2a`;
  const payloadServices = state.a2aServices.services.map((service) => ({
    name: service.name,
    endpoint: service.endpoint,
    service_type: service.service_type || "external",
    user_id: service.user_id || "",
    enabled: service.enabled !== false,
    description: service.description,
    display_name: service.display_name,
    headers: service.headers || {},
    auth: service.auth || null,
    agent_card: service.agent_card || {},
    allow_self: service.allow_self || false,
    max_depth: service.max_depth || 0,
    default_method: service.default_method || "SendMessage",
  }));
  const response = await fetch(endpoint, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ services: payloadServices }),
  });
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const result = await response.json();
  if (saveVersion !== state.a2aServices.saveVersion) {
    return;
  }
  syncPromptTools();
  if (!refreshUI) {
    return;
  }
  state.a2aServices.services = Array.isArray(result.services)
    ? result.services.map(normalizeA2aService)
    : [];
  if (selectedName) {
    const nextIndex = state.a2aServices.services.findIndex((item) => item.name === selectedName);
    state.a2aServices.selectedIndex = nextIndex >= 0 ? nextIndex : state.a2aServices.services.length ? 0 : -1;
  } else {
    state.a2aServices.selectedIndex = state.a2aServices.services.length ? 0 : -1;
  }
  renderA2aServices();
  renderA2aDetail();
};

const connectA2aService = async () => {
  const service = state.a2aServices.services[state.a2aServices.selectedIndex];
  if (!service || !service.name || !service.endpoint) {
    notify(t("a2aServices.form.required"), "warn");
    return;
  }
  const wasConnected = isA2aServiceConnected(service);
  if (elements.a2aServiceCardBasic) {
    elements.a2aServiceCardBasic.textContent = t("a2aServices.connect.connecting");
  }
  try {
    const card = await requestA2aAgentCard(service);
    service.agent_card = card;
    renderA2aDetail();
    const message = wasConnected
      ? t("a2aServices.connect.refreshed")
      : t("a2aServices.connect.connected");
    appendLog(message);
    notify(message, "success");
    await saveA2aServices({ refreshUI: false });
  } catch (error) {
    renderA2aDetail();
    notify(t("a2aServices.connect.failed"), "error");
  }
};

const toggleA2aServiceEnabled = async () => {
  const index = state.a2aServices.selectedIndex;
  if (index < 0) {
    return;
  }
  const service = state.a2aServices.services[index];
  if (!service) {
    return;
  }
  const previous = service.enabled !== false;
  const next = elements.a2aServiceEnabled.checked;
  if (previous === next) {
    return;
  }
  service.enabled = next;
  try {
    await saveA2aServices();
    const serviceName = service.display_name || service.name || t("a2aServices.detail.none");
    const message = next
      ? t("a2aServices.enabled", { name: serviceName })
      : t("a2aServices.disabled", { name: serviceName });
    appendLog(message);
    notify(message, "success");
  } catch (error) {
    service.enabled = previous;
    elements.a2aServiceEnabled.checked = previous;
    notify(t("a2aServices.saveFailed", { message: error.message || "-" }), "error");
  }
};

const updateA2aServiceTypeView = () => {
  const serviceType = elements.a2aServiceType?.value === "internal" ? "internal" : "external";
  if (elements.a2aServiceUserIdRow) {
    elements.a2aServiceUserIdRow.classList.toggle("is-hidden", serviceType !== "internal");
  }
};

const openA2aModal = (index) => {
  state.a2aServiceModal.index = index;
  const service = index !== null ? state.a2aServices.services[index] : null;
  elements.a2aServiceModalTitle.textContent =
    index === null ? t("a2aServices.modal.addTitle") : t("a2aServices.modal.editTitle");
  elements.a2aServiceName.value = service?.name || "";
  elements.a2aServiceDisplayName.value = service?.display_name || "";
  elements.a2aServiceType.value = service?.service_type || "external";
  elements.a2aServiceEndpoint.value = service?.endpoint || "";
  elements.a2aServiceUserId.value = service?.user_id || "";
  elements.a2aServiceDescription.value = service?.description || "";
  elements.a2aServiceHeaders.value =
    service?.headers && Object.keys(service.headers).length
      ? JSON.stringify(service.headers, null, 2)
      : "";
  elements.a2aServiceHeadersError.textContent = "";
  updateA2aServiceTypeView();
  elements.a2aServiceModal.classList.add("active");
};

const closeA2aModal = () => {
  elements.a2aServiceModal.classList.remove("active");
};

const upsertA2aService = (incoming) => {
  const targetIndex = state.a2aServices.services.findIndex((item) => item.name === incoming.name);
  if (targetIndex >= 0) {
    const previous = state.a2aServices.services[targetIndex];
    state.a2aServices.services[targetIndex] = {
      ...previous,
      ...incoming,
      agent_card: incoming.agent_card && Object.keys(incoming.agent_card).length
        ? incoming.agent_card
        : previous.agent_card,
    };
    return targetIndex;
  }
  state.a2aServices.services.push(incoming);
  return state.a2aServices.services.length - 1;
};

const applyA2aModal = () => {
  const name = elements.a2aServiceName.value.trim();
  const endpoint = elements.a2aServiceEndpoint.value.trim();
  if (!name || !endpoint) {
    notify(t("a2aServices.form.required"), "warn");
    return false;
  }
  const serviceType = elements.a2aServiceType.value === "internal" ? "internal" : "external";
  const userId = elements.a2aServiceUserId.value.trim();
  if (serviceType === "internal" && !userId) {
    notify(t("a2aServices.form.userIdRequired"), "warn");
    return false;
  }
  const headersResult = parseHeadersValue(elements.a2aServiceHeaders.value);
  if (headersResult.error) {
    elements.a2aServiceHeadersError.textContent = headersResult.error;
    return false;
  }
  const baseService =
    state.a2aServiceModal.index !== null ? state.a2aServices.services[state.a2aServiceModal.index] : null;
  const enabled = baseService ? baseService.enabled !== false : true;
  const service = normalizeA2aService({
    name,
    display_name: elements.a2aServiceDisplayName.value.trim(),
    service_type: serviceType,
    endpoint,
    user_id: serviceType === "internal" ? userId : "",
    description: elements.a2aServiceDescription.value.trim(),
    headers: headersResult.headers || {},
    enabled,
    auth: baseService?.auth || "",
    agent_card: baseService?.agent_card || {},
    allow_self: baseService?.allow_self || false,
    max_depth: baseService?.max_depth || 0,
    default_method: baseService?.default_method || "SendMessage",
  });

  if (state.a2aServiceModal.index === null) {
    const nextIndex = upsertA2aService(service);
    state.a2aServices.selectedIndex = nextIndex;
  } else {
    const index = state.a2aServiceModal.index;
    const previous = state.a2aServices.services[index];
    state.a2aServices.services[index] = { ...previous, ...service };
    state.a2aServices.selectedIndex = index;
  }
  renderA2aServices();
  renderA2aDetail();
  return true;
};

const deleteA2aService = async () => {
  if (state.a2aServices.selectedIndex < 0) {
    return;
  }
  const removed = state.a2aServices.services[state.a2aServices.selectedIndex];
  const removedName = removed?.display_name || removed?.name || t("a2aServices.detail.none");
  state.a2aServices.services.splice(state.a2aServices.selectedIndex, 1);
  if (!state.a2aServices.services.length) {
    state.a2aServices.selectedIndex = -1;
  } else {
    state.a2aServices.selectedIndex = Math.max(0, state.a2aServices.selectedIndex - 1);
  }
  renderA2aServices();
  renderA2aDetail();
  try {
    await saveA2aServices();
    const message = t("a2aServices.delete.success", { name: removedName });
    appendLog(message);
    notify(message, "success");
  } catch (error) {
    notify(t("a2aServices.saveFailed", { message: error.message || "-" }), "error");
  }
};

export const loadA2aServices = async () => {
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/a2a`;
  const response = await fetch(endpoint);
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const result = await response.json();
  state.a2aServices.services = Array.isArray(result.services)
    ? result.services.map(normalizeA2aService)
    : [];
  state.a2aServices.selectedIndex = state.a2aServices.services.length ? 0 : -1;
  renderA2aServices();
  renderA2aDetail();
};

export const initA2aServicesPanel = () => {
  elements.a2aServiceAddBtn.addEventListener("click", () => openA2aModal(null));
  elements.a2aServiceConnectBtn.addEventListener("click", connectA2aService);
  elements.a2aServiceEditBtn.addEventListener("click", () => {
    if (state.a2aServices.selectedIndex < 0) {
      return;
    }
    openA2aModal(state.a2aServices.selectedIndex);
  });
  elements.a2aServiceDeleteBtn.addEventListener("click", deleteA2aService);
  elements.a2aServiceEnabled.addEventListener("change", toggleA2aServiceEnabled);
  elements.a2aServiceModalSave.addEventListener("click", async () => {
    const ok = applyA2aModal();
    if (!ok) {
      return;
    }
    try {
      await saveA2aServices();
      appendLog(t("a2aServices.save.success"));
      notify(t("a2aServices.save.success"), "success");
      closeA2aModal();
    } catch (error) {
      notify(t("a2aServices.saveFailed", { message: error.message || "-" }), "error");
    }
  });
  elements.a2aServiceModalCancel.addEventListener("click", closeA2aModal);
  elements.a2aServiceModalClose.addEventListener("click", closeA2aModal);
  elements.a2aServiceModal.addEventListener("click", (event) => {
    if (event.target === elements.a2aServiceModal) {
      closeA2aModal();
    }
  });
  elements.a2aServiceHeaders.addEventListener("input", () => {
    elements.a2aServiceHeadersError.textContent = "";
  });
  elements.a2aServiceType.addEventListener("change", updateA2aServiceTypeView);
};


