import { APP_CONFIG } from "../app.config.js?v=20260110-04";
import { elements } from "./elements.js?v=20260214-01";
import { getWunderBase } from "./api.js";
import { getCurrentLanguage, t } from "./i18n.js?v=20260214-01";

const DEFAULT_API_DOCS_SRC = "/docs/api-docs.json";

const apiDocsState = {
  data: { groups: [] },
  endpointMap: new Map(),
  selectedId: "",
  loading: false,
  openGroups: new Set(),
};

const resolveApiBase = () => {
  const normalized = String(getWunderBase() || "").trim();
  if (normalized) {
    return normalized;
  }
  const fallback = String(APP_CONFIG.defaultApiBase || "").trim();
  return fallback || "/wunder";
};

const resolveApiOrigin = (base) => {
  const raw = String(base || "").trim();
  if (/^https?:/i.test(raw)) {
    return raw.replace(/\/?wunder\/?$/i, "").replace(/\/$/, "");
  }
  if (typeof window !== "undefined" && window.location?.origin) {
    return window.location.origin;
  }
  return "";
};

const resolveUserId = () => {
  const current = String(elements.userId?.value || "").trim();
  if (current) {
    return current;
  }
  const fallback = String(APP_CONFIG.defaultUserId || "").trim();
  return fallback || "demo_user";
};

const applyTemplate = (template, context) => {
  if (template === null || template === undefined) {
    return "";
  }
  const text = String(template);
  return text.replace(/\{(\w+)\}/g, (match, key) => {
    if (Object.prototype.hasOwnProperty.call(context, key)) {
      const value = context[key];
      return value === null || value === undefined ? "" : String(value);
    }
    return match;
  });
};

const resolveDocText = (value) => {
  if (value === null || value === undefined) {
    return "";
  }
  if (typeof value === "string" || typeof value === "number") {
    return String(value);
  }
  if (typeof value === "object") {
    const lang = getCurrentLanguage();
    if (value[lang]) {
      return String(value[lang]);
    }
    if (value["zh-CN"]) {
      return String(value["zh-CN"]);
    }
    if (value["en-US"]) {
      return String(value["en-US"]);
    }
    if (value.default) {
      return String(value.default);
    }
  }
  return String(value);
};

const resolveDocArray = (value) => {
  if (!value) {
    return [];
  }
  if (Array.isArray(value)) {
    return value;
  }
  return [value];
};

