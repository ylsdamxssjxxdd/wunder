import { elements } from "./elements.js?v=20260215-01";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { formatTimestamp, isPlainObject } from "./utils.js?v=20251229-02";
import { notify } from "./notify.js";
import { t } from "./i18n.js?v=20260215-01";

const FEISHU_LONG_CONNECTION_STATUS_KEYS = {
  running: "channels.runtime.feishu.status.running",
  waiting_binding: "channels.runtime.feishu.status.waitingBinding",
  missing_credentials: "channels.runtime.feishu.status.missingCredentials",
  disabled: "channels.runtime.feishu.status.disabled",
  account_inactive: "channels.runtime.feishu.status.accountInactive",
  not_configured: "channels.runtime.feishu.status.notConfigured",
  unknown: "channels.runtime.feishu.status.unknown",
};

const WEIXIN_LONG_CONNECTION_STATUS_KEYS = {
  running: "channels.runtime.weixin.status.running",
  waiting_binding: "channels.runtime.weixin.status.waitingBinding",
  missing_credentials: "channels.runtime.weixin.status.missingCredentials",
  disabled: "channels.runtime.weixin.status.disabled",
  account_inactive: "channels.runtime.weixin.status.accountInactive",
  not_configured: "channels.runtime.weixin.status.notConfigured",
  unknown: "channels.runtime.weixin.status.unknown",
};

const ACTIVITY_FILTER_WINDOW_SECONDS = {
  "1h": 3600,
  "24h": 24 * 3600,
  "7d": 7 * 24 * 3600,
  "30d": 30 * 24 * 3600,
};

const ensureChannelsState = () => {
  if (!state.channels) {
    state.channels = {};
  }
  if (!Array.isArray(state.channels.accounts)) {
    state.channels.accounts = [];
  }
  if (!Number.isFinite(state.channels.selectedIndex)) {
    state.channels.selectedIndex = -1;
  }
  if (!isPlainObject(state.channels.filters)) {
    state.channels.filters = {};
  }
  if (typeof state.channels.filters.keyword !== "string") {
    state.channels.filters.keyword = "";
  }
  if (typeof state.channels.filters.status !== "string") {
    state.channels.filters.status = "";
  }
  if (typeof state.channels.filters.activity !== "string") {
    state.channels.filters.activity = "";
  }
  if (typeof state.channels.filters.issueOnly !== "boolean") {
    state.channels.filters.issueOnly = false;
  }
  if (!(state.channels.selectedKeys instanceof Set)) {
    const fallbackKeys = Array.isArray(state.channels.selectedKeys) ? state.channels.selectedKeys : [];
    state.channels.selectedKeys = new Set(
      fallbackKeys
        .map((value) => String(value || "").trim())
        .filter(Boolean)
    );
  }
  if (typeof state.channels.batchBusy !== "boolean") {
    state.channels.batchBusy = false;
  }
};

const toSafeInt = (value, fallback = 0) => {
  const parsed = Math.floor(Number(value));
  if (!Number.isFinite(parsed)) {
    return fallback;
  }
  return Math.max(0, parsed);
};

const toSafeFloat = (value, fallback = 0) => {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) {
    return fallback;
  }
  return parsed;
};

const formatEpochSeconds = (value) => {
  const ts = Number(value);
  if (!Number.isFinite(ts) || ts <= 0) {
    return "-";
  }
  return formatTimestamp(ts * 1000);
};

const formatRate = (value) => {
  const ratio = Number(value);
  if (!Number.isFinite(ratio) || ratio <= 0) {
    return "0%";
  }
  if (ratio >= 1) {
    return "100%";
  }
  return `${(ratio * 100).toFixed(1)}%`;
};

const normalizeOwners = (owners) => {
  if (!Array.isArray(owners)) {
    return [];
  }
  return owners
    .map((item) => ({
      user_id: String(item?.user_id || "").trim(),
      username: String(item?.username || "").trim(),
    }))
    .filter((item) => item.user_id || item.username);
};

