export type BeeroomSwarmNodeStatusTaskLike = {
  status?: string | null | undefined;
  updated_time?: number | null | undefined;
  finished_time?: number | null | undefined;
  started_time?: number | null | undefined;
};

export type BeeroomSwarmNodeStatusMemberLike = {
  idle?: boolean | null | undefined;
  active_session_total?: number | null | undefined;
};

export const resolveBeeroomSwarmNodeStatus = (options: {
  tasks: BeeroomSwarmNodeStatusTaskLike[];
  member: BeeroomSwarmNodeStatusMemberLike | undefined;
  missionStatus: string;
  workflowTailTone?: string | null | undefined;
}) => {
  const tasks = Array.isArray(options.tasks) ? options.tasks : [];
  const member = options.member;
  const missionStatus = options.missionStatus;
  const normalizedMissionStatus = String(missionStatus || '').trim().toLowerCase() || 'idle';
  const normalizedWorkflowTailTone = String(options.workflowTailTone || '').trim().toLowerCase();
  const workflowStillActive =
    normalizedWorkflowTailTone === 'loading' || normalizedWorkflowTailTone === 'pending';
  const hasActiveMemberSession =
    member?.idle === false || Math.max(0, Number(member?.active_session_total || 0)) > 0;
  if (!tasks.length) {
    if (hasActiveMemberSession) {
      if (workflowStillActive) return 'running';
      return normalizedMissionStatus === 'awaiting_idle' ? 'awaiting_idle' : 'running';
    }
    return missionStatus || 'idle';
  }
  const statuses = tasks.map((task) => String(task.status || '').trim().toLowerCase());
  if (statuses.some((status) => status === 'running' || status === 'queued' || status === 'pending')) return 'running';
  if (statuses.some((status) => status === 'awaiting_idle')) {
    return workflowStillActive ? 'running' : 'awaiting_idle';
  }
  if (hasActiveMemberSession) {
    if (workflowStillActive) return 'running';
    if (
      normalizedMissionStatus === 'awaiting_idle' ||
      statuses.every((status) => ['success', 'completed', 'failed', 'error', 'timeout', 'cancelled'].includes(status))
    ) {
      return 'awaiting_idle';
    }
    return 'running';
  }

  // A worker can retry several model actions for one swarm invocation. Once
  // no action remains active, its card reflects the newest action rather than
  // an earlier recoverable error from the same worker session.
  const resolveTaskMoment = (task: BeeroomSwarmNodeStatusTaskLike): number =>
    Math.max(0, Number(task.updated_time || task.finished_time || task.started_time || 0));
  const latestTask = tasks.reduce<BeeroomSwarmNodeStatusTaskLike | null>((latest, task) => {
    if (!latest || resolveTaskMoment(task) >= resolveTaskMoment(latest)) return task;
    return latest;
  }, null);
  const latestStatus = String(latestTask?.status || '').trim().toLowerCase();
  if (resolveTaskMoment(latestTask || {}) > 0) {
    if (latestStatus === 'failed' || latestStatus === 'error' || latestStatus === 'timeout') return 'failed';
    if (latestStatus === 'cancelled') return 'cancelled';
    if (latestStatus === 'success' || latestStatus === 'completed') return 'completed';
  }
  if (statuses.some((status) => status === 'failed' || status === 'error' || status === 'timeout')) return 'failed';
  if (statuses.some((status) => status === 'cancelled')) return 'cancelled';
  if (statuses.every((status) => status === 'success' || status === 'completed')) return 'completed';
  return missionStatus || 'idle';
};
