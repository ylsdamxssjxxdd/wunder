export type BeeroomProjectedSubagentLike = {
  key?: string;
  sessionId?: string;
  runId?: string;
  workflowItems?: unknown[];
};

export type BeeroomProjectedTaskLike = {
  task_id?: string | null;
};

export const mergeBeeroomProjectedSubagents = <T extends BeeroomProjectedSubagentLike>(
  taskSubagents: T[],
  runtimeSubagents: T[]
) => {
  const merged = new Map<string, T>();
  const order: string[] = [];

  const append = (item: T) => {
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
