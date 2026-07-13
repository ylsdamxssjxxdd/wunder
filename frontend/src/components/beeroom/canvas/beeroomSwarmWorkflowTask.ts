import type {
  BeeroomWorkflowItem
} from '@/components/beeroom/beeroomTaskWorkflow';
import { compareBeeroomMissionTasksByDisplayPriority } from '@/components/beeroom/beeroomTaskWorkflow';
import type { BeeroomMissionTask } from '@/stores/beeroom';

export const resolveBeeroomWorkflowTask = (options: {
  tasks: BeeroomMissionTask[];
  itemsByTask: Record<string, BeeroomWorkflowItem[]>;
  fallbackTask: BeeroomMissionTask | null;
}): BeeroomMissionTask | null =>
  [...options.tasks]
    .filter((task) => {
      const taskId = String(task.task_id || '').trim();
      return Boolean(
        taskId &&
          options.itemsByTask[taskId]?.some((item) => {
            const eventType = String(item.eventType || '').trim().toLowerCase();
            return item.isTool === true || eventType === 'tool_call' || eventType === 'tool_result';
          })
      );
    })
    .sort(compareBeeroomMissionTasksByDisplayPriority)[0] || options.fallbackTask;
