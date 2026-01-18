import { elements } from "./elements.js?v=20260118-04";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { formatTimestamp, isPlainObject } from "./utils.js?v=20251229-02";
import { notify } from "./notify.js";
import { appendLog } from "./log.js?v=20260108-02";
import { t } from "./i18n.js?v=20260118-06";
import { loadWorkspace } from "./workspace.js?v=20260118-04";

const normalizeLspConfig = (config) => {
  const raw = isPlainObject(config) ? config : {};
  const toNumber = (value) => {
    const parsed = Number(value);
    if (!Number.isFinite(parsed)) {
      return 0;
    }
    return Math.max(0, Math.round(parsed));
  };
  return {
    enabled: raw.enabled === true,
    timeout_s: toNumber(raw.timeout_s ?? raw.timeoutS),
    diagnostics_debounce_ms: toNumber(raw.diagnostics_debounce_ms ?? raw.diagnosticsDebounceMs),
    idle_ttl_s: toNumber(raw.idle_ttl_s ?? raw.idleTtlS),
    servers: Array.isArray(raw.servers) ? raw.servers : [],
  };
};

const LSP_TEMPLATES = [
  {
    id: "fullstack",
    labelKey: "lsp.template.fullstack",
    servers: [
      {
        id: "clangd",
        name: "Clangd",
        command: ["clangd"],
        extensions: ["c", "h", "cpp", "hpp", "cc", "cxx", "hh"],
        root_markers: ["compile_commands.json", "compile_flags.txt", "CMakeLists.txt", "Makefile"],
        enabled: true,
      },
      {
        id: "rust-analyzer",
        name: "Rust Analyzer",
        command: ["rust-analyzer"],
        extensions: ["rs"],
        root_markers: ["Cargo.toml"],
        enabled: true,
      },
      {
        id: "pyright",
        name: "Pyright",
        command: ["pyright-langserver", "--stdio"],
        extensions: ["py"],
        root_markers: ["pyproject.toml", "setup.py", "requirements.txt"],
        enabled: true,
      },
      {
        id: "vue",
        name: "Volar",
        command: ["vue-language-server", "--stdio"],
        extensions: ["vue"],
        root_markers: ["package.json", "pnpm-lock.yaml", "yarn.lock", "package-lock.json"],
        enabled: true,
      },
      {
        id: "ts",
        name: "TypeScript",
        command: ["typescript-language-server", "--stdio"],
        extensions: ["ts", "tsx", "js", "jsx"],
        root_markers: ["tsconfig.json", "package.json"],
        enabled: true,
      },
      {
        id: "json",
        name: "JSON LS",
        command: ["vscode-json-language-server", "--stdio"],
        extensions: ["json", "jsonc"],
        root_markers: ["package.json", ".git"],
        enabled: true,
      },
      {
        id: "html",
        name: "HTML LS",
        command: ["vscode-html-language-server", "--stdio"],
        extensions: ["html", "htm"],
        root_markers: ["package.json", ".git"],
        enabled: true,
      },
      {
        id: "css",
        name: "CSS LS",
        command: ["vscode-css-language-server", "--stdio"],
        extensions: ["css", "scss", "less"],
        root_markers: ["package.json", ".git"],
        enabled: true,
      },
      {
        id: "bash",
        name: "Bash LS",
        command: ["bash-language-server", "start"],
        extensions: ["sh", "bash", "zsh"],
        root_markers: [".git"],
        enabled: true,
      },
      {
        id: "dockerfile",
        name: "Dockerfile LS",
        command: ["docker-langserver", "--stdio"],
        extensions: ["dockerfile"],
        root_markers: ["Dockerfile", ".git"],
        enabled: true,
      },
    ],
  },
  {
    id: "frontend",
    labelKey: "lsp.template.frontend",
    servers: [
      {
        id: "vue",
        name: "Volar",
        command: ["vue-language-server", "--stdio"],
        extensions: ["vue"],
        root_markers: ["package.json", "pnpm-lock.yaml", "yarn.lock", "package-lock.json"],
        enabled: true,
      },
      {
        id: "ts",
        name: "TypeScript",
        command: ["typescript-language-server", "--stdio"],
        extensions: ["ts", "tsx", "js", "jsx"],
        root_markers: ["tsconfig.json", "package.json"],
        enabled: true,
      },
      {
        id: "json",
        name: "JSON LS",
        command: ["vscode-json-language-server", "--stdio"],
        extensions: ["json", "jsonc"],
        root_markers: ["package.json", ".git"],
        enabled: true,
      },
      {
        id: "html",
        name: "HTML LS",
        command: ["vscode-html-language-server", "--stdio"],
        extensions: ["html", "htm"],
        root_markers: ["package.json", ".git"],
        enabled: true,
      },
      {
        id: "css",
        name: "CSS LS",
        command: ["vscode-css-language-server", "--stdio"],
        extensions: ["css", "scss", "less"],
        root_markers: ["package.json", ".git"],
        enabled: true,
      },
    ],
  },
  {
    id: "backend",
    labelKey: "lsp.template.backend",
    servers: [
      {
        id: "rust-analyzer",
        name: "Rust Analyzer",
        command: ["rust-analyzer"],
        extensions: ["rs"],
        root_markers: ["Cargo.toml"],
        enabled: true,
      },
      {
        id: "pyright",
        name: "Pyright",
        command: ["pyright-langserver", "--stdio"],
        extensions: ["py"],
        root_markers: ["pyproject.toml", "setup.py", "requirements.txt"],
        enabled: true,
      },
    ],
  },
  {
    id: "cpp",
    labelKey: "lsp.template.cpp",
    servers: [
      {
        id: "clangd",
        name: "Clangd",
        command: ["clangd"],
        extensions: ["c", "h", "cpp", "hpp", "cc", "cxx", "hh"],
        root_markers: ["compile_commands.json", "compile_flags.txt", "CMakeLists.txt", "Makefile"],
        enabled: true,
      },
    ],
  },
  {
    id: "ops",
    labelKey: "lsp.template.ops",
    servers: [
      {
        id: "bash",
        name: "Bash LS",
        command: ["bash-language-server", "start"],
        extensions: ["sh", "bash", "zsh"],
        root_markers: [".git"],
        enabled: true,
      },
      {
        id: "dockerfile",
        name: "Dockerfile LS",
        command: ["docker-langserver", "--stdio"],
        extensions: ["dockerfile"],
        root_markers: ["Dockerfile", ".git"],
        enabled: true,
      },
    ],
  },
];

