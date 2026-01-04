import { elements } from "./elements.js?v=20260104-11";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { appendLog } from "./log.js?v=20251229-02";
import { notify } from "./notify.js";
import { loadMonitorData, setMonitorUserFilter } from "./monitor.js";
import { formatTokenCount } from "./utils.js?v=20251229-02";
import { t } from "./i18n.js?v=20260104-11";

const DEFAULT_USER_STATS_PAGE_SIZE = 100;

// 兼容旧版本状态结构，避免缓存旧 state.js 时导致空指针
const ensureUsersState = () => {
  if (!state.users) {
    state.users = {
      list: [],
      selectedId: "",
      loaded: false,
      search: "",
      pagination: {
        pageSize: DEFAULT_USER_STATS_PAGE_SIZE,
        page: 1,
      },
    };
  }
  if (!state.users.pagination || typeof state.users.pagination !== "object") {
    state.users.pagination = {
      pageSize: DEFAULT_USER_STATS_PAGE_SIZE,
      page: 1,
    };
  }
  if (
    !Number.isFinite(state.users.pagination.pageSize) ||
    state.users.pagination.pageSize <= 0
  ) {
    state.users.pagination.pageSize = DEFAULT_USER_STATS_PAGE_SIZE;
  }
  if (!Number.isFinite(state.users.pagination.page) || state.users.pagination.page < 1) {
    state.users.pagination.page = 1;
  }
  if (!state.panelLoaded) {
    state.panelLoaded = {};
  }
  if (typeof state.panelLoaded.users !== "boolean") {
    state.panelLoaded.users = false;
  }
};

// 统一检查用户管理面板依赖的 DOM 是否存在，避免绑定时报错
const ensureUserElements = () => {
  const requiredKeys = [
    "userRefreshBtn",
    "userSearchInput",
    "userStatsBody",
    "userStatsEmpty",
    "userStatsPagination",
    "userStatsPageInfo",
    "userStatsPrevBtn",
    "userStatsNextBtn",
    "userDetailTitle",
    "userDetailMeta",
  ];
  const missing = requiredKeys.filter((key) => !elements[key]);
  if (missing.length) {
    appendLog(t("users.domMissing", { nodes: missing.join(", ") }));
    return false;
  }
  return true;
};

// 统一处理搜索关键词，避免空值影响筛选逻辑
const getUserSearchKeyword = () => String(state.users.search || "").trim();

// 根据搜索关键词过滤用户列表，支持模糊匹配 user_id
const getFilteredUsers = () => {
  const keyword = getUserSearchKeyword();
  if (!keyword) {
    return state.users.list;
  }
  const lowered = keyword.toLowerCase();
  return state.users.list.filter((item) =>
    String(item?.user_id || "").toLowerCase().includes(lowered)
  );
};

// 解析用户统计分页大小，兜底为默认值
const resolveUserStatsPageSize = () => {
  const rawValue = Math.floor(Number(state.users.pagination?.pageSize));
  if (!Number.isFinite(rawValue) || rawValue <= 0) {
    return DEFAULT_USER_STATS_PAGE_SIZE;
  }
  return rawValue;
};

// 约束分页页码，避免超出范围
const clampUserStatsPage = (value, totalPages) => {
  const page = Number(value);
  if (!Number.isFinite(page) || page < 1) {
    return 1;
  }
  if (!Number.isFinite(totalPages) || totalPages <= 0) {
    return 1;
  }
  return Math.min(page, totalPages);
};

// 根据当前分页状态裁剪用户统计列表
const resolveUserStatsPageSlice = (users) => {
  const pageSize = resolveUserStatsPageSize();
  const total = Array.isArray(users) ? users.length : 0;
  const totalPages = Math.max(1, Math.ceil(total / pageSize));
  const currentPage = clampUserStatsPage(state.users.pagination?.page, totalPages);
  state.users.pagination.page = currentPage;
  const startIndex = (currentPage - 1) * pageSize;
  const pageUsers = users.slice(startIndex, startIndex + pageSize);
  return { total, totalPages, currentPage, pageSize, users: pageUsers };
};

