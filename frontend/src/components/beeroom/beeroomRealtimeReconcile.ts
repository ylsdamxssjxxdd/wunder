const normalizeRealtimeEventType = (eventType: unknown): string =>
  String(eventType || '')
    .trim()
    .toLowerCase();

const workflowRefreshEventTypes = new Set([
  'team_task_dispatch',
  'team_task_update',
  'team_task_result',
  'team_finish',
  'team_error'
]);

export const shouldForceWorkflowRefresh = (eventType: unknown): boolean =>
  workflowRefreshEventTypes.has(normalizeRealtimeEventType(eventType));

type TeamRealtimeReconcileOptions = {
  eventType: unknown;
  accepted: boolean;
};

export const shouldForceImmediateTeamRealtimeReconcile = (
  options: TeamRealtimeReconcileOptions
): boolean => {
  const normalizedType = normalizeRealtimeEventType(options.eventType);
  return (
    !options.accepted ||
    normalizedType === 'team_task_update' ||
    normalizedType === 'team_finish' ||
    normalizedType === 'team_error'
  );
};
