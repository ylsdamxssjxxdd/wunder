import { elements } from "./elements.js?v=20261011-01";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { appendLog } from "./log.js?v=20261011-01";
import { notify } from "./notify.js";
import { formatTimestamp } from "./utils.js?v=20251229-02";
import { t, getCurrentLanguage } from "./i18n.js?v=20261011-01";
// 危险操作复用预设面板的「影响面预览 + 二次确认」弹层（与用户管理面板同款做法）
import { openImpactConfirmModal } from "./preset-agents.js?v=20261007-01";

// 契约见 docs/本地云端连接落地方案.md §3.2/§3.3：
// - GET  /wunder/admin/cloud/devices?user_id=&offset=&limit=
// - GET  /wunder/admin/cloud/calls?user_id=&device_id=&model=&status=&offset=&limit=
// - GET  /wunder/admin/cloud/device_logs?user_id=&device_id=&level=&category=&offset=&limit=
// - DELETE /wunder/admin/cloud/devices/{id} → { data: { revoked: true } }
// 统一响应包裹为 { data: { items, total } } / 失败 { error: { code, message } }。

const DEFAULT_CLOUD_PAGE_SIZE = 50;
const CLOUD_TABS = ["devices", "calls", "logs"];
const CALL_STATUS_VALUES = ["admitted", "ok", "upstream_error", "quota_blocked", "queue_timeout"];
const LOG_LEVEL_VALUES = ["critical", "error", "warn", "info"];

// 各子页的 DOM 键与端点路径
const CLOUD_VIEWS = {
  devices: {
    path: "devices",
    tbody: "cloudDeviceTableBody",
    empty: "cloudDeviceEmpty",
    pagination: "cloudDevicePagination",
    pageInfo: "cloudDevicePageInfo",
    prev: "cloudDevicePrevBtn",
    next: "cloudDeviceNextBtn",
  },
  calls: {
    path: "calls",
    tbody: "cloudCallTableBody",
    empty: "cloudCallEmpty",
    pagination: "cloudCallPagination",
    pageInfo: "cloudCallPageInfo",
    prev: "cloudCallPrevBtn",
    next: "cloudCallNextBtn",
  },
  logs: {
    path: "device_logs",
    tbody: "cloudLogTableBody",
    empty: "cloudLogEmpty",
    pagination: "cloudLogPagination",
    pageInfo: "cloudLogPageInfo",
    prev: "cloudLogPrevBtn",
    next: "cloudLogNextBtn",
  },
};

const ensureCloudState = () => {
  if (!state.cloud) {
    state.cloud = { activeTab: "devices", devices: null, calls: null, logs: null };
  }
  if (!state.panelLoaded) {
    state.panelLoaded = {};
  }
  if (typeof state.panelLoaded.cloud !== "boolean") {
    state.panelLoaded.cloud = false;
  }
  CLOUD_TABS.forEach((tab) => {
    if (!state.cloud[tab]) {
      state.cloud[tab] = {
        list: [],
        loading: false,
        loaded: false,
        pendingReload: false,
        pagination: {
          pageSize: DEFAULT_CLOUD_PAGE_SIZE,
          page: 1,
          total: 0,
        },
      };
    }
    const view = state.cloud[tab];
    if (!view.pagination || typeof view.pagination !== "object") {
      view.pagination = { pageSize: DEFAULT_CLOUD_PAGE_SIZE, page: 1, total: 0 };
    }
    const pagination = view.pagination;
    if (!Number.isFinite(pagination.pageSize) || pagination.pageSize <= 0) {
      pagination.pageSize = DEFAULT_CLOUD_PAGE_SIZE;
    }
    if (!Number.isFinite(pagination.page) || pagination.page < 1) {
      pagination.page = 1;
    }
    if (!Number.isFinite(pagination.total) || pagination.total < 0) {
      pagination.total = 0;
    }
    if (!Array.isArray(view.list)) {
      view.list = [];
    }
  });
  if (!CLOUD_TABS.includes(state.cloud.activeTab)) {
    state.cloud.activeTab = "devices";
  }
};

