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
  target_session_id?: string | null;
  targetSessionId?: string | null;
  spawned_session_id?: string | null;
  spawnedSessionId?: string | null;
  session_run_id?: string | null;
  sessionRunId?: string | null;
};

const normalizeProjectedSubagentFlag = (value: unknown): string =>
  String(value || '').trim().toLowerCase();

const normalizeProjectedIdentity = (value: unknown): string => String(value || '').trim();

export type BeeroomSwarmSubagentProjectionContext = {
  swarmTaskSessionIds: ReadonlySet<string>;
  swarmTaskRunIds: ReadonlySet<string>;
};

export type BeeroomSwarmSubagentProjectionDecision = {
  projectable: boolean;
  reason: string;
};

export const buildBeeroomSwarmSubagentProjectionContext = <Task extends BeeroomProjectedTaskLike>(
  tasks: Task[],
  overrides?: Partial<BeeroomSwarmSubagentProjectionContext>
): BeeroomSwarmSubagentProjectionContext => {
  const swarmTaskSessionIds = new Set<string>(overrides?.swarmTaskSessionIds || []);
  const swarmTaskRunIds = new Set<string>(overrides?.swarmTaskRunIds || []);

  tasks.forEach((task) => {
    [
      task?.spawned_session_id,
      task?.spawnedSessionId,
      task?.target_session_id,
      task?.targetSessionId
    ].forEach((value) => {
      const normalized = normalizeProjectedIdentity(value);
      if (normalized) {
        swarmTaskSessionIds.add(normalized);
      }
    });
    [task?.session_run_id, task?.sessionRunId].forEach((value) => {
      const normalized = normalizeProjectedIdentity(value);
      if (normalized) {
        swarmTaskRunIds.add(normalized);
      }
    });
  });

  return {
    swarmTaskSessionIds,
    swarmTaskRunIds
  };
};

export const resolveBeeroomSwarmSubagentProjectionDecision = <T extends BeeroomProjectedSubagentLike>(
  item: T,
  context?: Partial<BeeroomSwarmSubagentProjectionContext>
): BeeroomSwarmSubagentProjectionDecision => {
  const sessionId = normalizeProjectedIdentity(item.sessionId);
  if (sessionId && context?.swarmTaskSessionIds?.has(sessionId)) {
    return {
      projectable: false,
      reason: 'filtered:task_session_shadow'
    };
  }
  const runId = normalizeProjectedIdentity(item.runId);
  if (runId && context?.swarmTaskRunIds?.has(runId)) {
    return {
      projectable: false,
      reason: 'filtered:task_run_shadow'
    };
  }
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

export const shouldProjectBeeroomSwarmSubagent = <T extends BeeroomProjectedSubagentLike>(
  item: T,
  context?: Partial<BeeroomSwarmSubagentProjectionContext>
) => {
  return resolveBeeroomSwarmSubagentProjectionDecision(item, context).projectable;
};

export const mergeBeeroomProjectedSubagents = <T extends BeeroomProjectedSubagentLike>(
  taskSubagents: T[],
  runtimeSubagents: T[],
  context?: Partial<BeeroomSwarmSubagentProjectionContext>
) => {
  const merged = new Map<string, T>();
  const order: string[] = [];

  const append = (item: T) => {
    if (!shouldProjectBeeroomSwarmSubagent(item, context)) {
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
  swarmTaskProjectionContext?: Partial<BeeroomSwarmSubagentProjectionContext>;
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
  return mergeBeeroomProjectedSubagents(
    taskSubagents,
    runtimeScopedSubagents,
    buildBeeroomSwarmSubagentProjectionContext(options.tasks, options.swarmTaskProjectionContext)
  );
};