const normalizeChannelAccount = (record) => {
  const channel = String(record?.channel || "").trim();
  const account_id = String(record?.account_id || "").trim();
  const status = String(record?.status || "active").trim() || "active";
  const config = isPlainObject(record?.config) ? record.config : {};
  const runtime = isPlainObject(record?.runtime) ? record.runtime : {};
  const owners = normalizeOwners(record?.owners);
  const ownerUserId = String(record?.owner_user_id || owners[0]?.user_id || "").trim();
  const ownerUsername = String(record?.owner_username || owners[0]?.username || "").trim();
  const ownerCount = toSafeInt(record?.owner_count, owners.length || (ownerUsername ? 1 : 0));
  const inboundCount = toSafeInt(record?.inbound_message_count ?? record?.message_count, 0);
  const outboundTotal = toSafeInt(record?.outbound_total_count, 0);
  return {
    channel,
    account_id,
    status,
    config,
    runtime,
    owners,
    owner_user_id: ownerUserId,
    owner_username: ownerUsername,
    owner_count: ownerCount,
    binding_count: toSafeInt(record?.binding_count, 0),
    session_count: toSafeInt(record?.session_count, 0),
    inbound_message_count: inboundCount,
    message_count: inboundCount,
    outbound_total_count: outboundTotal,
    outbound_sent_count: toSafeInt(record?.outbound_sent_count, 0),
    outbound_failed_count: toSafeInt(record?.outbound_failed_count, 0),
    outbound_retry_count: toSafeInt(record?.outbound_retry_count, 0),
    outbound_pending_count: toSafeInt(record?.outbound_pending_count, 0),
    outbound_retry_attempts: toSafeInt(record?.outbound_retry_attempts, 0),
    outbound_success_rate: toSafeFloat(record?.outbound_success_rate, 0),
    communication_count: toSafeInt(record?.communication_count, inboundCount + outboundTotal),
    has_issue: Boolean(record?.has_issue),
    last_communication_at: toSafeFloat(record?.last_communication_at, 0),
    created_at: toSafeFloat(record?.created_at, 0),
    updated_at: toSafeFloat(record?.updated_at, 0),
  };
};

const buildAccountKey = (account) => `${account?.channel || ""}::${account?.account_id || ""}`;

const buildAccountLabel = (account) => {
  const channel = String(account?.channel || "").trim() || "-";
  const accountId = String(account?.account_id || "").trim() || "-";
  return `${channel} / ${accountId}`;
};

const isAccountActive = (account) => String(account?.status || "").trim().toLowerCase() === "active";

const resolveSelectedChannelAccounts = () => {
  const selectedKeys = state.channels.selectedKeys instanceof Set ? state.channels.selectedKeys : new Set();
  return state.channels.accounts.filter((account) => selectedKeys.has(buildAccountKey(account)));
};

const syncChannelSelectionMeta = () => {
  if (!elements.channelsSelectionMeta) {
    return;
  }
  const visible = state.channels.accounts.length;
  const selected = resolveSelectedChannelAccounts().length;
  elements.channelsSelectionMeta.textContent = t("channels.selection.meta", {
    selected,
    visible,
  });
};

const syncChannelSelectAllControl = () => {
  if (!elements.channelsSelectAllVisible) {
    return;
  }
  const visible = state.channels.accounts.length;
  const selected = resolveSelectedChannelAccounts().length;
  elements.channelsSelectAllVisible.disabled = visible <= 0;
  elements.channelsSelectAllVisible.indeterminate = visible > 0 && selected > 0 && selected < visible;
  elements.channelsSelectAllVisible.checked = visible > 0 && selected === visible;
};

const syncChannelBatchActionButtons = () => {
  const hasSelection = resolveSelectedChannelAccounts().length > 0;
  const hasItems = state.channels.accounts.length > 0;
  const busy = Boolean(state.channels.batchBusy);
  if (elements.channelsBatchEnableBtn) {
    elements.channelsBatchEnableBtn.disabled = busy || !hasSelection;
  }
  if (elements.channelsBatchDisableBtn) {
    elements.channelsBatchDisableBtn.disabled = busy || !hasSelection;
  }
  if (elements.channelsBatchDeleteBtn) {
    elements.channelsBatchDeleteBtn.disabled = busy || !hasSelection;
  }
  if (elements.channelsExportBtn) {
    elements.channelsExportBtn.disabled = busy || !hasItems;
  }
};

const formatFeishuLongConnectionRuntime = (runtime) => {
  if (!isPlainObject(runtime)) {
    return "";
  }
  const status = String(runtime.status || "").trim().toLowerCase() || "unknown";
  const statusKey = FEISHU_LONG_CONNECTION_STATUS_KEYS[status] || FEISHU_LONG_CONNECTION_STATUS_KEYS.unknown;
  const segments = [t(statusKey)];
  if (Number.isFinite(runtime.binding_count)) {
    segments.push(t("channels.runtime.bindingCount", { count: runtime.binding_count }));
  }
  return `${t("channels.runtime.feishu.longConnection")}: ${segments.join(" | ")}`;
};

