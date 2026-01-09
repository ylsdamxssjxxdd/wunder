import { elements } from "./elements.js?v=20260105-02";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { notify } from "./notify.js";
import { formatDuration, formatTimestamp } from "./utils.js?v=20251229-02";
import { t } from "./i18n.js?v=20260110-01";

const MEMORY_POLL_INTERVAL_MS = 4000;
const DEFAULT_MEMORY_USERS_PAGE_SIZE = 100;
const DEFAULT_MEMORY_QUEUE_PAGE_SIZE = 100;

// 统一管理长期记忆面板状态，避免旧缓存导致字段缺失
const ensureMemoryState = () => {
  if (!state.memory) {
    state.memory = {
      users: [],
      records: [],
      selectedId: "",
      enabled: false,
      search: "",
      pagination: {
        pageSize: DEFAULT_MEMORY_USERS_PAGE_SIZE,
        page: 1,
      },
      status: {
        active: [],
        history: [],
        updatedAt: "",
        updatedAtTs: 0,
      },
      queuePagination: {
        pageSize: DEFAULT_MEMORY_QUEUE_PAGE_SIZE,
        activePage: 1,
        historyPage: 1,
      },
      queueDetailId: "",
      queueDetail: null,
      editingRecord: null,
      loaded: false,
    };
  }
  if (typeof state.memory.search !== "string") {
    state.memory.search = "";
  }
  if (!state.memory.status || typeof state.memory.status !== "object") {
    state.memory.status = {
      active: [],
      history: [],
      updatedAt: "",
      updatedAtTs: 0,
    };
  }
  if (!state.memory.pagination || typeof state.memory.pagination !== "object") {
    state.memory.pagination = {
      pageSize: DEFAULT_MEMORY_USERS_PAGE_SIZE,
      page: 1,
    };
  }
  if (
    !Number.isFinite(state.memory.pagination.pageSize) ||
    state.memory.pagination.pageSize <= 0
  ) {
    state.memory.pagination.pageSize = DEFAULT_MEMORY_USERS_PAGE_SIZE;
  }
  if (!Number.isFinite(state.memory.pagination.page) || state.memory.pagination.page < 1) {
    state.memory.pagination.page = 1;
  }
  if (
    !state.memory.queuePagination ||
    typeof state.memory.queuePagination !== "object"
  ) {
    state.memory.queuePagination = {
      pageSize: DEFAULT_MEMORY_QUEUE_PAGE_SIZE,
      activePage: 1,
      historyPage: 1,
    };
  }
  if (
    !Number.isFinite(state.memory.queuePagination.pageSize) ||
    state.memory.queuePagination.pageSize <= 0
  ) {
    state.memory.queuePagination.pageSize = DEFAULT_MEMORY_QUEUE_PAGE_SIZE;
  }
  if (
    !Number.isFinite(state.memory.queuePagination.activePage) ||
    state.memory.queuePagination.activePage < 1
  ) {
    state.memory.queuePagination.activePage = 1;
  }
  if (
    !Number.isFinite(state.memory.queuePagination.historyPage) ||
    state.memory.queuePagination.historyPage < 1
  ) {
    state.memory.queuePagination.historyPage = 1;
  }
  if (typeof state.memory.queueDetailId !== "string") {
    state.memory.queueDetailId = "";
  }
  if (!("queueDetail" in state.memory)) {
    state.memory.queueDetail = null;
  }
  if (!("editingRecord" in state.memory)) {
    state.memory.editingRecord = null;
  }
  if (state.runtime && !("memoryPollTimer" in state.runtime)) {
    state.runtime.memoryPollTimer = null;
  }
};

// 检查页面元素是否齐全，避免绑定事件时报错
const ensureMemoryElements = () => {
  const requiredKeys = [
    "memoryRefreshBtn",
    "memorySearchInput",
    "memoryUsersBody",
    "memoryUsersEmpty",
    "memoryUsersPagination",
    "memoryUsersPageInfo",
    "memoryUsersPrevBtn",
    "memoryUsersNextBtn",
    "memoryStatusMeta",
    "memoryStatusActiveBody",
    "memoryStatusActiveEmpty",
    "memoryStatusActivePagination",
    "memoryStatusActivePageInfo",
    "memoryStatusActivePrevBtn",
    "memoryStatusActiveNextBtn",
    "memoryStatusHistoryBody",
    "memoryStatusHistoryEmpty",
    "memoryStatusHistoryPagination",
    "memoryStatusHistoryPageInfo",
    "memoryStatusHistoryPrevBtn",
    "memoryStatusHistoryNextBtn",
    "memoryModal",
    "memoryModalTitle",
    "memoryModalMeta",
    "memoryModalClose",
    "memoryModalCloseBtn",
    "memoryModalEnableToggle",
    "memoryModalClearBtn",
    "memoryModalRecordBody",
    "memoryModalRecordEmpty",
    "memoryRecordEditModal",
    "memoryRecordEditTitle",
    "memoryRecordEditMeta",
    "memoryRecordEditInput",
    "memoryRecordEditClose",
    "memoryRecordEditCloseBtn",
    "memoryRecordEditSave",
    "memoryQueueModal",
    "memoryQueueTitle",
    "memoryQueueMeta",
    "memoryQueueRequest",
    "memoryQueueResult",
    "memoryQueueClose",
    "memoryQueueCloseBtn",
  ];
  const missing = requiredKeys.filter((key) => !elements[key]);
  if (missing.length) {
    return false;
  }
  return true;
};

