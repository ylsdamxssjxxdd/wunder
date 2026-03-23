import { elements } from "./elements.js?v=20260215-01";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { formatTimestamp } from "./utils.js?v=20251229-02";
import { notify } from "./notify.js";

const BRIDGE_OWNER_PREFIX = "bridge_center_owner__";
const USER_ONLY_CHANNELS = new Set(["wechat", "wechat_mp", "weixin"]);

const emptyCenter = () => ({
  center_id: "",
  name: "",
  code: "",
  status: "active",
  default_preset_agent_name: "",
  target_unit_id: "",
  description: "",
  owner_username: "",
  account_count: 0,
  route_count: 0,
  active_route_count: 0,
  created_at: 0,
  updated_at: 0,
});

const emptyChannelForm = () => ({
  mode: "create",
  center_account_id: "",
  channel: "",
  account_id: "",
  account_name: "",
  peer_kind: "group",
  enabled: true,
  config_text: "{}",
  default_preset_agent_name_override: "",
  thread_strategy: "main_thread",
  status_reason: "",
  owned: true,
});

const ensureBridgeState = () => {
  if (!state.bridgeCenter) {
    state.bridgeCenter = {};
  }
  state.bridgeCenter.meta ||= null;
  state.bridgeCenter.centers ||= [];
  state.bridgeCenter.accounts ||= [];
  state.bridgeCenter.routes ||= [];
  state.bridgeCenter.logs ||= [];
  state.bridgeCenter.selectedCenterId ||= "";
  state.bridgeCenter.selectedAccountId ||= "";
  state.bridgeCenter.selectedRouteId ||= "";
  state.bridgeCenter.routeStatus ||= "";
  state.bridgeCenter.configEditingCenterId ||= "";
  state.bridgeCenter.channelForm ||= emptyChannelForm();
};

const resolveChannelLabel = (channel) => {
  const hit = (state.bridgeCenter.meta?.supported_channels || []).find((item) => item.channel === channel);
  return hit?.display_name || channel || "-";
};

const cleanText = (value) => String(value || "").trim();
const isPlainObject = (value) => Boolean(value) && typeof value === "object" && !Array.isArray(value);
const bridgeAccountKey = (channel, accountId) => `${cleanText(channel).toLowerCase()}::${cleanText(accountId).toLowerCase()}`;
const centerOwnerUserId = (centerId) => (cleanText(centerId) ? `${BRIDGE_OWNER_PREFIX}${cleanText(centerId)}` : "");

const safeTs = (value) => {
  const ts = Number(value);
  if (!Number.isFinite(ts) || ts <= 0) {
    return "-";
  }
  return formatTimestamp(ts * 1000);
};

const currentCenter = () =>
  state.bridgeCenter.centers.find((item) => item.center_id === state.bridgeCenter.selectedCenterId) || null;

const currentAccount = () =>
  state.bridgeCenter.accounts.find((item) => item.center_account_id === state.bridgeCenter.selectedAccountId) || null;

const parseResponseError = async (response) => {
  const payload = await response.json().catch(() => ({}));
  return payload?.error?.message || payload?.detail?.message || `HTTP ${response.status}`;
};

const fetchJson = async (path, options = {}) => {
  const response = await fetch(`${getWunderBase()}${path}`, options);
  if (!response.ok) {
    throw new Error(await parseResponseError(response));
  }
  return response.json();
};

const fillSelect = (element, items, placeholder = "请选择") => {
  if (!element) {
    return;
  }
  const currentValue = element.value;
  element.textContent = "";
  const empty = document.createElement("option");
  empty.value = "";
  empty.textContent = placeholder;
  element.appendChild(empty);
  items.forEach((item) => {
    const option = document.createElement("option");
    option.value = item.value;
    option.textContent = item.label;
    element.appendChild(option);
  });
  element.value = currentValue;
};

const openModal = (modal) => modal?.classList.add("active");
const closeModal = (modal) => modal?.classList.remove("active");

const sanitizeCenterCode = (rawValue, fallbackValue) => {
  const normalized = String(rawValue || "")
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "");
  if (normalized) {
    return normalized;
  }
  const fallback = String(fallbackValue || "")
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "");
  return fallback || `bridge_center_${Date.now()}`;
};

