import { elements } from "./elements.js?v=20261007-01";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { appendLog } from "./log.js?v=20260518-01";
import { notify } from "./notify.js";
import { t } from "./i18n.js?v=20261007-01";
import { formatTimestamp } from "./utils.js?v=20251229-02";
import { listGlobalCompanions } from "./companions.js?v=20260506-01";
import { ensureOrgUnitsLoaded } from "./org-units.js?v=20260518-01";
import { isRemovedSwarmTool } from "../shared/deprecated-tools.js";
import {
  BINDING_DEFAULT_PAGE_SIZE,
  CONTRACT_STATE_UNAVAILABLE,
  CUSTOMIZABLE_FIELDS,
  CUSTOMIZABLE_FIELD_LABEL_KEYS,
  getContractState,
  listPresetAgents as listPresetAgentsContract,
  listPresetBindings,
  mutatePresetBindings,
  normalizeCustomizable,
  resetContractState,
  syncPresetAgents,
} from "./preset-bindings-api.js?v=20261007-01";

const TAB_KEYS = ["preset", "cron", "channels"];
const DEFAULT_AGENT_ID_ALIAS = "__default__";
const TEMPLATE_USER_ID = "preset_template";
const AVATAR_PAGE_SIZE = 24;
// 绑定用户表分页大小（page_size 上限由适配层收敛为 100）
const PRESET_BINDING_PAGE_SIZE = BINDING_DEFAULT_PAGE_SIZE;
// 绑定用户选择器分页大小
const PRESET_PICKER_PAGE_SIZE = 20;
// 影响面预览最多列举的用户数
const PRESET_IMPACT_PREVIEW_LIMIT = 12;
// 操作结果日志保留条数
const PRESET_SYNC_LOG_LIMIT = 50;
// 可自定义字段 -> 表单开关 id（顺序与契约 §12.2.1 一致）
const CUSTOMIZABLE_ELEMENT_KEYS = {
  system_prompt: "presetAgentCustomizableSystemPrompt",
  welcome: "presetAgentCustomizableWelcome",
  model_name: "presetAgentCustomizableModelName",
  reasoning_effort: "presetAgentCustomizableReasoningEffort",
  tool_names: "presetAgentCustomizableToolNames",
  approval_mode: "presetAgentCustomizableApprovalMode",
};
const CUSTOMIZABLE_FIELD_META = CUSTOMIZABLE_FIELDS.map((key) => ({
  key,
  elementKey: CUSTOMIZABLE_ELEMENT_KEYS[key],
  labelKey: CUSTOMIZABLE_FIELD_LABEL_KEYS[key],
}));
const PRESET_AVATAR_COLOR_OPTIONS = [
  "#f97316",
  "#ef4444",
  "#ec4899",
  "#8b5cf6",
  "#6366f1",
  "#3b82f6",
  "#06b6d4",
  "#14b8a6",
  "#10b981",
  "#84cc16",
  "#f59e0b",
  "#64748b",
  "#94a3b8",
];

const PRESET_AVATAR_BASE_URL = (() => {
  try {
    return new URL("../assets/agent-avatars/", import.meta.url).toString();
  } catch (_error) {
    return "/assets/agent-avatars/";
  }
})();

// Dynamic avatar config loaded from backend - populated after init
let PRESET_AGENT_AVATAR_EXTENSION_MAP = {};
let PRESET_AGENT_AVATAR_KEYS = [];
let PRESET_AVATAR_OPTIONS = [];
let PRESET_AVATAR_IMAGE_CANDIDATE_MAP = new Map();
let PRESET_AVATAR_OPTION_KEYS = new Set();
let DEFAULT_PRESET_AVATAR_ICON_NAME = "avatar-000";
let FALLBACK_PRESET_AVATAR_ICON_NAME = "initial";
const LEGACY_DEFAULT_AVATAR_KEY = "qq-avatar-0199";

async function loadPresetAvatars() {
  try {
    const response = await fetch(`${getWunderBase()}/admin/agent_avatars`);
    if (!response.ok) {
      console.warn("[preset-agents] failed to load avatar config");
      return;
    }
    const json = await response.json();
    const data = json?.data;
    if (!data || !Array.isArray(data.keys)) {
      return;
    }
    PRESET_AGENT_AVATAR_KEYS = data.keys;
    PRESET_AGENT_AVATAR_EXTENSION_MAP = data.extension_map || {};
    buildPresetAvatarOptions();
  } catch (err) {
    console.warn("[preset-agents] avatar config load error:", err);
  }
}

function buildPresetAvatarOptions() {
  const extensionCandidates = ["png", "jpg", "jpeg"];

  function buildImageCandidates(key) {
    const normalized = String(key || "").trim();
    if (!normalized) {
      return [];
    }
    const preferredExtension = String(PRESET_AGENT_AVATAR_EXTENSION_MAP[normalized] || "")
      .trim()
      .toLowerCase();
    return Array.from(
      new Set(
        [preferredExtension, ...extensionCandidates].filter((item) => Boolean(String(item || "").trim()))
      )
    ).map((extension) => `${PRESET_AVATAR_BASE_URL}${normalized}.${extension}`);
  }

  PRESET_AVATAR_OPTIONS = [{ key: "initial", image: "", imageCandidates: [], label: "initial" }].concat(
    PRESET_AGENT_AVATAR_KEYS.map((key) => {
      const imageCandidates = buildImageCandidates(key);
      return {
        key,
        image: imageCandidates[0] || "",
        imageCandidates,
        label: `Agent Avatar ${key.slice("avatar-".length)}`,
      };
    })
  );

  PRESET_AVATAR_IMAGE_CANDIDATE_MAP = new Map(
    PRESET_AVATAR_OPTIONS.filter((item) => Array.isArray(item.imageCandidates) && item.imageCandidates.length).map((item) => [
      item.key,
      item.imageCandidates,
    ])
  );
  PRESET_AVATAR_OPTION_KEYS = new Set(PRESET_AVATAR_OPTIONS.map((item) => item.key));
  DEFAULT_PRESET_AVATAR_ICON_NAME = PRESET_AVATAR_OPTION_KEYS.has("avatar-046")
    ? "avatar-046"
    : PRESET_AGENT_AVATAR_KEYS[0] || "initial";
}

const normalizeAgentAvatarSequenceKey = (rawValue) => {
  const sequence = Number.parseInt(rawValue, 10);
  if (!Number.isFinite(sequence) || sequence < 0) {
    return DEFAULT_PRESET_AVATAR_ICON_NAME;
  }
  const candidate = `avatar-${String(sequence).padStart(3, "0")}`;
  return PRESET_AVATAR_OPTION_KEYS.has(candidate) ? candidate : DEFAULT_PRESET_AVATAR_ICON_NAME;
};

const avatarModalState = {
  kind: "static",
  iconName: DEFAULT_PRESET_AVATAR_ICON_NAME,
  color: "#94a3b8",
  companionScope: "global",
  companionId: "",
  page: 1,
};

const buildVisibilityOrgTree = () => {
  const units = Array.isArray(state.orgUnits?.list) ? state.orgUnits.list : [];
  const byParent = new Map();
  units.forEach((unit) => {
    const key = String(unit.parent_id || "");
    if (!byParent.has(key)) {
      byParent.set(key, []);
    }
    byParent.get(key).push(unit);
  });
  byParent.forEach((list) => {
    list.sort((left, right) => String(left.path_name || left.name).localeCompare(String(right.path_name || right.name)));
  });
  const build = (parentId = "") =>
    (byParent.get(parentId) || []).map((unit) => ({
      ...unit,
      children: build(unit.unit_id),
    }));
  return build("");
};

const collectVisibilityDescendantIds = (node) => {
  const output = [node.unit_id];
  (Array.isArray(node.children) ? node.children : []).forEach((child) => {
    output.push(...collectVisibilityDescendantIds(child));
  });
  return output;
};

const renderPresetVisibilityTree = () => {
  const container = elements.presetAgentVisibilityTree;
  if (!container) {
    return;
  }
  container.textContent = "";
  const tree = buildVisibilityOrgTree();
  const allIds = [];
  const collectAll = (node) => {
    allIds.push(node.unit_id);
    (Array.isArray(node.children) ? node.children : []).forEach(collectAll);
  };
  tree.forEach(collectAll);
  const selected = new Set(
    (state.presetAgents.visibilityDraftIds || []).length
      ? state.presetAgents.visibilityDraftIds
      : allIds
  );
  const toggleNode = (node, checked) => {
    const ids = collectVisibilityDescendantIds(node);
    if (checked) {
      ids.forEach((id) => selected.add(id));
    } else {
      ids.forEach((id) => selected.delete(id));
    }
    state.presetAgents.visibilityDraftIds = Array.from(selected);
    renderPresetVisibilityTree();
  };
  const renderNode = (node, depth) => {
    const row = document.createElement("div");
    row.className = "tool-item";
    row.style.paddingLeft = `${8 + depth * 14}px`;
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = selected.has(node.unit_id);
    checkbox.addEventListener("change", () => toggleNode(node, checkbox.checked));
    const label = document.createElement("label");
    label.innerHTML = `<strong>${node.name || node.unit_id}</strong><span class="muted">${node.path_name || node.unit_id}</span>`;
    row.appendChild(checkbox);
    row.appendChild(label);
    container.appendChild(row);
    (Array.isArray(node.children) ? node.children : []).forEach((child) => renderNode(child, depth + 1));
  };
  tree.forEach((node) => renderNode(node, 0));
};

const openPresetVisibilityModal = () => {
  const preset = selectedPreset();
  if (!preset || !elements.presetAgentVisibilityModal) {
    return;
  }
  state.presetAgents.visibilityDraftIds = Array.isArray(preset.visible_unit_ids)
    ? [...preset.visible_unit_ids]
    : [];
  renderPresetVisibilityTree();
  elements.presetAgentVisibilityModal.classList.add("active");
};

const closePresetVisibilityModal = () => {
  elements.presetAgentVisibilityModal?.classList.remove("active");
};

const savePresetVisibilityModal = () => {
  const preset = selectedPreset();
  if (!preset) {
    return;
  }
  const tree = buildVisibilityOrgTree();
  const allIds = [];
  const collectAll = (node) => {
    allIds.push(node.unit_id);
    (Array.isArray(node.children) ? node.children : []).forEach(collectAll);
  };
  tree.forEach(collectAll);
  const selected = normalizeNameList(state.presetAgents.visibilityDraftIds || []);
  preset.visible_unit_ids = selected.length === allIds.length ? [] : selected;
  renderPresetDetail();
  markPresetDraftDirty();
  closePresetVisibilityModal();
};

const ensureState = () => {
  if (!state.presetAgents) {
    state.presetAgents = {
      presets: [],
      selectedPresetName: "",
      selectedPresetId: "",
      activeTab: "preset",
      userAgent: null,
      syncPreview: null,
      syncLoading: false,
      syncRequestToken: 0,
      syncLog: [],
      bindings: null,
      picker: null,
      toolGroups: [],
      modelOptions: [],
      defaultModelName: "",
      companions: [],
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
      visibilityDraftIds: [],
    };
  }
  if (typeof state.presetAgents.selectedPresetId !== "string") {
    state.presetAgents.selectedPresetId = "";
  }
  if (!Array.isArray(state.presetAgents.syncLog)) {
    state.presetAgents.syncLog = [];
  }
  if (!("bindings" in state.presetAgents)) {
    state.presetAgents.bindings = null;
  }
  if (!("picker" in state.presetAgents)) {
    state.presetAgents.picker = null;
  }
  if (!state.panelLoaded) {
    state.panelLoaded = {};
  }
  if (typeof state.panelLoaded.presetAgents !== "boolean") {
    state.panelLoaded.presetAgents = false;
  }
};

