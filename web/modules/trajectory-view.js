// 线程轨迹视图（对齐蜂巢 ThreadTrajectoryView.vue）：
// 工具栏 + 三泳道时间线 + 台账表格（虚拟滚动）+ 详情检查器。
// 纯 DOM 实现，数据由调用方注入（setRawTurns），并提供全屏「轨迹页面」弹窗。

import { elements } from "./elements.js?v=20261011-01";
import { t } from "./i18n.js?v=20261011-01";
import { getWunderBase } from "./api.js";
import { normalizeMarkdownForWebPreview, enhanceRenderedMarkdown } from "./markdown-preview.js";
import {
  attachTurnItems,
  buildRequestNumbers,
  buildTrajectoryLayout,
  collapseTurnRecords,
  createModelTranslate,
  deriveTimeline,
  flattenRecords,
  formatClock,
  formatDurationMillis,
  formatElapsedSeconds,
  formatStartedAt,
  groupVirtualRows,
  requestIdentity,
  timelineFocusIndexes,
} from "./trajectory-model.js?v=20261011-01";

const OVERSCAN_PX = 360;
const VIRTUAL_THRESHOLD = 100;
const MIN_VIEWPORT_MS = 20;
const MIN_VIEWPORT_OPS = 4;
const EDGE_PAN_FRACTION = 0.08;
const TOOLTIP_DELAY_MS = 500;

const modelT = createModelTranslate(t);

/* ---------------------------------------------------------------- */
/* 数据获取                                                          */
/* ---------------------------------------------------------------- */

/** 拉取线程日志快照（管理端点），返回挂好 items 的轮次数组。 */
export const fetchThreadLogSnapshot = async (sessionId) => {
  const endpoint = `${getWunderBase()}/admin/monitor/${encodeURIComponent(
    sessionId
  )}/thread-log/snapshot`;
  const response = await fetch(endpoint);
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  const result = await response.json();
  return attachTurnItems(result?.data ?? null);
};

/* ---------------------------------------------------------------- */
/* DOM 工具                                                          */
/* ---------------------------------------------------------------- */

const h = (tag, className, text) => {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined && text !== null) node.textContent = text;
  return node;
};

const SVG_NS = "http://www.w3.org/2000/svg";
const svgEl = (tag, attrs) => {
  const node = document.createElementNS(SVG_NS, tag);
  Object.entries(attrs).forEach(([key, value]) => node.setAttribute(key, value));
  return node;
};

const safeParseJson = (raw) => {
  if (!raw) return null;
  try {
    return JSON.parse(raw);
  } catch {
    return null;
  }
};

const renderMarkdownHtml = (raw) => {
  if (!raw) return "";
  const text = normalizeMarkdownForWebPreview(raw);
  const renderer = globalThis.marked;
  if (renderer && typeof renderer.parse === "function") {
    try {
      if (typeof renderer.setOptions === "function") {
        renderer.setOptions({ breaks: true, gfm: true });
      }
      return renderer.parse(text);
    } catch {
      return "";
    }
  }
  return "";
};

/* ---------------------------------------------------------------- */
/* JSON 树（对齐 TrajectoryJsonTree.vue）                             */
/* ---------------------------------------------------------------- */

function appendJsonTree(host, data, collapsedStringLines = 12) {
  const wrap = h("div", "tt-json-tree");
  buildJsonNode(wrap, data, null, collapsedStringLines, { open: true, stringOpen: false });
  host.appendChild(wrap);
}

function buildJsonNode(parent, data, label, collapsedStringLines, ui) {
  const isBranch = data !== null && typeof data === "object";

  if (isBranch) {
    const isArray = Array.isArray(data);
    const headRow = h("div", "tt-json-row");
    const toggle = h("button", "tt-json-toggle");
    toggle.type = "button";
    toggle.appendChild(h("span", "tt-json-punct", isArray ? "[" : "{"));
    const ellipsis = h("span", "tt-json-ellipsis", "…");
    const count = h("span", "tt-json-count", String(jsonEntries(data).length));
    const closePunct = h("span", "tt-json-punct", isArray ? "]" : "}");
    headRow.appendChild(toggle);
    headRow.appendChild(ellipsis);
    headRow.appendChild(count);
    headRow.appendChild(closePunct);
    if (label !== null) headRow.appendChild(h("span", "tt-json-label", `"${label}"`));
    parent.appendChild(headRow);

    const children = h("div", "tt-json-children");
    parent.appendChild(children);
    const closeRow = h("div", "tt-json-row");
    closeRow.appendChild(h("span", "tt-json-punct", isArray ? "]" : "}"));
    parent.appendChild(closeRow);

    const syncOpen = () => {
      children.style.display = ui.open ? "" : "none";
      closeRow.style.display = ui.open ? "" : "none";
      ellipsis.style.display = ui.open ? "none" : "";
      count.style.display = ui.open ? "none" : "";
      closePunct.style.display = ui.open ? "none" : "";
    };
    toggle.addEventListener("click", () => {
      ui.open = !ui.open;
      syncOpen();
    });
    syncOpen();

    for (const entry of jsonEntries(data)) {
      const child = h("div", "tt-json-child");
      children.appendChild(child);
      buildJsonNode(child, entry.value, entry.key, collapsedStringLines, {
        open: true,
        stringOpen: false,
      });
    }
    return;
  }

  const row = h("div", "tt-json-row");
  if (label !== null) {
    row.appendChild(h("span", "tt-json-label", `"${label}"`));
  }
  if (isLongJsonString(data, collapsedStringLines)) {
    const stringNode = h("span", "tt-json-string", `"${data}"`);
    const syncString = () => {
      stringNode.classList.toggle("tt-json-string-open", ui.stringOpen);
      stringNode.style.maxHeight = ui.stringOpen ? "none" : `${collapsedStringLines * 16}px`;
    };
    stringNode.addEventListener("click", () => {
      ui.stringOpen = !ui.stringOpen;
      syncString();
    });
    syncString();
    row.appendChild(stringNode);
  } else {
    row.appendChild(h("span", jsonValueClass(data), jsonValueText(data)));
  }
  parent.appendChild(row);
}

function jsonEntries(value) {
  if (Array.isArray(value)) {
    return value.map((item, index) => ({ key: String(index), value: item }));
  }
  if (value !== null && typeof value === "object") {
    return Object.entries(value).map(([key, item]) => ({ key, value: item }));
  }
  return [];
}

function isLongJsonString(value, collapsedStringLines) {
  if (typeof value !== "string") return false;
  return value.length > 1024 || value.split("\n").length > collapsedStringLines;
}

function jsonValueClass(value) {
  if (value === null || value === undefined) return "tt-json-keyword";
  if (typeof value === "string") return "tt-json-string";
  if (typeof value === "number") return "tt-json-number";
  if (typeof value === "boolean") return "tt-json-keyword";
  return "tt-json-punct";
}

function jsonValueText(value) {
  if (value === null) return "null";
  if (value === undefined) return "undefined";
  if (typeof value === "string") return JSON.stringify(value);
  return String(value);
}

/* ---------------------------------------------------------------- */
/* 视图工厂                                                          */
/* ---------------------------------------------------------------- */