let lspWorkspaceActive = false;
let lspWorkspaceTriggering = false;
let lastLspPath = "";
let lastLspResultAt = 0;
let lastLspOutput = null;

const moveWorkspaceBlock = (target) => {
  const block = elements.workspaceSharedBlock;
  if (!block || !target) {
    return;
  }
  if (block.parentElement === target) {
    return;
  }
  target.appendChild(block);
};

const setLspWorkspaceStatus = (message, status) => {
  if (!elements.lspWorkspaceStatus) {
    return;
  }
  elements.lspWorkspaceStatus.textContent = message || "";
  elements.lspWorkspaceStatus.classList.remove("is-success", "is-error", "is-warning");
  if (status === "success") {
    elements.lspWorkspaceStatus.classList.add("is-success");
  } else if (status === "error") {
    elements.lspWorkspaceStatus.classList.add("is-error");
  } else if (status === "warning") {
    elements.lspWorkspaceStatus.classList.add("is-warning");
  }
};

const setLspResultStatus = (message, status) => {
  if (!elements.lspTestStatus) {
    return;
  }
  elements.lspTestStatus.textContent = message || "";
  elements.lspTestStatus.classList.remove("is-success", "is-error", "is-warning");
  if (status === "success") {
    elements.lspTestStatus.classList.add("is-success");
  } else if (status === "error") {
    elements.lspTestStatus.classList.add("is-error");
  } else if (status === "warning") {
    elements.lspTestStatus.classList.add("is-warning");
  }
};

