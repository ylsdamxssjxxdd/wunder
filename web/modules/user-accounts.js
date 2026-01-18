import { elements } from "./elements.js?v=20260118-07";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { appendLog } from "./log.js?v=20260108-02";
import { notify } from "./notify.js";
import { formatTimestamp } from "./utils.js?v=20251229-02";
import { t } from "./i18n.js?v=20260118-07";

const DEFAULT_USER_ACCOUNT_PAGE_SIZE = 50;

const ensureUserAccountsState = () => {
  if (!state.userAccounts) {
    state.userAccounts = {
      list: [],
      selectedId: "",
      loaded: false,
      search: "",
      pagination: {
        pageSize: DEFAULT_USER_ACCOUNT_PAGE_SIZE,
        page: 1,
        total: 0,
      },
      toolAccess: {},
    };
  }
  if (!state.userAccounts.pagination || typeof state.userAccounts.pagination !== "object") {
    state.userAccounts.pagination = {
      pageSize: DEFAULT_USER_ACCOUNT_PAGE_SIZE,
      page: 1,
      total: 0,
    };
  }
  if (!Number.isFinite(state.userAccounts.pagination.pageSize) || state.userAccounts.pagination.pageSize <= 0) {
    state.userAccounts.pagination.pageSize = DEFAULT_USER_ACCOUNT_PAGE_SIZE;
  }
  if (!Number.isFinite(state.userAccounts.pagination.page) || state.userAccounts.pagination.page < 1) {
    state.userAccounts.pagination.page = 1;
  }
  if (!Number.isFinite(state.userAccounts.pagination.total) || state.userAccounts.pagination.total < 0) {
    state.userAccounts.pagination.total = 0;
  }
  if (!state.panelLoaded) {
    state.panelLoaded = {};
  }
  if (typeof state.panelLoaded.userAccounts !== "boolean") {
    state.panelLoaded.userAccounts = false;
  }
};

const ensureUserAccountElements = () => {
  const requiredKeys = [
    "userAccountSearchInput",
    "userAccountRefreshBtn",
    "userAccountCreateBtn",
    "userAccountTableBody",
    "userAccountEmpty",
    "userAccountPagination",
    "userAccountPageInfo",
    "userAccountPrevBtn",
    "userAccountNextBtn",
    "userAccountModal",
    "userAccountModalClose",
    "userAccountModalCancel",
    "userAccountModalSave",
    "userAccountFormUsername",
    "userAccountFormEmail",
    "userAccountFormPassword",
    "userAccountFormAccess",
    "userAccountFormStatus",
    "userAccountFormRoles",
    "userAccountPasswordModal",
    "userAccountPasswordClose",
    "userAccountPasswordCancel",
    "userAccountPasswordSave",
    "userAccountPasswordInput",
    "userAccountToolModal",
    "userAccountToolClose",
    "userAccountToolCancel",
    "userAccountToolSave",
    "userAccountToolUser",
    "userAccountToolDefault",
    "userAccountToolList",
    "userAccountToolEmpty",
  ];
  const missing = requiredKeys.filter((key) => !elements[key]);
  if (missing.length) {
    appendLog(t("userAccounts.domMissing", { nodes: missing.join(", ") }));
    return false;
  }
  return true;
};

const normalizeUserAccount = (item) => {
  const id = String(item?.id || item?.user_id || item?.userId || "").trim();
  const username = String(item?.username || item?.user_name || id).trim();
  const activeSessions = Number(item?.active_sessions ?? item?.activeSessions ?? 0);
  const online =
    typeof item?.online === "boolean" ? item.online : Number.isFinite(activeSessions) && activeSessions > 0;
  return {
    id,
    username,
    email: item?.email || "",
    access_level: String(item?.access_level || item?.accessLevel || "A"),
    status: String(item?.status || "active"),
    roles: Array.isArray(item?.roles) ? item.roles : [],
    last_login_at: item?.last_login_at ?? item?.lastLoginAt ?? null,
    is_demo: Boolean(item?.is_demo || item?.isDemo),
    active_sessions: Number.isFinite(activeSessions) ? activeSessions : 0,
    online,
  };
};

