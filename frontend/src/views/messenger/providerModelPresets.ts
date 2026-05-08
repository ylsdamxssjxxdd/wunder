export type ProviderModelPreset = {
  id: string;
  label?: string;
  maxContext?: number;
};

export type ProviderModelType = 'llm' | 'embedding' | 'asr' | 'tts' | 'image' | 'video';

const PROVIDER_MODEL_PRESETS_BY_TYPE: Record<
  ProviderModelType,
  Record<string, ProviderModelPreset[]>
> = {
  llm: {
  openai_compatible: [
    { id: 'gpt-5.1', maxContext: 128000 },
    { id: 'gpt-5', maxContext: 128000 },
    { id: 'gpt-5-mini', maxContext: 128000 },
    { id: 'gpt-5-nano', maxContext: 128000 },
    { id: 'gpt-5-chat', maxContext: 128000 },
    { id: 'gpt-image-1', maxContext: 32768 },
    { id: 'deepseek-chat', maxContext: 128000 },
    { id: 'deepseek-reasoner', maxContext: 128000 },
    { id: 'qwen-max', maxContext: 131072 },
    { id: 'qwen-plus', maxContext: 131072 }
  ],
  openai: [
    { id: 'gpt-5.1', maxContext: 128000 },
    { id: 'gpt-5', maxContext: 128000 },
    { id: 'gpt-5-mini', maxContext: 128000 },
    { id: 'gpt-5-nano', maxContext: 128000 },
    { id: 'gpt-5-pro', maxContext: 128000 },
    { id: 'gpt-5-chat', maxContext: 128000 },
    { id: 'gpt-image-1', maxContext: 32768 }
  ],
  anthropic: [
    { id: 'claude-opus-4-1-20250805', maxContext: 200000 },
    { id: 'claude-sonnet-4-5-20250929', maxContext: 200000 },
    { id: 'claude-3-7-sonnet-20250219', maxContext: 200000 },
    { id: 'claude-3-5-sonnet-20241022', maxContext: 200000 },
    { id: 'glm-5', maxContext: 128000 }
  ],
  openrouter: [
    { id: 'google/gemini-2.5-flash-image-preview', maxContext: 1048576 },
    { id: 'google/gemini-2.5-flash-preview', maxContext: 1048576 },
    { id: 'qwen/qwen-2.5-7b-instruct:free', maxContext: 32768 },
    { id: 'deepseek/deepseek-chat', maxContext: 128000 },
    { id: 'mistralai/mistral-7b-instruct:free', maxContext: 32768 }
  ],
  siliconflow: [
    { id: 'deepseek-ai/DeepSeek-V3.2', maxContext: 128000 },
    { id: 'Qwen/Qwen3-8B', maxContext: 32768 },
    { id: 'BAAI/bge-m3', maxContext: 8192 }
  ],
  deepseek: [
    { id: 'deepseek-chat', maxContext: 128000 },
    { id: 'deepseek-reasoner', maxContext: 128000 }
  ],
  moonshot: [
    { id: 'moonshot-v1-auto', maxContext: 128000 },
    { id: 'kimi-k2-0711-preview', maxContext: 128000 },
    { id: 'kimi-k2.5', maxContext: 128000 },
    { id: 'kimi-k2-0905-Preview', maxContext: 128000 },
    { id: 'kimi-k2-turbo-preview', maxContext: 128000 },
    { id: 'kimi-k2-thinking', maxContext: 128000 },
    { id: 'kimi-k2-thinking-turbo', maxContext: 128000 }
  ],
  qwen: [
    { id: 'qwen-vl-plus', maxContext: 131072 },
    { id: 'qwen-coder-plus', maxContext: 131072 },
    { id: 'qwen-flash', maxContext: 131072 },
    { id: 'qwen-plus', maxContext: 131072 },
    { id: 'qwen-max', maxContext: 131072 },
    { id: 'qwen3-max', maxContext: 131072 },
    { id: 'qwen3.5-plus', maxContext: 131072 },
    { id: 'qwen3.5-397b-a17b', maxContext: 131072 }
  ],
  groq: [
    { id: 'llama3-8b-8192', maxContext: 8192 },
    { id: 'llama3-70b-8192', maxContext: 8192 },
    { id: 'mistral-saba-24b', maxContext: 32768 },
    { id: 'gemma-9b-it', maxContext: 8192 }
  ],
  mistral: [
    { id: 'pixtral-12b-2409', maxContext: 131072 },
    { id: 'pixtral-large-latest', maxContext: 131072 },
    { id: 'ministral-3b-latest', maxContext: 128000 },
    { id: 'ministral-8b-latest', maxContext: 128000 },
    { id: 'codestral-latest', maxContext: 256000 },
    { id: 'mistral-large-latest', maxContext: 128000 },
    { id: 'mistral-small-latest', maxContext: 128000 },
    { id: 'open-mistral-nemo', maxContext: 128000 }
  ],
  together: [
    { id: 'meta-llama/Llama-3.2-11B-Vision-Instruct-Turbo', maxContext: 131072 },
    { id: 'meta-llama/Llama-3.2-90B-Vision-Instruct-Turbo', maxContext: 131072 },
    { id: 'google/gemma-2-27b-it', maxContext: 8192 },
    { id: 'google/gemma-2-9b-it', maxContext: 8192 }
  ],
  ollama: [
    { id: 'qwen2.5:7b-instruct', maxContext: 32768 },
    { id: 'qwen2.5:14b-instruct', maxContext: 32768 },
    { id: 'qwen3:8b', maxContext: 32768 },
    { id: 'deepseek-r1:8b', maxContext: 32768 },
    { id: 'llama3.1:8b-instruct', maxContext: 131072 },
    { id: 'llama3.1:70b-instruct', maxContext: 131072 },
    { id: 'mistral:7b-instruct', maxContext: 32768 },
    { id: 'gemma2:9b', maxContext: 8192 }
  ],
  lmstudio: [
    { id: 'openai/gpt-oss-20b', maxContext: 131072 },
    { id: 'qwen/qwen3-8b', maxContext: 32768 },
    { id: 'qwen/qwen2.5-14b-instruct', maxContext: 32768 },
    { id: 'deepseek/deepseek-r1-distill-qwen-14b', maxContext: 32768 },
    { id: 'meta-llama/llama-3.1-8b-instruct', maxContext: 131072 },
    { id: 'meta-llama/llama-3.1-70b-instruct', maxContext: 131072 },
    { id: 'mistralai/mistral-7b-instruct-v0.3', maxContext: 32768 },
    { id: 'google/gemma-2-9b-it', maxContext: 8192 }
  ]
  },
  embedding: {
    vllm_omni: [
      { id: 'BAAI/bge-m3', maxContext: 8192 },
      { id: 'Qwen/Qwen3-Embedding-8B', maxContext: 32768 }
    ],
    openai_compatible: [
      { id: 'text-embedding-3-large', maxContext: 8192 },
      { id: 'text-embedding-3-small', maxContext: 8192 }
    ],
    siliconflow: [{ id: 'BAAI/bge-m3', maxContext: 8192 }],
    qwen: [{ id: 'text-embedding-v4', maxContext: 8192 }]
  },
  asr: {
    vllm_omni: [
      { id: 'fixie-ai/ultravox-v0_5-llama-3_2-1b', maxContext: 32768 },
      { id: 'openai/whisper-large-v3-turbo', maxContext: 32768 }
    ],
    whisper_cpp: [
      { id: 'whisper.cpp', label: 'whisper.cpp server' },
      { id: 'ggml-large-v3-turbo', label: 'ggml-large-v3-turbo' }
    ],
    openai_compatible: [
      { id: 'whisper-1', maxContext: 32768 },
      { id: 'gpt-4o-mini-transcribe', maxContext: 32768 }
    ],
    openai: [
      { id: 'whisper-1', maxContext: 32768 },
      { id: 'gpt-4o-mini-transcribe', maxContext: 32768 }
    ],
    siliconflow: [{ id: 'FunAudioLLM/SenseVoiceSmall', maxContext: 32768 }],
    qwen: [{ id: 'qwen-audio-asr', maxContext: 32768 }]
  },
  tts: {
    vllm_omni: [
      { id: 'hexgrad/Kokoro-82M', maxContext: 32768 },
      { id: 'cartesia/sonic-2', maxContext: 32768 }
    ],
    openai_compatible: [
      { id: 'gpt-4o-mini-tts', maxContext: 32768 },
      { id: 'tts-1', maxContext: 32768 },
      { id: 'tts-1-hd', maxContext: 32768 }
    ],
    openai: [
      { id: 'gpt-4o-mini-tts', maxContext: 32768 },
      { id: 'tts-1', maxContext: 32768 },
      { id: 'tts-1-hd', maxContext: 32768 }
    ],
    siliconflow: [{ id: 'FunAudioLLM/CosyVoice2-0.5B', maxContext: 32768 }],
    qwen: [{ id: 'qwen-tts-latest', maxContext: 32768 }]
  },
  image: {
    vllm_omni: [
      { id: 'black-forest-labs/FLUX.1-schnell', maxContext: 32768 },
      { id: 'stabilityai/stable-diffusion-xl-base-1.0', maxContext: 32768 }
    ],
    openai_compatible: [{ id: 'gpt-image-1', maxContext: 32768 }],
    openai: [{ id: 'gpt-image-1', maxContext: 32768 }],
    siliconflow: [
      { id: 'black-forest-labs/FLUX.1-schnell', maxContext: 32768 },
      { id: 'stabilityai/stable-diffusion-xl-base-1.0', maxContext: 32768 }
    ],
    openrouter: [{ id: 'google/gemini-2.5-flash-image-preview', maxContext: 1048576 }],
    mistral: [{ id: 'black-forest-labs/FLUX.1-schnell', maxContext: 32768 }]
  },
  video: {
    vllm_omni: [
      { id: 'genmo/mochi-1-preview', maxContext: 32768 },
      { id: 'Lightricks/LTX-Video', maxContext: 32768 }
    ],
    openai_compatible: [{ id: 'genmo/mochi-1-preview', maxContext: 32768 }],
    openai: [{ id: 'sora-1', maxContext: 32768 }],
    siliconflow: [{ id: 'Lightricks/LTX-Video', maxContext: 32768 }],
    openrouter: [{ id: 'genmo/mochi-1-preview', maxContext: 32768 }]
  }
};