const setLspResultOutput = (value) => {
  if (!elements.lspTestResult) {
    return;
  }
  if (value === null || value === undefined || value === "") {
    elements.lspTestResult.textContent = "";
    return;
  }
  if (typeof value === "string") {
    elements.lspTestResult.textContent = value;
    return;
  }
  try {
    elements.lspTestResult.textContent = JSON.stringify(value, null, 2);
  } catch (error) {
    elements.lspTestResult.textContent = String(value);
  }
};

const updateLspResultMeta = (path, timestamp) => {
  if (!elements.lspResultMeta) {
    return;
  }
  if (!path) {
    elements.lspResultMeta.textContent = "";
    return;
  }
  const time = formatTimestamp(timestamp || Date.now());
  elements.lspResultMeta.textContent = t("lsp.result.meta", { path, time });
};

const resetLspResult = () => {
  setLspResultStatus("", "");
  if (lastLspOutput !== null && lastLspOutput !== undefined) {
    setLspResultOutput(lastLspOutput);
  } else {
    setLspResultOutput(t("lsp.result.empty"));
  }
  updateLspResultMeta(lastLspPath, lastLspResultAt);
};

const updateLspStatusIndicator = () => {
  if (!elements.lspStatusIndicator) {
    return;
  }
  const items = Array.isArray(state.lsp.status) ? state.lsp.status : [];
  const total = items.length;
  const connected = items.filter((item) => {
    const status = String(item?.status || "").trim().toLowerCase();
    return status === "connected";
  }).length;
  let status = "idle";
  let label = t("lsp.status.summary.empty");
  if (total === 0) {
    status = "idle";
    label = t("lsp.status.summary.empty");
  } else if (connected > 0) {
    status = "active";
    label = t("lsp.status.summary.connected", { connected, total });
  } else {
    status = "error";
    label = t("lsp.status.summary.error");
  }
  elements.lspStatusIndicator.dataset.status = status;
  elements.lspStatusIndicator.classList.toggle("is-active", status === "active");
  if (elements.lspStatusIndicatorText) {
    elements.lspStatusIndicatorText.textContent = label;
  }
};

const getLspWorkspaceUserId = () => {
  const userId = String(elements.lspTestUserId?.value || "").trim();
  if (userId) {
    return userId;
  }
  return String(elements.userId?.value || "").trim();
};

const syncLspWorkspace = async () => {
  const userId = String(elements.lspTestUserId?.value || "").trim();
  if (!userId) {
    setLspWorkspaceStatus(t("lsp.test.error.userIdRequired"), "warning");
    notify(t("lsp.test.error.userIdRequired"), "warn");
    return;
  }
  if (elements.userId) {
    elements.userId.value = userId;
  }
  setLspWorkspaceStatus(t("lsp.workspace.status.loading"), "warning");
  const result = await loadWorkspace({ refreshTree: true, resetExpanded: true, resetSearch: true });
  if (!result?.ok) {
    const message = result?.error || t("common.loadFailed");
    setLspWorkspaceStatus(t("lsp.workspace.status.loadFailed", { message }), "error");
    return;
  }
  setLspWorkspaceStatus(t("lsp.workspace.status.synced"), "success");
};

const requestLspTest = async (payload) => {
  const wunderBase = getWunderBase();
  const response = await fetch(`${wunderBase}/admin/lsp/test`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify(payload),
  });
  const result = await response.json().catch(() => ({}));
  if (!response.ok) {
    const message =
      result?.detail?.message ||
      result?.detail?.code ||
      result?.detail ||
      response.statusText;
    throw new Error(message || t("common.requestFailed", { status: response.status }));
  }
  return result;
};

