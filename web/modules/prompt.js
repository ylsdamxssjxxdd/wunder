import { elements } from "./elements.js?v=20260214-01";
import { state } from "./state.js";
import { escapeHtml } from "./utils.js?v=20251229-02";
import { getWunderBase } from "./api.js";
import { notify } from "./notify.js";
import { t } from "./i18n.js?v=20260214-01";

const TEMPLATE_FILES = [
  { key: "role", labelKey: "prompt.template.file.role", defaultLabel: "角色定位" },
  { key: "engineering", labelKey: "prompt.template.file.engineering", defaultLabel: "工程指导" },
  { key: "tools_protocol", labelKey: "prompt.template.file.tools", defaultLabel: "工具协议" },
  { key: "skills_protocol", labelKey: "prompt.template.file.skills", defaultLabel: "技能协议" },
  { key: "memory", labelKey: "prompt.template.file.memory", defaultLabel: "历史记忆" },
  { key: "extra", labelKey: "prompt.template.file.extra", defaultLabel: "额外提示" },
];

const templateState = {
  loaded: false,
  packs: [],
  activePack: "default",
  selectedPack: "default",
  key: TEMPLATE_FILES[0].key,
  loadedContent: "",
};

const normalizePackId = (value) => {
  const raw = String(value || "").trim();
  return raw || "default";
};

const buildPromptTemplateBase = () => `${getWunderBase()}/admin/prompt_templates`;

const fetchPromptTemplateStatus = async () => {
  const endpoint = buildPromptTemplateBase();
  const response = await fetch(endpoint);
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const payload = await response.json();
  return payload?.data || {};
};

const fetchPromptTemplateFile = async (packId, key) => {
  const base = buildPromptTemplateBase();
  const endpoint = `${base}/file?pack_id=${encodeURIComponent(packId)}&key=${encodeURIComponent(
    key
  )}`;
  const response = await fetch(endpoint);
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const payload = await response.json();
  return payload?.data || {};
};

const savePromptTemplateFile = async (packId, key, content) => {
  const base = buildPromptTemplateBase();
  const response = await fetch(`${base}/file`, {
    method: "PUT",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ pack_id: packId, key, content }),
  });
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const payload = await response.json();
  return payload?.data || {};
};

const setActivePromptPack = async (packId) => {
  const base = buildPromptTemplateBase();
  const response = await fetch(`${base}/active`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ active: packId }),
  });
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const payload = await response.json();
  return payload?.data || {};
};

const createPromptPack = async (packId) => {
  const base = buildPromptTemplateBase();
  const response = await fetch(`${base}/packs`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ pack_id: packId, copy_from: "default" }),
  });
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const payload = await response.json();
  return payload?.data || {};
};

const deletePromptPack = async (packId) => {
  const base = buildPromptTemplateBase();
  const response = await fetch(`${base}/packs/${encodeURIComponent(packId)}`, { method: "DELETE" });
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const payload = await response.json();
  return payload?.data || {};
};

const hasUnsavedChanges = () => {
  if (!elements.promptTemplateEditor) {
    return false;
  }
  return elements.promptTemplateEditor.value !== templateState.loadedContent;
};

const confirmDiscardChanges = () => {
  if (!hasUnsavedChanges()) {
    return true;
  }
  return window.confirm(t("prompt.template.confirmDiscard"));
};

const setStatusText = (message) => {
  if (!elements.promptTemplateStatus) {
    return;
  }
  elements.promptTemplateStatus.textContent = message || "";
};

