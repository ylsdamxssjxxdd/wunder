import { getCurrentLanguage } from "./i18n.js?v=20260710-01";

export const simulationProfiles = { fast: [2000, 200], medium: [500, 50], slow: [100, 10] };
export const normalizeSimulationSpeed = value => Object.hasOwn(simulationProfiles, value) ? value : "fast";
const zh = () => getCurrentLanguage().startsWith("zh");
const names = { fast: ["快", "Fast"], medium: ["中", "Medium"], slow: ["慢", "Slow"] };
export function simulationSpeedLabel(speed) {
  const key = normalizeSimulationSpeed(speed);
  const [prefill, generation] = simulationProfiles[key];
  return `${names[key][zh() ? 0 : 1]} · ${zh() ? "预填充" : "Prefill"} ${prefill} / ${zh() ? "生成" : "Generation"} ${generation} tok/s`;
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
    window.addEventListener("wunder:language-changed", () => {
      try { renderSimulationConfig(readSimulationConfig(), readSimulationOptions()); } catch { /* Preserve invalid edits for correction. */ }
    });
  }
  return row;
}

export function renderSimulationConfig(speed, options = {}) {
  const row = mount();
  if (!row) return;
  row.querySelector("label").textContent = zh() ? "模拟速度" : "Simulation speed";
  const select = row.querySelector("select");
  select.replaceChildren(...Object.keys(simulationProfiles).map(key => new Option(simulationSpeedLabel(key), key)));
  select.value = normalizeSimulationSpeed(speed);
  row.querySelector(".muted").textContent = zh()
    ? "默认快档。启用思考时先思考再生成正文，两阶段使用同一速度；回放同样受能力和预算限制。"
    : "Fast by default. Enabled reasoning precedes the answer at the same rate. Replay respects capabilities and budgets.";
  renderCapabilities(options);
}

export function showSimulationConfig(visible) {
  const row = mount();
  if (row) row.style.display = visible ? "" : "none";
  const capabilities = document.getElementById("llmSimulationCapabilities");
  if (capabilities) capabilities.style.display = visible ? "" : "none";
}
export function readSimulationConfig() { return normalizeSimulationSpeed(document.getElementById("llmSimulationSpeed")?.value); }

function renderCapabilities(options) {
  let panel = document.getElementById("llmSimulationCapabilities");
  if (!panel) {
    panel = document.createElement("div");
    panel.id = "llmSimulationCapabilities";
    document.getElementById("llmSimulationRow").after(panel);
  }
  const text = (cn, en) => zh() ? cn : en;
  panel.innerHTML = `
    <div class="muted">${text("模拟能力复用下方的最大上下文、最大输出、视觉、听觉和思考预算。留空时上下文为 128k、输出为 4k。输入（含工具定义）加预留输出超限时返回 context_length_exceeded。", "Simulation uses the context, output, vision, hearing and reasoning settings below. Empty limits default to 128k context and 4k output. Input (including tool schemas) plus reserved output exceeding context returns context_length_exceeded.")}</div>
    <div class="form-row"><div class="checkbox-row-group">
      <label class="checkbox-row checkbox-compact"><input id="llmSimulationReasoning" type="checkbox"><span>${text("模拟思考能力", "Reasoning support")}</span></label>
      <label class="checkbox-row checkbox-compact"><input id="llmSimulationTools" type="checkbox"><span>${text("支持工具调用", "Tool calling support")}</span></label>
    </div></div><div class="grid">
    <div class="form-row"><label for="llmSimulationImageTokens">${text("每张图片 token", "Tokens per image")}</label><input id="llmSimulationImageTokens" type="number" min="1" max="4294967295" step="1"></div>
    <div class="form-row"><label for="llmSimulationAudioTokens">${text("每段音频 token", "Tokens per audio part")}</label><input id="llmSimulationAudioTokens" type="number" min="1" max="4294967295" step="1"></div>
    </div><div class="muted">${text("回放按下方“工具调用方式”模拟模型协议，工具名与参数来自日志。function_call 使用原生工具调用，tool_call 使用文本协议，freeform_call 沿用原生或文本回退方式。工具执行仍遵守权限与审批。合成回复不主动调用工具。媒体仅模拟能力校验、耗时和用量。", "Replay follows the selected tool calling mode using names and arguments from the log: native function_call, text tool_call, or native/text fallback for freeform_call. Execution respects permissions and approvals. Synthetic replies do not initiate tools. Media simulates validation, timing and usage.")}</div>`;
  document.getElementById("llmSimulationReasoning").checked = options?.support_reasoning !== false;
  document.getElementById("llmSimulationTools").checked = options?.support_tools !== false;
  document.getElementById("llmSimulationImageTokens").value = options?.image_tokens ?? 256;
  document.getElementById("llmSimulationAudioTokens").value = options?.audio_tokens ?? 1024;
}

export function readSimulationOptions() {
  const integer = id => {
    const value = Number(document.getElementById(id)?.value);
    if (!Number.isInteger(value) || value < 1 || value > 4294967295) throw new Error(zh() ? "媒体 token 数必须是正整数" : "Media tokens must be positive integers");
    return value;
  };
  const support_tools = document.getElementById("llmSimulationTools")?.checked !== false;
  return { support_tools, support_reasoning: document.getElementById("llmSimulationReasoning")?.checked !== false, image_tokens: integer("llmSimulationImageTokens"), audio_tokens: integer("llmSimulationAudioTokens") };
}
