import { elements } from "./elements.js?v=20260214-01";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { isPlainObject } from "./utils.js?v=20251229-02";
import { notify } from "./notify.js";
import { t } from "./i18n.js?v=20260214-01";

const FEISHU_LONG_CONNECTION_STATUS_KEYS = {
  running: "channels.runtime.feishu.status.running",
  waiting_binding: "channels.runtime.feishu.status.waitingBinding",
  missing_credentials: "channels.runtime.feishu.status.missingCredentials",
  disabled: "channels.runtime.feishu.status.disabled",
  account_inactive: "channels.runtime.feishu.status.accountInactive",
  not_configured: "channels.runtime.feishu.status.notConfigured",
  unknown: "channels.runtime.feishu.status.unknown",
};

const normalizeChannelAccount = (record) => {
  const channel = String(record?.channel || "").trim();
  const account_id = String(record?.account_id || "").trim();
  const status = String(record?.status || "active").trim() || "active";
  const config = isPlainObject(record?.config) ? record.config : {};
  const runtime = isPlainObject(record?.runtime) ? record.runtime : {};
  return {
    channel,
    account_id,
    status,
    config,
    runtime,
    created_at: record?.created_at,
    updated_at: record?.updated_at,
  };
};

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

const renderChannelAccountDetail = () => {
  if (!elements.channelAccountDetailTitle || !elements.channelAccountDetailMeta || !elements.channelAccountDetailConfig) {
    return;
  }
  const account = state.channels.accounts[state.channels.selectedIndex];
  if (!account) {
    elements.channelAccountDetailTitle.textContent = t("channels.detail.empty");
    elements.channelAccountDetailMeta.textContent = "";
    elements.channelAccountDetailConfig.textContent = "{}";
    return;
  }

  const title = [account.channel, account.account_id].filter(Boolean).join(" / ") || t("channels.detail.empty");
  elements.channelAccountDetailTitle.textContent = title;

  const runtimeText =
    String(account.channel || "").trim().toLowerCase() === "feishu"
      ? formatFeishuLongConnectionRuntime(account.runtime?.feishu_long_connection)
      : "";
  const statusText = isAccountActive(account)
    ? t("channels.status.active")
    : t("channels.status.disabled");
  elements.channelAccountDetailMeta.textContent = [statusText, runtimeText].filter(Boolean).join(" | ");

  try {
    elements.channelAccountDetailConfig.textContent = JSON.stringify(account.config || {}, null, 2);
  } catch (error) {
    elements.channelAccountDetailConfig.textContent = "{}";
  }
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
    const statusText = isAccountActive(account)
      ? t("channels.status.active")
      : t("channels.status.disabled");
    const runtimeText =
      String(account.channel || "").trim().toLowerCase() === "feishu"
        ? formatFeishuLongConnectionRuntime(account.runtime?.feishu_long_connection)
        : "";
    item.innerHTML = `
      <div>${account.channel || "-"} / ${account.account_id || "-"}</div>
      <div class="muted">${statusText}${runtimeText ? ` | ${runtimeText}` : ""}</div>
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

const loadChannelAccounts = async () => {
  const wunderBase = getWunderBase();
  const response = await fetch(`${wunderBase}/admin/channels/accounts`);
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const result = await response.json();
  const items = Array.isArray(result?.data?.items) ? result.data.items : [];
  state.channels.accounts = items.map(normalizeChannelAccount);
  state.channels.selectedIndex = state.channels.accounts.length ? 0 : -1;
  renderChannelAccountList();
};

export const initChannelsPanel = () => {
  if (elements.channelsRefreshBtn) {
    elements.channelsRefreshBtn.addEventListener("click", async () => {
      try {
        await loadChannelAccounts();
        notify(t("channels.toast.refreshSuccess"), "success");
      } catch (error) {
        notify(t("channels.toast.loadFailed", { message: error.message || "-" }), "error");
      }
    });
  }
};

export { loadChannelAccounts };
