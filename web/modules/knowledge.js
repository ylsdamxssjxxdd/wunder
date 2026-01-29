import { elements } from "./elements.js?v=20260118-07";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { notify } from "./notify.js";
import { syncPromptTools } from "./tools.js?v=20251227-13";
import { buildHeadingHighlightHtml, escapeHtml, formatTimestamp } from "./utils.js?v=20251229-02";
import { t } from "./i18n.js?v=20260118-07";

const knowledgeModal = document.getElementById("knowledgeModal");
const knowledgeModalTitle = document.getElementById("knowledgeModalTitle");
const knowledgeModalName = document.getElementById("knowledgeModalName");
const knowledgeModalTypeRow = document.getElementById("knowledgeModalTypeRow");
const knowledgeModalType = document.getElementById("knowledgeModalType");
const knowledgeModalEmbeddingRow = document.getElementById("knowledgeModalEmbeddingRow");
const knowledgeModalEmbeddingModel = document.getElementById("knowledgeModalEmbeddingModel");
const knowledgeModalChunkRow = document.getElementById("knowledgeModalChunkRow");
const knowledgeModalChunkSize = document.getElementById("knowledgeModalChunkSize");
const knowledgeModalChunkOverlap = document.getElementById("knowledgeModalChunkOverlap");
const knowledgeModalSearchRow = document.getElementById("knowledgeModalSearchRow");
const knowledgeModalTopK = document.getElementById("knowledgeModalTopK");
const knowledgeModalScoreThreshold = document.getElementById("knowledgeModalScoreThreshold");
const knowledgeModalRoot = document.getElementById("knowledgeModalRoot");
const knowledgeModalRootHint = document.getElementById("knowledgeModalRootHint");
const knowledgeModalDesc = document.getElementById("knowledgeModalDesc");
const knowledgeModalEnabled = document.getElementById("knowledgeModalEnabled");
const knowledgeModalSave = document.getElementById("knowledgeModalSave");
const knowledgeModalCancel = document.getElementById("knowledgeModalCancel");
const knowledgeModalClose = document.getElementById("knowledgeModalClose");
const knowledgeChunkModal = document.getElementById("knowledgeChunkModal");
const knowledgeChunkModalTitle = document.getElementById("knowledgeChunkModalTitle");
const knowledgeChunkModalContent = document.getElementById("knowledgeChunkModalContent");
const knowledgeChunkModalSave = document.getElementById("knowledgeChunkModalSave");
const knowledgeChunkModalCancel = document.getElementById("knowledgeChunkModalCancel");
const knowledgeChunkModalClose = document.getElementById("knowledgeChunkModalClose");
const knowledgeDocModal = document.getElementById("knowledgeDocModal");
const knowledgeDocModalTitle = document.getElementById("knowledgeDocModalTitle");
const knowledgeDocModalMeta = document.getElementById("knowledgeDocModalMeta");
const knowledgeDocModalContent = document.getElementById("knowledgeDocModalContent");
const knowledgeDocModalClose = document.getElementById("knowledgeDocModalClose");
const knowledgeDocModalCloseBtn = document.getElementById("knowledgeDocModalCloseBtn");
const knowledgeTestModal = document.getElementById("knowledgeTestModal");
const knowledgeTestModalTitle = document.getElementById("knowledgeTestModalTitle");
const knowledgeTestModalClose = document.getElementById("knowledgeTestModalClose");
const knowledgeTestModalCloseBtn = document.getElementById("knowledgeTestModalCloseBtn");
const knowledgeTestQuestion = document.getElementById("knowledgeTestQuestion");
const knowledgeTestRunBtn = document.getElementById("knowledgeTestRunBtn");
const knowledgeTestStatus = document.getElementById("knowledgeTestStatus");
const knowledgeTestResult = document.getElementById("knowledgeTestResult");
const knowledgeTestBtn = document.getElementById("knowledgeTestBtn");
const knowledgeDocChunkSelectAllBtn = document.getElementById("knowledgeDocChunkSelectAllBtn");
const knowledgeEditBtn = document.getElementById("knowledgeEditBtn");
const knowledgeDetailDesc = document.getElementById("knowledgeDetailDesc");
const knowledgeFileUploadBtn = document.getElementById("knowledgeFileUploadBtn");
const knowledgeFileUploadInput = document.getElementById("knowledgeFileUploadInput");

// 记录当前正在编辑的知识库索引（-1 表示新增）
let knowledgeEditingIndex = -1;
let editingChunkIndex = null;

const resetVectorState = () => {
  state.knowledge.vectorDocs = [];
  state.knowledge.activeDocId = "";
  state.knowledge.docContent = "";
  state.knowledge.docMeta = null;
  state.knowledge.docChunks = [];
  state.knowledge.selectedChunkIndices = new Set();
  state.knowledge.embeddingChunkIndices = new Set();
};

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
  ".pdf",
  ".pptx",
  ".odp",
  ".xlsx",
  ".ods",
  ".wps",
  ".et",
  ".dps",
];
const SUPPORTED_UPLOAD_ACCEPT = SUPPORTED_UPLOAD_EXTENSIONS.join(",");
const DEFAULT_ROOT_PLACEHOLDER =
  knowledgeModalRoot?.getAttribute("placeholder") || "可留空，自动创建 ./knowledge/<名称>";

const normalizeBaseType = (value) => {
  const raw = String(value || "").trim().toLowerCase();
  if (!raw) {
    return "literal";
  }
  if (raw === "vector" || raw === "embedding") {
    return "vector";
  }
  return "literal";
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

const isVectorBase = (base) => normalizeBaseType(base?.base_type) === "vector";

const parseOptionalInt = (value) => {
  if (value === null || value === undefined || value === "") {
    return null;
  }
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) ? parsed : null;
};

const parseOptionalFloat = (value) => {
  if (value === null || value === undefined || value === "") {
    return null;
  }
  const parsed = Number.parseFloat(value);
  return Number.isFinite(parsed) ? parsed : null;
};

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
          base_type: normalizeBaseType(base.base_type),
          embedding_model: base.embedding_model || "",
          chunk_size: parseOptionalInt(base.chunk_size),
          chunk_overlap: parseOptionalInt(base.chunk_overlap),
          top_k: parseOptionalInt(base.top_k),
          score_threshold: parseOptionalFloat(base.score_threshold),
        }))
      : [],
  };
};

const getActiveBase = () => state.knowledge.bases[state.knowledge.selectedIndex] || null;

const formatDocStatus = (status) => {
  const normalized = String(status || "").trim().toLowerCase();
  if (!normalized) {
    return "-";
  }
  const key = `knowledge.doc.status.${normalized}`;
  const localized = t(key);
  return localized === key ? normalized : localized;
};

const formatDocUpdatedAt = (timestamp) => {
  if (!Number.isFinite(timestamp)) {
    return "";
  }
  return formatTimestamp(timestamp * 1000);
};

const buildDocMetaText = (meta) => {
  if (!meta) {
    return "";
  }
  const parts = [];
  if (meta.embedding_model) {
    parts.push(t("knowledge.doc.meta.embedding", { name: meta.embedding_model }));
  }
  if (Number.isFinite(meta.chunk_count)) {
    parts.push(t("knowledge.doc.meta.chunks", { count: meta.chunk_count }));
  }
  if (Number.isFinite(meta.updated_at)) {
    const formatted = formatDocUpdatedAt(meta.updated_at);
    if (formatted) {
      parts.push(t("knowledge.doc.meta.updated", { time: formatted }));
    }
  }
  if (meta.status) {
    parts.push(formatDocStatus(meta.status));
  }
  return parts.join(" · ");
};

