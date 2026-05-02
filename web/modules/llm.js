import { elements } from "./elements.js?v=20260215-01";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { appendLog } from "./log.js?v=20260108-02";
import { notify } from "./notify.js";
import { t } from "./i18n.js?v=20260215-01";

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
  { id: "anthropic", label: "anthropic", baseUrl: "https://api.anthropic.com/v1" },
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
    case "claude":
    case "anthropic_api":
      return "anthropic";
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

const resolveDefaultToolCallMode = (provider) =>
  normalizeProviderId(provider) === "openai" ? "freeform_call" : "function_call";

const TOOL_CALL_MODE_OPTIONS = new Set(["tool_call", "function_call", "freeform_call"]);
const normalizeToolCallMode = (value, provider) => {
  const raw = String(value || "").trim();
  if (!raw) {
    return resolveDefaultToolCallMode(provider);
  }
  const normalized = raw.toLowerCase().replace(/[\s-]+/g, "_");
  if (normalized === "function" || normalized === "functioncall" || normalized === "fc") {
    return "function_call";
  }
  if (
    normalized === "freeform" ||
    normalized === "freeformcall" ||
    normalized === "custom_tool_call"
  ) {
    return "freeform_call";
  }
  if (
    normalized === "tool" ||
    normalized === "toolcall" ||
    normalized === "tag" ||
    normalized === "xml"
  ) {
    return "tool_call";
  }
  return TOOL_CALL_MODE_OPTIONS.has(normalized)
    ? normalized
    : resolveDefaultToolCallMode(provider);
};

const normalizeReasoningEffort = (value) => {
  const raw = String(value || "").trim();
  if (!raw) {
    return "";
  }
  const normalized = raw.toLowerCase().replace(/[\s-]+/g, "_");
  if (
    normalized === "default" ||
    normalized === "auto" ||
    normalized === "inherit"
  ) {
    return "";
  }
  if (
    normalized === "none" ||
    normalized === "off" ||
    normalized === "disable" ||
    normalized === "disabled"
  ) {
    return "none";
  }
  if (normalized === "minimal" || normalized === "min") {
    return "minimal";
  }
  if (normalized === "low") return "low";
  if (normalized === "medium" || normalized === "med" || normalized === "normal") {
    return "medium";
  }
  if (normalized === "high") return "high";
  if (
    normalized === "xhigh" ||
    normalized === "x_high" ||
    normalized === "extra_high" ||
    normalized === "very_high"
  ) {
    return "xhigh";
  }
  return "";
};

const MODEL_TYPE_OPTIONS = new Set(["llm", "embedding", "tts", "image"]);
const normalizeModelType = (value) => {
  const raw = String(value || "").trim().toLowerCase();
  if (!raw) {
    return "llm";
  }
  const normalized = raw.replace(/[\s-]+/g, "_");
  if (normalized === "embed" || normalized === "emb" || normalized === "embeddings") {
    return "embedding";
  }
  if (
    normalized === "tts" ||
    normalized === "speech" ||
    normalized === "text_to_speech" ||
    normalized === "text2speech" ||
    normalized === "audio_speech"
  ) {
    return "tts";
  }
  if (
    normalized === "image" ||
    normalized === "draw" ||
    normalized === "drawing" ||
    normalized === "text_to_image" ||
    normalized === "text2image" ||
    normalized === "image_generation"
  ) {
    return "image";
  }
  return MODEL_TYPE_OPTIONS.has(normalized) ? normalized : "llm";
};

const isLlmConfig = (config) => normalizeModelType(config?.model_type) === "llm";

const resolveDefaultLlmName = (desiredName, models, order) => {
  const desired = String(desiredName || "").trim();
  if (desired && models[desired] && isLlmConfig(models[desired])) {
    return desired;
  }
  const fallback = order.find((name) => isLlmConfig(models[name]));
  return fallback || "";
};

