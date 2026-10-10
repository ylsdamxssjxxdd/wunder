// 舰桥「互通舰队」- 舰队总览视图 + 节点详情抽屉 + 策略操作（docs §5.2 / §9.2 / §13.5）
//
// 数据：GET /wunder/admin/interlink/fleet（有界 offset/limit + 页内聚合）
// 抽屉：影子摘要、生效能力、策略 overrides、最近 20 条命令、最近隧道开合、策略操作。
//
// 纪律：
// - 表格永远分页（offset/limit），一次性渲染的行数不超过一页；
// - 策略操作逐条二次确认，后端拒绝时保留按钮并显式呈现原因（前端确认不替代鉴权）；
// - 轮询由宿主（interlink.js）驱动，本模块只暴露 refresh()，不自建定时器。

import {
  COMMAND_KINDS,
  KNOWN_CAPS,
  DEFAULT_DEVICE_CAPS,
  NODE_STATUSES,
  adminGet,
  adminSend,
  badge,
  buildQuery,
  chip,
  clearNode,
  confirmAction,
  contract,
  createPager,
  drawer,
  el,
  formatClock,
  formatMs,
  formatRelative,
  formatRtt,
  normalizeCapabilities,
  normalizePolicy,
  pickFields,
  revokeCloudDevice,
  statusBadge,
  statusLabel,
  t,
  toEpochMs,
} from "./interlink-shared.js";

const HEAT_LEVELS = [0, 0.0001, 0.2, 0.4, 0.6, 0.8];

const heatLevel = (rate) => {
  const value = Number(rate) || 0;
  let level = 0;
  HEAT_LEVELS.forEach((threshold, index) => {
    if (value >= threshold) {
      level = index;
    }
  });
  return level;
};

const clientFamily = (client) => {
  const text = String(client || "").toLowerCase();
  if (text.includes("cli")) {
    return "cli";
  }
  if (text.includes("web")) {
    return "web";
  }
  return "desktop";
};

const normalizeDevice = (item) => {
  const policy = normalizePolicy(item?.policy_overrides);
  return {
    device_id: String(item?.device_id || "").trim(),
    user_id: String(item?.user_id || "").trim(),
    client: String(item?.client || "").trim(),
    name: String(item?.name || "").trim(),
    os: String(item?.os || "").trim(),
    arch: String(item?.arch || "").trim(),
    app_version: String(item?.app_version || "").trim(),
    status: String(item?.status || "").trim(),
    connected: item?.connected === true,
    tunnel_connected:
      item?.tunnel_connected === true || item?.tunnel_connected === false
        ? item.tunnel_connected
        : null,
    last_seen_at: Number(item?.last_seen_at ?? 0),
    last_tunnel_at: Number(item?.last_tunnel_at ?? 0),
    secret_version: Number(item?.secret_version ?? 0) || 0,
    interlink_enabled: item?.interlink_enabled !== false,
    revoked: item?.revoked === true,
    capabilities: normalizeCapabilities(item?.capabilities),
    policy,
    shadow_revision: Number(item?.shadow_revision ?? 0) || 0,
    rtt_ms: Number.isFinite(Number(item?.rtt_ms)) ? Number(item.rtt_ms) : null,
    resumed_count: Number(item?.resumed_count ?? 0) || 0,
  };
};

const normalizeAggregates = (value) => ({
  total: Number(value?.total ?? 0) || 0,
  scanned: Number(value?.scanned ?? 0) || 0,
  online: Number(value?.online ?? 0) || 0,
  busy: Number(value?.busy ?? 0) || 0,
  away: Number(value?.away ?? 0) || 0,
  reconnecting: Number(value?.reconnecting ?? 0) || 0,
  offline: Number(value?.offline ?? 0) || 0,
  desktop: Number(value?.by_client?.desktop ?? 0) || 0,
  cli: Number(value?.by_client?.cli ?? 0) || 0,
  web: Number(value?.by_client?.web ?? 0) || 0,
  rtt_p50_ms: Number.isFinite(Number(value?.rtt_p50_ms)) ? Number(value.rtt_p50_ms) : null,
  rtt_p95_ms: Number.isFinite(Number(value?.rtt_p95_ms)) ? Number(value.rtt_p95_ms) : null,
  reconnecting_top: Array.isArray(value?.reconnecting_top)
    ? value.reconnecting_top
        .map((item) => ({
          device_id: String(item?.device_id || "").trim(),
          resumed_count: Number(item?.resumed_count ?? 0) || 0,
        }))
        .filter((item) => item.device_id)
    : [],
});

const emptyPolicy = () => ({
  disabled_kinds: [],
  disabled_caps: [],
  force_approval_kinds: [],
  shadow_mode: "",
});

