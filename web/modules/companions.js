import { elements } from "./elements.js?v=20260506-01";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { appendLog } from "./log.js?v=20260215-01";
import { notify } from "./notify.js";
import { t } from "./i18n.js?v=20260215-01";

const REQUIRED_KEYS = [
  "companionAdminRefreshBtn",
  "companionAdminImportBtn",
  "companionAdminImportInput",
  "companionAdminSearch",
  "companionAdminList",
  "companionAdminEmpty",
  "companionAdminDetailTitle",
  "companionAdminDetailMeta",
  "companionAdminPreview",
  "companionAdminPreviewActions",
  "companionAdminName",
  "companionAdminDescription",
  "companionAdminSaveBtn",
  "companionAdminExportBtn",
  "companionAdminDeleteBtn",
];

const FRAME_WIDTH = 192;
const FRAME_HEIGHT = 208;
const SPRITE_SCALE = 0.46;
const PREVIEW_ACTIONS = [
  { id: "idle", row: 0, frames: 6, duration: 1100 },
  { id: "running-right", row: 1, frames: 8, duration: 1060 },
  { id: "running-left", row: 2, frames: 8, duration: 1060 },
  { id: "waving", row: 3, frames: 4, duration: 700 },
  { id: "jumping", row: 4, frames: 5, duration: 840 },
  { id: "failed", row: 5, frames: 8, duration: 1220 },
  { id: "waiting", row: 6, frames: 6, duration: 1010 },
  { id: "running", row: 7, frames: 6, duration: 820 },
  { id: "review", row: 8, frames: 6, duration: 1030 },
];

let previewAnimationTimer = null;

const ensureState = () => {
  if (!state.companions) {
    state.companions = {
      items: [],
      selectedId: "",
      search: "",
      loading: false,
      previewAction: "idle",
    };
  }
  if (!state.panelLoaded) {
    state.panelLoaded = {};
  }
  if (typeof state.panelLoaded.companions !== "boolean") {
    state.panelLoaded.companions = false;
  }
};

const ensureElements = () => {
  const missing = REQUIRED_KEYS.filter((key) => !elements[key]);
  if (missing.length) {
    appendLog(t("companionsAdmin.domMissing", { nodes: missing.join(", ") }));
    return false;
  }
  return true;
};

const requestJson = async (path, { method = "GET", body } = {}) => {
  const headers = {};
  let payloadBody = body;
  if (body && !(body instanceof FormData)) {
    headers["Content-Type"] = "application/json";
    payloadBody = JSON.stringify(body);
  }
  const response = await fetch(`${getWunderBase()}${path}`, {
    method,
    headers,
    body: payloadBody,
  });
  const payload = await response.json().catch(() => ({}));
  if (!response.ok) {
    const message =
      payload?.error?.message || payload?.detail?.message || payload?.detail || payload?.message || String(response.status);
    throw new Error(message);
  }
  return payload;
};

const normalizeCompanion = (item) => ({
  id: String(item?.id || "").trim(),
  display_name: String(item?.display_name || item?.displayName || item?.name || "").trim(),
  description: String(item?.description || "").trim(),
  spritesheet_path: String(item?.spritesheet_path || item?.spritesheetPath || "").trim(),
  spritesheet_data_url: String(item?.spritesheet_data_url || item?.spritesheetDataUrl || "").trim(),
  imported_at: Number(item?.imported_at || item?.importedAt || 0),
  updated_at: Number(item?.updated_at || item?.updatedAt || 0),
});

export const listGlobalCompanions = async () => {
  const payload = await requestJson("/admin/companions");
  return (Array.isArray(payload?.data?.items) ? payload.data.items : [])
    .map(normalizeCompanion)
    .filter((item) => item.id && item.display_name && item.spritesheet_data_url);
};

const selectedCompanion = () =>
  (state.companions.items || []).find((item) => item.id === state.companions.selectedId) || null;

const filteredCompanions = () => {
  const keyword = String(state.companions.search || "").trim().toLowerCase();
  const items = Array.isArray(state.companions.items) ? state.companions.items : [];
  if (!keyword) {
    return items;
  }
  return items.filter((item) =>
    [item.id, item.display_name, item.description, item.spritesheet_path].some((value) =>
      String(value || "").toLowerCase().includes(keyword)
    )
  );
};

const formatTime = (value) => {
  const numeric = Number(value);
  if (!Number.isFinite(numeric) || numeric <= 0) {
    return "-";
  }
  try {
    return new Date(numeric * 1000).toLocaleString();
  } catch (_error) {
    return "-";
  }
};

const stopPreviewAnimation = () => {
  if (previewAnimationTimer !== null && typeof window !== "undefined") {
    window.clearInterval(previewAnimationTimer);
  }
  previewAnimationTimer = null;
};