const formatLoginTime = (value) => {
  const ts = Number(value);
  if (!Number.isFinite(ts) || ts <= 0) {
    return "-";
  }
  return formatTimestamp(ts * 1000);
};

const resolveUserAccountPageSize = () => {
  const rawValue = Math.floor(Number(state.userAccounts.pagination?.pageSize));
  if (!Number.isFinite(rawValue) || rawValue <= 0) {
    return DEFAULT_USER_ACCOUNT_PAGE_SIZE;
  }
  return rawValue;
};

const renderUserAccountPagination = () => {
  const { userAccountPagination, userAccountPageInfo, userAccountPrevBtn, userAccountNextBtn } =
    elements;
  const total = Number(state.userAccounts.pagination?.total) || 0;
  if (!total) {
    userAccountPagination.style.display = "none";
    return;
  }
  const pageSize = resolveUserAccountPageSize();
  const totalPages = Math.max(1, Math.ceil(total / pageSize));
  const currentPage = Math.min(Math.max(1, state.userAccounts.pagination.page), totalPages);
  state.userAccounts.pagination.page = currentPage;
  userAccountPagination.style.display = "flex";
  userAccountPageInfo.textContent = t("pagination.info", {
    total,
    current: currentPage,
    pages: totalPages,
    size: pageSize,
  });
  userAccountPrevBtn.disabled = currentPage <= 1;
  userAccountNextBtn.disabled = currentPage >= totalPages;
};

const renderUserAccountRows = () => {
  elements.userAccountTableBody.textContent = "";
  if (!state.userAccounts.list.length) {
    elements.userAccountEmpty.textContent = t("userAccounts.empty");
    elements.userAccountEmpty.style.display = "block";
    renderUserAccountPagination();
    return;
  }
  elements.userAccountEmpty.style.display = "none";
  state.userAccounts.list.forEach((user) => {
    const row = document.createElement("tr");

    const userCell = document.createElement("td");
    userCell.textContent = user.username || user.id || "-";

    const emailCell = document.createElement("td");
    emailCell.textContent = user.email || "-";

    const accessCell = document.createElement("td");
    const accessSelect = document.createElement("select");
    ["A", "B", "C"].forEach((level) => {
      const option = document.createElement("option");
      option.value = level;
      option.textContent = level;
      accessSelect.appendChild(option);
    });
    accessSelect.value = user.access_level || "A";
    accessSelect.addEventListener("change", (event) => {
      event.stopPropagation();
      updateUserAccount(user.id, { access_level: accessSelect.value });
    });
    accessCell.appendChild(accessSelect);

    const statusCell = document.createElement("td");
    const statusSelect = document.createElement("select");
    ["active", "disabled"].forEach((status) => {
      const option = document.createElement("option");
      option.value = status;
      option.textContent = status;
      statusSelect.appendChild(option);
    });
    statusSelect.value = user.status || "active";
    statusSelect.addEventListener("change", (event) => {
      event.stopPropagation();
      updateUserAccount(user.id, { status: statusSelect.value });
    });
    statusCell.appendChild(statusSelect);

    const onlineCell = document.createElement("td");
    onlineCell.textContent = user.online
      ? t("userAccounts.status.online")
      : t("userAccounts.status.offline");

    const rolesCell = document.createElement("td");
    const rolesText = document.createElement("span");
    rolesText.textContent = user.roles.length ? user.roles.join(",") : "-";
    const rolesEdit = document.createElement("button");
    rolesEdit.type = "button";
    rolesEdit.className = "link-button";
    rolesEdit.textContent = t("common.edit");
    rolesEdit.addEventListener("click", (event) => {
      event.stopPropagation();
      requestUpdateRoles(user);
    });
    rolesCell.appendChild(rolesText);
    rolesCell.appendChild(rolesEdit);

    const loginCell = document.createElement("td");
    loginCell.textContent = formatLoginTime(user.last_login_at);

    const passwordCell = document.createElement("td");
    const resetBtn = document.createElement("button");
    resetBtn.type = "button";
    resetBtn.className = "secondary";
    resetBtn.textContent = t("userAccounts.action.resetPassword");
    resetBtn.addEventListener("click", (event) => {
      event.stopPropagation();
      openPasswordModal(user);
    });
    passwordCell.appendChild(resetBtn);

    const actionCell = document.createElement("td");
    const toolBtn = document.createElement("button");
    toolBtn.type = "button";
    toolBtn.className = "secondary";
    toolBtn.textContent = t("userAccounts.action.toolAccess");
    toolBtn.addEventListener("click", (event) => {
      event.stopPropagation();
      openToolAccessModal(user);
    });
    const deleteBtn = document.createElement("button");
    deleteBtn.type = "button";
    deleteBtn.className = "danger";
    deleteBtn.textContent = t("common.delete");
    deleteBtn.addEventListener("click", (event) => {
      event.stopPropagation();
      requestDeleteUser(user.id);
    });
    actionCell.appendChild(toolBtn);
    actionCell.appendChild(deleteBtn);

    row.appendChild(userCell);
    row.appendChild(emailCell);
    row.appendChild(accessCell);
    row.appendChild(statusCell);
    row.appendChild(onlineCell);
    row.appendChild(rolesCell);
    row.appendChild(loginCell);
    row.appendChild(passwordCell);
    row.appendChild(actionCell);

    elements.userAccountTableBody.appendChild(row);
  });
  renderUserAccountPagination();
};

