export type BeeroomProjectedSubagentLike = {
  key?: string;
  sessionId?: string;
  runId?: string;
  runKind?: string;
  requestedBy?: string;
  workflowItems?: unknown[];
};

export type BeeroomProjectedTaskLike = {
  task_id?: string | null;
};

const normalizeProjectedSubagentFlag = (value: unknown): string =>
  String(value || '').trim().toLowerCase();

export type BeeroomSwarmSubagentProjectionDecision = {
  projectable: boolean;
  reason: string;
};

export const resolveBeeroomSwarmSubagentProjectionDecision = <T extends BeeroomProjectedSubagentLike>(
  item: T
): BeeroomSwarmSubagentProjectionDecision => {
  const runKind = normalizeProjectedSubagentFlag(item.runKind);
  const requestedBy = normalizeProjectedSubagentFlag(item.requestedBy);
  if (runKind === 'swarm' || requestedBy === 'agent_swarm') {
    return {
      projectable: false,
      reason: runKind === 'swarm' ? 'filtered:run_kind_swarm' : 'filtered:requested_by_agent_swarm'
    };
  }
  return {
    projectable: true,
    reason: 'projectable'
  };
};

export const shouldProjectBeeroomSwarmSubagent = <T extends BeeroomProjectedSubagentLike>(item: T) => {
  return resolveBeeroomSwarmSubagentProjectionDecision(item).projectable;
};

export const mergeBeeroomProjectedSubagents = <T extends BeeroomProjectedSubagentLike>(
  taskSubagents: T[],
  runtimeSubagents: T[]
) => {
  const merged = new Map<string, T>();
  const order: string[] = [];

  const append = (item: T) => {
    if (!shouldProjectBeeroomSwarmSubagent(item)) {
      return;
    }
    const identity = String(item.runId || item.sessionId || item.key).trim();
    if (!identity) return;
    const existing = merged.get(identity);
    if (!existing) {
      merged.set(identity, item);
      order.push(identity);
      return;
    }
    merged.set(identity, {
      ...existing,
      ...item,
      workflowItems:
        Array.isArray(item.workflowItems) && item.workflowItems.length > 0
          ? item.workflowItems
          : existing.workflowItems
    });
  };

  taskSubagents.forEach(append);
  runtimeSubagents.forEach(append);

  return order
    .map((identity) => merged.get(identity))
    .filter((item): item is T => Boolean(item));
};

export const resolveProjectedWorkerSubagents = <
  T extends BeeroomProjectedSubagentLike,
  Task extends BeeroomProjectedTaskLike
>(options: {
  workerRole: string;
  workerNodeId: string;
  runtimeTargetNodeId: string;
  runtimeSubagents: T[];
  tasks: Task[];
  subagentsByTask: Record<string, T[]>;
}) => {
  const runtimeScopedSubagents =
    options.runtimeTargetNodeId === options.workerNodeId ? options.runtimeSubagents : [];
  const taskSubagents =
    options.workerRole !== 'mother'
      ? options.tasks.flatMap((task) => {
          const taskId = String(task?.task_id || '').trim();
          return taskId && Array.isArray(options.subagentsByTask[taskId]) ? options.subagentsByTask[taskId] : [];
        })
      : [];
  return mergeBeeroomProjectedSubagents(taskSubagents, runtimeScopedSubagents);
};
