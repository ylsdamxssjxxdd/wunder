import { t } from "./i18n.js?v=20260610-01";
import { formatBytes } from "./utils.js?v=20251229-02";

let initialized = false;

const getElements = () => ({
  modal: document.getElementById("workspacePropertiesModal"),
  icon: document.getElementById("workspacePropertiesIcon"),
  name: document.getElementById("workspacePropertiesName"),
  subtitle: document.getElementById("workspacePropertiesSubtitle"),
  list: document.getElementById("workspacePropertiesList"),
  hint: document.getElementById("workspacePropertiesHint"),
  close: document.getElementById("workspacePropertiesClose"),
  closeBtn: document.getElementById("workspacePropertiesCloseBtn"),
});

const normalizePath = (path) => String(path || "").replace(/\\/g, "/").replace(/^\/+/, "");

const extensionOf = (entry) => {
  const rawName = String(entry?.name || entry?.path || "");
  const baseName = rawName.split("/").pop().split("\\").pop();
  const dotIndex = baseName.lastIndexOf(".");
  if (dotIndex === -1 || dotIndex === baseName.length - 1) {
    return "";
  }
  return baseName.slice(dotIndex + 1).toLowerCase();
};

const normalizeTimestamp = (value) => {
  if (value === null || value === undefined || value === "") {
    return 0;
  }
  const date = new Date(value);
  if (!Number.isNaN(date.getTime())) {
    return date.getTime();
  }
  const numeric = Number(value);
  if (!Number.isFinite(numeric)) {
    return 0;
  }
  return numeric < 1_000_000_000_000 ? numeric * 1000 : numeric;
};

const formatTimestamp = (value) => {
  const timestamp = normalizeTimestamp(value);
  if (!timestamp) {
    return "";
  }
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) {
    return "";
  }
  return date.toLocaleString();
};

const collectLoadedDirectoryStats = (entry) => {
  const result = {
    loaded: Array.isArray(entry?.children),
    files: 0,
    folders: 0,
    size: 0,
  };
  const walk = (items) => {
    if (!Array.isArray(items)) {
      return;
    }
    items.forEach((item) => {
      if (!item || typeof item !== "object") {
        return;
      }
      if (item.type === "dir") {
        result.folders += 1;
        walk(item.children);
        return;
      }
      result.files += 1;
      result.size += Number(item.size) || 0;
    });
  };
  if (result.loaded) {
    walk(entry.children);
  }
  return result;
};

const defaultIcon = (entry) =>
  entry?.type === "dir"
    ? { icon: "fa-folder", className: "icon-folder" }
    : { icon: "fa-file", className: "icon-file" };

const resolveIcon = (entry, options) => {
  if (typeof options.resolveIcon === "function") {
    const icon = options.resolveIcon(entry);
    if (icon?.icon) {
      return icon;
    }
  }
  return defaultIcon(entry);
};

const resolveTypeLabel = (entry, options) => {
  if (!entry) {
    return t("workspace.properties.unavailable");
  }
  if (typeof options.resolveTypeLabel === "function") {
    const label = options.resolveTypeLabel(entry);
    if (label) {
      return label;
    }
  }
  if (entry.type === "dir") {
    return t("workspace.entry.folder");
  }
  const extension = extensionOf(entry);
  const fileLabel = t("workspace.entry.file");
  return extension ? `${fileLabel} (.${extension})` : fileLabel;
};

