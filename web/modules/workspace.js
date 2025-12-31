import { elements } from "./elements.js?v=20251231-03";
import { state } from "./state.js";
import { appendLog } from "./log.js?v=20251229-02";
import { formatBytes } from "./utils.js?v=20251229-02";
import { getWunderBase } from "./api.js";
import { notify } from "./notify.js";

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
const OFFICE_EXTENSIONS = new Set(["doc", "docx", "xls", "xlsx", "ppt", "pptx"]);
const OFFICE_WORD_EXTENSIONS = new Set(["doc", "docx"]);
const OFFICE_EXCEL_EXTENSIONS = new Set(["xls", "xlsx"]);
const OFFICE_PPT_EXTENSIONS = new Set(["ppt", "pptx"]);
const CODE_EXTENSIONS = new Set(["py", "js", "ts", "css", "html", "htm", "sh", "bat", "ps1", "sql"]);
const ARCHIVE_EXTENSIONS = new Set(["zip", "rar", "7z", "tar", "gz", "bz2"]);
const AUDIO_EXTENSIONS = new Set(["mp3", "wav", "flac", "aac", "ogg", "m4a"]);
const VIDEO_EXTENSIONS = new Set(["mp4", "mov", "avi", "mkv", "webm"]);
const MAX_TEXT_PREVIEW_SIZE = 512 * 1024;
const WORKSPACE_DRAG_KEY = "application/x-wunder-workspace-entry";
const WORKSPACE_SORT_ICONS = {
  asc: "fa-arrow-up-short-wide",
  desc: "fa-arrow-down-wide-short",
};
const WORKSPACE_SEARCH_DEBOUNCE_MS = 300;
let previewObjectUrl = null;
let editorEntry = null;
let editorLoading = false;

// 统一解析文件后缀，供图标与预览判断使用
const getWorkspaceExtension = (entry) => {
  const rawName = String(entry?.name || entry?.path || "");
  const baseName = rawName.split("/").pop().split("\\").pop();
  const dotIndex = baseName.lastIndexOf(".");
  if (dotIndex === -1 || dotIndex === baseName.length - 1) {
    return "";
  }
  return baseName.slice(dotIndex + 1).toLowerCase();
};

// 根据文件类型选择图标与配色
const getWorkspaceEntryIcon = (entry) => {
  if (entry.type === "dir") {
    return { icon: "fa-folder", className: "icon-folder" };
  }
  const ext = getWorkspaceExtension(entry);
  if (IMAGE_EXTENSIONS.has(ext)) {
    return { icon: "fa-file-image", className: "icon-image" };
  }
  if (PDF_EXTENSIONS.has(ext)) {
    return { icon: "fa-file-pdf", className: "icon-pdf" };
  }
  if (OFFICE_WORD_EXTENSIONS.has(ext)) {
    return { icon: "fa-file-word", className: "icon-word" };
  }
  if (OFFICE_EXCEL_EXTENSIONS.has(ext)) {
    return { icon: "fa-file-excel", className: "icon-excel" };
  }
  if (OFFICE_PPT_EXTENSIONS.has(ext)) {
    return { icon: "fa-file-powerpoint", className: "icon-ppt" };
  }
  if (ARCHIVE_EXTENSIONS.has(ext)) {
    return { icon: "fa-file-zipper", className: "icon-archive" };
  }
  if (AUDIO_EXTENSIONS.has(ext)) {
    return { icon: "fa-file-audio", className: "icon-audio" };
  }
  if (VIDEO_EXTENSIONS.has(ext)) {
    return { icon: "fa-file-video", className: "icon-video" };
  }
  if (CODE_EXTENSIONS.has(ext)) {
    return { icon: "fa-file-code", className: "icon-code" };
  }
  if (TEXT_EXTENSIONS.has(ext)) {
    return { icon: "fa-file-lines", className: "icon-text" };
  }
  return { icon: "fa-file", className: "icon-file" };
};

// 规范化工作区路径，统一分隔符并移除前导斜杠
const normalizeWorkspacePath = (path) => {
  if (!path) {
    return "";
  }
  return String(path).replace(/\\/g, "/").replace(/^\/+/, "");
};

// 判断文件是否可编辑（文本类型且大小可控）
const isWorkspaceTextEditable = (entry) => {
  if (!entry || entry.type !== "file") {
    return false;
  }
  const extension = getWorkspaceExtension(entry);
  if (!TEXT_EXTENSIONS.has(extension)) {
    return false;
  }
  const sizeValue = Number.isFinite(entry.size) ? entry.size : 0;
  return sizeValue <= MAX_TEXT_PREVIEW_SIZE;
};

// 规范化拼接工作区路径，避免出现重复斜杠
const joinWorkspacePath = (basePath, name) =>
  normalizeWorkspacePath([basePath, name].filter(Boolean).join("/"));

// 提取路径的父级目录
const getWorkspaceParentPath = (path) => {
  const normalized = normalizeWorkspacePath(path);
  if (!normalized) {
    return "";
  }
  const parts = normalized.split("/").filter(Boolean);
  parts.pop();
  return parts.join("/");
};

// 校验名称合法性，禁止包含路径分隔符
const isValidWorkspaceName = (value) => {
  const trimmed = String(value || "").trim();
  if (!trimmed) {
    return false;
  }
  if (trimmed === "." || trimmed === "..") {
    return false;
  }
  return !/[\\/]/.test(trimmed);
};

// 校验路径片段是否全部合法
const isValidWorkspacePath = (value) => {
  const normalized = normalizeWorkspacePath(value);
  if (!normalized) {
    return true;
  }
  return normalized.split("/").filter(Boolean).every(isValidWorkspaceName);
};

// 根据当前排序方向刷新按钮图标
const updateWorkspaceSortIcon = () => {
  const icon = elements.workspaceSortOrderBtn?.querySelector("i");
  if (!icon) {
    return;
  }
  const order = state.workspace.sortOrder === "desc" ? "desc" : "asc";
  icon.className = `fa-solid ${WORKSPACE_SORT_ICONS[order]}`;
};

// 选中状态管理（支持多选/范围选择）
const updateWorkspaceSelectionMeta = () => {
  if (!elements.workspaceSelectionMeta) {
    return;
  }
  const count = state.workspace.selectedPaths.size;
  elements.workspaceSelectionMeta.textContent = count > 0 ? `已选择 ${count} 项` : "";
};

const resetWorkspaceSelection = () => {
  state.workspace.selectedPaths = new Set();
  state.workspace.selected = null;
  state.workspace.lastSelectedPath = "";
  updateWorkspaceSelectionMeta();
};

const setWorkspaceSelection = (paths, primaryPath) => {
  state.workspace.selectedPaths = new Set(paths.filter(Boolean));
  state.workspace.selected =
    primaryPath && state.workspace.selectedPaths.has(primaryPath)
      ? findWorkspaceEntry(state.workspace.entries, primaryPath)
      : null;
  if (primaryPath) {
    state.workspace.lastSelectedPath = primaryPath;
  }
  updateWorkspaceSelectionMeta();
};

const toggleWorkspaceSelection = (path) => {
  if (!path) {
    return;
  }
  if (state.workspace.selectedPaths.has(path)) {
    state.workspace.selectedPaths.delete(path);
  } else {
    state.workspace.selectedPaths.add(path);
    state.workspace.lastSelectedPath = path;
  }
  updateWorkspaceSelectionMeta();
};

const getWorkspaceSelectionPaths = () => Array.from(state.workspace.selectedPaths);

// 展开树形结构为线性列表，便于 Shift 区间选择
const flattenWorkspaceEntries = (entries, depth = 0, result = []) => {
  entries.forEach((entry) => {
    result.push(entry);
    if (
      entry.type === "dir" &&
      state.workspace.expanded.has(entry.path) &&
      Array.isArray(entry.children) &&
      entry.children.length
    ) {
      flattenWorkspaceEntries(entry.children, depth + 1, result);
    }
  });
  return result;
};

