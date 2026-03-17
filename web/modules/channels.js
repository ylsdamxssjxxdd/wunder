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

const toSafeInt = (value, fallback = 0) => {
  const parsed = Math.floor(Number(value));
  if (!Number.isFinite(parsed)) {
    return fallback;
  }
  return Math.max(0, parsed);
};

const formatEpochSeconds = (value) => {
  const ts = Number(value);
  if (!Number.isFinite(ts) || ts <= 0) {
    return "-";
  }
  return formatTimestamp(ts * 1000);
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
  const messageCount = toSafeInt(record?.message_count ?? record?.communication_count, 0);
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
    message_count: messageCount,
    communication_count: toSafeInt(record?.communication_count, messageCount),
    last_communication_at: Number(record?.last_communication_at) || 0,
    created_at: Number(record?.created_at) || 0,
    updated_at: Number(record?.updated_at) || 0,
  };
};

const buildAccountKey = (account) => `${account?.channel || ""}::${account?.account_id || ""}`;

const isAccountActive = (account) => String(account?.status || "").trim().toLowerCase() === "active";

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
    elements.channelAccountSessionCount.textContent = "0";
    elements.channelAccountBindingCount.textContent = "0";
    elements.channelAccountLastCommunication.textContent = "-";
    elements.channelAccountUpdatedAt.textContent = "-";
    elements.channelAccountDetailConfig.textContent = "{}";
    setChannelDeleteButtonState(true);
    return;
  }

  const title = [account.channel, account.account_id].filter(Boolean).join(" / ") || t("channels.detail.empty");
  elements.channelAccountDetailTitle.textContent = title;

  const runtimeText =
    String(account.channel || "").trim().toLowerCase() === "feishu"
      ? formatFeishuLongConnectionRuntime(account.runtime?.feishu_long_connection)
      : "";
  const statusText = isAccountActive(account) ? t("channels.status.active") : t("channels.status.disabled");
  elements.channelAccountDetailMeta.textContent = [statusText, runtimeText].filter(Boolean).join(" | ");

  elements.channelAccountOwner.textContent = formatOwnerSummary(account);
  elements.channelAccountOwnerExtra.textContent = formatOwnerExtra(account);
  elements.channelAccountMessageCount.textContent = String(toSafeInt(account.communication_count, 0));
  elements.channelAccountSessionCount.textContent = String(toSafeInt(account.session_count, 0));
  elements.channelAccountBindingCount.textContent = String(toSafeInt(account.binding_count, 0));
  elements.channelAccountLastCommunication.textContent = formatEpochSeconds(account.last_communication_at);
  elements.channelAccountUpdatedAt.textContent = formatEpochSeconds(account.updated_at);

  try {
    elements.channelAccountDetailConfig.textContent = JSON.stringify(account.config || {}, null, 2);
  } catch (error) {
    elements.channelAccountDetailConfig.textContent = "{}";
  }
  setChannelDeleteButtonState(false);
};

const renderChannelAccountList = () => {
  if (!elements.channelAccountList) {
    return;
  }
  elements.channelAccountList.textContent = "";
  if (!state.channels.accounts.length) {
    elements.channelAccountList.textContent = t("channels.list.empty");
    renderChannelAccountDetail();
    return;
  }

  state.channels.accounts.forEach((account, index) => {
    const item = document.createElement("button");
    item.type = "button";
    item.className = "list-item channel-item";
    if (index === state.channels.selectedIndex) {
      item.classList.add("active");
    }

    const statusText = isAccountActive(account) ? t("channels.status.active") : t("channels.status.disabled");
    const runtimeText =
      String(account.channel || "").trim().toLowerCase() === "feishu"
        ? formatFeishuLongConnectionRuntime(account.runtime?.feishu_long_connection)
        : "";
    const ownerText = formatOwnerSummary(account);
    const summarySegments = [
      statusText,
      `${t("channels.field.owner")}: ${ownerText}`,
      `${t("channels.field.messageCount")}: ${toSafeInt(account.communication_count, 0)}`,
    ];
    if (runtimeText) {
      summarySegments.push(runtimeText);
    }

    item.innerHTML = `
      <div>${account.channel || "-"} / ${account.account_id || "-"}</div>
      <div class="muted">${summarySegments.join(" | ")}</div>
    `;
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

const loadChannelAccounts = async (options = {}) => {
  const preserveSelection = options.preserveSelection !== false;
  const selectedAccount = preserveSelection ? resolveSelectedChannelAccount() : null;
  const selectedKey = selectedAccount ? buildAccountKey(selectedAccount) : "";

  const wunderBase = getWunderBase();
  const response = await fetch(`${wunderBase}/admin/channels/accounts`);
  if (!response.ok) {
    throw new Error(await parseResponseErrorMessage(response));
  }
  const result = await response.json();
  const items = Array.isArray(result?.data?.items) ? result.data.items : [];
  state.channels.accounts = items.map(normalizeChannelAccount);

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

const deleteSelectedChannelAccount = async () => {
  const account = resolveSelectedChannelAccount();
  if (!account?.channel || !account?.account_id) {
    return;
  }

  const confirmed = window.confirm(
    t("channels.confirm.delete", { channel: account.channel, account: account.account_id })
  );
  if (!confirmed) {
    return;
  }

  setChannelDeleteButtonState(true);
  try {
    const wunderBase = getWunderBase();
    const endpoint =
      `${wunderBase}/admin/channels/accounts/` +
      `${encodeURIComponent(account.channel)}/${encodeURIComponent(account.account_id)}`;
    const response = await fetch(endpoint, { method: "DELETE" });
    if (!response.ok) {
      throw new Error(await parseResponseErrorMessage(response));
    }
    await loadChannelAccounts({ preserveSelection: false });
    notify(t("channels.toast.deleteSuccess"), "success");
  } catch (error) {
    notify(t("channels.toast.deleteFailed", { message: error.message || "-" }), "error");
  } finally {
    renderChannelAccountDetail();
  }
};

export const initChannelsPanel = () => {
  if (!elements.channelsRefreshBtn || !elements.channelAccountDeleteBtn) {
    return;
  }
  if (elements.channelsRefreshBtn.dataset.bound === "1") {
    return;
  }
  elements.channelsRefreshBtn.dataset.bound = "1";

  elements.channelsRefreshBtn.addEventListener("click", async () => {
    try {
      await loadChannelAccounts();
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