// 规范化用户记忆统计数据
const normalizeMemoryUser = (item) => ({
  user_id: String(item?.user_id || ""),
  enabled: Boolean(item?.enabled),
  record_count: Number(item?.record_count) || 0,
  last_updated_time: String(item?.last_updated_time || ""),
  last_updated_time_ts: Number(item?.last_updated_time_ts) || 0,
});

// 规范化记忆记录数据
const normalizeMemoryRecord = (item) => ({
  session_id: String(item?.session_id || ""),
  summary: String(item?.summary || ""),
  created_time: String(item?.created_time || ""),
  updated_time: String(item?.updated_time || ""),
  created_time_ts: Number(item?.created_time_ts) || 0,
  updated_time_ts: Number(item?.updated_time_ts) || 0,
});

// 规范化队列任务数据
const normalizeMemoryQueueItem = (item) => ({
  task_id: String(item?.task_id || ""),
  user_id: String(item?.user_id || ""),
  session_id: String(item?.session_id || ""),
  status: String(item?.status || ""),
  queued_time: String(item?.queued_time || ""),
  queued_time_ts: Number(item?.queued_time_ts) || 0,
  started_time: String(item?.started_time || ""),
  started_time_ts: Number(item?.started_time_ts) || 0,
  finished_time: String(item?.finished_time || ""),
  finished_time_ts: Number(item?.finished_time_ts) || 0,
  elapsed_s: Number(item?.elapsed_s) || 0,
});

// 规范化队列详情数据
const normalizeMemoryQueueDetail = (item) => ({
  ...normalizeMemoryQueueItem(item),
  request: item?.request && typeof item.request === "object" ? item.request : {},
  result: String(item?.result || ""),
  error: String(item?.error || ""),
});

const getMemoryKeyword = () => String(state.memory.search || "").trim().toLowerCase();

const getFilteredMemoryUsers = () => {
  const keyword = getMemoryKeyword();
  if (!keyword) {
    return state.memory.users;
  }
  return state.memory.users.filter((item) =>
    String(item?.user_id || "").toLowerCase().includes(keyword)
  );
};

// 解析记忆用户列表分页大小，避免无效值导致渲染异常
const resolveMemoryUsersPageSize = () => {
  const rawValue = Math.floor(Number(state.memory.pagination?.pageSize));
  if (!Number.isFinite(rawValue) || rawValue <= 0) {
    return DEFAULT_MEMORY_USERS_PAGE_SIZE;
  }
  return rawValue;
};

// 约束记忆用户列表分页页码，防止越界
const clampMemoryUsersPage = (value, totalPages) => {
  const page = Number(value);
  if (!Number.isFinite(page) || page < 1) {
    return 1;
  }
  if (!Number.isFinite(totalPages) || totalPages <= 0) {
    return 1;
  }
  return Math.min(page, totalPages);
};

// 根据分页状态裁剪记忆用户列表，避免一次渲染过多行
const resolveMemoryUsersPageSlice = (users) => {
  const pageSize = resolveMemoryUsersPageSize();
  const total = Array.isArray(users) ? users.length : 0;
  const totalPages = Math.max(1, Math.ceil(total / pageSize));
  const currentPage = clampMemoryUsersPage(state.memory.pagination?.page, totalPages);
  state.memory.pagination.page = currentPage;
  const startIndex = (currentPage - 1) * pageSize;
  const pageUsers = users.slice(startIndex, startIndex + pageSize);
  return { total, totalPages, currentPage, pageSize, users: pageUsers };
};

// 渲染记忆用户列表分页控件
const renderMemoryUsersPagination = (pageData) => {
  const {
    memoryUsersPagination,
    memoryUsersPageInfo,
    memoryUsersPrevBtn,
    memoryUsersNextBtn,
  } = elements;
  if (!pageData || pageData.total <= 0) {
    memoryUsersPagination.style.display = "none";
    return;
  }
  memoryUsersPagination.style.display = "flex";
  memoryUsersPageInfo.textContent = t("pagination.info", {
    total: pageData.total,
    current: pageData.currentPage,
    pages: pageData.totalPages,
    size: pageData.pageSize,
  });
  memoryUsersPrevBtn.disabled = pageData.currentPage <= 1;
  memoryUsersNextBtn.disabled = pageData.currentPage >= pageData.totalPages;
};

