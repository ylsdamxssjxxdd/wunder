import { getChatThreadState } from '@/realtime/chat/chatThreadRuntime';
import type { ThreadChangeFrame } from '@/realtime/chat/chatThreadTypes';
import { markRuntimeProjectionChanged } from '@/realtime/chat/chatRuntimeProjectionInvalidation';
import { ensureRuntime, applyCanonicalStreamSideEffects, getSessionMessages } from './chatRuntimeState';
import { settleTerminalAssistantArtifacts } from './chatTerminalArtifacts';
import { emitAgentRuntimeRefresh, type AgentRuntimeCompletion } from '@/utils/workspaceEvents';

// Completion is a durable state transition, but the feeder may publish it as
// both turn_upsert and turn_status (and a reconnect can replay either shape).
// Keep the side effect idempotent per reducer state instance. A WeakMap makes
// the guard disappear when a session runtime is reset, so a later execution
// with the same identifiers is not accidentally suppressed.
const completedTurnClaims = new WeakMap<object, Set<string>>();

const terminal = (status: unknown) => ['completed', 'failed', 'cancelled', 'interrupted', 'rejected', 'stopped'].includes(String(status));

/** Bridge committed state to shell controls. Never feed timeline data into another reducer. */
export const syncChatThreadShell = (store, key: string): string | null => {
  const state = getChatThreadState(key);
  if (!state?.turns.size) return null;
  const turns = [...state.turns.values()];
  const active = turns.find((turn) => turn.status && !terminal(turn.status));
  const latest = turns.reduce((a, b) => (b.userRound ?? 0) >= (a.userRound ?? 0) ? b : a);
  const status = active?.status ?? latest.status ?? 'idle';
  const runtime = ensureRuntime(key);
  runtime.threadStatus = status;
  runtime.loaded = true;
  runtime.activeTurnId = active?.turnId ?? '';
  runtime.lastThreadStatusAt = Date.now();
  const session = Array.isArray(store.sessions)
    ? store.sessions.find((item) => String(item?.id || '').trim() === key)
    : null;
  if (session) {
    // Keep the catalog row aligned with the durable turn state. The transport
    // may emit session_idle immediately after a terminal turn; retaining the
    // completed/failed status here lets the work-thread icon communicate the
    // result instead of falling back to a generic idle icon.
    session.runtime_status = status;
    session.runtimeStatus = status;
  }
  const projection = store.runtimeProjection?.sessions?.[key];
  if (projection) {
    projection.runtimeStatus = status;
    projection.busyReason = active ? (status === 'queued' ? 'queued' : 'streaming') : null;
  }
  if (!active) {
    delete store.loadingBySession[key];
    runtime.pendingApprovalIds = [];
    runtime.pendingApprovalCount = 0;
    runtime.waitingForUserInput = false;
    const messages = store.activeSessionId === key ? store.messages : getSessionMessages(key);
    settleTerminalAssistantArtifacts(messages, { failed: status === 'failed', cancelled: status === 'cancelled' });
  }
  markRuntimeProjectionChanged(store, { sessionId: key, reason: 'thread_shell', immediate: true });
  return status;
};

export const applyChatThreadEffects = (store, key: string, changes: ThreadChangeFrame[]): void => {
  // The reducer invokes this only for accepted changes, including drained gap frames.
  // Replayed seq/revisions therefore cannot repeat side effects.
  for (const change of changes) {
    if (change.change_type !== 'item_upsert') continue;
    const data = change.data;
    const eventType = String(data.event_type ?? data.kind ?? '');
    applyCanonicalStreamSideEffects(store, key, eventType, { session_id: key, data });
  }
  if (changes.some((change) => change.change_type === 'turn_upsert' || change.change_type === 'turn_status' || ['terminal', 'user_message'].includes(String(change.data.kind)))) {
    syncChatThreadShell(store, key);
    if (changes.some((change) => change.change_type === 'turn_status' || (change.change_type === 'turn_upsert' && terminal(change.data.status)) || change.data.kind === 'terminal')) {
      const agentId = store.sessions?.find((session) => session.id === key)?.agent_id;
      // Only a durable normal completion can acknowledge a finished task.
      // Failure and cancellation stay visible on their original assistant
      // bubble and must never be presented as successful completion.
      const state = getChatThreadState(key);
      const claims = state ? (completedTurnClaims.get(state) ?? new Set<string>()) : null;
      if (state && claims && !completedTurnClaims.has(state)) completedTurnClaims.set(state, claims);
      const completedTurns: AgentRuntimeCompletion[] = changes
        .filter((change) =>
          (change.change_type === 'turn_upsert' || change.change_type === 'turn_status') &&
          ['completed', 'failed', 'cancelled', 'canceled', 'interrupted', 'stopped']
            .includes(String(change.data.status ?? '').toLowerCase())
        )
        .map((change) => ({
          sessionId: key,
          turnId: String(change.data.turn_id ?? change.turn_id ?? '').trim(),
          status: String(change.data.status ?? '').toLowerCase() === 'failed'
            ? 'failed' as const
            : ['cancelled', 'canceled', 'interrupted', 'stopped'].includes(String(change.data.status ?? '').toLowerCase())
              ? 'cancelled' as const
              : 'completed' as const,
          ...(agentId ? { agentId: String(agentId) } : {})
        }))
        .filter((completion) => Boolean(completion.turnId));
      const freshCompletedTurns = completedTurns.filter((completion) => {
        if (!claims) return true;
        const claim = `${completion.sessionId}:${completion.turnId}`;
        if (claims.has(claim)) return false;
        claims.add(claim);
        // A bounded presentation guard; the durable state itself remains
        // unbounded only for the session lifetime and is replaced on reset.
        if (claims.size > 2048) claims.delete(claims.values().next().value as string);
        return true;
      });
      emitAgentRuntimeRefresh({
        ...(agentId ? { agentIds: [agentId] } : {}),
        ...(freshCompletedTurns.length ? { completedTurns: freshCompletedTurns } : {})
      });
    }
  }
};