const ensureCloudElements = () => {
  const requiredKeys = [
    "cloudRefreshBtn",
    "cloudTabDevices",
    "cloudTabCalls",
    "cloudTabLogs",
    "cloudTabContentDevices",
    "cloudTabContentCalls",
    "cloudTabContentLogs",
    "cloudDeviceUserInput",
    "cloudDeviceStatusFilter",
    "cloudDeviceResetBtn",
    "cloudDeviceTableBody",
    "cloudDeviceEmpty",
    "cloudDevicePagination",
    "cloudDevicePageInfo",
    "cloudDevicePrevBtn",
    "cloudDeviceNextBtn",
    "cloudCallUserInput",
    "cloudCallDeviceInput",
    "cloudCallModelInput",
    "cloudCallStatusFilter",
    "cloudCallResetBtn",
    "cloudCallTableBody",
    "cloudCallEmpty",
    "cloudCallPagination",
    "cloudCallPageInfo",
    "cloudCallPrevBtn",
    "cloudCallNextBtn",
    "cloudLogUserInput",
    "cloudLogDeviceInput",
    "cloudLogLevelFilter",
    "cloudLogCategoryInput",
    "cloudLogResetBtn",
    "cloudLogTableBody",
    "cloudLogEmpty",
    "cloudLogPagination",
    "cloudLogPageInfo",
    "cloudLogPrevBtn",
    "cloudLogNextBtn",
  ];
  const missing = requiredKeys.filter((key) => !elements[key]);
  if (missing.length) {
    appendLog(t("userAccounts.domMissing", { nodes: missing.join(", ") }));
    return false;
  }
  return true;
};

const extractResponseMessage = async (response, fallback) => {
  try {
    const payload = await response.json();
    return payload?.error?.message || payload?.message || payload?.detail?.message || fallback;
  } catch {
    return fallback;
  }
};

const readInputValue = (node) => String(node?.value || "").trim();

// 契约时间戳为秒（REAL）；对毫秒量级做防御性兼容
const toEpochMs = (value) => {
  const ts = Number(value);
  if (!Number.isFinite(ts) || ts <= 0) {
    return 0;
  }
  return ts > 1e12 ? ts : ts * 1000;
};

const formatCloudTime = (value) => {
  const ms = toEpochMs(value);
  if (!ms) {
    return "-";
  }
  return formatTimestamp(ms);
};

// 最近在线的相对时间展示；悬浮 title 提供绝对时间
const formatRelativeSeen = (value) => {
  const ms = toEpochMs(value);
  if (!ms) {
    return "-";
  }
  const diffSeconds = Math.round((Date.now() - ms) / 1000);
  if (!Number.isFinite(diffSeconds)) {
    return "-";
  }
  try {
    const formatter = new Intl.RelativeTimeFormat(getCurrentLanguage(), { numeric: "auto" });
    if (Math.abs(diffSeconds) < 60) {
      return formatter.format(-diffSeconds, "second");
    }
    const diffMinutes = Math.round(diffSeconds / 60);
    if (Math.abs(diffMinutes) < 60) {
      return formatter.format(-diffMinutes, "minute");
    }
    const diffHours = Math.round(diffMinutes / 60);
    if (Math.abs(diffHours) < 48) {
      return formatter.format(-diffHours, "hour");
    }
    return formatter.format(-Math.round(diffHours / 24), "day");
  } catch {
    return formatTimestamp(ms);
  }
};

const formatCount = (value) => {
  const num = Number(value);
  return Number.isFinite(num) ? String(num) : "-";
};

const normalizeCloudDevice = (item) => ({
  device_id: String(item?.device_id || "").trim(),
  user_id: String(item?.user_id || "").trim(),
  client: String(item?.client || "").trim(),
  name: String(item?.name || "").trim(),
  os: String(item?.os || "").trim(),
  arch: String(item?.arch || "").trim(),
  app_version: String(item?.app_version || "").trim(),
  last_seen_at: Number(item?.last_seen_at ?? 0),
  created_at: Number(item?.created_at ?? 0),
  revoked: item?.revoked === true || Number(item?.revoked) === 1,
});

