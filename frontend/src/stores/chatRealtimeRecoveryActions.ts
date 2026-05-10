import { chatDebugLog } from '@/utils/chatDebug';
import {
  hasRunningAssistantMessage,
  isThreadRuntimeBusy,
  normalizeThreadRuntimeStatus
} from '@/utils/chatSessionRuntime';
import { findPendingAssistantMessage } from './chatPendingMessage';
import {
  recoverRuntimeInteractiveControllers,
  resolveMaterializedMessageEventId
} from './chatRuntimeControls';
import {
  buildRuntimeDebugSnapshot,
  ensureRuntime,
  getRuntime,
  getSessionMessages,
  hasKnownSessionInStore,
  resolveSessionKey
} from './chatRuntimeState';
import { startSessionWatcher } from './chatWatcher';
import { resolveActiveSessionRealtimeRecoveryPlan } from './chatActiveSessionRealtime';

const normalizeErrorMessage = (error: unknown): string =>
  String((error as { message?: unknown })?.message || error || '').trim();

export const chatRealtimeRecoveryActions = {
  async ensureActiveSessionRealtime(options: {
    sessionId?: unknown;
    reason?: string;
    hydrateIfCold?: boolean;
    forceHydrate?: boolean;
  } = {}) {
    const targetSessionId = resolveSessionKey(options.sessionId || this.activeSessionId);
    if (!targetSessionId) {
      return { status: 'skipped', plan: 'skip_no_session' };
    }
    if (!hasKnownSessionInStore(this, targetSessionId)) {
      return { status: 'skipped', plan: 'skip_unknown_session', sessionId: targetSessionId };
    }

    const activeSessionId = resolveSessionKey(this.activeSessionId);
    const runtime = ensureRuntime(targetSessionId);
    const messages =
      activeSessionId === targetSessionId && Array.isArray(this.messages)
        ? this.messages
        : getSessionMessages(targetSessionId);
    recoverRuntimeInteractiveControllers(this, targetSessionId, runtime, {
      localLastEventId: resolveMaterializedMessageEventId(messages)
    });
    const runtimeStatus = normalizeThreadRuntimeStatus(runtime?.threadStatus);
    const plan = resolveActiveSessionRealtimeRecoveryPlan({
      targetSessionId,
      activeSessionId,
      hasWatchController: Boolean(runtime?.watchController),
      hasSendController: Boolean(runtime?.sendController),
      hasResumeController: Boolean(runtime?.resumeController),
      loading: Boolean(this.loadingBySession?.[targetSessionId]),
      runtimeBusy: isThreadRuntimeBusy(runtimeStatus),
      hasPendingAssistant: Boolean(findPendingAssistantMessage(messages)),
      hasRunningAssistant: hasRunningAssistantMessage(messages),
      hydrateIfCold: options.hydrateIfCold !== false,
      forceHydrate: options.forceHydrate === true
    });

    chatDebugLog('messenger.conversation', 'active-realtime-recovery-plan', {
      sessionId: targetSessionId,
      reason: String(options.reason || '').trim(),
      plan,
      messageCount: Array.isArray(messages) ? messages.length : 0,
      runtime: buildRuntimeDebugSnapshot(runtime)
    });

    if (plan.startsWith('skip_')) {
      return { status: 'skipped', plan, sessionId: targetSessionId };
    }

    if (plan === 'hydrate_then_watch') {
      try {
        await this.loadSessionDetail(targetSessionId, {
          preserveWatcher: true,
          forceHydrateForeground: true
        });
      } catch (error) {
        chatDebugLog('messenger.conversation', 'active-realtime-hydrate-failed', {
          sessionId: targetSessionId,
          reason: String(options.reason || '').trim(),
          error: normalizeErrorMessage(error)
        });
      }
    }

    const nextRuntime = getRuntime(targetSessionId) || runtime;
    if (
      !nextRuntime?.watchController &&
      !nextRuntime?.sendController &&
      !nextRuntime?.resumeController &&
      hasKnownSessionInStore(this, targetSessionId)
    ) {
      startSessionWatcher(this, targetSessionId);
      return { status: 'watch_started', plan, sessionId: targetSessionId };
    }

    return {
      status: 'already_realtime_driven',
      plan,
      sessionId: targetSessionId,
      runtime: buildRuntimeDebugSnapshot(nextRuntime)
    };
  }
};
