export type BeeroomSwarmNodeStatusTaskLike = {
  status?: string | null | undefined;
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
  if (statuses.some((status) => status === 'failed' || status === 'error' || status === 'timeout')) return 'failed';
  if (statuses.some((status) => status === 'cancelled')) return 'cancelled';
  if (statuses.every((status) => status === 'success' || status === 'completed')) return 'completed';
  return missionStatus || 'idle';
};
