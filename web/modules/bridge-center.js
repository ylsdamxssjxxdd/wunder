import { elements } from "./elements.js?v=20260324-04";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { formatTimestamp } from "./utils.js?v=20251229-02";
import { notify } from "./notify.js";

const WEIXIN_CHANNEL = "weixin";
const DEFAULT_WEIXIN_API_BASE = "https://ilinkai.weixin.qq.com";
const DEFAULT_WEIXIN_BOT_TYPE = "3";
const BRIDGE_RUNTIME_LOG_POLL_INTERVAL_MS = 5000;
const USER_ONLY_CHANNELS = new Set(["wechat", "wechat_mp", WEIXIN_CHANNEL]);

const CHANNEL_FORM_SCHEMAS = {
  feishu: {
    mode: "feishu",
    fields: [
      { key: "app_id", label: "App ID", required: true },
      { key: "app_secret", label: "App Secret", type: "password", required: true },
      { key: "domain", label: "Domain", defaultValue: "open.feishu.cn" },
    ],
  },
  wechat: {
    mode: "wechat",
    fields: [
      { key: "corp_id", label: "Corp ID", required: true },
      { key: "agent_id", label: "Agent ID", required: true },
      { key: "secret", label: "Secret", type: "password", required: true },
      { key: "token", label: "Token", type: "password" },
      { key: "encoding_aes_key", label: "Encoding AES Key", type: "password" },
      { key: "domain", label: "Domain", defaultValue: "qyapi.weixin.qq.com" },
    ],
  },
  wechat_mp: {
    mode: "wechat_mp",
    fields: [
      { key: "app_id", label: "App ID", required: true },
      { key: "app_secret", label: "App Secret", type: "password", required: true },
      { key: "token", label: "Token", type: "password" },
      { key: "encoding_aes_key", label: "Encoding AES Key", type: "password" },
      { key: "original_id", label: "Original ID" },
      { key: "domain", label: "Domain", defaultValue: "api.weixin.qq.com" },
    ],
  },
  weixin: {
    mode: "weixin",
    fields: [
      { key: "api_base", label: "API Base", defaultValue: DEFAULT_WEIXIN_API_BASE },
      { key: "cdn_base", label: "CDN Base" },
      { key: "bot_token", label: "Bot Token", type: "password", required: true },
      { key: "ilink_bot_id", label: "iLink Bot ID", required: true },
      { key: "ilink_user_id", label: "iLink User ID" },
      { key: "bot_type", label: "Bot Type", defaultValue: DEFAULT_WEIXIN_BOT_TYPE },
      { key: "allow_from", label: "Allow From (comma separated)" },
      { key: "long_connection_enabled", label: "Long Connection Enabled", type: "checkbox", defaultValue: true },
      { key: "poll_timeout_ms", label: "Poll Timeout (ms)" },
      { key: "api_timeout_ms", label: "API Timeout (ms)" },
      { key: "max_consecutive_failures", label: "Max Consecutive Failures" },
      { key: "backoff_ms", label: "Backoff (ms)" },
      { key: "route_tag", label: "Route Tag" },
    ],
  },
  qqbot: {
    mode: "config",
    configRoot: "qqbot",
    fields: [
      { key: "app_id", label: "App ID" },
      { key: "client_secret", label: "Client Secret", type: "password" },
      { key: "token", label: "Token", type: "password" },
      { key: "markdown_support", label: "Markdown Support", type: "checkbox", defaultValue: false },
    ],
  },
  whatsapp: {
    mode: "config",
    configRoot: "whatsapp_cloud",
    fields: [
      { key: "phone_number_id", label: "Phone Number ID", required: true },
      { key: "access_token", label: "Access Token", type: "password", required: true },
      { key: "verify_token", label: "Verify Token", type: "password" },
      { key: "api_version", label: "API Version", defaultValue: "v19.0" },
    ],
  },
  telegram: {
    mode: "config",
    configRoot: "telegram",
    fields: [{ key: "bot_token", label: "Bot Token", type: "password", required: true }],
  },
  discord: {
    mode: "config",
    configRoot: "discord",
    fields: [{ key: "bot_token", label: "Bot Token", type: "password", required: true }],
  },
  slack: {
    mode: "config",
    configRoot: "slack",
    fields: [
      { key: "app_token", label: "App Token", type: "password", required: true },
      { key: "bot_token", label: "Bot Token", type: "password", required: true },
    ],
  },
  line: {
    mode: "config",
    configRoot: "line",
    fields: [
      { key: "channel_secret", label: "Channel Secret", type: "password", required: true },
      { key: "access_token", label: "Access Token", type: "password", required: true },
    ],
  },
  dingtalk: {
    mode: "config",
    configRoot: "dingtalk",
    fields: [
      { key: "access_token", label: "Access Token", type: "password", required: true },
      { key: "secret", label: "Secret", type: "password", required: true },
    ],
  },
  xmpp: {
    mode: "config",
    configRoot: "xmpp",
    fields: [
      { key: "jid", label: "JID", required: true },
      { key: "password", label: "Password", type: "password", required: true },
      { key: "domain", label: "Domain", advanced: true },
      { key: "host", label: "Host" },
      { key: "port", label: "Port" },
      { key: "muc_nick", label: "MUC Nick", advanced: true },
      { key: "muc_rooms", label: "MUC Rooms (comma separated)", advanced: true },
      { key: "direct_tls", label: "Direct TLS", type: "checkbox", defaultValue: false, advanced: true },
      { key: "trust_self_signed", label: "Trust Self Signed", type: "checkbox", defaultValue: true },
      { key: "heartbeat_enabled", label: "Heartbeat Enabled", type: "checkbox", defaultValue: true, advanced: true },
      { key: "heartbeat_interval_s", label: "Heartbeat Interval (s)", advanced: true },
      { key: "heartbeat_timeout_s", label: "Heartbeat Timeout (s)", advanced: true },
      { key: "respond_ping", label: "Respond Ping", type: "checkbox", defaultValue: true, advanced: true },
    ],
  },
};

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
  dynamic_fields: {},
  xmpp_advanced_enabled: false,
  weixin_advanced_enabled: false,
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

