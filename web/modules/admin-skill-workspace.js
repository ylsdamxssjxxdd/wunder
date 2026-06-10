import { getWunderBase } from "./api.js";
import { notify } from "./notify.js";
import { t } from "./i18n.js?v=20260610-01";
import { escapeHtml, formatBytes } from "./utils.js?v=20251229-02";
import { resolveApiErrorMessage } from "./api-error.js";
import {
  closeWorkspacePropertiesModal,
  initWorkspacePropertiesModal,
  openWorkspacePropertiesModal,
} from "./workspace-properties.js?v=20260610-01";

const TEXT_EXTENSIONS = new Set([
  "txt",
  "md",
  "log",
  "json",
  "yaml",
  "yml",
  "toml",
  "ini",
  "xml",
  "csv",
  "tsv",
  "py",
  "js",
  "ts",
  "css",
  "html",
  "htm",
  "sh",
  "bat",
  "ps1",
  "sql",
]);
const IMAGE_EXTENSIONS = new Set(["png", "jpg", "jpeg", "gif", "bmp", "webp", "svg"]);
const PDF_EXTENSIONS = new Set(["pdf"]);
const OFFICE_WORD_EXTENSIONS = new Set(["doc", "docx"]);
const OFFICE_EXCEL_EXTENSIONS = new Set(["xls", "xlsx"]);
const OFFICE_PPT_EXTENSIONS = new Set(["ppt", "pptx"]);
const ARCHIVE_EXTENSIONS = new Set(["zip", "rar", "7z", "tar", "gz", "bz2"]);
const AUDIO_EXTENSIONS = new Set(["mp3", "wav", "flac", "aac", "ogg", "m4a"]);
const VIDEO_EXTENSIONS = new Set(["mp4", "mov", "avi", "mkv", "webm"]);
const CODE_EXTENSIONS = new Set(["py", "js", "ts", "css", "html", "htm", "sh", "bat", "ps1", "sql"]);
const MAX_TEXT_PREVIEW_SIZE = 512 * 1024;
const SKILL_DRAG_KEY = "application/x-wunder-admin-skill-entry";
const SEARCH_DEBOUNCE_MS = 300;

const elements = {
  title: document.getElementById("skillDetailTitle"),
  meta: document.getElementById("skillDetailMeta"),
  path: document.getElementById("skillWorkspacePath"),
  list: document.getElementById("skillFileTree"),
  search: document.getElementById("skillFileSearchInput"),
  selectionMeta: document.getElementById("skillWorkspaceSelectionMeta"),
  upBtn: document.getElementById("skillWorkspaceUpBtn"),
  refreshBtn: document.getElementById("skillWorkspaceRefreshBtn"),
  uploadBtn: document.getElementById("skillWorkspaceUploadBtn"),
  downloadAllBtn: document.getElementById("skillWorkspaceDownloadAllBtn"),
  uploadInput: document.getElementById("skillWorkspaceUploadInput"),
  uploadProgress: document.getElementById("skillWorkspaceUploadProgress"),
  uploadProgressBar: document.getElementById("skillWorkspaceUploadProgressBar"),
  uploadProgressText: document.getElementById("skillWorkspaceUploadProgressText"),
  menu: document.getElementById("skillWorkspaceMenu"),
  newFileBtn: document.getElementById("skillWorkspaceNewFileBtn"),
  editBtn: document.getElementById("skillWorkspaceEditBtn"),
  renameBtn: document.getElementById("skillWorkspaceRenameBtn"),
  moveBtn: document.getElementById("skillWorkspaceMoveBtn"),
  copyBtn: document.getElementById("skillWorkspaceCopyBtn"),
  propertiesBtn: document.getElementById("skillWorkspacePropertiesBtn"),
  newFolderBtn: document.getElementById("skillWorkspaceNewFolderBtn"),
  downloadBtn: document.getElementById("skillWorkspaceDownloadBtn"),
  deleteBtn: document.getElementById("skillWorkspaceDeleteBtn"),
  editorModal: document.getElementById("skillEditorModal"),
  editorClose: document.getElementById("skillEditorModalClose"),
  editorCloseBtn: document.getElementById("skillEditorModalCloseBtn"),
  editorPath: document.getElementById("skillEditorPath"),
  editorSave: document.getElementById("skillFileSaveBtn"),
  editorContent: document.getElementById("skillFileContent"),
  editorBody: document.getElementById("skillEditorBody"),
  editorHighlight: document.getElementById("skillFileHighlight"),
};

const workspace = {
  skill: null,
  entries: [],
  path: "",
  selected: null,
  selectedPaths: new Set(),
  lastSelectedPath: "",
  expanded: new Set(),
  flatEntries: [],
  searchKeyword: "",
  searchMode: false,
  renamingPath: "",
  menuEntry: null,
  editorEntry: null,
  editorLoading: false,
  editorContent: "",
  fileVersion: 0,
};

let initialized = false;
let highlightTimer = 0;
let searchTimer = 0;
let isSkillEditable = () => true;
let onSkillMetadataChanged = async () => {};

