import type { AgentRuntimeState } from './model';

export const WAITING_SESSION_RUNTIME_STATUS_SET = new Set([
  'queued',
  'waiting_approval',
  'waiting_user_input'
]);

export const TERMINAL_SESSION_RUNTIME_STATUS_SET = new Set([
  'idle',
  'not_loaded',
  'completed',
  'failed',
  'cancelled',
  'canceled',
  'done',
  'error',
  'system_error'
]);

export const normalizeMessengerRuntimeStatus = (value: unknown): string =>
  String(value || '').trim().toLowerCase();

export const isWaitingMessengerRuntimeStatus = (value: unknown): boolean =>
  WAITING_SESSION_RUNTIME_STATUS_SET.has(normalizeMessengerRuntimeStatus(value));

export const isTerminalMessengerRuntimeStatus = (value: unknown): boolean =>
  TERMINAL_SESSION_RUNTIME_STATUS_SET.has(normalizeMessengerRuntimeStatus(value));

export const isTerminalAgentRuntimeState = (value: unknown): boolean => {
  const state = normalizeMessengerRuntimeStatus(value);
  return state === 'done' || state === 'error';
};

export const resolveAgentRuntimeStateFromSignals = (options: {
  pendingApproval?: boolean;
  pendingInquiry?: boolean;
  localWaiting?: boolean;
  localStreaming?: boolean;
  remoteState?: AgentRuntimeState | null;
  overrideState?: AgentRuntimeState | null;
}): AgentRuntimeState => {
  const remoteState = options.remoteState || 'idle';
  if (
    options.pendingApproval === true ||
    options.pendingInquiry === true ||
    options.localWaiting === true ||
    remoteState === 'pending'
  ) {
    return 'pending';
  }
  if (isTerminalAgentRuntimeState(remoteState) && options.overrideState === 'running') {
    return remoteState;
  }
  if (options.overrideState) {
    return options.overrideState;
  }
  if (isTerminalAgentRuntimeState(remoteState)) {
    return remoteState;
  }
  if (options.localStreaming === true) {
    return 'running';
  }
  return remoteState;
};
