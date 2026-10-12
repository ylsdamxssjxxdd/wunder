// AI生成
import { state } from "../state.js";
import { getWunderBase } from "../api.js";
import { ensureLlmConfigLoaded } from "../llm.js";
import { escapeHtml, formatTimestamp } from "../utils.js?v=20251229-02";
import { resolveApiErrorMessage } from "../api-error.js";
import { label as l } from "./copy.js?v=20261012-01";
import { mount, details, history, number } from "./view.js?v=20261012-01";
import { concurrencySeries, speedLabel } from "./chart.js?v=20261012-01";

const KEY = "wunder_throughput_scenario_v2";
let initialized = false;
let visible = false;
let generation = 0;
let socket = null;
let reconnect = null;
let poll = null;
let pending = null;
let busy = false;
let chart = null;
let observer = null;
let viewed = "";
let data = { active: null, history: [] };
let historySignature = "";
let chartSignature = "";
let requestSequence = 0;
let appliedSequence = 0;
const $ = (id) => document.getElementById(id);
const running = () => ["running", "stopping"].includes(data.active?.status);
// The run whose details and chart are shown: an explicitly selected history row,
// otherwise the live run, otherwise the most recent history entry.
const currentRecord = () => viewed ? data.history.find((item) => item.id === viewed) : data.active || data.history.at(-1);

async function api(path, body, signal) {
  const response = await fetch(`${getWunderBase()}/admin/throughput/${path}`, {
    method: body === undefined ? "GET" : "POST", signal,
    ...(body === undefined ? {} : { headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }),
  });
  if (!response.ok) throw new Error(await resolveApiErrorMessage(response, `HTTP ${response.status}`));
  return response.json();
}

const parseConcurrencyList = (value) => {
  const seen = new Set();
  const list = [];
  for (const part of String(value || "").split(/[,，\s]+/).filter(Boolean)) {
    const concurrency = Number(part);
    if (!Number.isSafeInteger(concurrency) || concurrency < 1 || concurrency > 1024) return null;
    if (!seen.has(concurrency)) { seen.add(concurrency); list.push(concurrency); }
  }
  return list.length ? list : null;
};
const form = () => ({ model_name: $("tpModel").value, concurrency_list: parseConcurrencyList($("tpConcurrencyList").value), input_tokens: Number($("tpInput").value), output_tokens: Number($("tpOutput").value) });
function saveForm() { try { localStorage.setItem(KEY, JSON.stringify(form())); } catch { /* Optional preference storage. */ } }
function restoreForm(config) {
  if (!config) { try { config = JSON.parse(localStorage.getItem(KEY) || "{}"); } catch { config = {}; } }
  const list = Array.isArray(config.concurrency_list) ? config.concurrency_list : config.concurrency == null ? null : [config.concurrency];
  for (const [id, value] of [["tpModel", config.model_name], ["tpConcurrencyList", list?.join(",")], ["tpInput", config.input_tokens], ["tpOutput", config.output_tokens]]) {
    if (value != null) $(id).value = String(value);
  }
}

function models() {
  const preferred = $("tpModel").value;
  $("tpModel").replaceChildren();
  for (const name of state.llm.order) {
    const config = state.llm.configs[name];
    if (config?.enable === false || !["", "llm"].includes(String(config?.model_type || "").toLowerCase())) continue;
    $("tpModel").add(new Option(name, name));
  }
  if (!$("tpModel").options.length) $("tpModel").add(new Option(l("noModels"), ""));
  if (!preferred && [...$("tpModel").options].some((option) => option.value === state.llm.defaultName)) $("tpModel").value = state.llm.defaultName;
  if ([...$("tpModel").options].some((option) => option.value === preferred)) $("tpModel").value = preferred;
}

function apply(snapshot, sequence) {
  if (sequence < appliedSequence || snapshot.schema_version !== 2) return;
  appliedSequence = sequence;
  data = snapshot;
  const ids = new Set(data.history.map((item) => item.id));
  if (viewed && !ids.has(viewed)) viewed = "";
  render();
}

function render(force = false) {
  const item = currentRecord();
  details($("tpDetail"), item);
  if (running() && data.active?.config?.concurrency_list?.length) {
    const total = data.active.config.concurrency_list.length;
    const current = Math.min(data.active.samples?.length || 0, total);
    $("tpFeedback").textContent = l("batchProgress").replace("{current}", String(current)).replace("{total}", String(total));
  }
  $("tpCurrent").hidden = !viewed;
  $("tpStart").hidden = running();
  $("tpStart").disabled = busy || !$("tpModel").value;
  $("tpStop").hidden = !running();
  $("tpStop").disabled = busy || data.active?.status === "stopping";
  $("tpStop").textContent = l(data.active?.status === "stopping" ? "stopping" : "stop");
  for (const id of ["tpModel", "tpConcurrencyList", "tpInput", "tpOutput"]) $(id).disabled = running() || busy;
  const signature = JSON.stringify([data.history, viewed]);
  if (force || signature !== historySignature) {
    history($("tpHistory"), data.history, viewed);
    historySignature = signature;
  }
  $("tpViewed").textContent = viewed && item ? `${l("viewing")} ${formatTimestamp(item.started_at)}` : "";
  $("tpExport").disabled = !viewed;
  draw(force);
}