const normalizeCloudCall = (item) => ({
  call_id: String(item?.call_id || "").trim(),
  device_id: String(item?.device_id || "").trim(),
  user_id: String(item?.user_id || "").trim(),
  client: String(item?.client || "").trim(),
  local_session_id: String(item?.local_session_id || "").trim(),
  model: String(item?.model || "").trim(),
  provider: String(item?.provider || "").trim(),
  status: String(item?.status || "").trim(),
  quota_consumed: Number(item?.quota_consumed ?? 0),
  queue_waited_ms: Number(item?.queue_waited_ms ?? 0),
  prompt_tokens: Number(item?.prompt_tokens ?? 0),
  completion_tokens: Number(item?.completion_tokens ?? 0),
  started_at: Number(item?.started_at ?? 0),
  finished_at: Number(item?.finished_at ?? 0),
  duration_ms: Number(item?.duration_ms ?? 0),
  error_summary: String(item?.error_summary || "").trim(),
});

const normalizeCloudLog = (item) => ({
  seq: Number(item?.seq ?? 0),
  device_id: String(item?.device_id || "").trim(),
  user_id: String(item?.user_id || "").trim(),
  client: String(item?.client || "").trim(),
  level: String(item?.level || "").trim(),
  category: String(item?.category || "").trim(),
  event: String(item?.event || "").trim(),
  message: String(item?.message || "").trim(),
  local_session_id: String(item?.local_session_id || "").trim(),
  created_at: Number(item?.created_at ?? 0),
});

const CLOUD_NORMALIZERS = {
  devices: normalizeCloudDevice,
  calls: normalizeCloudCall,
  logs: normalizeCloudLog,
};

// 徽标：沿用 .monitor-status 体系，cloud-* 变体见 app.admin.css
const buildBadge = (text, variant, title = "") => {
  const badge = document.createElement("span");
  badge.className = variant ? `monitor-status ${variant}` : "monitor-status";
  badge.textContent = text;
  if (title) {
    badge.title = title;
  }
  return badge;
};

const callStatusVariant = (status) =>
  CALL_STATUS_VALUES.includes(status) ? `cloud-${status.replace(/_/g, "-")}` : "";

const logLevelVariant = (level) =>
  LOG_LEVEL_VALUES.includes(level) ? `cloud-${level}` : "";

const createCell = (text, title = "") => {
  const cell = document.createElement("td");
  cell.textContent = text;
  if (title && title !== text) {
    cell.title = title;
  }
  return cell;
};

const renderCloudPagination = (tab) => {
  const viewMeta = CLOUD_VIEWS[tab];
  const view = state.cloud[tab];
  const paginationNode = elements[viewMeta.pagination];
  const infoNode = elements[viewMeta.pageInfo];
  const prevNode = elements[viewMeta.prev];
  const nextNode = elements[viewMeta.next];
  if (!paginationNode || !infoNode || !prevNode || !nextNode) {
    return;
  }
  const total = Number(view.pagination.total) || 0;
  if (!total) {
    paginationNode.style.display = "none";
    return;
  }
  const pageSize = view.pagination.pageSize;
  const totalPages = Math.max(1, Math.ceil(total / pageSize));
  const currentPage = Math.min(Math.max(1, view.pagination.page), totalPages);
  view.pagination.page = currentPage;
  paginationNode.style.display = "flex";
  infoNode.textContent = t("pagination.info", {
    total,
    current: currentPage,
    pages: totalPages,
    size: pageSize,
  });
  prevNode.disabled = view.loading || currentPage <= 1;
  nextNode.disabled = view.loading || currentPage >= totalPages;
};

const setCloudTabLoading = (tab, loading) => {
  const view = state.cloud[tab];
  view.loading = loading;
  const viewMeta = CLOUD_VIEWS[tab];
  if (elements.cloudRefreshBtn) {
    elements.cloudRefreshBtn.disabled = loading;
  }
  const prevNode = elements[viewMeta.prev];
  const nextNode = elements[viewMeta.next];
  if (prevNode) {
    prevNode.disabled = loading || view.pagination.page <= 1;
  }
  const total = Number(view.pagination.total) || 0;
  const totalPages = total ? Math.max(1, Math.ceil(total / view.pagination.pageSize)) : 1;
  if (nextNode) {
    nextNode.disabled = loading || view.pagination.page >= totalPages;
  }
};

