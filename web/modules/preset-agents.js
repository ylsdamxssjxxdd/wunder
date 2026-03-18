import { elements } from "./elements.js?v=20260215-01";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { appendLog } from "./log.js?v=20260215-01";
import { notify } from "./notify.js";
import { t } from "./i18n.js?v=20260215-01";

const TAB_KEYS = ["preset", "cron", "channels"];
const DEFAULT_AGENT_ID_ALIAS = "__default__";
const TEMPLATE_USER_ID = "preset_template";
const DEFAULT_HIVE_ID = "default";

const ensureState = () => {
  if (!state.presetAgents) {
    state.presetAgents = {
      presets: [],
      selectedPresetName: "",
      activeTab: "preset",
      userAgent: null,
      syncSummary: null,
      syncLoading: false,
      syncRequestToken: 0,
      toolGroups: [],
      modelOptions: [],
      defaultModelName: "",
      cronJobs: [],
      channelAccounts: [],
      supportedChannels: [],
      loading: false,
      initialized: false,
    };
  }
  if (!state.panelLoaded) {
    state.panelLoaded = {};
  }
  if (typeof state.panelLoaded.presetAgents !== "boolean") {
    state.panelLoaded.presetAgents = false;
  }
};

const REQUIRED_KEYS = [
  "presetAgentsPanel",
  "presetAgentsRefreshBtn",
  "presetAgentCreateBtn",
  "presetAgentList",
  "presetAgentDetailTitle",
  "presetAgentDetailMeta",
  "presetAgentSaveBtn",
  "presetAgentSyncSafeBtn",
  "presetAgentSyncForceBtn",
  "presetAgentSyncSummary",
  "presetAgentDeleteBtn",
  "presetAgentsStatusText",
  "presetAgentTabPreset",
  "presetAgentTabCron",
  "presetAgentTabChannels",
  "presetAgentTabContentPreset",
  "presetAgentTabContentCron",
  "presetAgentTabContentChannels",
  "presetAgentFormName",
  "presetAgentFormDescription",
  "presetAgentFormPrompt",
  "presetAgentFormModelName",
  "presetAgentPresetQuestions",
  "presetAgentPresetQuestionsEmpty",
  "presetAgentPresetQuestionAddBtn",
  "presetAgentFormContainerId",
  "presetUserAgentTools",
  "presetUserAgentToolsEmpty",
  "presetUserAgentApproval",
  "presetUserAgentHive",
  "presetCronList",
  "presetCronJobId",
  "presetCronName",
  "presetCronScheduleText",
  "presetCronMessage",
  "presetCronEnabled",
  "presetCronSaveBtn",
  "presetChannelsAccountList",
  "presetChannelFormChannel",
  "presetChannelFormAccountId",
  "presetChannelFormPeerKind",
  "presetChannelFormEnabled",
  "presetChannelFormAccountName",
  "presetChannelFormConfig",
  "presetChannelSaveBtn",
];

const ensureElements = () => {
  const missing = REQUIRED_KEYS.filter((key) => !elements[key]);
  if (!missing.length) {
    return true;
  }
  appendLog(t("presetAgents.domMissing", { nodes: missing.join(", ") }));
  return false;
};

const toQueryString = (params = {}) => {
  const search = new URLSearchParams();
  Object.entries(params).forEach(([key, value]) => {
    if (value === undefined || value === null || value === "") {
      return;
    }
    search.set(key, String(value));
  });
  const encoded = search.toString();
  return encoded ? `?${encoded}` : "";
};