const triggerLspForPath = async (path) => {
  if (!lspWorkspaceActive) {
    return;
  }
  if (!elements.lspWorkspaceAutoTrigger?.checked) {
    return;
  }
  if (!path) {
    return;
  }
  if (state.lsp.config && !state.lsp.config.enabled) {
    setLspWorkspaceStatus(t("lsp.workspace.status.lspDisabled"), "warning");
    setLspResultStatus(t("lsp.workspace.status.lspDisabled"), "warning");
    return;
  }
  if (lspWorkspaceTriggering) {
    return;
  }
  const userId = getLspWorkspaceUserId();
  if (!userId) {
    setLspWorkspaceStatus(t("lsp.test.error.userIdRequired"), "warning");
    setLspResultStatus(t("lsp.test.error.userIdRequired"), "warning");
    return;
  }
  lspWorkspaceTriggering = true;
  lastLspPath = path;
  lastLspResultAt = Date.now();
  updateLspResultMeta(path, lastLspResultAt);
  setLspWorkspaceStatus(t("lsp.workspace.status.lspRunning"), "warning");
  setLspResultStatus(t("lsp.result.running"), "warning");
  try {
    const result = await requestLspTest({
      user_id: userId,
      path,
      operation: "documentSymbol",
    });
    lastLspOutput = result;
    setLspResultOutput(result);
    setLspResultStatus(t("lsp.result.success"), "success");
    setLspWorkspaceStatus(t("lsp.workspace.status.lspDone"), "success");
  } catch (error) {
    const message = error?.message || String(error);
    lastLspOutput = message;
    setLspResultOutput(message);
    setLspResultStatus(t("lsp.result.failed", { message }), "error");
    setLspWorkspaceStatus(t("lsp.workspace.status.lspFailed", { message }), "error");
  } finally {
    lspWorkspaceTriggering = false;
  }
};

const handleWorkspaceFileSaved = (event) => {
  if (!lspWorkspaceActive) {
    return;
  }
  const detail = event?.detail || {};
  const path = String(detail.path || "").trim();
  if (!path) {
    return;
  }
  lastLspPath = path;
  updateLspResultMeta(path, Date.now());
  triggerLspForPath(path);
};

const resolveTemplate = () => {
  const templateId = String(elements.lspTemplateSelect?.value || "").trim();
  if (!templateId) {
    return null;
  }
  return LSP_TEMPLATES.find((item) => item.id === templateId) || null;
};

const applyTemplate = (template) => {
  if (!elements.lspServers) {
    return;
  }
  const payload = JSON.stringify(template?.servers || [], null, 2);
  const current = String(elements.lspServers.value || "").trim();
  if (current && current !== payload) {
    const confirmed = window.confirm(t("lsp.template.confirm"));
    if (!confirmed) {
      return;
    }
  }
  elements.lspServers.value = payload;
  notify(t("lsp.template.applied", { name: t(template.labelKey) }), "success");
};

const applyLspConfigForm = (config) => {
  if (!config) {
    return;
  }
  if (elements.lspEnabled) {
    elements.lspEnabled.checked = Boolean(config.enabled);
  }
  if (elements.lspTimeout) {
    elements.lspTimeout.value = config.timeout_s ? String(config.timeout_s) : "";
  }
  if (elements.lspDiagnosticsDebounce) {
    elements.lspDiagnosticsDebounce.value = config.diagnostics_debounce_ms
      ? String(config.diagnostics_debounce_ms)
      : "";
  }
  if (elements.lspIdleTtl) {
    elements.lspIdleTtl.value = config.idle_ttl_s ? String(config.idle_ttl_s) : "";
  }
  if (elements.lspServers) {
    elements.lspServers.value = JSON.stringify(config.servers || [], null, 2);
  }
};

const collectNumberInput = (input) => {
  const raw = String(input?.value || "").trim();
  if (!raw) {
    return { value: 0, error: "" };
  }
  const parsed = Number(raw);
  if (!Number.isFinite(parsed) || parsed < 0) {
    return { value: 0, error: t("lsp.error.invalidNumber") };
  }
  return { value: Math.round(parsed), error: "" };
};

const collectServers = () => {
  const raw = String(elements.lspServers?.value || "").trim();
  if (!raw) {
    return { servers: [], error: "" };
  }
  try {
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) {
      return { servers: [], error: t("lsp.error.invalidServersJson") };
    }
    return { servers: parsed, error: "" };
  } catch (error) {
    return { servers: [], error: t("lsp.error.invalidServersJson") };
  }
};

