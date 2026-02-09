import { elements } from "./elements.js?v=20260124-01";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { appendLog } from "./log.js?v=20260108-02";
import { notify } from "./notify.js";
import { t } from "./i18n.js?v=20260124-01";

const LEVEL_FIELDS = [
  { level: 1, key: "externalLinkLevel1" },
  { level: 2, key: "externalLinkLevel2" },
  { level: 3, key: "externalLinkLevel3" },
  { level: 4, key: "externalLinkLevel4" },
];

const ICON_OPTIONS = [
  "fa-globe",
  "fa-book-open",
  "fa-graduation-cap",
  "fa-briefcase",
  "fa-chart-line",
  "fa-shop",
  "fa-comments",
  "fa-headset",
  "fa-cloud",
  "fa-rocket",
  "fa-compass",
  "fa-lightbulb",
  "fa-code",
  "fa-gears",
  "fa-shield-halved",
  "fa-life-ring",
  "fa-bolt",
  "fa-link",
  "fa-desktop",
  "fa-screwdriver-wrench",
];

const DEFAULT_ICON_NAME = "fa-globe";
const DEFAULT_ICON_COLOR = "#2563eb";
const META_SEPARATOR = " | ";

const iconModalState = {
  name: DEFAULT_ICON_NAME,
  color: "",
};

const ensureExternalLinksState = () => {
  if (!state.externalLinks) {
    state.externalLinks = {
      list: [],
      selectedId: "",
      loading: false,
      loaded: false,
    };
  }
  if (!state.panelLoaded) {
    state.panelLoaded = {};
  }
  if (typeof state.panelLoaded.externalLinks !== "boolean") {
    state.panelLoaded.externalLinks = false;
  }
};

const ensureExternalLinkElements = () => {
  const requiredKeys = [
    "externalLinkRefreshBtn",
    "externalLinkCreateBtn",
    "externalLinkList",
    "externalLinkDetailTitle",
    "externalLinkDetailMeta",
    "externalLinkFormTitle",
    "externalLinkFormUrl",
    "externalLinkFormIcon",
    "externalLinkIconTrigger",
    "externalLinkIconPreview",
    "externalLinkIconName",
    "externalLinkIconModal",
    "externalLinkIconModalClose",
    "externalLinkIconModalCancel",
    "externalLinkIconModalApply",
    "externalLinkIconModalPreview",
    "externalLinkIconPicker",
    "externalLinkIconColorInput",
    "externalLinkIconColorText",
    "externalLinkIconColorReset",
    "externalLinkFormDescription",
    "externalLinkFormSortOrder",
    "externalLinkFormEnabled",
    "externalLinkSaveBtn",
    "externalLinkDeleteBtn",
    ...LEVEL_FIELDS.map((item) => item.key),
  ];
  const missing = requiredKeys.filter((key) => !elements[key]);
  if (missing.length) {
    appendLog(t("externalLinks.domMissing", { nodes: missing.join(", ") }));
    return false;
  }
  return true;
};

const normalizeLevels = (levels) => {
  if (!Array.isArray(levels)) {
    return [];
  }
  const output = levels
    .map((item) => Number(item))
    .filter((item) => Number.isFinite(item) && item >= 1 && item <= 4)
    .map((item) => Math.floor(item));
  output.sort((left, right) => left - right);
  return Array.from(new Set(output));
};

const normalizeIconName = (icon) => {
  const cleaned = String(icon || "").trim();
  if (!cleaned) {
    return DEFAULT_ICON_NAME;
  }
  const match = cleaned.split(/\s+/).find((part) => part.startsWith("fa-"));
  return match || DEFAULT_ICON_NAME;
};