const findWorkspaceEntry = (entries, targetPath) => {
  if (!Array.isArray(entries) || !targetPath) {
    return null;
  }
  for (const entry of entries) {
    if (entry.path === targetPath) {
      return entry;
    }
    if (entry.children?.length) {
      const found = findWorkspaceEntry(entry.children, targetPath);
      if (found) {
        return found;
      }
    }
  }
  return null;
};

const attachWorkspaceChildren = (entries, targetPath, children) => {
  const target = findWorkspaceEntry(entries, targetPath);
  if (!target || target.type !== "dir") {
    return false;
  }
  target.children = Array.isArray(children) ? children : [];
  target.childrenLoaded = true;
  return true;
};

// 解析拖拽内容，支持文件与文件夹（Chrome/Edge 使用 webkitGetAsEntry）
const readDirectoryEntries = (reader) =>
  new Promise((resolve) => {
    const entries = [];
    const readBatch = () => {
      reader.readEntries(
        (batch) => {
          if (!batch.length) {
            resolve(entries);
            return;
          }
          entries.push(...batch);
          readBatch();
        },
        () => resolve(entries)
      );
    };
    readBatch();
  });

const walkEntry = async (entry, prefix) => {
  if (!entry) {
    return [];
  }
  if (entry.isFile) {
    const file = await new Promise((resolve) => {
      entry.file((f) => resolve(f), () => resolve(null));
    });
    if (!file) {
      return [];
    }
    return [
      {
        file,
        relativePath: `${prefix}${file.name}`,
      },
    ];
  }
  if (entry.isDirectory) {
    const nextPrefix = `${prefix}${entry.name}/`;
    const reader = entry.createReader();
    const children = await readDirectoryEntries(reader);
    const nested = await Promise.all(children.map((child) => walkEntry(child, nextPrefix)));
    return nested.flat();
  }
  return [];
};

const collectDroppedFiles = async (dataTransfer) => {
  const items = Array.from(dataTransfer?.items || []);
  if (items.length) {
    const batches = await Promise.all(
      items.map((item) => {
        const entry = item.webkitGetAsEntry?.();
        if (entry) {
          return walkEntry(entry, "");
        }
        const file = item.getAsFile();
        return file ? [{ file, relativePath: file.name }] : [];
      })
    );
    return batches.flat();
  }
  const files = Array.from(dataTransfer?.files || []);
  return files.map((file) => ({
    file,
    relativePath: file.webkitRelativePath || file.name,
  }));
};

// 判断是否为工作区内部拖拽
const hasWorkspaceDrag = (dataTransfer) =>
  Array.from(dataTransfer?.types || []).includes(WORKSPACE_DRAG_KEY);

// 获取内部拖拽携带的路径列表
const getWorkspaceDragPaths = (dataTransfer) => {
  const raw = dataTransfer?.getData(WORKSPACE_DRAG_KEY) || "";
  if (!raw) {
    return [];
  }
  try {
    const parsed = JSON.parse(raw);
    if (Array.isArray(parsed)) {
      return parsed.filter(Boolean);
    }
  } catch (error) {
    // 非 JSON 时按单一路径处理
  }
  return [raw].filter(Boolean);
};

// 使用相对路径一次性上传，避免大量请求
const uploadWorkspaceGroups = async (items, basePath = "") => {
  const targetBase = normalizeWorkspacePath(basePath || state.workspace.path);
  const files = items.map((item) => item.file).filter(Boolean);
  const relativePaths = items.map((item) => item.relativePath || item.file?.name || "");
  await uploadWorkspaceFiles(files, targetBase, {
    refreshTree: false,
    relativePaths,
  });
  await reloadWorkspaceView({ refreshTree: true });
};

const renderWorkspaceList = (entries) => {
  elements.workspaceList.textContent = "";
  if (!Array.isArray(entries) || entries.length === 0) {
    const empty = document.createElement("div");
    empty.className = "muted";
    empty.textContent = state.workspace.searchMode ? "未找到匹配文件。" : "暂无文件。";
    elements.workspaceList.appendChild(empty);
    state.workspace.flatEntries = [];
    updateWorkspaceSelectionMeta();
    return;
  }

  const flatEntries = [];
  const isTreeView = !state.workspace.searchMode;

  const renderEntry = (entry, depth) => {
    flatEntries.push(entry);
    const item = document.createElement("div");
    item.className = "workspace-item";
    if (entry.type === "dir") {
      item.classList.add("is-folder");
    }
    if (state.workspace.selectedPaths.has(entry.path)) {
      item.classList.add("is-selected");
    }
    if (state.workspace.selected?.path === entry.path) {
      item.classList.add("active");
    }
    item.style.setProperty("--workspace-indent", `${depth * 16}px`);
    const { icon, className } = getWorkspaceEntryIcon(entry);
    const main = document.createElement("div");
    main.className = "workspace-item-main";
    const caret = document.createElement("i");
    caret.className = "fa-solid fa-chevron-right workspace-item-caret";
    if (!isTreeView || entry.type !== "dir") {
      caret.classList.add("hidden");
    } else if (state.workspace.expanded.has(entry.path)) {
      caret.classList.add("expanded");
    }
    caret.addEventListener("click", (event) => {
      event.stopPropagation();
      toggleWorkspaceDirectory(entry);
    });
    const iconNode = document.createElement("i");
    iconNode.className = `fa-solid ${icon} workspace-item-icon ${className}`;
    const name = document.createElement("div");
    name.className = "workspace-item-name";
    if (state.workspace.renamingPath === entry.path) {
      const input = document.createElement("input");
      input.className = "workspace-item-rename";
      input.value = entry.name || "";
      input.addEventListener("click", (event) => event.stopPropagation());
      input.addEventListener("keydown", (event) => {
        if (event.key === "Enter") {
          event.preventDefault();
          finishWorkspaceRename(entry, input.value);
        }
        if (event.key === "Escape") {
          event.preventDefault();
          cancelWorkspaceRename();
        }
      });
      input.addEventListener("blur", () => {
        finishWorkspaceRename(entry, input.value);
      });
      name.appendChild(input);
      requestAnimationFrame(() => {
        input.focus();
        input.select();
      });
    } else {
      name.textContent = entry.name;
    }
    const meta = document.createElement("div");
    meta.className = "workspace-item-meta";
    const metaParts = [];
    if (entry.type === "dir") {
      metaParts.push("目录");
    } else {
      metaParts.push(formatBytes(entry.size || 0));
    }
    if (state.workspace.searchMode && entry.path) {
      metaParts.push(entry.path);
      meta.title = entry.path;
    }
    meta.textContent = metaParts.join(" · ");

    main.appendChild(caret);
    main.appendChild(iconNode);
    main.appendChild(name);
    item.appendChild(main);
    item.appendChild(meta);
    item.addEventListener("click", (event) => {
      handleWorkspaceItemClick(event, entry);
    });
    item.addEventListener("dblclick", () => {
      handleWorkspaceItemDoubleClick(entry);
    });
    item.addEventListener("contextmenu", (event) => {
      event.preventDefault();
      event.stopPropagation();
      openWorkspaceMenu(event, entry);
    });
    item.setAttribute("draggable", "true");
    item.addEventListener("dragstart", (event) => {
      handleWorkspaceItemDragStart(event, entry);
    });
    item.addEventListener("dragend", handleWorkspaceItemDragEnd);
    if (entry.type === "dir") {
      item.addEventListener("dragenter", (event) => {
        handleWorkspaceItemDragEnter(event);
      });
      item.addEventListener("dragover", (event) => {
        handleWorkspaceItemDragOver(event);
      });
      item.addEventListener("dragleave", (event) => {
        handleWorkspaceItemDragLeave(event);
      });
      item.addEventListener("drop", (event) => {
        handleWorkspaceItemDrop(event, entry);
      });
    }
    elements.workspaceList.appendChild(item);

    if (
      isTreeView &&
      entry.type === "dir" &&
      state.workspace.expanded.has(entry.path) &&
      Array.isArray(entry.children) &&
      entry.children.length
    ) {
      entry.children.forEach((child) => renderEntry(child, depth + 1));
    }
  };

  entries.forEach((entry) => renderEntry(entry, 0));
  state.workspace.flatEntries = flatEntries;
};

