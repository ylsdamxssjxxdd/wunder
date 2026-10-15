import { normalizeThreadRuntimeStatus } from '@/utils/chatSessionRuntime';
import { resolveAgentRuntimeTerminalStateFromSessionStatus } from './agentRuntimeState';
import type { AgentRuntimeState } from './model';

type RuntimeMessage = Record<string, unknown>;

export type TaskIndicatorState = AgentRuntimeState | 'inquiry' | 'approval';

export const resolveTaskIndicatorState = (
  state: AgentRuntimeState,
  projectionStatus: unknown,
  persistedStatus: unknown,
  runtimeStatus?: unknown
): TaskIndicatorState => {
  if (state === 'done' || state === 'error' || state === 'idle') return state;
  const runtime = normalizeThreadRuntimeStatus(runtimeStatus);
  const projected = normalizeThreadRuntimeStatus(projectionStatus);
  const status = runtime === 'waiting_user_input' || runtime === 'waiting_approval' ? runtime
    : projected !== 'not_loaded' && projected !== 'idle' ? projected
    : runtime !== 'not_loaded' && runtime !== 'idle' ? runtime
    : normalizeThreadRuntimeStatus(persistedStatus);
  if (status === 'waiting_user_input') return 'inquiry';
  if (status === 'waiting_approval') return 'approval';
  if (status === 'queued') return 'pending';
  return state;
};

const resolveExplicitTerminalTaskState = (value: unknown): AgentRuntimeState | null => {
  const status = normalizeThreadRuntimeStatus(value);
  // `idle` is emitted after a terminal turn as a transport state. It does not
  // identify the result by itself, so let cached messages/persisted state
  // resolve it below.
  if (status === 'idle' || status === 'not_loaded') return null;
  const terminal = resolveAgentRuntimeTerminalStateFromSessionStatus(status);
  if (!terminal) return null;
  return status === 'completed' ? 'done' : terminal === 'error' ? 'error' : 'idle';
};

export const hasCompletedTaskTurn = (messages: unknown): boolean => {
  if (!Array.isArray(messages) || messages.length === 0) return false;
  let latestUserIndex = -1;
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    if (String((messages[index] as RuntimeMessage)?.role || '').trim().toLowerCase() === 'user') {
      latestUserIndex = index;
      break;
    }
  }
  if (latestUserIndex < 0) return false;
  return messages.slice(latestUserIndex + 1).some((item) => {
    if (!item || typeof item !== 'object' || Array.isArray(item)) return false;
    const message = item as RuntimeMessage;
    if (String(message.role || '').trim().toLowerCase() !== 'assistant') return false;
    const status = String(message.status ?? message.runtime_status ?? '').trim().toLowerCase();
    return message.final === true || message.is_final === true ||
      ['final', 'completed', 'done'].includes(status) ||
      (message.stream_incomplete !== true && message.workflowStreaming !== true && Boolean(String(message.content || '').trim()));
  });
};

export function resolveTaskRuntimeState(
  projectionStatus: unknown,
  persistedStatus: unknown,
  loading: boolean,
  runtimeStatus?: unknown,
  runtimeActive = false
): AgentRuntimeState {
  const live = String(projectionStatus || '').trim().toLowerCase();
  const runtimeRaw = String(runtimeStatus || '').trim().toLowerCase();
  const runtime = normalizeThreadRuntimeStatus(runtimeRaw);
  // A durable terminal update can arrive before the request-scoped stream has
  // completed its local teardown. In that short window the controller is
  // merely a transport cleanup handle; it must not revive the work-thread
  // spinner after the task result is already known.
  const projectedTerminal = resolveExplicitTerminalTaskState(live);
  if (projectedTerminal) return projectedTerminal;
  const runtimeTerminal = resolveExplicitTerminalTaskState(runtimeRaw);
  if (runtimeTerminal) return runtimeTerminal;
  // Switching the foreground thread detaches its watcher, but an in-flight
  // send/resume controller still represents real work until a durable
  // terminal transition arrives.
  if (runtimeActive) return 'running';
  // The active session projection can briefly publish `idle` while its
  // watcher is being detached during a thread switch. A durable/runtime
  // status is authoritative in that gap and keeps the row visibly working.
  if (runtime === 'queued' || runtime === 'waiting_approval' || runtime === 'waiting_user_input') return 'pending';
  if (runtime === 'running' || runtimeRaw === 'finalizing') return 'running';
  // An explicit idle projection settles a stale catalog running state as well.
  const status = live && live !== 'not_loaded' ? live : String(persistedStatus || '').trim().toLowerCase();
  const normalized = normalizeThreadRuntimeStatus(status);
  if (normalized === 'queued' || normalized === 'waiting_approval' || normalized === 'waiting_user_input') return 'pending';
  if (normalized === 'running' || status === 'finalizing' || status === 'resuming') return 'running';
  if (normalized !== 'idle') {
    const terminal = resolveAgentRuntimeTerminalStateFromSessionStatus(normalized);
    if (terminal) {
      if (normalized === 'completed') return 'done';
      return terminal === 'error' ? 'error' : 'idle';
    }
  }
  // `session_idle` is the transport state after a completed turn, not the
  // user-facing result. Preserve the terminal result long enough for the row
  // to show its completed/error icon.
  const persisted = normalizeThreadRuntimeStatus(persistedStatus);
  if (persisted !== 'idle' && persisted !== 'not_loaded') {
    const terminal = resolveAgentRuntimeTerminalStateFromSessionStatus(persisted);
    if (terminal) {
      if (persisted === 'completed') return 'done';
      return terminal === 'error' ? 'error' : 'idle';
    }
  }
  return loading ? 'running' : 'idle';
}
