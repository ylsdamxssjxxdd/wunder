import { APP_CONFIG } from "../app.config.js?v=20260110-04";
import { elements } from "./elements.js?v=20260113-01";
import { getWunderBase } from "./api.js";
import { t } from "./i18n.js?v=20260113-01";

const ENDPOINT_GROUPS = [
  {
    titleKey: "apiDocs.group.core",
    endpoints: [
      { method: "POST", path: "/wunder", descKey: "apiDocs.endpoint.wunder" },
    ],
  },
  {
    titleKey: "apiDocs.group.tools",
    endpoints: [
      { method: "POST", path: "/wunder/system_prompt", descKey: "apiDocs.endpoint.systemPrompt" },
      { method: "GET", path: "/wunder/tools", descKey: "apiDocs.endpoint.tools" },
      { method: "GET", path: "/wunder/i18n", descKey: "apiDocs.endpoint.i18n" },
      { method: "POST", path: "/wunder/attachments/convert", descKey: "apiDocs.endpoint.attachments" },
    ],
  },
  {
    titleKey: "apiDocs.group.userTools",
    endpoints: [
      { method: "GET/POST", path: "/wunder/user_tools/mcp", descKey: "apiDocs.endpoint.userToolsMcp" },
      { method: "GET/POST", path: "/wunder/user_tools/skills", descKey: "apiDocs.endpoint.userToolsSkills" },
      {
        method: "GET/POST",
        path: "/wunder/user_tools/knowledge",
        descKey: "apiDocs.endpoint.userToolsKnowledge",
      },
      {
        method: "POST",
        path: "/wunder/user_tools/extra_prompt",
        descKey: "apiDocs.endpoint.userToolsExtraPrompt",
      },
    ],
  },
  {
    titleKey: "apiDocs.group.workspace",
    endpoints: [
      { method: "GET", path: "/wunder/workspace", descKey: "apiDocs.endpoint.workspace" },
      {
        method: "GET/PUT/DELETE",
        path: "/wunder/workspace/file",
        descKey: "apiDocs.endpoint.workspaceFile",
      },
      { method: "POST", path: "/wunder/workspace/upload", descKey: "apiDocs.endpoint.workspaceUpload" },
      {
        method: "POST",
        path: "/wunder/workspace/download",
        descKey: "apiDocs.endpoint.workspaceDownload",
      },
    ],
  },
  {
    titleKey: "apiDocs.group.admin",
    endpoints: [
      { method: "GET/POST", path: "/wunder/admin/mcp", descKey: "apiDocs.endpoint.adminMcp" },
      { method: "GET/POST", path: "/wunder/admin/a2a", descKey: "apiDocs.endpoint.adminA2a" },
      { method: "GET/POST", path: "/wunder/admin/llm", descKey: "apiDocs.endpoint.adminLlm" },
      { method: "GET", path: "/wunder/admin/monitor", descKey: "apiDocs.endpoint.adminMonitor" },
    ],
  },
  {
    titleKey: "apiDocs.group.a2a",
    endpoints: [
      { method: "POST", path: "/a2a", descKey: "apiDocs.endpoint.a2a" },
      { method: "GET", path: "/.well-known/agent-card.json", descKey: "apiDocs.endpoint.agentCard" },
    ],
  },
];

const resolveApiBase = () => {
  const normalized = String(getWunderBase() || "").trim();
  if (normalized) {
    return normalized;
  }
  const fallback = String(APP_CONFIG.defaultApiBase || "").trim();
  return fallback || "/wunder";
};

const resolveUserId = () => {
  const current = String(elements.userId?.value || "").trim();
  if (current) {
    return current;
  }
  const fallback = String(APP_CONFIG.defaultUserId || "").trim();
  return fallback || "demo_user";
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

const renderEndpointGroups = () => {
  if (!elements.apiDocsEndpointGroups) {
    return;
  }
  elements.apiDocsEndpointGroups.textContent = "";
  ENDPOINT_GROUPS.forEach((group, index) => {
    const wrapper = document.createElement("details");
    wrapper.className = "api-docs-group-card";
    if (index === 0) {
      wrapper.open = true;
    }

    const summary = document.createElement("summary");
    summary.className = "api-docs-group-summary";
    const title = document.createElement("span");
    title.className = "api-docs-group-summary-title";
    title.textContent = t(group.titleKey);
    const count = document.createElement("span");
    count.className = "api-docs-group-summary-count";
    count.textContent = t("apiDocs.endpoint.count", { count: group.endpoints.length });
    summary.appendChild(title);
    summary.appendChild(count);
    wrapper.appendChild(summary);

    const groupBody = document.createElement("div");
    groupBody.className = "api-docs-group-body";

    group.endpoints.forEach((endpoint) => {
      const row = document.createElement("div");
      row.className = "api-docs-endpoint";
      const method = document.createElement("span");
      method.className = getMethodClass(endpoint.method);
      method.textContent = endpoint.method;
      const endpointBody = document.createElement("div");
      endpointBody.className = "api-docs-endpoint-body";
      const path = document.createElement("div");
      path.className = "api-docs-endpoint-path";
      path.textContent = endpoint.path;
      const desc = document.createElement("div");
      desc.className = "api-docs-endpoint-desc";
      desc.textContent = t(endpoint.descKey);
      endpointBody.appendChild(path);
      endpointBody.appendChild(desc);
      row.appendChild(method);
      row.appendChild(endpointBody);
      groupBody.appendChild(row);
    });
    wrapper.appendChild(groupBody);
    elements.apiDocsEndpointGroups.appendChild(wrapper);
  });
};

const renderExamples = () => {
  const base = resolveApiBase();
  const userId = resolveUserId();
  if (elements.apiDocsBaseUrl) {
    elements.apiDocsBaseUrl.textContent = base;
  }
  if (elements.apiDocsCurlStream) {
    elements.apiDocsCurlStream.textContent = t("apiDocs.example.curlStream", { base, userId });
  }
  if (elements.apiDocsCurlJson) {
    elements.apiDocsCurlJson.textContent = t("apiDocs.example.curlJson", { base, userId });
  }
  if (elements.apiDocsRequestExample) {
    elements.apiDocsRequestExample.textContent = t("apiDocs.example.requestBody", { userId });
  }
  if (elements.apiDocsEventExample) {
    elements.apiDocsEventExample.textContent = t("apiDocs.example.sseEvent");
  }
};

export const initApiDocsPanel = () => {
  if (!elements.apiDocsPanel) {
    return;
  }
  renderExamples();
  renderEndpointGroups();

  if (elements.apiBase) {
    elements.apiBase.addEventListener("change", renderExamples);
  }
  if (elements.userId) {
    elements.userId.addEventListener("change", renderExamples);
  }
  if (elements.promptUserId) {
    elements.promptUserId.addEventListener("change", renderExamples);
  }
  window.addEventListener("wunder:language-changed", () => {
    renderExamples();
    renderEndpointGroups();
  });
};