const requestJson = async (path, { method = "GET", body, query } = {}) => {
  const response = await fetch(getWunderBase() + path + toQueryString(query), {
    method,
    headers: { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const payload = await response.json().catch(() => ({}));
  if (!response.ok) {
    const message =
      payload?.error?.message || payload?.detail?.message || payload?.detail || payload?.message || String(response.status);
    throw new Error(message);
  }
  return payload;
};

const normalizeIconName = (value) => String(value || "").trim() || "spark";

const normalizeIconColor = (value) => {
  const cleaned = String(value || "").trim();
  const match = cleaned.match(/^#?([0-9a-fA-F]{3}|[0-9a-fA-F]{6})$/);
  if (!match) {
    return "#94a3b8";
  }
  let hex = match[1].toLowerCase();
  if (hex.length === 3) {
    hex = hex.split("").map((part) => part + part).join("");
  }
  return "#" + hex;
};

const normalizeQuestionDrafts = (values) =>
  Array.isArray(values) ? values.map((value) => String(value ?? "")) : [];

const normalizeQuestionList = (values) => {
  const seen = new Set();
  const output = [];
  normalizeQuestionDrafts(values).forEach((value) => {
    const cleaned = String(value || "").trim();
    if (!cleaned || seen.has(cleaned)) {
      return;
    }
    seen.add(cleaned);
    output.push(cleaned);
  });
  return output;
};

const normalizeOptionalModelName = (value) => {
  const cleaned = String(value || "").trim();
  return cleaned || "";
};

const normalizeModelType = (value) => {
  const raw = String(value || "").trim().toLowerCase();
  if (!raw) {
    return "llm";
  }
  if (raw === "embed" || raw === "embeddings") {
    return "embedding";
  }
  return raw === "embedding" ? "embedding" : "llm";
};

const resolveDefaultModelDisplayName = () => {
  const configured = normalizeOptionalModelName(state.presetAgents?.defaultModelName);
  return configured || t("presetAgents.form.modelDefaultName");
};

const extractLlmModelCatalog = (payload) => {
  const root =
    payload?.llm && typeof payload.llm === "object"
      ? payload.llm
      : payload?.data?.llm && typeof payload.data.llm === "object"
        ? payload.data.llm
        : {};
  const rawModels = root?.models && typeof root.models === "object" ? root.models : {};
  const names = Object.keys(rawModels)
    .map((name) => String(name || "").trim())
    .filter(Boolean);
  const options = names.filter((name) => normalizeModelType(rawModels?.[name]?.model_type) === "llm");
  const requestedDefault = normalizeOptionalModelName(root?.default);
  const defaultModelName =
    (requestedDefault && options.includes(requestedDefault) && requestedDefault) ||
    options[0] ||
    "";
  return { options, defaultModelName };
};

const renderModelOptions = (selectedModelName = "") => {
  const select = elements.presetAgentFormModelName;
  if (!select) {
    return;
  }
  const options = Array.isArray(state.presetAgents.modelOptions) ? state.presetAgents.modelOptions : [];
  const selected = normalizeOptionalModelName(selectedModelName || select.value);
  select.textContent = "";

  const defaultOption = document.createElement("option");
  defaultOption.value = "";
  defaultOption.textContent = t("presetAgents.form.modelDefaultOption", {
    name: resolveDefaultModelDisplayName(),
  });
  select.appendChild(defaultOption);

  options.forEach((name) => {
    const value = normalizeOptionalModelName(name);
    if (!value) {
      return;
    }
    const option = document.createElement("option");
    option.value = value;
    option.textContent = value;
    select.appendChild(option);
  });

  if (selected && options.includes(selected)) {
    select.value = selected;
  } else {
    select.value = "";
  }
};

const normalizePreset = (item) => ({
  preset_id: String(item?.preset_id || "").trim(),
  is_default_agent: item?.is_default_agent === true,
  revision: Number.isFinite(Number(item?.revision)) ? Number(item.revision) : 1,
  name: String(item?.name || "").trim(),
  description: String(item?.description || "").trim(),
  system_prompt: String(item?.system_prompt || "").trim(),
  model_name: normalizeOptionalModelName(item?.model_name || item?.modelName),
  icon_name: normalizeIconName(item?.icon_name),
  icon_color: normalizeIconColor(item?.icon_color),
  sandbox_container_id: Number.isFinite(Number(item?.sandbox_container_id)) ? Number(item.sandbox_container_id) : 1,
  tool_names: Array.isArray(item?.tool_names)
    ? item.tool_names.map((value) => String(value || "").trim()).filter(Boolean)
    : [],
  declared_tool_names: Array.isArray(item?.declared_tool_names)
    ? item.declared_tool_names.map((value) => String(value || "").trim()).filter(Boolean)
    : [],
  declared_skill_names: Array.isArray(item?.declared_skill_names)
    ? item.declared_skill_names.map((value) => String(value || "").trim()).filter(Boolean)
    : [],
  preset_questions: normalizeQuestionList(item?.preset_questions),
  approval_mode: String(item?.approval_mode || "full_auto").trim() || "full_auto",
  status: String(item?.status || "active").trim() || "active",
});

const normalizeUserAgent = (item) => ({
  id: String(item?.id || item?.agent_id || "").trim(),
  name: String(item?.name || "").trim(),
  description: String(item?.description || "").trim(),
  system_prompt: String(item?.system_prompt || "").trim(),
  configured_model_name: normalizeOptionalModelName(item?.configured_model_name || item?.configuredModelName),
  model_name: normalizeOptionalModelName(item?.model_name || item?.modelName),
  tool_names: Array.isArray(item?.tool_names)
    ? item.tool_names.map((value) => String(value || "").trim()).filter(Boolean)
    : [],
  declared_tool_names: Array.isArray(item?.declared_tool_names)
    ? item.declared_tool_names.map((value) => String(value || "").trim()).filter(Boolean)
    : [],
  declared_skill_names: Array.isArray(item?.declared_skill_names)
    ? item.declared_skill_names.map((value) => String(value || "").trim()).filter(Boolean)
    : [],
  preset_questions: normalizeQuestionList(item?.preset_questions),
  approval_mode: String(item?.approval_mode || "full_auto").trim() || "full_auto",
  status: String(item?.status || "active").trim() || "active",
  icon: item?.icon || null,
  sandbox_container_id: Number.isFinite(Number(item?.sandbox_container_id)) ? Number(item.sandbox_container_id) : 1,
  updated_at: item?.updated_at || "",
});

const normalizeCronJob = (item) => ({
  job_id: String(item?.job_id || "").trim(),
  name: String(item?.name || "").trim(),
  agent_id: String(item?.agent_id || "").trim(),
  enabled: item?.enabled !== false,
  schedule_text: String(item?.schedule_text || "").trim(),
  payload: item?.payload && typeof item.payload === "object" ? item.payload : {},
  next_run_at_text: String(item?.next_run_at_text || "").trim(),
  running: item?.running === true,
});

const normalizeChannelAccount = (item) => ({
  channel: String(item?.channel || "").trim(),
  account_id: String(item?.account_id || "").trim(),
  peer_kind: String(item?.peer_kind || "group").trim() || "group",
  status: String(item?.status || "active").trim() || "active",
  config: item?.config && typeof item.config === "object" ? item.config : {},
});

const normalizeChannelBinding = (item) => ({
  channel: String(item?.channel || "").trim(),
  account_id: String(item?.account_id || "").trim(),
  agent_id: String(item?.agent_id || "").trim(),
  enabled: item?.enabled !== false,
});

const buildToolOptions = (list) =>
  (Array.isArray(list) ? list : [])
    .map((item) => {
      if (!item) return null;
      const name = item.name || item.tool_name || item.toolName;
      if (!name) return null;
      return {
        value: String(name),
        label: String(name),
        description: String(item.description || ""),
      };
    })
    .filter(Boolean);

const buildToolGroups = (payload) => [
  { label: t("userAccounts.toolGroup.builtin"), options: buildToolOptions(payload.builtin_tools) },
  { label: t("userAccounts.toolGroup.mcp"), options: buildToolOptions(payload.mcp_tools) },
  { label: t("userAccounts.toolGroup.a2a"), options: buildToolOptions(payload.a2a_tools) },
  { label: t("userAccounts.toolGroup.skills"), options: buildToolOptions(payload.skills) },
  { label: t("userAccounts.toolGroup.knowledge"), options: buildToolOptions(payload.knowledge_tools) },
  { label: t("userAccounts.toolGroup.user"), options: buildToolOptions(payload.user_tools) },
  { label: t("userAccounts.toolGroup.shared"), options: buildToolOptions(payload.shared_tools) },
].filter((group) => group.options.length > 0);

const collectSelectedTools = (list) => {
  if (!list) {
    return [];
  }
  return Array.from(list.querySelectorAll('input[type="checkbox"]'))
    .filter((input) => input.checked)
    .map((input) => String(input.value || "").trim())
    .filter(Boolean);
};

const renderToolSelector = (selected) => {
  const list = elements.presetUserAgentTools;
  const empty = elements.presetUserAgentToolsEmpty;
  if (!list || !empty) {
    return;
  }
  list.textContent = "";
  const groups = Array.isArray(state.presetAgents.toolGroups) ? state.presetAgents.toolGroups : [];
  if (!groups.length) {
    empty.textContent = t("presetAgents.userAgent.toolsEmpty");
    empty.style.display = "block";
    return;
  }
  empty.style.display = "none";
  const selectedSet = new Set(
    (Array.isArray(selected) ? selected : []).map((item) => String(item || "").trim()).filter(Boolean)
  );
  groups.forEach((group) => {
    const groupHead = document.createElement("div");
    groupHead.className = "preset-tool-group-head";
    const title = document.createElement("div");
    title.className = "user-account-tool-group-title";
    title.textContent = group.label;
    const selectAllBtn = document.createElement("button");
    selectAllBtn.type = "button";
    selectAllBtn.className = "secondary preset-tool-group-select-all";
    selectAllBtn.textContent = t("mcp.tools.enableAll");
    groupHead.appendChild(title);
    groupHead.appendChild(selectAllBtn);
    list.appendChild(groupHead);

    const groupCheckboxes = [];
    group.options.forEach((option) => {
      const row = document.createElement("div");
      row.className = "tool-item";
      const checkbox = document.createElement("input");
      checkbox.type = "checkbox";
      checkbox.value = option.value;
      checkbox.checked = selectedSet.has(option.value);
      groupCheckboxes.push(checkbox);
      const label = document.createElement("label");
      const desc = option.description ? `<span class="muted">${option.description}</span>` : "";
      label.innerHTML = `<strong>${option.label}</strong>${desc}`;
      row.addEventListener("click", (event) => {
        if (event.target === checkbox) {
          return;
        }
        checkbox.checked = !checkbox.checked;
      });
      row.appendChild(checkbox);
      row.appendChild(label);
      list.appendChild(row);
    });

    selectAllBtn.addEventListener("click", (event) => {
      event.preventDefault();
      event.stopPropagation();
      groupCheckboxes.forEach((checkbox) => {
        checkbox.checked = true;
      });
    });
  });
};

const parseDate = (value) => {
  if (!value) {
    return 0;
  }
  const ts = Date.parse(String(value));
  return Number.isFinite(ts) ? ts : 0;
};

const setStatus = (text, kind = "") => {
  elements.presetAgentsStatusText.textContent = String(text || "").trim();
  elements.presetAgentsStatusText.dataset.kind = kind;
};

const selectedPreset = () =>
  state.presetAgents.presets.find((item) => item.name === state.presetAgents.selectedPresetName) || null;

const isDefaultPreset = (preset) =>
  Boolean(preset) &&
  (preset.is_default_agent === true || String(preset.preset_id || "").trim() === DEFAULT_AGENT_ID_ALIAS);

const buildEffectivePreset = (preset) => {
  if (!preset) {
    return null;
  }
  const agent = state.presetAgents.userAgent;
  if (!agent || (agent.name && agent.name !== preset.name)) {
    return preset;
  }
  const configuredModelName = normalizeOptionalModelName(agent.configured_model_name);
  return {
    ...preset,
    description: agent.description,
    system_prompt: agent.system_prompt,
    model_name: configuredModelName || normalizeOptionalModelName(preset.model_name),
    sandbox_container_id: Number.isFinite(Number(agent.sandbox_container_id))
      ? Number(agent.sandbox_container_id)
      : preset.sandbox_container_id,
    tool_names: Array.isArray(agent.tool_names) ? [...agent.tool_names] : preset.tool_names,
    declared_tool_names: Array.isArray(agent.declared_tool_names)
      ? [...agent.declared_tool_names]
      : preset.declared_tool_names,
    declared_skill_names: Array.isArray(agent.declared_skill_names)
      ? [...agent.declared_skill_names]
      : preset.declared_skill_names,
    preset_questions: Array.isArray(agent.preset_questions)
      ? [...agent.preset_questions]
      : preset.preset_questions,
    approval_mode: String(agent.approval_mode || preset.approval_mode || "full_auto").trim() || "full_auto",
    status: String(agent.status || preset.status || "active").trim() || "active",
  };
};

const effectiveSelectedPreset = () => buildEffectivePreset(selectedPreset());

const fillPresetForm = (preset) => {
  elements.presetAgentFormName.value = preset?.name || "";
  elements.presetAgentFormDescription.value = preset?.description || "";
  elements.presetAgentFormPrompt.value = preset?.system_prompt || "";
  renderModelOptions(normalizeOptionalModelName(preset?.model_name));
  elements.presetAgentFormModelName.disabled = preset?.is_default_agent === true;
  elements.presetAgentFormContainerId.value = String(
    Number.isFinite(Number(preset?.sandbox_container_id)) ? Number(preset.sandbox_container_id) : 1
  );
};

const collectPresetQuestionDrafts = () => {
  const container = elements.presetAgentPresetQuestions;
  if (!container) {
    return [];
  }
  return Array.from(container.querySelectorAll("textarea")).map((input) => String(input.value || ""));
};

const collectPresetQuestionValues = () => normalizeQuestionList(collectPresetQuestionDrafts());

const renderPresetQuestionEditor = (questions) => {
  const list = elements.presetAgentPresetQuestions;
  const empty = elements.presetAgentPresetQuestionsEmpty;
  const addBtn = elements.presetAgentPresetQuestionAddBtn;
  if (!list || !empty || !addBtn) {
    return;
  }
  const drafts = normalizeQuestionDrafts(questions);
  list.textContent = "";
  addBtn.disabled = !selectedPreset();
  if (!drafts.length) {
    empty.style.display = "block";
    return;
  }
  empty.style.display = "none";
  const fragment = document.createDocumentFragment();
  drafts.forEach((question, index) => {
    const row = document.createElement("div");
    row.className = "preset-question-item";

    const badge = document.createElement("div");
    badge.className = "preset-question-index";
    badge.textContent = String(index + 1);

    const textarea = document.createElement("textarea");
    textarea.className = "preset-question-input";
    textarea.rows = 2;
    textarea.placeholder = t("presetAgents.form.presetQuestionsPlaceholder");
    textarea.value = question;

    const removeBtn = document.createElement("button");
    removeBtn.type = "button";
    removeBtn.className = "preset-question-remove";
    removeBtn.title = t("common.delete");
    removeBtn.setAttribute("aria-label", t("common.delete"));
    removeBtn.innerHTML = '<i class="fa-solid fa-trash-can"></i>';
    removeBtn.addEventListener("click", () => {
      const nextDrafts = collectPresetQuestionDrafts();
      nextDrafts.splice(index, 1);
      renderPresetQuestionEditor(nextDrafts);
    });

    row.appendChild(badge);
    row.appendChild(textarea);
    row.appendChild(removeBtn);
    fragment.appendChild(row);
  });
  list.appendChild(fragment);
};

const fillAgentForm = (preset) => {
  renderToolSelector(preset?.tool_names || []);
  renderPresetQuestionEditor(preset?.preset_questions || []);
  elements.presetUserAgentApproval.value = preset?.approval_mode || "full_auto";
  elements.presetUserAgentHive.value = t("presetAgents.userAgent.hiveFixed");
};

const renderTabAvailability = () => {
  const preset = selectedPreset();
  const locked = isDefaultPreset(preset);
  elements.presetAgentTabCron.disabled = locked;
  elements.presetAgentTabChannels.disabled = locked;
  if (locked && state.presetAgents.activeTab !== "preset") {
    state.presetAgents.activeTab = "preset";
  }
};

const renderSyncSummary = () => {
  const preset = selectedPreset();
  const canSync = Boolean(preset?.preset_id);
  elements.presetAgentSyncSafeBtn.disabled = !canSync || state.presetAgents.syncLoading;
  elements.presetAgentSyncForceBtn.disabled = !canSync || state.presetAgents.syncLoading;

  if (!preset || !preset.preset_id) {
    elements.presetAgentSyncSummary.textContent = t("presetAgents.sync.empty");
    return;
  }
  if (state.presetAgents.syncLoading) {
    elements.presetAgentSyncSummary.textContent = t("presetAgents.sync.summaryLoading");
    return;
  }
  const summary =
    state.presetAgents.syncSummary?.preset_id === preset.preset_id ? state.presetAgents.syncSummary : null;
  if (!summary) {
    elements.presetAgentSyncSummary.textContent = t("presetAgents.sync.empty");
    return;
  }
  elements.presetAgentSyncSummary.textContent = t("presetAgents.sync.summary", {
    linked: summary.linked_users || 0,
    missing: summary.missing_users || 0,
    safe: summary.safe_update_agents || 0,
    overridden: summary.overridden_agents || 0,
    uptodate: summary.up_to_date_agents || 0,
  });
};

const renderPresetList = () => {
  const list = elements.presetAgentList;
  list.textContent = "";
  if (!state.presetAgents.presets.length) {
    const empty = document.createElement("div");
    empty.className = "preset-agent-list-empty";
    empty.textContent = t("presetAgents.list.empty");
    list.appendChild(empty);
    return;
  }
  const fragment = document.createDocumentFragment();
  state.presetAgents.presets.forEach((preset) => {
    const row = document.createElement("button");
    row.type = "button";
    row.className = "preset-agent-item";
    if (preset.name === state.presetAgents.selectedPresetName) {
      row.classList.add("is-active");
    }
    const title = document.createElement("div");
    title.className = "preset-agent-item-title";
    title.textContent = preset.name || "-";
    const meta = document.createElement("div");
    meta.className = "preset-agent-item-meta";
    meta.textContent = `v${Math.max(1, Number(preset.revision) || 1)} · #${String(preset.sandbox_container_id || 1)}`;
    row.appendChild(title);
    row.appendChild(meta);
    row.addEventListener("click", async () => {
      state.presetAgents.selectedPresetName = preset.name;
      state.presetAgents.syncSummary = null;
      renderAll();
      await refreshContext({ ensureAgent: true, silent: true });
      await loadSyncSummary({ silent: true });
    });
    fragment.appendChild(row);
  });
  list.appendChild(fragment);
};

const setTab = (tab) => {
  const nextTab = TAB_KEYS.includes(tab) ? tab : "preset";
  state.presetAgents.activeTab = nextTab;
  TAB_KEYS.forEach((key) => {
    const button = elements[`presetAgentTab${key.charAt(0).toUpperCase()}${key.slice(1)}`];
    const content = elements[`presetAgentTabContent${key.charAt(0).toUpperCase()}${key.slice(1)}`];
    button.classList.toggle("is-active", key === nextTab);
    button.setAttribute("aria-selected", key === nextTab ? "true" : "false");
    content.classList.toggle("active", key === nextTab);
  });
};

const renderPresetDetail = () => {
  const rawPreset = selectedPreset();
  const preset = effectiveSelectedPreset();
  if (!rawPreset || !preset) {
    elements.presetAgentDetailTitle.textContent = t("presetAgents.detail.empty");
    elements.presetAgentDetailMeta.textContent = "";
    elements.presetAgentDeleteBtn.disabled = true;
    fillPresetForm(null);
    fillAgentForm(null);
    return;
  }
  elements.presetAgentDetailTitle.textContent = rawPreset.name;
  const workspaceLabel = t("workspace.container.label");
  elements.presetAgentDetailMeta.textContent = [
    t("presetAgents.detail.templateUser", { user: TEMPLATE_USER_ID }),
    `v${Math.max(1, Number(rawPreset.revision) || 1)}`,
    `${workspaceLabel}: ${preset.sandbox_container_id}`,
    `hive: ${DEFAULT_HIVE_ID}`,
  ].join(" | ");
  elements.presetAgentDeleteBtn.disabled = rawPreset.is_default_agent === true;
  fillPresetForm(preset);
  fillAgentForm(preset);
};

const renderCronList = () => {
  const list = elements.presetCronList;
  list.textContent = "";
  if (!state.presetAgents.cronJobs.length) {
    const empty = document.createElement("div");
    empty.className = "preset-cron-list-empty";
    empty.textContent = t("presetAgents.cron.empty");
    list.appendChild(empty);
    return;
  }
  const fragment = document.createDocumentFragment();
  state.presetAgents.cronJobs.forEach((job) => {
    const row = document.createElement("div");
    row.className = "preset-cron-item";
    row.innerHTML = `
      <div class="preset-cron-item-title">${job.name || job.job_id}</div>
      <div class="preset-cron-item-meta">${[
        job.schedule_text || "-",
        job.enabled ? t("presetAgents.cron.enabled") : t("presetAgents.cron.disabled"),
        job.running ? t("presetAgents.cron.running") : "",
        job.next_run_at_text || "",
      ].filter(Boolean).join(" | ")}</div>
    `;
    const actions = document.createElement("div");
    actions.className = "preset-cron-item-actions";

    const editBtn = document.createElement("button");
    editBtn.type = "button";
    editBtn.className = "secondary";
    editBtn.textContent = t("common.edit");
    editBtn.addEventListener("click", () => {
      elements.presetCronJobId.value = job.job_id;
      elements.presetCronName.value = job.name || "";
      elements.presetCronScheduleText.value = job.schedule_text || "";
      elements.presetCronMessage.value = String(job.payload?.message || "");
      elements.presetCronEnabled.checked = job.enabled;
      setTab("cron");
    });

    const runBtn = document.createElement("button");
    runBtn.type = "button";
    runBtn.className = "secondary";
    runBtn.textContent = t("presetAgents.cron.run");
    runBtn.addEventListener("click", async () => executeCronAction("/cron/run", { job_id: job.job_id }, t("presetAgents.cron.run")));

    const toggleBtn = document.createElement("button");
    toggleBtn.type = "button";
    toggleBtn.className = "secondary";
    toggleBtn.textContent = job.enabled ? t("presetAgents.cron.disable") : t("presetAgents.cron.enable");
    toggleBtn.addEventListener("click", async () =>
      executeCronAction(job.enabled ? "/cron/disable" : "/cron/enable", { job_id: job.job_id }, toggleBtn.textContent)
    );

    const deleteBtn = document.createElement("button");
    deleteBtn.type = "button";
    deleteBtn.className = "danger";
    deleteBtn.textContent = t("common.delete");
    deleteBtn.addEventListener("click", async () => {
      if (!window.confirm(t("presetAgents.cron.confirmDelete", { name: job.name || job.job_id }))) {
        return;
      }
      await executeCronAction("/cron/remove", { job_id: job.job_id }, t("common.delete"));
    });

    actions.appendChild(editBtn);
    actions.appendChild(runBtn);
    actions.appendChild(toggleBtn);
    actions.appendChild(deleteBtn);
    row.appendChild(actions);
    fragment.appendChild(row);
  });
  list.appendChild(fragment);
};

const renderChannelForms = () => {
  const select = elements.presetChannelFormChannel;
  const current = String(select.value || "").trim();
  select.textContent = "";
  const placeholder = document.createElement("option");
  placeholder.value = "";
  placeholder.textContent = t("presetAgents.channels.selectChannel");
  select.appendChild(placeholder);

  (state.presetAgents.supportedChannels || []).forEach((item) => {
    const value = String(item?.channel || item?.value || item || "").trim();
    if (!value) {
      return;
    }
    const label = String(item?.name || item?.label || value).trim();
    const option = document.createElement("option");
    option.value = value;
    option.textContent = label || value;
    select.appendChild(option);
  });
  if (current) {
    select.value = current;
  }
};

const renderChannelAccounts = () => {
  const list = elements.presetChannelsAccountList;
  list.textContent = "";
  if (!state.presetAgents.channelAccounts.length) {
    const empty = document.createElement("div");
    empty.className = "preset-channel-list-empty";
    empty.textContent = t("presetAgents.channels.accounts.empty");
    list.appendChild(empty);
    return;
  }
  const fragment = document.createDocumentFragment();
  state.presetAgents.channelAccounts.forEach((account) => {
    const row = document.createElement("div");
    row.className = "preset-channel-item";
    row.innerHTML = `
      <div class="preset-channel-item-title">${account.channel || "-"} / ${account.account_id || "-"}</div>
      <div class="preset-channel-item-meta">${account.status || "active"} | ${account.peer_kind || "group"}</div>
    `;
    const actions = document.createElement("div");
    actions.className = "preset-channel-item-actions";

    const useBtn = document.createElement("button");
    useBtn.type = "button";
    useBtn.className = "secondary";
    useBtn.textContent = t("presetAgents.channels.use");
    useBtn.addEventListener("click", () => {
      elements.presetChannelFormChannel.value = account.channel || "";
      elements.presetChannelFormAccountId.value = account.account_id || "";
      elements.presetChannelFormPeerKind.value = account.peer_kind || "group";
      elements.presetChannelFormConfig.value = JSON.stringify(account.config || {}, null, 2);
      setTab("channels");
    });

    const deleteBtn = document.createElement("button");
    deleteBtn.type = "button";
    deleteBtn.className = "danger";
    deleteBtn.textContent = t("common.delete");
    deleteBtn.addEventListener("click", async () => {
      if (!window.confirm(t("presetAgents.channels.accounts.confirmDelete", { account: `${account.channel}/${account.account_id}` }))) {
        return;
      }
      await deleteChannelAccount(account.channel, account.account_id);
    });

    actions.appendChild(useBtn);
    actions.appendChild(deleteBtn);
    row.appendChild(actions);
    fragment.appendChild(row);
  });
  list.appendChild(fragment);
};

const renderAll = () => {
  renderTabAvailability();
  renderPresetList();
  renderPresetDetail();
  renderSyncSummary();
  renderCronList();
  renderChannelForms();
  renderChannelAccounts();
  setTab(state.presetAgents.activeTab);
};

const listUserAgents = async () => {
  const payload = await requestJson("/agents", { query: { user_id: TEMPLATE_USER_ID } });
  return (Array.isArray(payload?.data?.items) ? payload.data.items : []).map(normalizeUserAgent);
};

const loadDefaultTemplateAgent = async () => {
  const payload = await requestJson(`/agents/${encodeURIComponent(DEFAULT_AGENT_ID_ALIAS)}`, {
    query: { user_id: TEMPLATE_USER_ID },
  });
  return normalizeUserAgent(payload?.data || {});
};

const sameNameAgent = (agents, name) => {
  const cleaned = String(name || "").trim();
  if (!cleaned) {
    return null;
  }
  return agents
    .filter((item) => item.name === cleaned)
    .sort((a, b) => parseDate(b.updated_at) - parseDate(a.updated_at))[0] || null;
};

const ensureAgentForPreset = async (preset) => {
  if (isDefaultPreset(preset)) {
    return loadDefaultTemplateAgent();
  }
  const agents = await listUserAgents();
  const existed = sameNameAgent(agents, preset.name);
  if (existed) {
    return existed;
  }
  const payload = {
    name: preset.name,
    description: preset.description,
    system_prompt: preset.system_prompt,
    model_name: normalizeOptionalModelName(preset.model_name) || undefined,
    tool_names: Array.isArray(preset.tool_names) ? preset.tool_names : [],
    approval_mode: preset.approval_mode || "full_auto",
    status: preset.status || "active",
    is_shared: false,
    hive_id: DEFAULT_HIVE_ID,
    icon: JSON.stringify({ name: normalizeIconName(preset.icon_name), color: normalizeIconColor(preset.icon_color) }),
    sandbox_container_id: Number.isFinite(Number(preset.sandbox_container_id)) ? Number(preset.sandbox_container_id) : 1,
  };
  const created = await requestJson("/agents", { method: "POST", query: { user_id: TEMPLATE_USER_ID }, body: payload });
  return normalizeUserAgent(created?.data || {});
};

const loadModelCatalog = async ({ silent = false } = {}) => {
  try {
    const payload = await requestJson("/admin/llm");
    const catalog = extractLlmModelCatalog(payload);
    state.presetAgents.modelOptions = catalog.options;
    state.presetAgents.defaultModelName = catalog.defaultModelName;
    renderModelOptions(normalizeOptionalModelName(effectiveSelectedPreset()?.model_name));
  } catch (error) {
    state.presetAgents.modelOptions = [];
    state.presetAgents.defaultModelName = "";
    renderModelOptions(normalizeOptionalModelName(effectiveSelectedPreset()?.model_name));
    if (!silent) {
      notify(t("presetAgents.toast.refreshFailed", { message: error.message || "-" }), "error");
    }
  }
};

const loadToolCatalog = async () => {
  const payload = await requestJson("/tools", { query: { user_id: TEMPLATE_USER_ID } });
  const source = payload?.data && typeof payload.data === "object" ? payload.data : payload || {};
  state.presetAgents.toolGroups = buildToolGroups(source);
  renderToolSelector(effectiveSelectedPreset()?.tool_names || []);
};

const loadCronJobs = async () => {
  if (!state.presetAgents.userAgent?.id) {
    state.presetAgents.cronJobs = [];
    renderCronList();
    return;
  }
  const payload = await requestJson("/cron/list", {
    query: { user_id: TEMPLATE_USER_ID, agent_id: state.presetAgents.userAgent.id },
  });
  state.presetAgents.cronJobs = (Array.isArray(payload?.data?.jobs) ? payload.data.jobs : [])
    .map(normalizeCronJob)
    .filter((item) => item.agent_id === state.presetAgents.userAgent.id);
  renderCronList();
};

const loadChannelAccounts = async () => {
  const [accountsPayload, bindingsPayload] = await Promise.all([
    requestJson("/channels/accounts", { query: { user_id: TEMPLATE_USER_ID } }),
    requestJson("/channels/bindings", { query: { user_id: TEMPLATE_USER_ID } }),
  ]);
  const allAccounts = (Array.isArray(accountsPayload?.data?.items) ? accountsPayload.data.items : []).map(normalizeChannelAccount);
  const allBindings = (Array.isArray(bindingsPayload?.data?.items) ? bindingsPayload.data.items : []).map(normalizeChannelBinding);
  const agentId = String(state.presetAgents.userAgent?.id || "").trim();

  let accounts = allAccounts;
  if (agentId) {
    const keys = new Set(
      allBindings
        .filter((binding) => binding.enabled && binding.agent_id === agentId)
        .map((binding) => `${binding.channel.toLowerCase()}::${binding.account_id}`)
    );
    accounts = allAccounts.filter((account) => keys.has(`${account.channel.toLowerCase()}::${account.account_id}`));
  }

  state.presetAgents.channelAccounts = accounts;
  state.presetAgents.supportedChannels = Array.isArray(accountsPayload?.data?.supported_channels)
    ? accountsPayload.data.supported_channels
    : [];
  renderChannelForms();
  renderChannelAccounts();
};

const refreshContext = async ({ ensureAgent = true, silent = false } = {}) => {
  const preset = selectedPreset();
  if (!preset) {
    state.presetAgents.userAgent = null;
    state.presetAgents.syncSummary = null;
    state.presetAgents.syncLoading = false;
    state.presetAgents.toolGroups = [];
    state.presetAgents.cronJobs = [];
    state.presetAgents.channelAccounts = [];
    renderAll();
    return;
  }

  try {
    elements.presetUserAgentTools.textContent = "";
    elements.presetUserAgentToolsEmpty.textContent = t("common.loading");
    elements.presetUserAgentToolsEmpty.style.display = "block";

    if (ensureAgent) {
      state.presetAgents.userAgent = await ensureAgentForPreset(preset);
    } else {
      state.presetAgents.userAgent = isDefaultPreset(preset)
        ? await loadDefaultTemplateAgent()
        : sameNameAgent(await listUserAgents(), preset.name);
    }
    fillAgentForm(preset);
    if (isDefaultPreset(preset)) {
      state.presetAgents.cronJobs = [];
      state.presetAgents.channelAccounts = [];
      state.presetAgents.supportedChannels = [];
      await loadToolCatalog();
      renderCronList();
      renderChannelForms();
      renderChannelAccounts();
    } else {
      await Promise.all([loadCronJobs(), loadChannelAccounts(), loadToolCatalog()]);
    }
    renderPresetDetail();
    setStatus(t("presetAgents.status.ready", { user: TEMPLATE_USER_ID, agent: state.presetAgents.userAgent?.name || "-" }), "success");
    if (!silent) {
      notify(t("presetAgents.toast.userContextReady"), "success");
    }
  } catch (error) {
    setStatus(t("presetAgents.status.failed", { message: error.message || "-" }), "error");
    if (!silent) {
      notify(t("presetAgents.toast.userContextFailed", { message: error.message || "-" }), "error");
    }
  }
};

const collectPresetForm = () => {
  const name = String(elements.presetAgentFormName.value || "").trim();
  if (!name) {
    throw new Error(t("presetAgents.error.nameRequired"));
  }
  const sandbox = Number.parseInt(elements.presetAgentFormContainerId.value, 10);
  return {
    name,
    description: String(elements.presetAgentFormDescription.value || "").trim(),
    system_prompt: String(elements.presetAgentFormPrompt.value || "").trim(),
    model_name: normalizeOptionalModelName(elements.presetAgentFormModelName?.value),
    sandbox_container_id: Number.isFinite(sandbox) && sandbox > 0 ? sandbox : 1,
  };
};

const persistPresets = async (selectedName) => {
  const defaultPreset = state.presetAgents.presets.find((item) => item.is_default_agent === true) || null;
  const payload = {
    items: state.presetAgents.presets
      .filter((item) => item.is_default_agent !== true)
      .map((item) => ({
        preset_id: item.preset_id,
        revision: item.revision,
        name: item.name,
        description: item.description,
        system_prompt: item.system_prompt,
        model_name: normalizeOptionalModelName(item.model_name),
        icon_name: item.icon_name,
        icon_color: item.icon_color,
        sandbox_container_id: item.sandbox_container_id,
        tool_names: item.tool_names,
        declared_tool_names: item.declared_tool_names,
        declared_skill_names: item.declared_skill_names,
        preset_questions: item.preset_questions,
        approval_mode: item.approval_mode,
        status: item.status,
      })),
  };
  const saved = await requestJson("/admin/preset_agents", { method: "POST", body: payload });
  state.presetAgents.presets = (Array.isArray(saved?.data?.items) ? saved.data.items : []).map(normalizePreset).filter((item) => item.name);
  if (defaultPreset && !state.presetAgents.presets.some((item) => item.is_default_agent === true)) {
    state.presetAgents.presets.unshift(defaultPreset);
  }
  if (!state.presetAgents.presets.length) {
    state.presetAgents.selectedPresetName = "";
    return;
  }
  const preferred = String(selectedName || "").trim();
  if (preferred && state.presetAgents.presets.some((item) => item.name === preferred)) {
    state.presetAgents.selectedPresetName = preferred;
    return;
  }
  if (!state.presetAgents.presets.some((item) => item.name === state.presetAgents.selectedPresetName)) {
    state.presetAgents.selectedPresetName = state.presetAgents.presets[0].name;
  }
};

const savePreset = async () => {
  try {
    const current = selectedPreset();
    const effective = effectiveSelectedPreset() || current;
    if (!current || !effective) {
      throw new Error(t("presetAgents.error.noPresetSelected"));
    }
    const draft = collectPresetForm();
    const next = { ...current, ...effective, ...draft };
    const duplicate = state.presetAgents.presets.find(
      (item) => item.name.toLowerCase() === next.name.toLowerCase() && item.name !== current.name
    );
    if (duplicate) {
      throw new Error(t("presetAgents.error.duplicateName", { name: next.name }));
    }
    const currentTemplateAgentId = String(state.presetAgents.userAgent?.id || "").trim();
    const agentPayload = collectAgentForm();
    next.tool_names = agentPayload.tool_names;
    next.declared_tool_names = Array.isArray(effective.declared_tool_names) ? [...effective.declared_tool_names] : [];
    next.declared_skill_names = Array.isArray(effective.declared_skill_names) ? [...effective.declared_skill_names] : [];
    next.preset_questions = agentPayload.preset_questions;
    next.approval_mode = agentPayload.approval_mode;
    next.status = agentPayload.status || effective.status || current.status || "active";
    next.model_name = normalizeOptionalModelName(agentPayload.model_name);
    next.sandbox_container_id = agentPayload.sandbox_container_id;
    if (isDefaultPreset(current)) {
      await requestJson(`/agents/${encodeURIComponent(DEFAULT_AGENT_ID_ALIAS)}`, {
        method: "PUT",
        query: { user_id: TEMPLATE_USER_ID },
        body: { ...agentPayload, name: next.name },
      });
      await loadPresetAgents({ silent: true, selectedName: next.name });
      notify(t("presetAgents.toast.savePresetSuccess"), "success");
      return;
    }
    state.presetAgents.presets = state.presetAgents.presets.map((item) => (item.name === current.name ? next : item));
    state.presetAgents.selectedPresetName = next.name;
    await persistPresets(next.name);

    if (currentTemplateAgentId) {
      await requestJson(`/agents/${encodeURIComponent(currentTemplateAgentId)}`, {
        method: "PUT",
        query: { user_id: TEMPLATE_USER_ID },
        body: { ...agentPayload, name: next.name },
      });
      await refreshContext({ ensureAgent: false, silent: true });
    } else {
      await refreshContext({ ensureAgent: true, silent: true });
    }
    await loadSyncSummary({ silent: true });
    renderAll();
    notify(t("presetAgents.toast.savePresetSuccess"), "success");
  } catch (error) {
    notify(t("presetAgents.toast.savePresetFailed", { message: error.message || "-" }), "error");
  }
};

const createPreset = () => {
  const baseName = t("presetAgents.newPresetName");
  let index = state.presetAgents.presets.length + 1;
  let candidate = `${baseName}${index}`;
  const existing = new Set(state.presetAgents.presets.map((item) => item.name.toLowerCase()));
  while (existing.has(candidate.toLowerCase())) {
    index += 1;
    candidate = `${baseName}${index}`;
  }
  state.presetAgents.presets.push({
    preset_id: "",
    revision: 1,
    name: candidate,
    description: "",
    system_prompt: "",
    model_name: "",
    icon_name: "spark",
    icon_color: "#94a3b8",
    sandbox_container_id: 1,
    tool_names: [],
    declared_tool_names: [],
    declared_skill_names: [],
    preset_questions: [],
    approval_mode: "full_auto",
    status: "active",
  });
  state.presetAgents.selectedPresetName = candidate;
  state.presetAgents.syncSummary = null;
  renderAll();
  setTab("preset");
};

const deletePreset = async () => {
  const current = selectedPreset();
  if (!current || isDefaultPreset(current)) {
    return;
  }
  if (!window.confirm(t("presetAgents.confirmDelete", { name: current.name }))) {
    return;
  }
  try {
    state.presetAgents.presets = state.presetAgents.presets.filter((item) => item.name !== current.name);
    state.presetAgents.selectedPresetName = state.presetAgents.presets[0]?.name || "";
    state.presetAgents.syncSummary = null;
    await persistPresets(state.presetAgents.selectedPresetName);
    if (state.presetAgents.selectedPresetName) {
      await refreshContext({ ensureAgent: true, silent: true });
      await loadSyncSummary({ silent: true });
    } else {
      state.presetAgents.userAgent = null;
      state.presetAgents.cronJobs = [];
      state.presetAgents.channelAccounts = [];
    }
    renderAll();
    notify(t("presetAgents.toast.deletePresetSuccess"), "success");
  } catch (error) {
    notify(t("presetAgents.toast.deletePresetFailed", { message: error.message || "-" }), "error");
  }
};

const collectAgentForm = () => {
  const name = String(elements.presetAgentFormName.value || "").trim();
  if (!name) {
    throw new Error(t("presetAgents.error.agentNameRequired"));
  }
  const sandbox = Number.parseInt(elements.presetAgentFormContainerId.value, 10);
  const tool_names = collectSelectedTools(elements.presetUserAgentTools);
  return {
    name,
    description: String(elements.presetAgentFormDescription.value || "").trim(),
    system_prompt: String(elements.presetAgentFormPrompt.value || "").trim(),
    model_name: normalizeOptionalModelName(elements.presetAgentFormModelName?.value),
    tool_names,
    preset_questions: collectPresetQuestionValues(),
    approval_mode:
      String(elements.presetUserAgentApproval.value || effectiveSelectedPreset()?.approval_mode || "full_auto").trim() ||
      "full_auto",
    sandbox_container_id: Number.isFinite(sandbox) && sandbox > 0 ? sandbox : 1,
    status: String(effectiveSelectedPreset()?.status || "active").trim() || "active",
    is_shared: false,
    hive_id: DEFAULT_HIVE_ID,
    icon: String(state.presetAgents.userAgent?.icon || ""),
  };
};

const resolveCronSessionId = async () => {
  const agentId = state.presetAgents.userAgent?.id;
  if (!agentId) {
    return `cron_${Date.now()}`;
  }
  try {
    const payload = await requestJson(`/agents/${encodeURIComponent(agentId)}/default-session`, {
      query: { user_id: TEMPLATE_USER_ID },
    });
    const sessionId = String(payload?.data?.session_id || "").trim();
    return sessionId || `cron_${Date.now()}`;
  } catch (_error) {
    return `cron_${Date.now()}`;
  }
};

const executeCronAction = async (path, job, actionLabel) => {
  try {
    await requestJson(path, {
      method: "POST",
      query: { user_id: TEMPLATE_USER_ID, agent_id: state.presetAgents.userAgent?.id },
      body: { action: "manual", job },
    });
    await loadCronJobs();
    notify(t("presetAgents.toast.cronActionSuccess", { action: actionLabel || path }), "success");
  } catch (error) {
    notify(t("presetAgents.toast.cronActionFailed", { action: actionLabel || path, message: error.message || "-" }), "error");
  }
};

const saveCronJob = async () => {
  try {
    const agent = state.presetAgents.userAgent;
    if (!agent?.id) {
      throw new Error(t("presetAgents.error.agentNotReady"));
    }
    const name = String(elements.presetCronName.value || "").trim();
    const schedule_text = String(elements.presetCronScheduleText.value || "").trim();
    const message = String(elements.presetCronMessage.value || "").trim();
    const job_id = String(elements.presetCronJobId.value || "").trim();
    if (!name || !schedule_text || !message) {
      throw new Error(t("presetAgents.error.cronRequired"));
    }

    const path = job_id ? "/cron/update" : "/cron/add";
    await requestJson(path, {
      method: "POST",
      query: { user_id: TEMPLATE_USER_ID, agent_id: agent.id },
      body: {
        action: job_id ? "update" : "add",
        job: {
          job_id: job_id || undefined,
          name,
          schedule_text,
          enabled: elements.presetCronEnabled.checked,
          session_id: await resolveCronSessionId(),
          agent_id: agent.id,
          payload: { message },
        },
      },
    });
    elements.presetCronJobId.value = "";
    await loadCronJobs();
    notify(t("presetAgents.toast.saveCronSuccess"), "success");
  } catch (error) {
    notify(t("presetAgents.toast.saveCronFailed", { message: error.message || "-" }), "error");
  }
};

const parseConfigJson = (raw) => {
  const text = String(raw || "").trim();
  if (!text) {
    return {};
  }
  const parsed = JSON.parse(text);
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw new Error(t("presetAgents.error.configObject"));
  }
  return parsed;
};

const saveChannelAccount = async () => {
  try {
    const agentId = state.presetAgents.userAgent?.id;
    if (!agentId) {
      throw new Error(t("presetAgents.error.agentNotReady"));
    }
    const channel = String(elements.presetChannelFormChannel.value || "").trim();
    if (!channel) {
      throw new Error(t("presetAgents.error.channelRequired"));
    }
    const accountId = String(elements.presetChannelFormAccountId.value || "").trim();
    const payload = {
      channel,
      account_id: accountId || undefined,
      create_new: !accountId,
      agent_id: agentId,
      account_name: String(elements.presetChannelFormAccountName.value || "").trim() || undefined,
      peer_kind: String(elements.presetChannelFormPeerKind.value || "").trim() || undefined,
      enabled: elements.presetChannelFormEnabled.checked,
      config: parseConfigJson(elements.presetChannelFormConfig.value),
    };
    const result = await requestJson("/channels/accounts", { method: "POST", query: { user_id: TEMPLATE_USER_ID }, body: payload });
    const savedAccountId = String(result?.data?.account_id || "").trim();
    if (savedAccountId) {
      elements.presetChannelFormAccountId.value = savedAccountId;
    }
    await loadChannelAccounts();
    notify(t("presetAgents.toast.saveChannelSuccess"), "success");
  } catch (error) {
    notify(t("presetAgents.toast.saveChannelFailed", { message: error.message || "-" }), "error");
  }
};

const deleteChannelAccount = async (channel, accountId) => {
  try {
    await requestJson(`/channels/accounts/${encodeURIComponent(channel)}/${encodeURIComponent(accountId)}`, {
      method: "DELETE",
      query: { user_id: TEMPLATE_USER_ID },
    });
    await loadChannelAccounts();
    notify(t("presetAgents.toast.deleteChannelSuccess"), "success");
  } catch (error) {
    notify(t("presetAgents.toast.deleteChannelFailed", { message: error.message || "-" }), "error");
  }
};

const loadSyncSummary = async ({ silent = false } = {}) => {
  const preset = selectedPreset();
  if (!preset?.preset_id) {
    state.presetAgents.syncSummary = null;
    state.presetAgents.syncLoading = false;
    renderSyncSummary();
    return null;
  }

  const requestToken = (Number(state.presetAgents.syncRequestToken) || 0) + 1;
  state.presetAgents.syncRequestToken = requestToken;
  state.presetAgents.syncLoading = true;
  renderSyncSummary();

  try {
    const payload = await requestJson("/admin/preset_agents/sync", {
      method: "POST",
      body: {
        preset_id: preset.preset_id,
        mode: "safe",
        dry_run: true,
      },
    });
    if (state.presetAgents.syncRequestToken !== requestToken) {
      return null;
    }
    state.presetAgents.syncSummary = {
      preset_id: preset.preset_id,
      ...(payload?.data?.summary && typeof payload.data.summary === "object" ? payload.data.summary : {}),
    };
    return state.presetAgents.syncSummary;
  } catch (error) {
    if (state.presetAgents.syncRequestToken === requestToken) {
      state.presetAgents.syncSummary = null;
    }
    if (!silent) {
      notify(t("presetAgents.toast.syncFailed", { message: error.message || "-" }), "error");
    }
    return null;
  } finally {
    if (state.presetAgents.syncRequestToken === requestToken) {
      state.presetAgents.syncLoading = false;
      renderSyncSummary();
    }
  }
};

const runPresetSync = async (mode = "safe") => {
  const preset = selectedPreset();
  if (!preset?.preset_id) {
    return;
  }
  if (mode === "force" && !window.confirm(t("presetAgents.confirmSyncForce"))) {
    return;
  }
  state.presetAgents.syncLoading = true;
  renderSyncSummary();
  try {
    const payload = await requestJson("/admin/preset_agents/sync", {
      method: "POST",
      body: {
        preset_id: preset.preset_id,
        mode: mode === "force" ? "force" : "safe",
        dry_run: false,
      },
    });
    const summary = payload?.data?.summary && typeof payload.data.summary === "object" ? payload.data.summary : {};
    notify(
      t("presetAgents.toast.syncSuccess", {
        created: summary.created_agents || 0,
        updated: summary.updated_agents || 0,
        rebound: summary.rebound_agents || 0,
      }),
      "success"
    );
  } catch (error) {
    notify(t("presetAgents.toast.syncFailed", { message: error.message || "-" }), "error");
    return;
  } finally {
    state.presetAgents.syncLoading = false;
    renderSyncSummary();
  }
  await loadSyncSummary({ silent: true });
};

export const loadPresetAgents = async ({ silent = false, selectedName = "" } = {}) => {
  ensureState();
  if (!ensureElements()) {
    return;
  }
  state.presetAgents.loading = true;
  try {
    await loadModelCatalog({ silent: true });
    const payload = await requestJson("/admin/preset_agents");
    state.presetAgents.presets = (Array.isArray(payload?.data?.items) ? payload.data.items : []).map(normalizePreset).filter((item) => item.name);

    const preferred = String(selectedName || "").trim();
    if (preferred && state.presetAgents.presets.some((item) => item.name === preferred)) {
      state.presetAgents.selectedPresetName = preferred;
    } else if (!state.presetAgents.presets.some((item) => item.name === state.presetAgents.selectedPresetName)) {
      state.presetAgents.selectedPresetName = state.presetAgents.presets[0]?.name || "";
    }
    if (!TAB_KEYS.includes(state.presetAgents.activeTab)) {
      state.presetAgents.activeTab = "preset";
    }

    await refreshContext({ ensureAgent: true, silent: true });
    await loadSyncSummary({ silent: true });
    renderAll();
    if (!silent) {
      notify(t("presetAgents.toast.refreshSuccess"), "success");
    }
  } catch (error) {
    renderAll();
    setStatus(t("presetAgents.status.failed", { message: error.message || "-" }), "error");
    if (!silent) {
      notify(t("presetAgents.toast.refreshFailed", { message: error.message || "-" }), "error");
    }
  } finally {
    state.presetAgents.loading = false;
  }
};

const bindTabs = () => {
  TAB_KEYS.forEach((key) => {
    const button = elements[`presetAgentTab${key.charAt(0).toUpperCase()}${key.slice(1)}`];
    if (!button || button.dataset.bound === "1") {
      return;
    }
    button.dataset.bound = "1";
    button.addEventListener("click", () => setTab(key));
  });
};

const bindActions = () => {
  if (elements.presetAgentsRefreshBtn.dataset.bound !== "1") {
    elements.presetAgentsRefreshBtn.dataset.bound = "1";
    elements.presetAgentsRefreshBtn.addEventListener("click", async () => loadPresetAgents());
  }
  if (elements.presetAgentCreateBtn.dataset.bound !== "1") {
    elements.presetAgentCreateBtn.dataset.bound = "1";
    elements.presetAgentCreateBtn.addEventListener("click", createPreset);
  }
  if (elements.presetAgentSaveBtn.dataset.bound !== "1") {
    elements.presetAgentSaveBtn.dataset.bound = "1";
    elements.presetAgentSaveBtn.addEventListener("click", savePreset);
  }
  if (elements.presetAgentPresetQuestionAddBtn.dataset.bound !== "1") {
    elements.presetAgentPresetQuestionAddBtn.dataset.bound = "1";
    elements.presetAgentPresetQuestionAddBtn.addEventListener("click", () => {
      const nextDrafts = collectPresetQuestionDrafts();
      nextDrafts.push("");
      renderPresetQuestionEditor(nextDrafts);
    });
  }
  if (elements.presetAgentSyncSafeBtn.dataset.bound !== "1") {
    elements.presetAgentSyncSafeBtn.dataset.bound = "1";
    elements.presetAgentSyncSafeBtn.addEventListener("click", async () => runPresetSync("safe"));
  }
  if (elements.presetAgentSyncForceBtn.dataset.bound !== "1") {
    elements.presetAgentSyncForceBtn.dataset.bound = "1";
    elements.presetAgentSyncForceBtn.addEventListener("click", async () => runPresetSync("force"));
  }
  if (elements.presetAgentDeleteBtn.dataset.bound !== "1") {
    elements.presetAgentDeleteBtn.dataset.bound = "1";
    elements.presetAgentDeleteBtn.addEventListener("click", deletePreset);
  }
  if (elements.presetCronSaveBtn.dataset.bound !== "1") {
    elements.presetCronSaveBtn.dataset.bound = "1";
    elements.presetCronSaveBtn.addEventListener("click", saveCronJob);
  }
  if (elements.presetChannelSaveBtn.dataset.bound !== "1") {
    elements.presetChannelSaveBtn.dataset.bound = "1";
    elements.presetChannelSaveBtn.addEventListener("click", saveChannelAccount);
  }
};

export const initPresetAgentsPanel = () => {
  ensureState();
  if (!ensureElements()) {
    return;
  }
  if (state.presetAgents.initialized) {
    return;
  }
  bindTabs();
  bindActions();
  renderAll();
  state.presetAgents.initialized = true;
  appendLog(t("presetAgents.init"));
};
