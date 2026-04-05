import { computed, ref, type Ref } from 'vue';
import { chatDebugLog } from '../../utils/chatDebug';

export type MessengerInteractionBlockReason = 'new_session' | 'refresh';

type UseMessengerInteractionBlockerOptions = {
  rootRef: Ref<HTMLElement | null>;
  resolveLabel: (reason: MessengerInteractionBlockReason) => string;
  minVisibleMs?: number;
};

const DEFAULT_MIN_VISIBLE_MS = 260;

const waitFor = (ms: number) =>
  new Promise<void>((resolve) => {
    setTimeout(resolve, Math.max(0, ms));
  });

export const useMessengerInteractionBlocker = (
  options: UseMessengerInteractionBlockerOptions
) => {
  const activeReason = ref<MessengerInteractionBlockReason | ''>('');
  const blockStartedAt = ref(0);

  const isBlocked = computed(() => activeReason.value !== '');
  const label = computed(() =>
    activeReason.value ? options.resolveLabel(activeReason.value) : ''
  );

  const blurFocusedElementInsideRoot = () => {
    if (typeof document === 'undefined') return;
    const root = options.rootRef.value;
    if (!root) return;
    const focused = document.activeElement as HTMLElement | null;
    if (!focused || !root.contains(focused)) return;
    if (typeof focused.blur === 'function') {
      focused.blur();
    }
  };

  const releaseBlock = async (reason: MessengerInteractionBlockReason) => {
    if (activeReason.value !== reason) return;
    const elapsed = Date.now() - (blockStartedAt.value || 0);
    const minVisibleMs = Number.isFinite(options.minVisibleMs)
      ? Number(options.minVisibleMs)
      : DEFAULT_MIN_VISIBLE_MS;
    if (elapsed < minVisibleMs) {
      await waitFor(minVisibleMs - elapsed);
    }
    if (activeReason.value === reason) {
      activeReason.value = '';
      blockStartedAt.value = 0;
    }
  };

  const runWithBlock = async <T>(
    reason: MessengerInteractionBlockReason,
    task: () => Promise<T>
  ): Promise<T | null> => {
    if (activeReason.value) {
      chatDebugLog('messenger.interaction-blocker', 'reject-concurrent', {
        incomingReason: reason,
        activeReason: activeReason.value
      });
      return null;
    }
    activeReason.value = reason;
    blockStartedAt.value = Date.now();
    chatDebugLog('messenger.interaction-blocker', 'block-start', {
      reason,
      startedAt: blockStartedAt.value
    });
    blurFocusedElementInsideRoot();
    try {
      return await task();
    } finally {
      await releaseBlock(reason);
      chatDebugLog('messenger.interaction-blocker', 'block-end', {
        reason
      });
    }
  };

  return {
    isBlocked,
    label,
    activeReason,
    runWithBlock
  };
};
