import { elements } from "./elements.js?v=20251231-03";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { notify } from "./notify.js";
import { syncPromptTools } from "./tools.js?v=20251227-13";
import { buildHeadingHighlightHtml } from "./utils.js?v=20251229-02";

const knowledgeModal = document.getElementById("knowledgeModal");
const knowledgeModalTitle = document.getElementById("knowledgeModalTitle");
const knowledgeModalName = document.getElementById("knowledgeModalName");
const knowledgeModalRoot = document.getElementById("knowledgeModalRoot");
const knowledgeModalDesc = document.getElementById("knowledgeModalDesc");
const knowledgeModalEnabled = document.getElementById("knowledgeModalEnabled");
const knowledgeModalSave = document.getElementById("knowledgeModalSave");
const knowledgeModalCancel = document.getElementById("knowledgeModalCancel");
const knowledgeModalClose = document.getElementById("knowledgeModalClose");
const knowledgeEditBtn = document.getElementById("knowledgeEditBtn");
const knowledgeDetailDesc = document.getElementById("knowledgeDetailDesc");
const knowledgeFileUploadBtn = document.getElementById("knowledgeFileUploadBtn");
const knowledgeFileUploadInput = document.getElementById("knowledgeFileUploadInput");

// 记录当前正在编辑的知识库索引（-1 表示新增）
let knowledgeEditingIndex = -1;

const syncKnowledgeEditorStyles = () => {
  if (!elements.knowledgeFileHighlight || !elements.knowledgeFileContent) {
    return;
  }
  const styles = window.getComputedStyle(elements.knowledgeFileContent);
  elements.knowledgeFileHighlight.style.font = styles.font;
  elements.knowledgeFileHighlight.style.letterSpacing = styles.letterSpacing;
  elements.knowledgeFileHighlight.style.wordSpacing = styles.wordSpacing;
  elements.knowledgeFileHighlight.style.textAlign = styles.textAlign;
  elements.knowledgeFileHighlight.style.textTransform = styles.textTransform;
  elements.knowledgeFileHighlight.style.textIndent = styles.textIndent;
  elements.knowledgeFileHighlight.style.textRendering = styles.textRendering;
  elements.knowledgeFileHighlight.style.whiteSpace = styles.whiteSpace;
  elements.knowledgeFileHighlight.style.wordBreak = styles.wordBreak;
  elements.knowledgeFileHighlight.style.overflowWrap = styles.overflowWrap;
  elements.knowledgeFileHighlight.style.tabSize = styles.tabSize;
  elements.knowledgeFileHighlight.style.direction = styles.direction;
  elements.knowledgeFileHighlight.style.setProperty(
    "--knowledge-editor-padding-top",
    styles.paddingTop
  );
  elements.knowledgeFileHighlight.style.setProperty(
    "--knowledge-editor-padding-right",
    styles.paddingRight
  );
  elements.knowledgeFileHighlight.style.setProperty(
    "--knowledge-editor-padding-bottom",
    styles.paddingBottom
  );
  elements.knowledgeFileHighlight.style.setProperty(
    "--knowledge-editor-padding-left",
    styles.paddingLeft
  );
};

const syncKnowledgeEditorMetrics = () => {
  if (!elements.knowledgeFileHighlight || !elements.knowledgeFileContent) {
    return;
  }
  syncKnowledgeEditorStyles();
  const styles = window.getComputedStyle(elements.knowledgeFileContent);
  const borderX =
    parseFloat(styles.borderLeftWidth) + parseFloat(styles.borderRightWidth);
  const borderY =
    parseFloat(styles.borderTopWidth) + parseFloat(styles.borderBottomWidth);
  const scrollbarWidth = Math.max(
    0,
    elements.knowledgeFileContent.offsetWidth -
      elements.knowledgeFileContent.clientWidth -
      borderX
  );
  const scrollbarHeight = Math.max(
    0,
    elements.knowledgeFileContent.offsetHeight -
      elements.knowledgeFileContent.clientHeight -
      borderY
  );
  // 同步滚动条占位，避免自动换行宽度不一致导致高亮错位
  elements.knowledgeFileHighlight.style.setProperty(
    "--knowledge-scrollbar-width",
    `${scrollbarWidth}px`
  );
  elements.knowledgeFileHighlight.style.setProperty(
    "--knowledge-scrollbar-height",
    `${scrollbarHeight}px`
  );
};