const normalizeColor = (color) => {
  const cleaned = String(color || "").trim();
  if (!cleaned) {
    return "";
  }
  const match = cleaned.match(/^#?([0-9a-fA-F]{3}|[0-9a-fA-F]{6})$/);
  if (!match) {
    return "";
  }
  let hex = match[1].toLowerCase();
  if (hex.length === 3) {
    hex = hex
      .split("")
      .map((part) => part + part)
      .join("");
  }
  return "#" + hex;
};

const parseIconConfig = (value) => {
  const raw = String(value || "").trim();
  if (!raw) {
    return { name: DEFAULT_ICON_NAME, color: "" };
  }
  try {
    const parsed = JSON.parse(raw);
    if (typeof parsed === "string") {
      return { name: normalizeIconName(parsed), color: "" };
    }
    if (parsed && typeof parsed === "object") {
      return {
        name: normalizeIconName(parsed.name),
        color: normalizeColor(parsed.color),
      };
    }
  } catch (error) {
    // Fall back to plain icon string.
  }
  return {
    name: normalizeIconName(raw),
    color: "",
  };
};

const serializeIconConfig = (config) => {
  const name = normalizeIconName(config?.name);
  const color = normalizeColor(config?.color);
  if (!color) {
    return name;
  }
  return JSON.stringify({ name, color });
};

const normalizeExternalLink = (item) => {
  const iconConfig = parseIconConfig(item?.icon);
  return {
    link_id: String(item?.link_id || item?.id || "").trim(),
    title: String(item?.title || "").trim(),
    description: String(item?.description || "").trim(),
    url: String(item?.url || "").trim(),
    icon: serializeIconConfig(iconConfig),
    icon_name: iconConfig.name,
    icon_color: iconConfig.color,
    allowed_levels: normalizeLevels(item?.allowed_levels),
    sort_order: Number.isFinite(Number(item?.sort_order)) ? Number(item.sort_order) : 0,
    enabled: item?.enabled !== false,
    updated_at: Number(item?.updated_at || 0),
  };
};

const resolveSelectedLink = () =>
  state.externalLinks.list.find((item) => item.link_id === state.externalLinks.selectedId) || null;

const getHostLabel = (url) => {
  const cleaned = String(url || "").trim();
  if (!cleaned) {
    return "-";
  }
  try {
    const parsed = new URL(cleaned);
    return parsed.host || cleaned;
  } catch (error) {
    return cleaned;
  }
};

const setLevelChecks = (levels) => {
  const selected = new Set(normalizeLevels(levels));
  LEVEL_FIELDS.forEach((item) => {
    const checkbox = elements[item.key];
    checkbox.checked = selected.has(item.level);
  });
};

const readLevelChecks = () =>
  LEVEL_FIELDS.filter((item) => elements[item.key].checked).map((item) => item.level);

const setFormIconConfig = (config) => {
  elements.externalLinkFormIcon.value = normalizeIconName(config?.name);
  const normalizedColor = normalizeColor(config?.color);
  elements.externalLinkFormIcon.dataset.color = normalizedColor;
};

const getFormIconConfig = () => ({
  name: normalizeIconName(elements.externalLinkFormIcon.value),
  color: normalizeColor(elements.externalLinkFormIcon.dataset.color),
});

const applyPreview = (container, config) => {
  if (!container) {
    return;
  }
  const name = normalizeIconName(config?.name);
  const color = normalizeColor(config?.color);
  let iconNode = container.querySelector("i");
  if (!iconNode) {
    iconNode = document.createElement("i");
    container.appendChild(iconNode);
  }
  iconNode.className = "fa-solid " + name;
  container.style.color = color || "";
};

const syncIconPreview = () => {
  const config = getFormIconConfig();
  applyPreview(elements.externalLinkIconPreview, config);
  if (elements.externalLinkIconName) {
    elements.externalLinkIconName.textContent = config.name;
    elements.externalLinkIconName.style.color = config.color || "";
  }
};

const syncIconPickerSelection = (activeName) => {
  const name = normalizeIconName(activeName);
  const options = elements.externalLinkIconPicker.querySelectorAll(".external-link-icon-option");
  options.forEach((node) => {
    node.classList.toggle("is-active", node.dataset.icon === name);
  });
};

const syncModalPreview = () => {
  applyPreview(elements.externalLinkIconModalPreview, {
    name: iconModalState.name,
    color: iconModalState.color,
  });
  syncIconPickerSelection(iconModalState.name);
};

const syncColorInputsFromState = () => {
  elements.externalLinkIconColorInput.value = iconModalState.color || DEFAULT_ICON_COLOR;
  elements.externalLinkIconColorText.value = iconModalState.color || "";
};

const fillForm = (item) => {
  elements.externalLinkFormTitle.value = item?.title || "";
  elements.externalLinkFormUrl.value = item?.url || "";
  setFormIconConfig({ name: item?.icon_name || DEFAULT_ICON_NAME, color: item?.icon_color || "" });
  elements.externalLinkFormDescription.value = item?.description || "";
  elements.externalLinkFormSortOrder.value = Number.isFinite(Number(item?.sort_order))
    ? String(Math.floor(Number(item.sort_order)))
    : "0";
  elements.externalLinkFormEnabled.checked = item ? item.enabled !== false : true;
  setLevelChecks(item?.allowed_levels || []);
  syncIconPreview();
};

const renderListEmptyState = () => {
  const empty = document.createElement("div");
  empty.className = "external-link-list-empty";
  const icon = document.createElement("i");
  icon.className = "fa-solid fa-arrow-up-right-from-square";
  icon.setAttribute("aria-hidden", "true");
  const text = document.createElement("span");
  text.textContent = t("externalLinks.list.empty");
  empty.appendChild(icon);
  empty.appendChild(text);
  return empty;
};

const renderExternalLinkList = () => {
  const list = elements.externalLinkList;
  list.textContent = "";
  if (!state.externalLinks.list.length) {
    list.appendChild(renderListEmptyState());
    return;
  }
  const fragment = document.createDocumentFragment();
  state.externalLinks.list.forEach((item) => {
    const row = document.createElement("button");
    row.type = "button";
    row.className = "external-link-item";
    if (item.link_id === state.externalLinks.selectedId) {
      row.classList.add("is-active");
    }

    const title = document.createElement("div");
    title.className = "external-link-item-title";
    const iconWrap = document.createElement("span");
    iconWrap.className = "external-link-item-icon";
    const icon = document.createElement("i");
    icon.className = "fa-solid " + normalizeIconName(item.icon_name);
    if (item.icon_color) {
      icon.style.color = item.icon_color;
    }
    iconWrap.appendChild(icon);
    title.appendChild(iconWrap);

    const titleText = document.createElement("span");
    titleText.textContent = item.title || "-";
    title.appendChild(titleText);
    row.appendChild(title);

    const meta = document.createElement("div");
    meta.className = "external-link-item-meta";
    const levelLabel =
      item.allowed_levels.length > 0
        ? item.allowed_levels.map((level) => t("externalLinks.level." + level)).join(" / ")
        : t("externalLinks.level.all");
    meta.textContent = [getHostLabel(item.url), levelLabel].join(META_SEPARATOR);
    row.appendChild(meta);

    if (!item.enabled) {
      const badge = document.createElement("span");
      badge.className = "external-link-item-badge";
      badge.textContent = t("externalLinks.status.disabled");
      row.appendChild(badge);
    }

    row.addEventListener("click", () => {
      state.externalLinks.selectedId = item.link_id;
      renderExternalLinkList();
      renderExternalLinkDetail();
    });
    fragment.appendChild(row);
  });
  list.appendChild(fragment);
};

const renderExternalLinkDetail = () => {
  const selected = resolveSelectedLink();
  if (!selected) {
    elements.externalLinkDetailTitle.textContent = t("externalLinks.detail.new");
    elements.externalLinkDetailMeta.textContent = t("externalLinks.detail.newHint");
    elements.externalLinkDeleteBtn.disabled = true;
    fillForm(null);
    return;
  }
  const levelLabel =
    selected.allowed_levels.length > 0
      ? selected.allowed_levels.map((level) => t("externalLinks.level." + level)).join(" / ")
      : t("externalLinks.level.all");
  const statusLabel = selected.enabled ? t("externalLinks.status.enabled") : t("externalLinks.status.disabled");
  elements.externalLinkDetailTitle.textContent = selected.title || t("externalLinks.detail.empty");
  elements.externalLinkDetailMeta.textContent = [selected.url || "-", levelLabel, statusLabel].join(META_SEPARATOR);
  elements.externalLinkDeleteBtn.disabled = false;
  fillForm(selected);
};

const normalizeExternalUrl = (value) => {
  const cleaned = String(value || "").trim();
  if (!cleaned) {
    return "";
  }
  let parsed;
  try {
    parsed = new URL(cleaned);
  } catch (error) {
    throw new Error(t("externalLinks.error.invalidUrl"));
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
    throw new Error(t("externalLinks.error.invalidUrl"));
  }
  return cleaned;
};

const collectFormPayload = () => {
  const title = elements.externalLinkFormTitle.value.trim();
  const rawUrl = elements.externalLinkFormUrl.value.trim();
  if (!title || !rawUrl) {
    throw new Error(t("externalLinks.error.required"));
  }
  const sortOrder = Number.parseInt(elements.externalLinkFormSortOrder.value, 10);
  return {
    link_id: state.externalLinks.selectedId || undefined,
    title,
    description: elements.externalLinkFormDescription.value.trim(),
    url: normalizeExternalUrl(rawUrl),
    icon: serializeIconConfig(getFormIconConfig()),
    allowed_levels: readLevelChecks(),
    sort_order: Number.isFinite(sortOrder) ? sortOrder : 0,
    enabled: Boolean(elements.externalLinkFormEnabled.checked),
  };
};

const requestJson = async (path, init = {}) => {
  const response = await fetch(getWunderBase() + path, {
    method: "GET",
    ...init,
    headers: {
      "Content-Type": "application/json",
      ...(init.headers || {}),
    },
  });
  const payload = await response.json().catch(() => ({}));
  if (!response.ok) {
    throw new Error(
      payload?.detail?.message || payload?.detail || payload?.message || String(response.status)
    );
  }
  return payload;
};

export const loadExternalLinks = async (options = {}) => {
  ensureExternalLinksState();
  if (!ensureExternalLinkElements()) {
    return;
  }
  state.externalLinks.loading = true;
  try {
    const payload = await requestJson("/admin/external_links");
    const list = Array.isArray(payload?.data?.items) ? payload.data.items : [];
    state.externalLinks.list = list
      .map(normalizeExternalLink)
      .filter((item) => item.link_id)
      .sort((left, right) => left.sort_order - right.sort_order || right.updated_at - left.updated_at);

    const preferredId = String(options.selectedId || "").trim();
    if (preferredId && state.externalLinks.list.some((item) => item.link_id === preferredId)) {
      state.externalLinks.selectedId = preferredId;
    } else if (
      !state.externalLinks.selectedId ||
      !state.externalLinks.list.some((item) => item.link_id === state.externalLinks.selectedId)
    ) {
      state.externalLinks.selectedId = state.externalLinks.list[0]?.link_id || "";
    }

    state.externalLinks.loaded = true;
    renderExternalLinkList();
    renderExternalLinkDetail();
    if (!options.silent) {
      notify(t("externalLinks.toast.refreshSuccess"), "success");
    }
  } catch (error) {
    if (!options.silent) {
      notify(t("externalLinks.toast.loadFailed", { message: error.message }), "error");
    }
  } finally {
    state.externalLinks.loading = false;
  }
};

const saveExternalLink = async () => {
  try {
    const payload = collectFormPayload();
    const response = await requestJson("/admin/external_links", {
      method: "POST",
      body: JSON.stringify(payload),
    });
    const saved = normalizeExternalLink(response?.data || {});
    await loadExternalLinks({ silent: true, selectedId: saved.link_id });
    notify(t("externalLinks.toast.saveSuccess"), "success");
  } catch (error) {
    notify(t("externalLinks.toast.saveFailed", { message: error.message }), "error");
  }
};

const deleteExternalLink = async () => {
  const selected = resolveSelectedLink();
  if (!selected?.link_id) {
    return;
  }
  const confirmed = window.confirm(
    t("externalLinks.confirmDelete", { name: selected.title || selected.link_id })
  );
  if (!confirmed) {
    return;
  }
  try {
    await requestJson("/admin/external_links/" + encodeURIComponent(selected.link_id), {
      method: "DELETE",
    });
    await loadExternalLinks({ silent: true });
    notify(t("externalLinks.toast.deleteSuccess"), "success");
  } catch (error) {
    notify(t("externalLinks.toast.deleteFailed", { message: error.message }), "error");
  }
};

const createExternalLink = () => {
  state.externalLinks.selectedId = "";
  renderExternalLinkList();
  renderExternalLinkDetail();
  elements.externalLinkFormTitle.focus();
};

const renderIconPicker = () => {
  const picker = elements.externalLinkIconPicker;
  if (!picker) {
    return;
  }
  if (picker.dataset.rendered === "1") {
    syncIconPickerSelection(iconModalState.name);
    return;
  }
  picker.textContent = "";
  const fragment = document.createDocumentFragment();
  ICON_OPTIONS.forEach((iconName) => {
    const option = document.createElement("button");
    option.type = "button";
    option.className = "external-link-icon-option";
    option.dataset.icon = iconName;
    option.title = iconName;
    option.setAttribute("aria-label", iconName);

    const icon = document.createElement("i");
    icon.className = "fa-solid " + iconName;
    icon.setAttribute("aria-hidden", "true");
    option.appendChild(icon);

    option.addEventListener("click", () => {
      iconModalState.name = iconName;
      syncModalPreview();
    });

    fragment.appendChild(option);
  });
  picker.appendChild(fragment);
  picker.dataset.rendered = "1";
  syncIconPickerSelection(iconModalState.name);
};

const openIconModal = () => {
  const formIcon = getFormIconConfig();
  iconModalState.name = formIcon.name;
  iconModalState.color = formIcon.color;
  renderIconPicker();
  syncColorInputsFromState();
  syncModalPreview();
  elements.externalLinkIconModal.classList.add("active");
};

const closeIconModal = () => {
  elements.externalLinkIconModal.classList.remove("active");
};

const applyIconModal = () => {
  setFormIconConfig({ name: iconModalState.name, color: iconModalState.color });
  syncIconPreview();
  closeIconModal();
};

const bindIconControls = () => {
  if (elements.externalLinkIconTrigger.dataset.bound === "1") {
    return;
  }
  elements.externalLinkIconTrigger.dataset.bound = "1";

  elements.externalLinkIconTrigger.addEventListener("click", openIconModal);
  elements.externalLinkIconModalClose.addEventListener("click", closeIconModal);
  elements.externalLinkIconModalCancel.addEventListener("click", closeIconModal);
  elements.externalLinkIconModalApply.addEventListener("click", applyIconModal);

  elements.externalLinkIconModal.addEventListener("click", (event) => {
    if (event.target === elements.externalLinkIconModal) {
      closeIconModal();
    }
  });

  elements.externalLinkIconColorInput.addEventListener("input", () => {
    iconModalState.color = normalizeColor(elements.externalLinkIconColorInput.value);
    syncColorInputsFromState();
    syncModalPreview();
  });

  elements.externalLinkIconColorText.addEventListener("input", () => {
    const cleaned = String(elements.externalLinkIconColorText.value || "").trim();
    if (!cleaned) {
      iconModalState.color = "";
      syncColorInputsFromState();
      syncModalPreview();
      return;
    }
    const normalized = normalizeColor(cleaned);
    if (!normalized) {
      return;
    }
    iconModalState.color = normalized;
    syncColorInputsFromState();
    syncModalPreview();
  });

  elements.externalLinkIconColorText.addEventListener("blur", () => {
    syncColorInputsFromState();
  });

  elements.externalLinkIconColorReset.addEventListener("click", () => {
    iconModalState.color = "";
    syncColorInputsFromState();
    syncModalPreview();
  });
};

export const initExternalLinksPanel = () => {
  ensureExternalLinksState();
  if (!ensureExternalLinkElements()) {
    return;
  }
  if (elements.externalLinkRefreshBtn.dataset.bound === "1") {
    return;
  }

  elements.externalLinkRefreshBtn.dataset.bound = "1";
  elements.externalLinkRefreshBtn.addEventListener("click", () => {
    loadExternalLinks();
  });
  elements.externalLinkCreateBtn.addEventListener("click", createExternalLink);
  elements.externalLinkSaveBtn.addEventListener("click", saveExternalLink);
  elements.externalLinkDeleteBtn.addEventListener("click", deleteExternalLink);

  bindIconControls();
  renderIconPicker();
  renderExternalLinkDetail();
  appendLog(t("externalLinks.init"));
};