const applyKnowledgeModalType = (baseType) => {
  const type = normalizeBaseType(baseType);
  const isVector = type === "vector";
  const toggleRow = (element, visible, displayStyle = "") => {
    if (!element) {
      return;
    }
    element.hidden = !visible;
    element.style.display = visible ? displayStyle : "none";
  };
  const setInputEnabled = (element, enabled) => {
    if (!element) {
      return;
    }
    element.disabled = !enabled;
  };
  if (knowledgeModalType) {
    knowledgeModalType.value = type;
  }
  toggleRow(knowledgeModalEmbeddingRow, isVector, "");
  toggleRow(knowledgeModalChunkRow, isVector, "grid");
  toggleRow(knowledgeModalSearchRow, isVector, "grid");
  setInputEnabled(knowledgeModalEmbeddingModel, isVector);
  setInputEnabled(knowledgeModalChunkSize, isVector);
  setInputEnabled(knowledgeModalChunkOverlap, isVector);
  setInputEnabled(knowledgeModalTopK, isVector);
  setInputEnabled(knowledgeModalScoreThreshold, isVector);
  if (knowledgeModalRoot) {
    knowledgeModalRoot.disabled = isVector;
    knowledgeModalRoot.placeholder = isVector
      ? t("knowledge.modal.placeholder.vectorRoot")
      : DEFAULT_ROOT_PLACEHOLDER;
  }
  if (knowledgeModalRootHint) {
    knowledgeModalRootHint.hidden = !isVector;
    knowledgeModalRootHint.style.display = isVector ? "" : "none";
  }
};

const renderEmbeddingModelOptions = (selected = "") => {
  if (!knowledgeModalEmbeddingModel) {
    return;
  }
  const current = selected || knowledgeModalEmbeddingModel.value;
  knowledgeModalEmbeddingModel.textContent = "";
  if (!state.knowledge.embeddingModels.length) {
    const option = document.createElement("option");
    option.value = "";
    option.textContent = t("knowledge.embedding.empty");
    knowledgeModalEmbeddingModel.appendChild(option);
    knowledgeModalEmbeddingModel.disabled = true;
    return;
  }
  knowledgeModalEmbeddingModel.disabled = false;
  const models = [...state.knowledge.embeddingModels];
  if (current && !models.includes(current)) {
    models.unshift(current);
  }
  models.forEach((name) => {
    const option = document.createElement("option");
    option.value = name;
    option.textContent = name;
    knowledgeModalEmbeddingModel.appendChild(option);
  });
  if (current && knowledgeModalEmbeddingModel.querySelector(`option[value="${current}"]`)) {
    knowledgeModalEmbeddingModel.value = current;
  } else {
    knowledgeModalEmbeddingModel.value = models[0] || "";
  }
};

