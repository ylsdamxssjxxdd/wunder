// 舰桥「互通舰队」- 互通告警与运行时（docs §9.4 告警 / §5.2 治理面）
//
// 数据（全部只读）：
//   GET /wunder/admin/interlink/runtime            告警计数 + 运行时快照（404＝契约未就绪）
//   GET /wunder/admin/interlink/audit?action=...   告警行，固定默认 alert.raised，offset/limit 有界
//
// 纪律：轮询由宿主（interlink.js）驱动，本模块只暴露 refresh()；列表只渲染一页；
// 运行时数组只取长度与分位，不做全量渲染；加载失败 / 404 / 空数据三态分开呈现。

import { formatBytes } from "./utils.js";
import {
  AUDIT_ACTIONS,
  CSV_MAX_ROWS_FALLBACK,
  adminGet,
  badge,
  buildQuery,
  clearNode,
  contract,
  createPager,
  downloadCsv,
  drawer,
  el,
  formatClock,
  formatRelative,
  formatRtt,
  t,
  triggerLabel,
} from "./interlink-shared.js";

export const ALERT_ACTION = "alert.raised";
const LIST_PAGE_SIZE = 25;
const DETAIL_FIELDS_MAX = 12;
const DEVICE_NODE_PREFIX = "device:";

const toNumber = (value) => {
  const num = Number(value);
  return Number.isFinite(num) ? num : null;
};

const toCount = (value) => {
  const num = Number(value);
  return Number.isFinite(num) ? num : 0;
};

// 审计行的设备端描述是 `device:<id>`，云端端描述不是设备；只有带前缀的那一端才算设备。
const deviceFromNodes = (...nodes) => {
  for (const node of nodes) {
    const text = String(node || "");
    if (text.startsWith(DEVICE_NODE_PREFIX)) {
      return text.slice(DEVICE_NODE_PREFIX.length).trim();
    }
  }
  return "";
};

const normalizeAlert = (item) => {
  const detail =
    item?.detail_digest && typeof item.detail_digest === "object" && !Array.isArray(item.detail_digest)
      ? item.detail_digest
      : {};
  return {
    seq: toCount(item?.seq),
    action: String(item?.action || "").trim(),
    created_at: toNumber(item?.created_at),
    actor: String(item?.actor || "").trim(),
    command_id: String(item?.command_id || "").trim(),
    device_id:
      String(detail.device_id || "").trim() ||
      deviceFromNodes(item?.to_node, item?.from_node),
    // The alert's own audit row keeps the account in `actor`; `user_id` only
    // ever appears on a webhook payload, so an alert read straight from the
    // trail would otherwise show an empty owner.
    user_id: String(detail.user_id || item?.actor || "").trim(),
    trigger: String(detail.trigger || "").trim(),
    kind: String(detail.kind || "").trim(),
    level: String(detail.level || "").trim(),
    result: String(detail.result || "").trim(),
    count: toCount(detail.count),
    detail,
  };
};

const normalizeRuntime = (data) => ({
  live_channels: toCount(data?.live_channels),
  rtt_p50_ms: toNumber(data?.rtt_p50_ms),
  rtt_p95_ms: toNumber(data?.rtt_p95_ms),
  channel_rows: Array.isArray(data?.channels) ? data.channels.length : 0,
  watched_threads: Array.isArray(data?.watched_threads) ? data.watched_threads.length : 0,
  blob_cache_bytes: toCount(data?.blob_cache_bytes),
  open_streams: toCount(data?.open_streams),
  commands: data?.commands && typeof data.commands === "object" ? data.commands : null,
  alerts: {
    raised: toCount(data?.alerts?.raised),
    dropped: toCount(data?.alerts?.dropped),
    delivered: toCount(data?.alerts?.delivered),
    webhook_failures: toCount(data?.alerts?.webhook_failures),
    queue_capacity: toCount(data?.alerts?.queue_capacity),
    tracked: toCount(data?.alerts?.tracked),
    pump_running: data?.alerts?.pump_running === true,
    pump_known: typeof data?.alerts?.pump_running === "boolean",
  },
});

const emptyRuntime = () => normalizeRuntime(null);