const updateKnowledgeEditorHighlight = () => {
  if (!elements.knowledgeFileHighlight || !elements.knowledgeFileContent) {
    return;
  }
  syncKnowledgeEditorMetrics();
  // 同步渲染一级标题高亮，帮助定位知识条目
  elements.knowledgeFileHighlight.innerHTML = buildHeadingHighlightHtml(
    elements.knowledgeFileContent.value
  );
  syncKnowledgeEditorScroll();
};

const syncKnowledgeEditorScroll = () => {
  if (!elements.knowledgeFileHighlight || !elements.knowledgeFileContent) {
    return;
  }
  elements.knowledgeFileHighlight.scrollTop = elements.knowledgeFileContent.scrollTop;
  elements.knowledgeFileHighlight.scrollLeft = elements.knowledgeFileContent.scrollLeft;
};

// doc2md 支持的扩展名列表（用于前端选择过滤）
const SUPPORTED_UPLOAD_EXTENSIONS = [
  ".txt",
  ".md",
  ".markdown",
  ".html",
  ".htm",
  ".py",
  ".c",
  ".cpp",
  ".cc",
  ".h",
  ".hpp",
  ".json",
  ".js",
  ".ts",
  ".css",
  ".ini",
  ".cfg",
  ".log",
  ".doc",
  ".docx",
  ".odt",
  ".pptx",
  ".odp",
  ".xlsx",
  ".ods",
  ".wps",
  ".et",
  ".dps",
];
const SUPPORTED_UPLOAD_ACCEPT = SUPPORTED_UPLOAD_EXTENSIONS.join(",");

// 规范化知识库配置，确保字段齐全
const normalizeKnowledgeConfig = (raw) => {
  const config = raw || {};
  return {
    bases: Array.isArray(config.bases)
      ? config.bases.map((base) => ({
          name: base.name || "",
          description: base.description || "",
          root: base.root || "",
          enabled: base.enabled !== false,
        }))
      : [],
  };
};

const getActiveBase = () => state.knowledge.bases[state.knowledge.selectedIndex] || null;

// 打开知识库配置弹窗
const openKnowledgeModal = (base = null, index = -1) => {
  if (!knowledgeModal) {
    return;
  }
  knowledgeEditingIndex = Number.isInteger(index) ? index : -1;
  const payload = base || { name: "", description: "", root: "", enabled: true };
  if (knowledgeModalTitle) {
    knowledgeModalTitle.textContent = knowledgeEditingIndex >= 0 ? "编辑知识库" : "新增知识库";
  }
  if (knowledgeModalName) {
    knowledgeModalName.value = payload.name || "";
  }
  if (knowledgeModalRoot) {
    knowledgeModalRoot.value = payload.root || "";
  }
  if (knowledgeModalDesc) {
    knowledgeModalDesc.value = payload.description || "";
  }
  if (knowledgeModalEnabled) {
    knowledgeModalEnabled.checked = payload.enabled !== false;
  }
  knowledgeModal.classList.add("active");
  knowledgeModalName?.focus();
};

// 关闭知识库配置弹窗并清理状态
const closeKnowledgeModal = () => {
  if (!knowledgeModal) {
    return;
  }
  knowledgeModal.classList.remove("active");
  knowledgeEditingIndex = -1;
};

// 从弹窗中读取配置内容
const getKnowledgeModalPayload = () => ({
  name: knowledgeModalName?.value?.trim() || "",
  description: knowledgeModalDesc?.value?.trim() || "",
  root: knowledgeModalRoot?.value?.trim() || "",
  enabled: knowledgeModalEnabled ? knowledgeModalEnabled.checked : true,
});

// 校验单个知识库配置，避免空值或重名
const validateKnowledgeBase = (payload, index) => {
  if (!payload.name) {
    return "请填写知识库名称。";
  }
  for (let i = 0; i < state.knowledge.bases.length; i += 1) {
    if (i === index) {
      continue;
    }
    if (state.knowledge.bases[i].name.trim() === payload.name) {
      return `知识库名称重复：${payload.name}`;
    }
  }
  return "";
};

