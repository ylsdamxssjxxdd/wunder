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
  isSessionDetailWarm,
  isSessionUnavailableStatus,
  loadSessionEventsSnapshot,
  resolveChatHttpStatus,
  resolveSessionKey,
  settleTerminalSessionRuntime
} from './chatRuntimeState';
import { normalizeStreamEventId } from './chatStreamIds';
import { isTerminalRuntimeStatus } from './chatWorkflowHydration';
import { startSessionWatcher } from './chatWatcher';
import {
  resolveActiveSessionRealtimeRecoveryPlan,
  shouldReconcileInteractiveStream
} from './chatActiveSessionRealtime';

const normalizeErrorMessage = (error: unknown): string =>
  String((error as { message?: unknown })?.message || error || '').trim();

export const chatRealtimeRecoveryActions = {
  async ensureActiveSessionRealtime(options: {
    sessionId?: unknown;
    reason?: string;
    hydrateIfCold?: boolean;
    forceHydrate?: boolean;
  } = {}) {
    const recoveryStart = typeof performance !== 'undefined' ? performance.now() : Date.now();
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
    const hasCachedMessages = Array.isArray(messages) && messages.length > 0;
    const hasWarmDetail = isSessionDetailWarm(targetSessionId);
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
      forceHydrate: options.forceHydrate === true,
      hasWarmDetail,
      hasCachedMessages
    });

    if (plan === 'skip_interactive_stream' && shouldReconcileInteractiveStream(runtime)) {
      try {
        const localLastEventId = resolveMaterializedMessageEventId(messages);
        const snapshot = await loadSessionEventsSnapshot(targetSessionId, {
          allowCached: false,
          dedupeInFlight: false,
          minLastEventId: localLastEventId
        });
        const remoteLastEventId = normalizeStreamEventId(
          snapshot?.last_event_id ?? snapshot?.lastEventId
        );
        const rawRuntimeStatus = snapshot?.runtime?.thread_status ?? snapshot?.runtime?.status;
        const hasRuntimeStatus = String(rawRuntimeStatus ?? '').trim().length > 0;
        const remoteRuntimeStatus = normalizeThreadRuntimeStatus(rawRuntimeStatus);
        recoverRuntimeInteractiveControllers(this, targetSessionId, runtime, {
          remoteRunning: snapshot?.running,
          remoteLastEventId,
          localLastEventId
        });
        if (
          snapshot?.running === false ||
          (hasRuntimeStatus && isTerminalRuntimeStatus(remoteRuntimeStatus))
        ) {
          if (remoteRuntimeStatus !== 'not_loaded') {
            runtime.threadStatus = remoteRuntimeStatus;
            runtime.loaded = true;
          }
          settleTerminalSessionRuntime(this, targetSessionId, {
            eventType: 'interactive_reconcile',
            failed: remoteRuntimeStatus === 'system_error'
          });
          chatDebugLog('messenger.conversation', 'active-realtime-interactive-settled', {
            sessionId: targetSessionId,
            reason: String(options.reason || '').trim(),
            remoteLastEventId,
            localLastEventId,
            runtime: buildRuntimeDebugSnapshot(runtime)
          });
          return { status: 'settled', plan, sessionId: targetSessionId };
        }
        chatDebugLog('messenger.conversation', 'active-realtime-interactive-still-running', {
          sessionId: targetSessionId,
          reason: String(options.reason || '').trim(),
          remoteRunning: snapshot?.running,
          remoteRuntimeStatus,
          remoteLastEventId,
          localLastEventId,
          runtime: buildRuntimeDebugSnapshot(runtime)
        });
      } catch (error) {
        if (isSessionUnavailableStatus(resolveChatHttpStatus(error))) {
          throw error;
        }
        chatDebugLog('messenger.conversation', 'active-realtime-interactive-reconcile-failed', {
          sessionId: targetSessionId,
          reason: String(options.reason || '').trim(),
          error: normalizeErrorMessage(error)
        });
      }
    }

    chatDebugLog('messenger.conversation', 'active-realtime-recovery-plan', {
      sessionId: targetSessionId,
      reason: String(options.reason || '').trim(),
      plan,
      messageCount: Array.isArray(messages) ? messages.length : 0,
      hasWarmDetail,
      hasCachedMessages,
      runtime: buildRuntimeDebugSnapshot(runtime)
    });

    if (plan.startsWith('skip_')) {
      return { status: 'skipped', plan, sessionId: targetSessionId };
    }

    let hydrateDurationMs: number | null = null;
    let finalPlan = plan;
    if (plan === 'hydrate_then_watch') {
      const hydrateStart = typeof performance !== 'undefined' ? performance.now() : Date.now();
      try {
        await this.loadSessionDetail(targetSessionId, {
          preserveWatcher: true,
          forceHydrateForeground: true
        });
        hydrateDurationMs = (typeof performance !== 'undefined' ? performance.now() : Date.now()) - hydrateStart;
      } catch (error) {
        chatDebugLog('messenger.conversation', 'active-realtime-hydrate-failed', {
          sessionId: targetSessionId,
          reason: String(options.reason || '').trim(),
          error: normalizeErrorMessage(error)
        });
      }
      const hydratedRuntime = getRuntime(targetSessionId) || runtime;
      const hydratedMessages =
        resolveSessionKey(this.activeSessionId) === targetSessionId && Array.isArray(this.messages)
          ? this.messages
          : getSessionMessages(targetSessionId);
      const hydratedRuntimeStatus = normalizeThreadRuntimeStatus(hydratedRuntime?.threadStatus);
      finalPlan = resolveActiveSessionRealtimeRecoveryPlan({
        targetSessionId,
        activeSessionId: resolveSessionKey(this.activeSessionId),
        hasWatchController: Boolean(hydratedRuntime?.watchController),
        hasSendController: Boolean(hydratedRuntime?.sendController),
        hasResumeController: Boolean(hydratedRuntime?.resumeController),
        loading: Boolean(this.loadingBySession?.[targetSessionId]),
        runtimeBusy: isThreadRuntimeBusy(hydratedRuntimeStatus),
        hasPendingAssistant: Boolean(findPendingAssistantMessage(hydratedMessages)),
        hasRunningAssistant: hasRunningAssistantMessage(hydratedMessages),
        hydrateIfCold: false,
        forceHydrate: false,
        hasWarmDetail: isSessionDetailWarm(targetSessionId),
        hasCachedMessages: Array.isArray(hydratedMessages) && hydratedMessages.length > 0
      });
    }

    const nextRuntime = getRuntime(targetSessionId) || runtime;
    if (finalPlan.startsWith('skip_')) {
      chatDebugLog('messenger.conversation', 'active-realtime-recovery-finish', {
        sessionId: targetSessionId,
        reason: String(options.reason || '').trim(),
        plan: finalPlan,
        initialPlan: plan,
        status: 'idle_confirmed',
        hydrateDurationMs,
        totalDurationMs:
          (typeof performance !== 'undefined' ? performance.now() : Date.now()) - recoveryStart
      });
      return { status: 'skipped', plan: finalPlan, sessionId: targetSessionId };
    }
    if (
      !nextRuntime?.watchController &&
      !nextRuntime?.sendController &&
      !nextRuntime?.resumeController &&
      hasKnownSessionInStore(this, targetSessionId)
    ) {
      startSessionWatcher(this, targetSessionId);
      chatDebugLog('messenger.conversation', 'active-realtime-recovery-finish', {
        sessionId: targetSessionId,
        reason: String(options.reason || '').trim(),
        plan: finalPlan,
        initialPlan: plan,
        status: 'watch_started',
        hydrateDurationMs,
        totalDurationMs:
          (typeof performance !== 'undefined' ? performance.now() : Date.now()) - recoveryStart
      });
      return { status: 'watch_started', plan: finalPlan, sessionId: targetSessionId };
    }

    chatDebugLog('messenger.conversation', 'active-realtime-recovery-finish', {
      sessionId: targetSessionId,
      reason: String(options.reason || '').trim(),
      plan: finalPlan,
      initialPlan: plan,
      status: 'already_realtime_driven',
      hydrateDurationMs,
      totalDurationMs:
        (typeof performance !== 'undefined' ? performance.now() : Date.now()) - recoveryStart
    });
    return {
      status: 'already_realtime_driven',
      plan: finalPlan,
      sessionId: targetSessionId,
      runtime: buildRuntimeDebugSnapshot(nextRuntime)
    };
  }
};