const normalizeBridgeAccount = (item = {}) => ({
  center_account_id: cleanText(item.center_account_id),
  center_id: cleanText(item.center_id),
  channel: cleanText(item.channel).toLowerCase(),
  account_id: cleanText(item.account_id),
  enabled: item.enabled !== false,
  default_preset_agent_name_override: cleanText(item.default_preset_agent_name_override),
  thread_strategy: cleanText(item.thread_strategy) || "main_thread",
  status_reason: cleanText(item.status_reason),
  route_count: Number(item.route_count) || 0,
  updated_at: Number(item.updated_at) || 0,
});

const normalizeOwnedChannelAccount = (item = {}) => ({
  key: bridgeAccountKey(item.channel, item.account_id),
  channel: cleanText(item.channel).toLowerCase(),
  account_id: cleanText(item.account_id),
  name: cleanText(item.name),
  status: cleanText(item.status) || "active",
  active: item.active !== false,
  meta: isPlainObject(item.meta) ? item.meta : {},
  raw_config: isPlainObject(item.raw_config) ? item.raw_config : {},
  updated_at: Number(item.updated_at) || 0,
});

const mergeBridgeAccounts = (bridgeAccounts, ownedAccounts) => {
  const ownedMap = new Map(ownedAccounts.map((item) => [item.key, item]));
  return bridgeAccounts.map((item) => {
    const owned = ownedMap.get(bridgeAccountKey(item.channel, item.account_id));
    return {
      ...item,
      owned: Boolean(owned),
      name: owned?.name || "",
      active: owned?.active ?? false,
      meta: owned?.meta || {},
      raw_config: owned?.raw_config || {},
      account_updated_at: owned?.updated_at || 0,
    };
  });
};

const refreshMetaOptions = () => {
  const meta = state.bridgeCenter.meta || {};
  fillSelect(
    elements.bridgeCenterConfigPreset,
    (meta.preset_agents || []).map((item) => ({ value: item.name, label: item.name })),
    "请选择预设"
  );
  fillSelect(
    elements.bridgeCenterConfigUnit,
    (meta.org_units || []).map((item) => ({ value: item.unit_id, label: item.path_name || item.name })),
    "默认不指定单位"
  );
  fillSelect(
    elements.bridgeCenterChannelFormChannel,
    (meta.supported_channels || []).map((item) => ({
      value: item.channel,
      label: `${item.display_name || item.channel} (${item.channel})`,
    })),
    "选择渠道"
  );
  fillSelect(
    elements.bridgeCenterChannelPresetOverride,
    (meta.preset_agents || []).map((item) => ({ value: item.name, label: item.name })),
    "沿用节点默认预设"
  );
};

const renderCenterList = () => {
  if (!elements.bridgeCenterList) {
    return;
  }
  elements.bridgeCenterList.textContent = "";
  if (!state.bridgeCenter.centers.length) {
    elements.bridgeCenterList.textContent = "暂无舰桥节点";
    if (elements.bridgeCenterSelectionMeta) {
      elements.bridgeCenterSelectionMeta.textContent = "0 个节点";
    }
    return;
  }
  state.bridgeCenter.centers.forEach((center) => {
    const item = document.createElement("button");
    item.type = "button";
    item.className = "external-link-item bridge-center-list-item";
    if (center.center_id === state.bridgeCenter.selectedCenterId) {
      item.classList.add("is-active");
    }
    item.innerHTML = `
      <div class="external-link-item-title">
        <span class="external-link-item-icon"><i class="fa-solid fa-tower-observation"></i></span>
        <span>${center.name || center.center_id}</span>
      </div>
      <div class="external-link-item-meta">状态 ${center.status} | 渠道 ${center.shared_channel_count || center.account_count || 0} | 路由 ${center.route_count || 0}</div>
    `;
    item.addEventListener("click", async () => {
      state.bridgeCenter.selectedCenterId = center.center_id;
      state.bridgeCenter.selectedAccountId = "";
      state.bridgeCenter.selectedRouteId = "";
      renderCenterList();
      renderCenterOverview();
      await Promise.all([loadBridgeCenterAccounts(), loadBridgeRoutes(), loadBridgeLogs()]);
    });
    elements.bridgeCenterList.appendChild(item);
  });
  if (elements.bridgeCenterSelectionMeta) {
    elements.bridgeCenterSelectionMeta.textContent = `${state.bridgeCenter.centers.length} 个节点`;
  }
};