const updateWorkspacePath = () => {
  const displayPath = state.workspace.path ? `/${state.workspace.path}` : "/";
  elements.workspacePath.textContent = displayPath;
  elements.workspaceUpBtn.disabled = !state.workspace.path;
};

export const loadWorkspace = async (options = {}) => {
  const refreshTree = Boolean(options.refreshTree);
  const resetExpanded = Boolean(options.resetExpanded);
  const resetSearch = Boolean(options.resetSearch);
  const userId = elements.userId.value.trim();
  if (!userId) {
    const message = "请先填写 user_id。";
    elements.workspaceList.textContent = message;
    return { ok: false, error: message };
  }
  if (resetSearch) {
    state.workspace.searchMode = false;
    state.workspace.searchKeyword = "";
    if (elements.workspaceSearchInput) {
      elements.workspaceSearchInput.value = "";
    }
  }
  if (resetExpanded) {
    state.workspace.expanded = new Set();
  }
  state.workspace.renamingPath = "";
  resetWorkspaceSelection();
  const currentPath = normalizeWorkspacePath(state.workspace.path);
  try {
    const result = await fetchWorkspaceContent(currentPath, {
      includeContent: true,
      depth: 1,
      sortBy: state.workspace.sortBy,
      order: state.workspace.sortOrder,
      keyword: "",
      refreshTree,
    });
    if (!result) {
      throw new Error("加载失败");
    }
    const normalizedPath = normalizeWorkspacePath(result.path ?? currentPath);
    state.workspace.path = normalizedPath;
    state.workspace.parent = getWorkspaceParentPath(normalizedPath);
    state.workspace.entries = Array.isArray(result.entries) ? result.entries : [];
    const rootPath = normalizedPath;
    if (state.workspace.expanded.size) {
      const filtered = new Set();
      state.workspace.expanded.forEach((path) => {
        if (!rootPath || path === rootPath || path.startsWith(`${rootPath}/`)) {
          filtered.add(path);
        }
      });
      state.workspace.expanded = filtered;
    }
    await hydrateExpandedEntries();
    updateWorkspacePath();
    renderWorkspaceList(state.workspace.entries);
    return { ok: true };
  } catch (error) {
    const message = error?.message || "加载失败";
    elements.workspaceList.textContent = `加载失败：${message}`;
    return { ok: false, error: message };
  }
};

const loadWorkspaceSearch = async (options = {}) => {
  const keyword = String(state.workspace.searchKeyword || "").trim();
  if (!keyword) {
    state.workspace.searchMode = false;
    return loadWorkspace({ ...options, resetSearch: true });
  }
  state.workspace.renamingPath = "";
  resetWorkspaceSelection();
  try {
    const result = await fetchWorkspaceSearch(keyword, { offset: 0, limit: 200 });
    state.workspace.entries = Array.isArray(result?.entries) ? result.entries : [];
    state.workspace.searchMode = true;
    updateWorkspacePath();
    renderWorkspaceList(state.workspace.entries);
    return { ok: true };
  } catch (error) {
    const message = error?.message || "搜索失败";
    elements.workspaceList.textContent = `搜索失败：${message}`;
    return { ok: false, error: message };
  }
};

const hydrateExpandedEntries = async () => {
  const expandedPaths = Array.from(state.workspace.expanded);
  if (!expandedPaths.length) {
    return;
  }
  for (const path of expandedPaths) {
    const entry = findWorkspaceEntry(state.workspace.entries, path);
    if (!entry || entry.type !== "dir" || entry.childrenLoaded) {
      continue;
    }
    try {
      const result = await fetchWorkspaceContent(path, {
        includeContent: true,
        depth: 1,
        sortBy: state.workspace.sortBy,
        order: state.workspace.sortOrder,
      });
      attachWorkspaceChildren(state.workspace.entries, path, result?.entries || []);
    } catch (error) {
      state.workspace.expanded.delete(path);
    }
  }
};

const toggleWorkspaceDirectory = async (entry) => {
  if (!entry || entry.type !== "dir") {
    return;
  }
  if (state.workspace.expanded.has(entry.path)) {
    state.workspace.expanded.delete(entry.path);
    renderWorkspaceList(state.workspace.entries);
    return;
  }
  state.workspace.expanded.add(entry.path);
  if (entry.childrenLoaded) {
    renderWorkspaceList(state.workspace.entries);
    return;
  }
  try {
    const result = await fetchWorkspaceContent(entry.path, {
      includeContent: true,
      depth: 1,
      sortBy: state.workspace.sortBy,
      order: state.workspace.sortOrder,
    });
    attachWorkspaceChildren(state.workspace.entries, entry.path, result?.entries || []);
  } catch (error) {
    state.workspace.expanded.delete(entry.path);
    notify(error.message || "目录加载失败。", "error");
  }
  renderWorkspaceList(state.workspace.entries);
};

const handleWorkspaceItemClick = (event, entry) => {
  if (!entry || state.workspace.renamingPath) {
    return;
  }
  const path = entry.path;
  const useRange = event.shiftKey && state.workspace.lastSelectedPath;
  const toggle = event.ctrlKey || event.metaKey;
  if (useRange) {
    const flat = state.workspace.flatEntries || [];
    const startIndex = flat.findIndex((item) => item.path === state.workspace.lastSelectedPath);
    const endIndex = flat.findIndex((item) => item.path === path);
    if (startIndex !== -1 && endIndex !== -1) {
      const [from, to] = startIndex < endIndex ? [startIndex, endIndex] : [endIndex, startIndex];
      const rangePaths = flat.slice(from, to + 1).map((item) => item.path);
      if (toggle) {
        rangePaths.forEach((rangePath) => state.workspace.selectedPaths.add(rangePath));
        state.workspace.selected = entry;
        state.workspace.lastSelectedPath = path;
        updateWorkspaceSelectionMeta();
      } else {
        setWorkspaceSelection(rangePaths, path);
      }
      renderWorkspaceList(state.workspace.entries);
      return;
    }
  }
  if (toggle) {
    toggleWorkspaceSelection(path);
    if (state.workspace.selectedPaths.has(path)) {
      state.workspace.selected = entry;
    } else if (state.workspace.selected?.path === path) {
      state.workspace.selected = null;
    }
    renderWorkspaceList(state.workspace.entries);
    return;
  }
  setWorkspaceSelection([path], path);
  renderWorkspaceList(state.workspace.entries);
};

const handleWorkspaceItemDoubleClick = (entry) => {
  if (!entry || state.workspace.renamingPath) {
    return;
  }
  if (entry.type === "dir") {
    state.workspace.path = entry.path;
    state.workspace.expanded = new Set();
    loadWorkspace({ refreshTree: true, resetExpanded: true, resetSearch: true });
    return;
  }
  if (entry.type === "file") {
    openWorkspacePreview(entry);
  }
};

const startWorkspaceRename = (entry) => {
  if (!entry) {
    return;
  }
  state.workspace.renamingPath = entry.path;
  renderWorkspaceList(state.workspace.entries);
};

const cancelWorkspaceRename = () => {
  state.workspace.renamingPath = "";
  renderWorkspaceList(state.workspace.entries);
};

const finishWorkspaceRename = async (entry, nextName) => {
  if (!entry || state.workspace.renamingPath !== entry.path) {
    return;
  }
  const trimmed = String(nextName || "").trim();
  state.workspace.renamingPath = "";
  if (!trimmed || !isValidWorkspaceName(trimmed)) {
    notify("名称不能为空，且不能包含斜杠。", "warn");
    renderWorkspaceList(state.workspace.entries);
    return;
  }
  if (trimmed === entry.name) {
    renderWorkspaceList(state.workspace.entries);
    return;
  }
  const parentPath = getWorkspaceParentPath(entry.path);
  const destination = joinWorkspacePath(parentPath, trimmed);
  try {
    const ok = await moveWorkspaceEntry(entry.path, destination);
    if (!ok) {
      return;
    }
    notify(`已重命名为 ${trimmed}`, "success");
  } catch (error) {
    notify(error.message || "重命名失败。", "error");
  } finally {
    state.workspace.renamingPath = "";
    renderWorkspaceList(state.workspace.entries);
  }
};