// 设备页
const renderCloudDeviceRows = () => {
  const view = state.cloud.devices;
  const tbody = elements[CLOUD_VIEWS.devices.tbody];
  const emptyNode = elements[CLOUD_VIEWS.devices.empty];
  tbody.textContent = "";
  if (!view.list.length) {
    emptyNode.style.display = "block";
    renderCloudPagination("devices");
    return;
  }
  emptyNode.style.display = "none";
  const fragment = document.createDocumentFragment();
  view.list.forEach((device) => {
    const row = document.createElement("tr");

    const nameCell = document.createElement("td");
    const nameLabel = document.createElement("div");
    nameLabel.textContent = device.name || "-";
    const idLabel = document.createElement("div");
    idLabel.className = "cloud-cell-sub";
    idLabel.textContent = device.device_id || "-";
    nameCell.appendChild(nameLabel);
    nameCell.appendChild(idLabel);
    if (device.device_id) {
      nameCell.title = device.device_id;
    }

    const clientCell = document.createElement("td");
    clientCell.appendChild(buildBadge(device.client || "-", "cloud-client"));

    const userCell = createCell(device.user_id || "-", device.user_id);

    const systemText = [device.os, device.arch, device.app_version].filter(Boolean).join(" / ");
    const systemCell = createCell(systemText || "-", systemText);

    const seenCell = createCell(formatRelativeSeen(device.last_seen_at), formatCloudTime(device.last_seen_at));

    const statusCell = document.createElement("td");
    statusCell.appendChild(
      device.revoked
        ? buildBadge(t("cloud.device.status.revoked"), "cloud-revoked")
        : buildBadge(t("cloud.device.status.active"), "cloud-active")
    );

    const actionCell = document.createElement("td");
    actionCell.className = "cloud-actions";
    if (device.revoked) {
      // 契约目前仅提供吊销端点；恢复端点上线前按钮保持禁用
      const restoreBtn = document.createElement("button");
      restoreBtn.type = "button";
      restoreBtn.className = "secondary";
      restoreBtn.textContent = t("cloud.device.action.restore");
      restoreBtn.disabled = true;
      restoreBtn.title = t("cloud.device.restoreUnavailableTitle");
      actionCell.appendChild(restoreBtn);
    } else {
      const revokeBtn = document.createElement("button");
      revokeBtn.type = "button";
      revokeBtn.className = "danger";
      revokeBtn.textContent = t("cloud.device.action.revoke");
      revokeBtn.addEventListener("click", (event) => {
        event.stopPropagation();
        requestRevokeDevice(device);
      });
      actionCell.appendChild(revokeBtn);
    }

    row.appendChild(nameCell);
    row.appendChild(clientCell);
    row.appendChild(userCell);
    row.appendChild(systemCell);
    row.appendChild(seenCell);
    row.appendChild(statusCell);
    row.appendChild(actionCell);
    fragment.appendChild(row);
  });
  tbody.appendChild(fragment);
  renderCloudPagination("devices");
};

// 调用记账页
const renderCloudCallRows = () => {
  const view = state.cloud.calls;
  const tbody = elements[CLOUD_VIEWS.calls.tbody];
  const emptyNode = elements[CLOUD_VIEWS.calls.empty];
  tbody.textContent = "";
  if (!view.list.length) {
    emptyNode.style.display = "block";
    renderCloudPagination("calls");
    return;
  }
  emptyNode.style.display = "none";
  const fragment = document.createDocumentFragment();
  view.list.forEach((call) => {
    const row = document.createElement("tr");

    const timeText = formatCloudTime(call.started_at);
    const timeCell = createCell(timeText, timeText);

    const modelTitle = call.provider ? `${call.model} · ${call.provider}` : call.model;
    const modelCell = createCell(call.model || "-", modelTitle);

    const userCell = createCell(call.user_id || "-", call.user_id);

    const deviceCell = createCell(call.device_id || "-", call.device_id);

    const statusCell = document.createElement("td");
    statusCell.appendChild(buildBadge(call.status || "-", callStatusVariant(call.status), call.status));

    const quotaCell = createCell(call.quota_consumed === 1 ? "1" : "0");

    const queueCell = createCell(formatCount(call.queue_waited_ms));

    const tokensCell = createCell(
      `${formatCount(call.prompt_tokens)} / ${formatCount(call.completion_tokens)}`
    );

    const durationCell = createCell(formatCount(call.duration_ms));

    const errorCell = createCell(call.error_summary || "-", call.error_summary);

    row.appendChild(timeCell);
    row.appendChild(modelCell);
    row.appendChild(userCell);
    row.appendChild(deviceCell);
    row.appendChild(statusCell);
    row.appendChild(quotaCell);
    row.appendChild(queueCell);
    row.appendChild(tokensCell);
    row.appendChild(durationCell);
    row.appendChild(errorCell);
    fragment.appendChild(row);
  });
  tbody.appendChild(fragment);
  renderCloudPagination("calls");
};

