import { elements } from "./elements.js?v=20260118-07";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { isPlainObject } from "./utils.js?v=20251229-02";
import { notify } from "./notify.js";
import { appendLog } from "./log.js?v=20260108-02";
import { t } from "./i18n.js?v=20260118-07";

const SUPPORTED_CHANNELS = [
  { value: "", labelKey: "channels.support.select", placeholder: true },
  { value: "whatsapp", labelKey: "channels.support.whatsapp" },
  { value: "feishu", labelKey: "channels.support.feishu" },
  { value: "qqbot", labelKey: "channels.support.qqbot" },
  { value: "wechat", labelKey: "channels.support.wechat" },
  { value: "telegram", labelKey: "channels.support.telegram" },
];

const CHANNEL_ACCOUNT_PLACEHOLDER = {
  whatsapp: "phone_number_id",
  feishu: "cli_xxx",
  qqbot: "app_id",
  wechat: "wechat_account",
  telegram: "bot_id",
};

const buildChannelConfigTemplate = (channel) => {
  switch (channel) {
    case "whatsapp":
      return {
        inbound_token: "replace-with-inbound-token",
        whatsapp_cloud: {
          phone_number_id: "",
          access_token: "",
          verify_token: "",
          app_secret: "",
          api_version: "v20.0",
        },
      };
    case "feishu":
      return {
        inbound_token: "replace-with-inbound-token",
        feishu: {
          app_id: "",
          app_secret: "",
          verification_token: "",
          encrypt_key: "",
          domain: "open.feishu.cn",
          receive_id_type: "chat_id",
          long_connection_enabled: true,
        },
      };
    case "qqbot":
      return {
        inbound_token: "replace-with-inbound-token",
        qqbot: {
          app_id: "",
          client_secret: "",
          markdown_support: false,
        },
      };
    case "wechat":
      return {
        inbound_token: "replace-with-inbound-token",
        outbound_url: "",
      };
    case "telegram":
      return {
        inbound_token: "replace-with-inbound-token",
        outbound_url: "",
      };
    default:
      return {};
  }
};

const updateAccountIdPlaceholder = (channel) => {
  const normalizedChannel = String(channel || "").trim().toLowerCase();
  const placeholder = CHANNEL_ACCOUNT_PLACEHOLDER[normalizedChannel] || "account_id";
  elements.channelAccountId.placeholder = placeholder;
};

const applyChannelTemplateIfNeeded = () => {
  if (state.channelAccountModal.index !== null) {
    return;
  }
  const channel = String(elements.channelAccountChannel.value || "")
    .trim()
    .toLowerCase();
  updateAccountIdPlaceholder(channel);
  if (!channel) {
    elements.channelAccountConfig.value = "{}";
    return;
  }
  const template = buildChannelConfigTemplate(channel);
  elements.channelAccountConfig.value = JSON.stringify(template, null, 2);
  elements.channelAccountConfigError.textContent = "";
};