const renderKnowledgeBaseList = () => {
  elements.knowledgeBaseList.textContent = "";
  if (!state.knowledge.bases.length) {
    elements.knowledgeBaseList.textContent = "暂无知识库配置，请先新增。";
    return;
  }
  state.knowledge.bases.forEach((base, index) => {
    const item = document.createElement("button");
    item.type = "button";
    item.className = "list-item";
    if (index === state.knowledge.selectedIndex) {
      item.classList.add("active");
    }
    const title = base.name || "(未命名知识库)";
    const subtitle = base.root || "未设置目录";
    item.innerHTML = `<div>${title}</div><small>${subtitle}</small>`;
    item.addEventListener("click", async () => {
      state.knowledge.selectedIndex = index;
      state.knowledge.files = [];
      state.knowledge.activeFile = "";
      state.knowledge.fileContent = "";
      renderKnowledgeBaseList();
      renderKnowledgeDetail();
      await loadKnowledgeFiles();
    });
    elements.knowledgeBaseList.appendChild(item);
  });
};

const renderKnowledgeDetailHeader = () => {
  const base = getActiveBase();
  if (!base) {
    elements.knowledgeDetailTitle.textContent = "未选择知识库";
    elements.knowledgeDetailMeta.textContent = "";
    if (knowledgeDetailDesc) {
      knowledgeDetailDesc.textContent = "";
    }
    if (knowledgeEditBtn) {
      knowledgeEditBtn.disabled = true;
    }
    elements.knowledgeDeleteBtn.disabled = true;
    return;
  }
  elements.knowledgeDetailTitle.textContent = base.name || "(未命名知识库)";
  const metaParts = [base.root || "未设置目录"];
  metaParts.push(base.enabled !== false ? "已启用" : "未启用");
  elements.knowledgeDetailMeta.textContent = metaParts.join(" · ");
  if (knowledgeDetailDesc) {
    knowledgeDetailDesc.textContent = base.description || "";
  }
  if (knowledgeEditBtn) {
    knowledgeEditBtn.disabled = false;
  }
  elements.knowledgeDeleteBtn.disabled = false;
};

const renderKnowledgeDetail = () => {
  renderKnowledgeDetailHeader();
  renderKnowledgeFiles();
};

const renderKnowledgeFiles = () => {
  elements.knowledgeFileList.textContent = "";
  if (!state.knowledge.files.length) {
    elements.knowledgeFileList.textContent = "暂无文档，请先刷新列表。";
  } else {
    state.knowledge.files.forEach((filePath) => {
      const item = document.createElement("div");
      item.className = "knowledge-file-item";
      if (filePath === state.knowledge.activeFile) {
        item.classList.add("active");
      }
      const name = document.createElement("span");
      name.className = "knowledge-file-name";
      name.textContent = filePath;
      const deleteBtn = document.createElement("button");
      deleteBtn.type = "button";
      deleteBtn.className = "knowledge-file-delete-btn";
      deleteBtn.title = "删除文档";
      deleteBtn.innerHTML = '<i class="fa-solid fa-trash"></i>';
      deleteBtn.addEventListener("click", async (event) => {
        event.stopPropagation();
        try {
          await deleteKnowledgeFile(filePath);
        } catch (error) {
          notify(`删除失败：${error.message}`, "error");
        }
      });
      item.append(name, deleteBtn);
      item.addEventListener("click", () => {
        selectKnowledgeFile(filePath);
      });
      elements.knowledgeFileList.appendChild(item);
    });
  }
  elements.knowledgeFileName.textContent = state.knowledge.activeFile || "未选择文档";
  elements.knowledgeFileContent.value = state.knowledge.fileContent || "";
  updateKnowledgeEditorHighlight();
};

const buildKnowledgePayload = () => ({
  bases: state.knowledge.bases.map((base) => ({
    name: base.name.trim(),
    description: base.description || "",
    root: base.root.trim(),
    enabled: base.enabled !== false,
  })),
});

const validateKnowledgePayload = (payload) => {
  const invalid = payload.bases.filter((base) => !base.name);
  if (invalid.length) {
    return "存在未填写名称的知识库，请补全后再保存。";
  }
  const nameSet = new Set();
  for (const base of payload.bases) {
    if (nameSet.has(base.name)) {
      return `知识库名称重复：${base.name}`;
    }
    nameSet.add(base.name);
  }
  return "";
};