const collectLspConfigForm = () => {
  const timeout = collectNumberInput(elements.lspTimeout);
  if (timeout.error) {
    return { config: null, error: timeout.error };
  }
  const debounce = collectNumberInput(elements.lspDiagnosticsDebounce);
  if (debounce.error) {
    return { config: null, error: debounce.error };
  }
  const idle = collectNumberInput(elements.lspIdleTtl);
  if (idle.error) {
    return { config: null, error: idle.error };
  }
  const servers = collectServers();
  if (servers.error) {
    return { config: null, error: servers.error };
  }
  return {
    config: {
      enabled: Boolean(elements.lspEnabled?.checked),
      timeout_s: timeout.value,
      diagnostics_debounce_ms: debounce.value,
      idle_ttl_s: idle.value,
      servers: servers.servers,
    },
    error: "",
  };
};

const renderLspStatus = () => {
  if (!elements.lspStatusList) {
    return;
  }
  elements.lspStatusList.textContent = "";
  const items = Array.isArray(state.lsp.status) ? state.lsp.status : [];
  if (!items.length) {
    elements.lspStatusList.textContent = t("lsp.status.empty");
    return;
  }
  items.forEach((item) => {
    const wrapper = document.createElement("div");
    wrapper.className = "list-item";
    const header = document.createElement("div");
    header.className = "lsp-status-item-header";
    const title = document.createElement("div");
    title.className = "lsp-status-item-title";
    const name = String(item?.server_name || item?.server_id || "-");
    const serverId = String(item?.server_id || "").trim();
    title.textContent = serverId && serverId !== name ? `${name} (${serverId})` : name;
    const indicator = document.createElement("div");
    indicator.className = "status-indicator";
    const status = String(item?.status || "").trim().toLowerCase();
    const statusKey = status === "connected" ? "active" : status || "idle";
    indicator.dataset.status = statusKey;
    indicator.innerHTML = `<span class="status-dot"></span><span>${
      status === "connected" ? t("lsp.status.connected") : t("lsp.status.error")
    }</span>`;
    header.appendChild(title);
    header.appendChild(indicator);
    const meta = document.createElement("small");
    const lastUsed = item?.last_used_at ? formatTimestamp(item.last_used_at * 1000) : "-";
    meta.textContent = t("lsp.status.meta", {
      user_id: String(item?.user_id || "-"),
      root: String(item?.root || "-"),
      last_used: lastUsed,
    });
    wrapper.appendChild(header);
    wrapper.appendChild(meta);
    elements.lspStatusList.appendChild(wrapper);
  });
};

const syncTestUserId = () => {
  if (!elements.lspTestUserId || elements.lspTestUserId.value.trim()) {
    return;
  }
  const fallback = String(elements.userId?.value || elements.settingsDefaultUserId?.value || "").trim();
  if (fallback) {
    elements.lspTestUserId.value = fallback;
  }
};

const openLspStatusModal = () => {
  if (!elements.lspStatusModal) {
    return;
  }
  elements.lspStatusModal.classList.add("active");
  renderLspStatus();
};

const closeLspStatusModal = () => {
  elements.lspStatusModal?.classList.remove("active");
};

const openLspConfigModal = async () => {
  if (!elements.lspConfigModal) {
    return;
  }
  if (!state.lsp.config) {
    try {
      await loadLspConfig();
    } catch (error) {
      notify(t("lsp.refresh.failed", { message: error.message }), "error");
    }
  }
  elements.lspConfigModal.classList.add("active");
};

const closeLspConfigModal = () => {
  elements.lspConfigModal?.classList.remove("active");
};

export const loadLspConfig = async () => {
  const wunderBase = getWunderBase();
  const response = await fetch(`${wunderBase}/admin/lsp`);
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const payload = await response.json();
  state.lsp.config = normalizeLspConfig(payload?.lsp || {});
  state.lsp.status = Array.isArray(payload?.status) ? payload.status : [];
  applyLspConfigForm(state.lsp.config);
  renderLspStatus();
  updateLspStatusIndicator();
  syncTestUserId();
};