const openWorkspaceMenu = (event, entry = null) => {
  if (entry?.path && !state.workspace.selectedPaths.has(entry.path)) {
    setWorkspaceSelection([entry.path], entry.path);
  }
  if (entry?.path) {
    state.workspace.selected = entry;
  }
  renderWorkspaceList(state.workspace.entries);
  const selectedPaths = getWorkspaceSelectionPaths();
  const hasSelection = selectedPaths.length > 0;
  let singleEntry = null;
  if (selectedPaths.length === 1) {
    if (entry?.path === selectedPaths[0]) {
      singleEntry = entry;
    } else {
      singleEntry = findWorkspaceEntry(state.workspace.entries, selectedPaths[0]);
    }
  }
  if (!singleEntry && entry) {
    singleEntry = entry;
  }
  if (!singleEntry && state.workspace.selected) {
    singleEntry = state.workspace.selected;
  }
  elements.workspaceDownloadBtn.disabled = !singleEntry || !["file", "dir"].includes(singleEntry.type);
  elements.workspaceDeleteBtn.disabled = !hasSelection;
  elements.workspaceRenameBtn.disabled = !singleEntry;
  elements.workspaceMoveBtn.disabled = !hasSelection;
  elements.workspaceCopyBtn.disabled = !hasSelection;
  elements.workspaceEditBtn.disabled = !singleEntry || !isWorkspaceTextEditable(singleEntry);
  const menu = elements.workspaceMenu;
  menu.style.display = "flex";
  const menuRect = menu.getBoundingClientRect();
  const maxLeft = window.innerWidth - menuRect.width - 8;
  const maxTop = window.innerHeight - menuRect.height - 8;
  const left = Math.min(event.clientX, maxLeft);
  const top = Math.min(event.clientY, maxTop);
  menu.style.left = `${Math.max(8, left)}px`;
  menu.style.top = `${Math.max(8, top)}px`;
};

const closeWorkspaceMenu = () => {
  elements.workspaceMenu.style.display = "none";
};

const buildWorkspaceDownloadUrl = (entry) => {
  const wunderBase = getWunderBase();
  const params = new URLSearchParams({
    user_id: elements.userId.value.trim(),
    path: entry.path,
  });
  return `${wunderBase}/workspace/download?${params.toString()}`;
};

// 生成工作区压缩包下载地址（支持全量或指定目录）
const buildWorkspaceArchiveUrl = (path = "") => {
  const wunderBase = getWunderBase();
  const userId = elements.userId.value.trim();
  if (!userId) {
    return "";
  }
  const params = new URLSearchParams({
    user_id: userId,
  });
  const normalizedPath = normalizeWorkspacePath(path);
  if (normalizedPath) {
    params.set("path", normalizedPath);
  }
  return `${wunderBase}/workspace/archive?${params.toString()}`;
};

const getWorkspaceAuthHeaders = () => {
  const apiKey = String(elements.apiKey?.value || "").trim();
  if (!apiKey) {
    return undefined;
  }
  return { "X-API-Key": apiKey };
};

const downloadWorkspaceByFetch = async (url, filename) => {
  if (!url) {
    return false;
  }
  try {
    const response = await fetch(url, { headers: getWorkspaceAuthHeaders() });
    if (!response.ok) {
      throw new Error(`请求失败：${response.status}`);
    }
    const blob = await response.blob();
    const objectUrl = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = objectUrl;
    link.download = filename || "download";
    document.body.appendChild(link);
    link.click();
    document.body.removeChild(link);
    URL.revokeObjectURL(objectUrl);
    return true;
  } catch (error) {
    notify(`下载失败：${error.message}`, "error");
    return false;
  }
};

const downloadWorkspaceEntry = (entry) => {
  if (!entry) {
    return;
  }
  if (entry.type === "dir") {
    // 目录下载走压缩包接口，保持目录结构
    const userId = getWorkspaceUserId();
    if (!userId) {
      return;
    }
    const url = buildWorkspaceArchiveUrl(entry.path);
    if (!url) {
      appendLog("请先填写 user_id。");
      return;
    }
    const downloadName = entry.name ? `${entry.name}.zip` : "workspace_folder.zip";
    downloadWorkspaceByFetch(url, downloadName);
    return;
  }
  if (entry.type !== "file") {
    return;
  }
  const url = buildWorkspaceDownloadUrl(entry);
  downloadWorkspaceByFetch(url, entry.name || "download");
};

// 下载工作区全量压缩包，便于一次性保存所有文件
const downloadWorkspaceArchive = () => {
  const url = buildWorkspaceArchiveUrl();
  if (!url) {
    appendLog("请先填写 user_id。");
    return false;
  }
  const userId = elements.userId.value.trim();
  return downloadWorkspaceByFetch(url, `workspace_${userId || "all"}.zip`);
};

// 统一获取 user_id，避免重复提示
const getWorkspaceUserId = () => {
  const userId = elements.userId.value.trim();
  if (!userId) {
    notify("请先填写 user_id。", "warn");
    return "";
  }
  return userId;
};

// 统一提取后端报错信息，兼容 detail.message 格式
const getWorkspaceErrorMessage = (result, fallback) => {
  if (result?.message) {
    return result.message;
  }
  if (result?.detail?.message) {
    return result.detail.message;
  }
  return fallback;
};

const buildWorkspaceContentUrl = (path, options = {}) => {
  const wunderBase = getWunderBase();
  const params = new URLSearchParams({
    user_id: getWorkspaceUserId(),
    path: normalizeWorkspacePath(path || ""),
  });
  if (options.includeContent !== undefined) {
    params.set("include_content", options.includeContent ? "true" : "false");
  }
  if (options.maxBytes) {
    params.set("max_bytes", String(options.maxBytes));
  }
  if (options.depth) {
    params.set("depth", String(options.depth));
  }
  if (options.keyword) {
    params.set("keyword", options.keyword);
  }
  if (options.offset) {
    params.set("offset", String(options.offset));
  }
  if (options.limit) {
    params.set("limit", String(options.limit));
  }
  if (options.sortBy) {
    params.set("sort_by", options.sortBy);
  }
  if (options.order) {
    params.set("order", options.order);
  }
  return `${wunderBase}/workspace/content?${params.toString()}`;
};

const fetchWorkspaceContent = async (path, options = {}) => {
  const userId = getWorkspaceUserId();
  if (!userId) {
    return null;
  }
  const endpoint = buildWorkspaceContentUrl(path, options);
  const response = await fetch(endpoint);
  if (!response.ok) {
    const result = await response.json().catch(() => ({}));
    throw new Error(getWorkspaceErrorMessage(result, `加载失败：${response.status}`));
  }
  return response.json();
};

const fetchWorkspaceSearch = async (keyword, options = {}) => {
  const userId = getWorkspaceUserId();
  if (!userId) {
    return null;
  }
  const wunderBase = getWunderBase();
  const params = new URLSearchParams({
    user_id: userId,
    keyword,
  });
  if (options.offset) {
    params.set("offset", String(options.offset));
  }
  if (options.limit) {
    params.set("limit", String(options.limit));
  }
  const endpoint = `${wunderBase}/workspace/search?${params.toString()}`;
  const response = await fetch(endpoint);
  if (!response.ok) {
    const result = await response.json().catch(() => ({}));
    throw new Error(getWorkspaceErrorMessage(result, `搜索失败：${response.status}`));
  }
  return response.json();
};