const formatWeixinLongConnectionRuntime = (runtime) => {
  if (!isPlainObject(runtime)) {
    return "";
  }
  const status = String(runtime.status || "").trim().toLowerCase() || "unknown";
  const statusKey = WEIXIN_LONG_CONNECTION_STATUS_KEYS[status] || WEIXIN_LONG_CONNECTION_STATUS_KEYS.unknown;
  const segments = [t(statusKey)];
  if (Number.isFinite(runtime.binding_count)) {
    segments.push(t("channels.runtime.bindingCount", { count: runtime.binding_count }));
  }
  return `${t("channels.runtime.weixin.longConnection")}: ${segments.join(" | ")}`;
};

const formatChannelRuntime = (account) => {
  const channel = String(account?.channel || "").trim().toLowerCase();
  if (channel === "feishu") {
    return formatFeishuLongConnectionRuntime(account.runtime?.feishu_long_connection);
  }
  if (channel === "weixin") {
    return formatWeixinLongConnectionRuntime(account.runtime?.weixin_long_connection);
  }
  return "";
};

const resolveSelectedChannelAccount = () => state.channels.accounts[state.channels.selectedIndex] || null;

const resolvePrimaryOwnerName = (account) => {
  const direct = String(account?.owner_username || "").trim();
  if (direct) {
    return direct;
  }
  const fromOwners = String(account?.owners?.[0]?.username || account?.owners?.[0]?.user_id || "").trim();
  if (fromOwners) {
    return fromOwners;
  }
  return t("channels.owner.none");
};

const formatOwnerSummary = (account) => {
  const primary = resolvePrimaryOwnerName(account);
  const extra = Math.max(0, toSafeInt(account?.owner_count, 0) - 1);
  if (extra > 0) {
    return `${primary} +${extra}`;
  }
  return primary;
};

const formatOwnerExtra = (account) => {
  const owners = Array.isArray(account?.owners) ? account.owners : [];
  if (!owners.length) {
    const ownerUserId = String(account?.owner_user_id || "").trim();
    return ownerUserId || t("channels.owner.unbound");
  }
  return owners
    .map((item) => String(item.username || item.user_id || "").trim())
    .filter(Boolean)
    .join(", ");
};

const setChannelDeleteButtonState = (disabled) => {
  if (!elements.channelAccountDeleteBtn) {
    return;
  }
  elements.channelAccountDeleteBtn.disabled = Boolean(disabled);
};

const renderChannelAccountDetail = () => {
  if (
    !elements.channelAccountDetailTitle ||
    !elements.channelAccountDetailMeta ||
    !elements.channelAccountDetailConfig ||
    !elements.channelAccountOwner ||
    !elements.channelAccountOwnerExtra ||
    !elements.channelAccountMessageCount ||
    !elements.channelAccountInboundCount ||
    !elements.channelAccountOutboundTotal ||
    !elements.channelAccountOutboundSent ||
    !elements.channelAccountOutboundFailed ||
    !elements.channelAccountOutboundRetry ||
    !elements.channelAccountSuccessRate ||
    !elements.channelAccountSessionCount ||
    !elements.channelAccountBindingCount ||
    !elements.channelAccountLastCommunication ||
    !elements.channelAccountUpdatedAt
  ) {
    return;
  }
  const account = resolveSelectedChannelAccount();
  if (!account) {
    elements.channelAccountDetailTitle.textContent = t("channels.detail.empty");
    elements.channelAccountDetailMeta.textContent = "";
    elements.channelAccountOwner.textContent = t("channels.owner.none");
    elements.channelAccountOwnerExtra.textContent = "";
    elements.channelAccountMessageCount.textContent = "0";
    elements.channelAccountInboundCount.textContent = "0";
    elements.channelAccountOutboundTotal.textContent = "0";
    elements.channelAccountOutboundSent.textContent = "0";
    elements.channelAccountOutboundFailed.textContent = "0";
    elements.channelAccountOutboundRetry.textContent = "0";
    elements.channelAccountSuccessRate.textContent = "0%";
    elements.channelAccountSessionCount.textContent = "0";
    elements.channelAccountBindingCount.textContent = "0";
    elements.channelAccountLastCommunication.textContent = "-";
    elements.channelAccountUpdatedAt.textContent = "-";
    elements.channelAccountDetailConfig.textContent = "{}";
    setChannelDeleteButtonState(true);
    syncChannelSelectionMeta();
    syncChannelSelectAllControl();
    syncChannelBatchActionButtons();
    return;
  }

  const title = [account.channel, account.account_id].filter(Boolean).join(" / ") || t("channels.detail.empty");
  elements.channelAccountDetailTitle.textContent = title;

  const runtimeText = formatChannelRuntime(account);
  const statusText = isAccountActive(account) ? t("channels.status.active") : t("channels.status.disabled");
  const issueText = account.has_issue ? t("channels.status.issue") : "";
  elements.channelAccountDetailMeta.textContent = [statusText, issueText, runtimeText].filter(Boolean).join(" | ");

  elements.channelAccountOwner.textContent = formatOwnerSummary(account);
  elements.channelAccountOwnerExtra.textContent = formatOwnerExtra(account);
  elements.channelAccountMessageCount.textContent = String(toSafeInt(account.communication_count, 0));
  elements.channelAccountInboundCount.textContent = String(toSafeInt(account.inbound_message_count, 0));
  elements.channelAccountOutboundTotal.textContent = String(toSafeInt(account.outbound_total_count, 0));
  elements.channelAccountOutboundSent.textContent = String(toSafeInt(account.outbound_sent_count, 0));
  elements.channelAccountOutboundFailed.textContent = String(toSafeInt(account.outbound_failed_count, 0));
  elements.channelAccountOutboundRetry.textContent = String(toSafeInt(account.outbound_retry_count, 0));
  elements.channelAccountSuccessRate.textContent = formatRate(account.outbound_success_rate);
  elements.channelAccountSessionCount.textContent = String(toSafeInt(account.session_count, 0));
  elements.channelAccountBindingCount.textContent = String(toSafeInt(account.binding_count, 0));
  elements.channelAccountLastCommunication.textContent = formatEpochSeconds(account.last_communication_at);
  elements.channelAccountUpdatedAt.textContent = formatEpochSeconds(account.updated_at);

  try {
    elements.channelAccountDetailConfig.textContent = JSON.stringify(account.config || {}, null, 2);
  } catch (error) {
    elements.channelAccountDetailConfig.textContent = "{}";
  }
  setChannelDeleteButtonState(Boolean(state.channels.batchBusy));
  syncChannelSelectionMeta();
  syncChannelSelectAllControl();
  syncChannelBatchActionButtons();
};

