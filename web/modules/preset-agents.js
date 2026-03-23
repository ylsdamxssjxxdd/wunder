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
      selectedPresetId: "",
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
      draftDirty: false,
      draftVersion: 0,
      saving: false,
      savePromise: null,
      toolListScrollTopByPresetKey: {},
    };
  }
  if (typeof state.presetAgents.selectedPresetId !== "string") {
    state.presetAgents.selectedPresetId = "";
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
  "presetAgentExportBtn",
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

const downloadJsonFile = (filename, value) => {
  if (typeof window === "undefined" || typeof document === "undefined") {
    return;
  }
  const payload = typeof value === "string" ? value : JSON.stringify(value, null, 2);
  const blob = new Blob(["\uFEFF", payload], { type: "application/json;charset=utf-8" });
  const url = window.URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = String(filename || "worker-card.json").trim() || "worker-card.json";
  document.body.appendChild(anchor);
  anchor.click();
  document.body.removeChild(anchor);
  window.URL.revokeObjectURL(url);
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

const normalizeNameList = (values) => {
  const seen = new Set();
  const output = [];
  (Array.isArray(values) ? values : []).forEach((value) => {
    const cleaned = String(value || "").trim();
    if (!cleaned || seen.has(cleaned)) {
      return;
    }
    seen.add(cleaned);
    output.push(cleaned);
  });
  return output;
};

const normalizePresetId = (value) => String(value || "").trim();
const normalizePresetNameKey = (value) => String(value || "").trim().toLowerCase();

const isSamePreset = (left, right) => {
  if (!left || !right) {
    return false;
  }
  const leftId = normalizePresetId(left.preset_id);
  const rightId = normalizePresetId(right.preset_id);
  if (leftId || rightId) {
    return leftId !== "" && leftId === rightId;
  }
  return String(left.name || "").trim() === String(right.name || "").trim();
};

const setSelectedPreset = (preset) => {
  if (!preset) {
    state.presetAgents.selectedPresetName = "";
    state.presetAgents.selectedPresetId = "";
    return;
  }
  state.presetAgents.selectedPresetName = String(preset.name || "").trim();
  state.presetAgents.selectedPresetId = normalizePresetId(preset.preset_id);
};

const findPresetById = (presetId) => {
  const cleaned = normalizePresetId(presetId);
  if (!cleaned) {
    return null;
  }
  return state.presetAgents.presets.find((item) => normalizePresetId(item.preset_id) === cleaned) || null;
};

const findPresetByName = (name) => {
  const cleaned = String(name || "").trim();
  if (!cleaned) {
    return null;
  }
  const matches = state.presetAgents.presets.filter((item) => item.name === cleaned);
  if (!matches.length) {
    return null;
  }
  return matches.find((item) => item.is_default_agent !== true) || matches[0];
};

const resolvePresetSelection = ({ presetId = "", name = "" } = {}) =>
  findPresetById(presetId) || findPresetByName(name);

const presetStableOrderKey = (preset) => {
  const presetId = normalizePresetId(preset?.preset_id);
  if (presetId) {
    return `id:${presetId}`;
  }
  const nameKey = normalizePresetNameKey(preset?.name);
  if (nameKey) {
    return `name:${nameKey}`;
  }
  return "";
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

const normalizePresetItems = (items) =>
  (Array.isArray(items) ? items : []).map(normalizePreset).filter((item) => item.name);

const stabilizePresetListOrder = (incomingItems, previousItems = state.presetAgents.presets) => {
  const normalizedIncoming = normalizePresetItems(incomingItems);
  const normalizedPrevious = Array.isArray(previousItems) ? previousItems : [];
  const defaultPreset =
    normalizedIncoming.find((item) => item.is_default_agent === true) ||
    normalizedPrevious.find((item) => item?.is_default_agent === true) ||
    null;

  const incomingById = new Map();
  const incomingByName = new Map();
  normalizedIncoming.forEach((item) => {
    if (item.is_default_agent === true) {
      return;
    }
    const presetId = normalizePresetId(item.preset_id);
    if (presetId) {
      incomingById.set(presetId, item);
    }
    const nameKey = normalizePresetNameKey(item.name);
    if (nameKey && !incomingByName.has(nameKey)) {
      incomingByName.set(nameKey, item);
    }
  });

  const ordered = [];
  const seen = new Set();
  normalizedPrevious.forEach((item) => {
    if (!item || item.is_default_agent === true) {
      return;
    }
    const presetId = normalizePresetId(item.preset_id);
    const nameKey = normalizePresetNameKey(item.name);
    const candidate = (presetId && incomingById.get(presetId)) || (nameKey && incomingByName.get(nameKey)) || null;
    if (!candidate) {
      return;
    }
    const orderKey = presetStableOrderKey(candidate);
    if (!orderKey || seen.has(orderKey)) {
      return;
    }
    seen.add(orderKey);
    ordered.push(candidate);
  });

  normalizedIncoming.forEach((item) => {
    if (item.is_default_agent === true) {
      return;
    }
    const orderKey = presetStableOrderKey(item);
    if (!orderKey || seen.has(orderKey)) {
      return;
    }
    seen.add(orderKey);
    ordered.push(item);
  });

  return defaultPreset ? [defaultPreset, ...ordered] : ordered;
};

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

const normalizeAbilityKind = (value) =>
  String(value || "").trim().toLowerCase() === "skill" ? "skill" : "tool";

const abilityGroupLabel = (groupKey) => {
  switch (groupKey) {
    case "builtin":
      return t("userAccounts.toolGroup.builtin");
    case "mcp":
      return t("userAccounts.toolGroup.mcp");
    case "a2a":
      return t("userAccounts.toolGroup.a2a");
    case "skills":
      return t("userAccounts.toolGroup.skills");
    case "knowledge":
      return t("userAccounts.toolGroup.knowledge");
    case "user":
      return t("userAccounts.toolGroup.user");
    case "shared":
      return t("userAccounts.toolGroup.shared");
    default:
      return "";
  }
};

const classifyAbilityGroupKey = (item) => {
  const group = String(item?.group || "").trim().toLowerCase();
  const source = String(item?.source || "").trim().toLowerCase();
  if (group === "builtin" || source === "builtin") {
    return "builtin";
  }
  if (group === "mcp" || source === "mcp") {
    return "mcp";
  }
  if (group === "a2a" || source === "a2a") {
    return "a2a";
  }
  if (group === "skills" || source === "skill") {
    return "skills";
  }
  if (group === "knowledge" || source === "knowledge") {
    return "knowledge";
  }
  if (group === "shared" || source === "shared") {
    return "shared";
  }
  return "user";
};

const buildAbilityOption = (item, fallbackKind = "tool") => {
  if (!item) {
    return null;
  }
  const runtimeName = item.runtime_name || item.name || item.tool_name || item.toolName;
  const cleanedRuntimeName = String(runtimeName || "").trim();
  if (!cleanedRuntimeName) {
    return null;
  }
  const displayName = item.display_name || item.displayName || cleanedRuntimeName;
  return {
    value: cleanedRuntimeName,
    label: String(displayName || cleanedRuntimeName),
    description: String(item.description || ""),
    kind: normalizeAbilityKind(item.kind || fallbackKind),
  };
};

const buildToolOptions = (list, fallbackKind = "tool") =>
  (Array.isArray(list) ? list : [])
    .map((item) => buildAbilityOption(item, fallbackKind))
    .filter(Boolean);

const buildToolGroupsFromItems = (items) => {
  const groups = new Map(
    ["builtin", "mcp", "a2a", "skills", "knowledge", "user", "shared"].map((groupKey) => [
      groupKey,
      { label: abilityGroupLabel(groupKey), options: [] },
    ])
  );
  const seenByGroup = new Map();
  (Array.isArray(items) ? items : []).forEach((item) => {
    const groupKey = classifyAbilityGroupKey(item);
    const option = buildAbilityOption(item, item?.kind);
    if (!option || !groups.has(groupKey)) {
      return;
    }
    if (!seenByGroup.has(groupKey)) {
      seenByGroup.set(groupKey, new Set());
    }
    const seen = seenByGroup.get(groupKey);
    if (seen.has(option.value)) {
      return;
    }
    seen.add(option.value);
    groups.get(groupKey).options.push(option);
  });
  return Array.from(groups.values()).filter((group) => group.options.length > 0);
};

const buildToolGroups = (payload) => {
  const itemGroups = buildToolGroupsFromItems(payload?.items);
  if (itemGroups.length) {
    return itemGroups;
  }
  const userOptions = [
    ...buildToolOptions(payload?.user_mcp_tools, "tool"),
    ...buildToolOptions(payload?.user_skills, "skill"),
    ...buildToolOptions(payload?.user_knowledge_tools, "tool"),
  ];
  if (!userOptions.length) {
    userOptions.push(...buildToolOptions(payload?.user_tools, "tool"));
  }
  return [
    { label: t("userAccounts.toolGroup.builtin"), options: buildToolOptions(payload?.builtin_tools, "tool") },
    { label: t("userAccounts.toolGroup.mcp"), options: buildToolOptions(payload?.mcp_tools, "tool") },
    { label: t("userAccounts.toolGroup.a2a"), options: buildToolOptions(payload?.a2a_tools, "tool") },
    { label: t("userAccounts.toolGroup.skills"), options: buildToolOptions(payload?.skills, "skill") },
    { label: t("userAccounts.toolGroup.knowledge"), options: buildToolOptions(payload?.knowledge_tools, "tool") },
    { label: t("userAccounts.toolGroup.user"), options: userOptions },
    { label: t("userAccounts.toolGroup.shared"), options: buildToolOptions(payload?.shared_tools, "tool") },
  ].filter((group) => group.options.length > 0);
};

const collectSelectedAbilities = (list) => {
  if (!list) {
    return [];
  }
  const seen = new Set();
  return Array.from(list.querySelectorAll('input[type="checkbox"]'))
    .filter((input) => input.checked)
    .map((input) => ({
      name: String(input.value || "").trim(),
      kind: normalizeAbilityKind(input.dataset.kind),
    }))
    .filter((item) => {
      if (!item.name || seen.has(item.name)) {
        return false;
      }
      seen.add(item.name);
      return true;
    });
};

const splitSelectedAbilityNames = (selectedAbilities) => {
  const declared_tool_names = [];
  const declared_skill_names = [];
  (Array.isArray(selectedAbilities) ? selectedAbilities : []).forEach((item) => {
    if (item.kind === "skill") {
      declared_skill_names.push(item.name);
      return;
    }
    declared_tool_names.push(item.name);
  });
  return {
    declared_tool_names: normalizeNameList(declared_tool_names),
    declared_skill_names: normalizeNameList(declared_skill_names),
  };
};

const collectSelectedAbilityNames = (list) =>
  normalizeNameList(collectSelectedAbilities(list).map((item) => item.name));

const selectedAbilityNamesFromPreset = (preset) =>
  normalizeNameList([
    ...(Array.isArray(preset?.tool_names) ? preset.tool_names : []),
    ...(Array.isArray(preset?.declared_tool_names) ? preset.declared_tool_names : []),
    ...(Array.isArray(preset?.declared_skill_names) ? preset.declared_skill_names : []),
  ]);

const renderPresetActionState = () => {
  const preset = selectedPreset();
  const hasPreset = Boolean(preset);
  const hasSavedPreset = Boolean(preset?.preset_id);
  const dirty = state.presetAgents.draftDirty === true;
  const saving = state.presetAgents.saving === true;
  if (elements.presetAgentSaveBtn) {
    elements.presetAgentSaveBtn.disabled = !hasPreset || !dirty || saving;
  }
  if (elements.presetAgentExportBtn) {
    elements.presetAgentExportBtn.disabled = !hasSavedPreset || dirty || saving;
  }
  if (elements.presetAgentDeleteBtn) {
    elements.presetAgentDeleteBtn.disabled = !hasPreset || isDefaultPreset(preset) || saving;
  }
};

const renderToolSelector = (selected) => {
  const list = elements.presetUserAgentTools;
  const empty = elements.presetUserAgentToolsEmpty;
  if (!list || !empty) {
    return;
  }
  const presetKey = currentToolListPresetKey();
  rememberToolListScroll(presetKey);
  list.textContent = "";
  const groups = Array.isArray(state.presetAgents.toolGroups) ? state.presetAgents.toolGroups : [];
  if (!groups.length) {
    empty.textContent = t("presetAgents.userAgent.toolsEmpty");
    empty.style.display = "block";
    list.scrollTop = 0;
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
      checkbox.dataset.kind = option.kind;
      checkbox.checked = selectedSet.has(option.value);
      checkbox.addEventListener("change", () => {
        markPresetDraftDirty();
      });
      groupCheckboxes.push(checkbox);
      const label = document.createElement("label");
      const desc = option.description ? `<span class="muted">${option.description}</span>` : "";
      label.innerHTML = `<strong>${option.label}</strong>${desc}`;
      row.addEventListener("click", (event) => {
        if (event.target === checkbox) {
          return;
        }
        checkbox.checked = !checkbox.checked;
        markPresetDraftDirty();
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
      markPresetDraftDirty();
    });
  });
  list.scrollTop = resolveToolListScrollTop(presetKey);
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

const markPresetDraftDirty = () => {
  ensureState();
  state.presetAgents.draftDirty = true;
  state.presetAgents.draftVersion = (Number(state.presetAgents.draftVersion) || 0) + 1;
  state.presetAgents.syncSummary = null;
  setStatus(t("presetAgents.status.dirty"), "warning");
  renderPresetActionState();
  renderSyncSummary();
};

const selectedPreset = () =>
  resolvePresetSelection({
    presetId: state.presetAgents.selectedPresetId,
    name: state.presetAgents.selectedPresetName,
  }) || null;

const currentToolListPresetKey = () => {
  const preset = selectedPreset();
  const presetId = normalizePresetId(preset?.preset_id);
  if (presetId) {
    return `id:${presetId}`;
  }
  const nameKey = normalizePresetNameKey(preset?.name);
  return nameKey ? `name:${nameKey}` : "";
};

const rememberToolListScroll = (presetKey = currentToolListPresetKey()) => {
  if (!presetKey || !elements.presetUserAgentTools) {
    return;
  }
  state.presetAgents.toolListScrollTopByPresetKey[presetKey] = elements.presetUserAgentTools.scrollTop;
};

const resolveToolListScrollTop = (presetKey = currentToolListPresetKey()) => {
  if (!presetKey) {
    return 0;
  }
  const raw = state.presetAgents.toolListScrollTopByPresetKey?.[presetKey];
  return Number.isFinite(Number(raw)) ? Number(raw) : 0;
};

const isDefaultPreset = (preset) =>
  Boolean(preset) &&
  (preset.is_default_agent === true || String(preset.preset_id || "").trim() === DEFAULT_AGENT_ID_ALIAS);

const buildEffectivePreset = (preset) => {
  if (!preset) {
    return null;
  }
  if (!isDefaultPreset(preset)) {
    return preset;
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
    textarea.addEventListener("input", () => {
      markPresetDraftDirty();
    });

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
      markPresetDraftDirty();
    });

    row.appendChild(badge);
    row.appendChild(textarea);
    row.appendChild(removeBtn);
    fragment.appendChild(row);
  });
  list.appendChild(fragment);
};

