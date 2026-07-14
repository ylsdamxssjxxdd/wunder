import type { AgentRuntimeState } from './model';

export const WAITING_SESSION_RUNTIME_STATUS_SET = new Set([
  'queued',
  'waiting_approval',
  'waiting_user_input'
]);

export const TERMINAL_SESSION_RUNTIME_STATUS_SET = new Set([
  'idle',
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

export const isHotAgentRuntimeState = (value: unknown): boolean => {
  const state = normalizeMessengerRuntimeStatus(value);
  return state === 'running' || state === 'pending';
};

export const resolveAgentRuntimeTerminalStateFromSessionStatus = (
  value: unknown
): AgentRuntimeState | null => {
  const status = normalizeMessengerRuntimeStatus(value);
  if (!TERMINAL_SESSION_RUNTIME_STATUS_SET.has(status)) {
    return null;
  }
  if (status === 'failed' || status === 'error' || status === 'system_error') {
    return 'error';
  }
  return 'done';
};

export const shouldSettleAgentRuntimeFromTerminalSession = (options: {
  sessionStatus?: unknown;
  currentState?: AgentRuntimeState | null;
  localStreaming?: boolean;
  localWaiting?: boolean;
  overrideState?: AgentRuntimeState | null;
}): boolean => {
  if (!resolveAgentRuntimeTerminalStateFromSessionStatus(options.sessionStatus)) {
    return false;
  }
  return (
    isHotAgentRuntimeState(options.currentState) ||
    isHotAgentRuntimeState(options.overrideState) ||
    options.localStreaming === true ||
    options.localWaiting === true
  );
};

export const hasAgentTerminalSettlementEvidence = (options: {
  targetSessionId?: unknown;
  currentRuntimeSessionId?: unknown;
  activeSessionId?: unknown;
  hasRuntimeActivity?: boolean;
  currentState?: AgentRuntimeState | null;
  localStreaming?: boolean;
  localWaiting?: boolean;
  overrideState?: AgentRuntimeState | null;
}): boolean => {
  if (options.hasRuntimeActivity === true) {
    return true;
  }
  const targetSessionId = String(options.targetSessionId || '').trim();
  if (!targetSessionId) {
    return false;
  }
  const currentRuntimeSessionId = String(options.currentRuntimeSessionId || '').trim();
  const activeSessionId = String(options.activeSessionId || '').trim();
  const isCurrentRuntimeSession =
    currentRuntimeSessionId === targetSessionId || activeSessionId === targetSessionId;
  if (!isCurrentRuntimeSession) {
    return false;
  }
  return (
    isHotAgentRuntimeState(options.currentState) ||
    isHotAgentRuntimeState(options.overrideState) ||
    options.localStreaming === true ||
    options.localWaiting === true
  );
};

export const shouldSettleAgentSessionsFromRuntimeState = (options: {
  previousState?: AgentRuntimeState | null;
  nextState?: AgentRuntimeState | null;
}): boolean => {
  const previousState = options.previousState || 'idle';
  const nextState = options.nextState || 'idle';
  if (isTerminalAgentRuntimeState(nextState)) {
    return true;
  }
  return nextState === 'idle' &&
    (isHotAgentRuntimeState(previousState) || isTerminalAgentRuntimeState(previousState));
};

export const shouldNotifyAgentTaskCompletion = (options: {
  previousState?: AgentRuntimeState | null;
  nextState?: AgentRuntimeState | null;
}): boolean => {
  const previousState = options.previousState || 'idle';
  const nextState = options.nextState || 'idle';
  // The first runtime snapshot is intentionally ignored by the caller. A
  // notification is only valid after this page observed this task in progress.
  return isHotAgentRuntimeState(previousState) &&
    (nextState === 'done' || nextState === 'idle');
};

export const resolveAgentRuntimeStateFromSignals = (options: {
  pendingApproval?: boolean;
  pendingInquiry?: boolean;
  localWaiting?: boolean;
  localStreaming?: boolean;
  activeBlockingSwarm?: boolean;
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
  if (options.activeBlockingSwarm === true) {
    return 'running';
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