export function createTrajectoryView(host, options = {}) {
  if (!host) {
    throw new Error("trajectory host missing");
  }
  const showBackButton = options.showBack === true;
  const onBack = typeof options.onBack === "function" ? options.onBack : null;

  /* ---------- 状态 ---------- */
  const st = {
    rawTurns: [],
    turnsKey: 0,
    loading: true,
    loadError: false,
    showDuration: true,
    showTurns: false,
    showCalls: false,
    searchQuery: "",
    selectedIndex: null,
    activeTab: "",
    detailsWidth: null,
    openSections: { input: true, output: true },
    timestampMode: "local",
    collapsedTurns: new Set(),
    collapsedAssistants: new Set(),
    scrollTop: 0,
    ledgerViewport: 360,
    trackWidth: 600,
    viewStart: 0,
    viewSpan: 0,
    hoverX: null,
    panning: false,
    dragging: false,
    selection: null,
  };

  let dragState = null;
  let tooltipTimer = null;
  let resizeState = null;
  let renderScheduled = false;
  let destroyed = false;
  let turnsMemo = null;
  let turnsMemoKey = -1;

  /* ---------- 骨架 ---------- */
  const root = h("div", "tt-root");

  const toolbar = h("header", "tt-toolbar");
  const backBtn = h("button", "tt-icon-button");
  backBtn.type = "button";
  backBtn.title = modelT("back");
  backBtn.setAttribute("aria-label", modelT("back"));
  const backSvg = svgEl("svg", { viewBox: "0 0 16 16", width: "14", height: "14", "aria-hidden": "true" });
  backSvg.appendChild(
    svgEl("path", {
      d: "M10 3.5 5.5 8l4.5 4.5",
      fill: "none",
      stroke: "currentColor",
      "stroke-width": "1.3",
      "stroke-linecap": "round",
      "stroke-linejoin": "round",
    })
  );
  backBtn.appendChild(backSvg);
  if (showBackButton) toolbar.appendChild(backBtn);

  const mkToggle = (glyph, label) => {
    const button = h("button", "tt-toggle");
    button.type = "button";
    if (glyph === "clock") {
      const svg = svgEl("svg", { viewBox: "0 0 16 16", width: "12", height: "12", "aria-hidden": "true" });
      svg.appendChild(svgEl("circle", { cx: "8", cy: "8", r: "5.25", fill: "none", stroke: "currentColor", "stroke-width": "1.25" }));
      svg.appendChild(svgEl("path", { d: "M8 4.75V8l2.25 1.5", fill: "none", stroke: "currentColor", "stroke-width": "1.25", "stroke-linecap": "round" }));
      button.appendChild(svg);
    } else {
      button.appendChild(h("span", "tt-toggle-glyph", glyph));
    }
    button.appendChild(h("span", "tt-toggle-label", label));
    return button;
  };

  const durationToggle = mkToggle("clock", modelT("duration"));
  const turnsToggle = mkToggle("⊞", modelT("turns"));
  const callsToggle = mkToggle("⊟", modelT("calls"));
  toolbar.appendChild(durationToggle);
  toolbar.appendChild(turnsToggle);
  toolbar.appendChild(callsToggle);
  toolbar.appendChild(h("div", "tt-toolbar-spacer"));

  const search = h("div", "tt-search");
  const searchSvg = svgEl("svg", { viewBox: "0 0 16 16", width: "12", height: "12", "aria-hidden": "true" });
  searchSvg.appendChild(svgEl("circle", { cx: "7", cy: "7", r: "4.4", fill: "none", stroke: "currentColor", "stroke-width": "1.25" }));
  searchSvg.appendChild(svgEl("path", { d: "m10.4 10.4 3 3", stroke: "currentColor", "stroke-width": "1.25", "stroke-linecap": "round" }));
  search.appendChild(searchSvg);
  const searchInput = h("input", "tt-search-input");
  searchInput.type = "text";
  searchInput.placeholder = modelT("searchTrajectory");
  searchInput.spellcheck = false;
  search.appendChild(searchInput);
  toolbar.appendChild(search);

  const timelineEl = h("div", "tt-timeline");
  timelineEl.style.display = "none";
  const axisEl = h("div", "tt-axis");
  const trackEl = h("div", "tt-track");
  const lane0 = h("span", "tt-lane-label tt-lane-0", modelT("laneInput"));
  const lane1 = h("span", "tt-lane-label tt-lane-1", modelT("laneModel"));
  const lane2 = h("span", "tt-lane-label tt-lane-2", modelT("laneTool"));
  trackEl.appendChild(lane0);
  trackEl.appendChild(lane1);
  trackEl.appendChild(lane2);
  const hoverlineEl = h("span", "tt-hoverline");
  hoverlineEl.style.display = "none";
  const selectionEl = h("span", "tt-selection");
  selectionEl.style.display = "none";
  selectionEl.appendChild(h("span", "tt-selection-edge tt-selection-edge-start"));
  selectionEl.appendChild(h("span", "tt-selection-edge tt-selection-edge-end"));
  trackEl.appendChild(hoverlineEl);
  trackEl.appendChild(selectionEl);
  timelineEl.appendChild(axisEl);
  timelineEl.appendChild(trackEl);

  const tooltipEl = h("div", "tt-tooltip");
  tooltipEl.style.display = "none";

  const bodyEl = h("div", "tt-body");
  const ledgerEl = h("div", "tt-ledger");
  const ledgerCanvas = h("div", "tt-ledger-canvas");
  const table = h("table", "tt-table");
  const colgroup = h("colgroup");
  colgroup.appendChild(h("col", "tt-col-event"));
  colgroup.appendChild(h("col"));
  table.appendChild(colgroup);
  const tbody = h("tbody");
  table.appendChild(tbody);
  ledgerCanvas.appendChild(table);
  ledgerEl.appendChild(ledgerCanvas);
  const emptyEl = h("div", "tt-empty");
  emptyEl.style.display = "none";
  ledgerEl.appendChild(emptyEl);
  bodyEl.appendChild(ledgerEl);

  const resizeHandle = h("div", "tt-resize-handle");
  const detailsEl = h("aside", "tt-details");

  root.appendChild(toolbar);
  root.appendChild(timelineEl);
  root.appendChild(bodyEl);
  root.appendChild(tooltipEl);
  host.textContent = "";
  host.appendChild(root);

  /* ---------- 计算管线 ---------- */

  const timelineMode = () => (st.showDuration ? "duration" : "sequence");

  const computeTurns = () => {
    if (!turnsMemo || turnsMemoKey !== st.turnsKey) {
      turnsMemo = buildTrajectoryLayout(st.rawTurns, modelT);
      turnsMemoKey = st.turnsKey;
    }
    return turnsMemo;
  };

  const findCell = (turns, index) => {
    if (index === null || index === undefined) return null;
    for (const turn of turns) {
      for (const group of turn.groups) {
        for (const cell of group.cells) {
          if (cell.index === index) return cell;
        }
      }
    }
    return null;
  };

  const needleOfRecord = (record) =>
    [
      record.cell.text,
      record.cell.toolName ?? "",
      record.cell.inputDetail ?? "",
      record.cell.outputDetail ?? "",
      record.cell.result ?? "",
    ]
      .join("\n")
      .toLowerCase();

  const kindLabels = () => ({
    system: modelT("kindSystem"),
    user: modelT("kindUser"),
    context: modelT("kindContext"),
    compacted: modelT("kindCompacted"),
    message: modelT("kindAssistant"),
    tool: modelT("kindTool"),
    subtool: modelT("kindSubtool"),
  });

  const kindLabel = (kind) => kindLabels()[kind] ?? kind;

  const padAxis2 = (value) => String(value).padStart(2, "0");

  const formatAxisDuration = (ms) => {
    const abs = Math.abs(ms);
    if (abs < 1000) return `${Math.round(abs)} ms`;
    if (abs < 60000) return `${(abs / 1000).toFixed(abs < 10000 ? 1 : 0)} s`;
    if (abs < 3600000) {
      const minutes = Math.floor(abs / 60000);
      const seconds = Math.round((abs % 60000) / 1000);
      return seconds > 0 ? `${minutes}m ${padAxis2(seconds)}s` : `${minutes}m`;
    }
    const hours = Math.floor(abs / 3600000);
    const minutes = Math.round((abs % 3600000) / 60000);
    return `${hours}h ${padAxis2(minutes)}m`;
  };

  /* ---------- 渲染 ---------- */

  const render = () => {
    if (destroyed) return;
    const turns = computeTurns();
    const requests = buildRequestNumbers(turns);
    const allRecords = flattenRecords(turns);
    const needle = st.searchQuery.trim().toLowerCase();
    const matched = new Set();
    if (needle) {
      for (const record of allRecords) {
        if (needleOfRecord(record).includes(needle)) matched.add(record.cell.index);
      }
    }
    const filteredRecords = needle
      ? allRecords.filter((record) => needleOfRecord(record).includes(needle))
      : allRecords;
    const records = collapseTurnRecords(
      filteredRecords,
      st.collapsedTurns,
      st.collapsedAssistants,
      modelT
    );
    const rows = groupVirtualRows(records).rows;
    const prefix = [0];
    for (const row of rows) prefix.push(prefix[prefix.length - 1] + row.height);
    const ledgerHeight = prefix[prefix.length - 1];
    const virtual = ledgerHeight > VIRTUAL_THRESHOLD;
    let winStart = 0;
    let winEnd = rows.length;
    if (virtual) {
      const top = Math.max(0, st.scrollTop - OVERSCAN_PX);
      const bottom = st.scrollTop + st.ledgerViewport + OVERSCAN_PX;
      while (winStart < rows.length && prefix[winStart + 1] <= top) winStart += 1;
      winEnd = winStart;
      while (winEnd < rows.length && prefix[winEnd] < bottom) winEnd += 1;
    }
    const timeline = deriveTimeline(turns, timelineMode());
    const domain = timeline
      ? { start: timeline.start, end: Math.max(timeline.end, timeline.start + 1) }
      : { start: 0, end: 1 };
    // 对齐蜂巢 watch(timeline)：时间线重建（数据刷新或模式切换）后回到全域视图。
    if (timeline && st.viewSpan <= 0) {
      st.viewStart = timeline.start;
      st.viewSpan = Math.max(1, timeline.end - timeline.start);
    }

    const selectedCell = st.selectedIndex === null ? null : findCell(turns, st.selectedIndex);
    const selectedRecord = selectedCell
      ? allRecords.find((record) => record.cell.index === selectedCell.index) ?? null
      : null;
    const focusSet = st.selection
      ? timelineFocusIndexes(turns, st.selection, timelineMode())
      : null;

    renderToolbar();
    renderTimeline(timeline, domain, matched, focusSet);
    renderLedger({
      rows,
      winStart,
      winEnd,
      topPx: virtual ? prefix[winStart] ?? 0 : 0,
      ledgerHeight,
      records,
      requests,
      focusSet,
    });
    renderDetails(selectedCell, selectedRecord, requests, allRecords);
  };

  const scheduleRender = () => {
    if (renderScheduled || destroyed) return;
    renderScheduled = true;
    requestAnimationFrame(() => {
      renderScheduled = false;
      render();
    });
  };

  const renderToolbar = () => {
    durationToggle.classList.toggle("tt-toggle-active", st.showDuration);
    durationToggle.setAttribute("aria-pressed", st.showDuration ? "true" : "false");
    turnsToggle.classList.toggle("tt-toggle-active", st.showTurns);
    turnsToggle.setAttribute("aria-pressed", st.showTurns ? "true" : "false");
    callsToggle.classList.toggle("tt-toggle-active", st.showCalls);
    callsToggle.setAttribute("aria-pressed", st.showCalls ? "true" : "false");
  };

  const clampViewport = (start, span, domain) => {
    const domainSpan = Math.max(1e-9, domain.end - domain.start);
    const minSpanValue = timelineMode() === "sequence" ? MIN_VIEWPORT_OPS : MIN_VIEWPORT_MS;
    const nextSpan = Math.min(Math.max(span, minSpanValue), domainSpan);
    const maxStart = domain.end - nextSpan;
    return {
      start: Math.min(Math.max(start, domain.start), Math.max(domain.start, maxStart)),
      span: nextSpan,
    };
  };

  const domainOf = () => {
    const timeline = deriveTimeline(computeTurns(), timelineMode());
    if (!timeline) return { start: 0, end: 1 };
    return { start: timeline.start, end: Math.max(timeline.end, timeline.start + 1) };
  };

  const setViewport = (start, span) => {
    const clamped = clampViewport(start, span, domainOf());
    st.viewStart = clamped.start;
    st.viewSpan = clamped.span;
  };

  const renderTimeline = (timeline, domain, matched, focusSet) => {
    timelineEl.style.display = timeline ? "" : "none";
    timelineEl.classList.toggle("tt-panning", st.panning);
    timelineEl.classList.toggle("tt-dragging", st.dragging);
    if (!timeline) return;

    const width = st.trackWidth || 600;
    const span = Math.max(1e-9, st.viewSpan);
    const needle = st.searchQuery.trim().toLowerCase();

    axisEl.textContent = "";
    if (width > 0) {
      const targetCount = Math.max(2, Math.floor(width / 120));
      const raw = span / targetCount;
      const magnitude = 10 ** Math.floor(Math.log10(raw));
      const residual = raw / magnitude;
      const step = (residual >= 5 ? 5 : residual >= 2 ? 2 : 1) * magnitude;
      const timed = timelineMode() !== "sequence";
      const first = Math.ceil(st.viewStart / step) * step;
      for (let value = first; value <= st.viewStart + span; value += step) {
        const tick = h("span", "tt-axis-tick");
        tick.style.left = `${((value - st.viewStart) / span) * width}px`;
        tick.textContent = timed
          ? formatAxisDuration(value - domain.start)
          : String(Math.round(value));
        axisEl.appendChild(tick);
      }
    }

    trackEl.querySelectorAll(".tt-span, .tt-turn-boundary").forEach((node) => node.remove());

    if (st.showTurns) {
      for (const boundary of timeline.turnBoundaries) {
        const x = ((boundary.time - st.viewStart) / span) * width;
        if (x < 0 || x > width) continue;
        const node = h("span", "tt-turn-boundary");
        node.style.left = `${x}px`;
        trackEl.appendChild(node);
      }
    }

    for (let i = 0; i < timeline.spans.length; i += 1) {
      const modelSpan = timeline.spans[i];
      const left = ((modelSpan.start - st.viewStart) / span) * width;
      const pw = ((modelSpan.end - modelSpan.start) / span) * width;
      const gap = Math.min(pw * 0.08, 1);
      const spanLeft = left + gap;
      const spanWidth = Math.max(2, pw - gap * 2);
      if (spanLeft + spanWidth <= 0 || spanLeft >= width) continue;
      const node = h("span", "tt-span");
      node.classList.add(`tt-span-${modelSpan.kind}`);
      if (modelSpan.isError) node.classList.add("tt-span-error");
      if (needle && !matched.has(modelSpan.index)) node.classList.add("tt-span-dimmed");
      if (focusSet && !focusSet.has(modelSpan.index)) node.classList.add("tt-span-outside");
      node.style.left = `${spanLeft}px`;
      node.style.width = `${spanWidth}px`;
      node.style.top = `${modelSpan.lane * 14}px`;
      if (modelSpan.kind === "message" && modelSpan.ttftFraction !== null && spanWidth > 6) {
        const split = Math.round(spanWidth * modelSpan.ttftFraction);
        node.style.background = `linear-gradient(90deg, rgba(65,118,230,0.92) ${split}px, rgba(65,118,230,0.45) ${split}px)`;
      }
      node.addEventListener("pointerdown", (event) => event.stopPropagation());
      node.addEventListener("pointerenter", (event) => onSpanEnter(modelSpan, event));
      node.addEventListener("pointerleave", onSpanLeave);
      node.addEventListener("click", (event) => {
        event.stopPropagation();
        selectIndex(modelSpan.index);
      });
      trackEl.appendChild(node);
    }

    if (st.hoverX !== null) {
      hoverlineEl.style.display = "";
      hoverlineEl.style.left = `${st.hoverX}px`;
    } else {
      hoverlineEl.style.display = "none";
    }
    if (st.selection) {
      const left = ((st.selection.start - st.viewStart) / span) * width;
      const right = ((st.selection.end - st.viewStart) / span) * width;
      selectionEl.style.display = "";
      selectionEl.style.left = `${Math.max(0, left)}px`;
      selectionEl.style.width = `${Math.max(2, right - left)}px`;
    } else {
      selectionEl.style.display = "none";
    }
  };

  const renderLedger = ({ rows, winStart, winEnd, topPx, ledgerHeight, records, requests, focusSet }) => {
    ledgerCanvas.style.height = `${ledgerHeight}px`;
    table.style.transform = `translateY(${topPx}px)`;
    tbody.textContent = "";

    // 请求编号圆点挂在助手消息行上；同轮多个请求按序横向错开。
    const dotsByAssistantIndex = new Map();
    const perTurn = new Map();
    for (const request of requests) {
      const count = perTurn.get(request.turn) ?? 0;
      perTurn.set(request.turn, count + 1);
      dotsByAssistantIndex.set(request.assistantIndex, {
        request,
        left: 12 + (count % 4) * 8,
      });
    }

    for (let i = winStart; i < winEnd; i += 1) {
      const row = rows[i];
      if (!row) continue;
      const record = row.record;
      const tr = h("tr", "tt-row");
      tr.classList.add(`tt-row-${record.cell.kind}`);
      if (st.selectedIndex !== null && st.selectedIndex === record.cell.index) {
        tr.classList.add("tt-row-selected");
      }
      if (record.turnStart) tr.classList.add("tt-row-turn-start");
      if (record.turnEnd) tr.classList.add("tt-row-turn-end");
      if (record.collapsedSummary) tr.classList.add("tt-row-summary");
      if (record.cell.isError === true) tr.classList.add("tt-row-error");
      if (focusSet) {
        tr.dataset.timelineFocus = focusSet.has(record.cell.index) ? "inside" : "outside";
      }
      tr.dataset.recordIndex = String(record.cell.index);
      tr.addEventListener("click", () => selectIndex(record.cell.index));
      tr.addEventListener("dblclick", (event) => {
        event.stopPropagation();
        onRowDoubleClick(record);
      });

      const eventCell = h("td", "tt-event-cell");
      const dot = dotsByAssistantIndex.get(record.cell.index);
      if (dot) {
        const dotBtn = h("button", "tt-request-dot");
        dotBtn.type = "button";
        dotBtn.style.left = `${dot.left}px`;
        dotBtn.title = modelT("requestLabel", { request: dot.request.number });
        dotBtn.appendChild(
          h("span", "tt-request-dot-label", modelT("requestLabel", { request: dot.request.number }))
        );
        dotBtn.addEventListener("click", (event) => {
          event.stopPropagation();
          selectIndex(dot.request.assistantIndex);
        });
        eventCell.appendChild(dotBtn);
      }
      if (record.turnStart && record.turn !== null) {
        eventCell.appendChild(h("span", "tt-turn-label", modelT("turnLabel", { turn: record.turn })));
      }
      tr.appendChild(eventCell);

      const contentCell = h("td", "tt-content-cell");
      if (record.collapsedSummary) {
        contentCell.appendChild(
          h(
            "div",
            `tt-collapsed-summary tt-collapsed-${record.collapsedSummaryKind}`,
            record.collapsedSummary
          )
        );
      } else {
        const kindSlot = h("div", "tt-kind-slot");
        kindSlot.appendChild(
          h("span", `tt-kind-tag tt-kind-${record.cell.kind}`, kindLabel(record.cell.kind))
        );
        contentCell.appendChild(kindSlot);

        const content = h("div", "tt-record-content");
        const cell = record.cell;
        if (cell.kind === "tool" || cell.kind === "subtool") {
          content.appendChild(h("span", "tt-tool-name", cell.text));
          const args = argsPreview(cell);
          if (args) content.appendChild(h("span", "tt-tool-args", args));
          if (cell.resultPreviewMarkdown) {
            const resultNode = h("span", "tt-tool-result");
            resultNode.appendChild(h("span", "tt-tool-result-arrow", "→"));
            resultNode.appendChild(document.createTextNode(cell.resultPreviewMarkdown));
            content.appendChild(resultNode);
          }
          if (cell.isError) {
            content.appendChild(h("span", "tt-state-chip tt-state-error", modelT("stateFailed")));
          }
        } else if (cell.toolCallOnly) {
          content.appendChild(h("span", "tt-record-empty", modelT("toolCallsOnly")));
        } else if (cell.text) {
          content.appendChild(h("span", "tt-record-text", cell.text));
        } else {
          content.appendChild(h("span", "tt-record-empty", modelT("noContent")));
        }
        contentCell.appendChild(content);
      }
      tr.appendChild(contentCell);
      tbody.appendChild(tr);
    }

    emptyEl.style.display = records.length === 0 ? "" : "none";
    if (records.length === 0) {
      emptyEl.textContent = st.loading
        ? t("common.loading")
        : st.loadError
          ? modelT("loadFailed")
          : modelT("empty");
    }
  };

  /* ---------- 详情面板 ---------- */

  const tabs = () => ({
    overview: modelT("tabOverview"),
    preview: modelT("tabPreview"),
    raw: modelT("tabRaw"),
    params: modelT("tabParams"),
    result: modelT("tabResult"),
    schema: modelT("tabSchema"),
    timing: modelT("tabTiming"),
    rawOutput: modelT("tabRawOutput"),
  });

  const detailTabsFor = (cell) => {
    const label = tabs();
    if (!cell) return [];
    if (cell.kind === "system") return [label.overview, label.raw];
    if (cell.kind === "compacted") return [label.overview, label.rawOutput];
    if (cell.kind === "tool" || cell.kind === "subtool") {
      return [label.overview, label.params, label.result, label.schema, label.timing];
    }
    return [label.overview, label.preview, label.raw];
  };

  const defaultTabFor = (cell) => {
    const label = tabs();
    if (cell?.kind === "tool" || cell?.kind === "subtool") return label.params;
    return label.overview;
  };

  const argsPreview = (cell) => {
    const raw = cell.inputDetail;
    if (!raw) return "";
    const single = raw.replace(/\s+/g, " ").trim();
    return single.length > 160 ? `${single.slice(0, 160)}…` : single;
  };

  const decodeSeconds = (cell) => {
    const metrics = cell.metrics;
    if (!metrics?.timingRecorded || metrics.firstTokenTime === null || metrics.completedTime === null) {
      return null;
    }
    return Math.max(0, (metrics.completedTime - metrics.firstTokenTime) / 1000);
  };

  const decodeThroughput = (cell) => {
    const decode = decodeSeconds(cell);
    const tokens = cell.usage?.output;
    if (decode === null || decode <= 0 || tokens === undefined || tokens <= 0) return null;
    return tokens / decode;
  };

  const formatTokens = (value) => {
    if (value === undefined || !Number.isFinite(value)) return modelT("unavailable");
    return modelT("tokens", { value: Math.round(value).toLocaleString("en-US") });
  };

  const durationValue = (seconds) =>
    seconds === null ? modelT("notRecorded") : formatElapsedSeconds(seconds, modelT);

  const startedAtValue = (ms) => {
    if (ms === null) return modelT("notRecorded");
    if (st.timestampMode === "unix") return String(Math.round(ms));
    return formatStartedAt(ms) || modelT("unavailable");
  };

  const statusLabel = (cell) => {
    if (cell.isError) return modelT("stateFailed");
    if (cell.kind === "message" || cell.kind === "tool" || cell.kind === "subtool") {
      return cell.metrics?.timingRecorded || cell.timeSeconds !== null
        ? modelT("stateCompleted")
        : modelT("stateWaiting");
    }
    return modelT("stateCompleted");
  };

  const timingRowsFor = (cell) => {
    if (!cell) return [];
    if (cell.kind === "user" || cell.kind === "context" || cell.kind === "system") return [];
    const metrics = cell.metrics;
    const rowsOut = [];
    if (cell.kind === "message" && (!metrics || !metrics.timingRecorded)) {
      return [{ label: modelT("timingSource"), value: modelT("notRecorded") }];
    }
    if (cell.kind === "message" && metrics) {
      rowsOut.push({
        label: modelT("startedAt"),
        value: startedAtValue(metrics.stepStartTime),
        toggleable: true,
      });
      rowsOut.push({
        label: modelT("totalDuration"),
        value:
          metrics.completedTime !== null && metrics.stepStartTime !== null
            ? formatDurationMillis(metrics.completedTime - metrics.stepStartTime, modelT)
            : modelT("notRecorded"),
      });
      if (metrics.firstTokenTime !== null && metrics.stepStartTime !== null) {
        rowsOut.push({
          label: modelT("firstToken"),
          value: formatDurationMillis(metrics.firstTokenTime - metrics.stepStartTime, modelT),
        });
      }
      const decode = decodeSeconds(cell);
      if (decode !== null) {
        rowsOut.push({
          label: modelT("generation"),
          value: formatDurationMillis(decode * 1000, modelT),
        });
      }
      const throughput = decodeThroughput(cell);
      if (throughput !== null) {
        rowsOut.push({
          label: modelT("throughput"),
          value: modelT("tokensPerSecond", { value: throughput.toFixed(1) }),
        });
      }
    }
    if (cell.kind === "tool" || cell.kind === "subtool") {
      rowsOut.push({
        label: modelT("startedAt"),
        value: startedAtValue(cell.startedAt),
        toggleable: true,
      });
      rowsOut.push({ label: modelT("duration"), value: durationValue(cell.timeSeconds) });
    }
    return rowsOut;
  };

  const detailsLocationFor = (record, cell) => {
    if (!record || !cell) return "";
    const turnPart =
      record.turn === null ? modelT("betweenTurns") : modelT("turnTitle", { turn: record.turn });
    const stepMatch = record.group ? /(\d+)/.exec(record.group) : null;
    if (stepMatch) {
      return `${turnPart} · ${modelT("stepLabel", { step: Number(stepMatch[1]) })}`;
    }
    if (cell.kind === "compacted") return `${turnPart} · ${modelT("compaction", { seq: "" })}`;
    if (record.group) return `${turnPart} · ${record.group}`;
    return turnPart;
  };

  const buildPayloadPre = (text) => {
    const pre = h("pre", "tt-payload-pre");
    const code = h("code");
    code.textContent = text;
    pre.appendChild(code);
    return pre;
  };

  const fillMarkdownPayload = (payload, raw) => {
    const html = renderMarkdownHtml(raw);
    if (html) {
      payload.innerHTML = html;
      enhanceRenderedMarkdown(payload);
    } else {
      payload.textContent = raw || modelT("noContent");
    }
  };

  const renderDetails = (selectedCell, selectedRecord, requests, allRecords) => {
    bodyEl.classList.toggle("tt-has-details", Boolean(selectedCell && selectedRecord));
    if (!selectedCell || !selectedRecord) {
      resizeHandle.style.display = "none";
      detailsEl.style.display = "none";
      return;
    }

    if (detailsEl.parentNode !== bodyEl) {
      bodyEl.appendChild(resizeHandle);
      bodyEl.appendChild(detailsEl);
    }
    resizeHandle.style.display = "";
    detailsEl.style.display = "";
    detailsEl.style.width = st.detailsWidth === null ? "" : `${st.detailsWidth}px`;

    const identityOf = (cell) => {
      const record = allRecords.find((item) => item.cell.index === cell.index);
      return record ? requestIdentity(record.turn, record.group) : "";
    };
    let request = requests.find((item) => item.assistantIndex === selectedCell.index) ?? null;
    if (!request && (selectedCell.kind === "tool" || selectedCell.kind === "subtool")) {
      const identity = identityOf(selectedCell);
      request = requests.find((item) => item.identity === identity) ?? null;
    }

    detailsEl.textContent = "";

    const header = h("div", "tt-details-header");
    header.appendChild(
      h("span", `tt-kind-tag tt-kind-${selectedCell.kind}`, kindLabel(selectedCell.kind))
    );
    header.appendChild(
      h("span", "tt-details-location", detailsLocationFor(selectedRecord, selectedCell))
    );
    const closeBtn = h("button", "tt-details-close");
    closeBtn.type = "button";
    closeBtn.title = modelT("closeDetails");
    const closeSvg = svgEl("svg", { viewBox: "0 0 16 16", width: "12", height: "12", "aria-hidden": "true" });
    closeSvg.appendChild(
      svgEl("path", { d: "m4 4 8 8m0-8-8 8", stroke: "currentColor", "stroke-width": "1.3", "stroke-linecap": "round" })
    );
    closeBtn.appendChild(closeSvg);
    closeBtn.addEventListener("click", clearSelection);
    header.appendChild(closeBtn);
    detailsEl.appendChild(header);

    const availableTabs = detailTabsFor(selectedCell);
    if (!availableTabs.includes(st.activeTab)) {
      st.activeTab = availableTabs[0] ?? "";
    }
    const tabBar = h("div", "tt-detail-tabs");
    for (const tab of availableTabs) {
      const tabBtn = h("button", "tt-detail-tab");
      tabBtn.type = "button";
      if (tab === st.activeTab) tabBtn.classList.add("tt-detail-tab-active");
      tabBtn.textContent = tab;
      tabBtn.addEventListener("click", () => {
        st.activeTab = tab;
        scheduleRender();
      });
      tabBar.appendChild(tabBtn);
    }
    detailsEl.appendChild(tabBar);

    const body = h("div", "tt-detail-body");
    detailsEl.appendChild(body);
    const label = tabs();

    if (st.activeTab === label.overview) {
      const wrap = h("div", "tt-detail-body-summary");
      const dl = h("dl", "tt-overview");
      const addRow = (dt, dd, opts = {}) => {
        const row = h("div", "tt-overview-row");
        row.appendChild(h("dt", null, dt));
        const ddNode = h("dd", opts.mono ? "tt-mono" : null, dd);
        if (opts.clickable) {
          ddNode.classList.add("tt-dd-clickable");
          ddNode.addEventListener("click", opts.clickable);
        }
        row.appendChild(ddNode);
        dl.appendChild(row);
        return ddNode;
      };

      addRow(modelT("status"), statusLabel(selectedCell));

      const links = [];
      if ((selectedCell.kind === "tool" || selectedCell.kind === "subtool") && request) {
        links.push({
          label: modelT("assistantMessage"),
          action: () => selectIndex(request.assistantIndex),
        });
      }
      if (request) {
        links.push({
          label: modelT("requestLabel", { request: request.number }),
          action: () => selectIndex(request.assistantIndex),
        });
      }
      if (links.length > 0) {
        const row = h("div", "tt-overview-row");
        row.appendChild(h("dt", null, modelT("hierarchy")));
        const dd = h("dd");
        links.forEach((link, index) => {
          const linkBtn = h("button", "tt-link");
          linkBtn.type = "button";
          linkBtn.textContent = `${link.label} ›`;
          linkBtn.addEventListener("click", link.action);
          dd.appendChild(linkBtn);
          if (index < links.length - 1) dd.appendChild(document.createTextNode(" "));
        });
        row.appendChild(dd);
        dl.appendChild(row);
      }

      if (selectedCell.toolName) {
        addRow(modelT("kindTool"), selectedCell.toolName, { mono: true });
      }
      if (selectedCell.usage) {
        addRow(modelT("token"), formatTokens(selectedCell.usage.output));
        addRow(modelT("reasoning"), formatTokens(selectedCell.usage.think));
      } else if (selectedCell.kind === "message") {
        addRow(modelT("token"), modelT("usageNotReported"));
      }
      if (selectedCell.kind !== "message") {
        addRow(modelT("duration"), durationValue(selectedCell.timeSeconds));
      }
      for (const row of timingRowsFor(selectedCell)) {
        addRow(row.label, row.value, {
          clickable: row.toggleable
            ? () => {
                st.timestampMode = st.timestampMode === "local" ? "unix" : "local";
                scheduleRender();
              }
            : null,
        });
      }
      wrap.appendChild(dl);

      if (selectedCell.kind === "message") {
        const sections = [
          { key: "input", title: modelT("input"), content: selectedCell.inputDetail },
          { key: "output", title: modelT("output"), content: selectedCell.outputDetail },
        ];
        for (const section of sections) {
          if (!section.content) continue;
          const sectionEl = h("div", "tt-overview-section");
          const titleBtn = h("button", "tt-section-title");
          titleBtn.type = "button";
          titleBtn.appendChild(document.createTextNode(section.title));
          titleBtn.appendChild(
            h(
              "span",
              `tt-section-chevron${st.openSections[section.key] ? " tt-section-chevron-open" : ""}`,
              "›"
            )
          );
          titleBtn.addEventListener("click", () => {
            st.openSections[section.key] = !st.openSections[section.key];
            scheduleRender();
          });
          sectionEl.appendChild(titleBtn);
          if (st.openSections[section.key]) {
            const scroll = h("div", "tt-section-scroll");
            const payload = h("div", "tt-markdown-payload");
            fillMarkdownPayload(payload, section.content);
            scroll.appendChild(payload);
            sectionEl.appendChild(scroll);
          }
          wrap.appendChild(sectionEl);
        }
      }
      body.appendChild(wrap);
    } else if (st.activeTab === label.preview) {
      const wrap = h("div", "tt-markdown-preview");
      const payload = h("div", "tt-markdown-payload");
      fillMarkdownPayload(payload, selectedCell.previewMarkdown);
      wrap.appendChild(payload);
      body.appendChild(wrap);
    } else if (st.activeTab === label.raw) {
      const stack = h("div", "tt-payload-stack");
      if (selectedCell.thinkingDetail) stack.appendChild(buildPayloadPre(selectedCell.thinkingDetail));
      if (selectedCell.inputDetail) stack.appendChild(buildPayloadPre(selectedCell.inputDetail));
      if (!selectedCell.inputDetail && !selectedCell.thinkingDetail) {
        stack.appendChild(h("div", "tt-no-payload", modelT("noContent")));
      }
      body.appendChild(stack);
    } else if (st.activeTab === label.params) {
      const stack = h("div", "tt-payload-stack");
      const args = safeParseJson(selectedCell.inputDetail);
      if (args !== null && args !== undefined) {
        appendJsonTree(stack, args, 12);
      } else if (selectedCell.inputDetail) {
        stack.appendChild(buildPayloadPre(selectedCell.inputDetail));
      } else {
        stack.appendChild(h("div", "tt-no-payload", modelT("paramUnavailable")));
      }
      body.appendChild(stack);
    } else if (st.activeTab === label.result) {
      const stack = h("div", "tt-payload-stack");
      const result = safeParseJson(selectedCell.result);
      if (result !== null && result !== undefined) {
        appendJsonTree(stack, result, 12);
      } else if (selectedCell.result) {
        stack.appendChild(buildPayloadPre(selectedCell.result));
      } else {
        stack.appendChild(h("div", "tt-no-payload", modelT("noOutput")));
      }
      body.appendChild(stack);
    } else if (st.activeTab === label.schema) {
      const stack = h("div", "tt-payload-stack");
      if (selectedCell.schemaDetail) {
        stack.appendChild(buildPayloadPre(selectedCell.schemaDetail));
      } else {
        stack.appendChild(h("div", "tt-no-payload", modelT("schemaUnavailable")));
      }
      body.appendChild(stack);
    } else {
      const wrap = h("div", "tt-detail-body-summary");
      const dl = h("dl", "tt-overview");
      const rowsOut = [
        ...timingRowsFor(selectedCell),
        ...(selectedCell.kind === "message"
          ? [
              {
                label: modelT("sessionTimestamp"),
                value:
                  st.timestampMode === "local"
                    ? modelT("showUnixTimestamp")
                    : modelT("showLocalTime"),
                toggleable: true,
              },
            ]
          : []),
      ];
      for (const row of rowsOut) {
        const rowEl = h("div", "tt-overview-row");
        rowEl.appendChild(h("dt", null, row.label));
        const dd = h("dd", null, row.value);
        if (row.toggleable) {
          dd.classList.add("tt-dd-clickable");
          dd.addEventListener("click", () => {
            st.timestampMode = st.timestampMode === "local" ? "unix" : "local";
            scheduleRender();
          });
        }
        rowEl.appendChild(dd);
        dl.appendChild(rowEl);
      }
      wrap.appendChild(dl);
      body.appendChild(wrap);
    }
  };

  /* ---------- 交互 ---------- */

  const selectIndex = (index) => {
    st.selectedIndex = index;
    st.activeTab = defaultTabFor(findCell(computeTurns(), index));
    st.openSections.input = true;
    st.openSections.output = true;
    scheduleRender();
  };

  const clearSelection = () => {
    st.selection = null;
    st.selectedIndex = null;
    st.activeTab = "";
    scheduleRender();
  };

  const onRowDoubleClick = (record) => {
    if (record.collapsedSummary) return;
    if (record.turnStart) {
      const next = new Set(st.collapsedTurns);
      if (next.has(record.turn)) next.delete(record.turn);
      else next.add(record.turn);
      st.collapsedTurns = next;
      scheduleRender();
      return;
    }
    if (record.groupStart && record.group) {
      const key = requestIdentity(record.turn, record.group);
      const next = new Set(st.collapsedAssistants);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      st.collapsedAssistants = next;
      scheduleRender();
    }
  };

  const valueAt = (clientX) => {
    const rect = trackEl.getBoundingClientRect();
    if (rect.width <= 0) return st.viewStart;
    const fraction = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width));
    return st.viewStart + fraction * st.viewSpan;
  };

  const onTimelineWheel = (event) => {
    const anchor = valueAt(event.clientX);
    const factor = event.deltaY > 0 ? 1.25 : 0.8;
    const nextSpan = st.viewSpan * factor;
    const fraction = (anchor - st.viewStart) / Math.max(1e-9, st.viewSpan);
    setViewport(anchor - fraction * nextSpan, nextSpan);
    scheduleRender();
  };

  const onTimelinePointerDown = (event) => {
    const value = valueAt(event.clientX);
    if (event.button === 2) {
      dragState = {
        kind: "pan",
        anchorX: event.clientX,
        anchorValue: value,
        moved: false,
        panStart: st.viewStart,
      };
      st.panning = true;
      scheduleRender();
      return;
    }
    if (event.button !== 0) return;
    dragState = {
      kind: "select",
      anchorX: event.clientX,
      anchorValue: value,
      moved: false,
      panStart: st.viewStart,
    };
    st.dragging = true;
    st.selection = null;
    scheduleRender();
  };

  const edgePan = (clientX, rect) => {
    const edge = rect.width * EDGE_PAN_FRACTION;
    let shift = 0;
    if (clientX - rect.left < edge) shift = -st.viewSpan * 0.03;
    else if (rect.right - clientX < edge) shift = st.viewSpan * 0.03;
    if (shift !== 0) setViewport(st.viewStart + shift, st.viewSpan);
  };

  const handleTimelineMove = (event) => {
    if (!dragState) return;
    if (dragState.kind === "pan") {
      const dx = event.clientX - dragState.anchorX;
      if (Math.abs(dx) > 2) dragState.moved = true;
      const rect = trackEl.getBoundingClientRect();
      const perPx = st.viewSpan / Math.max(1, rect.width);
      setViewport(dragState.panStart - dx * perPx, st.viewSpan);
      scheduleRender();
      return;
    }
    const rect = trackEl.getBoundingClientRect();
    if (Math.abs(event.clientX - dragState.anchorX) >= 3) dragState.moved = true;
    edgePan(event.clientX, rect);
    const anchor = valueAt(dragState.anchorX);
    const current = valueAt(event.clientX);
    st.selection = { start: Math.min(anchor, current), end: Math.max(anchor, current) };
    scheduleRender();
  };

  const finishTimelinePointer = () => {
    if (!dragState) return;
    const state = dragState;
    dragState = null;
    st.dragging = false;
    st.panning = false;
    if (state.kind !== "select") {
      scheduleRender();
      return;
    }
    if (!state.moved) {
      // 视为点击：聚焦时间上最近的记录。
      const timeline = deriveTimeline(computeTurns(), timelineMode());
      if (timeline && timeline.spans.length > 0) {
        let best = timeline.spans[0];
        let bestDistance = Number.POSITIVE_INFINITY;
        for (const modelSpan of timeline.spans) {
          const center = (modelSpan.start + modelSpan.end) / 2;
          const distance = Math.abs(center - state.anchorValue);
          if (distance < bestDistance) {
            bestDistance = distance;
            best = modelSpan;
          }
        }
        st.selection = null;
        selectIndex(best.index);
        return;
      }
    }
    if (st.selection && st.selection.end - st.selection.start < 1e-9) {
      st.selection = null;
    }
    scheduleRender();
  };

  const onWindowPointerMove = (event) => {
    if (resizeState) {
      const delta = resizeState.startX - event.clientX;
      const maxWidth = Math.max(320, window.innerWidth - 280);
      st.detailsWidth = Math.min(
        720,
        Math.max(320, Math.min(maxWidth, resizeState.startWidth + delta))
      );
      scheduleRender();
    }
    handleTimelineMove(event);
  };

  const onWindowPointerUp = () => {
    resizeState = null;
    finishTimelinePointer();
  };

  const onTimelinePointerMove = (event) => {
    const rect = trackEl.getBoundingClientRect();
    st.hoverX =
      event.clientX >= rect.left && event.clientX <= rect.right ? event.clientX - rect.left : null;
    if (st.hoverX !== null) {
      hoverlineEl.style.display = "";
      hoverlineEl.style.left = `${st.hoverX}px`;
    } else {
      hoverlineEl.style.display = "none";
    }
    handleTimelineMove(event);
  };

  const onTimelineLeave = () => {
    st.hoverX = null;
    hoverlineEl.style.display = "none";
  };

  const onSpanEnter = (modelSpan, event) => {
    if (tooltipTimer !== null) window.clearTimeout(tooltipTimer);
    const clientX = event.clientX;
    const clientY = event.clientY;
    tooltipTimer = window.setTimeout(() => {
      const cell = findCell(computeTurns(), modelSpan.index);
      const lines = [kindLabel(modelSpan.kind)];
      if (modelSpan.label) {
        lines.push(
          modelSpan.label.length > 64 ? `${modelSpan.label.slice(0, 64)}…` : modelSpan.label
        );
      }
      if (cell) {
        if (cell.startedAt !== null) {
          const end = cell.startedAt + (cell.timeSeconds ?? 0) * 1000;
          lines.push(`${formatClock(cell.startedAt)} → ${formatClock(end)}`);
        }
        if (cell.timeSeconds !== null && cell.timeSeconds > 0) {
          lines.push(
            modelT("totalDurationValue", {
              duration: formatDurationMillis(cell.timeSeconds * 1000, modelT),
            })
          );
        }
        const decode = decodeSeconds(cell);
        const metrics = cell.metrics;
        const ttft =
          metrics?.firstTokenTime != null && metrics?.stepStartTime != null
            ? metrics.firstTokenTime - metrics.stepStartTime
            : null;
        if (ttft !== null && decode !== null) {
          lines.push(
            modelT("tooltipTtft", {
              ttft: formatDurationMillis(ttft, modelT),
              decoding: formatDurationMillis(decode * 1000, modelT),
            })
          );
        }
      }
      tooltipEl.textContent = "";
      lines.forEach((line) => tooltipEl.appendChild(h("div", "tt-tooltip-line", line)));
      tooltipEl.style.left = `${clientX}px`;
      tooltipEl.style.top = `${clientY}px`;
      tooltipEl.style.display = "";
    }, TOOLTIP_DELAY_MS);
  };

  const onSpanLeave = () => {
    if (tooltipTimer !== null) {
      window.clearTimeout(tooltipTimer);
      tooltipTimer = null;
    }
    tooltipEl.style.display = "none";
  };

  const onKeyDown = (event) => {
    if (event.key !== "Escape") return;
    if (st.selection || st.dragging || st.selectedIndex !== null) {
      st.selection = null;
      st.dragging = false;
      st.selectedIndex = null;
      st.activeTab = "";
      scheduleRender();
      return;
    }
    if (onBack) onBack();
  };

  const updateSizes = () => {
    if (trackEl) st.trackWidth = trackEl.clientWidth;
    if (ledgerEl) st.ledgerViewport = ledgerEl.clientHeight;
  };

  /* ---------- 事件绑定 ---------- */

  durationToggle.addEventListener("click", () => {
    st.showDuration = !st.showDuration;
    st.viewSpan = 0;
    scheduleRender();
  });
  turnsToggle.addEventListener("click", () => {
    st.showTurns = !st.showTurns;
    if (!st.showTurns) st.collapsedTurns = new Set();
    scheduleRender();
  });
  callsToggle.addEventListener("click", () => {
    st.showCalls = !st.showCalls;
    if (!st.showCalls) st.collapsedAssistants = new Set();
    scheduleRender();
  });
  searchInput.addEventListener("input", () => {
    st.searchQuery = searchInput.value;
    scheduleRender();
  });
  if (showBackButton) {
    backBtn.addEventListener("click", () => onBack?.());
  }

  timelineEl.addEventListener("wheel", onTimelineWheel, { passive: false });
  timelineEl.addEventListener("contextmenu", (event) => event.preventDefault());
  timelineEl.addEventListener("pointerdown", onTimelinePointerDown);
  timelineEl.addEventListener("pointermove", onTimelinePointerMove);
  timelineEl.addEventListener("pointerup", finishTimelinePointer);
  timelineEl.addEventListener("pointerleave", onTimelineLeave);
  timelineEl.addEventListener("dblclick", clearSelection);

  ledgerEl.addEventListener("scroll", () => {
    st.scrollTop = ledgerEl.scrollTop;
    st.ledgerViewport = ledgerEl.clientHeight;
    scheduleRender();
  }, { passive: true });
  ledgerEl.addEventListener("pointerdown", (event) => {
    if (event.target === ledgerEl) clearSelection();
  });

  resizeHandle.addEventListener("pointerdown", (event) => {
    event.preventDefault();
    resizeState = {
      startX: event.clientX,
      startWidth: detailsEl.getBoundingClientRect().width,
    };
    resizeHandle.setPointerCapture(event.pointerId);
  });
  resizeHandle.addEventListener("dblclick", () => {
    st.detailsWidth = null;
    scheduleRender();
  });

  window.addEventListener("pointermove", onWindowPointerMove);
  window.addEventListener("pointerup", onWindowPointerUp);
  window.addEventListener("keydown", onKeyDown);
  window.addEventListener("resize", onWindowResize);
  window.addEventListener("wunder:language-changed", onLanguageChanged);

  function onWindowResize() {
    updateSizes();
    scheduleRender();
  }

  function onLanguageChanged() {
    searchInput.placeholder = modelT("searchTrajectory");
    lane0.textContent = modelT("laneInput");
    lane1.textContent = modelT("laneModel");
    lane2.textContent = modelT("laneTool");
    backBtn.title = modelT("back");
    backBtn.setAttribute("aria-label", modelT("back"));
    durationToggle
      .querySelector(".tt-toggle-label")
      ?.replaceChildren(document.createTextNode(modelT("duration")));
    turnsToggle
      .querySelector(".tt-toggle-label")
      ?.replaceChildren(document.createTextNode(modelT("turns")));
    callsToggle
      .querySelector(".tt-toggle-label")
      ?.replaceChildren(document.createTextNode(modelT("calls")));
    turnsMemo = null;
    scheduleRender();
  }

  /* ---------- 对外 API ---------- */

  render();

  return {
    setRawTurns(turns) {
      st.rawTurns = Array.isArray(turns) ? turns : [];
      st.turnsKey += 1;
      st.loading = false;
      st.loadError = false;
      st.selectedIndex = null;
      st.activeTab = "";
      st.selection = null;
      st.collapsedTurns = new Set();
      st.collapsedAssistants = new Set();
      ledgerEl.scrollTop = 0;
      st.scrollTop = 0;
      st.viewSpan = 0;
      scheduleRender();
    },
    setLoading(loading) {
      st.loading = Boolean(loading);
      scheduleRender();
    },
    setLoadError(loadError) {
      st.loadError = Boolean(loadError);
      st.loading = false;
      scheduleRender();
    },
    refreshSizes() {
      updateSizes();
      scheduleRender();
    },
    destroy() {
      destroyed = true;
      window.removeEventListener("pointermove", onWindowPointerMove);
      window.removeEventListener("pointerup", onWindowPointerUp);
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("wunder:language-changed", onLanguageChanged);
      if (tooltipTimer !== null) window.clearTimeout(tooltipTimer);
      host.textContent = "";
    },
  };
}