const HIGHLIGHT_KEYWORDS = new Set([
  "await",
  "break",
  "case",
  "catch",
  "class",
  "const",
  "continue",
  "default",
  "do",
  "else",
  "enum",
  "export",
  "extends",
  "finally",
  "for",
  "fn",
  "function",
  "if",
  "impl",
  "import",
  "in",
  "interface",
  "let",
  "match",
  "new",
  "pub",
  "return",
  "self",
  "static",
  "struct",
  "switch",
  "throw",
  "try",
  "type",
  "use",
  "var",
  "while",
  "yield",
]);

const HIGHLIGHT_TOKEN_REGEX =
  /(\"(?:\\.|[^\"\\])*\"|\'(?:\\.|[^'\\])*\'|`(?:\\.|[^`\\])*`|\/\/.*?$|\/\*[\s\S]*?\*\/|\b\d+(?:\.\d+)?\b|\b[A-Za-z_][A-Za-z0-9_]*\b)/gm;

const activeSkillName = () => String(workspace.skill?.name || "").trim();

const isActiveSkillEditable = () => Boolean(workspace.skill && isSkillEditable(workspace.skill));

const normalizePath = (path) => String(path || "").replace(/\\/g, "/").replace(/^\/+/, "");

const joinPath = (basePath, name) =>
  normalizePath([basePath, name].filter(Boolean).join("/"));

const parentPath = (path) => {
  const parts = normalizePath(path).split("/").filter(Boolean);
  parts.pop();
  return parts.join("/");
};

const isValidName = (value) => {
  const trimmed = String(value || "").trim();
  return Boolean(trimmed && trimmed !== "." && trimmed !== ".." && !/[\\/]/.test(trimmed));
};

const isValidPath = (value) =>
  normalizePath(value).split("/").filter(Boolean).every(isValidName);

const extensionOf = (entry) => {
  const name = String(entry?.name || entry?.path || "");
  const base = name.split("/").pop().split("\\").pop();
  const dot = base.lastIndexOf(".");
  return dot >= 0 && dot < base.length - 1 ? base.slice(dot + 1).toLowerCase() : "";
};

const entryIcon = (entry) => {
  if (entry?.type === "dir") {
    return { icon: "fa-folder", className: "icon-folder" };
  }
  const ext = extensionOf(entry);
  if (IMAGE_EXTENSIONS.has(ext)) return { icon: "fa-file-image", className: "icon-image" };
  if (PDF_EXTENSIONS.has(ext)) return { icon: "fa-file-pdf", className: "icon-pdf" };
  if (OFFICE_WORD_EXTENSIONS.has(ext)) return { icon: "fa-file-word", className: "icon-word" };
  if (OFFICE_EXCEL_EXTENSIONS.has(ext)) return { icon: "fa-file-excel", className: "icon-excel" };
  if (OFFICE_PPT_EXTENSIONS.has(ext)) return { icon: "fa-file-powerpoint", className: "icon-ppt" };
  if (ARCHIVE_EXTENSIONS.has(ext)) return { icon: "fa-file-zipper", className: "icon-archive" };
  if (AUDIO_EXTENSIONS.has(ext)) return { icon: "fa-file-audio", className: "icon-audio" };
  if (VIDEO_EXTENSIONS.has(ext)) return { icon: "fa-file-video", className: "icon-video" };
  if (CODE_EXTENSIONS.has(ext)) return { icon: "fa-file-code", className: "icon-code" };
  if (TEXT_EXTENSIONS.has(ext)) return { icon: "fa-file-lines", className: "icon-text" };
  return { icon: "fa-file", className: "icon-file" };
};

const isTextEditable = (entry) =>
  entry?.type === "file" &&
  TEXT_EXTENSIONS.has(extensionOf(entry)) &&
  (Number(entry.size) || 0) <= MAX_TEXT_PREVIEW_SIZE;

const normalizeEntries = (entries) =>
  (Array.isArray(entries) ? entries : []).filter((entry) => entry && typeof entry === "object");

const findEntry = (entries, targetPath) => {
  for (const entry of normalizeEntries(entries)) {
    if (entry.path === targetPath) return entry;
    const child = findEntry(entry.children, targetPath);
    if (child) return child;
  }
  return null;
};

const attachChildren = (targetPath, children) => {
  const target = findEntry(workspace.entries, targetPath);
  if (!target || target.type !== "dir") return false;
  target.children = normalizeEntries(children);
  target.childrenLoaded = true;
  return true;
};

const flattenEntries = (entries, result = []) => {
  normalizeEntries(entries).forEach((entry) => {
    result.push(entry);
    if (entry.type === "dir" && workspace.expanded.has(entry.path)) {
      flattenEntries(entry.children, result);
    }
  });
  return result;
};

const setUploadProgress = (active, text = "") => {
  elements.uploadProgress?.classList.toggle("active", active);
  elements.uploadProgress?.classList.toggle("indeterminate", active);
  if (elements.uploadProgressBar) {
    elements.uploadProgressBar.style.width = active ? "30%" : "0%";
  }
  if (elements.uploadProgressText) {
    elements.uploadProgressText.textContent = text;
  }
};

const apiError = async (response, fallback) => {
  const detail = await resolveApiErrorMessage(response, "");
  return detail || fallback || t("common.requestFailed", { status: response.status });
};

const skillParams = (extra = {}) => {
  const name = activeSkillName();
  const params = new URLSearchParams({ name });
  Object.entries(extra).forEach(([key, value]) => {
    if (value !== undefined && value !== null && value !== "") {
      params.set(key, String(value));
    }
  });
  return params;
};

const fetchFsContent = async (path, options = {}) => {
  const endpoint = `${getWunderBase()}/admin/skills/fs?${skillParams({
    path: normalizePath(path),
    include_content: options.includeContent ? "true" : "false",
    max_bytes: options.maxBytes || "",
    depth: options.depth || "",
    sort_by: "name",
    order: "asc",
  }).toString()}`;
  const response = await fetch(endpoint);
  if (!response.ok) {
    throw new Error(await apiError(response, t("common.loadFailed")));
  }
  return response.json();
};

const searchFs = async (keyword) => {
  const endpoint = `${getWunderBase()}/admin/skills/fs/search?${skillParams({
    keyword,
    limit: 200,
  }).toString()}`;
  const response = await fetch(endpoint);
  if (!response.ok) {
    throw new Error(await apiError(response, t("workspace.searchFailed")));
  }
  return response.json();
};

const jsonRequest = async (path, payload, method = "POST") => {
  const response = await fetch(`${getWunderBase()}${path}`, {
    method,
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ name: activeSkillName(), ...payload }),
  });
  if (!response.ok) {
    throw new Error(await apiError(response, t("common.requestFailed", { status: response.status })));
  }
  return response.json();
};

