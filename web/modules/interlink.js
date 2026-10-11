// 蜂窝/舰桥「互通舰队」页装配：
//   1) 总览（用户面 GET /wunder/interlink/nodes，本模块渲染进 #interlinkBody）
//   2) 治理面（只读告警与运行时，web/modules/interlink-alerts.js 渲染进 #interlinkAlertsHost）
// 治理面的数据契约与请求封装在 web/modules/interlink-shared.js，抽屉/分页器同样是共享单例，
// 本文件只做装配与节奏（首次进入拉取、可见时按视图声明的周期轮询）。
//
// 数据来源（I2 已实现，用户面端点）：
//   GET /wunder/interlink/nodes
//   -> { data: { nodes: [InterlinkNodeView...], aggregate_status, online_count, total } }
//   InterlinkNodeView 字段见 crates/wunder-core/src/interlink.rs：
//   node_id / node_type(web|desktop|cli|server) / user_id / label / status /
//   last_seen_at / capabilities / shadow_revision / connected / meta
//
// 本页只读：请求走 app.js 的全局 fetch 包装（自动补 Authorization / X-API-Key /
// X-Wunder-Language），与 cloud.js 等既有管理页保持一致，无需手动拼鉴权头。
//
// 模块自带样式与文案：为把对既有文件的改动压到最小，本模块通过 <style> 注入
// 自身的 .interlink-* 样式，动态文案走本地 TEXT 表（静态骨架文案仍走 i18n.js）。

import { getWunderBase } from "./api.js";
import { notify } from "./notify.js";
import { drawer } from "./interlink-shared.js";
import { alertsView } from "./interlink-alerts.js";

// 节点类型顺序：与 §5.2「按 client 分布」一致
const NODE_TYPES = ["desktop", "cli", "web", "server"];
// 统一状态模型（§5.1）
const NODE_STATUSES = ["online", "busy", "away", "reconnecting", "offline"];

const TYPE_ICONS = {
  desktop: "fa-solid fa-desktop",
  cli: "fa-solid fa-terminal",
  web: "fa-solid fa-globe",
  server: "fa-solid fa-server",
};

const STATUS_TONE = {
  online: "online",
  busy: "busy",
  away: "away",
  reconnecting: "reconnecting",
  offline: "offline",
};

const TEXT = {
  "zh-CN": {
    statOnline: "在线 / 总数",
    statAggregate: "聚合状态",
    statRefreshed: "最近刷新",
    distributionTitle: "按客户端分布",
    nodesTitle: "节点",
    nodeCount: "{count} 个节点",
    label: "名称",
    type: "类型",
    status: "状态",
    lastSeen: "最近活跃",
    shadow: "影子",
    shadowNever: "未同步",
    shadowRevision: "rev {revision}",
    connected: "隧道在线",
    disconnected: "隧道断开",
    capabilities: "能力",
    noCapabilities: "无",
    userId: "账号",
    never: "从未活跃",
    loading: "加载中...",
    empty: "当前账号下暂无互通节点。",
    disabled: "服务端未启用互通（interlink）。",
    loadFailed: "舰队数据加载失败：{message}",
    refreshedAt: "更新于 {time}",
    statusOnline: "在线",
    statusBusy: "忙碌",
    statusAway: "离开",
    statusReconnecting: "重连中",
    statusOffline: "离线",
    statusUnknown: "未知",
    heatmapTitle: "24h 在线率热图",
    tunnelTitle: "隧道质量",
    heatmapPlaceholder: "待管理端点接入后展示（24h 在线率）。",
    tunnelPlaceholder: "待管理端点接入后展示（RTT 分位、重连次数 TopN）。",
    pendingAdmin: "待接入管理端点：{path}",
  },
  "en-US": {
    statOnline: "Online / Total",
    statAggregate: "Aggregate",
    statRefreshed: "Refreshed",
    distributionTitle: "By client",
    nodesTitle: "Nodes",
    nodeCount: "{count} nodes",
    label: "Name",
    type: "Type",
    status: "Status",
    lastSeen: "Last seen",
    shadow: "Shadow",
    shadowNever: "not synced",
    shadowRevision: "rev {revision}",
    connected: "tunnel up",
    disconnected: "tunnel down",
    capabilities: "Capabilities",
    noCapabilities: "none",
    userId: "Account",
    never: "never",
    loading: "Loading...",
    empty: "No interlink nodes for this account.",
    disabled: "Interlink is disabled on this server.",
    loadFailed: "Failed to load fleet data: {message}",
    refreshedAt: "Updated {time}",
    statusOnline: "Online",
    statusBusy: "Busy",
    statusAway: "Away",
    statusReconnecting: "Reconnecting",
    statusOffline: "Offline",
    statusUnknown: "Unknown",
    heatmapTitle: "24h online heatmap",
    tunnelTitle: "Tunnel quality",
    heatmapPlaceholder: "Shown once the admin endpoint is available (24h online rate).",
    tunnelPlaceholder: "Shown once the admin endpoint is available (RTT percentiles, reconnect TopN).",
    pendingAdmin: "Pending admin endpoint: {path}",
  },
};

