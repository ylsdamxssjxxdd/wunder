import { computed, type ComputedRef } from 'vue';

import {
  CONTEXT_DANGER_RATIO,
  CONTEXT_WARNING_RATIO,
  formatContextTokenCount,
  resolveComposerContextUsageSource,
  resolveStableComposerContextPair
} from '@/components/chat/composerContextUsage';
import {
  resolveComposerContextDisplay,
  type ComposerContextDisplayState
} from '@/components/chat/composerContextDisplay';
import { resolveAnyProviderModelPresetMaxContext } from '@/views/messenger/providerModelPresets';

/**
 * 上下文占用投影（ctx usage）——**唯一**一份，供底部状态栏渲染。
 *
 * 为什么抽出来：B4 曾在输入卡内放一份 `ComposerStatusBar`（工作目录/云端/上下文），
 * B1 的壳体底部又放了一份 `MessengerStatusBar`（工作区/在线/上下文），两处显示同一份
 * 占用率与在线状态。B6 合并为「状态类信息留底部、模型/工作区入口留输入卡」，
 * 上下文占用随之下沉到底部，占用计算必须只有一处实现，避免两份漂移出不同的百分比。
 */
export type SessionContextUsageDisplay = {
  ratio: number | null;
  percentText: string;
  counts: string;
  level: '' | 'is-warning' | 'is-danger';
  overflow: boolean;
};

export type SessionContextUsageInput = {
  /** 会话/模型切换时用于重置显示快照。 */
  scope: () => string;
  /** 用于估算未收到服务端占用时的上下文消息。 */
  messages: () => unknown[];
  session: () => Record<string, unknown> | null | undefined;
  loading: () => boolean;
  modelName: () => string;
  /** 显式占用覆盖（harness 注入）；不传则退回消息估算。 */
  observedUsedTokens?: () => unknown;
  observedTotalTokens?: () => unknown;
};

const normalizeTokenCount = (value: unknown): number | null => {
  if (value === null || value === undefined) {
    return null;
  }
  const normalizedValue = typeof value === 'string' ? value.trim() : value;
  if (normalizedValue === '') {
    return null;
  }
  const parsed = Number(normalizedValue);
  if (!Number.isFinite(parsed) || parsed < 0) {
    return null;
  }
  return Math.round(parsed);
};

export const useSessionContextUsage = (
  input: SessionContextUsageInput
): ComputedRef<SessionContextUsageDisplay> => {
  const observedUsed = () => normalizeTokenCount(input.observedUsedTokens?.());
  const observedTotal = () => normalizeTokenCount(input.observedTotalTokens?.());
  const usageSource = computed(() =>
    resolveComposerContextUsageSource(
      Array.isArray(input.messages()) ? (input.messages() as Record<string, unknown>[]) : [],
      input.session(),
      Boolean(input.loading())
    )
  );
  const usedTokensRaw = computed(() => {
    const fromObserved = observedUsed();
    return fromObserved !== null ? fromObserved : usageSource.value.contextTokens;
  });
  const totalTokensRaw = computed(() => {
    const fromObserved = observedTotal();
    if (fromObserved !== null && fromObserved > 0) return fromObserved;
    const fromSource = usageSource.value.contextTotalTokens;
    if (fromSource !== null && fromSource > 0) return fromSource;
    const fromPreset = normalizeTokenCount(
      resolveAnyProviderModelPresetMaxContext(String(input.modelName() || ''))
    );
    return fromPreset !== null && fromPreset > 0 ? fromPreset : null;
  });
  // Occupancy and capacity are snapshots; never accumulate display deltas.
  const snapshot = computed<ComposerContextDisplayState>((previous) =>
    resolveComposerContextDisplay(previous, {
      scope: input.scope(),
      assistant: usageSource.value.assistantSignature,
      observed: observedUsed() !== null || usageSource.value.contextObserved === true,
      used: usedTokensRaw.value,
      total: totalTokensRaw.value
    })
  );
  const pair = computed(() =>
    resolveStableComposerContextPair(snapshot.value.used, snapshot.value.total)
  );

  return computed<SessionContextUsageDisplay>(() => {
    const used = pair.value.used;
    const total = pair.value.total;
    const ratio = used === null || total === null || total <= 0 ? null : Math.max(0, used) / total;
    const counts = `${formatContextTokenCount(used)} / ${formatContextTokenCount(total)}`;
    const level: SessionContextUsageDisplay['level'] =
      ratio === null ? '' : ratio >= CONTEXT_DANGER_RATIO ? 'is-danger' : ratio >= CONTEXT_WARNING_RATIO ? 'is-warning' : '';
    return {
      ratio,
      percentText: ratio === null ? '--' : `${Math.min(999, Math.round(ratio * 100))}%`,
      counts,
      level,
      overflow: ratio !== null && ratio >= CONTEXT_DANGER_RATIO
    };
  });
};