const renderSpritePreview = (container, item) => {
  container.textContent = "";
  if (!item?.spritesheet_data_url) {
    return;
  }
  const viewport = document.createElement("span");
  viewport.className = "companion-admin-sprite";
  viewport.style.width = `${FRAME_WIDTH * SPRITE_SCALE}px`;
  viewport.style.height = `${FRAME_HEIGHT * SPRITE_SCALE}px`;
  const sheet = document.createElement("span");
  sheet.className = "companion-admin-sprite-sheet";
  sheet.style.width = `${FRAME_WIDTH}px`;
  sheet.style.height = `${FRAME_HEIGHT}px`;
  sheet.style.backgroundImage = `url("${item.spritesheet_data_url}")`;
  sheet.style.backgroundPosition = "0 0";
  sheet.style.transform = `scale(${SPRITE_SCALE})`;
  viewport.appendChild(sheet);
  container.appendChild(viewport);
};

const renderPreviewSprite = (container, item, actionId) => {
  stopPreviewAnimation();
  container.textContent = "";
  if (!item?.spritesheet_data_url) {
    return;
  }
  const action = PREVIEW_ACTIONS.find((entry) => entry.id === actionId) || PREVIEW_ACTIONS[0];
  const viewport = document.createElement("span");
  viewport.className = "companion-admin-sprite companion-admin-sprite--detail";
  viewport.style.width = `${FRAME_WIDTH * 0.9}px`;
  viewport.style.height = `${FRAME_HEIGHT * 0.9}px`;
  const sheet = document.createElement("span");
  sheet.className = "companion-admin-sprite-sheet";
  sheet.style.width = `${FRAME_WIDTH}px`;
  sheet.style.height = `${FRAME_HEIGHT}px`;
  sheet.style.backgroundImage = `url("${item.spritesheet_data_url}")`;
  sheet.style.backgroundPosition = `0 -${action.row * FRAME_HEIGHT}px`;
  sheet.style.transform = "scale(0.9)";
  viewport.appendChild(sheet);
  container.appendChild(viewport);
  if (typeof window === "undefined" || action.frames <= 1) {
    return;
  }
  let frameIndex = 0;
  const frameMs = Math.max(50, Math.round(action.duration / Math.max(1, action.frames)));
  previewAnimationTimer = window.setInterval(() => {
    frameIndex = (frameIndex + 1) % action.frames;
    sheet.style.backgroundPosition = `-${frameIndex * FRAME_WIDTH}px -${action.row * FRAME_HEIGHT}px`;
  }, frameMs);
};

const renderPreviewActions = () => {
  const container = elements.companionAdminPreviewActions;
  if (!container) {
    return;
  }
  container.textContent = "";
  const fragment = document.createDocumentFragment();
  PREVIEW_ACTIONS.forEach((entry) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "secondary companion-admin-preview-action";
    button.classList.toggle("is-active", entry.id === state.companions.previewAction);
    button.textContent = t(`companions.state.${entry.id}`);
    button.addEventListener("click", () => {
      state.companions.previewAction = entry.id;
      renderDetail();
    });
    fragment.appendChild(button);
  });
  container.appendChild(fragment);
};

const renderList = () => {
  const list = elements.companionAdminList;
  const empty = elements.companionAdminEmpty;
  list.textContent = "";
  const items = filteredCompanions();
  empty.style.display = items.length ? "none" : "block";
  const fragment = document.createDocumentFragment();
  items.forEach((item) => {
    const row = document.createElement("button");
    row.type = "button";
    row.className = "companion-admin-item";
    row.classList.toggle("is-active", item.id === state.companions.selectedId);
    const preview = document.createElement("span");
    preview.className = "companion-admin-item-preview";
    renderSpritePreview(preview, item);
    const main = document.createElement("span");
    main.className = "companion-admin-item-main";
    const title = document.createElement("span");
    title.className = "companion-admin-item-title";
    title.textContent = item.display_name;
    const meta = document.createElement("span");
    meta.className = "companion-admin-item-meta";
    meta.textContent = item.description || item.id;
    main.appendChild(title);
    main.appendChild(meta);
    row.appendChild(preview);
    row.appendChild(main);
    row.addEventListener("click", () => {
      state.companions.selectedId = item.id;
      renderAll();
    });
    fragment.appendChild(row);
  });
  list.appendChild(fragment);
};

const renderDetail = () => {
  const item = selectedCompanion();
  const hasItem = Boolean(item);
  elements.companionAdminDetailTitle.textContent = hasItem
    ? item.display_name
    : t("companionsAdmin.detailEmpty");
  elements.companionAdminDetailMeta.textContent = hasItem
    ? [
        `id: ${item.id}`,
        item.spritesheet_path,
        `${t("companionsAdmin.updatedAt")}: ${formatTime(item.updated_at)}`,
      ].filter(Boolean).join(" | ")
    : "";
  elements.companionAdminName.value = item?.display_name || "";
  elements.companionAdminDescription.value = item?.description || "";
  elements.companionAdminSaveBtn.disabled = !hasItem || state.companions.loading;
  elements.companionAdminExportBtn.disabled = !hasItem || state.companions.loading;
  elements.companionAdminDeleteBtn.disabled = !hasItem || state.companions.loading;
  if (!hasItem) {
    stopPreviewAnimation();
  }
  renderPreviewSprite(elements.companionAdminPreview, item, state.companions.previewAction);
  renderPreviewActions();
};