// 切换记忆用户列表分页页码并刷新列表
const updateMemoryUsersPage = (delta) => {
  const current = Number(state.memory.pagination?.page) || 1;
  const nextPage = Math.max(1, current + delta);
  state.memory.pagination.page = nextPage;
  renderMemoryUsers();
};

// 解析记忆队列分页大小，避免无效值影响展示
const resolveMemoryQueuePageSize = () => {
  const rawValue = Math.floor(Number(state.memory.queuePagination?.pageSize));
  if (!Number.isFinite(rawValue) || rawValue <= 0) {
    return DEFAULT_MEMORY_QUEUE_PAGE_SIZE;
  }
  return rawValue;
};

// 约束记忆队列分页页码，避免超出页码范围
const clampMemoryQueuePage = (value, totalPages) => {
  const page = Number(value);
  if (!Number.isFinite(page) || page < 1) {
    return 1;
  }
  if (!Number.isFinite(totalPages) || totalPages <= 0) {
    return 1;
  }
  return Math.min(page, totalPages);
};

// 根据分页状态裁剪队列列表，减少一次性渲染压力
const resolveMemoryQueuePageSlice = (items, pageKey) => {
  const pageSize = resolveMemoryQueuePageSize();
  const total = Array.isArray(items) ? items.length : 0;
  const totalPages = Math.max(1, Math.ceil(total / pageSize));
  const currentPage = clampMemoryQueuePage(state.memory.queuePagination?.[pageKey], totalPages);
  if (state.memory.queuePagination) {
    state.memory.queuePagination[pageKey] = currentPage;
  }
  const startIndex = (currentPage - 1) * pageSize;
  const pageItems = items.slice(startIndex, startIndex + pageSize);
  return { total, totalPages, currentPage, pageSize, items: pageItems };
};

// 渲染记忆队列分页控件
const renderMemoryQueuePagination = (kind, pageData) => {
  const map = {
    active: {
      wrapper: elements.memoryStatusActivePagination,
      info: elements.memoryStatusActivePageInfo,
      prev: elements.memoryStatusActivePrevBtn,
      next: elements.memoryStatusActiveNextBtn,
    },
    history: {
      wrapper: elements.memoryStatusHistoryPagination,
      info: elements.memoryStatusHistoryPageInfo,
      prev: elements.memoryStatusHistoryPrevBtn,
      next: elements.memoryStatusHistoryNextBtn,
    },
  };
  const target = map[kind];
  if (!target) {
    return;
  }
  if (!pageData || pageData.total <= 0) {
    target.wrapper.style.display = "none";
    return;
  }
  target.wrapper.style.display = "flex";
  target.info.textContent = t("pagination.info", {
    total: pageData.total,
    current: pageData.currentPage,
    pages: pageData.totalPages,
    size: pageData.pageSize,
  });
  target.prev.disabled = pageData.currentPage <= 1;
  target.next.disabled = pageData.currentPage >= pageData.totalPages;
};

// 切换记忆队列分页页码并刷新列表
const updateMemoryQueuePage = (pageKey, delta) => {
  const current = Number(state.memory.queuePagination?.[pageKey]) || 1;
  const nextPage = Math.max(1, current + delta);
  if (state.memory.queuePagination) {
    state.memory.queuePagination[pageKey] = nextPage;
  }
  renderMemoryStatus();
};

// 渲染用户列表
const renderMemoryUsers = () => {
  elements.memoryUsersBody.textContent = "";
  const hasUsers = Array.isArray(state.memory.users) && state.memory.users.length > 0;
  if (!hasUsers) {
    elements.memoryUsersEmpty.textContent = t("memory.users.empty");
    elements.memoryUsersEmpty.style.display = "block";
    renderMemoryUsersPagination(null);
    return;
  }
  const filtered = getFilteredMemoryUsers();
  if (!filtered.length) {
    elements.memoryUsersEmpty.textContent = t("memory.users.search.empty");
    elements.memoryUsersEmpty.style.display = "block";
    renderMemoryUsersPagination(null);
    return;
  }
  elements.memoryUsersEmpty.style.display = "none";
  const pageData = resolveMemoryUsersPageSlice(filtered);
  renderMemoryUsersPagination(pageData);
  pageData.users.forEach((user) => {
    const row = document.createElement("tr");
    if (user.user_id === state.memory.selectedId) {
      row.classList.add("active");
    }

    const idCell = document.createElement("td");
    idCell.textContent = user.user_id || "-";

    const countCell = document.createElement("td");
    countCell.textContent = `${user.record_count}`;

    const timeCell = document.createElement("td");
    timeCell.textContent = user.last_updated_time
      ? formatTimestamp(user.last_updated_time)
      : "-";

    const toggleCell = document.createElement("td");
    const toggle = document.createElement("input");
    toggle.type = "checkbox";
    toggle.checked = user.enabled;
    toggle.addEventListener("click", (event) => {
      event.stopPropagation();
    });
    toggle.addEventListener("change", async () => {
      await updateMemoryEnabled(user.user_id, toggle.checked);
    });
    toggleCell.appendChild(toggle);

    const actionCell = document.createElement("td");
    const viewBtn = document.createElement("button");
    viewBtn.type = "button";
    viewBtn.className = "secondary btn-with-icon icon-only";
    viewBtn.innerHTML = '<i class="fa-solid fa-eye"></i>';
    viewBtn.title = t("common.view");
    viewBtn.setAttribute("aria-label", t("common.view"));
    viewBtn.addEventListener("click", (event) => {
      event.stopPropagation();
      openMemoryModal(user.user_id);
    });
    actionCell.appendChild(viewBtn);

    row.appendChild(idCell);
    row.appendChild(countCell);
    row.appendChild(timeCell);
    row.appendChild(toggleCell);
    row.appendChild(actionCell);
    row.addEventListener("click", () => openMemoryModal(user.user_id));
    elements.memoryUsersBody.appendChild(row);
  });
};