const loadEmbeddingModels = async (force = false) => {
  if (!force && state.knowledge.embeddingModels.length) {
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/llm`;
  const response = await fetch(endpoint);
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const result = await response.json();
  const models = result?.llm?.models || {};
  const embeddingModels = Object.entries(models)
    .filter(([, config]) => normalizeModelType(config?.model_type) === "embedding")
    .map(([name]) => name);
  embeddingModels.sort();
  state.knowledge.embeddingModels = embeddingModels;
};

// 打开知识库配置弹窗
const openKnowledgeModal = (base = null, index = -1) => {
  if (!knowledgeModal) {
    return;
  }
  knowledgeEditingIndex = Number.isInteger(index) ? index : -1;
  const payload = base || {
    name: "",
    description: "",
    root: "",
    enabled: true,
    base_type: "literal",
    embedding_model: "",
    chunk_size: null,
    chunk_overlap: null,
    top_k: null,
    score_threshold: null,
  };
  const baseType = normalizeBaseType(payload.base_type);
  if (knowledgeModalTitle) {
    knowledgeModalTitle.textContent =
      knowledgeEditingIndex >= 0
        ? t("knowledge.modal.editTitle")
        : t("knowledge.modal.addTitle");
  }
  if (knowledgeModalName) {
    knowledgeModalName.value = payload.name || "";
  }
  applyKnowledgeModalType(baseType);
  if (knowledgeModalEmbeddingModel) {
    knowledgeModalEmbeddingModel.value = payload.embedding_model || "";
  }
  if (knowledgeModalChunkSize) {
    knowledgeModalChunkSize.value =
      payload.chunk_size !== null && payload.chunk_size !== undefined
        ? payload.chunk_size
        : "";
  }
  if (knowledgeModalChunkOverlap) {
    knowledgeModalChunkOverlap.value =
      payload.chunk_overlap !== null && payload.chunk_overlap !== undefined
        ? payload.chunk_overlap
        : "";
  }
  if (knowledgeModalTopK) {
    knowledgeModalTopK.value =
      payload.top_k !== null && payload.top_k !== undefined ? payload.top_k : "";
  }
  if (knowledgeModalScoreThreshold) {
    knowledgeModalScoreThreshold.value =
      payload.score_threshold !== null && payload.score_threshold !== undefined
        ? payload.score_threshold
        : "";
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
  if (baseType === "vector") {
    loadEmbeddingModels(true)
      .then(() => {
        renderEmbeddingModelOptions(payload.embedding_model || "");
      })
      .catch((error) => {
        notify(t("knowledge.embedding.empty"), "warn");
        console.warn(error);
      });
  } else {
    renderEmbeddingModelOptions(payload.embedding_model || "");
  }
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
const getKnowledgeModalPayload = () => {
  const baseType = normalizeBaseType(knowledgeModalType?.value);
  const isVector = baseType === "vector";
  return {
    name: knowledgeModalName?.value?.trim() || "",
    description: knowledgeModalDesc?.value?.trim() || "",
    root: knowledgeModalRoot?.value?.trim() || "",
    enabled: knowledgeModalEnabled ? knowledgeModalEnabled.checked : true,
    base_type: baseType,
    embedding_model: isVector ? knowledgeModalEmbeddingModel?.value?.trim() || "" : "",
    chunk_size: isVector ? parseOptionalInt(knowledgeModalChunkSize?.value) : null,
    chunk_overlap: isVector ? parseOptionalInt(knowledgeModalChunkOverlap?.value) : null,
    top_k: isVector ? parseOptionalInt(knowledgeModalTopK?.value) : null,
    score_threshold: isVector ? parseOptionalFloat(knowledgeModalScoreThreshold?.value) : null,
  };
};

// 校验单个知识库配置，避免空值或重名
const validateKnowledgeBase = (payload, index) => {
  if (!payload.name) {
    return t("knowledge.name.required");
  }
  if (normalizeBaseType(payload.base_type) === "vector" && !payload.embedding_model) {
    return t("knowledge.embedding.required");
  }
  for (let i = 0; i < state.knowledge.bases.length; i += 1) {
    if (i === index) {
      continue;
    }
    if (state.knowledge.bases[i].name.trim() === payload.name) {
      return t("knowledge.name.duplicate", { name: payload.name });
    }
  }
  return "";
};

const renderKnowledgeBaseList = () => {
  elements.knowledgeBaseList.textContent = "";
  if (!state.knowledge.bases.length) {
    elements.knowledgeBaseList.textContent = t("knowledge.list.empty");
    return;
  }
  state.knowledge.bases.forEach((base, index) => {
    const item = document.createElement("button");
    item.type = "button";
    item.className = "list-item";
    if (index === state.knowledge.selectedIndex) {
      item.classList.add("active");
    }
    const title = base.name || t("knowledge.name.unnamed");
    const meta = [];
    meta.push(base.root || t("knowledge.root.unset"));
    const isVector = isVectorBase(base);
    const typeLabel = isVector ? t("knowledge.type.vector") : t("knowledge.type.literal");
    meta.push(typeLabel);
    if (isVector && base.embedding_model) {
      meta.push(base.embedding_model);
    }
    item.innerHTML = "";
    const titleWrap = document.createElement("div");
    titleWrap.className = "knowledge-list-item-title";
    const icon = document.createElement("i");
    icon.className = `fa-solid ${
      isVector ? "fa-diagram-project" : "fa-file-lines"
    } knowledge-type-icon ${isVector ? "is-vector" : "is-literal"}`;
    const titleText = document.createElement("span");
    titleText.className = "knowledge-list-item-name";
    titleText.textContent = title;
    titleWrap.append(icon, titleText);
    const metaText = document.createElement("small");
    metaText.textContent = meta.join(" · ");
    item.append(titleWrap, metaText);
    item.addEventListener("click", async () => {
      state.knowledge.selectedIndex = index;
      state.knowledge.files = [];
      state.knowledge.activeFile = "";
      state.knowledge.fileContent = "";
      resetVectorState();
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
    elements.knowledgeDetailTitle.textContent = t("knowledge.detail.empty");
    elements.knowledgeDetailMeta.textContent = "";
    if (knowledgeDetailDesc) {
      knowledgeDetailDesc.textContent = "";
    }
    if (knowledgeEditBtn) {
      knowledgeEditBtn.disabled = true;
    }
    if (knowledgeTestBtn) {
      knowledgeTestBtn.disabled = true;
      knowledgeTestBtn.style.display = "none";
    }
    elements.knowledgeDeleteBtn.disabled = true;
    return;
  }
  elements.knowledgeDetailTitle.textContent = base.name || t("knowledge.name.unnamed");
  const metaParts = [base.root || t("knowledge.root.unset")];
  metaParts.push(base.enabled !== false ? t("knowledge.status.enabled") : t("knowledge.status.disabled"));
  metaParts.push(isVectorBase(base) ? t("knowledge.type.vector") : t("knowledge.type.literal"));
  if (isVectorBase(base) && base.embedding_model) {
    metaParts.push(base.embedding_model);
  }
  elements.knowledgeDetailMeta.textContent = metaParts.join(" · ");
  if (knowledgeDetailDesc) {
    knowledgeDetailDesc.textContent = base.description || "";
  }
  if (knowledgeEditBtn) {
    knowledgeEditBtn.disabled = false;
  }
  if (knowledgeTestBtn) {
    const isVector = isVectorBase(base);
    knowledgeTestBtn.disabled = !isVector;
    knowledgeTestBtn.style.display = isVector ? "" : "none";
  }
  elements.knowledgeDeleteBtn.disabled = false;
};

const renderKnowledgeDetail = () => {
  renderKnowledgeDetailHeader();
  const base = getActiveBase();
  const vectorMode = base && isVectorBase(base);
  const toggleLayout = (element, visible, displayStyle) => {
    if (!element) {
      return;
    }
    element.hidden = !visible;
    element.style.display = visible ? displayStyle : "none";
  };
  toggleLayout(elements.knowledgeFileLayout, !vectorMode, "grid");
  toggleLayout(elements.knowledgeVectorLayout, vectorMode, "grid");
  const toggleButton = (element, visible) => {
    if (!element) {
      return;
    }
    element.disabled = !visible;
    element.style.display = visible ? "" : "none";
  };
  const showLiteralActions = Boolean(base && !vectorMode);
  const showVectorActions = Boolean(base && vectorMode);
  toggleButton(knowledgeFileUploadBtn, showLiteralActions);
  toggleButton(elements.knowledgeFileNewBtn, showLiteralActions);
  toggleButton(elements.knowledgeFileSaveBtn, showLiteralActions);
  toggleButton(elements.knowledgeDocUploadBtn, showVectorActions);
  toggleButton(elements.knowledgeDocRebuildAllBtn, showVectorActions);
  if (vectorMode) {
    renderVectorDocList();
    renderVectorDocDetail();
  } else {
    renderKnowledgeFiles();
  }
};

const renderKnowledgeFiles = () => {
  elements.knowledgeFileList.textContent = "";
  if (!state.knowledge.files.length) {
    elements.knowledgeFileList.textContent = t("knowledge.file.empty");
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
      deleteBtn.title = t("knowledge.file.delete");
      deleteBtn.innerHTML = '<i class="fa-solid fa-trash"></i>';
      deleteBtn.addEventListener("click", async (event) => {
        event.stopPropagation();
        try {
          await deleteKnowledgeFile(filePath);
        } catch (error) {
          notify(t("knowledge.file.deleteFailed", { message: error.message }), "error");
        }
      });
      item.append(name, deleteBtn);
      item.addEventListener("click", () => {
        selectKnowledgeFile(filePath);
      });
      elements.knowledgeFileList.appendChild(item);
    });
  }
  elements.knowledgeFileName.textContent = state.knowledge.activeFile || t("knowledge.file.none");
  elements.knowledgeFileContent.value = state.knowledge.fileContent || "";
  updateKnowledgeEditorHighlight();
};

const renderVectorDocList = () => {
  if (!elements.knowledgeDocList) {
    return;
  }
  elements.knowledgeDocList.textContent = "";
  if (!state.knowledge.vectorDocs.length) {
    elements.knowledgeDocList.textContent = t("knowledge.doc.list.empty");
    return;
  }
  state.knowledge.vectorDocs.forEach((doc) => {
    const item = document.createElement("div");
    item.className = "knowledge-doc-item";
    if (doc.doc_id === state.knowledge.activeDocId) {
      item.classList.add("active");
    }
    const header = document.createElement("div");
    header.className = "knowledge-doc-item-header";
    const title = document.createElement("div");
    title.className = "knowledge-doc-title";
    title.textContent = doc.name || doc.doc_id || t("knowledge.doc.none");
    const deleteBtn = document.createElement("button");
    deleteBtn.type = "button";
    deleteBtn.className = "knowledge-doc-delete-btn";
    deleteBtn.title = t("knowledge.doc.action.delete");
    deleteBtn.innerHTML = '<i class="fa-solid fa-trash"></i>';
    deleteBtn.addEventListener("click", (event) => {
      event.stopPropagation();
      deleteVectorDoc(doc.doc_id).catch((error) => {
        notify(t("knowledge.doc.deleteFailed", { message: error.message }), "error");
      });
    });
    header.append(title, deleteBtn);
    const meta = document.createElement("div");
    meta.className = "knowledge-doc-meta";
    meta.textContent = buildDocMetaText(doc);
    item.append(header, meta);
    item.addEventListener("click", () => {
      selectVectorDoc(doc.doc_id);
    });
    elements.knowledgeDocList.appendChild(item);
  });
};

const buildHighlightedContent = (content, chunk) => {
  if (!chunk) {
    return escapeHtml(content);
  }
  const chars = Array.from(content);
  const start = Math.min(Math.max(chunk.start ?? 0, 0), chars.length);
  const end = Math.min(Math.max(chunk.end ?? start, start), chars.length);
  const before = chars.slice(0, start).join("");
  const target = chars.slice(start, end).join("");
  const after = chars.slice(end).join("");
  return `${escapeHtml(before)}<mark>${escapeHtml(target)}</mark>${escapeHtml(after)}`;
};

const getSelectedChunkIndices = () =>
  Array.from(state.knowledge.selectedChunkIndices || []);

const renderDocModalContent = () => {
  if (!knowledgeDocModalContent) {
    return;
  }
  const content = state.knowledge.docContent || "";
  if (!content) {
    knowledgeDocModalContent.textContent = t("knowledge.doc.content.empty");
    return;
  }
  const selected = getSelectedChunkIndices();
  const chunk =
    selected.length === 1
      ? state.knowledge.docChunks.find((item) => item.index === selected[0])
      : null;
  knowledgeDocModalContent.innerHTML = buildHighlightedContent(content, chunk);
};

const openDocModal = () => {
  if (!knowledgeDocModal) {
    return;
  }
  if (!state.knowledge.docMeta) {
    notify(t("knowledge.doc.none"), "warn");
    return;
  }
  if (knowledgeDocModalTitle) {
    knowledgeDocModalTitle.textContent =
      state.knowledge.docMeta.name || t("knowledge.doc.modal.title");
  }
  if (knowledgeDocModalMeta) {
    knowledgeDocModalMeta.textContent = buildDocMetaText(state.knowledge.docMeta);
  }
  renderDocModalContent();
  knowledgeDocModal.classList.add("active");
};

const closeDocModal = () => {
  if (!knowledgeDocModal) {
    return;
  }
  knowledgeDocModal.classList.remove("active");
};

const resolveChunkStatus = (chunk) => {
  if (state.knowledge.embeddingChunkIndices?.has(chunk?.index)) {
    return "embedding";
  }
  const raw = String(chunk?.status || "").trim().toLowerCase();
  return raw || "pending";
};

const formatChunkStatus = (chunk) => {
  const status = resolveChunkStatus(chunk);
  const key = `knowledge.chunk.status.${status}`;
  const label = t(key);
  return label === key ? status : label;
};

const renderVectorDocChunks = () => {
  if (!elements.knowledgeDocChunks) {
    return;
  }
  elements.knowledgeDocChunks.textContent = "";
  if (!state.knowledge.docChunks.length) {
    elements.knowledgeDocChunks.textContent = t("knowledge.chunk.empty");
    return;
  }
  state.knowledge.docChunks.forEach((chunk) => {
    const item = document.createElement("div");
    item.className = "knowledge-doc-chunk-item";
    item.dataset.index = chunk.index;
    const titleRow = document.createElement("div");
    titleRow.className = "knowledge-doc-chunk-title-row";
    const title = document.createElement("div");
    title.className = "knowledge-doc-chunk-title";
    const selectMark = document.createElement("span");
    selectMark.className = "knowledge-doc-chunk-select";
    const titleText = document.createElement("span");
    titleText.textContent = `#${chunk.index} ${chunk.start}-${chunk.end}`;
    title.append(selectMark, titleText);
    const status = document.createElement("span");
    status.className = `knowledge-doc-chunk-status status-${resolveChunkStatus(chunk)}`;
    status.textContent = formatChunkStatus(chunk);
    titleRow.append(title, status);
    const preview = document.createElement("div");
    preview.className = "knowledge-doc-chunk-preview";
    preview.textContent = chunk.preview || chunk.content || "";
    if (state.knowledge.selectedChunkIndices?.has(chunk.index)) {
      item.classList.add("selected");
    }
    if (state.knowledge.embeddingChunkIndices?.has(chunk.index)) {
      item.classList.add("embedding");
    }
    item.append(titleRow, preview);
    item.addEventListener("click", () => {
      toggleChunkSelection(chunk.index);
    });
    item.addEventListener("dblclick", (event) => {
      event.stopPropagation();
      openChunkModal(chunk);
    });
    elements.knowledgeDocChunks.appendChild(item);
  });
};

