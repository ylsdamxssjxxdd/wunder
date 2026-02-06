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

const normalizeIcon = (icon) => {
  const cleaned = String(icon || "").trim();
  if (!cleaned) {
    return "fa-globe";
  }
  const match = cleaned.split(/\s+/).find((part) => part.startsWith("fa-"));
  return match || "fa-globe";
};

const normalizeExternalLink = (item) => ({
  link_id: String(item?.link_id || item?.id || "").trim(),
  title: String(item?.title || "").trim(),
  description: String(item?.description || "").trim(),
  url: String(item?.url || "").trim(),
  icon: normalizeIcon(item?.icon),
  allowed_levels: normalizeLevels(item?.allowed_levels),
  sort_order: Number.isFinite(Number(item?.sort_order)) ? Number(item.sort_order) : 0,
  enabled: item?.enabled !== false,
  updated_at: Number(item?.updated_at || 0),
});

const resolveSelectedLink = () =>
  state.externalLinks.list.find((item) => item.link_id === state.externalLinks.selectedId) || null;

const setLevelChecks = (levels) => {
  const selected = new Set(normalizeLevels(levels));
  LEVEL_FIELDS.forEach((item) => {
    const checkbox = elements[item.key];
    checkbox.checked = selected.has(item.level);
  });
};

const readLevelChecks = () =>
  LEVEL_FIELDS.filter((item) => elements[item.key].checked).map((item) => item.level);

const fillForm = (item) => {
  elements.externalLinkFormTitle.value = item?.title || "";
  elements.externalLinkFormUrl.value = item?.url || "";
  elements.externalLinkFormIcon.value = item?.icon || "fa-globe";
  elements.externalLinkFormDescription.value = item?.description || "";
  elements.externalLinkFormSortOrder.value = Number.isFinite(Number(item?.sort_order))
    ? String(Math.floor(Number(item.sort_order)))
    : "0";
  elements.externalLinkFormEnabled.checked = item ? item.enabled !== false : true;
  setLevelChecks(item?.allowed_levels || []);
};

const renderExternalLinkList = () => {
  const list = elements.externalLinkList;
  list.textContent = "";
  if (!state.externalLinks.list.length) {
    list.textContent = t("externalLinks.list.empty");
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
    const icon = document.createElement("i");
    icon.className = "fa-solid " + normalizeIcon(item.icon);
    title.appendChild(icon);
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
    meta.textContent = (item.url || "-") + " · " + levelLabel;
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
  elements.externalLinkDetailTitle.textContent = selected.title || t("externalLinks.detail.empty");
  elements.externalLinkDetailMeta.textContent = selected.url || "";
  elements.externalLinkDeleteBtn.disabled = false;
  fillForm(selected);
};

const collectFormPayload = () => {
  const title = elements.externalLinkFormTitle.value.trim();
  const url = elements.externalLinkFormUrl.value.trim();
  if (!title || !url) {
    throw new Error(t("externalLinks.error.required"));
  }
  const sortOrder = Number.parseInt(elements.externalLinkFormSortOrder.value, 10);
  return {
    link_id: state.externalLinks.selectedId || undefined,
    title,
    description: elements.externalLinkFormDescription.value.trim(),
    url,
    icon: normalizeIcon(elements.externalLinkFormIcon.value),
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
      notify.success(t("externalLinks.toast.refreshSuccess"));
    }
  } catch (error) {
    if (!options.silent) {
      notify.error(t("externalLinks.toast.loadFailed", { message: error.message }));
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
    notify.success(t("externalLinks.toast.saveSuccess"));
  } catch (error) {
    notify.error(t("externalLinks.toast.saveFailed", { message: error.message }));
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
    notify.success(t("externalLinks.toast.deleteSuccess"));
  } catch (error) {
    notify.error(t("externalLinks.toast.deleteFailed", { message: error.message }));
  }
};

const createExternalLink = () => {
  state.externalLinks.selectedId = "";
  renderExternalLinkList();
  renderExternalLinkDetail();
  elements.externalLinkFormTitle.focus();
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
  renderExternalLinkDetail();
  appendLog(t("externalLinks.init"));
};