// 渲染记忆弹窗头部与操作区
const renderMemoryModalHeader = () => {
  const userId = state.memory.selectedId;
  if (!userId) {
    elements.memoryModalTitle.textContent = t("memory.modal.title");
    elements.memoryModalMeta.textContent = t("memory.modal.selectUser");
    elements.memoryModalEnableToggle.disabled = true;
    elements.memoryModalEnableToggle.checked = false;
    elements.memoryModalClearBtn.disabled = true;
    return;
  }
  elements.memoryModalTitle.textContent = t("memory.modal.titleWithUser", { userId });
  const recordCount = Array.isArray(state.memory.records)
    ? state.memory.records.length
    : 0;
  elements.memoryModalMeta.textContent = t("memory.modal.count", { count: recordCount });
  elements.memoryModalEnableToggle.disabled = false;
  elements.memoryModalEnableToggle.checked = Boolean(state.memory.enabled);
  elements.memoryModalClearBtn.disabled = recordCount <= 0;
};

// 渲染记忆记录列表
const renderMemoryRecords = () => {
  elements.memoryModalRecordBody.textContent = "";
  const records = Array.isArray(state.memory.records) ? state.memory.records : [];
  if (!records.length) {
    elements.memoryModalRecordEmpty.textContent = t("memory.records.empty");
    elements.memoryModalRecordEmpty.style.display = "block";
    return;
  }
  elements.memoryModalRecordEmpty.style.display = "none";
  records.forEach((record) => {
    const row = document.createElement("tr");

    const sessionCell = document.createElement("td");
    sessionCell.textContent = record.session_id || "-";

    const timeCell = document.createElement("td");
    timeCell.textContent = record.updated_time
      ? formatTimestamp(record.updated_time)
      : "-";

    const summaryCell = document.createElement("td");
    const summary = document.createElement("div");
    summary.className = "memory-summary";
    summary.textContent = record.summary || "-";
    summaryCell.appendChild(summary);

    const actionCell = document.createElement("td");
    const actionWrap = document.createElement("div");
    actionWrap.className = "table-actions";
    const editBtn = document.createElement("button");
    editBtn.type = "button";
    editBtn.className = "secondary btn-with-icon icon-only";
    editBtn.innerHTML = '<i class="fa-solid fa-pen"></i>';
    editBtn.title = t("common.edit");
    editBtn.setAttribute("aria-label", t("common.edit"));
    editBtn.addEventListener("click", () => {
      openMemoryRecordEditor(record);
    });
    const deleteBtn = document.createElement("button");
    deleteBtn.type = "button";
    deleteBtn.className = "danger btn-with-icon icon-only";
    deleteBtn.innerHTML = '<i class="fa-solid fa-trash"></i>';
    deleteBtn.title = t("common.delete");
    deleteBtn.setAttribute("aria-label", t("common.delete"));
    deleteBtn.addEventListener("click", () => {
      requestDeleteMemoryRecord(record.session_id);
    });
    actionWrap.appendChild(editBtn);
    actionWrap.appendChild(deleteBtn);
    actionCell.appendChild(actionWrap);

    row.appendChild(sessionCell);
    row.appendChild(timeCell);
    row.appendChild(summaryCell);
    row.appendChild(actionCell);
    elements.memoryModalRecordBody.appendChild(row);
  });
};

// 打开记忆记录编辑弹窗，允许管理员直接修改内容
const openMemoryRecordEditor = (record) => {
  const userId = state.memory.selectedId;
  const sessionId = String(record?.session_id || "").trim();
  if (!userId || !sessionId) {
    return;
  }
  state.memory.editingRecord = {
    user_id: userId,
    session_id: sessionId,
  };
  elements.memoryRecordEditTitle.textContent = t("memory.record.editTitle", { sessionId });
  const metaParts = [];
  metaParts.push(userId);
  if (record?.updated_time) {
    metaParts.push(t("memory.record.updatedAt", { time: formatTimestamp(record.updated_time) }));
  } else if (record?.created_time) {
    metaParts.push(t("memory.record.createdAt", { time: formatTimestamp(record.created_time) }));
  }
  elements.memoryRecordEditMeta.textContent = metaParts.join(" · ");
  elements.memoryRecordEditInput.value = String(record?.summary || "");
  elements.memoryRecordEditModal.classList.add("active");
};