const saveFsFile = (path, content, createIfMissing = false) =>
  jsonRequest(
    "/admin/skills/fs/file",
    { path: normalizePath(path), content, create_if_missing: createIfMissing },
    "PUT"
  );

const createFsDir = (path) => jsonRequest("/admin/skills/dir", { path: normalizePath(path) });

const moveEntry = (source, destination) =>
  jsonRequest("/admin/skills/move", {
    source: normalizePath(source),
    destination: normalizePath(destination),
  });

const copyEntry = (source, destination) =>
  jsonRequest("/admin/skills/copy", {
    source: normalizePath(source),
    destination: normalizePath(destination),
  });

const batchAction = (action, paths, destination = "") =>
  jsonRequest("/admin/skills/batch", {
    action,
    paths: paths.map(normalizePath),
    destination: normalizePath(destination),
  });

const fetchBlob = async (endpoint, filename) => {
  const response = await fetch(endpoint);
  if (!response.ok) {
    throw new Error(await apiError(response, t("workspace.downloadFailed", { message: response.status })));
  }
  const blob = await response.blob();
  const objectUrl = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = objectUrl;
  link.download = filename || "download";
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(objectUrl);
};

const downloadEntry = async (entry) => {
  if (!entry) return;
  const params = skillParams({ path: entry.path });
  if (entry.type === "dir") {
    await fetchBlob(
      `${getWunderBase()}/admin/skills/archive?${params.toString()}`,
      `${entry.name || "skill-folder"}.zip`
    );
    return;
  }
  await fetchBlob(
    `${getWunderBase()}/admin/skills/download?${params.toString()}`,
    entry.name || "download"
  );
};

const downloadArchive = async () => {
  const name = activeSkillName();
  if (!name) return;
  await fetchBlob(
    `${getWunderBase()}/admin/skills/archive?${skillParams().toString()}`,
    `${name}.zip`
  );
};

const updateHeader = () => {
  const skill = workspace.skill;
  if (elements.title) {
    elements.title.textContent = skill?.name || t("skills.detail.unselected");
  }
  if (elements.meta) {
    const parts = [];
    if (skill && !isActiveSkillEditable()) parts.push(t("skills.readonly.hint"));
    if (skill?.path) parts.push(String(skill.path).replace(/\\/g, "/"));
    elements.meta.textContent = parts.join(" · ");
    elements.meta.title = parts.join(" · ");
  }
  if (elements.path) {
    elements.path.textContent = workspace.path ? `/${workspace.path}` : "/";
  }
  if (elements.upBtn) elements.upBtn.disabled = !workspace.path;
  const hasSkill = Boolean(skill);
  if (elements.refreshBtn) elements.refreshBtn.disabled = !hasSkill;
  if (elements.uploadBtn) elements.uploadBtn.disabled = !hasSkill || !isActiveSkillEditable();
  if (elements.downloadAllBtn) elements.downloadAllBtn.disabled = !hasSkill;
};

export const refreshAdminSkillWorkspaceHeader = updateHeader;

const updateSelectionMeta = () => {
  if (!elements.selectionMeta) return;
  const count = workspace.selectedPaths.size;
  elements.selectionMeta.textContent = count > 0 ? t("workspace.selection.count", { count }) : "";
};

const setSelection = (paths, primaryPath) => {
  workspace.selectedPaths = new Set(paths.filter(Boolean));
  workspace.selected =
    primaryPath && workspace.selectedPaths.has(primaryPath)
      ? findEntry(workspace.entries, primaryPath)
      : null;
  workspace.lastSelectedPath = primaryPath || workspace.lastSelectedPath;
  updateSelectionMeta();
};

const resetSelection = () => {
  workspace.selected = null;
  workspace.selectedPaths = new Set();
  workspace.lastSelectedPath = "";
  updateSelectionMeta();
};

const selectionPaths = () => Array.from(workspace.selectedPaths);