const saveLspConfig = async () => {
  const { config, error } = collectLspConfigForm();
  if (error) {
    throw new Error(error);
  }
  const wunderBase = getWunderBase();
  const response = await fetch(`${wunderBase}/admin/lsp`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ lsp: config }),
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
  state.lsp.config = normalizeLspConfig(payload?.lsp || {});
  state.lsp.status = Array.isArray(payload?.status) ? payload.status : [];
  applyLspConfigForm(state.lsp.config);
  renderLspStatus();
  updateLspStatusIndicator();
};

export const onLspPanelActivate = () => {
  lspWorkspaceActive = true;
  moveWorkspaceBlock(elements.lspWorkspaceMount);
  syncTestUserId();
  resetLspResult();
};

export const onLspPanelDeactivate = () => {
  lspWorkspaceActive = false;
  moveWorkspaceBlock(elements.debugWorkspaceMount);
  closeLspStatusModal();
  closeLspConfigModal();
};

export const initLspPanel = () => {
  if (elements.lspStatusIndicator) {
    elements.lspStatusIndicator.addEventListener("click", openLspStatusModal);
  }
  if (elements.lspStatusBtn) {
    elements.lspStatusBtn.addEventListener("click", openLspStatusModal);
  }
  if (elements.lspConfigBtn) {
    elements.lspConfigBtn.addEventListener("click", () => {
      openLspConfigModal().catch(() => {});
    });
  }
  if (elements.lspStatusModalClose) {
    elements.lspStatusModalClose.addEventListener("click", closeLspStatusModal);
  }
  if (elements.lspStatusModalCloseBtn) {
    elements.lspStatusModalCloseBtn.addEventListener("click", closeLspStatusModal);
  }
  if (elements.lspStatusModal) {
    elements.lspStatusModal.addEventListener("click", (event) => {
      if (event.target === elements.lspStatusModal) {
        closeLspStatusModal();
      }
    });
  }
  if (elements.lspConfigModalClose) {
    elements.lspConfigModalClose.addEventListener("click", closeLspConfigModal);
  }
  if (elements.lspConfigModalCloseBtn) {
    elements.lspConfigModalCloseBtn.addEventListener("click", closeLspConfigModal);
  }
  if (elements.lspConfigModal) {
    elements.lspConfigModal.addEventListener("click", (event) => {
      if (event.target === elements.lspConfigModal) {
        closeLspConfigModal();
      }
    });
  }
  if (elements.lspRefreshBtn) {
    elements.lspRefreshBtn.addEventListener("click", async () => {
      try {
        await loadLspConfig();
        notify(t("lsp.refresh.success"), "success");
      } catch (error) {
        notify(t("lsp.refresh.failed", { message: error.message }), "error");
      }
    });
  }
  if (elements.lspSaveBtn) {
    elements.lspSaveBtn.addEventListener("click", async () => {
      try {
        await saveLspConfig();
        appendLog(t("lsp.save.success"));
        notify(t("lsp.save.success"), "success");
      } catch (error) {
        notify(t("lsp.save.failed", { message: error.message }), "error");
      }
    });
  }
  if (elements.lspTemplateApplyBtn) {
    elements.lspTemplateApplyBtn.addEventListener("click", () => {
      const template = resolveTemplate();
      if (!template) {
        notify(t("lsp.template.error.required"), "error");
        return;
      }
      applyTemplate(template);
    });
  }
  if (elements.lspWorkspaceSyncBtn) {
    elements.lspWorkspaceSyncBtn.addEventListener("click", () => {
      syncLspWorkspace().catch(() => {});
    });
  }
  if (elements.lspTestUserId) {
    elements.lspTestUserId.addEventListener("keydown", (event) => {
      if (event.key === "Enter") {
        event.preventDefault();
        syncLspWorkspace().catch(() => {});
      }
    });
  }
  document.addEventListener("wunder:workspace-file-saved", handleWorkspaceFileSaved);
  window.addEventListener("wunder:language-changed", () => {
    updateLspStatusIndicator();
    resetLspResult();
  });
  syncTestUserId();
  updateLspStatusIndicator();
  resetLspResult();
};
