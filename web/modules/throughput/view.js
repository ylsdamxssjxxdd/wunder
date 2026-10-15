// AI生成
import { escapeHtml, formatTimestamp } from "../utils.js?v=20251229-02";
import { label as l } from "./copy.js?v=20261012-01";
import { speedLabel, tokenLabel, samplePoints } from "./chart.js?v=20261012-01";
import { simulationSpeedLabel } from "../llm-simulation.js?v=20260923-02";

const escape = (value) => escapeHtml(String(value ?? ""));
export const number = (value, digits = 1) => value == null || !Number.isFinite(Number(value)) ? "—" : Number(value).toLocaleString(undefined, { maximumFractionDigits: digits });
export const durationMs = (value) => {
  if (value == null || !Number.isFinite(Number(value))) return "—";
  const ms = Number(value);
  return ms >= 1000 ? `${number(ms / 1000, 2)} s` : `${number(ms, 0)} ms`;
};
const options = (values) => values.map((value) => `<option value="${value}">${tokenLabel(value)}</option>`).join("");
const numberField = (id, label, value, presets, hint) => `<div class="form-row" title="${escape(hint)}"><label for="${id}">${label}</label><input id="${id}" type="number" min="1" step="1" value="${value}" list="${id}Presets" inputmode="numeric" required><datalist id="${id}Presets">${options(presets)}</datalist></div>`;

export function mount(panel) {
  panel.innerHTML = `
    <div class="list-header"><div><h1>${l("title")}</h1></div>
      <button id="tpRefresh" class="secondary" type="button">${l("refresh")}</button></div>
    <div class="tp-layout">
      <aside class="tp-config monitor-block">
        <h2>${l("scenario")}</h2>
        <form id="tpForm">
          <div class="form-row"><label for="tpModel">${l("model")}</label><select id="tpModel" required></select></div>
          <div class="form-row" title="${escape(l("concurrencyListHint"))}"><label for="tpConcurrencyList">${l("concurrencyList")}</label><input id="tpConcurrencyList" type="text" value="1,2,4,8" inputmode="numeric" autocomplete="off" required></div>
          ${numberField("tpInput", l("input"), 8192, [1024,2048,8192,16384,32768,65536,131072,262144,524288,1048576], l("inputHint"))}
          ${numberField("tpOutput", l("output"), 128, [1024,2048,4096,8192], l("outputHint"))}
          <button id="tpStart" type="submit">${l("start")}</button>
          <button id="tpStop" type="button" class="secondary" hidden>${l("stop")}</button>
        </form>
        <p id="tpFeedback" role="status" aria-live="polite" class="muted"></p>
      </aside>
      <div class="tp-results">
        <section class="monitor-block tp-details">
          <div class="tp-section-header"><h2>${l("details")}</h2><span id="tpConnection" class="muted"></span><button id="tpCurrent" class="secondary" type="button" hidden>${l("current")}</button></div>
          <div id="tpDetail"></div>
        </section>
        <section class="monitor-block tp-comparison">
          <div class="tp-section-header"><h2>${l("comparison")}</h2></div>
          <p class="muted">${l("comparisonHint")}</p>
          <div id="tpChart" role="img" aria-label="${l("comparison")}"></div><p id="tpChartEmpty" class="muted"></p>
        </section>
        <section class="monitor-block tp-history">
          <div class="tp-section-header"><h2>${l("history")}</h2><span id="tpViewed" class="muted"></span>
            <button id="tpExport" class="secondary" type="button">${l("exportCurrent")}</button></div>
          <div class="tp-table-wrap"><table class="monitor-table"><thead><tr>
            ${["time","model","concurrencyList","input","target","actualOutput","decode","ttft","status","reuse"].map((key) => `<th scope="col">${l(key)}</th>`).join("")}
          </tr></thead><tbody id="tpHistory"></tbody></table></div>
        </section>
      </div>
    </div>`;
}

export function details(element, item) {
  if (!item) { element.innerHTML = `<div class="tp-empty">${l("empty")}</div>`; return; }
  const headings = ["concurrency", "status", "actualInput", "actualOutput", "decode", "avgDecode", "prefill", "avgPrefill", "ttft", "ttftMax", "elapsed"];
  const rows = samplePoints(item).map((point) => {
    const m = point.metrics || {};
    return `<tr>
      <td>${number(point.concurrency, 0)}</td>
      <td><span class="tp-badge tp-${escape(point.status)}">${l(point.status)}</span></td>
      <td>${tokenLabel(m.input_tokens)}</td>
      <td>${tokenLabel(m.output_tokens)}</td>
      <td>${speedLabel(m.decode_tps)}</td>
      <td>${speedLabel(m.avg_decode_tps)}</td>
      <td>${speedLabel(m.prefill_tps)}</td>
      <td>${speedLabel(m.avg_prefill_tps)}</td>
      <td>${durationMs(m.ttft_ms)}</td>
      <td>${durationMs(m.max_ttft_ms)}</td>
      <td>${point.elapsed_s == null ? "—" : `${number(point.elapsed_s, 2)} s`}</td>
    </tr>`;
  }).join("");
  element.innerHTML = `<div class="tp-run-heading"><strong>${escape(item.config.model_name)}</strong><span class="tp-badge tp-${escape(item.status)}">${l(item.status)}</span>${item.simulated ? `<span class="tp-badge">${l("simulated")}</span>` : ""}${item.simulated && item.simulation_speed ? `<span class="muted">${l("simulationSpeed")}: ${escape(simulationSpeedLabel(item.simulation_speed))}</span>` : ""}<span class="muted">${l("concurrencyList")}: ${escape((item.config.concurrency_list || [item.config.concurrency || 1]).join(", "))}</span></div>
    <div class="tp-table-wrap"><table class="monitor-table tp-detail-table"><thead><tr>${headings.map((key) => `<th scope="col">${l(key)}</th>`).join("")}</tr></thead><tbody>${rows || `<tr><td colspan="${headings.length}" class="tp-empty">${l("none")}</td></tr>`}</tbody></table></div>
    ${item.error ? `<p class="tp-error" role="alert">${escape(item.error)}</p>` : ""}
    ${item.persistence_error ? `<p class="tp-warning">${l("persistError")}</p>` : ""}`;
}

export function history(element, items, viewed) {
  element.innerHTML = [...items].reverse().map((item) => `<tr class="tp-row${viewed === item.id ? " tp-viewed" : ""}" data-view="${escape(item.id)}" tabindex="0" role="button" aria-label="${l("view")} ${escape(formatTimestamp(item.started_at))}">
    <td>${escape(formatTimestamp(item.started_at))}</td><td>${escape(item.config.model_name)}</td><td>${escape((item.config.concurrency_list || [item.config.concurrency || 1]).join(", "))}</td><td>${tokenLabel(item.config.input_tokens)}</td><td>${tokenLabel(item.config.output_tokens)}</td>
    <td>${tokenLabel(item.metrics.output_tokens)}</td><td>${speedLabel(item.metrics.decode_tps)}</td><td>${durationMs(item.metrics.ttft_ms)}</td><td><span class="tp-badge tp-${escape(item.status)}">${l(item.status)}</span></td>
    <td><button class="secondary" type="button" data-reuse="${escape(item.id)}">${l("reuse")}</button></td>
  </tr>`).join("") || `<tr><td colspan="10" class="tp-empty">${l("none")}</td></tr>`;
}