function draw(force = false) {
  const item = currentRecord();
  const signature = JSON.stringify([item?.id, item?.status, item?.samples?.length, item?.metrics?.decode_tps]);
  if (!force && signature === chartSignature) return;
  chartSignature = signature;
  const series = item ? concurrencySeries(item, l) : [];
  $("tpChartEmpty").textContent = !window.echarts ? l("chartUnavailable") : series.length ? "" : l("chartEmpty");
  if (!window.echarts) return;
  chart ||= window.echarts.init($("tpChart"));
  chart.setOption({
    animation: false, color: ["#2563eb", "#8b5cf6", "#16a34a", "#d97706", "#7c3aed", "#dc2626", "#0891b2"],
    tooltip: { trigger: "axis", confine: true, formatter: (points) => {
      const concurrency = points?.[0]?.value?.[0];
      const header = `${l("concurrency")}: ${number(concurrency, 0)}`;
      const rows = (points || []).map((point) => `${point.marker}${escapeHtml(point.seriesName)}: ${speedLabel(Array.isArray(point.value) ? point.value[1] : point.value)}`);
      return [header, ...rows].join("<br>");
    } },
    legend: { type: "scroll", bottom: 0 }, grid: { top: 48, left: 82, right: 25, bottom: 68 },
    xAxis: { type: "value", name: l("concurrency"), minInterval: 1, axisLabel: { formatter: (value) => number(value, 0) } },
    yAxis: { type: "value", name: l("speed"), axisLabel: { formatter: (value) => speedLabel(value) } }, series,
  }, true);
  chart.resize();
}

async function refresh() {
  if (!visible || pending) return;
  const epoch = generation;
  const controller = new AbortController();
  pending = controller;
  const sequence = ++requestSequence;
  try {
    const snapshot = await api("status", undefined, controller.signal);
    if (visible && epoch === generation) apply(snapshot, sequence);
  } catch (error) {
    if (visible && epoch === generation && error.name !== "AbortError") $("tpFeedback").textContent = error.message;
  } finally { if (pending === controller) pending = null; }
}

async function connect(epoch) {
  if (!visible || epoch !== generation) return;
  $("tpConnection").textContent = l("reconnecting");
  try {
    const { ticket } = await api("ticket", {});
    if (!visible || epoch !== generation) return;
    const url = new URL(`${getWunderBase()}/throughput/ws`);
    url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
    const connection = new WebSocket(url, ["wunder-throughput", `ticket.${ticket}`]);
    socket = connection;
    let sequence = 0;
    connection.onopen = () => { if (visible && epoch === generation) $("tpConnection").textContent = l("connected"); };
    connection.onmessage = (event) => {
      if (!visible || epoch !== generation) return;
      try {
        const message = JSON.parse(event.data);
        if (message.event !== "snapshot" || message.sequence <= sequence) return;
        sequence = message.sequence;
        apply(message.data, ++requestSequence);
      } catch { /* The next full snapshot repairs a malformed or missing frame. */ }
    };
    connection.onclose = () => {
      if (!visible || epoch !== generation) return;
      socket = null;
      $("tpConnection").textContent = l("reconnecting");
      reconnect = setTimeout(() => connect(epoch), 3000);
    };
    connection.onerror = () => connection.close();
  } catch {
    if (visible && epoch === generation) reconnect = setTimeout(() => connect(epoch), 5000);
  }
}