const renderChannelAccountList = () => {
  if (!elements.channelAccountList) {
    return;
  }
  elements.channelAccountList.textContent = "";
  if (!state.channels.accounts.length) {
    elements.channelAccountList.textContent = t("channels.list.empty");
    if (state.channels.selectedKeys instanceof Set) {
      state.channels.selectedKeys.clear();
    }
    renderChannelAccountDetail();
    return;
  }

  state.channels.accounts.forEach((account, index) => {
    const item = document.createElement("div");
    item.className = "list-item channel-item";
    if (index === state.channels.selectedIndex) {
      item.classList.add("active");
    }

    const statusText = isAccountActive(account) ? t("channels.status.active") : t("channels.status.disabled");
    const issueText = account.has_issue ? t("channels.status.issue") : "";
    const runtimeText = formatChannelRuntime(account);
    const ownerText = formatOwnerSummary(account);
    const summarySegments = [
      statusText,
      issueText,
      `${t("channels.field.owner")}: ${ownerText}`,
      `${t("channels.field.messageCount")}: ${toSafeInt(account.communication_count, 0)}`,
    ].filter(Boolean);
    if (runtimeText) {
      summarySegments.push(runtimeText);
    }

    const topRow = document.createElement("div");
    topRow.className = "channel-item-top";

    const checkboxWrap = document.createElement("label");
    checkboxWrap.className = "channel-item-select";
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = state.channels.selectedKeys.has(buildAccountKey(account));
    checkboxWrap.appendChild(checkbox);
    const title = document.createElement("span");
    title.className = "channel-item-title";
    title.textContent = buildAccountLabel(account);
    checkboxWrap.appendChild(title);
    topRow.appendChild(checkboxWrap);
    item.appendChild(topRow);

    const meta = document.createElement("div");
    meta.className = "muted channel-item-meta";
    meta.textContent = summarySegments.join(" | ");
    item.appendChild(meta);

    checkbox.addEventListener("click", (event) => {
      event.stopPropagation();
    });
    checkbox.addEventListener("change", () => {
      const accountKey = buildAccountKey(account);
      if (checkbox.checked) {
        state.channels.selectedKeys.add(accountKey);
      } else {
        state.channels.selectedKeys.delete(accountKey);
      }
      syncChannelSelectionMeta();
      syncChannelSelectAllControl();
      syncChannelBatchActionButtons();
    });

    item.addEventListener("click", () => {
      state.channels.selectedIndex = index;
      renderChannelAccountList();
      renderChannelAccountDetail();
    });
    elements.channelAccountList.appendChild(item);
  });
  renderChannelAccountDetail();
};