// 关闭记忆编辑弹窗并清理状态
const closeMemoryRecordEditor = () => {
  elements.memoryRecordEditModal.classList.remove("active");
  elements.memoryRecordEditInput.value = "";
  elements.memoryRecordEditMeta.textContent = "";
  state.memory.editingRecord = null;
};

// 保存记忆编辑内容
const requestUpdateMemoryRecord = async () => {
  const editing = state.memory.editingRecord;
  if (!editing?.user_id || !editing?.session_id) {
    return;
  }
  const rawSummary = String(elements.memoryRecordEditInput.value || "");
  const trimmed = rawSummary.trim();
  if (!trimmed) {
    const confirmed = window.confirm(t("memory.record.emptyConfirm"));
    if (!confirmed) {
      return;
    }
  }
  const endpoint = `${getWunderBase()}/admin/memory/${encodeURIComponent(
    editing.user_id
  )}/${encodeURIComponent(editing.session_id)}`;
  try {
    const response = await fetch(endpoint, {
      method: "PUT",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ summary: rawSummary }),
    });
    if (!response.ok) {
      throw new Error(t("common.requestFailed", { status: response.status }));
    }
    await loadMemoryRecords(editing.user_id);
    await loadMemoryUsers();
    closeMemoryRecordEditor();
    notify(t("memory.record.updated"), "success");
  } catch (error) {
    notify(t("memory.record.updateFailed", { message: error.message }), "error");
  }
};

const openMemoryModal = async (userId) => {
  const cleaned = String(userId || "").trim();
  if (!cleaned) {
    return;
  }
  closeMemoryRecordEditor();
  state.memory.selectedId = cleaned;
  state.memory.records = [];
  state.memory.enabled = false;
  renderMemoryUsers();
  elements.memoryModalRecordBody.textContent = "";
  elements.memoryModalRecordEmpty.textContent = t("common.loading");
  elements.memoryModalRecordEmpty.style.display = "block";
  elements.memoryModalEnableToggle.disabled = true;
  elements.memoryModalClearBtn.disabled = true;
  elements.memoryModalMeta.textContent = t("common.loading");
  elements.memoryModal.classList.add("active");
  await loadMemoryRecords(cleaned);
};

const closeMemoryModal = () => {
  elements.memoryModal.classList.remove("active");
  closeMemoryRecordEditor();
};

// 拉取用户列表
export const loadMemoryUsers = async () => {
  ensureMemoryState();
  if (!ensureMemoryElements()) {
    return;
  }
  state.memory.search = String(elements.memorySearchInput.value || "").trim();
  const endpoint = `${getWunderBase()}/admin/memory/users`;
  elements.memoryUsersBody.textContent = "";
  elements.memoryUsersEmpty.textContent = t("common.loading");
  elements.memoryUsersEmpty.style.display = "block";
  renderMemoryUsersPagination(null);
  try {
    const response = await fetch(endpoint);
    if (!response.ok) {
      throw new Error(t("common.requestFailed", { status: response.status }));
    }
    const result = await response.json();
    state.memory.users = Array.isArray(result.users)
      ? result.users.map(normalizeMemoryUser)
      : [];
    state.memory.loaded = true;
    if (
      state.memory.selectedId &&
      !state.memory.users.some((item) => item.user_id === state.memory.selectedId)
    ) {
      state.memory.selectedId = "";
      state.memory.records = [];
      state.memory.enabled = false;
      closeMemoryModal();
    }
    renderMemoryUsers();
  } catch (error) {
    state.memory.users = [];
    elements.memoryUsersEmpty.textContent = t("common.loadFailedWithMessage", {
      message: error.message,
    });
    elements.memoryUsersEmpty.style.display = "block";
    renderMemoryUsersPagination(null);
    throw error;
  }
};

// 拉取指定用户的记忆记录
const loadMemoryRecords = async (userId) => {
  const cleaned = String(userId || "").trim();
  if (!cleaned) {
    return;
  }
  const endpoint = `${getWunderBase()}/admin/memory/${encodeURIComponent(cleaned)}`;
  try {
    const response = await fetch(endpoint);
    if (!response.ok) {
      throw new Error(t("common.requestFailed", { status: response.status }));
    }
    const result = await response.json();
    state.memory.enabled = Boolean(result.enabled);
    state.memory.records = Array.isArray(result.records)
      ? result.records.map(normalizeMemoryRecord)
      : [];
    renderMemoryModalHeader();
    renderMemoryRecords();
  } catch (error) {
    state.memory.records = [];
    elements.memoryModalRecordEmpty.textContent = t("common.loadFailedWithMessage", {
      message: error.message,
    });
    elements.memoryModalRecordEmpty.style.display = "block";
    notify(t("memory.records.loadFailed", { message: error.message }), "error");
  }
};