const fillAgentForm = (preset) => {
  renderToolSelector(selectedAbilityNamesFromPreset(preset));
  renderPresetQuestionEditor(preset?.preset_questions || []);
  elements.presetUserAgentApproval.value = preset?.approval_mode || "full_auto";
  elements.presetUserAgentHive.value = t("presetAgents.userAgent.hiveFixed");
};

const renderTabAvailability = () => {
  elements.presetAgentTabCron.disabled = true;
  elements.presetAgentTabChannels.disabled = true;
  if (state.presetAgents.activeTab !== "preset") {
    state.presetAgents.activeTab = "preset";
  }
};

const renderSyncSummary = () => {
  const preset = selectedPreset();
  const dirty = state.presetAgents.draftDirty === true;
  const canSync = Boolean(preset?.preset_id) && !dirty && state.presetAgents.saving !== true;
  elements.presetAgentSyncSafeBtn.disabled = !canSync || state.presetAgents.syncLoading;
  elements.presetAgentSyncForceBtn.disabled = !canSync || state.presetAgents.syncLoading;

  if (!preset || !preset.preset_id) {
    elements.presetAgentSyncSummary.textContent = t("presetAgents.sync.empty");
    return;
  }
  if (dirty) {
    elements.presetAgentSyncSummary.textContent = t("presetAgents.sync.saveRequired");
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
  const activePreset = selectedPreset();
  state.presetAgents.presets.forEach((preset) => {
    const row = document.createElement("button");
    row.type = "button";
    row.className = "preset-agent-item";
    if (isSamePreset(preset, activePreset)) {
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
      const draftState = await resolvePresetDraftForReload({
        selectedName: preset.name,
        selectedPresetId: preset.preset_id,
      });
      if (!draftState.ok || draftState.reloaded) {
        return;
      }
      setSelectedPreset(preset);
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
    fillPresetForm(null);
    fillAgentForm(null);
    renderPresetActionState();
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
  fillPresetForm(preset);
  fillAgentForm(preset);
  renderPresetActionState();
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
    const label = String(
      item?.display_name || item?.displayName || item?.name || item?.label || value
    ).trim();
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
    declared_tool_names: Array.isArray(preset.declared_tool_names) ? preset.declared_tool_names : [],
    declared_skill_names: Array.isArray(preset.declared_skill_names) ? preset.declared_skill_names : [],
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
  renderToolSelector(selectedAbilityNamesFromPreset(effectiveSelectedPreset()));
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

    if (isDefaultPreset(preset) && ensureAgent) {
      state.presetAgents.userAgent = await loadDefaultTemplateAgent();
    } else {
      state.presetAgents.userAgent = null;
    }
    fillAgentForm(effectiveSelectedPreset() || preset);
    state.presetAgents.cronJobs = [];
    state.presetAgents.channelAccounts = [];
    state.presetAgents.supportedChannels = [];
    await loadToolCatalog();
    renderCronList();
    renderChannelForms();
    renderChannelAccounts();
    renderPresetDetail();
    setStatus(t("presetAgents.status.ready", { user: TEMPLATE_USER_ID, agent: preset.name || "-" }), "success");
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

const waitForPresetSave = async () => {
  ensureState();
  if (!state.presetAgents.saving) {
    return true;
  }
  return (await state.presetAgents.savePromise) !== false;
};

const resolvePresetDraftForReload = async ({ selectedName = "", selectedPresetId = "" } = {}) => {
  const settled = await waitForPresetSave();
  if (!settled) {
    return { ok: false, reloaded: false };
  }
  if (!state.presetAgents.draftDirty) {
    return { ok: true, reloaded: false };
  }
  if (!window.confirm(t("presetAgents.confirmDiscard"))) {
    return { ok: false, reloaded: false };
  }
  state.presetAgents.draftDirty = false;
  state.presetAgents.syncSummary = null;
  await loadPresetAgents({
    silent: true,
    selectedName,
    selectedPresetId,
    flushDraft: false,
  });
  return { ok: true, reloaded: true };
};

const ensurePresetDraftSaved = async (errorKey) => {
  const settled = await waitForPresetSave();
  if (!settled) {
    return false;
  }
  if (!state.presetAgents.draftDirty) {
    return true;
  }
  setStatus(t("presetAgents.status.dirty"), "warning");
  notify(t(errorKey), "warning");
  return false;
};

const persistPresets = async ({ selectedName = "", selectedPresetId = "" } = {}) => {
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
  state.presetAgents.presets = stabilizePresetListOrder(saved?.data?.items, state.presetAgents.presets);
  if (!state.presetAgents.presets.length) {
    setSelectedPreset(null);
    return;
  }
  const preferred = resolvePresetSelection({ presetId: selectedPresetId, name: selectedName });
  if (preferred) {
    setSelectedPreset(preferred);
    return;
  }
  setSelectedPreset(state.presetAgents.presets[0]);
};

const savePreset = async ({ silentSuccess = false } = {}) => {
  if (state.presetAgents.saving) {
    return state.presetAgents.savePromise || false;
  }
  const draftVersion = Number(state.presetAgents.draftVersion) || 0;
  const saveTask = (async () => {
    try {
      const current = selectedPreset();
      const effective = effectiveSelectedPreset() || current;
      if (!current || !effective) {
        throw new Error(t("presetAgents.error.noPresetSelected"));
      }
      const draft = collectPresetForm();
      const next = { ...current, ...effective, ...draft };
      const duplicate = state.presetAgents.presets.find(
        (item) => item.name.toLowerCase() === next.name.toLowerCase() && !isSamePreset(item, current)
      );
      if (duplicate) {
        throw new Error(t("presetAgents.error.duplicateName", { name: next.name }));
      }
      const agentPayload = collectAgentForm();
      next.tool_names = agentPayload.tool_names;
      next.declared_tool_names = agentPayload.declared_tool_names;
      next.declared_skill_names = agentPayload.declared_skill_names;
      next.preset_questions = agentPayload.preset_questions;
      next.approval_mode = agentPayload.approval_mode;
      next.status = agentPayload.status || effective.status || current.status || "active";
      next.model_name = normalizeOptionalModelName(agentPayload.model_name);
      next.sandbox_container_id = agentPayload.sandbox_container_id;
      setStatus(t("presetAgents.status.saving"), "warning");
      renderPresetActionState();
      renderSyncSummary();
      if (isDefaultPreset(current)) {
        const savedDefaultPayload = await requestJson(`/agents/${encodeURIComponent(DEFAULT_AGENT_ID_ALIAS)}`, {
          method: "PUT",
          query: { user_id: TEMPLATE_USER_ID },
          body: { ...agentPayload, name: next.name },
        });
        const nextDefaultPreset = {
          ...current,
          ...next,
          is_default_agent: true,
          preset_id: current.preset_id,
          revision: Math.max(1, Number(current.revision) || 1),
        };
        state.presetAgents.presets = state.presetAgents.presets.map((item) =>
          isSamePreset(item, current) ? nextDefaultPreset : item
        );
        setSelectedPreset(nextDefaultPreset);
        state.presetAgents.userAgent = normalizeUserAgent(savedDefaultPayload?.data || {});
        await refreshContext({ ensureAgent: true, silent: true });
        await loadSyncSummary({ silent: true });
        renderAll();
      } else {
        state.presetAgents.presets = state.presetAgents.presets.map((item) =>
          isSamePreset(item, current) ? next : item
        );
        setSelectedPreset(next);
        await persistPresets({ selectedName: next.name, selectedPresetId: next.preset_id });
        await refreshContext({ ensureAgent: false, silent: true });
        await loadSyncSummary({ silent: true });
        renderAll();
      }
      if ((Number(state.presetAgents.draftVersion) || 0) === draftVersion) {
        state.presetAgents.draftDirty = false;
      } else {
        state.presetAgents.draftDirty = true;
      }
      setStatus(
        t(state.presetAgents.draftDirty ? "presetAgents.status.dirty" : "presetAgents.status.saved"),
        state.presetAgents.draftDirty ? "warning" : "success"
      );
      if (!silentSuccess) {
        notify(t("presetAgents.toast.savePresetSuccess"), "success");
      }
      return true;
    } catch (error) {
      state.presetAgents.draftDirty = true;
      setStatus(t("presetAgents.status.saveFailed", { message: error.message || "-" }), "error");
      notify(t("presetAgents.toast.savePresetFailed", { message: error.message || "-" }), "error");
      return false;
    } finally {
      state.presetAgents.saving = false;
      state.presetAgents.savePromise = null;
      renderPresetActionState();
      renderSyncSummary();
    }
  })();
  state.presetAgents.saving = true;
  state.presetAgents.savePromise = saveTask;
  return saveTask;
};

const exportPresetWorkerCard = async () => {
  try {
    const saved = await ensurePresetDraftSaved("presetAgents.error.saveBeforeExport");
    if (!saved) {
      return;
    }
    const preset = selectedPreset();
    if (!preset?.preset_id) {
      throw new Error(t("presetAgents.error.noPresetSelected"));
    }
    const payload = await requestJson(
      `/admin/preset_agents/${encodeURIComponent(preset.preset_id)}/worker_card`
    );
    const filename = String(payload?.data?.filename || "").trim() || "worker-card.json";
    const document = payload?.data?.document;
    if (!document || typeof document !== "object") {
      throw new Error("worker card payload missing");
    }
    downloadJsonFile(filename, document);
    notify(t("presetAgents.toast.exportWorkerCardSuccess", { filename }), "success");
  } catch (error) {
    notify(t("presetAgents.toast.exportWorkerCardFailed", { message: error.message || "-" }), "error");
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
  const createdPreset = {
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
  };
  state.presetAgents.presets.push(createdPreset);
  setSelectedPreset(createdPreset);
  state.presetAgents.userAgent = null;
  state.presetAgents.cronJobs = [];
  state.presetAgents.channelAccounts = [];
  state.presetAgents.supportedChannels = [];
  state.presetAgents.syncSummary = null;
  renderAll();
  setTab("preset");
  markPresetDraftDirty();
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
    state.presetAgents.presets = state.presetAgents.presets.filter((item) => !isSamePreset(item, current));
    setSelectedPreset(state.presetAgents.presets[0] || null);
    state.presetAgents.syncSummary = null;
    await persistPresets({
      selectedName: state.presetAgents.selectedPresetName,
      selectedPresetId: state.presetAgents.selectedPresetId,
    });
    if (selectedPreset()) {
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
  const selectedAbilities = collectSelectedAbilities(elements.presetUserAgentTools);
  const tool_names = normalizeNameList(selectedAbilities.map((item) => item.name));
  const { declared_tool_names, declared_skill_names } = splitSelectedAbilityNames(selectedAbilities);
  const preset = effectiveSelectedPreset() || selectedPreset();
  return {
    name,
    description: String(elements.presetAgentFormDescription.value || "").trim(),
    system_prompt: String(elements.presetAgentFormPrompt.value || "").trim(),
    model_name: normalizeOptionalModelName(elements.presetAgentFormModelName?.value),
    tool_names,
    declared_tool_names,
    declared_skill_names,
    preset_questions: collectPresetQuestionValues(),
    approval_mode:
      String(elements.presetUserAgentApproval.value || effectiveSelectedPreset()?.approval_mode || "full_auto").trim() ||
      "full_auto",
    sandbox_container_id: Number.isFinite(sandbox) && sandbox > 0 ? sandbox : 1,
    status: String(effectiveSelectedPreset()?.status || "active").trim() || "active",
    is_shared: false,
    hive_id: DEFAULT_HIVE_ID,
    icon: JSON.stringify({
      name: normalizeIconName(preset?.icon_name),
      color: normalizeIconColor(preset?.icon_color),
    }),
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
  const saved = await ensurePresetDraftSaved("presetAgents.error.saveBeforeSync");
  if (!saved) {
    return;
  }
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

export const loadPresetAgents = async ({
  silent = false,
  selectedName = "",
  selectedPresetId = "",
  flushDraft = true,
} = {}) => {
  ensureState();
  if (!ensureElements()) {
    return;
  }
  if (!(await waitForPresetSave())) {
    return;
  }
  if (flushDraft && state.presetAgents.draftDirty) {
    setStatus(t("presetAgents.status.dirty"), "warning");
    return;
  }
  state.presetAgents.loading = true;
  try {
    await loadModelCatalog({ silent: true });
    const payload = await requestJson("/admin/preset_agents");
    state.presetAgents.presets = stabilizePresetListOrder(payload?.data?.items, state.presetAgents.presets);

    const preferredName = String(selectedName || "").trim();
    const preferredPreset = preferredName
      ? resolvePresetSelection({ presetId: selectedPresetId, name: preferredName })
      : resolvePresetSelection({
          presetId: normalizePresetId(selectedPresetId) || state.presetAgents.selectedPresetId,
          name: state.presetAgents.selectedPresetName,
        });
    setSelectedPreset(preferredPreset || state.presetAgents.presets[0] || null);
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

const bindPresetDraftFields = () => {
  const textFields = [
    elements.presetAgentFormName,
    elements.presetAgentFormDescription,
    elements.presetAgentFormPrompt,
  ].filter(Boolean);
  textFields.forEach((field) => {
    if (field.dataset.autosaveBound === "1") {
      return;
    }
    field.dataset.autosaveBound = "1";
    field.addEventListener("input", () => {
      markPresetDraftDirty();
    });
  });

  const changeFields = [
    elements.presetAgentFormModelName,
    elements.presetAgentFormContainerId,
    elements.presetUserAgentApproval,
  ].filter(Boolean);
  changeFields.forEach((field) => {
    if (field.dataset.autosaveBound === "1") {
      return;
    }
    field.dataset.autosaveBound = "1";
    field.addEventListener("change", () => {
      markPresetDraftDirty();
    });
  });
};

const bindActions = () => {
  if (elements.presetUserAgentTools.dataset.scrollBound !== "1") {
    elements.presetUserAgentTools.dataset.scrollBound = "1";
    elements.presetUserAgentTools.addEventListener(
      "scroll",
      () => {
        rememberToolListScroll();
      },
      { passive: true }
    );
  }
  if (elements.presetAgentsRefreshBtn.dataset.bound !== "1") {
    elements.presetAgentsRefreshBtn.dataset.bound = "1";
    elements.presetAgentsRefreshBtn.addEventListener("click", async () => {
      const draftState = await resolvePresetDraftForReload({
        selectedName: state.presetAgents.selectedPresetName,
        selectedPresetId: state.presetAgents.selectedPresetId,
      });
      if (!draftState.ok || draftState.reloaded) {
        return;
      }
      await loadPresetAgents();
    });
  }
  if (elements.presetAgentCreateBtn.dataset.bound !== "1") {
    elements.presetAgentCreateBtn.dataset.bound = "1";
    elements.presetAgentCreateBtn.addEventListener("click", async () => {
      const draftState = await resolvePresetDraftForReload({
        selectedName: state.presetAgents.selectedPresetName,
        selectedPresetId: state.presetAgents.selectedPresetId,
      });
      if (!draftState.ok) {
        return;
      }
      createPreset();
    });
  }
  if (elements.presetAgentSaveBtn.dataset.bound !== "1") {
    elements.presetAgentSaveBtn.dataset.bound = "1";
    elements.presetAgentSaveBtn.addEventListener("click", async () => {
      await savePreset();
    });
  }
  if (elements.presetAgentExportBtn.dataset.bound !== "1") {
    elements.presetAgentExportBtn.dataset.bound = "1";
    elements.presetAgentExportBtn.addEventListener("click", exportPresetWorkerCard);
  }
  if (elements.presetAgentPresetQuestionAddBtn.dataset.bound !== "1") {
    elements.presetAgentPresetQuestionAddBtn.dataset.bound = "1";
    elements.presetAgentPresetQuestionAddBtn.addEventListener("click", () => {
      const nextDrafts = collectPresetQuestionDrafts();
      nextDrafts.push("");
      renderPresetQuestionEditor(nextDrafts);
      markPresetDraftDirty();
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
  bindPresetDraftFields();
  bindActions();
  renderAll();
  state.presetAgents.initialized = true;
  appendLog(t("presetAgents.init"));
};