// 渲染用户统计分页控件
const renderUserStatsPagination = (pageData) => {
  const { userStatsPagination, userStatsPageInfo, userStatsPrevBtn, userStatsNextBtn } =
    elements;
  if (!userStatsPagination || !userStatsPageInfo || !userStatsPrevBtn || !userStatsNextBtn) {
    return;
  }
  if (!pageData || pageData.total <= 0) {
    userStatsPagination.style.display = "none";
    return;
  }
  userStatsPagination.style.display = "flex";
  userStatsPageInfo.textContent = t("pagination.info", {
    total: pageData.total,
    current: pageData.currentPage,
    pages: pageData.totalPages,
    size: pageData.pageSize,
  });
  userStatsPrevBtn.disabled = pageData.currentPage <= 1;
  userStatsNextBtn.disabled = pageData.currentPage >= pageData.totalPages;
};

// 切换用户统计分页页码并刷新列表
const updateUserStatsPage = (delta) => {
  const current = Number(state.users.pagination?.page) || 1;
  const nextPage = Math.max(1, current + delta);
  state.users.pagination.page = nextPage;
  renderUserStats();
};

// 规范化用户统计数据，避免后端字段缺失导致渲染异常
const normalizeUserStats = (item) => ({
  user_id: String(item?.user_id || ""),
  active_sessions: Number(item?.active_sessions) || 0,
  history_sessions: Number(item?.history_sessions) || 0,
  total_sessions: Number(item?.total_sessions) || 0,
  chat_records: Number(item?.chat_records) || 0,
  tool_calls: Number(item?.tool_calls) || 0,
  token_usage: Number(item?.token_usage) || 0,
});

// 汇总全部用户统计，便于展示全局视角
const resolveAllUserStats = () => {
  const summary = {
    user_count: 0,
    active_sessions: 0,
    history_sessions: 0,
    total_sessions: 0,
    chat_records: 0,
    tool_calls: 0,
    token_usage: 0,
  };
  if (!Array.isArray(state.users.list)) {
    return summary;
  }
  summary.user_count = state.users.list.length;
  state.users.list.forEach((item) => {
    summary.active_sessions += Number(item?.active_sessions) || 0;
    summary.history_sessions += Number(item?.history_sessions) || 0;
    summary.total_sessions += Number(item?.total_sessions) || 0;
    summary.chat_records += Number(item?.chat_records) || 0;
    summary.tool_calls += Number(item?.tool_calls) || 0;
    // 累加所有用户的 token_usage，展示总占用 Token
    summary.token_usage += Number(item?.token_usage) || 0;
  });
  return summary;
};

// 刷新用户详情标题与操作区状态
const renderUserDetailHeader = () => {
  const allStats = resolveAllUserStats();
  if (!state.users.selectedId) {
    elements.userDetailTitle.textContent = t("users.all");
    elements.userDetailMeta.textContent = t("users.detail.allMeta", {
      users: allStats.user_count,
      sessions: allStats.total_sessions,
      records: allStats.chat_records,
      tools: allStats.tool_calls,
      active: allStats.active_sessions,
      tokens: formatTokenCount(allStats.token_usage),
    });
    return;
  }
  const user = state.users.list.find((item) => item.user_id === state.users.selectedId);
  if (!user) {
    elements.userDetailTitle.textContent = t("users.all");
    elements.userDetailMeta.textContent = t("users.detail.allMeta", {
      users: allStats.user_count,
      sessions: allStats.total_sessions,
      records: allStats.chat_records,
      tools: allStats.tool_calls,
      active: allStats.active_sessions,
      tokens: formatTokenCount(allStats.token_usage),
    });
    return;
  }
  elements.userDetailTitle.textContent = user.user_id || "-";
  elements.userDetailMeta.textContent = t("users.detail.meta", {
    sessions: user.total_sessions,
    records: user.chat_records,
    tools: user.tool_calls,
    active: user.active_sessions,
    tokens: formatTokenCount(user.token_usage),
  });
};

