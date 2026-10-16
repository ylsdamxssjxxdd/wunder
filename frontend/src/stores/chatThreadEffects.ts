import { threadViewActiveTurnId, threadViewOwnsExecution } from '@/realtime/chat/threadViewStore';
import { getChatThreadState } from '@/realtime/chat/chatThreadRuntime';
import type { ThreadChangeFrame } from '@/realtime/chat/chatThreadTypes';
import { markRuntimeProjectionChanged } from '@/realtime/chat/chatRuntimeProjectionInvalidation';
import { ensureRuntime, applyCanonicalStreamSideEffects, getSessionMessages } from './chatRuntimeState';
import { settleTerminalAssistantArtifacts } from './chatTerminalArtifacts';
import { emitAgentRuntimeRefresh, type AgentRuntimeCompletion } from '@/utils/workspaceEvents';
import {threadViewSlots,threadViewStatus} from '@/realtime/chat/threadViewStore';
import { refreshWorkspaceForThreadStatus, shouldRefreshWorkspaceForThreadStatus } from './chatWorkspaceEffects';

// Completion is a durable state transition, but the feeder may publish it as
// both turn_upsert and turn_status (and a reconnect can replay either shape).
// Keep the side effect idempotent per reducer state instance. A WeakMap makes
// the guard disappear when a session runtime is reset, so a later execution
// with the same identifiers is not accidentally suppressed.
const completedTurnClaims = new WeakMap<object, Set<string>>();
const workspaceTurnStatuses = new WeakMap<object, Map<string, string>>();
const workspaceViewChanges = new WeakMap<object, Map<string, number>>();

const viewCompletions=new Map<string,Set<string>>();
export const applyThreadViewEffects=(store,key:string,prior:Map<string,string>)=>{
  const previousStatus = ensureRuntime(key).threadStatus;
  const status = syncChatThreadShell(store,key, { refreshWorkspace: false });
  const agentId=store.sessions?.find(s=>s.id===key)?.agent_id;
  const slots = threadViewSlots(key) ?? [];
  const workspaceStatus = slots.find(slot =>
    shouldRefreshWorkspaceForThreadStatus(
      prior.get(slot.rootTurnId) ?? prior.get(`pending:${slot.assistant.client_message_id}`),
      slot.assistant.status
    )
  )?.assistant.status;
  const watermarks = workspaceViewChanges.get(store) ?? new Map<string, number>();
  workspaceViewChanges.set(store, watermarks);
  let watermark = 0;
  for (const slot of slots) watermark = Math.max(watermark, Number(slot.assistant.workspace_change_seq || 0));
  const workspaceChanged = watermark > (watermarks.get(key) ?? 0);
  if (workspaceChanged) {
    watermarks.delete(key);
    watermarks.set(key, watermark);
    while (watermarks.size > 32) watermarks.delete(watermarks.keys().next().value!);
  }
  if (workspaceChanged || workspaceStatus || shouldRefreshWorkspaceForThreadStatus(previousStatus, status)) {
    refreshWorkspaceForThreadStatus(store, key, workspaceChanged ? 'workspace_changed' : workspaceStatus || status);
  }
  const claims=viewCompletions.get(key)??new Set<string>();viewCompletions.delete(key);viewCompletions.set(key,claims);
  while(viewCompletions.size>32)viewCompletions.delete(viewCompletions.keys().next().value!);
  const completedTurns:AgentRuntimeCompletion[]=[];
  for(const slot of slots){const status=String(slot.assistant.status),turnId=String(slot.assistant.turn_id);
    const previous = prior.get(slot.rootTurnId) ?? prior.get(`pending:${slot.assistant.client_message_id}`);
    if(!terminal(status)||previous===undefined||terminal(previous)||claims.has(turnId))continue;
    claims.add(turnId);if(claims.size>256)claims.delete(claims.values().next().value!);
    completedTurns.push({sessionId:key,turnId,status:status==='completed'?'completed':status==='failed'?'failed':'cancelled',...(agentId?{agentId}: {})});
  }
  if(completedTurns.length)emitAgentRuntimeRefresh({...(agentId?{agentIds:[agentId]}:{}),completedTurns});
};