// 更新用户长期记忆开关
const updateMemoryEnabled = async (userId, enabled) => {
  const cleaned = String(userId || "").trim();
  if (!cleaned) {
    return;
  }
  const endpoint = `${getWunderBase()}/admin/memory/${encodeURIComponent(cleaned)}/enabled`;
  try {
    const response = await fetch(endpoint, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ enabled }),
    });
    if (!response.ok) {
      throw new Error(t("common.requestFailed", { status: response.status }));
    }
    const result = await response.json();
    const nextEnabled = Boolean(result.enabled);
    state.memory.users = state.memory.users.map((item) =>
      item.user_id === cleaned ? { ...item, enabled: nextEnabled } : item
    );
    if (state.memory.selectedId === cleaned) {
      state.memory.enabled = nextEnabled;
      elements.memoryModalEnableToggle.checked = nextEnabled;
    }
    renderMemoryUsers();
    renderMemoryModalHeader();
    notify(
      nextEnabled ? t("memory.enabled.on") : t("memory.enabled.off"),
      "success"
    );
  } catch (error) {
    notify(t("memory.enabled.updateFailed", { message: error.message }), "error");
    renderMemoryUsers();
  }
};

// 删除单条记忆记录
const requestDeleteMemoryRecord = async (sessionId) => {
  const userId = state.memory.selectedId;
  if (!userId || !sessionId) {
    return;
  }
  const endpoint = `${getWunderBase()}/admin/memory/${encodeURIComponent(
    userId
  )}/${encodeURIComponent(sessionId)}`;
  try {
    const response = await fetch(endpoint, { method: "DELETE" });
    if (!response.ok) {
      throw new Error(t("common.requestFailed", { status: response.status }));
    }
    await loadMemoryRecords(userId);
    await loadMemoryUsers();
    notify(t("memory.record.deleted"), "success");
  } catch (error) {
    notify(t("memory.record.deleteFailed", { message: error.message }), "error");
  }
};

// 清空用户所有记忆记录
const requestClearMemoryRecords = async () => {
  const userId = state.memory.selectedId;
  if (!userId) {
    return;
  }
  const endpoint = `${getWunderBase()}/admin/memory/${encodeURIComponent(userId)}`;
  try {
    const response = await fetch(endpoint, { method: "DELETE" });
    if (!response.ok) {
      throw new Error(t("common.requestFailed", { status: response.status }));
    }
    await loadMemoryRecords(userId);
    await loadMemoryUsers();
    notify(t("memory.records.cleared"), "success");
  } catch (error) {
    notify(t("memory.records.clearFailed", { message: error.message }), "error");
  }
};

const buildMemoryStatusBadge = (status) => {
  const label = String(status || "").trim() || "-";
  const badge = document.createElement("span");
  badge.className = "monitor-status";
  const lower = label.toLowerCase();
  if (label === t("memory.status.running") || lower === "running") {
    badge.classList.add("running");
  } else if (label === t("memory.status.queued") || lower === "queued") {
    badge.classList.add("waiting");
  } else if (label === t("memory.status.done") || lower === "done" || lower === "finished") {
    badge.classList.add("finished");
  } else if (label === t("memory.status.failed") || lower === "failed" || lower === "error") {
    badge.classList.add("error");
  }
  badge.textContent = label;
  return badge;
};

const resolveMemoryQueueTime = (item) =>
  item.started_time || item.queued_time || item.finished_time || "";

const renderMemoryQueueTable = (body, emptyElement, items, emptyText) => {
  body.textContent = "";
  if (!items.length) {
    emptyElement.textContent = emptyText;
    emptyElement.style.display = "block";
    return;
  }
  emptyElement.style.display = "none";
  items.forEach((item) => {
    const row = document.createElement("tr");
    const timeCell = document.createElement("td");
    const timeValue = resolveMemoryQueueTime(item);
    timeCell.textContent = timeValue ? formatTimestamp(timeValue) : "-";

    const sessionCell = document.createElement("td");
    sessionCell.textContent = item.session_id || "-";

    const userCell = document.createElement("td");
    userCell.textContent = item.user_id || "-";

    const statusCell = document.createElement("td");
    statusCell.appendChild(buildMemoryStatusBadge(item.status));

    const elapsedCell = document.createElement("td");
    elapsedCell.textContent = formatDuration(item.elapsed_s);

    const actionCell = document.createElement("td");
    const viewBtn = document.createElement("button");
    viewBtn.type = "button";
    viewBtn.className = "secondary btn-with-icon icon-only";
    viewBtn.innerHTML = '<i class="fa-solid fa-eye"></i>';
    viewBtn.title = t("common.view");
    viewBtn.setAttribute("aria-label", t("common.view"));
    viewBtn.addEventListener("click", (event) => {
      event.stopPropagation();
      openMemoryQueueDetail(item.task_id);
    });
    actionCell.appendChild(viewBtn);

    row.appendChild(timeCell);
    row.appendChild(sessionCell);
    row.appendChild(userCell);
    row.appendChild(statusCell);
    row.appendChild(elapsedCell);
    row.appendChild(actionCell);

    row.addEventListener("click", () => openMemoryQueueDetail(item.task_id));
    body.appendChild(row);
  });
};