const renderPackSelect = () => {
  if (!elements.promptTemplatePack) {
    return;
  }
  elements.promptTemplatePack.textContent = "";
  templateState.packs.forEach((pack) => {
    const option = document.createElement("option");
    option.value = pack.id;
    option.textContent = pack.id;
    elements.promptTemplatePack.appendChild(option);
  });
  elements.promptTemplatePack.value = templateState.selectedPack;
  if (elements.promptTemplateSetActiveBtn) {
    const isActive = templateState.selectedPack === templateState.activePack;
    elements.promptTemplateSetActiveBtn.classList.toggle("is-active", isActive);
  }
  const isDefault = normalizePackId(templateState.selectedPack).toLowerCase() === "default";
  if (elements.promptTemplateSaveBtn) {
    elements.promptTemplateSaveBtn.disabled = isDefault;
  }
  if (elements.promptTemplateDeletePackBtn) {
    elements.promptTemplateDeletePackBtn.disabled = isDefault;
  }
};

const renderFileList = () => {
  if (!elements.promptTemplateFileList) {
    return;
  }
  elements.promptTemplateFileList.textContent = "";
  TEMPLATE_FILES.forEach((file) => {
    const row = document.createElement("div");
    row.className = "tool-item";
    if (file.key === templateState.key) {
      row.classList.add("is-active");
    }
    row.tabIndex = 0;
    row.setAttribute("role", "button");
    row.dataset.key = file.key;
    const label = document.createElement("label");
    const resolved = t(file.labelKey);
    const title = resolved && resolved !== file.labelKey ? resolved : file.defaultLabel;
    label.innerHTML = `<strong>${escapeHtml(title)}</strong><span class=\"muted\">${escapeHtml(
      file.key
    )}</span>`;
    row.appendChild(label);
    row.addEventListener("click", () => {
      if (!confirmDiscardChanges()) {
        return;
      }
      selectTemplateFile(file.key).catch(() => {});
    });
    row.addEventListener("keydown", (event) => {
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        row.click();
      }
    });
    elements.promptTemplateFileList.appendChild(row);
  });
};

const renderFileTitle = () => {
  if (!elements.promptTemplateFileTitle) {
    return;
  }
  const current = TEMPLATE_FILES.find((item) => item.key === templateState.key);
  if (!current) {
    elements.promptTemplateFileTitle.textContent = t("prompt.template.editor");
    return;
  }
  const resolved = t(current.labelKey);
  const title =
    resolved && resolved !== current.labelKey ? resolved : current.defaultLabel;
  elements.promptTemplateFileTitle.textContent = title;
};

const applyEditorContent = (text) => {
  if (!elements.promptTemplateEditor) {
    return;
  }
  elements.promptTemplateEditor.value = text || "";
};

const isDefaultTemplatePack = () => normalizePackId(templateState.selectedPack).toLowerCase() === "default";

const applyPackEditingState = () => {
  const isDefault = isDefaultTemplatePack();
  if (elements.promptTemplateEditor) {
    elements.promptTemplateEditor.readOnly = isDefault;
    elements.promptTemplateEditor.classList.toggle("is-readonly", isDefault);
  }
  if (elements.promptTemplateSaveBtn) {
    elements.promptTemplateSaveBtn.disabled = isDefault;
  }
  if (elements.promptTemplateDeletePackBtn) {
    elements.promptTemplateDeletePackBtn.disabled = isDefault;
  }
};

const loadTemplateStatus = async () => {
  const data = await fetchPromptTemplateStatus();
  const active = normalizePackId(data.active);
  const packs = Array.isArray(data.packs) ? data.packs : [];
  const ids = packs
    .map((item) => String(item?.id || "").trim())
    .filter(Boolean)
    .sort((a, b) => a.toLowerCase().localeCompare(b.toLowerCase()));
  if (!ids.includes("default")) {
    ids.unshift("default");
  }
  templateState.packs = ids.map((id) => ({ id }));
  templateState.activePack = active;
  if (!ids.includes(templateState.selectedPack)) {
    templateState.selectedPack = active && ids.includes(active) ? active : "default";
  }
  renderPackSelect();
};