const renderList = () => {
  if (!elements.list) return;
  const entries = normalizeEntries(workspace.entries);
  elements.list.textContent = "";
  if (!workspace.skill) {
    elements.list.textContent = t("skills.files.unselected");
    workspace.flatEntries = [];
    return;
  }
  if (!entries.length) {
    elements.list.textContent = workspace.searchMode ? t("workspace.empty.search") : t("skills.files.empty");
    workspace.flatEntries = [];
    return;
  }

  const flat = [];
  const treeView = !workspace.searchMode;
  const renderEntry = (entry, depth) => {
    flat.push(entry);
    const item = document.createElement("div");
    item.className = "workspace-item";
    if (entry.type === "dir") item.classList.add("is-folder");
    if (workspace.selectedPaths.has(entry.path)) item.classList.add("is-selected");
    if (workspace.selected?.path === entry.path) item.classList.add("active");
    item.style.setProperty("--workspace-indent", `${depth * 16}px`);
    item.dataset.path = entry.path || "";

    const main = document.createElement("div");
    main.className = "workspace-item-main";
    const caret = document.createElement("i");
    caret.className = "fa-solid fa-chevron-right workspace-item-caret";
    if (!treeView || entry.type !== "dir") {
      caret.classList.add("hidden");
    } else if (workspace.expanded.has(entry.path)) {
      caret.classList.add("expanded");
    }
    caret.addEventListener("click", (event) => {
      event.stopPropagation();
      toggleDirectory(entry);
    });
    const icon = entryIcon(entry);
    const iconNode = document.createElement("i");
    iconNode.className = `fa-solid ${icon.icon} workspace-item-icon ${icon.className}`;
    const name = document.createElement("div");
    name.className = "workspace-item-name";
    if (workspace.renamingPath === entry.path) {
      const input = document.createElement("input");
      input.className = "workspace-item-rename";
      input.value = entry.name || "";
      input.addEventListener("click", (event) => event.stopPropagation());
      input.addEventListener("keydown", (event) => {
        if (event.key === "Enter") {
          event.preventDefault();
          finishRename(entry, input.value);
        }
        if (event.key === "Escape") {
          workspace.renamingPath = "";
          renderList();
        }
      });
      input.addEventListener("blur", () => finishRename(entry, input.value));
      name.appendChild(input);
      requestAnimationFrame(() => {
        input.focus();
        input.select();
      });
    } else {
      name.textContent = entry.name || entry.path || "";
    }
    const meta = document.createElement("div");
    meta.className = "workspace-item-meta";
    const metaParts = [entry.type === "dir" ? t("workspace.entry.folder") : formatBytes(entry.size || 0)];
    if (workspace.searchMode && entry.path) metaParts.push(entry.path);
    meta.textContent = metaParts.join(" · ");

    main.append(caret, iconNode, name);
    item.append(main, meta);
    item.addEventListener("click", (event) => handleItemClick(event, entry));
    item.addEventListener("dblclick", () => handleItemDoubleClick(entry));
    item.addEventListener("contextmenu", (event) => {
      event.preventDefault();
      event.stopPropagation();
      openMenu(event, entry);
    });
    item.draggable = true;
    item.addEventListener("dragstart", (event) => handleDragStart(event, entry));
    item.addEventListener("dragend", () => item.classList.remove("dragging"));
    if (entry.type === "dir") {
      item.addEventListener("dragover", (event) => {
        event.preventDefault();
        item.classList.add("drop-target");
      });
      item.addEventListener("dragleave", () => item.classList.remove("drop-target"));
      item.addEventListener("drop", (event) => handleDrop(event, entry));
    }
    elements.list.appendChild(item);

    if (treeView && entry.type === "dir" && workspace.expanded.has(entry.path)) {
      normalizeEntries(entry.children).forEach((child) => renderEntry(child, depth + 1));
    }
  };
  entries.forEach((entry) => renderEntry(entry, 0));
  workspace.flatEntries = flat;
};

const hydrateExpanded = async () => {
  for (const path of Array.from(workspace.expanded)) {
    const entry = findEntry(workspace.entries, path);
    if (!entry || entry.type !== "dir" || entry.childrenLoaded) continue;
    try {
      const result = await fetchFsContent(path, { includeContent: true, depth: 1 });
      attachChildren(path, result.entries);
    } catch {
      workspace.expanded.delete(path);
    }
  }
};

export const resetAdminSkillWorkspace = () => {
  workspace.skill = null;
  workspace.entries = [];
  workspace.path = "";
  workspace.expanded = new Set();
  workspace.searchKeyword = "";
  workspace.searchMode = false;
  workspace.renamingPath = "";
  workspace.menuEntry = null;
  resetSelection();
  closeEditor();
  closeMenu();
  closeWorkspacePropertiesModal();
  if (elements.search) elements.search.value = "";
  updateHeader();
  renderList();
};