const resolveSelectedChannel = () =>
  cleanText(elements.bridgeCenterChannelFormChannel?.value || state.bridgeCenter.channelForm.channel).toLowerCase();

const resolveChannelBindingAccountId = () => {
  const accountId = cleanText(
    elements.bridgeCenterChannelFormAccountId?.value || state.bridgeCenter.channelForm.account_id
  );
  state.bridgeCenter.channelForm.account_id = accountId;
  return accountId;
};

const parsePositiveInteger = (value) => {
  const parsed = Number.parseInt(String(value || "").trim(), 10);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    return undefined;
  }
  return parsed;
};

const parseCommaSeparatedList = (value) =>
  String(value || "")
    .split(/[,\n]/)
    .map((item) => item.trim())
    .filter(Boolean);

const isChannel = (value, channel) => cleanText(value).toLowerCase() === channel;

const schemaForChannel = (channel) => CHANNEL_FORM_SCHEMAS[cleanText(channel).toLowerCase()] || null;

const resolveVisibleSchemaFields = (channel, schema, fields) => {
  if (!schema) {
    return [];
  }
  if (isChannel(channel, WEIXIN_CHANNEL) && !state.bridgeCenter.channelForm.weixin_advanced_enabled) {
    return [];
  }
  if (isChannel(channel, "xmpp") && !state.bridgeCenter.channelForm.xmpp_advanced_enabled) {
    return fields.filter((field) => !field.advanced);
  }
  return fields;
};

const readSchemaNode = (mode, config) => {
  if (!isPlainObject(config)) {
    return {};
  }
  if (mode === "feishu" || mode === "wechat" || mode === "wechat_mp" || mode === "weixin") {
    const nested = config[mode];
    return isPlainObject(nested) ? nested : {};
  }
  return {};
};

const initDynamicFields = (channel, rawConfig = {}) => {
  const schema = schemaForChannel(channel);
  const dynamic = {};
  if (!schema) {
    state.bridgeCenter.channelForm.dynamic_fields = dynamic;
    return;
  }
  const source =
    schema.mode === "config"
      ? (isPlainObject(rawConfig?.[schema.configRoot || channel]) ? rawConfig[schema.configRoot || channel] : {})
      : readSchemaNode(schema.mode, rawConfig);
  schema.fields.forEach((field) => {
    if (field.type === "checkbox") {
      dynamic[field.key] = Boolean(source[field.key] ?? field.defaultValue);
      return;
    }
    const value = cleanText(source[field.key]);
    if (value) {
      dynamic[field.key] = value;
      return;
    }
    dynamic[field.key] = typeof field.defaultValue === "string" ? field.defaultValue : "";
  });
  state.bridgeCenter.channelForm.dynamic_fields = dynamic;
};