// admin 面契约（I11，尚未实现）：/wunder/admin/interlink/fleet
const ADMIN_FLEET_PATH = "/admin/interlink/fleet";

const STATUS_LABEL_KEY = {
  online: "statusOnline",
  busy: "statusBusy",
  away: "statusAway",
  reconnecting: "statusReconnecting",
  offline: "statusOffline",
};

// ---------------------------------------------------------------------------
// 环境 / 文案
// ---------------------------------------------------------------------------

const LANG_STORAGE_KEY = "wunder_app_config";

// 独立解析语言：避免与 app.js 的 i18n 实例产生重复模块实例（不同 ?v= 会导致
// 各自独立的状态），静态骨架文案仍由既有 i18n 负责。
const resolveLanguage = () => {
  try {
    const fromDom = String(document?.documentElement?.lang || "").trim();
    if (fromDom) {
      return fromDom;
    }
    const raw = localStorage.getItem(LANG_STORAGE_KEY);
    if (raw) {
      const parsed = JSON.parse(raw);
      const stored = String(parsed?.language || "").trim();
      if (stored) {
        return stored;
      }
    }
  } catch (error) {
    // 忽略：退回默认语言
  }
  return "zh-CN";
};

const t = (key, vars) => {
  const lang = resolveLanguage();
  const table = TEXT[lang] || TEXT["zh-CN"];
  let text = table[key] ?? TEXT["zh-CN"][key] ?? key;
  if (vars && typeof vars === "object") {
    Object.entries(vars).forEach(([name, value]) => {
      text = text.split(`{${name}}`).join(String(value));
    });
  }
  return text;
};

const statusLabel = (status) => t(STATUS_LABEL_KEY[status] || "statusUnknown");

// ---------------------------------------------------------------------------
// 样式（模块自带，避免改动 web/styles/*.css）
// ---------------------------------------------------------------------------

const STYLE_ID = "interlinkModuleStyles";