const ensureChunkSelection = () => {
  if (!(state.knowledge.selectedChunkIndices instanceof Set)) {
    state.knowledge.selectedChunkIndices = new Set();
  }
};

const syncChunkSelection = () => {
  ensureChunkSelection();
  if (!state.knowledge.docChunks.length) {
    state.knowledge.selectedChunkIndices.clear();
    return;
  }
  const available = new Set(state.knowledge.docChunks.map((chunk) => chunk.index));
  state.knowledge.selectedChunkIndices.forEach((index) => {
    if (!available.has(index)) {
      state.knowledge.selectedChunkIndices.delete(index);
    }
  });
};

const toggleChunkSelection = (index) => {
  ensureChunkSelection();
  if (state.knowledge.selectedChunkIndices.has(index)) {
    state.knowledge.selectedChunkIndices.delete(index);
  } else {
    state.knowledge.selectedChunkIndices.add(index);
  }
  renderVectorDocChunks();
  updateChunkActionState();
  if (knowledgeDocModal?.classList.contains("active")) {
    renderDocModalContent();
  }
};

const setIconButtonLabel = (button, label) => {
  if (!button) {
    return;
  }
  button.title = label;
  button.setAttribute("aria-label", label);
};

const updateChunkSelectAllState = () => {
  if (!knowledgeDocChunkSelectAllBtn) {
    return;
  }
  const hasDoc = Boolean(state.knowledge.docMeta);
  const total = state.knowledge.docChunks.length;
  const selectedCount = state.knowledge.selectedChunkIndices?.size || 0;
  const disabled = !hasDoc || total === 0;
  knowledgeDocChunkSelectAllBtn.disabled = disabled;
  const allSelected = !disabled && selectedCount === total;
  const labelKey = allSelected
    ? "knowledge.chunk.action.clearSelection"
    : "knowledge.chunk.action.selectAll";
  const label = t(labelKey);
  setIconButtonLabel(knowledgeDocChunkSelectAllBtn, label);
  const icon = knowledgeDocChunkSelectAllBtn.querySelector("i");
  if (icon) {
    icon.className = allSelected ? "fa-solid fa-square-minus" : "fa-solid fa-square-check";
  }
};

const updateChunkActionState = () => {
  const hasDoc = Boolean(state.knowledge.docMeta);
  const selectedCount = state.knowledge.selectedChunkIndices?.size || 0;
  const embeddingActive = (state.knowledge.embeddingChunkIndices?.size || 0) > 0;
  const embedBtn = elements.knowledgeDocEmbedBtn;
  if (embedBtn) {
    embedBtn.disabled = !hasDoc || selectedCount === 0 || embeddingActive;
    embedBtn.classList.toggle("is-loading", embeddingActive);
    const icon = embedBtn.querySelector("i");
    if (icon) {
      icon.className = embeddingActive ? "fa-solid fa-spinner" : "fa-solid fa-cube";
    }
    const label = embeddingActive
      ? t("knowledge.chunk.action.embedding")
      : t("knowledge.doc.action.embed");
    setIconButtonLabel(embedBtn, label);
  }
  const deleteBtn = elements.knowledgeDocChunkDeleteBtn;
  if (deleteBtn) {
    deleteBtn.disabled = !hasDoc || selectedCount === 0 || embeddingActive;
  }
  updateChunkSelectAllState();
};

const toggleSelectAllChunks = () => {
  if (!state.knowledge.docMeta) {
    return;
  }
  const chunks = state.knowledge.docChunks;
  if (!chunks.length) {
    return;
  }
  ensureChunkSelection();
  if (state.knowledge.selectedChunkIndices.size === chunks.length) {
    state.knowledge.selectedChunkIndices.clear();
  } else {
    state.knowledge.selectedChunkIndices = new Set(chunks.map((chunk) => chunk.index));
  }
  renderVectorDocChunks();
  updateChunkActionState();
  if (knowledgeDocModal?.classList.contains("active")) {
    renderDocModalContent();
  }
};

const renderVectorDocDetail = () => {
  if (!elements.knowledgeDocTitle || !elements.knowledgeDocMeta) {
    return;
  }
  const meta = state.knowledge.docMeta;
  if (!meta) {
    elements.knowledgeDocTitle.textContent = t("knowledge.doc.none");
    elements.knowledgeDocMeta.textContent = "";
    if (elements.knowledgeDocToggleBtn) {
      elements.knowledgeDocToggleBtn.disabled = true;
    }
    state.knowledge.selectedChunkIndices = new Set();
    updateChunkActionState();
    renderVectorDocChunks();
    return;
  }
  elements.knowledgeDocTitle.textContent = meta.name || meta.doc_id || t("knowledge.doc.none");
  elements.knowledgeDocMeta.textContent = buildDocMetaText(meta);
  if (elements.knowledgeDocToggleBtn) {
    elements.knowledgeDocToggleBtn.disabled = false;
  }
  syncChunkSelection();
  updateChunkActionState();
  renderVectorDocChunks();
};