// 设备日志页
const renderCloudLogRows = () => {
  const view = state.cloud.logs;
  const tbody = elements[CLOUD_VIEWS.logs.tbody];
  const emptyNode = elements[CLOUD_VIEWS.logs.empty];
  tbody.textContent = "";
  if (!view.list.length) {
    emptyNode.style.display = "block";
    renderCloudPagination("logs");
    return;
  }
  emptyNode.style.display = "none";
  const fragment = document.createDocumentFragment();
  view.list.forEach((log) => {
    const row = document.createElement("tr");

    const timeText = formatCloudTime(log.created_at);
    const timeCell = createCell(timeText, timeText);

    const levelCell = document.createElement("td");
    levelCell.appendChild(buildBadge(log.level || "-", logLevelVariant(log.level), log.level));

    const categoryCell = createCell(log.category || "-", log.category);

    const eventCell = createCell(log.event || "-", log.event);

    // td 自带 ellipsis 截断；悬浮 title 展示全文
    const messageCell = createCell(log.message || "-", log.message);

    const sessionCell = createCell(log.local_session_id || "-", log.local_session_id);

    const deviceCell = createCell(log.device_id || "-", log.device_id);

    row.appendChild(timeCell);
    row.appendChild(levelCell);
    row.appendChild(categoryCell);
    row.appendChild(eventCell);
    row.appendChild(messageCell);
    row.appendChild(sessionCell);
    row.appendChild(deviceCell);
    fragment.appendChild(row);
  });
  tbody.appendChild(fragment);
  renderCloudPagination("logs");
};

const CLOUD_RENDERERS = {
  devices: renderCloudDeviceRows,
  calls: renderCloudCallRows,
  logs: renderCloudLogRows,
};

const reportCloudLoadError = (error) => {
  const message = error?.message || t("common.unknownError");
  appendLog(t("cloud.loadFailed", { message }));
  notify(t("cloud.loadFailed", { message }), "error");
};

const fetchCloudPage = async (tab, params) => {
  const view = state.cloud[tab];
  if (view.loading) {
    view.pendingReload = true;
    return;
  }
  const pageSize = view.pagination.pageSize;
  const page = Math.max(1, view.pagination.page);
  const query = new URLSearchParams(params);
  query.set("offset", String((page - 1) * pageSize));
  query.set("limit", String(pageSize));
  const endpoint = `${getWunderBase()}/admin/cloud/${CLOUD_VIEWS[tab].path}?${query.toString()}`;
  const emptyNode = elements[CLOUD_VIEWS[tab].empty];
  const shouldShowLoading = !view.loaded || !view.list.length;
  if (shouldShowLoading && emptyNode) {
    emptyNode.textContent = t("common.loading");
    emptyNode.style.display = "block";
  }
  setCloudTabLoading(tab, true);
  try {
    const response = await fetch(endpoint);
    if (!response.ok) {
      throw new Error(
        await extractResponseMessage(response, t("common.requestFailed", { status: response.status }))
      );
    }
    const result = await response.json();
    const payload = result?.data || {};
    const items = Array.isArray(payload.items) ? payload.items : [];
    view.list = items.map(CLOUD_NORMALIZERS[tab]);
    view.pagination.total = Number(payload.total) || 0;
    view.loaded = true;
    state.panelLoaded.cloud = true;
    CLOUD_RENDERERS[tab]();
  } catch (error) {
    view.list = [];
    if (emptyNode) {
      emptyNode.textContent = t("common.loadFailedWithMessage", { message: error.message });
      emptyNode.style.display = "block";
    }
    renderCloudPagination(tab);
    throw error;
  } finally {
    setCloudTabLoading(tab, false);
    if (view.pendingReload) {
      view.pendingReload = false;
      fetchCloudPage(tab, params).catch(() => {});
    }
  }
};