const ensureStyles = () => {
  if (document.getElementById(STYLE_ID)) {
    return;
  }
  const style = document.createElement("style");
  style.id = STYLE_ID;
  style.textContent = `
.interlink-body { flex: 1; min-height: 0; overflow: auto; display: flex; flex-direction: column; gap: 14px; }
.interlink-summary { display: flex; flex-wrap: wrap; align-items: center; gap: 18px; padding: 12px 14px; border: 1px solid #e2e8f0; border-radius: 10px; background: #f8fafc; }
.interlink-stat { display: flex; flex-direction: column; gap: 4px; }
.interlink-stat-label { font-size: 11px; color: #94a3b8; }
.interlink-stat-value { font-size: 18px; font-weight: 600; color: #1e293b; line-height: 1.1; }
.interlink-block { border: 1px solid #e2e8f0; border-radius: 10px; padding: 12px 14px; }
.interlink-block-title { display: flex; align-items: baseline; gap: 8px; margin: 0 0 10px; font-size: 13px; font-weight: 600; color: #334155; }
.interlink-block-title .interlink-block-hint { font-size: 11px; font-weight: 400; color: #94a3b8; }
.interlink-chips { display: flex; flex-wrap: wrap; gap: 8px; }
.interlink-chip { display: inline-flex; align-items: center; gap: 6px; padding: 4px 10px; border-radius: 999px; border: 1px solid #dbe4f0; background: #f1f5f9; color: #475569; font-size: 12px; }
.interlink-chip i { font-size: 11px; color: #64748b; }
.interlink-chip-value { font-weight: 600; color: #1e293b; }
.interlink-chip.is-zero { opacity: 0.55; }
.interlink-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); gap: 12px; }
.interlink-card { border: 1px solid #e2e8f0; border-radius: 10px; padding: 12px; background: #fff; display: flex; flex-direction: column; gap: 8px; }
.interlink-card-head { display: flex; align-items: center; gap: 8px; }
.interlink-dot { width: 10px; height: 10px; border-radius: 50%; flex: none; box-shadow: 0 0 0 2px rgba(148, 163, 184, 0.18); }
.interlink-dot.tone-online { background: #16a34a; }
.interlink-dot.tone-busy { background: #f59e0b; }
.interlink-dot.tone-away { background: #cbd5e1; }
.interlink-dot.tone-reconnecting { background: #0ea5e9; animation: interlinkPulse 1.2s ease-in-out infinite; }
.interlink-dot.tone-offline { background: #cbd5e1; box-shadow: none; }
@keyframes interlinkPulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.35; } }
.interlink-card-label { font-size: 13px; font-weight: 600; color: #1e293b; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.interlink-card-meta { font-size: 11px; color: #94a3b8; line-height: 1.5; word-break: break-all; }
.interlink-card-meta b { font-weight: 600; color: #64748b; }
.interlink-card-caps { display: flex; flex-wrap: wrap; gap: 4px; }
.interlink-cap { font-size: 10px; padding: 1px 6px; border-radius: 4px; background: #eef2f7; color: #64748b; border: 1px solid #e2e8f0; }
.interlink-status { display: inline-flex; align-items: center; padding: 1px 8px; border-radius: 999px; font-size: 11px; font-weight: 600; border: 1px solid transparent; }
.interlink-status.tone-online { background: #dcfce7; color: #15803d; border-color: #bbf7d0; }
.interlink-status.tone-busy { background: #fef3c7; color: #b45309; border-color: #fde68a; }
.interlink-status.tone-away { background: #f1f5f9; color: #64748b; border-color: #e2e8f0; }
.interlink-status.tone-reconnecting { background: #e0f2fe; color: #0369a1; border-color: #bae6fd; }
.interlink-status.tone-offline { background: #fee2e2; color: #b91c1c; border-color: #fecaca; }
.interlink-type { display: inline-flex; align-items: center; gap: 4px; font-size: 11px; color: #475569; }
.interlink-placeholder-box { border: 1px dashed #cbd5e1; border-radius: 8px; padding: 18px 14px; text-align: center; color: #94a3b8; font-size: 12px; background: #fbfcfe; }
.interlink-placeholder-box code { font-size: 11px; color: #64748b; background: #f1f5f9; padding: 1px 5px; border-radius: 4px; }
.interlink-empty { padding: 24px; text-align: center; color: #94a3b8; font-size: 13px; }
.interlink-empty.is-error { color: #b91c1c; }
`;
  document.head.appendChild(style);
};

// ---------------------------------------------------------------------------
// DOM 工具
// ---------------------------------------------------------------------------

const el = (tag, className, text) => {
  const node = document.createElement(tag);
  if (className) {
    node.className = className;
  }
  if (text !== undefined && text !== null) {
    node.textContent = String(text);
  }
  return node;
};

const clear = (node) => {
  if (node) {
    node.textContent = "";
  }
};

const buildStatusBadge = (status) => {
  const tone = STATUS_TONE[status] || "away";
  const badge = el("span", `interlink-status tone-${tone}`, statusLabel(status));
  badge.title = status || "";
  return badge;
};

const buildTypeChip = (type) => {
  const chip = el("span", "interlink-type");
  const icon = el("i", TYPE_ICONS[type] || "fa-solid fa-circle-nodes");
  chip.appendChild(icon);
  chip.appendChild(el("span", "", type || "-"));
  return chip;
};

// 契约时间戳为秒（f64）；对毫秒量级做防御性兼容（与 cloud.js 同口径）
const toEpochMs = (value) => {
  const ts = Number(value);
  if (!Number.isFinite(ts) || ts <= 0) {
    return 0;
  }
  return ts > 1e12 ? ts : ts * 1000;
};

const formatRelativeSeen = (value) => {
  const ms = toEpochMs(value);
  if (!ms) {
    return t("never");
  }
  const diffSeconds = Math.round((Date.now() - ms) / 1000);
  if (!Number.isFinite(diffSeconds)) {
    return t("never");
  }
  try {
    const formatter = new Intl.RelativeTimeFormat(resolveLanguage(), { numeric: "auto" });
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
  } catch (error) {
    return new Date(ms).toLocaleString();
  }
};