const loadVectorDocs = async () => {
  const base = getActiveBase();
  if (!base || !base.name) {
    resetVectorState();
    renderVectorDocList();
    renderVectorDocDetail();
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/knowledge/docs?base=${encodeURIComponent(base.name)}`;
  if (elements.knowledgeDocList) {
    elements.knowledgeDocList.textContent = t("common.loading");
  }
  const response = await fetch(endpoint);
  if (!response.ok) {
    if (elements.knowledgeDocList) {
      elements.knowledgeDocList.textContent = t("common.loadFailedWithMessage", {
        message: response.status,
      });
    }
    return;
  }
  const result = await response.json();
  state.knowledge.vectorDocs = Array.isArray(result.docs) ? result.docs : [];
  if (
    state.knowledge.activeDocId &&
    !state.knowledge.vectorDocs.some((doc) => doc.doc_id === state.knowledge.activeDocId)
  ) {
    state.knowledge.activeDocId = "";
    state.knowledge.docContent = "";
    state.knowledge.docMeta = null;
    state.knowledge.docChunks = [];
    state.knowledge.selectedChunkIndices = new Set();
    state.knowledge.embeddingChunkIndices = new Set();
  }
  renderVectorDocList();
  renderVectorDocDetail();
};

const selectVectorDoc = async (docId, options = {}) => {
  const base = getActiveBase();
  if (!base || !base.name) {
    notify(t("knowledge.base.selectRequired"), "warn");
    return;
  }
  if (!docId) {
    notify(t("knowledge.doc.none"), "warn");
    return;
  }
  const keepSelection = options.keepSelection === true;
  const previousSelection = keepSelection
    ? new Set(state.knowledge.selectedChunkIndices || [])
    : new Set();
  const wunderBase = getWunderBase();
  const docEndpoint = `${wunderBase}/admin/knowledge/doc?base=${encodeURIComponent(
    base.name
  )}&doc_id=${encodeURIComponent(docId)}`;
  const chunkEndpoint = `${wunderBase}/admin/knowledge/chunks?base=${encodeURIComponent(
    base.name
  )}&doc_id=${encodeURIComponent(docId)}`;
  try {
    const [docResponse, chunkResponse] = await Promise.all([
      fetch(docEndpoint),
      fetch(chunkEndpoint),
    ]);
    if (!docResponse.ok) {
      throw new Error(t("knowledge.doc.loadFailed", { message: docResponse.status }));
    }
    if (!chunkResponse.ok) {
      throw new Error(t("knowledge.doc.loadFailed", { message: chunkResponse.status }));
    }
    const docResult = await docResponse.json();
    const chunkResult = await chunkResponse.json();
    state.knowledge.activeDocId = docId;
    state.knowledge.docMeta = docResult.doc || null;
    state.knowledge.docContent = docResult.content || "";
    state.knowledge.docChunks = Array.isArray(chunkResult.chunks) ? chunkResult.chunks : [];
    state.knowledge.selectedChunkIndices = previousSelection;
    state.knowledge.embeddingChunkIndices = new Set();
    renderVectorDocList();
    renderVectorDocDetail();
  } catch (error) {
    notify(t("knowledge.doc.loadFailed", { message: error.message }), "error");
  }
};

const deleteVectorDoc = async (docId) => {
  const base = getActiveBase();
  if (!base || !base.name) {
    notify(t("knowledge.base.selectRequired"), "warn");
    return;
  }
  const targetId = docId || state.knowledge.activeDocId;
  if (!targetId) {
    notify(t("knowledge.doc.none"), "warn");
    return;
  }
  const docMeta = state.knowledge.docMeta;
  const name = docMeta?.name || targetId;
  if (!window.confirm(t("knowledge.doc.deleteConfirm", { name }))) {
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/knowledge/doc?base=${encodeURIComponent(
    base.name
  )}&doc_id=${encodeURIComponent(targetId)}`;
  const response = await fetch(endpoint, { method: "DELETE" });
  if (!response.ok) {
    throw new Error(t("knowledge.doc.deleteFailed", { message: response.status }));
  }
  if (state.knowledge.activeDocId === targetId) {
    state.knowledge.activeDocId = "";
    state.knowledge.docContent = "";
    state.knowledge.docMeta = null;
    state.knowledge.docChunks = [];
    state.knowledge.selectedChunkIndices = new Set();
    state.knowledge.embeddingChunkIndices = new Set();
  }
  await loadVectorDocs();
  notify(t("knowledge.doc.deleted"), "success");
};

const refreshActiveVectorDoc = async () => {
  if (!state.knowledge.activeDocId) {
    return;
  }
  await selectVectorDoc(state.knowledge.activeDocId, { keepSelection: true });
  await loadVectorDocs();
};

const updateVectorChunk = async (chunkIndex, content) => {
  const base = getActiveBase();
  if (!base || !base.name) {
    notify(t("knowledge.base.selectRequired"), "warn");
    return;
  }
  if (!state.knowledge.activeDocId) {
    notify(t("knowledge.doc.none"), "warn");
    return;
  }
  const payload = {
    base: base.name,
    doc_id: state.knowledge.activeDocId,
    chunk_index: chunkIndex,
    content,
  };
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/knowledge/chunk/update`;
  const response = await fetch(endpoint, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify(payload),
  });
  if (!response.ok) {
    throw new Error(t("knowledge.chunk.updateFailed", { message: response.status }));
  }
  await refreshActiveVectorDoc();
  notify(t("knowledge.chunk.updateSuccess"), "success");
};

const embedVectorChunk = async (chunkIndex, options = {}) => {
  const base = getActiveBase();
  if (!base || !base.name) {
    notify(t("knowledge.base.selectRequired"), "warn");
    return;
  }
  if (!state.knowledge.activeDocId) {
    notify(t("knowledge.doc.none"), "warn");
    return;
  }
  const payload = {
    base: base.name,
    doc_id: state.knowledge.activeDocId,
    chunk_index: chunkIndex,
  };
  if (!(state.knowledge.embeddingChunkIndices instanceof Set)) {
    state.knowledge.embeddingChunkIndices = new Set();
  }
  state.knowledge.embeddingChunkIndices.add(chunkIndex);
  renderVectorDocChunks();
  updateChunkActionState();
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/knowledge/chunk/embed`;
  try {
    const response = await fetch(endpoint, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(payload),
    });
    if (!response.ok) {
      throw new Error(t("knowledge.chunk.embedFailed", { message: response.status }));
    }
    const localChunk = state.knowledge.docChunks.find(
      (item) => item.index === chunkIndex
    );
    if (localChunk) {
      localChunk.status = "embedded";
    }
    if (options.refresh !== false) {
      await refreshActiveVectorDoc();
    }
    if (options.silent !== true) {
      notify(t("knowledge.chunk.embedSuccess"), "success");
    }
  } finally {
    state.knowledge.embeddingChunkIndices.delete(chunkIndex);
    renderVectorDocChunks();
    updateChunkActionState();
  }
};