/* ---------------------------------------------------------------- */
/* 全屏「轨迹页面」弹窗（线程日志 → 轨迹视图）                         */
/* ---------------------------------------------------------------- */

let pageView = null;
let pageRequestId = 0;

export const closeTrajectoryPage = () => {
  pageRequestId += 1;
  elements.trajectoryModal?.classList.remove("active");
};

export const openTrajectoryPage = async (sessionId) => {
  const cleaned = String(sessionId || "").trim();
  if (!cleaned || !elements.trajectoryModal || !elements.trajectoryMount) {
    return;
  }
  if (!pageView) {
    pageView = createTrajectoryView(elements.trajectoryMount, {
      showBack: true,
      onBack: closeTrajectoryPage,
    });
    elements.trajectoryModal.addEventListener("click", (event) => {
      if (event.target === elements.trajectoryModal) {
        closeTrajectoryPage();
      }
    });
  }
  const requestId = ++pageRequestId;
  elements.trajectoryModal.classList.add("active");
  pageView.refreshSizes();
  pageView.setLoading(true);
  try {
    const turns = await fetchThreadLogSnapshot(cleaned);
    if (requestId !== pageRequestId) return;
    pageView.setRawTurns(turns);
  } catch {
    if (requestId !== pageRequestId) return;
    pageView.setLoadError(true);
  }
};