const buildDefaultBridgeAccountId = (channel) => {
  const center = currentCenter();
  const normalizedCenter = String(center?.center_id || "node")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "");
  const normalizedChannel = String(channel || "channel")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "");
  return `bridge_${normalizedChannel}_${normalizedCenter || "node"}`;
};

const renderChannelDynamicFields = () => {
  const container = elements.bridgeCenterChannelFormDynamicFields;
  if (!container) {
    return;
  }
  const channel = resolveSelectedChannel();
  const schema = schemaForChannel(channel);
  container.textContent = "";
  if (!schema) {
    const hint = document.createElement("div");
    hint.className = "muted";
    hint.textContent = "当前渠道暂无可视化配置字段，可切换到支持的渠道。";
    container.appendChild(hint);
    return;
  }
  const values = state.bridgeCenter.channelForm.dynamic_fields || {};
  if (isChannel(channel, WEIXIN_CHANNEL)) {
    const toggle = document.createElement("label");
    toggle.className = "bridge-channel-config-checkbox";
    const input = document.createElement("input");
    input.type = "checkbox";
    input.checked = Boolean(state.bridgeCenter.channelForm.weixin_advanced_enabled);
    input.addEventListener("change", () => {
      state.bridgeCenter.channelForm.weixin_advanced_enabled = Boolean(input.checked);
      renderChannelDynamicFields();
    });
    const text = document.createElement("span");
    text.textContent = "高级选项";
    toggle.appendChild(input);
    toggle.appendChild(text);
    container.appendChild(toggle);
  }
  if (isChannel(channel, "xmpp") && schema.fields.some((field) => field.advanced)) {
    const toggle = document.createElement("label");
    toggle.className = "bridge-channel-config-checkbox";
    const input = document.createElement("input");
    input.type = "checkbox";
    input.checked = Boolean(state.bridgeCenter.channelForm.xmpp_advanced_enabled);
    input.addEventListener("change", () => {
      state.bridgeCenter.channelForm.xmpp_advanced_enabled = Boolean(input.checked);
      renderChannelDynamicFields();
    });
    const text = document.createElement("span");
    text.textContent = "显示 XMPP 高级选项";
    toggle.appendChild(input);
    toggle.appendChild(text);
    container.appendChild(toggle);
  }
  const visibleFields = resolveVisibleSchemaFields(channel, schema, schema.fields);
  if (!visibleFields.length) {
    if (isChannel(channel, WEIXIN_CHANNEL)) {
      const hint = document.createElement("div");
      hint.className = "muted";
      hint.textContent = "开启高级选项后可填写 Weixin iLink 连接参数。";
      container.appendChild(hint);
    }
    state.bridgeCenter.channelForm.dynamic_fields = values;
    return;
  }
  visibleFields.forEach((field) => {
    if (field.type === "checkbox") {
      const row = document.createElement("label");
      row.className = "bridge-channel-config-checkbox";
      const input = document.createElement("input");
      input.type = "checkbox";
      input.checked = Boolean(values[field.key]);
      input.addEventListener("change", () => {
        values[field.key] = Boolean(input.checked);
      });
      const text = document.createElement("span");
      text.textContent = field.label;
      row.appendChild(input);
      row.appendChild(text);
      container.appendChild(row);
      return;
    }
    const row = document.createElement("label");
    row.className = "bridge-channel-config-field";
    const label = document.createElement("span");
    label.textContent = field.required ? `${field.label} *` : field.label;
    const input = document.createElement("input");
    input.type = field.type === "password" ? "password" : "text";
    input.placeholder = cleanText(field.placeholder || "");
    input.value = cleanText(values[field.key]);
    input.autocomplete = "off";
    input.addEventListener("input", () => {
      values[field.key] = input.value;
    });
    row.appendChild(label);
    row.appendChild(input);
    container.appendChild(row);
  });
  state.bridgeCenter.channelForm.dynamic_fields = values;
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
  const existingAccountId = resolveChannelBindingAccountId();
  const accountId = existingAccountId || buildDefaultBridgeAccountId(channel);
  state.bridgeCenter.channelForm.account_id = accountId;
  if (elements.bridgeCenterChannelFormAccountId) {
    elements.bridgeCenterChannelFormAccountId.value = accountId;
  }
  const ownedAccount = state.bridgeCenter.availableAccounts.find(
    (item) => item.channel === channel && item.account_id === accountId
  );
  initDynamicFields(channel, ownedAccount?.raw_config || {});
  if (elements.bridgeCenterChannelConfigHint) {
    const label = resolveChannelLabel(channel);
    elements.bridgeCenterChannelConfigHint.textContent = cleanText(channel)
      ? `当前将配置 ${label}，请填写连接参数后保存。`
      : "请先选择渠道并填写连接参数，保存后绑定到当前舰桥节点。";
  }
  renderChannelDynamicFields();
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
  const payload = await fetchJson("/admin/channels/accounts");
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
  if (!account) {
    state.bridgeCenter.selectedAccountId = "";
    const defaultChannel = state.bridgeCenter.meta?.supported_channels?.[0]?.channel || "";
    state.bridgeCenter.channelForm = {
      mode: "create",
      center_account_id: "",
      channel: defaultChannel,
      account_id: buildDefaultBridgeAccountId(defaultChannel),
      dynamic_fields: {},
      xmpp_advanced_enabled: false,
      weixin_advanced_enabled: false,
    };
  } else {
    state.bridgeCenter.selectedAccountId = account.center_account_id;
    state.bridgeCenter.channelForm = {
      mode: "edit",
      center_account_id: account.center_account_id,
      channel: account.channel,
      account_id: account.account_id,
      dynamic_fields: {},
      xmpp_advanced_enabled: false,
      weixin_advanced_enabled: false,
    };
  }
  const form = state.bridgeCenter.channelForm;
  if (elements.bridgeCenterChannelModalTitle) elements.bridgeCenterChannelModalTitle.textContent = "渠道设置";
  if (elements.bridgeCenterChannelEditorHint) {
    elements.bridgeCenterChannelEditorHint.textContent =
      form.mode === "edit"
        ? "当前节点已经绑定了一个渠道账号；切换绑定会清理该节点已有的自动路由和投递日志。"
        : "为当前舰桥节点配置渠道连接参数。";
  }
  if (elements.bridgeCenterChannelOwnedBadge) elements.bridgeCenterChannelOwnedBadge.textContent = form.mode === "edit" ? "已绑定" : "未绑定";
  if (elements.bridgeCenterChannelFormChannel) {
    elements.bridgeCenterChannelFormChannel.value = form.channel || "";
    elements.bridgeCenterChannelFormChannel.disabled = false;
  }
  if (elements.bridgeCenterChannelFormAccountId) {
    elements.bridgeCenterChannelFormAccountId.value = form.account_id || "";
  }
  if (elements.bridgeCenterChannelDeleteBtn) elements.bridgeCenterChannelDeleteBtn.disabled = form.mode !== "edit";
  const sourceConfig =
    state.bridgeCenter.availableAccounts.find(
      (item) => item.channel === form.channel && item.account_id === form.account_id
    )?.raw_config || {};
  initDynamicFields(form.channel, sourceConfig);
  refreshChannelAccountOptions();
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

const validateChannelFields = (channel, values) => {
  const schema = schemaForChannel(channel);
  if (!schema) {
    return `当前渠道暂不支持可视化配置：${channel}`;
  }
  const missing = schema.fields.find((field) => {
    if (!field.required) {
      return false;
    }
    if (field.type === "checkbox") {
      return false;
    }
    return !cleanText(values[field.key]);
  });
  if (!missing) {
    return "";
  }
  return `${missing.label} 为必填项`;
};

const validateWeixinNumericFields = (channel, values) => {
  if (!isChannel(channel, WEIXIN_CHANNEL)) {
    return "";
  }
  const numericFields = [
    { key: "poll_timeout_ms", label: "Poll Timeout (ms)" },
    { key: "api_timeout_ms", label: "API Timeout (ms)" },
    { key: "max_consecutive_failures", label: "Max Consecutive Failures" },
    { key: "backoff_ms", label: "Backoff (ms)" },
  ];
  for (const field of numericFields) {
    const rawValue = cleanText(values[field.key]);
    if (!rawValue) {
      continue;
    }
    if (!parsePositiveInteger(rawValue)) {
      return `${field.label} 需为正整数`;
    }
  }
  return "";
};

const validateWeixinCredentialsReady = (channel, values) => {
  if (!isChannel(channel, WEIXIN_CHANNEL)) {
    return "";
  }
  if (state.bridgeCenter.channelForm.weixin_advanced_enabled) {
    return "";
  }
  const hasBotToken = Boolean(cleanText(values.bot_token));
  const hasIlinkBotId = Boolean(cleanText(values.ilink_bot_id));
  const accountId = resolveChannelBindingAccountId();
  const ownedAccount = state.bridgeCenter.availableAccounts.find(
    (item) => item.channel === WEIXIN_CHANNEL && item.account_id === accountId
  );
  const rawConfig = isPlainObject(ownedAccount?.raw_config) ? ownedAccount.raw_config : {};
  const weixinNode = isPlainObject(rawConfig.weixin) ? rawConfig.weixin : {};
  const previewNode = isPlainObject(rawConfig.preview) ? rawConfig.preview : {};
  const previewWeixin = isPlainObject(previewNode.weixin) ? previewNode.weixin : {};
  const fallbackBotTokenSet =
    Boolean(cleanText(weixinNode.bot_token)) || previewWeixin.bot_token_set === true;
  const fallbackIlinkBotIdSet =
    Boolean(cleanText(weixinNode.ilink_bot_id)) || Boolean(cleanText(previewWeixin.ilink_bot_id));
  if ((hasBotToken || fallbackBotTokenSet) && (hasIlinkBotId || fallbackIlinkBotIdSet)) {
    return "";
  }
  return "请开启高级选项并填写 Bot Token 与 iLink Bot ID";
};

const buildStructuredConfigPatch = (channel, values) => {
  const schema = schemaForChannel(channel);
  if (!schema || schema.mode !== "config") {
    return {};
  }
  const configRoot = schema.configRoot || channel;
  const node = {};
  schema.fields.forEach((field) => {
    if (field.type === "checkbox") {
      node[field.key] = Boolean(values[field.key]);
      return;
    }
    const value = cleanText(values[field.key]);
    if (!value) {
      return;
    }
    if (configRoot === "xmpp" && field.key === "port") {
      const parsedPort = parsePositiveInteger(value);
      if (parsedPort && parsedPort <= 65535) {
        node[field.key] = parsedPort;
      }
      return;
    }
    if (
      configRoot === "xmpp" &&
      (field.key === "heartbeat_interval_s" || field.key === "heartbeat_timeout_s")
    ) {
      const parsed = parsePositiveInteger(value);
      if (parsed) {
        node[field.key] = parsed;
      }
      return;
    }
    if (configRoot === "xmpp" && field.key === "muc_rooms") {
      const rooms = parseCommaSeparatedList(value);
      if (rooms.length) {
        node[field.key] = rooms;
      }
      return;
    }
    node[field.key] = value;
  });
  if (!Object.keys(node).length) {
    return {};
  }
  return { [configRoot]: node };
};

const buildChannelUpsertPayload = (channel, accountId, values) => {
  const schema = schemaForChannel(channel);
  const payload = {
    channel,
    account_id: accountId,
    create_new: false,
    enabled: true,
    peer_kind: USER_ONLY_CHANNELS.has(channel) ? "user" : "group",
  };
  if (!schema) {
    return payload;
  }
  if (schema.mode === "feishu") {
    payload.app_id = cleanText(values.app_id);
    payload.app_secret = cleanText(values.app_secret);
    payload.domain = cleanText(values.domain) || "open.feishu.cn";
    payload.receive_group_chat = true;
    return payload;
  }
  if (schema.mode === "wechat") {
    payload.wechat = {
      corp_id: cleanText(values.corp_id),
      agent_id: cleanText(values.agent_id),
      secret: cleanText(values.secret),
      token: cleanText(values.token) || undefined,
      encoding_aes_key: cleanText(values.encoding_aes_key) || undefined,
      domain: cleanText(values.domain) || undefined,
    };
    payload.peer_kind = "user";
    return payload;
  }
  if (schema.mode === "wechat_mp") {
    payload.wechat_mp = {
      app_id: cleanText(values.app_id),
      app_secret: cleanText(values.app_secret),
      token: cleanText(values.token) || undefined,
      encoding_aes_key: cleanText(values.encoding_aes_key) || undefined,
      original_id: cleanText(values.original_id) || undefined,
      domain: cleanText(values.domain) || undefined,
    };
    payload.peer_kind = "user";
    return payload;
  }
  if (schema.mode === "weixin") {
    payload.weixin = {
      api_base: cleanText(values.api_base) || undefined,
      cdn_base: cleanText(values.cdn_base) || undefined,
      bot_token: cleanText(values.bot_token),
      ilink_bot_id: cleanText(values.ilink_bot_id),
      ilink_user_id: cleanText(values.ilink_user_id) || undefined,
      bot_type: cleanText(values.bot_type) || undefined,
      long_connection_enabled: Boolean(values.long_connection_enabled),
      allow_from: parseCommaSeparatedList(values.allow_from),
      poll_timeout_ms: parsePositiveInteger(values.poll_timeout_ms),
      api_timeout_ms: parsePositiveInteger(values.api_timeout_ms),
      max_consecutive_failures: parsePositiveInteger(values.max_consecutive_failures),
      backoff_ms: parsePositiveInteger(values.backoff_ms),
      route_tag: cleanText(values.route_tag) || undefined,
    };
    payload.peer_kind = "user";
    return payload;
  }
  const configPatch = buildStructuredConfigPatch(channel, values);
  if (Object.keys(configPatch).length) {
    payload.config = configPatch;
  }
  return payload;
};

const listOwnerChannelAccounts = async (ownerUserId, channel) => {
  const params = new URLSearchParams({ user_id: ownerUserId });
  if (channel) {
    params.set("channel", channel);
  }
  const payload = await fetchJson(`/channels/accounts?${params.toString()}`);
  const items = Array.isArray(payload?.data?.items) ? payload.data.items : [];
  return items
    .map((item) => ({
      channel: cleanText(item.channel).toLowerCase(),
      account_id: cleanText(item.account_id),
    }))
    .filter((item) => item.channel && item.account_id);
};

const upsertChannelAccountForCenter = async (center, channel, accountId) => {
  const ownerUserId = cleanText(center?.owner_user_id);
  if (!ownerUserId) {
    throw new Error("当前节点缺少 owner_user_id，无法写入渠道账号");
  }
  const values = state.bridgeCenter.channelForm.dynamic_fields || {};
  const weixinReadyError = validateWeixinCredentialsReady(channel, values);
  if (weixinReadyError) {
    throw new Error(weixinReadyError);
  }
  const weixinNumericError = validateWeixinNumericFields(channel, values);
  if (weixinNumericError) {
    throw new Error(weixinNumericError);
  }
  const fieldError = validateChannelFields(channel, values);
  if (fieldError) {
    throw new Error(fieldError);
  }
  const payload = buildChannelUpsertPayload(channel, accountId, values);
  const requestedAccountId = cleanText(accountId);
  const ownerAccounts = await listOwnerChannelAccounts(ownerUserId, channel);
  const matchedOwnedAccount = requestedAccountId
    ? ownerAccounts.find((item) => item.account_id.toLowerCase() === requestedAccountId.toLowerCase())
    : null;
  if (matchedOwnedAccount?.account_id) {
    payload.account_id = matchedOwnedAccount.account_id;
    payload.create_new = false;
  } else {
    delete payload.account_id;
    payload.create_new = true;
  }
  const params = new URLSearchParams({ user_id: ownerUserId });
  const result = await fetchJson(`/channels/accounts?${params.toString()}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(payload),
  });
  const resolvedAccountId = cleanText(result?.data?.account_id || payload.account_id);
  if (!resolvedAccountId) {
    throw new Error("渠道账号保存成功但未返回 account_id");
  }
  state.bridgeCenter.channelForm.account_id = resolvedAccountId;
  if (elements.bridgeCenterChannelFormAccountId) {
    elements.bridgeCenterChannelFormAccountId.value = resolvedAccountId;
  }
  return resolvedAccountId;
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
  const accountId = resolveChannelBindingAccountId() || buildDefaultBridgeAccountId(channel);
  state.bridgeCenter.channelForm.account_id = accountId;
  if (elements.bridgeCenterChannelFormAccountId) {
    elements.bridgeCenterChannelFormAccountId.value = accountId;
  }
  if (!confirmChannelReplacement(channel, accountId)) {
    return;
  }
  const resolvedAccountId = await upsertChannelAccountForCenter(center, channel, accountId);
  const existing = currentAccount();
  const bridgePayload = {
    center_id: center.center_id,
    channel,
    account_id: resolvedAccountId,
    enabled: true,
  };
  const bindingChanged = !existing || existing.channel !== channel || existing.account_id !== resolvedAccountId;
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
  const accountId = resolveChannelBindingAccountId();
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
    const channel = cleanText(elements.bridgeCenterChannelFormChannel.value).toLowerCase();
    state.bridgeCenter.channelForm.channel = channel;
    state.bridgeCenter.channelForm.account_id = buildDefaultBridgeAccountId(channel);
    state.bridgeCenter.channelForm.xmpp_advanced_enabled = false;
    state.bridgeCenter.channelForm.weixin_advanced_enabled = false;
    refreshChannelAccountOptions();
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