const parseResponseErrorMessage = async (response) => {
  const payload = await response.json().catch(() => ({}));
  return (
    payload?.error?.message ||
    payload?.detail?.message ||
    t("common.requestFailed", { status: response.status })
  );
};

const syncFilterControlsFromState = () => {
  if (elements.channelsSearchInput) {
    elements.channelsSearchInput.value = state.channels.filters.keyword || "";
  }
  if (elements.channelsStatusFilter) {
    elements.channelsStatusFilter.value = state.channels.filters.status || "";
  }
  if (elements.channelsActivityFilter) {
    elements.channelsActivityFilter.value = state.channels.filters.activity || "";
  }
  if (elements.channelsIssueOnly) {
    elements.channelsIssueOnly.checked = Boolean(state.channels.filters.issueOnly);
  }
};

const readFiltersFromControls = () => {
  state.channels.filters.keyword = String(elements.channelsSearchInput?.value || "").trim();
  state.channels.filters.status = String(elements.channelsStatusFilter?.value || "").trim();
  state.channels.filters.activity = String(elements.channelsActivityFilter?.value || "").trim();
  state.channels.filters.issueOnly = Boolean(elements.channelsIssueOnly?.checked);
};

const buildChannelAccountQuery = () => {
  const params = new URLSearchParams();
  const filters = state.channels.filters || {};
  if (filters.keyword) {
    params.set("keyword", filters.keyword);
  }
  if (filters.status) {
    params.set("status", filters.status);
  }
  if (filters.issueOnly) {
    params.set("issue_only", "true");
  }
  const activity = String(filters.activity || "").trim();
  if (activity && Object.prototype.hasOwnProperty.call(ACTIVITY_FILTER_WINDOW_SECONDS, activity)) {
    const seconds = ACTIVITY_FILTER_WINDOW_SECONDS[activity];
    const nowSec = Date.now() / 1000;
    params.set("last_active_after", String(nowSec - seconds));
  }
  const encoded = params.toString();
  return encoded ? `?${encoded}` : "";
};

const loadChannelAccounts = async (options = {}) => {
  ensureChannelsState();
  const preserveSelection = options.preserveSelection !== false;
  const updateFiltersFromUi = options.updateFiltersFromUi !== false;
  if (updateFiltersFromUi) {
    readFiltersFromControls();
  }
  const selectedAccount = preserveSelection ? resolveSelectedChannelAccount() : null;
  const selectedKey = selectedAccount ? buildAccountKey(selectedAccount) : "";

  const wunderBase = getWunderBase();
  const query = buildChannelAccountQuery();
  const response = await fetch(`${wunderBase}/admin/channels/accounts${query}`);
  if (!response.ok) {
    throw new Error(await parseResponseErrorMessage(response));
  }
  const result = await response.json();
  const items = Array.isArray(result?.data?.items) ? result.data.items : [];
  state.channels.accounts = items.map(normalizeChannelAccount);
  const visibleKeys = new Set(state.channels.accounts.map((item) => buildAccountKey(item)));
  if (state.channels.selectedKeys instanceof Set) {
    state.channels.selectedKeys = new Set(
      Array.from(state.channels.selectedKeys).filter((key) => visibleKeys.has(key))
    );
  } else {
    state.channels.selectedKeys = new Set();
  }

  if (!state.channels.accounts.length) {
    state.channels.selectedIndex = -1;
  } else if (selectedKey) {
    const nextIndex = state.channels.accounts.findIndex((item) => buildAccountKey(item) === selectedKey);
    state.channels.selectedIndex = nextIndex >= 0 ? nextIndex : 0;
  } else {
    state.channels.selectedIndex = 0;
  }

  renderChannelAccountList();
};

const resetChannelFilters = async () => {
  state.channels.filters.keyword = "";
  state.channels.filters.status = "";
  state.channels.filters.activity = "";
  state.channels.filters.issueOnly = false;
  syncFilterControlsFromState();
  await loadChannelAccounts({ preserveSelection: false, updateFiltersFromUi: false });
};

const fetchDeleteImpact = async (account) => {
  const wunderBase = getWunderBase();
  const endpoint =
    `${wunderBase}/admin/channels/accounts/` +
    `${encodeURIComponent(account.channel)}/${encodeURIComponent(account.account_id)}/impact`;
  const response = await fetch(endpoint);
  if (!response.ok) {
    throw new Error(await parseResponseErrorMessage(response));
  }
  const payload = await response.json();
  return payload?.data || {};
};

