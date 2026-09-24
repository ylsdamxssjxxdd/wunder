import { computed, ref, type Ref } from 'vue';
import type { AgentRuntimeState } from './model';

export function useTaskListActivity<T>(
  items: Readonly<Ref<T[]>>,
  resolveState: (item: T) => AgentRuntimeState
) {
  const showActiveOnly = ref(false);
  // Read status clocks only, never message content. Preserve catalog order and
  // item identity so activity updates do not resort or clone the thread list.
  const activity = computed(() => {
    let running = false;
    const activeItems = items.value.filter((item) => {
      const state = resolveState(item);
      if (state === 'running') running = true;
      return state === 'running' || state === 'pending';
    });
    return { items: activeItems, state: running ? 'running' as const : 'pending' as const };
  });
  const activeCount = computed(() => activity.value.items.length);
  const activityState = computed(() => activity.value.state);
  const displayItems = computed(() => showActiveOnly.value ? activity.value.items : items.value);
  return { showActiveOnly, activeCount, activityState, displayItems };
}
