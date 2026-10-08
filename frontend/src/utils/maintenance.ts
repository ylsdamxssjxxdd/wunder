const MAINTENANCE_STATUS_CODES = new Set([502, 503, 504]);

type MaintenanceState = {
  active: boolean;
  status: number | null;
  reason: string;
  updatedAt: number;
};

type MaintenancePayload = {
  status?: number | string | null;
  reason?: string;
};

type MaintenanceListener = (state: MaintenanceState) => void;

const state: MaintenanceState = {
  active: false,
  status: null,
  reason: '',
  updatedAt: 0
};

const listeners = new Set<MaintenanceListener>();

const emit = (): void => {
  const snapshot = { ...state };
  listeners.forEach((listener) => {
    try {
      listener(snapshot);
    } catch {
      // ignore listener failures
    }
  });
};

export const isMaintenanceStatus = (status: unknown): boolean => {
  const code = Number(status);
  return Number.isFinite(code) && MAINTENANCE_STATUS_CODES.has(code);
};

export const getMaintenanceState = (): MaintenanceState => ({ ...state });

export const subscribeMaintenance = (listener: unknown): (() => void) => {
  if (typeof listener !== 'function') {
    return () => {};
  }
  const handler = listener as MaintenanceListener;
  listeners.add(handler);
  handler({ ...state });
  return () => {
    listeners.delete(handler);
  };
};

export const markMaintenance = (payload: MaintenancePayload = {}): void => {
  const nextStatus = isMaintenanceStatus(payload.status) ? Number(payload.status) : null;
  const nextReason = payload.reason ? String(payload.reason) : '';
  if (state.active && state.status === nextStatus && state.reason === nextReason) {
    return;
  }
  state.active = true;
  state.status = nextStatus;
  state.reason = nextReason;
  state.updatedAt = Date.now();
  emit();
};

export const clearMaintenance = (): void => {
  if (!state.active) {
    return;
  }
  state.active = false;
  state.status = null;
  state.reason = '';
  state.updatedAt = Date.now();
  emit();
};