// 绑定用户区块状态：分页、筛选、选中集合与契约就绪标记
const ensureBindingsState = () => {
  ensureState();
  if (!state.presetAgents.bindings || typeof state.presetAgents.bindings !== "object") {
    state.presetAgents.bindings = {
      presetId: "",
      items: [],
      total: 0,
      page: 1,
      pageSize: PRESET_BINDING_PAGE_SIZE,
      keyword: "",
      loading: false,
      requestToken: 0,
      selected: new Set(),
      loaded: false,
    };
  }
  const bindings = state.presetAgents.bindings;
  if (!(bindings.selected instanceof Set)) {
    bindings.selected = new Set(Array.isArray(bindings.selected) ? bindings.selected : []);
  }
  if (!Array.isArray(bindings.items)) {
    bindings.items = [];
  }
  if (!Number.isFinite(Number(bindings.page)) || Number(bindings.page) < 1) {
    bindings.page = 1;
  }
  if (!Number.isFinite(Number(bindings.pageSize)) || Number(bindings.pageSize) <= 0) {
    bindings.pageSize = PRESET_BINDING_PAGE_SIZE;
  }
  if (!Number.isFinite(Number(bindings.total)) || Number(bindings.total) < 0) {
    bindings.total = 0;
  }
  bindings.keyword = String(bindings.keyword || "").trim();
  return bindings;
};

const ensurePickerState = () => {
  ensureState();
  if (!state.presetAgents.picker || typeof state.presetAgents.picker !== "object") {
    state.presetAgents.picker = {
      items: [],
      total: 0,
      page: 1,
      pageSize: PRESET_PICKER_PAGE_SIZE,
      keyword: "",
      loading: false,
      requestToken: 0,
      selected: new Set(),
    };
  }
  const picker = state.presetAgents.picker;
  if (!(picker.selected instanceof Set)) {
    picker.selected = new Set();
  }
  if (!Array.isArray(picker.items)) {
    picker.items = [];
  }
  return picker;
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
  "presetAgentVisibilityBtn",
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
  "presetAgentPreviewSkill",
  "presetAgentFormModelName",
  "presetAgentCustomizableSystemPrompt",
  "presetAgentCustomizableWelcome",
  "presetAgentCustomizableModelName",
  "presetAgentCustomizableReasoningEffort",
  "presetAgentCustomizableToolNames",
  "presetAgentCustomizableApprovalMode",
  "presetAgentSyncContractBadge",
  "presetAgentSyncLog",
  "presetAgentSyncLogClearBtn",
  "presetBindingContractBadge",
  "presetBindingSearchInput",
  "presetBindingSearchBtn",
  "presetBindingReloadBtn",
  "presetBindingAddBtn",
  "presetBindingRebindBtn",
  "presetBindingUnbindBtn",
  "presetBindingSummary",
  "presetBindingTableBody",
  "presetBindingSelectAll",
  "presetBindingEmpty",
  "presetBindingPagination",
  "presetBindingPageInfo",
  "presetBindingPrevBtn",
  "presetBindingNextBtn",
  "presetBindingPickerModal",
  "presetBindingPickerClose",
  "presetBindingPickerCancel",
  "presetBindingPickerConfirm",
  "presetBindingPickerSearch",
  "presetBindingPickerSearchBtn",
  "presetBindingPickerSelectAll",
  "presetBindingPickerSummary",
  "presetBindingPickerList",
  "presetBindingPickerEmpty",
  "presetBindingPickerPageInfo",
  "presetBindingPickerPrevBtn",
  "presetBindingPickerNextBtn",
  "presetImpactModal",
  "presetImpactClose",
  "presetImpactCancel",
  "presetImpactConfirm",
  "presetImpactTitle",
  "presetImpactSummary",
  "presetImpactList",
  "presetImpactPresetRow",
  "presetImpactPresetLabel",
  "presetImpactPresetSelect",
  "presetImpactAckRow",
  "presetImpactAck",
  "presetImpactAckLabel",
  "presetImpactHint",
  "presetAgentAvatarTrigger",
  "presetAgentAvatarPreview",
  "presetAgentAvatarModal",
  "presetAgentAvatarModalClose",
  "presetAgentAvatarModalCancel",
  "presetAgentAvatarModalApply",
  "presetAgentAvatarModalReset",
  "presetAgentAvatarModalPreview",
  "presetAgentAvatarStaticTab",
  "presetAgentAvatarGlobalTab",
  "presetAgentVisibilityModal",
  "presetAgentVisibilityModalClose",
  "presetAgentVisibilityModalCancel",
  "presetAgentVisibilityModalSave",
  "presetAgentVisibilityTree",
  "presetAgentAvatarPicker",
  "presetAgentAvatarPager",
  "presetAgentAvatarPagePrev",
  "presetAgentAvatarPageIndicator",
  "presetAgentAvatarPageNext",
  "presetAgentAvatarColorRow",
  "presetAgentAvatarColorChip",
  "presetAgentAvatarColorSelect",
  "presetAgentPresetQuestions",
  "presetAgentPresetQuestionsEmpty",
  "presetAgentPresetQuestionAddBtn",
  "presetUserAgentTools",
  "presetUserAgentToolsEmpty",
  "presetUserAgentApproval",
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

const normalizeLegacyAvatarName = (value) => {
  const text = String(value || "")
    .trim()
    .toLowerCase();
  if (!text) {
    return "";
  }
  const agentAvatarMatch = text.match(/^agent-avatar-(\d{1,4})$/);
  if (agentAvatarMatch) {
    return normalizeAgentAvatarSequenceKey(agentAvatarMatch[1]);
  }
  const directAvatarMatch = text.match(/^avatar-(\d{1,4})$/);
  if (directAvatarMatch) {
    return normalizeAgentAvatarSequenceKey(directAvatarMatch[1]);
  }
  const qqAvatarMatch = text.match(/^qq-avatar-(\d{1,4})$/);
  if (qqAvatarMatch) {
    const normalized = `qq-avatar-${String(Number.parseInt(qqAvatarMatch[1], 10)).padStart(4, "0")}`;
    if (normalized === LEGACY_DEFAULT_AVATAR_KEY) {
      return DEFAULT_PRESET_AVATAR_ICON_NAME;
    }
    return normalizeAgentAvatarSequenceKey(qqAvatarMatch[1]);
  }
  if (text === "default") {
    return DEFAULT_PRESET_AVATAR_ICON_NAME;
  }
  return text;
};

const normalizeIconName = (
  value,
  { fallbackWhenEmpty = DEFAULT_PRESET_AVATAR_ICON_NAME, fallbackWhenUnknown = FALLBACK_PRESET_AVATAR_ICON_NAME } = {}
) => {
  const normalizedLegacy = normalizeLegacyAvatarName(value);
  if (!normalizedLegacy) {
    return fallbackWhenEmpty;
  }
  if (normalizedLegacy === "initial") {
    return "initial";
  }
  if (PRESET_AVATAR_OPTION_KEYS.has(normalizedLegacy)) {
    return normalizedLegacy;
  }
  return fallbackWhenUnknown;
};

const resolveAvatarImageCandidatesByKey = (value) => {
  const normalized = String(value || "").trim();
  return PRESET_AVATAR_IMAGE_CANDIDATE_MAP.get(normalized) || [];
};

const bindAvatarImageSource = (image, candidates, onExhausted) => {
  const urls = Array.from(
    new Set((Array.isArray(candidates) ? candidates : []).map((item) => String(item || "").trim()).filter(Boolean))
  );
  if (!urls.length) {
    if (typeof onExhausted === "function") {
      onExhausted();
    }
    return false;
  }
  let index = 0;
  const loadNext = () => {
    if (index >= urls.length) {
      image.removeAttribute("src");
      if (typeof onExhausted === "function") {
        onExhausted();
      }
      return;
    }
    image.src = urls[index];
    index += 1;
  };
  image.addEventListener("error", loadNext);
  loadNext();
  return true;
};

const resolveAvatarInitial = (value) => {
  const cleaned = String(value || "").trim();
  return cleaned ? cleaned.slice(0, 1).toUpperCase() : "?";
};

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

const tryParseIconObject = (value) => {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    return value;
  }
  const text = String(value || "").trim();
  if (!text || !text.startsWith("{")) {
    return null;
  }
  try {
    const parsed = JSON.parse(text);
    return parsed && typeof parsed === "object" && !Array.isArray(parsed) ? parsed : null;
  } catch (_error) {
    return null;
  }
};

const normalizeIconKind = (value) =>
  String(value || "").trim().toLowerCase() === "companion" ? "companion" : "static";

const normalizeCompanionScope = (value) =>
  String(value || "").trim().toLowerCase() === "private" ? "private" : "global";

const normalizeCompanionScale = (value) => {
  const numeric = Number(value);
  if (!Number.isFinite(numeric)) {
    return 1;
  }
  return Math.min(1.6, Math.max(0.7, Number(numeric.toFixed(1))));
};

const parseIconConfig = (value, fallback = {}) => {
  const parsed = tryParseIconObject(value);
  const source = parsed || {};
  const rawName = parsed
    ? source.name || source.icon || source.avatar_icon || source.avatarIcon || ""
    : String(value || "").trim();
  const rawColor = parsed ? source.color || source.avatar_color || source.avatarColor || "" : "";
  const rawKind = normalizeIconKind(parsed ? source.kind || source.type : "");
  const companionId = String(
    parsed ? source.id || source.companion_id || source.companionId || "" : ""
  ).trim();
  const fallbackName = fallback.icon_name || fallback.iconName || DEFAULT_PRESET_AVATAR_ICON_NAME;
  const fallbackColor = fallback.icon_color || fallback.iconColor || "#94a3b8";
  const iconName = normalizeIconName(rawName || companionId || fallbackName, {
    fallbackWhenEmpty: DEFAULT_PRESET_AVATAR_ICON_NAME,
    fallbackWhenUnknown: FALLBACK_PRESET_AVATAR_ICON_NAME,
  });
  const kind = rawKind === "companion" || companionId ? "companion" : "static";
  return {
    kind,
    name: iconName,
    color: normalizeIconColor(rawColor || fallbackColor),
    scope: normalizeCompanionScope(parsed ? source.scope : "global"),
    id: companionId,
    show: parsed && "show" in source ? source.show !== false : true,
    messageHints:
      parsed && ("messageHints" in source || "message_hints" in source)
        ? source.messageHints !== false && source.message_hints !== false
        : true,
    scale: normalizeCompanionScale(parsed ? source.scale : 1),
  };
};

const stringifyIconConfig = (config) => {
  const icon = parseIconConfig(config);
  if (icon.kind === "companion" && icon.id) {
    return JSON.stringify({
      kind: "companion",
      scope: normalizeCompanionScope(icon.scope),
      id: icon.id,
      color: normalizeIconColor(icon.color),
      show: icon.show !== false,
      messageHints: icon.messageHints !== false,
      scale: normalizeCompanionScale(icon.scale),
    });
  }
  return JSON.stringify({
    kind: "static",
    name: normalizeIconName(icon.name),
    color: normalizeIconColor(icon.color),
  });
};

const normalizeCompanionRecord = (item) => ({
  id: String(item?.id || "").trim(),
  display_name: String(item?.display_name || item?.displayName || item?.name || "").trim(),
  description: String(item?.description || "").trim(),
  spritesheet_url: String(item?.spritesheet_url || item?.spritesheetUrl || "").trim(),
  spritesheet_data_url: String(item?.spritesheet_data_url || item?.spritesheetDataUrl || "").trim(),
});

const buildDefaultStaticIconConfig = () => ({
  kind: "static",
  name: DEFAULT_PRESET_AVATAR_ICON_NAME,
  color: "#94a3b8",
});

const deriveLegacyIconParts = (icon) => {
  const config = parseIconConfig(icon);
  return {
    icon_name: normalizeIconName(config.kind === "static" ? config.name : config.name || DEFAULT_PRESET_AVATAR_ICON_NAME),
    icon_color: normalizeIconColor(config.color),
  };
};

