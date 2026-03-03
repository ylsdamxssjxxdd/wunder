export type ProviderModelPreset = {
  id: string;
  label?: string;
  maxContext?: number;
};

const PROVIDER_MODEL_PRESETS: Record<string, ProviderModelPreset[]> = {
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
};

const normalizeProviderId = (value: unknown): string =>
  String(value || '')
    .trim()
    .toLowerCase()
    .replace(/[\s-]+/g, '_');

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

export const getProviderModelPresets = (provider: unknown): ProviderModelPreset[] =>
  PROVIDER_MODEL_PRESETS[normalizeProviderId(provider)] || [];

export const resolveProviderModelPresetMaxContext = (provider: unknown, model: unknown): number | null => {
  const modelId = normalizeModelId(model);
  if (!modelId) return null;
  const presets = getProviderModelPresets(provider);
  const matched = presets.find((item) => normalizeModelId(item.id) === modelId);
  if (Number.isFinite(Number(matched?.maxContext)) && Number(matched?.maxContext) > 0) {
    return Math.round(Number(matched?.maxContext));
  }
  return inferContextFromModelId(modelId);
};