const renderAll = () => {
  renderList();
  renderDetail();
};

export const loadCompanions = async ({ silent = false } = {}) => {
  ensureState();
  if (!ensureElements()) {
    return;
  }
  state.companions.loading = true;
  try {
    const previous = state.companions.selectedId;
    const items = await listGlobalCompanions();
    state.companions.items = items;
    state.companions.selectedId = items.some((item) => item.id === previous)
      ? previous
      : items[0]?.id || "";
    renderAll();
    if (!silent) {
      notify(t("companionsAdmin.toast.refreshSuccess"), "success");
    }
  } catch (error) {
    if (!silent) {
      notify(t("companionsAdmin.toast.refreshFailed", { message: error.message || "-" }), "error");
    }
    throw error;
  } finally {
    state.companions.loading = false;
    renderDetail();
  }
};

const importCompanion = async (file) => {
  const form = new FormData();
  form.append("file", file, file.name || "companion.zip");
  await requestJson("/admin/companions", { method: "POST", body: form });
};

const handleImportChange = async () => {
  const file = elements.companionAdminImportInput.files?.[0] || null;
  elements.companionAdminImportInput.value = "";
  if (!file) {
    return;
  }
  try {
    await importCompanion(file);
    await loadCompanions({ silent: true });
    notify(t("companionsAdmin.toast.importSuccess"), "success");
  } catch (error) {
    notify(t("companionsAdmin.toast.importFailed", { message: error.message || "-" }), "error");
  }
};

const saveSelected = async () => {
  const item = selectedCompanion();
  if (!item) {
    return;
  }
  try {
    await requestJson(`/admin/companions/${encodeURIComponent(item.id)}`, {
      method: "PATCH",
      body: {
        display_name: elements.companionAdminName.value,
        description: elements.companionAdminDescription.value,
      },
    });
    await loadCompanions({ silent: true });
    notify(t("companionsAdmin.toast.saveSuccess"), "success");
  } catch (error) {
    notify(t("companionsAdmin.toast.saveFailed", { message: error.message || "-" }), "error");
  }
};

const downloadBlob = (blob, filename) => {
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  document.body.appendChild(anchor);
  anchor.click();
  document.body.removeChild(anchor);
  window.setTimeout(() => URL.revokeObjectURL(url), 1200);
};

const exportSelected = async () => {
  const item = selectedCompanion();
  if (!item) {
    return;
  }
  try {
    const response = await fetch(`${getWunderBase()}/admin/companions/${encodeURIComponent(item.id)}/package`);
    if (!response.ok) {
      throw new Error(`HTTP ${response.status}`);
    }
    const blob = await response.blob();
    downloadBlob(blob, `${item.id}.zip`);
  } catch (error) {
    notify(t("companionsAdmin.toast.exportFailed", { message: error.message || "-" }), "error");
  }
};

const deleteSelected = async () => {
  const item = selectedCompanion();
  if (!item) {
    return;
  }
  if (!window.confirm(t("companionsAdmin.confirmDelete", { name: item.display_name }))) {
    return;
  }
  try {
    await requestJson(`/admin/companions/${encodeURIComponent(item.id)}`, { method: "DELETE" });
    await loadCompanions({ silent: true });
    notify(t("companionsAdmin.toast.deleteSuccess"), "success");
  } catch (error) {
    notify(t("companionsAdmin.toast.deleteFailed", { message: error.message || "-" }), "error");
  }
};

export const initCompanionsPanel = () => {
  ensureState();
  if (!ensureElements()) {
    return;
  }
  if (elements.companionAdminRefreshBtn.dataset.bound === "1") {
    return;
  }
  elements.companionAdminRefreshBtn.dataset.bound = "1";
  elements.companionAdminRefreshBtn.addEventListener("click", () => {
    loadCompanions({ silent: false }).catch(() => undefined);
  });
  elements.companionAdminImportBtn.addEventListener("click", () => {
    elements.companionAdminImportInput.click();
  });
  elements.companionAdminImportInput.addEventListener("change", handleImportChange);
  elements.companionAdminSearch.addEventListener("input", () => {
    state.companions.search = String(elements.companionAdminSearch.value || "");
    renderList();
  });
  elements.companionAdminSaveBtn.addEventListener("click", saveSelected);
  elements.companionAdminExportBtn.addEventListener("click", exportSelected);
  elements.companionAdminDeleteBtn.addEventListener("click", deleteSelected);
  renderAll();
};
