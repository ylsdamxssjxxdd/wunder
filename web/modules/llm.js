import { elements } from "./elements.js?v=20260113-02";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { appendLog } from "./log.js?v=20260108-02";
import { notify } from "./notify.js";
import { t } from "./i18n.js?v=20260113-01";

let contextProbeTimer = null;
let lastProbeKey = "";
let lastAutoContext = null;
let probeInFlight = false;
let pendingProbe = false;
const FLOAT_INPUT_PRECISION = 7;
const DEFAULT_PROVIDER_ID = "openai_compatible";
const PROVIDER_PRESETS = [
  { id: "openai_compatible", label: "openai_compatible", baseUrl: "" },
  { id: "openai", label: "openai", baseUrl: "https://api.openai.com/v1" },
  { id: "openrouter", label: "openrouter", baseUrl: "https://openrouter.ai/api/v1" },
  { id: "siliconflow", label: "siliconflow", baseUrl: "https://api.siliconflow.cn/v1" },
  { id: "deepseek", label: "deepseek", baseUrl: "https://api.deepseek.com" },
  { id: "moonshot", label: "moonshot", baseUrl: "https://api.moonshot.ai/v1" },
  { id: "qwen", label: "qwen", baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1" },
  { id: "groq", label: "groq", baseUrl: "https://api.groq.com/openai/v1" },
  { id: "mistral", label: "mistral", baseUrl: "https://api.mistral.ai/v1" },
  { id: "together", label: "together", baseUrl: "https://api.together.xyz/v1" },
  { id: "ollama", label: "ollama", baseUrl: "http://127.0.0.1:11434/v1" },
  { id: "lmstudio", label: "lmstudio", baseUrl: "http://127.0.0.1:1234/v1" },
];
const PROVIDER_PRESET_MAP = new Map(PROVIDER_PRESETS.map((item) => [item.id, item]));
const DEFAULT_BASE_URL_PLACEHOLDER =
  elements.llmBaseUrl?.getAttribute("placeholder") || "https://api.example.com";
let lastProviderSelection = DEFAULT_PROVIDER_ID;

const normalizeProviderId = (value) => {
  const raw = String(value || "").trim();
  if (!raw) {
    return DEFAULT_PROVIDER_ID;
  }
  const normalized = raw.toLowerCase().replace(/[\s-]+/g, "_");
  switch (normalized) {
    case "openai_compat":
      return "openai_compatible";
    case "openai_native":
      return "openai";
    case "silicon_flow":
      return "siliconflow";
    case "kimi":
      return "moonshot";
    case "dashscope":
      return "qwen";
    case "lm_studio":
      return "lmstudio";
    default:
      return normalized;
  }
};

const getProviderPreset = (provider) =>
  PROVIDER_PRESET_MAP.get(normalizeProviderId(provider)) || null;

const resolveProviderBaseUrl = (provider) => getProviderPreset(provider)?.baseUrl || "";

const renderProviderOptions = (activeProvider) => {
  if (!elements.llmProvider) {
    return;
  }
  const current = normalizeProviderId(activeProvider || elements.llmProvider.value);
  elements.llmProvider.textContent = "";
  if (current && !PROVIDER_PRESET_MAP.has(current)) {
    const option = document.createElement("option");
    option.value = current;
    option.textContent = current;
    elements.llmProvider.appendChild(option);
  }
  PROVIDER_PRESETS.forEach((item) => {
    const option = document.createElement("option");
    option.value = item.id;
    option.textContent = item.label;
    elements.llmProvider.appendChild(option);
  });
  if (current && elements.llmProvider.querySelector(`option[value="${current}"]`)) {
    elements.llmProvider.value = current;
  } else {
    elements.llmProvider.value = DEFAULT_PROVIDER_ID;
  }
};

const updateBaseUrlPlaceholder = (provider) => {
  if (!elements.llmBaseUrl) {
    return;
  }
  const preset = resolveProviderBaseUrl(provider);
  elements.llmBaseUrl.placeholder = preset || DEFAULT_BASE_URL_PLACEHOLDER;
};

const applyProviderDefaults = (provider, options = {}) => {
  const normalized = normalizeProviderId(provider);
  const presetBaseUrl = resolveProviderBaseUrl(normalized);
  const forceBaseUrl = options.forceBaseUrl === true;
  updateBaseUrlPlaceholder(normalized);
  if (!presetBaseUrl || !elements.llmBaseUrl) {
    return;
  }
  const currentValue = elements.llmBaseUrl.value.trim();
  const previousBaseUrl = resolveProviderBaseUrl(options.previousProvider || "");
  const shouldReplace =
    forceBaseUrl || !currentValue || (previousBaseUrl && currentValue === previousBaseUrl);
  if (shouldReplace) {
    elements.llmBaseUrl.value = presetBaseUrl;
  }
};

const roundFloat = (value) => {
  const factor = 10 ** FLOAT_INPUT_PRECISION;
  return Math.round(value * factor) / factor;
};

const trimTrailingZeros = (valueText) => {
  if (!valueText.includes(".")) {
    return valueText;
  }
  const trimmed = valueText.replace(/(?:\.0+|(\.\d*?[1-9])0+)$/, "$1").replace(/\.$/, "");
  return trimmed === "-0" ? "0" : trimmed;
};

const formatFloatForInput = (value, fallback) => {
  const num = Number.isFinite(value) ? value : fallback;
  if (!Number.isFinite(num)) {
    return "";
  }
  return trimTrailingZeros(roundFloat(num).toFixed(FLOAT_INPUT_PRECISION));
};

const parseFloatInput = (input, fallback) => {
  const parsed = Number.parseFloat(input?.value);
  if (!Number.isFinite(parsed)) {
    return fallback;
  }
  return roundFloat(parsed);
};

// 规范化 LLM 配置，避免空值影响展示。
const normalizeLlmConfig = (raw) => ({
  enable: raw?.enable !== false,
  provider: normalizeProviderId(raw?.provider || DEFAULT_PROVIDER_ID),
  base_url: raw?.base_url || "",
  api_key: raw?.api_key || "",
  model: raw?.model || "",
  temperature:
    typeof raw?.temperature === "number" && !Number.isNaN(raw.temperature) ? raw.temperature : 0.7,
  timeout_s:
    typeof raw?.timeout_s === "number" && !Number.isNaN(raw.timeout_s) ? raw.timeout_s : 60,
  retry: typeof raw?.retry === "number" && !Number.isNaN(raw.retry) ? raw.retry : 1,
  max_rounds:
    typeof raw?.max_rounds === "number" && !Number.isNaN(raw.max_rounds) ? raw.max_rounds : 10,
  max_context:
    typeof raw?.max_context === "number" && !Number.isNaN(raw.max_context) ? raw.max_context : null,
  max_output:
    typeof raw?.max_output === "number" && !Number.isNaN(raw.max_output) ? raw.max_output : null,
  support_vision: raw?.support_vision === true,
  stream: raw?.stream === true,
  stream_include_usage: raw?.stream_include_usage !== false,
  history_compaction_ratio:
    typeof raw?.history_compaction_ratio === "number" && !Number.isNaN(raw.history_compaction_ratio)
      ? raw.history_compaction_ratio
      : 0.8,
  history_compaction_reset: ["zero", "current", "keep"].includes(
    String(raw?.history_compaction_reset || "zero").trim()
  )
    ? String(raw?.history_compaction_reset || "zero").trim()
    : "zero",
  mock_if_unconfigured: raw?.mock_if_unconfigured !== false,
});

// 规范化多模型配置集合。
const normalizeLlmSet = (raw) => {
  const llm = raw || {};
  const models = llm.models || {};
  const normalizedModels = {};
  const order = [];
  Object.entries(models).forEach(([name, config]) => {
    const trimmed = String(name || "").trim();
    if (!trimmed) {
      return;
    }
    normalizedModels[trimmed] = normalizeLlmConfig(config || {});
    order.push(trimmed);
  });
  let defaultName = String(llm.default || "").trim();
  if (!defaultName || !normalizedModels[defaultName]) {
    defaultName = order[0] || "";
  }
  return { defaultName, models: normalizedModels, order };
};

const getDisplayName = (name) => state.llm.nameEdits?.[name] || name;

const resetProbeState = () => {
  lastProbeKey = "";
  lastAutoContext = null;
  pendingProbe = false;
  if (contextProbeTimer) {
    clearTimeout(contextProbeTimer);
    contextProbeTimer = null;
  }
};

const clearLlmForm = () => {
  if (elements.llmConfigName) {
    elements.llmConfigName.value = "";
  }
  renderProviderOptions(DEFAULT_PROVIDER_ID);
  elements.llmProvider.value = DEFAULT_PROVIDER_ID;
  elements.llmModel.value = "";
  elements.llmBaseUrl.value = "";
  elements.llmApiKey.value = "";
  elements.llmTemperature.value = formatFloatForInput(0.7, 0.7);
  elements.llmTimeout.value = 60;
  elements.llmRetry.value = 1;
  elements.llmMaxRounds.value = 10;
  elements.llmMaxContext.value = "";
  elements.llmMaxOutput.value = "";
  elements.llmVision.checked = false;
  elements.llmStreamIncludeUsage.checked = true;
  elements.llmHistoryCompactionRatio.value = formatFloatForInput(0.8, 0.8);
  elements.llmHistoryCompactionReset.value = "zero";
  applyProviderDefaults(DEFAULT_PROVIDER_ID, { forceBaseUrl: false });
  lastProviderSelection = DEFAULT_PROVIDER_ID;
};

// 将 LLM 配置渲染到表单。
const applyLlmConfigToForm = (name, config) => {
  if (!name || !config) {
    clearLlmForm();
    return;
  }
  const llm = normalizeLlmConfig(config || {});
  if (elements.llmConfigName) {
    elements.llmConfigName.value = getDisplayName(name);
  }
  renderProviderOptions(llm.provider);
  elements.llmProvider.value = llm.provider;
  elements.llmModel.value = llm.model;
  elements.llmBaseUrl.value = llm.base_url;
  elements.llmApiKey.value = llm.api_key;
  elements.llmTemperature.value = formatFloatForInput(llm.temperature, 0.7);
  elements.llmTimeout.value = llm.timeout_s;
  elements.llmRetry.value = llm.retry;
  elements.llmMaxRounds.value = llm.max_rounds ?? 10;
  elements.llmMaxContext.value = llm.max_context ?? "";
  elements.llmMaxOutput.value = llm.max_output ?? "";
  elements.llmVision.checked = llm.support_vision;
  elements.llmStreamIncludeUsage.checked = llm.stream_include_usage === true;
  elements.llmHistoryCompactionRatio.value = formatFloatForInput(
    llm.history_compaction_ratio ?? 0.8,
    0.8
  );
  elements.llmHistoryCompactionReset.value = llm.history_compaction_reset || "zero";
  applyProviderDefaults(llm.provider, {
    forceBaseUrl: !llm.base_url,
    previousProvider: lastProviderSelection,
  });
  lastProviderSelection = llm.provider;
};

const updateDetailHeader = () => {
  const activeName = state.llm.activeName;
  const config = state.llm.configs[activeName];
  if (!activeName || !config) {
    if (elements.llmDetailTitle) {
      elements.llmDetailTitle.textContent = t("llm.detail.emptyTitle");
    }
    if (elements.llmDetailMeta) {
      elements.llmDetailMeta.textContent = t("llm.detail.emptyMeta");
    }
    if (elements.llmSetDefaultBtn) {
      elements.llmSetDefaultBtn.disabled = true;
      elements.llmSetDefaultBtn.classList.remove("llm-default-btn");
    }
    if (elements.llmDeleteBtn) {
      elements.llmDeleteBtn.disabled = true;
    }
    return;
  }
  const title = getDisplayName(activeName);
  if (elements.llmDetailTitle) {
    elements.llmDetailTitle.textContent = title;
  }
  if (elements.llmDetailMeta) {
    const parts = [];
    if (activeName === state.llm.defaultName) {
      parts.push(t("llm.default"));
    }
    if (config.model) {
      parts.push(t("llm.modelLabel", { model: config.model }));
    }
    if (config.base_url) {
      parts.push(config.base_url);
    }
    elements.llmDetailMeta.textContent = parts.join(" · ") || t("llm.detail.selected");
  }
  if (elements.llmSetDefaultBtn) {
    const isDefault = activeName === state.llm.defaultName;
    elements.llmSetDefaultBtn.disabled = isDefault;
    elements.llmSetDefaultBtn.classList.toggle("llm-default-btn", isDefault);
  }
  if (elements.llmDeleteBtn) {
    elements.llmDeleteBtn.disabled = state.llm.order.length <= 1;
  }
};

// 渲染模型配置列表，支持默认标记与当前选中状态。
const renderLlmList = () => {
  if (!elements.llmConfigList) {
    return;
  }
  elements.llmConfigList.textContent = "";
  if (!state.llm.order.length) {
    elements.llmConfigList.textContent = t("llm.list.empty");
    return;
  }
  state.llm.order.forEach((name) => {
    const config = state.llm.configs[name];
    const item = document.createElement("button");
    item.type = "button";
    item.className = "list-item";
    if (name === state.llm.activeName) {
      item.classList.add("active");
    }

    const title = document.createElement("div");
    title.className = "llm-list-item-title";
    const titleText = document.createElement("span");
    titleText.textContent = getDisplayName(name);
    title.appendChild(titleText);
    if (name === state.llm.defaultName) {
      const badge = document.createElement("span");
      badge.className = "llm-default-tag";
      badge.innerHTML = `<i class="fa-solid fa-star"></i>${t("llm.defaultBadge")}`;
      title.appendChild(badge);
    }

    const meta = document.createElement("small");
    const metaParts = [];
    if (config?.model) {
      metaParts.push(config.model);
    }
    if (config?.base_url) {
      metaParts.push(config.base_url);
    }
    meta.textContent = metaParts.join(" · ") || t("llm.meta.empty");

    item.appendChild(title);
    item.appendChild(meta);
    item.addEventListener("click", () => {
      selectLlmConfig(name);
    });
    elements.llmConfigList.appendChild(item);
  });
};

// 从表单构建 LLM 配置。
const buildLlmConfigFromForm = (baseConfig) => {
  const base = normalizeLlmConfig(baseConfig || {});
  const temperature = parseFloatInput(elements.llmTemperature, 0.7);
  const timeout = Number.parseInt(elements.llmTimeout.value, 10);
  const retry = Number.parseInt(elements.llmRetry.value, 10);
  const maxRounds = Number.parseInt(elements.llmMaxRounds.value, 10);
  const maxContext = Number.parseInt(elements.llmMaxContext.value, 10);
  const maxOutput = Number.parseInt(elements.llmMaxOutput.value, 10);
  const historyCompactionRatio = parseFloatInput(elements.llmHistoryCompactionRatio, 0.8);
  const historyCompactionReset = String(
    elements.llmHistoryCompactionReset.value || ""
  ).trim();
  return {
    enable: base.enable,
    provider: normalizeProviderId(elements.llmProvider.value || base.provider),
    base_url: elements.llmBaseUrl.value.trim(),
    api_key: elements.llmApiKey.value.trim(),
    model: elements.llmModel.value.trim(),
    temperature: Number.isFinite(temperature) ? temperature : 0.7,
    timeout_s: Number.isFinite(timeout) ? timeout : 60,
    retry: Number.isFinite(retry) ? retry : 1,
    max_rounds: Number.isFinite(maxRounds) && maxRounds > 0 ? maxRounds : base.max_rounds ?? 10,
    max_context: Number.isFinite(maxContext) && maxContext > 0 ? maxContext : null,
    max_output: Number.isFinite(maxOutput) && maxOutput > 0 ? maxOutput : null,
    support_vision: elements.llmVision.checked,
    stream: base.stream,
    stream_include_usage: elements.llmStreamIncludeUsage.checked,
    history_compaction_ratio:
      Number.isFinite(historyCompactionRatio) && historyCompactionRatio > 0
        ? historyCompactionRatio
        : base.history_compaction_ratio ?? 0.8,
    history_compaction_reset: ["zero", "current", "keep"].includes(historyCompactionReset)
      ? historyCompactionReset
      : base.history_compaction_reset ?? "zero",
    mock_if_unconfigured: base.mock_if_unconfigured,
  };
};

// 将当前表单内容写回状态，避免切换时丢失编辑内容。
const syncActiveConfigToState = () => {
  const activeName = state.llm.activeName;
  if (!activeName || !state.llm.configs[activeName]) {
    return;
  }
  state.llm.configs[activeName] = buildLlmConfigFromForm(state.llm.configs[activeName]);
};

const selectLlmConfig = (name) => {
  if (!name || name === state.llm.activeName) {
    return;
  }
  syncActiveConfigToState();
  state.llm.activeName = name;
  resetProbeState();
  applyLlmConfigToForm(name, state.llm.configs[name]);
  renderLlmList();
  updateDetailHeader();
};

// 构建模型上下文探测请求体。
const buildContextProbePayload = () => {
  const provider = normalizeProviderId(elements.llmProvider.value || DEFAULT_PROVIDER_ID);
  const baseUrl = elements.llmBaseUrl.value.trim() || resolveProviderBaseUrl(provider);
  const model = elements.llmModel.value.trim();
  const apiKey = elements.llmApiKey.value.trim();
  if (!baseUrl || !model) {
    return null;
  }
  return {
    provider,
    base_url: baseUrl,
    api_key: apiKey,
    model,
    timeout_s: 15,
  };
};

// 判断是否需要覆盖当前 max_context 输入。
const shouldApplyContextValue = (probeKey, value) => {
  const currentValue = elements.llmMaxContext.value.trim();
  if (!currentValue) {
    return true;
  }
  if (probeKey !== lastProbeKey) {
    return true;
  }
  return lastAutoContext !== null && String(currentValue) === String(lastAutoContext);
};

// 请求模型最大上下文长度。
const requestContextWindow = async (force = false) => {
  if (probeInFlight) {
    pendingProbe = true;
    return;
  }
  const payload = buildContextProbePayload();
  if (!payload) {
    return;
  }
  const probeKey = `${payload.provider}|${payload.base_url}|${payload.model}|${payload.api_key ? 1 : 0}`;
  if (!force && probeKey === lastProbeKey && lastAutoContext !== null) {
    return;
  }
  probeInFlight = true;
  try {
    const wunderBase = getWunderBase();
    const endpoint = `${wunderBase}/admin/llm/context_window`;
    const response = await fetch(endpoint, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(payload),
    });
    if (!response.ok) {
      throw new Error(t("common.requestFailed", { status: response.status }));
    }
    const result = await response.json();
    const latestPayload = buildContextProbePayload();
    const latestKey = latestPayload
      ? `${latestPayload.provider}|${latestPayload.base_url}|${latestPayload.model}|${
          latestPayload.api_key ? 1 : 0
        }`
      : "";
    if (latestKey && latestKey !== probeKey) {
      return;
    }
    if (Number.isFinite(result.max_context) && result.max_context > 0) {
      if (shouldApplyContextValue(probeKey, result.max_context)) {
        elements.llmMaxContext.value = String(result.max_context);
        lastAutoContext = result.max_context;
        lastProbeKey = probeKey;
        appendLog(t("llm.contextProbe.auto", { value: result.max_context }));
        if (force) {
          notify(t("llm.contextProbe.auto", { value: result.max_context }), "info");
        }
      }
      return;
    }
    lastProbeKey = probeKey;
    if (result.message) {
      appendLog(t("llm.contextProbe.result", { message: result.message }));
    }
  } catch (error) {
    appendLog(t("llm.contextProbe.failed", { message: error.message }));
  } finally {
    probeInFlight = false;
    if (pendingProbe) {
      pendingProbe = false;
      setTimeout(() => {
        requestContextWindow(true);
      }, 0);
    }
  }
};

