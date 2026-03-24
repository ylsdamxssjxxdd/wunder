import { elements } from "./elements.js?v=20260324-01";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { formatTimestamp } from "./utils.js?v=20251229-02";
import { notify } from "./notify.js";

const WEIXIN_CHANNEL = "weixin";
const DEFAULT_WEIXIN_API_BASE = "https://ilinkai.weixin.qq.com";
const DEFAULT_WEIXIN_BOT_TYPE = "1";
const BRIDGE_RUNTIME_LOG_POLL_INTERVAL_MS = 5000;

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
});

const emptyWeixinQrState = () => ({
  sessionKey: "",
  qrcode: "",
  qrcodeUrl: "",
  apiBase: DEFAULT_WEIXIN_API_BASE,
  status: "",
  message: "",
  loadingStart: false,
  loadingWait: false,
  imageRetryCount: 0,
});

const emptyRuntimeState = () => ({
  items: [],
  status: null,
  error: "",
  loading: false,
  probeLoading: false,
  clearedAt: 0,
});

const ensureBridgeState = () => {
  if (!state.bridgeCenter) {
    state.bridgeCenter = {};
  }
  state.bridgeCenter.meta ||= null;
  state.bridgeCenter.centers ||= [];
  state.bridgeCenter.accounts ||= [];
  state.bridgeCenter.availableAccounts ||= [];
  state.bridgeCenter.routes ||= [];
  state.bridgeCenter.logs ||= [];
  state.bridgeCenter.selectedCenterId ||= "";
  state.bridgeCenter.selectedAccountId ||= "";
  state.bridgeCenter.selectedRouteId ||= "";
  state.bridgeCenter.routeStatus ||= "";
  state.bridgeCenter.configEditingCenterId ||= "";
  state.bridgeCenter.channelForm ||= emptyChannelForm();
  state.bridgeCenter.weixinQr ||= emptyWeixinQrState();
  state.bridgeCenter.channelRuntime ||= emptyRuntimeState();
};

let bridgeRuntimeLogPollTimer = null;
let bridgeRuntimeLogRequestId = 0;

const channelMeta = (channel) =>
  (state.bridgeCenter.meta?.supported_channels || []).find((item) => item.channel === cleanText(channel).toLowerCase()) || null;

const resolveChannelLabel = (channel) => {
  const hit = channelMeta(channel);
  return hit?.display_name || channel || "-";
};

const cleanText = (value) => String(value || "").trim();
const isPlainObject = (value) => Boolean(value) && typeof value === "object" && !Array.isArray(value);
const bridgeAccountKey = (channel, accountId) => `${cleanText(channel).toLowerCase()}::${cleanText(accountId).toLowerCase()}`;
const isWeixinChannel = (channel) => cleanText(channel).toLowerCase() === WEIXIN_CHANNEL;

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
  state.bridgeCenter.accounts.find((item) => item.center_account_id === state.bridgeCenter.selectedAccountId) ||
  state.bridgeCenter.accounts[0] ||
  null;

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
const currentWeixinQrState = () => state.bridgeCenter.weixinQr || emptyWeixinQrState();

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
  route_count: Number(item.route_count) || 0,
  updated_at: Number(item.updated_at) || 0,
});

const normalizeAvailableChannelAccount = (item = {}) => {
  const config = isPlainObject(item.config) ? item.config : {};
  return {
    key: bridgeAccountKey(item.channel, item.account_id),
    channel: cleanText(item.channel).toLowerCase(),
    account_id: cleanText(item.account_id),
    name: cleanText(config.display_name),
    status: cleanText(item.status) || "active",
    active: cleanText(item.status || "active").toLowerCase() === "active",
    meta: {},
    raw_config: config,
    updated_at: Number(item.updated_at) || 0,
  };
};

const resetWeixinQrState = () => {
  state.bridgeCenter.weixinQr = emptyWeixinQrState();
};