const loadTemplateFile = async () => {
  const packId = templateState.selectedPack;
  const key = templateState.key;
  setStatusText(t("common.loading"));
  const data = await fetchPromptTemplateFile(packId, key);
  const content = typeof data.content === "string" ? data.content : "";
  templateState.loadedContent = content;
  applyEditorContent(content);
  renderFileTitle();
  renderFileList();
  applyPackEditingState();
  if (isDefaultTemplatePack()) {
    setStatusText(t("prompt.template.readonlyDefault"));
  } else {
    setStatusText(
      data.fallback_used ? t("prompt.template.fallbackHint", { pack: packId }) : ""
    );
  }
};

const selectTemplatePack = async (packId) => {
  templateState.selectedPack = normalizePackId(packId);
  renderPackSelect();
  await loadTemplateFile();
};

const selectTemplateFile = async (key) => {
  templateState.key = String(key || "").trim() || TEMPLATE_FILES[0].key;
  await loadTemplateFile();
};

// --- Prompt preview (built system prompt) ---

const SKILL_HEADERS = new Set(["[Mounted Skills]", "[已挂载技能]"]);

const renderToolHighlightLine = (line) => {
  const match = line.match(/"name"\s*:\s*"([^"]+)"/);
  const escapedLine = escapeHtml(line);
  if (!match) {
    return escapedLine;
  }
  const escapedMatch = escapeHtml(match[0]);
  const escapedName = escapeHtml(match[1]);
  const highlightedMatch = escapedMatch.replace(
    escapedName,
    `<span class=\"tool-highlight\">${escapedName}</span>`
  );
  return escapedLine.replace(escapedMatch, highlightedMatch);
};

const renderSystemPrompt = (rawText) => {
  if (!rawText) {
    return "";
  }
  const lines = rawText.split(/\r?\n/);
  const state = { inSkills: false };
  const output = lines.map((line) => {
    const trimmed = line.trim();
    if (SKILL_HEADERS.has(trimmed)) {
      state.inSkills = true;
      return escapeHtml(line);
    }
    if (trimmed.startsWith("[") && trimmed.endsWith("]") && !SKILL_HEADERS.has(trimmed)) {
      state.inSkills = false;
      return escapeHtml(line);
    }
    if (state.inSkills) {
      const match = line.match(/^(\s*-\s+)(.+)$/);
      if (match) {
        return `${escapeHtml(match[1])}<span class=\"skill-highlight\">${escapeHtml(
          match[2]
        )}</span>`;
      }
    }
    return renderToolHighlightLine(line);
  });
  return output.join("\n");
};

const updatePromptBuildTime = (value, options = {}) => {
  if (!elements.promptBuildTime) {
    return;
  }
  if (options.loading) {
    elements.promptBuildTime.textContent = t("prompt.buildTime.loading");
    return;
  }
  if (!Number.isFinite(value)) {
    elements.promptBuildTime.textContent = t("prompt.buildTime.empty");
    return;
  }
  const ms = Math.max(0, Number(value));
  const display = ms >= 1000 ? `${(ms / 1000).toFixed(2)} s` : `${ms.toFixed(2)} ms`;
  elements.promptBuildTime.textContent = t("prompt.buildTime.value", { duration: display });
};