const formatClock = (ms) => {
  if (!ms) {
    return "-";
  }
  try {
    return new Date(ms).toLocaleTimeString();
  } catch (error) {
    return "-";
  }
};

const normalizeNode = (item) => ({
  node_id: String(item?.node_id || "").trim(),
  node_type: String(item?.node_type || "").trim().toLowerCase(),
  user_id: String(item?.user_id || "").trim(),
  label: String(item?.label || "").trim(),
  status: String(item?.status || "").trim().toLowerCase(),
  last_seen_at: Number(item?.last_seen_at ?? 0),
  capabilities: Array.isArray(item?.capabilities)
    ? item.capabilities.map((cap) => String(cap || "").trim()).filter(Boolean)
    : [],
  shadow_revision: Number(item?.shadow_revision ?? 0) || 0,
  connected: item?.connected === true,
  meta: item?.meta && typeof item.meta === "object" ? item.meta : {},
});

// ---------------------------------------------------------------------------
// 渲染
// ---------------------------------------------------------------------------

const nodeStatusOrder = (node) => {
  const index = NODE_STATUSES.indexOf(node.status);
  return index === -1 ? NODE_STATUSES.length : index;
};

const renderSummary = (container, data) => {
  const summary = el("div", "interlink-summary");

  const onlineStat = el("div", "interlink-stat");
  onlineStat.appendChild(el("span", "interlink-stat-label", t("statOnline")));
  const onlineValue = el(
    "span",
    "interlink-stat-value",
    `${Number(data.online_count) || 0} / ${Number(data.total) || 0}`
  );
  onlineStat.appendChild(onlineValue);
  summary.appendChild(onlineStat);

  const aggregateStat = el("div", "interlink-stat");
  aggregateStat.appendChild(el("span", "interlink-stat-label", t("statAggregate")));
  aggregateStat.appendChild(buildStatusBadge(String(data.aggregate_status || "")));
  summary.appendChild(aggregateStat);

  const refreshedStat = el("div", "interlink-stat");
  refreshedStat.appendChild(el("span", "interlink-stat-label", t("statRefreshed")));
  refreshedStat.appendChild(el("span", "interlink-stat-value", formatClock(Date.now())));
  summary.appendChild(refreshedStat);

  container.appendChild(summary);
};

const renderDistribution = (container, nodes) => {
  const block = el("section", "interlink-block");
  const title = el("h2", "interlink-block-title", t("distributionTitle"));
  title.appendChild(
    el("span", "interlink-block-hint", t("nodeCount", { count: nodes.length }))
  );
  block.appendChild(title);

  const chips = el("div", "interlink-chips");
  NODE_TYPES.forEach((type) => {
    const count = nodes.filter((node) => node.node_type === type).length;
    const chip = el("span", count ? "interlink-chip" : "interlink-chip is-zero");
    const icon = el("i", TYPE_ICONS[type] || "fa-solid fa-circle-nodes");
    chip.appendChild(icon);
    chip.appendChild(el("span", "", type));
    chip.appendChild(el("span", "interlink-chip-value", count));
    chips.appendChild(chip);
  });
  block.appendChild(chips);
  container.appendChild(block);
};