const renderCenterOverview = () => {
  const center = currentCenter();
  const units = state.bridgeCenter.meta?.org_units || [];
  const unit = units.find((item) => item.unit_id === center?.target_unit_id);
  if (elements.bridgeCenterCurrentName) {
    elements.bridgeCenterCurrentName.textContent = center?.name || "未选择舰桥节点";
  }
  if (elements.bridgeCenterOwner) {
    elements.bridgeCenterOwner.textContent = center
      ? `${center.owner_username ? `创建人：${center.owner_username} | ` : ""`}更新时间：${safeTs(center.updated_at)}`
      : "先创建舰桥节点，再配置接入渠道。";
  }
  if (elements.bridgeCenterSummaryStatus) elements.bridgeCenterSummaryStatus.textContent = center?.status || "-";
  if (elements.bridgeCenterSummaryPreset) elements.bridgeCenterSummaryPreset.textContent = center?.default_preset_agent_name || "-";
  if (elements.bridgeCenterSummaryUnit) elements.bridgeCenterSummaryUnit.textContent = unit?.path_name || unit?.name || "默认不指定";
  if (elements.bridgeCenterSummaryChannels) elements.bridgeCenterSummaryChannels.textContent = String(center?.shared_channel_count || center?.account_count || 0);
  if (elements.bridgeCenterSummaryRoutes) elements.bridgeCenterSummaryRoutes.textContent = String(center?.route_count || 0);
  if (elements.bridgeCenterSummaryActiveRoutes) elements.bridgeCenterSummaryActiveRoutes.textContent = String(center?.active_route_count || 0);
  if (elements.bridgeCenterSummaryPassword) elements.bridgeCenterSummaryPassword.textContent = state.bridgeCenter.meta?.default_password || "123456";
  if (elements.bridgeCenterSummaryDescription) elements.bridgeCenterSummaryDescription.textContent = center?.description || "该节点暂无说明。";
  if (elements.bridgeCenterDeleteBtn) elements.bridgeCenterDeleteBtn.disabled = !center;
  if (elements.bridgeCenterChannelsBtn) elements.bridgeCenterChannelsBtn.disabled = !center;
};
const renderAccountList = () => {
  if (!elements.bridgeCenterAccountList) {
    return;
  }
  elements.bridgeCenterAccountList.textContent = "";
  if (!state.bridgeCenter.accounts.length) {
    const row = document.createElement("tr");
    row.innerHTML = '<td colspan="8" class="muted">暂无接入渠道</td>';
    elements.bridgeCenterAccountList.appendChild(row);
    return;
  }
  state.bridgeCenter.accounts.forEach((account) => {
    const configStatus = account.owned ? (account.meta?.configured === false ? "未配置" : "已配置") : "外部账号";
    const row = document.createElement("tr");
    row.innerHTML = `
      <td>${resolveChannelLabel(account.channel)}</td>
      <td>${account.account_id}</td>
      <td>${account.name || "-"}</td>
      <td>${configStatus}</td>
      <td>${account.thread_strategy}</td>
      <td>${account.route_count || 0}</td>
      <td>${safeTs(account.account_updated_at || account.updated_at)}</td>
      <td><button type="button" class="secondary">配置</button></td>
    `;
    row.querySelector("button")?.addEventListener("click", () => {
      applyChannelForm(account);
      openModal(elements.bridgeCenterChannelModal);
    });
    elements.bridgeCenterAccountList.appendChild(row);
  });
};

const renderRouteList = () => {
  if (!elements.bridgeCenterRouteList) {
    return;
  }
  elements.bridgeCenterRouteList.textContent = "";
  if (!state.bridgeCenter.routes.length) {
    const row = document.createElement("tr");
    row.innerHTML = '<td colspan="5" class="muted">暂无自动分配路由</td>';
    elements.bridgeCenterRouteList.appendChild(row);
    return;
  }
  state.bridgeCenter.routes.forEach((route) => {
    const row = document.createElement("tr");
    if (route.route_id === state.bridgeCenter.selectedRouteId) {
      row.classList.add("active");
    }
    row.innerHTML = `
      <td>${route.external_display_name || route.external_user_key || route.external_identity_key}</td>
      <td>${route.wunder_username || route.wunder_user_id}</td>
      <td>${route.agent_name || route.agent_id}</td>
      <td>${route.status}</td>
      <td>${safeTs(route.last_seen_at)}</td>
    `;
    row.addEventListener("click", async () => {
      state.bridgeCenter.selectedRouteId = route.route_id;
      renderRouteList();
      await loadBridgeLogs();
    });
    elements.bridgeCenterRouteList.appendChild(row);
  });
};

const renderLogList = () => {
  if (!elements.bridgeCenterLogList) {
    return;
  }
  elements.bridgeCenterLogList.textContent = "";
  if (!state.bridgeCenter.logs.length) {
    const row = document.createElement("tr");
    row.innerHTML = '<td colspan="4" class="muted">暂无投递日志</td>';
    elements.bridgeCenterLogList.appendChild(row);
    return;
  }
  state.bridgeCenter.logs.forEach((item) => {
    const row = document.createElement("tr");
    row.innerHTML = `
      <td>${item.direction}/${item.stage}</td>
      <td>${item.status}</td>
      <td>${item.summary || "-"}</td>
      <td>${safeTs(item.created_at)}</td>
    `;
    elements.bridgeCenterLogList.appendChild(row);
  });
};

const loadBridgeMetadata = async () => {
  const payload = await fetchJson("/admin/bridge/metadata");
  state.bridgeCenter.meta = payload?.data || {};
  refreshMetaOptions();
};

const loadBridgeCenterAccounts = async () => {
  const centerId = state.bridgeCenter.selectedCenterId;
  if (!centerId) {
    state.bridgeCenter.accounts = [];
    renderAccountList();
    return;
  }
  const ownerUserId = centerOwnerUserId(centerId);
  const [bridgePayload, ownerPayload] = await Promise.all([
    fetchJson(`/admin/bridge/centers/${encodeURIComponent(centerId)}/accounts`),
    fetchJson(`/channels/accounts?user_id=${encodeURIComponent(ownerUserId)}`).catch(() => ({ data: { items: [] } })),
  ]);
  const bridgeAccounts = (bridgePayload?.data?.items || []).map((item) => normalizeBridgeAccount(item));
  const ownedAccounts = (ownerPayload?.data?.items || []).map((item) => normalizeOwnedChannelAccount(item));
  state.bridgeCenter.accounts = mergeBridgeAccounts(bridgeAccounts, ownedAccounts);
  if (!state.bridgeCenter.accounts.some((item) => item.center_account_id === state.bridgeCenter.selectedAccountId)) {
    state.bridgeCenter.selectedAccountId = "";
  }
  renderAccountList();
};

const loadBridgeRoutes = async () => {
  const centerId = state.bridgeCenter.selectedCenterId;
  if (!centerId) {
    state.bridgeCenter.routes = [];
    renderRouteList();
    return;
  }
  const params = new URLSearchParams({ center_id: centerId, limit: "100" });
  if (state.bridgeCenter.routeStatus) {
    params.set("status", state.bridgeCenter.routeStatus);
  }
  const payload = await fetchJson(`/admin/bridge/routes?${params.toString()}`);
  state.bridgeCenter.routes = payload?.data?.items || [];
  if (!state.bridgeCenter.routes.some((item) => item.route_id === state.bridgeCenter.selectedRouteId)) {
    state.bridgeCenter.selectedRouteId = "";
  }
  renderRouteList();
};

const loadBridgeLogs = async () => {
  const centerId = state.bridgeCenter.selectedCenterId;
  if (!centerId) {
    state.bridgeCenter.logs = [];
    renderLogList();
    return;
  }
  const params = new URLSearchParams({ center_id: centerId, limit: "50" });
  if (state.bridgeCenter.selectedRouteId) {
    params.set("route_id", state.bridgeCenter.selectedRouteId);
  }
  const payload = await fetchJson(`/admin/bridge/delivery_logs?${params.toString()}`);
  state.bridgeCenter.logs = payload?.data?.items || [];
  renderLogList();
};

const loadBridgeCenters = async ({ silent = false, selectedCenterId = "" } = {}) => {
  ensureBridgeState();
  if (!state.bridgeCenter.meta) {
    await loadBridgeMetadata();
  }
  const payload = await fetchJson("/admin/bridge/centers?limit=100");
  state.bridgeCenter.centers = payload?.data?.items || [];
  state.bridgeCenter.selectedCenterId = selectedCenterId || state.bridgeCenter.selectedCenterId || state.bridgeCenter.centers[0]?.center_id || "";
  renderCenterList();
  renderCenterOverview();
  if (!state.bridgeCenter.selectedCenterId) {
    state.bridgeCenter.accounts = [];
    state.bridgeCenter.routes = [];
    state.bridgeCenter.logs = [];
    renderAccountList();
    renderRouteList();
    renderLogList();
  } else {
    await Promise.all([loadBridgeCenterAccounts(), loadBridgeRoutes(), loadBridgeLogs()]);
  }
  if (!silent) {
    notify("舰桥中心已刷新", "success");
  }
};
const applyChannelForm = (account) => {
  if (!account) {
    state.bridgeCenter.selectedAccountId = "";
    state.bridgeCenter.channelForm = {
      ...emptyChannelForm(),
      channel: state.bridgeCenter.meta?.supported_channels?.[0]?.channel || "",
      peer_kind: "group",
    };
  } else {
    state.bridgeCenter.selectedAccountId = account.center_account_id;
    state.bridgeCenter.channelForm = {
      mode: "edit",
      center_account_id: account.center_account_id,
      channel: account.channel,
      account_id: account.account_id,
      account_name: account.name || "",
      peer_kind: USER_ONLY_CHANNELS.has(account.channel) ? "user" : cleanText(account.meta?.peer_kind) || "group",
      enabled: account.enabled,
      config_text: account.owned ? JSON.stringify(account.raw_config || {}, null, 2) : "{}",
      default_preset_agent_name_override: account.default_preset_agent_name_override || "",
      thread_strategy: account.thread_strategy || "main_thread",
      status_reason: account.status_reason || "",
      owned: account.owned !== false,
    };
  }
  const form = state.bridgeCenter.channelForm;
  if (elements.bridgeCenterChannelModalTitle) elements.bridgeCenterChannelModalTitle.textContent = form.mode === "edit" ? "编辑接入渠道" : "新增接入渠道";
  if (elements.bridgeCenterChannelEditorHint) elements.bridgeCenterChannelEditorHint.textContent = form.mode === "edit" ? (form.owned ? "修改后会同步更新渠道账号和舰桥策略。" : "该账号不归当前节点所有，仅允许调整舰桥策略。") : "直接在这里配置节点专属渠道账号。";
  if (elements.bridgeCenterChannelOwnedBadge) elements.bridgeCenterChannelOwnedBadge.textContent = form.mode === "edit" ? (form.owned ? "节点内账号" : "外部账号") : "新渠道";
  if (elements.bridgeCenterChannelFormChannel) {
    elements.bridgeCenterChannelFormChannel.value = form.channel || "";
    elements.bridgeCenterChannelFormChannel.disabled = form.mode === "edit";
  }
  if (elements.bridgeCenterChannelFormAccountId) {
    elements.bridgeCenterChannelFormAccountId.value = form.account_id || "";
    elements.bridgeCenterChannelFormAccountId.readOnly = form.mode === "edit";
  }
  if (elements.bridgeCenterChannelFormAccountName) elements.bridgeCenterChannelFormAccountName.value = form.account_name || "";
  if (elements.bridgeCenterChannelFormPeerKind) {
    elements.bridgeCenterChannelFormPeerKind.value = form.peer_kind || "group";
    elements.bridgeCenterChannelFormPeerKind.disabled = USER_ONLY_CHANNELS.has(form.channel) || (form.mode === "edit" && form.owned === false);
  }
  if (elements.bridgeCenterChannelFormEnabled) elements.bridgeCenterChannelFormEnabled.checked = Boolean(form.enabled);
  if (elements.bridgeCenterChannelConfig) {
    elements.bridgeCenterChannelConfig.value = form.config_text || "{}";
    elements.bridgeCenterChannelConfig.disabled = form.mode === "edit" && form.owned === false;
  }
  if (elements.bridgeCenterChannelPresetOverride) elements.bridgeCenterChannelPresetOverride.value = form.default_preset_agent_name_override || "";
  if (elements.bridgeCenterChannelThreadStrategy) elements.bridgeCenterChannelThreadStrategy.value = form.thread_strategy || "main_thread";
  if (elements.bridgeCenterChannelStatusReason) elements.bridgeCenterChannelStatusReason.value = form.status_reason || "";
  if (elements.bridgeCenterChannelDeleteBtn) elements.bridgeCenterChannelDeleteBtn.disabled = form.mode !== "edit";
};

const readCenterConfig = () => {
  const current =
    state.bridgeCenter.centers.find((item) => item.center_id === state.bridgeCenter.configEditingCenterId) || null;
  return {
    center_id: current?.center_id || undefined,
    name: elements.bridgeCenterConfigName?.value || "",
    code: sanitizeCenterCode(current?.code, elements.bridgeCenterConfigName?.value || "bridge_center"),
    status: elements.bridgeCenterConfigStatus?.value || "active",
    default_preset_agent_name: elements.bridgeCenterConfigPreset?.value || "",
    target_unit_id: elements.bridgeCenterConfigUnit?.value || undefined,
    description: elements.bridgeCenterConfigDescription?.value || undefined,
    default_identity_strategy: "sender_in_peer",
    username_policy: "namespaced_generated",
    settings: {},
  };
};

const saveCenterConfig = async () => {
  const payload = readCenterConfig();
  const result = await fetchJson("/admin/bridge/centers", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(payload),
  });
  const centerId = result?.data?.center?.center_id || payload.center_id || "";
  state.bridgeCenter.configEditingCenterId = centerId;
  await loadBridgeCenters({ silent: true, selectedCenterId: centerId });
  closeModal(elements.bridgeCenterConfigModal);
  notify("舰桥节点已保存", "success");
};

const saveChannelConfig = async () => {
  const center = currentCenter();
  if (!center?.center_id) {
    throw new Error("请先保存舰桥节点，再配置接入渠道");
  }
  const form = state.bridgeCenter.channelForm;
  const channel = cleanText(elements.bridgeCenterChannelFormChannel?.value).toLowerCase();
  const accountId = cleanText(elements.bridgeCenterChannelFormAccountId?.value);
  const accountName = cleanText(elements.bridgeCenterChannelFormAccountName?.value);
  const peerKind = USER_ONLY_CHANNELS.has(channel) ? "user" : cleanText(elements.bridgeCenterChannelFormPeerKind?.value) || "group";
  const enabled = Boolean(elements.bridgeCenterChannelFormEnabled?.checked);
  const rawConfig = cleanText(elements.bridgeCenterChannelConfig?.value);
  let config = null;
  if (form.mode === "create" || form.owned !== false) {
    try {
      config = JSON.parse(rawConfig || "{}");
    } catch (error) {
      throw new Error("JSON 配置格式错误，请输入对象格式");
    }
    if (!isPlainObject(config)) {
      throw new Error("JSON 配置格式错误，请输入对象格式");
    }
  }
  const ownerUserId = centerOwnerUserId(center.center_id);
  let savedAccount = { channel, account_id: accountId, name: accountName };
  if (form.mode === "create" || form.owned !== false) {
    const channelPayload = {
      channel,
      account_id: accountId || undefined,
      create_new: !accountId && form.mode !== "edit",
      account_name: accountName || undefined,
      peer_kind: peerKind,
      enabled,
      config,
    };
    const result = await fetchJson(`/channels/accounts?user_id=${encodeURIComponent(ownerUserId)}`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(channelPayload),
    });
    savedAccount = result?.data || savedAccount;
  }
  const bridgePayload = {
    center_id: center.center_id,
    channel: savedAccount.channel,
    account_id: savedAccount.account_id,
    enabled,
    default_preset_agent_name_override: cleanText(elements.bridgeCenterChannelPresetOverride?.value) || undefined,
    thread_strategy: cleanText(elements.bridgeCenterChannelThreadStrategy?.value) || "main_thread",
    status_reason: cleanText(elements.bridgeCenterChannelStatusReason?.value) || undefined,
  };
  if (form.mode === "edit" && form.center_account_id) {
    await fetchJson(`/admin/bridge/accounts/${encodeURIComponent(form.center_account_id)}`, {
      method: "PATCH",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(bridgePayload),
    });
  } else {
    await fetchJson(`/admin/bridge/centers/${encodeURIComponent(center.center_id)}/accounts`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(bridgePayload),
    });
  }
  await loadBridgeCenterAccounts();
  applyChannelForm();
  notify("接入渠道已保存", "success");
};

const removeChannelConfig = async () => {
  const center = currentCenter();
  const account = currentAccount();
  if (!center?.center_id || !account?.center_account_id) {
    return;
  }
  if (!window.confirm(`确认移除 ${account.channel} / ${account.account_id} 吗？`)) {
    return;
  }
  await fetchJson(`/admin/bridge/accounts/${encodeURIComponent(account.center_account_id)}`, { method: "DELETE" });
  if (account.owned) {
    const ownerUserId = centerOwnerUserId(center.center_id);
    await fetchJson(`/channels/accounts/${encodeURIComponent(account.channel)}/${encodeURIComponent(account.account_id)}?user_id=${encodeURIComponent(ownerUserId)}`, { method: "DELETE" }).catch(() => null);
  }
  await loadBridgeCenterAccounts();
  applyChannelForm();
  notify("接入渠道已移除", "success");
};

const patchRouteStatus = async (status) => {
  if (!state.bridgeCenter.selectedRouteId) {
    notify("请先选择路由", "warning");
    return;
  }
  await fetchJson(`/admin/bridge/routes/${encodeURIComponent(state.bridgeCenter.selectedRouteId)}`, {
    method: "PATCH",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ status, clear_last_error: status === "active" }),
  });
  notify(`路由已切换为 ${status}`, "success");
  await Promise.all([loadBridgeRoutes(), loadBridgeLogs()]);
};

export const initBridgeCenterPanel = () => {
  ensureBridgeState();
  if (!elements.bridgeCenterPanel || elements.bridgeCenterPanel.dataset.bound === "1") return;
  elements.bridgeCenterPanel.dataset.bound = "1";
  elements.bridgeCenterRefreshBtn?.addEventListener("click", () => loadBridgeCenters());
  elements.bridgeCenterNewBtn?.addEventListener("click", () => {
    state.bridgeCenter.configEditingCenterId = "";
    if (elements.bridgeCenterConfigModalTitle) elements.bridgeCenterConfigModalTitle.textContent = "新建舰桥节点";
    if (elements.bridgeCenterConfigName) elements.bridgeCenterConfigName.value = "";
    if (elements.bridgeCenterConfigStatus) elements.bridgeCenterConfigStatus.value = "active";
    if (elements.bridgeCenterConfigPreset) elements.bridgeCenterConfigPreset.value = "";
    if (elements.bridgeCenterConfigUnit) elements.bridgeCenterConfigUnit.value = "";
    if (elements.bridgeCenterConfigDescription) elements.bridgeCenterConfigDescription.value = "";
    if (elements.bridgeCenterConfigOwner) elements.bridgeCenterConfigOwner.textContent = "保存后即可继续配置该节点接入渠道";
    openModal(elements.bridgeCenterConfigModal);
  });
  elements.bridgeCenterDeleteBtn?.addEventListener("click", async () => {
    const center = currentCenter();
    if (!center?.center_id || !window.confirm("删除当前舰桥节点及其路由/日志？")) return;
    await fetchJson(`/admin/bridge/centers/${encodeURIComponent(center.center_id)}`, { method: "DELETE" });
    state.bridgeCenter.selectedCenterId = "";
    notify("舰桥节点已删除", "success");
    await loadBridgeCenters({ silent: true });
  });
  elements.bridgeCenterConfigBtn?.addEventListener("click", () => {
    const center = currentCenter() || emptyCenter();
    state.bridgeCenter.configEditingCenterId = center.center_id || "";
    if (elements.bridgeCenterConfigModalTitle) elements.bridgeCenterConfigModalTitle.textContent = center.center_id ? "编辑舰桥节点" : "新建舰桥节点";
    if (elements.bridgeCenterConfigName) elements.bridgeCenterConfigName.value = center.name || "";
    if (elements.bridgeCenterConfigStatus) elements.bridgeCenterConfigStatus.value = center.status || "active";
    if (elements.bridgeCenterConfigPreset) elements.bridgeCenterConfigPreset.value = center.default_preset_agent_name || "";
    if (elements.bridgeCenterConfigUnit) elements.bridgeCenterConfigUnit.value = center.target_unit_id || "";
    if (elements.bridgeCenterConfigDescription) elements.bridgeCenterConfigDescription.value = center.description || "";
    if (elements.bridgeCenterConfigOwner) elements.bridgeCenterConfigOwner.textContent = center.owner_username ? `创建人：${center.owner_username} | 最近更新：${safeTs(center.updated_at)}` : "保存后即可继续配置该节点接入渠道";
    openModal(elements.bridgeCenterConfigModal);
  });
  elements.bridgeCenterChannelsBtn?.addEventListener("click", async () => {
    if (!currentCenter()?.center_id) return notify("请先保存舰桥节点，再配置接入渠道", "warning");
    await loadBridgeCenterAccounts();
    applyChannelForm();
    openModal(elements.bridgeCenterChannelModal);
  });
  elements.bridgeCenterConfigSaveBtn?.addEventListener("click", () => saveCenterConfig().catch((error) => notify(error.message, "error")));
  elements.bridgeCenterChannelNewBtn?.addEventListener("click", () => applyChannelForm());
  elements.bridgeCenterChannelSaveBtn?.addEventListener("click", () => saveChannelConfig().catch((error) => notify(error.message, "error")));
  elements.bridgeCenterChannelDeleteBtn?.addEventListener("click", () => removeChannelConfig().catch((error) => notify(error.message, "error")));
  elements.bridgeCenterChannelFormChannel?.addEventListener("change", () => {
    const channel = cleanText(elements.bridgeCenterChannelFormChannel.value).toLowerCase();
    state.bridgeCenter.channelForm.channel = channel;
    if (elements.bridgeCenterChannelFormPeerKind) {
      if (USER_ONLY_CHANNELS.has(channel)) {
        elements.bridgeCenterChannelFormPeerKind.value = "user";
        elements.bridgeCenterChannelFormPeerKind.disabled = true;
      } else {
        elements.bridgeCenterChannelFormPeerKind.disabled = false;
      }
    }
  });
  elements.bridgeCenterConfigClose?.addEventListener("click", () => closeModal(elements.bridgeCenterConfigModal));
  elements.bridgeCenterConfigCancel?.addEventListener("click", () => closeModal(elements.bridgeCenterConfigModal));
  elements.bridgeCenterChannelClose?.addEventListener("click", () => closeModal(elements.bridgeCenterChannelModal));
  elements.bridgeCenterChannelCancel?.addEventListener("click", () => closeModal(elements.bridgeCenterChannelModal));
  elements.bridgeCenterRouteStatusFilter?.addEventListener("change", async () => {
    state.bridgeCenter.routeStatus = elements.bridgeCenterRouteStatusFilter.value || "";
    await loadBridgeRoutes();
  });
  elements.bridgeCenterRoutesRefreshBtn?.addEventListener("click", () => Promise.all([loadBridgeRoutes(), loadBridgeLogs()]).catch((error) => notify(error.message, "error")));
  elements.bridgeCenterRouteActivateBtn?.addEventListener("click", () => patchRouteStatus("active").catch((error) => notify(error.message, "error")));
  elements.bridgeCenterRoutePauseBtn?.addEventListener("click", () => patchRouteStatus("paused").catch((error) => notify(error.message, "error")));
  elements.bridgeCenterRouteBlockBtn?.addEventListener("click", () => patchRouteStatus("blocked").catch((error) => notify(error.message, "error")));
  [elements.bridgeCenterConfigModal, elements.bridgeCenterChannelModal].forEach((modal) => {
    modal?.addEventListener("click", (event) => {
      if (event.target === modal) closeModal(modal);
    });
  });
};

export { loadBridgeCenters };