export const loadAdminSkillWorkspace = async (skill, options = {}) => {
  if (!skill) {
    resetAdminSkillWorkspace();
    return;
  }
  const sameSkill = workspace.skill?.name === skill.name;
  workspace.skill = skill;
  if (!sameSkill || !options.preservePath) {
    workspace.path = "";
    workspace.expanded = new Set();
  }
  if (options.resetSearch) {
    workspace.searchKeyword = "";
    workspace.searchMode = false;
    if (elements.search) elements.search.value = "";
  }
  workspace.renamingPath = "";
  resetSelection();
  updateHeader();
  if (elements.list) elements.list.textContent = t("common.loading");
  try {
    const result = await fetchFsContent(workspace.path, { includeContent: true, depth: 1 });
    workspace.path = normalizePath(result.path || workspace.path);
    workspace.entries = normalizeEntries(result.entries);
    await hydrateExpanded();
    updateHeader();
    renderList();
  } catch (error) {
    if (elements.list) {
      elements.list.textContent = t("skills.files.loadFailed", { message: error.message });
    }
  }
};

const reloadView = async () => {
  if (workspace.searchMode && workspace.searchKeyword) {
    await loadSearch();
    return;
  }
  await loadAdminSkillWorkspace(workspace.skill, { preservePath: true });
};

const loadSearch = async () => {
  const keyword = String(workspace.searchKeyword || "").trim();
  if (!keyword) {
    workspace.searchMode = false;
    await loadAdminSkillWorkspace(workspace.skill, { preservePath: true, resetSearch: true });
    return;
  }
  resetSelection();
  try {
    const result = await searchFs(keyword);
    workspace.entries = normalizeEntries(result.entries);
    workspace.searchMode = true;
    renderList();
  } catch (error) {
    if (elements.list) {
      elements.list.textContent = t("workspace.searchFailedWithMessage", { message: error.message });
    }
  }
};

const toggleDirectory = async (entry) => {
  if (!entry || entry.type !== "dir") return;
  if (workspace.expanded.has(entry.path)) {
    workspace.expanded.delete(entry.path);
    renderList();
    return;
  }
  workspace.expanded.add(entry.path);
  if (!entry.childrenLoaded) {
    try {
      const result = await fetchFsContent(entry.path, { includeContent: true, depth: 1 });
      attachChildren(entry.path, result.entries);
    } catch (error) {
      workspace.expanded.delete(entry.path);
      notify(t("workspace.folder.loadFailed", { message: error.message }), "error");
    }
  }
  renderList();
};

const handleItemClick = (event, entry) => {
  if (!entry || workspace.renamingPath) return;
  const path = entry.path;
  const range = event.shiftKey && workspace.lastSelectedPath;
  const toggle = event.ctrlKey || event.metaKey;
  if (range) {
    const flat = workspace.flatEntries || [];
    const start = flat.findIndex((item) => item.path === workspace.lastSelectedPath);
    const end = flat.findIndex((item) => item.path === path);
    if (start !== -1 && end !== -1) {
      const [from, to] = start < end ? [start, end] : [end, start];
      setSelection(flat.slice(from, to + 1).map((item) => item.path), path);
      renderList();
      return;
    }
  }
  if (toggle) {
    if (workspace.selectedPaths.has(path)) {
      workspace.selectedPaths.delete(path);
      if (workspace.selected?.path === path) workspace.selected = null;
    } else {
      workspace.selectedPaths.add(path);
      workspace.selected = entry;
      workspace.lastSelectedPath = path;
    }
    updateSelectionMeta();
    renderList();
    return;
  }
  setSelection([path], path);
  renderList();
};

const handleItemDoubleClick = (entry) => {
  if (!entry || workspace.renamingPath) return;
  if (entry.type === "dir") {
    workspace.path = entry.path;
    workspace.expanded = new Set();
    loadAdminSkillWorkspace(workspace.skill, { preservePath: true, resetSearch: true });
    return;
  }
  openEditor(entry);
};

const finishRename = async (entry, nextName) => {
  if (!entry || workspace.renamingPath !== entry.path) return;
  workspace.renamingPath = "";
  const trimmed = String(nextName || "").trim();
  if (!isValidName(trimmed)) {
    notify(t("workspace.name.invalid"), "warn");
    renderList();
    return;
  }
  if (trimmed === entry.name) {
    renderList();
    return;
  }
  try {
    await moveEntry(entry.path, joinPath(parentPath(entry.path), trimmed));
    notify(t("workspace.rename.success"), "success");
    await reloadView();
    await maybeRefreshSkillList(entry.path);
  } catch (error) {
    notify(t("workspace.rename.failed", { message: error.message }), "error");
    renderList();
  }
};

const openMenu = (event, entry = null) => {
  if (!elements.menu) return;
  if (entry?.path && !workspace.selectedPaths.has(entry.path)) {
    setSelection([entry.path], entry.path);
  }
  if (entry?.path) workspace.selected = entry;
  renderList();
  const paths = selectionPaths();
  const single = paths.length === 1 ? findEntry(workspace.entries, paths[0]) || entry : null;
  workspace.menuEntry = single || entry || null;
  const editable = isActiveSkillEditable();
  elements.downloadBtn.disabled = !single;
  elements.editBtn.disabled = !single || !isTextEditable(single);
  elements.renameBtn.disabled = !editable || !single;
  elements.moveBtn.disabled = !editable || !paths.length;
  elements.copyBtn.disabled = !editable || !paths.length;
  elements.deleteBtn.disabled = !editable || !paths.length;
  if (elements.propertiesBtn) {
    elements.propertiesBtn.disabled = !workspace.menuEntry;
  }
  elements.newFileBtn.disabled = !editable || !workspace.skill;
  elements.newFolderBtn.disabled = !editable || !workspace.skill;
  elements.menu.style.display = "flex";
  const rect = elements.menu.getBoundingClientRect();
  elements.menu.style.left = `${Math.max(8, Math.min(event.clientX, window.innerWidth - rect.width - 8))}px`;
  elements.menu.style.top = `${Math.max(8, Math.min(event.clientY, window.innerHeight - rect.height - 8))}px`;
};