const renderChannelOptions = (selectedChannel, includeUnknown = false) => {
  const select = elements.channelAccountChannel;
  if (!select) {
    return;
  }
  const normalizedSelected = String(selectedChannel || "").trim().toLowerCase();
  const options = [...SUPPORTED_CHANNELS];
  if (
    includeUnknown &&
    normalizedSelected &&
    !options.some((item) => item.value === normalizedSelected)
  ) {
    options.push({ value: normalizedSelected, label: normalizedSelected });
  }
  select.textContent = "";
  const available = new Set();
  options.forEach((item) => {
    const option = document.createElement("option");
    option.value = item.value;
    option.textContent = item.labelKey ? t(item.labelKey) : item.label || item.value;
    option.disabled = Boolean(item.placeholder);
    select.appendChild(option);
    available.add(item.value);
  });
  if (normalizedSelected && available.has(normalizedSelected)) {
    select.value = normalizedSelected;
  } else {
    select.value = "";
  }
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

const FEISHU_LONG_CONNECTION_STATUS_KEYS = {
  running: "channels.runtime.feishu.status.running",
  waiting_binding: "channels.runtime.feishu.status.waitingBinding",
  missing_credentials: "channels.runtime.feishu.status.missingCredentials",
  disabled: "channels.runtime.feishu.status.disabled",
  account_inactive: "channels.runtime.feishu.status.accountInactive",
  not_configured: "channels.runtime.feishu.status.notConfigured",
  unknown: "channels.runtime.feishu.status.unknown",
};

const isAccountActive = (record) => record?.status?.trim().toLowerCase() === "active";

const resolveFeishuLongConnectionRuntime = (account) => {
  if (String(account?.channel || "").trim().toLowerCase() !== "feishu") {
    return null;
  }
  const runtime = isPlainObject(account?.runtime?.feishu_long_connection)
    ? account.runtime.feishu_long_connection
    : null;
  if (!runtime) {
    return null;
  }
  const normalizedStatus = String(runtime.status || "unknown")
    .trim()
    .toLowerCase();
  const status = FEISHU_LONG_CONNECTION_STATUS_KEYS[normalizedStatus]
    ? normalizedStatus
    : "unknown";
  const rawBindingCount = Number(runtime.binding_count);
  const bindingCount = Number.isFinite(rawBindingCount) ? rawBindingCount : null;
  return {
    status,
    statusLabel: t(FEISHU_LONG_CONNECTION_STATUS_KEYS[status]),
    bindingCount,
  };
};

const formatFeishuLongConnectionRuntime = (account, withLabel = false) => {
  const runtime = resolveFeishuLongConnectionRuntime(account);
  if (!runtime) {
    return "";
  }
  const segments = [runtime.statusLabel];
  if (runtime.bindingCount !== null) {
    segments.push(t("channels.runtime.bindingCount", { count: runtime.bindingCount }));
  }
  if (withLabel) {
    return `${t("channels.runtime.feishu.longConnection")}: ${segments.join(" | ")}`;
  }
  return segments.join(" | ");
};

const renderChannelAccountDetail = () => {
  const account = state.channels.accounts[state.channels.selectedIndex];
  if (!account) {
    elements.channelAccountDetailTitle.textContent = t("channels.detail.empty");
    elements.channelAccountDetailMeta.textContent = "";
    if (elements.channelAccountDetailConfig) {
      elements.channelAccountDetailConfig.textContent = "";
    }
    elements.channelAccountEnabled.checked = false;
    elements.channelAccountEnabled.disabled = true;
    elements.channelAccountEditBtn.disabled = true;
    elements.channelAccountDeleteBtn.disabled = true;
    return;
  }
  const label = `${account.channel} / ${account.account_id}`;
  elements.channelAccountDetailTitle.textContent = label;
  const meta = [
    isAccountActive(account) ? t("channels.status.active") : t("channels.status.disabled"),
  ];
  const longConnectionText = formatFeishuLongConnectionRuntime(account, true);
  if (longConnectionText) {
    meta.push(longConnectionText);
  }
  if (account.updated_at) {
    meta.push(`updated: ${account.updated_at}`);
  }
  elements.channelAccountDetailMeta.textContent = meta.join(" | ");
  elements.channelAccountEnabled.checked = isAccountActive(account);
  elements.channelAccountEnabled.disabled = false;
  elements.channelAccountEditBtn.disabled = false;
  elements.channelAccountDeleteBtn.disabled = false;
  if (elements.channelAccountDetailConfig) {
    const json = JSON.stringify(account.config || {}, null, 2);
    elements.channelAccountDetailConfig.textContent = json;
  }
};

const renderChannelAccountList = () => {
  elements.channelAccountList.textContent = "";
  if (!state.channels.accounts.length) {
    elements.channelAccountList.textContent = t("channels.list.empty");
    renderChannelAccountDetail();
    return;
  }
  state.channels.accounts.forEach((account, index) => {
    const item = document.createElement("button");
    item.type = "button";
    item.className = "list-item";
    if (index === state.channels.selectedIndex) {
      item.classList.add("active");
    }
    const title = `${account.channel} / ${account.account_id}`;
    const statusText = isAccountActive(account)
      ? t("channels.status.active")
      : t("channels.status.disabled");
    const longConnectionText = formatFeishuLongConnectionRuntime(account, false);
    const summaryText = longConnectionText ? `${statusText} | ${longConnectionText}` : statusText;
    item.innerHTML = `<div>${title}</div><small>${summaryText}</small>`;
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
  renderChannelAccountDetail();
};

const parseConfigInput = () => {
  const raw = String(elements.channelAccountConfig.value || "").trim();
  if (!raw) {
    return { ok: true, value: {} };
  }
  try {
    const parsed = JSON.parse(raw);
    if (!isPlainObject(parsed)) {
      return { ok: false, error: t("channels.error.configObject") };
    }
    return { ok: true, value: parsed };
  } catch (error) {
    return { ok: false, error: t("channels.error.configInvalid") };
  }
};

const openChannelAccountModal = (index) => {
  state.channelAccountModal.index = index;
  const account = index !== null ? state.channels.accounts[index] : null;
  elements.channelAccountModalTitle.textContent =
    index === null ? t("channels.modal.addTitle") : t("channels.modal.editTitle");
  renderChannelOptions(account?.channel || "", index !== null);
  elements.channelAccountId.value = account?.account_id || "";
  elements.channelAccountStatus.value = account?.status || "active";
  elements.channelAccountChannel.disabled = index !== null;
  elements.channelAccountId.disabled = index !== null;
  updateAccountIdPlaceholder(account?.channel || "");
  elements.channelAccountConfig.value = account
    ? JSON.stringify(account.config || {}, null, 2)
    : "{}";
  if (index === null) {
    applyChannelTemplateIfNeeded();
  }
  elements.channelAccountConfigError.textContent = "";
  elements.channelAccountModal.classList.add("active");
};

const closeChannelAccountModal = () => {
  elements.channelAccountModal.classList.remove("active");
};

const saveChannelAccount = async () => {
  const channel = String(elements.channelAccountChannel.value || "")
    .trim()
    .toLowerCase();
  const account_id = String(elements.channelAccountId.value || "").trim();
  if (!channel || !account_id) {
    notify(t("channels.error.required"), "warn");
    return;
  }
  const configResult = parseConfigInput();
  if (!configResult.ok) {
    elements.channelAccountConfigError.textContent = configResult.error;
    return;
  }
  const payload = {
    channel,
    account_id,
    status: String(elements.channelAccountStatus.value || "active"),
    config: configResult.value,
  };
  const wunderBase = getWunderBase();
  const response = await fetch(`${wunderBase}/admin/channels/accounts`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const result = await response.json();
  const record = normalizeChannelAccount(result?.data || payload);
  const existingIndex = state.channels.accounts.findIndex(
    (item) => item.channel === record.channel && item.account_id === record.account_id
  );
  if (existingIndex >= 0) {
    state.channels.accounts[existingIndex] = record;
    state.channels.selectedIndex = existingIndex;
  } else {
    state.channels.accounts.push(record);
    state.channels.selectedIndex = state.channels.accounts.length - 1;
  }
  renderChannelAccountList();
  renderChannelAccountDetail();
};

const deleteChannelAccount = async () => {
  const account = state.channels.accounts[state.channels.selectedIndex];
  if (!account) {
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/channels/accounts/${encodeURIComponent(
    account.channel
  )}/${encodeURIComponent(account.account_id)}`;
  const response = await fetch(endpoint, { method: "DELETE" });
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  state.channels.accounts.splice(state.channels.selectedIndex, 1);
  state.channels.selectedIndex = state.channels.accounts.length ? 0 : -1;
  renderChannelAccountList();
  renderChannelAccountDetail();
};

const toggleChannelAccountEnabled = async () => {
  const account = state.channels.accounts[state.channels.selectedIndex];
  if (!account) {
    return;
  }
  const nextEnabled = elements.channelAccountEnabled.checked;
  const nextStatus = nextEnabled ? "active" : "disabled";
  if (account.status === nextStatus) {
    return;
  }
  const payload = {
    channel: account.channel,
    account_id: account.account_id,
    status: nextStatus,
    config: account.config || {},
  };
  const wunderBase = getWunderBase();
  const response = await fetch(`${wunderBase}/admin/channels/accounts`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const result = await response.json().catch(() => null);
  const normalized = normalizeChannelAccount(result?.data || payload);
  state.channels.accounts[state.channels.selectedIndex] = {
    ...account,
    ...normalized,
  };
  renderChannelAccountList();
  renderChannelAccountDetail();
};

export const initChannelsPanel = () => {
  elements.channelsRefreshBtn.addEventListener("click", async () => {
    try {
      await loadChannelAccounts();
      notify(t("channels.toast.refreshSuccess"), "success");
    } catch (error) {
      notify(t("channels.toast.loadFailed", { message: error.message || "-" }), "error");
    }
  });
  elements.channelAccountAddBtn.addEventListener("click", () => openChannelAccountModal(null));
  elements.channelAccountEditBtn.addEventListener("click", () => {
    if (state.channels.selectedIndex < 0) {
      return;
    }
    openChannelAccountModal(state.channels.selectedIndex);
  });
  elements.channelAccountDeleteBtn.addEventListener("click", async () => {
    if (state.channels.selectedIndex < 0) {
      return;
    }
    try {
      await deleteChannelAccount();
      notify(t("channels.toast.deleteSuccess"), "success");
      appendLog(t("channels.toast.deleteSuccess"));
    } catch (error) {
      notify(t("channels.toast.deleteFailed", { message: error.message || "-" }), "error");
    }
  });
  elements.channelAccountEnabled.addEventListener("change", async () => {
    try {
      await toggleChannelAccountEnabled();
      notify(t("channels.toast.saveSuccess"), "success");
    } catch (error) {
      notify(t("channels.toast.saveFailed", { message: error.message || "-" }), "error");
      renderChannelAccountDetail();
    }
  });
  elements.channelAccountModalSave.addEventListener("click", async () => {
    try {
      await saveChannelAccount();
      notify(t("channels.toast.saveSuccess"), "success");
      appendLog(t("channels.toast.saveSuccess"));
      closeChannelAccountModal();
    } catch (error) {
      notify(t("channels.toast.saveFailed", { message: error.message || "-" }), "error");
    }
  });
  elements.channelAccountModalCancel.addEventListener("click", closeChannelAccountModal);
  elements.channelAccountModalClose.addEventListener("click", closeChannelAccountModal);
  elements.channelAccountModal.addEventListener("click", (event) => {
    if (event.target === elements.channelAccountModal) {
      closeChannelAccountModal();
    }
  });
  elements.channelAccountConfig.addEventListener("input", () => {
    elements.channelAccountConfigError.textContent = "";
  });
  elements.channelAccountChannel.addEventListener("change", () => {
    applyChannelTemplateIfNeeded();
  });
};

export { loadChannelAccounts };
