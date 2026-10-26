export interface ProcessRefreshGateState {
  pending: boolean;
}

export function requestProcessRefresh(
  state: ProcessRefreshGateState,
  canStart: boolean,
): boolean {
  state.pending = true;
  if (!canStart) return false;
  state.pending = false;
  return true;
}

export function completeProcessRefresh(
  state: ProcessRefreshGateState,
  canStart: boolean,
): boolean {
  if (!state.pending || !canStart) return false;
  state.pending = false;
  return true;
}
