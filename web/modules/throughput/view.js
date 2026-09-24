import { escapeHtml, formatTimestamp } from "../utils.js?v=20251229-02";
import { label as l } from "./copy.js?v=20260924-03";
import { tokenLabel } from "./chart.js?v=20260924-02";
import { simulationSpeedLabel } from "../llm-simulation.js?v=20260923-02";

const escape = (value) => escapeHtml(String(value ?? ""));
export const number = (value, digits = 1) => value == null || !Number.isFinite(Number(value)) ? "—" : Number(value).toLocaleString(undefined, { maximumFractionDigits: digits });
export const durationMs = (value) => {
  if (value == null || !Number.isFinite(Number(value))) return "—";
  const ms = Number(value);
  return ms >= 1000 ? `${number(ms / 1000, 2)} s` : `${number(ms, 0)} ms`;
};
const options = (values) => values.map((value) => `<option value="${value}">${tokenLabel(value)}</option>`).join("");
const numberField = (id, label, value, presets, hint) => `<div class="form-row"><label for="${id}">${label}</label><input id="${id}" type="number" min="1" step="1" value="${value}" list="${id}Presets" inputmode="numeric" required><datalist id="${id}Presets">${options(presets)}</datalist><p class="muted">${hint}</p></div>`;

export function mount(panel) {
  panel.innerHTML = `
    <div class="list-header"><div><h1>${l("title")}</h1><p class="muted">${l("subtitle")}</p></div>
      <button id="tpRefresh" class="secondary" type="button">${l("refresh")}</button></div>
    <div class="tp-layout">
      <aside class="tp-config monitor-block">
        <h2>${l("scenario")}</h2>
        <form id="tpForm">
          <div class="form-row"><label for="tpModel">${l("model")}</label><select id="tpModel" required></select></div>
          ${numberField("tpConcurrency", l("concurrency"), 1, [1,2,4,8,16,32], l("concurrencyHint"))}
          ${numberField("tpOutput", l("output"), 2048, [1024,2048,4096,8192], l("outputHint"))}
          <button id="tpStart" type="submit">${l("start")}</button>
          <button id="tpStop" type="button" class="secondary" hidden>${l("stop")}</button>
        </form>
        <p id="tpFeedback" role="status" aria-live="polite" class="muted"></p>
        <p class="tp-retention muted"><i class="fa-solid fa-database" aria-hidden="true"></i> ${l("privacy")}</p>
      </aside>
      <div class="tp-results">
        <section class="monitor-block tp-details">
          <div class="tp-section-header"><h2>${l("details")}</h2><span id="tpConnection" class="muted"></span><button id="tpCurrent" class="secondary" type="button" hidden>${l("current")}</button></div>
          <div id="tpDetail"></div>
        </section>
        <section class="monitor-block tp-comparison">
          <div class="tp-section-header"><h2>${l("comparison")}</h2>
            <label>${l("metric")} <select id="tpMetric"><option value="decode_tps">${l("decode")}</option><option value="avg_decode_tps">${l("avgDecode")}</option><option value="ttft_ms">${l("ttft")}</option><option value="prefill_tps">${l("prefill")}</option><option value="avg_prefill_tps">${l("avgPrefill")}</option></select></label>
            <label>${l("axis")} <select id="tpAxis"><option value="input">${l("axisInput")}</option><option value="time">${l("axisTime")}</option></select></label>
          </div>
          <p class="muted">${l("comparisonHint")}</p>
          <div id="tpChart" role="img" aria-label="${l("comparison")}"></div><p id="tpChartEmpty" class="muted"></p>
        </section>
        <section class="monitor-block tp-history">
          <div class="tp-section-header"><h2>${l("history")}</h2><span id="tpSelected" class="muted"></span>
            <button id="tpSelectValid" class="secondary" type="button">${l("selectValid")}</button><button id="tpClear" class="secondary" type="button">${l("clear")}</button><button id="tpExport" class="secondary" type="button">${l("export")}</button></div>
          <div class="tp-table-wrap"><table class="monitor-table"><thead><tr>
            ${["select","time","model","concurrency","input","target","actualOutput","decode","ttft","status","view"].map((key) => `<th scope="col">${l(key)}</th>`).join("")}
          </tr></thead><tbody id="tpHistory"></tbody></table></div>
        </section>
      </div>
    </div>`;
}

export function details(element, item) {
  if (!item) { element.innerHTML = `<div class="tp-empty">${l("empty")}</div>`; return; }
  const m = item.metrics;
  const stats = [["ttft", durationMs(m.ttft_ms)], ["decode", `${number(m.decode_tps)} tok/s`], ["elapsed", `${number(item.elapsed_s, 2)} s`]];
  const fields = [["concurrency", number(item.config.concurrency || 1,0)], ["context", tokenLabel(item.config.input_tokens)], ["target", number(item.config.output_tokens,0)],
    ["actualInput", number(m.input_tokens,0)], ["actualOutput", number(m.output_tokens,0)], ["reasoning", number(m.reasoning_tokens,0)],
    ["ttftMax", durationMs(m.max_ttft_ms)], ["avgDecode", `${number(m.avg_decode_tps)} tok/s`],
    ["prefill", `${number(m.prefill_tps)} tok/s`], ["avgPrefill", `${number(m.avg_prefill_tps)} tok/s`]];
  if (item.simulated && item.simulation_speed) fields.push(["simulationSpeed", simulationSpeedLabel(item.simulation_speed)]);
  element.innerHTML = `<div class="tp-run-heading"><strong>${escape(item.config.model_name)}</strong><span class="tp-badge tp-${escape(item.status)}">${l(item.status)}</span>${item.simulated ? `<span class="tp-badge">${l("simulated")}</span>` : ""}</div>
    <div class="tp-stats">${stats.map(([key,value])=>`<div><span class="muted">${l(key)}</span><strong>${escape(value)}</strong></div>`).join("")}</div>
    <dl class="tp-facts">${fields.map(([key,value])=>`<div><dt>${l(key)}</dt><dd>${escape(value)}</dd></div>`).join("")}</dl>
    <p class="muted">${l("measurement")}</p>
    ${item.error ? `<p class="tp-error" role="alert">${escape(item.error)}</p>` : ""}
    ${item.persistence_error ? `<p class="tp-warning">${l("persistError")}</p>` : ""}`;
}

export function history(element, items, selected, viewed) {
  element.innerHTML = [...items].reverse().map((item) => `<tr class="${viewed === item.id ? "tp-viewed" : ""}">
    <td><input type="checkbox" data-select="${escape(item.id)}" aria-label="${l("select")} ${escape(formatTimestamp(item.started_at))}" ${selected.has(item.id) ? "checked" : ""}></td>
    <td>${escape(formatTimestamp(item.started_at))}</td><td>${escape(item.config.model_name)}</td><td>${number(item.config.concurrency || 1,0)}</td><td>${tokenLabel(item.config.input_tokens)}</td><td>${tokenLabel(item.config.output_tokens)}</td>
    <td>${number(item.metrics.output_tokens,0)}</td><td>${number(item.metrics.decode_tps)}</td><td>${durationMs(item.metrics.ttft_ms)}</td><td><span class="tp-badge tp-${escape(item.status)}">${l(item.status)}</span></td>
    <td><button class="secondary" type="button" data-view="${escape(item.id)}">${l("view")}</button> <button class="secondary" type="button" data-reuse="${escape(item.id)}">${l("reuse")}</button></td>
  </tr>`).join("") || `<tr><td colspan="11" class="tp-empty">${l("none")}</td></tr>`;
}