const batchWorkspaceAction = async (action, paths, destination) => {
  const userId = getWorkspaceUserId();
  if (!userId) {
    return null;
  }
  const wunderBase = getWunderBase();
  const response = await fetch(`${wunderBase}/workspace/batch`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      user_id: userId,
      action,
      paths,
      destination,
    }),
  });
  if (!response.ok) {
    const result = await response.json().catch(() => ({}));
    throw new Error(getWorkspaceErrorMessage(result, `批量操作失败：${response.status}`));
  }
  return response.json();
};

const reloadWorkspaceView = async (options = {}) => {
  if (state.workspace.searchMode && state.workspace.searchKeyword) {
    return loadWorkspaceSearch(options);
  }
  return loadWorkspace(options);
};

// 请求后端新建目录
const createWorkspaceDirectory = async (path) => {
  const userId = getWorkspaceUserId();
  if (!userId) {
    return false;
  }
  const wunderBase = getWunderBase();
  const response = await fetch(`${wunderBase}/workspace/dir`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ user_id: userId, path }),
  });
  if (!response.ok) {
    const result = await response.json().catch(() => ({}));
    throw new Error(getWorkspaceErrorMessage(result, `创建失败：${response.status}`));
  }
  await reloadWorkspaceView({ refreshTree: true });
  return true;
};

// 请求后端移动/重命名条目
const moveWorkspaceEntry = async (source, destination) => {
  const userId = getWorkspaceUserId();
  if (!userId) {
    return false;
  }
  const wunderBase = getWunderBase();
  const response = await fetch(`${wunderBase}/workspace/move`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ user_id: userId, source, destination }),
  });
  if (!response.ok) {
    const result = await response.json().catch(() => ({}));
    throw new Error(getWorkspaceErrorMessage(result, `移动失败：${response.status}`));
  }
  await reloadWorkspaceView({ refreshTree: true });
  return true;
};

// 保存文件内容到后端
const saveWorkspaceFileContent = async (path, content, options = {}) => {
  const userId = getWorkspaceUserId();
  if (!userId) {
    return false;
  }
  const wunderBase = getWunderBase();
  const response = await fetch(`${wunderBase}/workspace/file`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      user_id: userId,
      path,
      content,
      create_if_missing: Boolean(options.createIfMissing),
    }),
  });
  if (!response.ok) {
    const result = await response.json().catch(() => ({}));
    throw new Error(getWorkspaceErrorMessage(result, `保存失败：${response.status}`));
  }
  await reloadWorkspaceView({ refreshTree: true });
  return true;
};

const clearPreviewObjectUrl = () => {
  if (previewObjectUrl) {
    URL.revokeObjectURL(previewObjectUrl);
    previewObjectUrl = null;
  }
};

const setPreviewHint = (message) => {
  const text = message ? String(message) : "";
  elements.workspacePreviewHint.textContent = text;
  elements.workspacePreviewHint.style.display = text ? "block" : "none";
};

const resetPreviewContainer = () => {
  clearPreviewObjectUrl();
  elements.workspacePreviewContainer.classList.remove("embed");
  elements.workspacePreviewContainer.textContent = "";
};

const renderUnsupportedPreview = (message) => {
  const placeholder = document.createElement("div");
  placeholder.className = "muted";
  placeholder.textContent = message || "暂不支持预览。";
  elements.workspacePreviewContainer.appendChild(placeholder);
};

const renderTextPreview = async (entry, url) => {
  resetPreviewContainer();
  renderUnsupportedPreview("正在加载预览...");
  try {
    const response = await fetch(url, { headers: getWorkspaceAuthHeaders() });
    if (!response.ok) {
      throw new Error(`请求失败：${response.status}`);
    }
    const text = await response.text();
    if (state.workspace.previewEntry?.path !== entry.path) {
      return;
    }
    resetPreviewContainer();
    const pre = document.createElement("pre");
    pre.className = "workspace-preview-text";
    pre.textContent = text || "（文件为空）";
    elements.workspacePreviewContainer.appendChild(pre);
  } catch (error) {
    if (state.workspace.previewEntry?.path !== entry.path) {
      return;
    }
    resetPreviewContainer();
    setPreviewHint("预览加载失败，请下载查看。");
    renderUnsupportedPreview("预览加载失败，请下载查看。");
  }
};

const renderImagePreview = async (entry, url) => {
  resetPreviewContainer();
  renderUnsupportedPreview("正在加载预览...");
  try {
    const response = await fetch(url, { headers: getWorkspaceAuthHeaders() });
    if (!response.ok) {
      throw new Error(`请求失败：${response.status}`);
    }
    const blob = await response.blob();
    if (state.workspace.previewEntry?.path !== entry.path) {
      return;
    }
    resetPreviewContainer();
    previewObjectUrl = URL.createObjectURL(blob);
    const img = document.createElement("img");
    img.src = previewObjectUrl;
    img.alt = entry.name || "image";
    elements.workspacePreviewContainer.appendChild(img);
  } catch (error) {
    if (state.workspace.previewEntry?.path !== entry.path) {
      return;
    }
    resetPreviewContainer();
    setPreviewHint("预览加载失败，请下载查看。");
    renderUnsupportedPreview("预览加载失败，请下载查看。");
  }
};

const renderPdfPreview = async (entry, url) => {
  resetPreviewContainer();
  renderUnsupportedPreview("正在加载预览...");
  try {
    const response = await fetch(url, { headers: getWorkspaceAuthHeaders() });
    if (!response.ok) {
      throw new Error(`请求失败：${response.status}`);
    }
    const blob = await response.blob();
    if (state.workspace.previewEntry?.path !== entry.path) {
      return;
    }
    resetPreviewContainer();
    previewObjectUrl = URL.createObjectURL(blob);
    elements.workspacePreviewContainer.classList.add("embed");
    const iframe = document.createElement("iframe");
    iframe.src = previewObjectUrl;
    iframe.title = entry.name || "pdf";
    elements.workspacePreviewContainer.appendChild(iframe);
  } catch (error) {
    if (state.workspace.previewEntry?.path !== entry.path) {
      return;
    }
    resetPreviewContainer();
    setPreviewHint("预览加载失败，请下载查看。");
    renderUnsupportedPreview("预览加载失败，请下载查看。");
  }
};

const openWorkspacePreview = async (entry) => {
  if (!entry || entry.type !== "file") {
    return;
  }
  state.workspace.previewEntry = entry;
  elements.workspacePreviewTitle.textContent = entry.name || "文件预览";
  const metaParts = [];
  if (entry.path) {
    metaParts.push(entry.path);
  }
  if (Number.isFinite(entry.size)) {
    metaParts.push(formatBytes(entry.size));
  }
  if (entry.updated_time) {
    const updated = new Date(entry.updated_time);
    if (!Number.isNaN(updated.getTime())) {
      metaParts.push(updated.toLocaleString());
    }
  }
  elements.workspacePreviewMeta.textContent = metaParts.join(" · ");
  setPreviewHint("");
  resetPreviewContainer();
  elements.workspacePreviewModal.classList.add("active");

  const extension = getWorkspaceExtension(entry);
  const downloadUrl = buildWorkspaceDownloadUrl(entry);
  const sizeValue = Number.isFinite(entry.size) ? entry.size : 0;
  const canPreviewText = sizeValue <= MAX_TEXT_PREVIEW_SIZE;

  if (OFFICE_EXTENSIONS.has(extension)) {
    setPreviewHint("浏览器不支持该格式预览，可下载后使用本机程序打开。");
    renderUnsupportedPreview("暂不支持该格式预览。");
    return;
  }
  if (IMAGE_EXTENSIONS.has(extension)) {
    await renderImagePreview(entry, downloadUrl);
    return;
  }
  if (PDF_EXTENSIONS.has(extension)) {
    await renderPdfPreview(entry, downloadUrl);
    return;
  }
  if (TEXT_EXTENSIONS.has(extension)) {
    if (!canPreviewText) {
      setPreviewHint("文件过大，无法预览，请下载查看。");
      renderUnsupportedPreview("文件过大，无法预览。");
      return;
    }
    await renderTextPreview(entry, downloadUrl);
    return;
  }
  if (!canPreviewText) {
    setPreviewHint("文件过大，无法预览，请下载查看。");
    renderUnsupportedPreview("文件过大，无法预览。");
    return;
  }
  setPreviewHint("未识别格式，已按文本方式尝试预览。");
  await renderTextPreview(entry, downloadUrl);
};

