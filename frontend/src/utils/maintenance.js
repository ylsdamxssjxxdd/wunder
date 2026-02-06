const MAINTENANCE_STATUS_CODES = new Set([502, 503, 504]);

const state = {
  active: false,
  status: null,
  reason: '',
  updatedAt: 0
};

const listeners = new Set();

const emit = () => {
  const snapshot = { ...state };
  listeners.forEach((listener) => {
    try {
      listener(snapshot);
    } catch (error) {
      // ignore listener failures
    }
  });
};

export const isMaintenanceStatus = (status) => {
  const code = Number(status);
  return Number.isFinite(code) && MAINTENANCE_STATUS_CODES.has(code);
};

export const getMaintenanceState = () => ({ ...state });

export const subscribeMaintenance = (listener) => {
  if (typeof listener !== 'function') {
    return () => {};
  }
  listeners.add(listener);
  listener({ ...state });
  return () => {
    listeners.delete(listener);
  };
};

export const markMaintenance = (payload = {}) => {
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

export const clearMaintenance = () => {
  if (!state.active) {
    return;
  }
  state.active = false;
  state.status = null;
  state.reason = '';
  state.updatedAt = Date.now();
  emit();
};