// 延迟触发探测，避免频繁请求。
const scheduleContextProbe = () => {
  if (contextProbeTimer) {
    clearTimeout(contextProbeTimer);
  }
  contextProbeTimer = setTimeout(() => {
    requestContextWindow(false);
  }, 600);
};

const renderDebugModelOptions = () => {
  if (!elements.debugModelName) {
    return;
  }
  const select = elements.debugModelName;
  const currentValue = select.value;
  select.textContent = "";
  const defaultOption = document.createElement("option");
  const defaultLabel = state.llm.defaultName
    ? t("llm.defaultWithName", { name: state.llm.defaultName })
    : t("llm.default");
  defaultOption.value = "";
  defaultOption.textContent = defaultLabel;
  select.appendChild(defaultOption);
  state.llm.order.forEach((name) => {
    const option = document.createElement("option");
    option.value = name;
    option.textContent = name;
    select.appendChild(option);
  });
  if (currentValue && select.querySelector(`option[value="${currentValue}"]`)) {
    select.value = currentValue;
  }
};

const applyLlmSet = (raw, options = {}) => {
  const normalized = normalizeLlmSet(raw || {});
  if (!normalized.order.length) {
    // 首次无模型配置时，模拟点击新增的状态，避免表单无法保存。
    const baseName = t("llm.newName");
    let name = baseName;
    let index = 1;
    while (normalized.models[name]) {
      name = `${baseName}${index}`;
      index += 1;
    }
    normalized.models[name] = normalizeLlmConfig({});
    normalized.order = [name];
    normalized.defaultName = name;
  }
  resetProbeState();
  state.llm.configs = normalized.models;
  state.llm.order = normalized.order;
  state.llm.defaultName = normalized.defaultName;
  state.llm.loaded = true;
  state.llm.nameEdits = {};
  const desiredActive = state.llm.activeName;
  state.llm.activeName =
    (desiredActive && normalized.models[desiredActive] && desiredActive) ||
    normalized.defaultName ||
    normalized.order[0] ||
    "";
  renderLlmList();
  updateDetailHeader();
  if (state.llm.activeName && state.llm.configs[state.llm.activeName]) {
    applyLlmConfigToForm(state.llm.activeName, state.llm.configs[state.llm.activeName]);
  } else {
    clearLlmForm();
  }
  if (options.syncDebug) {
    renderDebugModelOptions();
  }
  if (typeof window !== "undefined") {
    window.dispatchEvent(
      new CustomEvent("wunder:llm-updated", {
        detail: {
          defaultName: state.llm.defaultName,
          order: [...state.llm.order],
        },
      })
    );
  }
};