const renderNodeCard = (node) => {
  const card = el("article", "interlink-card");

  const head = el("div", "interlink-card-head");
  const dot = el("span", `interlink-dot tone-${STATUS_TONE[node.status] || "away"}`);
  dot.title = statusLabel(node.status);
  head.appendChild(dot);
  const label = el("span", "interlink-card-label", node.label || node.node_id || "-");
  label.title = node.node_id || "";
  head.appendChild(label);
  head.appendChild(buildStatusBadge(node.status));
  card.appendChild(head);

  const typeLine = el("div", "interlink-card-meta");
  typeLine.appendChild(buildTypeChip(node.node_type));
  typeLine.appendChild(el("span", "", ` · ${node.node_id || "-"}`));
  card.appendChild(typeLine);

  const seenLine = el("div", "interlink-card-meta");
  seenLine.appendChild(el("b", "", `${t("lastSeen")}: `));
  const relative = el("span", "", formatRelativeSeen(node.last_seen_at));
  const ms = toEpochMs(node.last_seen_at);
  relative.title = ms ? new Date(ms).toLocaleString() : t("never");
  seenLine.appendChild(relative);
  card.appendChild(seenLine);

  const shadowLine = el("div", "interlink-card-meta");
  shadowLine.appendChild(el("b", "", `${t("shadow")}: `));
  shadowLine.appendChild(
    el(
      "span",
      "",
      node.shadow_revision > 0
        ? t("shadowRevision", { revision: node.shadow_revision })
        : t("shadowNever")
    )
  );
  shadowLine.appendChild(
    el("span", "", ` · ${node.connected ? t("connected") : t("disconnected")}`)
  );
  card.appendChild(shadowLine);

  const metaParts = [];
  ["os", "arch", "app_version"].forEach((key) => {
    const value = node.meta?.[key];
    if (value) {
      metaParts.push(String(value));
    }
  });
  if (metaParts.length) {
    card.appendChild(el("div", "interlink-card-meta", metaParts.join(" / ")));
  }
  if (node.user_id) {
    card.appendChild(el("div", "interlink-card-meta", `${t("userId")}: ${node.user_id}`));
  }

  const capsLine = el("div", "interlink-card-meta");
  capsLine.appendChild(el("b", "", `${t("capabilities")}: `));
  card.appendChild(capsLine);
  const caps = el("div", "interlink-card-caps");
  if (node.capabilities.length) {
    node.capabilities.slice(0, 8).forEach((cap) => caps.appendChild(el("span", "interlink-cap", cap)));
  } else {
    caps.appendChild(el("span", "interlink-cap", t("noCapabilities")));
  }
  card.appendChild(caps);

  return card;
};

const renderNodes = (container, nodes) => {
  const block = el("section", "interlink-block");
  block.appendChild(el("h2", "interlink-block-title", t("nodesTitle")));

  if (!nodes.length) {
    block.appendChild(el("div", "interlink-empty", t("empty")));
    container.appendChild(block);
    return;
  }

  const grid = el("div", "interlink-grid");
  nodes
    .slice()
    .sort((a, b) => nodeStatusOrder(a) - nodeStatusOrder(b))
    .forEach((node) => grid.appendChild(renderNodeCard(node)));
  block.appendChild(grid);
  container.appendChild(block);
};

// 占位区块：24h 在线率热图与隧道质量依赖 admin 端点（I11），本轮不伪造数据。
const renderPlaceholders = (container) => {
  // TODO(I11): /wunder/admin/interlink/fleet 落地后，用其返回的
  // online_rate_24h（按小时桶）替换下方热图占位，用 tunnel（rtt_p50/p95、
  // reconnect_count）替换隧道质量占位。
  const blocks = [
    { title: t("heatmapTitle"), text: t("heatmapPlaceholder") },
    { title: t("tunnelTitle"), text: t("tunnelPlaceholder") },
  ];
  blocks.forEach((item) => {
    const block = el("section", "interlink-block");
    block.appendChild(el("h2", "interlink-block-title", item.title));
    const box = el("div", "interlink-placeholder-box");
    box.appendChild(el("div", "", item.text));
    const hint = el("div", "", t("pendingAdmin", { path: ADMIN_FLEET_PATH }));
    hint.style.marginTop = "6px";
    box.appendChild(hint);
    block.appendChild(box);
    container.appendChild(block);
  });
};

// ---------------------------------------------------------------------------
// 数据加载
// ---------------------------------------------------------------------------

const getBody = () => document.getElementById("interlinkBody");

const renderBody = (builder) => {
  const body = getBody();
  if (!body) {
    return;
  }
  clear(body);
  builder(body);
};

const renderMessage = (message, isError) => {
  renderBody((body) => {
    const box = el("div", isError ? "interlink-empty is-error" : "interlink-empty", message);
    body.appendChild(box);
  });
};

const extractResponseMessage = async (response, fallback) => {
  try {
    const payload = await response.json();
    return payload?.error?.message || payload?.message || payload?.detail?.message || fallback;
  } catch (error) {
    return fallback;
  }
};

// 首次进入面板时展示的骨架（尚未拿到数据）
const renderSkeleton = () => {
  renderBody((body) => {
    body.appendChild(el("div", "interlink-empty", t("loading")));
  });
};