// 打开文件编辑弹窗
const openWorkspaceEditor = async (entry) => {
  if (!entry || entry.type !== "file") {
    return;
  }
  if (!getWorkspaceUserId()) {
    return;
  }
  if (!isWorkspaceTextEditable(entry)) {
    notify("仅支持预览范围内的文本文件编辑。", "warn");
    return;
  }
  editorEntry = entry;
  editorLoading = true;
  elements.workspaceEditorTitle.textContent = `编辑文件：${entry.name || ""}`;
  elements.workspaceEditorPath.textContent = entry.path || "";
  elements.workspaceEditorContent.value = "正在加载...";
  elements.workspaceEditorModal.classList.add("active");
  try {
    const result = await fetchWorkspaceContent(entry.path, {
      includeContent: true,
      maxBytes: MAX_TEXT_PREVIEW_SIZE,
    });
    const text = result?.content ?? "";
    if (result?.truncated) {
      throw new Error("文件过大，无法编辑。");
    }
    if (!editorEntry || editorEntry.path !== entry.path) {
      return;
    }
    elements.workspaceEditorContent.value = text;
  } catch (error) {
    if (editorEntry?.path === entry.path) {
      notify(error.message || "文件加载失败。", "error");
      closeWorkspaceEditor();
    }
  } finally {
    editorLoading = false;
  }
};

// 关闭编辑弹窗并清理状态
const closeWorkspaceEditor = () => {
  editorEntry = null;
  editorLoading = false;
  elements.workspaceEditorContent.value = "";
  elements.workspaceEditorModal.classList.remove("active");
};

// 保存编辑内容
const saveWorkspaceEditor = async () => {
  if (!editorEntry || editorLoading) {
    return;
  }
  try {
    const ok = await saveWorkspaceFileContent(
      editorEntry.path,
      elements.workspaceEditorContent.value
    );
    if (!ok) {
      return;
    }
    notify(`已保存 ${editorEntry.name || "文件"}`, "success");
    closeWorkspaceEditor();
  } catch (error) {
    notify(error.message || "文件保存失败。", "error");
  }
};

const closeWorkspacePreview = () => {
  state.workspace.previewEntry = null;
  elements.workspacePreviewTitle.textContent = "文件预览";
  elements.workspacePreviewMeta.textContent = "";
  setPreviewHint("");
  resetPreviewContainer();
  elements.workspacePreviewModal.classList.remove("active");
};

const notifyBatchResult = (result, actionLabel) => {
  const failedCount = result?.failed?.length || 0;
  const succeededCount = result?.succeeded?.length || 0;
  if (failedCount) {
    notify(`${actionLabel}部分失败：成功 ${succeededCount} 项，失败 ${failedCount} 项。`, "warn");
  } else {
    notify(`${actionLabel}完成：成功 ${succeededCount} 项。`, "success");
  }
};

const deleteWorkspaceSelection = async () => {
  const selectedPaths = getWorkspaceSelectionPaths();
  if (!selectedPaths.length) {
    notify("未选择任何条目。", "info");
    return;
  }
  const confirmed = window.confirm(
    selectedPaths.length === 1
      ? "确认删除所选条目吗？"
      : `确认删除所选 ${selectedPaths.length} 项吗？`
  );
  if (!confirmed) {
    return;
  }
  try {
    const result = await batchWorkspaceAction("delete", selectedPaths);
    if (!result) {
      return;
    }
    notifyBatchResult(result, "删除");
    await reloadWorkspaceView({ refreshTree: true });
  } catch (error) {
    notify(error.message || "删除失败。", "error");
  }
};

const moveWorkspaceSelectionToDirectory = async () => {
  const selectedPaths = getWorkspaceSelectionPaths();
  if (!selectedPaths.length) {
    notify("未选择任何条目。", "info");
    return;
  }
  const targetDirInput = window.prompt("请输入目标目录（相对路径，留空为根目录）", "");
  if (targetDirInput === null) {
    return;
  }
  const targetDir = normalizeWorkspacePath(targetDirInput.trim());
  if (!isValidWorkspacePath(targetDir)) {
    notify("目录格式不正确，不能包含非法路径段。", "warn");
    return;
  }
  try {
    const result = await batchWorkspaceAction("move", selectedPaths, targetDir);
    if (!result) {
      return;
    }
    notifyBatchResult(result, "移动");
    await reloadWorkspaceView({ refreshTree: true });
  } catch (error) {
    notify(error.message || "移动失败。", "error");
  }
};

const copyWorkspaceSelectionToDirectory = async () => {
  const selectedPaths = getWorkspaceSelectionPaths();
  if (!selectedPaths.length) {
    notify("未选择任何条目。", "info");
    return;
  }
  const targetDirInput = window.prompt("请输入目标目录（相对路径，留空为根目录）", "");
  if (targetDirInput === null) {
    return;
  }
  const targetDir = normalizeWorkspacePath(targetDirInput.trim());
  if (!isValidWorkspacePath(targetDir)) {
    notify("目录格式不正确，不能包含非法路径段。", "warn");
    return;
  }
  try {
    const result = await batchWorkspaceAction("copy", selectedPaths, targetDir);
    if (!result) {
      return;
    }
    notifyBatchResult(result, "复制");
    await reloadWorkspaceView({ refreshTree: true });
  } catch (error) {
    notify(error.message || "复制失败。", "error");
  }
};

const moveWorkspaceEntryToDirectory = async (entry) => {
  if (!entry) {
    return;
  }
  if (state.workspace.selectedPaths.size > 1) {
    await moveWorkspaceSelectionToDirectory();
    return;
  }
  const targetDirInput = window.prompt("请输入目标目录（相对路径，留空为根目录）", "");
  if (targetDirInput === null) {
    return;
  }
  const targetDir = normalizeWorkspacePath(targetDirInput.trim());
  if (!isValidWorkspacePath(targetDir)) {
    notify("目录格式不正确，不能包含非法路径段。", "warn");
    return;
  }
  const sourceName = entry.name || entry.path.split("/").pop();
  if (!sourceName) {
    notify("无法解析源文件名称。", "error");
    return;
  }
  const destination = joinWorkspacePath(targetDir, sourceName);
  if (destination === entry.path) {
    notify("目标目录与当前目录一致。", "info");
    return;
  }
  try {
    const ok = await moveWorkspaceEntry(entry.path, destination);
    if (!ok) {
      return;
    }
    notify(`已移动到 ${targetDir || "/"}。`, "success");
  } catch (error) {
    notify(error.message || "移动失败。", "error");
  }
};

const renameWorkspaceEntry = (entry) => {
  if (!entry) {
    return;
  }
  startWorkspaceRename(entry);
};

const createWorkspaceFile = async () => {
  const fileName = window.prompt("请输入新文件名称", "untitled.txt");
  if (fileName === null) {
    return;
  }
  const trimmed = String(fileName || "").trim();
  if (!isValidWorkspaceName(trimmed)) {
    notify("名称不能为空，且不能包含斜杠。", "warn");
    return;
  }
  const basePath = normalizeWorkspacePath(state.workspace.path);
  const targetPath = joinWorkspacePath(basePath, trimmed);
  try {
    const ok = await saveWorkspaceFileContent(targetPath, "", { createIfMissing: true });
    if (!ok) {
      return;
    }
    notify(`已创建文件 ${trimmed}`, "success");
  } catch (error) {
    notify(error.message || "创建文件失败。", "error");
  }
};