const normalizeWeixinQrImageValue = (rawValue, apiBase = "") => {
  let value = cleanText(rawValue);
  if (!value) {
    return "";
  }
  if (
    (value.startsWith('"') && value.endsWith('"')) ||
    (value.startsWith("'") && value.endsWith("'"))
  ) {
    value = value.slice(1, -1).trim();
  }
  value = value
    .replace(/\\r\\n/g, "")
    .replace(/\\n/g, "")
    .replace(/\\r/g, "")
    .replace(/\r\n/g, "")
    .replace(/\n/g, "")
    .replace(/\r/g, "")
    .trim();
  if (!value) {
    return "";
  }
  if (value.startsWith("data:image/")) {
    return value;
  }
  const compact = value.replace(/\s+/g, "");
  const base64Candidate = compact.replace(/^data:image\/[a-z0-9.+-]+;base64,/i, "");
  if (base64Candidate.length > 64 && /^[A-Za-z0-9+/]+=*$/.test(base64Candidate)) {
    return `data:image/png;base64,${base64Candidate}`;
  }
  if (value.startsWith("blob:") || /^https?:\/\//i.test(value)) {
    return value;
  }
  if (value.startsWith("//")) {
    return `${window.location.protocol}${value}`;
  }
  if (value.startsWith("/")) {
    if (apiBase) {
      try {
        return new URL(value, apiBase).toString();
      } catch (error) {
        return `${window.location.origin}${value}`;
      }
    }
    return `${window.location.origin}${value}`;
  }
  return "";
};

const resolveWeixinQrPreviewUrl = (qrState) =>
  normalizeWeixinQrImageValue(qrState.qrcodeUrl, qrState.apiBase) ||
  normalizeWeixinQrImageValue(qrState.qrcode, qrState.apiBase);

const formatWeixinQrStatus = (status) => {
  const normalized = cleanText(status).toLowerCase();
  if (!normalized) {
    return "";
  }
  const labels = {
    wait: "等待扫码",
    confirmed: "已确认",
    expired: "已过期",
  };
  return labels[normalized] || normalized;
};

const resolveSelectedChannel = () =>
  cleanText(elements.bridgeCenterChannelFormChannel?.value || state.bridgeCenter.channelForm.channel).toLowerCase();

const findAutoBindCandidate = (channel) => {
  const normalizedChannel = cleanText(channel).toLowerCase();
  if (!normalizedChannel) {
    return null;
  }
  const current = currentAccount();
  if (current?.channel === normalizedChannel && cleanText(current.account_id)) {
    return {
      channel: normalizedChannel,
      account_id: cleanText(current.account_id),
      name: cleanText(current.name),
      active: current.active !== false,
      fromCurrent: true,
    };
  }
  const activeItem =
    state.bridgeCenter.availableAccounts.find(
      (item) => item.channel === normalizedChannel && item.active
    ) || null;
  if (activeItem) {
    return {
      channel: normalizedChannel,
      account_id: cleanText(activeItem.account_id),
      name: cleanText(activeItem.name),
      active: true,
      fromCurrent: false,
    };
  }
  const fallbackItem =
    state.bridgeCenter.availableAccounts.find((item) => item.channel === normalizedChannel) || null;
  if (!fallbackItem) {
    return null;
  }
  return {
    channel: normalizedChannel,
    account_id: cleanText(fallbackItem.account_id),
    name: cleanText(fallbackItem.name),
    active: fallbackItem.active !== false,
    fromCurrent: false,
  };
};

const resolveChannelBindingAccountId = (channel) => {
  const normalizedChannel = cleanText(channel).toLowerCase();
  if (!normalizedChannel) {
    return "";
  }
  if (isWeixinChannel(normalizedChannel)) {
    const current = currentAccount();
    if (current?.channel === WEIXIN_CHANNEL && cleanText(current.account_id)) {
      state.bridgeCenter.channelForm.account_id = cleanText(current.account_id);
      return state.bridgeCenter.channelForm.account_id;
    }
    state.bridgeCenter.channelForm.account_id = "";
    return "";
  }
  const candidate = findAutoBindCandidate(normalizedChannel);
  const accountId = cleanText(candidate?.account_id);
  state.bridgeCenter.channelForm.account_id = accountId;
  return accountId;
};

const renderChannelAutoBindSection = () => {
  const channel = resolveSelectedChannel();
  const show = Boolean(channel) && !isWeixinChannel(channel);
  if (elements.bridgeCenterChannelAutoBindSection) {
    elements.bridgeCenterChannelAutoBindSection.hidden = !show;
  }
  if (!show) {
    return;
  }
  const candidate = findAutoBindCandidate(channel);
  const channelLabel = resolveChannelLabel(channel);
  if (!candidate?.account_id) {
    if (elements.bridgeCenterChannelAutoBindSummary) {
      elements.bridgeCenterChannelAutoBindSummary.textContent = `${channelLabel} 暂无可绑定账号，请先在“渠道监控”创建并启用账号。`;
    }
    state.bridgeCenter.channelForm.account_id = "";
    return;
  }
  state.bridgeCenter.channelForm.account_id = candidate.account_id;
  const base = `${channelLabel} 将绑定账号 ${candidate.account_id}`;
  const named = candidate.name ? `（${candidate.name}）` : "";
  const source = candidate.fromCurrent ? "（沿用当前绑定）" : "（自动选择可用账号）";
  if (elements.bridgeCenterChannelAutoBindSummary) {
    elements.bridgeCenterChannelAutoBindSummary.textContent = `${base}${named}${source}`;
  }
};