// 获取当前 LLM 配置。
export const loadLlmConfig = async (options = {}) => {
  if (state.llm.loaded && options.force !== true) {
    renderLlmList();
    updateDetailHeader();
    renderDebugModelOptions();
    if (state.llm.activeName && state.llm.configs[state.llm.activeName]) {
      applyLlmConfigToForm(state.llm.activeName, state.llm.configs[state.llm.activeName]);
    }
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/llm`;
  const response = await fetch(endpoint);
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const result = await response.json();
  applyLlmSet(result.llm || {}, { syncDebug: true });
};

export const ensureLlmConfigLoaded = async () => {
  if (state.llm.loaded) {
    renderDebugModelOptions();
    return;
  }
  await loadLlmConfig();
};

const commitActiveConfigEdits = () => {
  const activeName = state.llm.activeName;
  if (!activeName) {
    throw new Error(t("llm.error.selectFirst"));
  }
  const desiredName = String(elements.llmConfigName?.value || "").trim();
  if (!desiredName) {
    throw new Error(t("llm.error.nameRequired"));
  }
  if (desiredName !== activeName && state.llm.configs[desiredName]) {
    throw new Error(t("llm.error.nameExists"));
  }
  syncActiveConfigToState();
  const currentConfig = state.llm.configs[activeName] || normalizeLlmConfig({});
  if (desiredName !== activeName) {
    delete state.llm.configs[activeName];
    state.llm.configs[desiredName] = currentConfig;
    state.llm.order = state.llm.order.map((item) =>
      item === activeName ? desiredName : item
    );
    if (state.llm.defaultName === activeName) {
      state.llm.defaultName = desiredName;
    }
    delete state.llm.nameEdits[activeName];
  } else {
    state.llm.configs[activeName] = currentConfig;
    delete state.llm.nameEdits[activeName];
  }
  if (!state.llm.defaultName) {
    state.llm.defaultName = desiredName;
  }
  state.llm.activeName = desiredName;
};

const buildLlmPayload = () => {
  const models = {};
  state.llm.order.forEach((name) => {
    if (state.llm.configs[name]) {
      models[name] = state.llm.configs[name];
    }
  });
  const defaultName = state.llm.defaultName || state.llm.order[0] || "";
  return { default: defaultName, models };
};

// 保存 LLM 配置。
export const saveLlmConfig = async () => {
  commitActiveConfigEdits();
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/llm`;
  const payload = buildLlmPayload();
  const response = await fetch(endpoint, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ llm: payload }),
  });
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const result = await response.json();
  applyLlmSet(result.llm || {}, { syncDebug: true });
};