const terminal = (status: unknown) => ['completed', 'failed', 'cancelled', 'interrupted', 'rejected', 'stopped'].includes(String(status));

/** Bridge committed state to shell controls. Never feed timeline data into another reducer. */
export const syncChatThreadShell = (store, key: string, options: { refreshWorkspace?: boolean } = {}): string | null => {
  const previousStatus = ensureRuntime(key).threadStatus;
  const refreshWorkspace = (status: string) => {
    if (options.refreshWorkspace !== false && previousStatus !== 'not_loaded' &&
        shouldRefreshWorkspaceForThreadStatus(previousStatus, status)) {
      refreshWorkspaceForThreadStatus(store, key, status);
    }
  };
  const slots=threadViewSlots(key);
  if(slots) {
    const active=slots.find(s=>!terminal(s.assistant.status));
    const latest=slots[slots.length-1];
    const status=String(threadViewStatus(key)??active?.assistant.status??latest?.assistant.status??'idle');
    const runtime=ensureRuntime(key);runtime.threadStatus=status;runtime.loaded=true;runtime.activeTurnId=threadViewOwnsExecution(key)?threadViewActiveTurnId(key):active?.rootTurnId??'';
    runtime.lastThreadStatusAt = Date.now();
    const session=store.sessions?.find(s=>s.id===key);if(session){session.runtime_status=status;session.runtimeStatus=status;session.thread_status=status;session.threadStatus=status;}
    const busy = ['accepted','running','queued','waiting_input','waiting_approval'].includes(status);
    if (busy) store.loadingBySession[key] = true;
    else {
      delete store.loadingBySession[key];
      runtime.pendingApprovalIds = [];
      runtime.pendingApprovalCount = 0;
      runtime.waitingForUserInput = false;
    }
    const projection = store.runtimeProjection?.sessions?.[key];
    const busyReason = busy ? (status === 'queued' ? 'queued' : 'streaming') : null;
    const shellChanged = previousStatus !== status || (projection &&
      (projection.runtimeStatus !== status || projection.busyReason !== busyReason));
    if (projection) {
      projection.runtimeStatus = status;
      projection.busyReason = busyReason;
    }
    if (shellChanged) markRuntimeProjectionChanged(store, { sessionId: key, reason: 'thread_shell', immediate: true });
    refreshWorkspace(status);
    return status;
  }
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
  refreshWorkspace(status);
  return status;
};

export const applyChatThreadEffects = (store, key: string, changes: ThreadChangeFrame[]): void => {
  // The reducer invokes this only for accepted changes, including drained gap frames.
  // Replayed seq/revisions therefore cannot repeat side effects.
  const state = getChatThreadState(key);
  if (state) {
    const statuses = workspaceTurnStatuses.get(state) ?? new Map<string, string>();
    workspaceTurnStatuses.set(state, statuses);
    let refreshStatus = '';
    for (const change of changes) {
      if (!['turn_upsert', 'turn_status'].includes(change.change_type)) continue;
      const turnId = String(change.data.turn_id ?? change.turn_id ?? '');
      const status = String(change.data.status ?? '');
      if (!turnId || !status) continue;
      if (shouldRefreshWorkspaceForThreadStatus(statuses.get(turnId), status)) refreshStatus = status;
      statuses.set(turnId, status);
      if (statuses.size > 2048) statuses.delete(statuses.keys().next().value!);
    }
    if (refreshStatus) refreshWorkspaceForThreadStatus(store, key, refreshStatus);
  }
  for (const change of changes) {
    if (change.change_type !== 'item_upsert') continue;
    const data = change.data;
    const eventType = String(data.event_type ?? data.kind ?? '');
    applyCanonicalStreamSideEffects(store, key, eventType, { session_id: key, data });
  }
  if (changes.some((change) => change.change_type === 'turn_upsert' || change.change_type === 'turn_status' || ['terminal', 'user_message'].includes(String(change.data.kind)))) {
    syncChatThreadShell(store, key, { refreshWorkspace: false });
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