const deleteVectorChunk = async (chunkIndex, options = {}) => {
  const base = getActiveBase();
  if (!base || !base.name) {
    notify(t("knowledge.base.selectRequired"), "warn");
    return;
  }
  if (!state.knowledge.activeDocId) {
    notify(t("knowledge.doc.none"), "warn");
    return;
  }
  if (
    options.skipConfirm !== true &&
    !window.confirm(t("knowledge.chunk.deleteConfirm", { index: chunkIndex }))
  ) {
    return;
  }
  const payload = {
    base: base.name,
    doc_id: state.knowledge.activeDocId,
    chunk_index: chunkIndex,
  };
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/knowledge/chunk/delete`;
  const response = await fetch(endpoint, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify(payload),
  });
  if (!response.ok) {
    throw new Error(t("knowledge.chunk.deleteFailed", { message: response.status }));
  }
  state.knowledge.docChunks = state.knowledge.docChunks.filter(
    (item) => item.index !== chunkIndex
  );
  state.knowledge.selectedChunkIndices?.delete(chunkIndex);
  if (options.refresh !== false) {
    await refreshActiveVectorDoc();
  }
  if (options.silent !== true) {
    notify(t("knowledge.chunk.deleted"), "success");
  }
};

const embedSelectedChunks = async () => {
  const selected = getSelectedChunkIndices();
  if (!selected.length) {
    notify(t("knowledge.chunk.selectRequired"), "warn");
    return;
  }
  const pending = selected.filter((index) => {
    const chunk = state.knowledge.docChunks.find((item) => item.index === index);
    return chunk && resolveChunkStatus(chunk) !== "embedded";
  });
  if (!pending.length) {
    notify(t("knowledge.chunk.embedSkipped"), "info");
    return;
  }
  let succeeded = 0;
  let failed = 0;
  for (const index of pending) {
    try {
      await embedVectorChunk(index, { silent: true, refresh: false });
      succeeded += 1;
    } catch (error) {
      failed += 1;
    }
  }
  await refreshActiveVectorDoc();
  if (succeeded) {
    notify(t("knowledge.chunk.embedBatchSuccess", { count: succeeded }), "success");
  }
  if (failed) {
    notify(t("knowledge.chunk.embedBatchFailed", { count: failed }), "error");
  }
};

const deleteSelectedChunks = async () => {
  const selected = getSelectedChunkIndices();
  if (!selected.length) {
    notify(t("knowledge.chunk.selectRequired"), "warn");
    return;
  }
  if (!window.confirm(t("knowledge.chunk.deleteBatchConfirm", { count: selected.length }))) {
    return;
  }
  let succeeded = 0;
  let failed = 0;
  for (const index of selected) {
    try {
      await deleteVectorChunk(index, { silent: true, refresh: false, skipConfirm: true });
      succeeded += 1;
    } catch (error) {
      failed += 1;
    }
  }
  await refreshActiveVectorDoc();
  if (succeeded) {
    notify(t("knowledge.chunk.deleteBatchSuccess", { count: succeeded }), "success");
  }
  if (failed) {
    notify(t("knowledge.chunk.deleteBatchFailed", { count: failed }), "error");
  }
};

const openChunkModal = (chunk) => {
  if (!knowledgeChunkModal || !knowledgeChunkModalContent) {
    return;
  }
  editingChunkIndex = chunk.index;
  if (knowledgeChunkModalTitle) {
    knowledgeChunkModalTitle.textContent = t("knowledge.chunk.modal.title", {
      index: chunk.index,
    });
  }
  knowledgeChunkModalContent.value = chunk.content || "";
  knowledgeChunkModal.classList.add("active");
  knowledgeChunkModalContent.focus();
};

const closeChunkModal = () => {
  if (!knowledgeChunkModal) {
    return;
  }
  knowledgeChunkModal.classList.remove("active");
  editingChunkIndex = null;
  if (knowledgeChunkModalContent) {
    knowledgeChunkModalContent.value = "";
  }
};

const saveChunkModal = async () => {
  if (editingChunkIndex === null || editingChunkIndex === undefined) {
    return;
  }
  const content = knowledgeChunkModalContent?.value || "";
  if (!content.trim()) {
    notify(t("knowledge.chunk.content.required"), "warn");
    return;
  }
  try {
    await updateVectorChunk(editingChunkIndex, content);
    closeChunkModal();
  } catch (error) {
    notify(t("knowledge.chunk.updateFailed", { message: error.message }), "error");
  }
};

const rebuildVectorDocs = async (docId) => {
  const base = getActiveBase();
  if (!base || !base.name) {
    notify(t("knowledge.base.selectRequired"), "warn");
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/knowledge/reindex`;
  const payload = { base: base.name };
  if (docId) {
    payload.doc_id = docId;
  }
  const response = await fetch(endpoint, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify(payload),
  });
  if (!response.ok) {
    throw new Error(t("knowledge.doc.rebuildFailed", { message: response.status }));
  }
  const result = await response.json();
  if (result?.ok === false) {
    notify(
      t("knowledge.doc.rebuildFailed", { message: JSON.stringify(result.failed || []) }),
      "error"
    );
  } else {
    notify(t("knowledge.doc.rebuildSuccess"), "success");
  }
  await loadVectorDocs();
  if (docId) {
    await selectVectorDoc(docId);
  }
};

const setKnowledgeTestStatus = (message) => {
  if (!knowledgeTestStatus) {
    return;
  }
  knowledgeTestStatus.textContent = message || "";
};

const clearKnowledgeTestResult = () => {
  if (!knowledgeTestResult) {
    return;
  }
  knowledgeTestResult.textContent = "";
};

const formatTestScore = (score) => {
  if (!Number.isFinite(score)) {
    return "-";
  }
  return score.toFixed(3);
};

const renderKnowledgeTestResults = (hits) => {
  if (!knowledgeTestResult) {
    return;
  }
  knowledgeTestResult.textContent = "";
  if (!hits.length) {
    knowledgeTestResult.textContent = t("knowledge.test.empty");
    return;
  }
  hits.forEach((hit, index) => {
    const item = document.createElement("div");
    item.className = "knowledge-test-result-item";
    const header = document.createElement("div");
    header.className = "knowledge-test-result-header";
    const docName = hit.document || hit.doc_id || t("knowledge.doc.none");
    const scoreText = formatTestScore(Number(hit.score));
    header.textContent = `${index + 1}. ${docName} #${hit.chunk_index ?? "-"} · ${scoreText}`;
    const content = document.createElement("div");
    content.className = "knowledge-test-result-content";
    content.textContent = hit.content || "";
    item.append(header, content);
    knowledgeTestResult.appendChild(item);
  });
};

const openKnowledgeTestModal = () => {
  if (!knowledgeTestModal) {
    return;
  }
  const base = getActiveBase();
  if (!base || !isVectorBase(base)) {
    notify(t("knowledge.base.selectRequired"), "warn");
    return;
  }
  if (knowledgeTestModalTitle) {
    knowledgeTestModalTitle.textContent = t("knowledge.test.title");
  }
  if (knowledgeTestQuestion && !knowledgeTestQuestion.value) {
    knowledgeTestQuestion.value = "";
  }
  clearKnowledgeTestResult();
  setKnowledgeTestStatus("");
  knowledgeTestModal.classList.add("active");
  knowledgeTestQuestion?.focus();
};

const closeKnowledgeTestModal = () => {
  if (!knowledgeTestModal) {
    return;
  }
  knowledgeTestModal.classList.remove("active");
};

const runKnowledgeTest = async () => {
  const base = getActiveBase();
  if (!base || !base.name || !isVectorBase(base)) {
    notify(t("knowledge.base.selectRequired"), "warn");
    return;
  }
  const query = String(knowledgeTestQuestion?.value || "").trim();
  if (!query) {
    notify(t("knowledge.test.queryRequired"), "warn");
    return;
  }
  if (knowledgeTestRunBtn) {
    knowledgeTestRunBtn.disabled = true;
    knowledgeTestRunBtn.classList.add("is-loading");
    const icon = knowledgeTestRunBtn.querySelector("i");
    if (icon) {
      icon.className = "fa-solid fa-spinner";
    }
  }
  setKnowledgeTestStatus(t("knowledge.test.running"));
  if (knowledgeTestResult) {
    knowledgeTestResult.textContent = t("common.loading");
  }
  try {
    const wunderBase = getWunderBase();
    const endpoint = `${wunderBase}/admin/knowledge/test`;
    const response = await fetch(endpoint, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ base: base.name, query }),
    });
    if (!response.ok) {
      const payload = await response.json().catch(() => null);
      const message =
        payload?.detail?.message || payload?.message || String(response.status);
      throw new Error(message);
    }
    const result = await response.json();
    renderKnowledgeTestResults(Array.isArray(result.hits) ? result.hits : []);
    setKnowledgeTestStatus(t("knowledge.test.done"));
  } catch (error) {
    setKnowledgeTestStatus(t("knowledge.test.failed", { message: error.message }));
  } finally {
    if (knowledgeTestRunBtn) {
      knowledgeTestRunBtn.disabled = false;
      knowledgeTestRunBtn.classList.remove("is-loading");
      const icon = knowledgeTestRunBtn.querySelector("i");
      if (icon) {
        icon.className = "fa-solid fa-play";
      }
    }
  }
};

const buildKnowledgePayload = () => ({
  bases: state.knowledge.bases.map((base) => ({
    name: base.name.trim(),
    description: base.description || "",
    root: base.root.trim(),
    enabled: base.enabled !== false,
    base_type: normalizeBaseType(base.base_type),
    embedding_model: base.embedding_model || "",
    chunk_size: base.chunk_size ?? null,
    chunk_overlap: base.chunk_overlap ?? null,
    top_k: base.top_k ?? null,
    score_threshold: base.score_threshold ?? null,
  })),
});