const renderWeixinQrPanel = () => {
  const show = isWeixinChannel(resolveSelectedChannel());
  const qrState = currentWeixinQrState();
  const previewUrl = resolveWeixinQrPreviewUrl(qrState);
  const canOpenPreview = /^https?:\/\//i.test(previewUrl) || previewUrl.startsWith("blob:");
  if (elements.bridgeCenterWeixinQrSection) {
    elements.bridgeCenterWeixinQrSection.hidden = !show;
  }
  if (!show) {
    return;
  }
  if (elements.bridgeCenterWeixinQrPreview) {
    elements.bridgeCenterWeixinQrPreview.hidden = !previewUrl;
  }
  if (elements.bridgeCenterWeixinQrImage) {
    if (previewUrl) {
      elements.bridgeCenterWeixinQrImage.src = previewUrl;
    } else {
      elements.bridgeCenterWeixinQrImage.removeAttribute("src");
    }
  }
  if (elements.bridgeCenterWeixinQrOpenLink) {
    elements.bridgeCenterWeixinQrOpenLink.hidden = !previewUrl || !canOpenPreview;
    elements.bridgeCenterWeixinQrOpenLink.href = previewUrl || "#";
  }
  if (elements.bridgeCenterWeixinQrSession) {
    elements.bridgeCenterWeixinQrSession.textContent = qrState.sessionKey ? `会话 Key: ${qrState.sessionKey}` : "";
  }
  if (elements.bridgeCenterWeixinQrStatus) {
    elements.bridgeCenterWeixinQrStatus.textContent = qrState.status ? `状态: ${formatWeixinQrStatus(qrState.status)}` : "";
  }
  if (elements.bridgeCenterWeixinQrMessage) {
    elements.bridgeCenterWeixinQrMessage.textContent = qrState.message || "";
  }
  if (elements.bridgeCenterWeixinQrStartBtn) {
    const loading = qrState.loadingStart || qrState.loadingWait;
    elements.bridgeCenterWeixinQrStartBtn.disabled = loading;
    const span = elements.bridgeCenterWeixinQrStartBtn.querySelector("span");
    const label = loading ? "处理中..." : qrState.sessionKey ? "重新生成二维码" : "生成二维码";
    if (span) {
      span.textContent = label;
    } else {
      elements.bridgeCenterWeixinQrStartBtn.textContent = label;
    }
  }
};

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
  refreshChannelAccountOptions();
};

const refreshChannelAccountOptions = () => {
  const channel = resolveSelectedChannel();
  state.bridgeCenter.channelForm.channel = channel;
  resolveChannelBindingAccountId(channel);
  renderChannelAutoBindSection();
  renderWeixinQrPanel();
  if (isBridgeChannelModalOpen()) {
    void refreshBridgeRuntimeLogs(true);
  }
};

const confirmChannelReplacement = (channel, accountId) => {
  const existing = currentAccount();
  if (!existing?.center_account_id) {
    return true;
  }
  const sameBinding =
    existing.channel === cleanText(channel).toLowerCase() && existing.account_id === cleanText(accountId);
  if (sameBinding) {
    return true;
  }
  return window.confirm("切换渠道会清理当前节点已有的自动路由和投递日志，确认继续吗？");
};

const bindWeixinQrResult = async (result) => {
  const center = currentCenter();
  const existing = currentAccount();
  if (!center?.center_id) {
    throw new Error("请先保存舰桥节点，再配置渠道");
  }
  const botToken = cleanText(result.bot_token);
  const ilinkBotId = cleanText(result.ilink_bot_id);
  if (!botToken || !ilinkBotId) {
    throw new Error("Weixin 扫码结果不完整，请重新生成二维码");
  }
  const existingWeixinAccountId =
    existing?.channel === WEIXIN_CHANNEL ? cleanText(existing.account_id) : "";
  const payload = {
    account_id: existingWeixinAccountId || undefined,
    api_base: cleanText(result.api_base) || currentWeixinQrState().apiBase || DEFAULT_WEIXIN_API_BASE,
    bot_type: DEFAULT_WEIXIN_BOT_TYPE,
    bot_token: botToken,
    ilink_bot_id: ilinkBotId,
    ilink_user_id: cleanText(result.ilink_user_id) || undefined,
  };
  await fetchJson(`/admin/bridge/centers/${encodeURIComponent(center.center_id)}/weixin_bind`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(payload),
  });
  await loadBridgeCenters({ silent: true, selectedCenterId: center.center_id });
  closeModal(elements.bridgeCenterChannelModal);
  clearBridgeRuntimeLogTimer();
  notify("Weixin 渠道已扫码绑定", "success");
};