const loadCloudDevices = async () => {
  ensureCloudState();
  if (!ensureCloudElements()) {
    return;
  }
  const params = new URLSearchParams();
  const userId = readInputValue(elements.cloudDeviceUserInput);
  const status = readInputValue(elements.cloudDeviceStatusFilter);
  if (userId) {
    params.set("user_id", userId);
  }
  if (status) {
    // 设备页 UI 需要状态筛选；该参数为契约行的扩展，后端未实现时会忽略
    params.set("status", status);
  }
  await fetchCloudPage("devices", params);
};

const loadCloudCalls = async () => {
  ensureCloudState();
  if (!ensureCloudElements()) {
    return;
  }
  const params = new URLSearchParams();
  const userId = readInputValue(elements.cloudCallUserInput);
  const deviceId = readInputValue(elements.cloudCallDeviceInput);
  const model = readInputValue(elements.cloudCallModelInput);
  const status = readInputValue(elements.cloudCallStatusFilter);
  if (userId) {
    params.set("user_id", userId);
  }
  if (deviceId) {
    params.set("device_id", deviceId);
  }
  if (model) {
    params.set("model", model);
  }
  if (status) {
    params.set("status", status);
  }
  await fetchCloudPage("calls", params);
};

const loadCloudLogs = async () => {
  ensureCloudState();
  if (!ensureCloudElements()) {
    return;
  }
  const params = new URLSearchParams();
  const userId = readInputValue(elements.cloudLogUserInput);
  const deviceId = readInputValue(elements.cloudLogDeviceInput);
  const level = readInputValue(elements.cloudLogLevelFilter);
  const category = readInputValue(elements.cloudLogCategoryInput);
  if (userId) {
    params.set("user_id", userId);
  }
  if (deviceId) {
    params.set("device_id", deviceId);
  }
  if (level) {
    params.set("level", level);
  }
  if (category) {
    params.set("category", category);
  }
  await fetchCloudPage("logs", params);
};

const CLOUD_LOADERS = {
  devices: loadCloudDevices,
  calls: loadCloudCalls,
  logs: loadCloudLogs,
};

const loadCloudTab = (tab) => CLOUD_LOADERS[tab]?.() ?? Promise.resolve();

// 高危操作：吊销设备（该设备将无法再用云端，日志会被清理）
const requestRevokeDevice = (device) => {
  if (!device?.device_id) {
    return;
  }
  openImpactConfirmModal({
    title: t("cloud.device.revoke.title"),
    summary: t("cloud.device.revoke.summary"),
    details: [
      t("cloud.device.revoke.detailLine", {
        name: device.name || "-",
        device: device.device_id,
        user: device.user_id || "-",
      }),
    ],
    confirmLabel: t("cloud.device.revoke.confirmLabel"),
    danger: true,
    onConfirm: async () => {
      const endpoint = `${getWunderBase()}/admin/cloud/devices/${encodeURIComponent(device.device_id)}`;
      try {
        const response = await fetch(endpoint, { method: "DELETE" });
        if (!response.ok) {
          const message = await extractResponseMessage(
            response,
            t("common.requestFailed", { status: response.status })
          );
          notify(t("cloud.device.revoke.failed", { message }), "error");
          return false;
        }
        notify(t("cloud.device.revoke.success"), "success");
        await loadCloudDevices();
        return true;
      } catch (error) {
        notify(
          t("cloud.device.revoke.failed", {
            message: error?.message || t("common.unknownError"),
          }),
          "error"
        );
        return false;
      }
    },
  });
};

const switchCloudTab = (tab) => {
  if (!CLOUD_TABS.includes(tab)) {
    return;
  }
  ensureCloudState();
  state.cloud.activeTab = tab;
  const tabButtons = {
    devices: elements.cloudTabDevices,
    calls: elements.cloudTabCalls,
    logs: elements.cloudTabLogs,
  };
  const tabContents = {
    devices: elements.cloudTabContentDevices,
    calls: elements.cloudTabContentCalls,
    logs: elements.cloudTabContentLogs,
  };
  CLOUD_TABS.forEach((key) => {
    tabButtons[key]?.classList.toggle("is-active", key === tab);
    tabButtons[key]?.setAttribute("aria-selected", key === tab ? "true" : "false");
    tabContents[key]?.classList.toggle("active", key === tab);
  });
  const view = state.cloud[tab];
  if (!view.loaded && !view.loading) {
    loadCloudTab(tab).catch(reportCloudLoadError);
  }
};