const fallbackDeleteImpact = (account) => ({
  bindings: toSafeInt(account?.binding_count, 0),
  user_bindings: toSafeInt(account?.binding_count, 0),
  sessions: toSafeInt(account?.session_count, 0),
  messages: toSafeInt(account?.inbound_message_count, 0),
  outbox_total: toSafeInt(account?.outbound_total_count, 0),
});

const deleteSelectedChannelAccount = async () => {
  if (state.channels.batchBusy) {
    return;
  }
  const account = resolveSelectedChannelAccount();
  if (!account?.channel || !account?.account_id) {
    return;
  }

  let impact = fallbackDeleteImpact(account);
  try {
    impact = {
      ...impact,
      ...(await fetchDeleteImpact(account)),
    };
  } catch (error) {
    notify(t("channels.toast.impactLoadFailed", { message: error.message || "-" }), "warning");
  }

  const confirmed = window.confirm(
    t("channels.confirm.deleteImpact", {
      channel: account.channel,
      account: account.account_id,
      bindings: toSafeInt(impact.bindings, 0),
      userBindings: toSafeInt(impact.user_bindings, 0),
      sessions: toSafeInt(impact.sessions, 0),
      messages: toSafeInt(impact.messages, 0),
      outbox: toSafeInt(impact.outbox_total, 0),
    })
  );
  if (!confirmed) {
    return;
  }

  state.channels.batchBusy = true;
  renderChannelAccountDetail();
  try {
    const wunderBase = getWunderBase();
    const endpoint =
      `${wunderBase}/admin/channels/accounts/` +
      `${encodeURIComponent(account.channel)}/${encodeURIComponent(account.account_id)}`;
    const response = await fetch(endpoint, { method: "DELETE" });
    if (!response.ok) {
      throw new Error(await parseResponseErrorMessage(response));
    }
    await loadChannelAccounts({ preserveSelection: false, updateFiltersFromUi: false });
    notify(t("channels.toast.deleteSuccess"), "success");
  } catch (error) {
    notify(t("channels.toast.deleteFailed", { message: error.message || "-" }), "error");
  } finally {
    state.channels.batchBusy = false;
    renderChannelAccountDetail();
  }
};

const runChannelAccountBatchAction = async (action, accounts) => {
  const wunderBase = getWunderBase();
  const response = await fetch(`${wunderBase}/admin/channels/accounts/batch`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
    },
    body: JSON.stringify({
      action,
      items: accounts.map((account) => ({
        channel: account.channel,
        account_id: account.account_id,
      })),
    }),
  });
  if (!response.ok) {
    throw new Error(await parseResponseErrorMessage(response));
  }
  const payload = await response.json();
  return payload?.data || {};
};

const summarizeBatchDeleteImpact = async (accounts) => {
  const total = {
    bindings: 0,
    user_bindings: 0,
    sessions: 0,
    messages: 0,
    outbox_total: 0,
  };
  let usedFallback = false;

  const impacts = await Promise.all(
    accounts.map(async (account) => {
      try {
        return await fetchDeleteImpact(account);
      } catch (error) {
        usedFallback = true;
        return fallbackDeleteImpact(account);
      }
    })
  );

  impacts.forEach((impact) => {
    total.bindings += toSafeInt(impact?.bindings, 0);
    total.user_bindings += toSafeInt(impact?.user_bindings, 0);
    total.sessions += toSafeInt(impact?.sessions, 0);
    total.messages += toSafeInt(impact?.messages, 0);
    total.outbox_total += toSafeInt(impact?.outbox_total, 0);
  });
  return {
    ...total,
    usedFallback,
  };
};

const notifyBatchSummary = (summary) => {
  notify(
    t("channels.toast.batchSummary", {
      success: toSafeInt(summary?.success, 0),
      skipped: toSafeInt(summary?.skipped, 0),
      failed: toSafeInt(summary?.failed, 0),
    }),
    toSafeInt(summary?.failed, 0) > 0 ? "warning" : "success"
  );
};

const runBatchAccountStatusAction = async (action) => {
  if (state.channels.batchBusy) {
    return;
  }
  const selectedAccounts = resolveSelectedChannelAccounts();
  if (!selectedAccounts.length) {
    notify(t("channels.toast.noSelection"), "warning");
    return;
  }
  state.channels.batchBusy = true;
  renderChannelAccountDetail();
  try {
    const summary = await runChannelAccountBatchAction(action, selectedAccounts);
    notifyBatchSummary(summary);
    await loadChannelAccounts({ preserveSelection: false, updateFiltersFromUi: false });
  } catch (error) {
    notify(t("channels.toast.batchFailed", { message: error.message || "-" }), "error");
  } finally {
    state.channels.batchBusy = false;
    renderChannelAccountDetail();
  }
};

