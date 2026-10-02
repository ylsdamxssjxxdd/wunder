import { cancelMessageStream } from '@/api/chat';
import { t } from '@/i18n';
import { chatDebugLog, isChatDebugVerboseEnabled } from '@/utils/chatDebug';
import { buildMessageIdentityDebugList, buildMessageIdentityDebugSnapshot } from '@/utils/chatMessageDebug';
import { clearSupersededPendingAssistantMessages, findPendingAssistantMessage, stopPendingAssistantMessage } from './chatPendingMessage';
import { isCompactionMarkerAssistantMessage } from './chatCompactionMarker';
import { normalizeInquiryPanelState } from './chatDemoPanels';
import { writeSessionGoalState } from './chatPersist';
import { abortWatchStream } from './chatRuntimeControls';
import { buildRuntimeDebugSnapshot, cacheSessionMessages, clearSessionEventsSnapshot, ensureRuntime, getSessionMessages, notifySessionSnapshot, resolveSessionKey, settleUserStoppedSessionRuntime, touchSessionUpdatedAt } from './chatRuntimeState';
import { settleTerminalAssistantArtifacts as settleTerminalAssistantArtifactsBase } from './chatTerminalArtifacts';
import { clearAssistantRetryState } from './chatStats';
import type { ResumeStreamOptions } from './chatTypes';
import { abortCompactRequest, abortResumeStream, abortSendStream, chatWsClient, finalizeManualCompactionAsCancelled, startSessionWatcher } from './chatWatcher';

const sendWsCancelForSessionStop = (
  sessionId: string
): void => {
  // A stop is scoped to the durable session.  Cancelling each start, resume
  // and watch request first caused repeated server settlements for one click.
  void chatWsClient
    .notify({
      type: 'cancel',
      session_id: sessionId,
      payload: {
        session_id: sessionId,
        cancel_source: 'user_stop'
      }
    })
    .catch((error) => {
      chatDebugLog('messenger.send', 'stop-session-ws-cancel-failed', {
        sessionId,
        message: String((error as { message?: unknown })?.message || '')
      });
    });
};

export const chatStopResumeActions = {
    async stopSessionActivity(
      sessionId = null,
      options: { terminateSubagents?: boolean } = {}
    ) {
      const targetSessionId = resolveSessionKey(sessionId || this.activeSessionId);
      if (!targetSessionId) {
        return false;
      }
      clearSessionEventsSnapshot(targetSessionId);
      const runtime = ensureRuntime(targetSessionId);
      if (runtime) {
        runtime.stopRequested = true;
        runtime.sendAbortReason = 'user_stop';
        runtime.resumeAbortReason = 'user_stop';
      }
      sendWsCancelForSessionStop(targetSessionId);
      abortSendStream(targetSessionId);
      abortResumeStream(targetSessionId);
      abortCompactRequest(targetSessionId);
      abortWatchStream(targetSessionId);
      let cancelled = false;
      const targetMessages =
        String(this.activeSessionId || '').trim() === targetSessionId
          ? this.messages
          : getSessionMessages(targetSessionId);
      clearSupersededPendingAssistantMessages(targetMessages);
      const pendingAssistant = findPendingAssistantMessage(targetMessages);
      if (pendingAssistant) {
        if (isCompactionMarkerAssistantMessage(pendingAssistant)) {
          finalizeManualCompactionAsCancelled(pendingAssistant);
        } else {
          pendingAssistant.workflowStreaming = false;
          pendingAssistant.reasoningStreaming = false;
          pendingAssistant.stream_incomplete = false;
          pendingAssistant.resume_available = false;
          pendingAssistant.status = 'cancelled';
          pendingAssistant.cancelled = true;
          pendingAssistant.failed = false;
          pendingAssistant.final = false;
          pendingAssistant.stop_reason = 'user_stop';
          clearAssistantRetryState(pendingAssistant);
          if (!pendingAssistant.content) {
            pendingAssistant.content = t('chat.workflow.aborted');
          }
        }
        const panel = normalizeInquiryPanelState(pendingAssistant.questionPanel);
        if (panel && panel.status === 'pending') {
          pendingAssistant.questionPanel = { ...panel, status: 'dismissed' };
        }
        stopPendingAssistantMessage(pendingAssistant, {
          cancelled: true,
          stopReason: 'user_stop'
        });
        cancelled = true;
      }
      chatDebugLog('messenger.send', 'stop-session-activity', {
        sessionId: targetSessionId,
        terminateSubagents: options.terminateSubagents !== false,
        runtime: runtime ? buildRuntimeDebugSnapshot(runtime) : null,
        pendingAssistant: buildMessageIdentityDebugSnapshot(
          pendingAssistant,
          Array.isArray(targetMessages) ? targetMessages.indexOf(pendingAssistant) : -1
        ),
        ...(isChatDebugVerboseEnabled()
          ? { messages: buildMessageIdentityDebugList(targetMessages) }
          : {})
      });
      this.dismissPendingInquiryPanel();
      if (Array.isArray(targetMessages)) {
        // A user stop is terminal, but it is not an execution failure. Keep
        // completed work intact and settle only unfinished artifacts as cancelled.
        settleTerminalAssistantArtifactsBase(targetMessages, { cancelled: true });
        cacheSessionMessages(targetSessionId, targetMessages);
        touchSessionUpdatedAt(this, targetSessionId, Date.now());
        notifySessionSnapshot(this, targetSessionId, targetMessages, true);
      }
      const locallyStopped = settleUserStoppedSessionRuntime(this, targetSessionId);
      if (Array.isArray(targetMessages)) {
        // The first snapshot above is taken while the runtime is still busy.
        // Publish the stopped snapshot again after projection settlement.
        notifySessionSnapshot(this, targetSessionId, targetMessages, true);
      }
      chatDebugLog('messenger.send', 'stop-session-local-settled', {
        sessionId: targetSessionId,
        locallyStopped,
        runtime: runtime ? buildRuntimeDebugSnapshot(runtime) : null,
        ...(isChatDebugVerboseEnabled()
          ? { messages: buildMessageIdentityDebugList(targetMessages) }
          : {})
      });
      try {
        const { data } = await cancelMessageStream(targetSessionId);
        cancelled = Boolean(data?.data?.cancelled) || cancelled;
        if (data?.data?.goal_cleared === true) {
          writeSessionGoalState(this, targetSessionId, null, { clear: true });
          cancelled = true;
        }
      } catch (error) {
        chatDebugLog('messenger.send', 'stop-session-cancel-request-failed', {
          sessionId: targetSessionId,
          message: String((error as { message?: unknown })?.message || '')
        });
        // Ignore cancel API failures; local stop behavior still applies.
      }
      // The server owns recursive interruption. Keep children available for later reuse.
      void this.refreshSessionSubagents(targetSessionId, { force: true }).catch(() => {});
      return locallyStopped || cancelled;
    },
    async stopStream() {
      return this.stopSessionActivity(this.activeSessionId, { terminateSubagents: true });
    },
    async resumeStream(sessionId, message, options: ResumeStreamOptions = {}) {
      if (!message && options.force !== true) return;
      if (message && !message.stream_incomplete && options.force !== true) return;
      // Recovery observes the existing turn; opening a transport never starts work.
      abortResumeStream(sessionId);
      clearSessionEventsSnapshot(sessionId);
      await this.loadSessionDetail(sessionId, {
        preserveWatcher: true,
        startWatcherAfterHydration: false
      });
      if (resolveSessionKey(this.activeSessionId) === resolveSessionKey(sessionId)) {
        startSessionWatcher(this, sessionId);
      }
    }
};