const resolveDefaultModelNameByType = (desiredName, modelType, models, order) => {
  const normalizedType = normalizeModelType(modelType);
  const desired = String(desiredName || "").trim();
  if (
    desired &&
    models[desired] &&
    normalizeModelType(models[desired]?.model_type) === normalizedType
  ) {
    return desired;
  }
  return order.find((name) => normalizeModelType(models[name]?.model_type) === normalizedType) || "";
};

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

const syncToolCallModeForProvider = (nextProvider, previousProvider) => {
  if (!elements.llmToolCallMode) {
    return;
  }
  const prevDefault = resolveDefaultToolCallMode(previousProvider || DEFAULT_PROVIDER_ID);
  const current = normalizeToolCallMode(elements.llmToolCallMode.value, previousProvider);
  if (!elements.llmToolCallMode.value || current === prevDefault) {
    elements.llmToolCallMode.value = resolveDefaultToolCallMode(nextProvider);
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

const parseOptionalIntInput = (input) => {
  const parsed = Number.parseInt(input?.value, 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

const parseOptionalFloatInput = (input) => {
  const parsed = Number.parseFloat(input?.value);
  return Number.isFinite(parsed) && parsed > 0 ? roundFloat(parsed) : null;
};

const normalizeTtsResponseFormat = (value) => {
  const raw = String(value || "").trim().toLowerCase();
  return ["wav", "mp3", "flac", "aac", "opus", "pcm"].includes(raw) ? raw : "wav";
};

const normalizeImageOutputFormat = (value) => {
  const raw = String(value || "").trim().toLowerCase();
  return ["png", "jpeg", "webp"].includes(raw) ? raw : "";
};

// 规范化 LLM 配置，避免空值影响展示。
const normalizeLlmConfig = (raw) => {
  const provider = normalizeProviderId(raw?.provider || DEFAULT_PROVIDER_ID);
  return {
    enable: raw?.enable !== false,
    model_type: normalizeModelType(raw?.model_type),
    provider,
    base_url: raw?.base_url || "",
    api_key: raw?.api_key || "",
    model: raw?.model || "",
    temperature:
      typeof raw?.temperature === "number" && !Number.isNaN(raw.temperature)
        ? raw.temperature
        : 0.7,
    timeout_s:
      typeof raw?.timeout_s === "number" && !Number.isNaN(raw.timeout_s) ? raw.timeout_s : 120,
    max_rounds:
      typeof raw?.max_rounds === "number" && !Number.isNaN(raw.max_rounds)
        ? raw.max_rounds
        : 1000,
    max_context:
      typeof raw?.max_context === "number" && !Number.isNaN(raw.max_context)
        ? raw.max_context
        : null,
    max_output:
      typeof raw?.max_output === "number" && !Number.isNaN(raw.max_output)
        ? raw.max_output
        : null,
    thinking_token_budget:
      typeof raw?.thinking_token_budget === "number" && !Number.isNaN(raw.thinking_token_budget)
        ? raw.thinking_token_budget
        : null,
    support_vision: raw?.support_vision === true,
    support_hearing: raw?.support_hearing === true,
    stream: raw?.stream === true,
    stream_include_usage: raw?.stream_include_usage !== false,
    tool_call_mode: normalizeToolCallMode(raw?.tool_call_mode, provider),
    reasoning_effort: normalizeReasoningEffort(raw?.reasoning_effort),
    history_compaction_ratio:
      typeof raw?.history_compaction_ratio === "number" &&
      !Number.isNaN(raw.history_compaction_ratio)
        ? raw.history_compaction_ratio
        : 0.9,
    tts_voice: raw?.tts_voice || "",
    tts_instructions: raw?.tts_instructions || "",
    tts_response_format: normalizeTtsResponseFormat(raw?.tts_response_format),
    tts_speed: typeof raw?.tts_speed === "number" && !Number.isNaN(raw.tts_speed) ? raw.tts_speed : 1,
    image_size: raw?.image_size || "",
    image_output_format: normalizeImageOutputFormat(raw?.image_output_format),
    image_negative_prompt: raw?.image_negative_prompt || "",
    image_num_inference_steps:
      typeof raw?.image_num_inference_steps === "number" && !Number.isNaN(raw.image_num_inference_steps)
        ? raw.image_num_inference_steps
        : null,
    image_guidance_scale:
      typeof raw?.image_guidance_scale === "number" && !Number.isNaN(raw.image_guidance_scale)
        ? raw.image_guidance_scale
        : null,
    mock_if_unconfigured: raw?.mock_if_unconfigured !== false,
  };
};

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
    defaultName = "";
  }
  defaultName = resolveDefaultLlmName(defaultName, normalizedModels, order);
  const defaultEmbeddingName = resolveDefaultModelNameByType(
    llm.default_embedding,
    "embedding",
    normalizedModels,
    order
  );
  const defaultTtsName = resolveDefaultModelNameByType(
    llm.default_tts,
    "tts",
    normalizedModels,
    order
  );
  const defaultImageName = resolveDefaultModelNameByType(
    llm.default_image,
    "image",
    normalizedModels,
    order
  );
  return {
    defaultName,
    defaultEmbeddingName,
    defaultTtsName,
    defaultImageName,
    models: normalizedModels,
    order,
  };
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
  if (elements.llmModelType) {
    elements.llmModelType.value = "llm";
  }
  renderProviderOptions(DEFAULT_PROVIDER_ID);
  elements.llmProvider.value = DEFAULT_PROVIDER_ID;
  elements.llmModel.value = "";
  elements.llmBaseUrl.value = "";
  elements.llmApiKey.value = "";
  elements.llmTemperature.value = formatFloatForInput(0.7, 0.7);
  elements.llmTimeout.value = 120;
  elements.llmMaxRounds.value = 1000;
  elements.llmMaxContext.value = "";
  elements.llmMaxOutput.value = "";
  if (elements.llmThinkingTokenBudget) {
    elements.llmThinkingTokenBudget.value = "";
  }
  elements.llmVision.checked = false;
  elements.llmHearing.checked = false;
  elements.llmStreamIncludeUsage.checked = true;
  if (elements.llmToolCallMode) {
    elements.llmToolCallMode.value = resolveDefaultToolCallMode(DEFAULT_PROVIDER_ID);
  }
  if (elements.llmReasoningEffort) {
    elements.llmReasoningEffort.value = "";
  }
  elements.llmHistoryCompactionRatio.value = formatFloatForInput(0.9, 0.9);
  if (elements.llmTtsVoice) elements.llmTtsVoice.value = "";
  if (elements.llmTtsInstructions) elements.llmTtsInstructions.value = "";
  if (elements.llmTtsResponseFormat) elements.llmTtsResponseFormat.value = "wav";
  if (elements.llmTtsSpeed) elements.llmTtsSpeed.value = formatFloatForInput(1, 1);
  if (elements.llmImageSize) elements.llmImageSize.value = "";
  if (elements.llmImageOutputFormat) elements.llmImageOutputFormat.value = "";
  if (elements.llmImageSteps) elements.llmImageSteps.value = "";
  if (elements.llmImageGuidanceScale) elements.llmImageGuidanceScale.value = "";
  if (elements.llmImageNegativePrompt) elements.llmImageNegativePrompt.value = "";
  applyProviderDefaults(DEFAULT_PROVIDER_ID, { forceBaseUrl: false });
  lastProviderSelection = DEFAULT_PROVIDER_ID;
  updateLlmTypeVisibility("llm");
};

const updateLlmTypeVisibility = (modelType) => {
  const normalized = normalizeModelType(modelType || elements.llmModelType?.value || "llm");
  const isLlm = normalized === "llm";
  const isTts = normalized === "tts";
  const isImage = normalized === "image";
  const showGeneration = true;
  const toggle = (element, visible) => {
    if (!element) {
      return;
    }
    element.style.display = visible ? "" : "none";
  };
  toggle(elements.llmGenerationCard, showGeneration);
  toggle(elements.llmTemperatureRow, isLlm);
  toggle(elements.llmTimeout?.closest(".form-row"), isLlm);
  toggle(elements.llmMaxOutputRow, isLlm);
  toggle(elements.llmThinkingTokenBudgetRow, isLlm);
  toggle(elements.llmMaxRoundsRow, isLlm);
  toggle(elements.llmMaxContextRow, isLlm);
  toggle(elements.llmTtsRows, isTts);
  toggle(elements.llmImageRows, isImage);
  toggle(elements.llmConnectionOnlyHint, !showGeneration);
  toggle(elements.llmCapabilitiesCard, isLlm);
  toggle(elements.llmCompactionCard, isLlm);
  if (elements.llmGenerationTitle && showGeneration) {
    const titleKey =
      normalized === "tts"
        ? "llm.section.tts"
        : normalized === "image"
          ? "llm.section.image"
          : "llm.section.generation";
    elements.llmGenerationTitle.textContent = t(titleKey);
  }
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
  if (elements.llmModelType) {
    elements.llmModelType.value = normalizeModelType(llm.model_type);
  }
  renderProviderOptions(llm.provider);
  elements.llmProvider.value = llm.provider;
  elements.llmModel.value = llm.model;
  elements.llmBaseUrl.value = llm.base_url;
  elements.llmApiKey.value = llm.api_key;
  elements.llmTemperature.value = formatFloatForInput(llm.temperature, 0.7);
  elements.llmTimeout.value = llm.timeout_s;
  elements.llmMaxRounds.value = llm.max_rounds ?? 1000;
  elements.llmMaxContext.value = llm.max_context ?? "";
  elements.llmMaxOutput.value = llm.max_output ?? "";
  if (elements.llmThinkingTokenBudget) {
    elements.llmThinkingTokenBudget.value = llm.thinking_token_budget ?? "";
  }
  elements.llmVision.checked = llm.support_vision;
  elements.llmHearing.checked = llm.support_hearing;
  elements.llmStreamIncludeUsage.checked = llm.stream_include_usage === true;
  if (elements.llmToolCallMode) {
    elements.llmToolCallMode.value = normalizeToolCallMode(llm.tool_call_mode, llm.provider);
  }
  if (elements.llmReasoningEffort) {
    elements.llmReasoningEffort.value = normalizeReasoningEffort(llm.reasoning_effort);
  }
  elements.llmHistoryCompactionRatio.value = formatFloatForInput(
    llm.history_compaction_ratio ?? 0.9,
    0.9
  );
  if (elements.llmTtsVoice) elements.llmTtsVoice.value = llm.tts_voice || "";
  if (elements.llmTtsInstructions) {
    elements.llmTtsInstructions.value = llm.tts_instructions || "";
  }
  if (elements.llmTtsResponseFormat) {
    elements.llmTtsResponseFormat.value = normalizeTtsResponseFormat(llm.tts_response_format);
  }
  if (elements.llmTtsSpeed) {
    elements.llmTtsSpeed.value = formatFloatForInput(llm.tts_speed ?? 1, 1);
  }
  if (elements.llmImageSize) elements.llmImageSize.value = llm.image_size || "";
  if (elements.llmImageOutputFormat) {
    elements.llmImageOutputFormat.value = normalizeImageOutputFormat(llm.image_output_format);
  }
  if (elements.llmImageSteps) elements.llmImageSteps.value = llm.image_num_inference_steps ?? "";
  if (elements.llmImageGuidanceScale) {
    elements.llmImageGuidanceScale.value = llm.image_guidance_scale ?? "";
  }
  if (elements.llmImageNegativePrompt) {
    elements.llmImageNegativePrompt.value = llm.image_negative_prompt || "";
  }
  updateLlmTypeVisibility(llm.model_type);
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
    const isChatModel = normalizeModelType(config?.model_type) === "llm";
    const isDefault = activeName === state.llm.defaultName && isChatModel;
    elements.llmSetDefaultBtn.disabled = isDefault || !isChatModel;
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
    const modelType = normalizeModelType(config?.model_type);
    const icon = document.createElement("i");
    const iconClass =
      modelType === "embedding"
        ? "fa-cube"
        : modelType === "tts"
          ? "fa-volume-high"
          : modelType === "image"
            ? "fa-image"
            : "fa-robot";
    icon.className = `fa-solid ${iconClass} llm-type-icon is-${modelType}`;
    const titleText = document.createElement("span");
    titleText.className = "llm-list-item-name";
    titleText.textContent = getDisplayName(name);
    title.appendChild(icon);
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
  const maxRounds = Number.parseInt(elements.llmMaxRounds.value, 10);
  const maxContext = Number.parseInt(elements.llmMaxContext.value, 10);
  const maxOutput = Number.parseInt(elements.llmMaxOutput.value, 10);
  const thinkingTokenBudget = Number.parseInt(elements.llmThinkingTokenBudget?.value, 10);
  const historyCompactionRatio = parseFloatInput(elements.llmHistoryCompactionRatio, 0.9);
  const modelType = normalizeModelType(elements.llmModelType?.value || base.model_type);
  const provider = normalizeProviderId(elements.llmProvider.value || base.provider);
  const baseUrl = elements.llmBaseUrl.value.trim();
  const apiKey = elements.llmApiKey.value.trim();
  const model = elements.llmModel.value.trim();
  const timeoutValue = Number.isFinite(timeout) ? timeout : 120;
  const reasoningEffort = normalizeReasoningEffort(
    elements.llmReasoningEffort?.value || base.reasoning_effort
  );
  const commonConfig = {
    enable: base.enable,
    model_type: modelType,
    provider,
    base_url: baseUrl,
    api_key: apiKey,
    model,
    mock_if_unconfigured: base.mock_if_unconfigured,
  };
  if (modelType === "embedding") {
    return commonConfig;
  }
  if (modelType === "tts") {
    return {
      ...commonConfig,
      tts_voice: elements.llmTtsVoice?.value.trim() || undefined,
      tts_instructions: elements.llmTtsInstructions?.value.trim() || undefined,
      tts_response_format: normalizeTtsResponseFormat(elements.llmTtsResponseFormat?.value),
      tts_speed: parseOptionalFloatInput(elements.llmTtsSpeed) ?? 1,
    };
  }
  if (modelType === "image") {
    return {
      ...commonConfig,
      image_size: elements.llmImageSize?.value.trim() || undefined,
      image_output_format:
        normalizeImageOutputFormat(elements.llmImageOutputFormat?.value) || undefined,
      image_negative_prompt: elements.llmImageNegativePrompt?.value.trim() || undefined,
      image_num_inference_steps: parseOptionalIntInput(elements.llmImageSteps) || undefined,
      image_guidance_scale: parseOptionalFloatInput(elements.llmImageGuidanceScale) || undefined,
    };
  }
  return {
    ...commonConfig,
    temperature: Number.isFinite(temperature) ? temperature : 0.7,
    timeout_s: timeoutValue,
    max_rounds: Number.isFinite(maxRounds) && maxRounds > 0 ? maxRounds : base.max_rounds ?? 1000,
    max_context: Number.isFinite(maxContext) && maxContext > 0 ? maxContext : null,
    max_output: Number.isFinite(maxOutput) && maxOutput > 0 ? maxOutput : null,
    thinking_token_budget:
      Number.isFinite(thinkingTokenBudget) && thinkingTokenBudget > 0 ? thinkingTokenBudget : null,
    support_vision: elements.llmVision.checked,
    support_hearing: elements.llmHearing.checked,
    stream: base.stream,
    stream_include_usage: elements.llmStreamIncludeUsage.checked,
    tool_call_mode: normalizeToolCallMode(
      elements.llmToolCallMode?.value || base.tool_call_mode,
      provider
    ),
    reasoning_effort: reasoningEffort || null,
    history_compaction_ratio:
      Number.isFinite(historyCompactionRatio) && historyCompactionRatio > 0
        ? historyCompactionRatio
        : base.history_compaction_ratio ?? 0.9,
  };
};

const buildLlmConfigForPayload = (rawConfig) => {
  const config = normalizeLlmConfig(rawConfig || {});
  const commonConfig = {
    enable: config.enable,
    model_type: normalizeModelType(config.model_type),
    provider: normalizeProviderId(config.provider),
    base_url: config.base_url || undefined,
    api_key: config.api_key || undefined,
    model: config.model || undefined,
    mock_if_unconfigured: config.mock_if_unconfigured,
  };
  if (commonConfig.model_type === "embedding") {
    return commonConfig;
  }
  if (commonConfig.model_type === "tts") {
    return {
      ...commonConfig,
      tts_voice: config.tts_voice || undefined,
      tts_instructions: config.tts_instructions || undefined,
      tts_response_format: normalizeTtsResponseFormat(config.tts_response_format),
      tts_speed: Number.isFinite(config.tts_speed) && config.tts_speed > 0 ? config.tts_speed : 1,
    };
  }
  if (commonConfig.model_type === "image") {
    return {
      ...commonConfig,
      image_size: config.image_size || undefined,
      image_output_format: config.image_output_format || undefined,
      image_negative_prompt: config.image_negative_prompt || undefined,
      image_num_inference_steps:
        Number.isFinite(config.image_num_inference_steps) && config.image_num_inference_steps > 0
          ? config.image_num_inference_steps
          : undefined,
      image_guidance_scale:
        Number.isFinite(config.image_guidance_scale) && config.image_guidance_scale > 0
          ? config.image_guidance_scale
          : undefined,
    };
  }
  return {
    ...commonConfig,
    api_mode: config.api_mode || undefined,
    temperature: config.temperature,
    timeout_s: config.timeout_s,
    max_rounds: config.max_rounds,
    max_context: config.max_context || undefined,
    max_output: config.max_output || undefined,
    thinking_token_budget: config.thinking_token_budget || undefined,
    support_vision: config.support_vision,
    support_hearing: config.support_hearing,
    stream: config.stream,
    stream_include_usage: config.stream_include_usage,
    tool_call_mode: config.tool_call_mode,
    reasoning_effort: config.reasoning_effort || undefined,
    history_compaction_ratio: config.history_compaction_ratio,
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
  const modelType = normalizeModelType(elements.llmModelType?.value || "llm");
  if (modelType !== "llm") {
    return null;
  }
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
    const config = state.llm.configs[name] || {};
    if (normalizeModelType(config.model_type) !== "llm") {
      return;
    }
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
  state.llm.defaultEmbeddingName = normalized.defaultEmbeddingName;
  state.llm.defaultTtsName = normalized.defaultTtsName;
  state.llm.defaultImageName = normalized.defaultImageName;
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
    if (state.llm.defaultEmbeddingName === activeName) {
      state.llm.defaultEmbeddingName = desiredName;
    }
    if (state.llm.defaultTtsName === activeName) {
      state.llm.defaultTtsName = desiredName;
    }
    if (state.llm.defaultImageName === activeName) {
      state.llm.defaultImageName = desiredName;
    }
    delete state.llm.nameEdits[activeName];
  } else {
    state.llm.configs[activeName] = currentConfig;
    delete state.llm.nameEdits[activeName];
  }
  if (!state.llm.defaultName) {
    state.llm.defaultName = desiredName;
  }
  state.llm.defaultName = resolveDefaultLlmName(
    state.llm.defaultName,
    state.llm.configs,
    state.llm.order
  );
  state.llm.defaultEmbeddingName = resolveDefaultModelNameByType(
    state.llm.defaultEmbeddingName,
    "embedding",
    state.llm.configs,
    state.llm.order
  );
  state.llm.defaultTtsName = resolveDefaultModelNameByType(
    state.llm.defaultTtsName,
    "tts",
    state.llm.configs,
    state.llm.order
  );
  state.llm.defaultImageName = resolveDefaultModelNameByType(
    state.llm.defaultImageName,
    "image",
    state.llm.configs,
    state.llm.order
  );
  state.llm.activeName = desiredName;
};

const buildLlmPayload = () => {
  const models = {};
  state.llm.order.forEach((name) => {
    if (state.llm.configs[name]) {
      models[name] = buildLlmConfigForPayload(state.llm.configs[name]);
    }
  });
  const defaultName = resolveDefaultLlmName(
    state.llm.defaultName,
    state.llm.configs,
    state.llm.order
  );
  state.llm.defaultName = defaultName;
  const defaultEmbeddingName = resolveDefaultModelNameByType(
    state.llm.defaultEmbeddingName,
    "embedding",
    state.llm.configs,
    state.llm.order
  );
  const defaultTtsName = resolveDefaultModelNameByType(
    state.llm.defaultTtsName,
    "tts",
    state.llm.configs,
    state.llm.order
  );
  const defaultImageName = resolveDefaultModelNameByType(
    state.llm.defaultImageName,
    "image",
    state.llm.configs,
    state.llm.order
  );
  state.llm.defaultEmbeddingName = defaultEmbeddingName;
  state.llm.defaultTtsName = defaultTtsName;
  state.llm.defaultImageName = defaultImageName;
  return {
    default: defaultName,
    default_embedding: defaultEmbeddingName || undefined,
    default_tts: defaultTtsName || undefined,
    default_image: defaultImageName || undefined,
    models,
  };
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
    state.llm.defaultName = resolveDefaultLlmName("", state.llm.configs, state.llm.order);
  }
  if (state.llm.defaultEmbeddingName === activeName) {
    state.llm.defaultEmbeddingName = resolveDefaultModelNameByType(
      "",
      "embedding",
      state.llm.configs,
      state.llm.order
    );
  }
  if (state.llm.defaultTtsName === activeName) {
    state.llm.defaultTtsName = resolveDefaultModelNameByType(
      "",
      "tts",
      state.llm.configs,
      state.llm.order
    );
  }
  if (state.llm.defaultImageName === activeName) {
    state.llm.defaultImageName = resolveDefaultModelNameByType(
      "",
      "image",
      state.llm.configs,
      state.llm.order
    );
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
  const activeConfig = state.llm.configs[activeName];
  if (normalizeModelType(activeConfig?.model_type) !== "llm") {
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

const handleModelTypeChange = () => {
  const activeName = state.llm.activeName;
  const modelType = normalizeModelType(elements.llmModelType?.value || "llm");
  if (!activeName) {
    updateLlmTypeVisibility(modelType);
    return;
  }
  syncActiveConfigToState();
  if (modelType !== "llm" && state.llm.defaultName === activeName) {
    state.llm.defaultName = resolveDefaultLlmName("", state.llm.configs, state.llm.order);
  }
  applyLlmConfigToForm(activeName, state.llm.configs[activeName]);
  renderLlmList();
  updateDetailHeader();
  renderDebugModelOptions();
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
  elements.llmModelType?.addEventListener("change", handleModelTypeChange);
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
    const previousProvider = lastProviderSelection;
    elements.llmProvider.value = nextProvider;
    syncToolCallModeForProvider(nextProvider, previousProvider);
    applyProviderDefaults(nextProvider, { previousProvider: lastProviderSelection });
    lastProviderSelection = nextProvider;
    handleProbeInput();
  });
  elements.llmBaseUrl.addEventListener("blur", () => requestContextWindow(true));
  elements.llmModel.addEventListener("blur", () => requestContextWindow(true));
};