const normalizeProviderId = (value: unknown): string => {
  const normalized = String(value || '')
    .trim()
    .toLowerCase()
    .replace(/[\s-]+/g, '_');
  if (normalized === 'claude' || normalized === 'anthropic_api') {
    return 'anthropic';
  }
  if (normalized === 'openai_compat') {
    return 'openai_compatible';
  }
  if (normalized === 'lm_studio') {
    return 'lmstudio';
  }
  if (normalized === 'vllmomni') {
    return 'vllm_omni';
  }
  if (normalized === 'whispercpp') {
    return 'whisper_cpp';
  }
  return normalized;
};

const normalizeModelId = (value: unknown): string =>
  String(value || '')
    .trim()
    .toLowerCase();

const inferContextFromModelId = (value: unknown): number | null => {
  const modelId = normalizeModelId(value);
  if (!modelId) return null;

  if (/(?:^|[-_/])1m(?:$|[-_/])/.test(modelId)) {
    return 1048576;
  }

  let inferred: number | null = null;
  const kiloMatches = modelId.matchAll(/(?:^|[-_/])(\d{1,4})k(?:$|[-_/])/g);
  for (const match of kiloMatches) {
    const parsed = Number.parseInt(match[1], 10);
    if (!Number.isFinite(parsed) || parsed <= 0) continue;
    const valueFromKilo = parsed * 1024;
    if (!inferred || valueFromKilo > inferred) {
      inferred = valueFromKilo;
    }
  }
  if (inferred) return inferred;

  const numericMatches = modelId.matchAll(/(?:^|[-_/])(\d{4,6})(?:$|[-_/])/g);
  for (const match of numericMatches) {
    const parsed = Number.parseInt(match[1], 10);
    if (!Number.isFinite(parsed)) continue;
    if (parsed < 2048 || parsed > 1048576) continue;
    if (!inferred || parsed > inferred) {
      inferred = parsed;
    }
  }
  return inferred;
};