const buildRows = (entry, options) => {
  const normalizedPath = normalizePath(entry.path || "");
  const rows = [
    {
      key: "name",
      label: t("workspace.properties.name"),
      value: String(entry.name || t("workspace.properties.unnamed")),
    },
    {
      key: "type",
      label: t("workspace.properties.type"),
      value: resolveTypeLabel(entry, options),
    },
    {
      key: "path",
      label: t("workspace.properties.path"),
      value: normalizedPath ? `/${normalizedPath}` : "/",
    },
  ];

  if (entry.type === "dir") {
    const stats = collectLoadedDirectoryStats(entry);
    rows.push({
      key: "children",
      label: t("workspace.properties.children"),
      value: stats.loaded
        ? t("workspace.properties.loadedChildrenSummary", {
            folders: stats.folders,
            files: stats.files,
          })
        : t("workspace.properties.notLoaded"),
    });
    rows.push({
      key: "loaded-size",
      label: t("workspace.properties.loadedSize"),
      value: stats.loaded ? formatBytes(stats.size) : t("workspace.properties.notLoaded"),
    });
  } else {
    const extension = extensionOf(entry);
    if (extension) {
      rows.push({
        key: "extension",
        label: t("workspace.properties.extension"),
        value: `.${extension}`,
      });
    }
    rows.push({
      key: "size",
      label: t("workspace.properties.size"),
      value: formatBytes(Number(entry.size) || 0),
    });
  }

  rows.push({
    key: "modified",
    label: t("workspace.properties.modified"),
    value:
      formatTimestamp(
        entry.updated_time || entry.updatedAt || entry.modified_at || entry.modifiedTime
      ) || t("workspace.properties.unavailable"),
  });

  if (options.containerValue !== undefined && options.containerValue !== null) {
    rows.push({
      key: "container",
      label: options.containerLabel || t("workspace.properties.container"),
      value: String(options.containerValue),
    });
  }

  if (Array.isArray(options.extraRows)) {
    options.extraRows.forEach((row) => {
      if (row?.label) {
        rows.push({
          key: row.key || row.label,
          label: row.label,
          value: row.value ?? t("workspace.properties.unavailable"),
        });
      }
    });
  }

  return rows;
};

const renderRows = (list, rows) => {
  list.textContent = "";
  rows.forEach((row) => {
    const term = document.createElement("dt");
    term.textContent = row.label;
    const value = document.createElement("dd");
    value.textContent = String(row.value ?? "");
    list.append(term, value);
  });
};

export const closeWorkspacePropertiesModal = () => {
  const { modal, icon, name, subtitle, list, hint } = getElements();
  modal?.classList.remove("active");
  if (icon) {
    icon.className = "workspace-properties-icon";
    icon.textContent = "";
  }
  if (name) {
    name.textContent = "";
    name.removeAttribute("title");
  }
  if (subtitle) {
    subtitle.textContent = "";
  }
  if (list) {
    list.textContent = "";
  }
  if (hint) {
    hint.textContent = "";
    hint.style.display = "none";
  }
};

export const openWorkspacePropertiesModal = (entry, options = {}) => {
  if (!entry) {
    return;
  }
  const elements = getElements();
  if (!elements.modal || !elements.icon || !elements.name || !elements.subtitle || !elements.list) {
    return;
  }

  const icon = resolveIcon(entry, options);
  const title = String(entry.name || t("workspace.properties.unnamed"));
  const typeLabel = resolveTypeLabel(entry, options);

  elements.icon.className = `workspace-properties-icon ${icon.className || ""}`.trim();
  elements.icon.textContent = "";
  const iconNode = document.createElement("i");
  iconNode.className = `fa-solid ${icon.icon || "fa-file"}`;
  elements.icon.appendChild(iconNode);

  elements.name.textContent = title;
  elements.name.title = title;
  elements.subtitle.textContent = typeLabel;
  renderRows(elements.list, buildRows(entry, options));

  if (elements.hint) {
    const hintText =
      entry.type === "dir"
        ? Array.isArray(entry.children)
          ? t("workspace.properties.loadedScopeHint")
          : t("workspace.properties.unloadedScopeHint")
        : "";
    elements.hint.textContent = hintText;
    elements.hint.style.display = hintText ? "block" : "none";
  }

  elements.modal.classList.add("active");
};

export const initWorkspacePropertiesModal = () => {
  if (initialized) {
    return;
  }
  initialized = true;
  const elements = getElements();
  elements.close?.addEventListener("click", closeWorkspacePropertiesModal);
  elements.closeBtn?.addEventListener("click", closeWorkspacePropertiesModal);
  elements.modal?.addEventListener("click", (event) => {
    if (event.target === elements.modal) {
      closeWorkspacePropertiesModal();
    }
  });
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && elements.modal?.classList.contains("active")) {
      closeWorkspacePropertiesModal();
    }
  });
};