const validateKnowledgePayload = (payload) => {
  const invalid = payload.bases.filter((base) => !base.name);
  if (invalid.length) {
    return t("knowledge.payload.invalid");
  }
  for (const base of payload.bases) {
    if (normalizeBaseType(base.base_type) === "vector" && !base.embedding_model) {
      return t("knowledge.embedding.required");
    }
  }
  const nameSet = new Set();
  for (const base of payload.bases) {
    if (nameSet.has(base.name)) {
      return t("knowledge.name.duplicate", { name: base.name });
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
    throw new Error(t("common.requestFailed", { status: response.status }));
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
    notify(t("knowledge.base.selectRequired"), "warn");
    return;
  }
  if (!file) {
    return;
  }
  const vectorMode = isVectorBase(base);
  const extension = normalizeUploadExtension(file.name);
  if (!extension) {
    notify(t("knowledge.file.extensionMissing"), "warn");
    return;
  }
  if (!SUPPORTED_UPLOAD_EXTENSIONS.includes(extension)) {
    notify(t("knowledge.file.unsupported", { extension }), "warn");
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
    throw new Error(t("knowledge.file.uploadFailed", { status: response.status }));
  }
  const result = await response.json();
  if (vectorMode) {
    await loadVectorDocs();
    if (result?.doc_id) {
      await selectVectorDoc(result.doc_id);
    }
    notify(t("knowledge.doc.uploaded", { name: result?.doc_name || file.name }), "success");
  } else {
    await loadKnowledgeFiles();
    if (result?.path) {
      await selectKnowledgeFile(result.path);
    }
    notify(t("knowledge.file.uploaded", { name: result?.path || file.name }), "success");
  }
  const warnings = Array.isArray(result?.warnings) ? result.warnings : [];
  if (warnings.length) {
    notify(t("knowledge.file.warnings", { message: warnings.join(" | ") }), "warn");
  }
};

const loadKnowledgeFiles = async () => {
  const base = getActiveBase();
  if (!base || !base.name) {
    state.knowledge.files = [];
    state.knowledge.activeFile = "";
    state.knowledge.fileContent = "";
    renderKnowledgeFiles();
    return;
  }
  if (isVectorBase(base)) {
    state.knowledge.files = [];
    state.knowledge.activeFile = "";
    state.knowledge.fileContent = "";
    renderKnowledgeFiles();
    await loadVectorDocs();
    return;
  }
  if (!base.root) {
    state.knowledge.files = [];
    state.knowledge.activeFile = "";
    state.knowledge.fileContent = "";
    renderKnowledgeFiles();
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/knowledge/files?base=${encodeURIComponent(base.name)}`;
  elements.knowledgeFileList.textContent = t("common.loading");
  const response = await fetch(endpoint);
  if (!response.ok) {
    elements.knowledgeFileList.textContent = t("common.loadFailedWithMessage", {
      message: response.status,
    });
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
    notify(t("knowledge.base.selectRequired"), "warn");
    return;
  }
  if (isVectorBase(base)) {
    notify(t("knowledge.vector.readonly"), "warn");
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/knowledge/file?base=${encodeURIComponent(
    base.name
  )}&path=${encodeURIComponent(filePath)}`;
  elements.knowledgeFileName.textContent = t("common.loading");
  const response = await fetch(endpoint);
  if (!response.ok) {
    notify(t("knowledge.file.readFailed", { status: response.status }), "error");
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
    notify(t("knowledge.base.selectRequired"), "warn");
    return;
  }
  if (isVectorBase(base)) {
    notify(t("knowledge.vector.readonly"), "warn");
    return;
  }
  if (!state.knowledge.activeFile) {
    notify(t("knowledge.file.saveRequired"), "warn");
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
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  await loadKnowledgeFiles();
  notify(t("knowledge.file.saved"), "success");
};

const createKnowledgeFile = async () => {
  const base = getActiveBase();
  if (!base || !base.name) {
    notify(t("knowledge.base.selectRequired"), "warn");
    return;
  }
  if (isVectorBase(base)) {
    notify(t("knowledge.vector.readonly"), "warn");
    return;
  }
  const filename = window.prompt(t("knowledge.file.newPrompt"), "example.md");
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
    notify(t("knowledge.base.selectRequired"), "warn");
    return;
  }
  if (isVectorBase(base)) {
    notify(t("knowledge.vector.readonly"), "warn");
    return;
  }
  const path = targetPath || state.knowledge.activeFile;
  if (!path) {
    notify(t("knowledge.file.deleteRequired"), "warn");
    return;
  }
  if (!window.confirm(t("knowledge.file.deleteConfirm", { name: path }))) {
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/knowledge/file?base=${encodeURIComponent(
    base.name
  )}&path=${encodeURIComponent(path)}`;
  const response = await fetch(endpoint, { method: "DELETE" });
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  if (path === state.knowledge.activeFile) {
    state.knowledge.activeFile = "";
    state.knowledge.fileContent = "";
  }
  await loadKnowledgeFiles();
  notify(t("knowledge.file.deleted"), "success");
};

const addKnowledgeBase = () => {
  openKnowledgeModal();
};

const editKnowledgeBase = () => {
  const base = getActiveBase();
  if (!base) {
    notify(t("knowledge.base.selectRequired"), "warn");
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
    vectorDocs: [...state.knowledge.vectorDocs],
    activeDocId: state.knowledge.activeDocId,
    docContent: state.knowledge.docContent,
    docMeta: state.knowledge.docMeta ? { ...state.knowledge.docMeta } : null,
    docChunks: [...state.knowledge.docChunks],
    selectedChunkIndices: [...(state.knowledge.selectedChunkIndices || [])],
    embeddingChunkIndices: [...(state.knowledge.embeddingChunkIndices || [])],
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
  resetVectorState();
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
      state.knowledge.vectorDocs = snapshot.vectorDocs;
      state.knowledge.activeDocId = snapshot.activeDocId;
      state.knowledge.docContent = snapshot.docContent;
      state.knowledge.docMeta = snapshot.docMeta;
      state.knowledge.docChunks = snapshot.docChunks;
      state.knowledge.selectedChunkIndices = new Set(snapshot.selectedChunkIndices || []);
      state.knowledge.embeddingChunkIndices = new Set(snapshot.embeddingChunkIndices || []);
      renderKnowledgeBaseList();
      renderKnowledgeDetail();
      return;
    }
    await loadKnowledgeFiles();
    notify(
      knowledgeEditingIndex >= 0 ? t("knowledge.base.updated") : t("knowledge.base.added"),
      "success"
    );
    closeKnowledgeModal();
  } catch (error) {
    state.knowledge.bases = snapshot.bases;
    state.knowledge.selectedIndex = snapshot.selectedIndex;
    state.knowledge.files = snapshot.files;
    state.knowledge.activeFile = snapshot.activeFile;
    state.knowledge.fileContent = snapshot.fileContent;
    state.knowledge.vectorDocs = snapshot.vectorDocs;
    state.knowledge.activeDocId = snapshot.activeDocId;
    state.knowledge.docContent = snapshot.docContent;
    state.knowledge.docMeta = snapshot.docMeta;
    state.knowledge.docChunks = snapshot.docChunks;
    state.knowledge.selectedChunkIndices = new Set(snapshot.selectedChunkIndices || []);
    state.knowledge.embeddingChunkIndices = new Set(snapshot.embeddingChunkIndices || []);
    renderKnowledgeBaseList();
    renderKnowledgeDetail();
    notify(t("knowledge.saveFailed", { message: error.message }), "error");
  }
};

const deleteKnowledgeBase = async () => {
  if (state.knowledge.selectedIndex < 0) {
    return;
  }
  const base = getActiveBase();
  const baseName = base && base.name ? base.name : t("knowledge.name.unnamed");
  if (!window.confirm(t("knowledge.base.deleteConfirm", { name: baseName }))) {
    return;
  }
  const snapshot = {
    bases: state.knowledge.bases.map((item) => ({ ...item })),
    selectedIndex: state.knowledge.selectedIndex,
    files: [...state.knowledge.files],
    activeFile: state.knowledge.activeFile,
    fileContent: state.knowledge.fileContent,
    vectorDocs: [...state.knowledge.vectorDocs],
    activeDocId: state.knowledge.activeDocId,
    docContent: state.knowledge.docContent,
    docMeta: state.knowledge.docMeta ? { ...state.knowledge.docMeta } : null,
    docChunks: [...state.knowledge.docChunks],
    selectedChunkIndices: [...(state.knowledge.selectedChunkIndices || [])],
    embeddingChunkIndices: [...(state.knowledge.embeddingChunkIndices || [])],
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
  resetVectorState();
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
      state.knowledge.vectorDocs = snapshot.vectorDocs;
      state.knowledge.activeDocId = snapshot.activeDocId;
      state.knowledge.docContent = snapshot.docContent;
      state.knowledge.docMeta = snapshot.docMeta;
      state.knowledge.docChunks = snapshot.docChunks;
      state.knowledge.selectedChunkIndices = new Set(snapshot.selectedChunkIndices || []);
      state.knowledge.embeddingChunkIndices = new Set(snapshot.embeddingChunkIndices || []);
      renderKnowledgeBaseList();
      renderKnowledgeDetail();
      return;
    }
    await loadKnowledgeFiles();
    notify(t("knowledge.base.deleted"), "success");
  } catch (error) {
    state.knowledge.bases = snapshot.bases;
    state.knowledge.selectedIndex = snapshot.selectedIndex;
    state.knowledge.files = snapshot.files;
    state.knowledge.activeFile = snapshot.activeFile;
    state.knowledge.fileContent = snapshot.fileContent;
    state.knowledge.vectorDocs = snapshot.vectorDocs;
    state.knowledge.activeDocId = snapshot.activeDocId;
    state.knowledge.docContent = snapshot.docContent;
    state.knowledge.docMeta = snapshot.docMeta;
    state.knowledge.docChunks = snapshot.docChunks;
    state.knowledge.selectedChunkIndices = new Set(snapshot.selectedChunkIndices || []);
    state.knowledge.embeddingChunkIndices = new Set(snapshot.embeddingChunkIndices || []);
    renderKnowledgeBaseList();
    renderKnowledgeDetail();
    notify(t("knowledge.deleteFailed", { message: error.message }), "error");
  }
};

export const loadKnowledgeConfig = async () => {
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/knowledge`;
  const response = await fetch(endpoint);
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const result = await response.json();
  const normalized = normalizeKnowledgeConfig(result.knowledge || {});
  state.knowledge.bases = normalized.bases;
  state.knowledge.selectedIndex = state.knowledge.bases.length ? 0 : -1;
  state.knowledge.files = [];
  state.knowledge.activeFile = "";
  state.knowledge.fileContent = "";
  resetVectorState();
  renderKnowledgeBaseList();
  renderKnowledgeDetail();
  try {
    await loadEmbeddingModels();
  } catch (error) {
    console.warn(error);
  }
  await loadKnowledgeFiles();
};

export const initKnowledgePanel = () => {
  elements.knowledgeAddBtn?.addEventListener("click", addKnowledgeBase);
  knowledgeEditBtn?.addEventListener("click", editKnowledgeBase);
  elements.knowledgeDeleteBtn?.addEventListener("click", () => {
    deleteKnowledgeBase().catch((error) =>
      notify(t("knowledge.deleteFailed", { message: error.message }), "error")
    );
  });
  elements.knowledgeRefreshBtn?.addEventListener("click", async () => {
    try {
      await loadKnowledgeConfig();
      notify(t("knowledge.refreshSuccess"), "success");
    } catch (error) {
      notify(t("knowledge.refreshFailed", { message: error.message }), "error");
    }
  });
  knowledgeModalSave?.addEventListener("click", () => {
    applyKnowledgeModal();
  });
  knowledgeModalCancel?.addEventListener("click", closeKnowledgeModal);
  knowledgeModalClose?.addEventListener("click", closeKnowledgeModal);
  knowledgeModalType?.addEventListener("change", () => {
    const type = normalizeBaseType(knowledgeModalType.value);
    applyKnowledgeModalType(type);
    if (type === "vector") {
      loadEmbeddingModels(true)
        .then(() => {
          renderEmbeddingModelOptions(knowledgeModalEmbeddingModel?.value || "");
        })
        .catch((error) => {
          notify(t("knowledge.embedding.empty"), "warn");
          console.warn(error);
        });
    }
  });
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
      notify(t("knowledge.base.selectRequired"), "warn");
      return;
    }
    if (knowledgeFileUploadInput) {
      knowledgeFileUploadInput.value = "";
      knowledgeFileUploadInput.click();
    }
  });
  elements.knowledgeDocUploadBtn?.addEventListener("click", () => {
    const base = getActiveBase();
    if (!base || !base.name) {
      notify(t("knowledge.base.selectRequired"), "warn");
      return;
    }
    if (knowledgeFileUploadInput) {
      knowledgeFileUploadInput.value = "";
      knowledgeFileUploadInput.click();
    }
  });
  elements.knowledgeDocRebuildAllBtn?.addEventListener("click", async () => {
    try {
      await rebuildVectorDocs();
    } catch (error) {
      notify(t("knowledge.doc.rebuildFailed", { message: error.message }), "error");
    }
  });
  elements.knowledgeDocToggleBtn?.addEventListener("click", () => {
    openDocModal();
  });
  elements.knowledgeDocEmbedBtn?.addEventListener("click", () => {
    embedSelectedChunks().catch((error) => {
      notify(t("knowledge.chunk.embedFailed", { message: error.message }), "error");
    });
  });
  elements.knowledgeDocChunkDeleteBtn?.addEventListener("click", () => {
    deleteSelectedChunks().catch((error) => {
      notify(t("knowledge.chunk.deleteFailed", { message: error.message }), "error");
    });
  });
  knowledgeDocChunkSelectAllBtn?.addEventListener("click", () => {
    toggleSelectAllChunks();
  });
  knowledgeTestBtn?.addEventListener("click", () => {
    openKnowledgeTestModal();
  });
  knowledgeDocModalClose?.addEventListener("click", closeDocModal);
  knowledgeDocModalCloseBtn?.addEventListener("click", closeDocModal);
  knowledgeDocModal?.addEventListener("click", (event) => {
    if (event.target === knowledgeDocModal) {
      closeDocModal();
    }
  });
  knowledgeTestModalClose?.addEventListener("click", closeKnowledgeTestModal);
  knowledgeTestModalCloseBtn?.addEventListener("click", closeKnowledgeTestModal);
  knowledgeTestModal?.addEventListener("click", (event) => {
    if (event.target === knowledgeTestModal) {
      closeKnowledgeTestModal();
    }
  });
  knowledgeTestRunBtn?.addEventListener("click", () => {
    runKnowledgeTest();
  });
  knowledgeFileUploadInput?.addEventListener("change", async () => {
    const files = Array.from(knowledgeFileUploadInput.files || []);
    if (!files.length) {
      return;
    }
    for (const file of files) {
      try {
        await uploadKnowledgeFile(file);
      } catch (error) {
        notify(t("knowledge.file.uploadFailedMessage", { message: error.message }), "error");
      }
    }
  });
  knowledgeChunkModalSave?.addEventListener("click", saveChunkModal);
  knowledgeChunkModalCancel?.addEventListener("click", closeChunkModal);
  knowledgeChunkModalClose?.addEventListener("click", closeChunkModal);
  knowledgeChunkModal?.addEventListener("click", (event) => {
    if (event.target === knowledgeChunkModal) {
      closeChunkModal();
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
      notify(t("knowledge.file.saveFailed", { message: error.message }), "error");
    }
  });
  elements.knowledgeFileNewBtn?.addEventListener("click", async () => {
    try {
      await createKnowledgeFile();
    } catch (error) {
      notify(t("knowledge.file.createFailed", { message: error.message }), "error");
    }
  });
};