export const loadInterlinkFleet = async () => {
  ensureStyles();
  const body = getBody();
  if (!body) {
    return;
  }
  if (!body.childElementCount) {
    renderSkeleton();
  }
  const endpoint = `${getWunderBase()}/interlink/nodes`;
  try {
    // credentials 显式声明同源携带；鉴权头由 app.js 的全局 fetch 包装补齐
    const response = await fetch(endpoint, { credentials: "same-origin" });
    if (response.status === 404) {
      throw new Error(t("disabled"));
    }
    if (!response.ok) {
      throw new Error(
        await extractResponseMessage(response, `HTTP ${response.status}`)
      );
    }
    const result = await response.json();
    const data = result?.data || {};
    const nodes = (Array.isArray(data.nodes) ? data.nodes : []).map(normalizeNode);
    renderBody((container) => {
      renderSummary(container, {
        online_count: data.online_count,
        total: data.total ?? nodes.length,
        aggregate_status: data.aggregate_status,
      });
      renderDistribution(container, nodes);
      renderNodes(container, nodes);
      renderPlaceholders(container);
    });
    return data;
  } catch (error) {
    const message = t("loadFailed", { message: error?.message || "unknown error" });
    renderMessage(message, true);
    notify(message, "error");
    throw error;
  }
};

// ---------------------------------------------------------------------------
// 治理面装配（互通告警与运行时，只读）
// ---------------------------------------------------------------------------

const GOVERNANCE_HOST_ID = "interlinkAlertsHost";
const GOVERNANCE_DRAWER_ID = "interlinkDrawer";
const governanceState = { mounted: false, timer: null, loaded: false };

const isInterlinkPanelActive = () =>
  Boolean(document.getElementById("interlinkPanel")?.classList.contains("active"));

const mountGovernance = () => {
  const host = document.getElementById(GOVERNANCE_HOST_ID);
  if (!host || governanceState.mounted) {
    return false;
  }
  // 抽屉是共享单例：节点详情与告警详情用同一层，不复制第二套详情实现
  drawer.init(GOVERNANCE_DRAWER_ID);
  alertsView.mount(host);
  governanceState.mounted = true;
  return true;
};

const loadGovernance = async () => {
  mountGovernance();
  if (!governanceState.mounted) {
    return;
  }
  governanceState.loaded = true;
  await alertsView.refresh();
};

// 语言切换后按新文案重建骨架（视图状态与筛选保留在模块实例里）
const rebuildGovernance = () => {
  if (!governanceState.mounted) {
    return;
  }
  alertsView.unmount();
  governanceState.mounted = false;
  if (mountGovernance()) {
    alertsView.refresh().catch(() => {});
  }
};

const startGovernancePolling = () => {
  if (governanceState.timer) {
    return;
  }
  governanceState.timer = setInterval(() => {
    // 面板不在前台就不打端点：治理页可能在后台停留很久
    if (!isInterlinkPanelActive() || !governanceState.loaded) {
      return;
    }
    alertsView.refresh().catch(() => {});
  }, alertsView.pollIntervalMs);
};

// ---------------------------------------------------------------------------
// 初始化
// ---------------------------------------------------------------------------

// 语言切换后重绘动态内容（静态骨架文案由既有 i18n 负责）
const bindLanguageSync = () => {
  if (typeof window === "undefined") {
    return;
  }
  window.addEventListener("wunder:language-changed", () => {
    loadInterlinkFleet().catch(() => {});
    rebuildGovernance();
  });
};

export const initInterlinkPanel = () => {
  ensureStyles();
  const refreshBtn = document.getElementById("interlinkRefreshBtn");
  if (refreshBtn && !refreshBtn.dataset.interlinkBound) {
    refreshBtn.dataset.interlinkBound = "1";
    refreshBtn.addEventListener("click", () => {
      loadInterlinkFleet().catch(() => {});
      loadGovernance().catch(() => {});
    });
  }
  const navBtn = document.getElementById("navInterlink");
  if (navBtn && !navBtn.dataset.interlinkGovBound) {
    navBtn.dataset.interlinkGovBound = "1";
    navBtn.addEventListener("click", () => {
      loadGovernance().catch(() => {});
    });
  }
  // 装配只建骨架；首次数据在用户进入面板（或点刷新）时才拉取
  mountGovernance();
  startGovernancePolling();
  bindLanguageSync();
  renderSkeleton();
};

// 供测试/其他模块复用的纯函数
export const __internals = { normalizeNode, STATUS_TONE, NODE_TYPES, NODE_STATUSES };