const saveKnowledgeConfig = async () => {
  const payload = buildKnowledgePayload();
  const error = validateKnowledgePayload(payload);
  if (error) {
    notify(error, "warn");
    return false;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/knowledge`;
  const response = await fetch(endpoint, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ knowledge: payload }),
  });
  if (!response.ok) {
    throw new Error(`请求失败：${response.status}`);
  }
  const result = await response.json();
  const normalized = normalizeKnowledgeConfig(result.knowledge || {});
  state.knowledge.bases = normalized.bases;
  if (!state.knowledge.bases.length) {
    state.knowledge.selectedIndex = -1;
  } else if (state.knowledge.selectedIndex >= state.knowledge.bases.length) {
    state.knowledge.selectedIndex = 0;
  }
  renderKnowledgeDetail();
  renderKnowledgeBaseList();
  syncPromptTools();
  return true;
};

const normalizeUploadExtension = (filename) => {
  const parts = String(filename || "").trim().split(".");
  if (parts.length <= 1) {
    return "";
  }
  return `.${parts.pop().toLowerCase()}`;
};

const uploadKnowledgeFile = async (file) => {
  const base = getActiveBase();
  if (!base || !base.name) {
    notify("请先选择知识库。", "warn");
    return;
  }
  if (!file) {
    return;
  }
  const extension = normalizeUploadExtension(file.name);
  if (!extension) {
    notify("文件缺少扩展名。", "warn");
    return;
  }
  if (!SUPPORTED_UPLOAD_EXTENSIONS.includes(extension)) {
    notify(`不支持的文件类型：${extension}`, "warn");
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/knowledge/upload`;
  const formData = new FormData();
  formData.append("base", base.name);
  formData.append("file", file, file.name);
  const response = await fetch(endpoint, {
    method: "POST",
    body: formData,
  });
  if (!response.ok) {
    throw new Error(`上传失败：${response.status}`);
  }
  const result = await response.json();
  await loadKnowledgeFiles();
  if (result?.path) {
    await selectKnowledgeFile(result.path);
  }
  notify(`上传完成：${result?.path || file.name}`, "success");
  const warnings = Array.isArray(result?.warnings) ? result.warnings : [];
  if (warnings.length) {
    notify(`转换警告：${warnings.join(" | ")}`, "warn");
  }
};

const loadKnowledgeFiles = async () => {
  const base = getActiveBase();
  if (!base || !base.name || !base.root) {
    state.knowledge.files = [];
    state.knowledge.activeFile = "";
    state.knowledge.fileContent = "";
    renderKnowledgeFiles();
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/knowledge/files?base=${encodeURIComponent(base.name)}`;
  elements.knowledgeFileList.textContent = "加载中...";
  const response = await fetch(endpoint);
  if (!response.ok) {
    elements.knowledgeFileList.textContent = `加载失败：${response.status}`;
    return;
  }
  const result = await response.json();
  state.knowledge.files = Array.isArray(result.files) ? result.files : [];
  if (!state.knowledge.files.includes(state.knowledge.activeFile)) {
    state.knowledge.activeFile = "";
    state.knowledge.fileContent = "";
  }
  renderKnowledgeFiles();
};

const selectKnowledgeFile = async (filePath) => {
  const base = getActiveBase();
  if (!base || !base.name) {
    notify("请先选择知识库。", "warn");
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/knowledge/file?base=${encodeURIComponent(
    base.name
  )}&path=${encodeURIComponent(filePath)}`;
  elements.knowledgeFileName.textContent = "加载中...";
  const response = await fetch(endpoint);
  if (!response.ok) {
    notify(`读取失败：${response.status}`, "error");
    return;
  }
  const result = await response.json();
  state.knowledge.activeFile = result.path || filePath;
  state.knowledge.fileContent = result.content || "";
  renderKnowledgeFiles();
};

const saveKnowledgeFile = async () => {
  const base = getActiveBase();
  if (!base || !base.name) {
    notify("请先选择知识库。", "warn");
    return;
  }
  if (!state.knowledge.activeFile) {
    notify("请先选择要保存的文档。", "warn");
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/knowledge/file`;
  const response = await fetch(endpoint, {
    method: "PUT",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      base: base.name,
      path: state.knowledge.activeFile,
      content: elements.knowledgeFileContent.value,
    }),
  });
  if (!response.ok) {
    throw new Error(`请求失败：${response.status}`);
  }
  await loadKnowledgeFiles();
  notify("文档已保存并刷新索引。", "success");
};

const createKnowledgeFile = async () => {
  const base = getActiveBase();
  if (!base || !base.name) {
    notify("请先选择知识库。", "warn");
    return;
  }
  const filename = window.prompt("请输入新文档文件名（.md）", "example.md");
  if (!filename) {
    return;
  }
  const trimmed = filename.trim();
  if (!trimmed) {
    return;
  }
  const target = trimmed.endsWith(".md") ? trimmed : `${trimmed}.md`;
  state.knowledge.activeFile = target;
  state.knowledge.fileContent = "";
  elements.knowledgeFileContent.value = "";
  await saveKnowledgeFile();
};

// 支持从列表项直接删除指定文档
const deleteKnowledgeFile = async (targetPath = "") => {
  const base = getActiveBase();
  if (!base || !base.name) {
    notify("请先选择知识库。", "warn");
    return;
  }
  const path = targetPath || state.knowledge.activeFile;
  if (!path) {
    notify("请先选择要删除的文档。", "warn");
    return;
  }
  if (!window.confirm(`确认删除文档 ${path}？`)) {
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/knowledge/file?base=${encodeURIComponent(
    base.name
  )}&path=${encodeURIComponent(path)}`;
  const response = await fetch(endpoint, { method: "DELETE" });
  if (!response.ok) {
    throw new Error(`请求失败：${response.status}`);
  }
  if (path === state.knowledge.activeFile) {
    state.knowledge.activeFile = "";
    state.knowledge.fileContent = "";
  }
  await loadKnowledgeFiles();
  notify("文档已删除。", "success");
};

const addKnowledgeBase = () => {
  openKnowledgeModal();
};

const editKnowledgeBase = () => {
  const base = getActiveBase();
  if (!base) {
    notify("请先选择知识库。", "warn");
    return;
  }
  openKnowledgeModal(base, state.knowledge.selectedIndex);
};

const applyKnowledgeModal = async () => {
  const payload = getKnowledgeModalPayload();
  const error = validateKnowledgeBase(payload, knowledgeEditingIndex);
  if (error) {
    notify(error, "warn");
    return;
  }
  const snapshot = {
    bases: state.knowledge.bases.map((base) => ({ ...base })),
    selectedIndex: state.knowledge.selectedIndex,
    files: [...state.knowledge.files],
    activeFile: state.knowledge.activeFile,
    fileContent: state.knowledge.fileContent,
  };
  if (knowledgeEditingIndex >= 0) {
    state.knowledge.bases[knowledgeEditingIndex] = { ...payload };
    state.knowledge.selectedIndex = knowledgeEditingIndex;
  } else {
    state.knowledge.bases.push({ ...payload });
    state.knowledge.selectedIndex = state.knowledge.bases.length - 1;
  }
  state.knowledge.files = [];
  state.knowledge.activeFile = "";
  state.knowledge.fileContent = "";
  renderKnowledgeBaseList();
  renderKnowledgeDetail();
  try {
    const saved = await saveKnowledgeConfig();
    if (!saved) {
      state.knowledge.bases = snapshot.bases;
      state.knowledge.selectedIndex = snapshot.selectedIndex;
      state.knowledge.files = snapshot.files;
      state.knowledge.activeFile = snapshot.activeFile;
      state.knowledge.fileContent = snapshot.fileContent;
      renderKnowledgeBaseList();
      renderKnowledgeDetail();
      return;
    }
    await loadKnowledgeFiles();
    notify(knowledgeEditingIndex >= 0 ? "知识库已更新。" : "知识库已新增。", "success");
    closeKnowledgeModal();
  } catch (error) {
    state.knowledge.bases = snapshot.bases;
    state.knowledge.selectedIndex = snapshot.selectedIndex;
    state.knowledge.files = snapshot.files;
    state.knowledge.activeFile = snapshot.activeFile;
    state.knowledge.fileContent = snapshot.fileContent;
    renderKnowledgeBaseList();
    renderKnowledgeDetail();
    notify(`保存失败：${error.message}`, "error");
  }
};

const deleteKnowledgeBase = async () => {
  if (state.knowledge.selectedIndex < 0) {
    return;
  }
  const base = getActiveBase();
  const baseName = base && base.name ? base.name : "未命名知识库";
  if (!window.confirm(`确认删除知识库 ${baseName}？`)) {
    return;
  }
  const snapshot = {
    bases: state.knowledge.bases.map((item) => ({ ...item })),
    selectedIndex: state.knowledge.selectedIndex,
    files: [...state.knowledge.files],
    activeFile: state.knowledge.activeFile,
    fileContent: state.knowledge.fileContent,
  };
  state.knowledge.bases.splice(state.knowledge.selectedIndex, 1);
  if (!state.knowledge.bases.length) {
    state.knowledge.selectedIndex = -1;
  } else {
    state.knowledge.selectedIndex = Math.max(0, state.knowledge.selectedIndex - 1);
  }
  state.knowledge.files = [];
  state.knowledge.activeFile = "";
  state.knowledge.fileContent = "";
  renderKnowledgeBaseList();
  renderKnowledgeDetail();
  try {
    const saved = await saveKnowledgeConfig();
    if (!saved) {
      state.knowledge.bases = snapshot.bases;
      state.knowledge.selectedIndex = snapshot.selectedIndex;
      state.knowledge.files = snapshot.files;
      state.knowledge.activeFile = snapshot.activeFile;
      state.knowledge.fileContent = snapshot.fileContent;
      renderKnowledgeBaseList();
      renderKnowledgeDetail();
      return;
    }
    await loadKnowledgeFiles();
    notify("知识库已删除。", "success");
  } catch (error) {
    state.knowledge.bases = snapshot.bases;
    state.knowledge.selectedIndex = snapshot.selectedIndex;
    state.knowledge.files = snapshot.files;
    state.knowledge.activeFile = snapshot.activeFile;
    state.knowledge.fileContent = snapshot.fileContent;
    renderKnowledgeBaseList();
    renderKnowledgeDetail();
    notify(`删除失败：${error.message}`, "error");
  }
};

export const loadKnowledgeConfig = async () => {
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/knowledge`;
  const response = await fetch(endpoint);
  if (!response.ok) {
    throw new Error(`请求失败：${response.status}`);
  }
  const result = await response.json();
  const normalized = normalizeKnowledgeConfig(result.knowledge || {});
  state.knowledge.bases = normalized.bases;
  state.knowledge.selectedIndex = state.knowledge.bases.length ? 0 : -1;
  state.knowledge.files = [];
  state.knowledge.activeFile = "";
  state.knowledge.fileContent = "";
  renderKnowledgeBaseList();
  renderKnowledgeDetail();
  await loadKnowledgeFiles();
};

export const initKnowledgePanel = () => {
  elements.knowledgeAddBtn?.addEventListener("click", addKnowledgeBase);
  knowledgeEditBtn?.addEventListener("click", editKnowledgeBase);
  elements.knowledgeDeleteBtn?.addEventListener("click", () => {
    deleteKnowledgeBase().catch((error) => notify(`删除失败：${error.message}`, "error"));
  });
  elements.knowledgeRefreshBtn?.addEventListener("click", async () => {
    try {
      await loadKnowledgeConfig();
      notify("知识库配置已刷新。", "success");
    } catch (error) {
      notify(`刷新失败：${error.message}`, "error");
    }
  });
  knowledgeModalSave?.addEventListener("click", () => {
    applyKnowledgeModal();
  });
  knowledgeModalCancel?.addEventListener("click", closeKnowledgeModal);
  knowledgeModalClose?.addEventListener("click", closeKnowledgeModal);
  knowledgeModal?.addEventListener("click", (event) => {
    if (event.target === knowledgeModal) {
      closeKnowledgeModal();
    }
  });

  if (knowledgeFileUploadInput) {
    knowledgeFileUploadInput.accept = SUPPORTED_UPLOAD_ACCEPT;
  }
  knowledgeFileUploadBtn?.addEventListener("click", () => {
    const base = getActiveBase();
    if (!base || !base.name) {
      notify("请先选择知识库。", "warn");
      return;
    }
    if (knowledgeFileUploadInput) {
      knowledgeFileUploadInput.value = "";
      knowledgeFileUploadInput.click();
    }
  });
  knowledgeFileUploadInput?.addEventListener("change", async () => {
    const file = knowledgeFileUploadInput.files?.[0];
    if (!file) {
      return;
    }
    try {
      await uploadKnowledgeFile(file);
    } catch (error) {
      notify(`上传失败：${error.message}`, "error");
    }
  });
  elements.knowledgeFileContent?.addEventListener("input", () => {
    state.knowledge.fileContent = elements.knowledgeFileContent.value;
    updateKnowledgeEditorHighlight();
  });
  elements.knowledgeFileContent?.addEventListener("scroll", syncKnowledgeEditorScroll);
  window.addEventListener("resize", updateKnowledgeEditorHighlight);

  elements.knowledgeFileSaveBtn?.addEventListener("click", async () => {
    try {
      await saveKnowledgeFile();
    } catch (error) {
      notify(`保存失败：${error.message}`, "error");
    }
  });
  elements.knowledgeFileNewBtn?.addEventListener("click", async () => {
    try {
      await createKnowledgeFile();
    } catch (error) {
      notify(`新建失败：${error.message}`, "error");
    }
  });
};




