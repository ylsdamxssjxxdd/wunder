import { normalizeThreadRuntimeStatus } from '@/utils/chatSessionRuntime';
import { resolveAgentRuntimeTerminalStateFromSessionStatus } from './agentRuntimeState';
import type { AgentRuntimeState } from './model';

export function resolveTaskRuntimeState(
  projectionStatus: unknown,
  persistedStatus: unknown,
  loading: boolean
): AgentRuntimeState {
  const live = String(projectionStatus || '').trim().toLowerCase();
  // An explicit idle projection settles a stale catalog running state as well.
  const status = live && live !== 'not_loaded' ? live : String(persistedStatus || '').trim().toLowerCase();
  const normalized = normalizeThreadRuntimeStatus(status);
  if (normalized === 'queued' || normalized === 'waiting_approval' || normalized === 'waiting_user_input') return 'pending';
  if (normalized === 'running' || status === 'finalizing' || status === 'resuming') return 'running';
  if (normalized !== 'idle') {
    const terminal = resolveAgentRuntimeTerminalStateFromSessionStatus(normalized);
    if (terminal) return terminal;
  }
  return loading ? 'running' : 'idle';
}