const normalizeEndpointId = (endpoint, groupIndex, endpointIndex) => {
  const method = String(endpoint?.method || "api")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  const path = String(endpoint?.path || "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  const fallback = `endpoint-${groupIndex + 1}-${endpointIndex + 1}`;
  if (!path) {
    return fallback;
  }
  const combined = `${method}-${path}`.replace(/-+/g, "-");
  return combined || fallback;
};

const normalizeApiDocs = (raw) => {
  const source = raw && typeof raw === "object" ? raw : {};
  const groups = Array.isArray(source.groups) ? source.groups : [];
  const normalizedGroups = groups.map((group, groupIndex) => {
    const endpoints = Array.isArray(group.endpoints) ? group.endpoints : [];
    const normalizedEndpoints = endpoints.map((endpoint, endpointIndex) => {
      const id = endpoint?.id ? String(endpoint.id) : normalizeEndpointId(endpoint, groupIndex, endpointIndex);
      return { ...endpoint, id };
    });
    const groupId = group?.id ? String(group.id) : `group-${groupIndex + 1}`;
    return { ...group, id: groupId, endpoints: normalizedEndpoints };
  });
  return { ...source, groups: normalizedGroups };
};

const rebuildEndpointMap = (data) => {
  const map = new Map();
  if (!data?.groups) {
    return map;
  }
  data.groups.forEach((group) => {
    (group.endpoints || []).forEach((endpoint) => {
      if (!endpoint?.id) {
        return;
      }
      map.set(endpoint.id, endpoint);
    });
  });
  return map;
};

const getDefaultEndpointId = (data) => {
  if (!data?.groups?.length) {
    return "";
  }
  for (const group of data.groups) {
    if (group?.endpoints?.length) {
      return group.endpoints[0].id || "";
    }
  }
  return "";
};

const getMethodClass = (method) => {
  const normalized = String(method || "").toLowerCase();
  if (normalized.includes("/")) {
    return "api-docs-method api-docs-method--multi";
  }
  if (normalized === "get") {
    return "api-docs-method api-docs-method--get";
  }
  if (normalized === "post") {
    return "api-docs-method api-docs-method--post";
  }
  if (normalized === "put") {
    return "api-docs-method api-docs-method--put";
  }
  if (normalized === "delete") {
    return "api-docs-method api-docs-method--delete";
  }
  return "api-docs-method";
};

const renderOverview = () => {
  if (elements.apiDocsBaseUrl) {
    elements.apiDocsBaseUrl.textContent = resolveApiBase();
  }
};

const renderMessage = (container, message, className = "api-docs-empty-state") => {
  if (!container) {
    return;
  }
  container.textContent = "";
  const messageEl = document.createElement("div");
  messageEl.className = className;
  messageEl.textContent = message;
  container.appendChild(messageEl);
};

const captureOpenGroups = () => {
  if (!elements.apiDocsEndpointList) {
    return new Set();
  }
  const openIds = new Set();
  elements.apiDocsEndpointList
    .querySelectorAll("details.api-docs-group-card[open]")
    .forEach((node) => {
      const groupId = node.dataset.groupId;
      if (groupId) {
        openIds.add(groupId);
      }
    });
  return openIds;
};

const findGroupIdByEndpoint = (endpointId, data) => {
  if (!endpointId || !data?.groups?.length) {
    return "";
  }
  for (const group of data.groups) {
    if (!group?.endpoints?.length) {
      continue;
    }
    if (group.endpoints.some((endpoint) => endpoint.id === endpointId)) {
      return group.id || "";
    }
  }
  return "";
};

const renderEndpointList = () => {
  if (!elements.apiDocsEndpointList) {
    return;
  }
  const listContainer = elements.apiDocsEndpointList;
  const scrollTop = listContainer.scrollTop;
  apiDocsState.openGroups = captureOpenGroups();
  listContainer.textContent = "";
  if (apiDocsState.loading) {
    renderMessage(listContainer, t("apiDocs.loading"));
    return;
  }
  const groups = apiDocsState.data?.groups || [];
  if (!groups.length) {
    renderMessage(listContainer, t("apiDocs.empty.list"));
    return;
  }
  const activeGroupId = findGroupIdByEndpoint(apiDocsState.selectedId, apiDocsState.data);
  const hasSavedOpen = apiDocsState.openGroups.size > 0;
  groups.forEach((group, index) => {
    const wrapper = document.createElement("details");
    wrapper.className = "api-docs-group-card";
    wrapper.dataset.groupId = group.id || "";
    wrapper.open =
      (group.id && apiDocsState.openGroups.has(group.id)) ||
      (group.id && group.id === activeGroupId) ||
      (!hasSavedOpen && !activeGroupId && index === 0);

    const summary = document.createElement("summary");
    summary.className = "api-docs-group-summary";
    const title = document.createElement("span");
    title.className = "api-docs-group-summary-title";
    title.textContent = resolveDocText(group.title) || group.name || group.id || "";
    const count = document.createElement("span");
    count.className = "api-docs-group-summary-count";
    count.textContent = t("apiDocs.endpoint.count", { count: (group.endpoints || []).length });
    summary.appendChild(title);
    summary.appendChild(count);
    wrapper.appendChild(summary);

    const groupBody = document.createElement("div");
    groupBody.className = "api-docs-group-body";

    (group.endpoints || []).forEach((endpoint) => {
      const row = document.createElement("button");
      row.type = "button";
      row.className = "api-docs-endpoint";
      row.dataset.endpointId = endpoint.id;
      if (endpoint.id === apiDocsState.selectedId) {
        row.classList.add("is-active");
      }
      const method = document.createElement("span");
      method.className = getMethodClass(endpoint.method);
      method.textContent = endpoint.method || "";
      const endpointBody = document.createElement("div");
      endpointBody.className = "api-docs-endpoint-body";
      const path = document.createElement("div");
      path.className = "api-docs-endpoint-path";
      path.textContent = endpoint.path || "";
      const desc = document.createElement("div");
      desc.className = "api-docs-endpoint-desc";
      desc.textContent = resolveDocText(endpoint.title) || resolveDocText(endpoint.summary) || "";
      endpointBody.appendChild(path);
      endpointBody.appendChild(desc);
      row.appendChild(method);
      row.appendChild(endpointBody);
      row.addEventListener("click", () => selectEndpoint(endpoint.id));
      groupBody.appendChild(row);
    });
    wrapper.appendChild(groupBody);
    listContainer.appendChild(wrapper);
  });
  if (typeof window !== "undefined" && typeof window.requestAnimationFrame === "function") {
    window.requestAnimationFrame(() => {
      listContainer.scrollTop = scrollTop;
    });
  } else {
    listContainer.scrollTop = scrollTop;
  }
};

const createSection = (titleKey, overrideTitle) => {
  const section = document.createElement("div");
  section.className = "api-docs-section";
  const title = document.createElement("div");
  title.className = "api-docs-section-title";
  title.textContent = overrideTitle || t(titleKey);
  section.appendChild(title);
  return section;
};

const renderFieldTable = (fields) => {
  if (!fields || !fields.length) {
    return null;
  }
  const table = document.createElement("table");
  table.className = "api-docs-table";
  const thead = document.createElement("thead");
  const headRow = document.createElement("tr");
  const headers = [
    t("apiDocs.table.field"),
    t("apiDocs.table.type"),
    t("apiDocs.table.location"),
    t("apiDocs.table.required"),
    t("apiDocs.table.desc"),
  ];
  headers.forEach((label) => {
    const th = document.createElement("th");
    th.textContent = label;
    headRow.appendChild(th);
  });
  thead.appendChild(headRow);
  table.appendChild(thead);
  const tbody = document.createElement("tbody");
  fields.forEach((field) => {
    const row = document.createElement("tr");
    const nameCell = document.createElement("td");
    const nameCode = document.createElement("code");
    nameCode.textContent = field.name || "";
    nameCell.appendChild(nameCode);
    const typeCell = document.createElement("td");
    typeCell.textContent = field.type || "";
    const locationCell = document.createElement("td");
    const locationKey = field.location ? `apiDocs.location.${field.location}` : "";
    const locationLabel = locationKey ? t(locationKey) : "";
    locationCell.textContent = locationLabel && locationLabel !== locationKey ? locationLabel : field.location || "-";
    const requiredCell = document.createElement("td");
    const badge = document.createElement("span");
    badge.className = field.required ? "api-docs-badge is-required" : "api-docs-badge";
    badge.textContent = field.required ? t("apiDocs.required") : t("apiDocs.optional");
    requiredCell.appendChild(badge);
    const descCell = document.createElement("td");
    descCell.textContent = resolveDocText(field.desc);
    row.appendChild(nameCell);
    row.appendChild(typeCell);
    row.appendChild(locationCell);
    row.appendChild(requiredCell);
    row.appendChild(descCell);
    tbody.appendChild(row);
  });
  table.appendChild(tbody);
  return table;
};

const renderCodeBlock = (codeText) => {
  const pre = document.createElement("pre");
  pre.className = "api-docs-code";
  const code = document.createElement("code");
  code.textContent = String(codeText || "");
  pre.appendChild(code);
  return pre;
};

const renderNotes = (section, notes) => {
  const noteItems = resolveDocArray(notes);
  noteItems.forEach((note) => {
    const text = resolveDocText(note);
    if (!text) {
      return;
    }
    const noteEl = document.createElement("div");
    noteEl.className = "api-docs-note";
    noteEl.textContent = text;
    section.appendChild(noteEl);
  });
};

const renderEndpointDetail = () => {
  if (!elements.apiDocsDetail) {
    return;
  }
  const container = elements.apiDocsDetail;
  container.textContent = "";
  container.classList.remove("is-empty");

  if (apiDocsState.loading) {
    container.classList.add("is-empty");
    renderMessage(container, t("apiDocs.loading"));
    return;
  }

  const endpoint = apiDocsState.endpointMap.get(apiDocsState.selectedId);
  if (!endpoint) {
    container.classList.add("is-empty");
    renderMessage(container, t("apiDocs.empty.detail"));
    return;
  }

  const base = resolveApiBase();
  const origin = resolveApiOrigin(base);
  const userId = resolveUserId();
  const templateContext = { base, origin, userId };

  const header = document.createElement("div");
  header.className = "api-docs-detail-header";
  const method = document.createElement("span");
  method.className = getMethodClass(endpoint.method);
  method.textContent = endpoint.method || "";
  const path = document.createElement("div");
  path.className = "api-docs-detail-path";
  path.textContent = endpoint.path || "";
  const title = document.createElement("div");
  title.className = "api-docs-detail-title";
  title.textContent = resolveDocText(endpoint.title) || endpoint.path || "";
  const titleWrap = document.createElement("div");
  titleWrap.className = "api-docs-detail-title-wrap";
  titleWrap.appendChild(title);
  titleWrap.appendChild(path);
  header.appendChild(method);
  header.appendChild(titleWrap);
  container.appendChild(header);

  const summary = resolveDocText(endpoint.summary);
  if (summary) {
    const summaryEl = document.createElement("div");
    summaryEl.className = "api-docs-detail-summary";
    summaryEl.textContent = summary;
    container.appendChild(summaryEl);
  }

  const metaItems = [];
  if (endpoint.scope) {
    metaItems.push({ label: t("apiDocs.meta.scope"), value: resolveDocText(endpoint.scope) });
  }
  if (endpoint.auth) {
    metaItems.push({ label: t("apiDocs.meta.auth"), value: resolveDocText(endpoint.auth) });
  }
  if (endpoint.contentType) {
    metaItems.push({ label: t("apiDocs.meta.contentType"), value: resolveDocText(endpoint.contentType) });
  }
  if (metaItems.length) {
    const meta = document.createElement("div");
    meta.className = "api-docs-detail-meta";
    metaItems.forEach((item) => {
      const row = document.createElement("div");
      row.className = "api-docs-detail-meta-row";
      const label = document.createElement("span");
      label.className = "api-docs-detail-meta-label";
      label.textContent = item.label;
      const value = document.createElement("span");
      value.className = "api-docs-detail-meta-value";
      value.textContent = item.value;
      row.appendChild(label);
      row.appendChild(value);
      meta.appendChild(row);
    });
    container.appendChild(meta);
  }

  const request = endpoint.request || null;
  if (request) {
    const section = createSection("apiDocs.section.request");
    const description = resolveDocText(request.description);
    if (description) {
      const descEl = document.createElement("div");
      descEl.className = "api-docs-text";
      descEl.textContent = description;
      section.appendChild(descEl);
    }
    const table = renderFieldTable(request.fields || []);
    if (table) {
      section.appendChild(table);
    } else if (!description && !(request.notes || request.example)) {
      const empty = document.createElement("div");
      empty.className = "api-docs-empty-inline";
      empty.textContent = t("apiDocs.empty.fields");
      section.appendChild(empty);
    }
    renderNotes(section, request.notes);
    if (request.example) {
      const exampleSection = createSection("apiDocs.section.requestExample");
      const code = applyTemplate(request.example, templateContext);
      exampleSection.appendChild(renderCodeBlock(code));
      container.appendChild(section);
      container.appendChild(exampleSection);
    } else {
      container.appendChild(section);
    }
  }

  const response = endpoint.response || null;
  if (response) {
    const section = createSection("apiDocs.section.response");
    const description = resolveDocText(response.description);
    if (description) {
      const descEl = document.createElement("div");
      descEl.className = "api-docs-text";
      descEl.textContent = description;
      section.appendChild(descEl);
    }
    const table = renderFieldTable(response.fields || []);
    if (table) {
      section.appendChild(table);
    } else if (!description && !(response.notes || response.example)) {
      const empty = document.createElement("div");
      empty.className = "api-docs-empty-inline";
      empty.textContent = t("apiDocs.empty.fields");
      section.appendChild(empty);
    }
    renderNotes(section, response.notes);
    if (response.example) {
      const exampleSection = createSection("apiDocs.section.responseExample");
      const code = applyTemplate(response.example, templateContext);
      exampleSection.appendChild(renderCodeBlock(code));
      container.appendChild(section);
      container.appendChild(exampleSection);
    } else {
      container.appendChild(section);
    }
  }

  const stream = endpoint.stream || null;
  if (stream) {
    const events = resolveDocArray(stream.events || []);
    if (events.length) {
      const section = createSection("apiDocs.section.events");
      const list = document.createElement("ul");
      list.className = "api-docs-list";
      events.forEach((eventItem) => {
        const item = document.createElement("li");
        const code = document.createElement("code");
        code.textContent = eventItem.name || "";
        const desc = document.createElement("span");
        desc.textContent = resolveDocText(eventItem.desc);
        item.appendChild(code);
        item.appendChild(desc);
        list.appendChild(item);
      });
      section.appendChild(list);
      renderNotes(section, stream.notes);
      container.appendChild(section);
    } else if (stream.notes) {
      const section = createSection("apiDocs.section.events");
      renderNotes(section, stream.notes);
      container.appendChild(section);
    }
    if (stream.example) {
      const exampleSection = createSection("apiDocs.section.eventExample");
      const code = applyTemplate(stream.example, templateContext);
      exampleSection.appendChild(renderCodeBlock(code));
      container.appendChild(exampleSection);
    }
  }

  const samples = resolveDocArray(endpoint.samples || []);
  if (samples.length) {
    const section = createSection("apiDocs.section.samples");
    samples.forEach((sample) => {
      const titleText = resolveDocText(sample.title);
      if (titleText) {
        const sampleTitle = document.createElement("div");
        sampleTitle.className = "api-docs-sample-title";
        sampleTitle.textContent = titleText;
        section.appendChild(sampleTitle);
      }
      const code = applyTemplate(sample.code, templateContext);
      section.appendChild(renderCodeBlock(code));
    });
    container.appendChild(section);
  }

  renderNotes(container, endpoint.notes);
};

const selectEndpoint = (endpointId) => {
  if (!endpointId || endpointId === apiDocsState.selectedId) {
    return;
  }
  apiDocsState.selectedId = endpointId;
  renderEndpointList();
  renderEndpointDetail();
};

const applyApiDocsData = (raw) => {
  const data = normalizeApiDocs(raw);
  apiDocsState.data = data;
  apiDocsState.endpointMap = rebuildEndpointMap(data);
  if (!apiDocsState.endpointMap.has(apiDocsState.selectedId)) {
    apiDocsState.selectedId = getDefaultEndpointId(data);
  }
  renderEndpointList();
  renderEndpointDetail();
};

const loadApiDocsData = async () => {
  if (!elements.apiDocsPanel) {
    return;
  }
  apiDocsState.loading = true;
  renderEndpointList();
  renderEndpointDetail();
  const src = elements.apiDocsPanel?.dataset?.apiDocsSrc || DEFAULT_API_DOCS_SRC;
  try {
    const response = await fetch(src, { cache: "no-store" });
    if (!response.ok) {
      throw new Error(`${response.status} ${response.statusText}`);
    }
    const data = await response.json();
    apiDocsState.loading = false;
    applyApiDocsData(data);
  } catch (error) {
    apiDocsState.loading = false;
    applyApiDocsData({ groups: [] });
    renderMessage(elements.apiDocsDetail, t("apiDocs.loadFailed", { message: error?.message || String(error) }));
  }
};

const handleContextChange = () => {
  renderOverview();
  renderEndpointDetail();
};

export const initApiDocsPanel = () => {
  if (!elements.apiDocsPanel) {
    return;
  }
  renderOverview();
  loadApiDocsData();

  if (elements.userId) {
    elements.userId.addEventListener("change", handleContextChange);
  }
  if (elements.promptUserId) {
    elements.promptUserId.addEventListener("change", handleContextChange);
  }
  window.addEventListener("wunder:language-changed", () => {
    renderOverview();
    renderEndpointList();
    renderEndpointDetail();
  });
};