async function command(action) {
  if (busy) return;
  if (action === "start") {
    const config = form();
    const limit = state.llm.configs[config.model_name]?.max_context;
    if (!config.concurrency_list) { $("tpFeedback").textContent = l("concurrencyError"); return; }
    if (!Number.isSafeInteger(config.input_tokens) || config.input_tokens < 1 || config.input_tokens > 16777216) { $("tpFeedback").textContent = l("inputError"); return; }
    if (!Number.isSafeInteger(config.output_tokens) || config.output_tokens < 1 || config.output_tokens > 1048576) { $("tpFeedback").textContent = l("outputError"); return; }
    if (Number(limit) > 0 && config.input_tokens + config.output_tokens > Number(limit)) { $("tpFeedback").textContent = l("contextError"); return; }
    saveForm();
    const epoch = generation;
    busy = true;
    $("tpFeedback").textContent = l("batchProgress").replace("{current}", "0").replace("{total}", String(config.concurrency_list.length));
    render();
    try {
      const snapshot = await api("start", config);
      if (!visible || epoch !== generation) return;
      viewed = "";
      appliedSequence = ++requestSequence;
      data.active = snapshot;
      $("tpFeedback").textContent = l("batchProgress").replace("{current}", "1").replace("{total}", String(config.concurrency_list.length));
    } catch (error) {
      if (visible && epoch === generation) $("tpFeedback").textContent = error.message;
    } finally {
      busy = false;
      if (visible && epoch === generation) { render(); await refresh(); }
    }
    return;
  }
  const epoch = generation;
  busy = true;
  $("tpFeedback").textContent = l("busy");
  render();
  try {
    const snapshot = await api(action, {});
    if (!visible || epoch !== generation) return;
    viewed = "";
    // Invalidate an older HTTP hydration response already in flight.
    appliedSequence = ++requestSequence;
    data.active = snapshot;
    $("tpFeedback").textContent = "";
  } catch (error) {
    if (visible && epoch === generation) $("tpFeedback").textContent = error.message;
  } finally {
    busy = false;
    if (visible && epoch === generation) { render(); await refresh(); }
  }
}

function exportCurrent() {
  const item = viewed ? data.history.find((entry) => entry.id === viewed) : null;
  if (!item) return;
  const blob = new Blob([JSON.stringify({ schema_version: 2, history: [item] }, null, 2)], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url; link.download = `throughput-${item.id}.json`; link.click();
  setTimeout(() => URL.revokeObjectURL(url), 0);
}

function bind() {
  $("tpForm").addEventListener("submit", (event) => { event.preventDefault(); void command("start"); });
  $("tpStop").addEventListener("click", () => void command("stop"));
  $("tpForm").addEventListener("change", () => { saveForm(); $("tpFeedback").textContent = ""; render(); });
  $("tpRefresh").addEventListener("click", async () => {
    const epoch = generation;
    try { await ensureLlmConfigLoaded(); if (visible && epoch === generation) { models(); await refresh(); } }
    catch (error) { if (visible && epoch === generation) $("tpFeedback").textContent = error.message; }
  });
  $("tpCurrent").addEventListener("click", () => { viewed = ""; render(); });
  $("tpExport").addEventListener("click", exportCurrent);
  $("tpHistory").addEventListener("click", (event) => {
    // The reuse button restores a run's configuration without changing the view.
    const reuse = event.target.closest("button")?.dataset.reuse;
    if (reuse) {
      if (running() || busy) return;
      const item = data.history.find((entry) => entry.id === reuse);
      if (item) { restoreForm(item.config); saveForm(); $("tpModel").focus(); }
      return;
    }
    const row = event.target.closest("tr[data-view]");
    if (row) { viewed = row.dataset.view; render(); }
  });
  $("tpHistory").addEventListener("keydown", (event) => {
    if (event.key !== "Enter" && event.key !== " ") return;
    const row = event.target.closest("tr[data-view]");
    if (row) { event.preventDefault(); viewed = row.dataset.view; render(); }
  });
}

function rebuild() {
  const config = initialized ? form() : null;
  observer?.disconnect(); chart?.dispose(); chart = null;
  mount($("throughputPanel")); models(); restoreForm(config); bind();
  historySignature = ""; chartSignature = "";
  render(true);
  observer = new ResizeObserver(() => { if (visible) chart?.resize(); });
  observer.observe($("tpChart"));
}

export async function initThroughputPanel() {
  await ensureLlmConfigLoaded();
  if (!initialized) {
    rebuild(); initialized = true;
    window.addEventListener("wunder:language-changed", () => { if (initialized) rebuild(); });
    window.addEventListener("pagehide", () => toggleThroughputPolling(false));
  }
  if (state.runtime.activePanel === "throughput") toggleThroughputPolling(true);
}

// Keep the public panel lifecycle hook; the normal transport is now WebSocket.
export function toggleThroughputPolling(active) {
  if (!initialized || visible === active) return;
  visible = active; generation += 1;
  clearTimeout(reconnect); clearInterval(poll);
  pending?.abort(); pending = null;
  socket?.close(); socket = null;
  if (!active) { observer?.disconnect(); return; }
  observer.observe($("tpChart"));
  chart?.resize();
  void refresh(); void connect(generation);
  poll = setInterval(() => { if (!socket || socket.readyState !== WebSocket.OPEN) void refresh(); }, 3000);
}