const closeMenu = () => {
  if (elements.menu) elements.menu.style.display = "none";
};

const openProperties = (entry) => {
  if (!entry) return;
  openWorkspacePropertiesModal(entry, {
    resolveIcon: entryIcon,
  });
};

const createFile = async () => {
  const rawName = window.prompt(t("workspace.file.prompt"), "untitled.txt");
  if (rawName === null) return;
  const name = rawName.trim();
  if (!isValidName(name)) {
    notify(t("workspace.name.invalid"), "warn");
    return;
  }
  const path = joinPath(workspace.path, name);
  try {
    await saveFsFile(path, "", true);
    notify(t("workspace.file.created", { name }), "success");
    await reloadView();
  } catch (error) {
    notify(t("workspace.file.createFailed", { message: error.message }), "error");
  }
};

const createFolder = async () => {
  const rawName = window.prompt(t("workspace.folder.prompt"));
  if (rawName === null) return;
  const name = rawName.trim();
  if (!isValidName(name)) {
    notify(t("workspace.name.invalid"), "warn");
    return;
  }
  try {
    await createFsDir(joinPath(workspace.path, name));
    notify(t("workspace.folder.created", { name }), "success");
    await reloadView();
  } catch (error) {
    notify(t("workspace.folder.createFailed", { message: error.message }), "error");
  }
};

const deleteSelection = async () => {
  const paths = selectionPaths();
  if (!paths.length) {
    notify(t("workspace.selection.empty"), "info");
    return;
  }
  const confirmed = window.confirm(
    paths.length === 1
      ? t("workspace.delete.confirm.single")
      : t("workspace.delete.confirm.multi", { count: paths.length })
  );
  if (!confirmed) return;
  try {
    const result = await batchAction("delete", paths);
    notifyBatchResult(result, t("workspace.action.delete"));
    await reloadView();
    await maybeRefreshSkillList(paths.join("/"));
  } catch (error) {
    notify(error.message || t("workspace.delete.failed"), "error");
  }
};

const moveSelectionToDirectory = async () => {
  const paths = selectionPaths();
  if (!paths.length) {
    notify(t("workspace.selection.empty"), "info");
    return;
  }
  const rawTarget = window.prompt(t("workspace.move.prompt"), "");
  if (rawTarget === null) return;
  const target = normalizePath(rawTarget.trim());
  if (!isValidPath(target)) {
    notify(t("workspace.path.invalid"), "warn");
    return;
  }
  try {
    const result = await batchAction("move", paths, target);
    notifyBatchResult(result, t("workspace.action.move"));
    await reloadView();
    await maybeRefreshSkillList(paths.join("/"));
  } catch (error) {
    notify(error.message || t("workspace.move.failed"), "error");
  }
};

const copySelectionToDirectory = async () => {
  const paths = selectionPaths();
  if (!paths.length) {
    notify(t("workspace.selection.empty"), "info");
    return;
  }
  const rawTarget = window.prompt(t("workspace.move.prompt"), "");
  if (rawTarget === null) return;
  const target = normalizePath(rawTarget.trim());
  if (!isValidPath(target)) {
    notify(t("workspace.path.invalid"), "warn");
    return;
  }
  try {
    const result = await batchAction("copy", paths, target);
    notifyBatchResult(result, t("workspace.action.copy"));
    await reloadView();
  } catch (error) {
    notify(error.message || t("workspace.copy.failed"), "error");
  }
};

const notifyBatchResult = (result, actionLabel) => {
  const failed = result?.failed?.length || 0;
  const success = result?.succeeded?.length || 0;
  notify(
    failed
      ? t("workspace.batch.partialFailed", { action: actionLabel, success, failed })
      : t("workspace.batch.success", { action: actionLabel, success }),
    failed ? "warn" : "success"
  );
};

const maybeRefreshSkillList = async (path) => {
  if (String(path || "").split("/").some((part) => part.toLowerCase() === "skill.md")) {
    await onSkillMetadataChanged();
  }
};

