export const readQueueSchedulingState = (detail: unknown): string => {
  try {
    const value = typeof detail === 'string' ? JSON.parse(detail) : detail;
    if (!value || typeof value !== 'object') return '';
    const source = value.data && typeof value.data === 'object' ? value.data : value;
    if (source.reason === 'admin_preempted') return String(source.queue_state || 'suspended');
    return Number(source.queue_priority) > 0 ? 'priority' : '';
  } catch { return ''; }
};
