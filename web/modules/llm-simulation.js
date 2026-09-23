import { getCurrentLanguage } from "./i18n.js?v=20260710-01";

export const simulationProfiles = { fast: [2000, 200], medium: [500, 50], slow: [100, 10] };
export const normalizeSimulationSpeed = value => Object.hasOwn(simulationProfiles, value) ? value : "fast";
const zh = () => getCurrentLanguage().startsWith("zh");
const names = { fast: ["快", "Fast"], medium: ["中", "Medium"], slow: ["慢", "Slow"] };
export function simulationSpeedLabel(speed) {
  const key = normalizeSimulationSpeed(speed);
  const [prefill, generation] = simulationProfiles[key];
  return `${names[key][zh() ? 0 : 1]} · ${zh() ? "预处理" : "Prefill"} ${prefill} / ${zh() ? "生成" : "Generation"} ${generation} tok/s`;
}

function mount() {
  let row = document.getElementById("llmSimulationRow");
  if (!row) {
    const anchor = document.getElementById("llmVirtualReplayRows");
    if (!anchor) return;
    row = document.createElement("div");
    row.id = "llmSimulationRow"; row.className = "form-row"; row.style.display = "none";
    row.innerHTML = '<label for="llmSimulationSpeed"></label><select id="llmSimulationSpeed"></select><div class="muted"></div>';
    anchor.after(row);
    window.addEventListener("wunder:language-changed", () => { renderSimulationConfig(readSimulationConfig()); });
  }
  return row;
}

export function renderSimulationConfig(speed) {
  const row = mount();
  if (!row) return;
  row.querySelector("label").textContent = zh() ? "模拟速度" : "Simulation speed";
  const select = row.querySelector("select");
  select.replaceChildren(...Object.keys(simulationProfiles).map(key => new Option(simulationSpeedLabel(key), key)));
  select.value = normalizeSimulationSpeed(speed);
  row.querySelector(".muted").textContent = zh()
    ? "默认快档。先模拟思考，再生成正文，两阶段使用同一生成速度；回放日志保留原有思考内容。"
    : "Fast by default. Simulated reasoning precedes the answer at the same generation rate. Replay preserves recorded reasoning.";
}

export function showSimulationConfig(visible) { const row = mount(); if (row) row.style.display = visible ? "" : "none"; }
export function readSimulationConfig() { return normalizeSimulationSpeed(document.getElementById("llmSimulationSpeed")?.value); }