const openEditor = async (entry) => {
  if (!entry || entry.type !== "file") return;
  if (!isTextEditable(entry)) {
    notify(t("workspace.editor.unsupported"), "warn");
    downloadEntry(entry).catch((error) => notify(error.message, "error"));
    return;
  }
  workspace.editorEntry = entry;
  workspace.editorLoading = true;
  workspace.fileVersion += 1;
  const version = workspace.fileVersion;
  if (elements.editorPath) elements.editorPath.textContent = entry.path || "";
  if (elements.editorContent) elements.editorContent.value = t("common.loading");
  setEditorDisabled(true);
  elements.editorModal?.classList.add("active");
  scheduleHighlight();
  try {
    const result = await fetchFsContent(entry.path, {
      includeContent: true,
      maxBytes: MAX_TEXT_PREVIEW_SIZE,
    });
    if (result?.truncated) throw new Error(t("workspace.editor.tooLarge"));
    if (version !== workspace.fileVersion) return;
    workspace.editorContent = String(result?.content || "");
    if (elements.editorContent) elements.editorContent.value = workspace.editorContent;
    setEditorDisabled(!isActiveSkillEditable());
    scheduleHighlight();
  } catch (error) {
    if (version === workspace.fileVersion) {
      notify(error.message || t("workspace.editor.loadFailed"), "error");
      closeEditor();
    }
  } finally {
    workspace.editorLoading = false;
  }
};

const closeEditor = () => {
  workspace.editorEntry = null;
  workspace.editorLoading = false;
  workspace.editorContent = "";
  if (elements.editorContent) elements.editorContent.value = "";
  elements.editorModal?.classList.remove("active");
  scheduleHighlight();
};

const saveEditor = async () => {
  if (!workspace.editorEntry || workspace.editorLoading || !isActiveSkillEditable()) return;
  const content = elements.editorContent?.value || "";
  try {
    await saveFsFile(workspace.editorEntry.path, content, false);
    workspace.editorContent = content;
    notify(t("workspace.editor.saveSuccess", { name: workspace.editorEntry.name || "" }), "success");
    await reloadView();
    await maybeRefreshSkillList(workspace.editorEntry.path);
    closeEditor();
  } catch (error) {
    notify(t("workspace.editor.saveFailed", { message: error.message }), "error");
  }
};

const setEditorDisabled = (disabled) => {
  elements.editorBody?.classList.toggle("is-disabled", disabled);
  if (elements.editorContent) elements.editorContent.disabled = disabled;
  if (elements.editorSave) elements.editorSave.disabled = disabled;
};

const highlightCode = (text) => {
  const raw = String(text ?? "");
  if (!raw) return "&nbsp;";
  let result = "";
  let lastIndex = 0;
  for (const match of raw.matchAll(HIGHLIGHT_TOKEN_REGEX)) {
    const token = match[0];
    const index = match.index ?? 0;
    if (index > lastIndex) result += escapeHtml(raw.slice(lastIndex, index));
    let className = "";
    if (token.startsWith("//") || token.startsWith("/*")) className = "code-token-comment";
    else if (token.startsWith("\"") || token.startsWith("'") || token.startsWith("`")) className = "code-token-string";
    else if (/^\d/.test(token)) className = "code-token-number";
    else if (HIGHLIGHT_KEYWORDS.has(token)) className = "code-token-keyword";
    result += className ? `<span class="${className}">${escapeHtml(token)}</span>` : escapeHtml(token);
    lastIndex = index + token.length;
  }
  if (lastIndex < raw.length) result += escapeHtml(raw.slice(lastIndex));
  return result || "&nbsp;";
};

const updateHighlight = () => {
  if (!elements.editorHighlight || !elements.editorContent) return;
  elements.editorHighlight.innerHTML = highlightCode(elements.editorContent.value);
  elements.editorHighlight.scrollTop = elements.editorContent.scrollTop;
  elements.editorHighlight.scrollLeft = elements.editorContent.scrollLeft;
};

const scheduleHighlight = () => {
  if (!elements.editorHighlight) return;
  if (highlightTimer) cancelAnimationFrame(highlightTimer);
  highlightTimer = requestAnimationFrame(() => {
    highlightTimer = 0;
    updateHighlight();
  });
};

const handleDragStart = (event, entry) => {
  if (!event.dataTransfer || !entry?.path) return;
  if (!workspace.selectedPaths.has(entry.path)) setSelection([entry.path], entry.path);
  event.dataTransfer.setData(SKILL_DRAG_KEY, JSON.stringify(selectionPaths()));
  event.dataTransfer.setData("text/plain", entry.path);
  event.dataTransfer.effectAllowed = "move";
  event.currentTarget?.classList?.add("dragging");
};

const dragPaths = (dataTransfer) => {
  const raw = dataTransfer?.getData(SKILL_DRAG_KEY) || "";
  if (!raw) return [];
  try {
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed.filter(Boolean) : [];
  } catch {
    return [raw].filter(Boolean);
  }
};

const handleDrop = async (event, targetEntry = null) => {
  event.preventDefault();
  event.stopPropagation();
  event.currentTarget?.classList?.remove("drop-target", "dragover");
  const targetPath = targetEntry?.type === "dir" ? targetEntry.path : workspace.path;
  const internalPaths = dragPaths(event.dataTransfer);
  if (internalPaths.length) {
    try {
      const result = await batchAction("move", internalPaths, targetPath);
      notifyBatchResult(result, t("workspace.action.move"));
      await reloadView();
    } catch (error) {
      notify(error.message || t("workspace.move.failed"), "error");
    }
    return;
  }
  const files = Array.from(event.dataTransfer?.files || []);
  if (files.length) {
    await uploadFiles(files, targetPath);
  }
};