// 切换当前选中的用户，同时同步监控筛选
const applyUserSelection = (userId) => {
  const normalizedId = String(userId || "").trim();
  state.users.selectedId = normalizedId;
  renderUserStats();
  renderUserDetailHeader();
  setMonitorUserFilter(normalizedId);
};

// 选中全部用户视图
const selectAllUsers = () => applyUserSelection("");

// 渲染用户统计表格
const renderUserStats = () => {
  elements.userStatsBody.textContent = "";
  const hasUsers = Array.isArray(state.users.list) && state.users.list.length > 0;
  if (!hasUsers) {
    elements.userStatsEmpty.textContent = t("users.empty");
    elements.userStatsEmpty.style.display = "block";
    renderUserStatsPagination(null);
    return;
  }
  const keyword = getUserSearchKeyword();
  const filteredUsers = getFilteredUsers();
  const allStats = resolveAllUserStats();

  const renderRow = (user, options = {}) => {
    const isAll = options.isAll === true;
    const row = document.createElement("tr");
    if (isAll ? !state.users.selectedId : user.user_id === state.users.selectedId) {
      row.classList.add("active");
    }

    const userCell = document.createElement("td");
    userCell.textContent = isAll ? t("users.all") : user.user_id || "-";

    const chatCell = document.createElement("td");
    const chatBtn = document.createElement("button");
    chatBtn.type = "button";
    chatBtn.className = "link-button";
    chatBtn.textContent = `${user.chat_records}`;
    chatBtn.addEventListener("click", (event) => {
      event.stopPropagation();
      if (isAll) {
        selectAllUsers();
      } else {
        selectUser(user.user_id);
      }
    });
    chatCell.appendChild(chatBtn);

    const sessionCell = document.createElement("td");
    sessionCell.textContent = `${user.total_sessions}`;

    const tokenCell = document.createElement("td");
    tokenCell.textContent = formatTokenCount(user.token_usage);

    const activeCell = document.createElement("td");
    activeCell.textContent = `${user.active_sessions}`;

    const actionCell = document.createElement("td");
    if (isAll) {
      const allBtn = document.createElement("button");
      allBtn.type = "button";
      allBtn.className = "secondary";
      allBtn.textContent = t("users.selectAll");
      allBtn.addEventListener("click", (event) => {
        event.stopPropagation();
        selectAllUsers();
      });
      actionCell.appendChild(allBtn);
    } else {
      const deleteBtn = document.createElement("button");
      deleteBtn.type = "button";
      deleteBtn.className = "danger";
      deleteBtn.textContent = t("common.delete");
      deleteBtn.addEventListener("click", (event) => {
        event.stopPropagation();
        requestDeleteUser(user.user_id);
      });
      actionCell.appendChild(deleteBtn);
    }

    row.appendChild(userCell);
    row.appendChild(chatCell);
    row.appendChild(sessionCell);
    row.appendChild(tokenCell);
    row.appendChild(activeCell);
    row.appendChild(actionCell);
    row.addEventListener("click", () => {
      if (isAll) {
        selectAllUsers();
      } else {
        selectUser(user.user_id);
      }
    });
    elements.userStatsBody.appendChild(row);
  };

  renderRow(
    {
      chat_records: allStats.chat_records,
      total_sessions: allStats.total_sessions,
      tool_calls: allStats.tool_calls,
      active_sessions: allStats.active_sessions,
      token_usage: allStats.token_usage,
    },
    { isAll: true }
  );

  if (filteredUsers.length === 0) {
    elements.userStatsEmpty.textContent = keyword
      ? t("users.search.empty")
      : t("users.empty");
    elements.userStatsEmpty.style.display = "block";
    renderUserStatsPagination(null);
    return;
  }
  elements.userStatsEmpty.style.display = "none";
  // 按分页切片渲染，避免长列表一次性加载
  const pageData = resolveUserStatsPageSlice(filteredUsers);
  renderUserStatsPagination(pageData);
  pageData.users.forEach((user) => renderRow(user));
};