const waitForWeixinQr = async () => {
  const qrState = currentWeixinQrState();
  if (!cleanText(qrState.sessionKey)) {
    return;
  }
  qrState.loadingWait = true;
  qrState.message = "等待扫码确认...";
  renderWeixinQrPanel();
  try {
    const payload = await fetchJson("/channels/weixin/qr/wait", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        session_key: qrState.sessionKey,
        api_base: qrState.apiBase || DEFAULT_WEIXIN_API_BASE,
        timeout_ms: 120000,
      }),
    });
    const result = isPlainObject(payload?.data) ? payload.data : {};
    qrState.status = cleanText(result.status).toLowerCase() || qrState.status || "wait";
    qrState.message = cleanText(result.message) || qrState.message;
    qrState.apiBase = cleanText(result.api_base) || qrState.apiBase || DEFAULT_WEIXIN_API_BASE;
    renderWeixinQrPanel();
    if (result.connected === true) {
      await bindWeixinQrResult(result);
      return;
    }
    if (qrState.status === "expired") {
      qrState.message = qrState.message || "二维码已过期，请重新生成";
      notify("Weixin 二维码已过期", "warning");
    }
  } catch (error) {
    qrState.message = error.message || "Weixin 扫码确认失败";
    notify(qrState.message, "error");
  } finally {
    qrState.loadingWait = false;
    renderWeixinQrPanel();
  }
};