const handleAddConfig = () => {
  syncActiveConfigToState();
  const baseName = t("llm.newName");
  let name = baseName;
  let index = 1;
  while (state.llm.configs[name]) {
    name = `${baseName}${index}`;
    index += 1;
  }
  state.llm.configs[name] = normalizeLlmConfig({});
  state.llm.order.push(name);
  if (!state.llm.defaultName) {
    state.llm.defaultName = name;
  }
  state.llm.activeName = name;
  resetProbeState();
  renderLlmList();
  applyLlmConfigToForm(name, state.llm.configs[name]);
  updateDetailHeader();
  appendLog(t("llm.added", { name }));
};

const handleDeleteConfig = () => {
  const activeName = state.llm.activeName;
  if (!activeName) {
    return;
  }
  if (state.llm.order.length <= 1) {
    notify(t("llm.error.keepOne"), "warn");
    return;
  }
  const confirmed = window.confirm(t("llm.deleteConfirm", { name: activeName }));
  if (!confirmed) {
    return;
  }
  delete state.llm.configs[activeName];
  delete state.llm.nameEdits[activeName];
  state.llm.order = state.llm.order.filter((name) => name !== activeName);
  if (state.llm.defaultName === activeName) {
    state.llm.defaultName = state.llm.order[0] || "";
  }
  state.llm.activeName = state.llm.defaultName || state.llm.order[0] || "";
  resetProbeState();
  renderLlmList();
  if (state.llm.activeName && state.llm.configs[state.llm.activeName]) {
    applyLlmConfigToForm(state.llm.activeName, state.llm.configs[state.llm.activeName]);
  } else {
    clearLlmForm();
  }
  updateDetailHeader();
  appendLog(t("llm.removed", { name: activeName }));
};