// 新建文件夹
const createWorkspaceFolder = async () => {
  const folderName = window.prompt("请输入新文件夹名称");
  if (folderName === null) {
    return;
  }
  const trimmed = String(folderName || "").trim();
  if (!isValidWorkspaceName(trimmed)) {
    notify("名称不能为空，且不能包含斜杠。", "warn");
    return;
  }
  const basePath = normalizeWorkspacePath(state.workspace.path);
  const targetPath = joinWorkspacePath(basePath, trimmed);
  try {
    const ok = await createWorkspaceDirectory(targetPath);
    if (!ok) {
      return;
    }
    notify(`已创建文件夹 ${trimmed}`, "success");
  } catch (error) {
    notify(error.message || "创建文件夹失败。", "error");
  }
};

export const uploadWorkspaceFiles = async (files, targetPath = "", options = {}) => {
  const { refreshTree = true, relativePaths = [] } = options;
  const userId = elements.userId.value.trim();
  if (!userId) {
    throw new Error("请先填写 user_id。");
  }
  if (!files || !Array.from(files).length) {
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/workspace/upload`;
  const form = new FormData();
  form.append("user_id", userId);
  form.append("path", normalizeWorkspacePath(targetPath));
  const fileList = Array.from(files);
  fileList.forEach((file, index) => {
    form.append("files", file);
    const relativePath = relativePaths[index] ?? "";
    form.append("relative_paths", relativePath);
  });
  const response = await fetch(endpoint, {
    method: "POST",
    body: form,
  });
  if (!response.ok) {
    throw new Error(`上传失败：${response.status}`);
  }
  if (refreshTree) {
    await reloadWorkspaceView({ refreshTree: true });
  }
};

// 处理拖拽上传：允许文件与文件夹直接拖入工作区
const handleWorkspaceDragEnter = (event) => {
  event.preventDefault();
  elements.workspaceList.classList.add("dragover");
};

const handleWorkspaceDragOver = (event) => {
  event.preventDefault();
  if (event.dataTransfer) {
    event.dataTransfer.dropEffect = hasWorkspaceDrag(event.dataTransfer) ? "move" : "copy";
  }
  elements.workspaceList.classList.add("dragover");
};

const handleWorkspaceDragLeave = (event) => {
  if (!event.currentTarget.contains(event.relatedTarget)) {
    elements.workspaceList.classList.remove("dragover");
  }
};

const handleWorkspaceDrop = async (event) => {
  event.preventDefault();
  elements.workspaceList.classList.remove("dragover");
  if (hasWorkspaceDrag(event.dataTransfer)) {
    return;
  }
  const dropped = await collectDroppedFiles(event.dataTransfer);
  if (!dropped.length) {
    return;
  }
  try {
    await uploadWorkspaceGroups(dropped);
    appendLog("工作区拖拽上传完成。");
    notify("工作区拖拽上传完成。", "success");
  } catch (error) {
    appendLog(`工作区拖拽上传失败：${error.message}`);
    notify(`工作区拖拽上传失败：${error.message}`, "error");
  }
};

// 在上级按钮上拖拽：允许直接移动到父目录
const handleWorkspaceUpDragOver = (event) => {
  if (!hasWorkspaceDrag(event.dataTransfer)) {
    return;
  }
  if (!state.workspace.path) {
    return;
  }
  event.preventDefault();
  event.currentTarget?.classList?.add("dragover");
  if (event.dataTransfer) {
    event.dataTransfer.dropEffect = "move";
  }
};

// 离开上级按钮时取消高亮
const handleWorkspaceUpDragLeave = (event) => {
  if (!event.currentTarget?.contains(event.relatedTarget)) {
    event.currentTarget?.classList?.remove("dragover");
  }
};

// 放置到上级按钮，触发移动到父目录
const handleWorkspaceUpDrop = async (event) => {
  if (!state.workspace.path) {
    return;
  }
  event.preventDefault();
  event.currentTarget?.classList?.remove("dragover");
  const sourcePaths = getWorkspaceDragPaths(event.dataTransfer);
  if (!sourcePaths.length) {
    return;
  }
  const parentPath = getWorkspaceParentPath(state.workspace.path);
  try {
    const result = await batchWorkspaceAction("move", sourcePaths, parentPath);
    if (!result) {
      return;
    }
    notifyBatchResult(result, "移动到上级目录");
    await reloadWorkspaceView({ refreshTree: true });
  } catch (error) {
    notify(error.message || "移动失败。", "error");
  }
};

// 工作区条目拖拽：开始时写入拖拽路径
const handleWorkspaceItemDragStart = (event, entry) => {
  if (!event.dataTransfer || !entry?.path) {
    return;
  }
  if (!state.workspace.selectedPaths.has(entry.path)) {
    setWorkspaceSelection([entry.path], entry.path);
  }
  const selectedPaths = state.workspace.selectedPaths.has(entry.path)
    ? getWorkspaceSelectionPaths()
    : [entry.path];
  event.dataTransfer.setData(WORKSPACE_DRAG_KEY, JSON.stringify(selectedPaths));
  // 同步写入 text/plain，提升浏览器兼容性
  event.dataTransfer.setData("text/plain", selectedPaths[0] || entry.path);
  event.dataTransfer.effectAllowed = "move";
  event.currentTarget?.classList?.add("dragging");
};

// 拖拽结束时清理样式
const handleWorkspaceItemDragEnd = (event) => {
  event.currentTarget?.classList?.remove("dragging");
};

// 拖拽进入目录时高亮
const handleWorkspaceItemDragEnter = (event) => {
  event.preventDefault();
  event.currentTarget?.classList?.add("drop-target");
};

// 拖拽悬停目录时允许放置
const handleWorkspaceItemDragOver = (event) => {
  event.preventDefault();
  if (event.dataTransfer) {
    event.dataTransfer.dropEffect = hasWorkspaceDrag(event.dataTransfer) ? "move" : "copy";
  }
};

// 拖拽离开目录时取消高亮
const handleWorkspaceItemDragLeave = (event) => {
  if (!event.currentTarget?.contains(event.relatedTarget)) {
    event.currentTarget?.classList?.remove("drop-target");
  }
};

// 放置到目录：内部拖拽则移动，外部拖拽则上传
const handleWorkspaceItemDrop = async (event, entry) => {
  event.preventDefault();
  event.stopPropagation();
  event.currentTarget?.classList?.remove("drop-target");
  if (!entry || entry.type !== "dir") {
    return;
  }
  const internalPaths = getWorkspaceDragPaths(event.dataTransfer);
  if (internalPaths.length) {
    const targetDir = normalizeWorkspacePath(entry.path);
    const filtered = internalPaths
      .map((path) => normalizeWorkspacePath(path))
      .filter((path) => path && path !== targetDir);
    if (!filtered.length) {
      return;
    }
    try {
      const result = await batchWorkspaceAction("move", filtered, targetDir);
      if (!result) {
        return;
      }
      notifyBatchResult(result, `移动到 ${entry.name || "目录"}`);
      await reloadWorkspaceView({ refreshTree: true });
    } catch (error) {
      notify(error.message || "移动失败。", "error");
    }
    return;
  }
  const dropped = await collectDroppedFiles(event.dataTransfer);
  if (!dropped.length) {
    return;
  }
  try {
    await uploadWorkspaceGroups(dropped, entry.path);
    appendLog("工作区拖拽上传完成。");
    notify("工作区拖拽上传完成。", "success");
  } catch (error) {
    appendLog(`工作区拖拽上传失败：${error.message}`);
    notify(`工作区拖拽上传失败：${error.message}`, "error");
  }
};

// 重置工作区状态，便于切换用户或重新加载
export const resetWorkspaceState = () => {
  state.workspace.path = "";
  state.workspace.parent = null;
  state.workspace.selected = null;
  state.workspace.selectedPaths = new Set();
  state.workspace.lastSelectedPath = "";
  state.workspace.expanded = new Set();
  state.workspace.searchKeyword = "";
  state.workspace.searchMode = false;
  state.workspace.renamingPath = "";
  state.workspace.flatEntries = [];
  updateWorkspaceSelectionMeta();
  if (elements.workspaceSearchInput) {
    elements.workspaceSearchInput.value = "";
  }
  closeWorkspacePreview();
  closeWorkspaceEditor();
};

// 初始化工作区相关交互
export const initWorkspace = () => {
  updateWorkspaceSortIcon();
  if (elements.workspaceSortSelect) {
    elements.workspaceSortSelect.value = state.workspace.sortBy || "name";
  }
  elements.workspaceRefreshBtn.addEventListener("click", async () => {
    const result = await reloadWorkspaceView({ refreshTree: true });
    if (result?.ok) {
      notify(state.workspace.searchMode ? "搜索结果已刷新。" : "工作区已刷新。", "success");
      return;
    }
    const message = result?.error;
    if (!message) {
      notify("工作区刷新失败。", "error");
      return;
    }
    const isUserMissing = message.includes("user_id");
    notify(isUserMissing ? message : `工作区刷新失败：${message}`, isUserMissing ? "warn" : "error");
  });
  elements.workspaceUpBtn.addEventListener("click", () => {
    if (!state.workspace.path) {
      return;
    }
    state.workspace.path = getWorkspaceParentPath(state.workspace.path);
    state.workspace.expanded = new Set();
    state.workspace.selected = null;
    loadWorkspace({ refreshTree: true, resetExpanded: true, resetSearch: true });
  });
  elements.workspaceUpBtn.addEventListener("dragover", handleWorkspaceUpDragOver);
  elements.workspaceUpBtn.addEventListener("dragleave", handleWorkspaceUpDragLeave);
  elements.workspaceUpBtn.addEventListener("drop", handleWorkspaceUpDrop);
  elements.workspaceNewFileBtn.addEventListener("click", () => {
    createWorkspaceFile();
  });
  elements.workspaceNewFolderQuickBtn.addEventListener("click", () => {
    createWorkspaceFolder();
  });
  elements.workspaceUploadBtn.addEventListener("click", () => {
    elements.workspaceUploadInput.value = "";
    elements.workspaceUploadInput.click();
  });
  elements.workspaceDownloadAllBtn.addEventListener("click", () => {
    const ok = downloadWorkspaceArchive();
    if (ok) {
      notify("工作区压缩包已开始下载。", "info");
    } else {
      notify("请先填写 user_id。", "warn");
    }
  });
  elements.workspaceUploadInput.addEventListener("change", async () => {
    const files = elements.workspaceUploadInput.files;
    if (!files || files.length === 0) {
      return;
    }
    try {
      await uploadWorkspaceFiles(files, state.workspace.path);
      appendLog("工作区上传完成。");
      notify("工作区上传完成。", "success");
    } catch (error) {
      appendLog(`工作区上传失败：${error.message}`);
      notify(`工作区上传失败：${error.message}`, "error");
    }
  });
  if (elements.workspaceSearchInput) {
    let searchTimer = null;
    elements.workspaceSearchInput.addEventListener("input", () => {
      const keyword = elements.workspaceSearchInput.value.trim();
      state.workspace.searchKeyword = keyword;
      if (searchTimer) {
        clearTimeout(searchTimer);
      }
      searchTimer = setTimeout(() => {
        if (state.workspace.searchKeyword) {
          loadWorkspaceSearch({ refreshTree: false });
        } else {
          loadWorkspace({ refreshTree: true, resetSearch: true });
        }
      }, WORKSPACE_SEARCH_DEBOUNCE_MS);
    });
    elements.workspaceSearchInput.addEventListener("keydown", (event) => {
      if (event.key === "Escape") {
        elements.workspaceSearchInput.value = "";
        state.workspace.searchKeyword = "";
        loadWorkspace({ refreshTree: true, resetSearch: true });
      }
    });
  }
  elements.workspaceSortSelect?.addEventListener("change", () => {
    state.workspace.sortBy = elements.workspaceSortSelect.value || "name";
    reloadWorkspaceView({ refreshTree: true });
  });
  elements.workspaceSortOrderBtn?.addEventListener("click", () => {
    state.workspace.sortOrder = state.workspace.sortOrder === "asc" ? "desc" : "asc";
    updateWorkspaceSortIcon();
    reloadWorkspaceView({ refreshTree: true });
  });
  elements.workspaceList.addEventListener("dragenter", handleWorkspaceDragEnter);
  elements.workspaceList.addEventListener("dragover", handleWorkspaceDragOver);
  elements.workspaceList.addEventListener("dragleave", handleWorkspaceDragLeave);
  elements.workspaceList.addEventListener("drop", handleWorkspaceDrop);
  elements.workspaceList.addEventListener("contextmenu", (event) => {
    // 右键空白区域时仅提供新建文件夹等操作
    if (event.target.closest(".workspace-item")) {
      return;
    }
    event.preventDefault();
    openWorkspaceMenu(event, null);
  });
  elements.workspaceDownloadBtn.addEventListener("click", () => {
    closeWorkspaceMenu();
    const selectedPaths = getWorkspaceSelectionPaths();
    const entry =
      (selectedPaths.length === 1
        ? findWorkspaceEntry(state.workspace.entries, selectedPaths[0])
        : null) || state.workspace.selected;
    if (!entry) {
      return;
    }
    downloadWorkspaceEntry(entry);
  });
  elements.workspaceDeleteBtn.addEventListener("click", async () => {
    closeWorkspaceMenu();
    await deleteWorkspaceSelection();
  });
  elements.workspaceEditBtn.addEventListener("click", () => {
    closeWorkspaceMenu();
    const selectedPaths = getWorkspaceSelectionPaths();
    if (selectedPaths.length !== 1) {
      return;
    }
    const entry = findWorkspaceEntry(state.workspace.entries, selectedPaths[0]);
    openWorkspaceEditor(entry);
  });
  elements.workspaceRenameBtn.addEventListener("click", () => {
    closeWorkspaceMenu();
    const selectedPaths = getWorkspaceSelectionPaths();
    if (selectedPaths.length !== 1) {
      return;
    }
    const entry = findWorkspaceEntry(state.workspace.entries, selectedPaths[0]);
    renameWorkspaceEntry(entry);
  });
  elements.workspaceMoveBtn.addEventListener("click", () => {
    closeWorkspaceMenu();
    moveWorkspaceEntryToDirectory(state.workspace.selected);
  });
  elements.workspaceCopyBtn.addEventListener("click", () => {
    closeWorkspaceMenu();
    copyWorkspaceSelectionToDirectory();
  });
  elements.workspaceNewFolderBtn.addEventListener("click", () => {
    closeWorkspaceMenu();
    createWorkspaceFolder();
  });
  elements.workspaceNewFileMenuBtn.addEventListener("click", () => {
    closeWorkspaceMenu();
    createWorkspaceFile();
  });
  elements.workspacePreviewClose.addEventListener("click", closeWorkspacePreview);
  elements.workspacePreviewCloseBtn.addEventListener("click", closeWorkspacePreview);
  elements.workspacePreviewModal.addEventListener("click", (event) => {
    if (event.target === elements.workspacePreviewModal) {
      closeWorkspacePreview();
    }
  });
  elements.workspacePreviewDownload.addEventListener("click", () => {
    downloadWorkspaceEntry(state.workspace.previewEntry);
  });
  elements.workspaceEditorClose.addEventListener("click", closeWorkspaceEditor);
  elements.workspaceEditorCloseBtn.addEventListener("click", closeWorkspaceEditor);
  elements.workspaceEditorSave.addEventListener("click", saveWorkspaceEditor);
  elements.workspaceEditorModal.addEventListener("click", (event) => {
    if (event.target === elements.workspaceEditorModal) {
      closeWorkspaceEditor();
    }
  });
  document.addEventListener("click", (event) => {
    if (!elements.workspaceMenu.contains(event.target)) {
      closeWorkspaceMenu();
    }
  });
  document.addEventListener("scroll", closeWorkspaceMenu, true);
  window.addEventListener("resize", closeWorkspaceMenu);
};