const openModal = (modal) => {
  if (!modal) return;
  modal.classList.add("active");
};

const closeModal = (modal) => {
  if (!modal) return;
  modal.classList.remove("active");
};

const getUserAccountSearchKeyword = () => String(state.userAccounts.search || "").trim();

export const loadUserAccounts = async () => {
  ensureUserAccountsState();
  if (!ensureUserAccountElements()) {
    return;
  }
  state.userAccounts.search = String(elements.userAccountSearchInput.value || "").trim();
  const keyword = getUserAccountSearchKeyword();
  const pageSize = resolveUserAccountPageSize();
  const currentPage = Math.max(1, Number(state.userAccounts.pagination.page) || 1);
  const offset = (currentPage - 1) * pageSize;
  const wunderBase = getWunderBase();
  const params = new URLSearchParams();
  params.set("offset", String(offset));
  params.set("limit", String(pageSize));
  if (keyword) {
    params.set("keyword", keyword);
  }
  const endpoint = `${wunderBase}/admin/user_accounts?${params.toString()}`;
  elements.userAccountTableBody.textContent = "";
  elements.userAccountEmpty.textContent = t("common.loading");
  elements.userAccountEmpty.style.display = "block";
  try {
    const response = await fetch(endpoint);
    if (!response.ok) {
      throw new Error(t("common.requestFailed", { status: response.status }));
    }
    const result = await response.json();
    const payload = result?.data || {};
    const items = Array.isArray(payload.items) ? payload.items : [];
    state.userAccounts.list = items.map(normalizeUserAccount);
    state.userAccounts.pagination.total = Number(payload.total) || 0;
    state.userAccounts.loaded = true;
    state.panelLoaded.userAccounts = true;
    renderUserAccountRows();
  } catch (error) {
    state.userAccounts.list = [];
    elements.userAccountEmpty.textContent = t("common.loadFailedWithMessage", {
      message: error.message,
    });
    elements.userAccountEmpty.style.display = "block";
    renderUserAccountPagination();
    throw error;
  }
};