// 拉取用户统计数据并刷新列表
export const loadUserStats = async () => {
  ensureUsersState();
  if (!ensureUserElements()) {
    return;
  }
  state.users.search = String(elements.userSearchInput.value || "").trim();
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/users`;
  elements.userStatsBody.textContent = "";
  elements.userStatsEmpty.textContent = t("common.loading");
  elements.userStatsEmpty.style.display = "block";
  try {
    const response = await fetch(endpoint);
    if (!response.ok) {
      throw new Error(t("common.requestFailed", { status: response.status }));
    }
    const result = await response.json();
    state.users.list = Array.isArray(result.users) ? result.users.map(normalizeUserStats) : [];
    state.users.loaded = true;
    state.panelLoaded.users = true;
    if (state.users.selectedId) {
      const exists = state.users.list.some((item) => item.user_id === state.users.selectedId);
      if (!exists) {
        state.users.selectedId = "";
      }
    }
    renderUserStats();
    renderUserDetailHeader();
    setMonitorUserFilter(state.users.selectedId);
  } catch (error) {
    state.users.list = [];
    elements.userStatsEmpty.textContent = t("common.loadFailedWithMessage", {
      message: error.message,
    });
    elements.userStatsEmpty.style.display = "block";
    throw error;
  }
};

// 切换当前选中的用户
const selectUser = (userId) => {
  ensureUsersState();
  if (!ensureUserElements()) {
    return;
  }
  if (!userId) {
    return;
  }
  applyUserSelection(userId);
};

// 删除当前选中的用户及其数据
const requestDeleteUser = async (userId) => {
  ensureUsersState();
  if (!ensureUserElements()) {
    return;
  }
  const targetId = userId || state.users.selectedId;
  if (!targetId) {
    return;
  }
  const confirmed = window.confirm(
    t("users.deleteConfirm", { userId: targetId })
  );
  if (!confirmed) {
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/users/${encodeURIComponent(targetId)}`;
  const response = await fetch(endpoint, { method: "DELETE" });
  if (!response.ok) {
    const message = t("users.deleteFailed", { status: response.status });
    appendLog(message);
    notify(message, "error");
    return;
  }
  const result = await response.json();
  notify(result.message || t("users.deleteSuccess"), "success");
  if (state.users.selectedId === targetId) {
    state.users.selectedId = "";
    renderUserDetailHeader();
    setMonitorUserFilter("");
  }
  try {
    await loadUserStats();
    // 用户管理页仅需要会话列表，避免触发监控图表的完整刷新
    await loadMonitorData({ mode: "sessions" });
  } catch (error) {
    appendLog(t("users.refreshFailed", { message: error.message }));
  }
};

// 用户管理面板初始化：绑定刷新与删除操作
export const initUserManagementPanel = () => {
  ensureUsersState();
  if (!ensureUserElements()) {
    return;
  }
  elements.userSearchInput.value = state.users.search || "";
  elements.userSearchInput.addEventListener("input", (event) => {
    state.users.search = String(event.target.value || "").trim();
    // 搜索条件变化时回到第一页，避免分页溢出
    state.users.pagination.page = 1;
    if (
      state.users.selectedId &&
      state.users.search &&
      !state.users.selectedId.toLowerCase().includes(state.users.search.toLowerCase())
    ) {
      state.users.selectedId = "";
      setMonitorUserFilter("");
    }
    renderUserStats();
    renderUserDetailHeader();
  });
  renderUserStats();
  renderUserDetailHeader();
  elements.userStatsPrevBtn.addEventListener("click", () => {
    updateUserStatsPage(-1);
  });
  elements.userStatsNextBtn.addEventListener("click", () => {
    updateUserStatsPage(1);
  });
  elements.userRefreshBtn.addEventListener("click", async () => {
    try {
      await loadUserStats();
      notify(t("users.refreshSuccess"), "success");
    } catch (error) {
      appendLog(t("users.refreshFailed", { message: error.message }));
      notify(t("users.refreshFailed", { message: error.message }), "error");
    }
  });
};




