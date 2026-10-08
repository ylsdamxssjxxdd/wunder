import type { CompanionSpriteStateId } from '@/stores/companions';

export type AgentRuntimeStateLike = 'idle' | 'running' | 'done' | 'pending' | 'error';

export type CompanionRuntimeSpriteOptions = {
  pendingState?: CompanionSpriteStateId;
  doneState?: CompanionSpriteStateId;
  idleState?: CompanionSpriteStateId;
};

export const STATIC_COMPANION_AVATAR_STATE: CompanionSpriteStateId = 'idle';

export const normalizeAgentRuntimeState = (value: unknown): AgentRuntimeStateLike => {
  const normalized = String(value || '').trim().toLowerCase();
  if (normalized === 'running') return 'running';
  if (normalized === 'pending') return 'pending';
  if (normalized === 'done') return 'done';
  if (normalized === 'error') return 'error';
  return 'idle';
};

export const resolveCompanionSpriteStateForRuntime = (
  value: unknown,
  options: CompanionRuntimeSpriteOptions = {}
): CompanionSpriteStateId => {
  switch (normalizeAgentRuntimeState(value)) {
    case 'running':
      return 'running';
    case 'pending':
      return options.pendingState || 'waiting';
    case 'done':
      return options.doneState || 'jumping';
    case 'error':
      return 'failed';
    default:
      return options.idleState || 'idle';
  }
};