const updateUserAccount = async (userId, payload) => {
  if (!userId) {
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/user_accounts/${encodeURIComponent(userId)}`;
  try {
    const response = await fetch(endpoint, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    if (!response.ok) {
      const message = t("common.requestFailed", { status: response.status });
      notify(message, "error");
      return;
    }
    notify(t("userAccounts.toast.updateSuccess"), "success");
    await loadUserAccounts();
  } catch (error) {
    notify(t("userAccounts.toast.updateFailed", { message: error.message }), "error");
  }
};

const requestUpdateRoles = async (user) => {
  if (!user?.id) {
    return;
  }
  const current = user.roles.length ? user.roles.join(",") : "";
  const raw = window.prompt(t("userAccounts.roles.prompt"), current);
  if (raw === null) {
    return;
  }
  const roles = raw
    .split(/[,\s]+/)
    .map((item) => item.trim())
    .filter(Boolean);
  await updateUserAccount(user.id, { roles });
};

const requestDeleteUser = async (userId) => {
  if (!userId) {
    return;
  }
  const confirmed = window.confirm(t("userAccounts.deleteConfirm", { userId }));
  if (!confirmed) {
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/user_accounts/${encodeURIComponent(userId)}`;
  try {
    const response = await fetch(endpoint, { method: "DELETE" });
    if (!response.ok) {
      notify(t("userAccounts.deleteFailed", { status: response.status }), "error");
      return;
    }
    notify(t("userAccounts.deleteSuccess"), "success");
    await loadUserAccounts();
  } catch (error) {
    notify(t("userAccounts.deleteFailed", { status: error.message }), "error");
  }
};

const openCreateModal = () => {
  elements.userAccountFormUsername.value = "";
  elements.userAccountFormEmail.value = "";
  elements.userAccountFormPassword.value = "";
  elements.userAccountFormAccess.value = "A";
  elements.userAccountFormStatus.value = "active";
  elements.userAccountFormRoles.value = "";
  if (elements.userAccountModalTitle) {
    elements.userAccountModalTitle.textContent = t("userAccounts.modal.create.title");
  }
  openModal(elements.userAccountModal);
};

const submitCreateUser = async () => {
  const username = String(elements.userAccountFormUsername.value || "").trim();
  const password = String(elements.userAccountFormPassword.value || "").trim();
  if (!username || !password) {
    notify(t("userAccounts.toast.createRequired"), "warn");
    return;
  }
  const email = String(elements.userAccountFormEmail.value || "").trim();
  const roles = String(elements.userAccountFormRoles.value || "")
    .split(/[,\s]+/)
    .map((item) => item.trim())
    .filter(Boolean);
  const payload = {
    username,
    email: email || null,
    password,
    access_level: elements.userAccountFormAccess.value || "A",
    status: elements.userAccountFormStatus.value || "active",
    roles,
  };
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/user_accounts`;
  try {
    const response = await fetch(endpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    if (!response.ok) {
      notify(t("userAccounts.toast.createFailed", { status: response.status }), "error");
      return;
    }
    closeModal(elements.userAccountModal);
    notify(t("userAccounts.toast.createSuccess"), "success");
    await loadUserAccounts();
  } catch (error) {
    notify(t("userAccounts.toast.createFailed", { status: error.message }), "error");
  }
};

let passwordTarget = null;

const openPasswordModal = (user) => {
  passwordTarget = user;
  elements.userAccountPasswordInput.value = "";
  if (elements.userAccountPasswordTitle) {
    const label = user?.username || user?.id || "-";
    elements.userAccountPasswordTitle.textContent = t("userAccounts.modal.password.titleWithUser", {
      user: label,
    });
  }
  openModal(elements.userAccountPasswordModal);
};

const submitPasswordReset = async () => {
  if (!passwordTarget?.id) {
    return;
  }
  const password = String(elements.userAccountPasswordInput.value || "").trim();
  if (!password) {
    notify(t("userAccounts.toast.passwordRequired"), "warn");
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/user_accounts/${encodeURIComponent(passwordTarget.id)}/password`;
  try {
    const response = await fetch(endpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ password }),
    });
    if (!response.ok) {
      notify(t("userAccounts.toast.passwordFailed", { status: response.status }), "error");
      return;
    }
    closeModal(elements.userAccountPasswordModal);
    notify(t("userAccounts.toast.passwordSuccess"), "success");
  } catch (error) {
    notify(t("userAccounts.toast.passwordFailed", { status: error.message }), "error");
  }
};

