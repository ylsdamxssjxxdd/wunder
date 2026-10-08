/**
 * 预设授权（`GET /wunder/user/agent`，方案 §12.2.1 3)）读取与降级。
 *
 * 服务端已经交付 `customizable`（system_prompt / welcome / model_name /
 * reasoning_effort / tool_names / approval_mode）；前端此前只用了旧版 `/agents`
 * 列表，因此在设置页「智能体设置」分类里按需补一次只读读取。
 *
 * 降级：接口未交付（404/网络失败）或响应缺少 `customizable` 时，标记
 * `available = false`，面板按「全部可编辑」处理并在页面上说明，不伪造授权范围。
 */

import { computed, ref } from 'vue';

import { getUserAgent } from '@/api/agents';

export const PRESET_CUSTOMIZABLE_FIELDS = [
  'system_prompt',
  'welcome',
  'model_name',
  'reasoning_effort',
  'tool_names',
  'approval_mode'
] as const;

export type PresetCustomizableField = (typeof PRESET_CUSTOMIZABLE_FIELDS)[number];

export type UserAgentPresetState = {
  loading: boolean;
  loaded: boolean;
  failed: boolean;
  /** True only when the server answered with a `customizable` object. */
  available: boolean;
  presetId: string;
  presetName: string;
  customizable: Record<string, boolean> | null;
  updatedAt: number;
};

const emptyState = (): UserAgentPresetState => ({
  loading: false,
  loaded: false,
  failed: false,
  available: false,
  presetId: '',
  presetName: '',
  customizable: null,
  updatedAt: 0
});

export const userAgentPresetState = ref<UserAgentPresetState>(emptyState());

let inflight: Promise<UserAgentPresetState> | null = null;

const asRecord = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};

const normalizeCustomizable = (value: unknown): Record<string, boolean> | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  const source = value as Record<string, unknown>;
  const output: Record<string, boolean> = {};
  PRESET_CUSTOMIZABLE_FIELDS.forEach((field) => {
    if (field in source) {
      output[field] = source[field] === true;
    }
  });
  return Object.keys(output).length ? output : null;
};

export const ensureUserAgentPreset = async (
  options: { force?: boolean } = {}
): Promise<UserAgentPresetState> => {
  const current = userAgentPresetState.value;
  if (!options.force && current.loaded) return current;
  if (inflight) return inflight;
  userAgentPresetState.value = { ...current, loading: true };
  inflight = (async () => {
    try {
      const response = await getUserAgent();
      const payload = asRecord(asRecord(response?.data)?.data);
      const binding = asRecord(payload.preset_binding);
      const customizable = normalizeCustomizable(payload.customizable);
      const next: UserAgentPresetState = {
        loading: false,
        loaded: true,
        failed: false,
        available: customizable !== null,
        presetId: String(binding.preset_id || '').trim(),
        presetName: String(binding.name || '').trim(),
        customizable,
        updatedAt: Date.now()
      };
      userAgentPresetState.value = next;
      return next;
    } catch {
      // 接口未交付 / 网络失败：保持可用但标记为不可判定，面板按可编辑处理。
      const failed: UserAgentPresetState = {
        ...emptyState(),
        loading: false,
        loaded: true,
        failed: true
      };
      userAgentPresetState.value = failed;
      return failed;
    } finally {
      inflight = null;
    }
  })();
  return inflight;
};

/** Fields the preset explicitly owns (`false`); empty when the surface is unknown. */
export const presetLockedFields = computed<string[]>(() => {
  const customizable = userAgentPresetState.value.customizable;
  if (!customizable) return [];
  return PRESET_CUSTOMIZABLE_FIELDS.filter((field) => customizable[field] === false);
});
