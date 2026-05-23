import { nextTick, onBeforeUnmount, onMounted, watch } from 'vue';

import { chatDebugLog } from '@/utils/chatDebug';

type MessengerRealtimeRecoveryContext = {
  sessionHub: {
    activeSection: string;
    activeConversationKey?: string;
  };
  chatStore: {
    activeSessionId?: unknown;
    ensureActiveSessionRealtime?: (options?: {
      reason?: string;
      hydrateIfCold?: boolean;
      forceHydrate?: boolean;
      keepActiveSessionWarm?: boolean;
    }) => Promise<unknown>;
  };
  isAgentConversationActive?: {
    value: boolean;
  };
};

const RECOVERY_DELAY_MS = 40;

const isPageVisible = (): boolean => {
  if (typeof document === 'undefined') return true;
  return document.visibilityState !== 'hidden';
};

const shouldRecoverActiveChat = (ctx: MessengerRealtimeRecoveryContext): boolean =>
  ctx.sessionHub.activeSection === 'messages' &&
  Boolean(String(ctx.chatStore.activeSessionId || '').trim()) &&
  ctx.isAgentConversationActive?.value !== false;

export const installActiveChatRealtimeRecovery = (
  ctx: MessengerRealtimeRecoveryContext
): void => {
  let recoveryTimer: number | null = null;

  const clearRecoveryTimer = () => {
    if (typeof window === 'undefined' || recoveryTimer === null) return;
    window.clearTimeout(recoveryTimer);
    recoveryTimer = null;
  };

  const runRecovery = (reason: string, forceHydrate = false) => {
    if (!shouldRecoverActiveChat(ctx)) return;
    const ensureRealtime = ctx.chatStore.ensureActiveSessionRealtime;
    if (typeof ensureRealtime !== 'function') return;
    void ensureRealtime.call(ctx.chatStore, {
      reason,
      hydrateIfCold: true,
      forceHydrate,
      keepActiveSessionWarm: true
    }).catch((error: unknown) => {
      chatDebugLog('messenger.conversation', 'active-realtime-recovery-failed', {
        reason,
        error: String((error as { message?: unknown })?.message || error || '').trim()
      });
    });
  };

  const scheduleRecovery = (reason: string, forceHydrate = false) => {
    if (!shouldRecoverActiveChat(ctx)) return;
    if (typeof window === 'undefined') {
      runRecovery(reason, forceHydrate);
      return;
    }
    clearRecoveryTimer();
    recoveryTimer = window.setTimeout(() => {
      recoveryTimer = null;
      runRecovery(reason, forceHydrate);
    }, RECOVERY_DELAY_MS);
  };

  const handleForegroundResume = () => {
    if (!isPageVisible()) return;
    scheduleRecovery('foreground-resume', false);
  };

  watch(
    () => [
      ctx.sessionHub.activeSection,
      String(ctx.chatStore.activeSessionId || '').trim(),
      String(ctx.sessionHub.activeConversationKey || '')
    ].join('::'),
    () => {
      void nextTick(() => scheduleRecovery('messenger-state-change', false));
    },
    { immediate: true, flush: 'post' }
  );

  onMounted(() => {
    if (typeof window === 'undefined') return;
    window.addEventListener('focus', handleForegroundResume);
    window.addEventListener('pageshow', handleForegroundResume);
    window.addEventListener('online', handleForegroundResume);
    document.addEventListener('visibilitychange', handleForegroundResume);
    scheduleRecovery('mounted', false);
  });

  onBeforeUnmount(() => {
    clearRecoveryTimer();
    if (typeof window === 'undefined') return;
    window.removeEventListener('focus', handleForegroundResume);
    window.removeEventListener('pageshow', handleForegroundResume);
    window.removeEventListener('online', handleForegroundResume);
    document.removeEventListener('visibilitychange', handleForegroundResume);
  });
};