const runBatchAccountDelete = async () => {
  if (state.channels.batchBusy) {
    return;
  }
  const selectedAccounts = resolveSelectedChannelAccounts();
  if (!selectedAccounts.length) {
    notify(t("channels.toast.noSelection"), "warning");
    return;
  }
  let impactSummary = null;
  try {
    impactSummary = await summarizeBatchDeleteImpact(selectedAccounts);
    if (impactSummary.usedFallback) {
      notify(t("channels.toast.batchImpactFallback"), "warning");
    }
  } catch (error) {
    notify(t("channels.toast.impactLoadFailed", { message: error.message || "-" }), "warning");
  }
  const confirmed = window.confirm(
    t("channels.confirm.batchDeleteImpact", {
      count: selectedAccounts.length,
      bindings: toSafeInt(impactSummary?.bindings, 0),
      userBindings: toSafeInt(impactSummary?.user_bindings, 0),
      sessions: toSafeInt(impactSummary?.sessions, 0),
      messages: toSafeInt(impactSummary?.messages, 0),
      outbox: toSafeInt(impactSummary?.outbox_total, 0),
    })
  );
  if (!confirmed) {
    return;
  }
  state.channels.batchBusy = true;
  renderChannelAccountDetail();
  try {
    const summary = await runChannelAccountBatchAction("delete", selectedAccounts);
    notifyBatchSummary(summary);
    await loadChannelAccounts({ preserveSelection: false, updateFiltersFromUi: false });
  } catch (error) {
    notify(t("channels.toast.batchFailed", { message: error.message || "-" }), "error");
  } finally {
    state.channels.batchBusy = false;
    renderChannelAccountDetail();
  }
};