let toolAccessTarget = null;
let toolGroupsCache = [];

const buildToolOptions = (list) =>
  (Array.isArray(list) ? list : [])
    .map((item) => {
      if (!item) return null;
      const name = item.name || item.tool_name || item.toolName;
      if (!name) return null;
      return {
        value: String(name),
        label: String(name),
        description: String(item.description || ""),
      };
    })
    .filter(Boolean);

const buildToolGroups = (payload) => [
  { label: t("userAccounts.toolGroup.builtin"), options: buildToolOptions(payload.builtin_tools) },
  { label: t("userAccounts.toolGroup.mcp"), options: buildToolOptions(payload.mcp_tools) },
  { label: t("userAccounts.toolGroup.a2a"), options: buildToolOptions(payload.a2a_tools) },
  { label: t("userAccounts.toolGroup.skills"), options: buildToolOptions(payload.skills) },
  { label: t("userAccounts.toolGroup.knowledge"), options: buildToolOptions(payload.knowledge_tools) },
  { label: t("userAccounts.toolGroup.user"), options: buildToolOptions(payload.user_tools) },
  { label: t("userAccounts.toolGroup.shared"), options: buildToolOptions(payload.shared_tools) },
].filter((group) => group.options.length > 0);

const renderToolOptions = (groups, selected) => {
  const list = elements.userAccountToolList;
  list.textContent = "";
  if (!groups.length) {
    elements.userAccountToolEmpty.style.display = "block";
    return;
  }
  elements.userAccountToolEmpty.style.display = "none";
  const selectedSet = new Set(selected || []);
  const disabled = elements.userAccountToolDefault.checked;
  groups.forEach((group) => {
    const title = document.createElement("div");
    title.className = "user-account-tool-group-title";
    title.textContent = group.label;
    list.appendChild(title);
    group.options.forEach((option) => {
      const item = document.createElement("div");
      item.className = "tool-item";
      const checkbox = document.createElement("input");
      checkbox.type = "checkbox";
      checkbox.value = option.value;
      checkbox.checked = selectedSet.has(option.value);
      checkbox.disabled = disabled;
      const label = document.createElement("label");
      const desc = option.description ? `<span class="muted">${option.description}</span>` : "";
      label.innerHTML = `<strong>${option.label}</strong>${desc}`;
      item.addEventListener("click", (event) => {
        if (event.target === checkbox || checkbox.disabled) {
          return;
        }
        checkbox.checked = !checkbox.checked;
      });
      item.appendChild(checkbox);
      item.appendChild(label);
      list.appendChild(item);
    });
  });
};

const loadToolCatalog = async (userId) => {
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/tools?user_id=${encodeURIComponent(userId)}`;
  const response = await fetch(endpoint);
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const payload = await response.json();
  return buildToolGroups(payload || {});
};

const loadToolAccess = async (userId) => {
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/user_accounts/${encodeURIComponent(userId)}/tool_access`;
  const response = await fetch(endpoint);
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const payload = await response.json();
  return payload?.data?.allowed_tools ?? null;
};

const syncToolAccessToggle = () => {
  const useDefault = elements.userAccountToolDefault.checked;
  if (elements.userAccountToolList) {
    elements.userAccountToolList.classList.toggle("is-disabled", useDefault);
    elements.userAccountToolList
      .querySelectorAll('input[type="checkbox"]')
      .forEach((input) => {
        input.disabled = useDefault;
      });
  }
};