// 渲染记忆体运行状态
const renderMemoryStatus = () => {
  const status = state.memory.status || {};
  const active = Array.isArray(status.active) ? status.active : [];
  const history = Array.isArray(status.history) ? status.history : [];
  const updatedAt = status.updatedAt;
  const updatedText = updatedAt
    ? t("memory.status.updatedAt", { time: formatTimestamp(updatedAt) })
    : "";
  elements.memoryStatusMeta.textContent = t("memory.status.meta", {
    active: active.length,
    history: history.length,
    updated: updatedText,
  });
  const activePage = resolveMemoryQueuePageSlice(active, "activePage");
  const historyPage = resolveMemoryQueuePageSlice(history, "historyPage");
  renderMemoryQueueTable(
    elements.memoryStatusActiveBody,
    elements.memoryStatusActiveEmpty,
    activePage.items,
    t("memory.status.emptyActive")
  );
  renderMemoryQueuePagination("active", activePage);
  renderMemoryQueueTable(
    elements.memoryStatusHistoryBody,
    elements.memoryStatusHistoryEmpty,
    historyPage.items,
    t("memory.status.emptyHistory")
  );
  renderMemoryQueuePagination("history", historyPage);
};

const renderMemoryQueueDetail = () => {
  const detail = state.memory.queueDetail;
  if (!detail) {
    elements.memoryQueueTitle.textContent = t("memory.queue.title");
    elements.memoryQueueMeta.textContent = t("memory.queue.notLoaded");
    elements.memoryQueueRequest.textContent = "";
    elements.memoryQueueResult.textContent = "";
    return;
  }
  elements.memoryQueueTitle.textContent = t("memory.queue.titleWithId", {
    taskId: detail.task_id || "-",
  });
  const metaParts = [];
  if (detail.user_id) {
    metaParts.push(detail.user_id);
  }
  if (detail.session_id) {
    metaParts.push(detail.session_id);
  }
  if (detail.status) {
    metaParts.push(detail.status);
  }
  if (Number.isFinite(detail.elapsed_s)) {
    metaParts.push(t("memory.queue.elapsed", { duration: formatDuration(detail.elapsed_s) }));
  }
  if (detail.started_time) {
    metaParts.push(t("memory.queue.startedAt", { time: formatTimestamp(detail.started_time) }));
  } else if (detail.queued_time) {
    metaParts.push(t("memory.queue.queuedAt", { time: formatTimestamp(detail.queued_time) }));
  }
  if (detail.finished_time) {
    metaParts.push(t("memory.queue.finishedAt", { time: formatTimestamp(detail.finished_time) }));
  }
  if (detail.error) {
    metaParts.push(t("memory.queue.error", { message: detail.error }));
  }
  elements.memoryQueueMeta.textContent = metaParts.join(" · ");
  const payload =
    detail.request && typeof detail.request === "object" ? detail.request : {};
  const text = JSON.stringify(payload, null, 2);
  elements.memoryQueueRequest.textContent = text && text !== "{}" ? text : "-";
  const resultText = String(detail.result || "").trim();
  elements.memoryQueueResult.textContent = resultText ? resultText : "-";
};

const loadMemoryQueueDetail = async (taskId) => {
  const cleaned = String(taskId || "").trim();
  if (!cleaned) {
    return;
  }
  const endpoint = `${getWunderBase()}/admin/memory/status/${encodeURIComponent(cleaned)}`;
  try {
    const response = await fetch(endpoint);
    if (!response.ok) {
      throw new Error(t("common.requestFailed", { status: response.status }));
    }
    const result = await response.json();
    state.memory.queueDetailId = cleaned;
    state.memory.queueDetail = normalizeMemoryQueueDetail(result);
    renderMemoryQueueDetail();
  } catch (error) {
    state.memory.queueDetail = null;
    elements.memoryQueueMeta.textContent = "";
    elements.memoryQueueRequest.textContent = t("common.loadFailedWithMessage", {
      message: error.message,
    });
    elements.memoryQueueResult.textContent = "-";
    notify(t("memory.queue.loadFailed", { message: error.message }), "error");
  }
};

const openMemoryQueueDetail = async (taskId) => {
  const cleaned = String(taskId || "").trim();
  if (!cleaned) {
    return;
  }
  elements.memoryQueueTitle.textContent = t("memory.queue.title");
  elements.memoryQueueMeta.textContent = t("common.loading");
  elements.memoryQueueRequest.textContent = t("common.loading");
  elements.memoryQueueResult.textContent = "";
  elements.memoryQueueModal.classList.add("active");
  await loadMemoryQueueDetail(cleaned);
};