const csvEscapeCell = (value) => {
  const text = String(value ?? "");
  if (/[",\r\n]/.test(text)) {
    return `"${text.replace(/"/g, "\"\"")}"`;
  }
  return text;
};

const downloadBlob = (blob, filename) => {
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
};

const exportChannelAccountsCsv = () => {
  if (state.channels.batchBusy) {
    return;
  }
  const selectedAccounts = resolveSelectedChannelAccounts();
  const rows = selectedAccounts.length ? selectedAccounts : state.channels.accounts;
  if (!rows.length) {
    notify(t("channels.toast.noDataToExport"), "warning");
    return;
  }
  const header = [
    "channel",
    "account_id",
    "status",
    "owner_username",
    "owner_count",
    "binding_count",
    "session_count",
    "communication_count",
    "inbound_message_count",
    "outbound_total_count",
    "outbound_sent_count",
    "outbound_failed_count",
    "outbound_retry_count",
    "outbound_pending_count",
    "outbound_success_rate",
    "last_communication_at",
    "updated_at",
    "has_issue",
  ];
  const csvRows = rows.map((account) => [
    account.channel,
    account.account_id,
    account.status,
    resolvePrimaryOwnerName(account),
    toSafeInt(account.owner_count, 0),
    toSafeInt(account.binding_count, 0),
    toSafeInt(account.session_count, 0),
    toSafeInt(account.communication_count, 0),
    toSafeInt(account.inbound_message_count, 0),
    toSafeInt(account.outbound_total_count, 0),
    toSafeInt(account.outbound_sent_count, 0),
    toSafeInt(account.outbound_failed_count, 0),
    toSafeInt(account.outbound_retry_count, 0),
    toSafeInt(account.outbound_pending_count, 0),
    Number(account.outbound_success_rate || 0).toFixed(4),
    account.last_communication_at || "",
    account.updated_at || "",
    account.has_issue ? "true" : "false",
  ]);
  const content = [
    header.map(csvEscapeCell).join(","),
    ...csvRows.map((row) => row.map(csvEscapeCell).join(",")),
  ].join("\r\n");
  const stamp = new Date().toISOString().replace(/[:.]/g, "-");
  const filename = `channel_accounts_${stamp}.csv`;
  const blob = new Blob(["\uFEFF", content], { type: "text/csv;charset=utf-8" });
  downloadBlob(blob, filename);
  notify(t("channels.toast.exportSuccess", { count: rows.length }), "success");
};

const bindFilterEvents = () => {
  if (elements.channelsFilterApplyBtn?.dataset.bound === "1") {
    return;
  }
  if (elements.channelsFilterApplyBtn) {
    elements.channelsFilterApplyBtn.dataset.bound = "1";
    elements.channelsFilterApplyBtn.addEventListener("click", async () => {
      try {
        await loadChannelAccounts({ preserveSelection: false, updateFiltersFromUi: true });
      } catch (error) {
        notify(t("channels.toast.loadFailed", { message: error.message || "-" }), "error");
      }
    });
  }
  if (elements.channelsFilterResetBtn) {
    elements.channelsFilterResetBtn.addEventListener("click", async () => {
      try {
        await resetChannelFilters();
      } catch (error) {
        notify(t("channels.toast.loadFailed", { message: error.message || "-" }), "error");
      }
    });
  }
  if (elements.channelsSearchInput) {
    elements.channelsSearchInput.addEventListener("keydown", async (event) => {
      if (event.key !== "Enter") {
        return;
      }
      event.preventDefault();
      try {
        await loadChannelAccounts({ preserveSelection: false, updateFiltersFromUi: true });
      } catch (error) {
        notify(t("channels.toast.loadFailed", { message: error.message || "-" }), "error");
      }
    });
  }
};

const bindBatchEvents = () => {
  if (elements.channelsSelectAllVisible && elements.channelsSelectAllVisible.dataset.bound !== "1") {
    elements.channelsSelectAllVisible.dataset.bound = "1";
    elements.channelsSelectAllVisible.addEventListener("change", () => {
      if (!state.channels.accounts.length) {
        return;
      }
      if (elements.channelsSelectAllVisible.checked) {
        state.channels.accounts.forEach((account) => {
          state.channels.selectedKeys.add(buildAccountKey(account));
        });
      } else {
        state.channels.accounts.forEach((account) => {
          state.channels.selectedKeys.delete(buildAccountKey(account));
        });
      }
      renderChannelAccountList();
    });
  }
  if (elements.channelsBatchEnableBtn && elements.channelsBatchEnableBtn.dataset.bound !== "1") {
    elements.channelsBatchEnableBtn.dataset.bound = "1";
    elements.channelsBatchEnableBtn.addEventListener("click", () => {
      runBatchAccountStatusAction("enable");
    });
  }
  if (elements.channelsBatchDisableBtn && elements.channelsBatchDisableBtn.dataset.bound !== "1") {
    elements.channelsBatchDisableBtn.dataset.bound = "1";
    elements.channelsBatchDisableBtn.addEventListener("click", () => {
      runBatchAccountStatusAction("disable");
    });
  }
  if (elements.channelsBatchDeleteBtn && elements.channelsBatchDeleteBtn.dataset.bound !== "1") {
    elements.channelsBatchDeleteBtn.dataset.bound = "1";
    elements.channelsBatchDeleteBtn.addEventListener("click", () => {
      runBatchAccountDelete();
    });
  }
  if (elements.channelsExportBtn && elements.channelsExportBtn.dataset.bound !== "1") {
    elements.channelsExportBtn.dataset.bound = "1";
    elements.channelsExportBtn.addEventListener("click", () => {
      exportChannelAccountsCsv();
    });
  }
};

export const initChannelsPanel = () => {
  ensureChannelsState();
  if (
    !elements.channelsRefreshBtn ||
    !elements.channelAccountDeleteBtn ||
    !elements.channelsSearchInput ||
    !elements.channelsStatusFilter ||
    !elements.channelsActivityFilter ||
    !elements.channelsIssueOnly ||
    !elements.channelsSelectAllVisible ||
    !elements.channelsBatchEnableBtn ||
    !elements.channelsBatchDisableBtn ||
    !elements.channelsBatchDeleteBtn ||
    !elements.channelsExportBtn ||
    !elements.channelsSelectionMeta
  ) {
    return;
  }
  if (elements.channelsRefreshBtn.dataset.bound === "1") {
    return;
  }
  elements.channelsRefreshBtn.dataset.bound = "1";

  syncFilterControlsFromState();
  bindFilterEvents();
  bindBatchEvents();

  elements.channelsRefreshBtn.addEventListener("click", async () => {
    try {
      await loadChannelAccounts({ updateFiltersFromUi: true });
      notify(t("channels.toast.refreshSuccess"), "success");
    } catch (error) {
      notify(t("channels.toast.loadFailed", { message: error.message || "-" }), "error");
    }
  });

  elements.channelAccountDeleteBtn.addEventListener("click", () => {
    deleteSelectedChannelAccount();
  });

  renderChannelAccountDetail();
};

export { loadChannelAccounts };