const uploadFiles = async (files, targetPath = workspace.path) => {
  if (!files?.length || !isActiveSkillEditable()) return;
  const form = new FormData();
  form.append("name", activeSkillName());
  form.append("path", normalizePath(targetPath));
  files.forEach((file) => {
    form.append("files", file, file.name);
    form.append("relative_paths", file.webkitRelativePath || file.name);
  });
  setUploadProgress(true, t("common.upload"));
  try {
    const response = await fetch(`${getWunderBase()}/admin/skills/fs/upload`, {
      method: "POST",
      body: form,
    });
    if (!response.ok) {
      throw new Error(await apiError(response, t("workspace.uploadFailed")));
    }
    notify(t("workspace.upload.success"), "success");
    await reloadView();
  } catch (error) {
    notify(t("workspace.upload.failed", { message: error.message }), "error");
  } finally {
    setUploadProgress(false);
  }
};

export const initAdminSkillWorkspace = (options = {}) => {
  if (initialized || !elements.list) return;
  initialized = true;
  initWorkspacePropertiesModal();
  isSkillEditable = options.isSkillEditable || isSkillEditable;
  onSkillMetadataChanged = options.onSkillMetadataChanged || onSkillMetadataChanged;

  elements.refreshBtn?.addEventListener("click", () => reloadView());
  elements.upBtn?.addEventListener("click", () => {
    if (!workspace.path) return;
    workspace.path = parentPath(workspace.path);
    workspace.expanded = new Set();
    loadAdminSkillWorkspace(workspace.skill, { preservePath: true, resetSearch: true });
  });
  elements.uploadBtn?.addEventListener("click", () => {
    if (!elements.uploadInput) return;
    elements.uploadInput.value = "";
    elements.uploadInput.click();
  });
  elements.uploadInput?.addEventListener("change", async () => {
    await uploadFiles(Array.from(elements.uploadInput.files || []), workspace.path);
  });
  elements.downloadAllBtn?.addEventListener("click", () => {
    downloadArchive()
      .then(() => notify(t("workspace.download.started"), "info"))
      .catch((error) => notify(t("workspace.downloadFailed", { message: error.message }), "error"));
  });
  elements.search?.addEventListener("input", () => {
    workspace.searchKeyword = elements.search.value.trim();
    if (searchTimer) clearTimeout(searchTimer);
    searchTimer = setTimeout(() => loadSearch(), SEARCH_DEBOUNCE_MS);
  });
  elements.search?.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      elements.search.value = "";
      workspace.searchKeyword = "";
      loadAdminSkillWorkspace(workspace.skill, { preservePath: true, resetSearch: true });
    }
  });
  elements.list.addEventListener("contextmenu", (event) => {
    if (event.target.closest(".workspace-item")) return;
    event.preventDefault();
    openMenu(event, null);
  });
  elements.list.addEventListener("dragover", (event) => {
    event.preventDefault();
    elements.list.classList.add("dragover");
  });
  elements.list.addEventListener("dragleave", (event) => {
    if (!event.currentTarget.contains(event.relatedTarget)) {
      elements.list.classList.remove("dragover");
    }
  });
  elements.list.addEventListener("drop", (event) => handleDrop(event, null));
  elements.newFileBtn?.addEventListener("click", () => {
    closeMenu();
    createFile();
  });
  elements.newFolderBtn?.addEventListener("click", () => {
    closeMenu();
    createFolder();
  });
  elements.editBtn?.addEventListener("click", () => {
    closeMenu();
    const paths = selectionPaths();
    openEditor(paths.length === 1 ? findEntry(workspace.entries, paths[0]) : null);
  });
  elements.renameBtn?.addEventListener("click", () => {
    closeMenu();
    const paths = selectionPaths();
    if (paths.length !== 1) return;
    workspace.renamingPath = paths[0];
    renderList();
  });
  elements.moveBtn?.addEventListener("click", () => {
    closeMenu();
    moveSelectionToDirectory();
  });
  elements.copyBtn?.addEventListener("click", () => {
    closeMenu();
    copySelectionToDirectory();
  });
  elements.propertiesBtn?.addEventListener("click", () => {
    const entry = workspace.menuEntry || workspace.selected;
    closeMenu();
    openProperties(entry);
  });
  elements.downloadBtn?.addEventListener("click", () => {
    closeMenu();
    const paths = selectionPaths();
    const entry = paths.length === 1 ? findEntry(workspace.entries, paths[0]) : workspace.selected;
    downloadEntry(entry).catch((error) =>
      notify(t("workspace.downloadFailed", { message: error.message }), "error")
    );
  });
  elements.deleteBtn?.addEventListener("click", () => {
    closeMenu();
    deleteSelection();
  });
  elements.editorSave?.addEventListener("click", saveEditor);
  elements.editorClose?.addEventListener("click", closeEditor);
  elements.editorCloseBtn?.addEventListener("click", closeEditor);
  elements.editorModal?.addEventListener("click", (event) => {
    if (event.target === elements.editorModal) closeEditor();
  });
  elements.editorContent?.addEventListener("input", scheduleHighlight);
  elements.editorContent?.addEventListener("scroll", updateHighlight);
  document.addEventListener("click", (event) => {
    if (!elements.menu?.contains(event.target)) closeMenu();
  });
  document.addEventListener("scroll", closeMenu, true);
  window.addEventListener("resize", closeMenu);
  resetAdminSkillWorkspace();
};