const bindCloudFilterEnter = (input, tab) => {
  input?.addEventListener("keydown", (event) => {
    if (event.key !== "Enter") {
      return;
    }
    state.cloud[tab].pagination.page = 1;
    loadCloudTab(tab).catch(reportCloudLoadError);
  });
};

const bindCloudFilterChange = (node, tab) => {
  node?.addEventListener("change", () => {
    state.cloud[tab].pagination.page = 1;
    loadCloudTab(tab).catch(reportCloudLoadError);
  });
};

const bindCloudReset = (button, tab, fields) => {
  button?.addEventListener("click", () => {
    fields.forEach((node) => {
      if (node) {
        node.value = "";
      }
    });
    state.cloud[tab].pagination.page = 1;
    loadCloudTab(tab).catch(reportCloudLoadError);
  });
};

const bindCloudPagination = (tab) => {
  const viewMeta = CLOUD_VIEWS[tab];
  elements[viewMeta.prev]?.addEventListener("click", async () => {
    state.cloud[tab].pagination.page = Math.max(1, state.cloud[tab].pagination.page - 1);
    try {
      await loadCloudTab(tab);
    } catch (error) {
      appendLog(t("cloud.loadFailed", { message: error.message }));
    }
  });
  elements[viewMeta.next]?.addEventListener("click", async () => {
    state.cloud[tab].pagination.page = state.cloud[tab].pagination.page + 1;
    try {
      await loadCloudTab(tab);
    } catch (error) {
      appendLog(t("cloud.loadFailed", { message: error.message }));
    }
  });
};

export const initCloudPanel = () => {
  ensureCloudState();
  if (!ensureCloudElements()) {
    return;
  }
  elements.cloudTabDevices.addEventListener("click", () => switchCloudTab("devices"));
  elements.cloudTabCalls.addEventListener("click", () => switchCloudTab("calls"));
  elements.cloudTabLogs.addEventListener("click", () => switchCloudTab("logs"));

  elements.cloudRefreshBtn.addEventListener("click", () => {
    loadCloudTab(state.cloud.activeTab).catch(reportCloudLoadError);
  });

  // 设备页：user_id 搜索 + 状态
  bindCloudFilterEnter(elements.cloudDeviceUserInput, "devices");
  bindCloudFilterChange(elements.cloudDeviceStatusFilter, "devices");
  bindCloudReset(elements.cloudDeviceResetBtn, "devices", [
    elements.cloudDeviceUserInput,
    elements.cloudDeviceStatusFilter,
  ]);

  // 调用页：user_id / device_id / model / status
  bindCloudFilterEnter(elements.cloudCallUserInput, "calls");
  bindCloudFilterEnter(elements.cloudCallDeviceInput, "calls");
  bindCloudFilterEnter(elements.cloudCallModelInput, "calls");
  bindCloudFilterChange(elements.cloudCallStatusFilter, "calls");
  bindCloudReset(elements.cloudCallResetBtn, "calls", [
    elements.cloudCallUserInput,
    elements.cloudCallDeviceInput,
    elements.cloudCallModelInput,
    elements.cloudCallStatusFilter,
  ]);

  // 日志页：user_id / device_id / level / category
  bindCloudFilterEnter(elements.cloudLogUserInput, "logs");
  bindCloudFilterEnter(elements.cloudLogDeviceInput, "logs");
  bindCloudFilterChange(elements.cloudLogLevelFilter, "logs");
  bindCloudFilterEnter(elements.cloudLogCategoryInput, "logs");
  bindCloudReset(elements.cloudLogResetBtn, "logs", [
    elements.cloudLogUserInput,
    elements.cloudLogDeviceInput,
    elements.cloudLogLevelFilter,
    elements.cloudLogCategoryInput,
  ]);

  bindCloudPagination("devices");
  bindCloudPagination("calls");
  bindCloudPagination("logs");
};

// 导航懒加载入口：首次进入面板时加载当前子页
export const loadCloudPanel = async () => {
  ensureCloudState();
  if (!ensureCloudElements()) {
    return;
  }
  await loadCloudTab(state.cloud.activeTab);
};