const assignIconConfig = (target, icon) => {
  if (!target || typeof target !== "object") {
    return parseIconConfig(icon);
  }
  const normalizedIcon = stringifyIconConfig(icon);
  const legacy = deriveLegacyIconParts(normalizedIcon);
  target.icon = normalizedIcon;
  target.icon_name = legacy.icon_name;
  target.icon_color = legacy.icon_color;
  return parseIconConfig(normalizedIcon);
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
  const normalized = raw.replace(/[\s-]+/g, "_");
  if (normalized === "embed" || normalized === "emb" || normalized === "embeddings") {
    return "embedding";
  }
  if (
    normalized === "asr" ||
    normalized === "stt" ||
    normalized === "speech_to_text" ||
    normalized === "speech2text" ||
    normalized === "audio_transcription" ||
    normalized === "transcription" ||
    normalized === "audio_to_text"
  ) {
    return "asr";
  }
  if (
    normalized === "tts" ||
    normalized === "speech" ||
    normalized === "text_to_speech" ||
    normalized === "text2speech" ||
    normalized === "audio_speech"
  ) {
    return "tts";
  }
  if (
    normalized === "image" ||
    normalized === "draw" ||
    normalized === "drawing" ||
    normalized === "text_to_image" ||
    normalized === "text2image" ||
    normalized === "image_generation"
  ) {
    return "image";
  }
  return normalized === "embedding" ? "embedding" : "llm";
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

const normalizePreset = (item) => {
  const icon = parseIconConfig(item?.icon, {
    icon_name: item?.icon_name,
    icon_color: item?.icon_color,
  });
  return {
    preset_id: String(item?.preset_id || "").trim(),
    is_default_agent: item?.is_default_agent === true,
    revision: Number.isFinite(Number(item?.revision)) ? Number(item.revision) : 1,
    name: String(item?.name || "").trim(),
    description: String(item?.description || "").trim(),
    system_prompt: String(item?.system_prompt || "").trim(),
    preview_skill: item?.preview_skill === true,
    model_name: normalizeOptionalModelName(item?.model_name || item?.modelName),
    icon: stringifyIconConfig(icon),
    icon_name: normalizeIconName(icon.kind === "static" ? icon.name : DEFAULT_PRESET_AVATAR_ICON_NAME),
    icon_color: normalizeIconColor(icon.color),
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
    bound_users: Number.isFinite(Number(item?.bound_users)) ? Math.max(0, Number(item.bound_users)) : null,
    customizable: normalizeCustomizable(item?.customizable),
    updated_at: String(item?.updated_at || item?.updatedAt || "").trim(),
  };
};

const normalizePresetItems = (items) =>
  (Array.isArray(items) ? items : []).map(normalizePreset).filter((item) => item.name);

// 更新时间兼容秒级/毫秒级时间戳与 ISO 字符串，无法识别时返回占位符
const formatPresetTimestamp = (value) => {
  if (value === undefined || value === null || value === "") {
    return "-";
  }
  const text = String(value).trim();
  const numeric = Number(text);
  if (Number.isFinite(numeric) && /^-?\d+(\.\d+)?$/.test(text)) {
    const ms = Math.abs(numeric) < 1e12 ? numeric * 1000 : numeric;
    return formatTimestamp(ms);
  }
  return formatTimestamp(text);
};

const presetBoundUsersText = (preset) => {
  const bound = preset?.bound_users;
  // 契约缺字段时必须显示占位符，不能把「未知」显示成 0
  if (typeof bound !== "number" || !Number.isFinite(bound)) {
    return t("presetAgents.list.boundUsersUnknown");
  }
  return t("presetAgents.list.boundUsers", { count: Math.max(0, Math.round(bound)) });
};

const presetModelText = (preset) =>
  normalizeOptionalModelName(preset?.model_name) || t("presetAgents.list.modelDefault");

// 可选的「目标预设」列表（排除当前预设本身）
const bindingTargetPresets = (excludePresetId = "") => {
  const excluded = normalizePresetId(excludePresetId);
  return state.presetAgents.presets.filter((item) => normalizePresetId(item.preset_id) !== excluded);
};

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
  preview_skill: item?.preview_skill === true,
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
  icon: stringifyIconConfig(parseIconConfig(item?.icon, {
    icon_name: item?.icon_name,
    icon_color: item?.icon_color,
  })),
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
  // 蜂群工具已全链路移除：预设智能体工具选择器不再列出（待后端移除后一并删除该过滤）
  if (isRemovedSwarmTool(item)) {
    return null;
  }
  const runtimeName = item.runtime_name || item.name || item.tool_name || item.toolName;
  const cleanedRuntimeName = String(runtimeName || "").trim();
  if (!cleanedRuntimeName) {
    return null;
  }
  const displayName = item.display_name || item.displayName || item.title || item.label || cleanedRuntimeName;
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

const selectedAbilityNamesFromPreset = (preset) =>
  normalizeNameList([
    ...(Array.isArray(preset?.tool_names) ? preset.tool_names : []),
    ...(Array.isArray(preset?.declared_tool_names) ? preset.declared_tool_names : []),
    ...(Array.isArray(preset?.declared_skill_names) ? preset.declared_skill_names : []),
  ]);

const renderPresetActionState = () => {
  const preset = selectedPreset();
  const hasPreset = Boolean(preset);
  const dirty = state.presetAgents.draftDirty === true;
  const saving = state.presetAgents.saving === true;
  if (elements.presetAgentSaveBtn) {
    elements.presetAgentSaveBtn.disabled = !hasPreset || !dirty || saving;
  }
  if (elements.presetAgentDeleteBtn) {
    elements.presetAgentDeleteBtn.disabled = !hasPreset || isDefaultPreset(preset) || saving;
  }
  if (elements.presetAgentAvatarTrigger) {
    elements.presetAgentAvatarTrigger.disabled = !hasPreset || saving;
  }
  if (elements.presetAgentVisibilityBtn) {
    elements.presetAgentVisibilityBtn.disabled = !hasPreset || saving;
  }
};

const presetVisibilitySummary = (preset) => {
  const ids = Array.isArray(preset?.visible_unit_ids) ? preset.visible_unit_ids.filter(Boolean) : [];
  if (!ids.length) {
    return t("visibility.all");
  }
  return t("visibility.scoped");
};

const editPresetVisibility = async () => {
  await ensureOrgUnitsLoaded({ silent: true });
  openPresetVisibilityModal();
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

const setStatus = (text, kind = "") => {
  elements.presetAgentsStatusText.textContent = String(text || "").trim();
  elements.presetAgentsStatusText.dataset.kind = kind;
};

const markPresetDraftDirty = () => {
  ensureState();
  state.presetAgents.draftDirty = true;
  state.presetAgents.draftVersion = (Number(state.presetAgents.draftVersion) || 0) + 1;
  state.presetAgents.syncPreview = null;
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
  const effectiveIcon = agent.icon || preset.icon;
  const legacyIconParts = deriveLegacyIconParts(effectiveIcon);
  return {
    ...preset,
    description: agent.description,
    system_prompt: agent.system_prompt,
    model_name: configuredModelName || normalizeOptionalModelName(preset.model_name),
    icon: effectiveIcon,
    icon_name: legacyIconParts.icon_name,
    icon_color: legacyIconParts.icon_color,
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

const staticAvatarItems = () =>
  PRESET_AVATAR_OPTIONS.map((item) => ({ ...item, kind: "static" }));

const globalCompanionItems = () =>
  (Array.isArray(state.presetAgents.companions) ? state.presetAgents.companions : [])
    .map((item) => ({ ...item, kind: "companion" }));

const avatarPickerItems = () =>
  avatarModalState.kind === "companion" ? globalCompanionItems() : staticAvatarItems();

const resolveAvatarPageCount = () => Math.max(1, Math.ceil(avatarPickerItems().length / AVATAR_PAGE_SIZE));

const normalizeAvatarPage = (value) => {
  const pageCount = resolveAvatarPageCount();
  const parsed = Number.parseInt(value, 10);
  if (!Number.isFinite(parsed)) {
    return 1;
  }
  return Math.min(Math.max(parsed, 1), pageCount);
};

const resolveAvatarPageByKey = (key) => {
  const normalized = normalizeIconName(key, {
    fallbackWhenEmpty: DEFAULT_PRESET_AVATAR_ICON_NAME,
    fallbackWhenUnknown: FALLBACK_PRESET_AVATAR_ICON_NAME,
  });
  const index = PRESET_AVATAR_OPTIONS.findIndex((item) => item.key === normalized);
  if (index < 0) {
    return 1;
  }
  return Math.floor(index / AVATAR_PAGE_SIZE) + 1;
};

const resolveCompanionPageById = (id) => {
  const cleaned = String(id || "").trim();
  const items = globalCompanionItems();
  const index = items.findIndex((item) => item.id === cleaned);
  if (index < 0) {
    return 1;
  }
  return Math.floor(index / AVATAR_PAGE_SIZE) + 1;
};

const buildIconFromAvatarModalState = () => {
  if (avatarModalState.kind === "companion" && avatarModalState.companionId) {
    return {
      kind: "companion",
      scope: "global",
      id: String(avatarModalState.companionId || "").trim(),
      color: normalizeIconColor(avatarModalState.color),
      show: true,
      messageHints: true,
      scale: 1,
    };
  }
  return {
    kind: "static",
    name: normalizeIconName(avatarModalState.iconName, {
      fallbackWhenEmpty: DEFAULT_PRESET_AVATAR_ICON_NAME,
      fallbackWhenUnknown: FALLBACK_PRESET_AVATAR_ICON_NAME,
    }),
    color: normalizeIconColor(avatarModalState.color),
  };
};

const ensureAvatarFace = (container) => {
  if (!container) {
    return null;
  }
  return container;
};

const renderAvatarFace = (container, { iconName, color, initial, imageClass = "", initialClass = "" }) => {
  const target = ensureAvatarFace(container);
  if (!target) {
    return;
  }
  const normalizedIcon = normalizeIconName(iconName, {
    fallbackWhenEmpty: DEFAULT_PRESET_AVATAR_ICON_NAME,
    fallbackWhenUnknown: FALLBACK_PRESET_AVATAR_ICON_NAME,
  });
  const normalizedColor = normalizeIconColor(color);
  const normalizedInitial = resolveAvatarInitial(initial);
  const imageCandidates = resolveAvatarImageCandidatesByKey(normalizedIcon);
  target.textContent = "";
  target.style.background = imageCandidates.length ? "transparent" : normalizedColor;

  if (imageCandidates.length) {
    const image = document.createElement("img");
    if (imageClass) {
      image.className = imageClass;
    }
    image.alt = "";
    target.appendChild(image);
    bindAvatarImageSource(image, imageCandidates, () => {
      target.textContent = "";
      target.style.background = normalizedColor;
      const fallback = document.createElement("span");
      fallback.className = initialClass;
      fallback.textContent = normalizedInitial;
      target.appendChild(fallback);
    });
    return;
  }

  const label = document.createElement("span");
  label.className = initialClass;
  label.textContent = normalizedInitial;
  target.appendChild(label);
};

const renderCompanionFace = (container, companion) => {
  const target = ensureAvatarFace(container);
  if (!target) {
    return;
  }
  target.textContent = "";
  target.style.background = "transparent";
  const source = String(companion?.spritesheet_data_url || companion?.spritesheet_url || "").trim();
  if (!source) {
    const fallback = document.createElement("span");
    fallback.className = "preset-agent-avatar-option-initial";
    fallback.textContent = resolveAvatarInitial(companion?.display_name || selectedPreset()?.name);
    fallback.style.background = normalizeIconColor(avatarModalState.color);
    target.appendChild(fallback);
    return;
  }
  const viewport = document.createElement("span");
  viewport.className = "preset-agent-avatar-companion-preview";
  const sheet = document.createElement("span");
  sheet.className = "preset-agent-avatar-companion-sheet";
  sheet.style.backgroundImage = `url("${source}")`;
  viewport.appendChild(sheet);
  target.appendChild(viewport);
};

const findGlobalCompanion = (id) =>
  (Array.isArray(state.presetAgents.companions) ? state.presetAgents.companions : [])
    .find((item) => item.id === String(id || "").trim()) || null;

const renderPresetIconFace = (container, icon, initial) => {
  const config = parseIconConfig(icon);
  if (config.kind === "companion") {
    const companion = findGlobalCompanion(config.id);
    renderCompanionFace(container, companion || { display_name: config.id });
    return;
  }
  renderAvatarFace(container, {
    iconName: config.name,
    color: config.color,
    initial,
    initialClass: "preset-agent-avatar-option-initial",
  });
};

const renderPresetAvatarTrigger = (preset) => {
  const initial = resolveAvatarInitial(preset?.name);
  renderPresetIconFace(elements.presetAgentAvatarPreview, preset?.icon, initial);
  elements.presetAgentAvatarTrigger.disabled = !selectedPreset();
};

const renderPresetAvatarColorOptions = () => {
  const select = elements.presetAgentAvatarColorSelect;
  if (!select || select.dataset.rendered === "1") {
    return;
  }
  select.textContent = "";
  PRESET_AVATAR_COLOR_OPTIONS.forEach((color) => {
    const option = document.createElement("option");
    option.value = color;
    option.textContent = color.toUpperCase();
    select.appendChild(option);
  });
  select.dataset.rendered = "1";
};

const syncPresetAvatarColorControl = () => {
  const color = normalizeIconColor(avatarModalState.color);
  avatarModalState.color = color;
  elements.presetAgentAvatarColorChip.style.setProperty("--avatar-color", color);
  if (!Array.from(elements.presetAgentAvatarColorSelect.options).some((item) => item.value === color)) {
    const option = document.createElement("option");
    option.value = color;
    option.textContent = color.toUpperCase();
    elements.presetAgentAvatarColorSelect.appendChild(option);
  }
  elements.presetAgentAvatarColorSelect.value = color;
};

const renderPresetAvatarPicker = () => {
  const picker = elements.presetAgentAvatarPicker;
  if (!picker) {
    return;
  }
  const pageCount = resolveAvatarPageCount();
  avatarModalState.page = normalizeAvatarPage(avatarModalState.page);
  const start = (avatarModalState.page - 1) * AVATAR_PAGE_SIZE;
  const items = avatarPickerItems().slice(start, start + AVATAR_PAGE_SIZE);
  picker.textContent = "";
  const fragment = document.createDocumentFragment();

  if (!items.length) {
    const empty = document.createElement("div");
    empty.className = "preset-agent-avatar-empty";
    empty.textContent = avatarModalState.kind === "companion"
      ? t("presetAgents.avatarModal.globalEmpty")
      : t("presetAgents.avatarModal.staticEmpty");
    fragment.appendChild(empty);
  }

  items.forEach((item) => {
    const option = document.createElement("button");
    option.type = "button";
    option.className = "preset-agent-avatar-option";
    const optionKey = item.kind === "companion" ? item.id : item.key;
    option.classList.toggle(
      "is-active",
      item.kind === "companion"
        ? item.id === avatarModalState.companionId
        : item.key === avatarModalState.iconName
    );
    option.dataset.avatarKey = optionKey;
    const label = item.kind === "companion"
      ? item.display_name
      : item.key === "initial"
        ? t("presetAgents.avatarModal.initial")
        : item.label;
    option.title = label;
    option.setAttribute("aria-label", label);

    if (item.kind === "companion") {
      renderCompanionFace(option, item);
    } else if (item.imageCandidates?.length) {
      const image = document.createElement("img");
      image.className = "preset-agent-avatar-option-image";
      image.alt = "";
      option.appendChild(image);
      bindAvatarImageSource(image, item.imageCandidates, () => {
        option.textContent = "";
        const fallback = document.createElement("span");
        fallback.className = "preset-agent-avatar-option-initial";
        fallback.textContent = resolveAvatarInitial(selectedPreset()?.name);
        fallback.style.background = normalizeIconColor(avatarModalState.color);
        option.appendChild(fallback);
      });
    } else {
      const initial = document.createElement("span");
      initial.className = "preset-agent-avatar-option-initial";
      initial.textContent = resolveAvatarInitial(selectedPreset()?.name);
      initial.style.background = normalizeIconColor(avatarModalState.color);
      option.appendChild(initial);
    }

    option.addEventListener("click", () => {
      if (item.kind === "companion") {
        avatarModalState.kind = "companion";
        avatarModalState.companionScope = "global";
        avatarModalState.companionId = item.id;
        avatarModalState.page = resolveCompanionPageById(item.id);
      } else {
        avatarModalState.kind = "static";
        avatarModalState.iconName = item.key;
        avatarModalState.page = resolveAvatarPageByKey(item.key);
      }
      renderPresetAvatarModalState();
    });
    fragment.appendChild(option);
  });

  picker.appendChild(fragment);
  elements.presetAgentAvatarPageIndicator.textContent = t("presetAgents.avatarModal.pageIndicator", {
    current: avatarModalState.page,
    total: pageCount,
  });
  elements.presetAgentAvatarPagePrev.disabled = avatarModalState.page <= 1;
  elements.presetAgentAvatarPageNext.disabled = avatarModalState.page >= pageCount;
  elements.presetAgentAvatarPager.style.display = pageCount > 1 ? "flex" : "none";
};

const renderPresetAvatarModalState = () => {
  const kind = avatarModalState.kind === "companion" ? "companion" : "static";
  avatarModalState.kind = kind;
  const companions = globalCompanionItems();
  const iconName = normalizeIconName(avatarModalState.iconName, {
    fallbackWhenEmpty: DEFAULT_PRESET_AVATAR_ICON_NAME,
    fallbackWhenUnknown: FALLBACK_PRESET_AVATAR_ICON_NAME,
  });
  const color = normalizeIconColor(avatarModalState.color);
  avatarModalState.iconName = iconName;
  avatarModalState.color = color;
  avatarModalState.companionScope = "global";
  if (kind === "companion" && !companions.some((item) => item.id === avatarModalState.companionId)) {
    avatarModalState.companionId = companions[0]?.id || "";
  }
  if (kind === "companion") {
    avatarModalState.page = normalizeAvatarPage(avatarModalState.page || resolveCompanionPageById(avatarModalState.companionId));
  }

  if (kind === "companion") {
    renderCompanionFace(elements.presetAgentAvatarModalPreview, findGlobalCompanion(avatarModalState.companionId));
  } else {
    renderAvatarFace(elements.presetAgentAvatarModalPreview, {
      iconName,
      color,
      initial: resolveAvatarInitial(selectedPreset()?.name),
      initialClass: "preset-agent-avatar-option-initial",
    });
  }
  if (kind === "companion" && !avatarModalState.companionId) {
    elements.presetAgentAvatarModalApply.disabled = true;
  } else {
    elements.presetAgentAvatarModalApply.disabled = false;
  }
  const hasImage = kind === "companion" || resolveAvatarImageCandidatesByKey(iconName).length > 0;
  elements.presetAgentAvatarColorRow.style.display = hasImage ? "none" : "";
  syncPresetAvatarColorControl();
  elements.presetAgentAvatarStaticTab.classList.toggle("is-active", kind === "static");
  elements.presetAgentAvatarGlobalTab.classList.toggle("is-active", kind === "companion");
  elements.presetAgentAvatarStaticTab.setAttribute("aria-selected", kind === "static" ? "true" : "false");
  elements.presetAgentAvatarGlobalTab.setAttribute("aria-selected", kind === "companion" ? "true" : "false");
  const label = elements.presetAgentAvatarModal.querySelector('[data-i18n="presetAgents.avatarModal.icons"]');
  if (label) {
    label.textContent = kind === "companion"
      ? t("presetAgents.avatarModal.globalTab")
      : t("presetAgents.avatarModal.icons");
  }
  renderPresetAvatarPicker();
};

const closePresetAvatarModal = () => {
  elements.presetAgentAvatarModal.classList.remove("active");
};

const openPresetAvatarModal = () => {
  const preset = effectiveSelectedPreset() || selectedPreset();
  if (!preset) {
    return;
  }
  const icon = parseIconConfig(preset.icon, {
    icon_name: preset.icon_name,
    icon_color: preset.icon_color,
  });
  avatarModalState.kind = icon.kind === "companion" && icon.id ? "companion" : "static";
  avatarModalState.iconName = normalizeIconName(icon.name, {
    fallbackWhenEmpty: DEFAULT_PRESET_AVATAR_ICON_NAME,
    fallbackWhenUnknown: FALLBACK_PRESET_AVATAR_ICON_NAME,
  });
  avatarModalState.color = normalizeIconColor(icon.color);
  avatarModalState.companionScope = "global";
  avatarModalState.companionId = icon.kind === "companion" ? String(icon.id || "").trim() : "";
  avatarModalState.page = avatarModalState.kind === "companion"
    ? resolveCompanionPageById(avatarModalState.companionId)
    : resolveAvatarPageByKey(avatarModalState.iconName);
  renderPresetAvatarColorOptions();
  renderPresetAvatarModalState();
  elements.presetAgentAvatarModal.classList.add("active");
};

const resetPresetAvatarModal = () => {
  avatarModalState.kind = "static";
  avatarModalState.iconName = DEFAULT_PRESET_AVATAR_ICON_NAME;
  avatarModalState.color = normalizeIconColor("#94a3b8");
  avatarModalState.companionScope = "global";
  avatarModalState.companionId = "";
  avatarModalState.page = resolveAvatarPageByKey(avatarModalState.iconName);
  renderPresetAvatarModalState();
};

const applyPresetAvatarModal = () => {
  const rawPreset = selectedPreset();
  if (!rawPreset) {
    closePresetAvatarModal();
    return;
  }
  assignIconConfig(rawPreset, buildIconFromAvatarModalState());
  if (isDefaultPreset(rawPreset) && state.presetAgents.userAgent) {
    assignIconConfig(state.presetAgents.userAgent, rawPreset.icon);
  }
  renderPresetAvatarTrigger(effectiveSelectedPreset() || rawPreset);
  markPresetDraftDirty();
  closePresetAvatarModal();
};

const fillPresetForm = (preset) => {
  elements.presetAgentFormName.value = preset?.name || "";
  elements.presetAgentFormDescription.value = preset?.description || "";
  elements.presetAgentFormPrompt.value = preset?.system_prompt || "";
  elements.presetAgentPreviewSkill.checked = preset?.preview_skill === true;
  renderModelOptions(normalizeOptionalModelName(preset?.model_name));
  elements.presetAgentFormModelName.disabled = preset?.is_default_agent === true;
  renderPresetAvatarTrigger(preset);
  renderCustomizableControls(preset);
};

// 「允许用户自定义」开关组：随预设一起保存，未开启的字段对用户只读
const renderCustomizableControls = (preset) => {
  const customizable = normalizeCustomizable(preset?.customizable);
  CUSTOMIZABLE_FIELD_META.forEach((field) => {
    const input = elements[field.elementKey];
    if (!input) {
      return;
    }
    input.checked = customizable[field.key] === true;
    input.disabled = !preset;
  });
};

const collectCustomizableForm = () => {
  const output = {};
  CUSTOMIZABLE_FIELD_META.forEach((field) => {
    output[field.key] = elements[field.elementKey]?.checked === true;
  });
  return output;
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
};

const renderTabAvailability = () => {
  elements.presetAgentTabCron.disabled = true;
  elements.presetAgentTabChannels.disabled = true;
  if (state.presetAgents.activeTab !== "preset") {
    state.presetAgents.activeTab = "preset";
  }
};

// 「契约未就绪」标记：后端绑定/同步接口尚未交付时统一提示，不伪装成功
const isBindingContractUnavailable = () => getContractState() === CONTRACT_STATE_UNAVAILABLE;

const renderContractBadges = () => {
  const missing = isBindingContractUnavailable();
  [
    elements.presetAgentSyncContractBadge,
    elements.presetBindingContractBadge,
  ].forEach((node) => {
    if (!node) {
      return;
    }
    node.textContent = missing ? t("presetAgents.contract.badge") : "";
    node.style.display = missing ? "" : "none";
    node.title = missing ? t("presetAgents.contract.hint") : "";
  });
};

const renderSyncLog = () => {
  const list = elements.presetAgentSyncLog;
  if (!list) {
    return;
  }
  const entries = Array.isArray(state.presetAgents.syncLog) ? state.presetAgents.syncLog : [];
  list.textContent = "";
  if (!entries.length) {
    const empty = document.createElement("div");
    empty.className = "muted preset-sync-log-empty";
    empty.textContent = t("presetAgents.sync.logEmpty");
    list.appendChild(empty);
    return;
  }
  const fragment = document.createDocumentFragment();
  entries.forEach((entry) => {
    const row = document.createElement("div");
    row.className = `preset-sync-log-item${entry.kind === "error" ? " is-error" : ""}`;
    const time = document.createElement("span");
    time.className = "preset-sync-log-time";
    time.textContent = formatTimestamp(entry.at);
    const text = document.createElement("span");
    text.className = "preset-sync-log-text";
    text.textContent = entry.text;
    row.appendChild(time);
    row.appendChild(text);
    fragment.appendChild(row);
  });
  list.appendChild(fragment);
};

const appendSyncLog = (text, kind = "info") => {
  ensureState();
  const cleaned = String(text || "").trim();
  if (!cleaned) {
    return;
  }
  state.presetAgents.syncLog = [
    { at: Date.now(), text: cleaned, kind },
    ...(Array.isArray(state.presetAgents.syncLog) ? state.presetAgents.syncLog : []),
  ].slice(0, PRESET_SYNC_LOG_LIMIT);
  renderSyncLog();
};

const renderSyncSummary = () => {
  const preset = selectedPreset();
  const dirty = state.presetAgents.draftDirty === true;
  const canSync = Boolean(preset?.preset_id) && !dirty && state.presetAgents.saving !== true;
  elements.presetAgentSyncSafeBtn.disabled = !canSync || state.presetAgents.syncLoading;
  elements.presetAgentSyncForceBtn.disabled = !canSync || state.presetAgents.syncLoading;

  if (!preset || !preset.preset_id) {
    elements.presetAgentSyncSummary.textContent = t("presetAgents.sync.empty");
    renderContractBadges();
    return;
  }
  if (dirty) {
    elements.presetAgentSyncSummary.textContent = t("presetAgents.sync.saveRequired");
    renderContractBadges();
    return;
  }
  if (state.presetAgents.syncLoading) {
    elements.presetAgentSyncSummary.textContent = t("presetAgents.sync.summaryLoading");
    renderContractBadges();
    return;
  }
  if (isBindingContractUnavailable()) {
    elements.presetAgentSyncSummary.textContent = t("presetAgents.sync.notReady");
    renderContractBadges();
    return;
  }
  const preview =
    state.presetAgents.syncPreview?.preset_id === preset.preset_id ? state.presetAgents.syncPreview : null;
  if (!preview) {
    elements.presetAgentSyncSummary.textContent = t("presetAgents.sync.empty");
    renderContractBadges();
    return;
  }
  elements.presetAgentSyncSummary.textContent = t("presetAgents.sync.summary", {
    affected: preview.affected_users || 0,
    updated: preview.updated_agents || 0,
    skipped: preview.skipped_customized || 0,
    created: preview.created_agents || 0,
  });
  renderContractBadges();
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
    const titleText = document.createElement("span");
    titleText.textContent = preset.name || "-";
    title.appendChild(titleText);
    if (preset.is_default_agent === true) {
      const badge = document.createElement("span");
      badge.className = "preset-agent-item-badge";
      badge.textContent = t("presetAgents.list.defaultBadge");
      title.appendChild(badge);
    }

    const description = document.createElement("div");
    description.className = "preset-agent-item-desc";
    description.textContent = preset.description || t("presetAgents.list.noDescription");
    if (preset.description) {
      description.title = preset.description;
    }

    const meta = document.createElement("div");
    meta.className = "preset-agent-item-meta";
    [
      presetBoundUsersText(preset),
      t("presetAgents.list.model", { model: presetModelText(preset) }),
      t("presetAgents.list.updatedAt", { time: formatPresetTimestamp(preset.updated_at) }),
      `v${Math.max(1, Number(preset.revision) || 1)}`,
    ].forEach((text) => {
      const chip = document.createElement("span");
      chip.className = "preset-agent-item-meta-chip";
      chip.textContent = text;
      meta.appendChild(chip);
    });

    row.appendChild(title);
    row.appendChild(description);
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
      state.presetAgents.syncPreview = null;
      renderAll();
      await refreshContext({ ensureAgent: true, silent: true });
      await loadSyncPreview({ silent: true });
      await loadBindings({ silent: true });
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
    closePresetAvatarModal();
    fillPresetForm(null);
    fillAgentForm(null);
    renderPresetActionState();
    return;
  }
  elements.presetAgentDetailTitle.textContent = rawPreset.name;
  elements.presetAgentDetailMeta.textContent = [
    presetBoundUsersText(rawPreset),
    t("presetAgents.list.model", { model: presetModelText(rawPreset) }),
    t("presetAgents.detail.updatedAt", { time: formatPresetTimestamp(rawPreset.updated_at) }),
    `v${Math.max(1, Number(rawPreset.revision) || 1)}`,
    presetVisibilitySummary(rawPreset),
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
  renderSyncLog();
  renderBindings();
  renderCronList();
  renderChannelForms();
  renderChannelAccounts();
  setTab(state.presetAgents.activeTab);
};

// ---------------------------------------------------------------------------
// 绑定用户区块：分页 / 筛选 / 批量绑定 / 换绑 / 解绑
// ---------------------------------------------------------------------------

const selectedBindingUserIds = () => {
  const bindings = ensureBindingsState();
  const ids = Array.from(bindings.selected).filter(Boolean);
  const available = new Set(bindings.items.map((item) => item.user_id));
  return ids.filter((id) => available.has(id));
};

const renderBindings = () => {
  const bindings = ensureBindingsState();
  const body = elements.presetBindingTableBody;
  if (!body) {
    return;
  }
  const preset = selectedPreset();
  const presetId = normalizePresetId(preset?.preset_id);
  // 预设切换后不允许残留上一个预设的绑定行
  if (bindings.presetId !== presetId) {
    bindings.presetId = presetId;
    bindings.items = [];
    bindings.total = 0;
    bindings.page = 1;
    bindings.selected = new Set();
  }
  const selectedIds = selectedBindingUserIds();
  elements.presetBindingSummary.textContent = t("presetAgents.bindings.summary", {
    total: bindings.total,
    selected: selectedIds.length,
  });
  elements.presetBindingRebindBtn.disabled = !selectedIds.length || bindings.loading;
  elements.presetBindingUnbindBtn.disabled = !selectedIds.length || bindings.loading;
  elements.presetBindingAddBtn.disabled = !presetId || bindings.loading;
  elements.presetBindingReloadBtn.disabled = bindings.loading;
  elements.presetBindingPrevBtn.disabled = bindings.loading || bindings.page <= 1;
  const totalPages = Math.max(1, Math.ceil(bindings.total / Math.max(1, bindings.pageSize)));
  elements.presetBindingNextBtn.disabled = bindings.loading || bindings.page >= totalPages;
  elements.presetBindingPageInfo.textContent = t("pagination.info", {
    total: bindings.total,
    current: Math.min(bindings.page, totalPages),
    pages: totalPages,
    size: bindings.pageSize,
  });
  elements.presetBindingPagination.style.display = bindings.total > 0 ? "flex" : "none";

  body.textContent = "";
  const selectAll = elements.presetBindingSelectAll;
  selectAll.checked = bindings.items.length > 0 && selectedIds.length === bindings.items.length;
  selectAll.disabled = bindings.loading || bindings.items.length === 0;

  if (bindings.loading) {
    elements.presetBindingEmpty.textContent = t("common.loading");
    elements.presetBindingEmpty.style.display = "block";
    return;
  }
  if (isBindingContractUnavailable()) {
    elements.presetBindingEmpty.textContent = t("presetAgents.bindings.notReady");
    elements.presetBindingEmpty.style.display = "block";
    return;
  }
  if (!bindings.items.length) {
    elements.presetBindingEmpty.textContent = bindings.keyword
      ? t("presetAgents.bindings.searchEmpty", { keyword: bindings.keyword })
      : t("presetAgents.bindings.empty");
    elements.presetBindingEmpty.style.display = "block";
    return;
  }
  elements.presetBindingEmpty.style.display = "none";

  const fragment = document.createDocumentFragment();
  bindings.items.forEach((item) => {
    const row = document.createElement("tr");
    const selectCell = document.createElement("td");
    selectCell.className = "preset-binding-select-cell";
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = bindings.selected.has(item.user_id);
    checkbox.setAttribute("aria-label", t("presetAgents.bindings.table.select"));
    checkbox.addEventListener("change", () => {
      if (checkbox.checked) {
        bindings.selected.add(item.user_id);
      } else {
        bindings.selected.delete(item.user_id);
      }
      renderBindings();
    });
    selectCell.appendChild(checkbox);

    const userCell = document.createElement("td");
    userCell.textContent = item.username || item.user_id || "-";
    if (item.username && item.user_id && item.username !== item.user_id) {
      const sub = document.createElement("div");
      sub.className = "muted preset-binding-user-id";
      sub.textContent = item.user_id;
      userCell.appendChild(sub);
    }

    const agentCell = document.createElement("td");
    agentCell.textContent = item.agent_id || "-";

    const customizedCell = document.createElement("td");
    if (item.customized.length) {
      const wrap = document.createElement("div");
      wrap.className = "preset-binding-tag-list";
      item.customized.forEach((field) => {
        const tag = document.createElement("span");
        tag.className = "preset-binding-tag";
        tag.textContent = customizableFieldLabel(field);
        wrap.appendChild(tag);
      });
      customizedCell.appendChild(wrap);
    } else {
      customizedCell.textContent = t("presetAgents.bindings.customizedNone");
    }

    row.appendChild(selectCell);
    row.appendChild(userCell);
    row.appendChild(agentCell);
    row.appendChild(customizedCell);
    fragment.appendChild(row);
  });
  body.appendChild(fragment);
};

const customizableFieldLabel = (field) => {
  const key = String(field || "").trim();
  const meta = CUSTOMIZABLE_FIELD_META.find((item) => item.key === key);
  return meta ? t(meta.labelKey) : key || "-";
};

const toggleBindingsSelectAll = (checked) => {
  const bindings = ensureBindingsState();
  bindings.items.forEach((item) => {
    if (checked) {
      bindings.selected.add(item.user_id);
    } else {
      bindings.selected.delete(item.user_id);
    }
  });
  renderBindings();
};

const loadBindings = async ({ silent = false, force = false } = {}) => {
  const bindings = ensureBindingsState();
  const preset = selectedPreset();
  const presetId = normalizePresetId(preset?.preset_id);
  if (!presetId) {
    state.presetAgents.bindings = {
      ...bindings,
      presetId: "",
      items: [],
      total: 0,
      page: 1,
      selected: new Set(),
      loaded: false,
    };
    renderBindings();
    renderSyncSummary();
    return null;
  }
  if (bindings.presetId !== presetId) {
    bindings.presetId = presetId;
    bindings.page = 1;
    bindings.selected = new Set();
    bindings.items = [];
    bindings.total = 0;
  }
  const requestToken = (Number(bindings.requestToken) || 0) + 1;
  bindings.requestToken = requestToken;
  bindings.loading = true;
  renderBindings();

  const result = await listPresetBindings({
    presetId,
    page: bindings.page,
    pageSize: bindings.pageSize,
    keyword: bindings.keyword,
    force,
  });
  if (bindings.requestToken !== requestToken) {
    return null;
  }
  bindings.loading = false;
  if (!result.ok) {
    bindings.items = [];
    bindings.total = 0;
    bindings.selected = new Set();
    bindings.loaded = true;
    renderBindings();
    renderSyncSummary();
    if (!result.unavailable && !silent) {
      notify(t("presetAgents.bindings.loadFailed", { message: result.message || "-" }), "error");
    }
    return null;
  }
  bindings.items = result.items;
  bindings.total = result.total;
  bindings.loaded = true;
  const available = new Set(bindings.items.map((item) => item.user_id));
  Array.from(bindings.selected).forEach((id) => {
    if (!available.has(id)) {
      bindings.selected.delete(id);
    }
  });
  renderBindings();
  renderSyncSummary();
  return result;
};

// ---------------------------------------------------------------------------
// 影响面预览 + 二次确认（换绑 / 解绑 / 强制同步 / 重建智能体共用）
// ---------------------------------------------------------------------------

const impactState = { onConfirm: null, busy: false, requireTarget: false, requireAck: false };

const closeImpactModal = () => {
  elements.presetImpactModal?.classList.remove("active");
  impactState.onConfirm = null;
  impactState.busy = false;
  impactState.requireTarget = false;
  impactState.requireAck = false;
};

const syncImpactConfirmState = () => {
  const confirmBtn = elements.presetImpactConfirm;
  if (!confirmBtn) {
    return;
  }
  const targetOk = !impactState.requireTarget || Boolean(String(elements.presetImpactPresetSelect?.value || "").trim());
  const ackOk = !impactState.requireAck || elements.presetImpactAck?.checked === true;
  confirmBtn.disabled = impactState.busy || !targetOk || !ackOk;
};

const renderImpactPresetOptions = (selectedId = "") => {
  const select = elements.presetImpactPresetSelect;
  if (!select) {
    return;
  }
  const current = selectedPreset();
  const options = bindingTargetPresets(current?.preset_id);
  select.textContent = "";
  const placeholder = document.createElement("option");
  placeholder.value = "";
  placeholder.textContent = t("presetAgents.impact.presetPlaceholder");
  select.appendChild(placeholder);
  options.forEach((preset) => {
    const option = document.createElement("option");
    option.value = normalizePresetId(preset.preset_id);
    option.textContent = preset.name || preset.preset_id;
    select.appendChild(option);
  });
  select.value = options.some((item) => normalizePresetId(item.preset_id) === selectedId) ? selectedId : "";
};

// 影响面预览弹层的事件绑定：与预设面板初始化解耦，
// 便于用户管理面板在未打开过预设面板时也能正常使用（重建智能体等）。
const bindImpactModalControls = () => {
  const bindOnce = (node, event, handler) => {
    if (!node) {
      return;
    }
    const flag = `bound${event}`;
    if (node.dataset[flag] === "1") {
      return;
    }
    node.dataset[flag] = "1";
    node.addEventListener(event, handler);
  };
  bindOnce(elements.presetImpactClose, "click", closeImpactModal);
  bindOnce(elements.presetImpactCancel, "click", closeImpactModal);
  bindOnce(elements.presetImpactConfirm, "click", submitImpactConfirm);
  bindOnce(elements.presetImpactPresetSelect, "change", syncImpactConfirmState);
  bindOnce(elements.presetImpactAck, "change", syncImpactConfirmState);
  bindOnce(elements.presetImpactModal, "click", (event) => {
    if (event.target === elements.presetImpactModal) {
      closeImpactModal();
    }
  });
};

const openImpactModal = ({
  title,
  summary,
  details = [],
  targetPreset = null,
  ackLabel = "",
  hint = "",
  confirmLabel = "",
  danger = false,
  onConfirm,
}) => {
  if (!elements.presetImpactModal) {
    return;
  }
  bindImpactModalControls();
  impactState.onConfirm = onConfirm;
  impactState.busy = false;
  impactState.requireTarget = Boolean(targetPreset);
  impactState.requireAck = Boolean(ackLabel);

  elements.presetImpactTitle.textContent = title || t("presetAgents.impact.title");
  elements.presetImpactSummary.textContent = summary || "";
  elements.presetImpactList.textContent = "";
  details.forEach((line) => {
    const item = document.createElement("li");
    item.textContent = line;
    elements.presetImpactList.appendChild(item);
  });
  elements.presetImpactList.style.display = details.length ? "" : "none";

  elements.presetImpactPresetRow.style.display = targetPreset ? "" : "none";
  if (targetPreset) {
    elements.presetImpactPresetLabel.textContent =
      targetPreset.label || t("presetAgents.impact.presetLabel");
    renderImpactPresetOptions(targetPreset.selected || "");
  } else {
    elements.presetImpactPresetSelect.value = "";
  }

  elements.presetImpactAckRow.style.display = ackLabel ? "" : "none";
  if (ackLabel) {
    elements.presetImpactAck.checked = false;
    elements.presetImpactAckLabel.textContent = ackLabel;
  }

  elements.presetImpactHint.textContent = hint || "";
  elements.presetImpactHint.style.display = hint ? "" : "none";
  elements.presetImpactConfirm.textContent = confirmLabel || t("presetAgents.impact.confirm");
  elements.presetImpactConfirm.classList.toggle("danger", danger === true);
  syncImpactConfirmState();
  elements.presetImpactModal.classList.add("active");
};

// 供其他管理面板复用的「影响面预览 + 二次确认」弹层（换绑 / 重建智能体等危险操作）
export const openImpactConfirmModal = (options) => openImpactModal(options || {});

const submitImpactConfirm = async () => {
  if (impactState.busy || typeof impactState.onConfirm !== "function") {
    return;
  }
  const targetPresetId = String(elements.presetImpactPresetSelect?.value || "").trim();
  if (impactState.requireTarget && !targetPresetId) {
    notify(t("presetAgents.impact.presetRequired"), "warn");
    return;
  }
  if (impactState.requireAck && elements.presetImpactAck?.checked !== true) {
    notify(t("presetAgents.impact.ackRequired"), "warn");
    return;
  }
  impactState.busy = true;
  syncImpactConfirmState();
  let ok = false;
  try {
    ok = (await impactState.onConfirm({ targetPresetId })) !== false;
  } catch (error) {
    notify(t("presetAgents.bindings.result.failed", { message: error?.message || "-" }), "error");
  } finally {
    impactState.busy = false;
    syncImpactConfirmState();
    if (ok) {
      closeImpactModal();
    }
  }
};

const buildBindingImpactDetails = (userIds, limit = PRESET_IMPACT_PREVIEW_LIMIT) => {
  const bindings = ensureBindingsState();
  const byId = new Map(bindings.items.map((item) => [item.user_id, item]));
  const details = userIds.slice(0, limit).map((userId) => {
    const item = byId.get(userId);
    return `${item?.username || userId} · ${item?.agent_id || "-"}`;
  });
  if (userIds.length > limit) {
    details.push(t("presetAgents.impact.more", { count: userIds.length - limit }));
  }
  return details;
};

// 绑定操作：bind / unbind（unbind 必须带新预设，保证用户始终有且仅有一个智能体）
const submitBindingChange = async ({ action, userIds, presetId, newPresetId = "" }) => {
  const result = await mutatePresetBindings({ presetId, userIds, action, newPresetId });
  if (!result.ok) {
    if (result.unavailable) {
      notify(t("presetAgents.contract.notReady"), "warn");
      appendSyncLog(t("presetAgents.bindings.result.notReady"), "error");
    } else {
      notify(t("presetAgents.bindings.result.failed", { message: result.message || "-" }), "error");
      appendSyncLog(t("presetAgents.bindings.result.failed", { message: result.message || "-" }), "error");
    }
    return false;
  }
  const summary = result.result || {};
  const text = t("presetAgents.bindings.result.done", {
    affected: summary.affected_users || userIds.length,
    created: summary.created_agents || 0,
    rebound: summary.rebound_agents || 0,
  });
  notify(text, "success");
  appendSyncLog(text);
  await reloadAfterBindingChange();
  return true;
};

// 绑定 / 同步完成后只刷新列表与绑定区块，绝不覆盖表单里未保存的草稿
const reloadAfterBindingChange = async () => {
  const listed = await listPresetAgentsContract({ force: true });
  if (listed.ok) {
    state.presetAgents.presets = stabilizePresetListOrder(listed.items, state.presetAgents.presets);
    renderPresetList();
  }
  await loadBindings({ silent: true, force: true });
  await loadSyncPreview({ silent: true, force: true });
};

const openBindSelectedUsers = (userIds) => {
  const preset = selectedPreset();
  if (!preset?.preset_id || !userIds.length) {
    return;
  }
  openImpactModal({
    title: t("presetAgents.bindings.impact.bindTitle"),
    summary: t("presetAgents.bindings.impact.bindSummary", {
      count: userIds.length,
      preset: preset.name || preset.preset_id,
    }),
    details: buildBindingImpactDetails(userIds),
    hint: t("presetAgents.bindings.impact.bindHint"),
    confirmLabel: t("presetAgents.bindings.add"),
    onConfirm: () =>
      submitBindingChange({ action: "bind", userIds, presetId: normalizePresetId(preset.preset_id) }),
  });
};

const openRebindSelectedUsers = (userIds) => {
  const preset = selectedPreset();
  if (!preset?.preset_id || !userIds.length) {
    return;
  }
  openImpactModal({
    title: t("presetAgents.bindings.impact.rebindTitle"),
    summary: t("presetAgents.bindings.impact.rebindSummary", { count: userIds.length }),
    details: buildBindingImpactDetails(userIds),
    targetPreset: { label: t("presetAgents.bindings.impact.targetLabel"), selected: "" },
    hint: t("presetAgents.bindings.impact.rebindHint"),
    confirmLabel: t("presetAgents.bindings.rebind"),
    onConfirm: ({ targetPresetId }) =>
      submitBindingChange({
        action: "bind",
        userIds,
        presetId: targetPresetId,
      }),
  });
};

const openUnbindSelectedUsers = (userIds) => {
  const preset = selectedPreset();
  if (!preset?.preset_id || !userIds.length) {
    return;
  }
  openImpactModal({
    title: t("presetAgents.bindings.impact.unbindTitle"),
    summary: t("presetAgents.bindings.impact.unbindSummary", {
      count: userIds.length,
      preset: preset.name || preset.preset_id,
    }),
    details: buildBindingImpactDetails(userIds),
    targetPreset: { label: t("presetAgents.bindings.impact.newPresetLabel"), selected: "" },
    hint: t("presetAgents.bindings.impact.unbindHint"),
    confirmLabel: t("presetAgents.bindings.unbind"),
    danger: true,
    onConfirm: ({ targetPresetId }) =>
      submitBindingChange({
        action: "unbind",
        userIds,
        presetId: normalizePresetId(preset.preset_id),
        newPresetId: targetPresetId,
      }),
  });
};

// ---------------------------------------------------------------------------
// 绑定用户选择器：从管理侧用户列表批量挑选待绑定用户
// ---------------------------------------------------------------------------

const closeBindingPicker = () => {
  elements.presetBindingPickerModal?.classList.remove("active");
};

const renderBindingPicker = () => {
  const picker = ensurePickerState();
  const list = elements.presetBindingPickerList;
  if (!list) {
    return;
  }
  elements.presetBindingPickerSummary.textContent = t("presetAgents.bindings.picker.selected", {
    count: picker.selected.size,
  });
  elements.presetBindingPickerConfirm.disabled = picker.loading || picker.selected.size === 0;
  elements.presetBindingPickerPrevBtn.disabled = picker.loading || picker.page <= 1;
  const totalPages = Math.max(1, Math.ceil(picker.total / Math.max(1, picker.pageSize)));
  elements.presetBindingPickerNextBtn.disabled = picker.loading || picker.page >= totalPages;
  elements.presetBindingPickerPageInfo.textContent = t("pagination.info", {
    total: picker.total,
    current: Math.min(picker.page, totalPages),
    pages: totalPages,
    size: picker.pageSize,
  });
  elements.presetBindingPickerSelectAll.checked =
    picker.items.length > 0 && picker.items.every((item) => picker.selected.has(item.user_id));
  elements.presetBindingPickerSelectAll.disabled = picker.loading || picker.items.length === 0;

  list.textContent = "";
  if (picker.loading) {
    elements.presetBindingPickerEmpty.textContent = t("common.loading");
    elements.presetBindingPickerEmpty.style.display = "block";
    return;
  }
  if (!picker.items.length) {
    elements.presetBindingPickerEmpty.textContent = t("presetAgents.bindings.picker.empty");
    elements.presetBindingPickerEmpty.style.display = "block";
    return;
  }
  elements.presetBindingPickerEmpty.style.display = "none";
  const fragment = document.createDocumentFragment();
  picker.items.forEach((item) => {
    const row = document.createElement("label");
    row.className = "preset-binding-picker-item";
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = picker.selected.has(item.user_id);
    checkbox.addEventListener("change", () => {
      if (checkbox.checked) {
        picker.selected.add(item.user_id);
      } else {
        picker.selected.delete(item.user_id);
      }
      renderBindingPicker();
    });
    const name = document.createElement("span");
    name.className = "preset-binding-picker-name";
    name.textContent = item.username || item.user_id;
    const id = document.createElement("span");
    id.className = "muted preset-binding-picker-id";
    id.textContent = item.user_id;
    row.appendChild(checkbox);
    row.appendChild(name);
    row.appendChild(id);
    fragment.appendChild(row);
  });
  list.appendChild(fragment);
};

const loadBindingPickerUsers = async () => {
  const picker = ensurePickerState();
  const requestToken = (Number(picker.requestToken) || 0) + 1;
  picker.requestToken = requestToken;
  picker.loading = true;
  renderBindingPicker();
  const offset = (Math.max(1, picker.page) - 1) * picker.pageSize;
  try {
    const payload = await requestJson("/admin/user_accounts", {
      query: {
        offset,
        limit: picker.pageSize,
        keyword: picker.keyword,
      },
    });
    if (picker.requestToken !== requestToken) {
      return;
    }
    const data = payload?.data && typeof payload.data === "object" ? payload.data : {};
    const items = Array.isArray(data.items) ? data.items : [];
    picker.items = items
      .map((item) => ({
        user_id: String(item?.id || item?.user_id || "").trim(),
        username: String(item?.username || "").trim(),
      }))
      .filter((item) => item.user_id);
    picker.total = Number.isFinite(Number(data.total)) ? Number(data.total) : picker.items.length;
  } catch (error) {
    if (picker.requestToken !== requestToken) {
      return;
    }
    picker.items = [];
    picker.total = 0;
    notify(t("presetAgents.bindings.picker.loadFailed", { message: error.message || "-" }), "error");
  } finally {
    if (picker.requestToken === requestToken) {
      picker.loading = false;
      renderBindingPicker();
    }
  }
};

// 绑定用户选择器的控件绑定：同样与面板初始化解耦，避免打开时按钮未绑定
const bindBindingPickerControls = () => {
  const bindOnce = (node, event, handler) => {
    if (!node) {
      return;
    }
    const flag = `bound${event}`;
    if (node.dataset[flag] === "1") {
      return;
    }
    node.dataset[flag] = "1";
    node.addEventListener(event, handler);
  };
  bindOnce(elements.presetBindingPickerClose, "click", closeBindingPicker);
  bindOnce(elements.presetBindingPickerCancel, "click", closeBindingPicker);
  bindOnce(elements.presetBindingPickerConfirm, "click", confirmBindingPicker);
  bindOnce(elements.presetBindingPickerSearchBtn, "click", () => {
    const picker = ensurePickerState();
    picker.keyword = String(elements.presetBindingPickerSearch.value || "").trim();
    picker.page = 1;
    loadBindingPickerUsers();
  });
  bindOnce(elements.presetBindingPickerSearch, "keydown", (event) => {
    if (event.key !== "Enter") {
      return;
    }
    event.preventDefault();
    elements.presetBindingPickerSearchBtn.click();
  });
  bindOnce(elements.presetBindingPickerSelectAll, "change", () => {
    const picker = ensurePickerState();
    const checked = elements.presetBindingPickerSelectAll.checked;
    picker.items.forEach((item) => {
      if (checked) {
        picker.selected.add(item.user_id);
      } else {
        picker.selected.delete(item.user_id);
      }
    });
    renderBindingPicker();
  });
  bindOnce(elements.presetBindingPickerPrevBtn, "click", () => {
    const picker = ensurePickerState();
    picker.page = Math.max(1, picker.page - 1);
    loadBindingPickerUsers();
  });
  bindOnce(elements.presetBindingPickerNextBtn, "click", () => {
    const picker = ensurePickerState();
    picker.page += 1;
    loadBindingPickerUsers();
  });
  bindOnce(elements.presetBindingPickerModal, "click", (event) => {
    if (event.target === elements.presetBindingPickerModal) {
      closeBindingPicker();
    }
  });
};

const openBindingPicker = () => {
  const preset = selectedPreset();
  if (!preset?.preset_id) {
    return;
  }
  bindBindingPickerControls();
  state.presetAgents.picker = {
    items: [],
    total: 0,
    page: 1,
    pageSize: PRESET_PICKER_PAGE_SIZE,
    keyword: "",
    loading: false,
    requestToken: 0,
    selected: new Set(),
  };
  elements.presetBindingPickerSearch.value = "";
  renderBindingPicker();
  elements.presetBindingPickerModal.classList.add("active");
  loadBindingPickerUsers();
};

const confirmBindingPicker = () => {
  const picker = ensurePickerState();
  const userIds = Array.from(picker.selected).filter(Boolean);
  if (!userIds.length) {
    notify(t("presetAgents.bindings.picker.noneSelected"), "warn");
    return;
  }
  closeBindingPicker();
  openBindSelectedUsers(userIds);
};

// 默认预设的后端模板智能体（定时任务 / 渠道配置 tab 作用对象）
const loadDefaultTemplateAgent = async () => {
  const payload = await requestJson(`/agents/${encodeURIComponent(DEFAULT_AGENT_ID_ALIAS)}`, {
    query: { user_id: TEMPLATE_USER_ID },
  });
  return normalizeUserAgent(payload?.data || {});
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

const loadGlobalCompanionsForPresetAgents = async ({ silent = false } = {}) => {
  try {
    state.presetAgents.companions = (await listGlobalCompanions()).map(normalizeCompanionRecord);
  } catch (error) {
    state.presetAgents.companions = [];
    if (!silent) {
      notify(t("companionsAdmin.toast.refreshFailed", { message: error.message || "-" }), "error");
    }
  }
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
    state.presetAgents.syncPreview = null;
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
    setStatus(t("presetAgents.status.ready", { preset: preset.name || "-" }), "success");
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
  return {
    name,
    description: String(elements.presetAgentFormDescription.value || "").trim(),
    system_prompt: String(elements.presetAgentFormPrompt.value || "").trim(),
    preview_skill: elements.presetAgentPreviewSkill?.checked === true,
    model_name: normalizeOptionalModelName(elements.presetAgentFormModelName?.value),
    customizable: collectCustomizableForm(),
    visible_unit_ids: Array.isArray(selectedPreset()?.visible_unit_ids)
      ? [...selectedPreset().visible_unit_ids]
      : [],
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
  state.presetAgents.syncPreview = null;
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
        preview_skill: item.preview_skill === true,
        model_name: normalizeOptionalModelName(item.model_name),
        icon: stringifyIconConfig(item.icon || {
          kind: "static",
          name: item.icon_name,
          color: item.icon_color,
        }),
        icon_name: item.icon_name,
        icon_color: item.icon_color,
        tool_names: item.tool_names,
        declared_tool_names: item.declared_tool_names,
        declared_skill_names: item.declared_skill_names,
        visible_unit_ids: Array.isArray(item.visible_unit_ids) ? item.visible_unit_ids : [],
        preset_questions: item.preset_questions,
        approval_mode: item.approval_mode,
        status: item.status,
        customizable: normalizeCustomizable(item.customizable),
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
      next.visible_unit_ids = agentPayload.visible_unit_ids;
      next.preset_questions = agentPayload.preset_questions;
      next.approval_mode = agentPayload.approval_mode;
      next.status = agentPayload.status || effective.status || current.status || "active";
      next.model_name = normalizeOptionalModelName(agentPayload.model_name);
      assignIconConfig(next, agentPayload.icon || effective.icon || current.icon);
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
        await loadSyncPreview({ silent: true });
        renderAll();
      } else {
        state.presetAgents.presets = state.presetAgents.presets.map((item) =>
          isSamePreset(item, current) ? next : item
        );
        setSelectedPreset(next);
        await persistPresets({ selectedName: next.name, selectedPresetId: next.preset_id });
        await refreshContext({ ensureAgent: false, silent: true });
        await loadSyncPreview({ silent: true });
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

const createPreset = () => {
  const baseName = t("presetAgents.newPresetName");
  let index = state.presetAgents.presets.length + 1;
  let candidate = `${baseName}${index}`;
  const existing = new Set(state.presetAgents.presets.map((item) => item.name.toLowerCase()));
  while (existing.has(candidate.toLowerCase())) {
    index += 1;
    candidate = `${baseName}${index}`;
  }
  const createdIcon = stringifyIconConfig(buildDefaultStaticIconConfig());
  const createdPreset = {
    preset_id: "",
    revision: 1,
    name: candidate,
    description: "",
    system_prompt: "",
    model_name: "",
    icon: createdIcon,
    ...deriveLegacyIconParts(createdIcon),
    tool_names: [],
    declared_tool_names: [],
    declared_skill_names: [],
    visible_unit_ids: [],
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
  state.presetAgents.syncPreview = null;
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
    state.presetAgents.syncPreview = null;
    await persistPresets({
      selectedName: state.presetAgents.selectedPresetName,
      selectedPresetId: state.presetAgents.selectedPresetId,
    });
    if (selectedPreset()) {
      await refreshContext({ ensureAgent: true, silent: true });
      await loadSyncPreview({ silent: true });
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
  const selectedAbilities = collectSelectedAbilities(elements.presetUserAgentTools);
  const tool_names = normalizeNameList(selectedAbilities.map((item) => item.name));
  const { declared_tool_names, declared_skill_names } = splitSelectedAbilityNames(selectedAbilities);
  const preset = effectiveSelectedPreset() || selectedPreset();
  const icon = stringifyIconConfig(preset?.icon || {
    kind: "static",
    name: preset?.icon_name,
    color: preset?.icon_color,
  });
  return {
    name,
    description: String(elements.presetAgentFormDescription.value || "").trim(),
    system_prompt: String(elements.presetAgentFormPrompt.value || "").trim(),
    preview_skill: elements.presetAgentPreviewSkill?.checked === true,
    model_name: normalizeOptionalModelName(elements.presetAgentFormModelName?.value),
    tool_names,
    declared_tool_names,
    declared_skill_names,
    visible_unit_ids: Array.isArray(preset?.visible_unit_ids) ? [...preset.visible_unit_ids] : [],
    preset_questions: collectPresetQuestionValues(),
    approval_mode:
      String(elements.presetUserAgentApproval.value || effectiveSelectedPreset()?.approval_mode || "full_auto").trim() ||
      "full_auto",
    status: String(effectiveSelectedPreset()?.status || "active").trim() || "active",
    is_shared: false,
    icon,
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

// 同步预览：mode=safe + dry_run=true，只统计影响面，不落库
const loadSyncPreview = async ({ silent = false, force = false } = {}) => {
  const preset = selectedPreset();
  if (!preset?.preset_id) {
    state.presetAgents.syncPreview = null;
    state.presetAgents.syncLoading = false;
    renderSyncSummary();
    return null;
  }

  const requestToken = (Number(state.presetAgents.syncRequestToken) || 0) + 1;
  state.presetAgents.syncRequestToken = requestToken;
  state.presetAgents.syncLoading = true;
  renderSyncSummary();

  const result = await syncPresetAgents({
    presetId: normalizePresetId(preset.preset_id),
    mode: "safe",
    dryRun: true,
    force,
  });
  if (state.presetAgents.syncRequestToken !== requestToken) {
    return null;
  }
  state.presetAgents.syncLoading = false;
  if (!result.ok) {
    state.presetAgents.syncPreview = null;
    if (!result.unavailable && !silent) {
      notify(t("presetAgents.toast.syncFailed", { message: result.message || "-" }), "error");
    }
    renderSyncSummary();
    renderBindings();
    return null;
  }
  state.presetAgents.syncPreview = result.result;
  renderSyncSummary();
  return result.result;
};

// 同步执行：先预览（dry_run=true）→ 影响面预览 + 二次确认 → dry_run=false 落库
const runPresetSync = async (mode = "safe") => {
  const saved = await ensurePresetDraftSaved("presetAgents.error.saveBeforeSync");
  if (!saved) {
    return;
  }
  const preset = selectedPreset();
  if (!preset?.preset_id) {
    return;
  }
  const normalizedMode = mode === "force" ? "force" : "safe";
  const presetId = normalizePresetId(preset.preset_id);
  const preview = await syncPresetAgents({ presetId, mode: normalizedMode, dryRun: true });
  if (!preview.ok) {
    if (preview.unavailable) {
      notify(t("presetAgents.contract.notReady"), "warn");
      appendSyncLog(t("presetAgents.sync.notReady"), "error");
    } else {
      notify(t("presetAgents.toast.syncFailed", { message: preview.message || "-" }), "error");
      appendSyncLog(t("presetAgents.toast.syncFailed", { message: preview.message || "-" }), "error");
    }
    return;
  }
  const stats = preview.result || {};
  const isForce = normalizedMode === "force";
  openImpactModal({
    title: isForce ? t("presetAgents.sync.previewTitle.force") : t("presetAgents.sync.previewTitle.safe"),
    summary: t("presetAgents.sync.previewSummary", {
      affected: stats.affected_users || 0,
      updated: stats.updated_agents || 0,
      skipped: stats.skipped_customized || 0,
      created: stats.created_agents || 0,
    }),
    details: [
      t("presetAgents.sync.previewOk", { count: stats.updated_agents || 0 }),
      t("presetAgents.sync.previewSkipped", { count: stats.skipped_customized || 0 }),
      t("presetAgents.sync.previewCreated", { count: stats.created_agents || 0 }),
    ],
    ackLabel: isForce ? t("presetAgents.sync.ack") : "",
    hint: isForce ? t("presetAgents.sync.forceHint") : t("presetAgents.sync.safeHint"),
    confirmLabel: isForce ? t("presetAgents.action.syncForce") : t("presetAgents.action.syncSafe"),
    danger: isForce,
    onConfirm: async () => {
      state.presetAgents.syncLoading = true;
      renderSyncSummary();
      const executed = await syncPresetAgents({ presetId, mode: normalizedMode, dryRun: false });
      state.presetAgents.syncLoading = false;
      renderSyncSummary();
      if (!executed.ok) {
        if (executed.unavailable) {
          notify(t("presetAgents.contract.notReady"), "warn");
          appendSyncLog(t("presetAgents.sync.notReady"), "error");
        } else {
          notify(t("presetAgents.toast.syncFailed", { message: executed.message || "-" }), "error");
          appendSyncLog(
            t("presetAgents.toast.syncFailed", { message: executed.message || "-" }),
            "error"
          );
        }
        return false;
      }
      const done = executed.result || {};
      const text = t("presetAgents.sync.result", {
        mode: isForce ? t("presetAgents.action.syncForce") : t("presetAgents.action.syncSafe"),
        affected: done.affected_users || 0,
        updated: done.updated_agents || 0,
        skipped: done.skipped_customized || 0,
        created: done.created_agents || 0,
      });
      notify(text, "success");
      appendSyncLog(text);
      state.presetAgents.syncPreview = done;
      await reloadAfterBindingChange();
      return true;
    },
  });
};

export const loadPresetAgents = async ({
  silent = false,
  selectedName = "",
  selectedPresetId = "",
  flushDraft = true,
  force = false,
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
    await Promise.all([
      loadModelCatalog({ silent: true }),
      loadGlobalCompanionsForPresetAgents({ silent: true }),
    ]);
    const listed = await listPresetAgentsContract({ force });
    if (!listed.ok) {
      throw new Error(listed.message || t("common.unknownError"));
    }
    state.presetAgents.presets = stabilizePresetListOrder(listed.items, state.presetAgents.presets);

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
    await loadSyncPreview({ silent: true, force });
    await loadBindings({ silent: true, force });
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
    elements.presetUserAgentApproval,
    ...CUSTOMIZABLE_FIELD_META.map((field) => elements[field.elementKey]),
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

// 绑定用户区块：分页 / 筛选 / 批量绑定 / 换绑 / 解绑
const bindBindingControls = () => {
  const bindOnce = (node, handler) => {
    if (!node || node.dataset.bound === "1") {
      return;
    }
    node.dataset.bound = "1";
    node.addEventListener("click", handler);
  };

  bindOnce(elements.presetBindingSearchBtn, () => {
    const bindings = ensureBindingsState();
    bindings.keyword = String(elements.presetBindingSearchInput.value || "").trim();
    bindings.page = 1;
    bindings.selected = new Set();
    loadBindings({ silent: false });
  });
  if (elements.presetBindingSearchInput?.dataset.bound !== "1") {
    elements.presetBindingSearchInput.dataset.bound = "1";
    elements.presetBindingSearchInput.addEventListener("keydown", (event) => {
      if (event.key !== "Enter") {
        return;
      }
      event.preventDefault();
      elements.presetBindingSearchBtn.click();
    });
  }
  bindOnce(elements.presetBindingReloadBtn, () => {
    resetContractState();
    loadBindings({ silent: false, force: true }).then(() => loadSyncPreview({ silent: true, force: true }));
  });
  bindOnce(elements.presetBindingAddBtn, () => openBindingPicker());
  bindOnce(elements.presetBindingRebindBtn, () => {
    const ids = selectedBindingUserIds();
    if (!ids.length) {
      notify(t("presetAgents.bindings.selectRequired"), "warn");
      return;
    }
    openRebindSelectedUsers(ids);
  });
  bindOnce(elements.presetBindingUnbindBtn, () => {
    const ids = selectedBindingUserIds();
    if (!ids.length) {
      notify(t("presetAgents.bindings.selectRequired"), "warn");
      return;
    }
    openUnbindSelectedUsers(ids);
  });
  bindOnce(elements.presetBindingPrevBtn, () => {
    const bindings = ensureBindingsState();
    bindings.page = Math.max(1, bindings.page - 1);
    loadBindings({ silent: false });
  });
  bindOnce(elements.presetBindingNextBtn, () => {
    const bindings = ensureBindingsState();
    bindings.page += 1;
    loadBindings({ silent: false });
  });
  if (elements.presetBindingSelectAll?.dataset.bound !== "1") {
    elements.presetBindingSelectAll.dataset.bound = "1";
    elements.presetBindingSelectAll.addEventListener("change", () => {
      toggleBindingsSelectAll(elements.presetBindingSelectAll.checked);
    });
  }

  // 用户选择器（与面板初始化解耦，打开时也会兜底绑定）
  bindBindingPickerControls();

  // 影响面预览模态
  bindImpactModalControls();

  bindOnce(elements.presetAgentSyncLogClearBtn, () => {
    state.presetAgents.syncLog = [];
    renderSyncLog();
  });
};

const bindPresetAvatarControls = () => {
  if (elements.presetAgentAvatarTrigger.dataset.bound === "1") {
    return;
  }
  elements.presetAgentAvatarTrigger.dataset.bound = "1";

  elements.presetAgentAvatarTrigger.addEventListener("click", openPresetAvatarModal);
  elements.presetAgentAvatarModalClose.addEventListener("click", closePresetAvatarModal);
  elements.presetAgentAvatarModalCancel.addEventListener("click", closePresetAvatarModal);
  elements.presetAgentAvatarModalApply.addEventListener("click", applyPresetAvatarModal);
  elements.presetAgentAvatarModalReset.addEventListener("click", resetPresetAvatarModal);
  elements.presetAgentAvatarStaticTab.addEventListener("click", () => {
    avatarModalState.kind = "static";
    avatarModalState.page = resolveAvatarPageByKey(avatarModalState.iconName);
    renderPresetAvatarModalState();
  });
  elements.presetAgentAvatarGlobalTab.addEventListener("click", () => {
    avatarModalState.kind = "companion";
    avatarModalState.companionScope = "global";
    if (!avatarModalState.companionId) {
      avatarModalState.companionId = globalCompanionItems()[0]?.id || "";
    }
    avatarModalState.page = resolveCompanionPageById(avatarModalState.companionId);
    renderPresetAvatarModalState();
  });

  elements.presetAgentAvatarModal.addEventListener("click", (event) => {
    if (event.target === elements.presetAgentAvatarModal) {
      closePresetAvatarModal();
    }
  });

  elements.presetAgentAvatarPagePrev.addEventListener("click", () => {
    avatarModalState.page = Math.max(1, normalizeAvatarPage(avatarModalState.page) - 1);
    renderPresetAvatarModalState();
  });

  elements.presetAgentAvatarPageNext.addEventListener("click", () => {
    avatarModalState.page = Math.min(resolveAvatarPageCount(), normalizeAvatarPage(avatarModalState.page) + 1);
    renderPresetAvatarModalState();
  });

  elements.presetAgentAvatarColorSelect.addEventListener("change", () => {
    avatarModalState.color = normalizeIconColor(elements.presetAgentAvatarColorSelect.value);
    renderPresetAvatarModalState();
  });
};

const bindActions = () => {
  bindPresetAvatarControls();
  bindBindingControls();
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
      resetContractState();
      await loadPresetAgents({ force: true });
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
  if (elements.presetAgentVisibilityBtn?.dataset.bound !== "1") {
    elements.presetAgentVisibilityBtn.dataset.bound = "1";
    elements.presetAgentVisibilityBtn.addEventListener("click", editPresetVisibility);
  }
  if (elements.presetAgentVisibilityModalClose?.dataset.bound !== "1") {
    elements.presetAgentVisibilityModalClose.dataset.bound = "1";
    elements.presetAgentVisibilityModalClose.addEventListener("click", closePresetVisibilityModal);
  }
  if (elements.presetAgentVisibilityModalCancel?.dataset.bound !== "1") {
    elements.presetAgentVisibilityModalCancel.dataset.bound = "1";
    elements.presetAgentVisibilityModalCancel.addEventListener("click", closePresetVisibilityModal);
  }
  if (elements.presetAgentVisibilityModalSave?.dataset.bound !== "1") {
    elements.presetAgentVisibilityModalSave.dataset.bound = "1";
    elements.presetAgentVisibilityModalSave.addEventListener("click", savePresetVisibilityModal);
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

export const initPresetAgentsPanel = async () => {
  ensureState();
  if (!ensureElements()) {
    return;
  }
  if (state.presetAgents.initialized) {
    return;
  }
  // Load avatar config from backend first
  await Promise.all([
    loadPresetAvatars(),
    loadGlobalCompanionsForPresetAgents({ silent: true }),
  ]);
  bindTabs();
  bindPresetDraftFields();
  bindActions();
  renderAll();
  state.presetAgents.initialized = true;
  appendLog(t("presetAgents.init"));
};