export const getProviderModelPresets = (
  provider: unknown,
  modelType: ProviderModelType = 'llm'
): ProviderModelPreset[] =>
  PROVIDER_MODEL_PRESETS_BY_TYPE[modelType]?.[normalizeProviderId(provider)] || [];

export const resolveAnyProviderModelPresetMaxContext = (model: unknown): number | null => {
  const modelId = normalizeModelId(model);
  if (!modelId) return null;
  let matchedMaxContext: number | null = null;
  Object.values(PROVIDER_MODEL_PRESETS_BY_TYPE).forEach((presetsByProvider) => {
    Object.values(presetsByProvider).forEach((presets) => {
      presets.forEach((item) => {
        if (normalizeModelId(item.id) !== modelId) return;
        const parsed = Number(item.maxContext);
        if (!Number.isFinite(parsed) || parsed <= 0) return;
        const normalized = Math.round(parsed);
        if (matchedMaxContext === null || normalized > matchedMaxContext) {
          matchedMaxContext = normalized;
        }
      });
    });
  });
  if (matchedMaxContext !== null) {
    return matchedMaxContext;
  }
  return inferContextFromModelId(modelId);
};

export const resolveProviderModelPresetMaxContext = (
  provider: unknown,
  model: unknown,
  modelType: ProviderModelType = 'llm'
): number | null => {
  const modelId = normalizeModelId(model);
  if (!modelId) return null;
  const presets = getProviderModelPresets(provider, modelType);
  const matched = presets.find((item) => normalizeModelId(item.id) === modelId);
  if (Number.isFinite(Number(matched?.maxContext)) && Number(matched?.maxContext) > 0) {
    return Math.round(Number(matched?.maxContext));
  }
  return inferContextFromModelId(modelId);
};