const closeMemoryQueueDetail = () => {
  elements.memoryQueueModal.classList.remove("active");
  state.memory.queueDetailId = "";
  state.memory.queueDetail = null;
  elements.memoryQueueRequest.textContent = "";
  elements.memoryQueueResult.textContent = "";
};

// 拉取记忆体运行状态
export const loadMemoryStatus = async () => {
  ensureMemoryState();
  if (!ensureMemoryElements()) {
    return;
  }
  const endpoint = `${getWunderBase()}/admin/memory/status`;
  try {
    const response = await fetch(endpoint);
    if (!response.ok) {
      throw new Error(t("common.requestFailed", { status: response.status }));
    }
    const result = await response.json();
    state.memory.status = {
      active: Array.isArray(result.active)
        ? result.active.map(normalizeMemoryQueueItem)
        : [],
      history: Array.isArray(result.history)
        ? result.history.map(normalizeMemoryQueueItem)
        : [],
      updatedAt: new Date().toISOString(),
      updatedAtTs: Date.now(),
    };
    renderMemoryStatus();
  } catch (error) {
    elements.memoryStatusMeta.textContent = t("common.loadFailedWithMessage", {
      message: error.message,
    });
  }
};

const startMemoryPolling = () => {
  if (state.runtime.memoryPollTimer) {
    return;
  }
  loadMemoryStatus();
  state.runtime.memoryPollTimer = window.setInterval(() => {
    loadMemoryStatus();
  }, MEMORY_POLL_INTERVAL_MS);
};

const stopMemoryPolling = () => {
  if (!state.runtime.memoryPollTimer) {
    return;
  }
  window.clearInterval(state.runtime.memoryPollTimer);
  state.runtime.memoryPollTimer = null;
};

// 控制记忆体运行状态的轮询开关
export const toggleMemoryPolling = (active) => {
  ensureMemoryState();
  if (active) {
    startMemoryPolling();
  } else {
    stopMemoryPolling();
  }
};

// 初始化长期记忆面板交互
export const initMemoryPanel = () => {
  ensureMemoryState();
  if (!ensureMemoryElements()) {
    return;
  }
  elements.memorySearchInput.value = state.memory.search || "";
  elements.memoryRefreshBtn.addEventListener("click", async () => {
    await loadMemoryUsers();
  });
  elements.memorySearchInput.addEventListener("input", () => {
    state.memory.search = String(elements.memorySearchInput.value || "").trim();
    // 搜索条件变化时回到第一页，避免分页溢出
    state.memory.pagination.page = 1;
    renderMemoryUsers();
  });
  elements.memoryUsersPrevBtn.addEventListener("click", () => {
    updateMemoryUsersPage(-1);
  });
  elements.memoryUsersNextBtn.addEventListener("click", () => {
    updateMemoryUsersPage(1);
  });
  elements.memoryStatusActivePrevBtn.addEventListener("click", () => {
    updateMemoryQueuePage("activePage", -1);
  });
  elements.memoryStatusActiveNextBtn.addEventListener("click", () => {
    updateMemoryQueuePage("activePage", 1);
  });
  elements.memoryStatusHistoryPrevBtn.addEventListener("click", () => {
    updateMemoryQueuePage("historyPage", -1);
  });
  elements.memoryStatusHistoryNextBtn.addEventListener("click", () => {
    updateMemoryQueuePage("historyPage", 1);
  });
  elements.memoryModalEnableToggle.addEventListener("change", async () => {
    const userId = state.memory.selectedId;
    if (!userId) {
      return;
    }
    await updateMemoryEnabled(userId, elements.memoryModalEnableToggle.checked);
  });
  elements.memoryModalClearBtn.addEventListener("click", () => {
    requestClearMemoryRecords();
  });
  elements.memoryModalClose.addEventListener("click", closeMemoryModal);
  elements.memoryModalCloseBtn.addEventListener("click", closeMemoryModal);
  elements.memoryModal.addEventListener("click", (event) => {
    if (event.target === elements.memoryModal) {
      closeMemoryModal();
    }
  });
  elements.memoryRecordEditClose.addEventListener("click", closeMemoryRecordEditor);
  elements.memoryRecordEditCloseBtn.addEventListener("click", closeMemoryRecordEditor);
  elements.memoryRecordEditSave.addEventListener("click", requestUpdateMemoryRecord);
  elements.memoryRecordEditModal.addEventListener("click", (event) => {
    if (event.target === elements.memoryRecordEditModal) {
      closeMemoryRecordEditor();
    }
  });
  elements.memoryQueueClose.addEventListener("click", closeMemoryQueueDetail);
  elements.memoryQueueCloseBtn.addEventListener("click", closeMemoryQueueDetail);
  elements.memoryQueueModal.addEventListener("click", (event) => {
    if (event.target === elements.memoryQueueModal) {
      closeMemoryQueueDetail();
    }
  });
};

