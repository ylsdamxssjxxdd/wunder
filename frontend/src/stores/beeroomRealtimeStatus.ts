const normalizeStatusText = (value: unknown): string =>
  String(value || '')
    .trim()
    .toLowerCase();

const toFiniteNumber = (value: unknown): number => {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : 0;
};

export const isStaleRealtimeUpdate = (
  currentUpdatedTime: unknown,
  incomingUpdatedTime: unknown
): boolean => {
  const current = toFiniteNumber(currentUpdatedTime);
  const incoming = toFiniteNumber(incomingUpdatedTime);
  return current > 0 && incoming > 0 && incoming < current;
};

type StatusTransitionOptions = {
  currentStatus: unknown;
  currentUpdatedTime: unknown;
  incomingStatus: unknown;
  incomingUpdatedTime: unknown;
  isTerminalStatus: (status: string) => boolean;
};

// Guard against out-of-order realtime events reverting terminal state to running/pending.
export const shouldApplyRealtimeStatusTransition = (
  options: StatusTransitionOptions
): boolean => {
  const nextStatus = normalizeStatusText(options.incomingStatus);
  if (!nextStatus) return false;
  if (isStaleRealtimeUpdate(options.currentUpdatedTime, options.incomingUpdatedTime)) {
    return false;
  }
  const currentStatus = normalizeStatusText(options.currentStatus);
  const currentTerminal = options.isTerminalStatus(currentStatus);
  if (!currentTerminal) {
    return true;
  }
  return options.isTerminalStatus(nextStatus);
};

