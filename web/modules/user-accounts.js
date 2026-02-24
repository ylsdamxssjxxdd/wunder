import { elements } from "./elements.js?v=20260215-01";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { appendLog } from "./log.js?v=20260108-02";
import { notify } from "./notify.js";
import { formatTimestamp } from "./utils.js?v=20251229-02";
import { t } from "./i18n.js?v=20260215-01";
import { ensureOrgUnitsLoaded, getOrgUnitOptions } from "./org-units.js?v=20260210-01";

const DEFAULT_USER_ACCOUNT_PAGE_SIZE = 50;
const DEFAULT_TEST_USER_PASSWORD = "Test@123456";
const DEFAULT_TEST_USER_PER_UNIT = 1;
const MAX_TEST_USERS_PER_UNIT = 200;

const ensureUserAccountsState = () => {
  if (!state.userAccounts) {
    state.userAccounts = {
      list: [],
      selectedId: "",
      loaded: false,
      search: "",
      loading: false,
      pendingReload: false,
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
    "userAccountSeedBtn",
    "userAccountCleanupBtn",
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
    "userAccountSeedModal",
    "userAccountSeedModalClose",
    "userAccountSeedModalCancel",
    "userAccountSeedModalConfirm",
    "userAccountSeedCount",
    "userAccountSeedHint",
    "userAccountFormUsername",
    "userAccountFormEmail",
    "userAccountFormPassword",
    "userAccountFormUnit",
    "userAccountFormStatus",
    "userAccountSettingsModal",
    "userAccountSettingsClose",
    "userAccountSettingsCancel",
    "userAccountSettingsUser",
    "userAccountQuotaInput",
    "userAccountQuotaSave",
    "userAccountQuotaMeta",
    "userAccountSettingsPasswordInput",
    "userAccountSettingsPasswordSave",
    "userAccountSettingsUnitSelect",
    "userAccountSettingsUnitSave",
    "userAccountSettingsRolesInput",
    "userAccountSettingsRolesSave",
    "userAccountSettingsDelete",
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
  const dailyQuota = Number(item?.daily_quota ?? item?.dailyQuota ?? 0);
  const dailyUsed = Number(item?.daily_quota_used ?? item?.dailyQuotaUsed ?? 0);
  const unit = item?.unit || item?.unit_profile || null;
  const unitId = String(item?.unit_id || item?.unitId || unit?.id || unit?.unit_id || "").trim();
  const unitPath = String(unit?.path_name || unit?.pathName || "").trim();
  const unitName = String(unit?.name || "").trim();
  let dailyRemaining = Number(item?.daily_quota_remaining ?? item?.dailyQuotaRemaining);
  const safeDailyQuota = Number.isFinite(dailyQuota) ? Math.max(0, Math.floor(dailyQuota)) : 0;
  const safeDailyUsed = Number.isFinite(dailyUsed) ? Math.max(0, Math.floor(dailyUsed)) : 0;
  if (!Number.isFinite(dailyRemaining)) {
    dailyRemaining = Math.max(safeDailyQuota - safeDailyUsed, 0);
  }
  return {
    id,
    username,
    email: item?.email || "",
    unit_id: unitId,
    unit_name: unitName,
    unit_path: unitPath,
    unit_level: Number.isFinite(Number(unit?.level)) ? Number(unit?.level) : null,
    status: String(item?.status || "active"),
    roles: Array.isArray(item?.roles) ? item.roles : [],
    daily_quota: safeDailyQuota,
    daily_quota_used: safeDailyUsed,
    daily_quota_remaining: Number.isFinite(dailyRemaining) ? Math.max(0, Math.floor(dailyRemaining)) : 0,
    daily_quota_date: String(item?.daily_quota_date || item?.dailyQuotaDate || "").trim(),
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

const resolveUnitLabel = (user) => {
  if (!user) {
    return "-";
  }
  if (user.unit_name) {
    return user.unit_name;
  }
  const fallback = String(user.unit_path || "").trim();
  if (fallback) {
    const parts = fallback.split("/").map((part) => part.trim()).filter(Boolean);
    if (parts.length) {
      return parts[parts.length - 1];
    }
  }
  return user.unit_id || "-";
};

const formatUnitLevel = (user) => {
  if (!user) {
    return "-";
  }
  const level = Number(user.unit_level);
  if (!Number.isFinite(level) || level <= 0) {
    return "-";
  }
  const labels = ["一", "二", "三", "四"];
  if (level >= 1 && level <= labels.length) {
    return labels[level - 1];
  }
  return String(level);
};

const formatQuotaValue = (user) => {
  if (!user) {
    return "-";
  }
  const total = Number(user.daily_quota);
  if (!Number.isFinite(total)) {
    return "-";
  }
  const safeTotal = Math.max(0, Math.floor(total));
  const remaining = Number(user.daily_quota_remaining);
  if (!Number.isFinite(remaining)) {
    return String(safeTotal);
  }
  const safeRemaining = Math.max(0, Math.floor(remaining));
  if (!safeTotal) {
    return "0";
  }
  return `${safeRemaining} / ${safeTotal}`;
};

const formatQuotaMeta = (user) => {
  if (!user) {
    return "";
  }
  const total = Number.isFinite(user.daily_quota) ? Math.max(0, Math.floor(user.daily_quota)) : 0;
  const used = Number.isFinite(user.daily_quota_used) ? Math.max(0, Math.floor(user.daily_quota_used)) : 0;
  const remaining = Number.isFinite(user.daily_quota_remaining)
    ? Math.max(0, Math.floor(user.daily_quota_remaining))
    : Math.max(total - used, 0);
  return t("userAccounts.modal.settings.quota.meta", { used, remaining, total });
};

const resolveUnitOptions = () =>
  getOrgUnitOptions({
    includeRoot: true,
    rootLabel: t("userAccounts.unit.default"),
  });

const syncUnitSelect = (select, selected) => {
  if (!select) {
    return;
  }
  const options = resolveUnitOptions();
  select.textContent = "";
  options.forEach((option) => {
    const node = document.createElement("option");
    node.value = option.value;
    node.textContent = option.label;
    select.appendChild(node);
  });
  select.value = selected || "";
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
  const busy = state.userAccounts.loading;
  userAccountPrevBtn.disabled = busy || currentPage <= 1;
  userAccountNextBtn.disabled = busy || currentPage >= totalPages;
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
  const fragment = document.createDocumentFragment();
  state.userAccounts.list.forEach((user) => {
    const row = document.createElement("tr");

    const userCell = document.createElement("td");
    userCell.textContent = user.username || user.id || "-";

    const emailCell = document.createElement("td");
    emailCell.textContent = user.email || "-";

    const unitCell = document.createElement("td");
    unitCell.textContent = resolveUnitLabel(user);
    unitCell.title = resolveUnitLabel(user);

    const unitLevelCell = document.createElement("td");
    unitLevelCell.textContent = formatUnitLevel(user);

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

    const quotaCell = document.createElement("td");
    quotaCell.textContent = formatQuotaValue(user);

    const loginCell = document.createElement("td");
    loginCell.textContent = formatLoginTime(user.last_login_at);

    const actionCell = document.createElement("td");
    const settingsBtn = document.createElement("button");
    settingsBtn.type = "button";
    settingsBtn.className = "secondary";
    settingsBtn.textContent = t("userAccounts.action.settings");
    settingsBtn.addEventListener("click", (event) => {
      event.stopPropagation();
      openSettingsModal(user);
    });
    actionCell.appendChild(settingsBtn);

    row.appendChild(userCell);
    row.appendChild(emailCell);
    row.appendChild(unitCell);
    row.appendChild(unitLevelCell);
    row.appendChild(statusCell);
    row.appendChild(quotaCell);
    row.appendChild(loginCell);
    row.appendChild(actionCell);

    fragment.appendChild(row);
  });
  elements.userAccountTableBody.appendChild(fragment);
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

const setUserAccountsLoading = (loading) => {
  state.userAccounts.loading = loading;
  if (elements.userAccountRefreshBtn) {
    elements.userAccountRefreshBtn.disabled = loading;
  }
  if (elements.userAccountPrevBtn) {
    elements.userAccountPrevBtn.disabled = loading || state.userAccounts.pagination.page <= 1;
  }
  if (elements.userAccountNextBtn) {
    const total = Number(state.userAccounts.pagination?.total) || 0;
    const pageSize = resolveUserAccountPageSize();
    const totalPages = total ? Math.max(1, Math.ceil(total / pageSize)) : 1;
    elements.userAccountNextBtn.disabled =
      loading || state.userAccounts.pagination.page >= totalPages;
  }
};

export const loadUserAccounts = async () => {
  ensureUserAccountsState();
  if (!ensureUserAccountElements()) {
    return;
  }
  if (state.userAccounts.loading) {
    state.userAccounts.pendingReload = true;
    return;
  }
  try {
    await ensureOrgUnitsLoaded({ silent: true });
  } catch (error) {
    appendLog(t("userAccounts.toast.unitLoadFailed", { message: error.message }));
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
  const shouldShowLoading = !state.userAccounts.loaded || !state.userAccounts.list.length;
  if (shouldShowLoading) {
    elements.userAccountEmpty.textContent = t("common.loading");
    elements.userAccountEmpty.style.display = "block";
  }
  setUserAccountsLoading(true);
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
  } finally {
    setUserAccountsLoading(false);
    if (state.userAccounts.pendingReload) {
      state.userAccounts.pendingReload = false;
      loadUserAccounts().catch((error) => {
        appendLog(t("userAccounts.toast.loadFailed", { message: error.message }));
      });
    }
  }
};

const updateUserAccount = async (userId, payload) => {
  if (!userId) {
    return false;
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
      return false;
    }
    notify(t("userAccounts.toast.updateSuccess"), "success");
    await loadUserAccounts();
    return true;
  } catch (error) {
    notify(t("userAccounts.toast.updateFailed", { message: error.message }), "error");
    return false;
  }
};

const requestDeleteUser = async (userId, options = {}) => {
  if (!userId) {
    return false;
  }
  const confirmed = window.confirm(t("userAccounts.deleteConfirm", { userId }));
  if (!confirmed) {
    return false;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/user_accounts/${encodeURIComponent(userId)}`;
  try {
    const response = await fetch(endpoint, { method: "DELETE" });
    if (!response.ok) {
      notify(t("userAccounts.deleteFailed", { status: response.status }), "error");
      return false;
    }
    notify(t("userAccounts.deleteSuccess"), "success");
    await loadUserAccounts();
    if (typeof options.onSuccess === "function") {
      options.onSuccess();
    }
    return true;
  } catch (error) {
    notify(t("userAccounts.deleteFailed", { status: error.message }), "error");
    return false;
  }
};

const openCreateModal = () => {
  elements.userAccountFormUsername.value = "";
  elements.userAccountFormEmail.value = "";
  elements.userAccountFormPassword.value = "";
  syncUnitSelect(elements.userAccountFormUnit, "");
  elements.userAccountFormStatus.value = "active";
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
  const unitId = String(elements.userAccountFormUnit.value || "").trim();
  const payload = {
    username,
    email: email || null,
    password,
    unit_id: unitId || null,
    status: elements.userAccountFormStatus.value || "active",
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

let seedBusy = false;
let cleanupBusy = false;

const setSeedBusy = (busy) => {
  seedBusy = busy;
  if (elements.userAccountSeedBtn) {
    elements.userAccountSeedBtn.disabled = busy;
  }
  if (elements.userAccountSeedModalConfirm) {
    elements.userAccountSeedModalConfirm.disabled = busy;
  }
  if (elements.userAccountSeedCount) {
    elements.userAccountSeedCount.disabled = busy;
  }
};

const setCleanupBusy = (busy) => {
  cleanupBusy = busy;
  if (elements.userAccountCleanupBtn) {
    elements.userAccountCleanupBtn.disabled = busy;
  }
};

const parseSeedCount = () => {
  const raw = Number(elements.userAccountSeedCount.value);
  if (!Number.isFinite(raw)) {
    return null;
  }
  const count = Math.floor(raw);
  if (count <= 0 || count > MAX_TEST_USERS_PER_UNIT) {
    return null;
  }
  return count;
};

const openSeedModal = () => {
  if (seedBusy) {
    return;
  }
  elements.userAccountSeedCount.value = DEFAULT_TEST_USER_PER_UNIT;
  if (elements.userAccountSeedHint) {
    elements.userAccountSeedHint.textContent = t("userAccounts.modal.seed.hint", {
      password: DEFAULT_TEST_USER_PASSWORD,
      max: MAX_TEST_USERS_PER_UNIT,
    });
  }
  openModal(elements.userAccountSeedModal);
};

const submitSeedUsers = async () => {
  if (seedBusy) {
    return;
  }
  const perUnit = parseSeedCount();
  if (!perUnit) {
    notify(t("userAccounts.toast.seedCountInvalid", { max: MAX_TEST_USERS_PER_UNIT }), "warn");
    return;
  }
  const confirmed = window.confirm(t("userAccounts.seed.confirm", { count: perUnit }));
  if (!confirmed) {
    return;
  }
  setSeedBusy(true);
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/user_accounts/test/seed`;
  try {
    const response = await fetch(endpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ per_unit: perUnit }),
    });
    if (!response.ok) {
      notify(t("userAccounts.toast.seedFailed", { status: response.status }), "error");
      return;
    }
    const payload = await response.json();
    const data = payload?.data || {};
    const created = Number(data.created) || 0;
    const unitCount = Number(data.unit_count) || 0;
    const password = data.password || DEFAULT_TEST_USER_PASSWORD;
    closeModal(elements.userAccountSeedModal);
    notify(
      t("userAccounts.toast.seedSuccess", {
        created,
        unitCount,
        perUnit,
        password,
      }),
      "success"
    );
    await loadUserAccounts();
  } catch (error) {
    notify(t("userAccounts.toast.seedFailed", { status: error.message }), "error");
  } finally {
    setSeedBusy(false);
  }
};

const requestCleanupTestUsers = async () => {
  if (cleanupBusy || seedBusy) {
    return;
  }
  const confirmed = window.confirm(t("userAccounts.cleanup.confirm"));
  if (!confirmed) {
    return;
  }
  setCleanupBusy(true);
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/user_accounts/test/cleanup`;
  try {
    const response = await fetch(endpoint, { method: "POST" });
    if (!response.ok) {
      notify(t("userAccounts.toast.cleanupFailed", { status: response.status }), "error");
      return;
    }
    const payload = await response.json();
    const deleted = Number(payload?.deleted_users) || 0;
    if (deleted > 0) {
      notify(t("userAccounts.toast.cleanupSuccess", { deleted }), "success");
    } else {
      notify(t("userAccounts.toast.cleanupEmpty"), "info");
    }
    await loadUserAccounts();
  } catch (error) {
    notify(t("userAccounts.toast.cleanupFailed", { status: error.message }), "error");
  } finally {
    setCleanupBusy(false);
  }
};

let settingsTarget = null;
let toolSaveTimer = null;

const resolveRoleSelection = (roles) => {
  if (Array.isArray(roles) && (roles.includes("admin") || roles.includes("super_admin"))) {
    return "admin";
  }
  return "user";
};

const syncSettingsTarget = (user) => {
  settingsTarget = user;
  if (!user) {
    return;
  }
  elements.userAccountSettingsUser.textContent = user.username || user.id || "-";
  elements.userAccountQuotaInput.value = Number.isFinite(user.daily_quota) ? user.daily_quota : "";
  elements.userAccountQuotaMeta.textContent = formatQuotaMeta(user);
  elements.userAccountSettingsPasswordInput.value = "";
  syncUnitSelect(elements.userAccountSettingsUnitSelect, user.unit_id || "");
  elements.userAccountSettingsRolesInput.value = resolveRoleSelection(user.roles);
};

const refreshSettingsTarget = () => {
  if (!settingsTarget?.id) {
    return;
  }
  const updated = state.userAccounts.list.find((item) => item.id === settingsTarget.id);
  if (updated) {
    syncSettingsTarget(updated);
  }
};

const submitPasswordReset = async () => {
  if (!settingsTarget?.id) {
    return;
  }
  const password = String(elements.userAccountSettingsPasswordInput.value || "").trim();
  if (!password) {
    notify(t("userAccounts.toast.passwordRequired"), "warn");
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/user_accounts/${encodeURIComponent(settingsTarget.id)}/password`;
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
    elements.userAccountSettingsPasswordInput.value = "";
    notify(t("userAccounts.toast.passwordSuccess"), "success");
  } catch (error) {
    notify(t("userAccounts.toast.passwordFailed", { status: error.message }), "error");
  }
};

const saveQuota = async () => {
  if (!settingsTarget?.id) {
    return;
  }
  const raw = Number(elements.userAccountQuotaInput.value);
  if (!Number.isFinite(raw) || raw < 0) {
    notify(t("userAccounts.toast.quotaInvalid"), "warn");
    return;
  }
  const ok = await updateUserAccount(settingsTarget.id, { daily_quota: Math.floor(raw) });
  if (ok) {
    refreshSettingsTarget();
  }
};

const saveRoles = async () => {
  if (!settingsTarget?.id) {
    return;
  }
  const role = String(elements.userAccountSettingsRolesInput.value || "").trim();
  const roles = role ? [role] : [];
  const ok = await updateUserAccount(settingsTarget.id, { roles });
  if (ok) {
    refreshSettingsTarget();
  }
};

const saveUnit = async () => {
  if (!settingsTarget?.id) {
    return;
  }
  const unitId = String(elements.userAccountSettingsUnitSelect.value || "").trim();
  const ok = await updateUserAccount(settingsTarget.id, { unit_id: unitId });
  if (ok) {
    refreshSettingsTarget();
  }
};

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

const scheduleToolSave = (options = {}) => {
  if (toolSaveTimer) {
    clearTimeout(toolSaveTimer);
  }
  const delay = Number.isFinite(options.delay) ? Math.max(0, options.delay) : 400;
  const silent = options.silent !== false;
  toolSaveTimer = setTimeout(() => {
    saveToolAccess({ silent }).catch(() => {});
  }, delay);
};

const setToolListDisabled = (list, disabled) => {
  if (!list) {
    return;
  }
  list.classList.toggle("is-disabled", disabled);
  list.querySelectorAll('input[type="checkbox"]').forEach((input) => {
    input.disabled = disabled;
  });
};

const renderToolOptions = (list, empty, groups, selected, options = {}) => {
  if (!list || !empty) {
    return;
  }
  list.textContent = "";
  if (!groups.length) {
    empty.style.display = "block";
    return;
  }
  empty.style.display = "none";
  const selectedSet = new Set(selected || []);
  const disabled = options.disabled === true;
  const onChange = options.onChange || (() => scheduleToolSave({ silent: true }));
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
      checkbox.addEventListener("change", () => onChange());
      const label = document.createElement("label");
      const desc = option.description ? `<span class="muted">${option.description}</span>` : "";
      label.innerHTML = `<strong>${option.label}</strong>${desc}`;
      item.addEventListener("click", (event) => {
        if (event.target === checkbox || checkbox.disabled) {
          return;
        }
        checkbox.checked = !checkbox.checked;
        checkbox.dispatchEvent(new Event("change", { bubbles: true }));
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
  return {
    allowed: payload?.data?.allowed_tools ?? null,
  };
};

const syncToolAccessToggle = () => {
  const useDefault = elements.userAccountToolDefault.checked;
  setToolListDisabled(elements.userAccountToolList, useDefault);
};

const openSettingsModal = async (user) => {
  if (!user?.id) {
    return;
  }
  syncSettingsTarget(user);
  elements.userAccountToolDefault.checked = true;
  elements.userAccountToolList.textContent = "";
  elements.userAccountToolEmpty.textContent = t("common.loading");
  elements.userAccountToolEmpty.style.display = "block";
  syncToolAccessToggle();
  openModal(elements.userAccountSettingsModal);
  try {
    const [groups, access] = await Promise.all([
      loadToolCatalog(user.id),
      loadToolAccess(user.id),
    ]);
    const allowed = access?.allowed ?? null;
    const useDefault = allowed === null;
    elements.userAccountToolDefault.checked = useDefault;
    renderToolOptions(
      elements.userAccountToolList,
      elements.userAccountToolEmpty,
      groups,
      Array.isArray(allowed) ? allowed : [],
      { disabled: useDefault }
    );
    syncToolAccessToggle();
  } catch (error) {
    notify(t("userAccounts.toast.toolLoadFailed", { message: error.message }), "error");
  }
};

const collectSelectedTools = (list) => {
  if (!list) {
    return [];
  }
  return Array.from(list.querySelectorAll('input[type="checkbox"]'))
    .filter((input) => input.checked)
    .map((input) => input.value);
};

const saveToolAccess = async (options = {}) => {
  if (!settingsTarget?.id) {
    return;
  }
  const silent = options.silent === true;
  const useDefault = elements.userAccountToolDefault.checked;
  const allowed = useDefault ? null : collectSelectedTools(elements.userAccountToolList);
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/user_accounts/${encodeURIComponent(settingsTarget.id)}/tool_access`;
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
    if (!silent) {
      notify(t("userAccounts.toast.toolSaveSuccess"), "success");
    }
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
  elements.userAccountSeedBtn.addEventListener("click", openSeedModal);
  elements.userAccountCleanupBtn.addEventListener("click", requestCleanupTestUsers);
  elements.userAccountCreateBtn.addEventListener("click", openCreateModal);
  elements.userAccountModalClose?.addEventListener("click", () => closeModal(elements.userAccountModal));
  elements.userAccountModalCancel.addEventListener("click", () => closeModal(elements.userAccountModal));
  elements.userAccountModalSave.addEventListener("click", submitCreateUser);
  elements.userAccountSeedModalClose?.addEventListener("click", () =>
    closeModal(elements.userAccountSeedModal)
  );
  elements.userAccountSeedModalCancel.addEventListener("click", () =>
    closeModal(elements.userAccountSeedModal)
  );
  elements.userAccountSeedModalConfirm.addEventListener("click", submitSeedUsers);
  elements.userAccountSettingsClose?.addEventListener("click", () => closeModal(elements.userAccountSettingsModal));
  elements.userAccountSettingsCancel.addEventListener("click", () => closeModal(elements.userAccountSettingsModal));
  elements.userAccountQuotaSave.addEventListener("click", saveQuota);
  elements.userAccountSettingsPasswordSave.addEventListener("click", submitPasswordReset);
  elements.userAccountSettingsUnitSave.addEventListener("click", saveUnit);
  elements.userAccountSettingsRolesSave.addEventListener("click", saveRoles);
  elements.userAccountSettingsDelete.addEventListener("click", () => {
    requestDeleteUser(settingsTarget?.id, {
      onSuccess: () => closeModal(elements.userAccountSettingsModal),
    });
  });
  elements.userAccountToolDefault.addEventListener("change", () => {
    syncToolAccessToggle();
    scheduleToolSave({ silent: true });
  });
  elements.userAccountPrevBtn.addEventListener("click", async () => {
    state.userAccounts.pagination.page = Math.max(1, state.userAccounts.pagination.page - 1);
    await loadUserAccounts();
  });
  elements.userAccountNextBtn.addEventListener("click", async () => {
    state.userAccounts.pagination.page = state.userAccounts.pagination.page + 1;
    await loadUserAccounts();
  });
  setCleanupBusy(false);
};