const openToolAccessModal = async (user) => {
  if (!user?.id) {
    return;
  }
  toolAccessTarget = user;
  elements.userAccountToolUser.textContent = user.username || user.id || "-";
  elements.userAccountToolDefault.checked = true;
  elements.userAccountToolList.textContent = "";
  elements.userAccountToolEmpty.textContent = t("common.loading");
  elements.userAccountToolEmpty.style.display = "block";
  syncToolAccessToggle();
  openModal(elements.userAccountToolModal);
  try {
    const [groups, allowed] = await Promise.all([
      loadToolCatalog(user.id),
      loadToolAccess(user.id),
    ]);
    toolGroupsCache = groups;
    const useDefault = allowed === null;
    elements.userAccountToolDefault.checked = useDefault;
    renderToolOptions(groups, Array.isArray(allowed) ? allowed : []);
    syncToolAccessToggle();
  } catch (error) {
    notify(t("userAccounts.toast.toolLoadFailed", { message: error.message }), "error");
  }
};

const collectSelectedTools = () =>
  Array.from(elements.userAccountToolList.querySelectorAll('input[type="checkbox"]'))
    .filter((input) => input.checked)
    .map((input) => input.value);

const saveToolAccess = async () => {
  if (!toolAccessTarget?.id) {
    return;
  }
  const useDefault = elements.userAccountToolDefault.checked;
  const allowed = useDefault ? null : collectSelectedTools();
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/user_accounts/${encodeURIComponent(toolAccessTarget.id)}/tool_access`;
  try {
    const response = await fetch(endpoint, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ allowed_tools: allowed }),
    });
    if (!response.ok) {
      notify(t("userAccounts.toast.toolSaveFailed", { status: response.status }), "error");
      return;
    }
    notify(t("userAccounts.toast.toolSaveSuccess"), "success");
    closeModal(elements.userAccountToolModal);
  } catch (error) {
    notify(t("userAccounts.toast.toolSaveFailed", { status: error.message }), "error");
  }
};

export const initUserAccountsPanel = () => {
  ensureUserAccountsState();
  if (!ensureUserAccountElements()) {
    return;
  }
  elements.userAccountSearchInput.value = state.userAccounts.search || "";
  elements.userAccountSearchInput.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      state.userAccounts.pagination.page = 1;
      loadUserAccounts().catch((error) => {
        appendLog(t("userAccounts.toast.loadFailed", { message: error.message }));
        notify(t("userAccounts.toast.loadFailed", { message: error.message }), "error");
      });
    }
  });
  elements.userAccountRefreshBtn.addEventListener("click", async () => {
    try {
      await loadUserAccounts();
      notify(t("userAccounts.toast.loadSuccess"), "success");
    } catch (error) {
      appendLog(t("userAccounts.toast.loadFailed", { message: error.message }));
      notify(t("userAccounts.toast.loadFailed", { message: error.message }), "error");
    }
  });
  elements.userAccountCreateBtn.addEventListener("click", openCreateModal);
  elements.userAccountModalClose?.addEventListener("click", () => closeModal(elements.userAccountModal));
  elements.userAccountModalCancel.addEventListener("click", () => closeModal(elements.userAccountModal));
  elements.userAccountModalSave.addEventListener("click", submitCreateUser);
  elements.userAccountPasswordClose?.addEventListener("click", () => closeModal(elements.userAccountPasswordModal));
  elements.userAccountPasswordCancel.addEventListener("click", () => closeModal(elements.userAccountPasswordModal));
  elements.userAccountPasswordSave.addEventListener("click", submitPasswordReset);
  elements.userAccountToolClose?.addEventListener("click", () => closeModal(elements.userAccountToolModal));
  elements.userAccountToolCancel.addEventListener("click", () => closeModal(elements.userAccountToolModal));
  elements.userAccountToolSave.addEventListener("click", saveToolAccess);
  elements.userAccountToolDefault.addEventListener("change", syncToolAccessToggle);
  elements.userAccountPrevBtn.addEventListener("click", async () => {
    state.userAccounts.pagination.page = Math.max(1, state.userAccounts.pagination.page - 1);
    await loadUserAccounts();
  });
  elements.userAccountNextBtn.addEventListener("click", async () => {
    state.userAccounts.pagination.page = state.userAccounts.pagination.page + 1;
    await loadUserAccounts();
  });
};