const handleSetDefault = () => {
  const activeName = state.llm.activeName;
  if (!activeName) {
    return;
  }
  state.llm.defaultName = activeName;
  renderLlmList();
  updateDetailHeader();
  appendLog(t("llm.setDefault", { name: activeName }));
  notify(t("llm.setDefault", { name: activeName }), "info");
};

const handleNameEdit = () => {
  const activeName = state.llm.activeName;
  if (!activeName) {
    return;
  }
  const value = String(elements.llmConfigName?.value || "").trim();
  if (!value) {
    delete state.llm.nameEdits[activeName];
  } else {
    state.llm.nameEdits[activeName] = value;
  }
  renderLlmList();
  updateDetailHeader();
};

// 初始化模型配置面板交互。
export const initLlmPanel = () => {
  renderProviderOptions();
  updateBaseUrlPlaceholder(DEFAULT_PROVIDER_ID);
  lastProviderSelection = normalizeProviderId(elements.llmProvider.value || DEFAULT_PROVIDER_ID);
  elements.saveLlmBtn.addEventListener("click", async () => {
    try {
      await saveLlmConfig();
      appendLog(t("llm.saved"));
      notify(t("llm.saved"), "success");
    } catch (error) {
      appendLog(t("llm.saveFailed", { message: error.message }));
      notify(t("llm.saveFailed", { message: error.message }), "error");
    }
  });

  elements.llmAddBtn?.addEventListener("click", handleAddConfig);
  elements.llmDeleteBtn?.addEventListener("click", handleDeleteConfig);
  elements.llmSetDefaultBtn?.addEventListener("click", handleSetDefault);
  elements.llmConfigName?.addEventListener("input", handleNameEdit);
  elements.llmProbeContextBtn?.addEventListener("click", () => {
    // 手动触发最大上下文探测，缺少必要字段时给出提示
    if (!buildContextProbePayload()) {
      notify(t("llm.error.probeMissing"), "warn");
      return;
    }
    requestContextWindow(true);
  });

  const handleProbeInput = () => scheduleContextProbe();
  elements.llmBaseUrl.addEventListener("input", handleProbeInput);
  elements.llmModel.addEventListener("input", handleProbeInput);
  elements.llmApiKey.addEventListener("input", handleProbeInput);
  elements.llmProvider.addEventListener("change", () => {
    const nextProvider = normalizeProviderId(elements.llmProvider.value);
    elements.llmProvider.value = nextProvider;
    applyProviderDefaults(nextProvider, { previousProvider: lastProviderSelection });
    lastProviderSelection = nextProvider;
    handleProbeInput();
  });
  elements.llmBaseUrl.addEventListener("blur", () => requestContextWindow(true));
  elements.llmModel.addEventListener("blur", () => requestContextWindow(true));
};