export const alertsView = {
  id: "alerts",
  pollIntervalMs: 20000,

  state: {
    page: 1,
    pageSize: LIST_PAGE_SIZE,
    total: 0,
    filters: { action: ALERT_ACTION, device_id: "" },
    rows: [],
    runtime: emptyRuntime(),
    // loading | ready | missing | error
    runtimeState: "loading",
    runtimeMessage: "",
    listState: "loading",
    listMessage: "",
    loading: false,
    pendingReload: false,
    loaded: false,
    exporting: false,
  },

  dom: null,
  pager: null,

  // -------------------------------------------------------------------------
  mount(root) {
    clearNode(root);
    const wrap = el("section", "interlink-view interlink-view-alerts");
    wrap.appendChild(el("h2", "interlink-block-title", t("alertsTitle")));
    wrap.appendChild(el("div", "interlink-flag", t("alertsTip")));

    const notice = el("div", "interlink-notice is-hidden");
    wrap.appendChild(notice);

    // 告警计数
    const counters = el("section", "interlink-block");
    const countersTitle = el("h3", "interlink-block-title", t("alertCountersTitle"));
    countersTitle.appendChild(el("span", "interlink-block-hint", t("alertCountersHint")));
    const counterBar = el("div", "interlink-summary");
    const pumpLine = el("div", "interlink-line");
    counters.appendChild(countersTitle);
    counters.appendChild(counterBar);
    counters.appendChild(pumpLine);
    wrap.appendChild(counters);

    // 运行时指标
    const runtime = el("section", "interlink-block");
    runtime.appendChild(el("h3", "interlink-block-title", t("alertRuntimeTitle")));
    const metricRow = el("div", "interlink-block-row");
    const metricState = el("div", "interlink-state is-hidden");
    runtime.appendChild(metricRow);
    runtime.appendChild(metricState);
    wrap.appendChild(runtime);

    // 最近告警
    const list = el("section", "interlink-block");
    const listTitle = el("h3", "interlink-block-title", t("alertListTitle"));
    listTitle.appendChild(el("span", "interlink-block-hint", t("alertListHint")));
    const bar = el("div", "cloud-filter-bar");

    const actionSelect = el("select");
    actionSelect.setAttribute("aria-label", t("alertFilterAction"));
    const allOption = el("option", "", t("filterAll"));
    allOption.value = "";
    actionSelect.appendChild(allOption);
    AUDIT_ACTIONS.forEach((action) => {
      const option = el("option", "", action);
      option.value = action;
      actionSelect.appendChild(option);
    });
    const actionBox = el("div", "header-input");
    actionBox.appendChild(el("label", "", t("alertFilterAction")));
    actionBox.appendChild(actionSelect);

    const deviceInput = el("input");
    deviceInput.type = "text";
    deviceInput.placeholder = t("filterDeviceHint");
    deviceInput.setAttribute("aria-label", t("alertFilterDevice"));
    const deviceBox = el("div", "header-input");
    deviceBox.appendChild(el("label", "", t("alertFilterDevice")));
    deviceBox.appendChild(deviceInput);

    const resetBtn = el("button", "secondary btn-with-icon btn-compact");
    resetBtn.type = "button";
    resetBtn.appendChild(el("i", "fa-solid fa-rotate-left"));
    resetBtn.appendChild(el("span", "", t("filterReset")));

    const csvBtn = el("button", "secondary btn-with-icon btn-compact");
    csvBtn.type = "button";
    csvBtn.appendChild(el("i", "fa-solid fa-file-arrow-down"));
    csvBtn.appendChild(el("span", "", t("csvExport")));

    bar.appendChild(actionBox);
    bar.appendChild(deviceBox);
    bar.appendChild(resetBtn);
    bar.appendChild(csvBtn);

    const table = el("table", "interlink-mini-table interlink-alert-table");
    const thead = el("thead");
    const headRow = el("tr");
    [
      t("colTime"),
      t("alertColTrigger"),
      t("alertColDevice"),
      t("alertColKind"),
      t("alertColResult"),
      t("alertColCount"),
      t("colActions"),
    ].forEach((label) => headRow.appendChild(el("th", "", label)));
    thead.appendChild(headRow);
    const tbody = el("tbody");
    table.appendChild(thead);
    table.appendChild(tbody);

    const stateLine = el("div", "interlink-state", t("loading"));
    const pagerRoot = el("div", "interlink-pager");
    list.appendChild(listTitle);
    list.appendChild(bar);
    list.appendChild(table);
    list.appendChild(stateLine);
    list.appendChild(pagerRoot);
    wrap.appendChild(list);

    root.appendChild(wrap);

    this.dom = { notice, counterBar, pumpLine, metricRow, metricState, actionSelect, deviceInput, resetBtn, csvBtn, tbody, stateLine };

    actionSelect.value = this.state.filters.action;
    deviceInput.value = this.state.filters.device_id;

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
      this.state.filters.action = actionSelect.value;
      this.state.filters.device_id = deviceInput.value.trim();
      this.applyFilters();
    };
    actionSelect.addEventListener("change", applyFilters);
    deviceInput.addEventListener("keydown", (event) => {
      if (event.key === "Enter") {
        applyFilters();
      }
    });
    resetBtn.addEventListener("click", () => {
      actionSelect.value = ALERT_ACTION;
      deviceInput.value = "";
      applyFilters();
    });
    csvBtn.addEventListener("click", () => {
      this.exportCsv().catch(() => {});
    });

    this.renderRuntime();
    this.renderRows();
    return wrap;
  },

  unmount() {
    drawer.close();
    this.dom = null;
    this.pager = null;
    this.state.rows = [];
    this.state.runtime = emptyRuntime();
    this.state.runtimeState = "loading";
    this.state.listState = "loading";
  },

  syncFilterDom() {
    if (!this.dom) {
      return;
    }
    this.dom.actionSelect.value = this.state.filters.action;
    this.dom.deviceInput.value = this.state.filters.device_id;
  },

  // 筛选/跳转改变了结果集，分页器必须回到第 1 页（offset 由分页器持有）
  resetPager() {
    this.state.page = 1;
    this.pager?.setMeta({ page: 1, pageSize: this.state.pageSize, total: this.state.total });
  },

  applyFilters() {
    this.resetPager();
    this.refresh().catch(() => {});
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
      this.state.runtimeState = "error";
      this.state.runtimeMessage = t("contractUnavailable", { reason: contract.reason });
      this.state.listState = "error";
      this.state.listMessage = this.state.runtimeMessage;
      this.renderRuntime();
      this.renderRows();
      return;
    }
    this.showNotice("", false);
    this.state.loading = true;
    this.setBusy(true);
    try {
      await Promise.all([this.loadRuntime(), this.loadAlerts()]);
    } finally {
      this.state.loading = false;
      this.setBusy(false);
      if (this.state.pendingReload) {
        this.state.pendingReload = false;
        this.refresh().catch(() => {});
      }
    }
  },

  async loadRuntime() {
    if (this.state.runtimeState === "loading") {
      this.renderRuntime();
    }
    try {
      const data = await adminGet("/runtime", "", { missingOk: true });
      this.state.runtime = normalizeRuntime(data);
      this.state.runtimeState = "ready";
      this.state.runtimeMessage = "";
    } catch (error) {
      this.state.runtime = emptyRuntime();
      if (error?.endpointMissing) {
        this.state.runtimeState = "missing";
        this.state.runtimeMessage = error.message;
      } else {
        this.state.runtimeState = "error";
        this.state.runtimeMessage = error?.message || String(error);
      }
    }
    this.renderRuntime();
  },

  async loadAlerts() {
    const query = buildQuery({
      offset: this.pager ? this.pager.offset() : 0,
      limit: this.state.pageSize,
      action: this.state.filters.action,
      device_id: this.state.filters.device_id,
    });
    try {
      const data = await adminGet("/audit", query);
      const events = Array.isArray(data.events) ? data.events : [];
      this.state.rows = events.map(normalizeAlert);
      this.state.total = toCount(data.total);
      this.state.loaded = true;
      this.state.listState = "ready";
      this.state.listMessage = "";
    } catch (error) {
      this.state.rows = [];
      if (error?.contractBlocked) {
        this.showNotice(t("contractUnavailable", { reason: contract.reason }), true);
      }
      this.state.listState = "error";
      this.state.listMessage = error?.message || String(error);
    }
    this.renderRows();
  },

  async exportCsv() {
    if (!this.dom || this.state.exporting) {
      return;
    }
    // CSV 是单有界页：上限取共享层常量，由服务端 CSV_PAGE_MAX 再收敛一次。
    const query = buildQuery({
      offset: 0,
      limit: CSV_MAX_ROWS_FALLBACK,
      action: this.state.filters.action,
      device_id: this.state.filters.device_id,
      format: "csv",
    });
    this.state.exporting = true;
    this.dom.csvBtn.disabled = true;
    try {
      const result = await downloadCsv(query);
      this.setListMessage(
        result.ok ? t("csvDone", { rows: result.rows }) : t("csvFailed", { message: result.message }),
        result.ok ? "info" : "error"
      );
    } catch (error) {
      this.setListMessage(t("csvFailed", { message: error?.message || String(error) }), "error");
    } finally {
      this.state.exporting = false;
      if (this.dom.csvBtn) {
        this.dom.csvBtn.disabled = false;
      }
    }
  },

  // -------------------------------------------------------------------------
  setBusy(busy) {
    if (!this.dom) {
      return;
    }
    this.dom.actionSelect.disabled = busy;
    this.dom.deviceInput.disabled = busy;
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
    notice.classList.toggle("is-error", Boolean(isError));
  },

  setListMessage(text, tone) {
    if (!this.dom) {
      return;
    }
    this.dom.stateLine.textContent = text;
    this.dom.stateLine.classList.remove("is-error", "is-empty", "is-hidden");
    if (tone === "error") {
      this.dom.stateLine.classList.add("is-error");
    } else if (tone === "empty") {
      this.dom.stateLine.classList.add("is-empty");
    }
  },

  // -------------------------------------------------------------------------
  renderRuntime() {
    if (!this.dom) {
      return;
    }
    const { runtime, runtimeState, runtimeMessage } = this.state;
    const counters = this.dom.counterBar;
    const pumpLine = this.dom.pumpLine;
    const metricRow = this.dom.metricRow;
    clearNode(counters);
    clearNode(pumpLine);
    clearNode(metricRow);

    if (runtimeState === "loading") {
      metricRow.appendChild(el("span", "interlink-state", t("runtimeStateLoading")));
      this.dom.metricState.classList.remove("is-hidden");
      this.dom.metricState.classList.remove("is-error");
      this.dom.metricState.textContent = "";
      return;
    }
    if (runtimeState === "missing" || runtimeState === "error") {
      const state = this.dom.metricState;
      state.classList.remove("is-hidden");
      state.classList.toggle("is-error", runtimeState === "error");
      state.classList.toggle("is-empty", runtimeState !== "error");
      state.textContent = runtimeMessage || t("runtimeStateFailed", { message: t("unknownError") });
      // 计数与指标都来自同一端点：契约未就绪时显示空态，绝不伪造 0。
      counters.appendChild(el("div", "interlink-flag", t("empty")));
      metricRow.appendChild(el("div", "interlink-flag", t("empty")));
      return;
    }
    this.dom.metricState.classList.add("is-hidden");

    const alerts = runtime.alerts;
    const stat = (label, value, tone, sub) => {
      const item = el("div", `interlink-stat${tone ? ` is-${tone}` : ""}`);
      item.appendChild(el("span", "interlink-stat-label", label));
      item.appendChild(el("span", "interlink-stat-value", String(value)));
      if (sub) {
        item.appendChild(el("span", "interlink-stat-sub", sub));
      }
      return item;
    };
    counters.appendChild(stat(t("statRaised"), alerts.raised));
    counters.appendChild(stat(t("statDropped"), alerts.dropped, alerts.dropped > 0 ? "danger" : ""));
    counters.appendChild(stat(t("statDelivered"), alerts.delivered));
    counters.appendChild(
      stat(t("statWebhookFailures"), alerts.webhook_failures, alerts.webhook_failures > 0 ? "danger" : "")
    );
    counters.appendChild(stat(t("statTracked"), alerts.tracked));
    counters.appendChild(stat(t("statQueueCapacity"), alerts.queue_capacity));
    counters.appendChild(
      stat(
        t("statPump"),
        alerts.pump_known ? (alerts.pump_running ? t("pumpRunning") : t("pumpStopped")) : "-",
        !alerts.pump_running ? "warn" : ""
      )
    );

    if (!alerts.pump_running) {
      const warn = el("span", "monitor-status interlink-disabled", t("pumpStoppedWarn"));
      pumpLine.appendChild(warn);
    } else {
      pumpLine.appendChild(badge(t("pumpRunning"), "interlink-live"));
    }
    if (alerts.webhook_failures > 0) {
      pumpLine.appendChild(
        el("span", "monitor-status interlink-revoked", t("webhookFailureWarn", { count: alerts.webhook_failures }))
      );
    }

    const pair = (label, value, title) => {
      const node = el("div", "interlink-pair");
      node.appendChild(el("span", "", label));
      const strong = el("b", "", value);
      if (title) {
        strong.title = title;
      }
      node.appendChild(strong);
      return node;
    };
    metricRow.appendChild(pair(t("metricLiveChannels"), String(runtime.live_channels)));
    metricRow.appendChild(
      pair(
        t("metricRtt"),
        `${runtime.rtt_p50_ms === null ? "-" : formatRtt(runtime.rtt_p50_ms)} / ${
          runtime.rtt_p95_ms === null ? "-" : formatRtt(runtime.rtt_p95_ms)
        }`,
        runtime.rtt_p50_ms === null ? t("noSample") : ""
      )
    );
    metricRow.appendChild(pair(t("metricBlobCache"), formatBytes(runtime.blob_cache_bytes)));
    metricRow.appendChild(pair(t("metricOpenStreams"), String(runtime.open_streams)));
    metricRow.appendChild(pair(t("metricWatchedThreads"), String(runtime.watched_threads)));
    metricRow.appendChild(pair(t("metricChannels"), String(runtime.channel_rows)));
    if (runtime.commands) {
      metricRow.appendChild(
        pair(t("metricCommandStats"), `${toCount(runtime.commands.inflight_total)} / ${toCount(runtime.commands.queued_total)}`)
      );
    }
  },

  renderRows() {
    if (!this.dom) {
      return;
    }
    const tbody = this.dom.tbody;
    clearNode(tbody);
    if (this.state.listState === "loading") {
      this.setListMessage(t("loading"), "info");
      this.pager?.render(true);
      return;
    }
    if (this.state.listState === "error") {
      this.setListMessage(this.state.listMessage || t("unknownError"), "error");
      this.pager?.render(false);
      return;
    }
    if (!this.state.rows.length) {
      this.setListMessage(t("empty"), "empty");
      this.pager?.render(false);
      return;
    }
    this.dom.stateLine.className = "interlink-state is-hidden";
    const fragment = document.createDocumentFragment();
    this.state.rows.forEach((row) => fragment.appendChild(this.renderRow(row)));
    tbody.appendChild(fragment);
    this.pager?.setMeta({
      page: this.pager.state.page,
      pageSize: this.state.pageSize,
      total: this.state.total,
    });
    this.pager?.render(false);
  },

  renderRow(row) {
    const tr = el("tr");
    tr.tabIndex = 0;

    const timeCell = el("td", "", row.created_at ? formatRelative(row.created_at) : "-");
    timeCell.title = row.created_at ? formatClock(row.created_at) : "";
    tr.appendChild(timeCell);

    const triggerCell = el("td", "");
    triggerCell.appendChild(
      badge(
        row.trigger ? `${triggerLabel(row.trigger)}` : t("alertTriggerUnknown"),
        row.trigger === "l3_execution" ? "interlink-revoked" : "interlink-disabled",
        row.trigger || ""
      )
    );
    tr.appendChild(triggerCell);

    const deviceCell = el("td", "interlink-cell-sub", row.device_id || "-");
    deviceCell.title = row.device_id || t("alertNoDevice");
    tr.appendChild(deviceCell);

    tr.appendChild(
      el(
        "td",
        "interlink-mono",
        [row.kind, row.level].filter(Boolean).join(" · ") || "-"
      )
    );
    tr.appendChild(el("td", "interlink-mono", row.result || "-"));
    tr.appendChild(el("td", "", String(row.count || 0)));

    const actionCell = el("td", "interlink-cell-actions");
    const detailBtn = el("button", "secondary", t("detail"));
    detailBtn.type = "button";
    detailBtn.addEventListener("click", (event) => {
      event.stopPropagation();
      this.openAlertDrawer(row);
    });
    actionCell.appendChild(detailBtn);
    tr.appendChild(actionCell);

    tr.addEventListener("click", () => this.openAlertDrawer(row));
    tr.addEventListener("keydown", (event) => {
      if (event.key === "Enter") {
        this.openAlertDrawer(row);
      }
    });
    return tr;
  },

  // 复用共享抽屉（同一实例），只补本面板需要的告警行事实与筛选跳转。
  openAlertDrawer(row) {
    drawer.open(t("alertDrawerTitle"), `${row.action || ALERT_ACTION} · ${t("alertColSeq")} ${row.seq}`, (body) =>
      this.renderAlertDetail(body, row)
    );
  },

  renderAlertDetail(body, row) {
    const facts = el("div", "interlink-facts");
    const fact = (label, value, title) => {
      const item = el("div", "interlink-fact");
      item.appendChild(el("span", "", label));
      const strong = el("b", "", value || "-");
      if (title) {
        strong.title = title;
      }
      item.appendChild(strong);
      return item;
    };
    facts.appendChild(fact(t("colTime"), row.created_at ? formatClock(row.created_at) : "-"));
    facts.appendChild(fact(t("alertColTrigger"), row.trigger, row.trigger));
    facts.appendChild(fact(t("alertColDevice"), row.device_id));
    facts.appendChild(fact(t("colUser"), row.user_id));
    facts.appendChild(fact(t("colKind"), row.kind));
    facts.appendChild(fact(t("alertColLevel"), row.level));
    facts.appendChild(fact(t("alertColResult"), row.result));
    facts.appendChild(fact(t("alertColCount"), String(row.count || 0)));
    facts.appendChild(fact(t("alertColActor"), row.actor));
    facts.appendChild(fact(t("alertColCommand"), row.command_id));
    facts.appendChild(fact(t("alertColSeq"), String(row.seq)));
    body.appendChild(facts);

    const entries = Object.entries(row.detail || {}).slice(0, DETAIL_FIELDS_MAX);
    if (entries.length) {
      const table = el("table", "interlink-mini-table");
      const thead = el("thead");
      const head = el("tr");
      head.appendChild(el("th", "", t("digestFields")));
      head.appendChild(el("th", "", ""));
      thead.appendChild(head);
      const tbody = el("tbody");
      entries.forEach(([key, value]) => {
        const tr = el("tr");
        tr.appendChild(el("td", "interlink-mono", key));
        tr.appendChild(el("td", "interlink-mono", typeof value === "object" ? JSON.stringify(value) : String(value)));
        tbody.appendChild(tr);
      });
      table.appendChild(thead);
      table.appendChild(tbody);
      body.appendChild(table);
    }

    body.appendChild(el("div", "interlink-flag", t("alertDrawerHint")));

    const jumpLine = el("div", "interlink-line");
    if (row.device_id) {
      const jump = el("button", "secondary", t("alertFilterThisDevice"));
      jump.type = "button";
      jump.addEventListener("click", () => {
        this.state.filters.device_id = row.device_id;
        this.state.filters.action = ALERT_ACTION;
        this.syncFilterDom();
        drawer.close();
        this.applyFilters();
      });
      jumpLine.appendChild(jump);
    } else {
      jumpLine.appendChild(el("span", "interlink-flag", t("alertNoDevice")));
    }
    body.appendChild(jumpLine);

    const elapsed = row.created_at ? formatRelative(row.created_at) : "-";
    body.appendChild(el("div", "interlink-flag", `${t("colSeen")}: ${elapsed}`));
  },
};

export { normalizeAlert, normalizeRuntime };