export const fleetView = {
  id: "fleet",
  pollIntervalMs: 20000,

  state: {
    page: 1,
    pageSize: 50,
    total: 0,
    filters: { user_id: "", client: "", status: "" },
    rows: [],
    aggregates: normalizeAggregates(null),
    onlineRate: [],
    liveChannels: 0,
    loading: false,
    pendingReload: false,
    loaded: false,
  },

  dom: null,
  pager: null,
  mountedRoot: null,
  drawerState: { device: null, commands: [], channels: [], busy: false, result: null },

  // -------------------------------------------------------------------------
  mount(root) {
    this.mountedRoot = root;
    clearNode(root);
    const wrap = el("div", "interlink-view interlink-view-fleet");

    // 筛选条
    const bar = el("div", "cloud-filter-bar");
    const userInput = el("input");
    userInput.type = "text";
    userInput.placeholder = t("filterUserHint");
    userInput.setAttribute("aria-label", t("filterUser"));
    const userBox = el("div", "header-input");
    userBox.appendChild(el("label", "", t("filterUser")));
    userBox.appendChild(userInput);

    const clientSelect = el("select");
    clientSelect.setAttribute("aria-label", t("filterClient"));
    clientSelect.appendChild(el("option", "", t("filterAll")));
    clientSelect.lastChild.value = "";
    ["desktop", "cli", "web"].forEach((value) => {
      const option = el("option", "", value);
      option.value = value;
      clientSelect.appendChild(option);
    });
    const clientBox = el("div", "header-input");
    clientBox.appendChild(el("label", "", t("filterClient")));
    clientBox.appendChild(clientSelect);

    const statusSelect = el("select");
    statusSelect.setAttribute("aria-label", t("filterStatus"));
    statusSelect.appendChild(el("option", "", t("filterAll")));
    statusSelect.lastChild.value = "";
    NODE_STATUSES.forEach((value) => {
      const option = el("option", "", statusLabel(value));
      option.value = value;
      statusSelect.appendChild(option);
    });
    const statusBox = el("div", "header-input");
    statusBox.appendChild(el("label", "", t("filterStatus")));
    statusBox.appendChild(statusSelect);

    const resetBtn = el("button", "secondary btn-with-icon btn-compact");
    resetBtn.type = "button";
    resetBtn.appendChild(el("i", "fa-solid fa-rotate-left"));
    resetBtn.appendChild(el("span", "", t("filterReset")));

    bar.appendChild(userBox);
    bar.appendChild(clientBox);
    bar.appendChild(statusBox);
    bar.appendChild(resetBtn);
    wrap.appendChild(bar);

    const notice = el("div", "interlink-notice is-hidden");
    wrap.appendChild(notice);

    const summary = el("div", "interlink-summary");
    wrap.appendChild(summary);

    const heatBlock = el("section", "interlink-block");
    const heatTitle = el("h2", "interlink-block-title", t("heatmapTitle"));
    heatTitle.appendChild(el("span", "interlink-block-hint", t("heatmapHint")));
    const heatBody = el("div", "");
    heatBlock.appendChild(heatTitle);
    heatBlock.appendChild(heatBody);
    wrap.appendChild(heatBlock);

    const qualityBlock = el("section", "interlink-block");
    qualityBlock.appendChild(el("h2", "interlink-block-title", t("qualityTitle")));
    const qualityBody = el("div", "interlink-block-row");
    const topBody = el("div", "interlink-chips");
    qualityBlock.appendChild(qualityBody);
    qualityBlock.appendChild(topBody);
    wrap.appendChild(qualityBlock);

    // 舰队表
    const tableBlock = el("section", "interlink-block");
    const scroll = el("div", "monitor-table-scroll");
    const table = el("table", "monitor-table interlink-table");
    const headers = [
      "colDevice",
      "colClient",
      "colUser",
      "colStatus",
      "colTunnel",
      "colShadow",
      "colRtt",
      "colResumed",
      "colSeen",
      "colInterlink",
      "colActions",
    ];
    const thead = el("thead");
    const headRow = el("tr");
    headers.forEach((key) => headRow.appendChild(el("th", "", t(key))));
    thead.appendChild(headRow);
    const tbody = el("tbody");
    table.appendChild(thead);
    table.appendChild(tbody);
    scroll.appendChild(table);
    const stateLine = el("div", "interlink-state", t("loading"));
    const pagerRoot = el("div", "interlink-pager");
    tableBlock.appendChild(el("h2", "interlink-block-title", t("colDevice")));
    tableBlock.appendChild(scroll);
    tableBlock.appendChild(stateLine);
    tableBlock.appendChild(pagerRoot);
    wrap.appendChild(tableBlock);

    root.appendChild(wrap);

    this.dom = { userInput, clientSelect, statusSelect, resetBtn, notice, summary, heatBody, qualityBody, topBody, tbody, stateLine };

    this.pager = createPager({
      root: pagerRoot,
      page: this.state.page,
      pageSize: this.state.pageSize,
      onChange: ({ page, pageSize }) => {
        this.state.page = page;
        this.state.pageSize = pageSize;
        this.refresh().catch(() => {});
      },
    });

    const applyFilters = () => {
      this.state.filters.user_id = userInput.value.trim();
      this.state.filters.client = clientSelect.value;
      this.state.filters.status = statusSelect.value;
      this.state.page = 1;
      this.refresh().catch(() => {});
    };
    userInput.addEventListener("keydown", (event) => {
      if (event.key === "Enter") {
        applyFilters();
      }
    });
    clientSelect.addEventListener("change", applyFilters);
    statusSelect.addEventListener("change", applyFilters);
    resetBtn.addEventListener("click", () => {
      userInput.value = "";
      clientSelect.value = "";
      statusSelect.value = "";
      applyFilters();
    });

    this.renderRows();
    return wrap;
  },

  unmount() {
    drawer.close();
    this.dom = null;
    this.pager = null;
    this.mountedRoot = null;
    this.drawerState = { device: null, commands: [], channels: [], busy: false, result: null };
  },

  syncFilterDom() {
    if (!this.dom) {
      return;
    }
    this.dom.userInput.value = this.state.filters.user_id || "";
    this.dom.clientSelect.value = this.state.filters.client || "";
    this.dom.statusSelect.value = this.state.filters.status || "";
  },

  // -------------------------------------------------------------------------
  async refresh() {
    if (!this.dom) {
      return;
    }
    if (this.state.loading) {
      this.state.pendingReload = true;
      return;
    }
    if (contract.isBlocked()) {
      this.showNotice(t("contractUnavailable", { reason: contract.reason }), true);
      this.dom.stateLine.textContent = t("contractUnavailable", { reason: contract.reason });
      this.dom.stateLine.classList.add("is-error");
      return;
    }
    this.state.loading = true;
    this.setBusy(true);
    if (!this.state.loaded) {
      this.dom.stateLine.textContent = t("loading");
      this.dom.stateLine.classList.remove("is-error");
    }
    const query = buildQuery({
      offset: this.pager ? this.pager.offset() : 0,
      limit: this.state.pageSize,
      user_id: this.state.filters.user_id,
      client: this.state.filters.client,
      status: this.state.filters.status,
    });
    try {
      const data = await adminGet("/fleet", query);
      this.state.rows = (Array.isArray(data.devices) ? data.devices : []).map(normalizeDevice);
      this.state.aggregates = normalizeAggregates(data.aggregates);
      this.state.total = Number(data.total ?? this.state.aggregates.total) || 0;
      this.state.onlineRate = Array.isArray(data.online_rate_24h) ? data.online_rate_24h : [];
      this.state.liveChannels = Number(data.live_channels ?? 0) || 0;
      this.state.loaded = true;
      this.state.page = this.pager ? this.pager.state.page : this.state.page;
      this.dom.stateLine.classList.remove("is-error");
      this.showNotice(
        t("scannedNote", {
          scanned: this.state.aggregates.scanned,
          total: this.state.total,
        }) +
          (this.state.filters.client || this.state.filters.status
            ? ` · ${t("filterClient")}/${t("filterStatus")}`
            : ""),
        false
      );
      this.renderSummary();
      this.renderHeat();
      this.renderQuality();
      this.renderRows();
    } catch (error) {
      this.state.rows = [];
      this.dom.stateLine.textContent = error?.message || String(error);
      this.dom.stateLine.classList.add("is-error");
      this.renderRows();
      if (error?.contractBlocked) {
        this.showNotice(t("contractUnavailable", { reason: contract.reason }), true);
      }
      throw error;
    } finally {
      this.state.loading = false;
      this.setBusy(false);
      if (this.state.pendingReload) {
        this.state.pendingReload = false;
        this.refresh().catch(() => {});
      }
    }
  },

  setBusy(busy) {
    if (!this.dom) {
      return;
    }
    this.dom.userInput.disabled = busy;
    this.dom.clientSelect.disabled = busy;
    this.dom.statusSelect.disabled = busy;
    this.pager?.render(busy);
  },

  showNotice(text, isError) {
    if (!this.dom) {
      return;
    }
    const notice = this.dom.notice;
    if (!text) {
      notice.classList.add("is-hidden");
      clearNode(notice);
      return;
    }
    notice.classList.remove("is-hidden");
    notice.textContent = text;
    notice.style.borderColor = isError ? "#fecaca" : "";
    notice.style.background = isError ? "#fef2f2" : "";
    notice.style.color = isError ? "#b91c1c" : "";
  },

  renderSummary() {
    if (!this.dom) {
      return;
    }
    const aggregates = this.state.aggregates;
    const box = this.dom.summary;
    clearNode(box);
    const onlineTotal = this.state.rows.length ? aggregates.online + aggregates.busy : 0;
    const stat = (label, value, sub) => {
      const item = el("div", "interlink-stat");
      item.appendChild(el("span", "interlink-stat-label", label));
      item.appendChild(el("span", "interlink-stat-value", value));
      if (sub) {
        item.appendChild(el("span", "interlink-stat-sub", sub));
      }
      return item;
    };
    box.appendChild(
      stat(t("statOnlineTotal"), `${onlineTotal} / ${aggregates.total}`, `p50 ${aggregates.scanned}`)
    );
    const distribution = el("div", "interlink-stat");
    distribution.appendChild(el("span", "interlink-stat-label", t("statByClient")));
    const chips = el("div", "interlink-chips");
    [
      ["desktop", aggregates.desktop],
      ["cli", aggregates.cli],
      ["web", aggregates.web],
    ].forEach(([name, count]) => {
      const node = chip(`${name} `);
      node.appendChild(el("span", "interlink-chip-value", String(count)));
      if (!count) {
        node.classList.add("is-zero");
      }
      chips.appendChild(node);
    });
    distribution.appendChild(chips);
    box.appendChild(distribution);

    box.appendChild(
      stat(
        t("statRtt"),
        `${aggregates.rtt_p50_ms === null ? "-" : formatRtt(aggregates.rtt_p50_ms)} / ${
          aggregates.rtt_p95_ms === null ? "-" : formatRtt(aggregates.rtt_p95_ms)
        }`,
        aggregates.rtt_p50_ms === null ? t("noSample") : ""
      )
    );
    box.appendChild(stat(t("statLiveTunnels"), String(this.state.liveChannels)));
    box.appendChild(stat(t("statusReconnecting"), String(aggregates.reconnecting)));
    box.appendChild(stat(t("statusOffline"), String(aggregates.offline)));
  },

  renderHeat() {
    if (!this.dom) {
      return;
    }
    const box = this.dom.heatBody;
    clearNode(box);
    const buckets = this.state.onlineRate;
    if (!buckets.length) {
      box.appendChild(el("div", "interlink-state is-empty", t("empty")));
      return;
    }
    const grid = el("div", "interlink-heat");
    buckets.forEach((bucket) => {
      const rate = Number(bucket?.rate ?? 0) || 0;
      const cell = el("div", "interlink-heat-cell");
      cell.dataset.level = String(heatLevel(rate));
      cell.title = `${formatClock(bucket?.hour_start)} · ${(rate * 100).toFixed(0)}% · ${
        Number(bucket?.active ?? 0) || 0
      }`;
      grid.appendChild(cell);
    });
    box.appendChild(grid);
    const axis = el("div", "interlink-heat-axis");
    const first = buckets[0]?.hour_start;
    const last = buckets[buckets.length - 1]?.hour_start;
    axis.appendChild(el("span", "", first ? formatClock(first) : "-"));
    axis.appendChild(el("span", "", last ? formatClock(last) : "-"));
    box.appendChild(axis);
  },

  renderQuality() {
    if (!this.dom) {
      return;
    }
    const aggregates = this.state.aggregates;
    const body = this.dom.qualityBody;
    clearNode(body);
    const pair = (label, value) => {
      const node = el("div", "interlink-pair");
      node.appendChild(el("span", "", label));
      node.appendChild(el("b", "", value));
      return node;
    };
    body.appendChild(
      pair(t("statRtt"), `${aggregates.rtt_p50_ms === null ? "-" : formatRtt(aggregates.rtt_p50_ms)} / ${
        aggregates.rtt_p95_ms === null ? "-" : formatRtt(aggregates.rtt_p95_ms)
      })
    );
    body.appendChild(pair(t("statusBusy"), String(aggregates.busy)));
    body.appendChild(pair(t("statusAway"), String(aggregates.away)));
    body.appendChild(pair(t("liveYes"), String(this.state.liveChannels)));

    const top = this.dom.topBody;
    clearNode(top);
    top.appendChild(el("span", "interlink-block-hint", `${t("statReconnectTop")}:`));
    if (!aggregates.reconnecting_top.length) {
      top.appendChild(el("span", "interlink-flag", t("topNone")));
      return;
    }
    aggregates.reconnecting_top.forEach((entry) => {
      const device = this.state.rows.find((row) => row.device_id === entry.device_id);
      const node = chip(`${entry.device_id} `);
      node.appendChild(el("span", "interlink-chip-value", `×${entry.resumed_count}`));
      node.style.cursor = "pointer";
      node.title = t("detail");
      node.addEventListener("click", () => this.openNodeDrawer(device || { device_id: entry.device_id }));
      top.appendChild(node);
    });
  },

  renderRows() {
    if (!this.dom) {
      return;
    }
    const tbody = this.dom.tbody;
    clearNode(tbody);
    if (!this.state.rows.length) {
      this.dom.stateLine.textContent = this.state.loaded ? t("empty") : t("loading");
      this.dom.stateLine.classList.toggle("is-empty", this.state.loaded);
      this.pager?.render(this.state.loading);
      return;
    }
    this.dom.stateLine.classList.add("is-hidden");
    const fragment = document.createDocumentFragment();
    this.state.rows.forEach((device) => {
      const row = el("tr");
      row.tabIndex = 0;

      const nameCell = el("td");
      nameCell.appendChild(el("div", "", device.name || device.device_id || "-"));
      nameCell.appendChild(el("div", "interlink-cell-sub", device.device_id));
      row.appendChild(nameCell);

      row.appendChild(el("td", "", device.client || "-"));
      row.appendChild(el("td", "", device.user_id || "-"));

      const statusCell = el("td");
      statusCell.appendChild(statusBadge(device.status));
      if (device.revoked) {
        statusCell.appendChild(badge(t("revokedBadge"), "interlink-revoked"));
      }
      row.appendChild(statusCell);

      const tunnelCell = el("td");
      tunnelCell.appendChild(
        device.connected
          ? badge(t("tunnelUp"), "interlink-live")
          : badge(t("tunnelDown"), "interlink-closed")
      );
      row.appendChild(tunnelCell);

      row.appendChild(
        el("td", "", device.shadow_revision > 0 ? `rev ${device.shadow_revision}` : t("shadowNever"))
      );
      row.appendChild(el("td", "", device.rtt_ms === null ? "-" : formatRtt(device.rtt_ms)));
      row.appendChild(el("td", "", String(device.resumed_count || 0)));

      const seenCell = el("td", "", formatRelative(device.last_seen_at));
      seenCell.title = formatClock(device.last_seen_at);
      row.appendChild(seenCell);

      row.appendChild(
        el(
          "td",
          "",
          device.interlink_enabled ? t("interlinkOn") : t("interlinkOff")
        )
      );

      const actionCell = el("td", "interlink-cell-actions");
      const detailBtn = el("button", "secondary", t("detail"));
      detailBtn.type = "button";
      detailBtn.addEventListener("click", (event) => {
        event.stopPropagation();
        this.openNodeDrawer(device);
      });
      actionCell.appendChild(detailBtn);
      row.appendChild(actionCell);

      row.addEventListener("click", () => this.openNodeDrawer(device));
      row.addEventListener("keydown", (event) => {
        if (event.key === "Enter") {
          this.openNodeDrawer(device);
        }
      });
      fragment.appendChild(row);
    });
    tbody.appendChild(fragment);
    this.pager?.render(this.state.loading);
  },

  // -------------------------------------------------------------------------
  // 节点详情抽屉
  // -------------------------------------------------------------------------

  openNodeDrawer(device) {
    if (!device?.device_id) {
      return;
    }
    this.drawerState = {
      device,
      commands: [],
      channels: [],
      busy: false,
      result: null,
    };
    drawer.open(
      t("drawerNodeTitle"),
      `${device.name || device.device_id} · ${device.device_id} · ${device.user_id || "-"}`,
      (body) => this.renderDrawer(body),
      () => {
        this.drawerState.device = null;
      }
    );
    this.loadDrawerDetail(device.device_id);
  },

  async loadDrawerDetail(deviceId) {
    const common = { device_id: deviceId, limit: 20, offset: 0 };
    try {
      const [commands, channels] = await Promise.all([
        adminGet("/commands", buildQuery(common)),
        adminGet("/channels", buildQuery(common)),
      ]);
      if (this.drawerState.device?.device_id !== deviceId) {
        return;
      }
      this.drawerState.commands = Array.isArray(commands.commands) ? commands.commands : [];
      this.drawerState.channels = Array.isArray(channels.channels) ? channels.channels : [];
      this.refreshDrawerSections();
    } catch (error) {
      if (this.drawerState.device?.device_id !== deviceId) {
        return;
      }
      this.drawerState.result = { tone: "error", text: error?.message || String(error) };
      this.refreshDrawerSections();
    }
  },

  renderDrawer(body) {
    const device = this.drawerState.device;
    if (!device) {
      return;
    }
    const sections = {};

    // 影子摘要
    const shadow = el("section", "interlink-section");
    shadow.appendChild(el("h3", "interlink-section-title", t("shadowSection")));
    const facts = el("div", "interlink-facts");
    const fact = (label, value, title) => {
      const item = el("div", "interlink-fact");
      item.appendChild(el("span", "", label));
      const strong = el("b", "", value);
      if (title) {
        strong.title = title;
      }
      item.appendChild(strong);
      return item;
    };
    facts.appendChild(
      fact(
        t("shadowRevision"),
        device.shadow_revision > 0 ? String(device.shadow_revision) : t("shadowNever")
      )
    );
    facts.appendChild(fact(t("colStatus"), statusLabel(device.status), device.status));
    facts.appendChild(
      fact(t("lastTunnel"), device.last_tunnel_at ? formatClock(device.last_tunnel_at) : "-")
    );
    facts.appendChild(fact(t("colSeen"), formatRelative(device.last_seen_at), formatClock(device.last_seen_at)));
    facts.appendChild(fact(t("secretVersion"), `v${device.secret_version}`));
    facts.appendChild(fact(t("colRtt"), device.rtt_ms === null ? "-" : formatRtt(device.rtt_ms)));
    facts.appendChild(fact(t("colResumed"), String(device.resumed_count || 0)));
    facts.appendChild(
      fact(
        t("osArch"),
        [device.os, device.arch, device.app_version].filter(Boolean).join(" / ") || "-"
      )
    );
    facts.appendChild(
      fact(t("colInterlink"), device.interlink_enabled ? t("interlinkOn") : t("interlinkOff"))
    );
    shadow.appendChild(facts);
    shadow.appendChild(el("div", "interlink-flag", t("shadowOwnerOnly")));
    body.appendChild(shadow);
    sections.shadow = shadow;

    // 生效能力
    const caps = el("section", "interlink-section");
    caps.appendChild(el("h3", "interlink-section-title", t("capsSection")));
    const capsRow = el("div", "interlink-chips");
    if (device.capabilities.length) {
      device.capabilities.forEach((cap) => capsRow.appendChild(chip(cap)));
    } else {
      capsRow.appendChild(el("span", "interlink-flag", t("capsNone")));
    }
    caps.appendChild(capsRow);
    body.appendChild(caps);
    sections.caps = caps;

    // 策略 overrides
    const policy = el("section", "interlink-section");
    policy.appendChild(el("h3", "interlink-section-title", t("policySection")));
    body.appendChild(policy);
    sections.policy = policy;
    this.renderPolicySection(policy, device);

    // 策略操作
    const ops = el("section", "interlink-section");
    ops.appendChild(el("h3", "interlink-section-title", t("opsSection")));
    ops.appendChild(el("div", "interlink-flag", t("opsHint")));
    const opsBody = el("div", "");
    ops.appendChild(opsBody);
    body.appendChild(ops);
    sections.ops = opsBody;
    this.renderOpsSection(opsBody, device);

    // 最近命令
    const commands = el("section", "interlink-section");
    commands.appendChild(el("h3", "interlink-section-title", t("commandsSection")));
    const commandsBody = el("div", "");
    commands.appendChild(commandsBody);
    body.appendChild(commands);
    sections.commands = commandsBody;

    // 最近隧道
    const channels = el("section", "interlink-section");
    channels.appendChild(el("h3", "interlink-section-title", t("channelsSection")));
    const channelsBody = el("div", "");
    channels.appendChild(channelsBody);
    body.appendChild(channels);
    sections.channels = channelsBody;

    const result = el("div", "interlink-op-result");
    body.appendChild(result);
    sections.result = result;

    this.drawerRefs = sections;
    this.renderCommandList(commandsBody);
    this.renderChannelList(channelsBody);
    this.renderResult();
  },

  refreshDrawer() {
    if (drawer.isOpen() && this.drawerState.device) {
      const body = drawer.body;
      clearNode(body);
      this.renderDrawer(body);
    }
  },

  refreshDrawerSections() {
    const device = this.drawerState.device;
    if (!device || !this.drawerRefs) {
      return;
    }
    this.renderPolicySection(this.drawerRefs.policy, device);
    this.renderOpsSection(this.drawerRefs.ops, device);
    this.renderCommandList(this.drawerRefs.commands);
    this.renderChannelList(this.drawerRefs.channels);
    this.renderResult();
  },

  renderPolicySection(node, device) {
    clearNode(node);
    node.appendChild(el("h3", "interlink-section-title", t("policySection")));
    const policy = device.policy || emptyPolicy();
    if (
      !policy.disabled_kinds.length &&
      !policy.disabled_caps.length &&
      !policy.force_approval_kinds.length &&
      !policy.shadow_mode
    ) {
      node.appendChild(el("div", "interlink-flag", t("policyNone")));
      return;
    }
    const line = (label, list, denied) => {
      const row = el("div", "interlink-line");
      row.appendChild(el("label", "", label));
      const chips = el("div", "interlink-chips");
      (list.length ? list : ["-"]).forEach((value) => {
        chips.appendChild(chip(value, { denied: denied === true }));
      });
      row.appendChild(chips);
      node.appendChild(row);
    };
    line(t("disabledKinds"), policy.disabled_kinds, true);
    line(t("disabledCaps"), policy.disabled_caps, true);
    line(t("forceApproval"), policy.force_approval_kinds);
    if (policy.shadow_mode) {
      const row = el("div", "interlink-line");
      row.appendChild(el("label", "", t("shadowMode")));
      row.appendChild(el("b", "", policy.shadow_mode));
      node.appendChild(row);
    }
  },

  renderCommandList(node) {
    if (!node) {
      return;
    }
    clearNode(node);
    if (!this.drawerState.commands.length) {
      node.appendChild(el("div", "interlink-flag", t("empty")));
      return;
    }
    const table = el("table", "interlink-mini-table");
    const head = el("tr");
    [t("colTime"), t("colKind"), t("colCommandStatus"), t("colApproval")].forEach((label) =>
      head.appendChild(el("th", "", label))
    );
    const thead = el("thead");
    thead.appendChild(head);
    const tbody = el("tbody");
    this.drawerState.commands.forEach((item) => {
      const row = el("tr");
      const timeCell = el("td", "", formatRelative(item.created_at));
      timeCell.title = formatClock(item.created_at);
      row.appendChild(timeCell);
      row.appendChild(el("td", "interlink-mono", String(item.kind || "-")));
      row.appendChild(el("td", "", String(item.status || "-")));
      row.appendChild(el("td", "", String(item.approval_state || "-")));
      row.style.cursor = "pointer";
      row.title = t("drawerCommandTitle");
      row.addEventListener("click", (event) => {
        event.stopPropagation();
        this.openCommandDrawer(item);
      });
      tbody.appendChild(row);
    });
    table.appendChild(thead);
    table.appendChild(tbody);
    node.appendChild(table);
  },

  renderChannelList(node) {
    if (!node) {
      return;
    }
    clearNode(node);
    if (!this.drawerState.channels.length) {
      node.appendChild(el("div", "interlink-flag", t("empty")));
      return;
    }
    const table = el("table", "interlink-mini-table");
    const head = el("tr");
    [t("colConnected"), t("colLastBeat"), t("colRtt"), t("colResumed"), t("colClosedReason")].forEach(
      (label) => head.appendChild(el("th", "", label))
    );
    const thead = el("thead");
    thead.appendChild(head);
    const tbody = el("tbody");
    this.drawerState.channels.slice(0, 20).forEach((item) => {
      const row = el("tr");
      const opened = el("td", "", formatRelative(item.connected_at));
      opened.title = formatClock(item.connected_at);
      row.appendChild(opened);
      const beat = el("td", "", formatRelative(item.last_seen_at));
      beat.title = formatClock(item.last_seen_at);
      row.appendChild(beat);
      row.appendChild(el("td", "", item.rtt_ms === null || item.rtt_ms === undefined ? "-" : formatMs(item.rtt_ms)));
      row.appendChild(el("td", "", String(item.resumed_count ?? 0)));
      const reason = el(
        "td",
        "interlink-cell-sub",
        item.closed_reason || (item.live ? t("liveYes") : t("liveNo"))
      );
      row.appendChild(reason);
      tbody.appendChild(row);
    });
    table.appendChild(thead);
    table.appendChild(tbody);
    node.appendChild(table);
  },

  renderResult() {
    const node = this.drawerRefs?.result;
    if (!node) {
      return;
    }
    const result = this.drawerState.result;
    clearNode(node);
    node.classList.remove("is-error", "is-ok");
    if (!result) {
      node.style.display = "none";
      return;
    }
    node.style.display = "";
    node.classList.add(result.tone === "error" ? "is-error" : "is-ok");
    node.textContent = result.text;
  },

  setDrawerBusy(busy) {
    this.drawerState.busy = busy;
    if (this.drawerRefs?.ops) {
      this.renderOpsSection(this.drawerRefs.ops, this.drawerState.device);
    }
  },

  // -------------------------------------------------------------------------
  // 策略操作：每项都二次确认，后端拒绝时显式呈现原因
  // -------------------------------------------------------------------------

  renderOpsSection(node, device) {
    if (!node || !device) {
      return;
    }
    clearNode(node);
    const policy = device.policy || emptyPolicy();
    const blocked = device.revoked || contract.isBlocked() || this.drawerState.busy;
    const blockedReason = device.revoked
      ? t("revokedDeviceBlocked")
      : contract.isBlocked()
        ? contract.reason
        : this.drawerState.busy
          ? t("opsPending")
          : "";

    const opButton = (label, className, onClick, options = {}) => {
      const button = el("button", `secondary interlink-op ${className || ""}`.trim(), label);
      button.type = "button";
      const disabled = options.forceDisabled === true || blocked;
      button.disabled = disabled;
      if (disabled) {
        button.title = options.disabledTitle || blockedReason || t("opsPending");
      }
      if (!disabled) {
        button.addEventListener("click", (event) => {
          event.stopPropagation();
          onClick();
        });
      }
      if (options.danger) {
        button.classList.remove("secondary");
        button.classList.add("danger");
      }
      return button;
    };

    // 1) kill switch
    const pauseLine = el("div", "interlink-line");
    pauseLine.appendChild(
      opButton(
        device.interlink_enabled ? t("opPause") : t("opResume"),
        "",
        () =>
          this.confirmAndPatch(
            device,
            { interlink_enabled: !device.interlink_enabled },
            device.interlink_enabled
              ? {
                  title: t("confirmPauseTitle"),
                  summary: t("confirmPauseSummary"),
                  danger: true,
                }
              : {
                  title: t("confirmResumeTitle"),
                  summary: t("confirmResumeSummary"),
                  danger: false,
                }
          ),
        {
          forceDisabled: false,
          disabledTitle: device.interlink_enabled ? t("opAlreadyActive") : t("opAlreadyPaused"),
        }
      )
    );
    pauseLine.appendChild(
      opButton(t("opRotateSecret"), "", () => this.confirmAndRotate(device), { danger: true })
    );
    pauseLine.appendChild(
      opButton(t("opRevokeDevice"), "", () => this.confirmAndRevoke(device), { danger: true })
    );
    node.appendChild(pauseLine);

    // 2) 禁用 / 解禁命令类型
    const kindSelect = el("select");
    kindSelect.setAttribute("aria-label", t("colKind"));
    COMMAND_KINDS.forEach((kind) => {
      const option = el("option", "", kind);
      option.value = kind;
      kindSelect.appendChild(option);
    });
    const kindLine = el("div", "interlink-line");
    kindLine.appendChild(el("label", "", t("colKind")));
    kindLine.appendChild(kindSelect);
    kindLine.appendChild(
      opButton(t("opRevokeKind"), "", () =>
        this.confirmAndPatch(
          device,
          {
            policy_overrides: {
              disabled_kinds: union(policy.disabled_kinds, [kindSelect.value]),
            },
          },
          {
            title: t("confirmApplyTitle"),
            summary: `${t("disabledKinds")}: ${kindSelect.value}`,
            danger: true,
          }
        )
      )
    );
    kindLine.appendChild(
      opButton(t("opForceApproval"), "", () =>
        this.confirmAndPatch(
          device,
          {
            policy_overrides: {
              force_approval_kinds: union(policy.force_approval_kinds, [kindSelect.value]),
            },
          },
          {
            title: t("confirmApplyTitle"),
            summary: `${t("forceApproval")}: ${kindSelect.value}`,
            danger: false,
          }
        )
      )
    );
    node.appendChild(kindLine);

    const removeKindLine = el("div", "interlink-line");
    removeKindLine.appendChild(el("label", "", t("disabledKinds")));
    if (policy.disabled_kinds.length) {
      policy.disabled_kinds.forEach((kind) => {
        removeKindLine.appendChild(
          chip(kind, {
            denied: true,
            removeTitle: t("opEnableKind"),
            onRemove: () =>
              this.confirmAndPatch(
                device,
                { policy_overrides: { disabled_kinds: without(policy.disabled_kinds, [kind]) } },
                {
                  title: t("confirmApplyTitle"),
                  summary: `${t("opEnableKind")}: ${kind}`,
                  danger: false,
                }
              ),
          })
        );
      });
    } else {
      removeKindLine.appendChild(el("span", "interlink-flag", t("filterAll")));
    }
    node.appendChild(removeKindLine);

    const unforceLine = el("div", "interlink-line");
    unforceLine.appendChild(el("label", "", t("forceApproval")));
    if (policy.force_approval_kinds.length) {
      policy.force_approval_kinds.forEach((kind) => {
        unforceLine.appendChild(
          chip(kind, {
            removeTitle: t("opUnforceApproval"),
            onRemove: () =>
              this.confirmAndPatch(
                device,
                {
                  policy_overrides: {
                    force_approval_kinds: without(policy.force_approval_kinds, [kind]),
                  },
                },
                {
                  title: t("confirmApplyTitle"),
                  summary: `${t("opUnforceApproval")}: ${kind}`,
                  danger: false,
                }
              ),
          })
        );
      });
    } else {
      unforceLine.appendChild(el("span", "interlink-flag", t("empty")));
    }
    node.appendChild(unforceLine);

    // 3) 影子模式
    const shadowLine = el("div", "interlink-line");
    shadowLine.appendChild(
      opButton(t("opShadowMinimal"), "", () =>
        this.confirmAndPatch(
          device,
          { policy_overrides: { shadow_mode: "minimal" } },
          { title: t("confirmApplyTitle"), summary: `${t("shadowMode")}: minimal`, danger: false }
        )
      )
    );
    shadowLine.appendChild(
      opButton(t("opShadowFull"), "", () =>
        this.confirmAndPatch(
          device,
          { policy_overrides: { shadow_mode: "" } },
          {
            title: t("confirmApplyTitle"),
            summary: `${t("shadowMode")}: ${t("opShadowFull")}`,
            danger: false,
          }
        ),
      { forceDisabled: policy.shadow_mode !== "minimal", disabledTitle: t("policyNone") }
    );
    node.appendChild(shadowLine);

    // 4) 能力收敛
    const capSelect = el("select");
    capSelect.setAttribute("aria-label", t("colCaps"));
    KNOWN_CAPS.forEach((cap) => {
      const option = el("option", "", cap);
      option.value = cap;
      capSelect.appendChild(option);
    });
    const capLine = el("div", "interlink-line");
    capLine.appendChild(el("label", "", t("colCaps")));
    capLine.appendChild(capSelect);
    capLine.appendChild(
      opButton(t("opDropCap"), "", () =>
        this.confirmAndPatch(
          device,
          { capabilities: without(authorizedCaps(device), [capSelect.value]) },
          {
            title: t("confirmApplyTitle"),
            summary: `${t("opDropCap")}: ${capSelect.value}`,
            danger: true,
          }
        )
      )
    );
    capLine.appendChild(
      opButton(t("opRestoreCaps"), "", () =>
        this.confirmAndPatch(
          device,
          { capabilities: [...DEFAULT_DEVICE_CAPS] },
          {
            title: t("confirmApplyTitle"),
            summary: `${t("opRestoreCaps")}: ${DEFAULT_DEVICE_CAPS.join(", ")}`,
            danger: false,
          }
        )
      )
    );
    node.appendChild(capLine);

    const capDenyLine = el("div", "interlink-line");
    capDenyLine.appendChild(el("label", "", t("disabledCaps")));
    if (policy.disabled_caps.length) {
      policy.disabled_caps.forEach((cap) => {
        capDenyLine.appendChild(
          chip(cap, {
            denied: true,
            removeTitle: t("opRestoreCaps"),
            onRemove: () =>
              this.confirmAndPatch(
                device,
                { policy_overrides: { disabled_caps: without(policy.disabled_caps, [cap]) } },
                {
                  title: t("confirmApplyTitle"),
                  summary: `${t("disabledCaps")} - ${cap}`,
                  danger: false,
                }
              ),
          })
        );
      });
    } else {
      capDenyLine.appendChild(el("span", "interlink-flag", t("empty")));
    }
    node.appendChild(capDenyLine);

    if (blockedReason) {
      node.appendChild(el("div", "interlink-op-result is-error", blockedReason));
    }
  },

  async confirmAndPatch(device, body, meta) {
    if (!this.guardDevice(device)) {
      return;
    }
    confirmAction({
      title: meta.title,
      summary: meta.summary,
      details: [
        t("detailDevice", { device: device.device_id }),
        t("detailUser", { user: device.user_id || "-" }),
        t("detailShadow", { revision: device.shadow_revision }),
      ],
      hint: t("opsHint"),
      confirmLabel: t("confirmOk"),
      danger: meta.danger === true,
      onConfirm: async () => {
        this.setDrawerBusy(true);
        const result = await adminSend(
          `/devices/${encodeURIComponent(device.device_id)}/policy`,
          "PATCH",
          body
        );
        this.applyOpResult(device, result, () => this.patchSuccess(result));
        this.setDrawerBusy(false);
        return result.ok;
      },
    });
  },

  patchSuccess(result) {
    const data = result.data || {};
    const effective = Array.isArray(data.effective?.capabilities)
      ? data.effective.capabilities.join(", ")
      : "-";
    return t("opsOk", {
      detail: `changed=${data.changed === true} tunnel_closed=${data.tunnel_closed === true} caps=[${effective}]`,
    });
  },

  async confirmAndRotate(device) {
    if (!this.guardDevice(device)) {
      return;
    }
    confirmAction({
      title: t("confirmRotateTitle"),
      summary: t("confirmRotateSummary"),
      details: [
        t("detailDevice", { device: device.device_id }),
        `${t("secretVersion")}: v${device.secret_version}`,
      ],
      hint: t("opsHint"),
      confirmLabel: t("confirmDangerOk"),
      danger: true,
      onConfirm: async () => {
        this.setDrawerBusy(true);
        const result = await adminSend(
          `/devices/${encodeURIComponent(device.device_id)}/rotate_secret`,
          "POST"
        );
        this.applyOpResult(device, result, () =>
          result.ok
            ? t("opsOk", {
                detail: `secret_version=v${result.data?.secret_version} rotated_at=${formatClock(
                  result.data?.rotated_at
                )} tunnel_closed=${result.data?.tunnel_closed === true}`,
              })
            : ""
        );
        this.setDrawerBusy(false);
        return result.ok;
      },
    });
  },

  async confirmAndRevoke(device) {
    if (!this.guardDevice(device)) {
      return;
    }
    confirmAction({
      title: t("confirmRevokeTitle"),
      summary: t("confirmRevokeSummary"),
      details: [
        t("detailDevice", { device: device.device_id }),
        t("detailUser", { user: device.user_id || "-" }),
      ],
      ackLabel: t("confirmRevokeAck"),
      danger: true,
      confirmLabel: t("confirmDangerOk"),
      onConfirm: async () => {
        this.setDrawerBusy(true);
        // 第 1 步：暂停互通（关闭在线隧道）；第 2 步：调用既有云端吊销端点。
        const first = await adminSend(`/devices/${encodeURIComponent(device.device_id)}/policy`, "PATCH", {
          interlink_enabled: false,
        });
        if (!first.ok) {
          this.applyOpResult(device, first, () => "");
          this.setDrawerBusy(false);
          return false;
        }
        const second = await revokeCloudDevice(device.device_id);
        this.applyOpResult(
          device,
          second.ok ? { ok: true, data: { tunnel_closed: first.data?.tunnel_closed } } : second,
          () =>
            second.ok
              ? t("opsOk", { detail: `${t("step1")} ✓ · ${t("step2")} ✓` })
              : t("stepFailed", {
                  step: t("step2"),
                  message: second.message || t("unknownError"),
                })
        );
        if (second.ok) {
          await this.refresh().catch(() => {});
          this.drawerState.device = {
            ...device,
            revoked: true,
            interlink_enabled: false,
            connected: false,
            status: "offline",
          };
          this.refreshDrawer();
        }
        this.setDrawerBusy(false);
        return second.ok;
      },
    });
  },

  guardDevice(device) {
    if (!device?.device_id) {
      return false;
    }
    if (device.revoked) {
      this.drawerState.result = { tone: "error", text: t("revokedDeviceBlocked") };
      this.renderResult();
      return false;
    }
    if (contract.isBlocked()) {
      this.drawerState.result = {
        tone: "error",
        text: t("contractUnavailable", { reason: contract.reason }),
      };
      this.renderResult();
      return false;
    }
    return true;
  },

  applyOpResult(device, result, successText) {
    if (result.ok) {
      this.drawerState.result = { tone: "ok", text: successText ? successText(result) : t("opsOk", { detail: "-" }) };
      // 成功后以服务端回写的有效策略刷新行与抽屉
      const nextPolicy = normalizePolicy(result.data?.policy_overrides ?? device.policy);
      this.drawerState.device = {
        ...device,
        interlink_enabled:
          result.data?.interlink_enabled === undefined
            ? device.interlink_enabled
            : result.data.interlink_enabled === true,
        capabilities: normalizeCapabilities(result.data?.effective?.capabilities ?? device.capabilities),
        policy: nextPolicy,
      };
      const index = this.state.rows.findIndex((row) => row.device_id === device.device_id);
      if (index >= 0) {
        this.state.rows[index] = this.drawerState.device;
      }
      this.refreshDrawerSections();
      this.refresh().catch(() => {});
      return;
    }
    this.drawerState.result = {
      tone: "error",
      text: `${t("opsDenied", { reason: result.message || `${t("requestFailed", { status: result.status })}` })}`,
    };
    this.renderResult();
  },

  // 命令生命周期抽屉（从舰队详情穿透，也供台账页复用）
  openCommandDrawer(item) {
    const detail = item && typeof item === "object" ? item : null;
    if (!detail) {
      return;
    }
    drawer.open(t("drawerCommandTitle"), `${detail.command_id || "-"} · ${detail.kind || "-"}`, (body) =>
      renderCommandTimeline(body, detail)
    );
  },
};

const authorizedCaps = (device) => {
  const declared = normalizeCapabilities(device?.capabilities);
  const disabled = normalizePolicy(device?.policy).disabled_caps;
  // 抽屉里的「生效能力」是求交结果；移除能力要作用在授权集上，这里以生效集
  // 加回被管理员禁用的项，得到可编辑的授权集近似值。
  return union(declared, disabled);
};

const union = (list, extra) => {
  const out = [];
  [...(list || []), ...(extra || [])].forEach((value) => {
    const text = String(value || "").trim();
    if (text && !out.includes(text)) {
      out.push(text);
    }
  });
  return out;
};

const without = (list, drop) =>
  (list || []).filter((value) => !(drop || []).includes(value));

export const renderCommandTimeline = (body, record) => {
  const created = Number(record?.created_at ?? 0);
  const acked = Number(record?.acked_at ?? 0);
  const finished = Number(record?.finished_at ?? 0);
  const status = String(record?.status || "");
  const failed = ["failed", "timeout", "canceled"].includes(status);

  const facts = el("div", "interlink-facts");
  const fact = (label, value, title) => {
    const item = el("div", "interlink-fact");
    item.appendChild(el("span", "", label));
    const strong = el("b", "", value);
    if (title) {
      strong.title = title;
    }
    item.appendChild(strong);
    return item;
  };
  facts.appendChild(fact(t("colKind"), String(record?.kind || "-")));
  facts.appendChild(fact(t("colDirection"), String(record?.direction || "-")));
  facts.appendChild(fact(t("colCommandStatus"), status || "-"));
  facts.appendChild(fact(t("colApproval"), String(record?.approval_state || "-")));
  facts.appendChild(fact(t("colUser"), String(record?.actor_user_id || "-")));
  facts.appendChild(
    fact(t("colRoute"), `${record?.from_node || "-"} → ${record?.to_node || "-"}`)
  );
  body.appendChild(facts);

  const timeline = el("div", "interlink-timeline");
  const step = (label, time, delta, tone) => {
    const item = el("div", `interlink-timeline-item${tone ? ` is-${tone}` : ""}`);
    item.appendChild(el("span", "interlink-timeline-mark"));
    const inner = el("div", "");
    inner.appendChild(el("div", "interlink-timeline-label", label));
    inner.appendChild(
      el(
        "div",
        "interlink-timeline-time",
        time ? `${formatClock(time)} · ${formatRelative(time)}` : "-"
      )
    );
    if (delta) {
      inner.appendChild(el("div", "interlink-delta", delta));
    }
    item.appendChild(inner);
    timeline.appendChild(item);
  };
  step(t("timelineIssued"), created, "", "done");
  step(
    t("timelineAcked"),
    acked,
    acked && created ? `${t("ackLatency")}: ${formatMs((acked - created) * 1000)}` : t("timelineNoAck"),
    acked ? "done" : ""
  );
  step(
    t("timelineFinished"),
    finished,
    finished && created ? `${t("totalDuration")}: ${formatMs((finished - created) * 1000)}` : t("timelineNoFinish"),
    finished ? (failed ? "fail" : "done") : ""
  );
  body.appendChild(timeline);

  if (record?.error_code || record?.error_summary) {
    const errorBox = el("div", "interlink-op-result is-error");
    errorBox.textContent = `${String(record.error_code || "-")} · ${String(
      record.error_summary || ""
    )}`.trim();
    body.appendChild(errorBox);
  }

  const digestSection = el("section", "interlink-section");
  digestSection.appendChild(el("h3", "interlink-section-title", t("colDigest")));
  const digest = record?.args_digest && typeof record.args_digest === "object" ? record.args_digest : null;
  if (!digest) {
    digestSection.appendChild(el("div", "interlink-flag", t("empty")));
  } else {
    const factsRow = el("div", "interlink-facts");
    factsRow.appendChild(fact(t("colKind"), String(digest.kind || record?.kind || "-")));
    factsRow.appendChild(fact("level", String(digest.level || "-")));
    factsRow.appendChild(
      fact(t("digestHash"), String(digest.sha256 || "-").slice(0, 16))
    );
    digestSection.appendChild(factsRow);
    const fields = pickFields(digest, 12);
    if (fields.length) {
      const table = el("table", "interlink-mini-table");
      const head = el("tr");
      head.appendChild(el("th", "", t("digestFields")));
      head.appendChild(el("th", "", ""));
      const thead = el("thead");
      thead.appendChild(head);
      const tbodyEl = el("tbody");
      fields.forEach(([key, value]) => {
        const row = el("tr");
        row.appendChild(el("td", "interlink-mono", key));
        row.appendChild(el("td", "", value));
        tbodyEl.appendChild(row);
      });
      table.appendChild(thead);
      table.appendChild(tbodyEl);
      digestSection.appendChild(table);
    }
    digestSection.appendChild(el("div", "interlink-flag", t("noBodyHint")));
  }
  body.appendChild(digestSection);
};

export { normalizeDevice, normalizeAggregates, heatLevel, clientFamily };