export const loadSystemPrompt = async (options = {}) => {
  const showToast = Boolean(options.showToast);
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/system_prompt`;
  const payload = {
    user_id: String(elements.userId?.value || elements.promptUserId?.value || "").trim(),
    session_id: elements.sessionId?.value?.trim() || null,
  };
  elements.systemPrompt.textContent = t("common.loading");
  updatePromptBuildTime(null, { loading: true });
  try {
    const response = await fetch(endpoint, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(payload),
    });
    if (!response.ok) {
      throw new Error(t("common.requestFailed", { status: response.status }));
    }
    const result = await response.json();
    elements.systemPrompt.innerHTML = renderSystemPrompt(result.prompt || "");
    updatePromptBuildTime(result.build_time_ms);
    state.runtime.promptNeedsRefresh = false;
    if (showToast) {
      notify(t("prompt.refreshSuccess"), "success");
    }
  } catch (error) {
    elements.systemPrompt.textContent = t("prompt.requestError", { message: error.message });
    updatePromptBuildTime(null);
    if (showToast) {
      notify(t("prompt.refreshFailed", { message: error.message }), "error");
    }
  }
};

export const ensurePromptTemplatesLoaded = async () => {
  if (templateState.loaded) {
    return;
  }
  await loadTemplateStatus();
  await loadTemplateFile();
  templateState.loaded = true;
};

export const initPromptPanel = () => {
  state.runtime.promptReloadHandler = loadSystemPrompt;

  if (elements.loadPromptBtn) {
    elements.loadPromptBtn.addEventListener("click", () => loadSystemPrompt({ showToast: true }));
  }

  if (elements.promptTemplatePack) {
    elements.promptTemplatePack.addEventListener("change", () => {
      if (!confirmDiscardChanges()) {
        elements.promptTemplatePack.value = templateState.selectedPack;
        return;
      }
      selectTemplatePack(elements.promptTemplatePack.value).catch(() => {});
    });
  }

  if (elements.promptTemplateSaveBtn) {
    elements.promptTemplateSaveBtn.addEventListener("click", async () => {
      if (!elements.promptTemplateEditor) {
        return;
      }
      if (isDefaultTemplatePack()) {
        setStatusText(t("prompt.template.readonlyDefault"));
        notify(t("prompt.template.readonlyDefault"), "warn");
        return;
      }
      const content = elements.promptTemplateEditor.value;
      try {
        setStatusText(t("common.loading"));
        await savePromptTemplateFile(templateState.selectedPack, templateState.key, content);
        templateState.loadedContent = content;
        setStatusText(t("prompt.template.saved"));
        notify(t("prompt.template.saved"), "success");
        state.runtime.promptNeedsRefresh = true;
        loadSystemPrompt().catch(() => {});
      } catch (error) {
        setStatusText(t("prompt.template.saveFailed", { message: error.message }));
        notify(t("prompt.template.saveFailed", { message: error.message }), "error");
      }
    });
  }

  if (elements.promptTemplateSetActiveBtn) {
    elements.promptTemplateSetActiveBtn.addEventListener("click", async () => {
      try {
        const packId = templateState.selectedPack;
        await setActivePromptPack(packId);
        await loadTemplateStatus();
        notify(t("prompt.template.activeUpdated", { pack: packId }), "success");
        state.runtime.promptNeedsRefresh = true;
        loadSystemPrompt().catch(() => {});
      } catch (error) {
        notify(t("prompt.template.activeUpdateFailed", { message: error.message }), "error");
      }
    });
  }

  if (elements.promptTemplateNewPackBtn) {
    elements.promptTemplateNewPackBtn.addEventListener("click", async () => {
      if (!confirmDiscardChanges()) {
        return;
      }
      const value = window.prompt(t("prompt.template.newPackPrompt"));
      const packId = normalizePackId(value);
      if (!packId || packId === "default") {
        return;
      }
      try {
        await createPromptPack(packId);
        await loadTemplateStatus();
        await selectTemplatePack(packId);
        notify(t("prompt.template.packCreated", { pack: packId }), "success");
      } catch (error) {
        notify(t("prompt.template.packCreateFailed", { message: error.message }), "error");
      }
    });
  }

  if (elements.promptTemplateDeletePackBtn) {
    elements.promptTemplateDeletePackBtn.addEventListener("click", async () => {
      const packId = templateState.selectedPack;
      if (!packId || packId === "default") {
        return;
      }
      if (!window.confirm(t("prompt.template.confirmDeletePack", { pack: packId }))) {
        return;
      }
      try {
        await deletePromptPack(packId);
        templateState.selectedPack = "default";
        await loadTemplateStatus();
        await loadTemplateFile();
        notify(t("prompt.template.packDeleted", { pack: packId }), "success");
      } catch (error) {
        notify(t("prompt.template.packDeleteFailed", { message: error.message }), "error");
      }
    });
  }
};