const startWeixinQr = async (force = false) => {
  const center = currentCenter();
  if (!center?.center_id) {
    notify("请先保存舰桥节点，再配置渠道", "warning");
    return;
  }
  if (!isWeixinChannel(resolveSelectedChannel())) {
    return;
  }
  const currentWeixinAccountId = currentAccount()?.channel === WEIXIN_CHANNEL ? currentAccount()?.account_id || "" : "";
  if (!confirmChannelReplacement(WEIXIN_CHANNEL, currentWeixinAccountId)) {
    return;
  }
  const qrState = currentWeixinQrState();
  qrState.loadingStart = true;
  qrState.message = "";
  qrState.status = "";
  renderWeixinQrPanel();
  try {
    const payload = await fetchJson("/channels/weixin/qr/start", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        api_base: qrState.apiBase || DEFAULT_WEIXIN_API_BASE,
        bot_type: DEFAULT_WEIXIN_BOT_TYPE,
        force,
      }),
    });
    const result = isPlainObject(payload?.data) ? payload.data : {};
    qrState.sessionKey = cleanText(result.session_key);
    qrState.qrcode = cleanText(result.qrcode);
    qrState.qrcodeUrl = cleanText(result.qrcode_url);
    qrState.apiBase = cleanText(result.api_base) || DEFAULT_WEIXIN_API_BASE;
    qrState.status = "wait";
    qrState.message = "二维码已生成，请扫码确认";
    qrState.imageRetryCount = 0;
    renderWeixinQrPanel();
    if (!qrState.sessionKey || !resolveWeixinQrPreviewUrl(qrState)) {
      throw new Error("Weixin 二维码生成失败");
    }
    void waitForWeixinQr();
  } catch (error) {
    qrState.message = error.message || "Weixin 二维码生成失败";
    notify(qrState.message, "error");
  } finally {
    qrState.loadingStart = false;
    renderWeixinQrPanel();
  }
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
  const account = currentAccount();
  const units = state.bridgeCenter.meta?.org_units || [];
  const unit = units.find((item) => item.unit_id === center?.target_unit_id);
  if (elements.bridgeCenterCurrentName) {
    elements.bridgeCenterCurrentName.textContent = center?.name || "未选择舰桥节点";
  }
  if (elements.bridgeCenterOwner) {
    elements.bridgeCenterOwner.textContent = center
      ? `${center.owner_username ? `创建人：${center.owner_username} | ` : ""}更新时间：${safeTs(center.updated_at)}`
      : "先创建舰桥节点，再配置渠道。";
  }
  if (elements.bridgeCenterSummaryStatus) elements.bridgeCenterSummaryStatus.textContent = center?.status || "-";
  if (elements.bridgeCenterSummaryPreset) elements.bridgeCenterSummaryPreset.textContent = center?.default_preset_agent_name || "-";
  if (elements.bridgeCenterSummaryUnit) elements.bridgeCenterSummaryUnit.textContent = unit?.path_name || unit?.name || "默认不指定";
  if (elements.bridgeCenterSummaryChannels) {
    const channelLabel = account ? resolveChannelLabel(account.channel) : "";
    elements.bridgeCenterSummaryChannels.textContent = account
      ? `${channelLabel === account.channel ? channelLabel : `${channelLabel} (${account.channel})`} / ${account.account_id}`
      : "未配置";
  }
  if (elements.bridgeCenterSummaryRoutes) elements.bridgeCenterSummaryRoutes.textContent = String(center?.route_count || 0);
  if (elements.bridgeCenterSummaryActiveRoutes) elements.bridgeCenterSummaryActiveRoutes.textContent = String(center?.active_route_count || 0);
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
    row.innerHTML = '<td colspan="8" class="muted">暂无渠道绑定</td>';
    elements.bridgeCenterAccountList.appendChild(row);
    return;
  }
  state.bridgeCenter.accounts.forEach((account) => {
    const configStatus = account.owned ? (account.meta?.configured === false ? "未配置" : "已配置") : "外部账号";
    const channelLabel = resolveChannelLabel(account.channel);
    const row = document.createElement("tr");
    row.innerHTML = `
      <td>${channelLabel === account.channel ? channelLabel : `${channelLabel} (${account.channel})`}</td>
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

const loadAvailableChannelAccounts = async () => {
  const payload = await fetchJson("/admin/channels/accounts?status=active");
  state.bridgeCenter.availableAccounts = (payload?.data?.items || []).map((item) => normalizeAvailableChannelAccount(item));
  refreshChannelAccountOptions();
};

const loadBridgeCenterAccounts = async () => {
  const centerId = state.bridgeCenter.selectedCenterId;
  if (!centerId) {
    state.bridgeCenter.accounts = [];
    renderAccountList();
    renderCenterOverview();
    return;
  }
  if (!state.bridgeCenter.availableAccounts.length) {
    await loadAvailableChannelAccounts().catch(() => null);
  }
  const bridgePayload = await fetchJson(`/admin/bridge/centers/${encodeURIComponent(centerId)}/accounts`);
  const bridgeAccounts = (bridgePayload?.data?.items || []).map((item) => normalizeBridgeAccount(item));
  state.bridgeCenter.accounts = mergeBridgeAccounts(bridgeAccounts, state.bridgeCenter.availableAccounts);
  state.bridgeCenter.selectedAccountId = state.bridgeCenter.accounts[0]?.center_account_id || "";
  renderAccountList();
  renderCenterOverview();
  refreshChannelAccountOptions();
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
    state.bridgeCenter.selectedAccountId = "";
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
  resetWeixinQrState();
  if (!account) {
    state.bridgeCenter.selectedAccountId = "";
    state.bridgeCenter.channelForm = {
      mode: "create",
      center_account_id: "",
      channel: state.bridgeCenter.meta?.supported_channels?.[0]?.channel || "",
      account_id: "",
    };
  } else {
    state.bridgeCenter.selectedAccountId = account.center_account_id;
    state.bridgeCenter.channelForm = {
      mode: "edit",
      center_account_id: account.center_account_id,
      channel: account.channel,
      account_id: account.account_id,
    };
  }
  const form = state.bridgeCenter.channelForm;
  if (elements.bridgeCenterChannelModalTitle) elements.bridgeCenterChannelModalTitle.textContent = "渠道设置";
  if (elements.bridgeCenterChannelEditorHint) {
    elements.bridgeCenterChannelEditorHint.textContent =
      form.mode === "edit"
        ? "当前节点已经绑定了一个渠道账号；切换绑定会清理该节点已有的自动路由和投递日志。"
        : "每个舰桥节点只绑定一个渠道账号。";
  }
  if (elements.bridgeCenterChannelOwnedBadge) elements.bridgeCenterChannelOwnedBadge.textContent = form.mode === "edit" ? "已绑定" : "未绑定";
  if (elements.bridgeCenterChannelFormChannel) {
    elements.bridgeCenterChannelFormChannel.value = form.channel || "";
    elements.bridgeCenterChannelFormChannel.disabled = false;
  }
  if (elements.bridgeCenterChannelDeleteBtn) elements.bridgeCenterChannelDeleteBtn.disabled = form.mode !== "edit";
  refreshChannelAccountOptions();
  renderWeixinQrPanel();
  renderBridgeRuntimeLogs();
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
    throw new Error("请先保存舰桥节点，再配置渠道");
  }
  const channel = cleanText(elements.bridgeCenterChannelFormChannel?.value).toLowerCase();
  if (!channel) {
    throw new Error("请选择渠道");
  }
  const accountId = resolveChannelBindingAccountId(channel);
  if (!accountId) {
    if (isWeixinChannel(channel)) {
      throw new Error("请先完成 Weixin 扫码绑定");
    }
    throw new Error("当前渠道暂无可绑定账号，请先在“渠道监控”创建账号");
  }
  if (!confirmChannelReplacement(channel, accountId)) {
    return;
  }
  const existing = currentAccount();
  const bridgePayload = {
    center_id: center.center_id,
    channel,
    account_id: accountId,
    enabled: true,
  };
  const bindingChanged = !existing || existing.channel !== channel || existing.account_id !== accountId;
  if (existing?.center_account_id && !bindingChanged) {
    await fetchJson(`/admin/bridge/accounts/${encodeURIComponent(existing.center_account_id)}`, {
      method: "PATCH",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(bridgePayload),
    });
  } else {
    if (existing?.center_account_id) {
      await fetchJson(`/admin/bridge/accounts/${encodeURIComponent(existing.center_account_id)}`, {
        method: "DELETE",
      });
    }
    await fetchJson(`/admin/bridge/centers/${encodeURIComponent(center.center_id)}/accounts`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(bridgePayload),
    });
  }
  await loadBridgeCenters({ silent: true, selectedCenterId: center.center_id });
  closeModal(elements.bridgeCenterChannelModal);
  clearBridgeRuntimeLogTimer();
  notify("渠道设置已保存", "success");
};

const removeChannelConfig = async () => {
  const center = currentCenter();
  const account = currentAccount();
  if (!center?.center_id || !account?.center_account_id) {
    return;
  }
  if (!window.confirm(`确认移除 ${account.channel} / ${account.account_id} 吗？这会清理该节点已有的自动路由和投递日志。`)) {
    return;
  }
  await fetchJson(`/admin/bridge/accounts/${encodeURIComponent(account.center_account_id)}`, { method: "DELETE" });
  await loadBridgeCenters({ silent: true, selectedCenterId: center.center_id });
  closeModal(elements.bridgeCenterChannelModal);
  clearBridgeRuntimeLogTimer();
  notify("渠道绑定已移除", "success");
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

const clearBridgeRuntimeLogTimer = () => {
  if (bridgeRuntimeLogPollTimer === null) {
    return;
  }
  clearTimeout(bridgeRuntimeLogPollTimer);
  bridgeRuntimeLogPollTimer = null;
};

const isBridgeChannelModalOpen = () =>
  Boolean(elements.bridgeCenterChannelModal?.classList.contains("active"));

const resolveRuntimeLogTarget = () => {
  const channel = resolveSelectedChannel();
  if (!channel) {
    return null;
  }
  const accountId = resolveChannelBindingAccountId(channel);
  return {
    channel,
    account_id: accountId,
  };
};

const formatRuntimeLogTs = (ts) => {
  const value = Number(ts);
  if (!Number.isFinite(value) || value <= 0) {
    return "-";
  }
  return formatTimestamp(value * 1000);
};

const renderBridgeRuntimeLogs = () => {
  const runtime = state.bridgeCenter.channelRuntime || emptyRuntimeState();
  const visibleItems = runtime.items.filter((item) => Number(item.ts || 0) > Number(runtime.clearedAt || 0));
  if (elements.bridgeCenterChannelRuntimeStatus) {
    const status = runtime.status;
    if (status && typeof status === "object") {
      const ownedAccounts = Number(status.owned_accounts || 0);
      const scannedTotal = Number(status.scanned_total || 0);
      const tsText = formatRuntimeLogTs(status.server_ts);
      elements.bridgeCenterChannelRuntimeStatus.textContent = `扫描 ${scannedTotal} 条 | 账号 ${ownedAccounts}${tsText !== "-" ? ` | ${tsText}` : ""}`;
    } else {
      elements.bridgeCenterChannelRuntimeStatus.textContent = "";
    }
  }
  if (elements.bridgeCenterChannelRuntimeError) {
    const hasError = Boolean(runtime.error);
    elements.bridgeCenterChannelRuntimeError.hidden = !hasError;
    elements.bridgeCenterChannelRuntimeError.textContent = runtime.error || "";
  }
  if (elements.bridgeCenterChannelRuntimeEmpty) {
    elements.bridgeCenterChannelRuntimeEmpty.hidden = visibleItems.length > 0;
  }
  if (elements.bridgeCenterChannelRuntimeList) {
    elements.bridgeCenterChannelRuntimeList.textContent = "";
    visibleItems.forEach((item) => {
      const row = document.createElement("div");
      row.className = "bridge-runtime-log-item";
      const level = cleanText(item.level).toLowerCase() || "info";
      const levelClass =
        level === "error" ? "is-error" : level === "warn" || level === "warning" ? "is-warn" : "is-info";
      row.innerHTML = `
        <div class="bridge-runtime-log-item-top">
          <span class="bridge-runtime-log-item-level ${levelClass}">${level.toUpperCase()}</span>
          <span class="bridge-runtime-log-item-meta">${cleanText(item.event) || "-"} | ${formatRuntimeLogTs(item.ts)}</span>
        </div>
        <div class="bridge-runtime-log-item-meta">${cleanText(item.channel)}/${cleanText(item.account_id) || "-"}</div>
        <div class="bridge-runtime-log-item-message">${cleanText(item.message) || "-"}</div>
      `;
      elements.bridgeCenterChannelRuntimeList.appendChild(row);
    });
  }
  if (elements.bridgeCenterChannelRuntimeRefreshBtn) {
    elements.bridgeCenterChannelRuntimeRefreshBtn.disabled = runtime.loading;
  }
  if (elements.bridgeCenterChannelRuntimeProbeBtn) {
    elements.bridgeCenterChannelRuntimeProbeBtn.disabled = runtime.probeLoading || runtime.loading;
  }
};

const scheduleBridgeRuntimeLogPolling = () => {
  clearBridgeRuntimeLogTimer();
  if (!isBridgeChannelModalOpen()) {
    return;
  }
  bridgeRuntimeLogPollTimer = setTimeout(() => {
    if (!isBridgeChannelModalOpen()) {
      return;
    }
    void refreshBridgeRuntimeLogs(true);
  }, BRIDGE_RUNTIME_LOG_POLL_INTERVAL_MS);
};

const refreshBridgeRuntimeLogs = async (silent = false) => {
  const runtime = state.bridgeCenter.channelRuntime || emptyRuntimeState();
  const target = resolveRuntimeLogTarget();
  if (!target?.channel) {
    runtime.items = [];
    runtime.status = null;
    runtime.error = "";
    state.bridgeCenter.channelRuntime = runtime;
    renderBridgeRuntimeLogs();
    return;
  }
  const requestId = ++bridgeRuntimeLogRequestId;
  if (!silent) {
    runtime.loading = true;
    state.bridgeCenter.channelRuntime = runtime;
    renderBridgeRuntimeLogs();
  }
  try {
    const params = new URLSearchParams({
      limit: "80",
      channel: target.channel,
    });
    if (target.account_id) {
      params.set("account_id", target.account_id);
    }
    const payload = await fetchJson(`/admin/channels/runtime_logs?${params.toString()}`);
    if (requestId !== bridgeRuntimeLogRequestId) {
      return;
    }
    const data = payload?.data || {};
    runtime.items = Array.isArray(data.items) ? data.items : [];
    runtime.status = isPlainObject(data.status) ? data.status : null;
    runtime.error = "";
  } catch (error) {
    if (requestId !== bridgeRuntimeLogRequestId) {
      return;
    }
    runtime.items = [];
    runtime.status = null;
    runtime.error = error.message || "运行日志加载失败";
  } finally {
    if (requestId === bridgeRuntimeLogRequestId) {
      runtime.loading = false;
      state.bridgeCenter.channelRuntime = runtime;
      renderBridgeRuntimeLogs();
      scheduleBridgeRuntimeLogPolling();
    }
  }
};

const writeBridgeRuntimeProbe = async () => {
  const runtime = state.bridgeCenter.channelRuntime || emptyRuntimeState();
  const target = resolveRuntimeLogTarget();
  if (!target?.channel) {
    notify("请先选择渠道", "warning");
    return;
  }
  runtime.probeLoading = true;
  state.bridgeCenter.channelRuntime = runtime;
  renderBridgeRuntimeLogs();
  try {
    const payload = {
      channel: target.channel,
      account_id: target.account_id || undefined,
    };
    await fetchJson("/admin/channels/runtime_logs/probe", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(payload),
    });
    notify("运行日志探针已写入", "success");
    await refreshBridgeRuntimeLogs(true);
  } catch (error) {
    notify(error.message || "运行日志探针写入失败", "error");
  } finally {
    runtime.probeLoading = false;
    state.bridgeCenter.channelRuntime = runtime;
    renderBridgeRuntimeLogs();
  }
};

const clearBridgeRuntimeLogView = () => {
  const runtime = state.bridgeCenter.channelRuntime || emptyRuntimeState();
  runtime.clearedAt = Date.now() / 1000;
  state.bridgeCenter.channelRuntime = runtime;
  renderBridgeRuntimeLogs();
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
    if (elements.bridgeCenterConfigOwner) elements.bridgeCenterConfigOwner.textContent = "保存后即可继续配置节点渠道";
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
    if (elements.bridgeCenterConfigOwner) elements.bridgeCenterConfigOwner.textContent = center.owner_username ? `创建人：${center.owner_username} | 最近更新：${safeTs(center.updated_at)}` : "保存后即可继续配置节点渠道";
    openModal(elements.bridgeCenterConfigModal);
  });
  elements.bridgeCenterChannelsBtn?.addEventListener("click", async () => {
    if (!currentCenter()?.center_id) return notify("请先保存舰桥节点，再配置渠道", "warning");
    await Promise.all([loadAvailableChannelAccounts(), loadBridgeCenterAccounts()]);
    applyChannelForm(currentAccount());
    openModal(elements.bridgeCenterChannelModal);
    const runtime = state.bridgeCenter.channelRuntime || emptyRuntimeState();
    runtime.clearedAt = 0;
    runtime.error = "";
    state.bridgeCenter.channelRuntime = runtime;
    renderBridgeRuntimeLogs();
    void refreshBridgeRuntimeLogs(true);
  });
  elements.bridgeCenterConfigSaveBtn?.addEventListener("click", () => saveCenterConfig().catch((error) => notify(error.message, "error")));
  elements.bridgeCenterChannelSaveBtn?.addEventListener("click", () => saveChannelConfig().catch((error) => notify(error.message, "error")));
  elements.bridgeCenterChannelDeleteBtn?.addEventListener("click", () => removeChannelConfig().catch((error) => notify(error.message, "error")));
  elements.bridgeCenterChannelFormChannel?.addEventListener("change", () => {
    resetWeixinQrState();
    const channel = cleanText(elements.bridgeCenterChannelFormChannel.value).toLowerCase();
    state.bridgeCenter.channelForm.channel = channel;
    state.bridgeCenter.channelForm.account_id = "";
    refreshChannelAccountOptions();
  });
  elements.bridgeCenterWeixinQrStartBtn?.addEventListener("click", () => startWeixinQr(Boolean(currentWeixinQrState().sessionKey)).catch((error) => notify(error.message, "error")));
  elements.bridgeCenterWeixinQrImage?.addEventListener("click", () => startWeixinQr(true).catch((error) => notify(error.message, "error")));
  elements.bridgeCenterWeixinQrImage?.addEventListener("error", () => {
    const qrState = currentWeixinQrState();
    if (!isWeixinChannel(resolveSelectedChannel())) {
      return;
    }
    if (qrState.imageRetryCount >= 1 || qrState.loadingStart || qrState.loadingWait) {
      return;
    }
    qrState.imageRetryCount += 1;
    qrState.message = "二维码加载失败，正在刷新...";
    renderWeixinQrPanel();
    void startWeixinQr(true).catch((error) => notify(error.message, "error"));
  });
  elements.bridgeCenterChannelRuntimeRefreshBtn?.addEventListener("click", () => {
    void refreshBridgeRuntimeLogs();
  });
  elements.bridgeCenterChannelRuntimeProbeBtn?.addEventListener("click", () => {
    void writeBridgeRuntimeProbe();
  });
  elements.bridgeCenterChannelRuntimeClearBtn?.addEventListener("click", () => {
    clearBridgeRuntimeLogView();
  });
  elements.bridgeCenterConfigClose?.addEventListener("click", () => closeModal(elements.bridgeCenterConfigModal));
  elements.bridgeCenterConfigCancel?.addEventListener("click", () => closeModal(elements.bridgeCenterConfigModal));
  elements.bridgeCenterChannelClose?.addEventListener("click", () => {
    closeModal(elements.bridgeCenterChannelModal);
    clearBridgeRuntimeLogTimer();
  });
  elements.bridgeCenterChannelCancel?.addEventListener("click", () => {
    closeModal(elements.bridgeCenterChannelModal);
    clearBridgeRuntimeLogTimer();
  });
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
      if (event.target === modal) {
        closeModal(modal);
        if (modal === elements.bridgeCenterChannelModal) {
          clearBridgeRuntimeLogTimer();
        }
      }
    });
  });
};

export { loadBridgeCenters };




