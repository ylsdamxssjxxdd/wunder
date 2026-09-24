import { state } from "../state.js";
import { getWunderBase } from "../api.js";
import { ensureLlmConfigLoaded } from "../llm.js";
import { escapeHtml, formatTimestamp } from "../utils.js?v=20251229-02";
import { resolveApiErrorMessage } from "../api-error.js";
import { label as l } from "./copy.js?v=20260924-03";
import { mount, details, history, number, durationMs } from "./view.js?v=20260924-03";
import { comparisonSeries, selectableResult, tokenLabel } from "./chart.js?v=20260924-02";

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
const selected = new Set();
const known = new Set();
const $ = (id) => document.getElementById(id);
const running = () => ["running", "stopping"].includes(data.active?.status);
// Keep the legacy API field stable while the page no longer exposes an injected-context control.
const DEFAULT_INPUT_TOKENS = 8192;

async function api(path, body, signal) {
  const response = await fetch(`${getWunderBase()}/admin/throughput/${path}`, {
    method: body === undefined ? "GET" : "POST", signal,
    ...(body === undefined ? {} : { headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }),
  });
  if (!response.ok) throw new Error(await resolveApiErrorMessage(response, `HTTP ${response.status}`));
  return response.json();
}

const form = () => ({ model_name: $("tpModel").value, concurrency: Number($("tpConcurrency").value), input_tokens: DEFAULT_INPUT_TOKENS, output_tokens: Number($("tpOutput").value) });
function saveForm() { try { localStorage.setItem(KEY, JSON.stringify(form())); } catch { /* Optional preference storage. */ } }
function restoreForm(config) {
  if (!config) { try { config = JSON.parse(localStorage.getItem(KEY) || "{}"); } catch { config = {}; } }
  for (const [id, value] of [["tpModel", config.model_name], ["tpConcurrency", config.concurrency], ["tpOutput", config.output_tokens]]) {
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
  for (const id of selected) if (!ids.has(id)) selected.delete(id);
  for (const item of data.history) {
    if (!known.has(item.id) && selectableResult(item)) selected.add(item.id);
    known.add(item.id);
  }
  for (const id of known) if (!ids.has(id)) known.delete(id);
  if (viewed && !ids.has(viewed)) viewed = "";
  render();
}

function render(force = false) {
  const item = viewed ? data.history.find((item) => item.id === viewed) : data.active || data.history.at(-1);
  details($("tpDetail"), item);
  $("tpCurrent").hidden = !viewed;
  $("tpStart").hidden = running();
  $("tpStart").disabled = busy || !$("tpModel").value;
  $("tpStop").hidden = !running();
  $("tpStop").disabled = busy || data.active?.status === "stopping";
  $("tpStop").textContent = l(data.active?.status === "stopping" ? "stopping" : "stop");
  for (const id of ["tpModel", "tpConcurrency", "tpOutput"]) $(id).disabled = running() || busy;
  const signature = JSON.stringify([data.history, [...selected], viewed]);
  if (force || signature !== historySignature) {
    history($("tpHistory"), data.history, selected, viewed);
    historySignature = signature;
  }
  $("tpSelected").textContent = `${l("selected")} ${selected.size} / ${data.history.length}`;
  $("tpExport").disabled = selected.size === 0;
  draw(force);
}

function draw(force = false) {
  const metric = $("tpMetric").value;
  const axis = $("tpAxis").value;
  const signature = JSON.stringify([historySignature, metric, axis]);
  if (!force && signature === chartSignature) return;
  chartSignature = signature;
  const series = comparisonSeries(data.history, selected, metric, axis);
  $("tpChartEmpty").textContent = !window.echarts ? l("chartUnavailable") : series.length ? "" : l("chartEmpty");
  if (!window.echarts) return;
  chart ||= window.echarts.init($("tpChart"));
  chart.setOption({
    animation: false, color: ["#2563eb", "#16a34a", "#d97706", "#7c3aed", "#dc2626", "#0891b2"],
    legend: { type: "scroll", bottom: 0 }, grid: { top: 25, left: 70, right: 25, bottom: 68 },
    tooltip: { trigger: "item", confine: true, formatter: (point) => {
      const run = point.data.run;
      const value = metric === "ttft_ms" ? durationMs(point.value[1]) : `${number(point.value[1])} tok/s`;
      return `${escapeHtml(run.config.model_name)}<br>${escapeHtml(formatTimestamp(run.started_at))}<br>${l("concurrency")}: ${number(run.config.concurrency || 1,0)}<br>${l("context")}: ${tokenLabel(run.config.input_tokens)}<br>${l("target")}: ${tokenLabel(run.config.output_tokens)}<br>${l("actualOutput")}: ${number(run.metrics.output_tokens,0)}<br>${escapeHtml(point.seriesName)}: ${value}`;
    } },
    xAxis: axis === "time" ? { type: "time" } : { type: "log", logBase: 2, min: 1, axisLabel: { formatter: tokenLabel } },
    yAxis: { type: "value", name: metric === "ttft_ms" ? "ms" : "tok/s", min: 0 }, series,
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
  const epoch = generation;
  busy = true;
  $("tpFeedback").textContent = l("busy");
  render();
  try {
    const config = form();
    const limit = state.llm.configs[config.model_name]?.max_context;
    if (action === "start") {
      if (!Number.isSafeInteger(config.concurrency) || config.concurrency < 1 || config.concurrency > 1024) throw new Error(l("concurrencyError"));
      if (!Number.isSafeInteger(config.output_tokens) || config.output_tokens < 1 || config.output_tokens > 1048576) throw new Error(l("outputError"));
      if (Number(limit) > 0 && config.input_tokens + config.output_tokens > Number(limit)) throw new Error(l("contextError"));
    }
    saveForm();
    const snapshot = await api(action, action === "start" ? config : {});
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

function exportSelected() {
  const blob = new Blob([JSON.stringify({ schema_version: 2, history: data.history.filter((item) => selected.has(item.id)) }, null, 2)], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url; link.download = "throughput-comparison.json"; link.click();
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
  $("tpMetric").addEventListener("change", () => draw());
  $("tpAxis").addEventListener("change", () => draw());
  $("tpSelectValid").addEventListener("click", () => { selected.clear(); data.history.filter(selectableResult).forEach((item) => selected.add(item.id)); render(); });
  $("tpClear").addEventListener("click", () => { selected.clear(); render(); });
  $("tpExport").addEventListener("click", exportSelected);
  $("tpHistory").addEventListener("change", (event) => {
    const id = event.target.dataset.select;
    if (!id) return;
    if (event.target.checked) selected.add(id); else selected.delete(id);
    render();
  });
  $("tpHistory").addEventListener("click", (event) => {
    const target = event.target.closest("button");
    if (target?.dataset.view) {
      viewed = target.dataset.view; render();
      // Keep the selected details visible even when the history pane was scrolled.
      $("tpDetail").closest("section").scrollIntoView({ block: "start" });
    }
    if (target?.dataset.reuse && !running() && !busy) {
      const item = data.history.find((item) => item.id === target.dataset.reuse);
      if (item) { restoreForm(item.config); saveForm(); $("tpModel").focus(); }
    }
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
  observer?.observe($("tpChart"));
  chart?.resize();
  void refresh(); void connect(generation);
  poll = setInterval(() => { if (!socket || socket.readyState !== WebSocket.OPEN) void refresh(); }, 3000);
}
